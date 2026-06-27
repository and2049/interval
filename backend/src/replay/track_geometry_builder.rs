use crate::{
    domain::{
        MapMode, TrackBounds, TrackGeometry, TrackGeometryQuality, TrackGeometrySource, TrackPoint,
        REPLAY_CONTRACT_VERSION,
    },
    normalization::{LapRecord, LocationRecord, RaceData},
};
use chrono::Utc;
use std::collections::HashMap;

const MIN_GEOMETRY_POINTS: usize = 12;
const MAX_CENTERLINE_POINTS: usize = 240;
const MIN_LAP_GEOMETRY_SAMPLES: usize = 60;
const MIN_LAP_DURATION_SECONDS: f64 = 30.0;
const LAP_CLOSURE_MAX_FRACTION: f64 = 0.05;

/// Builds geometry from the location trace of one completed lap, so the
/// centerline covers the circuit exactly once. Raw traces span many laps
/// (plus garage and pit-lane travel), which breaks `relative_distance`
/// semantics and, for live sessions, produces degenerate early-session
/// shapes. Returns None until a closed flying lap can be extracted.
pub(crate) fn build_track_geometry_from_lap(
    session_key: i64,
    data: &RaceData,
    location_source: TrackGeometrySource,
) -> Option<TrackGeometry> {
    let samples = single_lap_samples(&data.laps, &data.locations)?;
    let geometry = build_track_geometry(session_key, &samples, location_source.clone());
    (geometry.quality == TrackGeometryQuality::Ready && geometry.source == location_source)
        .then_some(geometry)
}

fn single_lap_samples(
    laps: &[LapRecord],
    locations: &[LocationRecord],
) -> Option<Vec<LocationRecord>> {
    let mut driver_counts = HashMap::<i32, usize>::new();
    for sample in locations {
        *driver_counts.entry(sample.driver_number).or_default() += 1;
    }
    let driver_number = driver_counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(driver_number, _)| driver_number)?;

    let mut driver_samples = locations
        .iter()
        .filter(|sample| sample.driver_number == driver_number)
        .collect::<Vec<_>>();
    driver_samples.sort_by(|a, b| a.t.total_cmp(&b.t));

    let mut candidate_windows = laps
        .iter()
        .filter(|lap| lap.lap.driver_number == driver_number && !lap.lap.is_pit_out_lap)
        .filter_map(|lap| {
            let duration = lap.lap.lap_duration?;
            (duration >= MIN_LAP_DURATION_SECONDS).then_some((lap.t_start, lap.t_start + duration))
        })
        .collect::<Vec<_>>();
    // Latest lap first: recent racing laps avoid the lap-one grid launch.
    candidate_windows.sort_by(|a, b| b.0.total_cmp(&a.0));

    for (start, end) in candidate_windows {
        let from = driver_samples.partition_point(|sample| sample.t < start);
        let to = driver_samples.partition_point(|sample| sample.t <= end);
        let window = &driver_samples[from..to];
        if window.len() < MIN_LAP_GEOMETRY_SAMPLES {
            continue;
        }
        let path_length = window
            .windows(2)
            .map(|pair| sample_distance(pair[0], pair[1]))
            .sum::<f64>();
        if path_length <= 0.0 {
            continue;
        }
        // A genuine lap trace ends where it began; a partial lap, or one that
        // ends in the pit lane, does not.
        let closure = sample_distance(window[0], window[window.len() - 1]);
        if closure <= path_length * LAP_CLOSURE_MAX_FRACTION {
            return Some(window.iter().map(|sample| (*sample).clone()).collect());
        }
    }
    None
}

fn sample_distance(a: &LocationRecord, b: &LocationRecord) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    (dx * dx + dy * dy).sqrt()
}

pub(crate) fn build_track_geometry(
    session_key: i64,
    locations: &[LocationRecord],
    location_source: TrackGeometrySource,
) -> TrackGeometry {
    let mut driver_counts = HashMap::<i32, usize>::new();
    for sample in locations {
        *driver_counts.entry(sample.driver_number).or_default() += 1;
    }
    let Some(driver_number) = driver_counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(driver_number, _)| driver_number)
    else {
        return crate::replay::curated_geometry(session_key)
            .unwrap_or_else(|| schematic_geometry(session_key));
    };

    let mut samples = locations
        .iter()
        .filter(|sample| sample.driver_number == driver_number)
        .cloned()
        .collect::<Vec<_>>();
    samples.sort_by(|a, b| a.t.total_cmp(&b.t));

    let mut points = simplify_samples(&samples);
    if points.len() < MIN_GEOMETRY_POINTS {
        return crate::replay::curated_geometry(session_key)
            .unwrap_or_else(|| schematic_geometry(session_key));
    }
    super::track_geometry_math::apply_distances(&mut points);
    let bounds = super::track_geometry_math::bounds_for(&points);
    let (inner_edge, outer_edge) = super::track_geometry_math::display_edges(&points);
    let circuit_length = points.last().map(|point| point.cumulative_distance);

    TrackGeometry {
        contract_version: REPLAY_CONTRACT_VERSION.to_string(),
        session_key,
        bounds,
        centerline: points,
        inner_edge,
        outer_edge,
        source: location_source,
        quality: TrackGeometryQuality::Ready,
        map_mode: MapMode::Gps,
        circuit_length,
        generated_at: Utc::now().to_rfc3339(),
    }
}

fn simplify_samples(samples: &[LocationRecord]) -> Vec<TrackPoint> {
    let stride = (samples.len() / MAX_CENTERLINE_POINTS).max(1);
    samples
        .iter()
        .step_by(stride)
        .map(|sample| TrackPoint {
            x: sample.x,
            y: sample.y,
            z: sample.z,
            cumulative_distance: 0.0,
            relative_distance: 0.0,
        })
        .collect()
}

fn schematic_geometry(session_key: i64) -> TrackGeometry {
    TrackGeometry {
        contract_version: REPLAY_CONTRACT_VERSION.to_string(),
        session_key,
        bounds: TrackBounds {
            min_x: 0.0,
            max_x: 100.0,
            min_y: 0.0,
            max_y: 100.0,
        },
        centerline: vec![],
        inner_edge: vec![],
        outer_edge: vec![],
        source: TrackGeometrySource::Schematic,
        quality: TrackGeometryQuality::Schematic,
        map_mode: MapMode::Schematic,
        circuit_length: None,
        generated_at: Utc::now().to_rfc3339(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circular_lap_locations(
        driver_number: i32,
        t_start: f64,
        duration: f64,
        samples: usize,
    ) -> Vec<LocationRecord> {
        (0..samples)
            .map(|idx| {
                let progress = idx as f64 / samples as f64;
                let angle = progress * std::f64::consts::TAU;
                LocationRecord {
                    t: t_start + progress * duration,
                    driver_number,
                    x: 1_000.0 * angle.cos(),
                    y: 800.0 * angle.sin(),
                    z: None,
                    relative_distance: None,
                }
            })
            .collect()
    }

    fn lap(driver_number: i32, lap_number: i32, t_start: f64, duration: Option<f64>) -> LapRecord {
        LapRecord {
            t_start,
            lap: crate::domain::Lap {
                driver_number,
                lap_number,
                lap_duration: duration,
                sector_1: None,
                sector_2: None,
                sector_3: None,
                is_pit_out_lap: false,
            },
        }
    }

    fn race_data(laps: Vec<LapRecord>, locations: Vec<LocationRecord>) -> RaceData {
        RaceData {
            source: crate::normalization::RaceDataSource::OpenF1Live,
            drivers: vec![],
            laps,
            intervals: vec![],
            positions: vec![],
            locations,
            geometry_locations: vec![],
            pits: vec![],
            race_control: vec![],
            stints: vec![],
            weather: vec![],
            session_results: vec![],
        }
    }

    #[test]
    fn builds_geometry_from_single_completed_lap() {
        let mut locations = circular_lap_locations(1, 100.0, 90.0, 200);
        // Garage noise before the lap should be excluded from the centerline.
        locations.extend((0..50).map(|idx| LocationRecord {
            t: idx as f64,
            driver_number: 1,
            x: 5_000.0 + (idx % 3) as f64,
            y: 5_000.0,
            z: None,
            relative_distance: None,
        }));
        let data = race_data(vec![lap(1, 2, 100.0, Some(90.0))], locations);

        let geometry =
            build_track_geometry_from_lap(7, &data, TrackGeometrySource::OpenF1Location).unwrap();

        assert_eq!(geometry.quality, TrackGeometryQuality::Ready);
        assert_eq!(geometry.source, TrackGeometrySource::OpenF1Location);
        // Bounds hug the lap, not the garage noise.
        assert!(geometry.bounds.max_x <= 1_100.0);
        let expected_length = std::f64::consts::PI * (1_000.0 + 800.0);
        assert!((geometry.circuit_length.unwrap() - expected_length).abs() < expected_length * 0.1);
    }

    #[test]
    fn rejects_garage_traces_without_completed_laps() {
        let locations = (0..500)
            .map(|idx| LocationRecord {
                t: idx as f64 * 0.25,
                driver_number: 1,
                x: (idx % 5) as f64,
                y: (idx % 7) as f64,
                z: None,
                relative_distance: None,
            })
            .collect();
        let data = race_data(vec![], locations);

        assert!(
            build_track_geometry_from_lap(7, &data, TrackGeometrySource::OpenF1Location).is_none()
        );
    }

    #[test]
    fn rejects_lap_windows_that_do_not_close() {
        let locations = (0..200)
            .map(|idx| LocationRecord {
                t: 100.0 + idx as f64 * 0.45,
                driver_number: 1,
                x: idx as f64 * 25.0,
                y: 0.0,
                z: None,
                relative_distance: None,
            })
            .collect();
        let data = race_data(vec![lap(1, 2, 100.0, Some(90.0))], locations);

        assert!(
            build_track_geometry_from_lap(7, &data, TrackGeometrySource::OpenF1Location).is_none()
        );
    }

    #[test]
    fn derives_geometry_from_location_samples() {
        let samples = (0..20)
            .map(|idx| LocationRecord {
                t: idx as f64,
                driver_number: 1,
                x: idx as f64 * 10.0,
                y: (idx % 3) as f64,
                z: None,
                relative_distance: None,
            })
            .collect::<Vec<_>>();

        let geometry = build_track_geometry(42, &samples, TrackGeometrySource::OpenF1Location);
        assert_eq!(geometry.quality, TrackGeometryQuality::Ready);
        assert!(geometry.centerline.len() >= MIN_GEOMETRY_POINTS);
        assert!(geometry.circuit_length.unwrap() > 0.0);
    }

    #[test]
    fn rejects_insufficient_geometry_samples() {
        let geometry = build_track_geometry(42, &[], TrackGeometrySource::OpenF1Location);
        assert_eq!(geometry.quality, TrackGeometryQuality::Schematic);
    }

    #[test]
    fn uses_curated_bahrain_geometry_when_location_is_missing() {
        let geometry = build_track_geometry(
            crate::replay::BAHRAIN_SESSION_KEY,
            &[],
            TrackGeometrySource::OpenF1Location,
        );
        assert_eq!(geometry.source, TrackGeometrySource::CuratedStatic);
        assert_eq!(geometry.quality, TrackGeometryQuality::Ready);
        assert_eq!(geometry.map_mode, MapMode::Projected);
    }

    #[test]
    fn prefers_openf1_geometry_when_location_samples_are_usable() {
        let samples = (0..20)
            .map(|idx| LocationRecord {
                t: idx as f64,
                driver_number: 1,
                x: idx as f64 * 10.0,
                y: (idx % 3) as f64,
                z: None,
                relative_distance: None,
            })
            .collect::<Vec<_>>();

        let geometry = build_track_geometry(
            crate::replay::BAHRAIN_SESSION_KEY,
            &samples,
            TrackGeometrySource::OpenF1Location,
        );
        assert_eq!(geometry.source, TrackGeometrySource::OpenF1Location);
        assert_eq!(geometry.map_mode, MapMode::Gps);
    }

    #[test]
    fn labels_fastf1_geometry_when_location_samples_are_usable() {
        let samples = (0..20)
            .map(|idx| LocationRecord {
                t: idx as f64,
                driver_number: 1,
                x: idx as f64 * 10.0,
                y: (idx % 3) as f64,
                z: None,
                relative_distance: None,
            })
            .collect::<Vec<_>>();

        let geometry = build_track_geometry(
            crate::replay::BAHRAIN_SESSION_KEY,
            &samples,
            TrackGeometrySource::FastF1Telemetry,
        );

        assert_eq!(geometry.source, TrackGeometrySource::FastF1Telemetry);
        assert_eq!(geometry.map_mode, MapMode::Gps);
    }
}
