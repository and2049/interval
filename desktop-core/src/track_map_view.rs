//! Port of frontend/src/lib/trackMapView.ts. Colours stay as data (`#RRGGBB`
//! strings built from `team_colour`) — the GPUI layer converts them to
//! concrete colors.

use std::collections::HashMap;

use interval_backend::domain::{
    Driver, DriverSnapshot, DriverStatus, MapMode, TrackGeometry, TrackGeometryQuality,
    TrackPositionQuality, TrackPositionSample, TrackPositionSource,
};

use crate::track_geometry::{
    Point, TrackPointLookup, has_usable_geometry, point_at_relative_distance,
    point_at_relative_distance_lookup, scale_point,
};

#[derive(Debug, Clone, PartialEq)]
pub struct TrackDistanceMarker {
    pub label: String,
    pub point: Point,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrackDriverDot {
    pub code: String,
    pub color: String,
    pub driver_number: i32,
    pub point: Point,
    pub source: TrackPositionSource,
    pub quality: TrackPositionQuality,
    pub label: String,
    pub is_leader: bool,
    pub is_out: bool,
    pub is_stale: bool,
    pub opacity: f64,
    pub radius: f64,
    pub show_code: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackMapRenderMode {
    Real,
    Schematic,
    Pending,
    Error,
}

pub fn has_real_track_geometry(geometry: Option<&TrackGeometry>) -> bool {
    real_track_geometry(geometry).is_some()
}

fn real_track_geometry(geometry: Option<&TrackGeometry>) -> Option<&TrackGeometry> {
    geometry.filter(|geometry| {
        geometry.quality == TrackGeometryQuality::Ready && has_usable_geometry(&geometry.centerline)
    })
}

pub fn track_map_render_mode(
    map_mode: MapMode,
    geometry: Option<&TrackGeometry>,
    has_error: bool,
) -> TrackMapRenderMode {
    if has_real_track_geometry(geometry) {
        return TrackMapRenderMode::Real;
    }
    if map_mode != MapMode::Schematic && has_error {
        return TrackMapRenderMode::Error;
    }
    if map_mode == MapMode::Schematic {
        TrackMapRenderMode::Schematic
    } else {
        TrackMapRenderMode::Pending
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackMapPlaceholder {
    pub label: &'static str,
    pub detail: &'static str,
}

pub fn track_map_placeholder(mode: TrackMapRenderMode) -> TrackMapPlaceholder {
    if mode == TrackMapRenderMode::Error {
        TrackMapPlaceholder {
            label: "TRACK GEOMETRY UNAVAILABLE",
            detail: "MAP POSITIONS PAUSED",
        }
    } else {
        TrackMapPlaceholder {
            label: "LOADING TRACK GEOMETRY",
            detail: "MAP POSITIONS PAUSED",
        }
    }
}

pub fn display_track_point(position: Point, geometry: Option<&TrackGeometry>) -> Point {
    if let Some(geometry) = real_track_geometry(geometry) {
        return scale_point(position, &geometry.bounds);
    }
    Point {
        x: clamp(position.x, 5.0, 95.0),
        y: clamp(position.y, 8.0, 92.0),
    }
}

pub fn distance_markers(geometry: Option<&TrackGeometry>) -> Vec<TrackDistanceMarker> {
    let Some(geometry) = real_track_geometry(geometry) else {
        return vec![];
    };
    let length = geometry.circuit_length.unwrap_or(0.0);
    if length <= 0.0 {
        return vec![];
    }
    let marker_count = ((length / 1000.0).floor() as i64).min(6);
    let mut markers = vec![];
    for index in 1..=marker_count {
        let point =
            point_at_relative_distance(&geometry.centerline, (index as f64 * 1000.0) / length);
        if let Some(point) = point {
            markers.push(TrackDistanceMarker {
                label: format!("{index}K"),
                point,
            });
        }
    }
    markers
}

pub fn driver_dots(
    positions: &[TrackPositionSample],
    timing_rows: &[DriverSnapshot],
    geometry: Option<&TrackGeometry>,
) -> Vec<TrackDriverDot> {
    let drivers: HashMap<i32, &Driver> = timing_rows
        .iter()
        .map(|row| (row.driver.driver_number, &row.driver))
        .collect();
    let ranks: HashMap<i32, i32> = timing_rows
        .iter()
        .map(|row| (row.driver.driver_number, row.position))
        .collect();
    let statuses: HashMap<i32, &DriverStatus> = timing_rows
        .iter()
        .map(|row| (row.driver.driver_number, &row.status))
        .collect();
    let leader_number = timing_rows.first().map(|row| row.driver.driver_number);
    positions
        .iter()
        .map(|position| {
            let driver = drivers.get(&position.driver_number).copied();
            let rank = ranks.get(&position.driver_number).copied();
            let is_out = matches!(statuses.get(&position.driver_number), Some(DriverStatus::Out));
            let is_stale = position.quality == TrackPositionQuality::Stale
                || position.stale_seconds.is_some();
            let is_leader = leader_number == Some(position.driver_number);
            let code = driver
                .map(|driver| driver.code.clone())
                .unwrap_or_else(|| position.driver_number.to_string());
            TrackDriverDot {
                color: driver
                    .map(|driver| format!("#{}", driver.team_colour))
                    .unwrap_or_else(|| "#2cf5bf".to_string()),
                driver_number: position.driver_number,
                point: display_track_point(
                    Point {
                        x: position.x,
                        y: position.y,
                    },
                    geometry,
                ),
                source: position.source.clone(),
                quality: position.quality.clone(),
                label: driver_dot_label(&code, position, is_out),
                code,
                is_leader,
                is_out,
                is_stale,
                opacity: if is_out {
                    0.38
                } else if is_stale {
                    0.52
                } else {
                    1.0
                },
                radius: if is_out || is_stale {
                    1.05
                } else if is_leader {
                    2.05
                } else {
                    1.55
                },
                show_code: rank.is_some_and(|rank| rank <= 3),
            }
        })
        .collect()
}

pub fn interpolate_track_positions(
    from: Option<&[TrackPositionSample]>,
    to: &[TrackPositionSample],
    progress: f64,
    geometry: Option<&TrackGeometry>,
    centerline_lookup: Option<&TrackPointLookup<'_>>,
) -> Vec<TrackPositionSample> {
    let Some(from) = from.filter(|from| !from.is_empty()) else {
        return to.to_vec();
    };

    let ratio = clamp(progress, 0.0, 1.0);
    if ratio >= 1.0 {
        return to.to_vec();
    }
    if ratio <= 0.0 {
        return to
            .iter()
            .map(|position| {
                from.iter()
                    .find(|row| row.driver_number == position.driver_number)
                    .unwrap_or(position)
                    .clone()
            })
            .collect();
    }

    let previous_by_driver: HashMap<i32, &TrackPositionSample> = from
        .iter()
        .map(|position| (position.driver_number, position))
        .collect();
    to.iter()
        .map(|next| {
            let Some(previous) = previous_by_driver.get(&next.driver_number).copied() else {
                return next.clone();
            };
            if !can_interpolate_position(previous) || !can_interpolate_position(next) {
                return next.clone();
            }

            let relative_distance = interpolate_relative_distance(
                previous.relative_distance,
                next.relative_distance,
                ratio,
            );
            let point = point_for_relative_distance(relative_distance, geometry, centerline_lookup);

            TrackPositionSample {
                x: point
                    .map(|point| point.x)
                    .unwrap_or_else(|| interpolate_number(previous.x, next.x, ratio)),
                y: point
                    .map(|point| point.y)
                    .unwrap_or_else(|| interpolate_number(previous.y, next.y, ratio)),
                z: interpolate_optional_number(previous.z, next.z, ratio),
                relative_distance: relative_distance.or(next.relative_distance),
                ..next.clone()
            }
        })
        .collect()
}

fn point_for_relative_distance(
    relative_distance: Option<f64>,
    geometry: Option<&TrackGeometry>,
    centerline_lookup: Option<&TrackPointLookup<'_>>,
) -> Option<Point> {
    let relative_distance = relative_distance?;
    if let Some(lookup) = centerline_lookup {
        return point_at_relative_distance_lookup(lookup, relative_distance);
    }
    real_track_geometry(geometry)
        .and_then(|geometry| point_at_relative_distance(&geometry.centerline, relative_distance))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StartFinishLine {
    pub start: Point,
    pub inner: Point,
    pub outer: Point,
}

pub fn start_finish_line(geometry: Option<&TrackGeometry>) -> Option<StartFinishLine> {
    let geometry = real_track_geometry(geometry)?;
    let start = geometry.centerline.first()?;
    let inner = geometry.inner_edge.first()?;
    let outer = geometry.outer_edge.first()?;
    Some(StartFinishLine {
        start: scale_point(
            Point {
                x: start.x,
                y: start.y,
            },
            &geometry.bounds,
        ),
        inner: scale_point(
            Point {
                x: inner.x,
                y: inner.y,
            },
            &geometry.bounds,
        ),
        outer: scale_point(
            Point {
                x: outer.x,
                y: outer.y,
            },
            &geometry.bounds,
        ),
    })
}

fn can_interpolate_position(position: &TrackPositionSample) -> bool {
    position.x.is_finite() && position.y.is_finite()
}

fn interpolate_number(from: f64, to: f64, progress: f64) -> f64 {
    from + (to - from) * progress
}

fn interpolate_optional_number(from: Option<f64>, to: Option<f64>, progress: f64) -> Option<f64> {
    match (from, to) {
        (None, _) => to,
        (Some(_), None) => from,
        (Some(from), Some(to)) => {
            if !from.is_finite() || !to.is_finite() {
                Some(to)
            } else {
                Some(interpolate_number(from, to, progress))
            }
        }
    }
}

fn interpolate_relative_distance(from: Option<f64>, to: Option<f64>, progress: f64) -> Option<f64> {
    let (from, to) = (from?, to?);
    if !from.is_finite() || !to.is_finite() {
        return None;
    }

    let normalized_from = wrap_unit(from);
    let mut normalized_to = wrap_unit(to);
    if normalized_to < normalized_from && normalized_from - normalized_to > 0.5 {
        normalized_to += 1.0;
    }

    Some(wrap_unit(interpolate_number(
        normalized_from,
        normalized_to,
        progress,
    )))
}

fn clamp(value: f64, min: f64, max: f64) -> f64 {
    if !value.is_finite() {
        return min;
    }
    min.max(max.min(value))
}

// Matches JS `((value % 1) + 1) % 1` bit-for-bit; do not swap in rem_euclid.
fn wrap_unit(value: f64) -> f64 {
    ((value % 1.0) + 1.0) % 1.0
}

fn driver_dot_label(code: &str, position: &TrackPositionSample, is_out: bool) -> String {
    let stale = position
        .stale_seconds
        .map(|seconds| format!(", {:.0}s stale", seconds.round()))
        .unwrap_or_default();
    let status = if is_out { ", out" } else { "" };
    format!(
        "{code}: {}/{}{stale}{status}",
        track_position_source_label(&position.source),
        track_position_quality_label(&position.quality)
    )
}

fn track_position_source_label(source: &TrackPositionSource) -> &'static str {
    match source {
        TrackPositionSource::Real => "real",
        TrackPositionSource::Interpolated => "interpolated",
        TrackPositionSource::Projected => "projected",
        TrackPositionSource::Schematic => "schematic",
    }
}

fn track_position_quality_label(quality: &TrackPositionQuality) -> &'static str {
    match quality {
        TrackPositionQuality::Real => "real",
        TrackPositionQuality::Interpolated => "interpolated",
        TrackPositionQuality::Stale => "stale",
        TrackPositionQuality::Projected => "projected",
        TrackPositionQuality::Schematic => "schematic",
        TrackPositionQuality::Missing => "missing",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track_geometry::create_track_point_lookup;
    use interval_backend::domain::{
        RankSource, TrackBounds, TrackGeometrySource, TrackPoint, TyreCompound,
    };

    fn track_point(x: f64, y: f64, cumulative_distance: f64, relative_distance: f64) -> TrackPoint {
        TrackPoint {
            x,
            y,
            z: None,
            cumulative_distance,
            relative_distance,
        }
    }

    fn geometry() -> TrackGeometry {
        TrackGeometry {
            contract_version: "replay.v1".to_string(),
            session_key: 9472,
            bounds: TrackBounds {
                min_x: 0.0,
                max_x: 200.0,
                min_y: 0.0,
                max_y: 100.0,
            },
            centerline: vec![
                track_point(0.0, 0.0, 0.0, 0.0),
                track_point(100.0, 50.0, 1500.0, 0.5),
                track_point(200.0, 100.0, 3000.0, 1.0),
            ],
            inner_edge: vec![
                track_point(0.0, -2.0, 0.0, 0.0),
                track_point(100.0, 48.0, 1500.0, 0.5),
                track_point(200.0, 98.0, 3000.0, 1.0),
            ],
            outer_edge: vec![
                track_point(0.0, 2.0, 0.0, 0.0),
                track_point(100.0, 52.0, 1500.0, 0.5),
                track_point(200.0, 102.0, 3000.0, 1.0),
            ],
            source: TrackGeometrySource::CuratedStatic,
            quality: TrackGeometryQuality::Ready,
            map_mode: MapMode::Projected,
            circuit_length: Some(3000.0),
            generated_at: "".to_string(),
        }
    }

    fn position(driver_number: i32, x: f64, y: f64) -> TrackPositionSample {
        TrackPositionSample {
            driver_number,
            x,
            y,
            z: None,
            relative_distance: None,
            source: TrackPositionSource::Projected,
            quality: TrackPositionQuality::Projected,
            stale_seconds: None,
        }
    }

    fn row(driver_number: i32, code: &str, team_colour: &str) -> DriverSnapshot {
        DriverSnapshot {
            driver: Driver {
                driver_number,
                code: code.to_string(),
                full_name: code.to_string(),
                team_name: "Team".to_string(),
                team_colour: team_colour.to_string(),
            },
            position: driver_number,
            rank_source: RankSource::OpenF1Position,
            gap_to_leader: None,
            interval: None,
            lap: 1,
            last_lap: None,
            compound: TyreCompound::Medium,
            stint_age: None,
            sectors: vec![],
            in_pit: false,
            status: DriverStatus::OnTrack,
        }
    }

    #[test]
    fn requires_ready_geometry_with_a_usable_centerline() {
        assert!(has_real_track_geometry(Some(&geometry())));
        assert!(!has_real_track_geometry(Some(&TrackGeometry {
            quality: TrackGeometryQuality::Schematic,
            ..geometry()
        })));
        assert!(!has_real_track_geometry(Some(&TrackGeometry {
            centerline: vec![],
            ..geometry()
        })));
    }

    #[test]
    fn uses_real_geometry_when_a_usable_track_is_available() {
        assert_eq!(
            track_map_render_mode(MapMode::Projected, Some(&geometry()), false),
            TrackMapRenderMode::Real
        );
        assert_eq!(
            track_map_render_mode(MapMode::Gps, Some(&geometry()), false),
            TrackMapRenderMode::Real
        );
    }

    #[test]
    fn keeps_schematic_fallback_only_for_schematic_snapshots() {
        assert_eq!(
            track_map_render_mode(MapMode::Schematic, None, false),
            TrackMapRenderMode::Schematic
        );
        assert_eq!(
            track_map_render_mode(
                MapMode::Schematic,
                Some(&TrackGeometry {
                    quality: TrackGeometryQuality::Schematic,
                    ..geometry()
                }),
                false
            ),
            TrackMapRenderMode::Schematic
        );
    }

    #[test]
    fn marks_projected_or_gps_snapshots_as_pending_while_geometry_loads() {
        assert_eq!(
            track_map_render_mode(MapMode::Projected, None, false),
            TrackMapRenderMode::Pending
        );
        assert_eq!(
            track_map_render_mode(
                MapMode::Gps,
                Some(&TrackGeometry {
                    centerline: vec![],
                    ..geometry()
                }),
                false
            ),
            TrackMapRenderMode::Pending
        );
    }

    #[test]
    fn marks_projected_or_gps_snapshots_as_failed_when_geometry_loading_errors() {
        assert_eq!(
            track_map_render_mode(MapMode::Projected, None, true),
            TrackMapRenderMode::Error
        );
        assert_eq!(
            track_map_render_mode(MapMode::Gps, None, true),
            TrackMapRenderMode::Error
        );
        assert_eq!(
            track_map_render_mode(MapMode::Schematic, None, true),
            TrackMapRenderMode::Schematic
        );
    }

    #[test]
    fn labels_pending_and_error_placeholder_states() {
        assert_eq!(
            track_map_placeholder(TrackMapRenderMode::Pending),
            TrackMapPlaceholder {
                label: "LOADING TRACK GEOMETRY",
                detail: "MAP POSITIONS PAUSED",
            }
        );
        assert_eq!(
            track_map_placeholder(TrackMapRenderMode::Error),
            TrackMapPlaceholder {
                label: "TRACK GEOMETRY UNAVAILABLE",
                detail: "MAP POSITIONS PAUSED",
            }
        );
    }

    #[test]
    fn scales_real_geometry_points_and_clamps_schematic_coordinates() {
        assert_eq!(
            display_track_point(Point { x: 100.0, y: 50.0 }, Some(&geometry())),
            Point { x: 50.0, y: 50.0 }
        );
        assert_eq!(
            display_track_point(Point { x: -50.0, y: 500.0 }, None),
            Point { x: 5.0, y: 92.0 }
        );
    }

    #[test]
    fn places_kilometer_markers_on_usable_geometry_only() {
        assert_eq!(
            distance_markers(Some(&geometry()))
                .iter()
                .map(|marker| marker.label.clone())
                .collect::<Vec<_>>(),
            vec!["1K", "2K", "3K"]
        );
        assert_eq!(
            distance_markers(Some(&TrackGeometry {
                quality: TrackGeometryQuality::Schematic,
                ..geometry()
            })),
            vec![]
        );
    }

    #[test]
    fn decorates_positions_with_timing_driver_metadata_and_leader_state() {
        let dots = driver_dots(
            &[position(1, 100.0, 50.0), position(4, 250.0, -20.0)],
            &[row(1, "VER", "3671C6"), row(4, "NOR", "FF8000")],
            Some(&geometry()),
        );

        assert_eq!(dots[0].code, "VER");
        assert_eq!(dots[0].color, "#3671C6");
        assert_eq!(dots[0].driver_number, 1);
        assert!(dots[0].is_leader);
        assert!(!dots[0].is_out);
        assert!(!dots[0].is_stale);
        assert_eq!(dots[0].opacity, 1.0);
        assert_eq!(dots[0].radius, 2.05);
        assert!(dots[0].show_code);
        assert_eq!(dots[0].source, TrackPositionSource::Projected);
        assert_eq!(dots[0].quality, TrackPositionQuality::Projected);
        assert_eq!(dots[0].label, "VER: projected/projected");
        assert_eq!(dots[0].point, Point { x: 50.0, y: 50.0 });
        assert_eq!(dots[1].code, "NOR");
        assert!(!dots[1].is_leader);
        assert!(!dots[1].show_code);
    }

    #[test]
    fn shows_compact_labels_only_for_the_top_three_timing_rows() {
        let dots = driver_dots(
            &[
                position(1, 0.0, 0.0),
                position(2, 0.0, 0.0),
                position(3, 0.0, 0.0),
                position(4, 0.0, 0.0),
            ],
            &[
                row(1, "VER", "3671C6"),
                row(2, "LEC", "E80020"),
                row(3, "RUS", "27F4D2"),
                row(4, "NOR", "FF8000"),
            ],
            Some(&geometry()),
        );

        assert_eq!(
            dots.iter()
                .map(|dot| (dot.code.clone(), dot.show_code))
                .collect::<Vec<_>>(),
            vec![
                ("VER".to_string(), true),
                ("LEC".to_string(), true),
                ("RUS".to_string(), true),
                ("NOR".to_string(), false),
            ]
        );
    }

    #[test]
    fn includes_stale_source_metadata_in_the_accessible_dot_label() {
        let dots = driver_dots(
            &[TrackPositionSample {
                source: TrackPositionSource::Interpolated,
                quality: TrackPositionQuality::Stale,
                stale_seconds: Some(12.0),
                ..position(4, 100.0, 50.0)
            }],
            &[row(4, "NOR", "FF8000")],
            Some(&geometry()),
        );

        assert!(dots[0].is_stale);
        assert!(!dots[0].is_out);
        assert_eq!(dots[0].opacity, 0.52);
        assert_eq!(dots[0].radius, 1.05);
        assert_eq!(dots[0].label, "NOR: interpolated/stale, 12s stale");
    }

    #[test]
    fn marks_out_drivers_as_faded_frozen_dots() {
        let dots = driver_dots(
            &[TrackPositionSample {
                quality: TrackPositionQuality::Stale,
                stale_seconds: Some(18.0),
                ..position(4, 100.0, 50.0)
            }],
            &[DriverSnapshot {
                status: DriverStatus::Out,
                ..row(4, "NOR", "FF8000")
            }],
            Some(&geometry()),
        );

        assert!(dots[0].is_out);
        assert!(dots[0].is_stale);
        assert_eq!(dots[0].opacity, 0.38);
        assert_eq!(dots[0].radius, 1.05);
        assert_eq!(dots[0].label, "NOR: projected/stale, 18s stale, out");
    }

    #[test]
    fn interpolates_driver_coordinates_between_fetched_replay_frames() {
        let positions = interpolate_track_positions(
            Some(&[position(1, 0.0, 0.0)]),
            &[position(1, 100.0, 50.0)],
            0.25,
            None,
            None,
        );

        assert_eq!(positions[0].driver_number, 1);
        assert_eq!(positions[0].x, 25.0);
        assert_eq!(positions[0].y, 12.5);
    }

    #[test]
    fn uses_relative_distance_on_real_geometry_so_projected_dots_stay_on_the_centerline() {
        let track_geometry = geometry();
        let positions = interpolate_track_positions(
            Some(&[TrackPositionSample {
                relative_distance: Some(0.9),
                ..position(1, 0.0, 0.0)
            }]),
            &[TrackPositionSample {
                relative_distance: Some(0.1),
                ..position(1, 0.0, 0.0)
            }],
            0.5,
            Some(&track_geometry),
            create_track_point_lookup(&track_geometry.centerline).as_ref(),
        );

        assert!((positions[0].relative_distance.unwrap() - 0.0).abs() < 0.005);
        assert_eq!(positions[0].x, 0.0);
        assert_eq!(positions[0].y, 0.0);
    }

    #[test]
    fn returns_the_target_positions_when_no_previous_frame_is_available() {
        let target = vec![position(1, 100.0, 50.0)];

        assert_eq!(
            interpolate_track_positions(None, &target, 0.5, None, None),
            target
        );
    }

    #[test]
    fn returns_scaled_start_and_edge_points_for_real_geometry() {
        assert_eq!(
            start_finish_line(Some(&geometry())),
            Some(StartFinishLine {
                start: Point { x: 5.0, y: 72.5 },
                inner: Point { x: 5.0, y: 73.4 },
                outer: Point { x: 5.0, y: 71.6 },
            })
        );
        assert_eq!(
            start_finish_line(Some(&TrackGeometry {
                centerline: vec![],
                ..geometry()
            })),
            None
        );
    }
}
