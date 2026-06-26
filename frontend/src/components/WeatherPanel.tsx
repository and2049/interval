import { For, Show } from "solid-js";
import type { ReplayWeatherSection } from "../../../shared/types/api";
import { hasWeatherSample, weatherMetrics } from "../lib/weatherDisplay";
import { EmptyState } from "./EmptyState";
import { Panel } from "./Panel";
import { QualityBadge } from "./QualityBadge";

export function WeatherPanel(props: { weather: ReplayWeatherSection }) {
  return (
    <Panel
      title="Weather"
      testId="weather-panel"
      action={<QualityBadge quality={props.weather.quality} title="Weather data quality" />}
    >
      <div class="grid h-full grid-cols-2 content-center gap-x-4 gap-y-2 p-3 font-mono text-[0.72rem]">
        <Show
          when={hasWeatherSample(props.weather)}
          fallback={
            <div class="col-span-2">
              <EmptyState label="No weather sample for this frame" />
            </div>
          }
        >
          <For each={weatherMetrics(props.weather)}>
            {(metric) => <Metric label={metric.label} value={metric.value} />}
          </For>
        </Show>
      </div>
    </Panel>
  );
}

function Metric(props: { label: string; value: string }) {
  return (
    <div>
      <div class="text-slate-500">{props.label}</div>
      <div class="text-sm text-timing">{props.value}</div>
    </div>
  );
}
