#!/usr/bin/env python3
"""Export a FastF1 race session as Interval's historical replay bundle."""

from __future__ import annotations

import argparse
import contextlib
import inspect
import json
import math
import os
import re
import sys
from collections.abc import Iterator
from datetime import datetime, timezone
from typing import Any
from urllib.parse import urlsplit

import numpy as np
import pandas as pd

try:
    import fastf1
except ModuleNotFoundError:
    fastf1 = None

try:
    import requests
except ModuleNotFoundError:
    requests = None

POSITION_SAMPLE_SECONDS = 0.5
F1_USERNAME_ENV = "F1_USERNAME"
F1_PASSWORD_ENV = "F1_PASSWORD"
SIGN_IN_HINT = (
    "Sign in with the Formula 1 account linked to your F1 TV subscription in Settings, "
    f"or set {F1_USERNAME_ENV} and {F1_PASSWORD_ENV}."
)
MISSING_CREDENTIALS_MESSAGE = (
    "No Formula 1 account is configured. Enter the email and password of the Formula 1 "
    f"account linked to your F1 TV subscription, or set {F1_USERNAME_ENV} and {F1_PASSWORD_ENV}."
)


class ExportError(RuntimeError):
    pass


class SessionTokenStore:
    def load(self) -> None:
        return None

    def save(self, token: Any) -> None:
        return None

    def delete(self) -> None:
        return None


class LivetimingAuthWatch:
    def __init__(self, hosts: frozenset[str]) -> None:
        self.hosts = hosts
        self.rejections: list[tuple[int, str]] = []

    def record(self, response: Any) -> None:
        if response.status_code in (401, 403) and urlsplit(response.url).netloc in self.hosts:
            self.rejections.append((response.status_code, response.url))

    def raise_if_rejected(self, authenticated: bool) -> None:
        if not self.rejections:
            return
        status, url = self.rejections[0]
        hint = (
            "The signed-in Formula 1 account was not accepted for this data; check that its "
            "F1 TV subscription is active and try again."
            if authenticated
            else f"Historical timing data requires an F1 TV subscription. {SIGN_IN_HINT}"
        )
        raise ExportError(f"The F1 archive rejected the request for {url} (HTTP {status}). {hint}")


def f1account() -> Any:
    if fastf1 is None:
        raise RuntimeError("fastf1 is required for Formula 1 account authentication")
    return fastf1.internals.f1account


def f1_credentials_configured() -> bool:
    return bool(os.environ.get(F1_USERNAME_ENV)) and bool(os.environ.get(F1_PASSWORD_ENV))


def configure_auth() -> None:
    account = f1account()
    fastf1.set_auth_method("password")
    account.token_manager._store = SessionTokenStore()
    account.token_manager.clear()


def archive_hosts(api: Any) -> frozenset[str]:
    return frozenset(urlsplit(url).netloc for url in (api.base_url, api.base_url_mirror))


@contextlib.contextmanager
def watch_livetiming_auth(cache: Any, hosts: frozenset[str]) -> Iterator[LivetimingAuthWatch]:
    watch = LivetimingAuthWatch(hosts)
    original = inspect.getattr_static(cache, "requests_get")
    requests_get = cache.requests_get

    def observed_get(url: Any, **kwargs: Any) -> Any:
        response = requests_get(url, **kwargs)
        watch.record(response)
        return response

    cache.requests_get = observed_get
    try:
        yield watch
    finally:
        cache.requests_get = original


def preflight_auth() -> bool:
    configure_auth()
    if not f1_credentials_configured():
        return False
    try:
        f1account().get_token()
    except fastf1.exceptions.F1AuthError as exc:
        raise ExportError(f"Formula 1 account sign-in failed: {exc}") from exc
    return True


def classify_auth_error(exc: Exception) -> str:
    cause = exc.__cause__
    if requests is not None and isinstance(cause, requests.RequestException):
        return "invalid" if isinstance(cause, requests.HTTPError) else "unreachable"
    message = str(exc).lower()
    if "subscription" in message:
        return "no_subscription"
    if "username and password" in message:
        return "unauthorized"
    return "invalid"


def auth_check() -> dict[str, str]:
    configure_auth()
    if not f1_credentials_configured():
        return {"result": "missing", "message": MISSING_CREDENTIALS_MESSAGE}
    try:
        token = f1account().get_token(force_renew=True, method="password")
    except fastf1.exceptions.F1AuthError as exc:
        return {"result": classify_auth_error(exc), "message": str(exc)}
    try:
        claims = token.claims
    except Exception:
        claims = {}
    status = claims.get("SubscriptionStatus") or "unknown"
    product = claims.get("SubscribedProduct") or "F1 TV"
    return {
        "result": "ok",
        "message": f"Signed in to Formula 1. Subscription status: {status} ({product}).",
    }


def require_lap_data(session: Any, authenticated: bool) -> None:
    try:
        laps = session.laps
    except Exception:
        laps = None
    if laps is not None and not laps.empty:
        return
    hint = (
        "The session may not be available in the F1 archive yet."
        if authenticated
        else f"Historical timing data requires an F1 TV subscription. {SIGN_IN_HINT}"
    )
    raise ExportError(f"FastF1 returned no lap timing data for this session. {hint}")


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


def normalize_match_text(value: Any) -> str:
    if value is None or pd.isna(value):
        return ""
    text = str(value).lower()
    text = re.sub(r"\b(grand prix|gp|formula 1|fia|world championship|202[0-9])\b", " ", text)
    text = re.sub(r"[^a-z0-9]+", " ", text)
    text = " ".join(text.split())
    aliases = {
        "saudi arabian": "saudi arabia",
        "emilia romagna": "imola",
        "great britain": "british",
        "united states": "usa",
        "las vegas": "vegas",
    }
    return aliases.get(text, text)


def match_score(needle: str, haystack: str) -> int:
    if not needle or not haystack:
        return 0
    if needle == haystack:
        return 12
    if needle in haystack or haystack in needle:
        return 8
    needle_tokens = set(needle.split())
    haystack_tokens = set(haystack.split())
    if not needle_tokens or not haystack_tokens:
        return 0
    overlap = len(needle_tokens & haystack_tokens)
    if overlap == len(needle_tokens):
        return 6
    if overlap > 0:
        return overlap
    return 0


def resolve_round(
    year: int,
    round_number: int | None,
    event_name: str | None,
    country: str | None,
    location: str | None,
    session_start: str | None = None,
) -> tuple[int, dict[str, Any]]:
    if round_number is not None:
        return round_number, {
            "round": round_number,
            "event_name": None,
            "match_method": "provided_round",
            "match_confidence": 1.0,
            "warnings": [],
        }

    if fastf1 is None:
        raise RuntimeError("fastf1 is required to resolve FastF1 event schedules")
    schedule = fastf1.get_event_schedule(year)
    event_query = normalize_match_text(event_name)
    country_query = normalize_match_text(country)
    location_query = normalize_match_text(location)
    session_start_dt = parse_datetime(session_start)
    candidates: list[tuple[int, dict[str, Any]]] = []

    for _, row in schedule.iterrows():
        event_fields = [
            normalize_match_text(row.get("EventName")),
            normalize_match_text(row.get("OfficialEventName")),
            normalize_match_text(row.get("EventFormat")),
        ]
        country_field = normalize_match_text(row.get("Country"))
        location_field = normalize_match_text(row.get("Location"))
        text_score = max(match_score(event_query, field) for field in event_fields)
        text_score += match_score(country_query, country_field)
        text_score += match_score(location_query, location_field)
        score = text_score
        date_delta_days = schedule_date_delta_days(row, session_start_dt)
        if date_delta_days is not None and date_delta_days <= 7:
            score += max(1, 8 - int(date_delta_days))
        if score <= 0:
            continue
        candidates.append(
            (
                score,
                {
                    "round": clean_int(row.get("RoundNumber")),
                    "event_name": str(row.get("EventName") or ""),
                    "official_event_name": str(row.get("OfficialEventName") or ""),
                    "country": str(row.get("Country") or ""),
                    "location": str(row.get("Location") or ""),
                    "date_delta_days": date_delta_days,
                    "text_score": text_score,
                    "score": score,
                },
            )
        )

    candidates = [
        (score, candidate)
        for score, candidate in candidates
        if candidate["round"] is not None
    ]
    candidates.sort(key=lambda item: item[0], reverse=True)
    if not candidates or candidates[0][0] < 8:
        date_candidate = nearest_date_candidate(schedule, session_start_dt)
        if date_candidate is not None:
            return date_candidate
        raise ValueError(
            "could not resolve FastF1 event from OpenF1 meeting metadata "
            f"(event={event_name!r}, country={country!r}, location={location!r})"
        )
    if len(candidates) > 1 and candidates[0][0] == candidates[1][0]:
        names = ", ".join(candidate["event_name"] for _, candidate in candidates[:2])
        raise ValueError(f"ambiguous FastF1 event match: {names}")

    score, candidate = candidates[0]
    confidence = min(1.0, score / 24.0)
    warnings = []
    match_method = (
        "fastf1_schedule_date_match"
        if candidate.get("text_score", 0) < 8 and candidate.get("date_delta_days") is not None
        else "fastf1_schedule_match"
    )
    if confidence < 0.75 or match_method == "fastf1_schedule_date_match":
        warnings.append(
            "FastF1 schedule resolver used an approximate event/date match "
            f"for {event_name or location or country}."
        )
    return int(candidate["round"]), {
        "round": int(candidate["round"]),
        "event_name": candidate["event_name"],
        "official_event_name": candidate["official_event_name"],
        "country": candidate["country"],
        "location": candidate["location"],
        "match_method": match_method,
        "match_confidence": round(confidence, 3),
        "warnings": warnings,
    }


def nearest_date_candidate(
    schedule: pd.DataFrame,
    session_start_dt: datetime | None,
) -> tuple[int, dict[str, Any]] | None:
    if session_start_dt is None:
        return None
    candidates = []
    for _, row in schedule.iterrows():
        round_number = clean_int(row.get("RoundNumber"))
        delta = schedule_date_delta_days(row, session_start_dt)
        if round_number is not None and delta is not None:
            candidates.append((delta, row))
    if not candidates:
        return None
    candidates.sort(key=lambda item: item[0])
    delta, row = candidates[0]
    if delta > 7:
        return None
    round_number = int(clean_int(row.get("RoundNumber")))
    event_name = str(row.get("EventName") or "")
    return round_number, {
        "round": round_number,
        "event_name": event_name,
        "official_event_name": str(row.get("OfficialEventName") or ""),
        "country": str(row.get("Country") or ""),
        "location": str(row.get("Location") or ""),
        "match_method": "fastf1_schedule_date_match",
        "match_confidence": round(max(0.5, 1.0 - (delta / 14.0)), 3),
        "warnings": [
            "FastF1 schedule resolver used session date because event metadata did not match "
            f"confidently for {event_name}."
        ],
    }


def schedule_date_delta_days(row: Any, session_start_dt: datetime | None) -> float | None:
    if session_start_dt is None:
        return None
    values = [
        row.get("EventDate"),
        row.get("Session1Date"),
        row.get("Session2Date"),
        row.get("Session3Date"),
        row.get("Session4Date"),
        row.get("Session5Date"),
    ]
    deltas = []
    for value in values:
        candidate = parse_datetime(value)
        if candidate is not None:
            deltas.append(abs((candidate - session_start_dt).total_seconds()) / 86400)
    return min(deltas) if deltas else None


def parse_datetime(value: Any) -> datetime | None:
    if value is None or pd.isna(value):
        return None
    if isinstance(value, pd.Timestamp):
        value = value.to_pydatetime()
    if isinstance(value, datetime):
        dt = value
    else:
        text = str(value).strip()
        if not text:
            return None
        if text.endswith("Z"):
            text = f"{text[:-1]}+00:00"
        try:
            dt = datetime.fromisoformat(text)
        except ValueError:
            return None
    if dt.tzinfo is None:
        return dt.replace(tzinfo=timezone.utc)
    return dt.astimezone(timezone.utc)


def load_session(
    year: int,
    round_number: int,
    session_code: str,
    cache_dir: str,
    authenticated: bool = False,
):
    if fastf1 is None:
        raise RuntimeError("fastf1 is required to load historical replay sessions")
    os.makedirs(cache_dir, exist_ok=True)
    fastf1.Cache.enable_cache(cache_dir)
    session = fastf1.get_session(year, round_number, session_code)
    with watch_livetiming_auth(fastf1.Cache, archive_hosts(fastf1._api)) as watch:
        try:
            session.load(telemetry=True, weather=True, messages=True)
        except TypeError:
            try:
                session.load(telemetry=True, weather=True)
            except TypeError:
                try:
                    session.load(telemetry=True)
                except TypeError:
                    session.load()
    watch.raise_if_rejected(authenticated)
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
            lap_duration = seconds(lap.get("LapTime"))
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
                        "lap_duration": lap_duration,
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
    positions = []
    for t, ranked in ranked_telemetry_frames(telemetry):
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


def export_intervals(telemetry: list[dict[str, Any]]) -> list[dict[str, Any]]:
    intervals = []
    for t, ranked in ranked_telemetry_frames(telemetry):
        if not ranked:
            continue
        leader = ranked[0]
        for index, row in enumerate(ranked):
            ahead = ranked[index - 1] if index > 0 else None
            intervals.append(
                {
                    "t": round(t, 3),
                    "driver_number": row["driver_number"],
                    "gap_to_leader": None if index == 0 else format_progress_gap(leader, row),
                    "interval": None if ahead is None else format_progress_gap(ahead, row),
                }
            )
    intervals.sort(key=lambda row: (row["t"], row["driver_number"]))
    return intervals


def ranked_telemetry_frames(telemetry: list[dict[str, Any]]) -> list[tuple[float, list[dict[str, Any]]]]:
    by_time: dict[float, list[dict[str, Any]]] = {}
    for row in telemetry:
        if progress(row) is None:
            continue
        t = round(float(row["t"]) / POSITION_SAMPLE_SECONDS) * POSITION_SAMPLE_SECONDS
        by_time.setdefault(t, []).append(row)

    frames = []
    for t, rows in by_time.items():
        best_by_driver = {}
        for row in rows:
            driver = row["driver_number"]
            if driver not in best_by_driver or row["t"] > best_by_driver[driver]["t"]:
                best_by_driver[driver] = row
        ranked = sorted(best_by_driver.values(), key=lambda row: progress(row) or 0.0, reverse=True)
        frames.append((t, ranked))
    frames.sort(key=lambda item: item[0])
    return frames


def progress(row: dict[str, Any]) -> float | None:
    lap_number = row.get("lap_number")
    relative_distance = row.get("relative_distance")
    if lap_number is None or relative_distance is None:
        return None
    return int(lap_number) + float(relative_distance)


def format_progress_gap(ahead: dict[str, Any], behind: dict[str, Any]) -> str | None:
    ahead_progress = progress(ahead)
    behind_progress = progress(behind)
    if ahead_progress is None or behind_progress is None:
        return None

    progress_gap = max(0.0, ahead_progress - behind_progress)
    lap_gap = int(math.floor(progress_gap))
    if lap_gap >= 1:
        return f"+{lap_gap} LAP" if lap_gap == 1 else f"+{lap_gap} LAPS"

    seconds_gap = estimate_seconds_gap(ahead, behind, progress_gap)
    if seconds_gap is None:
        return None
    return f"+{seconds_gap:.3f}"


def estimate_seconds_gap(ahead: dict[str, Any], behind: dict[str, Any], progress_gap: float) -> float | None:
    if progress_gap <= 0:
        return 0.0
    lap_number = int(behind.get("lap_number") or 0)
    lap_duration = clean_float(behind.get("lap_duration"))
    if lap_duration is not None and lap_duration > 0:
        return progress_gap * lap_duration

    speed = clean_float(behind.get("speed"))
    if speed is not None and speed > 0:
        ahead_progress = progress(ahead)
        behind_progress = progress(behind)
        if ahead_progress is not None and behind_progress is not None and ahead_progress > behind_progress:
            return progress_gap * 90.0

    return progress_gap * 90.0 if lap_number > 0 else None


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
                "dnf": is_dnf_status(row.get("Status")),
                "dns": is_dns_status(row.get("Status")),
                "dsq": is_dsq_status(row.get("Status")),
                "status": clean_string(row.get("Status")),
            }
        )
    return rows


def is_dnf_status(value: Any) -> bool:
    status = normalize_status(value)
    if not status or is_dns_status(status) or is_dsq_status(status):
        return False
    if status in {"finished", "classified"}:
        return False
    if status.startswith("+") or status.isdigit():
        return False
    return any(
        token in status
        for token in (
            "accident",
            "collision",
            "damage",
            "retired",
            "engine",
            "gearbox",
            "brake",
            "hydraulics",
            "electrical",
            "power unit",
            "powerunit",
            "overheating",
            "suspension",
            "puncture",
            "oil",
            "fuel",
            "water leak",
            "spun off",
            "withdrawn",
        )
    )


def is_dns_status(value: Any) -> bool:
    status = normalize_status(value)
    return status in {"dns", "did not start", "not started", "didnt start"}


def is_dsq_status(value: Any) -> bool:
    status = normalize_status(value)
    return status in {"dsq", "disqualified", "excluded"} or "disqualified" in status


def normalize_status(value: Any) -> str:
    text = clean_string(value)
    return text.lower().replace("_", " ").replace("-", " ").strip() if text else ""


def clean_string(value: Any) -> str | None:
    if value is None or pd.isna(value):
        return None
    text = str(value).strip()
    return text or None


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


def export_bundle(
    year: int,
    round_number: int | None,
    session_code: str,
    session_key: int,
    cache_dir: str,
    event_name: str | None,
    country: str | None,
    location: str | None,
    session_start: str | None,
    resolver_method: str,
) -> dict[str, Any]:
    authenticated = preflight_auth()
    resolved_round, resolver = resolve_round(
        year, round_number, event_name, country, location, session_start
    )
    if resolver_method == "curated_override":
        resolver["match_method"] = "curated_override"
        resolver["match_confidence"] = 1.0
    session = load_session(year, resolved_round, session_code, cache_dir, authenticated)
    require_lap_data(session, authenticated)
    laps, stints, pits = export_laps_and_stints(session)
    telemetry = export_telemetry(session)
    warnings = resolver.get("warnings", [])
    intervals = export_intervals(telemetry)
    if not telemetry:
        warnings.append("FastF1 intervals could not be derived because telemetry is missing.")
    elif not intervals:
        warnings.append(
            "FastF1 intervals could not be derived because telemetry is missing relative-distance data."
        )
    sections = {
        "drivers": export_drivers(session),
        "laps": laps,
        "telemetry": telemetry,
        "positions": export_positions(telemetry),
        "intervals": intervals,
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
            "round": resolved_round,
            "session": session_code,
            "session_key": session_key,
            "event_query": {
                "event_name": event_name,
                "country": country,
                "location": location,
                "session_start": session_start,
            },
            "resolver": resolver,
            "warnings": warnings,
        },
        "sections": sections,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--year", type=int)
    parser.add_argument("--round", type=int)
    parser.add_argument("--session")
    parser.add_argument("--session-key", type=int)
    parser.add_argument("--event-name")
    parser.add_argument("--country")
    parser.add_argument("--location")
    parser.add_argument("--session-start")
    parser.add_argument("--resolver-method", default="provided_round")
    parser.add_argument("--cache-dir", default="cache/fastf1")
    parser.add_argument("--auth-check", action="store_true")
    parser.add_argument("--output")
    args = parser.parse_args(argv)

    if args.auth_check:
        result = auth_check()
        print(json.dumps(result))
        return 0 if result["result"] == "ok" else 1

    required = {
        "--year": args.year,
        "--session": args.session,
        "--session-key": args.session_key,
        "--output": args.output,
    }
    missing = [name for name, value in required.items() if value is None]
    if missing:
        parser.error(f"the following arguments are required: {', '.join(missing)}")

    try:
        bundle = export_bundle(
            args.year,
            args.round,
            args.session,
            args.session_key,
            args.cache_dir,
            args.event_name,
            args.country,
            args.location,
            args.session_start,
            args.resolver_method,
        )
    except ExportError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1
    os.makedirs(os.path.dirname(args.output), exist_ok=True)
    with open(args.output, "w", encoding="utf-8") as handle:
        json.dump(bundle, handle, separators=(",", ":"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
