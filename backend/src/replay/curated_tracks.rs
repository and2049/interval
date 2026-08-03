use crate::domain::{
    MapMode, TrackGeometry, TrackGeometryQuality, TrackGeometrySource, TrackPoint,
    REPLAY_CONTRACT_VERSION,
};
use chrono::Utc;
use serde::Deserialize;

pub const BAHRAIN_SESSION_KEY: i64 = 9472;

const TRACK_WIDTH: f64 = 15.0;
const DENSIFIED_POINT_COUNT: usize = 240;
const ASSETS_DIR: &str = "backend/assets/tracks";

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

#[derive(Deserialize)]
struct TrackAssetJson {
    centerline: Vec<(f64, f64)>,
    rotation_deg: Option<f64>,
}

pub fn curated_geometry(session_key: i64) -> Option<TrackGeometry> {
    if let Some(geometry) = load_asset_geometry(session_key) {
        return Some(geometry);
    }
    match session_key {
        BAHRAIN_SESSION_KEY => Some(build_geometry(session_key, BAHRAIN_CENTERLINE, 0.0)),
        _ => None,
    }
}

fn load_asset_geometry(session_key: i64) -> Option<TrackGeometry> {
    let path = format!("{ASSETS_DIR}/{session_key}.json");
    let content = std::fs::read_to_string(&path).ok()?;
    let asset: TrackAssetJson = serde_json::from_str(&content).ok()?;
    if asset.centerline.len() < 3 {
        return None;
    }
    let rotation = asset.rotation_deg.unwrap_or(0.0);
    tracing::info!(
        path = %path,
        points = asset.centerline.len(),
        "loaded curated track asset"
    );
    Some(build_geometry(session_key, &asset.centerline, rotation))
}

fn build_geometry(session_key: i64, centerline: &[(f64, f64)], rotation_deg: f64) -> TrackGeometry {
    let raw = centerline
        .iter()
        .map(|(x, y)| TrackPoint {
            x: *x,
            y: *y,
            z: None,
            cumulative_distance: 0.0,
            relative_distance: 0.0,
        })
        .collect::<Vec<_>>();
    let mut points = super::track_geometry_math::densify_points(&raw, DENSIFIED_POINT_COUNT);
    if rotation_deg != 0.0 {
        rotate_points(&mut points, rotation_deg);
    }
    super::track_geometry_math::apply_distances(&mut points);
    let (inner_edge, outer_edge) =
        super::track_geometry_math::display_edges_with_width(&points, TRACK_WIDTH);
    let bounds = super::track_geometry_math::bounds_for_all([
        points.as_slice(),
        inner_edge.as_slice(),
        outer_edge.as_slice(),
    ]);
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

fn rotate_points(points: &mut [TrackPoint], rotation_deg: f64) {
    if points.is_empty() {
        return;
    }
    let (mut cx, mut cy) = (0.0f64, 0.0f64);
    for point in points.iter() {
        cx += point.x;
        cy += point.y;
    }
    cx /= points.len() as f64;
    cy /= points.len() as f64;

    let rad = rotation_deg.to_radians();
    let cos = rad.cos();
    let sin = rad.sin();

    for point in points.iter_mut() {
        let dx = point.x - cx;
        let dy = point.y - cy;
        point.x = dx * cos - dy * sin + cx;
        point.y = dx * sin + dy * cos + cy;
    }
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
        assert!(geometry.centerline.len() >= DENSIFIED_POINT_COUNT);
        assert!(geometry.inner_edge.len() == geometry.centerline.len());
        assert!(geometry.outer_edge.len() == geometry.centerline.len());
        assert!(geometry.circuit_length.unwrap() > 1_000.0);
    }

    #[test]
    fn unknown_session_returns_none() {
        assert!(curated_geometry(99999).is_none());
    }

    #[test]
    fn rotate_points_preserves_count() {
        let mut pts = vec![
            TrackPoint {
                x: 1.0,
                y: 0.0,
                z: None,
                cumulative_distance: 0.0,
                relative_distance: 0.0,
            },
            TrackPoint {
                x: 2.0,
                y: 0.0,
                z: None,
                cumulative_distance: 0.0,
                relative_distance: 0.0,
            },
            TrackPoint {
                x: 3.0,
                y: 0.0,
                z: None,
                cumulative_distance: 0.0,
                relative_distance: 0.0,
            },
        ];
        rotate_points(&mut pts, 90.0);
        assert_eq!(pts.len(), 3);
    }
}
