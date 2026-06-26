use crate::domain::{TrackBounds, TrackGeometry, TrackPoint};

const TRACK_WIDTH: f64 = 180.0;

pub(crate) fn point_at_relative_distance(
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

pub(crate) fn apply_distances(points: &mut [TrackPoint]) {
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

pub(crate) fn bounds_for(points: &[TrackPoint]) -> TrackBounds {
    bounds_for_all([points])
}

pub(crate) fn bounds_for_all<'a>(
    groups: impl IntoIterator<Item = &'a [TrackPoint]>,
) -> TrackBounds {
    let (mut min_x, mut max_x) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut min_y, mut max_y) = (f64::INFINITY, f64::NEG_INFINITY);
    for group in groups {
        for point in group {
            min_x = min_x.min(point.x);
            max_x = max_x.max(point.x);
            min_y = min_y.min(point.y);
            max_y = max_y.max(point.y);
        }
    }
    TrackBounds {
        min_x,
        max_x,
        min_y,
        max_y,
    }
}

pub(crate) fn display_edges(points: &[TrackPoint]) -> (Vec<TrackPoint>, Vec<TrackPoint>) {
    display_edges_with_width(points, TRACK_WIDTH)
}

pub(crate) fn display_edges_with_width(
    points: &[TrackPoint],
    track_width: f64,
) -> (Vec<TrackPoint>, Vec<TrackPoint>) {
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
        let offset = track_width / 2.0;
        let point = &points[idx];
        inner.push(edge_point(point, -nx * offset, -ny * offset));
        outer.push(edge_point(point, nx * offset, ny * offset));
    }
    (inner, outer)
}

pub(crate) fn project_relative_distance(geometry: &TrackGeometry, x: f64, y: f64) -> Option<f64> {
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
