use crate::{
    domain::{
        MapMode, TrackBounds, TrackGeometry, TrackGeometryQuality, TrackGeometrySource, TrackPoint,
        REPLAY_CONTRACT_VERSION,
    },
    normalization::LocationRecord,
};
use chrono::Utc;
use std::collections::HashMap;

const MIN_GEOMETRY_POINTS: usize = 12;
const MAX_CENTERLINE_POINTS: usize = 240;

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

    #[test]
    fn derives_geometry_from_location_samples() {
        let samples = (0..20)
            .map(|idx| LocationRecord {
                t: idx as f64,
                driver_number: 1,
                x: idx as f64 * 10.0,
                y: (idx % 3) as f64,
                z: None,
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
