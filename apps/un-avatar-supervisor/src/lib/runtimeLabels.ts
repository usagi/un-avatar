import {
  aaModeLabel,
  formatBytes,
  runtimeMetric,
  textureModeLabel,
} from "./formatting";
import { outputLabel, type OutputLabelData } from "./profileLabels";

export type RuntimeTextureSummaryLabelData = {
  image_count: number;
  resized_count: number;
  compressed_count: number;
  compression_fallback_count: number;
  uploaded_mip_bytes: number;
  cache_enabled: boolean;
  cache_hits: number;
  cache_misses: number;
  cache_writes: number;
  compressed_cache_hits: number;
  compressed_cache_misses: number;
  compressed_cache_writes: number;
};

export type RuntimeQualityStatusLabelData = {
  connected: boolean;
  surface_width: number | null;
  surface_height: number | null;
  aa: string | null;
  texture_resolution_limit: string | null;
  texture_compression: string | null;
  processed_texture_cache: boolean | null;
  texture_summary: RuntimeTextureSummaryLabelData | null;
};

export type ProfileQualityLabelData = {
  aa: string;
  texture_resolution_limit: string;
};

export type RuntimeStartupStatusLabelData = {
  startup_phase?: string | null;
  startup_message?: string | null;
  startup_progress?: [number, number] | null;
};

export type RuntimeConnectionStatusLabelData = {
  connected: boolean;
};

export type RuntimeSpoutStatusLabelData = {
  connected: boolean;
  spout_enabled: boolean;
  spout_available: boolean;
  spout_frames_attempted: number;
  spout_frames_sent: number;
  spout_frame_failures: number;
  spout_last_send_ok: boolean | null;
  spout_last_readback_ms: number | null;
  spout_last_send_ms: number | null;
  spout_last_total_ms: number | null;
  spout_sender_initialized: boolean | null;
  spout_sender_width: number | null;
  spout_sender_height: number | null;
  spout_name: string | null;
};

export type RendererHealthKind = "ok" | "pending" | "warn" | "error" | "idle";

export type RendererHealthRendererData = OutputLabelData & {
  state: string;
};

export type RendererHealthStatusData = RuntimeStartupStatusLabelData
  & RuntimeConnectionStatusLabelData
  & Pick<
    RuntimeSpoutStatusLabelData,
    "spout_available" | "spout_enabled" | "spout_last_send_ok" | "spout_name"
  >;

export type RuntimeOutputStatusData = RuntimeConnectionStatusLabelData
  & Pick<
    RuntimeSpoutStatusLabelData,
    "spout_available" | "spout_enabled" | "spout_name"
  >;

export type RuntimeSummaryStatusData = RuntimeQualityStatusLabelData
  & RuntimeSpoutStatusLabelData;

export type RuntimeStageStatusData = RuntimeQualityStatusLabelData
  & RendererHealthStatusData;

export type RuntimeTableStatusData = RuntimeStartupStatusLabelData
  & RendererHealthStatusData
  & {
    fps: number | null;
    cpu_ms: number | null;
    gpu_ms: number | null;
    ram_mb: number | null;
  };

export type RuntimeStatusLabels = {
  pending: string;
  connected: string;
};

export function runtimeResolution(
  status: RuntimeQualityStatusLabelData | null,
): string {
  if (!status?.surface_width || !status.surface_height) return "--";
  return `${status.surface_width} x ${status.surface_height}`;
}

export function runtimeAaLabel(
  status: RuntimeQualityStatusLabelData | null,
): string {
  return aaModeLabel(status?.aa ?? null);
}

export function qualitySummaryLabel(setting: ProfileQualityLabelData): string {
  const textureLimit =
    setting.texture_resolution_limit === "off"
      ? "Unlimited"
      : textureModeLabel(setting.texture_resolution_limit);
  return `AA: ${aaModeLabel(setting.aa)} / Tex: ${textureLimit}`;
}

export function texturePolicyLabel(
  status: RuntimeQualityStatusLabelData | null,
  pendingLabel: string,
): string {
  if (!status?.connected) return pendingLabel;
  const limit = textureModeLabel(status.texture_resolution_limit);
  const compression = textureModeLabel(status.texture_compression);
  const cache =
    status.processed_texture_cache == null
      ? "cache --"
      : status.processed_texture_cache
        ? "cache on"
        : "cache off";
  return `${limit} / ${compression} / ${cache}`;
}

export function textureSummaryLabel(
  status: RuntimeQualityStatusLabelData | null,
): string {
  const summary = status?.texture_summary;
  if (!summary) return "--";
  const resized =
    summary.resized_count > 0 ? `, ${summary.resized_count} resized` : "";
  const compressed =
    summary.compressed_count > 0
      ? `, ${summary.compressed_count} compressed`
      : "";
  const fallback =
    summary.compression_fallback_count > 0
      ? `, ${summary.compression_fallback_count} fallback`
      : "";
  return `${summary.image_count} images${resized}${compressed}${fallback}, ${formatBytes(summary.uploaded_mip_bytes)} uploaded`;
}

export function textureCacheLabel(
  status: RuntimeQualityStatusLabelData | null,
): string {
  const summary = status?.texture_summary;
  if (!summary) return "--";
  if (!summary.cache_enabled) return "cache disabled";
  return `cache ${summary.cache_hits}/${summary.cache_misses}/${summary.cache_writes}, compressed ${summary.compressed_cache_hits}/${summary.compressed_cache_misses}/${summary.compressed_cache_writes}`;
}

export function startupStatusLabel(
  status: RuntimeStartupStatusLabelData | null,
): string | null {
  if (!status?.startup_phase) return null;
  const message = status.startup_message ?? status.startup_phase;
  const progress = status.startup_progress;
  if (progress && progress[1] > 0) {
    return `${message} ${progress[0]}/${progress[1]}`;
  }
  return message;
}

export function startupProgressPercent(
  status: RuntimeStartupStatusLabelData | null,
): number {
  const progress = status?.startup_progress;
  if (!progress || progress[1] <= 0) return 0;
  return Math.max(0, Math.min(100, (progress[0] / progress[1]) * 100));
}

export function rendererHealthKind(
  renderer: RendererHealthRendererData,
  status: RendererHealthStatusData | null,
): RendererHealthKind {
  if (renderer.state === "Crashed") return "error";
  if (renderer.state === "Degraded") return "warn";
  if (renderer.state === "Exited") return "idle";
  if (renderer.state === "Stopping" || renderer.state === "Starting")
    return "pending";
  if (!status?.connected) return "pending";
  if (renderer.spout_enabled) {
    if (!status.spout_available) return "warn";
    if (status.spout_enabled && status.spout_last_send_ok === false)
      return "warn";
  }
  return "ok";
}

export function rendererHealthLabel(
  renderer: RendererHealthRendererData,
  status: RendererHealthStatusData | null,
  labels: RuntimeStatusLabels,
): string {
  const kind = rendererHealthKind(renderer, status);
  if (kind === "error") return "Crashed";
  if (kind === "idle") return "Idle";
  if (kind === "pending") return status?.startup_message ?? labels.pending;
  if (kind === "warn") {
    if (renderer.spout_enabled && !status?.spout_available)
      return "Spout unavailable";
    if (status?.spout_last_send_ok === false) return "Spout failing";
    return "Attention";
  }
  return labels.connected;
}

export function runtimeOutputLabel(
  renderer: OutputLabelData,
  status: RuntimeOutputStatusData | null | undefined,
): string {
  if (!status?.connected) return outputLabel(renderer);
  if (renderer.spout_enabled && !status.spout_available)
    return "Spout2 unavailable";
  return status.spout_enabled
    ? status.spout_name
      ? `Spout2 / ${status.spout_name}`
      : "Spout2"
    : "Window";
}

export function spoutHealthLabel(
  status: RuntimeSpoutStatusLabelData | null,
  pendingLabel: string,
): string {
  if (!status?.connected) return pendingLabel;
  if (!status.spout_enabled) return "Disabled";
  if (!status.spout_available) return "Backend unavailable";
  if (status.spout_frames_attempted === 0) return "Waiting for first frame";
  const failed = status.spout_frame_failures;
  const total = status.spout_frames_attempted;
  const state = status.spout_last_send_ok === false ? "Failing" : "Sending";
  return `${state}: ${status.spout_frames_sent}/${total} frames, ${failed} failed`;
}

export function spoutTimingLabel(status: RuntimeSpoutStatusLabelData | null): string {
  if (!status?.spout_enabled || status.spout_frames_attempted === 0)
    return "--";
  return `read ${runtimeMetric(status.spout_last_readback_ms, " ms")} / send ${runtimeMetric(status.spout_last_send_ms, " ms")} / total ${runtimeMetric(status.spout_last_total_ms, " ms")}`;
}

export function spoutSenderLabel(status: RuntimeSpoutStatusLabelData | null): string {
  if (!status?.spout_enabled) return "--";
  if (status.spout_sender_initialized == null) return "sender state unknown";
  const init = status.spout_sender_initialized
    ? "initialized"
    : "not initialized";
  const size =
    status.spout_sender_width && status.spout_sender_height
      ? `${status.spout_sender_width} x ${status.spout_sender_height}`
      : "size unknown";
  return `${init}, ${size}`;
}

export function runtimeConnectionLabel(
  status: RuntimeConnectionStatusLabelData | null,
  labels: RuntimeStatusLabels,
): string {
  if (!status?.connected) return labels.pending;
  return labels.connected;
}
