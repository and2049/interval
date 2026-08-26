//! Port of `frontend/src/lib/derivedMetrics.ts`.

use interval_backend::domain::{DerivedMetric, DriverSnapshot, MetricTrend};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedMetricDisplay {
    pub driver: String,
    pub label: String,
    pub value: String,
    pub trend: MetricTrend,
}

pub fn derived_metric_rows(
    metrics: &[DerivedMetric],
    timing_rows: &[DriverSnapshot],
) -> Vec<DerivedMetricDisplay> {
    let driver_code_by_number: HashMap<i32, &str> = timing_rows
        .iter()
        .map(|row| (row.driver.driver_number, row.driver.code.as_str()))
        .collect();

    metrics
        .iter()
        .map(|metric| DerivedMetricDisplay {
            driver: match metric.driver_number {
                None => "--".to_string(),
                Some(number) => driver_code_by_number
                    .get(&number)
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| number.to_string()),
            },
            label: metric.label.clone(),
            value: metric.value.clone(),
            trend: metric.trend.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use interval_backend::domain::{
        DerivedMetricKind, Driver, DriverStatus, RankSource, TyreCompound,
    };

    fn metric(
        driver_number: Option<i32>,
        kind: DerivedMetricKind,
        label: &str,
        value: &str,
        trend: MetricTrend,
    ) -> DerivedMetric {
        DerivedMetric {
            driver_number,
            kind,
            label: label.to_string(),
            value: value.to_string(),
            trend,
        }
    }

    fn timing_row(driver_number: i32, code: &str) -> DriverSnapshot {
        DriverSnapshot {
            driver: Driver {
                driver_number,
                code: code.to_string(),
                full_name: code.to_string(),
                team_name: "Team".to_string(),
                team_colour: "FFFFFF".to_string(),
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

    #[test]
    fn decorates_driver_metrics_with_timing_tower_driver_codes() {
        assert_eq!(
            derived_metric_rows(
                &[metric(
                    Some(1),
                    DerivedMetricKind::RecentPace,
                    "3-lap avg",
                    "96.936",
                    MetricTrend::Stable,
                )],
                &[timing_row(1, "VER")],
            ),
            vec![DerivedMetricDisplay {
                driver: "VER".to_string(),
                label: "3-lap avg".to_string(),
                value: "96.936".to_string(),
                trend: MetricTrend::Stable,
            }]
        );
    }

    #[test]
    fn falls_back_when_a_metric_is_session_wide_or_the_driver_is_missing() {
        assert_eq!(
            derived_metric_rows(
                &[
                    metric(
                        None,
                        DerivedMetricKind::PitState,
                        "pit lane",
                        "closed",
                        MetricTrend::Unknown,
                    ),
                    metric(
                        Some(99),
                        DerivedMetricKind::RecentPace,
                        "3-lap avg",
                        "100.000",
                        MetricTrend::Degrading,
                    ),
                ],
                &[],
            )
            .into_iter()
            .map(|row| row.driver)
            .collect::<Vec<_>>(),
            vec!["--", "99"]
        );
    }
}
