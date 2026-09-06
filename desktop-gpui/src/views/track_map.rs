//! The track map: gpui-canvas painting of what `TrackMapSvg.tsx` drew as SVG, with
//! the between-frames tween `TrackMap.tsx` ran on requestAnimationFrame.
//!
//! All geometry stays in the frontend's 0..100 "viewBox" space (the ported
//! `track_geometry`/`track_map_view` helpers produce it directly); the paint closure
//! maps it into pixels with an aspect-preserving fit, exactly like
//! `preserveAspectRatio="xMidYMid meet"`.

use std::time::Instant;

use gpui::{
    Context, PathBuilder, SharedString, TextRun, Window, canvas, div, point, prelude::*, px, rems,
    size,
};
use interval_backend::domain::{ReplaySnapshot, TrackGeometry};
use interval_desktop_core::replay_quality::{self};
use interval_desktop_core::track_geometry::{
    Point as MapPoint, create_track_point_lookup, scale_point, scaled_polyline,
};
use interval_desktop_core::track_map_view::{
    self, TrackDriverDot, TrackMapRenderMode, interpolate_track_positions,
};

use super::ui;
use crate::{IntervalApp, theme};

const MIN_TRANSITION_MS: f64 = 80.0;
const MAX_TRANSITION_MS: f64 = 1200.0;

pub struct MapAnimation {
    previous: ReplaySnapshot,
    started: Instant,
    duration_ms: f64,
}

fn transition_duration_ms(frame_step_seconds: f64, speed: f64) -> f64 {
    let speed = if speed.is_finite() && speed > 0.0 {
        speed
    } else {
        1.0
    };
    let frame_ms = if frame_step_seconds.is_finite() && frame_step_seconds > 0.0 {
        frame_step_seconds * 1000.0 / speed
    } else {
        MAX_TRANSITION_MS
    };
    frame_ms.clamp(MIN_TRANSITION_MS, MAX_TRANSITION_MS)
}

fn can_animate(previous: Option<&ReplaySnapshot>, next: &ReplaySnapshot, playing: bool) -> bool {
    playing
        && previous.is_some_and(|previous| {
            previous.cursor.session_key == next.cursor.session_key
                && previous.cursor.t < next.cursor.t
                && !previous.track.positions.is_empty()
                && !next.track.positions.is_empty()
        })
}

pub fn track_map(
    app: &mut IntervalApp,
    window: &mut Window,
    _cx: &mut Context<IntervalApp>,
) -> impl IntoElement {
    let (snapshot, geometry, geometry_error, playing, speed, frame_step) = {
        let store = app.store.state();
        let snapshot = store.active_snapshot().cloned();
        let geometry = store.active_geometry().cloned();
        let geometry_error = store.active_geometry_error().is_some();
        let playing = store.playing || store.live_active || store.live_simulation_active;
        let frame_step = store
            .display_metadata()
            .map(|meta| meta.frame_step_seconds)
            .unwrap_or(0.0);
        (
            snapshot,
            geometry,
            geometry_error,
            playing,
            store.speed,
            frame_step,
        )
    };

    let Some(snapshot) = snapshot else {
        return panel("Track Map", div().into_any_element());
    };

    // The tween state machine from TrackMap.tsx: a new frame while playing starts a
    // transition from the previous one; pausing snaps to the target.
    let cursor_changed = app
        .map_last_snapshot
        .as_ref()
        .map(|last| {
            last.cursor.session_key != snapshot.cursor.session_key
                || last.cursor.t != snapshot.cursor.t
        })
        .unwrap_or(true);
    if cursor_changed {
        let previous = app.map_last_snapshot.take();
        if can_animate(previous.as_ref(), &snapshot, playing) {
            app.map_animation = Some(MapAnimation {
                previous: previous.expect("checked by can_animate"),
                started: Instant::now(),
                duration_ms: transition_duration_ms(frame_step, speed),
            });
        } else {
            app.map_animation = None;
        }
        app.map_last_snapshot = Some(snapshot.clone());
    }
    if !playing {
        app.map_animation = None;
    }

    let progress = match &app.map_animation {
        Some(animation) => {
            (animation.started.elapsed().as_secs_f64() * 1000.0 / animation.duration_ms).min(1.0)
        }
        None => 1.0,
    };
    if progress >= 1.0 {
        app.map_animation = None;
    } else {
        window.request_animation_frame();
    }

    let mode = track_map_view::track_map_render_mode(
        snapshot.track.map_mode.clone(),
        geometry.as_ref(),
        geometry_error,
    );

    let positions = {
        let lookup = geometry
            .as_ref()
            .filter(|geometry| track_map_view::has_real_track_geometry(Some(geometry)))
            .and_then(|geometry| create_track_point_lookup(&geometry.centerline));
        interpolate_track_positions(
            app.map_animation
                .as_ref()
                .map(|animation| animation.previous.track.positions.as_slice()),
            &snapshot.track.positions,
            progress,
            geometry.as_ref(),
            lookup.as_ref(),
        )
    };
    let dots = if matches!(mode, TrackMapRenderMode::Pending | TrackMapRenderMode::Error) {
        Vec::new()
    } else {
        track_map_view::driver_dots(&positions, &snapshot.timing.rows, geometry.as_ref())
    };

    let scene = MapScene::build(mode, geometry.as_ref(), dots);
    let mono_font = app.mono_font.clone();

    // Overlay chips (top-left): lap, track status, map mode.
    let lap_chip = if snapshot.race_state.lap > 0 {
        format!("L{}", snapshot.race_state.lap)
    } else {
        "FORM".to_string()
    };
    let (status_chip, status_tone) =
        interval_desktop_core::replay_events::track_status_chip(&snapshot.race_state.track_status);
    let status_color = ui::tone_color(status_tone);
    let mode_label = replay_quality::map_mode_label(Some(&snapshot.track.map_mode))
        .unwrap_or("MAP UNKNOWN");
    let mode_tone =
        ui::quality_tone_color(replay_quality::map_mode_class(&snapshot.track.map_mode));

    let chip = |label: String, color: gpui::Hsla| {
        div()
            .border_1()
            .border_color(theme::LINE())
            .bg(theme::PANEL())
            .px_2()
            .py_1()
            .text_size(rems(0.68))
            .text_color(color)
            .child(label)
    };

    let body = div()
        .relative()
        .size_full()
        .min_h_0()
        .overflow_hidden()
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, cx| scene.paint(bounds, &mono_font, window, cx),
            )
            .absolute()
            .size_full(),
        )
        .child(
            div()
                .absolute()
                .left_3()
                .top_3()
                .flex()
                .flex_row()
                .gap_1()
                .child(chip(lap_chip, theme::TEXT()))
                .child(chip(status_chip, status_color))
                .child(chip(mode_label.to_string(), mode_tone)),
        )
        .into_any_element();

    panel("Track Map", body)
}

fn panel(title: &'static str, body: gpui::AnyElement) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .size_full()
        .min_h_0()
        .border_1()
        .border_color(theme::LINE())
        .bg(theme::PANEL())
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .h(rems(1.3))
                .border_b_1()
                .border_color(theme::LINE())
                .bg(theme::PANEL_HI())
                .px_2()
                .text_size(rems(0.6))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme::ACCENT())
                .child(title.to_uppercase()),
        )
        .child(div().flex_1().min_h_0().child(body))
}

/// Everything the paint closure needs, in viewBox units, computed on the entity side.
struct MapScene {
    mode: TrackMapRenderMode,
    centerline: Vec<MapPoint>,
    start_finish: Option<track_map_view::StartFinishLine>,
    markers: Vec<(String, MapPoint)>,
    placeholder: Option<track_map_view::TrackMapPlaceholder>,
    dots: Vec<TrackDriverDot>,
}

impl MapScene {
    fn build(
        mode: TrackMapRenderMode,
        geometry: Option<&TrackGeometry>,
        dots: Vec<TrackDriverDot>,
    ) -> Self {
        let (centerline, start_finish, markers) = match (mode, geometry) {
            (TrackMapRenderMode::Real, Some(geometry)) => (
                scaled_polyline(&geometry.centerline, &geometry.bounds),
                track_map_view::start_finish_line(Some(geometry)),
                track_map_view::distance_markers(Some(geometry))
                    .into_iter()
                    .map(|marker| (marker.label, scale_point(marker.point, &geometry.bounds)))
                    .collect(),
            ),
            _ => (Vec::new(), None, Vec::new()),
        };
        let placeholder = matches!(
            mode,
            TrackMapRenderMode::Pending | TrackMapRenderMode::Error
        )
        .then(|| track_map_view::track_map_placeholder(mode));
        Self {
            mode,
            centerline,
            start_finish,
            markers,
            placeholder,
            dots,
        }
    }

    fn paint(
        &self,
        bounds: gpui::Bounds<gpui::Pixels>,
        mono_font: &SharedString,
        window: &mut Window,
        cx: &mut gpui::App,
    ) {
        // xMidYMid meet: uniform scale, centered.
        let width = f32::from(bounds.size.width);
        let height = f32::from(bounds.size.height);
        let unit = (width.min(height) / 100.0).max(0.01);
        let ox = f32::from(bounds.origin.x) + (width - unit * 100.0) / 2.0;
        let oy = f32::from(bounds.origin.y) + (height - unit * 100.0) / 2.0;
        let map = |p: MapPoint| point(px(ox + p.x as f32 * unit), px(oy + p.y as f32 * unit));

        let stroke_polyline =
            |points: &[MapPoint],
             width_units: f32,
             color: gpui::Hsla,
             dash: Option<[f32; 2]>,
             window: &mut Window| {
                if points.len() < 2 {
                    return;
                }
                let mut builder = PathBuilder::stroke(px(width_units * unit));
                if let Some([on, off]) = dash {
                    builder = builder.dash_array(&[px(on * unit), px(off * unit)]);
                }
                builder.move_to(map(points[0]));
                for p in &points[1..] {
                    builder.line_to(map(*p));
                }
                if let Ok(path) = builder.build() {
                    window.paint_path(path, color);
                }
            };

        let fill_circle = |center: MapPoint, radius_units: f64, color: gpui::Hsla, window: &mut Window| {
            let radius = px(radius_units as f32 * unit);
            let center = map(center);
            window.paint_quad(
                gpui::fill(
                    gpui::Bounds::centered_at(center, size(radius * 2.0, radius * 2.0)),
                    color,
                )
                .corner_radii(radius),
            );
        };

        let ring = |center: MapPoint,
                    radius_units: f64,
                    stroke_units: f64,
                    color: gpui::Hsla,
                    window: &mut Window| {
            let radius = px(radius_units as f32 * unit);
            let center = map(center);
            let bounds = gpui::Bounds::centered_at(center, size(radius * 2.0, radius * 2.0));
            window.paint_quad(
                gpui::fill(bounds, gpui::transparent_black())
                    .corner_radii(radius)
                    .border_widths(px(stroke_units as f32 * unit))
                    .border_color(color),
            );
        };

        let text = |content: &str,
                    at: MapPoint,
                    size_units: f64,
                    color: gpui::Hsla,
                    centered: bool,
                    window: &mut Window,
                    cx: &mut gpui::App| {
            let font_size = px(size_units as f32 * unit);
            let run = TextRun {
                len: content.len(),
                font: gpui::font(mono_font.clone()),
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let line = window
                .text_system()
                .shape_line(SharedString::from(content.to_string()), font_size, &[run], None);
            let mut origin = map(at);
            // SVG text anchors at the baseline; shift up roughly one em.
            origin.y -= font_size;
            if centered {
                origin.x -= line.width / 2.0;
            }
            let _ = line.paint(origin, font_size * 1.2, gpui::TextAlign::Left, None, window, cx);
        };

        match self.mode {
            TrackMapRenderMode::Real => {
                // Three stacked strokes: dark casing, mid road (50% over the casing),
                // dashed hairline.
                stroke_polyline(&self.centerline, 2.6, theme::MAP_CASING(), None, window);
                stroke_polyline(
                    &self.centerline,
                    2.6,
                    theme::blend(theme::MAP_ROAD(), theme::MAP_CASING(), 0.5),
                    None,
                    window,
                );
                stroke_polyline(
                    &self.centerline,
                    0.45,
                    theme::MAP_MID(),
                    Some([1.4, 1.4]),
                    window,
                );

                if let Some(line) = &self.start_finish {
                    let mut builder = PathBuilder::stroke(px(0.8 * unit));
                    builder.move_to(map(line.inner));
                    builder.line_to(map(line.outer));
                    if let Ok(path) = builder.build() {
                        window.paint_path(path, theme::TEXT());
                    }
                    text(
                        "S/F",
                        MapPoint {
                            x: line.start.x + 1.2,
                            y: line.start.y - 1.2 + 2.4,
                        },
                        2.4,
                        ui::muted(),
                        false,
                        window,
                        cx,
                    );
                }

                for (label, at) in &self.markers {
                    fill_circle(*at, 0.45, theme::MAP_HAIRLINE(), window);
                    text(
                        label,
                        MapPoint {
                            x: at.x + 1.0,
                            y: at.y - 1.0 + 2.1,
                        },
                        2.1,
                        theme::MAP_HAIRLINE(),
                        false,
                        window,
                        cx,
                    );
                }
            }
            TrackMapRenderMode::Schematic => {
                // The two hardcoded beziers from TrackMapSvg.tsx.
                let outer: [(f64, f64); 13] = [
                    (15.0, 62.0),
                    (22.0, 22.0),
                    (61.0, 12.0),
                    (82.0, 27.0),
                    (95.0, 37.0),
                    (81.0, 66.0),
                    (61.0, 65.0),
                    (42.0, 64.0),
                    (43.0, 86.0),
                    (23.0, 79.0),
                    (12.0, 75.0),
                    (9.0, 70.0),
                    (15.0, 62.0),
                ];
                let inner: [(f64, f64); 13] = [
                    (18.0, 62.0),
                    (25.0, 29.0),
                    (60.0, 20.0),
                    (78.0, 31.0),
                    (88.0, 39.0),
                    (77.0, 59.0),
                    (61.0, 58.0),
                    (43.0, 57.0),
                    (45.0, 76.0),
                    (27.0, 72.0),
                    (18.0, 70.0),
                    (14.0, 67.0),
                    (18.0, 62.0),
                ];
                let paint_bezier_loop =
                    |points: &[(f64, f64); 13], width_units: f32, color: gpui::Hsla, window: &mut Window| {
                        let to_px = |(x, y): (f64, f64)| map(MapPoint { x, y });
                        let mut builder = PathBuilder::stroke(px(width_units * unit));
                        builder.move_to(to_px(points[0]));
                        for chunk in points[1..].chunks(3) {
                            builder.cubic_bezier_to(to_px(chunk[2]), to_px(chunk[0]), to_px(chunk[1]));
                        }
                        if let Ok(path) = builder.build() {
                            window.paint_path(path, color);
                        }
                    };
                paint_bezier_loop(&outer, 1.6, gpui::Rgba { r: 0.831, g: 0.843, b: 0.8, a: 1.0 }.into(), window);
                paint_bezier_loop(&inner, 0.6, theme::MAP_MID(), window);
            }
            TrackMapRenderMode::Pending | TrackMapRenderMode::Error => {
                let frame_color = gpui::Rgba { r: 0.165, g: 0.2, b: 0.24, a: 1.0 }.into();
                // Placeholder frame + dashed crosshair.
                let rect: [MapPoint; 5] = [
                    MapPoint { x: 8.0, y: 12.0 },
                    MapPoint { x: 92.0, y: 12.0 },
                    MapPoint { x: 92.0, y: 88.0 },
                    MapPoint { x: 8.0, y: 88.0 },
                    MapPoint { x: 8.0, y: 12.0 },
                ];
                stroke_polyline(&rect, 0.6, frame_color, None, window);
                stroke_polyline(
                    &[MapPoint { x: 18.0, y: 50.0 }, MapPoint { x: 82.0, y: 50.0 }],
                    0.5,
                    frame_color,
                    Some([1.5, 1.5]),
                    window,
                );
                stroke_polyline(
                    &[MapPoint { x: 50.0, y: 22.0 }, MapPoint { x: 50.0, y: 78.0 }],
                    0.5,
                    frame_color,
                    Some([1.5, 1.5]),
                    window,
                );
                if let Some(placeholder) = &self.placeholder {
                    text(
                        &placeholder.label,
                        MapPoint { x: 50.0, y: 49.0 },
                        3.0,
                        theme::MAP_HAIRLINE(),
                        true,
                        window,
                        cx,
                    );
                    text(
                        &placeholder.detail,
                        MapPoint { x: 50.0, y: 54.0 },
                        2.3,
                        ui::faint(),
                        true,
                        window,
                        cx,
                    );
                }
            }
        }

        for dot in &self.dots {
            let mut color = theme::team_colour(&dot.color);
            color.a = dot.opacity as f32;
            if dot.is_leader && !dot.is_out {
                ring(dot.point, 3.05, 0.45, theme::LEADER_HALO(), window);
            }
            // White outline ring standing in for the SVG circle's stroke.
            let mut outline = theme::TEXT();
            outline.a = dot.opacity as f32;
            fill_circle(dot.point, dot.radius + 0.175, outline, window);
            fill_circle(dot.point, dot.radius, color, window);
            if dot.is_out {
                let arm = 1.1;
                for (dx1, dy1, dx2, dy2) in [(-arm, -arm, arm, arm), (arm, -arm, -arm, arm)] {
                    let mut builder = PathBuilder::stroke(px(0.25 * unit));
                    builder.move_to(map(MapPoint {
                        x: dot.point.x + dx1,
                        y: dot.point.y + dy1,
                    }));
                    builder.line_to(map(MapPoint {
                        x: dot.point.x + dx2,
                        y: dot.point.y + dy2,
                    }));
                    if let Ok(path) = builder.build() {
                        let mut color = theme::TEXT();
                        color.a = 0.55;
                        window.paint_path(path, color);
                    }
                }
            }
            if dot.show_code {
                let mut color = gpui::Rgba { r: 0.906, g: 0.925, b: 0.941, a: 1.0 }.into();
                let _ = &mut color;
                let mut label_color: gpui::Hsla = color;
                label_color.a = dot.opacity as f32;
                text(
                    &dot.code,
                    MapPoint {
                        x: dot.point.x + 2.25,
                        y: dot.point.y + 0.9 + 2.35,
                    },
                    2.35,
                    label_color,
                    false,
                    window,
                    cx,
                );
            }
        }
    }
}
