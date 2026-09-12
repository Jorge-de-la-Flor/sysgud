use crate::{agent::AgentClient, AppState, ServiceError, ServiceOptions};
use std::time::Duration;
use sysgud_core::{ActionType, AgentAction, Decision, IncidentStatus, Severity};

fn options(path: &std::path::Path) -> ServiceOptions {
    ServiceOptions {
        database: Some(path.join("state.sqlite")),
        ..Default::default()
    }
}
fn state(options: ServiceOptions) -> AppState {
    AppState::with_options(
        AgentClient::new(None, "test".into()),
        "t".repeat(32),
        "123,456",
        3,
        options,
    )
    .unwrap()
}
async fn incident(state: &AppState) -> sysgud_core::Incident {
    state
        .ingest("worker".into(), "ERROR: example".into(), None)
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn restart_preserves_decisions_and_never_reexecutes() {
    let dir = tempfile::tempdir().unwrap();
    let one = state(options(dir.path()));
    let item = incident(&one).await;
    one.decide(item.id, 123, Decision::Approve).await.unwrap();
    one.shutdown().await;
    assert_eq!(
        one.get(item.id).await.unwrap().status,
        IncidentStatus::Executed
    );
    drop(one);
    let two = state(options(dir.path()));
    let (started, persisted) = two.decide(item.id, 123, Decision::Approve).await.unwrap();
    assert!(!started);
    assert_eq!(persisted.status, IncidentStatus::Executed);
    assert!(matches!(
        two.decide(item.id, 456, Decision::Approve).await,
        Err(ServiceError::Conflict)
    ));
}

#[tokio::test]
async fn executing_record_after_crash_becomes_unknown_failure() {
    let dir = tempfile::tempdir().unwrap();
    let one = state(options(dir.path()));
    let item = incident(&one).await;
    drop(one);
    let (storage, mut records) =
        crate::storage::Storage::open(options(dir.path()).database.as_deref(), 1000).unwrap();
    let record = &mut records[0];
    record.decision = Some(Decision::Approve);
    record.incident.decided_by = Some(123);
    record.incident.status = IncidentStatus::Executing;
    storage.save(record.clone(), None).await.unwrap();
    drop(storage);
    let recovered = state(options(dir.path()));
    let (started, view) = recovered
        .decide(item.id, 123, Decision::Approve)
        .await
        .unwrap();
    assert!(!started);
    assert_eq!(view.status, IncidentStatus::Failed);
    assert!(view.error.unwrap().contains("desconocido"));
}

#[tokio::test]
async fn database_has_a_single_owner_and_rejects_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let one = state(options(dir.path()));
    assert!(AppState::with_options(
        AgentClient::new(None, "test".into()),
        "t".repeat(32),
        "123",
        3,
        options(dir.path())
    )
    .is_err());
    drop(one);
    let db = options(dir.path()).database.unwrap();
    let connection = rusqlite::Connection::open(db).unwrap();
    connection
        .execute(
            "INSERT INTO incidents(id,data) VALUES ('bad','not-json')",
            [],
        )
        .unwrap();
    drop(connection);
    assert!(AppState::with_options(
        AgentClient::new(None, "test".into()),
        "t".repeat(32),
        "123",
        3,
        options(dir.path())
    )
    .is_err());
}

#[tokio::test]
async fn bounded_retention_never_evicts_pending_actions() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = options(dir.path());
    config.max_incidents = 1;
    let state = state(config.clone());
    let first = incident(&state).await;
    assert!(matches!(
        state
            .ingest("worker".into(), "ERROR another".into(), None)
            .await,
        Err(ServiceError::Busy)
    ));
    state.decide(first.id, 123, Decision::Reject).await.unwrap();
    let second = incident(&state).await;
    assert!(matches!(
        state.get(first.id).await,
        Err(ServiceError::NotFound)
    ));
    drop(state);
    let recovered = super::tests::state(config);
    assert_eq!(recovered.list(None, 100, 0).await[0].id, second.id);
}

#[tokio::test]
async fn secrets_are_removed_before_storage_and_unknown_shell_is_blocked() {
    let state = state(ServiceOptions::default());
    let item = state
        .ingest(
            "worker".into(),
            "ERROR token=verysecretvalue Bearer hidden123".into(),
            None,
        )
        .await
        .unwrap()
        .unwrap();
    let text = serde_json::to_string(&item).unwrap();
    assert!(!text.contains("verysecretvalue"));
    assert!(!text.contains("hidden123"));
    let dangerous = state
        .insert(
            "worker".into(),
            vec![],
            Severity::Error,
            AgentAction {
                action_type: ActionType::Execute,
                command: Some("echo arbitrary | shell".into()),
                diagnosis: "test".into(),
            },
            None,
        )
        .await
        .unwrap();
    assert!(matches!(
        state.decide(dangerous.id, 123, Decision::Approve).await,
        Err(ServiceError::Policy)
    ));
    assert_eq!(
        state.get(dangerous.id).await.unwrap().status,
        IncidentStatus::PendingApproval
    );
}

#[tokio::test]
async fn expired_approval_can_still_be_rejected() {
    let state = state(ServiceOptions {
        approval_ttl: Duration::ZERO,
        ..Default::default()
    });
    let item = incident(&state).await;
    tokio::time::sleep(Duration::from_millis(1050)).await;
    assert!(matches!(
        state.decide(item.id, 123, Decision::Approve).await,
        Err(ServiceError::Policy)
    ));
    assert_eq!(
        state
            .decide(item.id, 123, Decision::Reject)
            .await
            .unwrap()
            .1
            .status,
        IncidentStatus::Rejected
    );
}
