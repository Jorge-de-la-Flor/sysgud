use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

/// Tipo de remediación que el agente puede decidir aplicar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ActionType {
    /// Termina el proceso objetivo (usa el PID capturado por el monitor).
    Kill,
    /// Ejecuta un comando de shell no destructivo para remediar el problema.
    Execute,
    /// No se toma ninguna acción sobre el proceso, solo se reporta el diagnóstico.
    Notify,
    /// Valor de reserva si el LLM devuelve algo inesperado.
    #[serde(other)]
    None,
}

/// Payload que se envía al motor de decisiones (LLM).
#[derive(Debug, Clone, Serialize)]
pub struct AgentRequest {
    /// Contexto libre sobre qué se está supervisando.
    pub system_context: String,
    /// Últimas N líneas capturadas por el buffer circular en el momento del fallo.
    pub log_extract: Vec<String>,
}

/// Respuesta estructurada y estrictamente tipada que produce el agente.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAction {
    pub action_type: ActionType,
    /// Comando a ejecutar. Solo tiene sentido cuando `action_type` es
    /// `Execute`; para `Kill` se usa el PID capturado por el monitor.
    pub command: Option<String>,
    /// Explicación técnica breve y legible por humanos.
    pub diagnosis: String,
}

/// Aprobación pendiente que espera confirmación desde Telegram.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingApproval {
    /// La acción a ejecutar una vez aprobada.
    pub action: AgentAction,
    /// El PID del proceso objetivo (solo relevante para `Kill`).
    pub pid: Option<u32>,
}

/// Almacén compartido de aprobaciones pendientes entre el loop de
/// monitor y el task de polling de Telegram.
#[derive(Debug, Default)]
pub struct PendingStore(Arc<Mutex<Option<PendingApproval>>>);

impl PendingStore {
    /// Crea un almacén vacío.
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(None)))
    }

    /// Guarda una aprobación pendiente.
    pub fn store(&self, approval: PendingApproval) {
        let mut guard = self.0.lock().unwrap();
        *guard = Some(approval);
    }

    /// Devuelve la aprobación pendiente y la limpia.
    pub fn take(&self) -> Option<PendingApproval> {
        let mut guard = self.0.lock().unwrap();
        guard.take()
    }

    /// Devuelve `true` si hay una aprobación sin confirmar.
    pub fn has_pending(&self) -> bool {
        let guard = self.0.lock().unwrap();
        guard.is_some()
    }
}
