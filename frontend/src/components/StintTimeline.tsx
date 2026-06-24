import { For } from "solid-js";
import type { ReplaySnapshot } from "../../../shared/types/api";
import { compoundClass, formatLapTime } from "../lib/formatters";
import { Panel } from "./Panel";

export function StintTimeline(props: { snapshot: ReplaySnapshot }) {
  return (
    <Panel title="Run Timeline">
      <div class="overflow-auto p-2">
        <div class="grid min-w-[900px] grid-cols-5 gap-2">
          <For each={props.snapshot.timing.rows}>
            {(row) => (
              <div class="border border-line bg-[#151a20] p-2 font-mono text-[0.72rem]">
                <div class="mb-1 flex items-center justify-between">
                  <span class="font-semibold text-white">{row.driver.code}</span>
                  <span class={`rounded-full border px-1 ${compoundClass(row.compound)}`}>{row.compound[0]}</span>
                </div>
                <div class="h-20">
                  <div class="mb-1 flex justify-between text-slate-400">
                    <span>Age {row.stint_age ?? "--"}</span>
                    <span>{row.in_pit ? "IN PIT" : row.status.toUpperCase()}</span>
                  </div>
                  <div class="h-4 w-full bg-line">
                    <div
                      class={`h-4 ${row.in_pit ? "bg-danger" : "bg-mint"}`}
                      style={{ width: `${Math.min(100, (row.stint_age ?? 1) * 4)}%` }}
                    />
                  </div>
                  <div class="mt-3 text-timing">{formatLapTime(row.last_lap)}</div>
                </div>
              </div>
            )}
          </For>
        </div>
      </div>
    </Panel>
  );
}
