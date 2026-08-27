//! Port of `frontend/src/lib/stintTimeline.ts`.

#[derive(Debug, Clone, PartialEq)]
pub struct StintProgressDisplay {
    pub label: String,
    pub width_percent: f64,
    pub known: bool,
}

pub fn stint_progress_display(stint_age: Option<f64>) -> StintProgressDisplay {
    let Some(stint_age) = stint_age.filter(|age| age.is_finite() && *age >= 0.0) else {
        return StintProgressDisplay {
            label: "Age --".to_string(),
            width_percent: 0.0,
            known: false,
        };
    };

    StintProgressDisplay {
        label: format!("Age {}", stint_age.floor() as i64),
        width_percent: (stint_age * 4.0).clamp(0.0, 100.0),
        known: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_missing_or_invalid_stint_ages_as_unknown() {
        let unknown = StintProgressDisplay {
            label: "Age --".to_string(),
            width_percent: 0.0,
            known: false,
        };
        assert_eq!(stint_progress_display(None), unknown);
        assert_eq!(stint_progress_display(Some(f64::NAN)), unknown);
        assert_eq!(stint_progress_display(Some(-1.0)), unknown);
    }

    #[test]
    fn formats_known_stint_ages_and_clamps_progress_width() {
        assert_eq!(
            stint_progress_display(Some(8.7)),
            StintProgressDisplay {
                label: "Age 8".to_string(),
                width_percent: 34.8,
                known: true,
            }
        );
        let clamped = stint_progress_display(Some(40.0));
        assert_eq!(clamped.width_percent, 100.0);
        assert!(clamped.known);
    }
}
