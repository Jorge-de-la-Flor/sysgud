//! Motor de decisiones (`AgentEngine` en la propuesta original).
//!
//! Construye el payload contextualizado a partir del extracto de logs
//! y consulta al LLM para obtener una acción de remediación estrictamente
//! tipada (ver [`crate::core::AgentAction`]).

pub mod client;
pub mod prompt;

pub use client::AgentClient;
