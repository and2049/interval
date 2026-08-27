//! Port of `frontend/src/lib/sessionKeys.ts`. The browser build persisted the last
//! session key in localStorage; here the shell owns the IO and this module only
//! validates the stored string and renders the value to store.

pub const DEMO_SESSION_KEY: i64 = 9839;
pub const MVP_HISTORICAL_SESSION_KEY: i64 = 9472;
pub const MVP_HISTORICAL_SEASON: i32 = 2024;
pub const MVP_HISTORICAL_MEETING_KEY: i64 = 1229;
pub const LAST_SESSION_STORAGE_KEY: &str = "interval:last-session-key";

pub fn read_stored_session_key(stored: Option<&str>) -> Option<i64> {
    let value = stored.filter(|value| !value.is_empty())?;
    let parsed = value.trim().parse::<f64>().ok()?;
    if parsed.is_finite() && parsed.fract() == 0.0 && parsed > 0.0 {
        Some(parsed as i64)
    } else {
        None
    }
}

pub fn write_stored_session_key(session_key: i64) -> String {
    session_key.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_valid_stored_session_ids() {
        assert_eq!(read_stored_session_key(Some("9472")), Some(9472));
    }

    #[test]
    fn ignores_missing_invalid_and_non_positive_values() {
        assert_eq!(read_stored_session_key(None), None);
        assert_eq!(read_stored_session_key(Some("not-a-number")), None);
        assert_eq!(read_stored_session_key(Some("-1")), None);
    }

    #[test]
    fn writes_the_selected_session_id() {
        assert_eq!(write_stored_session_key(9839), "9839");
    }

    #[test]
    fn clears_stale_stored_session_ids() {
        // Clearing is shell IO (remove the stored value); a cleared store reads
        // back as no session key.
        assert_eq!(read_stored_session_key(None), None);
    }
}
