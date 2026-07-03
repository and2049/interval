# F1 Second Screen Dashboard — Project Overview

## Overview

This project is an interactive Formula 1 second-screen dashboard designed for fans who are watching a race broadcast and want richer timing, strategy, and telemetry context on another screen. The product should feel inspired by real engineer-style displays while remaining understandable and usable for non-engineers.[cite:6]

The build is **replay-first and live-capable**: historical sessions provide deterministic development, testing, demos, and portfolio presentation, while live race mode uses OpenF1 behind the same backend-owned snapshot contract. Historical replay now uses FastF1 as the primary telemetry source, with OpenF1 still used for discovery and live-race polling.[cite:6][cite:20]

[image:1]

The engineer display references suggest a dense, information-rich layout with a timing tower, lap and stint matrices, a track map, event messages, and session timelines. That visual language should influence the information architecture, color coding, and panel structure of the product.[cite:6]

[image:2]

## End Goal

The end goal is to build a polished, resume-worthy and genuinely useful F1 dashboard that lets a fan:

- Replay a full race or session with synchronized data views.
- Understand race context faster than the TV broadcast alone allows.
- Compare drivers, stints, pace, and incidents in a single interface.
- Switch between historical replay, live simulation, and live race mode without changing dashboard panels.

Long term, the project should be credible as both a fan product and a strong software engineering portfolio piece. It should demonstrate real-time systems thinking, strong domain modeling, efficient frontend rendering, clear UX for data-dense applications, and an architecture that supports both historical replay and live ingestion.[cite:6][cite:20]

## Product Vision

The product should be positioned as a **race engineer dashboard for fans**, not just a stats website. The value is not only showing raw data, but turning race data into fast situational awareness: who is gaining, who is vulnerable after pit stops, where tyre life is dropping, and what incidents or strategy pivots matter right now.

The first release should prioritize depth over breadth. A smaller set of tightly integrated panels is more valuable than a wide collection of disconnected widgets.

## Target Users

### Primary users

- Dedicated F1 fans watching a race live or on replay.
- Technical sports fans who enjoy timing, strategy, and telemetry.
- Portfolio reviewers or recruiters evaluating a real-time, data-heavy project.

### Secondary users

- Casual fans who want a cleaner race-view companion.
- Builders or open-source contributors interested in sports dashboards.

## Core User Experience

A user opens the app and either joins an active live race/sprint when available or selects a historical season, event, and session. Historical mode supports pause, scrub, speed, and notable event context. Live mode disables replay controls and lets backend-streamed snapshots drive the dashboard clock while preserving the same timing, map, race-control, weather, and stint panels.

The interface should be optimized for **quick scanning**. Users should be able to glance at the timing tower, strategy panel, and track map and immediately answer questions such as:

- Who is under pressure?
- Who is on the fastest long-run pace?
- Who is due for a pit stop?
- What incident just changed the race?
- Which drivers are net-ahead after stops?

## MVP Scope

### Included in MVP

- Historical session browser using OpenF1 discovery and FastF1 replay ingest.[cite:6]
- OpenF1 live race/sprint mode enabled by default, with server-side configuration for credentials or explicit disablement.
- Deterministic live simulation from cached historical replays for local testing.
- Replay engine with play, pause, speed control, and timeline scrubbing.
- Timing tower with gaps, intervals, tyre compound, stint age, sector colors, pit status, and lap state.
- Track map with driver markers.
- Strategy or stint timeline per driver.
- Race control and weather panels, since OpenF1 exposes both categories among its available data.[cite:6]
- Driver comparison view for lap-by-lap and stint-by-stint analysis.
- Derived metrics such as recent pace trend, stint delta, and pit-window estimates.

### Explicitly out of scope for MVP

- Practice, qualifying, sprint qualifying, and other non-race/sprint replay modes.
- Real-world live validation outside active OpenF1 live session windows.
- Mobile-first or mobile-parity support for all dense panels.
- Social features, accounts, or cloud sync.
- Native desktop packaging.
- Complex predictive models that require heavy ML infrastructure.

## Technical Direction

### Selected stack

- **Backend:** Rust.
- **Frontend:** SolidJS.
- **Styling/UI:** Tailwind CSS.
- **Primary app type:** Web app.
- **Data mode:** Replay-first, live-capable.

This stack is intended to balance performance, type safety, and a lightweight client. SolidJS is a good fit for a dashboard with many small, frequently updating UI regions, while Rust supports strong domain modeling, efficient replay processing, and live-ingestion services.

### Architectural principles

- Keep the frontend thin; most replay logic and data shaping should live in the backend.
- Separate raw data ingestion from normalized internal domain models.
- Treat replay mode, live simulation, and OpenF1 live mode as inputs into the same application model.
- Optimize for incremental rendering and fast screen updates.
- Prefer simple deployable web architecture before exploring desktop or TUI variants.

## Proposed System Structure

### High-level components

- `frontend/` — SolidJS web app and UI components.
- `backend/` — Rust services for ingestion, replay, APIs, and analytics.
- `shared/` — API contracts, event names, and schema definitions.
- `docs/` — planning docs, architecture notes, wireframes, and contributor guidance.
- `infra/` — deployment and local-dev infrastructure.

### Backend modules

- `connectors/fastf1_historical` — backend-invoked Python extraction for historical race/sprint replay bundles.
- `connectors/openf1_historical` — discovery and legacy historical fallback.[cite:6]
- `connectors/openf1_live` — live adapter for OpenF1 REST polling when enabled.[cite:6][cite:20]
- `domain/` — core entities such as session, driver, lap, sector, stint, race-control message, and weather sample.
- `replay/` — timeline engine, seek logic, speed control, and event synchronization.
- `analytics/` — derived metrics and strategy calculations.
- `api/` — backend API surface for the frontend.
- `storage/` — local caching and persistence.

### Frontend modules

- `routes/` — session selection, dashboard, compare views.
- `components/` — timing tower, track map, strategy board, event feed, charts.
- `stores/` — app state and subscriptions.
- `lib/formatters/` — lap time, gaps, tire labels, colors.
- `themes/` — dashboard theme tokens and density modes.

## Core Domain Model

The project should establish consistent language early. Recommended first-class entities:

- `Season`
- `Meeting`
- `Session`
- `Driver`
- `Lap`
- `Sector`
- `Stint`
- `TrackPositionSample`
- `RaceControlMessage`
- `WeatherSample`
- `ReplayCursor`
- `DerivedMetric`

The goal is to avoid leaking raw API shapes directly into the UI. A clean internal model lets replay, live simulation, and OpenF1 live use the same dashboard contract.

## UX Principles

- Dense, but never chaotic.
- Fast to scan at a glance.
- Color should encode race meaning, not decoration.
- Tables and matrices should be the core interaction model.
- Panels should stay synchronized around a single backend-owned snapshot cursor, whether replay or live.
- Important events should be discoverable from the timeline.
- The default layout should be fan-friendly, with room for an advanced mode later.

## Success Criteria

The MVP is successful if it:

- Replays at least one complete race session smoothly.
- Makes timing, stint, and incident context easier to follow than the TV broadcast alone.
- Demonstrates clear engineering depth in architecture and implementation.
- Looks polished enough to feature prominently on a resume or portfolio.
- Supports live race/sprint updates through the same dashboard panels as replay.

## Roadmap

### Phase 1 — Foundations

- Define domain model.
- Settle repo structure.
- Build data ingestion for historical sessions.
- Normalize OpenF1 payloads into internal entities.[cite:6]

### Phase 2 — Replay MVP

- Build replay engine.
- Add timing tower, event feed, track map, and strategy timeline.
- Add session selector and timeline controls.

### Phase 3 — Analytics

- Add derived pace metrics.
- Add stint comparison.
- Add pit-window or net-position insights.

### Phase 4 — Product polish

- Improve visual hierarchy.
- Add keyboard shortcuts.
- Improve loading states and error states.
- Make the dashboard feel polished and demo-ready.

### Phase 5 — Live validation and hardening

- Keep OpenF1 live as a first-class dashboard mode.
- Validate against real active race windows when available.
- Improve channel degradation, reconnect behavior, and live-specific race-state semantics.[cite:20][cite:6]

## Current Decisions

1. MVP scope is race and sprint sessions.
2. Historical replay uses FastF1 telemetry cached through SQLite.
3. OpenF1 remains the discovery source and the live-race source.
4. Replay, live simulation, and OpenF1 live use the same `replay.v1` snapshot/event contract.
5. The dashboard should stay fan-dense and data-first, with compact tables, timelines, map context, and channel-quality badges.
6. Real OpenF1 live behavior still needs validation during an active race or sprint window.
