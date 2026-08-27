//! Port of `frontend/src/lib/replayQuality.ts`: badge tone/label logic for
//! channel quality, live channel health, live availability, and map mode.

use interval_backend::domain::{
    DataQuality, LiveAvailability, LiveChannelHealth, LiveChannelState, LiveSessionStatus, MapMode,
    ReplayMetadata, TrackGeometryQuality, TrackGeometrySource,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeTone {
    Ready,
    Degraded,
    Missing,
}

/// Semantic color tone; the GPUI layer maps these to theme colors
/// (mint/amber accents, neutral border + muted text).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Mint,
    Amber,
    Neutral,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelBadge {
    pub label: String,
    pub ready: bool,
    pub tone: BadgeTone,
    pub title: Option<String>,
}

pub fn channel_badges(metadata: &ReplayMetadata) -> Vec<ChannelBadge> {
    let channels = &metadata.available_channels;
    let map_mode = map_mode_label(map_mode_from_geometry(metadata).as_ref());
    vec![
        source_badge(metadata),
        cache_badge(metadata),
        badge("TIMING", channels.timing),
        badge("GPS", channels.location),
        ChannelBadge {
            label: map_mode.unwrap_or_else(|| geometry_label(metadata)).to_string(),
            ready: channels.track_geometry,
            tone: if channels.track_geometry {
                match metadata.track_geometry.source {
                    TrackGeometrySource::OpenF1Location | TrackGeometrySource::FastF1Telemetry => {
                        BadgeTone::Ready
                    }
                    _ => BadgeTone::Degraded,
                }
            } else {
                BadgeTone::Missing
            },
            title: None,
        },
        badge("WEATHER", channels.weather),
        badge("RC", channels.race_control),
        badge("PIT", channels.pit_events),
        badge("INT", channels.intervals),
    ]
}

pub fn live_channel_badges(channels: &[LiveChannelHealth]) -> Vec<ChannelBadge> {
    channels
        .iter()
        .map(|channel| ChannelBadge {
            label: channel.endpoint.to_uppercase(),
            ready: matches!(
                channel.state,
                LiveChannelState::Fresh | LiveChannelState::Cached
            ),
            tone: live_channel_tone(channel),
            title: Some(live_channel_title(channel)),
        })
        .collect()
}

pub fn live_dashboard_badges(
    metadata: &ReplayMetadata,
    channels: &[LiveChannelHealth],
) -> Vec<ChannelBadge> {
    let mut badges = vec![source_badge(metadata)];
    badges.extend(live_channel_badges(channels));
    badges
}

pub fn live_status_label(
    connection: Option<&str>,
    status: Option<&LiveSessionStatus>,
    now_ms: i64,
) -> String {
    let state = connection.unwrap_or("idle");
    let updated_ms = status
        .and_then(|status| status.updated_at.as_deref())
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|updated_at| updated_at.timestamp_millis());
    let Some(updated_ms) = updated_ms else {
        return format!("LIVE {state}");
    };

    let age_seconds = (((now_ms - updated_ms) as f64) / 1000.0).round().max(0.0) as i64;
    format!("LIVE {state} · UPDATED {age_seconds}s")
}

pub fn live_availability_badge(
    availability: &LiveAvailability,
    checking: bool,
    active: bool,
) -> ChannelBadge {
    if active {
        return badge_with_tone("LIVE OPEN", true, BadgeTone::Ready);
    }
    if checking {
        return badge_with_tone("LIVE CHECKING", false, BadgeTone::Degraded);
    }

    match availability {
        LiveAvailability::Active => badge_with_tone("LIVE READY", true, BadgeTone::Ready),
        LiveAvailability::Inactive => badge_with_tone("LIVE WAITING", false, BadgeTone::Degraded),
        LiveAvailability::Disabled => badge_with_tone("LIVE OFF", false, BadgeTone::Missing),
        LiveAvailability::Error => badge_with_tone("LIVE ERROR", false, BadgeTone::Missing),
    }
}

fn source_badge(metadata: &ReplayMetadata) -> ChannelBadge {
    let source = metadata.data_sources.first().map(|source| source.name.as_str());
    let label = format!(
        "{} · {}",
        source_label(source),
        cadence_label(metadata.frame_step_seconds)
    );
    ChannelBadge {
        label,
        ready: true,
        tone: match source {
            Some("fastf1_historical" | "live_simulation" | "openf1_live") => BadgeTone::Ready,
            _ => BadgeTone::Degraded,
        },
        title: None,
    }
}

fn cache_badge(metadata: &ReplayMetadata) -> ChannelBadge {
    let channels = &metadata.available_channels;
    let degraded = [
        channels.timing,
        channels.location,
        channels.track_geometry,
        channels.weather,
        channels.race_control,
        channels.stints,
        channels.pit_events,
        channels.intervals,
    ]
    .contains(&false);
    badge_with_tone(
        if degraded { "DEGRADED" } else { "CACHED" },
        true,
        if degraded { BadgeTone::Degraded } else { BadgeTone::Ready },
    )
}

fn source_label(source: Option<&str>) -> &'static str {
    match source {
        Some("live_simulation") => "Live Sim",
        Some("openf1_live") => "LIVE · OpenF1",
        Some("fastf1_historical") => "FastF1",
        Some("openf1_historical") => "OpenF1",
        Some("demo") => "Demo",
        Some("mixed") => "Mixed",
        _ => "Replay",
    }
}

fn cadence_label(frame_step_seconds: f64) -> String {
    if !frame_step_seconds.is_finite() || frame_step_seconds <= 0.0 {
        return "? Hz".to_string();
    }
    let hz = 1.0 / frame_step_seconds;
    if hz.fract() == 0.0 {
        format!("{hz:.0} Hz")
    } else {
        format!("{hz:.1} Hz")
    }
}

fn map_mode_from_geometry(metadata: &ReplayMetadata) -> Option<MapMode> {
    if !metadata.available_channels.track_geometry {
        return None;
    }
    Some(match metadata.track_geometry.source {
        TrackGeometrySource::OpenF1Location | TrackGeometrySource::FastF1Telemetry => MapMode::Gps,
        TrackGeometrySource::CuratedStatic => MapMode::Projected,
        TrackGeometrySource::Schematic => MapMode::Schematic,
    })
}

pub fn badge_class(tone: BadgeTone) -> Tone {
    match tone {
        BadgeTone::Ready => Tone::Mint,
        BadgeTone::Degraded => Tone::Amber,
        BadgeTone::Missing => Tone::Neutral,
    }
}

pub fn quality_badge(quality: &DataQuality) -> ChannelBadge {
    match quality {
        DataQuality::Ready | DataQuality::Real => {
            badge_with_tone(quality_text(quality), true, BadgeTone::Ready)
        }
        DataQuality::Interpolated
        | DataQuality::Projected
        | DataQuality::Schematic
        | DataQuality::Stale => badge_with_tone(quality_text(quality), false, BadgeTone::Degraded),
        DataQuality::Missing => badge_with_tone("MISSING", false, BadgeTone::Missing),
    }
}

pub fn map_mode_label(mode: Option<&MapMode>) -> Option<&'static str> {
    match mode? {
        MapMode::Gps => Some("MAP GPS"),
        MapMode::Projected => Some("MAP PROJECTED"),
        MapMode::Schematic => Some("MAP SCHEMATIC"),
    }
}

pub fn map_mode_class(mode: &MapMode) -> Tone {
    match mode {
        MapMode::Gps => Tone::Mint,
        MapMode::Projected => Tone::Amber,
        MapMode::Schematic => Tone::Neutral,
    }
}

fn badge(label: &str, ready: bool) -> ChannelBadge {
    badge_with_tone(
        label,
        ready,
        if ready { BadgeTone::Ready } else { BadgeTone::Missing },
    )
}

fn badge_with_tone(label: &str, ready: bool, tone: BadgeTone) -> ChannelBadge {
    ChannelBadge {
        label: label.to_string(),
        ready,
        tone,
        title: None,
    }
}

fn live_channel_tone(channel: &LiveChannelHealth) -> BadgeTone {
    match channel.state {
        LiveChannelState::Fresh => BadgeTone::Ready,
        LiveChannelState::Cached => {
            if channel.rows == Some(0) {
                BadgeTone::Missing
            } else if channel.last_error.is_some() {
                BadgeTone::Degraded
            } else {
                BadgeTone::Ready
            }
        }
        LiveChannelState::Stale => BadgeTone::Degraded,
        LiveChannelState::Missing | LiveChannelState::Failed => BadgeTone::Missing,
    }
}

fn live_channel_title(channel: &LiveChannelHealth) -> String {
    let rows = channel
        .rows
        .map(|rows| format!(" · {rows} rows"))
        .unwrap_or_default();
    let age = channel
        .age_seconds
        .map(|age| format!(" · {}s old", age.round().max(0.0) as i64))
        .unwrap_or_default();
    let error = channel
        .last_error
        .as_deref()
        .map(|error| format!(" · {error}"))
        .unwrap_or_default();
    format!(
        "{}: {}{rows}{age}{error}",
        channel.endpoint,
        live_channel_state_text(&channel.state)
    )
}

fn live_channel_state_text(state: &LiveChannelState) -> &'static str {
    match state {
        LiveChannelState::Fresh => "fresh",
        LiveChannelState::Cached => "cached",
        LiveChannelState::Stale => "stale",
        LiveChannelState::Missing => "missing",
        LiveChannelState::Failed => "failed",
    }
}

fn quality_text(quality: &DataQuality) -> &'static str {
    match quality {
        DataQuality::Ready => "READY",
        DataQuality::Real => "REAL",
        DataQuality::Interpolated => "INTERPOLATED",
        DataQuality::Projected => "PROJECTED",
        DataQuality::Schematic => "SCHEMATIC",
        DataQuality::Missing => "MISSING",
        DataQuality::Stale => "STALE",
    }
}

fn geometry_label(metadata: &ReplayMetadata) -> &'static str {
    match metadata.track_geometry.status {
        TrackGeometryQuality::Ready => "MAP READY",
        TrackGeometryQuality::Schematic => "MAP SCHEMATIC",
        TrackGeometryQuality::Missing => "MAP MISSING",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use interval_backend::domain::{
        AvailableChannels, DataSource, EndpointLinks, Session, SessionType, TrackGeometrySummary,
    };

    struct MetadataOptions {
        source: TrackGeometrySource,
        track_ready: bool,
        data_source: Option<&'static str>,
        frame_step_seconds: f64,
    }

    fn metadata(options: MetadataOptions) -> ReplayMetadata {
        let location = matches!(
            options.source,
            TrackGeometrySource::OpenF1Location | TrackGeometrySource::FastF1Telemetry
        );
        ReplayMetadata {
            contract_version: "replay.v1".to_string(),
            session: Session {
                session_key: 9472,
                meeting_key: 1229,
                year: 2024,
                name: "Race".to_string(),
                session_type: SessionType::Race,
                start_time: String::new(),
                end_time: String::new(),
                total_laps: 57,
            },
            meeting: None,
            duration_seconds: 0.0,
            frame_step_seconds: options.frame_step_seconds,
            total_frames: 0,
            drivers: vec![],
            min_t: 0.0,
            max_t: 0.0,
            race_start_t: 0.0,
            generated_at: String::new(),
            data_sources: options
                .data_source
                .map(|name| {
                    vec![DataSource {
                        name: name.to_string(),
                        mode: "historical".to_string(),
                    }]
                })
                .unwrap_or_default(),
            available_channels: AvailableChannels {
                timing: true,
                location,
                track_geometry: options.track_ready,
                weather: false,
                race_control: false,
                stints: false,
                pit_events: false,
                intervals: false,
            },
            track_geometry: TrackGeometrySummary {
                status: if options.track_ready {
                    TrackGeometryQuality::Ready
                } else {
                    TrackGeometryQuality::Missing
                },
                source: options.source,
                quality: if options.track_ready {
                    TrackGeometryQuality::Ready
                } else {
                    TrackGeometryQuality::Missing
                },
            },
            endpoints: EndpointLinks::default(),
        }
    }

    fn channel(
        endpoint: &str,
        state: LiveChannelState,
        age_seconds: f64,
        rows: usize,
        last_error: Option<&str>,
    ) -> LiveChannelHealth {
        LiveChannelHealth {
            endpoint: endpoint.to_string(),
            state,
            age_seconds: Some(age_seconds),
            rows: Some(rows),
            last_error: last_error.map(str::to_string),
        }
    }

    #[test]
    fn labels_map_modes_for_compact_ui_badges() {
        assert_eq!(map_mode_label(Some(&MapMode::Gps)), Some("MAP GPS"));
        assert_eq!(map_mode_label(Some(&MapMode::Projected)), Some("MAP PROJECTED"));
        assert_eq!(map_mode_label(Some(&MapMode::Schematic)), Some("MAP SCHEMATIC"));
    }

    #[test]
    fn uses_semantic_tones_for_map_modes() {
        assert_eq!(map_mode_class(&MapMode::Gps), Tone::Mint);
        assert_eq!(map_mode_class(&MapMode::Projected), Tone::Amber);
        assert_eq!(map_mode_class(&MapMode::Schematic), Tone::Neutral);
    }

    #[test]
    fn marks_curated_static_geometry_as_projected_and_degraded() {
        let badges = channel_badges(&metadata(MetadataOptions {
            source: TrackGeometrySource::CuratedStatic,
            track_ready: true,
            data_source: None,
            frame_step_seconds: 1.0,
        }));

        let badge = badges
            .iter()
            .find(|badge| badge.label == "MAP PROJECTED")
            .unwrap();
        assert!(badge.ready);
        assert_eq!(badge.tone, BadgeTone::Degraded);
    }

    #[test]
    fn marks_open_f1_location_geometry_as_gps_and_ready() {
        let badges = channel_badges(&metadata(MetadataOptions {
            source: TrackGeometrySource::OpenF1Location,
            track_ready: true,
            data_source: None,
            frame_step_seconds: 1.0,
        }));

        let badge = badges.iter().find(|badge| badge.label == "MAP GPS").unwrap();
        assert!(badge.ready);
        assert_eq!(badge.tone, BadgeTone::Ready);
    }

    #[test]
    fn marks_fast_f1_telemetry_geometry_as_gps_and_ready() {
        let badges = channel_badges(&metadata(MetadataOptions {
            source: TrackGeometrySource::FastF1Telemetry,
            track_ready: true,
            data_source: None,
            frame_step_seconds: 1.0,
        }));

        let badge = badges.iter().find(|badge| badge.label == "MAP GPS").unwrap();
        assert!(badge.ready);
        assert_eq!(badge.tone, BadgeTone::Ready);
    }

    #[test]
    fn keeps_unavailable_channels_visually_muted() {
        assert_eq!(badge_class(BadgeTone::Missing), Tone::Neutral);
    }

    #[test]
    fn adds_source_cadence_and_cache_state_badges() {
        let badges = channel_badges(&metadata(MetadataOptions {
            source: TrackGeometrySource::FastF1Telemetry,
            track_ready: true,
            data_source: Some("fastf1_historical"),
            frame_step_seconds: 0.2,
        }));

        assert_eq!(badges[0].label, "FastF1 · 5 Hz");
        assert_eq!(badges[0].tone, BadgeTone::Ready);
        assert_eq!(badges[1].label, "DEGRADED");
        assert_eq!(badges[1].tone, BadgeTone::Degraded);
    }

    #[test]
    fn labels_live_simulation_as_a_ready_live_like_source() {
        let badges = channel_badges(&metadata(MetadataOptions {
            source: TrackGeometrySource::FastF1Telemetry,
            track_ready: true,
            data_source: Some("live_simulation"),
            frame_step_seconds: 0.2,
        }));

        assert_eq!(badges[0].label, "Live Sim · 5 Hz");
        assert_eq!(badges[0].tone, BadgeTone::Ready);
    }

    #[test]
    fn labels_openf1_live_as_a_ready_live_source() {
        let badges = channel_badges(&metadata(MetadataOptions {
            source: TrackGeometrySource::OpenF1Location,
            track_ready: true,
            data_source: Some("openf1_live"),
            frame_step_seconds: 0.5,
        }));

        assert_eq!(badges[0].label, "LIVE · OpenF1 · 2 Hz");
        assert_eq!(badges[0].tone, BadgeTone::Ready);
    }

    #[test]
    fn maps_section_quality_to_compact_badge_tones() {
        assert_eq!(
            quality_badge(&DataQuality::Ready),
            ChannelBadge {
                label: "READY".to_string(),
                ready: true,
                tone: BadgeTone::Ready,
                title: None,
            }
        );
        assert_eq!(
            quality_badge(&DataQuality::Projected),
            ChannelBadge {
                label: "PROJECTED".to_string(),
                ready: false,
                tone: BadgeTone::Degraded,
                title: None,
            }
        );
        assert_eq!(
            quality_badge(&DataQuality::Missing),
            ChannelBadge {
                label: "MISSING".to_string(),
                ready: false,
                tone: BadgeTone::Missing,
                title: None,
            }
        );
    }

    #[test]
    fn maps_live_endpoint_health_to_compact_badge_tones() {
        let badges = live_channel_badges(&[
            channel("location", LiveChannelState::Fresh, 0.2, 20, None),
            channel("weather", LiveChannelState::Cached, 5.0, 1, None),
            channel("pit", LiveChannelState::Cached, 5.0, 1, Some("timeout")),
            channel("drivers", LiveChannelState::Fresh, -1.0, 20, None),
            channel("pit", LiveChannelState::Fresh, 0.1, 0, None),
            channel("intervals", LiveChannelState::Stale, 12.0, 20, None),
            channel("race_control", LiveChannelState::Missing, 1.0, 0, None),
        ]);

        let expected = [
            ("LOCATION", true, BadgeTone::Ready, "location: fresh · 20 rows · 0s old"),
            ("WEATHER", true, BadgeTone::Ready, "weather: cached · 1 rows · 5s old"),
            ("PIT", true, BadgeTone::Degraded, "pit: cached · 1 rows · 5s old · timeout"),
            ("DRIVERS", true, BadgeTone::Ready, "drivers: fresh · 20 rows · 0s old"),
            ("PIT", true, BadgeTone::Ready, "pit: fresh · 0 rows · 0s old"),
            ("INTERVALS", false, BadgeTone::Degraded, "intervals: stale · 20 rows · 12s old"),
            (
                "RACE_CONTROL",
                false,
                BadgeTone::Missing,
                "race_control: missing · 0 rows · 1s old",
            ),
        ];
        let expected = expected
            .iter()
            .map(|(label, ready, tone, title)| ChannelBadge {
                label: label.to_string(),
                ready: *ready,
                tone: *tone,
                title: Some(title.to_string()),
            })
            .collect::<Vec<_>>();
        assert_eq!(badges, expected);
    }

    #[test]
    fn keeps_the_live_source_and_cadence_badge_before_endpoint_health() {
        let badges = live_dashboard_badges(
            &metadata(MetadataOptions {
                source: TrackGeometrySource::OpenF1Location,
                track_ready: true,
                data_source: Some("openf1_live"),
                frame_step_seconds: 0.5,
            }),
            &[channel("location", LiveChannelState::Fresh, 0.2, 20, None)],
        );

        assert_eq!(badges[0].label, "LIVE · OpenF1 · 2 Hz");
        assert_eq!(badges[0].tone, BadgeTone::Ready);
        assert_eq!(badges[1].label, "LOCATION");
        assert_eq!(badges[1].tone, BadgeTone::Ready);
    }

    #[test]
    fn includes_latest_backend_update_age_when_available() {
        let status = LiveSessionStatus {
            session_key: 88_001,
            active: true,
            current_t: Some(10.0),
            max_t: Some(100.0),
            started_at: Some("2026-06-28T20:00:00.000Z".to_string()),
            updated_at: Some("2026-06-28T20:00:03.000Z".to_string()),
            source: Some("openf1_live".to_string()),
            channels: vec![],
        };
        let now_ms = chrono::DateTime::parse_from_rfc3339("2026-06-28T20:00:04.200Z")
            .unwrap()
            .timestamp_millis();

        assert_eq!(
            live_status_label(Some("connected"), Some(&status), now_ms),
            "LIVE connected · UPDATED 1s"
        );
    }

    #[test]
    fn falls_back_to_connection_state_when_update_timestamp_is_missing() {
        assert_eq!(
            live_status_label(Some("reconnecting"), None, 0),
            "LIVE reconnecting"
        );
    }

    #[test]
    fn labels_startup_live_availability_as_a_primary_status() {
        let badge = live_availability_badge(&LiveAvailability::Active, false, false);
        assert_eq!((badge.label.as_str(), badge.tone), ("LIVE READY", BadgeTone::Ready));

        let badge = live_availability_badge(&LiveAvailability::Inactive, false, false);
        assert_eq!((badge.label.as_str(), badge.tone), ("LIVE WAITING", BadgeTone::Degraded));

        let badge = live_availability_badge(&LiveAvailability::Disabled, false, false);
        assert_eq!((badge.label.as_str(), badge.tone), ("LIVE OFF", BadgeTone::Missing));

        let badge = live_availability_badge(&LiveAvailability::Error, false, false);
        assert_eq!((badge.label.as_str(), badge.tone), ("LIVE ERROR", BadgeTone::Missing));

        let badge = live_availability_badge(&LiveAvailability::Inactive, true, false);
        assert_eq!((badge.label.as_str(), badge.tone), ("LIVE CHECKING", BadgeTone::Degraded));

        let badge = live_availability_badge(&LiveAvailability::Inactive, false, true);
        assert_eq!((badge.label.as_str(), badge.tone), ("LIVE OPEN", BadgeTone::Ready));
    }
}
