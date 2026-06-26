use chrono::{DateTime, Utc};

pub(crate) fn parse_date(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.with_timezone(&Utc))
}

pub(crate) fn t_since_start(
    value: Option<&str>,
    session_start: Option<DateTime<Utc>>,
) -> Option<f64> {
    let date = parse_date(value?)?;
    let start = session_start?;
    Some((date - start).num_milliseconds() as f64 / 1000.0)
}
