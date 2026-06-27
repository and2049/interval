#!/usr/bin/env python3
"""Export a FastF1 race session as Interval's historical replay bundle."""

from __future__ import annotations

import argparse
import json
import math
import os
import sys
from typing import Any

import fastf1
import numpy as np
import pandas as pd


def clean_float(value: Any) -> float | None:
    if value is None or pd.isna(value):
        return None
    value = float(value)
    if not math.isfinite(value):
        return None
    return value


def clean_int(value: Any) -> int | None:
    if value is None or pd.isna(value):
        return None
    return int(value)


def seconds(value: Any) -> float | None:
    if value is None or pd.isna(value):
        return None
    return clean_float(value.total_seconds())


def color_without_hash(value: Any) -> str | None:
    if value is None or pd.isna(value):
        return None
    return str(value).lstrip("#")


def load_session(year: int, round_number: int, session_code: str, cache_dir: str):
    os.makedirs(cache_dir, exist_ok=True)
    fastf1.Cache.enable_cache(cache_dir)
    session = fastf1.get_session(year, round_number, session_code)
    session.load(telemetry=True, weather=True, messages=True)
    return session


def export_drivers(session) -> list[dict[str, Any]]:
    rows = []
    for driver_number in session.drivers:
        info = session.get_driver(driver_number)
        rows.append(
            {
                "driver_number": int(driver_number),
                "code": str(info.get("Abbreviation") or driver_number),
                "full_name": str(info.get("FullName") or ""),
                "team_name": str(info.get("TeamName") or ""),
                "team_colour": color_without_hash(info.get("TeamColor")) or "7f8a99",
            }
        )
    return rows


def export_laps_and_stints(session) -> tuple[list[dict[str, Any]], list[dict[str, Any]], list[dict[str, Any]]]:
    laps = []
    stints_by_driver: dict[tuple[int, int, str, int], dict[str, Any]] = {}
    pits = []

    for _, lap in session.laps.iterlaps():
        driver_number = clean_int(lap.get("DriverNumber"))
        lap_number = clean_int(lap.get("LapNumber"))
        if driver_number is None or lap_number is None:
            continue
        t_start = seconds(lap.get("LapStartTime"))
        if t_start is None:
            continue

        compound = str(lap.get("Compound") or "UNKNOWN").upper()
        tyre_age = clean_int(lap.get("TyreLife"))
        stint_number = clean_int(lap.get("Stint")) or 0
        pit_in_t = seconds(lap.get("PitInTime"))
        pit_out_t = seconds(lap.get("PitOutTime"))

        laps.append(
            {
                "driver_number": driver_number,
                "lap_number": lap_number,
                "t_start": round(t_start, 3),
                "lap_duration": seconds(lap.get("LapTime")),
                "sector_1": seconds(lap.get("Sector1Time")),
                "sector_2": seconds(lap.get("Sector2Time")),
                "sector_3": seconds(lap.get("Sector3Time")),
                "is_pit_out_lap": pit_out_t is not None,
            }
        )

        key = (driver_number, stint_number, compound, tyre_age or 0)
        entry = stints_by_driver.setdefault(
            key,
            {
                "driver_number": driver_number,
                "stint_number": stint_number,
                "compound": compound,
                "lap_start": lap_number,
                "lap_end": lap_number,
                "tyre_age_at_start": tyre_age,
            },
        )
        entry["lap_start"] = min(entry["lap_start"], lap_number)
        entry["lap_end"] = max(entry["lap_end"], lap_number)

        if pit_in_t is not None:
            duration = None
            if pit_out_t is not None and pit_out_t >= pit_in_t:
                duration = round(pit_out_t - pit_in_t, 3)
            pits.append(
                {
                    "driver_number": driver_number,
                    "lap_number": lap_number,
                    "t": round(pit_in_t, 3),
                    "pit_duration": duration,
                }
            )

    return laps, sorted(stints_by_driver.values(), key=lambda row: (row["driver_number"], row["lap_start"])), pits


def export_telemetry(session) -> list[dict[str, Any]]:
    rows = []
    for driver_number in session.drivers:
        driver_laps = session.laps.pick_drivers(driver_number)
        if driver_laps.empty:
            continue
        for _, lap in driver_laps.iterlaps():
            lap_number = clean_int(lap.get("LapNumber"))
            try:
                telemetry = lap.get_telemetry()
            except Exception as exc:
                print(f"warning: telemetry skipped for driver {driver_number} lap {lap_number}: {exc}", file=sys.stderr)
                continue
            if telemetry is None or telemetry.empty:
                continue
            for _, sample in telemetry.iterrows():
                t = seconds(sample.get("SessionTime"))
                x = clean_float(sample.get("X"))
                y = clean_float(sample.get("Y"))
                if t is None or x is None or y is None:
                    continue
                rows.append(
                    {
                        "driver_number": int(driver_number),
                        "lap_number": lap_number,
                        "t": round(t, 3),
                        "x": round(x, 3),
                        "y": round(y, 3),
                        "z": clean_float(sample.get("Z")),
                        "relative_distance": clean_float(sample.get("RelativeDistance")),
                        "speed": clean_float(sample.get("Speed")),
                        "gear": clean_int(sample.get("nGear")),
                        "throttle": clean_float(sample.get("Throttle")),
                        "brake": clean_float(sample.get("Brake")),
                        "drs": clean_int(sample.get("DRS")),
                    }
                )
    rows.sort(key=lambda row: (row["t"], row["driver_number"]))
    return rows


def export_positions(telemetry: list[dict[str, Any]]) -> list[dict[str, Any]]:
    by_time: dict[float, list[dict[str, Any]]] = {}
    for row in telemetry:
        if row.get("relative_distance") is None or row.get("lap_number") is None:
            continue
        t = round(float(row["t"]) * 2.0) / 2.0
        by_time.setdefault(t, []).append(row)

    positions = []
    for t, rows in by_time.items():
        best_by_driver = {}
        for row in rows:
            driver = row["driver_number"]
            if driver not in best_by_driver or row["t"] > best_by_driver[driver]["t"]:
                best_by_driver[driver] = row
        ranked = sorted(
            best_by_driver.values(),
            key=lambda row: (int(row.get("lap_number") or 0) + float(row.get("relative_distance") or 0.0)),
            reverse=True,
        )
        for index, row in enumerate(ranked, start=1):
            positions.append(
                {
                    "t": round(t, 3),
                    "driver_number": row["driver_number"],
                    "position": index,
                }
            )
    positions.sort(key=lambda row: (row["t"], row["position"]))
    return positions


def export_weather(session) -> list[dict[str, Any]]:
    weather = getattr(session, "weather_data", None)
    if weather is None or weather.empty:
        return []
    rows = []
    for _, row in weather.iterrows():
        t = seconds(row.get("Time"))
        if t is None:
            continue
        rows.append(
            {
                "t": round(t, 3),
                "air_temp": clean_float(row.get("AirTemp")),
                "track_temp": clean_float(row.get("TrackTemp")),
                "humidity": clean_float(row.get("Humidity")),
                "rainfall": clean_float(row.get("Rainfall")),
                "wind_direction": clean_int(row.get("WindDirection")),
                "wind_speed": clean_float(row.get("WindSpeed")),
            }
        )
    return rows


def export_track_status(session) -> list[dict[str, Any]]:
    status = getattr(session, "track_status", None)
    if status is None or status.empty:
        return []
    rows = []
    for _, row in status.iterrows():
        t = seconds(row.get("Time"))
        if t is None:
            continue
        raw = str(row.get("Status") or "")
        flag = {
            "1": "green",
            "2": "yellow",
            "4": "safety_car",
            "5": "red",
            "6": "virtual_safety_car",
            "7": "virtual_safety_car_ending",
        }.get(raw, raw.lower() or None)
        rows.append(
            {
                "t": round(t, 3),
                "category": "track_status",
                "message": f"Track status {raw}",
                "flag": flag,
                "scope": None,
            }
        )
    return rows


def export_session_result(session) -> list[dict[str, Any]]:
    results = getattr(session, "results", None)
    if results is None or results.empty:
        return []
    rows = []
    for _, row in results.iterrows():
        driver_number = clean_int(row.get("DriverNumber"))
        if driver_number is None:
            continue
        rows.append(
            {
                "driver_number": driver_number,
                "position": clean_int(row.get("Position")),
                "dnf": False,
                "dns": False,
                "dsq": False,
            }
        )
    return rows


def export_geometry(session) -> dict[str, Any]:
    fastest = session.laps.pick_fastest()
    if fastest is None or fastest.empty:
        return {"centerline": []}
    telemetry = fastest.get_telemetry()
    if telemetry is None or telemetry.empty:
        return {"centerline": []}
    points = []
    for _, row in telemetry.iterrows():
        x = clean_float(row.get("X"))
        y = clean_float(row.get("Y"))
        if x is not None and y is not None:
            points.append([round(x, 3), round(y, 3)])
    rotation = 0.0
    try:
        info = session.get_circuit_info()
        rotation = clean_float(getattr(info, "rotation", 0.0)) or 0.0
    except Exception:
        rotation = 0.0
    return {"centerline": points, "rotation_deg": rotation}


def export_bundle(year: int, round_number: int, session_code: str, session_key: int, cache_dir: str) -> dict[str, Any]:
    session = load_session(year, round_number, session_code, cache_dir)
    laps, stints, pits = export_laps_and_stints(session)
    telemetry = export_telemetry(session)
    sections = {
        "drivers": export_drivers(session),
        "laps": laps,
        "telemetry": telemetry,
        "positions": export_positions(telemetry),
        "intervals": [],
        "stints": stints,
        "pits": pits,
        "weather": export_weather(session),
        "track_status": export_track_status(session),
        "session_result": export_session_result(session),
        "geometry": export_geometry(session),
    }
    return {
        "contract_version": "fastf1-export.v1",
        "metadata": {
            "source": "fastf1_historical",
            "year": year,
            "round": round_number,
            "session": session_code,
            "session_key": session_key,
        },
        "sections": sections,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--year", type=int, required=True)
    parser.add_argument("--round", type=int, required=True)
    parser.add_argument("--session", required=True)
    parser.add_argument("--session-key", type=int, required=True)
    parser.add_argument("--cache-dir", default="cache/fastf1")
    parser.add_argument("--output", required=True)
    args = parser.parse_args()

    bundle = export_bundle(args.year, args.round, args.session, args.session_key, args.cache_dir)
    os.makedirs(os.path.dirname(args.output), exist_ok=True)
    with open(args.output, "w", encoding="utf-8") as handle:
        json.dump(bundle, handle, separators=(",", ":"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
