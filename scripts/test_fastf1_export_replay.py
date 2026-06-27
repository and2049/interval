import unittest
from pathlib import Path
import sys

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


def sample(driver_number, t, lap_number, relative_distance, lap_duration=90.0):
    return {
        "driver_number": driver_number,
        "t": t,
        "lap_number": lap_number,
        "lap_duration": lap_duration,
        "relative_distance": relative_distance,
    }


if __name__ == "__main__":
    unittest.main()
