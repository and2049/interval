use super::REPLAY_CONTRACT_VERSION;
use crate::domain::{Driver, Meeting, Session, TrackGeometryQuality, TrackGeometrySource};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplayMetadata {
    #[serde(default = "contract_version")]
    pub contract_version: String,
    pub session: Session,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meeting: Option<Meeting>,
    pub duration_seconds: f64,
    #[serde(default)]
    pub frame_step_seconds: f64,
    pub total_frames: i64,
    pub drivers: Vec<Driver>,
    pub min_t: f64,
    pub max_t: f64,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub data_sources: Vec<DataSource>,
    #[serde(default)]
    pub available_channels: AvailableChannels,
    #[serde(default)]
    pub track_geometry: TrackGeometrySummary,
    #[serde(default)]
    pub endpoints: EndpointLinks,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DataSource {
    pub name: String,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AvailableChannels {
    pub timing: bool,
    pub location: bool,
    pub track_geometry: bool,
    pub weather: bool,
    pub race_control: bool,
    pub stints: bool,
    pub pit_events: bool,
    pub intervals: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrackGeometrySummary {
    pub status: TrackGeometryQuality,
    pub source: TrackGeometrySource,
    pub quality: TrackGeometryQuality,
}

impl Default for TrackGeometrySummary {
    fn default() -> Self {
        Self {
            status: TrackGeometryQuality::Missing,
            source: TrackGeometrySource::Schematic,
            quality: TrackGeometryQuality::Missing,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct EndpointLinks {
    pub snapshot_endpoint: String,
    pub stream_endpoint: String,
    pub events_endpoint: String,
    pub track_geometry_endpoint: String,
}

fn contract_version() -> String {
    REPLAY_CONTRACT_VERSION.to_string()
}
