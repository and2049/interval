export type SessionType = "race" | "sprint";
export type SectorStatus = "personal_best" | "overall_best" | "normal" | "unknown";
export type TyreCompound = "SOFT" | "MEDIUM" | "HARD" | "INTERMEDIATE" | "WET" | "UNKNOWN";
export type DriverStatus = "on_track" | "pit" | "out";
export type RankSource = "open_f1_position" | "fast_f1_position" | "session_result" | "derived_progress" | "fallback_grid";
export type DerivedMetricKind = "recent_pace" | "stint_delta" | "pit_state";
export type MetricTrend = "improving" | "stable" | "degrading" | "unknown";
export type TrackGeometrySource = "open_f1_location" | "fast_f1_telemetry" | "curated_static" | "schematic";
export type TrackGeometryQuality = "ready" | "schematic" | "missing";
export type DataQuality = "ready" | "real" | "interpolated" | "projected" | "schematic" | "missing" | "stale";
export type MapMode = "gps" | "projected" | "schematic";
export type EventKind =
  | "race_control"
  | "track_status"
  | "pit_stop"
  | "stint_change"
  | "leader_change"
  | "weather_change"
  | "data_gap"
  | "driver_out";
export type EventSeverity = "info" | "notice" | "warning" | "critical";
export type EventSource = "open_f1" | "fast_f1" | "derived" | "system";
export type IngestStatus = "not_ingested" | "fetching" | "normalizing" | "ready" | "failed";
export type SessionSupportStatus = "supported" | "future" | "cancelled";
export type TrackPositionSource = "real" | "interpolated" | "projected" | "schematic";
export type TrackPositionQuality =
  | "real"
  | "interpolated"
  | "stale"
  | "projected"
  | "schematic"
  | "missing";

export interface Season {
  year: number;
}

export interface Meeting {
  meeting_key: number;
  year: number;
  name: string;
  country: string;
  location: string;
}

export interface Session {
  session_key: number;
  meeting_key: number;
  year: number;
  name: string;
  session_type: SessionType;
  start_time: string;
  end_time: string;
  total_laps: number;
}

export interface SessionReadiness {
  session: Session;
  ingest_status: IngestStatus;
  replay_ready: boolean;
  is_demo: boolean;
  last_error: string | null;
  support_status: SessionSupportStatus;
  support_reason: string | null;
}

export interface Driver {
  driver_number: number;
  code: string;
  full_name: string;
  team_name: string;
  team_colour: string;
}

export interface Sector {
  index: number;
  duration?: number | null;
  status: SectorStatus;
}

export interface TrackPositionSample {
  driver_number: number;
  x: number;
  y: number;
  z?: number | null;
  relative_distance?: number | null;
  source: TrackPositionSource;
  quality: TrackPositionQuality;
  stale_seconds?: number | null;
}

export interface TrackBounds {
  min_x: number;
  max_x: number;
  min_y: number;
  max_y: number;
}

export interface TrackPoint {
  x: number;
  y: number;
  z?: number | null;
  cumulative_distance: number;
  relative_distance: number;
}

export interface TrackGeometry {
  contract_version: "replay.v1" | string;
  session_key: number;
  bounds: TrackBounds;
  centerline: TrackPoint[];
  inner_edge: TrackPoint[];
  outer_edge: TrackPoint[];
  source: TrackGeometrySource;
  quality: TrackGeometryQuality;
  map_mode: MapMode;
  circuit_length?: number | null;
  generated_at: string;
}

export interface RaceControlMessage {
  t: number;
  category: string;
  message: string;
  flag?: string | null;
  scope?: string | null;
}

export interface WeatherSample {
  t: number;
  air_temp?: number | null;
  track_temp?: number | null;
  humidity?: number | null;
  rainfall?: number | null;
  wind_direction?: number | null;
  wind_speed?: number | null;
}

export interface ReplayCursor {
  session_key: number;
  t: number;
  frame_index: number;
  playback_speed: number;
  is_paused: boolean;
}

export interface ReplayMetadata {
  contract_version: "replay.v1" | string;
  session: Session;
  meeting?: Meeting | null;
  duration_seconds: number;
  frame_step_seconds: number;
  total_frames: number;
  drivers: Driver[];
  min_t: number;
  max_t: number;
  race_start_t: number;
  generated_at: string;
  data_sources: DataSource[];
  available_channels: AvailableChannels;
  track_geometry: TrackGeometrySummary;
  endpoints: EndpointLinks;
}

export interface DataSource {
  name: string;
  mode: string;
}

export interface AvailableChannels {
  timing: boolean;
  location: boolean;
  track_geometry: boolean;
  weather: boolean;
  race_control: boolean;
  stints: boolean;
  pit_events: boolean;
  intervals: boolean;
}

export interface TrackGeometrySummary {
  status: TrackGeometryQuality;
  source: TrackGeometrySource;
  quality: TrackGeometryQuality;
}

export interface EndpointLinks {
  snapshot_endpoint: string;
  stream_endpoint: string;
  events_endpoint: string;
  track_geometry_endpoint: string;
}

export interface DriverSnapshot {
  driver: Driver;
  position: number;
  rank_source: RankSource;
  gap_to_leader?: string | null;
  interval?: string | null;
  lap: number;
  last_lap?: number | null;
  compound: TyreCompound;
  stint_age?: number | null;
  sectors: Sector[];
  in_pit: boolean;
  status: DriverStatus;
}

export interface RaceState {
  lap: number;
  track_status: string;
}

export interface TimingSection {
  rows: DriverSnapshot[];
  quality: DataQuality;
}

export interface TrackSection {
  positions: TrackPositionSample[];
  map_mode: MapMode;
  quality: DataQuality;
}

export interface ReplayWeatherSection {
  sample?: WeatherSample | null;
  quality: DataQuality;
}

export interface RaceControlSection {
  messages: RaceControlMessage[];
  quality: DataQuality;
}

export interface DerivedMetric {
  driver_number?: number | null;
  kind: DerivedMetricKind;
  label: string;
  value: string;
  trend: MetricTrend;
}

export interface ReplaySnapshot {
  contract_version: "replay.v1" | string;
  cursor: ReplayCursor;
  race_state: RaceState;
  timing: TimingSection;
  track: TrackSection;
  weather: ReplayWeatherSection;
  race_control: RaceControlSection;
  derived_metrics: DerivedMetric[];
}

export interface ReplayEventListResponse {
  contract_version: "replay.v1" | string;
  events: ReplayEvent[];
}

export interface LiveSessionStatus {
  session_key: number;
  active: boolean;
  current_t?: number | null;
  max_t?: number | null;
  started_at?: string | null;
  updated_at?: string | null;
  source?: string | null;
  channels: LiveChannelHealth[];
}

export type LiveSimulationStatus = LiveSessionStatus;

export type LiveChannelState = "fresh" | "cached" | "stale" | "missing" | "failed";

export interface LiveChannelHealth {
  endpoint: string;
  state: LiveChannelState;
  age_seconds?: number | null;
  rows?: number | null;
  last_error?: string | null;
}

export interface LiveCurrentResponse {
  availability: LiveAvailability;
  active: boolean;
  session?: Session | null;
  meeting?: Meeting | null;
  next_session?: Session | null;
  next_meeting?: Meeting | null;
  status?: LiveSessionStatus | null;
  message?: string | null;
}

export type LiveAvailability = "disabled" | "inactive" | "active" | "error";

export interface ReplayEvent {
  id: string;
  t: number;
  kind: EventKind;
  severity: EventSeverity;
  driver_number?: number | null;
  message: string;
  source: EventSource;
  payload: unknown;
}

export interface EndpointCoverage {
  endpoint: string;
  present: boolean;
  rows?: number | null;
}

export interface IngestTrackGeometrySummary {
  status: TrackGeometryQuality;
  source: TrackGeometrySource;
  quality: TrackGeometryQuality;
}

export interface IngestResponse {
  session_key: number;
  status: IngestStatus;
  cached_endpoints: number;
  endpoint_coverage: EndpointCoverage[];
  generated_snapshots: number;
  track_geometry: IngestTrackGeometrySummary | null;
  available_channels: AvailableChannels | null;
  warnings: string[];
  error: string | null;
}

export type OpenF1TokenSource = "settings" | "env" | "none";

export interface OpenF1TokenSettings {
  configured: boolean;
  /** Masked fingerprint of the stored token. Never the token itself. */
  hint: string | null;
  source: OpenF1TokenSource;
  /** True when INTERVAL_OPENF1_LIVE_TOKEN is also set. */
  env_token_present: boolean;
  path: string | null;
}

export type OpenF1TokenProbeResult = "ok" | "unauthorized" | "unreachable" | "invalid";

export interface OpenF1TokenProbe {
  result: OpenF1TokenProbeResult;
  message: string;
}
