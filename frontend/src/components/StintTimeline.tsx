import { createMemo, Index, type Accessor } from "solid-js";
import type { DriverSnapshot, ReplaySnapshot } from "../../../shared/types/api";
import { compoundAbbreviation, compoundClass, formatLapTime } from "../lib/formatters";
import { stintProgressDisplay } from "../lib/stintTimeline";
import { Panel } from "./Panel";

export function StintTimeline(props: { snapshot: ReplaySnapshot }) {
  const rows = createMemo(() => props.snapshot.timing.rows);

  return (
    <Panel title="Run Timeline" class="h-full min-h-0" testId="stint-timeline">
      <div class="h-full min-h-0 overflow-auto p-2">
        <div class="grid min-w-[900px] grid-cols-5 gap-2">
          <Index each={rows()}>
            {(row) => <StintCard row={row} />}
          </Index>
        </div>
      </div>
    </Panel>
  );
}

function StintCard(props: { row: Accessor<DriverSnapshot> }) {
  const progress = () => stintProgressDisplay(props.row().stint_age);

  return (
    <div class="border border-line bg-[#151a20] p-2 font-mono text-[0.72rem]">
      <div class="mb-1 flex items-center justify-between">
        <span class="font-semibold text-white">{props.row().driver.code}</span>
        <span class={`rounded-full border px-1 ${compoundClass(props.row().compound)}`}>
          {compoundAbbreviation(props.row().compound)}
        </span>
      </div>
      <div class="h-20">
        <div class="mb-1 flex justify-between text-slate-400">
          <span>{progress().label}</span>
          <span>{props.row().in_pit ? "IN PIT" : props.row().status.toUpperCase()}</span>
        </div>
        <div class="h-4 w-full bg-line">
          <div
            class={`h-4 ${stintBarClass(progress().known, props.row().in_pit)}`}
            style={{ width: `${progress().widthPercent}%` }}
          />
        </div>
        <div class="mt-3 text-timing">{formatLapTime(props.row().last_lap)}</div>
      </div>
    </div>
  );
}

function stintBarClass(known: boolean, inPit: boolean) {
  if (!known) return "bg-slate-700";
  return inPit ? "bg-danger" : "bg-mint";
}
