//! Port of `frontend/src/lib/timingDisplay.ts`.

use interval_backend::domain::{DriverSnapshot, Sector};

const SECTOR_COUNT: usize = 3;

pub fn sector_cells(sectors: &[Sector]) -> [Option<&Sector>; SECTOR_COUNT] {
    std::array::from_fn(|index| sectors.get(index))
}

pub fn has_timing_rows(rows: &[DriverSnapshot]) -> bool {
    !rows.is_empty()
}

pub fn gap_label<'a>(position: i32, gap: Option<&'a str>) -> &'a str {
    if position == 1 {
        return "LEADER";
    }
    gap.unwrap_or("--")
}

pub fn interval_label(interval: Option<&str>) -> &str {
    interval.unwrap_or("--")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formatters::compound_abbreviation;
    use interval_backend::domain::{SectorStatus, TyreCompound};

    fn sector(index: i32, duration: f64) -> Sector {
        Sector {
            index,
            duration: Some(duration),
            status: SectorStatus::Normal,
        }
    }

    #[test]
    fn pads_missing_sector_cells_to_a_stable_three_column_display() {
        assert_eq!(sector_cells(&[]), [None, None, None]);
        let sectors = [sector(1, 29.1)];
        assert_eq!(sector_cells(&sectors), [Some(&sectors[0]), None, None]);
    }

    #[test]
    fn trims_extra_sector_samples_to_the_display_columns() {
        let sectors = [
            sector(1, 1.0),
            sector(2, 2.0),
            sector(3, 3.0),
            sector(4, 4.0),
        ];
        assert_eq!(sector_cells(&sectors).len(), 3);
    }

    #[test]
    fn uses_compact_tyre_labels_with_an_explicit_unknown_fallback() {
        assert_eq!(compound_abbreviation(&TyreCompound::Soft), "S");
        assert_eq!(compound_abbreviation(&TyreCompound::Intermediate), "I");
        assert_eq!(compound_abbreviation(&TyreCompound::Unknown), "--");
    }

    #[test]
    fn detects_empty_timing_tower_data() {
        assert!(!has_timing_rows(&[]));
        assert!(has_timing_rows(&[row()]));
    }

    #[test]
    fn formats_leader_and_missing_gap_labels() {
        assert_eq!(gap_label(1, None), "LEADER");
        assert_eq!(gap_label(4, Some("+4.2")), "+4.2");
        assert_eq!(gap_label(4, None), "--");
    }

    #[test]
    fn renders_provided_intervals_and_falls_back_for_missing_values() {
        assert_eq!(interval_label(Some("+1.234")), "+1.234");
        assert_eq!(interval_label(None), "--");
    }

    fn row() -> DriverSnapshot {
        use interval_backend::domain::{Driver, DriverStatus, RankSource};
        DriverSnapshot {
            driver: Driver {
                driver_number: 1,
                code: "VER".to_string(),
                full_name: "Max Verstappen".to_string(),
                team_name: "Red Bull Racing".to_string(),
                team_colour: "3671C6".to_string(),
            },
            position: 1,
            rank_source: RankSource::OpenF1Position,
            gap_to_leader: None,
            interval: None,
            lap: 1,
            last_lap: None,
            compound: TyreCompound::Medium,
            stint_age: None,
            sectors: vec![],
            in_pit: false,
            status: DriverStatus::OnTrack,
        }
    }
}
