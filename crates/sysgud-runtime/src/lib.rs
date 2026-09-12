//! Owns analysis, persistence and approved OS effects. Transports use the service API.
pub mod agent;
mod execution;
pub mod monitor;
pub mod security;
mod service;
mod storage;
pub use execution::CommandSpec;
pub use service::{AppState, ServiceError, ServiceOptions, Target};
#[cfg(test)]
mod tests;
pub use sysgud_core as core;
