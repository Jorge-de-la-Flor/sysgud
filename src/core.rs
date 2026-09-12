//! Tipos, configuración y errores compartidos por el resto de módulos.
//!
//! Este módulo no depende de `monitor`, `agent` ni `actions`: es la base
//! sobre la que esos tres construyen sus implementaciones.

pub mod config;
pub mod error;
pub mod types;

pub use config::Config;
pub use error::SysgudError;
pub use types::{ActionType, AgentAction, AgentRequest, PendingApproval, PendingStore};
