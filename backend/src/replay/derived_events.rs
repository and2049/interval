use crate::{
    domain::{EventKind, EventSeverity, EventSource, ReplayEvent, WeatherSample},
    normalization::{PositionRecord, RaceData},
};

pub(crate) fn stint_change_events(data: &RaceData) -> Vec<ReplayEvent> {
    let mut events = Vec::new();
    for stint in data.stints.iter().filter(|stint| stint.stint_number > 1) {
        let Some(t) = data
            .laps
            .iter()
            .filter(|lap| {
                lap.lap.driver_number == stint.driver_number
                    && lap.lap.lap_number >= stint.lap_start
            })
            .map(|lap| lap.t_start)
            .min_by(f64::total_cmp)
        else {
            continue;
        };
        events.push(ReplayEvent {
            id: format!(
                "stint-change-{}-{}",
                stint.driver_number, stint.stint_number
            ),
            t,
            kind: EventKind::StintChange,
            severity: EventSeverity::Info,
            driver_number: Some(stint.driver_number),
            message: format!(
                "Driver {} started stint {} on {:?}",
                stint.driver_number, stint.stint_number, stint.compound
            ),
            source: EventSource::Derived,
            payload: serde_json::json!({
                "driver_number": stint.driver_number,
                "stint_number": stint.stint_number,
                "compound": stint.compound,
                "lap_start": stint.lap_start,
                "tyre_age_at_start": stint.tyre_age_at_start
            }),
        });
    }
    events
}

pub(crate) fn leader_change_events(positions: &[PositionRecord]) -> Vec<ReplayEvent> {
    let mut leaders = positions
        .iter()
        .filter(|record| record.position == 1)
        .collect::<Vec<_>>();
    leaders.sort_by(|a, b| a.t.total_cmp(&b.t));

    let mut events = Vec::new();
    let mut current_leader = None;
    for record in leaders {
        let driver_number = record.sample.driver_number;
        match current_leader {
            None => current_leader = Some(driver_number),
            Some(previous) if previous != driver_number => {
                current_leader = Some(driver_number);
                events.push(ReplayEvent {
                    id: format!("leader-change-{:.3}-{driver_number}", record.t),
                    t: record.t,
                    kind: EventKind::LeaderChange,
                    severity: EventSeverity::Notice,
                    driver_number: Some(driver_number),
                    message: format!("Driver {driver_number} took the lead"),
                    source: EventSource::Derived,
                    payload: serde_json::json!({
                        "driver_number": driver_number,
                        "previous_driver_number": previous,
                        "position": record.position
                    }),
                });
            }
            Some(_) => {}
        }
    }
    events
}

pub(crate) fn weather_change_events(weather: &[WeatherSample]) -> Vec<ReplayEvent> {
    let mut samples = weather.iter().collect::<Vec<_>>();
    samples.sort_by(|a, b| a.t.total_cmp(&b.t));

    let mut events = Vec::new();
    let mut previous: Option<&WeatherSample> = None;
    for sample in samples {
        let Some(prior) = previous else {
            previous = Some(sample);
            continue;
        };
        let rainfall_changed = prior.rainfall != sample.rainfall;
        let track_temp_delta = match (prior.track_temp, sample.track_temp) {
            (Some(before), Some(after)) => (after - before).abs(),
            _ => 0.0,
        };
        if rainfall_changed || track_temp_delta >= 2.0 {
            events.push(ReplayEvent {
                id: format!("weather-change-{:.3}", sample.t),
                t: sample.t,
                kind: EventKind::WeatherChange,
                severity: if sample.rainfall.unwrap_or(0.0) > 0.0 {
                    EventSeverity::Warning
                } else {
                    EventSeverity::Info
                },
                driver_number: None,
                message: weather_change_message(prior, sample),
                source: EventSource::Derived,
                payload: serde_json::json!({
                    "air_temp": sample.air_temp,
                    "track_temp": sample.track_temp,
                    "humidity": sample.humidity,
                    "rainfall": sample.rainfall,
                    "wind_direction": sample.wind_direction,
                    "wind_speed": sample.wind_speed
                }),
            });
            previous = Some(sample);
        }
    }
    events
}

fn weather_change_message(previous: &WeatherSample, current: &WeatherSample) -> String {
    if previous.rainfall != current.rainfall {
        return if current.rainfall.unwrap_or(0.0) > 0.0 {
            "Rainfall reported".to_string()
        } else {
            "Rainfall cleared".to_string()
        };
    }
    match (previous.track_temp, current.track_temp) {
        (Some(before), Some(after)) if after > before => {
            format!("Track temperature rose to {after:.1}C")
        }
        (Some(_), Some(after)) => format!("Track temperature fell to {after:.1}C"),
        _ => "Weather changed".to_string(),
    }
}
