import type {
  MotionLabelData,
  OutputLabelData,
  WindowLabelData,
} from "./profileLabels";
import type {
  RuntimeConnectionStatusLabelData,
  RuntimeQualityStatusLabelData,
  RuntimeStartupStatusLabelData,
} from "./runtimeLabels";

export type RendererState =
  | "Starting"
  | "Running"
  | "Stopping"
  | "Exited"
  | "Crashed"
  | "Degraded";

export type PrimaryMotionSource = "vmc" | "unmotion_zenoh";

export type TextureRuntimeSummary = {
  image_count: number;
  resized_count: number;
  compression_mode: string | null;
  compression_bc_supported: boolean;
  compression_astc_supported: boolean;
  compression_etc2_supported: boolean;
  compressed_count: number;
  compression_fallback_count: number;
  compressed_mip_bytes: number;
  cache_enabled: boolean;
  cache_hits: number;
  cache_misses: number;
  cache_writes: number;
  compressed_cache_hits: number;
  compressed_cache_misses: number;
  compressed_cache_writes: number;
  source_bytes: number;
  uploaded_mip_bytes: number;
  max_source_dimension: number;
  max_uploaded_dimension: number;
  limit_max_dimension: number | null;
};

export type RendererPaneTab =
  | "overview"
  | "controls"
  | "expressions"
  | "diagnostics";

export type RendererCameraSnapshot = {
  target: [number, number, number];
  longitude_deg: number;
  latitude_deg: number;
  radius: number;
  diagonal_fov_deg: number;
};

export type RendererWindowPatch = {
  decorations?: boolean;
  transparent?: boolean;
  inputPassthrough?: boolean;
  alwaysOnTop?: boolean;
  minimized?: boolean;
};

export type RendererRef = {
  id: number;
  pid: number | null;
};

export type RendererIdentityView = RendererRef & {
  name: string;
  state: string;
  avatar_path: string | null;
  manifest_path: string | null;
};

export type RendererOutputView = RendererIdentityView
  & MotionLabelData
  & OutputLabelData;

export type RendererStageView = RendererOutputView & WindowLabelData;

export type RendererTableView = RendererOutputView & {
  exit_code: number | null;
};

export type RendererInstance = RendererStageView & {
  pid: number | null;
  uptime_secs: number;
  avatar_path: string | null;
  manifest_path: string | null;
  spout_width: number | null;
  spout_height: number | null;
  aa: string;
  window_width: number;
  window_height: number;
  last_stderr: string | null;
  stderr_tail: string[];
  exit_code: number | null;
  primary_motion_source: PrimaryMotionSource;
};

export type RendererRuntimeDynamicsGroupStatus = {
  index: number;
  source_kind: string;
  authored_enabled: boolean;
  effective_enabled: boolean;
  source_id?: string;
  comment?: string;
  category?: string;
  bone_count: number;
  root_node?: number;
  root_path?: string;
  tip_node?: number;
  tip_path?: string;
  stiffness: number;
  drag_force: number;
  gravity_power: number;
  gravity_dir: [number, number, number];
  hit_radius: number;
  center_node?: number;
  center_path?: string;
  limit_type?: string;
  max_angle_x?: number;
  max_angle_z?: number;
  max_stretch?: number;
  allow_grabbing?: boolean;
  allow_posing?: boolean;
};

export type RendererRuntimeDynamicsColliderStatus = {
  index: number;
  source_kind: string;
  node: number;
  node_path?: string;
  shape: string;
  radius: number;
  height: number;
  position: [number, number, number];
  rotation: [number, number, number, number];
  inside_bounds: boolean;
};

export type RendererRuntimeContactParameterDeclarationStatus = {
  owner_key: string;
  source_id?: string;
  node: number;
  node_path?: string;
  parameter: string;
  collision_tags?: string[];
};

export type RendererRuntimeContactProbeStatus = {
  index: number;
  receiver_index: number;
  sender_index: number;
  receiver_source_id?: string;
  sender_source_id?: string;
  receiver_node: number;
  receiver_node_path?: string;
  sender_node: number;
  sender_node_path?: string;
  parameter: string;
  matched_tags?: string[];
  tag_match: boolean;
  overlap: boolean;
  would_emit: boolean;
  distance: number;
  threshold: number;
  receiver_radius: number;
  sender_radius: number;
  receiver_shape: string;
  sender_shape: string;
  approximation: string;
};

export type RendererRuntimeDynamicsConstraintRefStatus = {
  index: number;
  source_kind: string;
  source_id?: string;
  target_node: number;
  target_path?: string;
  source_nodes?: number[];
  source_paths?: string[];
  constraint_type?: string;
  weight: number;
};

export type RendererRuntimeStatus = {
  id: number;
  state: RendererState;
  pid: number | null;
  connected: boolean;
  protocol: string | null;
  control_capabilities: string[];
  uptime_secs: number;
  fps: number | null;
  cpu_ms: number | null;
  gpu_ms: number | null;
  ram_mb: number | null;
  surface_width: number | null;
  surface_height: number | null;
  aa: string | null;
  texture_resolution_limit: string | null;
  texture_compression: string | null;
  mipmap_filter: string | null;
  processed_texture_cache: boolean | null;
  texture_summary: TextureRuntimeSummary | null;
  spout_available: boolean;
  spout_enabled: boolean;
  spout_name: string | null;
  spout_width: number | null;
  spout_height: number | null;
  spout_frames_attempted: number;
  spout_frames_sent: number;
  spout_frame_failures: number;
  spout_consecutive_failures: number;
  spout_last_send_ok: boolean | null;
  spout_last_readback_ms: number | null;
  spout_last_send_ms: number | null;
  spout_last_total_ms: number | null;
  spout_sender_initialized: boolean | null;
  spout_sender_width: number | null;
  spout_sender_height: number | null;
  expression_presets: string[];
  look_at_enabled: boolean;
  look_at_clamp_deg: number | null;
  apply_vmc_root_translation: boolean;
  unmotion_zenoh_enabled: boolean;
  unmotion_zenoh_key: string;
  unmotion_zenoh_received_frames: number;
  motion_applied_frames: number;
  audio_link_texture_needed: boolean;
  unmotion_zenoh_received_fps?: number;
  motion_applied_fps?: number;
  primary_motion_source: PrimaryMotionSource;
  show_axes: boolean;
  show_bone_colliders: boolean;
  bone_collider_count: number;
  bone_collider_source: string;
  dynamics_group_count: number;
  dynamics_enabled_group_count: number;
  dynamics_source_enabled_group_count: number;
  dynamics_enabled_override_count: number;
  dynamics_vrm_spring_bone_group_count: number;
  dynamics_vrc_physbone_group_count: number;
  dynamics_unknown_group_count: number;
  dynamics_limit_group_count: number;
  dynamics_angle_limit_group_count: number;
  dynamics_stretch_limit_group_count: number;
  dynamics_grabbing_enabled_group_count: number;
  dynamics_posing_enabled_group_count: number;
  dynamics_collider_count: number;
  dynamics_vrm_spring_bone_collider_count: number;
  dynamics_vrc_physbone_collider_count: number;
  dynamics_unknown_collider_count: number;
  dynamics_contact_count: number;
  dynamics_vrc_contact_sender_count: number;
  dynamics_vrc_contact_receiver_count: number;
  dynamics_contact_parameter_declaration_count: number;
  dynamics_contact_probe_count: number;
  dynamics_contact_probe_would_emit_count: number;
  dynamics_constraint_ref_count: number;
  dynamics_vrc_constraint_ref_count: number;
  contact_parameter_declarations: RendererRuntimeContactParameterDeclarationStatus[];
  contact_probes: RendererRuntimeContactProbeStatus[];
  dynamics_groups: RendererRuntimeDynamicsGroupStatus[];
  dynamics_colliders: RendererRuntimeDynamicsColliderStatus[];
  dynamics_constraint_refs: RendererRuntimeDynamicsConstraintRefStatus[];
  camera_locked: boolean;
  window_focused: boolean;
  window_activation_seq: number;
  minimized: boolean;
  camera: RendererCameraSnapshot | null;
  clear_color: [number, number, number, number];
  transparent_window: boolean;
  input_passthrough: boolean;
  startup_phase: string | null;
  startup_progress: [number, number] | null;
  startup_message: string | null;
  window_position: [number, number] | null;
  window_inner_size: [number, number] | null;
  note: string | null;
};

export type RendererOverviewMotionStatus = Omit<Pick<
  RendererRuntimeStatus,
  | "unmotion_zenoh_enabled"
  | "unmotion_zenoh_received_frames"
  | "unmotion_zenoh_received_fps"
  | "motion_applied_frames"
  | "motion_applied_fps"
>, "unmotion_zenoh_received_fps" | "motion_applied_fps"> & {
  unmotion_zenoh_received_fps?: number | null;
  motion_applied_fps?: number | null;
};

export type RendererDiagnosticsData = Pick<
  RendererInstance,
  "last_stderr" | "stderr_tail" | "exit_code"
>;

export type RendererRuntimeDiagnosticsData = Pick<
  RendererRuntimeStatus,
  | "note"
  | "dynamics_group_count"
  | "dynamics_enabled_group_count"
  | "dynamics_collider_count"
  | "dynamics_contact_count"
  | "dynamics_contact_parameter_declaration_count"
  | "dynamics_contact_probe_count"
  | "dynamics_contact_probe_would_emit_count"
  | "dynamics_constraint_ref_count"
  | "dynamics_groups"
  | "dynamics_colliders"
  | "contact_parameter_declarations"
  | "contact_probes"
  | "dynamics_constraint_refs"
>;

export type RendererStageActionData = Pick<
  RendererInstance,
  "id" | "pid" | "manifest_path"
>;

export type RendererStageProfile = {
  id: string;
};

export type RendererOverviewData = MotionLabelData & Pick<
  RendererInstance,
  "name" | "pid" | "uptime_secs" | "manifest_path"
>;

export type RendererOverviewRuntimeStatus =
  & RuntimeConnectionStatusLabelData
  & RuntimeStartupStatusLabelData
  & Pick<RendererRuntimeStatus, "control_capabilities" | "note">;

export type RendererOverviewStatus =
  & RendererOverviewRuntimeStatus
  & RuntimeQualityStatusLabelData
  & RendererOverviewMotionStatus
  & Pick<RendererRuntimeStatus, "uptime_secs">;
