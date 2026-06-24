use super::{quality::DataQuality, quality::MapMode, REPLAY_CONTRACT_VERSION};
use crate::domain::{
    DerivedMetric, Driver, RaceControlMessage, ReplayCursor, Sector, SectorStatus,
    TrackPositionSample, TyreCompound, WeatherSample,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplaySnapshot {
    #[serde(default = "contract_version")]
    pub contract_version: String,
    pub cursor: ReplayCursor,
    pub race_state: RaceState,
    pub timing: TimingSection,
    pub track: TrackSection,
    pub weather: ReplayWeatherSection,
    pub race_control: RaceControlSection,
    pub derived_metrics: Vec<DerivedMetric>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RaceState {
    pub lap: i32,
    pub track_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimingSection {
    pub rows: Vec<DriverSnapshot>,
    pub quality: DataQuality,
}

impl Default for TimingSection {
    fn default() -> Self {
        Self {
            rows: vec![],
            quality: DataQuality::Missing,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrackSection {
    pub positions: Vec<TrackPositionSample>,
    pub map_mode: MapMode,
    pub quality: DataQuality,
}

impl Default for TrackSection {
    fn default() -> Self {
        Self {
            positions: vec![],
            map_mode: MapMode::Schematic,
            quality: DataQuality::Missing,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplayWeatherSection {
    pub sample: Option<WeatherSample>,
    pub quality: DataQuality,
}

impl Default for ReplayWeatherSection {
    fn default() -> Self {
        Self {
            sample: None,
            quality: DataQuality::Missing,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RaceControlSection {
    pub messages: Vec<RaceControlMessage>,
    pub quality: DataQuality,
}

impl Default for RaceControlSection {
    fn default() -> Self {
        Self {
            messages: vec![],
            quality: DataQuality::Missing,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DriverSnapshot {
    pub driver: Driver,
    pub position: i32,
    pub rank_source: RankSource,
    pub gap_to_leader: Option<String>,
    pub interval: Option<String>,
    pub lap: i32,
    pub last_lap: Option<f64>,
    /// Pace class of `last_lap` (overall/personal best/no improvement), same scale as
    /// sector statuses. `default` so snapshots cached before this field existed still
    /// deserialize — they surface as `Unknown` until the session is re-ingested.
    #[serde(default)]
    pub last_lap_status: SectorStatus,
    /// The driver's fastest lap so far, and whether it stands as the session's overall
    /// best (`OverallBest`) or just their own (`PersonalBest`). Same cache caveat.
    #[serde(default)]
    pub best_lap: Option<f64>,
    #[serde(default)]
    pub best_lap_status: SectorStatus,
    pub compound: TyreCompound,
    pub stint_age: Option<i32>,
    pub sectors: Vec<Sector>,
    pub in_pit: bool,
    pub status: DriverStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RankSource {
    OpenF1Position,
    FastF1Position,
    SessionResult,
    DerivedProgress,
    FallbackGrid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DriverStatus {
    OnTrack,
    Pit,
    Out,
}

fn contract_version() -> String {
    REPLAY_CONTRACT_VERSION.to_string()
}
