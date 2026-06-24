import { For } from "solid-js";
import type { ReplaySnapshot } from "../../../shared/types/api";
import { trendClass } from "../lib/formatters";
import { Panel } from "./Panel";

export function SidePanels(props: { snapshot: ReplaySnapshot }) {
  return (
    <div class="grid h-full grid-rows-[1.1fr_0.8fr_1fr] gap-2">
      <Panel title="Race Control">
        <div class="max-h-52 overflow-auto p-2 font-mono text-[0.72rem]">
          <For each={props.snapshot.race_control.messages}>
            {(event) => (
              <div class="mb-2 border-b border-line/70 pb-2">
                <div class="text-slate-400">{event.t.toFixed(0).padStart(4, "0")}s · {event.category}</div>
                <div class={event.flag === "yellow" ? "text-amber" : "text-slate-100"}>{event.message}</div>
              </div>
            )}
          </For>
        </div>
      </Panel>

      <Panel title="Weather">
        <div class="grid grid-cols-2 gap-x-4 gap-y-2 p-3 font-mono text-[0.75rem]">
          <Metric label="Air" value={`${props.snapshot.weather.sample?.air_temp?.toFixed(1) ?? "--"}C`} />
          <Metric label="Track" value={`${props.snapshot.weather.sample?.track_temp?.toFixed(1) ?? "--"}C`} />
          <Metric label="Humidity" value={`${props.snapshot.weather.sample?.humidity?.toFixed(0) ?? "--"}%`} />
          <Metric label="Wind" value={`${props.snapshot.weather.sample?.wind_speed?.toFixed(1) ?? "--"} m/s`} />
        </div>
      </Panel>

      <Panel title="Derived Metrics">
        <div class="overflow-auto p-2 font-mono text-[0.72rem]">
          <For each={props.snapshot.derived_metrics}>
            {(metric) => (
              <div class="grid grid-cols-[3rem_1fr_4rem] border-b border-line/60 py-1">
                <span class="text-slate-400">{metric.driver_number ?? "--"}</span>
                <span>{metric.label}</span>
                <span class={trendClass(metric.trend)}>{metric.value}</span>
              </div>
            )}
          </For>
        </div>
      </Panel>
    </div>
  );
}

function Metric(props: { label: string; value: string }) {
  return (
    <div>
      <div class="text-slate-500">{props.label}</div>
      <div class="text-base text-timing">{props.value}</div>
    </div>
  );
}
