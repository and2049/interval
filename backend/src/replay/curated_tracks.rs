use crate::domain::{
    MapMode, TrackBounds, TrackGeometry, TrackGeometryQuality, TrackGeometrySource, TrackPoint,
    REPLAY_CONTRACT_VERSION,
};
use chrono::Utc;

pub const BAHRAIN_SESSION_KEY: i64 = 9472;

const TRACK_WIDTH: f64 = 190.0;

const BAHRAIN_CENTERLINE: &[(f64, f64)] = &[
    (1120.0, 1420.0),
    (1540.0, 1390.0),
    (2010.0, 1250.0),
    (2250.0, 980.0),
    (2130.0, 760.0),
    (1780.0, 700.0),
    (1350.0, 760.0),
    (980.0, 920.0),
    (650.0, 860.0),
    (520.0, 640.0),
    (720.0, 430.0),
    (1130.0, 360.0),
    (1540.0, 420.0),
    (1830.0, 610.0),
    (2020.0, 560.0),
    (2150.0, 360.0),
    (2030.0, 170.0),
    (1660.0, 120.0),
    (1260.0, 190.0),
    (940.0, 330.0),
    (730.0, 260.0),
    (560.0, 120.0),
    (350.0, 170.0),
    (300.0, 410.0),
    (470.0, 680.0),
    (760.0, 840.0),
    (970.0, 1030.0),
    (860.0, 1220.0),
    (610.0, 1290.0),
    (420.0, 1170.0),
    (330.0, 930.0),
    (250.0, 700.0),
    (120.0, 580.0),
    (70.0, 760.0),
    (180.0, 1030.0),
    (430.0, 1300.0),
    (720.0, 1430.0),
    (1120.0, 1420.0),
];

pub fn curated_geometry(session_key: i64) -> Option<TrackGeometry> {
    match session_key {
        BAHRAIN_SESSION_KEY => Some(build_geometry(session_key, BAHRAIN_CENTERLINE)),
        _ => None,
    }
}

fn build_geometry(session_key: i64, centerline: &[(f64, f64)]) -> TrackGeometry {
    let mut points = centerline
        .iter()
        .map(|(x, y)| TrackPoint {
            x: *x,
            y: *y,
            z: None,
            cumulative_distance: 0.0,
            relative_distance: 0.0,
        })
        .collect::<Vec<_>>();
    apply_distances(&mut points);
    let (inner_edge, outer_edge) = display_edges(&points);
    let bounds = bounds_for(&points, &inner_edge, &outer_edge);
    let circuit_length = points.last().map(|point| point.cumulative_distance);

    TrackGeometry {
        contract_version: REPLAY_CONTRACT_VERSION.to_string(),
        session_key,
        bounds,
        centerline: points,
        inner_edge,
        outer_edge,
        source: TrackGeometrySource::CuratedStatic,
        quality: TrackGeometryQuality::Ready,
        map_mode: MapMode::Projected,
        circuit_length,
        generated_at: Utc::now().to_rfc3339(),
    }
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

fn bounds_for(center: &[TrackPoint], inner: &[TrackPoint], outer: &[TrackPoint]) -> TrackBounds {
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for point in center.iter().chain(inner).chain(outer) {
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
        let point = &points[idx];
        inner.push(edge_point(
            point,
            -nx * TRACK_WIDTH / 2.0,
            -ny * TRACK_WIDTH / 2.0,
        ));
        outer.push(edge_point(
            point,
            nx * TRACK_WIDTH / 2.0,
            ny * TRACK_WIDTH / 2.0,
        ));
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

fn distance(a: &TrackPoint, b: &TrackPoint) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    (dx * dx + dy * dy).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bahrain_geometry_is_ready() {
        let geometry = curated_geometry(BAHRAIN_SESSION_KEY).unwrap();
        assert_eq!(geometry.source, TrackGeometrySource::CuratedStatic);
        assert_eq!(geometry.quality, TrackGeometryQuality::Ready);
        assert_eq!(geometry.map_mode, MapMode::Projected);
        assert!(geometry.centerline.len() > 20);
        assert!(geometry.inner_edge.len() == geometry.centerline.len());
        assert!(geometry.outer_edge.len() == geometry.centerline.len());
        assert!(geometry.circuit_length.unwrap() > 1_000.0);
    }
}
