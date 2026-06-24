use crate::{
    domain::{
        AvailableChannels, DataQuality, DataSource, DriverSnapshot, DriverStatus, EndpointLinks,
        EventKind, EventSeverity, EventSource, MapMode, RaceControlMessage, RaceControlSection,
        RaceState, RankSource, ReplayCursor, ReplayEvent, ReplayMetadata, ReplaySnapshot,
        ReplayWeatherSection, Sector, SectorStatus, Session, Stint, TimingSection, TrackGeometry,
        TrackGeometryQuality, TrackGeometrySummary, TrackPositionSample, TrackSection,
        TyreCompound, WeatherSample, REPLAY_CONTRACT_VERSION,
    },
    normalization::{IntervalRecord, LapRecord, PitEvent, PositionRecord, RaceData, SessionResult},
};
use chrono::Utc;
use std::collections::HashMap;

const SNAPSHOT_STEP_SECONDS: f64 = 5.0;
const DEFAULT_DURATION_SECONDS: f64 = 7_200.0;
const PIT_WINDOW_SECONDS: f64 = 45.0;

pub struct GeneratedReplay {
    pub metadata: ReplayMetadata,
    pub snapshots: Vec<ReplaySnapshot>,
    pub events: Vec<ReplayEvent>,
    pub session: Session,
    pub track_geometry: TrackGeometry,
}

pub fn generate_replay(mut session: Session, data: RaceData) -> anyhow::Result<GeneratedReplay> {
    let max_lap = data
        .laps
        .iter()
        .map(|lap| lap.lap.lap_number)
        .max()
        .unwrap_or(session.total_laps);
    session.total_laps = max_lap.max(session.total_laps);

    let track_geometry =
        crate::replay::track_projection::build_track_geometry(session.session_key, &data.locations);
    let max_t = max_time(&data).unwrap_or(DEFAULT_DURATION_SECONDS);
    let mut snapshots = Vec::new();
    let mut frame_index = 0_i64;
    let mut t = 0.0;
    while t <= max_t {
        snapshots.push(snapshot_at(
            &session,
            &data,
            &track_geometry,
            t,
            frame_index,
        ));
        frame_index += 1;
        t += SNAPSHOT_STEP_SECONDS;
    }

    if snapshots.is_empty() {
        snapshots.push(snapshot_at(&session, &data, &track_geometry, 0.0, 0));
    }

    let metadata = ReplayMetadata {
        contract_version: REPLAY_CONTRACT_VERSION.to_string(),
        session: session.clone(),
        duration_seconds: max_t,
        frame_step_seconds: SNAPSHOT_STEP_SECONDS,
        total_frames: snapshots.len() as i64,
        drivers: data.drivers.clone(),
        min_t: 0.0,
        max_t,
        generated_at: Utc::now().to_rfc3339(),
        data_sources: vec![DataSource {
            name: "openf1".to_string(),
            mode: "historical".to_string(),
        }],
        available_channels: available_channels(&data, &track_geometry),
        track_geometry: TrackGeometrySummary {
            status: track_geometry.quality.clone(),
            source: track_geometry.source.clone(),
            quality: track_geometry.quality.clone(),
        },
        endpoints: endpoint_links(session.session_key),
        track_geometry_status: track_geometry_status(&track_geometry.quality).to_string(),
    };
    let events = replay_events(&data);

    Ok(GeneratedReplay {
        metadata,
        snapshots,
        events,
        session,
        track_geometry,
    })
}

fn track_geometry_status(quality: &TrackGeometryQuality) -> &'static str {
    match quality {
        TrackGeometryQuality::Ready => "ready",
        TrackGeometryQuality::Schematic => "schematic",
        TrackGeometryQuality::Missing => "missing",
    }
}

fn available_channels(data: &RaceData, geometry: &TrackGeometry) -> AvailableChannels {
    AvailableChannels {
        timing: !data.drivers.is_empty(),
        location: !data.locations.is_empty(),
        track_geometry: geometry.quality == TrackGeometryQuality::Ready,
        weather: !data.weather.is_empty(),
        race_control: !data.race_control.is_empty(),
        stints: !data.stints.is_empty(),
        pit_events: !data.pits.is_empty(),
        intervals: !data.intervals.is_empty(),
    }
}

fn endpoint_links(session_key: i64) -> EndpointLinks {
    EndpointLinks {
        snapshot_endpoint: format!("/api/sessions/{session_key}/replay/snapshot?t={{t}}"),
        stream_endpoint: format!("/api/sessions/{session_key}/replay/stream"),
        events_endpoint: format!("/api/sessions/{session_key}/replay/events"),
        track_geometry_endpoint: format!("/api/sessions/{session_key}/track/geometry"),
    }
}

fn replay_events(data: &RaceData) -> Vec<ReplayEvent> {
    let mut events = Vec::new();
    for (idx, event) in data.race_control.iter().enumerate() {
        events.push(ReplayEvent {
            id: format!("race-control-{idx}"),
            t: event.t,
            kind: EventKind::RaceControl,
            severity: if event.flag.as_deref() == Some("red") {
                EventSeverity::Critical
            } else if event.flag.is_some() {
                EventSeverity::Warning
            } else {
                EventSeverity::Info
            },
            driver_number: None,
            message: event.message.clone(),
            source: EventSource::OpenF1,
            payload: serde_json::to_value(event).unwrap_or(serde_json::Value::Null),
        });
        if event.flag.is_some() {
            events.push(ReplayEvent {
                id: format!("track-status-{idx}"),
                t: event.t,
                kind: EventKind::TrackStatus,
                severity: EventSeverity::Notice,
                driver_number: None,
                message: event.flag.clone().unwrap_or_default(),
                source: EventSource::OpenF1,
                payload: serde_json::to_value(event).unwrap_or(serde_json::Value::Null),
            });
        }
    }
    for (idx, pit) in data.pits.iter().enumerate() {
        events.push(ReplayEvent {
            id: format!("pit-stop-{idx}"),
            t: pit.t,
            kind: EventKind::PitStop,
            severity: EventSeverity::Info,
            driver_number: Some(pit.driver_number),
            message: format!("Driver {} pit stop", pit.driver_number),
            source: EventSource::OpenF1,
            payload: serde_json::json!({
                "driver_number": pit.driver_number,
                "lap_number": pit.lap_number,
                "pit_duration": pit.pit_duration
            }),
        });
    }
    events.sort_by(|a, b| a.t.total_cmp(&b.t).then_with(|| a.id.cmp(&b.id)));
    events
}

fn map_mode(geometry: &TrackGeometry) -> MapMode {
    match geometry.quality {
        TrackGeometryQuality::Ready => MapMode::Gps,
        TrackGeometryQuality::Schematic | TrackGeometryQuality::Missing => MapMode::Schematic,
    }
}

fn track_quality(geometry: &TrackGeometry) -> DataQuality {
    match geometry.quality {
        TrackGeometryQuality::Ready => DataQuality::Ready,
        TrackGeometryQuality::Schematic => DataQuality::Schematic,
        TrackGeometryQuality::Missing => DataQuality::Missing,
    }
}

fn snapshot_at(
    session: &Session,
    data: &RaceData,
    geometry: &TrackGeometry,
    t: f64,
    frame_index: i64,
) -> ReplaySnapshot {
    let lap_by_driver = latest_laps(&data.laps, t);
    let interval_by_driver = latest_intervals(&data.intervals, t);
    let rank_by_driver = latest_rank_records(&data.positions, t);
    let result_by_driver = data
        .session_results
        .iter()
        .map(|result| (result.driver_number, result))
        .collect::<HashMap<_, _>>();
    let lap = lap_by_driver
        .values()
        .map(|lap| lap.lap.lap_number)
        .max()
        .unwrap_or(1);
    let positions = latest_positions(data, geometry, t);
    let weather = latest_weather(&data.weather, t);

    let mut rows = data
        .drivers
        .iter()
        .map(|driver| {
            let lap_record = lap_by_driver.get(&driver.driver_number);
            let interval = interval_by_driver.get(&driver.driver_number);
            let rank_record = rank_by_driver.get(&driver.driver_number);
            let result = result_by_driver.get(&driver.driver_number).copied();
            let stint = stint_for(
                &data.stints,
                driver.driver_number,
                lap_record.map_or(1, |lap| lap.lap.lap_number),
            );
            let in_pit = in_pit_window(&data.pits, driver.driver_number, t);
            DriverSnapshot {
                driver: driver.clone(),
                position: rank_record
                    .map(|record| record.position)
                    .or_else(|| result.and_then(|result| result.position))
                    .unwrap_or(i32::MAX),
                rank_source: if rank_record.is_some() {
                    RankSource::OpenF1Position
                } else if result.and_then(|result| result.position).is_some() {
                    RankSource::SessionResult
                } else {
                    RankSource::FallbackGrid
                },
                gap_to_leader: interval.and_then(|row| row.gap_to_leader.clone()),
                interval: interval.and_then(|row| row.interval.clone()),
                lap: lap_record.map_or(1, |lap| lap.lap.lap_number),
                last_lap: lap_record.and_then(|lap| lap.lap.lap_duration),
                compound: stint.map_or(TyreCompound::Unknown, |stint| stint.compound.clone()),
                stint_age: stint.map(|stint| {
                    lap_record.map_or(stint.lap_start, |lap| lap.lap.lap_number) - stint.lap_start
                        + stint.tyre_age_at_start.unwrap_or(0)
                }),
                sectors: sectors_for(lap_record),
                in_pit,
                status: driver_status(in_pit, result),
            }
        })
        .collect::<Vec<_>>();

    rows.sort_by(|a, b| {
        a.position
            .cmp(&b.position)
            .then_with(|| a.gap_to_leader.cmp(&b.gap_to_leader))
            .then_with(|| a.driver.code.cmp(&b.driver.code))
    });
    for (idx, row) in rows.iter_mut().enumerate() {
        if row.position == i32::MAX {
            row.position = (idx + 1) as i32;
            row.rank_source = RankSource::FallbackGrid;
        }
    }
    let track_status = track_status(&data.race_control, t);
    let race_control_messages = data
        .race_control
        .iter()
        .filter(|event| event.t <= t)
        .cloned()
        .collect::<Vec<_>>();
    let derived_metrics = rows
        .iter()
        .filter_map(crate::analytics::recent_pace_metric)
        .collect::<Vec<_>>();
    let weather_quality = if weather.is_some() {
        DataQuality::Ready
    } else {
        DataQuality::Missing
    };
    let race_control_quality = if race_control_messages.is_empty() {
        DataQuality::Missing
    } else {
        DataQuality::Ready
    };

    ReplaySnapshot {
        contract_version: REPLAY_CONTRACT_VERSION.to_string(),
        cursor: ReplayCursor {
            session_key: session.session_key,
            t,
            frame_index,
            playback_speed: 1.0,
            is_paused: frame_index == 0,
        },
        race_state: RaceState {
            lap,
            track_status: track_status.clone(),
        },
        timing: TimingSection {
            rows: rows.clone(),
            quality: DataQuality::Ready,
        },
        track: TrackSection {
            positions: positions.clone(),
            map_mode: map_mode(geometry),
            quality: track_quality(geometry),
        },
        weather: ReplayWeatherSection {
            sample: weather.clone(),
            quality: weather_quality,
        },
        race_control: RaceControlSection {
            messages: race_control_messages.clone(),
            quality: race_control_quality,
        },
        derived_metrics: derived_metrics.clone(),
        lap,
        track_status,
        drivers: rows,
        positions,
    }
}

fn max_time(data: &RaceData) -> Option<f64> {
    data.laps
        .iter()
        .map(|lap| lap.t_start + lap.lap.lap_duration.unwrap_or(0.0))
        .chain(data.positions.iter().map(|position| position.t))
        .chain(data.locations.iter().map(|location| location.t))
        .chain(data.weather.iter().map(|weather| weather.t))
        .chain(data.race_control.iter().map(|event| event.t))
        .max_by(f64::total_cmp)
        .filter(|value| value.is_finite() && *value > 0.0)
}

fn latest_laps(laps: &[LapRecord], t: f64) -> HashMap<i32, &LapRecord> {
    let mut out = HashMap::new();
    for lap in laps.iter().filter(|lap| lap.t_start <= t) {
        let replace = out
            .get(&lap.lap.driver_number)
            .is_none_or(|existing: &&LapRecord| existing.t_start <= lap.t_start);
        if replace {
            out.insert(lap.lap.driver_number, lap);
        }
    }
    out
}

fn latest_intervals(intervals: &[IntervalRecord], t: f64) -> HashMap<i32, &IntervalRecord> {
    let mut out = HashMap::new();
    for interval in intervals.iter().filter(|interval| interval.t <= t) {
        let replace = out
            .get(&interval.driver_number)
            .is_none_or(|existing: &&IntervalRecord| existing.t <= interval.t);
        if replace {
            out.insert(interval.driver_number, interval);
        }
    }
    out
}

fn latest_positions(data: &RaceData, geometry: &TrackGeometry, t: f64) -> Vec<TrackPositionSample> {
    let ranks = latest_rank_records(&data.positions, t);
    data.drivers
        .iter()
        .map(|driver| {
            let rank = ranks
                .get(&driver.driver_number)
                .map_or(driver.driver_number, |record| record.position);
            let Some(location) = crate::replay::track_projection::interpolate_driver_location(
                &data.locations,
                driver.driver_number,
                t,
            ) else {
                return crate::replay::track_projection::schematic_position(
                    driver.driver_number,
                    rank,
                    t,
                );
            };
            let exact = data.locations.iter().any(|sample| {
                sample.driver_number == driver.driver_number && (sample.t - t).abs() < 0.001
            });
            crate::replay::track_projection::position_from_location(geometry, location, !exact)
        })
        .collect()
}

fn latest_rank_records(positions: &[PositionRecord], t: f64) -> HashMap<i32, &PositionRecord> {
    let mut out = HashMap::<i32, &PositionRecord>::new();
    for position in positions.iter().filter(|position| position.t <= t) {
        let replace = out
            .get(&position.sample.driver_number)
            .is_none_or(|existing| existing.t <= position.t);
        if replace {
            out.insert(position.sample.driver_number, position);
        }
    }
    out
}

fn latest_weather(weather: &[WeatherSample], t: f64) -> Option<WeatherSample> {
    weather
        .iter()
        .filter(|sample| sample.t <= t)
        .max_by(|a, b| a.t.total_cmp(&b.t))
        .cloned()
}

fn stint_for(stints: &[Stint], driver_number: i32, lap_number: i32) -> Option<&Stint> {
    stints.iter().find(|stint| {
        stint.driver_number == driver_number
            && stint.lap_start <= lap_number
            && stint.lap_end.unwrap_or(i32::MAX) >= lap_number
    })
}

fn in_pit_window(pits: &[PitEvent], driver_number: i32, t: f64) -> bool {
    pits.iter().any(|pit| {
        pit.driver_number == driver_number
            && pit.t <= t
            && t <= pit.t + pit.pit_duration.unwrap_or(PIT_WINDOW_SECONDS).max(10.0)
    })
}

fn sectors_for(lap: Option<&&LapRecord>) -> Vec<Sector> {
    let lap = lap.map(|lap| &lap.lap);
    [
        (1, lap.and_then(|lap| lap.sector_1)),
        (2, lap.and_then(|lap| lap.sector_2)),
        (3, lap.and_then(|lap| lap.sector_3)),
    ]
    .into_iter()
    .map(|(index, duration)| Sector {
        index,
        duration,
        status: if duration.is_some() {
            SectorStatus::Normal
        } else {
            SectorStatus::Unknown
        },
    })
    .collect()
}

fn driver_status(in_pit: bool, result: Option<&SessionResult>) -> DriverStatus {
    if in_pit {
        DriverStatus::Pit
    } else if result.is_some_and(|result| result.dnf || result.dns || result.dsq) {
        DriverStatus::Out
    } else {
        DriverStatus::OnTrack
    }
}

fn track_status(events: &[RaceControlMessage], t: f64) -> String {
    events
        .iter()
        .filter(|event| event.t <= t)
        .rev()
        .find_map(|event| event.flag.clone())
        .unwrap_or_else(|| "green".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Driver, TyreCompound};

    #[test]
    fn generates_snapshots_from_normalized_records() {
        let session = Session {
            session_key: 1,
            meeting_key: 1,
            year: 2024,
            name: "Race".to_string(),
            session_type: crate::domain::SessionType::Race,
            start_time: "2024-01-01T00:00:00Z".to_string(),
            end_time: "2024-01-01T02:00:00Z".to_string(),
            total_laps: 0,
        };
        let driver = Driver {
            driver_number: 4,
            code: "NOR".to_string(),
            full_name: "Lando Norris".to_string(),
            team_name: "McLaren".to_string(),
            team_colour: "FF8000".to_string(),
        };
        let data = RaceData {
            drivers: vec![driver],
            laps: vec![LapRecord {
                t_start: 5.0,
                lap: crate::domain::Lap {
                    driver_number: 4,
                    lap_number: 1,
                    lap_duration: Some(91.0),
                    sector_1: Some(18.0),
                    sector_2: Some(34.0),
                    sector_3: Some(22.0),
                    is_pit_out_lap: false,
                },
            }],
            intervals: vec![],
            positions: vec![PositionRecord {
                t: 5.0,
                position: 1,
                sample: TrackPositionSample {
                    driver_number: 4,
                    x: 10.0,
                    y: 20.0,
                    z: None,
                    relative_distance: None,
                    source: crate::domain::TrackPositionSource::Schematic,
                    quality: crate::domain::TrackPositionQuality::Missing,
                    stale_seconds: None,
                },
            }],
            locations: vec![crate::normalization::LocationRecord {
                t: 5.0,
                driver_number: 4,
                x: 10.0,
                y: 20.0,
                z: None,
            }],
            pits: vec![],
            race_control: vec![],
            stints: vec![Stint {
                driver_number: 4,
                stint_number: 1,
                compound: TyreCompound::Medium,
                lap_start: 1,
                lap_end: None,
                tyre_age_at_start: Some(0),
            }],
            weather: vec![],
            session_results: vec![SessionResult {
                driver_number: 4,
                position: Some(1),
                dnf: false,
                dns: false,
                dsq: false,
            }],
        };

        let generated = generate_replay(session, data).unwrap();
        assert_eq!(generated.metadata.session.total_laps, 1);
        assert!(generated.snapshots.len() > 1);
        assert_eq!(generated.snapshots[1].drivers[0].driver.code, "NOR");
    }
}
