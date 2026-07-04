# AGENTS.md

## Project identity

This repository contains an interactive Formula 1 second-screen dashboard focused on one shared live/replay race model. The product is inspired by engineer-style timing displays, but the intended audience is fans who want faster race understanding while watching a broadcast or replay.[cite:6][cite:20]

The architecture currently supports three data modes:

- Historical replay mode using OpenF1 discovery plus FastF1 historical telemetry extraction.
- Live simulation mode sourced from cached historical replay data.
- OpenF1 live mode during active race/sprint session windows, enabled by default and disabled only by server-side configuration.[cite:6][cite:20]

## Primary goals

1. Build a polished, lightweight, data-dense web application.
2. Keep replay mode deterministic and fully working.
3. Make live mode use the same normalized dashboard model instead of a separate UI path.
4. Produce a portfolio-quality codebase with strong engineering structure.
5. Favor clarity, speed, and maintainability over premature complexity.

## Chosen stack

- **Backend:** Rust.
- **Frontend:** SolidJS.
- **Styling:** Tailwind CSS.
- **App shape:** Web app, not Electron by default.
- **Delivery model:** Live/replay contract-first.

## Product principles

- The app is a **race engineer dashboard for fans**.
- The UI should optimize for scan speed, not novelty.
- Information density is good when grouped clearly.
- The most important UX concept is a single synchronized snapshot cursor.
- Tables, timelines, and track context matter more than decorative charts.
- Use visual emphasis sparingly and intentionally.

## Non-goals

Unless explicitly requested, do not optimize for:

- Native desktop packaging.
- Mobile parity for every panel in early phases.
- User accounts, auth, or social features.
- ML-heavy prediction systems.
- Support for multiple unrelated motorsport series.

## Repository structure

Preferred top-level structure:

```text
backend/
frontend/
shared/
docs/
infra/
```

Preferred backend structure:

```text
backend/
  src/
    connectors/
      openf1_historical/
      openf1_live/
    domain/
    replay/
    analytics/
    api/
    storage/
```

Preferred frontend structure:

```text
frontend/
  src/
    routes/
    components/
    stores/
    lib/
    themes/
```

## Domain vocabulary

Use these terms consistently across code, docs, issues, and pull requests:

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

Avoid passing raw upstream payloads directly into UI components when a normalized internal type is more appropriate.

## Data-source rules

- Treat OpenF1 as an upstream source, not as the app's internal model.[cite:6]
- Normalize upstream payloads before exposing them to the rest of the system.
- Keep replay ingestion and live ingestion behind connector boundaries.
- Do not hardwire UI components to the exact shape of upstream responses.
- Assume live access may require sponsor-tier credentials and keep those credentials server-side.[cite:20][cite:6]

## Replay/live contract rules

Historical replay remains the deterministic validation path. Live mode uses the
same backend-owned snapshot/event model, not a separate UI model.

- Every dashboard panel should work against historical replay data and live snapshots.[cite:6]
- Replay state must be deterministic.
- Timeline seek and playback speed changes must be first-class features for historical mode.
- Dashboard panels should derive their displayed state from the current `ReplaySnapshot`.
- Avoid live-only frontend panel logic; put source-specific behavior behind backend connectors.

## Live-mode rules

- Replay, live simulation, and OpenF1 live should feed the same normalized event model.
- Live updates should be incremental and stream backend-owned snapshots/events.
- Credentials must remain server-side.
- The frontend should subscribe to internal backend streams, not directly to paid upstream services.
- Add live improvements without forcing a redesign of existing dashboard panels.

## Backend guidance

### Rust design expectations

- Prefer explicit domain types over loose maps or generic JSON blobs.
- Use modules to enforce clean boundaries between connectors, replay logic, analytics, and API handlers.
- Keep parsing, normalization, replay state, and analytics separated.
- Design for testability; replay logic should be testable without the UI.
- Favor predictable data pipelines over clever abstractions.

### Backend responsibilities

The backend should own:

- OpenF1 fetching and normalization.[cite:6]
- Local caching or persistence.
- Replay timeline generation.
- Derived metrics and strategy calculations.
- API endpoints and event streams for the frontend.

The backend should not become a dumping ground for unscoped experiments. New features should map to a clear domain concept or product need.

## Frontend guidance

### SolidJS design expectations

- Keep components focused and composable.
- Use reactive state carefully; avoid hidden coupling between unrelated panels.
- Prefer clear data-flow from stores to presentation components.
- Optimize for dense information layouts that remain readable.
- Tailwind usage should support consistency, not utility-class chaos.

### UI principles

- The timing tower is the primary anchor panel.
- Track map, event feed, and strategy panels should stay synchronized with the same replay state.
- Use color to represent race semantics such as sectors, compounds, status, and incidents.
- Avoid decorative motion that slows reading.
- Fast scanability is more important than visual novelty.

## Styling rules

- Tailwind is the primary styling layer.
- Create shared tokens for colors, spacing, type scale, and panel density.
- Prefer reusable presentation primitives for tables, labels, panels, and timelines.
- Dark mode should be treated as a first-class experience.
- Design should feel lightweight and serious, not flashy or game-like.

## Feature prioritization

When deciding what to build next, prefer features in this order:

1. Core replay/live correctness.
2. Timing and stint clarity.
3. Race-context panels such as messages, weather, and track status.[cite:6]
4. Derived insights that improve understanding.
5. Visual polish.
6. Nice-to-have experiments.

## Quality bar

Every meaningful contribution should improve at least one of these:

- Replay correctness.
- Domain-model clarity.
- UI scanability.
- Performance.
- Maintainability.
- Product usefulness.

Changes that add code without improving one of those categories should be questioned.

## Testing expectations

### Backend

- Unit test normalization logic.
- Unit test replay timeline behavior.
- Test edge cases for pit events, lap transitions, and missing data.
- Add regression tests for any parsing bug caused by upstream data quirks.

### Frontend

- Test critical display formatting for lap time, interval, and status presentation.
- Test synchronization between the replay cursor and visible panels.
- Test loading and empty states.
- Avoid brittle tests tied to incidental markup details.

## Documentation expectations

When adding features, update relevant docs if any of the following change:

- Domain model.
- Repository structure.
- API contracts.
- Feature scope.
- Setup instructions.
- Architectural assumptions.

Keep docs concise but specific. Prefer real examples over vague guidance.

## Decision rules for agents

When making design or implementation decisions, follow this order:

1. Preserve one live/replay dashboard contract.
2. Keep replay, live simulation, and OpenF1 live on one dashboard contract.
3. Favor normalized domain models.
4. Prefer lightweight UI patterns.
5. Avoid unnecessary dependencies.
6. Make future contributors' jobs easier.

## Questions to resolve

These items still need explicit product decisions:

- Whether qualifying and practice should join the current race-plus-sprint scope.
- What should the default dashboard density be?
- How much charting is desirable in the first release versus table-first views?
- What persistence layer should be used in local development and in production?
- What deployment target should be assumed for the first public demo?
- Should there eventually be an advanced TUI companion, or should the web app remain the sole official client?
- What is the right production deployment model for OpenF1 live credentials and long-running SSE sessions?
