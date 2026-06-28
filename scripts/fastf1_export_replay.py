#!/usr/bin/env python3
"""Export a FastF1 race session as Interval's historical replay bundle."""

from __future__ import annotations

import argparse
import json
import math
import os
import re
import sys
from datetime import datetime, timezone
from typing import Any

import numpy as np
import pandas as pd

try:
    import fastf1
except ModuleNotFoundError:
    fastf1 = None

POSITION_SAMPLE_SECONDS = 0.5


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


