use serde::Deserialize;
use sysgud_core::IncidentStatus;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventRequest {
    pub source: String,
    #[serde(alias = "log_line")]
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionRequest {
    pub actor_id: i64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IncidentQuery {
    pub status: Option<IncidentStatus>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}
