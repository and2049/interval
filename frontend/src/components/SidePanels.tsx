import { createMemo } from "solid-js";
import type { ReplayEvent, ReplaySnapshot } from "../../../shared/types/api";
import { DerivedMetricsPanel } from "./DerivedMetricsPanel";
import { EventTimelinePanel } from "./EventTimelinePanel";
import { RaceControlPanel } from "./RaceControlPanel";
import { WeatherPanel } from "./WeatherPanel";

export function SidePanels(props: {
  snapshot: ReplaySnapshot;
  events: ReplayEvent[];
  eventsLoading?: boolean;
  eventsError?: unknown;
}) {
  const raceControl = createMemo(() => props.snapshot.race_control);
  const weather = createMemo(() => props.snapshot.weather);
  const cursorT = createMemo(() => props.snapshot.cursor.t);

  return (
    <div class="grid h-full min-h-0 grid-rows-[minmax(6rem,1fr)_minmax(6rem,1fr)_7rem_minmax(6rem,1fr)] gap-2">
      <RaceControlPanel raceControl={raceControl()} />
      <EventTimelinePanel
        events={props.events}
        t={cursorT()}
        loading={props.eventsLoading}
        error={props.eventsError}
      />
      <WeatherPanel weather={weather()} />
      <DerivedMetricsPanel snapshot={props.snapshot} />
    </div>
  );
}
