use crate::{
    domain::{DerivedMetric, DerivedMetricKind, DriverSnapshot, MetricTrend},
    normalization::LapRecord,
};

pub fn recent_pace_metric(driver: &DriverSnapshot) -> Option<DerivedMetric> {
    let last_lap = driver.last_lap?;
    let trend = if last_lap < 91.0 {
        MetricTrend::Improving
    } else if last_lap > 93.0 {
        MetricTrend::Degrading
    } else {
        MetricTrend::Stable
    };

    Some(DerivedMetric {
        driver_number: Some(driver.driver.driver_number),
        kind: DerivedMetricKind::RecentPace,
        label: "last lap".to_string(),
        value: format!("{last_lap:.3}"),
        trend,
    })
}

pub fn recent_pace_metrics(
    rows: &[DriverSnapshot],
    laps: &[LapRecord],
    t: f64,
) -> Vec<DerivedMetric> {
    rows.iter()
        .filter_map(|row| recent_pace_from_laps(row, laps, t))
        .collect()
}

fn recent_pace_from_laps(
    row: &DriverSnapshot,
    laps: &[LapRecord],
    t: f64,
) -> Option<DerivedMetric> {
    let mut valid_laps = laps
        .iter()
        .filter(|lap| lap.lap.driver_number == row.driver.driver_number)
        .filter_map(|lap| {
            let duration = lap.lap.lap_duration?;
            let finished_at = lap.t_start + duration;
            (duration.is_finite() && duration > 0.0 && finished_at <= t)
                .then_some((lap.lap.lap_number, duration))
        })
        .collect::<Vec<_>>();

    valid_laps.sort_by(|a, b| b.0.cmp(&a.0));
    let recent = valid_laps
        .into_iter()
        .take(3)
        .map(|(_, duration)| duration)
        .collect::<Vec<_>>();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Driver, DriverStatus, Lap, RankSource, TyreCompound};

    #[test]
    fn recent_pace_metrics_use_last_three_completed_laps() {
        let row = driver_row(1);
        let laps = vec![
            lap(1, 1, 0.0, 91.0),
            lap(1, 2, 100.0, 90.0),
            lap(1, 3, 200.0, 89.0),
            lap(1, 4, 300.0, 88.0),
            lap(4, 1, 0.0, 99.0),
        ];

        let metrics = recent_pace_metrics(&[row], &laps, 390.0);

        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].driver_number, Some(1));
        assert_eq!(metrics[0].label, "3-lap avg");
        assert_eq!(metrics[0].value, "89.000");
        assert_eq!(metrics[0].trend, MetricTrend::Improving);
    }

    #[test]
    fn recent_pace_metrics_wait_for_three_completed_laps() {
        let row = driver_row(1);
        let laps = vec![
            lap(1, 1, 0.0, 91.0),
            lap(1, 2, 100.0, 90.0),
            lap(1, 3, 200.0, 89.0),
        ];

        assert!(recent_pace_metrics(&[row], &laps, 250.0).is_empty());
    }

    fn driver_row(driver_number: i32) -> DriverSnapshot {
        DriverSnapshot {
            driver: Driver {
                driver_number,
                code: "VER".to_string(),
                full_name: "Max Verstappen".to_string(),
                team_name: "Red Bull Racing".to_string(),
                team_colour: "3671C6".to_string(),
            },
            position: 1,
            rank_source: RankSource::OpenF1Position,
            gap_to_leader: None,
            interval: None,
            lap: 1,
            last_lap: None,
            compound: TyreCompound::Unknown,
            stint_age: None,
            sectors: vec![],
            in_pit: false,
            status: DriverStatus::OnTrack,
        }
    }

    fn lap(driver_number: i32, lap_number: i32, t_start: f64, duration: f64) -> LapRecord {
        LapRecord {
            t_start,
            lap: Lap {
                driver_number,
                lap_number,
                lap_duration: Some(duration),
                sector_1: None,
                sector_2: None,
                sector_3: None,
                is_pit_out_lap: false,
            },
        }
    }
}
