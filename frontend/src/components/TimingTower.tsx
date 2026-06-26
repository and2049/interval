import { For, Show } from "solid-js";
import type { ReplaySnapshot } from "../../../shared/types/api";
import { compoundAbbreviation, compoundClass, formatLapTime, sectorClass } from "../lib/formatters";
import { gapLabel, hasTimingRows, sectorCells } from "../lib/timingDisplay";
import { EmptyState } from "./EmptyState";
import { Panel } from "./Panel";
import { QualityBadge } from "./QualityBadge";

export function TimingTower(props: { snapshot: ReplaySnapshot }) {
  return (
    <Panel
      title="Timing"
      testId="timing-tower"
      action={<QualityBadge quality={props.snapshot.timing.quality} title="Timing data quality" />}
    >
      <div class="overflow-auto font-mono text-[0.73rem]">
        <div class="table-grid sticky top-0 bg-[#20262e] text-slate-300">
          {["P", "DRV", "GAP", "INT", "S1", "S2", "S3", "LAP", "TY"].map((label) => (
            <div class="timing-cell border-b-line font-semibold">{label}</div>
          ))}
        </div>
        <Show
          when={hasTimingRows(props.snapshot.timing.rows)}
          fallback={<div class="px-2"><EmptyState label="No timing rows for this frame" /></div>}
        >
          <For each={props.snapshot.timing.rows}>
            {(row) => (
              <div class="table-grid hover:bg-panelHi/80">
                <div class="timing-cell text-mint">{row.position}</div>
                <div class="timing-cell gap-1 font-semibold text-white">
                  <span class="h-4 w-1" style={{ "background-color": `#${row.driver.team_colour}` }} />
                  {row.driver.code}
                </div>
                <div class="timing-cell text-timing">{gapLabel(row.position, row.gap_to_leader)}</div>
                <div class="timing-cell text-slate-200">{row.interval ?? "--"}</div>
                <For each={sectorCells(row.sectors)}>
                  {(sector) => (
                    <div class={`timing-cell ${sector ? sectorClass(sector.status) : "text-slate-500"}`}>
                      {sector?.duration?.toFixed(3) ?? "--"}
                    </div>
                  )}
                </For>
                <div class="timing-cell text-white">{formatLapTime(row.last_lap)}</div>
                <div class="timing-cell">
                  <span class={`rounded-full border px-1 text-[0.62rem] ${compoundClass(row.compound)}`}>
                    {compoundAbbreviation(row.compound)}
                  </span>
                </div>
              </div>
            )}
          </For>
        </Show>
      </div>
    </Panel>
  );
}
