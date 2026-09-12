//! Authenticated HTTP adapter. Business decisions live in sysgud-runtime.
mod handlers;
pub mod models;
mod routes;
pub use routes::router;
pub use sysgud_runtime::AppState;
#[cfg(test)]
mod tests;
