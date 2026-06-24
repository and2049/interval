import { Pause, Play, RotateCcw, StepBack, StepForward } from "lucide-solid";
import { For } from "solid-js";
import { formatRaceClock } from "../lib/formatters";
import type { ReplayMetadata } from "../../../shared/types/api";

interface ReplayControlsProps {
  metadata: ReplayMetadata;
  t: number;
  playing: boolean;
  speed: number;
  onPlayPause: () => void;
  onSeek: (t: number) => void;
  onSpeed: (speed: number) => void;
}

export function ReplayControls(props: ReplayControlsProps) {
  const nudge = (delta: number) => props.onSeek(Math.max(0, Math.min(props.metadata.max_t, props.t + delta)));

  return (
    <div class="grid grid-cols-[1fr_auto_1fr] items-center gap-3 border-b border-line bg-[#15191f] px-3 py-2">
      <div class="flex items-center gap-3">
        <div>
          <div class="font-mono text-[0.68rem] uppercase text-slate-400">Session</div>
          <div class="text-sm font-semibold">{props.metadata.session.year} {props.metadata.session.name}</div>
        </div>
        <div class="h-8 border-l border-line" />
        <div>
          <div class="font-mono text-[0.68rem] uppercase text-slate-400">Clock</div>
          <div class="font-mono text-xl text-white">{formatRaceClock(props.t)}</div>
        </div>
      </div>

      <div class="flex items-center gap-1">
        <button class="rounded border border-line p-2 hover:border-mint" title="Restart" onClick={() => props.onSeek(0)}>
          <RotateCcw size={16} />
        </button>
        <button class="rounded border border-line p-2 hover:border-mint" title="Back 15 seconds" onClick={() => nudge(-15)}>
          <StepBack size={16} />
        </button>
        <button class="rounded border border-mint bg-mint/10 p-2 text-mint" title={props.playing ? "Pause" : "Play"} onClick={props.onPlayPause}>
          {props.playing ? <Pause size={18} /> : <Play size={18} />}
        </button>
        <button class="rounded border border-line p-2 hover:border-mint" title="Forward 15 seconds" onClick={() => nudge(15)}>
          <StepForward size={16} />
        </button>
      </div>

      <div class="flex items-center justify-end gap-3">
        <div class="hidden items-center gap-1 xl:flex">
          <For each={channelBadges(props.metadata)}>
            {(badge) => (
              <span
                class={`border px-1.5 py-0.5 font-mono text-[0.62rem] uppercase ${badge.ready ? "border-mint/60 text-mint" : "border-line text-slate-500"}`}
              >
                {badge.label}
              </span>
            )}
          </For>
        </div>
        <input
          class="w-72 accent-mint"
          type="range"
          min="0"
          max={props.metadata.max_t}
          step="1"
          value={props.t}
          onInput={(event) => props.onSeek(Number(event.currentTarget.value))}
        />
        <select
          class="border border-line bg-panel px-2 py-1 font-mono text-xs"
          value={props.speed}
          onChange={(event) => props.onSpeed(Number(event.currentTarget.value))}
        >
          <option value="0.5">0.5x</option>
          <option value="1">1x</option>
          <option value="2">2x</option>
          <option value="4">4x</option>
        </select>
      </div>
    </div>
  );
}

function channelBadges(metadata: ReplayMetadata) {
  return [
    { label: "timing", ready: metadata.available_channels.timing },
    { label: "gps", ready: metadata.available_channels.location },
    { label: metadata.track_geometry.status, ready: metadata.available_channels.track_geometry },
    { label: "weather", ready: metadata.available_channels.weather },
    { label: "rc", ready: metadata.available_channels.race_control },
    { label: "pit", ready: metadata.available_channels.pit_events },
    { label: "int", ready: metadata.available_channels.intervals }
  ];
}
