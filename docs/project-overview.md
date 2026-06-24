# F1 Second Screen Dashboard — Project Overview

## Overview

This project is an interactive Formula 1 second-screen dashboard designed for fans who are watching a race broadcast and want richer timing, strategy, and telemetry context on another screen. The product should feel inspired by real engineer-style displays while remaining understandable and usable for non-engineers.[cite:6]

The initial build will be **replay-first** for easier development, testing, demos, and portfolio presentation. Historical OpenF1 data is available without authentication, while live data access during active sessions is part of a paid sponsor tier and is therefore a future phase rather than an MVP dependency.[cite:6][cite:20]

[image:1]

The engineer display references suggest a dense, information-rich layout with a timing tower, lap and stint matrices, a track map, event messages, and session timelines. That visual language should influence the information architecture, color coding, and panel structure of the product.[cite:6]

[image:2]

## End Goal

The end goal is to build a polished, resume-worthy and genuinely useful F1 dashboard that lets a fan:

- Replay a full race or session with synchronized data views.
- Understand race context faster than the TV broadcast alone allows.
- Compare drivers, stints, pace, and incidents in a single interface.
- Eventually switch from historical replay mode to live race mode with minimal architectural changes.

Long term, the project should be credible as both a fan product and a strong software engineering portfolio piece. It should demonstrate real-time systems thinking, strong domain modeling, efficient frontend rendering, clear UX for data-dense applications, and an architecture that can support both historical replay and future live ingestion.[cite:6][cite:20]

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

A user opens the app, selects a season, event, session, and replay point, then watches the dashboard update in sync as the session progresses. They can pause, scrub, jump to notable events, compare drivers, inspect stints, and monitor race-control or weather context without leaving the main dashboard.

The interface should be optimized for **quick scanning**. Users should be able to glance at the timing tower, strategy panel, and track map and immediately answer questions such as:

- Who is under pressure?
- Who is on the fastest long-run pace?
- Who is due for a pit stop?
- What incident just changed the race?
- Which drivers are net-ahead after stops?

## MVP Scope

### Included in MVP

- Historical session browser using OpenF1 data.[cite:6]
- Replay engine with play, pause, speed control, and timeline scrubbing.
- Timing tower with gaps, intervals, tyre compound, stint age, sector colors, pit status, and lap state.
- Track map with driver markers.
- Strategy or stint timeline per driver.
- Race control and weather panels, since OpenF1 exposes both categories among its available data.[cite:6]
- Driver comparison view for lap-by-lap and stint-by-stint analysis.
- Derived metrics such as recent pace trend, stint delta, and pit-window estimates.

### Explicitly out of scope for MVP

- Live session support.
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
- **Data mode:** Replay-first, live-ready.

This stack is intended to balance performance, type safety, and a lightweight client. SolidJS is a good fit for a dashboard with many small, frequently updating UI regions, while Rust supports strong domain modeling, efficient replay processing, and future live-ingestion services.

### Architectural principles

- Keep the frontend thin; most replay logic and data shaping should live in the backend.
- Separate raw data ingestion from normalized internal domain models.
- Treat replay mode and future live mode as two inputs into the same application model.
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

- `connectors/openf1_historical` — fetch and normalize historical data from OpenF1.[cite:6]
- `connectors/openf1_live` — future live adapter for sponsor-tier access using REST/WebSocket/MQTT when enabled.[cite:6][cite:20]
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

The goal is to avoid leaking raw API shapes directly into the UI. A clean internal model will make both replay mode and future live mode easier to support.

## UX Principles

- Dense, but never chaotic.
- Fast to scan at a glance.
- Color should encode race meaning, not decoration.
- Tables and matrices should be the core interaction model.
- Panels should stay synchronized around a single replay cursor.
- Important events should be discoverable from the timeline.
- The default layout should be fan-friendly, with room for an advanced mode later.

## Success Criteria

The MVP is successful if it:

- Replays at least one complete race session smoothly.
- Makes timing, stint, and incident context easier to follow than the TV broadcast alone.
- Demonstrates clear engineering depth in architecture and implementation.
- Looks polished enough to feature prominently on a resume or portfolio.
- Can plausibly evolve into a live-capable app without major rewrites.

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

### Phase 5 — Live mode

- Add sponsor-tier OpenF1 live adapter once available for the project.
- Support real-time updates through the same normalized internal event model.[cite:20][cite:6]

## Open Questions

These questions should be answered before implementation starts in earnest:

1. Should the first target be only race sessions, or also practice and qualifying?
2. Should the dashboard default to a fan-friendly layout or an engineer-dense layout?
3. Which historical seasons should be first-class supported in the MVP?
4. Should charts be minimal at first, with tables and timelines carrying most of the UI?
5. Should local caching be file-based, SQLite-backed, or Postgres-backed from day one?
6. What is the preferred deployment target for the first public demo?
