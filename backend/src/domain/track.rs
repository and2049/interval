use super::{MapMode, REPLAY_CONTRACT_VERSION};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrackGeometry {
    #[serde(default = "contract_version")]
    pub contract_version: String,
    pub session_key: i64,
    pub bounds: TrackBounds,
    pub centerline: Vec<TrackPoint>,
    pub inner_edge: Vec<TrackPoint>,
    pub outer_edge: Vec<TrackPoint>,
    pub source: TrackGeometrySource,
    pub quality: TrackGeometryQuality,
    #[serde(default)]
    pub map_mode: MapMode,
    pub circuit_length: Option<f64>,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrackBounds {
    pub min_x: f64,
    pub max_x: f64,
    pub min_y: f64,
    pub max_y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrackPoint {
    pub x: f64,
    pub y: f64,
    pub z: Option<f64>,
    pub cumulative_distance: f64,
    pub relative_distance: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrackGeometrySource {
    OpenF1Location,
    FastF1Telemetry,
    CuratedStatic,
    Schematic,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrackGeometryQuality {
    Ready,
    Schematic,
    Missing,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrackPositionSource {
    Real,
    Interpolated,
    Projected,
    Schematic,
}

impl Default for TrackPositionSource {
    fn default() -> Self {
        Self::Schematic
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrackPositionQuality {
    Real,
    Interpolated,
    Stale,
    Projected,
    Schematic,
    Missing,
}

impl Default for TrackPositionQuality {
    fn default() -> Self {
        Self::Schematic
    }
}

fn contract_version() -> String {
    REPLAY_CONTRACT_VERSION.to_string()
}
