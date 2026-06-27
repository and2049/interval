import type { DataQuality, MapMode, ReplayMetadata } from "../../../shared/types/api";

export interface ChannelBadge {
  label: string;
  ready: boolean;
  tone: "ready" | "degraded" | "missing";
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

function sourceBadge(metadata: ReplayMetadata): ChannelBadge {
  const source = metadata.data_sources[0]?.name;
  const label = `${sourceLabel(source)} · ${cadenceLabel(metadata.frame_step_seconds)}`;
  return { label, ready: true, tone: source === "fastf1_historical" ? "ready" : "degraded" };
}

function cacheBadge(metadata: ReplayMetadata): ChannelBadge {
  const degraded = Object.values(metadata.available_channels).some((ready) => !ready);
  return { label: degraded ? "DEGRADED" : "CACHED", ready: true, tone: degraded ? "degraded" : "ready" };
}

function sourceLabel(source?: string) {
  switch (source) {
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
