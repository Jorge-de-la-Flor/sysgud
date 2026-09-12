use crate::handlers;
use axum::{
    extract::{DefaultBodyLimit, Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
    Router,
};
use sysgud_runtime::AppState;

pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/api/v1/events", post(handlers::event))
        .route(
            "/api/v1/openapi.json",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "application/json")],
                    include_str!("../openapi.json"),
                )
            }),
        )
        .route("/api/v1/incidents", get(handlers::list))
        .route("/api/v1/incidents/{id}", get(handlers::get))
        .route("/api/v1/incidents/{id}/approve", post(handlers::approve))
        .route("/api/v1/incidents/{id}/reject", post(handlers::reject))
        .route_layer(middleware::from_fn_with_state(state.clone(), authorize));
    Router::new()
        .route("/health", get(handlers::health))
        .merge(protected)
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(middleware::from_fn_with_state(
            std::sync::Arc::new(tokio::sync::Semaphore::new(32)),
            admission,
        ))
        .layer(middleware::from_fn(headers))
        .with_state(state)
}

async fn admission(
    State(slots): State<std::sync::Arc<tokio::sync::Semaphore>>,
    request: Request,
    next: Next,
) -> Result<Response, handlers::ApiError> {
    let _permit = slots.try_acquire_owned().map_err(|_| {
        handlers::ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "demasiadas solicitudes simultáneas",
        )
    })?;
    tokio::time::timeout(std::time::Duration::from_secs(40), next.run(request))
        .await
        .map_err(|_| handlers::ApiError(StatusCode::REQUEST_TIMEOUT, "solicitud agotó su tiempo"))
}
async fn authorize(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, handlers::ApiError> {
    if request
        .headers()
        .get_all(header::AUTHORIZATION)
        .iter()
        .count()
        != 1
    {
        return Err(handlers::ApiError(
            StatusCode::UNAUTHORIZED,
            "autenticación requerida",
        ));
    }
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    if !token.is_some_and(|v| state.authorized(v)) {
        return Err(handlers::ApiError(
            StatusCode::UNAUTHORIZED,
            "autenticación requerida",
        ));
    }
    Ok(next.run(request).await)
}
async fn headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    if (response.status().is_client_error() || response.status().is_server_error())
        && !response
            .headers()
            .get(header::CONTENT_TYPE)
            .is_some_and(|v| v.as_bytes().starts_with(b"application/json"))
    {
        use axum::response::IntoResponse;
        response = handlers::ApiError(
            response.status(),
            response.status().canonical_reason().unwrap_or("error"),
        )
        .into_response();
    }
    if response.status() == StatusCode::UNAUTHORIZED {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
    }
    if response.status() == StatusCode::TOO_MANY_REQUESTS {
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("60"));
    }
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}
