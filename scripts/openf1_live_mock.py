#!/usr/bin/env python3
"""Tiny OpenF1-compatible live mock used by smoke-live.ps1."""

from __future__ import annotations

import argparse
import json
import math
from datetime import datetime, timedelta, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse


SESSION_KEY = 88001
MEETING_KEY = 88000
START = datetime.now(timezone.utc) - timedelta(minutes=10)
END = START + timedelta(minutes=100)
MISSING_ENDPOINTS: set[str] = set()
FAILED_ENDPOINTS: set[str] = set()
MALFORMED_AFTER_FIRST_ENDPOINTS: set[str] = set()
CALLS: dict[str, int] = {}
OMIT_SESSION_END = False
SESSION_NAME = "Race"
SESSION_TYPE = "Race"


def iso(value: datetime) -> str:
    return value.astimezone(timezone.utc).isoformat().replace("+00:00", "Z")


def payload_for(endpoint: str):
    CALLS[endpoint] = CALLS.get(endpoint, 0) + 1
    now = datetime.now(timezone.utc)
    recent = now - timedelta(milliseconds=250)

    if endpoint == "meetings":
        return [
            {
                "meeting_key": MEETING_KEY,
                "meeting_name": "Test Live Grand Prix",
                "country_name": "Testland",
                "location": "Test Circuit",
                "year": now.year,
            }
        ]
    if endpoint == "sessions":
        session = {
            "session_key": SESSION_KEY,
            "meeting_key": MEETING_KEY,
            "session_name": SESSION_NAME,
            "session_type": SESSION_TYPE,
            "date_start": iso(START),
            "year": now.year,
        }
        if not OMIT_SESSION_END:
            session["date_end"] = iso(END)
        return [session]
    if endpoint in FAILED_ENDPOINTS:
        raise RuntimeError(f"mocked OpenF1 failure for {endpoint}")
    if endpoint in MISSING_ENDPOINTS:
        return []
    if endpoint == "drivers":
        return [
            {
                "driver_number": 1,
                "full_name": "Max Verstappen",
                "name_acronym": "VER",
                "team_colour": "3671C6",
                "team_name": "Red Bull Racing",
            },
            {
                "driver_number": 16,
                "full_name": "Charles Leclerc",
                "name_acronym": "LEC",
                "team_colour": "E80020",
                "team_name": "Ferrari",
            },
        ]
    if endpoint == "laps":
        return [
            {
                "driver_number": 1,
                "lap_number": 6,
                "date_start": iso(START + timedelta(minutes=8)),
                "lap_duration": 90.120,
                "duration_sector_1": 29.100,
                "duration_sector_2": 31.020,
                "duration_sector_3": 30.000,
            },
            {
                "driver_number": 16,
                "lap_number": 6,
                "date_start": iso(START + timedelta(minutes=8, seconds=2)),
                "lap_duration": 90.750,
                "duration_sector_1": 29.280,
                "duration_sector_2": 31.190,
                "duration_sector_3": 30.280,
            },
        ]
    if endpoint == "intervals":
        return [
            {
                "date": iso(recent),
                "driver_number": 1,
                "gap_to_leader": None,
                "interval": None,
            },
            {
                "date": iso(recent),
                "driver_number": 16,
                "gap_to_leader": 2.431,
                "interval": 2.431,
            },
        ]
    if endpoint == "position":
        return [
            {"date": iso(recent), "driver_number": 1, "position": 1},
            {"date": iso(recent), "driver_number": 16, "position": 2},
        ]
    if endpoint == "location":
        if endpoint in MALFORMED_AFTER_FIRST_ENDPOINTS and CALLS[endpoint] > 1:
            return [
                {
                    "date": iso(recent),
                    "driver_number": 1,
                    "x": "not-a-number",
                    "y": 650.0,
                    "z": 0.0,
                }
            ]
        points = []
        for index in range(40):
            theta = (2.0 * math.pi * index) / 40.0
            points.append(
                {
                    "date": iso(recent),
                    "driver_number": 1,
                    "x": 1000.0 * math.cos(theta),
                    "y": 650.0 * math.sin(theta),
                    "z": 0.0,
                }
            )
        points.append(
            {
                "date": iso(recent),
                "driver_number": 16,
                "x": 1000.0 * math.cos(0.2),
                "y": 650.0 * math.sin(0.2),
                "z": 0.0,
            }
        )
        return points
    if endpoint == "pit":
        return []
    if endpoint == "race_control":
        return [
            {
                "date": iso(recent),
                "category": "Flag",
                "message": "GREEN LIGHT",
                "flag": "GREEN",
                "scope": "Track",
            }
        ]
    if endpoint == "stints":
        return [
            {
                "driver_number": 1,
                "stint_number": 1,
                "compound": "MEDIUM",
                "lap_start": 1,
                "lap_end": None,
                "tyre_age_at_start": 0,
            },
            {
                "driver_number": 16,
                "stint_number": 1,
                "compound": "HARD",
                "lap_start": 1,
                "lap_end": None,
                "tyre_age_at_start": 0,
            },
        ]
    if endpoint == "weather":
        return [
            {
                "date": iso(recent),
                "air_temperature": 22.0,
                "track_temperature": 34.0,
                "humidity": 45.0,
                "rainfall": 0.0,
                "wind_direction": 180,
                "wind_speed": 2.5,
            }
        ]
    if endpoint == "session_result":
        return [
            {"driver_number": 1, "position": 1, "dnf": False, "dns": False, "dsq": False},
            {"driver_number": 16, "position": 2, "dnf": False, "dns": False, "dsq": False},
        ]
    return None


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):  # noqa: N802 - http.server API
        parsed = urlparse(self.path)
        endpoint = parsed.path.removeprefix("/v1/").strip("/")
        try:
            payload = payload_for(endpoint)
        except RuntimeError as error:
            body = json.dumps({"error": str(error)}).encode("utf-8")
            self.send_response(500)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        if payload is None:
            self.send_response(404)
            self.end_headers()
            return

        body = json.dumps(payload).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format, *args):  # noqa: A002,N802 - http.server API
        return


def main() -> int:
    global START, END, OMIT_SESSION_END, SESSION_NAME, SESSION_TYPE

    parser = argparse.ArgumentParser()
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=45101)
    parser.add_argument("--start-offset-minutes", type=float, default=-10.0)
    parser.add_argument("--duration-minutes", type=float, default=100.0)
    parser.add_argument("--session-name", default="Race")
    parser.add_argument("--session-type", default="Race")
    parser.add_argument("--omit-session-end", action="store_true")
    parser.add_argument("--missing-endpoints", default="")
    parser.add_argument("--failed-endpoints", default="")
    parser.add_argument("--malformed-after-first-endpoints", default="")
    args = parser.parse_args()

    START = datetime.now(timezone.utc) + timedelta(minutes=args.start_offset_minutes)
    END = START + timedelta(minutes=args.duration_minutes)
    SESSION_NAME = args.session_name
    SESSION_TYPE = args.session_type
    OMIT_SESSION_END = args.omit_session_end
    MISSING_ENDPOINTS.update(
        endpoint.strip()
        for endpoint in args.missing_endpoints.split(",")
        if endpoint.strip()
    )
    FAILED_ENDPOINTS.update(
        endpoint.strip()
        for endpoint in args.failed_endpoints.split(",")
        if endpoint.strip()
    )
    MALFORMED_AFTER_FIRST_ENDPOINTS.update(
        endpoint.strip()
        for endpoint in args.malformed_after_first_endpoints.split(",")
        if endpoint.strip()
    )

    server = ThreadingHTTPServer((args.host, args.port), Handler)
    print(f"OpenF1 live mock listening on http://{args.host}:{args.port}/v1/", flush=True)
    server.serve_forever()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
