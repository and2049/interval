//! Desktop UI state persisted between launches: the window rectangle (the electron
//! shell's `window-state.json`) and the last opened session key (the renderer's
//! `localStorage` entry). Both live in the backend's config dir, next to
//! `settings.json`. Every failure degrades to "no saved state" — persistence is a
//! convenience, never fatal.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const WINDOW_STATE_FILE: &str = "window-state.json";
const UI_STATE_FILE: &str = "ui-state.json";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WindowState {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct UiState {
    pub last_session_key: Option<i64>,
}

fn state_path(file: &str) -> Option<PathBuf> {
    interval_backend::settings::config_dir().map(|dir| dir.join(file))
}

fn load<T: serde::de::DeserializeOwned>(file: &str) -> Option<T> {
    let text = std::fs::read_to_string(state_path(file)?).ok()?;
    serde_json::from_str(&text).ok()
}

fn save<T: Serialize>(file: &str, value: &T) {
    let Some(path) = state_path(file) else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(value) {
        let _ = std::fs::write(path, text);
    }
}

pub fn load_window_state() -> Option<WindowState> {
    load(WINDOW_STATE_FILE)
}

pub fn save_window_state(state: WindowState) {
    save(WINDOW_STATE_FILE, &state);
}

pub fn load_ui_state() -> UiState {
    load(UI_STATE_FILE).unwrap_or_default()
}

pub fn save_ui_state(state: UiState) {
    save(UI_STATE_FILE, &state);
}
