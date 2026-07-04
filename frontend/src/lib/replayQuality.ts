import type {
  DataQuality,
  LiveAvailability,
  LiveChannelHealth,
  LiveSessionStatus,
  MapMode,
  ReplayMetadata
} from "../../../shared/types/api";

export interface ChannelBadge {
  label: string;
  ready: boolean;
  tone: "ready" | "degraded" | "missing";
  title?: string;
}

export function channelBadges(metadata: ReplayMetadata): ChannelBadge[] {
  const mapMode = mapModeLabel(mapModeFromGeometry(metadata));
  return [
    sourceBadge(metadata),
    cacheBadge(metadata),
    badge("TIMING", metadata.available_channels.timing),
    badge("GPS", metadata.available_channels.location),
    {
      label: mapMode ?? geometryLabel(metadata),
      ready: metadata.available_channels.track_geometry,
      tone: metadata.available_channels.track_geometry
        ? metadata.track_geometry.source === "open_f1_location"
          || metadata.track_geometry.source === "fast_f1_telemetry"
          ? "ready"
          : "degraded"
        : "missing"
    },
    badge("WEATHER", metadata.available_channels.weather),
    badge("RC", metadata.available_channels.race_control),
    badge("PIT", metadata.available_channels.pit_events),
    badge("INT", metadata.available_channels.intervals)
  ];
}

export function liveChannelBadges(channels: LiveChannelHealth[]): ChannelBadge[] {
  return channels.map((channel) => ({
    label: channel.endpoint.toUpperCase(),
    ready: channel.state === "fresh" || channel.state === "cached",
    tone: liveChannelTone(channel),
    title: liveChannelTitle(channel)
  }));
}

export function liveDashboardBadges(
  metadata: ReplayMetadata,
  channels: LiveChannelHealth[]
): ChannelBadge[] {
  return [sourceBadge(metadata), ...liveChannelBadges(channels)];
}

export function liveStatusLabel(
  connection: string | undefined,
  status?: LiveSessionStatus,
  nowMs = Date.now()
): string {
  const state = connection ?? "idle";
  const updatedAt = status?.updated_at ? Date.parse(status.updated_at) : Number.NaN;
  if (!Number.isFinite(updatedAt)) return `LIVE ${state}`;

  const ageSeconds = Math.max(0, Math.round((nowMs - updatedAt) / 1000));
  return `LIVE ${state} · UPDATED ${ageSeconds}s`;
}

export function liveAvailabilityBadge(
  availability: LiveAvailability,
  options: { checking?: boolean; active?: boolean } = {}
): ChannelBadge {
  if (options.active) return { label: "LIVE OPEN", ready: true, tone: "ready" };
  if (options.checking) return { label: "LIVE CHECKING", ready: false, tone: "degraded" };

  switch (availability) {
    case "active":
      return { label: "LIVE READY", ready: true, tone: "ready" };
    case "inactive":
      return { label: "LIVE WAITING", ready: false, tone: "degraded" };
    case "disabled":
      return { label: "LIVE OFF", ready: false, tone: "missing" };
    case "error":
      return { label: "LIVE ERROR", ready: false, tone: "missing" };
  }
}

function sourceBadge(metadata: ReplayMetadata): ChannelBadge {
  const source = metadata.data_sources[0]?.name;
  const label = `${sourceLabel(source)} · ${cadenceLabel(metadata.frame_step_seconds)}`;
  return {
    label,
    ready: true,
    tone: source === "fastf1_historical" || source === "live_simulation" || source === "openf1_live" ? "ready" : "degraded"
  };
}

function cacheBadge(metadata: ReplayMetadata): ChannelBadge {
  const degraded = Object.values(metadata.available_channels).some((ready) => !ready);
  return { label: degraded ? "DEGRADED" : "CACHED", ready: true, tone: degraded ? "degraded" : "ready" };
}

function sourceLabel(source?: string) {
  switch (source) {
    case "live_simulation":
      return "Live Sim";
    case "openf1_live":
      return "LIVE · OpenF1";
    case "fastf1_historical":
      return "FastF1";
    case "openf1_historical":
      return "OpenF1";
    case "demo":
      return "Demo";
    case "mixed":
      return "Mixed";
    default:
      return "Replay";
  }
}

function cadenceLabel(frameStepSeconds: number) {
  if (!Number.isFinite(frameStepSeconds) || frameStepSeconds <= 0) return "? Hz";
  const hz = 1 / frameStepSeconds;
  return `${Number.isInteger(hz) ? hz.toFixed(0) : hz.toFixed(1)} Hz`;
}

function mapModeFromGeometry(metadata: ReplayMetadata): MapMode | undefined {
  if (!metadata.available_channels.track_geometry) return undefined;
  switch (metadata.track_geometry.source) {
    case "open_f1_location":
    case "fast_f1_telemetry":
      return "gps";
    case "curated_static":
      return "projected";
    case "schematic":
      return "schematic";
    default:
      return undefined;
  }
}

export function badgeClass(tone: ChannelBadge["tone"]) {
  switch (tone) {
    case "ready":
      return "border-mint/60 text-mint";
    case "degraded":
      return "border-amber/70 text-amber";
    default:
      return "border-line text-slate-500";
  }
}

export function qualityBadge(quality: DataQuality): ChannelBadge {
  switch (quality) {
    case "ready":
    case "real":
      return { label: quality.toUpperCase(), ready: true, tone: "ready" };
    case "interpolated":
    case "projected":
    case "schematic":
    case "stale":
      return { label: quality.toUpperCase(), ready: false, tone: "degraded" };
    default:
      return { label: "MISSING", ready: false, tone: "missing" };
  }
}

export function mapModeLabel(mode?: MapMode) {
  switch (mode) {
    case "gps":
      return "MAP GPS";
    case "projected":
      return "MAP PROJECTED";
    case "schematic":
      return "MAP SCHEMATIC";
    default:
      return undefined;
  }
}

export function mapModeClass(mode: MapMode) {
  switch (mode) {
    case "gps":
      return "border-mint text-mint";
    case "projected":
      return "border-amber text-amber";
    default:
      return "border-line text-slate-400";
  }
}

function badge(label: string, ready: boolean): ChannelBadge {
  return {
    label,
    ready,
    tone: ready ? "ready" : "missing"
  };
}

function liveChannelTone(channel: LiveChannelHealth): ChannelBadge["tone"] {
  switch (channel.state) {
    case "fresh":
      return "ready";
    case "cached":
      if (channel.rows === 0) return "missing";
      return channel.last_error ? "degraded" : "ready";
    case "stale":
      return "degraded";
    default:
      return "missing";
  }
}

function liveChannelTitle(channel: LiveChannelHealth): string {
  const rows = channel.rows == null ? "" : ` · ${channel.rows} rows`;
  const age = channel.age_seconds == null
    ? ""
    : ` · ${Math.max(0, Math.round(channel.age_seconds))}s old`;
  const error = channel.last_error ? ` · ${channel.last_error}` : "";
  return `${channel.endpoint}: ${channel.state}${rows}${age}${error}`;
}

function geometryLabel(metadata: ReplayMetadata) {
  switch (metadata.track_geometry.status) {
    case "ready":
      return "MAP READY";
    case "schematic":
      return "MAP SCHEMATIC";
    default:
      return "MAP MISSING";
  }
}
