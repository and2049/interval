import type { DataQuality, MapMode, ReplayMetadata } from "../../../shared/types/api";

export interface ChannelBadge {
  label: string;
  ready: boolean;
  tone: "ready" | "degraded" | "missing";
}

export function channelBadges(metadata: ReplayMetadata): ChannelBadge[] {
  const mapMode = mapModeLabel(mapModeFromGeometry(metadata));
  return [
    badge("TIMING", metadata.available_channels.timing),
    badge("GPS", metadata.available_channels.location),
    {
      label: mapMode ?? geometryLabel(metadata),
      ready: metadata.available_channels.track_geometry,
      tone: metadata.available_channels.track_geometry
        ? metadata.track_geometry.source === "open_f1_location"
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

function mapModeFromGeometry(metadata: ReplayMetadata): MapMode | undefined {
  if (!metadata.available_channels.track_geometry) return undefined;
  switch (metadata.track_geometry.source) {
    case "open_f1_location":
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
