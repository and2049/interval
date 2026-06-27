use crate::{
    connectors::openf1_historical::RawEndpoint,
    domain::{AvailableChannels, EndpointCoverage},
};

pub fn endpoint_coverage(bundle: &[RawEndpoint]) -> Vec<EndpointCoverage> {
    bundle
        .iter()
        .map(|entry| EndpointCoverage {
            endpoint: entry.endpoint.clone(),
            present: rows_in_payload(&entry.payload).is_some_and(|rows| rows > 0),
            rows: rows_in_payload(&entry.payload),
        })
        .collect()
}

pub fn coverage_warnings(
    coverage: &[EndpointCoverage],
    channels: &AvailableChannels,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if !channels.location {
        warnings.push(if channels.track_geometry {
            "location channel missing; track map will use projected curated geometry".to_string()
        } else {
            "location channel missing; track map will use schematic fallback".to_string()
        });
    }
    if !channels.weather {
        warnings.push("weather channel missing".to_string());
    }
    if !channels.race_control {
        warnings.push("race-control channel missing".to_string());
    }
    if !channels.intervals {
        warnings.push("interval channel missing; timing gaps may be incomplete".to_string());
    }
    for endpoint in coverage.iter().filter(|endpoint| !endpoint.present) {
        warnings.push(format!(
            "historical data section '{}' returned no rows",
            endpoint.endpoint
        ));
    }
    warnings.sort();
    warnings.dedup();
    warnings
}

fn rows_in_payload(payload: &serde_json::Value) -> Option<usize> {
    payload.as_array().map(Vec::len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn endpoint_coverage_counts_array_rows() {
        let coverage = endpoint_coverage(&[
            raw("drivers", json!([{ "driver_number": 1 }])),
            raw("weather", json!([])),
            raw("metadata", json!({ "ok": true })),
        ]);

        assert_eq!(
            coverage,
            vec![
                EndpointCoverage {
                    endpoint: "drivers".to_string(),
                    present: true,
                    rows: Some(1)
                },
                EndpointCoverage {
                    endpoint: "weather".to_string(),
                    present: false,
                    rows: Some(0)
                },
                EndpointCoverage {
                    endpoint: "metadata".to_string(),
                    present: false,
                    rows: None
                }
            ]
        );
    }

    #[test]
    fn coverage_warnings_explain_curated_geometry_fallback() {
        let warnings = coverage_warnings(
            &[EndpointCoverage {
                endpoint: "location".to_string(),
                present: false,
                rows: Some(0),
            }],
            &channels_with(|channels| {
                channels.location = false;
                channels.track_geometry = true;
            }),
        );

        assert!(warnings
            .iter()
            .any(|warning| warning.contains("projected curated geometry")));
        assert!(warnings
            .iter()
            .any(|warning| warning.contains("'location' returned no rows")));
    }

    #[test]
    fn coverage_warnings_explain_schematic_fallback() {
        let warnings = coverage_warnings(
            &[],
            &channels_with(|channels| {
                channels.location = false;
                channels.track_geometry = false;
            }),
        );

        assert!(warnings
            .iter()
            .any(|warning| warning.contains("schematic fallback")));
    }

    #[test]
    fn coverage_warnings_are_sorted_and_deduplicated() {
        let coverage = vec![
            EndpointCoverage {
                endpoint: "weather".to_string(),
                present: false,
                rows: Some(0),
            },
            EndpointCoverage {
                endpoint: "weather".to_string(),
                present: false,
                rows: Some(0),
            },
        ];
        let warnings = coverage_warnings(
            &coverage,
            &channels_with(|channels| {
                channels.weather = false;
            }),
        );

        assert_eq!(warnings.windows(2).all(|pair| pair[0] <= pair[1]), true);
        assert_eq!(
            warnings
                .iter()
                .filter(|warning| warning.contains("'weather' returned no rows"))
                .count(),
            1
        );
    }

    fn raw(endpoint: &str, payload: serde_json::Value) -> RawEndpoint {
        RawEndpoint {
            endpoint: endpoint.to_string(),
            session_key: 1,
            payload,
        }
    }

    fn channels_with(mut update: impl FnMut(&mut AvailableChannels)) -> AvailableChannels {
        let mut channels = AvailableChannels {
            timing: true,
            location: true,
            track_geometry: true,
            weather: true,
            race_control: true,
            stints: true,
            pit_events: true,
            intervals: true,
        };
        update(&mut channels);
        channels
    }
}
