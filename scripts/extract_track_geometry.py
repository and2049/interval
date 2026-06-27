#!/usr/bin/env python3
"""Extract track geometry from FastF1 telemetry and save as a JSON asset.

Usage:
    python scripts/extract_track_geometry.py --year 2024 --gp Bahrain --session R --session-key 9472
    python scripts/extract_track_geometry.py --year 2024 --gp Bahrain --session R --session-key 9472 --output backend/assets/tracks/9472.json

The output JSON contains:
  - session_key: int
  - centerline: [[x, y], ...]  (dense points from fastest lap telemetry)
  - rotation_deg: float        (circuit rotation from FastF1)
  - circuit_length: float      (meters, from telemetry Distance)
  - total_laps: int            (from session data)

The Rust backend loads this asset at ingest time, densifies/normalizes to
240 points, and computes inner/outer edges with the configured track width.
"""

import argparse
import json
import os
import sys

import fastf1
import numpy as np


def extract_track_geometry(year, gp, session_name, session_key):
    fastf1.Cache.enable_cache("cache")

    print(f"Loading FastF1 session: {year} {gp} {session_name}...")
    session = fastf1.get_session(year, gp, session_name)
    session.load()

    fastest_lap = session.laps.pick_fastest()
    if fastest_lap is None or fastest_lap.empty:
        print("ERROR: No fastest lap found", file=sys.stderr)
        sys.exit(1)

    print(f"Fastest lap: {fastest_lap['LapTime']} by driver {fastest_lap['Driver']}")
    telemetry = fastest_lap.get_telemetry()
    if telemetry is None or telemetry.empty:
        print("ERROR: No telemetry data for fastest lap", file=sys.stderr)
        sys.exit(1)

    xs = telemetry["X"].to_numpy().astype(float)
    ys = telemetry["Y"].to_numpy().astype(float)
    dists = (
        telemetry["Distance"].to_numpy().astype(float)
        if "Distance" in telemetry
        else None
    )

    if len(xs) < 20:
        print(
            f"ERROR: Only {len(xs)} telemetry points, need at least 20", file=sys.stderr
        )
        sys.exit(1)

    if dists is not None and len(dists) > 0 and dists[-1] > 0:
        xy_length = float(np.sqrt(np.diff(xs) ** 2 + np.diff(ys) ** 2).sum())
        scale = float(dists[-1]) / xy_length if xy_length > 0 else 1.0
        print(
            f"Coordinate scale: {scale:.4f} (XY length={xy_length:.1f}, telemetry distance={dists[-1]:.1f})"
        )
        xs = xs * scale
        ys = ys * scale
        circuit_length = float(dists[-1])
    else:
        diffs = np.diff(xs) ** 2 + np.diff(ys) ** 2
        circuit_length = float(np.sqrt(diffs).sum())

    try:
        circuit_info = session.get_circuit_info()
        rotation_deg = float(circuit_info.rotation) if circuit_info.rotation else 0.0
    except Exception:
        rotation_deg = 0.0

    total_laps = int(session.laps["LapNumber"].max()) if not session.laps.empty else 0

    centerline = [[round(float(x), 2), round(float(y), 2)] for x, y in zip(xs, ys)]

    print(f"Centerline points: {len(centerline)}")
    print(f"Circuit length: {circuit_length:.1f} m")
    print(f"Rotation: {rotation_deg:.1f} deg")
    print(f"Total laps: {total_laps}")

    return {
        "session_key": session_key,
        "centerline": centerline,
        "rotation_deg": rotation_deg,
        "circuit_length": round(circuit_length, 2),
        "total_laps": total_laps,
    }


def main():
    parser = argparse.ArgumentParser(description="Extract track geometry from FastF1")
    parser.add_argument("--year", type=int, required=True, help="Race year (e.g. 2024)")
    parser.add_argument(
        "--gp", type=str, required=True, help="Grand Prix name (e.g. Bahrain)"
    )
    parser.add_argument(
        "--session",
        type=str,
        required=True,
        help="Session code (R=Race, Q=Qualifying, FP1, etc.)",
    )
    parser.add_argument(
        "--session-key",
        type=int,
        required=True,
        help="OpenF1 session_key for output filename",
    )
    parser.add_argument(
        "--output",
        type=str,
        default=None,
        help="Output path (default: backend/assets/tracks/{session_key}.json)",
    )
    args = parser.parse_args()

    output_path = args.output or os.path.join(
        "backend", "assets", "tracks", f"{args.session_key}.json"
    )

    geometry = extract_track_geometry(
        args.year, args.gp, args.session, args.session_key
    )

    os.makedirs(os.path.dirname(output_path), exist_ok=True)
    with open(output_path, "w") as f:
        json.dump(geometry, f, indent=2)

    print(f"Saved to {output_path}")


if __name__ == "__main__":
    main()
