mod discovery;
mod fastf1_bundle;
mod location;
mod race_endpoints;
mod records;
mod time;

pub use discovery::{meetings_from_openf1, race_sessions_from_openf1};
pub use location::{location_samples, LocationRecord};
pub use records::{
    IntervalRecord, LapRecord, PitEvent, PositionRecord, RaceData, RaceDataSource, SessionResult,
};
pub(crate) use time::{parse_date, t_since_start};

use crate::{connectors::openf1_historical::RawEndpoint, domain::Session};
use serde_json::Value;
use std::collections::HashMap;

pub fn race_data_from_bundle(
    bundle: &[RawEndpoint],
    session: &Session,
) -> anyhow::Result<RaceData> {
    if bundle
        .iter()
        .any(|entry| entry.endpoint.starts_with("fastf1_"))
    {
        return fastf1_bundle::race_data_from_bundle(bundle, session);
    }

    let by_endpoint = bundle
        .iter()
        .map(|entry| (entry.endpoint.as_str(), entry.payload.clone()))
        .collect::<HashMap<_, _>>();
    let session_start = parse_date(&session.start_time);

    Ok(RaceData {
        source: RaceDataSource::OpenF1Historical,
        drivers: race_endpoints::drivers(
            by_endpoint
                .get("drivers")
                .cloned()
                .unwrap_or(Value::Array(vec![])),
        )?,
        laps: race_endpoints::laps(
            by_endpoint
                .get("laps")
                .cloned()
                .unwrap_or(Value::Array(vec![])),
            session_start,
        )?,
        intervals: race_endpoints::intervals(
            by_endpoint
                .get("intervals")
                .cloned()
                .unwrap_or(Value::Array(vec![])),
            session_start,
        )?,
        positions: race_endpoints::positions(
            by_endpoint
                .get("position")
                .cloned()
                .unwrap_or(Value::Array(vec![])),
            session_start,
        )?,
        locations: location_samples(
            by_endpoint
                .get("location")
                .cloned()
                .unwrap_or(Value::Array(vec![])),
            session_start,
        )?,
        geometry_locations: vec![],
        pits: race_endpoints::pits(
            by_endpoint
                .get("pit")
                .cloned()
                .unwrap_or(Value::Array(vec![])),
            session_start,
        )?,
        race_control: race_endpoints::race_control(
            by_endpoint
                .get("race_control")
                .cloned()
                .unwrap_or(Value::Array(vec![])),
            session_start,
        )?,
        stints: race_endpoints::stints(
            by_endpoint
                .get("stints")
                .cloned()
                .unwrap_or(Value::Array(vec![])),
        )?,
        weather: race_endpoints::weather(
            by_endpoint
                .get("weather")
                .cloned()
                .unwrap_or(Value::Array(vec![])),
            session_start,
        )?,
        session_results: race_endpoints::session_results(
            by_endpoint
                .get("session_result")
                .cloned()
                .unwrap_or(Value::Array(vec![])),
        )?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::SessionType;
    use serde_json::json;

    #[test]
    fn normalizes_meetings() {
        let meetings = meetings_from_openf1(json!([{
            "meeting_key": 1216,
            "meeting_name": "Belgian Grand Prix",
            "country_name": "Belgium",
            "location": "Spa-Francorchamps",
            "year": 2023
        }]))
        .unwrap();

        assert_eq!(meetings[0].meeting_key, 1216);
        assert_eq!(meetings[0].name, "Belgian Grand Prix");
    }

    #[test]
    fn filters_race_sessions() {
        let sessions = race_sessions_from_openf1(json!([
            {
                "session_key": 1,
                "meeting_key": 10,
                "session_name": "Qualifying",
                "session_type": "Qualifying",
                "date_start": "2023-01-01T12:00:00+00:00",
                "date_end": "2023-01-01T13:00:00+00:00",
                "year": 2023
            },
            {
                "session_key": 2,
                "meeting_key": 10,
                "session_name": "Race",
                "session_type": "Race",
                "date_start": "2023-01-02T12:00:00+00:00",
                "date_end": "2023-01-02T14:00:00+00:00",
                "year": 2023
            }
        ]))
        .unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_key, 2);
    }

    #[test]
    fn normalizes_interval_strings_and_numbers() {
        let endpoint = RawEndpoint {
            endpoint: "intervals".to_string(),
            session_key: 2,
            payload: json!([
                {
                    "date": "2023-01-02T12:00:04+00:00",
                    "driver_number": 4,
                    "gap_to_leader": "+1 LAP",
                    "interval": 1.234
                }
            ]),
        };
        let session = Session {
            session_key: 2,
            meeting_key: 10,
            year: 2023,
            name: "Race".to_string(),
            session_type: SessionType::Race,
            start_time: "2023-01-02T12:00:00+00:00".to_string(),
            end_time: "2023-01-02T14:00:00+00:00".to_string(),
            total_laps: 0,
        };

        let data = race_data_from_bundle(&[endpoint], &session).unwrap();
        assert_eq!(data.intervals[0].t, 4.0);
        assert_eq!(data.intervals[0].gap_to_leader.as_deref(), Some("+1 LAP"));
        assert_eq!(data.intervals[0].interval.as_deref(), Some("+1.234"));
    }
}
