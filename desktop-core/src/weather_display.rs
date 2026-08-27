//! Port of `frontend/src/lib/weatherDisplay.ts`.

use crate::formatters::{format_percent, format_speed, format_temperature};
use interval_backend::domain::ReplayWeatherSection;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeatherMetricDisplay {
    pub label: &'static str,
    pub value: String,
}

pub fn weather_metrics(weather: &ReplayWeatherSection) -> Vec<WeatherMetricDisplay> {
    let sample = weather.sample.as_ref();
    vec![
        WeatherMetricDisplay {
            label: "Air",
            value: format_temperature(sample.and_then(|s| s.air_temp)),
        },
        WeatherMetricDisplay {
            label: "Track",
            value: format_temperature(sample.and_then(|s| s.track_temp)),
        },
        WeatherMetricDisplay {
            label: "Humidity",
            value: format_percent(sample.and_then(|s| s.humidity)),
        },
        WeatherMetricDisplay {
            label: "Wind",
            value: format_speed(sample.and_then(|s| s.wind_speed)),
        },
    ]
}

pub fn has_weather_sample(weather: &ReplayWeatherSection) -> bool {
    weather.sample.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use interval_backend::domain::{DataQuality, WeatherSample};

    fn display(label: &'static str, value: &str) -> WeatherMetricDisplay {
        WeatherMetricDisplay {
            label,
            value: value.to_string(),
        }
    }

    fn weather(sample: Option<WeatherSample>) -> ReplayWeatherSection {
        ReplayWeatherSection {
            sample,
            quality: DataQuality::Ready,
        }
    }

    fn sample(
        air_temp: Option<f64>,
        track_temp: Option<f64>,
        humidity: Option<f64>,
        wind_speed: Option<f64>,
    ) -> WeatherSample {
        WeatherSample {
            t: 0.0,
            air_temp,
            track_temp,
            humidity,
            rainfall: None,
            wind_direction: None,
            wind_speed,
        }
    }

    #[test]
    fn formats_compact_weather_metrics_from_a_sample() {
        let metrics = weather_metrics(&weather(Some(sample(
            Some(22.25),
            Some(31.7),
            Some(42.0),
            Some(1.24),
        ))));
        assert_eq!(
            metrics,
            vec![
                display("Air", "22.3C"),
                display("Track", "31.7C"),
                display("Humidity", "42%"),
                display("Wind", "1.2 m/s"),
            ]
        );
    }

    #[test]
    fn detects_missing_weather_samples_separately_from_missing_fields() {
        let empty = weather(Some(sample(None, None, None, None)));
        assert!(has_weather_sample(&empty));
        assert_eq!(
            weather_metrics(&empty)
                .into_iter()
                .map(|metric| metric.value)
                .collect::<Vec<_>>(),
            vec!["--", "--", "--", "--"]
        );
        assert!(!has_weather_sample(&ReplayWeatherSection {
            sample: None,
            quality: DataQuality::Missing,
        }));
    }
}
