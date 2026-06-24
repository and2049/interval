use reqwest::Url;
use serde_json::Value;
use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};
use thiserror::Error;
use tokio::{
    sync::Mutex,
    time::{sleep, timeout},
};

const BASE_URL: &str = "https://api.openf1.org/v1/";

#[derive(Clone)]
pub struct HistoricalClient {
    http: reqwest::Client,
    base_url: Url,
    limiter: Arc<Mutex<RateLimiter>>,
}

impl Default for HistoricalClient {
    fn default() -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: Url::parse(BASE_URL).expect("valid OpenF1 base url"),
            limiter: Arc::new(Mutex::new(RateLimiter::default())),
        }
    }
}

impl HistoricalClient {
    pub async fn fetch_meetings(&self, year: i32) -> Result<Value, HistoricalError> {
        self.fetch_endpoint("meetings", &[("year", year.to_string())])
            .await
    }

    pub async fn fetch_sessions_for_meeting(
        &self,
        meeting_key: i64,
    ) -> Result<Value, HistoricalError> {
        self.fetch_endpoint("sessions", &[("meeting_key", meeting_key.to_string())])
            .await
    }

    pub async fn fetch_endpoint(
        &self,
        endpoint: &str,
        params: &[(&str, String)],
    ) -> Result<Value, HistoricalError> {
        self.limiter.lock().await.wait_turn().await;

        let mut url = self.base_url.join(endpoint)?;
        url.query_pairs_mut()
            .extend_pairs(params.iter().map(|(key, value)| (*key, value.as_str())));

        let response = self.http.get(url).send().await?.error_for_status()?;
        Ok(response.json::<Value>().await?)
    }

    pub async fn fetch_race_bundle(
        &self,
        session_key: i64,
    ) -> Result<Vec<RawEndpoint>, HistoricalError> {
        let required_endpoints = [
            "drivers",
            "laps",
            "intervals",
            "position",
            "pit",
            "race_control",
            "stints",
            "weather",
            "session_result",
        ];
        let optional_endpoints = ["starting_grid"];

        let mut out = Vec::with_capacity(required_endpoints.len() + optional_endpoints.len());
        for endpoint in required_endpoints {
            let payload = self
                .fetch_endpoint(endpoint, &[("session_key", session_key.to_string())])
                .await?;
            out.push(RawEndpoint {
                endpoint: endpoint.to_string(),
                session_key,
                payload,
            });
        }
        let location_payload = timeout(
            Duration::from_secs(25),
            self.fetch_location_payload(session_key, &out),
        )
        .await
        .unwrap_or_else(|_| Value::Array(vec![]));
        out.push(RawEndpoint {
            endpoint: "location".to_string(),
            session_key,
            payload: location_payload,
        });
        for endpoint in optional_endpoints {
            let payload = self
                .fetch_endpoint(endpoint, &[("session_key", session_key.to_string())])
                .await
                .unwrap_or_else(|_| Value::Array(vec![]));
            out.push(RawEndpoint {
                endpoint: endpoint.to_string(),
                session_key,
                payload,
            });
        }
        Ok(out)
    }

    async fn fetch_location_payload(&self, session_key: i64, bundle: &[RawEndpoint]) -> Value {
        let broad = self
            .fetch_endpoint("location", &[("session_key", session_key.to_string())])
            .await
            .unwrap_or_else(|_| Value::Array(vec![]));
        if broad.as_array().is_some_and(|items| !items.is_empty()) {
            return broad;
        }

        let driver_numbers = bundle
            .iter()
            .find(|entry| entry.endpoint == "drivers")
            .and_then(|entry| entry.payload.as_array())
            .into_iter()
            .flatten()
            .filter_map(|row| row.get("driver_number")?.as_i64())
            .collect::<Vec<_>>();

        let mut merged = Vec::new();
        for driver_number in driver_numbers {
            let payload = self
                .fetch_endpoint(
                    "location",
                    &[
                        ("session_key", session_key.to_string()),
                        ("driver_number", driver_number.to_string()),
                    ],
                )
                .await
                .unwrap_or_else(|_| Value::Array(vec![]));
            if let Some(items) = payload.as_array() {
                merged.extend(items.iter().cloned());
            }
        }

        Value::Array(merged)
    }
}

#[derive(Debug, Clone)]
pub struct RawEndpoint {
    pub endpoint: String,
    pub session_key: i64,
    pub payload: Value,
}

#[derive(Debug, Error)]
pub enum HistoricalError {
    #[error("invalid OpenF1 url: {0}")]
    Url(#[from] url::ParseError),
    #[error("OpenF1 request failed: {0}")]
    Request(#[from] reqwest::Error),
}

#[derive(Default)]
struct RateLimiter {
    second_window: VecDeque<Instant>,
    minute_window: VecDeque<Instant>,
}

impl RateLimiter {
    async fn wait_turn(&mut self) {
        loop {
            let now = Instant::now();
            self.drop_old(now);

            let second_full = self.second_window.len() >= 3;
            let minute_full = self.minute_window.len() >= 30;

            if !second_full && !minute_full {
                self.second_window.push_back(now);
                self.minute_window.push_back(now);
                return;
            }

            let second_wait = self.second_window.front().map(|oldest| {
                Duration::from_secs(1).saturating_sub(now.saturating_duration_since(*oldest))
            });
            let minute_wait = self.minute_window.front().map(|oldest| {
                Duration::from_secs(60).saturating_sub(now.saturating_duration_since(*oldest))
            });

            let wait_for = [second_wait, minute_wait]
                .into_iter()
                .flatten()
                .max()
                .unwrap_or(Duration::from_millis(50));
            sleep(wait_for.max(Duration::from_millis(50))).await;
        }
    }

    fn drop_old(&mut self, now: Instant) {
        while self
            .second_window
            .front()
            .is_some_and(|instant| now.duration_since(*instant) >= Duration::from_secs(1))
        {
            self.second_window.pop_front();
        }
        while self
            .minute_window
            .front()
            .is_some_and(|instant| now.duration_since(*instant) >= Duration::from_secs(60))
        {
            self.minute_window.pop_front();
        }
    }
}
