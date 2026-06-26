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
  return (
    <div class="grid h-full min-h-0 grid-rows-[minmax(6rem,1fr)_minmax(6rem,1fr)_7rem_minmax(6rem,1fr)] gap-2">
      <RaceControlPanel raceControl={props.snapshot.race_control} />
      <EventTimelinePanel
        events={props.events}
        t={props.snapshot.cursor.t}
        loading={props.eventsLoading}
        error={props.eventsError}
      />
      <WeatherPanel weather={props.snapshot.weather} />
      <DerivedMetricsPanel snapshot={props.snapshot} />
    </div>
  );
}
