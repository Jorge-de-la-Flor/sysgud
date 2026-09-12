use super::{router, AppState};
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use sysgud_core::Severity;
use sysgud_runtime::{
    agent::AgentClient,
    core::{ActionType, AgentAction},
};
use tower::ServiceExt;

const TOKEN: &str = "0123456789abcdef0123456789abcdef";
const ACTOR: i64 = 123456789;

fn setup() -> (Router, AppState) {
    let state = AppState::new(
        AgentClient::new(None, "unused".into()),
        TOKEN.into(),
        "123456789,987654321",
        3,
    )
    .unwrap();
    (router(state.clone()), state)
}

async fn call(app: Router, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("content-type", "application/json")
        .body(Body::from(
            body.map(|value| value.to_string()).unwrap_or_default(),
        ))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 128 * 1024).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

async fn event(app: Router, source: &str, message: &str) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        "/api/v1/events",
        Some(json!({"source": source, "message": message})),
    )
    .await
}

async fn create(app: Router) -> Value {
    let (status, incident) = event(app, "test", "ERROR: test").await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(incident["status"], "pending_approval");
    incident
}

async fn wait_status(app: Router, path: &str, expected: &str) -> Value {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let (_, incident) = call(app.clone(), "GET", path, None).await;
            if incident["status"] == expected {
                return incident;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn health_public_and_invalid_token_rejected() {
    let (app, _) = setup();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    for token in [None, Some("Bearer wrong")] {
        for path in [
            "/api/v1/incidents",
            "/api/v1/incidents/00000000-0000-0000-0000-000000000000/approve",
        ] {
            let mut request = Request::builder()
                .method(if path.ends_with("approve") {
                    "POST"
                } else {
                    "GET"
                })
                .uri(path);
            if let Some(token) = token {
                request = request.header("authorization", token);
            }
            let response = app
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
    }
}

#[tokio::test]
async fn creation_preserves_context_per_source_and_full_contract() {
    let (app, _) = setup();
    for message in ["old", "before", "ready"] {
        assert_eq!(
            event(app.clone(), "worker", message).await.0,
            StatusCode::ACCEPTED
        );
    }
    event(app.clone(), "other", "unrelated").await;
    let (status, incident) = call(
        app.clone(),
        "POST",
        "/api/v1/events",
        Some(json!({"source":"worker","log_line":"CRITICAL: fail"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        incident["logs"],
        json!(["before", "ready", "CRITICAL: fail"])
    );
    assert_eq!(incident["source"], "worker");
    assert_eq!(incident["severity"], "critical");
    assert_eq!(incident["status"], "pending_approval");
    assert_eq!(incident["proposed_action"]["action_type"], "NOTIFY");
    assert!(incident["diagnosis"].is_string());
    assert!(chrono::DateTime::parse_from_rfc3339(incident["created_at"].as_str().unwrap()).is_ok());
    assert!(incident["decided_at"].is_null());
    assert!(incident["decided_by"].is_null());
    let second = create(app.clone()).await;
    assert_ne!(incident["id"], second["id"]);
    assert_eq!(second["severity"], "error");
    let path = format!("/api/v1/incidents/{}", incident["id"].as_str().unwrap());
    assert_eq!(call(app, "GET", &path, None).await.1, incident);
}

#[tokio::test]
async fn pending_filter_excludes_decided_incidents() {
    let (app, _) = setup();
    let first = create(app.clone()).await;
    let second = create(app.clone()).await;
    let path = format!("/api/v1/incidents/{}/reject", first["id"].as_str().unwrap());
    assert_eq!(
        call(app.clone(), "POST", &path, Some(json!({"actor_id": ACTOR})))
            .await
            .0,
        StatusCode::OK
    );
    let (_, pending) = call(
        app.clone(),
        "GET",
        "/api/v1/incidents?status=pending_approval",
        None,
    )
    .await;
    assert_eq!(pending, json!([second]));
    assert_eq!(
        call(app.clone(), "GET", "/api/v1/incidents", None)
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        call(app, "GET", "/api/v1/incidents?status=invalid", None)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn client_cannot_supply_commands_actions_or_targets() {
    let (app, _) = setup();
    for field in [
        "command",
        "action",
        "action_type",
        "target_pid",
        "system_context",
        "log_extract",
    ] {
        let mut payload = json!({"source":"test", "message":"ERROR"});
        payload[field] = json!("untrusted");
        assert_eq!(
            call(app.clone(), "POST", "/api/v1/events", Some(payload))
                .await
                .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    let both = json!({"source":"test", "message":"ERROR", "log_line":"PANIC"});
    assert_eq!(
        call(app.clone(), "POST", "/api/v1/events", Some(both))
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        event(app.clone(), "", "ERROR").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        event(app.clone(), "test", " ").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        event(app, "test", &"x".repeat(70000)).await.0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
}

#[tokio::test]
async fn decisions_require_allowed_actor_and_strict_payload() {
    let (app, _) = setup();
    let incident = create(app.clone()).await;
    let path = format!("/api/v1/incidents/{}", incident["id"].as_str().unwrap());
    for suffix in ["approve", "reject"] {
        let route = format!("{path}/{suffix}");
        assert_eq!(
            call(app.clone(), "POST", &route, Some(json!({"actor_id": 999})))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            call(app.clone(), "POST", &route, Some(json!({}))).await.0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        for field in ["command", "action", "proposed_action"] {
            let mut body = json!({"actor_id": ACTOR});
            body[field] = json!("untrusted");
            assert_eq!(
                call(app.clone(), "POST", &route, Some(body)).await.0,
                StatusCode::UNPROCESSABLE_ENTITY
            );
        }
    }
    assert_eq!(
        call(app, "GET", &path, None).await.1["status"],
        "pending_approval"
    );
}

#[tokio::test]
async fn approval_is_idempotent_even_when_concurrent() {
    let (app, _) = setup();
    let incident = create(app.clone()).await;
    let path = format!("/api/v1/incidents/{}", incident["id"].as_str().unwrap());
    let route = format!("{path}/approve");
    let (a, b) = tokio::join!(
        call(
            app.clone(),
            "POST",
            &route,
            Some(json!({"actor_id": ACTOR}))
        ),
        call(
            app.clone(),
            "POST",
            &route,
            Some(json!({"actor_id": ACTOR}))
        )
    );
    assert!(
        (a.0 == StatusCode::ACCEPTED && b.0 == StatusCode::OK)
            || (b.0 == StatusCode::ACCEPTED && a.0 == StatusCode::OK)
    );
    let executed = wait_status(app.clone(), &path, "executed").await;
    assert_eq!(executed["decided_by"], ACTOR);
    assert!(executed["decided_at"].is_string());
    assert_eq!(
        call(
            app.clone(),
            "POST",
            &route,
            Some(json!({"actor_id": ACTOR}))
        )
        .await
        .1,
        executed
    );
    assert_eq!(
        call(
            app.clone(),
            "POST",
            &route,
            Some(json!({"actor_id": 987654321}))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            app,
            "POST",
            &format!("{path}/reject"),
            Some(json!({"actor_id": ACTOR}))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn rejection_is_idempotent_and_final() {
    let (app, _) = setup();
    let incident = create(app.clone()).await;
    let path = format!("/api/v1/incidents/{}", incident["id"].as_str().unwrap());
    let route = format!("{path}/reject");
    let first = call(
        app.clone(),
        "POST",
        &route,
        Some(json!({"actor_id": ACTOR})),
    )
    .await;
    assert_eq!(first.0, StatusCode::OK);
    assert_eq!(first.1["status"], "rejected");
    assert_eq!(first.1["decided_by"], ACTOR);
    assert_eq!(
        call(
            app.clone(),
            "POST",
            &route,
            Some(json!({"actor_id": ACTOR}))
        )
        .await
        .1,
        first.1
    );
    assert_eq!(
        call(
            app,
            "POST",
            &format!("{path}/approve"),
            Some(json!({"actor_id": ACTOR}))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn missing_and_invalid_ids_are_errors() {
    let (app, _) = setup();
    let path = format!("/api/v1/incidents/{}", uuid::Uuid::new_v4());
    assert_eq!(
        call(app.clone(), "GET", &path, None).await.0,
        StatusCode::NOT_FOUND
    );
    for suffix in ["approve", "reject"] {
        assert_eq!(
            call(
                app.clone(),
                "POST",
                &format!("{path}/{suffix}"),
                Some(json!({"actor_id": ACTOR}))
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        call(app, "GET", "/api/v1/incidents/invalid", None).await.0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn empty_allowlist_denies_decisions_and_bad_configuration_fails() {
    let state = AppState::new(
        AgentClient::new(None, "unused".into()),
        TOKEN.into(),
        "",
        12,
    )
    .unwrap();
    let app = router(state);
    let incident = create(app.clone()).await;
    let path = format!(
        "/api/v1/incidents/{}/approve",
        incident["id"].as_str().unwrap()
    );
    assert_eq!(
        call(app, "POST", &path, Some(json!({"actor_id": ACTOR})))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    for ids in ["abc", "-1", "0", "123,"] {
        assert!(AppState::new(
            AgentClient::new(None, "unused".into()),
            TOKEN.into(),
            ids,
            12
        )
        .is_err());
    }
}

#[test]
fn supervised_child_fixture() {
    if std::env::var_os("SYSGUD_TEST_CHILD").is_some() {
        std::thread::sleep(std::time::Duration::from_secs(30));
    }
}

#[tokio::test]
async fn kill_only_terminates_owned_child_after_approval() {
    let (app, state) = setup();
    let child = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "tests::supervised_child_fixture"])
        .env("SYSGUD_TEST_CHILD", "1")
        .stdout(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let target = std::sync::Arc::new(tokio::sync::Mutex::new(child));
    let incident = state
        .insert(
            "test".into(),
            vec!["PANIC".into()],
            Severity::Critical,
            AgentAction {
                action_type: ActionType::Kill,
                command: None,
                diagnosis: "test".into(),
            },
            Some(target.clone()),
        )
        .await
        .unwrap();
    assert!(target.lock().await.try_wait().unwrap().is_none());
    let path = format!("/api/v1/incidents/{}", incident.id);
    assert_eq!(
        call(app.clone(), "GET", &path, None).await.1["status"],
        "pending_approval"
    );
    assert_eq!(
        call(
            app.clone(),
            "POST",
            &format!("{path}/approve"),
            Some(json!({"actor_id": ACTOR}))
        )
        .await
        .0,
        StatusCode::ACCEPTED
    );
    wait_status(app, &path, "executed").await;
    assert!(target.lock().await.try_wait().unwrap().is_some());
}

#[tokio::test]
async fn kill_without_owned_target_cannot_be_approved() {
    let (app, state) = setup();
    let incident = state
        .insert(
            "http".into(),
            vec![],
            Severity::Critical,
            AgentAction {
                action_type: ActionType::Kill,
                command: None,
                diagnosis: "test".into(),
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        call(
            app,
            "POST",
            &format!("/api/v1/incidents/{}/approve", incident.id),
            Some(json!({"actor_id": ACTOR}))
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[tokio::test]
async fn execute_records_failure_and_duplicate_never_retries() {
    let mut options = sysgud_runtime::ServiceOptions::default();
    #[cfg(windows)]
    let spec = sysgud_runtime::CommandSpec {
        program: std::path::PathBuf::from(std::env::var("SystemRoot").unwrap())
            .join("System32/cmd.exe"),
        args: vec!["/D".into(), "/C".into(), "exit 7".into()],
    };
    #[cfg(not(windows))]
    let spec = sysgud_runtime::CommandSpec {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), "exit 7".into()],
    };
    options.commands.insert("test_exit".into(), spec);
    let state = AppState::with_options(
        AgentClient::new(None, "unused".into()),
        TOKEN.into(),
        &ACTOR.to_string(),
        3,
        options,
    )
    .unwrap();
    let app = router(state.clone());
    let incident = state
        .insert(
            "test".into(),
            vec![],
            Severity::Error,
            AgentAction {
                action_type: ActionType::Execute,
                command: Some("test_exit".into()),
                diagnosis: "test".into(),
            },
            None,
        )
        .await
        .unwrap();
    let path = format!("/api/v1/incidents/{}", incident.id);
    assert_eq!(
        call(app.clone(), "GET", &path, None).await.1["status"],
        "pending_approval"
    );
    assert_eq!(
        call(
            app.clone(),
            "POST",
            &format!("{path}/approve"),
            Some(json!({"actor_id": ACTOR}))
        )
        .await
        .0,
        StatusCode::ACCEPTED
    );
    let failed = wait_status(app.clone(), &path, "failed").await;
    assert!(failed["error"].is_string());
    let repeated = call(
        app,
        "POST",
        &format!("{path}/approve"),
        Some(json!({"actor_id": ACTOR})),
    )
    .await;
    assert_eq!(repeated.0, StatusCode::OK);
    assert_eq!(repeated.1, failed);
}
