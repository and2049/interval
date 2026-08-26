use crate::{
    domain::{
        DerivedMetric, DerivedMetricKind, DriverSnapshot, DriverStatus, MetricTrend,
        RaceControlMessage, RankSource, Sector, SectorStatus, Stint, TrackGeometry,
        TrackPositionSample, TyreCompound, WeatherSample,
    },
    normalization::{
        IntervalRecord, LapRecord, LocationRecord, PitEvent, PositionRecord, RaceData,
        SessionResult,
    },
};
use std::collections::HashMap;

const PIT_WINDOW_SECONDS: f64 = 45.0;
const LOCATION_INTERPOLATION_MAX_GAP_SECONDS: f64 = 2.0;
const SNAPSHOT_RACE_CONTROL_LIMIT: usize = 12;

pub(crate) struct ReplayDataIndex<'a> {
    data: &'a RaceData,
    laps: HashMap<i32, Vec<&'a LapRecord>>,
    intervals: HashMap<i32, Vec<&'a IntervalRecord>>,
    positions: HashMap<i32, Vec<&'a PositionRecord>>,
    locations: HashMap<i32, Vec<&'a LocationRecord>>,
    pits: HashMap<i32, Vec<&'a PitEvent>>,
    results: HashMap<i32, &'a SessionResult>,
    weather: Vec<&'a WeatherSample>,
    race_control: Vec<&'a RaceControlMessage>,
}

impl<'a> ReplayDataIndex<'a> {
    pub(crate) fn new(data: &'a RaceData) -> Self {
        let mut index = Self {
            data,
            laps: HashMap::new(),
            intervals: HashMap::new(),
            positions: HashMap::new(),
            locations: HashMap::new(),
            pits: HashMap::new(),
            results: data
                .session_results
                .iter()
                .map(|result| (result.driver_number, result))
                .collect(),
            weather: data.weather.iter().collect(),
            race_control: data.race_control.iter().collect(),
        };

        for lap in &data.laps {
            index
                .laps
                .entry(lap.lap.driver_number)
                .or_default()
                .push(lap);
        }
        for interval in &data.intervals {
            index
                .intervals
                .entry(interval.driver_number)
                .or_default()
                .push(interval);
        }
        for position in &data.positions {
            index
                .positions
                .entry(position.sample.driver_number)
                .or_default()
                .push(position);
        }
        for location in &data.locations {
            index
                .locations
                .entry(location.driver_number)
                .or_default()
                .push(location);
        }
        for pit in &data.pits {
            index.pits.entry(pit.driver_number).or_default().push(pit);
        }

        for rows in index.laps.values_mut() {
            rows.sort_by(|a, b| a.t_start.total_cmp(&b.t_start));
        }
        for rows in index.intervals.values_mut() {
            rows.sort_by(|a, b| a.t.total_cmp(&b.t));
        }
        for rows in index.positions.values_mut() {
            rows.sort_by(|a, b| a.t.total_cmp(&b.t));
        }
        for rows in index.locations.values_mut() {
            rows.sort_by(|a, b| a.t.total_cmp(&b.t));
        }
        for rows in index.pits.values_mut() {
            rows.sort_by(|a, b| a.t.total_cmp(&b.t));
        }
        index.weather.sort_by(|a, b| a.t.total_cmp(&b.t));
        index.race_control.sort_by(|a, b| a.t.total_cmp(&b.t));

        index
    }

    pub(crate) fn latest_lap_number(&self, t: f64) -> i32 {
        self.data
            .drivers
            .iter()
            .filter_map(|driver| self.latest_lap(driver.driver_number, t))
            .map(|lap| lap.lap.lap_number)
            .max()
            .unwrap_or(0)
    }

    pub(crate) fn timing_rows(&self, t: f64) -> Vec<DriverSnapshot> {
        let (bests_by_driver, overall_bests) = best_times(&self.data.laps, t);
        let mut rows = self
            .data
            .drivers
            .iter()
            .map(|driver| {
                let lap_record = self.latest_lap(driver.driver_number, t);
                let interval = self.latest_interval(driver.driver_number, t);
                let rank_record = self.latest_rank_record(driver.driver_number, t);
                let result = self.results.get(&driver.driver_number).copied();
                let lap_number = lap_record.map_or(0, |lap| lap.lap.lap_number);
                let stint = stint_for(&self.data.stints, driver.driver_number, lap_number);
                let in_pit = self.in_pit_window(driver.driver_number, t);
                let latest_location = self.latest_location_sample(driver.driver_number, t);
                let driver_bests = bests_by_driver.get(&driver.driver_number);
                let last_lap = lap_record.and_then(|lap| lap.lap.lap_duration);

                DriverSnapshot {
                    driver: driver.clone(),
                    position: rank_record
                        .map(|record| record.position)
                        .or_else(|| result.and_then(|result| result.position))
                        .unwrap_or(i32::MAX),
                    rank_source: rank_source(rank_record, result),
                    gap_to_leader: interval.and_then(|row| row.gap_to_leader.clone()),
                    interval: interval.and_then(|row| row.interval.clone()),
                    lap: lap_number,
                    last_lap,
                    last_lap_status: pace_status(
                        last_lap,
                        driver_bests.and_then(|bests| bests.lap),
                        overall_bests.lap,
                    ),
                    best_lap: driver_bests.and_then(|bests| bests.lap),
                    best_lap_status: pace_status(
                        driver_bests.and_then(|bests| bests.lap),
                        driver_bests.and_then(|bests| bests.lap),
                        overall_bests.lap,
                    ),
                    compound: stint.map_or(TyreCompound::Unknown, |stint| stint.compound.clone()),
                    stint_age: stint.map(|stint| {
                        lap_number - stint.lap_start + stint.tyre_age_at_start.unwrap_or(0)
                    }),
                    sectors: sectors_for(lap_record, driver_bests, &overall_bests),
                    in_pit,
                    status: driver_status(in_pit, result, latest_location, t),
                }
            })
            .collect::<Vec<_>>();

        rows.sort_by(|a, b| {
            a.position
                .cmp(&b.position)
                .then_with(|| a.gap_to_leader.cmp(&b.gap_to_leader))
                .then_with(|| a.driver.code.cmp(&b.driver.code))
        });

        for (idx, row) in rows.iter_mut().enumerate() {
            if row.position == i32::MAX {
                row.position = (idx + 1) as i32;
                row.rank_source = RankSource::FallbackGrid;
            }
        }

        rows
    }

    pub(crate) fn track_positions(
        &self,
        geometry: &TrackGeometry,
        t: f64,
    ) -> Vec<TrackPositionSample> {
        self.data
            .drivers
            .iter()
            .map(|driver| {
                let rank = self
                    .latest_rank_record(driver.driver_number, t)
                    .map_or(driver.driver_number, |record| record.position);

                if let Some(track_location) =
                    self.interpolate_driver_location(driver.driver_number, t)
                {
                    return match track_location {
                        TrackLocation::Fresh {
                            location,
                            interpolated,
                        } => super::track_projection::position_from_location(
                            geometry,
                            location,
                            interpolated,
                        ),
                        TrackLocation::Stale {
                            location,
                            stale_seconds,
                        } => super::track_projection::stale_position(
                            geometry,
                            location,
                            stale_seconds,
                        ),
                    };
                }

                if geometry.quality == crate::domain::TrackGeometryQuality::Ready {
                    let relative_distance = projected_relative_distance(
                        self.latest_lap(driver.driver_number, t),
                        rank,
                        t,
                    );
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

    pub(crate) fn latest_weather(&self, t: f64) -> Option<WeatherSample> {
        latest_by_time(&self.weather, t, |sample| sample.t).cloned()
    }

    pub(crate) fn track_status(&self, t: f64) -> String {
        self.race_control
            .iter()
            .copied()
            .take_while(|event| event.t <= t)
            .filter(|event| event.flag.is_some())
            .max_by(|a, b| a.t.total_cmp(&b.t))
            .and_then(|event| event.flag.clone())
            .unwrap_or_else(|| "green".to_string())
    }

    pub(crate) fn race_control_history(&self, t: f64) -> Vec<RaceControlMessage> {
        let history = self
            .race_control
            .iter()
            .copied()
            .take_while(|event| event.t <= t)
            .collect::<Vec<_>>();
        let start = history.len().saturating_sub(SNAPSHOT_RACE_CONTROL_LIMIT);
        history[start..]
            .iter()
            .map(|event| (*event).clone())
            .collect()
    }

    pub(crate) fn recent_pace_metrics(
        &self,
        rows: &[DriverSnapshot],
        t: f64,
    ) -> Vec<DerivedMetric> {
        rows.iter()
            .filter_map(|row| self.recent_pace_from_laps(row, t))
            .collect()
    }

    fn latest_lap(&self, driver_number: i32, t: f64) -> Option<&'a LapRecord> {
        latest_by_time(self.laps.get(&driver_number)?, t, |lap| lap.t_start)
    }

    fn latest_interval(&self, driver_number: i32, t: f64) -> Option<&'a IntervalRecord> {
        latest_by_time(self.intervals.get(&driver_number)?, t, |interval| {
            interval.t
        })
    }

    fn latest_rank_record(&self, driver_number: i32, t: f64) -> Option<&'a PositionRecord> {
        latest_by_time(self.positions.get(&driver_number)?, t, |position| {
            position.t
        })
    }

    fn in_pit_window(&self, driver_number: i32, t: f64) -> bool {
        self.pits
            .get(&driver_number)
            .into_iter()
            .flatten()
            .take_while(|pit| pit.t <= t)
            .any(|pit| t <= pit.t + pit.pit_duration.unwrap_or(PIT_WINDOW_SECONDS).max(10.0))
    }

    fn interpolate_driver_location(&self, driver_number: i32, t: f64) -> Option<TrackLocation> {
        let rows = self.locations.get(&driver_number)?;
        if rows.is_empty() {
            return None;
        }

        let insertion = rows.partition_point(|sample| sample.t < t);
        let before = insertion
            .checked_sub(1)
            .and_then(|idx| rows.get(idx))
            .copied();
        let after = rows.get(insertion).copied();

        match (before, after) {
            (Some(a), Some(b))
                if (b.t - a.t).abs() > f64::EPSILON
                    && b.t - a.t <= LOCATION_INTERPOLATION_MAX_GAP_SECONDS =>
            {
                let ratio = ((t - a.t) / (b.t - a.t)).clamp(0.0, 1.0);
                Some(TrackLocation::Fresh {
                    location: LocationRecord {
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
                            (Some(from), Some(to)) => {
                                Some(interpolate_relative_distance(from, to, ratio))
                            }
                            (Some(value), None) | (None, Some(value)) => Some(value),
                            (None, None) => None,
                        },
                    },
                    interpolated: true,
                })
            }
            (Some(sample), Some(_)) if t - sample.t > LOCATION_INTERPOLATION_MAX_GAP_SECONDS => {
                Some(TrackLocation::Stale {
                    location: sample.clone(),
                    stale_seconds: t - sample.t,
                })
            }
            (Some(sample), _) if t - sample.t > LOCATION_INTERPOLATION_MAX_GAP_SECONDS => {
                Some(TrackLocation::Stale {
                    location: sample.clone(),
                    stale_seconds: t - sample.t,
                })
            }
            (Some(sample), _) | (_, Some(sample)) => Some(TrackLocation::Fresh {
                location: sample.clone(),
                interpolated: false,
            }),
            (None, None) => None,
        }
    }

    fn latest_location_sample(&self, driver_number: i32, t: f64) -> Option<&'a LocationRecord> {
        latest_by_time(self.locations.get(&driver_number)?, t, |location| {
            location.t
        })
    }

    fn recent_pace_from_laps(&self, row: &DriverSnapshot, t: f64) -> Option<DerivedMetric> {
        let laps = self.laps.get(&row.driver.driver_number)?;
        let mut recent = Vec::with_capacity(3);

        for lap in laps.iter().rev() {
            let duration = lap.lap.lap_duration?;
            let finished_at = lap.t_start + duration;
            if duration.is_finite() && duration > 0.0 && finished_at <= t {
                recent.push(duration);
                if recent.len() == 3 {
                    break;
                }
            }
        }

        if recent.len() < 3 {
            return None;
        }

        let average = recent.iter().sum::<f64>() / recent.len() as f64;
        let last = recent[0];
        let trend = if last < average - 0.25 {
            MetricTrend::Improving
        } else if last > average + 0.25 {
            MetricTrend::Degrading
        } else {
            MetricTrend::Stable
        };

        Some(DerivedMetric {
            driver_number: Some(row.driver.driver_number),
            kind: DerivedMetricKind::RecentPace,
            label: "3-lap avg".to_string(),
            value: format!("{average:.3}"),
            trend,
        })
    }
}

fn latest_by_time<'a, T>(rows: &[&'a T], t: f64, time: impl Fn(&T) -> f64) -> Option<&'a T> {
    let insertion = rows.partition_point(|row| time(row) <= t);
    insertion
        .checked_sub(1)
        .and_then(|idx| rows.get(idx))
        .copied()
}

fn rank_source(rank_record: Option<&PositionRecord>, result: Option<&SessionResult>) -> RankSource {
    if let Some(rank_record) = rank_record {
        rank_record.rank_source.clone()
    } else if result.and_then(|result| result.position).is_some() {
        RankSource::SessionResult
    } else {
        RankSource::FallbackGrid
    }
}

fn stint_for(stints: &[Stint], driver_number: i32, lap_number: i32) -> Option<&Stint> {
    stints.iter().find(|stint| {
        stint.driver_number == driver_number
            && stint.lap_start <= lap_number
            && stint.lap_end.unwrap_or(i32::MAX) >= lap_number
    })
}

/// Best sector and lap times seen so far — one per driver, plus the session's overall.
#[derive(Default, Clone, Copy)]
struct BestTimes {
    sectors: [Option<f64>; 3],
    lap: Option<f64>,
}

fn keep_min(current: &mut Option<f64>, candidate: Option<f64>) {
    if let Some(candidate) = candidate {
        if current.is_none_or(|best| candidate < best) {
            *current = Some(candidate);
        }
    }
}

/// Folds every lap visible at `t` (same `t_start <= t` visibility rule the displayed
/// rows use) into per-driver and overall bests.
fn best_times(laps: &[LapRecord], t: f64) -> (HashMap<i32, BestTimes>, BestTimes) {
    let mut per_driver = HashMap::<i32, BestTimes>::new();
    let mut overall = BestTimes::default();
    for record in laps.iter().filter(|lap| lap.t_start <= t) {
        let entry = per_driver.entry(record.lap.driver_number).or_default();
        let sectors = [
            record.lap.sector_1,
            record.lap.sector_2,
            record.lap.sector_3,
        ];
        for (index, sector) in sectors.into_iter().enumerate() {
            keep_min(&mut entry.sectors[index], sector);
            keep_min(&mut overall.sectors[index], sector);
        }
        keep_min(&mut entry.lap, record.lap.lap_duration);
        keep_min(&mut overall.lap, record.lap.lap_duration);
    }
    (per_driver, overall)
}

/// F1 pace classification: purple while it stands as the overall best, green while it
/// is the driver's own best, plain otherwise. `duration` itself is part of the folds,
/// so equality (within float noise) is what "stands as the best" means.
fn pace_status(
    duration: Option<f64>,
    personal_best: Option<f64>,
    overall_best: Option<f64>,
) -> SectorStatus {
    const EPSILON: f64 = 1e-6;
    let Some(duration) = duration else {
        return SectorStatus::Unknown;
    };
    if overall_best.is_some_and(|best| duration <= best + EPSILON) {
        SectorStatus::OverallBest
    } else if personal_best.is_some_and(|best| duration <= best + EPSILON) {
        SectorStatus::PersonalBest
    } else {
        SectorStatus::Normal
    }
}

fn sectors_for(
    lap: Option<&LapRecord>,
    driver_bests: Option<&BestTimes>,
    overall: &BestTimes,
) -> Vec<Sector> {
    let lap = lap.map(|lap| &lap.lap);
    [
        (1, lap.and_then(|lap| lap.sector_1)),
        (2, lap.and_then(|lap| lap.sector_2)),
        (3, lap.and_then(|lap| lap.sector_3)),
    ]
    .into_iter()
    .map(|(index, duration)| Sector {
        index,
        duration,
        status: pace_status(
            duration,
            driver_bests.and_then(|bests| bests.sectors[index as usize - 1]),
            overall.sectors[index as usize - 1],
        ),
    })
    .collect()
}

fn driver_status(
    in_pit: bool,
    result: Option<&SessionResult>,
    latest_location: Option<&LocationRecord>,
    t: f64,
) -> DriverStatus {
    if in_pit {
        DriverStatus::Pit
    } else if result.is_some_and(|result| result.dns || result.dsq) {
        DriverStatus::Out
    } else if result.is_some_and(|result| result.dnf)
        && latest_location
            .is_none_or(|location| t - location.t > LOCATION_INTERPOLATION_MAX_GAP_SECONDS)
    {
        DriverStatus::Out
    } else {
        DriverStatus::OnTrack
    }
}

enum TrackLocation {
    Fresh {
        location: LocationRecord,
        interpolated: bool,
    },
    Stale {
        location: LocationRecord,
        stale_seconds: f64,
    },
}

fn interpolate_relative_distance(from: f64, to: f64, ratio: f64) -> f64 {
    let normalized_from = from.rem_euclid(1.0);
    let mut normalized_to = to.rem_euclid(1.0);
    if normalized_to < normalized_from && normalized_from - normalized_to > 0.5 {
        normalized_to += 1.0;
    }
    (normalized_from + (normalized_to - normalized_from) * ratio).rem_euclid(1.0)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_status_uses_latest_flag_by_timestamp() {
        let data = race_data_with_control(vec![
            race_control(120.0, Some("green"), "green flag"),
            race_control(60.0, Some("yellow"), "yellow flag"),
            race_control(90.0, None, "message only"),
        ]);
        let index = ReplayDataIndex::new(&data);

        assert_eq!(index.track_status(100.0), "yellow");
        assert_eq!(index.track_status(130.0), "green");
    }

    #[test]
    fn race_control_history_is_ordered_by_event_time() {
        let data = race_data_with_control(vec![
            race_control(120.0, Some("green"), "green flag"),
            race_control(60.0, Some("yellow"), "yellow flag"),
            race_control(90.0, None, "message only"),
        ]);
        let index = ReplayDataIndex::new(&data);

        let history = index.race_control_history(120.0);

        assert_eq!(
            history
                .iter()
                .map(|event| event.message.as_str())
                .collect::<Vec<_>>(),
            vec!["yellow flag", "message only", "green flag"]
        );
    }

    #[test]
    fn race_control_history_keeps_recent_snapshot_window() {
        let data = race_data_with_control(
            (0..20)
                .map(|idx| race_control(idx as f64, None, &format!("message {idx}")))
                .collect(),
        );
        let index = ReplayDataIndex::new(&data);

        let history = index.race_control_history(20.0);

        assert_eq!(history.len(), SNAPSHOT_RACE_CONTROL_LIMIT);
        assert_eq!(history[0].message, "message 8");
        assert_eq!(
            history.last().map(|event| event.message.as_str()),
            Some("message 19")
        );
    }

    #[test]
    fn pace_statuses_classify_overall_and_personal_bests_at_time_t() {
        let lap = |driver: i32, number: i32, t_start: f64, s1: f64, duration: f64| LapRecord {
            t_start,
            lap: crate::domain::Lap {
                driver_number: driver,
                lap_number: number,
                lap_duration: Some(duration),
                sector_1: Some(s1),
                sector_2: None,
                sector_3: None,
                is_pit_out_lap: false,
            },
        };
        // Driver 1 improves then slows; driver 2 holds the overall best throughout.
        let laps = vec![
            lap(1, 1, 0.0, 26.0, 92.0),
            lap(2, 1, 0.0, 25.0, 90.0),
            lap(1, 2, 100.0, 25.5, 91.0),
            lap(1, 3, 200.0, 27.0, 93.0),
        ];

        let (by_driver, overall) = best_times(&laps, 150.0);
        assert_eq!(overall.lap, Some(90.0));
        // At t=150 driver 1's lap 2 (91.0) is their personal best but not overall.
        let d1 = by_driver.get(&1).unwrap();
        assert_eq!(
            pace_status(Some(91.0), d1.lap, overall.lap),
            SectorStatus::PersonalBest
        );
        assert_eq!(
            pace_status(Some(90.0), by_driver.get(&2).unwrap().lap, overall.lap),
            SectorStatus::OverallBest
        );

        // At t=250 driver 1's latest lap (93.0) beats nothing.
        let (by_driver, overall) = best_times(&laps, 250.0);
        assert_eq!(
            pace_status(Some(93.0), by_driver.get(&1).unwrap().lap, overall.lap),
            SectorStatus::Normal
        );
        // Missing time stays unknown.
        assert_eq!(
            pace_status(None, by_driver.get(&1).unwrap().lap, overall.lap),
            SectorStatus::Unknown
        );
    }

    #[test]
    fn track_positions_use_relative_distance_on_centerline() {
        let data = race_data_with_locations(
            vec![
                location(1.0, 4, 999.0, 999.0, Some(0.20)),
                location(1.5, 4, 999.0, 999.0, Some(0.30)),
            ],
            vec![],
        );
        let index = ReplayDataIndex::new(&data);

        let positions = index.track_positions(&test_geometry(), 1.25);

        assert_eq!(
            positions[0].quality,
            crate::domain::TrackPositionQuality::Interpolated
        );
        assert_eq!(
            positions[0].source,
            crate::domain::TrackPositionSource::Interpolated
        );
        assert!(positions[0].x > 20.0 && positions[0].x < 30.0);
        assert_eq!(positions[0].y, 0.0);
        assert!(positions[0].relative_distance.unwrap() > 0.20);
    }

    #[test]
    fn stale_dnf_driver_freezes_after_final_location() {
        let data = race_data_with_locations(
            vec![location(1.0, 4, 10.0, 0.0, Some(0.10))],
            vec![SessionResult {
                driver_number: 4,
                position: Some(20),
                dnf: true,
                dns: false,
                dsq: false,
            }],
        );
        let index = ReplayDataIndex::new(&data);

        let fresh_rows = index.timing_rows(2.0);
        let stale_rows = index.timing_rows(4.0);
        let positions = index.track_positions(&test_geometry(), 4.0);

        assert_eq!(fresh_rows[0].status, DriverStatus::OnTrack);
        assert_eq!(stale_rows[0].status, DriverStatus::Out);
        assert_eq!(
            positions[0].quality,
            crate::domain::TrackPositionQuality::Stale
        );
        assert_eq!(positions[0].stale_seconds, Some(3.0));
    }

    fn race_data_with_control(race_control: Vec<RaceControlMessage>) -> RaceData {
        RaceData {
            source: crate::normalization::RaceDataSource::OpenF1Historical,
            drivers: vec![],
            laps: vec![],
            intervals: vec![],
            positions: vec![],
            locations: vec![],
            geometry_locations: vec![],
            pits: vec![],
            race_control,
            stints: vec![],
            weather: vec![],
            session_results: vec![],
        }
    }

    fn race_data_with_locations(
        locations: Vec<LocationRecord>,
        session_results: Vec<SessionResult>,
    ) -> RaceData {
        RaceData {
            source: crate::normalization::RaceDataSource::FastF1Historical,
            drivers: vec![crate::domain::Driver {
                driver_number: 4,
                code: "NOR".to_string(),
                full_name: "Lando Norris".to_string(),
                team_name: "McLaren".to_string(),
                team_colour: "FF8000".to_string(),
            }],
            laps: vec![],
            intervals: vec![],
            positions: vec![],
            locations,
            geometry_locations: vec![],
            pits: vec![],
            race_control: vec![],
            stints: vec![],
            weather: vec![],
            session_results,
        }
    }

    fn location(
        t: f64,
        driver_number: i32,
        x: f64,
        y: f64,
        relative_distance: Option<f64>,
    ) -> LocationRecord {
        LocationRecord {
            t,
            driver_number,
            x,
            y,
            z: None,
            relative_distance,
        }
    }

    fn test_geometry() -> TrackGeometry {
        TrackGeometry {
            contract_version: crate::domain::REPLAY_CONTRACT_VERSION.to_string(),
            session_key: 1,
            bounds: crate::domain::TrackBounds {
                min_x: 0.0,
                max_x: 100.0,
                min_y: 0.0,
                max_y: 10.0,
            },
            centerline: vec![
                crate::domain::TrackPoint {
                    x: 0.0,
                    y: 0.0,
                    z: None,
                    cumulative_distance: 0.0,
                    relative_distance: 0.0,
                },
                crate::domain::TrackPoint {
                    x: 100.0,
                    y: 0.0,
                    z: None,
                    cumulative_distance: 100.0,
                    relative_distance: 1.0,
                },
            ],
            inner_edge: vec![],
            outer_edge: vec![],
            source: crate::domain::TrackGeometrySource::FastF1Telemetry,
            quality: crate::domain::TrackGeometryQuality::Ready,
            map_mode: crate::domain::MapMode::Gps,
            circuit_length: Some(100.0),
            generated_at: String::new(),
        }
    }

    fn race_control(t: f64, flag: Option<&str>, message: &str) -> RaceControlMessage {
        RaceControlMessage {
            t,
            category: "race_control".to_string(),
            message: message.to_string(),
            flag: flag.map(str::to_string),
            scope: None,
        }
    }
}
