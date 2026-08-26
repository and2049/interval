//! Platform-neutral logic for the interval desktop app: the HTTP/SSE client for the
//! embedded backend, the replay store (state machine), and the pure-logic ports of
//! `frontend/src/lib/*.ts`. No gpui dependency, so `cargo test -p interval-desktop-core`
//! iterates without linking the UI stack.
//!
//! Each port module mirrors one `frontend/src/lib/*.ts` file, and its `#[cfg(test)]`
//! module mirrors the colocated `*.test.ts` — the bun suite is the behavioral spec.

pub mod api_client;
pub mod derived_metrics;
pub mod formatters;
pub mod playback;
pub mod replay_events;
pub mod replay_quality;
pub mod selector;
pub mod session_keys;
pub mod session_readiness;
pub mod session_selection;
pub mod settings_panel;
pub mod sse;
pub mod stint_timeline;
pub mod store;
pub mod timing_display;
pub mod track_geometry;
pub mod track_map_view;
pub mod weather_display;
