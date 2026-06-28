import unittest
from pathlib import Path
import sys
from unittest.mock import patch

import pandas as pd

sys.path.insert(0, str(Path(__file__).resolve().parent))
import fastf1_export_replay as export


class FastF1IntervalExportTests(unittest.TestCase):
    def test_export_intervals_formats_leader_gap_and_interval(self):
        telemetry = [
            sample(1, 10.0, 2, 0.500, lap_duration=100.0),
            sample(16, 10.0, 2, 0.450, lap_duration=100.0),
            sample(44, 10.0, 2, 0.425, lap_duration=100.0),
        ]

        intervals = export.export_intervals(telemetry)

        self.assertEqual(
            intervals,
            [
                {"t": 10.0, "driver_number": 1, "gap_to_leader": None, "interval": None},
                {"t": 10.0, "driver_number": 16, "gap_to_leader": "+5.000", "interval": "+5.000"},
                {"t": 10.0, "driver_number": 44, "gap_to_leader": "+7.500", "interval": "+2.500"},
            ],
        )

    def test_export_intervals_labels_lapped_cars(self):
        telemetry = [
            sample(1, 12.0, 4, 0.100, lap_duration=90.0),
            sample(4, 12.0, 2, 0.950, lap_duration=90.0),
            sample(31, 12.0, 2, 0.900, lap_duration=90.0),
        ]

        intervals = export.export_intervals(telemetry)

        self.assertEqual(intervals[1]["gap_to_leader"], "+1 LAP")
        self.assertEqual(intervals[1]["interval"], "+1 LAP")
        self.assertEqual(intervals[2]["gap_to_leader"], "+1 LAP")
        self.assertEqual(intervals[2]["interval"], "+4.500")

    def test_export_intervals_ignores_missing_relative_distance(self):
        telemetry = [
            sample(1, 10.0, 2, 0.500, lap_duration=100.0),
            {"driver_number": 16, "t": 10.0, "lap_number": 2, "relative_distance": None},
        ]

        intervals = export.export_intervals(telemetry)

        self.assertEqual(len(intervals), 1)
        self.assertEqual(intervals[0]["driver_number"], 1)

    def test_positions_and_intervals_use_same_frame_cadence(self):
        telemetry = [
            sample(1, 10.12, 2, 0.500),
            sample(16, 10.13, 2, 0.450),
            sample(1, 10.64, 2, 0.510),
            sample(16, 10.65, 2, 0.460),
        ]

        position_times = {row["t"] for row in export.export_positions(telemetry)}
        interval_times = {row["t"] for row in export.export_intervals(telemetry)}

        self.assertEqual(position_times, interval_times)
        self.assertEqual(position_times, {10.0, 10.5})


class FastF1ResultStatusTests(unittest.TestCase):
    def test_classification_status_maps_finished_and_lapped_to_not_dnf(self):
        self.assertFalse(export.is_dnf_status("Finished"))
        self.assertFalse(export.is_dnf_status("+1 Lap"))
        self.assertFalse(export.is_dnf_status("12"))

    def test_classification_status_maps_retirements_to_dnf(self):
        self.assertTrue(export.is_dnf_status("Accident"))
        self.assertTrue(export.is_dnf_status("Engine"))
        self.assertTrue(export.is_dnf_status("Retired"))

    def test_dns_and_dsq_are_distinct_from_dnf(self):
        self.assertTrue(export.is_dns_status("Did not start"))
        self.assertTrue(export.is_dsq_status("Disqualified"))
        self.assertFalse(export.is_dnf_status("Did not start"))
        self.assertFalse(export.is_dnf_status("Disqualified"))


class FastF1ScheduleResolverTests(unittest.TestCase):
    def test_resolves_saudi_arabian_metadata_against_saudi_arabia_schedule(self):
        schedule = pd.DataFrame(
            [
                {
                    "RoundNumber": 2,
                    "EventName": "Saudi Arabian Grand Prix",
                    "OfficialEventName": "FORMULA 1 STC SAUDI ARABIAN GRAND PRIX 2024",
                    "Country": "Saudi Arabia",
                    "Location": "Jeddah",
                    "EventDate": pd.Timestamp("2024-03-09T17:00:00Z"),
                }
            ]
        )

        with patch.object(export, "fastf1", FakeFastF1(schedule)):
            round_number, metadata = export.resolve_round(
                2024,
                None,
                "Saudi Arabian Grand Prix",
                "Saudi Arabia",
                "Jeddah",
                "2024-03-09T17:00:00Z",
            )

        self.assertEqual(round_number, 2)
        self.assertEqual(metadata["match_method"], "fastf1_schedule_match")

    def test_uses_session_date_when_text_metadata_is_not_confident(self):
        schedule = pd.DataFrame(
            [
                {
                    "RoundNumber": 5,
                    "EventName": "Known Event",
                    "OfficialEventName": "Known Event",
                    "Country": "Known",
                    "Location": "Known",
                    "EventDate": pd.Timestamp("2024-05-05T12:00:00Z"),
                }
            ]
        )

        with patch.object(export, "fastf1", FakeFastF1(schedule)):
            round_number, metadata = export.resolve_round(
                2024,
                None,
                "Completely Unrelated",
                "Elsewhere",
                "Nowhere",
                "2024-05-05T14:00:00Z",
            )

        self.assertEqual(round_number, 5)
        self.assertEqual(metadata["match_method"], "fastf1_schedule_date_match")
        self.assertTrue(metadata["warnings"])


def sample(driver_number, t, lap_number, relative_distance, lap_duration=90.0):
    return {
        "driver_number": driver_number,
        "t": t,
        "lap_number": lap_number,
        "lap_duration": lap_duration,
        "relative_distance": relative_distance,
    }


class FakeFastF1:
    def __init__(self, schedule):
        self.schedule = schedule

    def get_event_schedule(self, year):
        return self.schedule


if __name__ == "__main__":
    unittest.main()
