use super::{AvailableChannels, Session, TrackGeometryQuality, TrackGeometrySource};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IngestStatus {
    NotIngested,
    Fetching,
    Normalizing,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionReadiness {
    pub session: Session,
    pub ingest_status: IngestStatus,
    pub replay_ready: bool,
    pub is_demo: bool,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EndpointCoverage {
    pub endpoint: String,
    pub present: bool,
    pub rows: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IngestTrackGeometrySummary {
    pub status: TrackGeometryQuality,
    pub source: TrackGeometrySource,
    pub quality: TrackGeometryQuality,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IngestResponse {
    pub session_key: i64,
    pub status: IngestStatus,
    pub cached_endpoints: usize,
    pub endpoint_coverage: Vec<EndpointCoverage>,
    pub generated_snapshots: usize,
    pub track_geometry: Option<IngestTrackGeometrySummary>,
    pub available_channels: Option<AvailableChannels>,
    pub warnings: Vec<String>,
    pub error: Option<String>,
}
