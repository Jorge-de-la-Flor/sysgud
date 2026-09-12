use crate::models::{DecisionRequest, EventRequest, IncidentQuery};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use sysgud_core::{Decision, Incident};
use sysgud_runtime::{AppState, ServiceError};
use uuid::Uuid;

pub struct ApiError(pub StatusCode, pub &'static str);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error":self.1}))).into_response()
    }
}
impl From<ServiceError> for ApiError {
    fn from(error: ServiceError) -> Self {
        match error {
            ServiceError::Invalid => Self(StatusCode::BAD_REQUEST, "entrada inválida"),
            ServiceError::Forbidden => Self(StatusCode::FORBIDDEN, "actor no permitido"),
            ServiceError::NotFound => Self(StatusCode::NOT_FOUND, "incidente inexistente"),
            ServiceError::Conflict => {
                Self(StatusCode::CONFLICT, "otra decisión o actor ya registrado")
            }
            ServiceError::Policy => Self(
                StatusCode::UNPROCESSABLE_ENTITY,
                "acción bloqueada por política o propuesta caducada",
            ),
            ServiceError::Busy => Self(StatusCode::TOO_MANY_REQUESTS, "capacidad temporal agotada"),
            ServiceError::Storage => Self(
                StatusCode::SERVICE_UNAVAILABLE,
                "almacenamiento no disponible",
            ),
        }
    }
}
pub async fn health(State(state): State<AppState>) -> (StatusCode, Json<Value>) {
    if state.healthy() {
        (StatusCode::OK, Json(json!({"status":"ok"})))
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"status":"unavailable"})),
        )
    }
}
pub async fn event(
    State(state): State<AppState>,
    Json(event): Json<EventRequest>,
) -> Result<Response, ApiError> {
    Ok(
        match state.ingest(event.source, event.message, None).await? {
            Some(incident) => (StatusCode::CREATED, Json(incident)).into_response(),
            None => (StatusCode::ACCEPTED, Json(json!({"status":"buffered"}))).into_response(),
        },
    )
}
pub async fn list(
    State(state): State<AppState>,
    Query(query): Query<IncidentQuery>,
) -> Result<Json<Vec<Incident>>, ApiError> {
    let limit = query.limit.unwrap_or(50);
    let offset = query.offset.unwrap_or(0);
    if !(1..=sysgud_core::MAX_PAGE_SIZE).contains(&limit) || offset > 10000 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "paginación inválida"));
    }
    Ok(Json(state.list(query.status, limit, offset).await))
}
pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Incident>, ApiError> {
    Ok(Json(state.get(id).await?))
}
pub async fn approve(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<DecisionRequest>,
) -> Result<Response, ApiError> {
    decide(state, id, body.actor_id, Decision::Approve).await
}
pub async fn reject(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<DecisionRequest>,
) -> Result<Response, ApiError> {
    decide(state, id, body.actor_id, Decision::Reject).await
}
async fn decide(
    state: AppState,
    id: Uuid,
    actor: i64,
    decision: Decision,
) -> Result<Response, ApiError> {
    let (started, incident) = state.decide(id, actor, decision).await?;
    Ok((
        if started {
            StatusCode::ACCEPTED
        } else {
            StatusCode::OK
        },
        Json(incident),
    )
        .into_response())
}
