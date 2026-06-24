import { For } from "solid-js";
import type { ReplaySnapshot } from "../../../shared/types/api";
import { compoundClass, formatLapTime, sectorClass } from "../lib/formatters";
import { Panel } from "./Panel";

export function TimingTower(props: { snapshot: ReplaySnapshot }) {
  return (
    <Panel title="Timing">
      <div class="overflow-auto font-mono text-[0.73rem]">
        <div class="table-grid sticky top-0 bg-[#20262e] text-slate-300">
          {["P", "DRV", "GAP", "INT", "S1", "S2", "S3", "LAP", "TY"].map((label) => (
            <div class="timing-cell border-b-line font-semibold">{label}</div>
          ))}
        </div>
        <For each={props.snapshot.timing.rows}>
          {(row) => (
            <div class="table-grid hover:bg-panelHi/80">
              <div class="timing-cell text-mint">{row.position}</div>
              <div class="timing-cell gap-1 font-semibold text-white">
                <span class="h-4 w-1" style={{ "background-color": `#${row.driver.team_colour}` }} />
                {row.driver.code}
              </div>
              <div class="timing-cell text-timing">{row.gap_to_leader ?? "LEADER"}</div>
              <div class="timing-cell text-slate-200">{row.interval ?? "--"}</div>
              <For each={row.sectors}>
                {(sector) => <div class={`timing-cell ${sectorClass(sector.status)}`}>{sector.duration?.toFixed(3) ?? "--"}</div>}
              </For>
              <div class="timing-cell text-white">{formatLapTime(row.last_lap)}</div>
              <div class="timing-cell">
                <span class={`rounded-full border px-1 text-[0.62rem] ${compoundClass(row.compound)}`}>
                  {row.compound[0]}
                </span>
              </div>
            </div>
          )}
        </For>
      </div>
    </Panel>
  );
}
