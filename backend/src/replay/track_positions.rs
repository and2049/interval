use crate::{
    domain::{TrackGeometry, TrackGeometryQuality, TrackPositionSample},
    normalization::{LapRecord, RaceData},
};

pub(crate) fn latest_positions(
    data: &RaceData,
    geometry: &TrackGeometry,
    t: f64,
) -> Vec<TrackPositionSample> {
    let ranks = super::timing::latest_rank_records(&data.positions, t);
    let laps = super::timing::latest_laps(&data.laps, t);

    data.drivers
        .iter()
        .map(|driver| {
            let rank = ranks
                .get(&driver.driver_number)
                .map_or(driver.driver_number, |record| record.position);

            if let Some(location) = super::track_projection::interpolate_driver_location(
                &data.locations,
                driver.driver_number,
                t,
            ) {
                let exact = data.locations.iter().any(|sample| {
                    sample.driver_number == driver.driver_number && (sample.t - t).abs() < 0.001
                });
                return super::track_projection::position_from_location(geometry, location, !exact);
            }

            if geometry.quality == TrackGeometryQuality::Ready {
                let relative_distance =
                    projected_relative_distance(laps.get(&driver.driver_number).copied(), rank, t);
                if let Some(position) = super::track_projection::projected_position(
                    geometry,
                    driver.driver_number,
                    relative_distance,
                ) {
                    return position;
                }
            }

            super::track_projection::schematic_position(driver.driver_number, rank, t)
        })
        .collect()
}

fn projected_relative_distance(lap: Option<&LapRecord>, rank: i32, t: f64) -> f64 {
    let rank_offset = (rank.max(1) - 1) as f64 * 0.006;
    let progress = lap
        .and_then(|lap| {
            let duration = lap.lap.lap_duration?;
            if duration <= 0.0 {
                return None;
            }
            Some(((t - lap.t_start) / duration).clamp(0.0, 0.995))
        })
        .unwrap_or_else(|| (t / 95.0).rem_euclid(1.0));

    (progress - rank_offset).rem_euclid(1.0)
}
