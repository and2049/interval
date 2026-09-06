//! Derives the track-wide status shown on the map from the race control history.
//!
//! Two shapes of input reach this. FastF1 exports pre-resolved status rows (category
//! `track_status`, flag `green` / `yellow` / `red` / `safety_car` / `virtual_safety_car`
//! / `virtual_safety_car_ending`), one per change. OpenF1's live feed has no such row:
//! it carries the raw steward messages instead. Sector-scoped `YELLOW` / `DOUBLE
//! YELLOW` / `CLEAR` flags, driver-scoped `BLUE`, `SafetyCar`-category messages with no
//! flag at all (`SAFETY CAR DEPLOYED`, `VSC DEPLOYED`, `VSC ENDING`), and red flags as
//! plain `Other` text (`RED FLAG - RACE SUSPENDED`, alongside `SESSION ABORTED`).
//!
//! Taking "the latest row with a flag", as this once did, let a blue flag for a
//! backmarker outrank a live VSC and never showed the VSC at all. This folds the whole
//! history into one state and ranks it: red > chequered > safety car > VSC > double
//! yellow > yellow > green. Verified against the 2026 Italian GP feed.

use crate::domain::RaceControlMessage;
use std::collections::BTreeMap;

pub(crate) const GREEN: &str = "green";
pub(crate) const YELLOW: &str = "yellow";
pub(crate) const DOUBLE_YELLOW: &str = "double_yellow";
pub(crate) const RED: &str = "red";
pub(crate) const SAFETY_CAR: &str = "safety_car";
pub(crate) const SAFETY_CAR_ENDING: &str = "safety_car_ending";
pub(crate) const VIRTUAL_SAFETY_CAR: &str = "virtual_safety_car";
pub(crate) const VIRTUAL_SAFETY_CAR_ENDING: &str = "virtual_safety_car_ending";
pub(crate) const CHEQUERED: &str = "chequered";

const NORMALIZED_STATUSES: &[&str] = &[
    GREEN,
    YELLOW,
    DOUBLE_YELLOW,
    RED,
    SAFETY_CAR,
    SAFETY_CAR_ENDING,
    VIRTUAL_SAFETY_CAR,
    VIRTUAL_SAFETY_CAR_ENDING,
    CHEQUERED,
];

/// The status in force after every message in `messages` (which must be in time
/// order) has been applied.
pub(crate) fn track_status<'a>(messages: impl IntoIterator<Item = &'a RaceControlMessage>) -> String {
    let mut state = TrackStatusState::default();
    for message in messages {
        state.apply(message);
    }
    state.status()
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct TrackStatusState {
    /// A pre-resolved status row (FastF1). Authoritative while present.
    direct: Option<String>,
    red: bool,
    chequered: bool,
    safety_car: Option<Phase>,
    virtual_safety_car: Option<Phase>,
    /// Yellow flags still standing, keyed by sector ("track" for a track-wide one).
    yellows: BTreeMap<String, Yellow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Deployed,
    Ending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Yellow {
    Single,
    Double,
}

impl TrackStatusState {
    pub(crate) fn apply(&mut self, message: &RaceControlMessage) {
        if let Some(flag) = message
            .flag
            .as_deref()
            .filter(|flag| NORMALIZED_STATUSES.contains(flag))
        {
            self.direct = Some(flag.to_string());
            return;
        }

        let text = message.message.to_ascii_uppercase();
        let flag = message.flag.as_deref().map(str::to_ascii_uppercase);
        let scope = message.scope.as_deref().map(str::to_ascii_uppercase);
        let category = message.category.to_ascii_uppercase();

        if category == "SESSIONSTATUS" {
            if text.contains("ABORTED") {
                self.suspend();
            } else if text.contains("STARTED") {
                // The (re)start after a red flag. Yellows standing before the
                // suspension are stale by now too.
                self.red = false;
                self.chequered = false;
                self.yellows.clear();
            } else if text.contains("FINISHED") || text.contains("FINALISED") || text.contains("ENDS") {
                self.chequered = true;
            }
            return;
        }
        if mentions_red_flag(&text) {
            self.suspend();
            return;
        }
        if category == "SAFETYCAR" || text.contains("SAFETY CAR") || text.starts_with("VSC") {
            let is_virtual = text.contains("VIRTUAL") || text.starts_with("VSC") || text.contains(" VSC");
            let phase = if text.contains("ENDING") || text.contains("IN THIS LAP") {
                Some(Phase::Ending)
            } else if text.contains("DEPLOYED") {
                Some(Phase::Deployed)
            } else {
                // "SAFETY CAR LIGHTS ON" and similar chatter change nothing.
                None
            };
            if let Some(phase) = phase {
                // A (virtual) safety car deployment is also how a suspended race resumes.
                self.red = false;
                if is_virtual {
                    self.virtual_safety_car = Some(phase);
                } else {
                    self.safety_car = Some(phase);
                }
            }
            return;
        }

        match flag.as_deref() {
            Some("CHEQUERED") => self.chequered = true,
            Some("GREEN") => {
                // "GREEN LIGHT - PIT EXIT OPEN" at the start, or the green after a restart.
                self.red = false;
                self.safety_car = None;
                self.virtual_safety_car = None;
                self.yellows.clear();
            }
            Some("CLEAR") => match sector_key(&text, scope.as_deref()) {
                Some(key) => {
                    self.yellows.remove(&key);
                }
                None => {
                    // "TRACK CLEAR". It is also sent while a race is suspended, so it
                    // ends a safety car period but never a red flag.
                    self.safety_car = None;
                    self.virtual_safety_car = None;
                    self.yellows.clear();
                }
            },
            Some("YELLOW") | Some("DOUBLE YELLOW") => {
                let level = if flag.as_deref() == Some("DOUBLE YELLOW") {
                    Yellow::Double
                } else {
                    Yellow::Single
                };
                let key = sector_key(&text, scope.as_deref()).unwrap_or_else(|| "track".to_string());
                self.yellows.insert(key, level);
            }
            // BLUE, BLACK AND WHITE, BLACK, ...: aimed at one driver, never a track condition.
            _ => {}
        }
    }

    fn suspend(&mut self) {
        self.red = true;
        self.safety_car = None;
        self.virtual_safety_car = None;
    }

    pub(crate) fn status(&self) -> String {
        if let Some(direct) = &self.direct {
            return direct.clone();
        }
        let status = if self.red {
            RED
        } else if self.chequered {
            CHEQUERED
        } else if let Some(phase) = self.safety_car {
            match phase {
                Phase::Deployed => SAFETY_CAR,
                Phase::Ending => SAFETY_CAR_ENDING,
            }
        } else if let Some(phase) = self.virtual_safety_car {
            match phase {
                Phase::Deployed => VIRTUAL_SAFETY_CAR,
                Phase::Ending => VIRTUAL_SAFETY_CAR_ENDING,
            }
        } else if self.yellows.values().any(|level| *level == Yellow::Double) {
            DOUBLE_YELLOW
        } else if !self.yellows.is_empty() {
            YELLOW
        } else {
            GREEN
        };
        status.to_string()
    }
}

/// Whole-word match: "CHEQUERED FLAG" contains the letters "RED FLAG" and must not count.
pub(crate) fn mentions_red_flag(upper_text: &str) -> bool {
    upper_text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .windows(2)
        .any(|pair| pair == ["RED", "FLAG"])
}

/// "YELLOW IN TRACK SECTOR 15" -> "15". Track-scoped rows ("TRACK CLEAR") have no
/// sector and return `None`; a sector-scoped row without a parseable number keys by
/// its text so that its own CLEAR (same text shape) can still remove it.
fn sector_key(text: &str, scope: Option<&str>) -> Option<String> {
    if scope == Some("TRACK") {
        return None;
    }
    let number = text
        .rsplit(' ')
        .next()
        .and_then(|last| last.parse::<u32>().ok());
    match (number, scope) {
        (Some(number), _) if text.contains("SECTOR") => Some(number.to_string()),
        (_, Some("SECTOR")) => Some(text.rsplit(' ').next().unwrap_or(text).to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(t: f64, category: &str, flag: Option<&str>, scope: Option<&str>, message: &str) -> RaceControlMessage {
        RaceControlMessage {
            t,
            category: category.to_string(),
            message: message.to_string(),
            flag: flag.map(str::to_string),
            scope: scope.map(str::to_string),
        }
    }

    fn openf1_flag(t: f64, flag: &str, sector: u32) -> RaceControlMessage {
        row(
            t,
            "Flag",
            Some(flag),
            Some("Sector"),
            &format!("{flag} IN TRACK SECTOR {sector}"),
        )
    }

    fn status_after(rows: &[RaceControlMessage]) -> String {
        track_status(rows.iter())
    }

    #[test]
    fn empty_history_is_green() {
        assert_eq!(status_after(&[]), GREEN);
    }

    #[test]
    fn fastf1_status_rows_are_taken_as_is() {
        let rows = [
            row(10.0, "track_status", Some("yellow"), None, "Track status 2"),
            row(20.0, "track_status", Some("safety_car"), None, "Track status 4"),
            row(30.0, "track_status", Some("green"), None, "Track status 1"),
        ];
        assert_eq!(track_status(rows[..1].iter()), YELLOW);
        assert_eq!(track_status(rows[..2].iter()), SAFETY_CAR);
        assert_eq!(track_status(rows.iter()), GREEN);
    }

    #[test]
    fn blue_flags_never_change_the_track_status() {
        let rows = [
            openf1_flag(10.0, "DOUBLE YELLOW", 3),
            row(11.0, "Flag", Some("BLUE"), Some("Driver"), "WAVED BLUE FLAG FOR CAR 43 (COL) TIMED AT 16:10:03"),
        ];
        assert_eq!(status_after(&rows), DOUBLE_YELLOW);
        let rows = [row(11.0, "Flag", Some("BLUE"), Some("Driver"), "WAVED BLUE FLAG FOR CAR 43 (COL)")];
        assert_eq!(status_after(&rows), GREEN);
    }

    #[test]
    fn sector_yellows_stand_until_their_own_sector_clears() {
        let mut rows = vec![openf1_flag(10.0, "YELLOW", 2), openf1_flag(10.5, "YELLOW", 5)];
        assert_eq!(status_after(&rows), YELLOW);
        rows.push(openf1_flag(20.0, "CLEAR", 2));
        assert_eq!(status_after(&rows), YELLOW, "sector 5 is still yellow");
        rows.push(openf1_flag(21.0, "CLEAR", 5));
        assert_eq!(status_after(&rows), GREEN);
    }

    #[test]
    fn double_yellow_outranks_yellow() {
        let rows = [openf1_flag(10.0, "YELLOW", 14), openf1_flag(10.0, "DOUBLE YELLOW", 15)];
        assert_eq!(status_after(&rows), DOUBLE_YELLOW);
    }

    /// The Monza 2026 VSC, verbatim: sector yellows, VSC deployed with no flag, a
    /// double yellow during it, VSC ending, then track clear.
    #[test]
    fn vsc_is_shown_from_the_safety_car_message_and_outranks_yellows() {
        let mut rows = vec![
            openf1_flag(0.0, "YELLOW", 3),
            openf1_flag(0.0, "YELLOW", 2),
            row(15.0, "SafetyCar", None, None, "VSC DEPLOYED"),
        ];
        assert_eq!(status_after(&rows), VIRTUAL_SAFETY_CAR);
        rows.push(openf1_flag(33.0, "DOUBLE YELLOW", 3));
        assert_eq!(status_after(&rows), VIRTUAL_SAFETY_CAR, "VSC outranks a double yellow");
        rows.push(openf1_flag(118.0, "CLEAR", 2));
        rows.push(row(121.0, "SafetyCar", None, None, "VSC ENDING"));
        assert_eq!(status_after(&rows), VIRTUAL_SAFETY_CAR_ENDING);
        rows.push(openf1_flag(123.0, "CLEAR", 3));
        rows.push(row(132.0, "Flag", Some("CLEAR"), Some("Track"), "TRACK CLEAR"));
        assert_eq!(status_after(&rows), GREEN);
    }

    #[test]
    fn long_form_virtual_safety_car_messages_are_recognised() {
        let rows = [row(1.0, "SafetyCar", None, None, "VIRTUAL SAFETY CAR DEPLOYED")];
        assert_eq!(status_after(&rows), VIRTUAL_SAFETY_CAR);
        let rows = [
            row(1.0, "SafetyCar", None, None, "VIRTUAL SAFETY CAR DEPLOYED"),
            row(2.0, "SafetyCar", None, None, "VIRTUAL SAFETY CAR ENDING"),
        ];
        assert_eq!(status_after(&rows), VIRTUAL_SAFETY_CAR_ENDING);
    }

    #[test]
    fn safety_car_phases_and_priority_over_vsc() {
        let mut rows = vec![
            row(1.0, "SafetyCar", None, None, "VSC DEPLOYED"),
            row(2.0, "SafetyCar", None, None, "SAFETY CAR DEPLOYED"),
        ];
        assert_eq!(status_after(&rows), SAFETY_CAR);
        rows.push(row(3.0, "Other", None, None, "SAFETY CAR LIGHTS ON"));
        assert_eq!(status_after(&rows), SAFETY_CAR, "chatter changes nothing");
        rows.push(row(4.0, "SafetyCar", None, None, "SAFETY CAR IN THIS LAP"));
        assert_eq!(status_after(&rows), SAFETY_CAR_ENDING);
        rows.push(row(5.0, "Flag", Some("CLEAR"), Some("Track"), "TRACK CLEAR"));
        assert_eq!(status_after(&rows), GREEN);
    }

    /// The Monza 2026 red flag, verbatim: SC, session aborted, red flag, a TRACK CLEAR
    /// while still suspended, then SESSION STARTED for the restart.
    #[test]
    fn red_flag_survives_track_clear_and_ends_with_the_session_restart() {
        let mut rows = vec![
            openf1_flag(0.0, "DOUBLE YELLOW", 15),
            row(16.0, "SafetyCar", None, None, "SAFETY CAR DEPLOYED"),
            row(69.0, "SessionStatus", None, None, "SESSION ABORTED"),
            row(70.0, "Other", None, None, "RED FLAG - RACE SUSPENDED"),
        ];
        assert_eq!(status_after(&rows), RED);
        rows.push(openf1_flag(70.0, "CLEAR", 15));
        rows.push(row(172.0, "Flag", Some("CLEAR"), Some("Track"), "TRACK CLEAR"));
        assert_eq!(status_after(&rows), RED, "the race is still suspended");
        rows.push(openf1_flag(189.0, "DOUBLE YELLOW", 16));
        assert_eq!(status_after(&rows), RED);
        rows.push(row(1_950.0, "SessionStatus", None, None, "SESSION STARTED"));
        assert_eq!(status_after(&rows), GREEN);
    }

    #[test]
    fn red_flag_ends_when_a_safety_car_restart_or_green_light_follows() {
        let rows = [
            row(1.0, "Other", None, None, "RED FLAG"),
            row(2.0, "SafetyCar", None, None, "SAFETY CAR DEPLOYED"),
        ];
        assert_eq!(status_after(&rows), SAFETY_CAR);
        let rows = [
            row(1.0, "Other", None, None, "RED FLAG"),
            row(2.0, "Flag", Some("GREEN"), Some("Track"), "GREEN LIGHT - PIT EXIT OPEN"),
        ];
        assert_eq!(status_after(&rows), GREEN);
    }

    #[test]
    fn chequered_flag_outranks_everything_but_red() {
        let rows = [
            openf1_flag(1.0, "YELLOW", 4),
            row(2.0, "Flag", Some("CHEQUERED"), Some("Track"), "CHEQUERED FLAG"),
        ];
        assert_eq!(status_after(&rows), CHEQUERED);
        let rows = [row(2.0, "SessionStatus", None, None, "SESSION FINISHED")];
        assert_eq!(status_after(&rows), CHEQUERED);
    }

    #[test]
    fn red_flag_mentions_are_whole_words() {
        assert!(mentions_red_flag("RED FLAG - RACE SUSPENDED"));
        assert!(mentions_red_flag("RED FLAG"));
        assert!(!mentions_red_flag("CHEQUERED FLAG"));
        assert!(!mentions_red_flag("RED AND YELLOW STRIPED FLAG IN SECTOR 3"));
    }

    #[test]
    fn sector_key_parses_sector_numbers_and_ignores_track_scope() {
        assert_eq!(sector_key("YELLOW IN TRACK SECTOR 15", Some("SECTOR")), Some("15".to_string()));
        assert_eq!(sector_key("CLEAR IN TRACK SECTOR 7", None), Some("7".to_string()));
        assert_eq!(sector_key("TRACK CLEAR", Some("TRACK")), None);
        assert_eq!(sector_key("TRACK CLEAR", None), None);
    }
}
