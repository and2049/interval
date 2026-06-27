use crate::{
    domain::{DriverSnapshot, DriverStatus, RankSource, Sector, SectorStatus, Stint, TyreCompound},
    normalization::{IntervalRecord, LapRecord, PitEvent, PositionRecord, RaceData, SessionResult},
};
use std::collections::HashMap;

const PIT_WINDOW_SECONDS: f64 = 45.0;

pub(crate) fn timing_rows(data: &RaceData, t: f64) -> Vec<DriverSnapshot> {
    let lap_by_driver = latest_laps(&data.laps, t);
    let interval_by_driver = latest_intervals(&data.intervals, t);
    let rank_by_driver = latest_rank_records(&data.positions, t);
    let result_by_driver = data
        .session_results
        .iter()
        .map(|result| (result.driver_number, result))
        .collect::<HashMap<_, _>>();

    let mut rows = data
        .drivers
        .iter()
        .map(|driver| {
            let lap_record = lap_by_driver.get(&driver.driver_number);
            let interval = interval_by_driver.get(&driver.driver_number);
            let rank_record = rank_by_driver.get(&driver.driver_number);
            let result = result_by_driver.get(&driver.driver_number).copied();
            let lap_number = lap_record.map_or(1, |lap| lap.lap.lap_number);
            let stint = stint_for(&data.stints, driver.driver_number, lap_number);
            let in_pit = in_pit_window(&data.pits, driver.driver_number, t);

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
                last_lap: lap_record.and_then(|lap| lap.lap.lap_duration),
                compound: stint.map_or(TyreCompound::Unknown, |stint| stint.compound.clone()),
                stint_age: stint.map(|stint| {
                    lap_number - stint.lap_start + stint.tyre_age_at_start.unwrap_or(0)
                }),
                sectors: sectors_for(lap_record.copied()),
                in_pit,
                status: driver_status(in_pit, result),
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

pub(crate) fn latest_lap_number(data: &RaceData, t: f64) -> i32 {
    latest_laps(&data.laps, t)
        .values()
        .map(|lap| lap.lap.lap_number)
        .max()
        .unwrap_or(1)
}

pub(crate) fn latest_laps(laps: &[LapRecord], t: f64) -> HashMap<i32, &LapRecord> {
    let mut out = HashMap::new();
    for lap in laps.iter().filter(|lap| lap.t_start <= t) {
        let replace = out
            .get(&lap.lap.driver_number)
            .is_none_or(|existing: &&LapRecord| existing.t_start <= lap.t_start);
        if replace {
            out.insert(lap.lap.driver_number, lap);
        }
    }
    out
}

pub(crate) fn latest_rank_records(
    positions: &[PositionRecord],
    t: f64,
) -> HashMap<i32, &PositionRecord> {
    let mut out = HashMap::<i32, &PositionRecord>::new();
    for position in positions.iter().filter(|position| position.t <= t) {
        let replace = out
            .get(&position.sample.driver_number)
            .is_none_or(|existing| existing.t <= position.t);
        if replace {
            out.insert(position.sample.driver_number, position);
        }
    }
    out
}

fn latest_intervals(intervals: &[IntervalRecord], t: f64) -> HashMap<i32, &IntervalRecord> {
    let mut out = HashMap::new();
    for interval in intervals.iter().filter(|interval| interval.t <= t) {
        let replace = out
            .get(&interval.driver_number)
            .is_none_or(|existing: &&IntervalRecord| existing.t <= interval.t);
        if replace {
            out.insert(interval.driver_number, interval);
        }
    }
    out
}

fn rank_source(
    rank_record: Option<&&PositionRecord>,
    result: Option<&SessionResult>,
) -> RankSource {
    if rank_record.is_some() {
        RankSource::OpenF1Position
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

fn in_pit_window(pits: &[PitEvent], driver_number: i32, t: f64) -> bool {
    pits.iter().any(|pit| {
        pit.driver_number == driver_number
            && pit.t <= t
            && t <= pit.t + pit.pit_duration.unwrap_or(PIT_WINDOW_SECONDS).max(10.0)
    })
}

fn sectors_for(lap: Option<&LapRecord>) -> Vec<Sector> {
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
        status: if duration.is_some() {
            SectorStatus::Normal
        } else {
            SectorStatus::Unknown
        },
    })
    .collect()
}

fn driver_status(in_pit: bool, result: Option<&SessionResult>) -> DriverStatus {
    if in_pit {
        DriverStatus::Pit
    } else if result.is_some_and(|result| result.dnf || result.dns || result.dsq) {
        DriverStatus::Out
    } else {
        DriverStatus::OnTrack
    }
}
