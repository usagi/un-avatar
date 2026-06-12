import type { MotionLabelData, OutputLabelData, WindowLabelData } from "./profileLabels";
import type { RuntimeConnectionStatusLabelData, RuntimeQualityStatusLabelData, RuntimeStartupStatusLabelData } from "./runtimeLabels";

export type RendererState = "Starting" | "Running" | "Stopping" | "Exited" | "Crashed" | "Degraded";

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

export type RendererPaneTab = "overview" | "controls" | "expressions" | "diagnostics";

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

export type RendererOutputView = RendererIdentityView & MotionLabelData & OutputLabelData;

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
	runtime_enabled_override?: boolean;
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
	writeback_mode?: string;
	allow_grabbing?: boolean;
	allow_posing?: boolean;
	interaction_parameter?: string;
};

export type RendererRuntimeDynamicsInteractionHookStatus = {
	group_index: number;
	source_kind: string;
	authored_enabled: boolean;
	effective_enabled: boolean;
	source_id?: string;
	root_path?: string;
	allow_grabbing: boolean;
	allow_posing: boolean;
	parameter?: string;
	suffix_parameters?: string[];
	metadata_only: boolean;
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

export type RendererRuntimeContactParameterEmissionStatus = {
	owner_key: string;
	source_id?: string;
	receiver_index: number;
	receiver_node: number;
	receiver_node_path?: string;
	parameter: string;
	value: number;
	emitted: boolean;
	sender_source_ids?: string[];
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

export type RendererRuntimeActionTargetWrite = {
	owner_key: string;
	action_id: string;
	effect_kind: string;
	target_kind: string;
	target_key: string;
};

export type RendererRuntimeActionTargetWriteCollision = {
	target_kind: string;
	target_key: string;
	owner_keys: string[];
	action_ids: string[];
	writes: RendererRuntimeActionTargetWrite[];
};

export type RendererRuntimeActionRestoreReadiness = {
	owner_key: string;
	action_id: string;
	effect_kind: string;
	target_kind: string;
	target_key: string;
	restore_target: boolean;
	current_value_available: boolean;
	current_value?: unknown;
	baseline_required: boolean;
	ready: boolean;
	reason: string;
};

export type RendererRuntimeActionRestoreBaselineCandidate = {
	owner_key: string;
	action_id: string;
	effect_kind: string;
	target_kind: string;
	target_key: string;
	baseline_value: unknown;
};

export type RendererRuntimeActionRestoreBaselineEntry = {
	owner_key: string;
	target_kind: string;
	target_key: string;
	baseline_value: unknown;
	source_action_ids: string[];
	source_effect_kinds: string[];
};

export type RendererRuntimeActionRestoreApplyEntry = {
	owner_key: string;
	action_id: string;
	condition_state?: string;
	target_kind: string;
	target_key: string;
	baseline_value?: unknown;
	current_value_available: boolean;
	current_value?: unknown;
	ready: boolean;
	reason: string;
};

export type RendererRuntimeMenuWardrobeCandidateStatus = {
	menu_component_index?: number;
	menu_key?: string;
	menu_path?: string[];
	menu_path_truncated?: boolean;
	menu_label?: string;
	action_id: string;
	wardrobe_set_id: string;
	match_kind: string;
	inverted: boolean;
};

export type RendererRuntimeParameterDefinition = {
	name: string;
	owner_keys?: string[];
	source_kinds?: string[];
	value_samples?: number[];
	current_value?: number;
	transient?: boolean;
};

export type RendererRuntimeParameterConflict = {
	name: string;
	reason: string;
	owner_keys?: string[];
	source_kinds?: string[];
	value_samples?: number[];
};

export type RendererRuntimeActionStatus = {
	action_id: string;
	label?: string;
	effect_count?: number;
	effect_kinds?: Record<string, number>;
	wardrobe_set_id?: string;
	expression_menu_path?: string;
	supervisor_command?: string;
	parameter_name?: string;
	parameter_value?: number;
	condition_parameter_names?: string[];
	current_condition_state?: string;
	target_writes?: RendererRuntimeActionTargetWrite[];
	node_visibility_effects?: {
		node_index?: number;
		source_node_id?: string;
		resolved_node_id?: string;
		path?: string;
		visible: boolean;
	}[];
	material_property_effects?: {
		property_kind: string;
		material_index?: number;
		material_name?: string;
		parameter: string;
		scalar_value?: number;
		color_value?: [number, number, number, number];
	}[];
	material_slot_effects?: {
		node_index?: number;
		source_node_id?: string;
		resolved_node_id?: string;
		path?: string;
		primitive_index?: number;
		material_index?: number;
		material_name?: string;
	}[];
	expression_weight_effects?: {
		name: string;
		weight: number;
	}[];
	dynamics_enabled_effects?: {
		source_id: string;
		enabled: boolean;
	}[];
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
	active_wardrobe_set: string | null;
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
	dynamics_contact_parameter_emission_count: number;
	dynamics_contact_parameter_emitted_count: number;
	dynamics_contact_parameter_reset_to_zero_count: number;
	dynamics_constraint_ref_count: number;
	dynamics_vrc_constraint_ref_count: number;
	runtime_actions: RendererRuntimeActionStatus[];
	runtime_action_target_write_collisions: RendererRuntimeActionTargetWriteCollision[];
	runtime_action_restore_readiness: RendererRuntimeActionRestoreReadiness[];
	runtime_action_restore_baseline_candidates: RendererRuntimeActionRestoreBaselineCandidate[];
	runtime_action_restore_baseline_capture_plan: RendererRuntimeActionRestoreBaselineEntry[];
	runtime_action_restore_apply_plan: RendererRuntimeActionRestoreApplyEntry[];
	runtime_parameter_definitions: RendererRuntimeParameterDefinition[];
	runtime_parameter_conflicts: RendererRuntimeParameterConflict[];
	menu_wardrobe_candidates: RendererRuntimeMenuWardrobeCandidateStatus[];
	contact_parameter_declarations: RendererRuntimeContactParameterDeclarationStatus[];
	contact_parameter_emission_enabled: boolean;
	contact_parameter_emissions: RendererRuntimeContactParameterEmissionStatus[];
	contact_probes: RendererRuntimeContactProbeStatus[];
	dynamics_groups: RendererRuntimeDynamicsGroupStatus[];
	dynamics_interaction_hooks: RendererRuntimeDynamicsInteractionHookStatus[];
	dynamics_colliders: RendererRuntimeDynamicsColliderStatus[];
	dynamics_constraint_refs: RendererRuntimeDynamicsConstraintRefStatus[];
	dynamics_warnings: string[];
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

export type RendererOverviewMotionStatus = Omit<
	Pick<
		RendererRuntimeStatus,
		| "unmotion_zenoh_enabled"
		| "unmotion_zenoh_received_frames"
		| "unmotion_zenoh_received_fps"
		| "motion_applied_frames"
		| "motion_applied_fps"
	>,
	"unmotion_zenoh_received_fps" | "motion_applied_fps"
> & {
	unmotion_zenoh_received_fps?: number | null;
	motion_applied_fps?: number | null;
};

export type RendererDiagnosticsData = Pick<RendererInstance, "id" | "pid" | "last_stderr" | "stderr_tail" | "exit_code">;

export type RendererRuntimeDiagnosticsData = Pick<
	RendererRuntimeStatus,
	| "note"
	| "control_capabilities"
	| "dynamics_group_count"
	| "dynamics_enabled_group_count"
	| "dynamics_source_enabled_group_count"
	| "dynamics_enabled_override_count"
	| "dynamics_collider_count"
	| "dynamics_contact_count"
	| "dynamics_contact_parameter_declaration_count"
	| "dynamics_contact_probe_count"
	| "dynamics_contact_probe_would_emit_count"
	| "dynamics_contact_parameter_emission_count"
	| "dynamics_contact_parameter_emitted_count"
	| "dynamics_contact_parameter_reset_to_zero_count"
	| "dynamics_constraint_ref_count"
	| "dynamics_warnings"
	| "runtime_actions"
	| "runtime_action_target_write_collisions"
	| "runtime_action_restore_readiness"
	| "runtime_action_restore_baseline_candidates"
	| "runtime_action_restore_baseline_capture_plan"
	| "runtime_action_restore_apply_plan"
	| "runtime_parameter_definitions"
	| "runtime_parameter_conflicts"
	| "menu_wardrobe_candidates"
	| "dynamics_groups"
	| "dynamics_interaction_hooks"
	| "dynamics_colliders"
	| "contact_parameter_declarations"
	| "contact_parameter_emission_enabled"
	| "contact_parameter_emissions"
	| "contact_probes"
	| "dynamics_constraint_refs"
>;

export type RendererStageActionData = Pick<RendererInstance, "id" | "pid" | "manifest_path">;

export type RendererStageProfile = {
	id: string;
};

export type RendererOverviewData = MotionLabelData & Pick<RendererInstance, "name" | "pid" | "uptime_secs" | "manifest_path">;

export type RendererOverviewRuntimeStatus = RuntimeConnectionStatusLabelData &
	RuntimeStartupStatusLabelData &
	Pick<RendererRuntimeStatus, "control_capabilities" | "note">;

export type RendererOverviewStatus = RendererOverviewRuntimeStatus &
	RuntimeQualityStatusLabelData &
	RendererOverviewMotionStatus &
	Pick<RendererRuntimeStatus, "uptime_secs">;
