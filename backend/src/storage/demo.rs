use crate::domain::{
    AvailableChannels, DataQuality, DataSource, DerivedMetric, Driver, DriverSnapshot,
    DriverStatus, EndpointLinks, EventKind, EventSeverity, EventSource, IngestStatus, MapMode,
    Meeting, RaceControlMessage, RaceControlSection, RaceState, RankSource, ReplayCursor,
    ReplayEvent, ReplayMetadata, ReplaySnapshot, ReplayWeatherSection, Sector, SectorStatus,
    Session, SessionType, TimingSection, TrackGeometry, TrackGeometryQuality, TrackGeometrySource,
    TrackGeometrySummary, TrackPositionQuality, TrackPositionSample, TrackPositionSource,
    TrackSection, TyreCompound, WeatherSample, REPLAY_CONTRACT_VERSION,
};
use sqlx::SqlitePool;

pub const DEMO_SESSION_KEY: i64 = 9839;

pub async fn seed_demo_session(pool: &SqlitePool) -> anyhow::Result<()> {
    let existing: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE session_key = ?")
        .bind(DEMO_SESSION_KEY)
        .fetch_one(pool)
        .await?;
    let session = demo_session();
    if existing > 0 {
        replace_demo_replay(pool, &session).await?;
        return Ok(());
    }

    let meeting = Meeting {
        meeting_key: 1276,
        year: 2025,
        name: "Abu Dhabi Grand Prix".to_string(),
        country: "United Arab Emirates".to_string(),
        location: "Yas Island".to_string(),
    };

    super::upsert_meetings(pool, &[meeting]).await?;
    super::upsert_sessions(pool, std::slice::from_ref(&session)).await?;
    replace_demo_replay(pool, &session).await?;

    Ok(())
}

async fn replace_demo_replay(pool: &SqlitePool, session: &Session) -> anyhow::Result<()> {
    let drivers = demo_drivers();
    let metadata = demo_metadata(&session, &drivers);
    let snapshots = demo_snapshots(session.session_key, drivers);
    let events = demo_replay_events();
    super::replace_replay(pool, &metadata, &snapshots, &events).await?;
    super::replace_track_geometry(pool, &demo_track_geometry(session.session_key)).await?;
    super::set_ingest_status(pool, session.session_key, IngestStatus::Ready, None).await?;
    Ok(())
}

fn demo_session() -> Session {
    Session {
        session_key: DEMO_SESSION_KEY,
        meeting_key: 1276,
        year: 2025,
        name: "Race".to_string(),
        session_type: SessionType::Race,
        start_time: "2025-12-07T13:00:00Z".to_string(),
        end_time: "2025-12-07T15:00:00Z".to_string(),
        total_laps: 58,
    }
}

fn demo_metadata(session: &Session, drivers: &[Driver]) -> ReplayMetadata {
    ReplayMetadata {
        contract_version: REPLAY_CONTRACT_VERSION.to_string(),
        session: session.clone(),
        meeting: Some(Meeting {
            meeting_key: 1276,
            year: 2025,
            name: "Abu Dhabi Grand Prix".to_string(),
            country: "United Arab Emirates".to_string(),
            location: "Yas Island".to_string(),
        }),
        duration_seconds: 180.0,
        frame_step_seconds: 60.0,
        total_frames: 4,
        drivers: drivers.to_vec(),
        min_t: 0.0,
        max_t: 180.0,
        generated_at: "demo".to_string(),
        data_sources: vec![DataSource {
            name: "seed_demo".to_string(),
            mode: "fixture".to_string(),
        }],
        available_channels: AvailableChannels {
            timing: true,
            location: false,
            track_geometry: false,
            weather: true,
            race_control: true,
            stints: true,
            pit_events: false,
            intervals: true,
        },
        track_geometry: TrackGeometrySummary {
            status: TrackGeometryQuality::Schematic,
            source: TrackGeometrySource::Schematic,
            quality: TrackGeometryQuality::Schematic,
        },
        endpoints: EndpointLinks {
            snapshot_endpoint: "/api/sessions/9839/replay/snapshot?t={t}".to_string(),
            stream_endpoint: "/api/sessions/9839/replay/stream".to_string(),
            events_endpoint: "/api/sessions/9839/replay/events".to_string(),
            track_geometry_endpoint: "/api/sessions/9839/track/geometry".to_string(),
        },
    }
}

fn demo_drivers() -> Vec<Driver> {
    vec![
        Driver {
            driver_number: 1,
            code: "VER".into(),
            full_name: "Max Verstappen".into(),
            team_name: "Red Bull Racing".into(),
            team_colour: "3671C6".into(),
        },
        Driver {
            driver_number: 4,
            code: "NOR".into(),
            full_name: "Lando Norris".into(),
            team_name: "McLaren".into(),
            team_colour: "FF8000".into(),
        },
        Driver {
            driver_number: 16,
            code: "LEC".into(),
            full_name: "Charles Leclerc".into(),
            team_name: "Ferrari".into(),
            team_colour: "E80020".into(),
        },
        Driver {
            driver_number: 44,
            code: "HAM".into(),
            full_name: "Lewis Hamilton".into(),
            team_name: "Ferrari".into(),
            team_colour: "E80020".into(),
        },
        Driver {
            driver_number: 81,
            code: "PIA".into(),
            full_name: "Oscar Piastri".into(),
            team_name: "McLaren".into(),
            team_colour: "FF8000".into(),
        },
    ]
}

fn demo_snapshots(session_key: i64, drivers: Vec<Driver>) -> Vec<ReplaySnapshot> {
    [0.0, 60.0, 120.0, 180.0]
        .into_iter()
        .enumerate()
        .map(|(frame, t)| {
            let lap = 1 + frame as i32;
            let track_status = if frame == 2 { "yellow" } else { "green" }.to_string();
            let driver_rows = drivers
                .iter()
                .enumerate()
                .map(|(idx, driver)| demo_driver_snapshot(driver.clone(), idx, lap, frame))
                .collect::<Vec<_>>();
            let positions = driver_rows
                .iter()
                .enumerate()
                .map(|(idx, row)| TrackPositionSample {
                    driver_number: row.driver.driver_number,
                    x: 18.0 + idx as f64 * 14.0 + frame as f64 * 5.0,
                    y: 28.0 + ((idx * 17 + frame * 8) % 62) as f64,
                    z: None,
                    relative_distance: Some(((idx as f64 * 0.09) + (frame as f64 * 0.07)) % 1.0),
                    source: TrackPositionSource::Schematic,
                    quality: TrackPositionQuality::Schematic,
                    stale_seconds: None,
                })
                .collect::<Vec<_>>();
            let weather = Some(WeatherSample {
                t,
                air_temp: Some(27.0 + frame as f64 * 0.2),
                track_temp: Some(34.0 + frame as f64 * 0.5),
                humidity: Some(41.0),
                rainfall: Some(0.0),
                wind_direction: Some(218),
                wind_speed: Some(1.8 + frame as f64 * 0.1),
            });
            let race_control_messages = demo_race_control_messages()
                .into_iter()
                .filter(|event| event.t <= t)
                .collect::<Vec<_>>();
            let derived_metrics = driver_rows
                .iter()
                .filter_map(crate::analytics::recent_pace_metric)
                .collect::<Vec<DerivedMetric>>();
            ReplaySnapshot {
                contract_version: REPLAY_CONTRACT_VERSION.to_string(),
                cursor: ReplayCursor {
                    session_key,
                    t,
                    frame_index: frame as i64,
                    playback_speed: 1.0,
                    is_paused: frame == 0,
                },
                race_state: RaceState {
                    lap,
                    track_status: track_status.clone(),
                },
                timing: TimingSection {
                    rows: driver_rows.clone(),
                    quality: DataQuality::Ready,
                },
                track: TrackSection {
                    positions: positions.clone(),
                    map_mode: MapMode::Schematic,
                    quality: DataQuality::Schematic,
                },
                weather: ReplayWeatherSection {
                    sample: weather.clone(),
                    quality: DataQuality::Ready,
                },
                race_control: RaceControlSection {
                    messages: race_control_messages.clone(),
                    quality: DataQuality::Ready,
                },
                derived_metrics: derived_metrics.clone(),
            }
        })
        .collect()
}

fn demo_track_geometry(session_key: i64) -> TrackGeometry {
    TrackGeometry {
        contract_version: REPLAY_CONTRACT_VERSION.to_string(),
        session_key,
        bounds: crate::domain::TrackBounds {
            min_x: 10.0,
            max_x: 90.0,
            min_y: 18.0,
            max_y: 86.0,
        },
        centerline: vec![],
        inner_edge: vec![],
        outer_edge: vec![],
        source: TrackGeometrySource::Schematic,
        quality: TrackGeometryQuality::Schematic,
        map_mode: MapMode::Schematic,
        circuit_length: None,
        generated_at: "demo".to_string(),
    }
}

fn demo_driver_snapshot(driver: Driver, idx: usize, lap: i32, frame: usize) -> DriverSnapshot {
    let base = 90.1 + idx as f64 * 0.55 - frame as f64 * 0.2;
    DriverSnapshot {
        driver,
        position: (idx + 1) as i32,
        rank_source: RankSource::FallbackGrid,
        gap_to_leader: if idx == 0 {
            None
        } else {
            Some(format!("+{:.1}", idx as f64 * 2.7 + frame as f64 * 0.4))
        },
        interval: if idx == 0 {
            None
        } else {
            Some(format!("+{:.1}", 1.2 + idx as f64 * 0.6))
        },
        lap,
        last_lap: Some(base),
        compound: match idx % 3 {
            0 => TyreCompound::Medium,
            1 => TyreCompound::Hard,
            _ => TyreCompound::Soft,
        },
        stint_age: Some(8 + lap + idx as i32),
        sectors: vec![
            Sector {
                index: 1,
                duration: Some(18.1 + idx as f64 * 0.08),
                status: if idx == 0 {
                    SectorStatus::OverallBest
                } else {
                    SectorStatus::Normal
                },
            },
            Sector {
                index: 2,
                duration: Some(33.4 + idx as f64 * 0.14),
                status: if idx == 1 {
                    SectorStatus::PersonalBest
                } else {
                    SectorStatus::Normal
                },
            },
            Sector {
                index: 3,
                duration: Some(22.2 + idx as f64 * 0.11),
                status: SectorStatus::Normal,
            },
        ],
        in_pit: frame == 2 && idx == 3,
        status: if frame == 2 && idx == 3 {
            DriverStatus::Pit
        } else {
            DriverStatus::OnTrack
        },
    }
}

fn demo_race_control_messages() -> Vec<RaceControlMessage> {
    vec![
        RaceControlMessage {
            t: 0.0,
            category: "session".to_string(),
            message: "Race replay loaded from local SQLite cache.".to_string(),
            flag: None,
            scope: Some("session".to_string()),
        },
        RaceControlMessage {
            t: 120.0,
            category: "flag".to_string(),
            message: "Yellow flag in sector 2.".to_string(),
            flag: Some("yellow".to_string()),
            scope: Some("sector_2".to_string()),
        },
    ]
}

fn demo_replay_events() -> Vec<ReplayEvent> {
    demo_race_control_messages()
        .into_iter()
        .enumerate()
        .map(|(idx, event)| ReplayEvent {
            id: format!("demo-race-control-{idx}"),
            t: event.t,
            kind: EventKind::RaceControl,
            severity: if event.flag.is_some() {
                EventSeverity::Warning
            } else {
                EventSeverity::Info
            },
            driver_number: None,
            message: event.message.clone(),
            source: EventSource::System,
            payload: serde_json::to_value(event).unwrap_or(serde_json::Value::Null),
        })
        .collect()
}
