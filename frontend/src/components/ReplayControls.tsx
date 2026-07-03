import { Pause, Play, Radio, RotateCcw, StepBack, StepForward } from "lucide-solid";
import { For } from "solid-js";
import { formatRaceClock } from "../lib/formatters";
import {
  clampReplayTime,
  parseReplaySpeedInput,
  parseReplayTimeInput,
  replaySessionTitle
} from "../lib/replayPlayback";
import { badgeClass, channelBadges, liveDashboardBadges, liveStatusLabel } from "../lib/replayQuality";
import type { LiveChannelHealth, LiveSessionStatus, ReplayMetadata } from "../../../shared/types/api";

interface ReplayControlsProps {
  metadata: ReplayMetadata;
  t: number;
  playing: boolean;
  speed: number;
  liveActive?: boolean;
  liveStatus?: "idle" | "connecting" | "connected" | "reconnecting" | "disconnected";
  liveRuntimeStatus?: LiveSessionStatus;
  liveChecking?: boolean;
  liveChannels?: LiveChannelHealth[];
  liveSimulationActive?: boolean;
  liveSimulationStatus?: "idle" | "connecting" | "connected" | "disconnected";
  onPlayPause: () => void;
  onSeek: (t: number) => void;
  onSpeed: (speed: number) => void;
  onLiveCheck?: () => void;
  onLiveStop?: () => void;
  onLiveSimulationToggle?: () => void;
}

export function ReplayControls(props: ReplayControlsProps) {
  const seek = (t: number) => props.onSeek(clampReplayTime(t, props.metadata.max_t));
  const nudge = (delta: number) => seek(props.t + delta);
  const controlsLocked = () => props.liveActive || props.liveSimulationActive;
  const statusLabel = () => {
    if (props.liveActive) return liveStatusLabel(props.liveStatus, props.liveRuntimeStatus);
    if (props.liveSimulationActive) return `SIM ${props.liveSimulationStatus ?? "idle"}`;
    return "REPLAY";
  };
  const displayedBadges = () =>
    props.liveActive
      ? liveDashboardBadges(props.metadata, props.liveChannels ?? [])
      : channelBadges(props.metadata);

  return (
    <div
      class="grid grid-cols-[minmax(16rem,1fr)_auto_minmax(20rem,1fr)] items-center gap-3 border-b border-line bg-[#15191f] px-3 py-2"
      data-testid="replay-controls"
    >
      <div class="flex min-w-0 items-center gap-3">
        <div class="min-w-0">
          <div class="font-mono text-[0.68rem] uppercase text-slate-400">Session</div>
          <div class="truncate text-sm font-semibold">{replaySessionTitle(props.metadata)}</div>
        </div>
        <div class="h-8 border-l border-line" />
        <div>
          <div class="font-mono text-[0.68rem] uppercase text-slate-400">Clock</div>
          <div class="font-mono text-xl text-white">{formatRaceClock(props.t)}</div>
        </div>
      </div>

      <div class="flex items-center gap-1">
        <button
          class={`rounded border px-2 py-2 font-mono text-[0.65rem] font-semibold ${
            props.liveActive
              ? "border-danger bg-danger/10 text-danger"
              : "border-mint bg-mint/10 text-mint"
          }`}
          title={props.liveActive ? "Stop OpenF1 live" : "Open active OpenF1 live session"}
          data-testid="live-toggle"
          disabled={props.liveChecking || props.liveSimulationActive}
          onClick={() => {
            if (props.liveActive) props.onLiveStop?.();
            else props.onLiveCheck?.();
          }}
        >
          <span class="inline-flex items-center gap-1">
            <Radio size={13} />
            {props.liveActive ? "STOP LIVE" : props.liveChecking ? "CHECKING" : "OPEN LIVE"}
          </span>
        </button>
        <button
          class={`rounded border px-2 py-2 font-mono text-[0.65rem] font-semibold ${
            props.liveSimulationActive
              ? "border-danger bg-danger/10 text-danger"
              : "border-line bg-panel text-slate-300 hover:border-mint hover:text-mint"
          }`}
          title={props.liveSimulationActive ? "Stop live simulation" : "Start live simulation from cached replay"}
          data-testid="live-sim-toggle"
          disabled={props.liveActive}
          onClick={() => props.onLiveSimulationToggle?.()}
        >
          {props.liveSimulationActive ? "STOP SIM" : "SIM"}
        </button>
        <button class="rounded border border-line p-2 hover:border-mint disabled:text-slate-600" title="Restart" data-testid="replay-restart" disabled={controlsLocked()} onClick={() => seek(0)}>
          <RotateCcw size={16} />
        </button>
        <button class="rounded border border-line p-2 hover:border-mint disabled:text-slate-600" title="Back 15 seconds" data-testid="replay-back" disabled={controlsLocked()} onClick={() => nudge(-15)}>
          <StepBack size={16} />
        </button>
        <button class="rounded border border-mint bg-mint/10 p-2 text-mint disabled:border-line disabled:text-slate-600" title={props.playing ? "Pause" : "Play"} data-testid="replay-play-toggle" disabled={controlsLocked()} onClick={props.onPlayPause}>
          {props.playing ? <Pause size={18} /> : <Play size={18} />}
        </button>
        <button class="rounded border border-line p-2 hover:border-mint disabled:text-slate-600" title="Forward 15 seconds" data-testid="replay-forward" disabled={controlsLocked()} onClick={() => nudge(15)}>
          <StepForward size={16} />
        </button>
      </div>

      <div class="flex min-w-0 items-center justify-end gap-3">
        <div class="hidden items-center gap-1 2xl:flex">
          <For each={displayedBadges()}>
            {(badge) => (
              <span
                class={`border px-1.5 py-0.5 font-mono text-[0.62rem] uppercase ${badgeClass(badge.tone)}`}
                title={badge.title ?? (badge.ready ? "Channel available" : "Channel unavailable")}
              >
                {badge.label}
              </span>
            )}
          </For>
        </div>
        <input
          class="w-48 accent-mint 2xl:w-72"
          data-testid="replay-seek"
          type="range"
          min="0"
          max={props.metadata.max_t}
          step="1"
          value={props.t}
          disabled={controlsLocked()}
          onInput={(event) => {
            const next = parseReplayTimeInput(event.currentTarget.value);
            if (next !== undefined) seek(next);
          }}
        />
        <select
          class="border border-line bg-panel px-2 py-1 font-mono text-xs"
          data-testid="replay-speed"
          value={props.speed}
          disabled={controlsLocked()}
          onChange={(event) => props.onSpeed(parseReplaySpeedInput(event.currentTarget.value))}
        >
          <option value="0.5">0.5x</option>
          <option value="1">1x</option>
          <option value="2">2x</option>
          <option value="4">4x</option>
        </select>
        <span class="border border-line px-1.5 py-0.5 font-mono text-[0.62rem] uppercase text-slate-400">
          {statusLabel()}
        </span>
      </div>
    </div>
  );
}
