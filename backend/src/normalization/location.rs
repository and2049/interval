use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct LocationRecord {
    pub t: f64,
    pub driver_number: i32,
    pub x: f64,
    pub y: f64,
    pub z: Option<f64>,
    pub relative_distance: Option<f64>,
}

pub fn location_samples(
    payload: Value,
    session_start: Option<DateTime<Utc>>,
) -> anyhow::Result<Vec<LocationRecord>> {
    let rows = serde_json::from_value::<Vec<OpenF1Location>>(payload)?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let x = row.x?;
            let y = row.y?;
            if !x.is_finite() || !y.is_finite() {
                return None;
            }
            Some(LocationRecord {
                t: super::t_since_start(row.date.as_deref(), session_start)?,
                driver_number: row.driver_number,
                x,
                y,
                z: row.z,
                relative_distance: None,
            })
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct OpenF1Location {
    date: Option<String>,
    driver_number: i32,
    x: Option<f64>,
    y: Option<f64>,
    z: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalizes_location_samples() {
        let start = DateTime::parse_from_rfc3339("2024-03-02T15:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let samples = location_samples(
            json!([{
                "date": "2024-03-02T15:00:02.500Z",
                "driver_number": 1,
                "x": 100.0,
                "y": -20.0,
                "z": 5.0
            }]),
            Some(start),
        )
        .unwrap();

        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].driver_number, 1);
        assert_eq!(samples[0].t, 2.5);
    }
}
