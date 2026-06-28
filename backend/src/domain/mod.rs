mod ingest;
mod replay;
mod track;

pub use ingest::{
    session_support, EndpointCoverage, IngestResponse, IngestStatus, IngestTrackGeometrySummary,
    SessionReadiness, SessionSupport, SessionSupportStatus,
};
pub use replay::*;
pub use track::{
    TrackBounds, TrackGeometry, TrackGeometryQuality, TrackGeometrySource, TrackPoint,
    TrackPositionQuality, TrackPositionSource,
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Season {
    pub year: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Meeting {
    pub meeting_key: i64,
    pub year: i32,
    pub name: String,
    pub country: String,
    pub location: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Session {
    pub session_key: i64,
    pub meeting_key: i64,
    pub year: i32,
    pub name: String,
    pub session_type: SessionType,
    pub start_time: String,
    pub end_time: String,
    pub total_laps: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionType {
    Race,
    Sprint,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Driver {
    pub driver_number: i32,
    pub code: String,
    pub full_name: String,
    pub team_name: String,
    pub team_colour: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Lap {
    pub driver_number: i32,
    pub lap_number: i32,
    pub lap_duration: Option<f64>,
    pub sector_1: Option<f64>,
    pub sector_2: Option<f64>,
    pub sector_3: Option<f64>,
    pub is_pit_out_lap: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Sector {
    pub index: i32,
    pub duration: Option<f64>,
    pub status: SectorStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SectorStatus {
    PersonalBest,
    OverallBest,
    Normal,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Stint {
    pub driver_number: i32,
    pub stint_number: i32,
    pub compound: TyreCompound,
    pub lap_start: i32,
    pub lap_end: Option<i32>,
    pub tyre_age_at_start: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum TyreCompound {
    Soft,
    Medium,
    Hard,
    Intermediate,
    Wet,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrackPositionSample {
    pub driver_number: i32,
    pub x: f64,
    pub y: f64,
    pub z: Option<f64>,
    pub relative_distance: Option<f64>,
    #[serde(default)]
    pub source: TrackPositionSource,
    #[serde(default)]
    pub quality: TrackPositionQuality,
    #[serde(default)]
    pub stale_seconds: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RaceControlMessage {
    pub t: f64,
    pub category: String,
    pub message: String,
    pub flag: Option<String>,
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WeatherSample {
    pub t: f64,
    pub air_temp: Option<f64>,
    pub track_temp: Option<f64>,
    pub humidity: Option<f64>,
    pub rainfall: Option<f64>,
    pub wind_direction: Option<i32>,
    pub wind_speed: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplayCursor {
    pub session_key: i64,
    pub t: f64,
    pub frame_index: i64,
    pub playback_speed: f64,
    pub is_paused: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DerivedMetric {
    pub driver_number: Option<i32>,
    pub kind: DerivedMetricKind,
    pub label: String,
    pub value: String,
    pub trend: MetricTrend,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DerivedMetricKind {
    RecentPace,
    StintDelta,
    PitState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MetricTrend {
    Improving,
    Stable,
    Degrading,
    Unknown,
}
