use super::REPLAY_CONTRACT_VERSION;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplayEventListResponse {
    #[serde(default = "contract_version")]
    pub contract_version: String,
    pub events: Vec<ReplayEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplayEvent {
    pub id: String,
    pub t: f64,
    pub kind: EventKind,
    pub severity: EventSeverity,
    pub driver_number: Option<i32>,
    pub message: String,
    pub source: EventSource,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    RaceControl,
    TrackStatus,
    PitStop,
    StintChange,
    LeaderChange,
    WeatherChange,
    DataGap,
    DriverOut,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventSeverity {
    Info,
    Notice,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventSource {
    OpenF1,
    FastF1,
    Derived,
    System,
}

fn contract_version() -> String {
    REPLAY_CONTRACT_VERSION.to_string()
}
