use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AgentAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IncidentStatus {
    PendingApproval,
    Executing,
    Executed,
    Failed,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Critical,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum Decision {
    Approve,
    Reject,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Incident {
    pub id: Uuid,
    pub source: String,
    pub status: IncidentStatus,
    pub severity: Severity,
    pub diagnosis: String,
    pub proposed_action: AgentAction,
    #[serde(default)]
    pub execution: Option<crate::types::CommandSpec>,
    pub logs: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub decided_at: Option<DateTime<Utc>>,
    pub decided_by: Option<i64>,
    pub target_pid: Option<u32>,
    pub error: Option<String>,
}
