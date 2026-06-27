import { Index, Show } from "solid-js";
import type { ReplayEvent } from "../../../shared/types/api";
import { formatEventClock } from "../lib/formatters";
import {
  eventFeedEmptyLabel,
  eventFeedState,
  eventKindLabel,
  eventSeverityClass,
  recentReplayEvents
} from "../lib/replayEvents";
import { EmptyState } from "./EmptyState";
import { Panel } from "./Panel";

export function EventTimelinePanel(props: {
  events: ReplayEvent[];
  t: number;
  loading?: boolean;
  error?: unknown;
}) {
  const rows = () => recentReplayEvents(props.events, props.t, 5);
  const state = () =>
    eventFeedState({
      rows: rows(),
      loading: props.loading ?? false,
      error: props.error
    });

  return (
    <Panel title="Event Feed" testId="event-feed-panel">
      <div class="h-full overflow-auto p-2 font-mono text-[0.7rem]">
        <Show
          when={state() === "ready"}
          fallback={<EmptyState label={eventFeedEmptyLabel(state())} />}
        >
          <Index each={rows()}>
            {(event) => (
              <div class="mb-2 border-b border-line/70 pb-2">
                <div class="flex items-center justify-between gap-2 text-slate-500">
                  <span>{formatEventClock(event().t)}</span>
                  <span class={eventSeverityClass(event().severity)}>
                    {eventKindLabel(event().kind)}
                  </span>
                </div>
                <div class="mt-0.5 text-slate-100">{event().message}</div>
              </div>
            )}
          </Index>
        </Show>
      </div>
    </Panel>
  );
}
