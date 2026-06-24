use crate::{
    domain::{
        MapMode, TrackBounds, TrackGeometry, TrackGeometryQuality, TrackGeometrySource, TrackPoint,
        TrackPositionQuality, TrackPositionSample, TrackPositionSource, REPLAY_CONTRACT_VERSION,
    },
    normalization::LocationRecord,
};
use chrono::Utc;
use std::collections::HashMap;

const MIN_GEOMETRY_POINTS: usize = 12;
const MAX_CENTERLINE_POINTS: usize = 240;
const TRACK_WIDTH: f64 = 180.0;

pub fn build_track_geometry(session_key: i64, locations: &[LocationRecord]) -> TrackGeometry {
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
    apply_distances(&mut points);
    let bounds = bounds_for(&points);
    let (inner_edge, outer_edge) = display_edges(&points);
    let circuit_length = points.last().map(|point| point.cumulative_distance);

    TrackGeometry {
        contract_version: REPLAY_CONTRACT_VERSION.to_string(),
        session_key,
        bounds,
        centerline: points,
        inner_edge,
        outer_edge,
        source: TrackGeometrySource::OpenF1Location,
        quality: TrackGeometryQuality::Ready,
        map_mode: MapMode::Gps,
        circuit_length,
        generated_at: Utc::now().to_rfc3339(),
    }
}

pub fn interpolate_driver_location(
    locations: &[LocationRecord],
    driver_number: i32,
    t: f64,
) -> Option<LocationRecord> {
    let mut before = None::<&LocationRecord>;
    let mut after = None::<&LocationRecord>;

    for sample in locations
        .iter()
        .filter(|sample| sample.driver_number == driver_number)
    {
        if sample.t <= t && before.is_none_or(|existing| existing.t <= sample.t) {
            before = Some(sample);
        }
        if sample.t >= t && after.is_none_or(|existing| existing.t >= sample.t) {
            after = Some(sample);
        }
    }

    match (before, after) {
        (Some(a), Some(b)) if (b.t - a.t).abs() > f64::EPSILON => {
            let ratio = ((t - a.t) / (b.t - a.t)).clamp(0.0, 1.0);
            Some(LocationRecord {
                t,
                driver_number,
                x: a.x + (b.x - a.x) * ratio,
                y: a.y + (b.y - a.y) * ratio,
                z: match (a.z, b.z) {
                    (Some(az), Some(bz)) => Some(az + (bz - az) * ratio),
                    (Some(z), None) | (None, Some(z)) => Some(z),
                    (None, None) => None,
                },
            })
        }
        (Some(sample), _) | (_, Some(sample)) => Some(sample.clone()),
        (None, None) => None,
    }
}

pub fn position_from_location(
    geometry: &TrackGeometry,
    location: LocationRecord,
    interpolated: bool,
) -> TrackPositionSample {
    let relative_distance = project_relative_distance(geometry, location.x, location.y);
    TrackPositionSample {
        driver_number: location.driver_number,
        x: location.x,
        y: location.y,
        z: location.z,
        relative_distance,
        source: if interpolated {
            TrackPositionSource::Interpolated
        } else {
            TrackPositionSource::Real
        },
        quality: if interpolated {
            TrackPositionQuality::Interpolated
        } else {
            TrackPositionQuality::Real
        },
        stale_seconds: None,
    }
}

pub fn projected_position(
    geometry: &TrackGeometry,
    driver_number: i32,
    relative_distance: f64,
) -> Option<TrackPositionSample> {
    let point = point_at_relative_distance(geometry, relative_distance)?;
    Some(TrackPositionSample {
        driver_number,
        x: point.x,
        y: point.y,
        z: point.z,
        relative_distance: Some(relative_distance.rem_euclid(1.0)),
        source: TrackPositionSource::Projected,
        quality: TrackPositionQuality::Projected,
        stale_seconds: None,
    })
}

pub fn schematic_position(driver_number: i32, field_position: i32, t: f64) -> TrackPositionSample {
    let field_position = field_position.max(1) as f64;
    let relative_distance = ((t / 105.0) + (field_position / 20.0)) % 1.0;
    let angle = relative_distance * std::f64::consts::TAU;
    TrackPositionSample {
        driver_number,
        x: 50.0 + angle.cos() * 35.0,
        y: 50.0 + angle.sin() * 25.0,
        z: None,
        relative_distance: Some(relative_distance),
        source: TrackPositionSource::Schematic,
        quality: TrackPositionQuality::Schematic,
        stale_seconds: None,
    }
}

fn point_at_relative_distance(
    geometry: &TrackGeometry,
    relative_distance: f64,
) -> Option<TrackPoint> {
    if geometry.centerline.len() < 2 {
        return None;
    }
    let relative_distance = relative_distance.rem_euclid(1.0);
    let target = relative_distance * geometry.circuit_length?;
    for pair in geometry.centerline.windows(2) {
        let a = &pair[0];
        let b = &pair[1];
        if target >= a.cumulative_distance && target <= b.cumulative_distance {
            let span = (b.cumulative_distance - a.cumulative_distance).max(f64::EPSILON);
            let ratio = ((target - a.cumulative_distance) / span).clamp(0.0, 1.0);
            return Some(TrackPoint {
                x: a.x + (b.x - a.x) * ratio,
                y: a.y + (b.y - a.y) * ratio,
                z: match (a.z, b.z) {
                    (Some(az), Some(bz)) => Some(az + (bz - az) * ratio),
                    (Some(z), None) | (None, Some(z)) => Some(z),
                    (None, None) => None,
                },
                cumulative_distance: target,
                relative_distance,
            });
        }
    }
    geometry.centerline.first().cloned()
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

fn apply_distances(points: &mut [TrackPoint]) {
    let mut total = 0.0;
    for idx in 1..points.len() {
        total += distance(&points[idx - 1], &points[idx]);
        points[idx].cumulative_distance = total;
    }
    if total > 0.0 {
        for point in points {
            point.relative_distance = point.cumulative_distance / total;
        }
    }
}

fn bounds_for(points: &[TrackPoint]) -> TrackBounds {
    let (mut min_x, mut max_x) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut min_y, mut max_y) = (f64::INFINITY, f64::NEG_INFINITY);
    for point in points {
        min_x = min_x.min(point.x);
        max_x = max_x.max(point.x);
        min_y = min_y.min(point.y);
        max_y = max_y.max(point.y);
    }
    TrackBounds {
        min_x,
        max_x,
        min_y,
        max_y,
    }
}

fn display_edges(points: &[TrackPoint]) -> (Vec<TrackPoint>, Vec<TrackPoint>) {
    let mut inner = Vec::with_capacity(points.len());
    let mut outer = Vec::with_capacity(points.len());
    for idx in 0..points.len() {
        let previous = points.get(idx.saturating_sub(1)).unwrap_or(&points[idx]);
        let next = points
            .get((idx + 1).min(points.len() - 1))
            .unwrap_or(&points[idx]);
        let dx = next.x - previous.x;
        let dy = next.y - previous.y;
        let norm = (dx * dx + dy * dy).sqrt().max(1.0);
        let nx = -dy / norm;
        let ny = dx / norm;
        let offset = TRACK_WIDTH / 2.0;
        let point = &points[idx];
        inner.push(edge_point(point, -nx * offset, -ny * offset));
        outer.push(edge_point(point, nx * offset, ny * offset));
    }
    (inner, outer)
}

fn edge_point(point: &TrackPoint, dx: f64, dy: f64) -> TrackPoint {
    TrackPoint {
        x: point.x + dx,
        y: point.y + dy,
        z: point.z,
        cumulative_distance: point.cumulative_distance,
        relative_distance: point.relative_distance,
    }
}

fn project_relative_distance(geometry: &TrackGeometry, x: f64, y: f64) -> Option<f64> {
    if geometry.centerline.len() < 2 {
        return None;
    }
    let mut best_distance_sq = f64::INFINITY;
    let mut best_progress = 0.0;
    for pair in geometry.centerline.windows(2) {
        let a = &pair[0];
        let b = &pair[1];
        let vx = b.x - a.x;
        let vy = b.y - a.y;
        let len_sq = vx * vx + vy * vy;
        if len_sq <= f64::EPSILON {
            continue;
        }
        let ratio = (((x - a.x) * vx + (y - a.y) * vy) / len_sq).clamp(0.0, 1.0);
        let px = a.x + vx * ratio;
        let py = a.y + vy * ratio;
        let dx = x - px;
        let dy = y - py;
        let distance_sq = dx * dx + dy * dy;
        if distance_sq < best_distance_sq {
            best_distance_sq = distance_sq;
            best_progress =
                a.cumulative_distance + (b.cumulative_distance - a.cumulative_distance) * ratio;
        }
    }
    geometry
        .circuit_length
        .filter(|length| *length > 0.0)
        .map(|length| (best_progress / length).clamp(0.0, 1.0))
}

fn distance(a: &TrackPoint, b: &TrackPoint) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    (dx * dx + dy * dy).sqrt()
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

        let geometry = build_track_geometry(42, &samples);
        assert_eq!(geometry.quality, TrackGeometryQuality::Ready);
        assert!(geometry.centerline.len() >= MIN_GEOMETRY_POINTS);
        assert!(geometry.circuit_length.unwrap() > 0.0);
    }

    #[test]
    fn rejects_insufficient_geometry_samples() {
        let geometry = build_track_geometry(42, &[]);
        assert_eq!(geometry.quality, TrackGeometryQuality::Schematic);
    }

    #[test]
    fn uses_curated_bahrain_geometry_when_location_is_missing() {
        let geometry = build_track_geometry(crate::replay::BAHRAIN_SESSION_KEY, &[]);
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

        let geometry = build_track_geometry(crate::replay::BAHRAIN_SESSION_KEY, &samples);
        assert_eq!(geometry.source, TrackGeometrySource::OpenF1Location);
        assert_eq!(geometry.map_mode, MapMode::Gps);
    }

    #[test]
    fn interpolates_between_driver_samples() {
        let samples = vec![
            LocationRecord {
                t: 0.0,
                driver_number: 4,
                x: 0.0,
                y: 0.0,
                z: None,
            },
            LocationRecord {
                t: 10.0,
                driver_number: 4,
                x: 20.0,
                y: 10.0,
                z: None,
            },
        ];

        let sample = interpolate_driver_location(&samples, 4, 5.0).unwrap();
        assert_eq!(sample.x, 10.0);
        assert_eq!(sample.y, 5.0);
    }

    #[test]
    fn projects_relative_distance_on_centerline() {
        let samples = (0..20)
            .map(|idx| LocationRecord {
                t: idx as f64,
                driver_number: 1,
                x: idx as f64 * 10.0,
                y: 0.0,
                z: None,
            })
            .collect::<Vec<_>>();
        let geometry = build_track_geometry(42, &samples);
        let relative = project_relative_distance(&geometry, 95.0, 2.0).unwrap();
        assert!(relative > 0.45 && relative < 0.55);
    }

    #[test]
    fn projected_position_uses_centerline_coordinates() {
        let geometry = build_track_geometry(crate::replay::BAHRAIN_SESSION_KEY, &[]);
        let position = projected_position(&geometry, 1, 0.25).unwrap();
        assert_eq!(position.source, TrackPositionSource::Projected);
        assert_eq!(position.quality, TrackPositionQuality::Projected);
        assert_eq!(position.relative_distance, Some(0.25));
        assert!(position.x >= geometry.bounds.min_x && position.x <= geometry.bounds.max_x);
        assert!(position.y >= geometry.bounds.min_y && position.y <= geometry.bounds.max_y);
    }
}
