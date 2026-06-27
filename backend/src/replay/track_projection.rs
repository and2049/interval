use crate::{
    domain::{TrackGeometry, TrackPositionQuality, TrackPositionSample, TrackPositionSource},
    normalization::LocationRecord,
};

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
                relative_distance: match (a.relative_distance, b.relative_distance) {
                    (Some(from), Some(to)) => Some(interpolate_relative_distance(from, to, ratio)),
                    (Some(value), None) | (None, Some(value)) => Some(value),
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
    let relative_distance = location.relative_distance.or_else(|| {
        super::track_geometry_math::project_relative_distance(geometry, location.x, location.y)
    });
    let display_point = relative_distance.and_then(|distance| {
        super::track_geometry_math::point_at_relative_distance(geometry, distance)
    });
    TrackPositionSample {
        driver_number: location.driver_number,
        x: display_point.as_ref().map_or(location.x, |point| point.x),
        y: display_point.as_ref().map_or(location.y, |point| point.y),
        z: display_point
            .as_ref()
            .and_then(|point| point.z)
            .or(location.z),
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

pub fn stale_position(
    geometry: &TrackGeometry,
    location: LocationRecord,
    stale_seconds: f64,
) -> TrackPositionSample {
    let mut position = position_from_location(geometry, location, false);
    position.quality = TrackPositionQuality::Stale;
    position.stale_seconds = Some(stale_seconds.max(0.0));
    position
}

pub fn projected_position(
    geometry: &TrackGeometry,
    driver_number: i32,
    relative_distance: f64,
) -> Option<TrackPositionSample> {
    let point =
        super::track_geometry_math::point_at_relative_distance(geometry, relative_distance)?;
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

fn interpolate_relative_distance(from: f64, to: f64, ratio: f64) -> f64 {
    let normalized_from = from.rem_euclid(1.0);
    let mut normalized_to = to.rem_euclid(1.0);
    if normalized_to < normalized_from && normalized_from - normalized_to > 0.5 {
        normalized_to += 1.0;
    }
    (normalized_from + (normalized_to - normalized_from) * ratio).rem_euclid(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolates_between_driver_samples() {
        let samples = vec![
            LocationRecord {
                t: 0.0,
                driver_number: 4,
                x: 0.0,
                y: 0.0,
                z: None,
                relative_distance: Some(0.0),
            },
            LocationRecord {
                t: 10.0,
                driver_number: 4,
                x: 20.0,
                y: 10.0,
                z: None,
                relative_distance: Some(0.5),
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
                relative_distance: None,
            })
            .collect::<Vec<_>>();
        let geometry = super::super::track_geometry_builder::build_track_geometry(
            42,
            &samples,
            crate::domain::TrackGeometrySource::OpenF1Location,
        );
        let relative =
            crate::replay::track_geometry_math::project_relative_distance(&geometry, 95.0, 2.0)
                .unwrap();
        assert!(relative > 0.45 && relative < 0.55);
    }

    #[test]
    fn projected_position_uses_centerline_coordinates() {
        let geometry = super::super::track_geometry_builder::build_track_geometry(
            crate::replay::BAHRAIN_SESSION_KEY,
            &[],
            crate::domain::TrackGeometrySource::OpenF1Location,
        );
        let position = projected_position(&geometry, 1, 0.25).unwrap();
        assert_eq!(position.source, TrackPositionSource::Projected);
        assert_eq!(position.quality, TrackPositionQuality::Projected);
        assert_eq!(position.relative_distance, Some(0.25));
        assert!(position.x >= geometry.bounds.min_x && position.x <= geometry.bounds.max_x);
        assert!(position.y >= geometry.bounds.min_y && position.y <= geometry.bounds.max_y);
    }
}
