use crate::domain::{DerivedMetric, DerivedMetricKind, DriverSnapshot, MetricTrend};

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
        label: "3-lap pace".to_string(),
        value: format!("{last_lap:.3}"),
        trend,
    })
}
