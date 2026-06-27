import { createMemo, Index, Show } from "solid-js";
import type { ReplaySnapshot } from "../../../shared/types/api";
import { derivedMetricRows } from "../lib/derivedMetrics";
import { trendClass } from "../lib/formatters";
import { EmptyState } from "./EmptyState";
import { Panel } from "./Panel";

export function DerivedMetricsPanel(props: { snapshot: ReplaySnapshot }) {
  const rows = createMemo(() =>
    derivedMetricRows(props.snapshot.derived_metrics, props.snapshot.timing.rows));

  return (
    <Panel title="Derived Metrics" testId="derived-metrics-panel">
      <div class="h-full overflow-auto p-2 font-mono text-[0.72rem]">
        <Show
          when={rows().length > 0}
          fallback={<EmptyState label="No derived metrics for this frame" />}
        >
          <Index each={rows()}>
            {(row) => (
              <div class="grid grid-cols-[3rem_1fr_4rem] border-b border-line/60 py-1">
                <span class="text-slate-400">{row().driver}</span>
                <span>{row().label}</span>
                <span class={trendClass(row().trend)}>{row().value}</span>
              </div>
            )}
          </Index>
        </Show>
      </div>
    </Panel>
  );
}
