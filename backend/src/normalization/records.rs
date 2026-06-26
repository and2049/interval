use crate::domain::{Lap, TrackPositionSample};

#[derive(Debug, Clone)]
pub struct RaceData {
    pub drivers: Vec<crate::domain::Driver>,
    pub laps: Vec<LapRecord>,
    pub intervals: Vec<IntervalRecord>,
    pub positions: Vec<PositionRecord>,
    pub locations: Vec<super::LocationRecord>,
    pub pits: Vec<PitEvent>,
    pub race_control: Vec<crate::domain::RaceControlMessage>,
    pub stints: Vec<crate::domain::Stint>,
    pub weather: Vec<crate::domain::WeatherSample>,
    pub session_results: Vec<SessionResult>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LapRecord {
    pub lap: Lap,
    pub t_start: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IntervalRecord {
    pub t: f64,
    pub driver_number: i32,
    pub gap_to_leader: Option<String>,
    pub interval: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionRecord {
    pub t: f64,
    pub position: i32,
    pub sample: TrackPositionSample,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PitEvent {
    pub t: f64,
    pub driver_number: i32,
    pub lap_number: Option<i32>,
    pub pit_duration: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionResult {
    pub driver_number: i32,
    pub position: Option<i32>,
    pub dnf: bool,
    pub dns: bool,
    pub dsq: bool,
}
