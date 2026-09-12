use crate::{
    agent::AgentClient,
    monitor::{is_trigger, RingBuffer},
    security::Redactor,
    storage::{Record, Storage},
    CommandSpec,
};
use chrono::Utc;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use sysgud_core::{
    ActionType, AgentAction, AgentRequest, Decision, Incident, IncidentStatus, Severity,
    MAX_LINE_BYTES, MAX_SOURCES,
};
use tokio::{
    process::Child,
    sync::{Mutex, Semaphore},
};
use uuid::Uuid;

pub type Target = Arc<Mutex<Child>>;
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("entrada inválida")]
    Invalid,
    #[error("actor no permitido")]
    Forbidden,
    #[error("incidente inexistente")]
    NotFound,
    #[error("el incidente ya tiene otra decisión o actor")]
    Conflict,
    #[error("acción bloqueada por política o propuesta caducada")]
    Policy,
    #[error("capacidad temporal agotada; reintente después")]
    Busy,
    #[error("almacenamiento no disponible; ejecución bloqueada")]
    Storage,
}

#[derive(Clone)]
pub struct ServiceOptions {
    pub database: Option<PathBuf>,
    pub max_incidents: usize,
    pub approval_ttl: Duration,
    pub commands: BTreeMap<String, CommandSpec>,
}
impl Default for ServiceOptions {
    fn default() -> Self {
        Self {
            database: None,
            max_incidents: 1000,
            approval_ttl: Duration::from_secs(900),
            commands: BTreeMap::new(),
        }
    }
}
struct Stored {
    record: Record,
    target: Option<Target>,
}
type Buffers = HashMap<(bool, String), RingBuffer>;
struct Inner {
    incidents: Mutex<BTreeMap<Uuid, Stored>>,
    buffers: Mutex<Buffers>,
    agent: AgentClient,
    redactor: Redactor,
    token: String,
    users: HashSet<i64>,
    context_lines: usize,
    options: ServiceOptions,
    storage: Storage,
    healthy: AtomicBool,
    stopping: AtomicBool,
    analyses: Arc<Semaphore>,
    executions: Arc<Semaphore>,
    event_budget: Mutex<(Instant, u32)>,
    analysis_budget: Mutex<(Instant, u32)>,
    events: tokio::sync::broadcast::Sender<Incident>,
}
#[derive(Clone)]
pub struct AppState(Arc<Inner>);

impl AppState {
    pub fn new(
        agent: AgentClient,
        token: String,
        users: &str,
        context_lines: usize,
    ) -> anyhow::Result<Self> {
        Self::with_options(
            agent,
            token,
            users,
            context_lines,
            ServiceOptions::default(),
        )
    }
    pub fn with_options(
        agent: AgentClient,
        token: String,
        users: &str,
        context_lines: usize,
        options: ServiceOptions,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            (32..=256).contains(&token.len())
                && token.is_ascii()
                && !token.chars().any(char::is_whitespace),
            "SYSGUD_BOT_API_TOKEN requiere 32–256 caracteres ASCII sin espacios"
        );
        anyhow::ensure!(
            (1..=100).contains(&context_lines),
            "SYSGUD_CONTEXT_LINES debe estar entre 1 y 100"
        );
        anyhow::ensure!(
            (1..=10000).contains(&options.max_incidents),
            "SYSGUD_MAX_INCIDENTS debe estar entre 1 y 10000"
        );
        let mut allowed = HashSet::new();
        if !users.trim().is_empty() {
            for user in users.split(',') {
                let id: i64 = user.trim().parse()?;
                anyhow::ensure!(id > 0, "ID de actor debe ser positivo");
                allowed.insert(id);
            }
        }
        for (id, spec) in &options.commands {
            anyhow::ensure!(
                !id.is_empty()
                    && id.len() <= 64
                    && id
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'),
                "ID de comando inválido"
            );
            anyhow::ensure!(
                spec.program.is_absolute() && spec.program.is_file(),
                "cada comando requiere un ejecutable absoluto existente"
            );
            anyhow::ensure!(
                spec.args.len() <= 32
                    && spec
                        .args
                        .iter()
                        .all(|v| v.len() <= 4096 && !v.contains('\0')),
                "argumentos de comando inválidos"
            );
        }
        let (storage, records) = Storage::open(options.database.as_deref(), options.max_incidents)?;
        let incidents = records
            .into_iter()
            .map(|mut record| {
                // Never replay an OS effect after an ambiguous shutdown.
                if record.incident.status == IncidentStatus::Executing {
                    record.incident.status = IncidentStatus::Failed;
                    record.incident.error = Some(
                        "Ejecución interrumpida; resultado desconocido. No se reintentará.".into(),
                    );
                }
                record.incident.target_pid = None;
                (
                    record.incident.id,
                    Stored {
                        record,
                        target: None,
                    },
                )
            })
            .collect();
        Ok(Self(Arc::new(Inner {
            incidents: Mutex::new(incidents),
            buffers: Mutex::new(HashMap::new()),
            agent,
            redactor: Redactor::from_env(),
            token,
            users: allowed,
            context_lines,
            options,
            storage,
            healthy: AtomicBool::new(true),
            stopping: AtomicBool::new(false),
            analyses: Arc::new(Semaphore::new(4)),
            executions: Arc::new(Semaphore::new(4)),
            event_budget: Mutex::new((Instant::now(), 0)),
            analysis_budget: Mutex::new((Instant::now(), 0)),
            events: tokio::sync::broadcast::channel(64).0,
        })))
    }
    pub fn authorized(&self, token: &str) -> bool {
        self.0.token.as_bytes().ct_eq(token.as_bytes()).into()
    }
    pub fn actor_allowed(&self, actor: i64) -> bool {
        self.0.users.contains(&actor)
    }
    pub fn healthy(&self) -> bool {
        self.0.healthy.load(Ordering::Acquire) && !self.0.stopping.load(Ordering::Acquire)
    }
    pub fn redact(&self, value: &str) -> String {
        self.0.redactor.redact(value)
    }
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<Incident> {
        self.0.events.subscribe()
    }
    async fn save(&self, record: Record, evict: Option<Uuid>) -> Result<(), ServiceError> {
        if self.0.storage.save(record, evict).await.is_err() {
            self.0.healthy.store(false, Ordering::Release);
            return Err(ServiceError::Storage);
        }
        Ok(())
    }
    pub async fn ingest(
        &self,
        source: String,
        message: String,
        target: Option<Target>,
    ) -> Result<Option<Incident>, ServiceError> {
        if !self.healthy() {
            return Err(ServiceError::Storage);
        }
        if source.trim().is_empty()
            || source.len() > 256
            || source.chars().any(char::is_control)
            || message.trim().is_empty()
            || message.len() > MAX_LINE_BYTES
        {
            return Err(ServiceError::Invalid);
        }
        let mut budget = self.0.event_budget.lock().await;
        if budget.0.elapsed() >= Duration::from_secs(1) {
            *budget = (Instant::now(), 0);
        }
        if budget.1 >= 60 {
            return Err(ServiceError::Busy);
        }
        budget.1 += 1;
        drop(budget);
        let source = self.redact(&source);
        let message = self.redact(&message);
        let logs = {
            let mut buffers = self.0.buffers.lock().await;
            let key = (target.is_some(), source.clone());
            if !buffers.contains_key(&key) && buffers.len() >= MAX_SOURCES {
                return Err(ServiceError::Busy);
            }
            let buffer = buffers
                .entry(key)
                .or_insert_with(|| RingBuffer::new(self.0.context_lines));
            buffer.push(message.clone());
            if !is_trigger(&message) {
                return Ok(None);
            }
            buffer.snapshot()
        };
        let mut budget = self.0.analysis_budget.lock().await;
        if budget.0.elapsed() >= Duration::from_secs(60) {
            *budget = (Instant::now(), 0);
        }
        if budget.1 >= 30 {
            return Err(ServiceError::Busy);
        }
        budget.1 += 1;
        drop(budget);
        let _permit = self
            .0
            .analyses
            .clone()
            .try_acquire_owned()
            .map_err(|_| ServiceError::Busy)?;
        let severity = if message.contains("CRITICAL") || message.contains("PANIC") {
            Severity::Critical
        } else {
            Severity::Error
        };
        let action = self
            .0
            .agent
            .analyze(AgentRequest {
                system_context: format!(
                    "OS: {}; source: {}; allowed command IDs: {:?}",
                    std::env::consts::OS,
                    source,
                    self.0.options.commands.keys().collect::<Vec<_>>()
                ),
                log_extract: logs.clone(),
            })
            .await
            .map_err(|_| ServiceError::Busy)?;
        self.insert(source, logs, severity, action, target)
            .await
            .map(Some)
    }
    /// Trusted producer entry point; transport payloads must use ingest instead.
    pub async fn insert(
        &self,
        source: String,
        logs: Vec<String>,
        severity: Severity,
        mut action: AgentAction,
        target: Option<Target>,
    ) -> Result<Incident, ServiceError> {
        if !self.healthy() {
            return Err(ServiceError::Storage);
        }
        action.diagnosis = self.redact(&action.diagnosis);
        if action.diagnosis.len() > 4096 || action.command.as_ref().is_some_and(|c| c.len() > 4096)
        {
            return Err(ServiceError::Invalid);
        }
        let target_pid = if let Some(child) = &target {
            child.lock().await.id()
        } else {
            None
        };
        let execution = if action.action_type == ActionType::Execute {
            action
                .command
                .as_ref()
                .and_then(|id| self.0.options.commands.get(id))
                .cloned()
        } else {
            None
        };
        let incident = Incident {
            id: Uuid::new_v4(),
            source,
            logs,
            severity,
            execution,
            status: IncidentStatus::PendingApproval,
            diagnosis: action.diagnosis.clone(),
            proposed_action: action,
            created_at: Utc::now(),
            decided_at: None,
            decided_by: None,
            target_pid,
            error: None,
        };
        let mut incidents = self.0.incidents.lock().await;
        let evict = if incidents.len() >= self.0.options.max_incidents {
            Some(
                incidents
                    .values()
                    .filter(|s| {
                        matches!(
                            s.record.incident.status,
                            IncidentStatus::Executed
                                | IncidentStatus::Failed
                                | IncidentStatus::Rejected
                        )
                    })
                    .min_by_key(|s| s.record.incident.created_at)
                    .ok_or(ServiceError::Busy)?
                    .record
                    .incident
                    .id,
            )
        } else {
            None
        };
        let record = Record {
            incident: incident.clone(),
            decision: None,
        };
        self.save(record.clone(), evict).await?;
        if let Some(id) = evict {
            incidents.remove(&id);
        }
        incidents.insert(incident.id, Stored { record, target });
        let _ = self.0.events.send(incident.clone());
        Ok(incident)
    }
    pub async fn list(
        &self,
        status: Option<IncidentStatus>,
        limit: usize,
        offset: usize,
    ) -> Vec<Incident> {
        self.0
            .incidents
            .lock()
            .await
            .values()
            .filter(|s| status.is_none_or(|v| s.record.incident.status == v))
            .skip(offset)
            .take(limit.min(sysgud_core::MAX_PAGE_SIZE))
            .map(|s| s.record.incident.clone())
            .collect()
    }
    pub async fn get(&self, id: Uuid) -> Result<Incident, ServiceError> {
        self.0
            .incidents
            .lock()
            .await
            .get(&id)
            .map(|s| s.record.incident.clone())
            .ok_or(ServiceError::NotFound)
    }
    pub async fn decide(
        &self,
        id: Uuid,
        actor: i64,
        decision: Decision,
    ) -> Result<(bool, Incident), ServiceError> {
        // Client disconnect must not cancel a durable reservation before its spawn.
        let state = self.clone();
        tokio::spawn(async move { state.decide_inner(id, actor, decision).await })
            .await
            .map_err(|_| ServiceError::Storage)?
    }
    async fn decide_inner(
        &self,
        id: Uuid,
        actor: i64,
        decision: Decision,
    ) -> Result<(bool, Incident), ServiceError> {
        if !self.actor_allowed(actor) {
            return Err(ServiceError::Forbidden);
        }
        if !self.healthy() {
            return Err(ServiceError::Storage);
        }
        let mut incidents = self.0.incidents.lock().await;
        let stored = incidents.get_mut(&id).ok_or(ServiceError::NotFound)?;
        if let Some(previous) = stored.record.decision {
            return if previous == decision && stored.record.incident.decided_by == Some(actor) {
                Ok((false, stored.record.incident.clone()))
            } else {
                Err(ServiceError::Conflict)
            };
        }
        let action = stored.record.incident.proposed_action.clone();
        let permit = if decision == Decision::Approve {
            if Utc::now()
                .signed_duration_since(stored.record.incident.created_at)
                .num_seconds()
                > self.0.options.approval_ttl.as_secs() as i64
            {
                return Err(ServiceError::Policy);
            }
            if (action.action_type == ActionType::Kill && stored.target.is_none())
                || (action.action_type == ActionType::Execute
                    && !action.command.as_ref().is_some_and(|id| {
                        self.0.options.commands.get(id).is_some_and(|spec| {
                            Some(spec) == stored.record.incident.execution.as_ref()
                        })
                    }))
            {
                return Err(ServiceError::Policy);
            }
            Some(
                self.0
                    .executions
                    .clone()
                    .try_acquire_owned()
                    .map_err(|_| ServiceError::Busy)?,
            )
        } else {
            None
        };
        let mut record = stored.record.clone();
        record.decision = Some(decision);
        record.incident.decided_at = Some(Utc::now());
        record.incident.decided_by = Some(actor);
        record.incident.status = if decision == Decision::Approve {
            IncidentStatus::Executing
        } else {
            IncidentStatus::Rejected
        };
        self.save(record.clone(), None).await?;
        stored.record = record.clone();
        let target = stored.target.take();
        if decision == Decision::Reject {
            return Ok((false, record.incident));
        }
        let state = self.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let result = crate::execution::execute(action, target, &state.0.options.commands).await;
            let mut incidents = state.0.incidents.lock().await;
            if let Some(stored) = incidents.get_mut(&id) {
                let mut completed = stored.record.clone();
                completed.incident.status = if result.is_ok() {
                    IncidentStatus::Executed
                } else {
                    IncidentStatus::Failed
                };
                completed.incident.error = result.err().map(|e| state.redact(&e.to_string()));
                if state.save(completed.clone(), None).await.is_ok() {
                    stored.record = completed;
                }
            }
        });
        Ok((true, record.incident))
    }
    pub async fn shutdown(&self) {
        self.0.stopping.store(true, Ordering::Release);
        let _ = tokio::time::timeout(
            Duration::from_secs(35),
            self.0.executions.clone().acquire_many_owned(4),
        )
        .await;
    }
}
