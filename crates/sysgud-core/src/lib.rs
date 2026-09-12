//! Domain contract only: no filesystem, network, process or HTTP dependencies.
pub mod incidents;
pub mod types;
pub use incidents::{Decision, Incident, IncidentStatus, Severity};
pub use types::{ActionType, AgentAction, AgentRequest};
pub const MAX_LINE_BYTES: usize = 8192;
pub const MAX_SOURCES: usize = 256;
pub const MAX_PAGE_SIZE: usize = 100;
