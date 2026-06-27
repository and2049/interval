import { Index, Show } from "solid-js";
import type { RaceControlSection } from "../../../shared/types/api";
import { formatEventClock } from "../lib/formatters";
import { EmptyState } from "./EmptyState";
import { Panel } from "./Panel";
import { QualityBadge } from "./QualityBadge";

export function RaceControlPanel(props: { raceControl: RaceControlSection }) {
  return (
    <Panel
      title="Race Control"
      testId="race-control-panel"
      action={<QualityBadge quality={props.raceControl.quality} title="Race-control data quality" />}
    >
      <div class="h-full overflow-auto p-2 font-mono text-[0.72rem]">
        <Show
          when={props.raceControl.messages.length > 0}
          fallback={<EmptyState label="No race-control messages at this time" />}
        >
          <Index each={props.raceControl.messages}>
            {(event) => (
              <div class="mb-2 border-b border-line/70 pb-2">
                <div class="text-slate-400">
                  {formatEventClock(event().t)} · {event().category}
                </div>
                <div class={event().flag === "yellow" ? "text-amber" : "text-slate-100"}>
                  {event().message}
                </div>
              </div>
            )}
          </Index>
        </Show>
      </div>
    </Panel>
  );
}
