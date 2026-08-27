//! Port of `frontend/src/lib/formatters.ts`.

use interval_backend::domain::{MetricTrend, SectorStatus, TyreCompound};

/// Semantic color tone standing in for the CSS class strings the TS module
/// returned. One variant per distinct class the original could emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// `text-fuchsia-300` — overall-best sector.
    Fuchsia,
    /// `text-mint` — personal best / improving.
    Mint,
    /// `text-timing` — normal timing figure.
    Timing,
    /// `text-danger` / `border-danger`.
    Danger,
    /// `text-amber` / `border-amber`.
    Amber,
    /// `text-slate-100` — the bright hard-compound outline.
    Bright,
    /// `text-emerald-300` — intermediate compound.
    Emerald,
    /// `text-sky-300` — wet compound.
    Sky,
    /// `text-slate-300` — plain body text fallback.
    Neutral,
    /// `text-slate-400/500` — unknown/missing data.
    Muted,
}

pub fn format_race_clock(seconds: f64) -> String {
    let total = seconds.floor().max(0.0) as i64;
    format_clock_parts(total)
}

pub fn format_event_clock(seconds: f64) -> String {
    let total = seconds.floor() as i64;
    if total < 0 {
        return format!("T-{}", format_clock_parts(total.abs()));
    }
    format_clock_parts(total)
}

fn format_clock_parts(total: i64) -> String {
    format!("{:02}:{:02}", total / 60, total % 60)
}

pub fn format_lap_time(value: Option<f64>) -> String {
    let Some(value) = value.filter(|v| !v.is_nan()) else {
        return "--".to_string();
    };
    let minutes = (value / 60.0).floor() as i64;
    let seconds = value - minutes as f64 * 60.0;
    format!("{}:{:0>6}", minutes, to_fixed(seconds, 3))
}

pub fn format_temperature(value: Option<f64>) -> String {
    match value.filter(|v| !v.is_nan()) {
        Some(value) => format!("{}C", to_fixed(value, 1)),
        None => "--".to_string(),
    }
}

pub fn format_percent(value: Option<f64>) -> String {
    match value.filter(|v| !v.is_nan()) {
        Some(value) => format!("{}%", to_fixed(value, 0)),
        None => "--".to_string(),
    }
}

pub fn format_speed(value: Option<f64>) -> String {
    match value.filter(|v| !v.is_nan()) {
        Some(value) => format!("{} m/s", to_fixed(value, 1)),
        None => "--".to_string(),
    }
}

/// JS `Number.prototype.toFixed` rounds ties away from zero, while Rust's
/// `{:.n$}` rounds ties to even — they disagree on exact halves like `22.25`.
fn to_fixed(value: f64, digits: usize) -> String {
    let scale = 10f64.powi(digits as i32);
    let scaled = (value * scale).round();
    format!("{:.*}", digits, scaled / scale)
}

pub fn sector_class(status: &SectorStatus) -> Tone {
    match status {
        SectorStatus::OverallBest => Tone::Fuchsia,
        SectorStatus::PersonalBest => Tone::Mint,
        SectorStatus::Normal => Tone::Timing,
        SectorStatus::Unknown => Tone::Muted,
    }
}

pub fn compound_class(compound: &TyreCompound) -> Tone {
    match compound {
        TyreCompound::Soft => Tone::Danger,
        TyreCompound::Medium => Tone::Amber,
        TyreCompound::Hard => Tone::Bright,
        TyreCompound::Intermediate => Tone::Emerald,
        TyreCompound::Wet => Tone::Sky,
        TyreCompound::Unknown => Tone::Muted,
    }
}

pub fn compound_abbreviation(compound: &TyreCompound) -> &'static str {
    match compound {
        TyreCompound::Soft => "S",
        TyreCompound::Medium => "M",
        TyreCompound::Hard => "H",
        TyreCompound::Intermediate => "I",
        TyreCompound::Wet => "W",
        TyreCompound::Unknown => "--",
    }
}

pub fn trend_class(trend: &MetricTrend) -> Tone {
    match trend {
        MetricTrend::Improving => Tone::Mint,
        MetricTrend::Degrading => Tone::Danger,
        _ => Tone::Neutral,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_negative_race_time_to_zero() {
        assert_eq!(format_race_clock(-4.0), "00:00");
    }

    #[test]
    fn formats_elapsed_seconds_as_minute_clock() {
        assert_eq!(format_race_clock(125.9), "02:05");
    }

    #[test]
    fn formats_pre_session_events_with_t_minus_clock() {
        assert_eq!(format_event_clock(-176.4), "T-02:57");
    }

    #[test]
    fn formats_in_session_events_as_race_clock() {
        assert_eq!(format_event_clock(61.8), "01:01");
    }

    #[test]
    fn formats_null_and_nan_as_missing_data() {
        assert_eq!(format_lap_time(None), "--");
        assert_eq!(format_lap_time(Some(f64::NAN)), "--");
    }

    #[test]
    fn formats_lap_durations_with_millisecond_precision() {
        assert_eq!(format_lap_time(Some(90.1234)), "1:30.123");
    }

    #[test]
    fn render_missing_values_without_dangling_units() {
        assert_eq!(format_temperature(None), "--");
        assert_eq!(format_percent(None), "--");
        assert_eq!(format_speed(Some(f64::NAN)), "--");
    }

    #[test]
    fn render_values_with_compact_units() {
        assert_eq!(format_temperature(Some(23.74)), "23.7C");
        assert_eq!(format_percent(Some(49.4)), "49%");
        assert_eq!(format_speed(Some(1.56)), "1.6 m/s");
    }
}
