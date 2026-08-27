//! Port of frontend/src/lib/trackGeometry.ts. The SVG path-string builders
//! (`pointsToPath`, `closedRoadPath`) become point-list builders because the
//! GPUI layer paints polylines/polygons instead of path strings.

use interval_backend::domain::{TrackBounds, TrackPoint};

const VIEWBOX_MIN: f64 = 5.0;
const VIEWBOX_MAX: f64 = 95.0;
const VIEWBOX_SIZE: f64 = VIEWBOX_MAX - VIEWBOX_MIN;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

pub fn has_usable_geometry(points: &[TrackPoint]) -> bool {
    points.len() > 1
}

#[derive(Debug, Clone)]
pub struct TrackPointLookup<'a> {
    pub points: &'a [TrackPoint],
    pub relative_distances: Vec<f64>,
}

pub fn create_track_point_lookup(points: &[TrackPoint]) -> Option<TrackPointLookup<'_>> {
    if !has_usable_geometry(points) {
        return None;
    }
    Some(TrackPointLookup {
        points,
        relative_distances: points.iter().map(|point| point.relative_distance).collect(),
    })
}

pub fn scale_point(point: Point, bounds: &TrackBounds) -> Point {
    let width = (bounds.max_x - bounds.min_x).max(1.0);
    let height = (bounds.max_y - bounds.min_y).max(1.0);
    let scale = (VIEWBOX_SIZE / width).min(VIEWBOX_SIZE / height);
    let drawn_width = width * scale;
    let drawn_height = height * scale;
    let offset_x = VIEWBOX_MIN + (VIEWBOX_SIZE - drawn_width) / 2.0;
    let offset_y = VIEWBOX_MIN + (VIEWBOX_SIZE - drawn_height) / 2.0;

    Point {
        x: offset_x + (point.x - bounds.min_x) * scale,
        y: offset_y + (bounds.max_y - point.y) * scale,
    }
}

/// Open polyline in draw order (was `pointsToPath`, an `M ... L ...` string).
pub fn scaled_polyline(points: &[TrackPoint], bounds: &TrackBounds) -> Vec<Point> {
    points
        .iter()
        .map(|point| scale_point(Point { x: point.x, y: point.y }, bounds))
        .collect()
}

/// Road fill polygon: outer edge followed by the reversed inner edge (was
/// `closedRoadPath`). The `Z` close is implicit — join the last point back to
/// the first when filling.
pub fn closed_road_polygon(
    outer_edge: &[TrackPoint],
    inner_edge: &[TrackPoint],
    bounds: &TrackBounds,
) -> Vec<Point> {
    if !has_usable_geometry(outer_edge) || !has_usable_geometry(inner_edge) {
        return vec![];
    }
    outer_edge
        .iter()
        .chain(inner_edge.iter().rev())
        .map(|point| scale_point(Point { x: point.x, y: point.y }, bounds))
        .collect()
}

pub fn point_at_relative_distance(points: &[TrackPoint], relative_distance: f64) -> Option<Point> {
    let lookup = create_track_point_lookup(points)?;
    point_at_relative_distance_lookup(&lookup, relative_distance)
}

pub fn point_at_relative_distance_lookup(
    lookup: &TrackPointLookup<'_>,
    relative_distance: f64,
) -> Option<Point> {
    let points = lookup.points;
    if !has_usable_geometry(points) {
        return None;
    }
    let relative = ((relative_distance % 1.0) + 1.0) % 1.0;
    let index = segment_index_for_relative_distance(&lookup.relative_distances, relative);
    if let (Some(current), Some(next)) = (points.get(index), points.get(index + 1)) {
        let span = (next.relative_distance - current.relative_distance).max(0.000001);
        let ratio = ((relative - current.relative_distance) / span).clamp(0.0, 1.0);
        return Some(Point {
            x: current.x + (next.x - current.x) * ratio,
            y: current.y + (next.y - current.y) * ratio,
        });
    }
    points.first().map(|point| Point { x: point.x, y: point.y })
}

fn segment_index_for_relative_distance(relative_distances: &[f64], relative: f64) -> usize {
    let mut low: isize = 0;
    let mut high = relative_distances.len() as isize - 2;
    while low <= high {
        let mid = (low + high) / 2;
        let current = relative_distances[mid as usize];
        let next = relative_distances[mid as usize + 1];
        if relative >= current && relative <= next {
            return mid as usize;
        }
        if relative < current {
            high = mid - 1;
        } else {
            low = mid + 1;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds() -> TrackBounds {
        TrackBounds {
            min_x: 0.0,
            max_x: 200.0,
            min_y: 0.0,
            max_y: 100.0,
        }
    }

    fn track_point(x: f64, y: f64, cumulative_distance: f64, relative_distance: f64) -> TrackPoint {
        TrackPoint {
            x,
            y,
            z: None,
            cumulative_distance,
            relative_distance,
        }
    }

    fn centerline() -> Vec<TrackPoint> {
        vec![
            track_point(0.0, 0.0, 0.0, 0.0),
            track_point(100.0, 50.0, 100.0, 0.5),
            track_point(200.0, 100.0, 200.0, 1.0),
        ]
    }

    #[test]
    fn scale_point_preserves_aspect_ratio_and_centers_the_drawing_area() {
        assert_eq!(
            scale_point(Point { x: 0.0, y: 0.0 }, &bounds()),
            Point { x: 5.0, y: 72.5 }
        );
        assert_eq!(
            scale_point(Point { x: 200.0, y: 100.0 }, &bounds()),
            Point { x: 95.0, y: 27.5 }
        );
    }

    #[test]
    fn scaled_polyline_builds_a_stable_polyline_from_backend_points() {
        assert_eq!(
            scaled_polyline(&centerline(), &bounds()),
            vec![
                Point { x: 5.0, y: 72.5 },
                Point { x: 50.0, y: 50.0 },
                Point { x: 95.0, y: 27.5 },
            ]
        );
    }

    #[test]
    fn closed_road_polygon_returns_an_empty_polygon_when_road_edges_are_not_usable() {
        assert_eq!(closed_road_polygon(&[], &centerline(), &bounds()), vec![]);
    }

    #[test]
    fn closed_road_polygon_closes_outer_and_reversed_inner_edges_into_one_road_fill_polygon() {
        assert_eq!(
            closed_road_polygon(&centerline(), &centerline(), &bounds()),
            vec![
                Point { x: 5.0, y: 72.5 },
                Point { x: 50.0, y: 50.0 },
                Point { x: 95.0, y: 27.5 },
                Point { x: 95.0, y: 27.5 },
                Point { x: 50.0, y: 50.0 },
                Point { x: 5.0, y: 72.5 },
            ]
        );
    }

    #[test]
    fn point_at_relative_distance_interpolates_between_centerline_points() {
        assert_eq!(
            point_at_relative_distance(&centerline(), 0.25),
            Some(Point { x: 50.0, y: 25.0 })
        );
    }

    #[test]
    fn point_at_relative_distance_uses_a_reusable_lookup_for_repeated_point_interpolation() {
        let centerline = centerline();
        let lookup = create_track_point_lookup(&centerline);

        assert!(lookup.is_some());
        assert_eq!(
            point_at_relative_distance_lookup(&lookup.unwrap(), 0.75),
            Some(Point { x: 150.0, y: 75.0 })
        );
    }

    #[test]
    fn point_at_relative_distance_wraps_negative_and_overflow_distances() {
        assert_eq!(
            point_at_relative_distance(&centerline(), -0.75),
            Some(Point { x: 50.0, y: 25.0 })
        );
        assert_eq!(
            point_at_relative_distance(&centerline(), 1.25),
            Some(Point { x: 50.0, y: 25.0 })
        );
    }
}
