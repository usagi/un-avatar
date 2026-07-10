//! UN Avatar CLI（bootstrap）。`crate-io-plugin-plan.md` Phase 2.2 の最小版。
//!
//! サブコマンド例: `formats list`, `formats probe`, `convert`, `validate`, `inspect`, `vmc listen`。

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use clap::{Parser, Subcommand};
use glam::{EulerRot, Mat4, Quat, Vec3, Vec4};
use serde::Serialize;
use un_avatar_core::{
	modular_avatar_component_support_kind, morph_weights_for_primitive, una_dynamics_translation_writeback_candidate_count,
	una_dynamics_translation_writeback_target_count, UnaAlphaMode, UnaDynamicsSourceKind, UnaHumanoidRuntimeBasis, UnaImagePixelFormat,
	UnaMaterialPbr, UnaNodeConstraintKind, UnaRuntimeActionEffect, UnaRuntimeActionTrigger, UnaRuntimeResolverCacheKey,
	UnaRuntimeSourceKind, UnaRuntimeToonModel, UnaSceneSnapshot, UnaShadingModel,
};
use un_avatar_io::{
	path_has_format_extension, AvatarExporter, AvatarImporter, ExportCapability, ExportContext, ExportOptions, ExportOutput, ExportReport,
	FormatDescriptor, FormatId, ImportContext, ImportInput, ImportOptions, ImportProbe, ImportReport, IoRegistry, UnaDocument,
};
use un_avatar_io_gltf::{apply_unavatar_wardrobe_set, register_gltf_importer, WardrobeApplyReport};
use un_avatar_io_vrm::register_vrm_importer;
use un_avatar_plugin_host::{register_stdio_exporters_from_plugin_root, register_stdio_importers_from_plugin_root};
use un_avatar_skeleton::{
	annotate_dynamics_response_group_visibility, apply_dynamics_mesh_cloth_assist_to_vertices, build_dynamics_bone_colliders_with_sources,
	classify_dynamics_group_category, dynamics_mesh_cloth_assist_deforming_nodes, dynamics_mesh_cloth_assist_joint_roles,
	dynamics_mesh_cloth_assist_mesh_matches_with_categories as skeleton_mesh_cloth_assist_mesh_matches_with_categories,
	dynamics_mesh_cloth_assist_transfer_candidate, for_each_dynamics_mesh_cloth_assist_neighbor, local_capsule_world, local_plane_world,
	local_sphere_world, BoneColliderConfig, BoneColliderPrimitive, DynamicsCategoryDefinition, DynamicsMeshClothAssistConfig,
	DynamicsMeshClothAssistJointRole, DynamicsMeshClothAssistTransferKind, DynamicsMeshClothAssistVertex, DynamicsPhysicsConfig,
	DynamicsPhysicsParams, DynamicsResponseCategorySummary, DynamicsResponseGroupSummary, DynamicsSimulator, DynamicsStepProfile,
	DynamicsTailSample, DynamicsVisualTargetContext,
};

const DIAGNOSE_DYNAMICS_GROUP_TEXT_LIMIT: usize = 24;
const DIAGNOSE_TEXT_LIST_LIMIT: usize = 16;
const DYNAMICS_SCAN_REQUIRED_SOURCE_PARAM_KEYS: &[&str] = &[
	"pull",
	"pullCurve",
	"spring",
	"springCurve",
	"momentum",
	"momentumCurve",
	"stiffness",
	"stiffnessCurve",
	"gravityFalloff",
	"gravityFalloffCurve",
	"immobile",
	"immobileCurve",
	"immobileType",
	"integrationType",
	"limitRotation",
];

#[derive(Serialize)]
struct ConvertJsonReport {
	import_format_id: String,
	export_format_id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	import_provider_plugin_id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	export_provider_plugin_id: Option<String>,
	import_report: ImportReport,
	export_report: ExportReport,
}

#[derive(Serialize)]
struct FormatsListJson {
	importers: Vec<FormatDescriptor>,
	exporters: Vec<FormatDescriptor>,
}

#[derive(Serialize)]
struct ValidateReport {
	valid: bool,
	path: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	error: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	format_id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	provider_plugin_id: Option<String>,
}

#[derive(Serialize)]
struct InspectReport {
	path: String,
	import_format_id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	import_provider_plugin_id: Option<String>,
	import_report: ImportReport,
	document: InspectDocumentSummary,
}

#[derive(Serialize)]
struct InspectDocumentSummary {
	has_scene: bool,
	has_vrm: bool,
	has_unavatar: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	scene_lighting: Option<InspectSceneLightingSummary>,
	node_count: usize,
	root_count: usize,
	mesh_count: usize,
	mesh_primitive_count: usize,
	material_count: usize,
	image_count: usize,
	skin_count: usize,
	morph_target_count: usize,
}

#[derive(Serialize)]
struct InspectSceneLightingSummary {
	has_environment: bool,
	has_directional: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	environment_color: Option<[f32; 3]>,
	#[serde(skip_serializing_if = "Option::is_none")]
	environment_intensity: Option<f32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	directional_color: Option<[f32; 3]>,
	#[serde(skip_serializing_if = "Option::is_none")]
	directional_intensity: Option<f32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	directional_azimuth_deg: Option<f32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	directional_elevation_deg: Option<f32>,
}

#[derive(Serialize)]
struct DynamicsScanReport {
	path: String,
	file_bytes: u64,
	json_bytes: usize,
	extension_keys: Vec<String>,
	source_params_count: usize,
	required_source_param_counts: BTreeMap<String, usize>,
	missing_required_source_params: Vec<String>,
	source_param_key_counts: BTreeMap<String, usize>,
	numeric_ranges: BTreeMap<String, DynamicsScanNumericRange>,
	curve_counts: BTreeMap<String, usize>,
}

#[derive(Serialize, Clone, Copy)]
struct DynamicsScanNumericRange {
	count: usize,
	min: f64,
	max: f64,
}

#[derive(Serialize)]
struct DynamicsImportAuditReport {
	path: String,
	import_format_id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	import_provider_plugin_id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	active_wardrobe_set: Option<String>,
	import_report: ImportReport,
	source_params_count: usize,
	group_count: usize,
	source_kind_counts: BTreeMap<String, usize>,
	enabled_group_count: usize,
	chain_joint_count: usize,
	collider_count: usize,
	contact_count: usize,
	constraint_ref_count: usize,
	node_constraint_count: usize,
	parent_node_constraint_count: usize,
	parent_node_constraint_source_count: usize,
	parent_node_constraint_multi_source_count: usize,
	source_angle_limit_group_count: usize,
	active_angle_limit_group_count: usize,
	cloth_angle_limit_metadata_only_count: usize,
	hard_angle_constraint_group_count: usize,
	response_category_count: usize,
	response_group_count: usize,
	runtime_ranges: BTreeMap<String, DynamicsScanNumericRange>,
	sample_counts: BTreeMap<String, usize>,
	group_samples: Vec<DynamicsImportGroupSample>,
	node_samples: Vec<DynamicsImportNodeSample>,
	skin_samples: Vec<DynamicsImportSkinSample>,
	mesh_cloth_assist_samples: Vec<DynamicsImportMeshClothAssistSample>,
	missing_runtime_evidence: Vec<String>,
}

#[derive(Serialize)]
struct DynamicsImportGroupSample {
	source_id: String,
	category: String,
	enabled: bool,
	source_kind: String,
	chain_len: usize,
	root_path: Option<String>,
	tip_path: Option<String>,
	chain_paths: Vec<String>,
}

#[derive(Serialize)]
struct DynamicsImportNodeSample {
	index: usize,
	name: Option<String>,
	path: Option<String>,
	parent_index: Option<usize>,
	parent_path: Option<String>,
	mesh: Option<usize>,
	skin: Option<usize>,
	children: Vec<usize>,
}

#[derive(Serialize)]
struct DynamicsImportSkinSample {
	node_index: usize,
	node_path: Option<String>,
	skin_index: usize,
	skeleton_node: Option<usize>,
	skeleton_path: Option<String>,
	joint_count: usize,
	joints: Vec<DynamicsImportSkinJointSample>,
	region_samples: Vec<DynamicsImportSkinRegionSample>,
}

#[derive(Serialize)]
struct DynamicsImportSkinJointSample {
	joint_index: usize,
	node_index: usize,
	name: Option<String>,
	path: Option<String>,
	parent_index: Option<usize>,
	parent_path: Option<String>,
}

#[derive(Serialize)]
struct DynamicsImportSkinRegionSample {
	primitive_index: usize,
	primitive_name: Option<String>,
	region: String,
	vertex_count: usize,
	dominant_counts: Vec<DynamicsImportSkinInfluenceSample>,
	weight_sums: Vec<DynamicsImportSkinInfluenceSample>,
}

#[derive(Serialize)]
struct DynamicsImportSkinInfluenceSample {
	joint_index: usize,
	node_index: usize,
	name: Option<String>,
	path: Option<String>,
	value: f32,
}

#[derive(Serialize)]
struct DynamicsImportMeshClothAssistSample {
	node_index: usize,
	node_path: Option<String>,
	mesh_index: usize,
	primitive_index: usize,
	primitive_name: Option<String>,
	region: String,
	vertex_count: usize,
	candidate_count: usize,
	existing_dynamic_candidate_count: usize,
	static_cloth_bridge_candidate_count: usize,
	seed_candidate_count: usize,
	body_weight_sum: f32,
	dynamic_weight_sum: f32,
	static_cloth_weight_sum: f32,
	suggested_assist_weight_sum: f32,
	seeded_assist_weight_sum: f32,
	body_sources: Vec<DynamicsImportSkinInfluenceSample>,
	dynamic_targets: Vec<DynamicsImportSkinInfluenceSample>,
}

#[derive(Serialize)]
struct DynamicsResponseAuditReport {
	path: String,
	import_format_id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	import_provider_plugin_id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	active_wardrobe_set: Option<String>,
	import_report: ImportReport,
	group_count: usize,
	joint_count: usize,
	modes: Vec<DynamicsResponseAuditMode>,
	missing_response_evidence: Vec<String>,
}

#[derive(Serialize)]
struct DynamicsResponseAuditMode {
	name: String,
	group_count: usize,
	joint_count: usize,
	average_rest_response: f32,
	average_shape_preservation: f32,
	average_bounce_response: f32,
	average_max_stretch_response: f32,
	average_max_squish_response: f32,
	average_stretch_motion_response: f32,
	average_damping_half_life_ms: Option<f32>,
	average_parent_motion_follow: f32,
	average_orientation_follow: f32,
	xpbd_group_count: usize,
	category_count: usize,
	categories: Vec<DynamicsResponseCategorySummary>,
	groups: Vec<DynamicsResponseGroupSummary>,
}

#[derive(Serialize)]
struct DynamicsMotionTraceAuditReport {
	path: String,
	import_format_id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	import_provider_plugin_id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	active_wardrobe_set: Option<String>,
	import_report: ImportReport,
	frame_count: usize,
	recovery_frame_count: usize,
	tuning: String,
	group_count: usize,
	joint_count: usize,
	categories: Vec<DynamicsMotionTraceCategorySummary>,
	groups: Vec<DynamicsMotionTraceGroupSummary>,
	findings: Vec<String>,
	finding_details: Vec<DynamicsMotionTraceFindingDetail>,
	finding_kind_counts: BTreeMap<String, usize>,
	missing_motion_evidence: Vec<String>,
}

#[derive(Serialize)]
struct DynamicsMotionTraceFindingDetail {
	kind: String,
	message: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	source_id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	category: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	visual_target: Option<bool>,
	#[serde(skip_serializing_if = "Option::is_none")]
	skinned_joint_count: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	mesh_subtree_node_count: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	interaction_metadata_only: Option<bool>,
	#[serde(skip_serializing_if = "Option::is_none")]
	tuning_hint: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	response_override_hint: Option<DynamicsMotionTraceResponseOverrideHint>,
}

#[derive(Serialize)]
struct DynamicsMotionTraceResponseOverrideHint {
	source_id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	rest_response: Option<f32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	damping_half_life_ms: Option<f32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	stretch_range_scale: Option<f32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	stretch_motion: Option<f32>,
}

#[derive(Serialize)]
struct DynamicsVertexProbeReport {
	path: String,
	wardrobe_set: Option<String>,
	tuning: String,
	node_index: usize,
	node_path: String,
	mesh_index: usize,
	skin_index: Option<usize>,
	settle_frames: usize,
	pose_left_upper_arm_z_deg: Option<f32>,
	pose_right_upper_arm_z_deg: Option<f32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	unmotion_frame_json: Option<String>,
	node_constraints_ignored: bool,
	authored_colliders_ignored: bool,
	runtime_collider_count: usize,
	solve_collision_projection_count: u32,
	solve_collision_projection_source_ids: Vec<String>,
	solve_collision_projection_source_counts: BTreeMap<String, u32>,
	probe_dynamic_source_ids: Vec<String>,
	probe_dynamic_source_weight_sums: BTreeMap<String, f32>,
	probe_collision_projection_count: u32,
	probe_collision_projection_source_ids: Vec<String>,
	probe_collision_projection_source_counts: BTreeMap<String, u32>,
	solve_collision_projection_collider_paths: Vec<String>,
	solve_collision_projection_collider_path_counts: BTreeMap<String, u32>,
	solve_collision_projection_source_collider_path_counts: BTreeMap<String, BTreeMap<String, u32>>,
	collider_path_summaries: Vec<DynamicsVertexProbeColliderPathSummary>,
	probe_collision_projection_collider_path_counts: BTreeMap<String, u32>,
	probe_collider_path_summaries: Vec<DynamicsVertexProbeColliderPathSummary>,
	mesh_cloth_assist_applied: bool,
	mesh_cloth_assist_changed_vertices: usize,
	node_samples: Vec<DynamicsVertexProbeNodeSample>,
	constraint_node_samples: Vec<DynamicsVertexProbeNodeSample>,
	probe_tail_samples: Vec<DynamicsTailSample>,
	interaction_parameters: Vec<DynamicsVertexProbeInteractionParameter>,
	animator_morph_overrides: Vec<DynamicsVertexProbeAnimatorMorphOverride>,
	animator_morph_override_regions: Vec<DynamicsVertexProbeRegionReport>,
	joint_weight_summaries: Vec<DynamicsVertexProbeJointWeightSummary>,
	regions: Vec<DynamicsVertexProbeRegionReport>,
	mirror_symmetry: Vec<DynamicsVertexProbeMirrorSymmetryReport>,
}

#[derive(Serialize)]
struct DynamicsVertexProbeInteractionParameter {
	parameter: String,
	angle_parameter: String,
	source_id: String,
	angle_value: f32,
	angle_norm: f32,
	angle_deg: f32,
	shape_angle_deg: f32,
	gravity_angle_deg: f32,
	dominant: String,
	max_angle_deg: f32,
	center_peak_scaled: bool,
	chain: Vec<String>,
}

#[derive(Serialize)]
struct DynamicsVertexProbeAnimatorMorphOverride {
	key: String,
	target_path: Option<String>,
	morph_name: String,
	value: f32,
}

#[derive(Clone)]
struct DynamicsVertexProbeInteractionValue {
	parameter: String,
	angle_parameter: String,
	source_id: String,
	angle_value: f32,
	angle_norm: f32,
	angle_deg: f32,
	shape_angle_deg: f32,
	gravity_angle_deg: f32,
	dominant: String,
	max_angle_deg: f32,
	center_peak_scaled: bool,
	chain: Vec<String>,
}

#[derive(Serialize)]
struct DynamicsVertexProbeColliderPathSummary {
	collider_path: String,
	collider_shape: String,
	inside_bounds: bool,
	candidate_count: usize,
	penetrating_count: usize,
	projection_count: u32,
	source_count: usize,
	min_margin: f32,
	min_distance: f32,
	min_threshold: f32,
	#[serde(skip_serializing_if = "Option::is_none")]
	min_margin_tail: Option<DynamicsVertexProbeColliderTailContact>,
	sample_source_ids: Vec<String>,
}

#[derive(Serialize)]
struct DynamicsVertexProbeColliderTailContact {
	source_id: String,
	runtime_index: usize,
	joint_index: usize,
	anchor_pos: [f32; 3],
	tail_pos: [f32; 3],
	closest_pos: [f32; 3],
	collider_a: Option<[f32; 3]>,
	collider_b: Option<[f32; 3]>,
	push_dir: [f32; 3],
}

#[derive(Serialize)]
struct DynamicsVertexProbeNodeSample {
	node_index: usize,
	path: String,
	rest_translation: [f32; 3],
	settled_translation: [f32; 3],
	delta: [f32; 3],
	displacement: f32,
}

#[derive(Serialize)]
struct DynamicsVertexProbeRegionReport {
	name: String,
	vertex_count: usize,
	dominant_joints: Vec<DynamicsVertexProbeJointCount>,
	morph_targets: Vec<DynamicsVertexProbeMorphTargetRegionSummary>,
	average_displacement: f32,
	max_displacement: f32,
	average_delta: [f32; 3],
	least_moved_samples: Vec<DynamicsVertexProbeVertexSample>,
	most_moved_samples: Vec<DynamicsVertexProbeVertexSample>,
}

#[derive(Serialize)]
struct DynamicsVertexProbeMorphTargetRegionSummary {
	index: usize,
	name: String,
	default_weight: f32,
	affected_vertices: usize,
	average_delta: f32,
	max_delta: f32,
}

#[derive(Serialize)]
struct DynamicsVertexProbeJointCount {
	joint: String,
	count: usize,
}

#[derive(Serialize)]
struct DynamicsVertexProbeJointWeightSummary {
	joint: String,
	node_index: Option<usize>,
	vertex_count: usize,
	dominant_vertex_count: usize,
	weight_sum: f32,
	max_weight: f32,
	average_weight: f32,
	average_position: [f32; 3],
	bounds_min: [f32; 3],
	bounds_max: [f32; 3],
}

#[derive(Clone, Serialize)]
struct DynamicsVertexProbeVertexSample {
	vertex_index: usize,
	position: [f32; 3],
	settled_position: [f32; 3],
	delta: [f32; 3],
	displacement: f32,
	dominant_joint: String,
	dominant_weight: f32,
	influences: Vec<DynamicsVertexProbeInfluence>,
}

#[derive(Clone, Serialize)]
struct DynamicsVertexProbeInfluence {
	joint: String,
	weight: f32,
}

#[derive(Serialize)]
struct DynamicsVertexProbeMirrorSymmetryReport {
	name: String,
	left_vertex_count: usize,
	right_vertex_count: usize,
	average_left_to_right_distance: f32,
	max_left_to_right_distance: f32,
	average_right_to_left_distance: f32,
	max_right_to_left_distance: f32,
	worst_left_samples: Vec<DynamicsVertexProbeMirrorSample>,
	worst_right_samples: Vec<DynamicsVertexProbeMirrorSample>,
}

#[derive(Clone, Serialize)]
struct DynamicsVertexProbeMirrorSample {
	vertex_index: usize,
	position: [f32; 3],
	mirrored_position: [f32; 3],
	nearest_vertex_index: usize,
	nearest_position: [f32; 3],
	mirror_distance: f32,
	dominant_joint: String,
	dominant_weight: f32,
	influences: Vec<DynamicsVertexProbeInfluence>,
}

#[derive(Serialize, Clone)]
struct DynamicsMotionTraceCategorySummary {
	category: String,
	group_count: usize,
	joint_count: usize,
	visual_target_group_count: usize,
	nonvisual_group_count: usize,
	visible_skinned_joint_count: usize,
	visible_mesh_subtree_node_count: usize,
	average_chain_rest_length: f32,
	max_lag: f32,
	max_lag_chain_ratio: f32,
	average_lag: f32,
	final_lag: f32,
	final_lag_chain_ratio: f32,
	recovery_final_lag: f32,
	recovery_ratio: f32,
	initial_stable_offset: f32,
	settled_recovery_lag: f32,
	stable_offset: f32,
	stable_offset_chain_ratio: f32,
	stable_offset_ratio: f32,
	recovery_state: String,
	settled_recovery_ratio: f32,
	residual_motion: f32,
	residual_motion_chain_ratio: f32,
	#[serde(skip_serializing_if = "Option::is_none")]
	recovery_half_life_frames: Option<f32>,
	average_rest_response: f32,
	average_shape_preservation: f32,
	average_bounce_response: f32,
	average_parent_motion_follow: f32,
	average_orientation_follow: f32,
	average_max_stretch_response: f32,
	average_stretch_motion_response: f32,
}

#[derive(Serialize, Clone)]
struct DynamicsMotionTraceGroupSummary {
	source_id: String,
	category: String,
	joint_count: usize,
	visual_target: bool,
	skinned_joint_count: usize,
	mesh_subtree_node_count: usize,
	interaction_metadata_only: bool,
	chain_rest_length: f32,
	max_lag: f32,
	max_lag_chain_ratio: f32,
	average_lag: f32,
	final_lag: f32,
	final_lag_chain_ratio: f32,
	recovery_final_lag: f32,
	recovery_ratio: f32,
	initial_stable_offset: f32,
	settled_recovery_lag: f32,
	stable_offset: f32,
	stable_offset_chain_ratio: f32,
	stable_offset_ratio: f32,
	recovery_state: String,
	settled_recovery_ratio: f32,
	residual_motion: f32,
	residual_motion_chain_ratio: f32,
	#[serde(skip_serializing_if = "Option::is_none")]
	recovery_half_life_frames: Option<f32>,
	average_rest_response: f32,
	average_shape_preservation: f32,
	average_bounce_response: f32,
	average_parent_motion_follow: f32,
	average_orientation_follow: f32,
	average_max_stretch_response: f32,
	average_stretch_motion_response: f32,
}

#[derive(Serialize)]
struct DiagnoseReport {
	path: String,
	import_format_id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	import_provider_plugin_id: Option<String>,
	timings: DiagnoseTimingSummary,
	import_report: ImportReport,
	runtime: DiagnoseRuntimeSummary,
	scene: DiagnoseSceneSummary,
	#[serde(skip_serializing_if = "Option::is_none")]
	humanoid: Option<DiagnoseHumanoidSummary>,
	#[serde(skip_serializing_if = "Option::is_none")]
	expressions: Option<DiagnoseExpressionSummary>,
	#[serde(skip_serializing_if = "Option::is_none")]
	actions: Option<DiagnoseActionSummary>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	menu_action_candidates: Vec<DiagnoseMenuActionCandidate>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	menu_wardrobe_candidates: Vec<DiagnoseMenuWardrobeCandidate>,
	dynamics: DiagnoseDynamicsSummary,
	#[serde(skip_serializing_if = "Option::is_none")]
	vrm: Option<DiagnoseVrmSummary>,
	#[serde(skip_serializing_if = "Option::is_none")]
	unavatar: Option<DiagnoseUnavatarSummary>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	wardrobe_probes: Vec<DiagnoseWardrobeProbeSummary>,
	warnings: Vec<String>,
}

#[derive(Serialize)]
struct DiagnoseTimingSummary {
	import_ms: u128,
	wardrobe_apply_ms: u128,
	wardrobe_probe_ms: u128,
	report_build_ms: u128,
}

#[derive(Serialize)]
struct DiagnoseRuntimeSummary {
	source_kind: UnaRuntimeSourceKind,
	humanoid_basis: UnaHumanoidRuntimeBasis,
	#[serde(skip_serializing_if = "Option::is_none")]
	active_wardrobe_set: Option<String>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	active_asset_groups: Vec<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	last_action_id: Option<String>,
	#[serde(skip_serializing_if = "BTreeMap::is_empty")]
	parameter_values: BTreeMap<String, f32>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	parameter_definitions: Vec<un_avatar_core::UnaRuntimeParameterDefinition>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	parameter_conflicts: Vec<un_avatar_core::UnaRuntimeParameterConflict>,
	resolver_cache_key: UnaRuntimeResolverCacheKey,
}

#[derive(Serialize)]
struct DiagnoseSceneSummary {
	has_scene: bool,
	mesh_count: usize,
	primitive_count: usize,
	morph_target_count: usize,
	node_count: usize,
	hidden_node_count: usize,
	skin_count: usize,
	image_count: usize,
	image_source_count: usize,
	image_source_bytes: u64,
	image_source_mime_counts: BTreeMap<String, usize>,
	image_source_color_space_counts: BTreeMap<String, usize>,
	image_source_texture_type_counts: BTreeMap<String, usize>,
	image_source_texture_shape_counts: BTreeMap<String, usize>,
	image_source_layout_counts: BTreeMap<String, usize>,
	image_pixel_format_counts: BTreeMap<String, usize>,
	non_rgba8_image_count: usize,
	largest_image_sources: Vec<DiagnoseImageSourceSummary>,
	material_count: usize,
	liltoon_feature_counts: BTreeMap<String, usize>,
	node_constraint_count: usize,
	node_constraint_kind_counts: BTreeMap<String, usize>,
	parent_node_constraint_source_count: usize,
	parent_node_constraint_multi_source_count: usize,
	asset_group_ownership_count: usize,
	asset_group_owned_mesh_primitive_count: usize,
	asset_group_owned_material_count: usize,
	asset_group_owned_image_count: usize,
	asset_group_owned_dynamics_count: usize,
	asset_group_ownership: Vec<DiagnoseAssetGroupOwnershipSummary>,
	scoped_active_asset_group_count: usize,
	scoped_missing_active_asset_groups: Vec<String>,
	scoped_resident_mesh_primitive_count: usize,
	scoped_resident_material_count: usize,
	scoped_resident_image_count: usize,
	scoped_resident_dynamics_count: usize,
	shading_counts: BTreeMap<String, usize>,
	alpha_counts: BTreeMap<String, usize>,
	visible_shading_counts: BTreeMap<String, usize>,
	visible_alpha_counts: BTreeMap<String, usize>,
	visible_material_indices: Vec<usize>,
	eye_like_material_indices: Vec<usize>,
	skins: Vec<DiagnoseSkinSummary>,
	materials: Vec<DiagnoseMaterialSummary>,
	visible_mesh_nodes: Vec<DiagnoseVisibleMeshNodeSummary>,
}

fn diagnose_node_constraint_kind(kind: &UnaNodeConstraintKind) -> &'static str {
	match kind {
		UnaNodeConstraintKind::Roll { .. } => "roll",
		UnaNodeConstraintKind::Aim { .. } => "aim",
		UnaNodeConstraintKind::Rotation => "rotation",
		UnaNodeConstraintKind::Parent { .. } => "parent",
	}
}

#[derive(Serialize)]
struct DiagnoseAssetGroupOwnershipSummary {
	group_id: String,
	mesh_primitives: Vec<un_avatar_core::UnaMeshPrimitiveKey>,
	materials: Vec<usize>,
	images: Vec<usize>,
	dynamics_source_ids: Vec<String>,
}

#[derive(Serialize)]
struct DiagnoseSkinSummary {
	index: usize,
	joint_count: usize,
	inverse_bind_count: usize,
	effective_joint_count: usize,
	over_renderer_bone_limit: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	skeleton_node: Option<usize>,
	used_by_node_count: usize,
	primitive_joint_attribute_count: usize,
	primitive_weight_attribute_count: usize,
	mismatched_joint_weight_attribute_count: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	max_joint_index: Option<u16>,
	out_of_range_joint_attribute_count: usize,
}

#[derive(Serialize)]
struct DiagnoseVisibleMeshNodeSummary {
	node: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	name: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	source_node_id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	resolved_node_id: Option<String>,
	mesh: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	skin: Option<usize>,
	materials: Vec<DiagnoseVisibleMaterialSummary>,
}

#[derive(Serialize)]
struct DiagnoseVisibleMaterialSummary {
	primitive: usize,
	index: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	name: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	source_shader: Option<String>,
	shading: UnaShadingModel,
	alpha_mode: UnaAlphaMode,
	alpha_cutoff: f32,
	transparent_with_z_write: bool,
	draw_skipped_fully_transparent: bool,
	morph_target_count: usize,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	nonzero_morph_weights: Vec<DiagnoseMorphWeightSummary>,
}

#[derive(Serialize)]
struct DiagnoseMorphWeightSummary {
	index: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	name: Option<String>,
	weight: f32,
	position_delta_abs_sum: f32,
	normal_delta_abs_sum: f32,
}

#[derive(Serialize)]
struct DiagnoseImageSourceSummary {
	index: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	name: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	mime_type: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	uri: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	source_pixel_format: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	channels: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	color_space: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	texture_type: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	texture_shape: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	source_layout: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	unity_generate_cubemap: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	srgb: Option<bool>,
	byte_length: u64,
	pixel_format: UnaImagePixelFormat,
	width: u32,
	height: u32,
}

#[derive(Serialize)]
struct DiagnoseMaterialSummary {
	index: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	name: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	source_shader: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	material_family: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	render_queue: Option<i32>,
	source_float_param_count: usize,
	source_color_param_count: usize,
	#[serde(skip_serializing_if = "BTreeMap::is_empty")]
	source_render_float_params: BTreeMap<String, f32>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	liltoon_features: Vec<String>,
	shading: UnaShadingModel,
	alpha_mode: UnaAlphaMode,
	alpha_cutoff: f32,
	double_sided: bool,
	cull_mode: un_avatar_core::UnaCullMode,
	base_color_factor: [f32; 4],
	#[serde(skip_serializing_if = "Option::is_none")]
	base_color_texture_index: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	base_color_texture_alpha: Option<DiagnoseTextureAlphaSummary>,
	#[serde(skip_serializing_if = "Option::is_none")]
	normal_texture_index: Option<usize>,
	normal_texture_scale: f32,
	eye_like_name: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	mtoon: Option<DiagnoseMToonSummary>,
}

#[derive(Clone, Serialize)]
struct DiagnoseTextureAlphaSummary {
	image: usize,
	width: u32,
	height: u32,
	pixel_format: UnaImagePixelFormat,
	has_alpha_channel: bool,
	min_alpha: u8,
	max_alpha: u8,
	transparent_pixels: usize,
	translucent_pixels: usize,
	opaque_pixels: usize,
	coverage: f32,
}

#[derive(Serialize)]
struct DiagnoseMToonSummary {
	transparent_with_z_write: bool,
	shade_color_factor: [f32; 3],
	shade_multiply_texture_index: Option<usize>,
	shading_shift_factor: f32,
	shading_shift_texture_index: Option<usize>,
	shading_toony_factor: f32,
	gi_equalization_factor: f32,
	matcap_factor: [f32; 3],
	matcap_texture_index: Option<usize>,
	parametric_rim_color_factor: [f32; 3],
	rim_multiply_texture_index: Option<usize>,
	reflection_cube_texture_index: Option<usize>,
	outline_width_mode: un_avatar_core::UnaMtoonOutlineWidthMode,
	outline_width_factor: f32,
	outline_width_multiply_texture_index: Option<usize>,
	outline_color_factor: [f32; 3],
	emissive_factor: [f32; 3],
	emissive_texture_index: Option<usize>,
}

#[derive(Serialize)]
struct DiagnoseHumanoidSummary {
	bone_count: usize,
	keys: Vec<String>,
	left_eye_node: Option<usize>,
	right_eye_node: Option<usize>,
}

#[derive(Serialize)]
struct DiagnoseExpressionSummary {
	preset_count: usize,
	presets: Vec<DiagnoseExpressionPresetSummary>,
	#[serde(skip_serializing_if = "Option::is_none")]
	apply_probe: Option<DiagnoseExpressionApplyProbe>,
}

#[derive(Serialize)]
struct DiagnoseExpressionPresetSummary {
	name: String,
	bind_count: usize,
}

#[derive(Serialize)]
struct DiagnoseExpressionApplyProbe {
	weights: BTreeMap<String, f32>,
	active_morph_slots: Vec<DiagnoseExpressionMorphSlot>,
}

#[derive(Serialize)]
struct DiagnoseExpressionMorphSlot {
	mesh: usize,
	primitive: usize,
	active_count: usize,
	max_weight: f32,
}

#[derive(Serialize)]
struct DiagnoseActionSummary {
	action_count: usize,
	trigger_count: usize,
	effect_count: usize,
	trigger_kinds: BTreeMap<String, usize>,
	effect_kinds: BTreeMap<String, usize>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	target_write_collisions: Vec<un_avatar_core::UnaEvaluationTargetWriteCollision>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	restore_readiness: Vec<un_avatar_core::UnaEvaluationRestoreReadiness>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	restore_baseline_candidates: Vec<un_avatar_core::UnaEvaluationRestoreBaselineCandidate>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	restore_baseline_capture_plan: Vec<un_avatar_core::UnaEvaluationRestoreBaselineEntry>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	restore_apply_plan: Vec<un_avatar_core::UnaEvaluationRestoreApplyEntry>,
	actions: Vec<DiagnoseActionItemSummary>,
}

#[derive(Serialize)]
struct DiagnoseActionItemSummary {
	id: String,
	label: String,
	trigger_count: usize,
	condition_count: usize,
	effect_count: usize,
	trigger_kinds: BTreeMap<String, usize>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	parameter_triggers: Vec<DiagnoseActionParameterTrigger>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	condition_parameter_names: Vec<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	current_condition_state: Option<String>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	conditions: Vec<DiagnoseActionConditionSummary>,
	effect_kinds: BTreeMap<String, usize>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	target_writes: Vec<un_avatar_core::UnaEvaluationRuntimeActionTargetWrite>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	node_visibility_effects: Vec<DiagnoseActionNodeVisibilityEffect>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	material_property_effects: Vec<DiagnoseActionMaterialPropertyEffect>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	material_slot_effects: Vec<DiagnoseActionMaterialSlotEffect>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	expression_weight_effects: Vec<DiagnoseActionExpressionWeightEffect>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	dynamics_enabled_effects: Vec<DiagnoseActionDynamicsEnabledEffect>,
}

#[derive(Serialize)]
struct DiagnoseActionParameterTrigger {
	name: String,
	value: f32,
}

#[derive(Serialize)]
struct DiagnoseActionConditionSummary {
	#[serde(skip_serializing_if = "Option::is_none")]
	source_component_id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	source_node_id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	resolved_node_id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	parameter_name: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	parameter_value: Option<f32>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	sub_parameter_names: Vec<String>,
	inverted: bool,
	active_parent_count: usize,
}

#[derive(Serialize)]
struct DiagnoseActionNodeVisibilityEffect {
	node_index: Option<usize>,
	source_node_id: Option<String>,
	resolved_node_id: Option<String>,
	path: Option<String>,
	visible: bool,
}

#[derive(Serialize)]
struct DiagnoseActionMaterialPropertyEffect {
	property_kind: String,
	material_index: Option<usize>,
	material_name: Option<String>,
	parameter: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	scalar_value: Option<f32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	color_value: Option<[f32; 4]>,
}

#[derive(Serialize)]
struct DiagnoseActionMaterialSlotEffect {
	node_index: Option<usize>,
	source_node_id: Option<String>,
	resolved_node_id: Option<String>,
	path: Option<String>,
	primitive_index: Option<usize>,
	material_index: Option<usize>,
	material_name: Option<String>,
}

#[derive(Serialize)]
struct DiagnoseActionExpressionWeightEffect {
	name: String,
	weight: f32,
}

#[derive(Serialize)]
struct DiagnoseActionDynamicsEnabledEffect {
	source_id: String,
	enabled: bool,
}

#[derive(Serialize)]
struct DiagnoseMenuActionCandidate {
	menu_component_index: usize,
	menu_key: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	menu_label: Option<String>,
	parameter_name: String,
	parameter_value: f32,
	action_id: String,
	action_label: String,
	match_kind: String,
	inverted: bool,
	effect_count: usize,
	effect_kinds: BTreeMap<String, usize>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	wardrobe_set_ids: Vec<String>,
}

#[derive(Serialize)]
struct DiagnoseMenuWardrobeCandidate {
	menu_component_index: usize,
	menu_key: String,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	menu_path: Vec<String>,
	#[serde(skip_serializing_if = "std::ops::Not::not")]
	menu_path_truncated: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	menu_label: Option<String>,
	action_id: String,
	wardrobe_set_id: String,
	match_kind: String,
	inverted: bool,
}

#[derive(Serialize)]
struct DiagnoseVrmSummary {
	spec_version: String,
	mtoon_materials_v0: usize,
	mtoon_material_indices_v1: Vec<usize>,
	spring_group_count: usize,
}

#[derive(Serialize)]
struct DiagnoseDynamicsSummary {
	group_count: usize,
	vrm_spring_bone_group_count: usize,
	vrc_physbone_group_count: usize,
	unknown_group_count: usize,
	limit_group_count: usize,
	angle_limit_group_count: usize,
	stretch_limit_group_count: usize,
	rotation_translation_writeback_group_count: usize,
	translation_writeback_candidate_count: usize,
	translation_writeback_target_count: usize,
	stretch_translation_writeback_group_count: usize,
	stretch_translation_writeback_target_group_count: usize,
	grabbing_enabled_group_count: usize,
	posing_enabled_group_count: usize,
	collider_count: usize,
	vrm_spring_bone_collider_count: usize,
	vrc_physbone_collider_count: usize,
	unknown_collider_count: usize,
	contact_count: usize,
	vrc_contact_sender_count: usize,
	vrc_contact_receiver_count: usize,
	contact_parameter_declaration_count: usize,
	contact_parameter_emission_enabled: bool,
	contact_probe_count: usize,
	contact_probe_would_emit_count: usize,
	contact_parameter_emission_count: usize,
	contact_parameter_emitted_count: usize,
	contact_parameter_reset_to_zero_count: usize,
	constraint_ref_count: usize,
	vrc_constraint_ref_count: usize,
	source_limit_count: usize,
	source_angle_limit_count: usize,
	source_stretch_limit_count: usize,
	source_curve_count: usize,
	source_radius_curve_count: usize,
	source_angle_limit_curve_count: usize,
	source_stretch_limit_curve_count: usize,
	source_collider_count: usize,
	source_unknown_shape_collider_count: usize,
	source_collision_disabled_count: usize,
	source_inside_bounds_collider_count: usize,
	source_grabbing_enabled_count: usize,
	source_posing_enabled_count: usize,
	source_interaction_parameter_count: usize,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	colliders: Vec<DiagnoseDynamicsColliderSummary>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	contacts: Vec<DiagnoseDynamicsContactSummary>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	contact_parameter_declarations: Vec<DiagnoseContactParameterDeclarationSummary>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	contact_probes: Vec<DiagnoseContactProbeSummary>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	contact_parameter_emissions: Vec<DiagnoseContactParameterEmissionSummary>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	constraint_refs: Vec<DiagnoseDynamicsConstraintRefSummary>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	interaction_hooks: Vec<DiagnoseDynamicsInteractionHookSummary>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	groups: Vec<DiagnoseDynamicsGroupSummary>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	response_categories: Vec<DynamicsResponseCategorySummary>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	response_groups: Vec<DynamicsResponseGroupSummary>,
}

#[derive(Serialize)]
struct DiagnoseDynamicsColliderSummary {
	index: usize,
	source_kind: UnaDynamicsSourceKind,
	#[serde(skip_serializing_if = "String::is_empty")]
	source_id: String,
	#[serde(skip_serializing_if = "String::is_empty")]
	collider_path: String,
	node: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	node_path: Option<String>,
	shape: un_avatar_core::UnaDynamicsColliderShape,
	radius: f32,
	height: f32,
	position: [f32; 3],
	rotation: [f32; 4],
	inside_bounds: bool,
}

#[derive(Default)]
struct DynamicsSourceFeatureCounts {
	limit_count: usize,
	angle_limit_count: usize,
	stretch_limit_count: usize,
	curve_count: usize,
	radius_curve_count: usize,
	angle_limit_curve_count: usize,
	stretch_limit_curve_count: usize,
	collider_count: usize,
	unknown_shape_collider_count: usize,
	collision_disabled_count: usize,
	inside_bounds_collider_count: usize,
	grabbing_enabled_count: usize,
	posing_enabled_count: usize,
	interaction_parameter_count: usize,
}

#[derive(Default)]
struct DynamicsSourceColliderAudit {
	collision_enabled_empty_collider_count: usize,
	collision_enabled_empty_collider_source_ids: Vec<String>,
	collision_enabled_empty_collider_samples: Vec<String>,
}

#[derive(Clone, Debug, Default)]
struct DynamicsSourceColliderSummary {
	component_path: Option<String>,
	root_paths: Vec<String>,
	allow_collision: Option<bool>,
	collider_count: usize,
	unknown_shape_collider_count: usize,
	inside_bounds_collider_count: usize,
	collider_paths: Vec<String>,
}

#[derive(Serialize)]
struct DiagnoseDynamicsGroupSummary {
	index: usize,
	source_kind: UnaDynamicsSourceKind,
	enabled: bool,
	source_enabled: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	runtime_enabled_override: Option<bool>,
	#[serde(skip_serializing_if = "String::is_empty")]
	source_id: String,
	#[serde(skip_serializing_if = "String::is_empty")]
	comment: String,
	#[serde(skip_serializing_if = "String::is_empty")]
	category: String,
	bone_count: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	source_component_path: Option<String>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	source_root_paths: Vec<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	source_allow_collision: Option<bool>,
	source_collider_count: usize,
	source_unknown_shape_collider_count: usize,
	source_inside_bounds_collider_count: usize,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	source_collider_paths: Vec<String>,
	runtime_collider_count: usize,
	selected_runtime_collider_count: usize,
	selected_global_collider_count: usize,
	selected_authored_collider_count: usize,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	selected_runtime_collider_paths: Vec<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	root_node: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	root_path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	tip_node: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	tip_path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	center_node: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	center_path: Option<String>,
	stiffness: f32,
	pull: f32,
	spring: f32,
	integration_type: un_avatar_core::UnaDynamicsIntegrationType,
	drag_force: f32,
	gravity_power: f32,
	gravity_falloff: f32,
	immobile: f32,
	immobile_type: un_avatar_core::UnaDynamicsImmobileType,
	gravity_dir: [f32; 3],
	gravity_target_max_angle_deg: f32,
	gravity_target_max_amount: f32,
	#[serde(skip_serializing_if = "Option::is_none")]
	limit_type: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	limit_rotation: Option<[f32; 3]>,
	#[serde(skip_serializing_if = "Option::is_none")]
	max_angle_x: Option<f32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	max_angle_z: Option<f32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	max_stretch: Option<f32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	max_squish: Option<f32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	stretch_motion: Option<f32>,
	max_stretch_sample_has_positive: bool,
	max_squish_sample_has_positive: bool,
	stretch_motion_sample_has_positive: bool,
	writeback_mode: un_avatar_core::UnaDynamicsWritebackMode,
	translation_writeback_candidate_count: usize,
	translation_writeback_target_count: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	allow_grabbing: Option<bool>,
	#[serde(skip_serializing_if = "Option::is_none")]
	allow_posing: Option<bool>,
	#[serde(skip_serializing_if = "String::is_empty")]
	interaction_parameter: String,
	hit_radius: f32,
	hit_radius_sample_count: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	hit_radius_sample_min: Option<f32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	hit_radius_sample_max: Option<f32>,
}

#[derive(Serialize)]
struct DiagnoseDynamicsInteractionHookSummary {
	group_index: usize,
	source_kind: UnaDynamicsSourceKind,
	enabled: bool,
	source_enabled: bool,
	#[serde(skip_serializing_if = "String::is_empty")]
	source_id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	root_path: Option<String>,
	allow_grabbing: bool,
	allow_posing: bool,
	#[serde(skip_serializing_if = "String::is_empty")]
	parameter: String,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	suffix_parameters: Vec<String>,
	metadata_only: bool,
}

#[derive(Serialize)]
struct DiagnoseDynamicsContactSummary {
	index: usize,
	source_kind: UnaDynamicsSourceKind,
	kind: un_avatar_core::UnaDynamicsContactKind,
	#[serde(skip_serializing_if = "String::is_empty")]
	source_id: String,
	node: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	node_path: Option<String>,
	#[serde(skip_serializing_if = "String::is_empty")]
	parameter: String,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	collision_tags: Vec<String>,
	shape: un_avatar_core::UnaDynamicsColliderShape,
	radius: f32,
	height: f32,
	position: [f32; 3],
}

#[derive(Serialize)]
struct DiagnoseContactParameterDeclarationSummary {
	index: usize,
	owner_key: String,
	#[serde(skip_serializing_if = "String::is_empty")]
	source_id: String,
	node: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	node_path: Option<String>,
	parameter: String,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	collision_tags: Vec<String>,
}

#[derive(Serialize)]
struct DiagnoseContactProbeSummary {
	index: usize,
	receiver_index: usize,
	sender_index: usize,
	#[serde(skip_serializing_if = "String::is_empty")]
	receiver_source_id: String,
	#[serde(skip_serializing_if = "String::is_empty")]
	sender_source_id: String,
	receiver_node: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	receiver_node_path: Option<String>,
	sender_node: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	sender_node_path: Option<String>,
	parameter: String,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	matched_tags: Vec<String>,
	tag_match: bool,
	overlap: bool,
	would_emit: bool,
	distance: f32,
	threshold: f32,
	receiver_radius: f32,
	sender_radius: f32,
	receiver_shape: un_avatar_core::UnaDynamicsColliderShape,
	sender_shape: un_avatar_core::UnaDynamicsColliderShape,
	approximation: String,
}

#[derive(Serialize)]
struct DiagnoseContactParameterEmissionSummary {
	index: usize,
	owner_key: String,
	#[serde(skip_serializing_if = "String::is_empty")]
	source_id: String,
	receiver_index: usize,
	receiver_node: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	receiver_node_path: Option<String>,
	parameter: String,
	value: f32,
	emitted: bool,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	sender_source_ids: Vec<String>,
}

#[derive(Serialize)]
struct DiagnoseDynamicsConstraintRefSummary {
	index: usize,
	source_kind: UnaDynamicsSourceKind,
	#[serde(skip_serializing_if = "String::is_empty")]
	source_id: String,
	target_node: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	target_path: Option<String>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	source_nodes: Vec<usize>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	source_paths: Vec<String>,
	#[serde(skip_serializing_if = "String::is_empty")]
	constraint_type: String,
	weight: f32,
}

#[derive(Serialize)]
struct DiagnoseUnavatarSummary {
	spec_version: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	generator: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	manifest_name: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	source_type: Option<String>,
	extension_node_count: usize,
	variant_count: usize,
	dynamics_entry_count: usize,
	modular_avatar_component_count: usize,
	#[serde(rename = "componentCount", skip_serializing_if = "zero_usize")]
	modular_avatar_component_count_alias: usize,
	modular_avatar_support_counts: BTreeMap<String, usize>,
	#[serde(rename = "supportCounts", skip_serializing_if = "BTreeMap::is_empty")]
	modular_avatar_support_counts_alias: BTreeMap<String, usize>,
	modular_avatar_type_counts: BTreeMap<String, usize>,
	#[serde(rename = "typeCounts", skip_serializing_if = "BTreeMap::is_empty")]
	modular_avatar_type_counts_alias: BTreeMap<String, usize>,
	modular_avatar_disabled_type_counts: BTreeMap<String, usize>,
	#[serde(rename = "disabledTypeCounts", skip_serializing_if = "BTreeMap::is_empty")]
	modular_avatar_disabled_type_counts_alias: BTreeMap<String, usize>,
	modular_avatar_disabled_component_count: usize,
	#[serde(rename = "disabledComponentCount", skip_serializing_if = "zero_usize")]
	modular_avatar_disabled_component_count_alias: usize,
	modular_avatar_menu_component_count: usize,
	modular_avatar_menu_components: Vec<DiagnoseModularAvatarMenuComponentSummary>,
	modular_avatar_menu_graph_candidate_count: usize,
	modular_avatar_menu_graph_candidates: Vec<DiagnoseModularAvatarMenuGraphCandidate>,
	modular_avatar_menu_graph_node_count: usize,
	modular_avatar_menu_graph_nodes: Vec<DiagnoseModularAvatarMenuGraphNode>,
	modular_avatar_menu_install_edge_count: usize,
	modular_avatar_menu_install_edges: Vec<DiagnoseModularAvatarMenuInstallEdge>,
	modular_avatar_parameter_count: usize,
	modular_avatar_parameters: Vec<DiagnoseModularAvatarParameterSummary>,
	modular_avatar_blendshape_sync_count: usize,
	modular_avatar_blendshape_syncs: Vec<DiagnoseModularAvatarBlendshapeSyncSummary>,
	modular_avatar_vertex_filter_group_count: usize,
	modular_avatar_vertex_filter_groups: Vec<DiagnoseModularAvatarVertexFilterGroupSummary>,
	#[serde(skip_serializing_if = "Option::is_none")]
	base_set: Option<String>,
	wardrobe_set_count: usize,
	wardrobe_set_ids: Vec<String>,
	asset_group_count: usize,
	asset_group_ids: Vec<String>,
	wardrobe_sets: Vec<DiagnoseUnavatarWardrobeSetSummary>,
	base_operation_count: usize,
	base_operation_counts: BTreeMap<String, usize>,
}

#[derive(Serialize)]
struct DiagnoseUnavatarWardrobeSetSummary {
	id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	display_name: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	source: Option<String>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	asset_groups: Vec<String>,
	operation_count: usize,
	operation_counts: BTreeMap<String, usize>,
}

#[derive(Serialize)]
struct DiagnoseModularAvatarMenuComponentSummary {
	component_index: usize,
	menu_key: String,
	short_type: String,
	enabled: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	source_component_index: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	hierarchy_path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	sibling_index: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	target_path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	label: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	control_type: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	parameter: Option<String>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	sub_parameters: Vec<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	value: Option<f32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	menu_source: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	menu_source_target_path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	menu_to_append_path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	menu_to_append_control_count: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	install_target_menu_path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	install_target_menu_control_count: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	installer_path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	external_menu_asset_path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	external_menu_control_index: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
struct DiagnoseModularAvatarMenuGraphCandidate {
	component_index: usize,
	menu_key: String,
	short_type: String,
	kind: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	label: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	hierarchy_path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	parent_path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	sibling_index: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	target_path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	menu_to_append_path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	install_target_menu_path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	installer_path: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct DiagnoseModularAvatarMenuGraphNode {
	node_index: usize,
	component_index: usize,
	menu_key: String,
	short_type: String,
	kind: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	label: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	hierarchy_path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	parent_path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	parent_node_index: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	parent_component_index: Option<usize>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	child_component_indices: Vec<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	menu_to_append_path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	install_target_menu_path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	installer_path: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct DiagnoseModularAvatarMenuInstallEdge {
	source_component_index: usize,
	source_kind: String,
	target_kind: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	source_hierarchy_path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	installer_path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	menu_to_append_path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	install_target_menu_path: Option<String>,
	ignored_by_install_target: bool,
}

#[derive(Serialize)]
struct DiagnoseModularAvatarParameterSummary {
	component_index: usize,
	name_or_prefix: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	remap_to: Option<String>,
	internal_parameter: bool,
	is_prefix: bool,
	sync_type: String,
	local_only: bool,
	default_value: f32,
	saved: bool,
	has_explicit_default_value: bool,
	override_animator_defaults: bool,
}

#[derive(Serialize)]
struct DiagnoseModularAvatarBlendshapeSyncSummary {
	component_index: usize,
	enabled: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	target_path: Option<String>,
	binding_count: usize,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	bindings: Vec<DiagnoseModularAvatarBlendshapeSyncBindingSummary>,
}

#[derive(Serialize)]
struct DiagnoseModularAvatarBlendshapeSyncBindingSummary {
	#[serde(skip_serializing_if = "Option::is_none")]
	reference_path: Option<String>,
	blendshape: String,
	local_blendshape: String,
	remap_key_count: usize,
}

#[derive(Serialize)]
struct DiagnoseModularAvatarVertexFilterGroupSummary {
	short_type: String,
	enabled: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	target_path: Option<String>,
	combine: String,
	filter_count: usize,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	filters: Vec<DiagnoseModularAvatarVertexFilterSummary>,
}

#[derive(Serialize)]
struct DiagnoseModularAvatarVertexFilterSummary {
	kind: String,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	shapes: Vec<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	threshold: Option<f32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	bone_path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	center: Option<[f32; 3]>,
	#[serde(skip_serializing_if = "Option::is_none")]
	axis: Option<[f32; 3]>,
	#[serde(skip_serializing_if = "Option::is_none")]
	material_index: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	texture: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	mode: Option<String>,
}

#[derive(Serialize)]
struct DiagnoseWardrobeProbeSummary {
	set_id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	display_name: Option<String>,
	probe_ms: u128,
	#[serde(skip_serializing_if = "Option::is_none")]
	visibility_applied: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	visibility_missing: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	blendshape_applied: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	blendshape_missing: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	dynamics_applied: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	dynamics_missing: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	material_applied: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	material_missing: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	material_slot_applied: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	material_slot_missing: Option<usize>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	active_asset_groups: Vec<String>,
	visible_mesh_node_count: usize,
	visible_mesh_paths: Vec<String>,
	nonzero_morph_weight_count: usize,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	nonzero_morph_weights: Vec<DiagnoseWardrobeProbeMorphSummary>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	missing_visibility_paths: Vec<String>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	missing_blendshapes: Vec<String>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	missing_dynamics_ids: Vec<String>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	missing_materials: Vec<String>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	missing_material_slots: Vec<String>,
}

#[derive(Serialize)]
struct DiagnoseWardrobeProbeMorphSummary {
	mesh: usize,
	primitive: usize,
	index: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	name: Option<String>,
	weight: f32,
}

#[derive(Serialize)]
struct ImporterProbeRow {
	format_id: String,
	confidence: u8,
	#[serde(skip_serializing_if = "Option::is_none")]
	provider_plugin_id: Option<String>,
}

#[derive(Serialize)]
struct ExporterProbeRow {
	format_id: String,
	confidence: u8,
	#[serde(skip_serializing_if = "Option::is_none")]
	provider_plugin_id: Option<String>,
}

#[derive(Serialize)]
struct FormatsProbeJson {
	path: String,
	importers: Vec<ImporterProbeRow>,
	/// `best_importer_for` が選ぶ形式（同点時はレジストリ順の先勝ち）
	best_importer: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	best_importer_provider_plugin_id: Option<String>,
	exporters: Vec<ExporterProbeRow>,
	/// [`IoRegistry::best_exporter_for`] が選ぶ形式（空の [`UnaDocument`]・既定 [`ExportOptions`] を仮定）
	best_exporter: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	best_exporter_provider_plugin_id: Option<String>,
}

#[derive(Parser)]
#[command(
	name = "un-avatar",
	version,
	about = "UN Avatar CLI（bootstrap）",
	long_about = "UN Avatar CLI（bootstrap）\n\n\
	              環境変数 UN_AVATAR_PLUGIN_PATH に、bundle または複数 bundle の親ディレクトリを PATH 形式で指定できる（Windows は `;`、それ以外は `:`）。\
	              `--plugin-dir` と併用したときはマージし、同一パスは 1 回だけ登録する。\
	              親配下の探索の深さ上限は UN_AVATAR_PLUGIN_DISCOVERY_MAX_DEPTH（既定 8）。\
	              stdio 子の cwd: 既定は bundle 根（manifest 親）。UN_AVATAR_PLUGIN_CHILD_CWD=host（大小無視）のときだけホストと同じ cwd を使う。\
	              プラグイン RPC の stdout 読取: 共通 **`UN_AVATAR_PLUGIN_RPC_TIMEOUT_SECS`**（既定 120 秒）、または **`UN_AVATAR_PLUGIN_RPC_HANDSHAKE_TIMEOUT_SECS`** / **`UN_AVATAR_PLUGIN_RPC_IMPORT_TIMEOUT_SECS`** で `initialize` と `import` を指定。**`export`** は **`UN_AVATAR_PLUGIN_RPC_EXPORT_TIMEOUT_SECS`** または（未設定時）**当該子の import と同じ上限**。セッション全体の壁時計は **`UN_AVATAR_PLUGIN_RPC_SESSION_WALL_SECS`**（未設定・0・無効は無制限）。"
)]
struct Cli {
	/// 外部 stdio プラグインの bundle ディレクトリ（`/path/to/my-plugin` に `manifest.toml`）、または **複数 bundle の親**（`/path/to/plugins` 直下が `plugin-a/` …）。`register_stdio_*_from_plugin_root`（importer と exporter）に渡す。複数指定可
	#[arg(long = "plugin-dir", value_name = "DIR", global = true, action = clap::ArgAction::Append)]
	plugin_dir: Vec<PathBuf>,
	#[command(subcommand)]
	command: Commands,
}

/// `UN_AVATAR_PLUGIN_PATH` の生文字列をパス列に分解する（空要素を除く）。
fn parse_plugin_path_list(raw: &OsStr) -> Vec<PathBuf> {
	let sep = if cfg!(windows) { ';' } else { ':' };
	raw.to_string_lossy()
		.split(sep)
		.map(|s| PathBuf::from(s.trim()))
		.filter(|p| !p.as_os_str().is_empty())
		.collect()
}

fn plugin_dirs_from_env() -> Vec<PathBuf> {
	std::env::var_os("UN_AVATAR_PLUGIN_PATH")
		.map(|raw| parse_plugin_path_list(&raw))
		.unwrap_or_default()
}

fn merge_unique_plugin_dirs(env_entries: Vec<PathBuf>, cli: &[PathBuf]) -> Vec<PathBuf> {
	use std::collections::HashSet;
	let mut seen: HashSet<PathBuf> = HashSet::new();
	let mut out = Vec::new();
	for p in env_entries.into_iter().chain(cli.iter().cloned()) {
		if seen.insert(p.clone()) {
			out.push(p);
		}
	}
	out
}

fn io_registry_for_cli(cli_plugin_dirs: &[PathBuf]) -> Result<IoRegistry, String> {
	let dirs = merge_unique_plugin_dirs(plugin_dirs_from_env(), cli_plugin_dirs);
	let mut reg = IoRegistry::new();
	register_vrm_importer(&mut reg);
	register_gltf_importer(&mut reg);
	for dir in dirs {
		register_stdio_importers_from_plugin_root(&mut reg, dir.as_path())
			.map_err(|e| format!("プラグイン検索パス {}: {e}", dir.display()))?;
		register_stdio_exporters_from_plugin_root(&mut reg, dir.as_path())
			.map_err(|e| format!("プラグイン検索パス {} (exporter): {e}", dir.display()))?;
	}
	Ok(reg)
}

fn cached_binary_import_bytes(path: &Path) -> Option<Arc<[u8]>> {
	let ext = path.extension().and_then(|e| e.to_str())?;
	if !ext.eq_ignore_ascii_case("vrm") && !ext.eq_ignore_ascii_case("glb") && !ext.eq_ignore_ascii_case("unavatar") {
		return None;
	}
	std::fs::read(path).ok().map(Arc::<[u8]>::from)
}

fn import_probe_for_path(path: &Path, bytes: Option<Arc<[u8]>>) -> ImportProbe {
	ImportProbe {
		path_hint: Some(path.to_path_buf()),
		bytes,
	}
}

fn import_input_for_path(path: &Path, format_id: &FormatId, bytes: Option<Arc<[u8]>>) -> ImportInput {
	match (format_id.0.as_str(), bytes) {
		("io.un-avatar.vrm", Some(bytes)) => ImportInput::Bytes {
			bytes,
			path_hint: Some(path.to_path_buf()),
		},
		("io.un-avatar.gltf", _) => ImportInput::Path(path.to_path_buf()),
		_ => ImportInput::Path(path.to_path_buf()),
	}
}

/// `formats probe` 用の集約 JSON（import / export の両方）。
fn build_formats_probe_json(reg: &IoRegistry, path: &Path) -> FormatsProbeJson {
	let path_str = path.to_string_lossy().to_string();
	let probe = import_probe_for_path(path, cached_binary_import_bytes(path));
	let mut importers = Vec::with_capacity(reg.importers().len());
	importers.extend(reg.importers().iter().map(|i| {
		let desc = i.descriptor();
		let r = i.probe(&probe);
		ImporterProbeRow {
			format_id: desc.id.0.clone(),
			confidence: r.confidence,
			provider_plugin_id: desc.provider_plugin_id.clone(),
		}
	}));
	let (best_importer, best_importer_provider_plugin_id) = if let Some(i) = reg.best_importer_for(&probe) {
		let desc = i.descriptor();
		(Some(desc.id.0), desc.provider_plugin_id)
	} else {
		(None, None)
	};

	let doc = UnaDocument::default();
	let opts = ExportOptions;
	let path_str_lossy = path.as_os_str().to_string_lossy();
	let mut exporters = Vec::with_capacity(reg.exporters().len());
	exporters.extend(reg.exporters().iter().map(|e| {
		let desc = e.descriptor();
		let mut confidence = 0u8;
		if e.can_export(&doc, &opts) == ExportCapability::Supported {
			confidence = 60;
			for ext in &desc.extensions {
				if path_has_format_extension(&path_str_lossy, ext) {
					confidence = 120;
					break;
				}
			}
		}
		ExporterProbeRow {
			format_id: desc.id.0.clone(),
			confidence,
			provider_plugin_id: desc.provider_plugin_id.clone(),
		}
	}));
	let (best_exporter, best_exporter_provider_plugin_id) = if let Some(e) = reg.best_exporter_for(&doc, path) {
		let desc = e.descriptor();
		(Some(desc.id.0), desc.provider_plugin_id)
	} else {
		(None, None)
	};

	FormatsProbeJson {
		path: path_str,
		importers,
		best_importer,
		best_importer_provider_plugin_id,
		exporters,
		best_exporter,
		best_exporter_provider_plugin_id,
	}
}

#[derive(Subcommand)]
enum Commands {
	/// 登録されている入出力形式を列挙する
	Formats {
		#[command(subcommand)]
		command: FormatsCommands,
	},
	/// アバターを別形式へ書き出す
	Convert {
		/// 入力ファイル
		input: PathBuf,
		/// 出力ファイル
		output: PathBuf,
		/// 使う importer の FormatId。省略時はパスから probe
		#[arg(long, value_name = "FORMAT_ID")]
		input_format: Option<String>,
		/// 使う exporter の FormatId。省略時は出力パスから選択
		#[arg(long, value_name = "FORMAT_ID")]
		output_format: Option<String>,
		/// import/export レポートを JSON で書き出す（`-` で stdout）
		#[arg(long, value_name = "PATH")]
		json_report: Option<PathBuf>,
	},
	/// Importer 経由で読めるか検証する（終了コード 0/1）
	Validate {
		/// 入力ファイル
		path: PathBuf,
		/// 使う importer の FormatId。省略時はパスから probe
		#[arg(long, value_name = "FORMAT_ID")]
		input_format: Option<String>,
		/// 結果を JSON で stdout に出す（失敗時も出力してから終了コード 1）
		#[arg(long)]
		json: bool,
	},
	/// Importer 経由でモデルを読み、軽量な概要を表示する
	Inspect {
		path: PathBuf,
		#[arg(long)]
		json: bool,
	},
	/// .unavatar / glTF の UNPhysics sourceParams だけを高速に検査する
	DynamicsScan {
		path: PathBuf,
		/// 現行 exporter が出すべき UNPhysics sourceParams が欠けていたら失敗する
		#[arg(long)]
		require_current_exporter: bool,
		#[arg(long)]
		json: bool,
	},
	/// Importer/lowering 後の UNPhysics runtime dynamics を監査する
	DynamicsImportAudit {
		path: PathBuf,
		/// 使う importer の FormatId。省略時はパスから probe
		#[arg(long, value_name = "FORMAT_ID")]
		input_format: Option<String>,
		/// Base 適用後に重ねる `.unavatar` wardrobe set id
		#[arg(long, value_name = "SET_ID")]
		wardrobe_set: Option<String>,
		/// sourceParams があるのに runtime dynamics が無いなど、必須の lowering evidence 欠落で失敗する
		#[arg(long)]
		require_runtime_evidence: bool,
		#[arg(long)]
		json: bool,
	},
	/// UNPhysics response terms が profile override で実際に変化するか監査する
	DynamicsResponseAudit {
		path: PathBuf,
		/// 使う importer の FormatId。省略時はパスから probe
		#[arg(long, value_name = "FORMAT_ID")]
		input_format: Option<String>,
		/// Base 適用後に重ねる `.unavatar` wardrobe set id
		#[arg(long, value_name = "SET_ID")]
		wardrobe_set: Option<String>,
		/// soft/firm override が runtime response に効いていなければ失敗する
		#[arg(long)]
		require_override_effect: bool,
		#[arg(long)]
		json: bool,
	},
	/// 実アバターに簡易motionを流し、カテゴリ別のUNPhysics lagを監査する
	DynamicsMotionTraceAudit {
		path: PathBuf,
		/// 使う importer の FormatId。省略時はパスから probe
		#[arg(long, value_name = "FORMAT_ID")]
		input_format: Option<String>,
		/// Base 適用後に重ねる `.unavatar` wardrobe set id
		#[arg(long, value_name = "SET_ID")]
		wardrobe_set: Option<String>,
		/// カテゴリ別 lag が得られなければ失敗する
		#[arg(long)]
		require_motion_evidence: bool,
		#[arg(long, default_value_t = 24)]
		frames: usize,
		/// 入力停止後に自然回復を観測する frame 数。省略時は長い cloth / tail / cable の収束確認向けに 240
		#[arg(long)]
		recovery_frames: Option<usize>,
		/// 監査時だけ適用する物理tuning。authored はモデル由来値、soft/firm と単一term tuning は全カテゴリoverride
		#[arg(
			long,
			default_value = "authored",
			value_parser = [
				"authored",
				"soft",
				"firm",
				"rest-low",
				"rest-high",
				"shape-low",
				"shape-high",
				"bounce-low",
				"bounce-high",
				"follow-low",
				"follow-high",
				"gravity-off",
				"gravity-low",
				"gravity-high",
				"stretch-off",
				"stretch-low",
				"stretch-high",
				"damping-long",
				"damping-short"
			]
		)]
		tuning: String,
		#[arg(long)]
		json: bool,
	},
	/// 実アバター上で物理 settle 後の skinned vertex 変位を調査する
	DynamicsVertexProbe {
		path: PathBuf,
		/// 使う importer の FormatId。省略時はパスから probe
		#[arg(long, value_name = "FORMAT_ID")]
		input_format: Option<String>,
		/// Base 適用後に重ねる `.unavatar` wardrobe set id
		#[arg(long, value_name = "SET_ID")]
		wardrobe_set: Option<String>,
		/// 対象 node path に含まれる文字列。空なら dynamics joint を含む skinned mesh を自動選択する
		#[arg(long, default_value = "")]
		node_contains: String,
		/// settle させる frame 数
		#[arg(long, default_value_t = 240)]
		settle_frames: usize,
		/// Renderer の mesh cloth assist と同じ補正を適用した頂点ウェイトで probe する
		#[arg(long)]
		apply_mesh_cloth_assist: bool,
		/// Authored PhysBone collider を外して settle する
		#[arg(long)]
		ignore_authored_colliders: bool,
		/// 診断用: Probe 前に node constraints を無効化する
		#[arg(long)]
		ignore_node_constraints: bool,
		/// Probe 前に UNMotion の LeftUpperArm Z 回転を適用する（度）
		#[arg(long)]
		pose_left_upper_arm_z_deg: Option<f32>,
		/// Probe 前に UNMotion の RightUpperArm Z 回転を適用する（度）
		#[arg(long)]
		pose_right_upper_arm_z_deg: Option<f32>,
		/// Probe 前に UNMotionFrame JSON を適用する（UNMotion の native-image-pipeline-probe 出力など）
		#[arg(long, value_name = "PATH")]
		unmotion_frame_json: Option<PathBuf>,
		/// 監査時だけ適用する物理tuning。dynamics-motion-trace-audit と同じ値
		#[arg(
			long,
			default_value = "authored",
			value_parser = [
				"authored",
				"soft",
				"firm",
				"rest-low",
				"rest-high",
				"shape-low",
				"shape-high",
				"bounce-low",
				"bounce-high",
				"follow-low",
				"follow-high",
				"gravity-off",
				"gravity-low",
				"gravity-high",
				"stretch-off",
				"stretch-low",
				"stretch-high",
				"damping-long",
				"damping-short"
			]
		)]
		tuning: String,
		#[arg(long)]
		json: bool,
	},
	/// Importer 経由でモデルを読み、材質・Humanoid・表情・VRM ヒントを診断する
	Diagnose {
		/// 入力ファイル
		path: PathBuf,
		/// 使う importer の FormatId。省略時はパスから probe
		#[arg(long, value_name = "FORMAT_ID")]
		input_format: Option<String>,
		/// Base 適用後に重ねる `.unavatar` wardrobe set id
		#[arg(long, value_name = "SET_ID")]
		wardrobe_set: Option<String>,
		/// Base と全 wardrobe set の可視メッシュ／blendshape 状態を比較表示する
		#[arg(long)]
		wardrobe_probe_all: bool,
		/// 人間向け出力で、現在の wardrobe 状態から参照される material だけを表示する
		#[arg(long)]
		visible_materials_only: bool,
		/// 人間向け出力で、現在の wardrobe 状態の可視 mesh node / primitive / material 対応を表示する
		#[arg(long)]
		visible_meshes: bool,
		/// 結果を JSON で stdout に出す
		#[arg(long)]
		json: bool,
	},
	/// VMC Protocol（OSC/UDP）— Marionette 受信デバッグ
	Vmc {
		#[command(subcommand)]
		command: VmcCommands,
	},
}

#[derive(Subcommand)]
enum VmcCommands {
	/// UDP で待受け、デコードしたイベント（既定）または `--frame` で UNMotionFrame を JSON 行で出力
	Listen {
		#[arg(long, default_value_t = un_avatar_vmc::DEFAULT_MARIONETTE_PORT)]
		port: u16,
		/// 各パケット受信後に蓄積状態から UNMotionFrame を 1 行 JSON で出す
		#[arg(long)]
		frame: bool,
	},
}

#[derive(Subcommand)]
enum FormatsCommands {
	/// importer / exporter の一覧を表示する
	List {
		/// JSON で stdout に出す（ツール連携用）
		#[arg(long)]
		json: bool,
	},
	/// 各 importer の [`ImportProbe`] 結果と、**出力パス**に対する exporter 候補（空ドキュメントで `can_export`／拡張子一致の目安）を表示する
	Probe {
		path: PathBuf,
		#[arg(long)]
		json: bool,
	},
}

fn main() {
	let cli = Cli::parse_from(normalize_cli_args(std::env::args_os()));
	if let Err(e) = run(cli) {
		eprintln!("{e}");
		std::process::exit(1);
	}
}

fn is_known_command(arg: &OsStr) -> bool {
	matches!(
		arg.to_string_lossy().as_ref(),
		"formats"
			| "convert"
			| "validate"
			| "inspect"
			| "dynamics-scan"
			| "dynamics-import-audit"
			| "dynamics-response-audit"
			| "dynamics-motion-trace-audit"
			| "dynamics-vertex-probe"
			| "diagnose"
			| "vmc" | "help"
	)
}

fn looks_like_input_path(arg: &OsStr) -> bool {
	let s = arg.to_string_lossy();
	if s.is_empty() || s.starts_with('-') || is_known_command(arg) {
		return false;
	}
	let p = Path::new(arg);
	if p.exists() {
		return true;
	}
	let pathish = s.contains('/') || s.contains('\\');
	let import_ext = p
		.extension()
		.and_then(OsStr::to_str)
		.map(|ext| {
			matches!(
				ext.to_ascii_lowercase().as_str(),
				"vrm" | "glb" | "gltf" | "unavatar" | "exampleavatar"
			)
		})
		.unwrap_or(false);
	pathish || import_ext
}

fn normalize_cli_args(args: impl IntoIterator<Item = OsString>) -> Vec<OsString> {
	let mut out: Vec<OsString> = args.into_iter().collect();
	let mut i = 1;
	while i < out.len() {
		let arg = &out[i];
		if arg == "--" {
			if out.get(i + 1).is_some_and(|next| looks_like_input_path(next)) {
				out.insert(i + 1, OsString::from("diagnose"));
			}
			break;
		}
		let s = arg.to_string_lossy();
		if s == "--plugin-dir" {
			i += 2;
			continue;
		}
		if s.starts_with("--plugin-dir=") || s == "--help" || s == "-h" || s == "--version" || s == "-V" {
			i += 1;
			continue;
		}
		if s.starts_with('-') {
			break;
		}
		if looks_like_input_path(arg) {
			out.insert(i, OsString::from("diagnose"));
		}
		break;
	}
	out
}

fn run(cli: Cli) -> Result<(), String> {
	let plugin_dirs = cli.plugin_dir;
	match cli.command {
		Commands::Formats { command } => {
			run_formats(&plugin_dirs, command)?;
			Ok(())
		}
		Commands::Convert {
			input,
			output,
			input_format,
			output_format,
			json_report,
		} => run_convert(&plugin_dirs, input, output, input_format, output_format, json_report),
		Commands::Validate { path, input_format, json } => run_validate(&plugin_dirs, path, input_format, json),
		Commands::Inspect { path, json } => run_inspect(&plugin_dirs, path, json),
		Commands::DynamicsScan {
			path,
			require_current_exporter,
			json,
		} => run_dynamics_scan(path, require_current_exporter, json),
		Commands::DynamicsImportAudit {
			path,
			input_format,
			wardrobe_set,
			require_runtime_evidence,
			json,
		} => run_dynamics_import_audit(&plugin_dirs, path, input_format, wardrobe_set, require_runtime_evidence, json),
		Commands::DynamicsResponseAudit {
			path,
			input_format,
			wardrobe_set,
			require_override_effect,
			json,
		} => run_dynamics_response_audit(&plugin_dirs, path, input_format, wardrobe_set, require_override_effect, json),
		Commands::DynamicsMotionTraceAudit {
			path,
			input_format,
			wardrobe_set,
			require_motion_evidence,
			frames,
			recovery_frames,
			tuning,
			json,
		} => run_dynamics_motion_trace_audit(
			&plugin_dirs,
			path,
			input_format,
			wardrobe_set,
			require_motion_evidence,
			frames,
			recovery_frames,
			&tuning,
			json,
		),
		Commands::DynamicsVertexProbe {
			path,
			input_format,
			wardrobe_set,
			node_contains,
			settle_frames,
			apply_mesh_cloth_assist,
			ignore_authored_colliders,
			ignore_node_constraints,
			pose_left_upper_arm_z_deg,
			pose_right_upper_arm_z_deg,
			unmotion_frame_json,
			tuning,
			json,
		} => run_dynamics_vertex_probe(
			&plugin_dirs,
			path,
			input_format,
			wardrobe_set,
			&node_contains,
			settle_frames,
			apply_mesh_cloth_assist,
			ignore_authored_colliders,
			ignore_node_constraints,
			pose_left_upper_arm_z_deg,
			pose_right_upper_arm_z_deg,
			unmotion_frame_json,
			&tuning,
			json,
		),
		Commands::Diagnose {
			path,
			input_format,
			wardrobe_set,
			wardrobe_probe_all,
			visible_materials_only,
			visible_meshes,
			json,
		} => run_diagnose(
			&plugin_dirs,
			path,
			input_format,
			wardrobe_set,
			wardrobe_probe_all,
			visible_materials_only,
			visible_meshes,
			json,
		),
		Commands::Vmc { command } => run_vmc(command),
	}
}

fn run_formats(plugin_dirs: &[PathBuf], cmd: FormatsCommands) -> Result<(), String> {
	match cmd {
		FormatsCommands::List { json } => run_formats_list(plugin_dirs, json),
		FormatsCommands::Probe { path, json } => run_formats_probe(plugin_dirs, path, json),
	}
}

fn run_formats_list(plugin_dirs: &[PathBuf], json: bool) -> Result<(), String> {
	let reg = io_registry_for_cli(plugin_dirs)?;
	if json {
		let out = FormatsListJson {
			importers: reg.importer_descriptors(),
			exporters: reg.exporter_descriptors(),
		};
		write_json_stdout(&out)?;
		return Ok(());
	}
	println!("importers:");
	for importer in reg.importers() {
		let d = importer.descriptor();
		let plug = d.provider_plugin_id.as_ref().map(|p| format!(" ({p})")).unwrap_or_default();
		println!("  {} — {} — [{}]{plug}", d.id.0, d.display_name, d.extensions.join(", "));
	}
	println!("exporters:");
	for exporter in reg.exporters() {
		let d = exporter.descriptor();
		let plug = d.provider_plugin_id.as_ref().map(|p| format!(" ({p})")).unwrap_or_default();
		println!("  {} — {} — [{}]{plug}", d.id.0, d.display_name, d.extensions.join(", "));
	}
	Ok(())
}

fn run_formats_probe(plugin_dirs: &[PathBuf], path: PathBuf, json: bool) -> Result<(), String> {
	let reg = io_registry_for_cli(plugin_dirs)?;
	if json {
		let out = build_formats_probe_json(&reg, &path);
		write_json_stdout(&out)?;
		return Ok(());
	}
	let out = build_formats_probe_json(&reg, &path);
	println!("probe: {}", path.display());
	println!("importers:");
	for row in &out.importers {
		let plug = row.provider_plugin_id.as_ref().map(|p| format!("  ({p})")).unwrap_or_default();
		println!("  {}  confidence {}{plug}", row.format_id, row.confidence);
	}
	if let Some(ref id) = out.best_importer {
		let plug = out
			.best_importer_provider_plugin_id
			.as_ref()
			.map(|p| format!(" ({p})"))
			.unwrap_or_default();
		println!("best importer: {id}{plug}");
	} else {
		println!("best importer: (none)");
	}
	println!("exporters:");
	for row in &out.exporters {
		let plug = row.provider_plugin_id.as_ref().map(|p| format!("  ({p})")).unwrap_or_default();
		println!("  {}  confidence {}{plug}", row.format_id, row.confidence);
	}
	if let Some(ref id) = out.best_exporter {
		let plug = out
			.best_exporter_provider_plugin_id
			.as_ref()
			.map(|p| format!(" ({p})"))
			.unwrap_or_default();
		println!("best exporter: {id}{plug}");
	} else {
		println!("best exporter: (none)");
	}
	Ok(())
}

fn write_json_stdout<T: Serialize>(value: &T) -> Result<(), String> {
	let stdout = io::stdout();
	let mut lock = stdout.lock();
	serde_json::to_writer_pretty(&mut lock, value).map_err(|e| e.to_string())?;
	writeln!(lock).map_err(|e| e.to_string())
}

fn write_convert_json_report(path: &Path, bundle: &ConvertJsonReport) -> Result<(), String> {
	if path.as_os_str() == "-" {
		write_json_stdout(bundle)?;
		return Ok(());
	}
	if let Some(parent) = path.parent() {
		if !parent.as_os_str().is_empty() {
			fs::create_dir_all(parent).map_err(|e| e.to_string())?;
		}
	}
	let file = fs::File::create(path).map_err(|e| e.to_string())?;
	let mut writer = BufWriter::new(file);
	serde_json::to_writer_pretty(&mut writer, bundle).map_err(|e| e.to_string())?;
	writeln!(writer).map_err(|e| e.to_string())?;
	Ok(())
}

fn write_validate_stdout(report: &ValidateReport) -> Result<(), String> {
	write_json_stdout(report)
}

fn run_validate(plugin_dirs: &[PathBuf], path: PathBuf, input_format: Option<String>, json: bool) -> Result<(), String> {
	let path_str = path.to_string_lossy().to_string();
	let reg = io_registry_for_cli(plugin_dirs)?;
	let cached_bytes = cached_binary_import_bytes(&path);

	let importer: &dyn AvatarImporter = if let Some(ref s) = input_format {
		let id = FormatId::new(s.as_str());
		match reg.importer_by_id(&id) {
			Some(i) => i,
			None => {
				let msg = format!("指定の importer が登録されていません: {s}");
				if json {
					write_validate_stdout(&ValidateReport {
						valid: false,
						path: path_str.clone(),
						error: Some(msg.clone()),
						format_id: None,
						provider_plugin_id: None,
					})?;
				}
				return Err(msg);
			}
		}
	} else {
		let probe = import_probe_for_path(&path, cached_bytes.clone());
		match reg.best_importer_for(&probe) {
			Some(i) => i,
			None => {
				let msg = "入力に合う importer が見つかりません（VRM / glTF / .unavatar、`--plugin-dir`、または --input-format を確認）"
					.to_string();
				if json {
					write_validate_stdout(&ValidateReport {
						valid: false,
						path: path_str.clone(),
						error: Some(msg.clone()),
						format_id: None,
						provider_plugin_id: None,
					})?;
				}
				return Err(msg);
			}
		}
	};

	let desc = importer.descriptor();
	let format_id = desc.id.0.clone();
	let provider_plugin_id = desc.provider_plugin_id.clone();
	let mut ictx = ImportContext {
		asset_root: path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(".")),
		temp_dir: std::env::temp_dir(),
		..ImportContext::dummy()
	};
	let path_display = path.display().to_string();
	let import_input = import_input_for_path(&path, &desc.id, cached_bytes);
	let import_result = importer.import(&mut ictx, import_input, ImportOptions);
	match import_result {
		Ok(_) if json => {
			write_validate_stdout(&ValidateReport {
				valid: true,
				path: path_str,
				error: None,
				format_id: Some(format_id),
				provider_plugin_id: provider_plugin_id.clone(),
			})?;
			Ok(())
		}
		Ok(_) => {
			let plug = provider_plugin_id.as_ref().map(|p| format!(" ({p})")).unwrap_or_default();
			println!("OK  {path_display}  ({format_id}){plug}");
			Ok(())
		}
		Err(e) if json => {
			write_validate_stdout(&ValidateReport {
				valid: false,
				path: path_str,
				error: Some(e.to_string()),
				format_id: Some(format_id),
				provider_plugin_id,
			})?;
			Err(e.to_string())
		}
		Err(e) => Err(e.to_string()),
	}
}

fn inspect_document_summary(document: &UnaDocument) -> InspectDocumentSummary {
	let Some(scene) = document.scene.as_ref() else {
		return InspectDocumentSummary {
			has_scene: false,
			has_vrm: document.vrm.is_some(),
			has_unavatar: document.unavatar.is_some(),
			scene_lighting: None,
			node_count: 0,
			root_count: 0,
			mesh_count: 0,
			mesh_primitive_count: 0,
			material_count: 0,
			image_count: 0,
			skin_count: 0,
			morph_target_count: 0,
		};
	};
	let mesh_primitive_count = scene.meshes.iter().map(Vec::len).sum();
	let morph_target_count = scene.meshes.iter().flatten().map(|primitive| primitive.morph_targets.len()).sum();
	let scene_lighting = scene.lighting.as_ref().map(|lighting| InspectSceneLightingSummary {
		has_environment: lighting.environment.is_some(),
		has_directional: lighting.directional.is_some(),
		environment_color: lighting.environment.as_ref().map(|light| light.color),
		environment_intensity: lighting.environment.as_ref().map(|light| light.intensity),
		directional_color: lighting.directional.as_ref().map(|light| light.color),
		directional_intensity: lighting.directional.as_ref().map(|light| light.intensity),
		directional_azimuth_deg: lighting.directional.as_ref().map(|light| light.azimuth_deg),
		directional_elevation_deg: lighting.directional.as_ref().map(|light| light.elevation_deg),
	});
	InspectDocumentSummary {
		has_scene: true,
		has_vrm: document.vrm.is_some(),
		has_unavatar: document.unavatar.is_some(),
		scene_lighting,
		node_count: scene.nodes.len(),
		root_count: scene.roots.len(),
		mesh_count: scene.meshes.len(),
		mesh_primitive_count,
		material_count: scene.materials.len(),
		image_count: scene.images.len(),
		skin_count: scene.skins.len(),
		morph_target_count,
	}
}

fn run_inspect(plugin_dirs: &[PathBuf], path: PathBuf, json: bool) -> Result<(), String> {
	let path_str = path.to_string_lossy().to_string();
	let reg = io_registry_for_cli(plugin_dirs)?;
	let cached_bytes = cached_binary_import_bytes(&path);
	let probe = import_probe_for_path(&path, cached_bytes.clone());
	let importer = reg.best_importer_for(&probe).ok_or_else(|| {
		"入力に合う importer が見つかりません（VRM / glTF / .unavatar、`--plugin-dir`、または --input-format を確認）".to_string()
	})?;
	let desc = importer.descriptor();
	let mut ictx = ImportContext {
		asset_root: path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(".")),
		temp_dir: std::env::temp_dir(),
		..ImportContext::dummy()
	};
	let imported = importer
		.import(&mut ictx, import_input_for_path(&path, &desc.id, cached_bytes), ImportOptions)
		.map_err(|e| e.to_string())?;
	let summary = inspect_document_summary(&imported.document);
	if json {
		let out = InspectReport {
			path: path_str,
			import_format_id: desc.id.0,
			import_provider_plugin_id: desc.provider_plugin_id,
			import_report: imported.report,
			document: summary,
		};
		write_json_stdout(&out)?;
		return Ok(());
	}
	println!("path: {}", path.display());
	let plug = desc.provider_plugin_id.as_ref().map(|p| format!(" ({p})")).unwrap_or_default();
	println!("importer: {}{}", desc.id.0, plug);
	println!(
		"document: scene={} vrm={} unavatar={}",
		summary.has_scene, summary.has_vrm, summary.has_unavatar
	);
	println!(
		"scene: nodes={} roots={} meshes={} primitives={} materials={} images={} skins={} morph_targets={}",
		summary.node_count,
		summary.root_count,
		summary.mesh_count,
		summary.mesh_primitive_count,
		summary.material_count,
		summary.image_count,
		summary.skin_count,
		summary.morph_target_count
	);
	if let Some(lighting) = &summary.scene_lighting {
		println!(
			"scene_lighting: environment={} directional={} env_intensity={:?} dir_intensity={:?} dir_azimuth_deg={:?} dir_elevation_deg={:?}",
			lighting.has_environment,
			lighting.has_directional,
			lighting.environment_intensity,
			lighting.directional_intensity,
			lighting.directional_azimuth_deg,
			lighting.directional_elevation_deg
		);
	} else {
		println!("scene_lighting: none");
	}
	Ok(())
}

fn read_gltf_json_value(path: &Path) -> Result<(serde_json::Value, usize), String> {
	let bytes = fs::read(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
	if bytes.starts_with(b"glTF") {
		if bytes.len() < 12 {
			return Err(format!("{} is too short for GLB header", path.display()));
		}
		let total_len = u32::from_le_bytes(bytes[8..12].try_into().expect("slice length checked")) as usize;
		let mut offset = 12usize;
		let scan_len = total_len.min(bytes.len());
		while offset + 8 <= scan_len {
			let chunk_len = u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("slice length checked")) as usize;
			let chunk_type = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().expect("slice length checked"));
			offset += 8;
			let end = offset.saturating_add(chunk_len).min(scan_len);
			if chunk_type == 0x4E4F534A {
				let chunk = &bytes[offset..end];
				let text = std::str::from_utf8(chunk)
					.map_err(|e| format!("{} JSON chunk is not valid UTF-8: {e}", path.display()))?
					.trim_matches(|c: char| c == '\0' || c.is_ascii_whitespace());
				let value = serde_json::from_str(text).map_err(|e| format!("{} JSON chunk parse failed: {e}", path.display()))?;
				return Ok((value, chunk.len()));
			}
			offset = end;
		}
		return Err(format!("{} has no GLB JSON chunk", path.display()));
	}
	let text = std::str::from_utf8(&bytes).map_err(|e| format!("{} is neither GLB nor UTF-8 glTF JSON: {e}", path.display()))?;
	let value = serde_json::from_str(text).map_err(|e| format!("{} JSON parse failed: {e}", path.display()))?;
	Ok((value, bytes.len()))
}

fn dynamics_scan_bump_numeric(ranges: &mut BTreeMap<String, DynamicsScanNumericRange>, key: &str, value: &serde_json::Value) {
	let Some(number) = value.as_f64() else {
		return;
	};
	ranges
		.entry(key.to_string())
		.and_modify(|range| {
			range.count += 1;
			range.min = range.min.min(number);
			range.max = range.max.max(number);
		})
		.or_insert(DynamicsScanNumericRange {
			count: 1,
			min: number,
			max: number,
		});
}

fn dynamics_scan_visit_value(
	value: &serde_json::Value,
	source_params_count: &mut usize,
	source_param_key_counts: &mut BTreeMap<String, usize>,
	numeric_ranges: &mut BTreeMap<String, DynamicsScanNumericRange>,
	curve_counts: &mut BTreeMap<String, usize>,
) {
	match value {
		serde_json::Value::Object(object) => {
			if let Some(serde_json::Value::Object(source_params)) = object.get("sourceParams").or_else(|| object.get("source_params")) {
				*source_params_count += 1;
				for (key, field_value) in source_params {
					bump_count(source_param_key_counts, key);
					dynamics_scan_bump_numeric(numeric_ranges, key, field_value);
					if key.ends_with("Curve") || key.ends_with("_curve") {
						if field_value.is_array() || field_value.is_object() {
							bump_count(curve_counts, key);
						}
					}
				}
			}
			for child in object.values() {
				dynamics_scan_visit_value(child, source_params_count, source_param_key_counts, numeric_ranges, curve_counts);
			}
		}
		serde_json::Value::Array(values) => {
			for child in values {
				dynamics_scan_visit_value(child, source_params_count, source_param_key_counts, numeric_ranges, curve_counts);
			}
		}
		_ => {}
	}
}

fn dynamics_scan_report(path: &Path) -> Result<DynamicsScanReport, String> {
	let (json_value, json_bytes) = read_gltf_json_value(path)?;
	let file_bytes = fs::metadata(path)
		.map_err(|e| format!("failed to stat {}: {e}", path.display()))?
		.len();
	let extension_keys = json_value
		.get("extensions")
		.and_then(serde_json::Value::as_object)
		.map(|object| object.keys().cloned().collect())
		.unwrap_or_default();
	let mut source_params_count = 0;
	let mut source_param_key_counts = BTreeMap::new();
	let mut numeric_ranges = BTreeMap::new();
	let mut curve_counts = BTreeMap::new();
	dynamics_scan_visit_value(
		&json_value,
		&mut source_params_count,
		&mut source_param_key_counts,
		&mut numeric_ranges,
		&mut curve_counts,
	);
	let required_source_param_counts: BTreeMap<String, usize> = DYNAMICS_SCAN_REQUIRED_SOURCE_PARAM_KEYS
		.iter()
		.copied()
		.map(|key| (key.to_string(), source_param_key_counts.get(key).copied().unwrap_or(0)))
		.collect();
	let missing_required_source_params = required_source_param_counts
		.iter()
		.filter_map(|(key, count)| (*count != source_params_count).then(|| format!("{key}={count}/{source_params_count}")))
		.collect();
	Ok(DynamicsScanReport {
		path: path.display().to_string(),
		file_bytes,
		json_bytes,
		extension_keys,
		source_params_count,
		required_source_param_counts,
		missing_required_source_params,
		source_param_key_counts,
		numeric_ranges,
		curve_counts,
	})
}

fn require_current_exporter_dynamics_scan(report: &DynamicsScanReport) -> Result<(), String> {
	if report.missing_required_source_params.is_empty() {
		Ok(())
	} else {
		Err(format!(
			"UNPhysics sourceParams missing required current-exporter fields: {}",
			report.missing_required_source_params.join(", ")
		))
	}
}

fn run_dynamics_scan(path: PathBuf, require_current_exporter: bool, json: bool) -> Result<(), String> {
	let report = dynamics_scan_report(&path)?;
	let required = if require_current_exporter {
		require_current_exporter_dynamics_scan(&report)
	} else {
		Ok(())
	};
	if json {
		write_json_stdout(&report)?;
		return required;
	}
	println!("path: {}", report.path);
	println!("file_bytes: {}", report.file_bytes);
	println!("json_bytes: {}", report.json_bytes);
	println!("extensions: {:?}", report.extension_keys);
	println!("source_params: {}", report.source_params_count);
	println!("required sourceParams:");
	for (key, count) in &report.required_source_param_counts {
		println!("  {key}: {count}/{}", report.source_params_count);
	}
	if !report.missing_required_source_params.is_empty() {
		println!(
			"missing required sourceParams: {}",
			report.missing_required_source_params.join(", ")
		);
	}
	if !report.numeric_ranges.is_empty() {
		println!("numeric ranges:");
		for (key, range) in &report.numeric_ranges {
			println!("  {key}: count={} min={} max={}", range.count, range.min, range.max);
		}
	}
	required
}

fn dynamics_audit_bump_numeric(ranges: &mut BTreeMap<String, DynamicsScanNumericRange>, key: &str, value: f32) {
	if !value.is_finite() {
		return;
	}
	let value = value as f64;
	ranges
		.entry(key.to_string())
		.and_modify(|range| {
			range.count += 1;
			range.min = range.min.min(value);
			range.max = range.max.max(value);
		})
		.or_insert(DynamicsScanNumericRange {
			count: 1,
			min: value,
			max: value,
		});
}

fn dynamics_audit_bump_samples(sample_counts: &mut BTreeMap<String, usize>, key: &str, count: usize) {
	if count > 0 {
		*sample_counts.entry(key.to_string()).or_default() += count;
	}
}

fn import_document_for_cli(
	plugin_dirs: &[PathBuf],
	path: &Path,
	input_format: Option<&str>,
) -> Result<(UnaDocument, ImportReport, FormatDescriptor), String> {
	let reg = io_registry_for_cli(plugin_dirs)?;
	let cached_bytes = cached_binary_import_bytes(path);
	let importer: &dyn AvatarImporter = if let Some(input_format) = input_format {
		let id = FormatId::new(input_format);
		reg.importer_by_id(&id)
			.ok_or_else(|| format!("指定の importer が登録されていません: {input_format}"))?
	} else {
		let probe = import_probe_for_path(path, cached_bytes.clone());
		reg.best_importer_for(&probe).ok_or_else(|| {
			"入力に合う importer が見つかりません（VRM / glTF / .unavatar、`--plugin-dir`、または --input-format を確認）".to_string()
		})?
	};
	let desc = importer.descriptor();
	let mut ictx = ImportContext {
		asset_root: path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(".")),
		temp_dir: std::env::temp_dir(),
		..ImportContext::dummy()
	};
	let imported = importer
		.import(&mut ictx, import_input_for_path(path, &desc.id, cached_bytes), ImportOptions)
		.map_err(|e| e.to_string())?;
	Ok((imported.document, imported.report, desc))
}

fn dynamics_import_audit_report(
	plugin_dirs: &[PathBuf],
	path: &Path,
	input_format: Option<&str>,
	wardrobe_set: Option<&str>,
) -> Result<DynamicsImportAuditReport, String> {
	let source_params_count = dynamics_scan_report(path).map(|report| report.source_params_count).unwrap_or(0);
	let (mut doc, import_report, desc) = import_document_for_cli(plugin_dirs, path, input_format)?;
	let active_wardrobe_set = wardrobe_set.filter(|set_id| !set_id.trim().is_empty()).map(str::to_string);
	if let Some(set_id) = active_wardrobe_set.as_deref() {
		apply_unavatar_wardrobe_set(&mut doc, set_id)?;
	}
	let settings = doc.dynamics();
	let raw_groups = settings.map(|settings| settings.groups.as_slice()).unwrap_or(&[]);
	let scoped_groups = active_wardrobe_set
		.as_ref()
		.map(|_| {
			let runtime_model = doc.runtime_model();
			let runtime_dynamics = runtime_model.dynamics();
			let scene = doc.scene.as_ref();
			let active_source_ids = runtime_dynamics
				.dynamics_groups()
				.filter(|group| {
					group.effective_enabled
						&& scene.is_none_or(|scene| runtime_dynamics.source_id_resident_in_scene(scene, group.source_id))
				})
				.map(|group| group.source_id.to_string())
				.collect::<BTreeSet<_>>();
			raw_groups
				.iter()
				.filter(|group| active_source_ids.contains(&group.source_id))
				.cloned()
				.collect::<Vec<_>>()
		})
		.unwrap_or_default();
	let groups = if active_wardrobe_set.is_some() {
		scoped_groups.as_slice()
	} else {
		raw_groups
	};
	let node_paths_by_index = doc.scene.as_ref().map(scene_node_paths_by_index).unwrap_or_default();
	let (node_samples, skin_samples, mesh_cloth_assist_samples) = doc
		.scene
		.as_ref()
		.map(|scene| dynamics_import_scene_samples(scene, groups, &node_paths_by_index))
		.unwrap_or_default();
	let mut source_kind_counts = BTreeMap::new();
	let mut runtime_ranges = BTreeMap::new();
	let mut sample_counts = BTreeMap::new();
	let mut group_sample_candidates = Vec::new();
	let mut enabled_group_count = 0usize;
	let mut chain_joint_count = 0usize;
	let mut source_angle_limit_group_count = 0usize;
	let mut angle_limit_by_source_id = BTreeMap::new();
	for (group_index, group) in groups.iter().enumerate() {
		bump_count(&mut source_kind_counts, &format!("{:?}", group.source_kind));
		if group.enabled {
			enabled_group_count += 1;
		}
		chain_joint_count += group.bone_node_indices.len().saturating_sub(1);
		let has_angle_limit = dynamics_group_has_angle_constraint(group);
		if has_angle_limit {
			source_angle_limit_group_count += 1;
		}
		angle_limit_by_source_id.insert(group.source_id.clone(), has_angle_limit);
		dynamics_audit_bump_numeric(&mut runtime_ranges, "stiffness", group.stiffness);
		dynamics_audit_bump_numeric(&mut runtime_ranges, "pull", group.pull);
		dynamics_audit_bump_numeric(&mut runtime_ranges, "spring", group.spring);
		dynamics_audit_bump_numeric(&mut runtime_ranges, "dragForce", group.drag_force);
		dynamics_audit_bump_numeric(&mut runtime_ranges, "gravityPower", group.gravity_power);
		dynamics_audit_bump_numeric(&mut runtime_ranges, "gravityFalloff", group.gravity_falloff);
		dynamics_audit_bump_numeric(&mut runtime_ranges, "immobile", group.immobile);
		dynamics_audit_bump_numeric(&mut runtime_ranges, "hitRadius", group.hit_radius);
		dynamics_audit_bump_samples(&mut sample_counts, "stiffnessSamples", group.stiffness_samples.len());
		dynamics_audit_bump_samples(&mut sample_counts, "pullSamples", group.pull_samples.len());
		dynamics_audit_bump_samples(&mut sample_counts, "springSamples", group.spring_samples.len());
		dynamics_audit_bump_samples(&mut sample_counts, "gravityPowerSamples", group.gravity_power_samples.len());
		dynamics_audit_bump_samples(&mut sample_counts, "gravityFalloffSamples", group.gravity_falloff_samples.len());
		dynamics_audit_bump_samples(&mut sample_counts, "immobileSamples", group.immobile_samples.len());
		dynamics_audit_bump_samples(&mut sample_counts, "maxAngleXSamples", group.max_angle_x_samples.len());
		dynamics_audit_bump_samples(&mut sample_counts, "maxAngleZSamples", group.max_angle_z_samples.len());
		if let Some(limit) = &group.limit {
			dynamics_audit_bump_samples(&mut sample_counts, "maxStretchSamples", limit.max_stretch_samples.len());
			dynamics_audit_bump_samples(&mut sample_counts, "maxSquishSamples", limit.max_squish_samples.len());
			dynamics_audit_bump_samples(&mut sample_counts, "stretchMotionSamples", limit.stretch_motion_samples.len());
		}
		dynamics_audit_bump_samples(&mut sample_counts, "hitRadiusSamples", group.hit_radius_samples.len());
		group_sample_candidates.push((
			dynamics_import_group_sample_score(group, has_angle_limit),
			group_index,
			dynamics_import_group_sample(group, &node_paths_by_index),
		));
	}
	group_sample_candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
	let group_samples = group_sample_candidates
		.into_iter()
		.take(64)
		.map(|(_, _, sample)| sample)
		.collect::<Vec<_>>();
	let response_category_count = dynamics_response_category_summaries(&doc).len();
	let response_groups = dynamics_response_group_summaries(&doc);
	let response_group_count = response_groups.len();
	let mut active_angle_limit_group_count = 0usize;
	let mut cloth_angle_limit_metadata_only_count = 0usize;
	let mut hard_angle_constraint_group_count = 0usize;
	for response_group in &response_groups {
		if !angle_limit_by_source_id.get(&response_group.source_id).copied().unwrap_or(false) {
			continue;
		}
		active_angle_limit_group_count += 1;
		if response_group.category == "cloth" {
			cloth_angle_limit_metadata_only_count += 1;
		} else {
			hard_angle_constraint_group_count += 1;
		}
	}
	let missing_runtime_evidence = dynamics_import_missing_runtime_evidence(
		source_params_count,
		raw_groups.len(),
		groups.len(),
		chain_joint_count,
		response_group_count,
	);
	let (collider_count, contact_count, constraint_ref_count) = settings
		.map(|settings| (settings.colliders.len(), settings.contacts.len(), settings.constraint_refs.len()))
		.unwrap_or_default();
	let (
		node_constraint_count,
		parent_node_constraint_count,
		parent_node_constraint_source_count,
		parent_node_constraint_multi_source_count,
	) = dynamics_import_node_constraint_counts(doc.scene.as_ref());
	Ok(DynamicsImportAuditReport {
		path: path.display().to_string(),
		import_format_id: desc.id.0,
		import_provider_plugin_id: desc.provider_plugin_id,
		active_wardrobe_set,
		import_report,
		source_params_count,
		group_count: groups.len(),
		source_kind_counts,
		enabled_group_count,
		chain_joint_count,
		collider_count,
		contact_count,
		constraint_ref_count,
		node_constraint_count,
		parent_node_constraint_count,
		parent_node_constraint_source_count,
		parent_node_constraint_multi_source_count,
		source_angle_limit_group_count,
		active_angle_limit_group_count,
		cloth_angle_limit_metadata_only_count,
		hard_angle_constraint_group_count,
		response_category_count,
		response_group_count,
		runtime_ranges,
		sample_counts,
		group_samples,
		node_samples,
		skin_samples,
		mesh_cloth_assist_samples,
		missing_runtime_evidence,
	})
}

fn dynamics_import_node_constraint_counts(scene: Option<&UnaSceneSnapshot>) -> (usize, usize, usize, usize) {
	let Some(scene) = scene else {
		return (0, 0, 0, 0);
	};
	let mut parent_count = 0usize;
	let mut parent_source_count = 0usize;
	let mut parent_multi_source_count = 0usize;
	for constraint in &scene.node_constraints {
		if matches!(constraint.kind, UnaNodeConstraintKind::Parent { .. }) {
			parent_count += 1;
			let source_count = if constraint.sources.is_empty() {
				1
			} else {
				constraint.sources.len()
			};
			parent_source_count += source_count;
			if source_count > 1 {
				parent_multi_source_count += 1;
			}
		}
	}
	(
		scene.node_constraints.len(),
		parent_count,
		parent_source_count,
		parent_multi_source_count,
	)
}

fn dynamics_import_missing_runtime_evidence(
	source_params_count: usize,
	raw_group_count: usize,
	scoped_group_count: usize,
	chain_joint_count: usize,
	response_group_count: usize,
) -> Vec<String> {
	let mut missing_runtime_evidence = Vec::new();
	if source_params_count > 0 && raw_group_count == 0 {
		missing_runtime_evidence.push(format!("sourceParams={source_params_count} but imported runtime dynamics groups=0"));
	}
	if scoped_group_count > 0 && chain_joint_count == 0 {
		missing_runtime_evidence.push(format!("imported dynamics groups={scoped_group_count} but chain_joint_count=0"));
	}
	if scoped_group_count > 0 && response_group_count == 0 {
		missing_runtime_evidence.push(format!(
			"imported dynamics groups={scoped_group_count} but simulator response_group_count=0"
		));
	}
	missing_runtime_evidence
}

fn dynamics_import_group_sample_score(group: &un_avatar_core::UnaDynamicsSourceGroup, has_angle_limit: bool) -> usize {
	let sample_count = group.stiffness_samples.len()
		+ group.pull_samples.len()
		+ group.spring_samples.len()
		+ group.gravity_power_samples.len()
		+ group.gravity_falloff_samples.len()
		+ group.immobile_samples.len()
		+ group.max_angle_x_samples.len()
		+ group.max_angle_z_samples.len()
		+ group.hit_radius_samples.len()
		+ group
			.limit
			.as_ref()
			.map(|limit| limit.max_stretch_samples.len() + limit.max_squish_samples.len() + limit.stretch_motion_samples.len())
			.unwrap_or_default();
	let has_stretch_limit = group.limit.as_ref().is_some_and(|limit| {
		limit.max_stretch.abs() > 0.0
			|| limit.max_squish.abs() > 0.0
			|| limit.stretch_motion.unwrap_or(0.0).abs() > 0.0
			|| !limit.max_stretch_samples.is_empty()
			|| !limit.max_squish_samples.is_empty()
			|| !limit.stretch_motion_samples.is_empty()
	});
	usize::from(group.enabled) * 10_000
		+ usize::from(has_angle_limit) * 4_000
		+ usize::from(has_stretch_limit) * 4_000
		+ sample_count * 32
		+ group.bone_node_indices.len().min(255)
}

fn dynamics_import_group_sample(
	group: &un_avatar_core::UnaDynamicsSourceGroup,
	node_paths_by_index: &[Option<String>],
) -> DynamicsImportGroupSample {
	let chain_paths = group
		.bone_node_indices
		.iter()
		.filter_map(|node| node_paths_by_index.get(*node).cloned().flatten())
		.collect::<Vec<_>>();
	DynamicsImportGroupSample {
		source_id: group.source_id.clone(),
		category: group.category.clone(),
		enabled: group.enabled,
		source_kind: format!("{:?}", group.source_kind),
		chain_len: group.bone_node_indices.len(),
		root_path: group
			.bone_node_indices
			.first()
			.and_then(|node| node_paths_by_index.get(*node).cloned().flatten()),
		tip_path: group
			.bone_node_indices
			.last()
			.and_then(|node| node_paths_by_index.get(*node).cloned().flatten()),
		chain_paths,
	}
}

fn dynamics_import_scene_samples(
	scene: &un_avatar_core::UnaSceneSnapshot,
	groups: &[un_avatar_core::UnaDynamicsSourceGroup],
	node_paths_by_index: &[Option<String>],
) -> (
	Vec<DynamicsImportNodeSample>,
	Vec<DynamicsImportSkinSample>,
	Vec<DynamicsImportMeshClothAssistSample>,
) {
	let parents = scene_parent_indices(scene);
	let dynamic_nodes = groups
		.iter()
		.flat_map(|group| group.bone_node_indices.iter().copied())
		.collect::<BTreeSet<_>>();
	let mut sampled_node_indices = dynamic_nodes.clone();
	for &node_index in &dynamic_nodes {
		if let Some(parent) = parents.get(node_index).copied().flatten() {
			sampled_node_indices.insert(parent);
		}
		if let Some(node) = scene.nodes.get(node_index) {
			sampled_node_indices.extend(node.children.iter().copied());
		}
	}
	for (node_index, node) in scene.nodes.iter().enumerate() {
		if let Some(skin_index) = node.skin {
			if scene
				.skins
				.get(skin_index)
				.is_some_and(|skin| skin.joint_nodes.iter().any(|joint_node| dynamic_nodes.contains(joint_node)))
			{
				sampled_node_indices.insert(node_index);
			}
		}
	}
	let node_samples = sampled_node_indices
		.into_iter()
		.filter_map(|index| {
			let node = scene.nodes.get(index)?;
			let path = node_paths_by_index.get(index).and_then(|path| path.as_deref());
			let parent_index = parents.get(index).copied().flatten();
			Some(DynamicsImportNodeSample {
				index,
				name: node.name.clone(),
				path: path.map(str::to_string),
				parent_index,
				parent_path: parent_index.and_then(|parent| node_paths_by_index.get(parent).cloned().flatten()),
				mesh: node.mesh,
				skin: node.skin,
				children: node.children.clone(),
			})
		})
		.take(160)
		.collect::<Vec<_>>();
	let skin_samples = scene
		.nodes
		.iter()
		.enumerate()
		.filter_map(|(node_index, node)| {
			let path = node_paths_by_index.get(node_index).cloned().flatten();
			let skin_index = node.skin?;
			let skin = scene.skins.get(skin_index)?;
			if !skin.joint_nodes.iter().any(|joint_node| dynamic_nodes.contains(joint_node)) {
				return None;
			}
			let joints = skin
				.joint_nodes
				.iter()
				.enumerate()
				.filter_map(|(joint_index, &joint_node)| {
					let joint = scene.nodes.get(joint_node)?;
					let joint_path = node_paths_by_index.get(joint_node).cloned().flatten();
					if !dynamic_nodes.contains(&joint_node)
						&& !parents
							.get(joint_node)
							.copied()
							.flatten()
							.is_some_and(|parent| dynamic_nodes.contains(&parent))
					{
						return None;
					}
					let parent_index = parents.get(joint_node).copied().flatten();
					Some(DynamicsImportSkinJointSample {
						joint_index,
						node_index: joint_node,
						name: joint.name.clone(),
						path: joint_path,
						parent_index,
						parent_path: parent_index.and_then(|parent| node_paths_by_index.get(parent).cloned().flatten()),
					})
				})
				.take(48)
				.collect::<Vec<_>>();
			let region_samples = dynamics_import_skin_region_samples(scene, node.mesh, skin, node_paths_by_index);
			Some(DynamicsImportSkinSample {
				node_index,
				node_path: path,
				skin_index,
				skeleton_node: skin.skeleton_node,
				skeleton_path: skin.skeleton_node.and_then(|node| node_paths_by_index.get(node).cloned().flatten()),
				joint_count: skin.joint_nodes.len(),
				joints,
				region_samples,
			})
		})
		.take(32)
		.collect::<Vec<_>>();
	let mesh_cloth_assist_samples = dynamics_import_mesh_cloth_assist_samples(scene, groups, node_paths_by_index);
	(node_samples, skin_samples, mesh_cloth_assist_samples)
}

fn dynamics_group_has_angle_constraint(group: &un_avatar_core::UnaDynamicsSourceGroup) -> bool {
	let Some(limit) = group.limit.as_ref() else {
		return !group.max_angle_x_samples.is_empty() || !group.max_angle_z_samples.is_empty();
	};
	let limit_type = limit.limit_type.trim();
	!limit_type.is_empty()
		|| limit.max_angle_x.abs() > 0.0
		|| limit.max_angle_z.abs() > 0.0
		|| limit.limit_rotation.iter().any(|value| value.abs() > 0.0)
		|| !group.max_angle_x_samples.is_empty()
		|| !group.max_angle_z_samples.is_empty()
}

fn dynamics_import_skin_region_samples(
	scene: &un_avatar_core::UnaSceneSnapshot,
	mesh_index: Option<usize>,
	skin: &un_avatar_core::UnaSkin,
	node_paths_by_index: &[Option<String>],
) -> Vec<DynamicsImportSkinRegionSample> {
	let Some(mesh_index) = mesh_index else {
		return Vec::new();
	};
	let Some(primitives) = scene.meshes.get(mesh_index) else {
		return Vec::new();
	};
	let mut out = Vec::new();
	for (primitive_index, primitive) in primitives.iter().enumerate() {
		let Some(joints) = primitive.joints.as_ref() else {
			continue;
		};
		let Some(weights) = primitive.weights.as_ref() else {
			continue;
		};
		let count = primitive.positions.len().min(joints.len()).min(weights.len());
		let bounds = dynamics_import_position_bounds(&primitive.positions[..count]);
		for region in DYNAMICS_IMPORT_SPATIAL_REGIONS {
			let mut vertex_count = 0usize;
			let mut dominant_counts = BTreeMap::<usize, f32>::new();
			let mut weight_sums = BTreeMap::<usize, f32>::new();
			for vertex_index in 0..count {
				let position = primitive.positions[vertex_index];
				if !dynamics_import_spatial_region_matches(region, position, bounds) {
					continue;
				}
				vertex_count += 1;
				let vertex_joints = joints[vertex_index];
				let vertex_weights = weights[vertex_index];
				let mut dominant_joint = None;
				let mut dominant_weight = f32::NEG_INFINITY;
				for lane in 0..4 {
					let joint_index = vertex_joints[lane] as usize;
					let weight = vertex_weights[lane];
					if weight <= 0.0 || !weight.is_finite() {
						continue;
					}
					*weight_sums.entry(joint_index).or_default() += weight;
					if weight > dominant_weight {
						dominant_weight = weight;
						dominant_joint = Some(joint_index);
					}
				}
				if let Some(joint_index) = dominant_joint {
					*dominant_counts.entry(joint_index).or_default() += 1.0;
				}
			}
			if vertex_count == 0 {
				continue;
			}
			out.push(DynamicsImportSkinRegionSample {
				primitive_index,
				primitive_name: primitive.name.clone(),
				region: (*region).to_string(),
				vertex_count,
				dominant_counts: dynamics_import_skin_influence_samples(&dominant_counts, skin, node_paths_by_index, 12),
				weight_sums: dynamics_import_skin_influence_samples(&weight_sums, skin, node_paths_by_index, 12),
			});
		}
	}
	out
}

const DYNAMICS_IMPORT_SPATIAL_REGIONS: &[&str] = &["all", "upper", "lower", "left", "right", "center_x", "front", "back"];

#[derive(Clone, Copy)]
struct DynamicsImportPositionBounds {
	min: Vec3,
	max: Vec3,
	center: Vec3,
}

fn dynamics_import_position_bounds(positions: &[[f32; 3]]) -> DynamicsImportPositionBounds {
	let mut min = Vec3::splat(f32::INFINITY);
	let mut max = Vec3::splat(f32::NEG_INFINITY);
	for position in positions {
		let p = Vec3::from_array(*position);
		min = min.min(p);
		max = max.max(p);
	}
	if !min.is_finite() || !max.is_finite() {
		min = Vec3::ZERO;
		max = Vec3::ZERO;
	}
	DynamicsImportPositionBounds {
		min,
		max,
		center: (min + max) * 0.5,
	}
}

fn dynamics_import_spatial_region_matches(region: &str, position: [f32; 3], bounds: DynamicsImportPositionBounds) -> bool {
	let p = Vec3::from_array(position);
	let size = (bounds.max - bounds.min).max(Vec3::splat(1e-5));
	match region {
		"all" => true,
		"upper" => p.y >= bounds.center.y,
		"lower" => p.y < bounds.center.y,
		"left" => p.x < bounds.center.x - size.x * 0.15,
		"right" => p.x > bounds.center.x + size.x * 0.15,
		"center_x" => (p.x - bounds.center.x).abs() <= size.x * 0.15,
		"front" => p.z >= bounds.center.z,
		"back" => p.z < bounds.center.z,
		_ => false,
	}
}

fn dynamics_import_skin_influence_samples(
	values: &BTreeMap<usize, f32>,
	skin: &un_avatar_core::UnaSkin,
	node_paths_by_index: &[Option<String>],
	limit: usize,
) -> Vec<DynamicsImportSkinInfluenceSample> {
	let mut items = values.iter().map(|(&joint_index, &value)| (joint_index, value)).collect::<Vec<_>>();
	items.sort_by(|a, b| {
		b.1.partial_cmp(&a.1)
			.unwrap_or(std::cmp::Ordering::Equal)
			.then_with(|| a.0.cmp(&b.0))
	});
	items
		.into_iter()
		.take(limit)
		.map(|(joint_index, value)| {
			let node_index = skin.joint_nodes.get(joint_index).copied().unwrap_or(usize::MAX);
			let node_path = node_paths_by_index.get(node_index).cloned().flatten();
			let name = node_path.as_deref().and_then(|path| path.rsplit('/').next()).map(str::to_string);
			DynamicsImportSkinInfluenceSample {
				joint_index,
				node_index,
				name,
				path: node_path,
				value,
			}
		})
		.collect()
}

fn dynamics_import_mesh_cloth_assist_samples(
	scene: &un_avatar_core::UnaSceneSnapshot,
	groups: &[un_avatar_core::UnaDynamicsSourceGroup],
	node_paths_by_index: &[Option<String>],
) -> Vec<DynamicsImportMeshClothAssistSample> {
	let physics_config = DynamicsPhysicsConfig::default().normalized();
	let config = &physics_config.mesh_cloth_assist;
	let dynamic_nodes = dynamics_mesh_cloth_assist_source_dynamic_nodes(scene, groups);
	if dynamic_nodes.is_empty() {
		return Vec::new();
	}
	let mut out = Vec::new();
	for (node_index, node) in scene.nodes.iter().enumerate() {
		let Some(mesh_index) = node.mesh else {
			continue;
		};
		let Some(skin_index) = node.skin else {
			continue;
		};
		let Some(skin) = scene.skins.get(skin_index) else {
			continue;
		};
		let node_path = node_paths_by_index.get(node_index).cloned().flatten();
		if !dynamics_mesh_cloth_assist_mesh_matches(node_path.as_deref(), &config.mesh_path_contains, &physics_config.categories) {
			continue;
		}
		let Some(primitives) = scene.meshes.get(mesh_index) else {
			continue;
		};
		let joint_count = skin.joint_nodes.len().min(skin.inverse_bind_matrices.len());
		let joint_roles = dynamics_mesh_cloth_assist_joint_roles(skin, joint_count, Some(&dynamic_nodes), |joint_index| {
			dynamics_mesh_cloth_assist_joint_leaf(skin, node_paths_by_index, joint_index)
		});
		if !joint_roles.iter().any(|role| *role == DynamicsMeshClothAssistJointRole::Dynamic) {
			continue;
		}
		for (primitive_index, primitive) in primitives.iter().enumerate() {
			let Some(joints) = primitive.joints.as_ref() else {
				continue;
			};
			let Some(weights) = primitive.weights.as_ref() else {
				continue;
			};
			let count = primitive.positions.len().min(joints.len()).min(weights.len());
			let bounds = dynamics_import_position_bounds(&primitive.positions[..count]);
			let profiles = (0..count)
				.map(|vertex_index| {
					dynamics_import_mesh_cloth_assist_vertex_profile(joints[vertex_index], weights[vertex_index], &joint_roles)
				})
				.collect::<Vec<_>>();
			let (neighbor_dynamic_max, neighbor_dynamic_joint) =
				dynamics_import_mesh_cloth_assist_neighbor_dynamic(count, primitive.indices.as_deref(), &profiles);
			for region in DYNAMICS_IMPORT_SPATIAL_REGIONS {
				let mut vertex_count = 0usize;
				let mut candidate_count = 0usize;
				let mut existing_dynamic_candidate_count = 0usize;
				let mut static_cloth_bridge_candidate_count = 0usize;
				let mut seed_candidate_count = 0usize;
				let mut body_weight_sum = 0.0_f32;
				let mut dynamic_weight_sum = 0.0_f32;
				let mut static_cloth_weight_sum = 0.0_f32;
				let mut suggested_assist_weight_sum = 0.0_f32;
				let mut seeded_assist_weight_sum = 0.0_f32;
				let mut body_sources = BTreeMap::<usize, f32>::new();
				let mut dynamic_targets = BTreeMap::<usize, f32>::new();
				for vertex_index in 0..count {
					let position = primitive.positions[vertex_index];
					if !dynamics_import_spatial_region_matches(region, position, bounds) {
						continue;
					}
					vertex_count += 1;
					let vertex_joints = joints[vertex_index];
					let vertex_weights = weights[vertex_index];
					let profile = profiles[vertex_index];
					let body_weight = profile.body_weight;
					let dynamic_weight = profile.dynamic_weight;
					let static_cloth_weight = profile.static_cloth_weight;
					let has_dynamic_lane = profile.strongest_dynamic_joint.is_some();
					let Some(candidate) = dynamics_mesh_cloth_assist_transfer_candidate(
						config,
						has_dynamic_lane,
						body_weight,
						dynamic_weight,
						static_cloth_weight,
						neighbor_dynamic_max[vertex_index],
						0.0,
					) else {
						continue;
					};
					if candidate.kind == DynamicsMeshClothAssistTransferKind::SeedMissingDynamicLane {
						let Some(seed_target) = neighbor_dynamic_joint[vertex_index] else {
							continue;
						};
						candidate_count += 1;
						seed_candidate_count += 1;
						static_cloth_bridge_candidate_count += 1;
						body_weight_sum += body_weight;
						dynamic_weight_sum += dynamic_weight;
						static_cloth_weight_sum += static_cloth_weight;
						let assist_weight = candidate.transfer_weight;
						suggested_assist_weight_sum += assist_weight;
						seeded_assist_weight_sum += assist_weight;
						for lane in 0..4 {
							let joint_index = vertex_joints[lane] as usize;
							let weight = vertex_weights[lane];
							if weight <= 0.0 || !weight.is_finite() {
								continue;
							}
							if joint_roles.get(joint_index).copied() == Some(DynamicsMeshClothAssistJointRole::Body) {
								*body_sources.entry(joint_index).or_default() += weight;
							}
						}
						*dynamic_targets.entry(seed_target).or_default() += assist_weight;
						continue;
					}
					if candidate.kind != DynamicsMeshClothAssistTransferKind::ExistingDynamicLane {
						continue;
					}
					candidate_count += 1;
					existing_dynamic_candidate_count += 1;
					if static_cloth_weight >= config.min_existing_dynamic_weight {
						static_cloth_bridge_candidate_count += 1;
					}
					body_weight_sum += body_weight;
					dynamic_weight_sum += dynamic_weight;
					static_cloth_weight_sum += static_cloth_weight;
					let assist_weight = candidate.transfer_weight;
					suggested_assist_weight_sum += assist_weight;
					for lane in 0..4 {
						let joint_index = vertex_joints[lane] as usize;
						let weight = vertex_weights[lane];
						if weight <= 0.0 || !weight.is_finite() {
							continue;
						}
						if joint_roles.get(joint_index).copied() == Some(DynamicsMeshClothAssistJointRole::Body) {
							*body_sources.entry(joint_index).or_default() += weight;
						}
					}
					if let Some(joint_index) = profile.strongest_dynamic_joint {
						*dynamic_targets.entry(joint_index).or_default() += assist_weight;
					}
				}
				if candidate_count == 0 {
					continue;
				}
				out.push(DynamicsImportMeshClothAssistSample {
					node_index,
					node_path: node_path.clone(),
					mesh_index,
					primitive_index,
					primitive_name: primitive.name.clone(),
					region: (*region).to_string(),
					vertex_count,
					candidate_count,
					existing_dynamic_candidate_count,
					static_cloth_bridge_candidate_count,
					seed_candidate_count,
					body_weight_sum,
					dynamic_weight_sum,
					static_cloth_weight_sum,
					suggested_assist_weight_sum,
					seeded_assist_weight_sum,
					body_sources: dynamics_import_skin_influence_samples(&body_sources, skin, node_paths_by_index, 8),
					dynamic_targets: dynamics_import_skin_influence_samples(&dynamic_targets, skin, node_paths_by_index, 8),
				});
			}
		}
	}
	out
}

fn dynamics_mesh_cloth_assist_mesh_matches(node_path: Option<&str>, filters: &[String], categories: &[DynamicsCategoryDefinition]) -> bool {
	let Some(node_path) = node_path else {
		return false;
	};
	skeleton_mesh_cloth_assist_mesh_matches_with_categories(node_path, filters, categories)
}

fn dynamics_mesh_cloth_assist_source_group_is_cloth(
	scene: &un_avatar_core::UnaSceneSnapshot,
	group: &un_avatar_core::UnaDynamicsSourceGroup,
	categories: &[DynamicsCategoryDefinition],
) -> bool {
	if group.category.trim().eq_ignore_ascii_case("cloth") {
		return true;
	}
	let mut text = String::new();
	text.push_str(&group.comment);
	text.push(' ');
	text.push_str(&group.source_id);
	for &node_index in &group.bone_node_indices {
		if let Some(name) = scene.nodes.get(node_index).and_then(|node| node.name.as_deref()) {
			text.push(' ');
			text.push_str(name);
		}
	}
	dynamics_mesh_cloth_assist_mesh_matches(Some(&text), &[], categories)
}

fn dynamics_mesh_cloth_assist_source_dynamic_nodes(
	scene: &un_avatar_core::UnaSceneSnapshot,
	groups: &[un_avatar_core::UnaDynamicsSourceGroup],
) -> Vec<usize> {
	let physics_config = DynamicsPhysicsConfig::default().normalized();
	let mut nodes = groups
		.iter()
		.filter(|group| group.enabled && dynamics_mesh_cloth_assist_source_group_is_cloth(scene, group, &physics_config.categories))
		.flat_map(|group| dynamics_mesh_cloth_assist_deforming_nodes(&group.bone_node_indices, group.interaction_chain_start_index))
		.collect::<Vec<_>>();
	nodes.sort_unstable();
	nodes.dedup();
	nodes
}

fn dynamics_mesh_cloth_assist_runtime_dynamic_nodes(
	scene: &un_avatar_core::UnaSceneSnapshot,
	runtime_dynamics: un_avatar_core::UnaRuntimeDynamics<'_>,
	categories: &[un_avatar_skeleton::DynamicsCategoryDefinition],
) -> Vec<usize> {
	let mut nodes = runtime_dynamics
		.dynamics_groups()
		.filter(|group| {
			group.effective_enabled
				&& runtime_dynamics.source_id_resident_in_scene(scene, group.source_id)
				&& classify_dynamics_group_category(scene, *group, &categories) == "cloth"
		})
		.flat_map(|group| dynamics_mesh_cloth_assist_deforming_nodes(group.chain.bone_node_indices, group.chain.interaction_start_index))
		.collect::<Vec<_>>();
	nodes.sort_unstable();
	nodes.dedup();
	nodes
}

#[derive(Clone, Copy, Default)]
struct DynamicsImportMeshClothAssistVertexProfile {
	body_weight: f32,
	dynamic_weight: f32,
	static_cloth_weight: f32,
	strongest_dynamic_joint: Option<usize>,
	strongest_dynamic_weight: f32,
}

fn dynamics_import_mesh_cloth_assist_vertex_profile(
	vertex_joints: [u16; 4],
	vertex_weights: [f32; 4],
	joint_roles: &[DynamicsMeshClothAssistJointRole],
) -> DynamicsImportMeshClothAssistVertexProfile {
	let mut profile = DynamicsImportMeshClothAssistVertexProfile::default();
	for lane in 0..4 {
		let joint_index = vertex_joints[lane] as usize;
		let weight = vertex_weights[lane];
		if weight <= 0.0 || !weight.is_finite() {
			continue;
		}
		match joint_roles
			.get(joint_index)
			.copied()
			.unwrap_or(DynamicsMeshClothAssistJointRole::Other)
		{
			DynamicsMeshClothAssistJointRole::Dynamic => {
				profile.dynamic_weight += weight;
				if weight > profile.strongest_dynamic_weight {
					profile.strongest_dynamic_weight = weight;
					profile.strongest_dynamic_joint = Some(joint_index);
				}
			}
			DynamicsMeshClothAssistJointRole::Body => {
				profile.body_weight += weight;
			}
			DynamicsMeshClothAssistJointRole::StaticCloth => {
				profile.static_cloth_weight += weight;
			}
			DynamicsMeshClothAssistJointRole::Other => {}
		}
	}
	profile
}

fn dynamics_import_mesh_cloth_assist_neighbor_dynamic(
	vertex_count: usize,
	indices: Option<&[u32]>,
	profiles: &[DynamicsImportMeshClothAssistVertexProfile],
) -> (Vec<f32>, Vec<Option<usize>>) {
	let mut neighbor_dynamic_max = vec![0.0_f32; vertex_count];
	let mut neighbor_dynamic_joint = vec![None::<usize>; vertex_count];
	let Some(indices) = indices else {
		return (neighbor_dynamic_max, neighbor_dynamic_joint);
	};
	for_each_dynamics_mesh_cloth_assist_neighbor(indices, vertex_count, |vertex_index, neighbor_index| {
		dynamics_import_mesh_cloth_assist_neighbor(
			vertex_index,
			profiles[neighbor_index],
			&mut neighbor_dynamic_max,
			&mut neighbor_dynamic_joint,
		);
	});
	(neighbor_dynamic_max, neighbor_dynamic_joint)
}

fn dynamics_import_mesh_cloth_assist_neighbor(
	vertex_index: usize,
	neighbor: DynamicsImportMeshClothAssistVertexProfile,
	neighbor_dynamic_max: &mut [f32],
	neighbor_dynamic_joint: &mut [Option<usize>],
) {
	if neighbor.strongest_dynamic_weight <= neighbor_dynamic_max[vertex_index] {
		return;
	}
	let Some(joint_index) = neighbor.strongest_dynamic_joint else {
		return;
	};
	neighbor_dynamic_max[vertex_index] = neighbor.strongest_dynamic_weight;
	neighbor_dynamic_joint[vertex_index] = Some(joint_index);
}

fn dynamics_mesh_cloth_assist_joint_leaf<'a>(
	skin: &un_avatar_core::UnaSkin,
	node_paths_by_index: &'a [Option<String>],
	joint_index: usize,
) -> &'a str {
	let Some(node_index) = skin.joint_nodes.get(joint_index).copied() else {
		return "";
	};
	node_paths_by_index
		.get(node_index)
		.and_then(|path| path.as_deref())
		.unwrap_or("")
		.rsplit('/')
		.next()
		.unwrap_or("")
}

fn require_dynamics_runtime_evidence(report: &DynamicsImportAuditReport) -> Result<(), String> {
	if report.missing_runtime_evidence.is_empty() {
		Ok(())
	} else {
		Err(format!(
			"UNPhysics importer/runtime evidence is missing: {}",
			report.missing_runtime_evidence.join(", ")
		))
	}
}

fn run_dynamics_import_audit(
	plugin_dirs: &[PathBuf],
	path: PathBuf,
	input_format: Option<String>,
	wardrobe_set: Option<String>,
	require_runtime_evidence: bool,
	json: bool,
) -> Result<(), String> {
	let report = dynamics_import_audit_report(plugin_dirs, &path, input_format.as_deref(), wardrobe_set.as_deref())?;
	let required = if require_runtime_evidence {
		require_dynamics_runtime_evidence(&report)
	} else {
		Ok(())
	};
	if json {
		write_json_stdout(&report)?;
		return required;
	}
	println!("path: {}", report.path);
	let plug = report
		.import_provider_plugin_id
		.as_ref()
		.map(|p| format!(" ({p})"))
		.unwrap_or_default();
	println!("importer: {}{}", report.import_format_id, plug);
	if let Some(set_id) = &report.active_wardrobe_set {
		println!("active_wardrobe_set: {set_id}");
	}
	println!(
		"dynamics: sourceParams={} groups={} enabled={} joints={} colliders={} contacts={} constraints={}",
		report.source_params_count,
		report.group_count,
		report.enabled_group_count,
		report.chain_joint_count,
		report.collider_count,
		report.contact_count,
		report.constraint_ref_count
	);
	println!(
		"node_constraints: total={} parent={} parent_sources={} parent_multi_source={}",
		report.node_constraint_count,
		report.parent_node_constraint_count,
		report.parent_node_constraint_source_count,
		report.parent_node_constraint_multi_source_count
	);
	println!(
		"response: categories={} groups={}",
		report.response_category_count, report.response_group_count
	);
	println!(
		"limit policy: source_angle_groups={} active_angle_groups={} hard_angle_constraints={} cloth_angle_metadata_only={}",
		report.source_angle_limit_group_count,
		report.active_angle_limit_group_count,
		report.hard_angle_constraint_group_count,
		report.cloth_angle_limit_metadata_only_count
	);
	println!("source_kinds: {:?}", report.source_kind_counts);
	if !report.runtime_ranges.is_empty() {
		println!("runtime ranges:");
		for (key, range) in &report.runtime_ranges {
			println!("  {key}: count={} min={} max={}", range.count, range.min, range.max);
		}
	}
	if !report.sample_counts.is_empty() {
		println!("sample counts: {:?}", report.sample_counts);
	}
	if !report.missing_runtime_evidence.is_empty() {
		println!("missing runtime evidence: {}", report.missing_runtime_evidence.join(", "));
	}
	required
}

fn dynamics_response_audit_config(params: DynamicsPhysicsParams) -> DynamicsPhysicsConfig {
	let categories = DynamicsPhysicsConfig::default().categories;
	let overrides = categories
		.iter()
		.map(|category| un_avatar_skeleton::DynamicsCategoryOverride {
			category: category.id.clone(),
			params,
		})
		.collect();
	DynamicsPhysicsConfig {
		categories,
		overrides,
		..Default::default()
	}
}

fn dynamics_soft_audit_params() -> DynamicsPhysicsParams {
	DynamicsPhysicsParams {
		rest_response: Some(0.04),
		shape_preservation: Some(0.02),
		bounce_scale: Some(0.45),
		stretch_range_scale: Some(0.5),
		stretch_motion: Some(0.05),
		motion_coupling: Some(0.25),
		damping_half_life_ms: Some(95.0),
		..Default::default()
	}
}

fn dynamics_firm_audit_params() -> DynamicsPhysicsParams {
	DynamicsPhysicsParams {
		rest_response: Some(0.28),
		shape_preservation: Some(0.22),
		bounce_scale: Some(0.35),
		stretch_range_scale: Some(1.5),
		stretch_motion: Some(0.85),
		motion_coupling: Some(0.82),
		damping_half_life_ms: Some(80.0),
		..Default::default()
	}
}

fn dynamics_motion_trace_tuning_config(tuning: &str) -> Result<(String, DynamicsPhysicsConfig), String> {
	match tuning {
		"authored" => Ok(("authored".to_string(), DynamicsPhysicsConfig::default())),
		"soft" => Ok(("soft".to_string(), dynamics_response_audit_config(dynamics_soft_audit_params()))),
		"firm" => Ok(("firm".to_string(), dynamics_response_audit_config(dynamics_firm_audit_params()))),
		"rest-low" => Ok((
			"rest-low".to_string(),
			dynamics_response_audit_config(DynamicsPhysicsParams {
				rest_response: Some(0.04),
				..Default::default()
			}),
		)),
		"rest-high" => Ok((
			"rest-high".to_string(),
			dynamics_response_audit_config(DynamicsPhysicsParams {
				rest_response: Some(0.28),
				..Default::default()
			}),
		)),
		"shape-low" => Ok((
			"shape-low".to_string(),
			dynamics_response_audit_config(DynamicsPhysicsParams {
				shape_preservation: Some(0.02),
				..Default::default()
			}),
		)),
		"shape-high" => Ok((
			"shape-high".to_string(),
			dynamics_response_audit_config(DynamicsPhysicsParams {
				shape_preservation: Some(0.22),
				..Default::default()
			}),
		)),
		"bounce-low" => Ok((
			"bounce-low".to_string(),
			dynamics_response_audit_config(DynamicsPhysicsParams {
				bounce_scale: Some(0.35),
				..Default::default()
			}),
		)),
		"bounce-high" => Ok((
			"bounce-high".to_string(),
			dynamics_response_audit_config(DynamicsPhysicsParams {
				bounce_scale: Some(0.55),
				..Default::default()
			}),
		)),
		"follow-low" => Ok((
			"follow-low".to_string(),
			dynamics_response_audit_config(DynamicsPhysicsParams {
				motion_coupling: Some(0.25),
				..Default::default()
			}),
		)),
		"follow-high" => Ok((
			"follow-high".to_string(),
			dynamics_response_audit_config(DynamicsPhysicsParams {
				motion_coupling: Some(0.82),
				..Default::default()
			}),
		)),
		"gravity-off" => Ok((
			"gravity-off".to_string(),
			dynamics_response_audit_config(DynamicsPhysicsParams {
				gravity_scale: Some(0.0),
				..Default::default()
			}),
		)),
		"gravity-low" => Ok((
			"gravity-low".to_string(),
			dynamics_response_audit_config(DynamicsPhysicsParams {
				gravity_scale: Some(0.35),
				..Default::default()
			}),
		)),
		"gravity-high" => Ok((
			"gravity-high".to_string(),
			dynamics_response_audit_config(DynamicsPhysicsParams {
				gravity_scale: Some(1.6),
				..Default::default()
			}),
		)),
		"stretch-off" => Ok((
			"stretch-off".to_string(),
			dynamics_response_audit_config(DynamicsPhysicsParams {
				stretch_range_scale: Some(0.0),
				stretch_motion: Some(0.0),
				..Default::default()
			}),
		)),
		"stretch-low" => Ok((
			"stretch-low".to_string(),
			dynamics_response_audit_config(DynamicsPhysicsParams {
				stretch_range_scale: Some(0.25),
				stretch_motion: Some(0.1),
				..Default::default()
			}),
		)),
		"stretch-high" => Ok((
			"stretch-high".to_string(),
			dynamics_response_audit_config(DynamicsPhysicsParams {
				stretch_range_scale: Some(1.5),
				stretch_motion: Some(0.85),
				..Default::default()
			}),
		)),
		"damping-long" => Ok((
			"damping-long".to_string(),
			dynamics_response_audit_config(DynamicsPhysicsParams {
				damping_half_life_ms: Some(220.0),
				..Default::default()
			}),
		)),
		"damping-short" => Ok((
			"damping-short".to_string(),
			dynamics_response_audit_config(DynamicsPhysicsParams {
				damping_half_life_ms: Some(80.0),
				..Default::default()
			}),
		)),
		other => Err(format!("unsupported dynamics motion trace tuning: {other}")),
	}
}

fn dynamics_response_audit_mode(name: &str, doc: &UnaDocument, config: DynamicsPhysicsConfig) -> Result<DynamicsResponseAuditMode, String> {
	let scene = doc.scene.as_ref().ok_or_else(|| "document has no scene".to_string())?;
	let settings = doc.dynamics().ok_or_else(|| "document has no dynamics settings".to_string())?;
	let runtime_model = doc.runtime_model();
	let runtime_dynamics = runtime_model
		.scene_profile_dynamics()
		.map(|runtime| runtime.dynamics)
		.unwrap_or_else(|| settings.runtime_dynamics());
	let category_definitions = config.categories.clone();
	let sim = DynamicsSimulator::new_with_runtime_dynamics(scene, runtime_dynamics, Vec::new(), config)
		.ok_or_else(|| "UNPhysics simulator could not be created".to_string())?;
	let mut categories = sim.response_category_summaries();
	let mut groups = sim.response_group_summaries();
	let visual_target_context = DynamicsVisualTargetContext::for_scene(scene);
	annotate_dynamics_response_group_visibility(&mut groups, scene, runtime_dynamics);
	for group in runtime_dynamics
		.dynamics_groups()
		.filter(|group| group.effective_enabled && runtime_dynamics.source_id_resident_in_scene(scene, group.source_id))
	{
		let category_name = classify_dynamics_group_category(scene, group, &category_definitions);
		let (skinned_joint_count, mesh_subtree_node_count) = visual_target_context.group_counts(group.chain.bone_node_indices);
		let Some(category) = categories.iter_mut().find(|category| category.category == category_name) else {
			continue;
		};
		if skinned_joint_count > 0 || mesh_subtree_node_count > 0 {
			category.visual_target_group_count += 1;
		} else {
			category.nonvisual_group_count += 1;
		}
		category.visible_skinned_joint_count += skinned_joint_count;
		category.visible_mesh_subtree_node_count += mesh_subtree_node_count;
	}
	let mut joint_count = 0usize;
	let mut group_count = 0usize;
	let mut rest = 0.0f32;
	let mut shape = 0.0f32;
	let mut bounce = 0.0f32;
	let mut max_stretch = 0.0f32;
	let mut max_squish = 0.0f32;
	let mut stretch_motion = 0.0f32;
	let mut damping = 0.0f32;
	let mut damping_weight = 0usize;
	let mut motion = 0.0f32;
	let mut orientation = 0.0f32;
	let mut xpbd_group_count = 0usize;
	for category in &categories {
		let weight = category.joint_count as f32;
		joint_count += category.joint_count;
		group_count += category.group_count;
		xpbd_group_count += category.xpbd_group_count;
		rest += category.average_rest_response * weight;
		shape += category.average_shape_preservation * weight;
		bounce += category.average_bounce_response * weight;
		max_stretch += category.average_max_stretch_response * weight;
		max_squish += category.average_max_squish_response * weight;
		stretch_motion += category.average_stretch_motion_response * weight;
		if let Some(half_life) = category.average_damping_half_life_ms {
			damping += half_life * weight;
			damping_weight += category.joint_count;
		}
		motion += category.average_parent_motion_follow * weight;
		orientation += category.average_orientation_follow * weight;
	}
	let denom = joint_count.max(1) as f32;
	Ok(DynamicsResponseAuditMode {
		name: name.to_string(),
		group_count,
		joint_count,
		average_rest_response: rest / denom,
		average_shape_preservation: shape / denom,
		average_bounce_response: bounce / denom,
		average_max_stretch_response: max_stretch / denom,
		average_max_squish_response: max_squish / denom,
		average_stretch_motion_response: stretch_motion / denom,
		average_damping_half_life_ms: (damping_weight > 0).then_some(damping / damping_weight as f32),
		average_parent_motion_follow: motion / denom,
		average_orientation_follow: orientation / denom,
		xpbd_group_count,
		category_count: categories.len(),
		categories,
		groups,
	})
}

fn dynamics_response_value_in_range(value: f32, min: f32, max: f32) -> bool {
	value.is_finite() && value >= min && value <= max
}

fn collect_dynamics_response_bounds_evidence(mode: &DynamicsResponseAuditMode, out: &mut Vec<String>) {
	let check = |out: &mut Vec<String>, key: &str, value: f32, min: f32, max: f32| {
		if !dynamics_response_value_in_range(value, min, max) {
			out.push(format!("{} {key} out of range [{min}, {max}]: {value}", mode.name));
		}
	};
	check(out, "average_rest_response", mode.average_rest_response, 0.0, 1.0);
	check(out, "average_shape_preservation", mode.average_shape_preservation, 0.0, 1.0);
	check(out, "average_bounce_response", mode.average_bounce_response, 0.0, 1.0);
	check(out, "average_parent_motion_follow", mode.average_parent_motion_follow, 0.0, 1.0);
	check(out, "average_orientation_follow", mode.average_orientation_follow, 0.0, 1.0);
	check(
		out,
		"average_max_stretch_response",
		mode.average_max_stretch_response,
		0.0,
		f32::MAX,
	);
	check(out, "average_max_squish_response", mode.average_max_squish_response, 0.0, 0.95);
	check(
		out,
		"average_stretch_motion_response",
		mode.average_stretch_motion_response,
		0.0,
		1.0,
	);
	if let Some(half_life) = mode.average_damping_half_life_ms {
		check(out, "average_damping_half_life_ms", half_life, 0.0, f32::MAX);
	}
	for category in &mode.categories {
		let prefix = format!("{} category {}", mode.name, category.category);
		let check_category = |out: &mut Vec<String>, key: &str, value: f32, min: f32, max: f32| {
			if !dynamics_response_value_in_range(value, min, max) {
				out.push(format!("{prefix} {key} out of range [{min}, {max}]: {value}"));
			}
		};
		check_category(out, "min_rest_response", category.min_rest_response, 0.0, 1.0);
		check_category(out, "max_rest_response", category.max_rest_response, 0.0, 1.0);
		check_category(out, "min_shape_preservation", category.min_shape_preservation, 0.0, 1.0);
		check_category(out, "max_shape_preservation", category.max_shape_preservation, 0.0, 1.0);
		check_category(out, "min_bounce_response", category.min_bounce_response, 0.0, 1.0);
		check_category(out, "max_bounce_response", category.max_bounce_response, 0.0, 1.0);
		check_category(out, "min_parent_motion_follow", category.min_parent_motion_follow, 0.0, 1.0);
		check_category(out, "max_parent_motion_follow", category.max_parent_motion_follow, 0.0, 1.0);
		check_category(out, "min_max_stretch_response", category.min_max_stretch_response, 0.0, f32::MAX);
		check_category(out, "max_max_stretch_response", category.max_max_stretch_response, 0.0, f32::MAX);
		check_category(out, "min_max_squish_response", category.min_max_squish_response, 0.0, 0.95);
		check_category(out, "max_max_squish_response", category.max_max_squish_response, 0.0, 0.95);
		check_category(out, "min_stretch_motion_response", category.min_stretch_motion_response, 0.0, 1.0);
		check_category(out, "max_stretch_motion_response", category.max_stretch_motion_response, 0.0, 1.0);
	}
}

fn dynamics_response_audit_report(
	plugin_dirs: &[PathBuf],
	path: &Path,
	input_format: Option<&str>,
	wardrobe_set: Option<&str>,
) -> Result<DynamicsResponseAuditReport, String> {
	let (mut doc, import_report, desc) = import_document_for_cli(plugin_dirs, path, input_format)?;
	let active_wardrobe_set = wardrobe_set.filter(|set_id| !set_id.trim().is_empty()).map(str::to_string);
	if let Some(set_id) = active_wardrobe_set.as_deref() {
		apply_unavatar_wardrobe_set(&mut doc, set_id)?;
	}
	let runtime_model = doc.runtime_model();
	let scene = doc.scene.as_ref();
	let runtime_dynamics = runtime_model.dynamics();
	let active_groups = runtime_dynamics
		.dynamics_groups()
		.filter(|group| {
			group.effective_enabled && scene.is_none_or(|scene| runtime_dynamics.source_id_resident_in_scene(scene, group.source_id))
		})
		.collect::<Vec<_>>();
	let group_count = active_groups.len();
	let joint_count = active_groups
		.iter()
		.map(|group| group.chain.bone_node_indices.len().saturating_sub(1))
		.sum();
	let authored = dynamics_response_audit_mode("authored", &doc, DynamicsPhysicsConfig::default())?;
	let soft = dynamics_response_audit_mode("soft_override", &doc, dynamics_response_audit_config(dynamics_soft_audit_params()))?;
	let firm = dynamics_response_audit_mode("firm_override", &doc, dynamics_response_audit_config(dynamics_firm_audit_params()))?;
	let mut missing_response_evidence = Vec::new();
	if group_count > 0 && authored.group_count == 0 {
		missing_response_evidence.push(format!("dynamics groups={group_count} but authored response groups=0"));
	}
	if joint_count > 0 && authored.joint_count == 0 {
		missing_response_evidence.push(format!("dynamics joints={joint_count} but authored response joints=0"));
	}
	for mode in [&authored, &soft, &firm] {
		collect_dynamics_response_bounds_evidence(mode, &mut missing_response_evidence);
	}
	if !(soft.average_rest_response < firm.average_rest_response) {
		missing_response_evidence.push(format!(
			"rest_response override did not separate soft={} firm={}",
			soft.average_rest_response, firm.average_rest_response
		));
	}
	if !(soft.average_shape_preservation < firm.average_shape_preservation) {
		missing_response_evidence.push(format!(
			"shape_preservation override did not separate soft={} firm={}",
			soft.average_shape_preservation, firm.average_shape_preservation
		));
	}
	if !(soft.average_parent_motion_follow < firm.average_parent_motion_follow) {
		missing_response_evidence.push(format!(
			"motion_coupling override did not separate soft={} firm={}",
			soft.average_parent_motion_follow, firm.average_parent_motion_follow
		));
	}
	if !(soft.average_damping_half_life_ms.unwrap_or_default() > firm.average_damping_half_life_ms.unwrap_or(f32::MAX)) {
		missing_response_evidence.push(format!(
			"damping_half_life_ms override did not separate soft={:?} firm={:?}",
			soft.average_damping_half_life_ms, firm.average_damping_half_life_ms
		));
	}
	if !(soft.average_bounce_response > firm.average_bounce_response) {
		missing_response_evidence.push(format!(
			"bounce_scale override did not separate soft={} firm={}",
			soft.average_bounce_response, firm.average_bounce_response
		));
	}
	let has_authored_stretch_range = authored.average_max_stretch_response > 0.0 || authored.average_max_squish_response > 0.0;
	if has_authored_stretch_range && !(soft.average_max_stretch_response < firm.average_max_stretch_response) {
		missing_response_evidence.push(format!(
			"stretch_range_scale override did not separate max_stretch soft={} firm={}",
			soft.average_max_stretch_response, firm.average_max_stretch_response
		));
	}
	if has_authored_stretch_range && !(soft.average_stretch_motion_response < firm.average_stretch_motion_response) {
		missing_response_evidence.push(format!(
			"stretch_motion override did not separate soft={} firm={}",
			soft.average_stretch_motion_response, firm.average_stretch_motion_response
		));
	}
	Ok(DynamicsResponseAuditReport {
		path: path.display().to_string(),
		import_format_id: desc.id.0,
		import_provider_plugin_id: desc.provider_plugin_id,
		active_wardrobe_set,
		import_report,
		group_count,
		joint_count,
		modes: vec![authored, soft, firm],
		missing_response_evidence,
	})
}

fn require_dynamics_response_override_effect(report: &DynamicsResponseAuditReport) -> Result<(), String> {
	if report.missing_response_evidence.is_empty() {
		Ok(())
	} else {
		Err(format!(
			"UNPhysics response override evidence is missing: {}",
			report.missing_response_evidence.join(", ")
		))
	}
}

fn run_dynamics_response_audit(
	plugin_dirs: &[PathBuf],
	path: PathBuf,
	input_format: Option<String>,
	wardrobe_set: Option<String>,
	require_override_effect: bool,
	json: bool,
) -> Result<(), String> {
	let report = dynamics_response_audit_report(plugin_dirs, &path, input_format.as_deref(), wardrobe_set.as_deref())?;
	let required = if require_override_effect {
		require_dynamics_response_override_effect(&report)
	} else {
		Ok(())
	};
	if json {
		write_json_stdout(&report)?;
		return required;
	}
	println!("path: {}", report.path);
	let plug = report
		.import_provider_plugin_id
		.as_ref()
		.map(|p| format!(" ({p})"))
		.unwrap_or_default();
	println!("importer: {}{}", report.import_format_id, plug);
	if let Some(set_id) = &report.active_wardrobe_set {
		println!("active_wardrobe_set: {set_id}");
	}
	println!("dynamics: groups={} joints={}", report.group_count, report.joint_count);
	for mode in &report.modes {
		println!(
			"response[{name}]: groups={} joints={} categories={} rest={} shape={} bounce={} stretch={} squish={} stretchMotion={} motion={} orientation={} xpbd_groups={}",
			mode.group_count,
			mode.joint_count,
			mode.category_count,
			mode.average_rest_response,
			mode.average_shape_preservation,
			mode.average_bounce_response,
			mode.average_max_stretch_response,
			mode.average_max_squish_response,
			mode.average_stretch_motion_response,
			mode.average_parent_motion_follow,
			mode.average_orientation_follow,
			mode.xpbd_group_count,
			name = mode.name
		);
		for category in mode.categories.iter().take(DIAGNOSE_TEXT_LIST_LIMIT) {
			println!(
				"  category[{name}/{}]: groups={} joints={} rest={} shape={} bounce={} stretch={} squish={} stretchMotion={} motion={} orientation={}",
				category.category,
				category.group_count,
				category.joint_count,
				category.average_rest_response,
				category.average_shape_preservation,
				category.average_bounce_response,
				category.average_max_stretch_response,
				category.average_max_squish_response,
				category.average_stretch_motion_response,
				category.average_parent_motion_follow,
				category.average_orientation_follow,
				name = mode.name
			);
		}
	}
	if !report.missing_response_evidence.is_empty() {
		println!("missing response evidence: {}", report.missing_response_evidence.join(", "));
	}
	required
}

fn cli_scene_world_matrices(scene: &UnaSceneSnapshot) -> Vec<Mat4> {
	let mut out = Vec::new();
	fill_cli_scene_world_matrices(scene, &mut out);
	out
}

fn fill_cli_scene_world_matrices(scene: &UnaSceneSnapshot, out: &mut Vec<Mat4>) {
	fn visit(scene: &UnaSceneSnapshot, index: usize, parent_world: Mat4, out: &mut [Mat4]) {
		let Some(node) = scene.nodes.get(index) else {
			return;
		};
		let world = parent_world * Mat4::from_cols_array(&node.transform);
		if let Some(slot) = out.get_mut(index) {
			*slot = world;
		}
		for &child in &node.children {
			visit(scene, child, world, out);
		}
	}

	if out.len() == scene.nodes.len() {
		out.fill(Mat4::IDENTITY);
	} else {
		out.clear();
		out.resize(scene.nodes.len(), Mat4::IDENTITY);
	}
	for &root in scene.resolved_roots().iter() {
		visit(scene, root, Mat4::IDENTITY, out);
	}
}

fn apply_motion_trace_root_rotation(scene: &mut UnaSceneSnapshot, rest_root_transforms: &[(usize, [f32; 16])], angle: f32) {
	let rotation = Mat4::from_rotation_z(angle);
	for &(root, rest_transform) in rest_root_transforms {
		if let Some(node) = scene.nodes.get_mut(root) {
			node.transform = (rotation * Mat4::from_cols_array(&rest_transform)).to_cols_array();
		}
	}
}

fn motion_trace_group_chain_rest_length(world: &[Mat4], bone_node_indices: &[usize]) -> f32 {
	bone_node_indices
		.windows(2)
		.filter_map(|pair| {
			let a = world.get(pair[0])?.transform_point3(Vec3::ZERO);
			let b = world.get(pair[1])?.transform_point3(Vec3::ZERO);
			Some(a.distance(b))
		})
		.filter(|distance| distance.is_finite())
		.sum()
}

fn dynamics_motion_trace_report(
	plugin_dirs: &[PathBuf],
	path: &Path,
	input_format: Option<&str>,
	wardrobe_set: Option<&str>,
	frames: usize,
	recovery_frames: Option<usize>,
	tuning: &str,
) -> Result<DynamicsMotionTraceAuditReport, String> {
	let (mut doc, import_report, desc) = import_document_for_cli(plugin_dirs, path, input_format)?;
	let active_wardrobe_set = wardrobe_set.filter(|set_id| !set_id.trim().is_empty()).map(str::to_string);
	if let Some(set_id) = active_wardrobe_set.as_deref() {
		apply_unavatar_wardrobe_set(&mut doc, set_id)?;
	}
	let scene = doc.scene.as_ref().ok_or_else(|| "document has no scene".to_string())?;
	let settings = doc.dynamics().ok_or_else(|| "document has no dynamics settings".to_string())?;
	let runtime_model = doc.runtime_model();
	let scene_profile_dynamics = runtime_model.scene_profile_dynamics();
	let runtime_dynamics = scene_profile_dynamics
		.as_ref()
		.map(|runtime| runtime.dynamics)
		.unwrap_or_else(|| settings.runtime_dynamics());
	let frames = frames.clamp(2, 240);
	let recovery_frames = recovery_frames.unwrap_or(240).clamp(2, 480);
	let (tuning, physics_config) = dynamics_motion_trace_tuning_config(tuning)?;
	let mut dynamic_scene = scene.clone();
	let mut rest_scene = scene.clone();
	let mut baseline_scene = scene.clone();
	let rest_root_transforms = scene
		.resolved_roots()
		.iter()
		.filter_map(|&root| scene.nodes.get(root).map(|node| (root, node.transform)))
		.collect::<Vec<_>>();
	let all_runtime_groups = runtime_dynamics
		.dynamics_groups()
		.filter(|group| runtime_dynamics.source_id_resident_in_scene(scene, group.source_id))
		.collect::<Vec<_>>();
	let categories = physics_config.categories.clone();
	let mut sim = DynamicsSimulator::new_with_runtime_dynamics(&dynamic_scene, runtime_dynamics, Vec::new(), physics_config.clone())
		.ok_or_else(|| "UNPhysics simulator could not be created".to_string())?;
	let response_by_category = sim
		.response_category_summaries()
		.into_iter()
		.map(|summary| (summary.category.clone(), summary))
		.collect::<BTreeMap<_, _>>();
	let response_by_source_id = sim
		.response_group_summaries()
		.into_iter()
		.map(|summary| (summary.source_id.clone(), summary))
		.collect::<BTreeMap<_, _>>();
	let runtime_groups = all_runtime_groups
		.into_iter()
		.filter(|group| group.effective_enabled && response_by_source_id.contains_key(group.source_id))
		.collect::<Vec<_>>();
	let group_categories = runtime_groups
		.iter()
		.map(|group| classify_dynamics_group_category(scene, *group, &categories))
		.collect::<Vec<_>>();
	let initial_world = cli_scene_world_matrices(scene);
	let group_chain_lengths = runtime_groups
		.iter()
		.map(|group| motion_trace_group_chain_rest_length(&initial_world, group.chain.bone_node_indices))
		.collect::<Vec<_>>();
	let mut rest_world = Vec::new();
	let mut dynamic_world = Vec::new();
	let mut baseline_world = Vec::new();
	let mut initial_dynamic_world = Vec::new();
	let mut final_rest_world = Vec::new();
	let mut previous_dynamic_world = Vec::new();
	let visual_target_context = DynamicsVisualTargetContext::for_scene(scene);
	let group_visual_target_counts = runtime_groups
		.iter()
		.map(|group| visual_target_context.group_counts(group.chain.bone_node_indices))
		.collect::<Vec<_>>();
	let final_angle = std::f32::consts::FRAC_PI_2;
	apply_motion_trace_root_rotation(&mut baseline_scene, &rest_root_transforms, final_angle);
	let mut baseline_sim = DynamicsSimulator::new_with_runtime_dynamics(&baseline_scene, runtime_dynamics, Vec::new(), physics_config)
		.ok_or_else(|| "UNPhysics baseline simulator could not be created".to_string())?;
	#[derive(Default)]
	struct Accum {
		group_count: usize,
		joint_count: usize,
		visual_target_group_count: usize,
		nonvisual_group_count: usize,
		visible_skinned_joint_count: usize,
		visible_mesh_subtree_node_count: usize,
		chain_length_sum: f32,
		sample_count: usize,
		lag_sum: f32,
		max_lag: f32,
		max_lag_chain_ratio: f32,
		final_lag_sum: f32,
		final_lag_chain_ratio_sum: f32,
		initial_settled_lag_sum: f32,
		recovery_final_lag_sum: f32,
		settled_recovery_lag_sum: f32,
		stable_offset_chain_ratio_sum: f32,
		residual_motion_sum: f32,
		residual_motion_chain_ratio_sum: f32,
		half_life_frame_sum: f32,
		half_life_frame_count: usize,
	}
	let mut by_category: BTreeMap<String, Accum> = BTreeMap::new();
	let mut by_group = runtime_groups
		.iter()
		.zip(group_chain_lengths.iter())
		.map(|(group, chain_length)| Accum {
			group_count: 1,
			joint_count: group.chain.bone_node_indices.len().saturating_sub(1),
			chain_length_sum: *chain_length,
			..Default::default()
		})
		.collect::<Vec<_>>();
	for (((group, category), chain_length), &(skinned_joint_count, mesh_subtree_node_count)) in runtime_groups
		.iter()
		.zip(group_categories.iter())
		.zip(group_chain_lengths.iter())
		.zip(group_visual_target_counts.iter())
	{
		let accum = by_category.entry(category.clone()).or_default();
		accum.group_count += 1;
		accum.joint_count += group.chain.bone_node_indices.len().saturating_sub(1);
		if skinned_joint_count > 0 || mesh_subtree_node_count > 0 {
			accum.visual_target_group_count += 1;
		} else {
			accum.nonvisual_group_count += 1;
		}
		accum.visible_skinned_joint_count += skinned_joint_count;
		accum.visible_mesh_subtree_node_count += mesh_subtree_node_count;
		accum.chain_length_sum += *chain_length;
	}
	for frame in 1..=frames {
		let t = frame as f32 / frames as f32;
		let angle = std::f32::consts::FRAC_PI_2 * t;
		apply_motion_trace_root_rotation(&mut rest_scene, &rest_root_transforms, angle);
		apply_motion_trace_root_rotation(&mut dynamic_scene, &rest_root_transforms, angle);
		sim.step_runtime_dynamics(&mut dynamic_scene, runtime_dynamics, 1.0 / 60.0);
		fill_cli_scene_world_matrices(&rest_scene, &mut rest_world);
		fill_cli_scene_world_matrices(&dynamic_scene, &mut dynamic_world);
		for (group_index, (group, category)) in runtime_groups.iter().zip(group_categories.iter()).enumerate() {
			let Some(&tip_node) = group.chain.bone_node_indices.last() else {
				continue;
			};
			let Some(rest_tip) = rest_world.get(tip_node) else {
				continue;
			};
			let Some(dynamic_tip) = dynamic_world.get(tip_node) else {
				continue;
			};
			let lag = dynamic_tip
				.transform_point3(Vec3::ZERO)
				.distance(rest_tip.transform_point3(Vec3::ZERO));
			let lag_chain_ratio = motion_trace_chain_ratio(lag, group_chain_lengths[group_index]);
			let accum = by_category.entry(category.clone()).or_default();
			accum.sample_count += 1;
			accum.lag_sum += lag;
			accum.max_lag = accum.max_lag.max(lag);
			accum.max_lag_chain_ratio = accum.max_lag_chain_ratio.max(lag_chain_ratio);
			if frame == frames {
				accum.final_lag_sum += lag;
				accum.final_lag_chain_ratio_sum += lag_chain_ratio;
			}
			if let Some(accum) = by_group.get_mut(group_index) {
				accum.sample_count += 1;
				accum.lag_sum += lag;
				accum.max_lag = accum.max_lag.max(lag);
				accum.max_lag_chain_ratio = accum.max_lag_chain_ratio.max(lag_chain_ratio);
				if frame == frames {
					accum.final_lag_sum += lag;
					accum.final_lag_chain_ratio_sum += lag_chain_ratio;
				}
			}
		}
	}
	apply_motion_trace_root_rotation(&mut rest_scene, &rest_root_transforms, final_angle);
	apply_motion_trace_root_rotation(&mut dynamic_scene, &rest_root_transforms, final_angle);
	for _ in 0..(frames + recovery_frames) {
		baseline_sim.step_runtime_dynamics(&mut baseline_scene, runtime_dynamics, 1.0 / 60.0);
	}
	fill_cli_scene_world_matrices(&baseline_scene, &mut baseline_world);
	fill_cli_scene_world_matrices(&dynamic_scene, &mut initial_dynamic_world);
	for (group_index, (group, category)) in runtime_groups.iter().zip(group_categories.iter()).enumerate() {
		let Some(&tip_node) = group.chain.bone_node_indices.last() else {
			continue;
		};
		let Some(dynamic_tip) = initial_dynamic_world.get(tip_node) else {
			continue;
		};
		let Some(baseline_tip) = baseline_world.get(tip_node) else {
			continue;
		};
		let initial_settled_lag = dynamic_tip
			.transform_point3(Vec3::ZERO)
			.distance(baseline_tip.transform_point3(Vec3::ZERO));
		let accum = by_category.entry(category.clone()).or_default();
		accum.initial_settled_lag_sum += initial_settled_lag;
		if let Some(accum) = by_group.get_mut(group_index) {
			accum.initial_settled_lag_sum += initial_settled_lag;
		}
	}
	fill_cli_scene_world_matrices(&rest_scene, &mut final_rest_world);
	for frame in 1..=recovery_frames {
		let has_previous_dynamic_world = frame == recovery_frames;
		if has_previous_dynamic_world {
			fill_cli_scene_world_matrices(&dynamic_scene, &mut previous_dynamic_world);
		}
		sim.step_runtime_dynamics(&mut dynamic_scene, runtime_dynamics, 1.0 / 60.0);
		fill_cli_scene_world_matrices(&dynamic_scene, &mut dynamic_world);
		for (group_index, (group, category)) in runtime_groups.iter().zip(group_categories.iter()).enumerate() {
			let Some(&tip_node) = group.chain.bone_node_indices.last() else {
				continue;
			};
			let Some(dynamic_tip) = dynamic_world.get(tip_node) else {
				continue;
			};
			let Some(baseline_tip) = baseline_world.get(tip_node) else {
				continue;
			};
			let dynamic_tip = dynamic_tip.transform_point3(Vec3::ZERO);
			let settled_recovery_lag = dynamic_tip.distance(baseline_tip.transform_point3(Vec3::ZERO));
			if let Some(accum) = by_group.get_mut(group_index) {
				let initial_settled_lag = motion_trace_group_average(accum.initial_settled_lag_sum, accum.group_count);
				if initial_settled_lag > 1.0e-5 && accum.half_life_frame_count == 0 && settled_recovery_lag <= initial_settled_lag * 0.5 {
					accum.half_life_frame_sum += frame as f32;
					accum.half_life_frame_count += 1;
					let category_accum = by_category.entry(category.clone()).or_default();
					category_accum.half_life_frame_sum += frame as f32;
					category_accum.half_life_frame_count += 1;
				}
			}
			if frame == recovery_frames {
				let Some(rest_tip) = final_rest_world.get(tip_node) else {
					continue;
				};
				let Some(previous_dynamic_tip) = has_previous_dynamic_world.then(|| previous_dynamic_world.get(tip_node)).flatten() else {
					continue;
				};
				let recovery_lag = dynamic_tip.distance(rest_tip.transform_point3(Vec3::ZERO));
				let residual_motion = dynamic_tip.distance(previous_dynamic_tip.transform_point3(Vec3::ZERO));
				let stable_offset_chain_ratio = motion_trace_chain_ratio(settled_recovery_lag, group_chain_lengths[group_index]);
				let residual_motion_chain_ratio = motion_trace_chain_ratio(residual_motion, group_chain_lengths[group_index]);
				let accum = by_category.entry(category.clone()).or_default();
				accum.recovery_final_lag_sum += recovery_lag;
				accum.settled_recovery_lag_sum += settled_recovery_lag;
				accum.stable_offset_chain_ratio_sum += stable_offset_chain_ratio;
				accum.residual_motion_sum += residual_motion;
				accum.residual_motion_chain_ratio_sum += residual_motion_chain_ratio;
				if let Some(accum) = by_group.get_mut(group_index) {
					accum.recovery_final_lag_sum += recovery_lag;
					accum.settled_recovery_lag_sum += settled_recovery_lag;
					accum.stable_offset_chain_ratio_sum += stable_offset_chain_ratio;
					accum.residual_motion_sum += residual_motion;
					accum.residual_motion_chain_ratio_sum += residual_motion_chain_ratio;
				}
			}
		}
	}
	fn motion_trace_average_lag(accum: &Accum) -> f32 {
		if accum.sample_count > 0 {
			accum.lag_sum / accum.sample_count as f32
		} else {
			0.0
		}
	}
	fn motion_trace_group_average(value_sum: f32, group_count: usize) -> f32 {
		if group_count > 0 {
			value_sum / group_count as f32
		} else {
			0.0
		}
	}
	fn motion_trace_recovery_ratio(final_lag: f32, recovery_lag: f32) -> f32 {
		if final_lag > f32::EPSILON {
			((final_lag - recovery_lag) / final_lag).clamp(-1.0, 1.0)
		} else {
			0.0
		}
	}
	fn motion_trace_half_life_frames(accum: &Accum) -> Option<f32> {
		(accum.half_life_frame_count > 0).then(|| accum.half_life_frame_sum / accum.half_life_frame_count as f32)
	}
	fn motion_trace_stable_offset_ratio(initial_stable_offset: f32, stable_offset: f32) -> f32 {
		if initial_stable_offset > 1.0e-5 {
			(stable_offset / initial_stable_offset).max(0.0)
		} else {
			0.0
		}
	}
	fn motion_trace_chain_ratio(value: f32, chain_length: f32) -> f32 {
		if chain_length > 1.0e-5 {
			(value / chain_length).max(0.0)
		} else {
			0.0
		}
	}
	fn motion_trace_recovery_state(
		initial_stable_offset: f32,
		stable_offset: f32,
		residual_motion: f32,
		residual_motion_chain_ratio: f32,
	) -> String {
		let ratio = motion_trace_stable_offset_ratio(initial_stable_offset, stable_offset);
		if residual_motion > 1.0e-3 && residual_motion_chain_ratio > 1.0e-2 {
			"moving".to_string()
		} else if stable_offset > 1.0e-3 && ratio > 0.2 {
			"settled_offset".to_string()
		} else {
			"settled".to_string()
		}
	}
	let mut summaries = by_category
		.into_iter()
		.map(|(category, accum)| {
			let response = response_by_category.get(&category);
			let average_lag = motion_trace_average_lag(&accum);
			let final_lag = motion_trace_group_average(accum.final_lag_sum, accum.group_count);
			let recovery_final_lag = motion_trace_group_average(accum.recovery_final_lag_sum, accum.group_count);
			let recovery_ratio = motion_trace_recovery_ratio(final_lag, recovery_final_lag);
			let initial_stable_offset = motion_trace_group_average(accum.initial_settled_lag_sum, accum.group_count);
			let settled_recovery_lag = motion_trace_group_average(accum.settled_recovery_lag_sum, accum.group_count);
			let settled_recovery_ratio = motion_trace_recovery_ratio(final_lag, settled_recovery_lag);
			let residual_motion = motion_trace_group_average(accum.residual_motion_sum, accum.group_count);
			let residual_motion_chain_ratio = motion_trace_group_average(accum.residual_motion_chain_ratio_sum, accum.group_count);
			let stable_offset_ratio = motion_trace_stable_offset_ratio(initial_stable_offset, settled_recovery_lag);
			let average_chain_rest_length = motion_trace_group_average(accum.chain_length_sum, accum.group_count);
			let recovery_state = motion_trace_recovery_state(
				initial_stable_offset,
				settled_recovery_lag,
				residual_motion,
				residual_motion_chain_ratio,
			);
			let recovery_half_life_frames = motion_trace_half_life_frames(&accum);
			DynamicsMotionTraceCategorySummary {
				category,
				group_count: accum.group_count,
				joint_count: accum.joint_count,
				visual_target_group_count: accum.visual_target_group_count,
				nonvisual_group_count: accum.nonvisual_group_count,
				visible_skinned_joint_count: accum.visible_skinned_joint_count,
				visible_mesh_subtree_node_count: accum.visible_mesh_subtree_node_count,
				average_chain_rest_length,
				max_lag: accum.max_lag,
				max_lag_chain_ratio: accum.max_lag_chain_ratio,
				average_lag,
				final_lag,
				final_lag_chain_ratio: motion_trace_group_average(accum.final_lag_chain_ratio_sum, accum.group_count),
				recovery_final_lag,
				recovery_ratio,
				initial_stable_offset,
				settled_recovery_lag,
				stable_offset: settled_recovery_lag,
				stable_offset_chain_ratio: motion_trace_group_average(accum.stable_offset_chain_ratio_sum, accum.group_count),
				stable_offset_ratio,
				recovery_state,
				settled_recovery_ratio,
				residual_motion,
				residual_motion_chain_ratio,
				recovery_half_life_frames,
				average_rest_response: response.map_or(0.0, |summary| summary.average_rest_response),
				average_shape_preservation: response.map_or(0.0, |summary| summary.average_shape_preservation),
				average_bounce_response: response.map_or(0.0, |summary| summary.average_bounce_response),
				average_parent_motion_follow: response.map_or(0.0, |summary| summary.average_parent_motion_follow),
				average_orientation_follow: response.map_or(0.0, |summary| summary.average_orientation_follow),
				average_max_stretch_response: response.map_or(0.0, |summary| summary.average_max_stretch_response),
				average_stretch_motion_response: response.map_or(0.0, |summary| summary.average_stretch_motion_response),
			}
		})
		.collect::<Vec<_>>();
	let mut group_summaries = runtime_groups
		.iter()
		.zip(group_categories.iter())
		.zip(group_visual_target_counts.iter())
		.zip(by_group.iter())
		.map(|(((group, category), &(skinned_joint_count, mesh_subtree_node_count)), accum)| {
			let response = response_by_source_id.get(group.source_id);
			let average_lag = motion_trace_average_lag(accum);
			let final_lag = motion_trace_group_average(accum.final_lag_sum, accum.group_count);
			let recovery_final_lag = motion_trace_group_average(accum.recovery_final_lag_sum, accum.group_count);
			let recovery_ratio = motion_trace_recovery_ratio(final_lag, recovery_final_lag);
			let initial_stable_offset = motion_trace_group_average(accum.initial_settled_lag_sum, accum.group_count);
			let settled_recovery_lag = motion_trace_group_average(accum.settled_recovery_lag_sum, accum.group_count);
			let settled_recovery_ratio = motion_trace_recovery_ratio(final_lag, settled_recovery_lag);
			let residual_motion = motion_trace_group_average(accum.residual_motion_sum, accum.group_count);
			let residual_motion_chain_ratio = motion_trace_group_average(accum.residual_motion_chain_ratio_sum, accum.group_count);
			let stable_offset_ratio = motion_trace_stable_offset_ratio(initial_stable_offset, settled_recovery_lag);
			let chain_rest_length = motion_trace_group_average(accum.chain_length_sum, accum.group_count);
			let recovery_state = motion_trace_recovery_state(
				initial_stable_offset,
				settled_recovery_lag,
				residual_motion,
				residual_motion_chain_ratio,
			);
			let recovery_half_life_frames = motion_trace_half_life_frames(accum);
			DynamicsMotionTraceGroupSummary {
				source_id: group.source_id.to_string(),
				category: category.clone(),
				joint_count: group.chain.bone_node_indices.len().saturating_sub(1),
				visual_target: skinned_joint_count > 0 || mesh_subtree_node_count > 0,
				skinned_joint_count,
				mesh_subtree_node_count,
				interaction_metadata_only: group.interaction.is_some_and(|interaction| {
					(interaction.allow_grabbing.unwrap_or(false) || interaction.allow_posing.unwrap_or(false))
						&& interaction.parameter.is_empty()
				}),
				chain_rest_length,
				max_lag: accum.max_lag,
				max_lag_chain_ratio: accum.max_lag_chain_ratio,
				average_lag,
				final_lag,
				final_lag_chain_ratio: motion_trace_group_average(accum.final_lag_chain_ratio_sum, accum.group_count),
				recovery_final_lag,
				recovery_ratio,
				initial_stable_offset,
				settled_recovery_lag,
				stable_offset: settled_recovery_lag,
				stable_offset_chain_ratio: motion_trace_group_average(accum.stable_offset_chain_ratio_sum, accum.group_count),
				stable_offset_ratio,
				recovery_state,
				settled_recovery_ratio,
				residual_motion,
				residual_motion_chain_ratio,
				recovery_half_life_frames,
				average_rest_response: response.map_or(0.0, |summary| summary.average_rest_response),
				average_shape_preservation: response.map_or(0.0, |summary| summary.average_shape_preservation),
				average_bounce_response: response.map_or(0.0, |summary| summary.average_bounce_response),
				average_parent_motion_follow: response.map_or(0.0, |summary| summary.average_parent_motion_follow),
				average_orientation_follow: response.map_or(0.0, |summary| summary.average_orientation_follow),
				average_max_stretch_response: response.map_or(0.0, |summary| summary.average_max_stretch_response),
				average_stretch_motion_response: response.map_or(0.0, |summary| summary.average_stretch_motion_response),
			}
		})
		.collect::<Vec<_>>();
	sort_motion_trace_category_summaries(&mut summaries);
	sort_motion_trace_group_summaries(&mut group_summaries);
	let group_count = runtime_groups.len();
	let joint_count = runtime_groups
		.iter()
		.map(|group| group.chain.bone_node_indices.len().saturating_sub(1))
		.sum();
	let mut missing_motion_evidence = Vec::new();
	if group_count > 0 && summaries.is_empty() {
		missing_motion_evidence.push(format!("dynamics groups={group_count} but no motion trace categories were sampled"));
	}
	if group_count > 0 && summaries.iter().all(|summary| summary.max_lag <= 0.0) {
		missing_motion_evidence.push("motion trace produced zero lag for all categories".to_string());
	}
	if group_count > 0 && group_summaries.is_empty() {
		missing_motion_evidence.push(format!("dynamics groups={group_count} but no motion trace groups were sampled"));
	}
	let finding_details = collect_motion_trace_finding_details(&summaries, &group_summaries);
	let finding_kind_counts = motion_trace_finding_kind_counts(&finding_details);
	let findings = finding_details.iter().map(|finding| finding.message.clone()).collect();
	collect_motion_trace_numeric_evidence(&summaries, &group_summaries, &mut missing_motion_evidence);
	Ok(DynamicsMotionTraceAuditReport {
		path: path.display().to_string(),
		import_format_id: desc.id.0,
		import_provider_plugin_id: desc.provider_plugin_id,
		active_wardrobe_set,
		import_report,
		frame_count: frames,
		recovery_frame_count: recovery_frames,
		tuning,
		group_count,
		joint_count,
		categories: summaries,
		groups: group_summaries,
		findings,
		finding_details,
		finding_kind_counts,
		missing_motion_evidence,
	})
}

fn sort_motion_trace_category_summaries(summaries: &mut [DynamicsMotionTraceCategorySummary]) {
	summaries.sort_by(|a, b| motion_trace_desc_finite_cmp(a.max_lag, b.max_lag).then_with(|| a.category.cmp(&b.category)));
}

fn sort_motion_trace_group_summaries(summaries: &mut [DynamicsMotionTraceGroupSummary]) {
	summaries.sort_by(|a, b| {
		motion_trace_desc_finite_cmp(a.settled_recovery_lag, b.settled_recovery_lag)
			.then_with(|| motion_trace_desc_finite_cmp(a.max_lag, b.max_lag))
			.then_with(|| a.source_id.cmp(&b.source_id))
	});
}

fn motion_trace_desc_finite_cmp(left: f32, right: f32) -> std::cmp::Ordering {
	match (left.is_finite(), right.is_finite()) {
		(true, true) => right.total_cmp(&left),
		(true, false) => std::cmp::Ordering::Less,
		(false, true) => std::cmp::Ordering::Greater,
		(false, false) => right.total_cmp(&left),
	}
}

fn push_motion_trace_finding(out: &mut Vec<DynamicsMotionTraceFindingDetail>, finding: DynamicsMotionTraceFindingDetail) {
	if out.len() < DIAGNOSE_TEXT_LIST_LIMIT {
		out.push(finding);
	}
}

fn collect_motion_trace_finding_details(
	categories: &[DynamicsMotionTraceCategorySummary],
	groups: &[DynamicsMotionTraceGroupSummary],
) -> Vec<DynamicsMotionTraceFindingDetail> {
	let mut findings = Vec::new();
	for group in groups {
		if !group.visual_target {
			if group.average_max_stretch_response >= 10.0
				|| group.max_lag_chain_ratio >= 4.0
				|| group.recovery_state == "moving"
				|| group.recovery_state == "settled_offset"
			{
				let message = format!(
					"nonvisual_control_motion: source_id={} category={} skinnedJoints={} meshSubtrees={} interactionMetadataOnly={} stretch={:.3} maxChain={:.3} state={}",
					group.source_id,
					group.category,
					group.skinned_joint_count,
					group.mesh_subtree_node_count,
					group.interaction_metadata_only,
					group.average_max_stretch_response,
					group.max_lag_chain_ratio,
					group.recovery_state
				);
				push_motion_trace_finding(
					&mut findings,
					DynamicsMotionTraceFindingDetail {
						kind: "nonvisual_control_motion".to_string(),
						message,
						source_id: Some(group.source_id.clone()),
						category: Some(group.category.clone()),
						visual_target: Some(group.visual_target),
						skinned_joint_count: Some(group.skinned_joint_count),
						mesh_subtree_node_count: Some(group.mesh_subtree_node_count),
						interaction_metadata_only: Some(group.interaction_metadata_only),
						tuning_hint: Some("verify whether this source drives visible meshes before applying physics overrides".to_string()),
						response_override_hint: None,
					},
				);
			}
			continue;
		}
		if group.average_max_stretch_response >= 10.0 && group.average_stretch_motion_response > 0.001 {
			let message = format!(
				"large_stretch: source_id={} category={} stretch={:.3} stretchMotion={:.3} chain={:.3} maxChain={:.3} state={}",
				group.source_id,
				group.category,
				group.average_max_stretch_response,
				group.average_stretch_motion_response,
				group.chain_rest_length,
				group.max_lag_chain_ratio,
				group.recovery_state
			);
			push_motion_trace_finding(
				&mut findings,
				DynamicsMotionTraceFindingDetail {
					kind: "large_stretch".to_string(),
					message,
					source_id: Some(group.source_id.clone()),
					category: Some(group.category.clone()),
					visual_target: Some(group.visual_target),
					skinned_joint_count: Some(group.skinned_joint_count),
					mesh_subtree_node_count: Some(group.mesh_subtree_node_count),
					interaction_metadata_only: Some(group.interaction_metadata_only),
					tuning_hint: Some("compare --tuning stretch-low and --tuning stretch-off before changing response terms".to_string()),
					response_override_hint: Some(DynamicsMotionTraceResponseOverrideHint {
						source_id: group.source_id.clone(),
						rest_response: None,
						damping_half_life_ms: None,
						stretch_range_scale: Some(0.25),
						stretch_motion: Some(0.1),
					}),
				},
			);
		}
		if group.max_lag_chain_ratio >= 4.0 {
			let message = format!(
				"high_chain_lag: source_id={} category={} maxChain={:.3} chain={:.3} maxLag={:.3} state={}",
				group.source_id, group.category, group.max_lag_chain_ratio, group.chain_rest_length, group.max_lag, group.recovery_state
			);
			push_motion_trace_finding(
				&mut findings,
				DynamicsMotionTraceFindingDetail {
					kind: "high_chain_lag".to_string(),
					message,
					source_id: Some(group.source_id.clone()),
					category: Some(group.category.clone()),
					visual_target: Some(group.visual_target),
					skinned_joint_count: Some(group.skinned_joint_count),
					mesh_subtree_node_count: Some(group.mesh_subtree_node_count),
					interaction_metadata_only: Some(group.interaction_metadata_only),
					tuning_hint: Some(
						"compare response terms with stretch-low/rest-high/follow-low before changing solver defaults".to_string(),
					),
					response_override_hint: None,
				},
			);
		}
		if group.recovery_state == "moving" {
			let message = format!(
				"moving_after_recovery: source_id={} category={} residualChain={:.3} stableChain={:.3} halfLife={}",
				group.source_id,
				group.category,
				group.residual_motion_chain_ratio,
				group.stable_offset_chain_ratio,
				group
					.recovery_half_life_frames
					.map(|value| format!("{value:.1}"))
					.unwrap_or_else(|| "none".to_string())
			);
			push_motion_trace_finding(
				&mut findings,
				DynamicsMotionTraceFindingDetail {
					kind: "moving_after_recovery".to_string(),
					message,
					source_id: Some(group.source_id.clone()),
					category: Some(group.category.clone()),
					visual_target: Some(group.visual_target),
					skinned_joint_count: Some(group.skinned_joint_count),
					mesh_subtree_node_count: Some(group.mesh_subtree_node_count),
					interaction_metadata_only: Some(group.interaction_metadata_only),
					tuning_hint: Some(
						"compare --recovery-frames 96/240 and damping-short/rest-high before changing response terms".to_string(),
					),
					response_override_hint: Some(DynamicsMotionTraceResponseOverrideHint {
						source_id: group.source_id.clone(),
						rest_response: Some(0.28),
						damping_half_life_ms: Some(80.0),
						stretch_range_scale: None,
						stretch_motion: None,
					}),
				},
			);
		} else if group.recovery_state == "settled_offset" {
			let message = format!(
				"settled_offset_after_recovery: source_id={} category={} stableRatio={:.3} stableChain={:.3}",
				group.source_id, group.category, group.stable_offset_ratio, group.stable_offset_chain_ratio
			);
			push_motion_trace_finding(
				&mut findings,
				DynamicsMotionTraceFindingDetail {
					kind: "settled_offset_after_recovery".to_string(),
					message,
					source_id: Some(group.source_id.clone()),
					category: Some(group.category.clone()),
					visual_target: Some(group.visual_target),
					skinned_joint_count: Some(group.skinned_joint_count),
					mesh_subtree_node_count: Some(group.mesh_subtree_node_count),
					interaction_metadata_only: Some(group.interaction_metadata_only),
					tuning_hint: Some(
						"compare rest-high and gravity-off to separate recovery weakness from natural gravity settle".to_string(),
					),
					response_override_hint: None,
				},
			);
		}
	}
	for category in categories {
		if category.recovery_state == "moving" || category.recovery_state == "settled_offset" {
			let message = format!(
				"category_recovery_state: category={} state={} groups={} stableRatio={:.3} residualChain={:.3}",
				category.category,
				category.recovery_state,
				category.group_count,
				category.stable_offset_ratio,
				category.residual_motion_chain_ratio
			);
			push_motion_trace_finding(
				&mut findings,
				DynamicsMotionTraceFindingDetail {
					kind: "category_recovery_state".to_string(),
					message,
					source_id: None,
					category: Some(category.category.clone()),
					visual_target: Some(category.visual_target_group_count > 0),
					skinned_joint_count: Some(category.visible_skinned_joint_count),
					mesh_subtree_node_count: Some(category.visible_mesh_subtree_node_count),
					interaction_metadata_only: None,
					tuning_hint: Some("inspect source_id group findings before changing category defaults".to_string()),
					response_override_hint: None,
				},
			);
		}
	}
	findings
}

fn motion_trace_finding_kind_counts(findings: &[DynamicsMotionTraceFindingDetail]) -> BTreeMap<String, usize> {
	let mut out = BTreeMap::new();
	for finding in findings {
		*out.entry(finding.kind.clone()).or_default() += 1;
	}
	out
}

fn push_motion_trace_nonfinite(out: &mut Vec<String>, scope: &str, key: &str, value: f32) {
	if !value.is_finite() && out.len() < 64 {
		out.push(format!("{scope} {key} is not finite: {value}"));
	}
}

fn push_motion_trace_optional_nonfinite(out: &mut Vec<String>, scope: &str, key: &str, value: Option<f32>) {
	if let Some(value) = value {
		push_motion_trace_nonfinite(out, scope, key, value);
	}
}

fn collect_motion_trace_numeric_evidence(
	categories: &[DynamicsMotionTraceCategorySummary],
	groups: &[DynamicsMotionTraceGroupSummary],
	out: &mut Vec<String>,
) {
	for category in categories {
		let scope = format!("motion category {}", category.category);
		push_motion_trace_nonfinite(out, &scope, "average_chain_rest_length", category.average_chain_rest_length);
		push_motion_trace_nonfinite(out, &scope, "max_lag", category.max_lag);
		push_motion_trace_nonfinite(out, &scope, "max_lag_chain_ratio", category.max_lag_chain_ratio);
		push_motion_trace_nonfinite(out, &scope, "average_lag", category.average_lag);
		push_motion_trace_nonfinite(out, &scope, "final_lag", category.final_lag);
		push_motion_trace_nonfinite(out, &scope, "final_lag_chain_ratio", category.final_lag_chain_ratio);
		push_motion_trace_nonfinite(out, &scope, "recovery_final_lag", category.recovery_final_lag);
		push_motion_trace_nonfinite(out, &scope, "recovery_ratio", category.recovery_ratio);
		push_motion_trace_nonfinite(out, &scope, "initial_stable_offset", category.initial_stable_offset);
		push_motion_trace_nonfinite(out, &scope, "settled_recovery_lag", category.settled_recovery_lag);
		push_motion_trace_nonfinite(out, &scope, "stable_offset", category.stable_offset);
		push_motion_trace_nonfinite(out, &scope, "stable_offset_chain_ratio", category.stable_offset_chain_ratio);
		push_motion_trace_nonfinite(out, &scope, "stable_offset_ratio", category.stable_offset_ratio);
		push_motion_trace_nonfinite(out, &scope, "settled_recovery_ratio", category.settled_recovery_ratio);
		push_motion_trace_nonfinite(out, &scope, "residual_motion", category.residual_motion);
		push_motion_trace_nonfinite(out, &scope, "residual_motion_chain_ratio", category.residual_motion_chain_ratio);
		push_motion_trace_optional_nonfinite(out, &scope, "recovery_half_life_frames", category.recovery_half_life_frames);
		push_motion_trace_nonfinite(out, &scope, "average_rest_response", category.average_rest_response);
		push_motion_trace_nonfinite(out, &scope, "average_shape_preservation", category.average_shape_preservation);
		push_motion_trace_nonfinite(out, &scope, "average_bounce_response", category.average_bounce_response);
		push_motion_trace_nonfinite(out, &scope, "average_parent_motion_follow", category.average_parent_motion_follow);
		push_motion_trace_nonfinite(out, &scope, "average_orientation_follow", category.average_orientation_follow);
		push_motion_trace_nonfinite(out, &scope, "average_max_stretch_response", category.average_max_stretch_response);
		push_motion_trace_nonfinite(
			out,
			&scope,
			"average_stretch_motion_response",
			category.average_stretch_motion_response,
		);
	}
	for group in groups {
		let scope = format!("motion group {}", group.source_id);
		push_motion_trace_nonfinite(out, &scope, "chain_rest_length", group.chain_rest_length);
		push_motion_trace_nonfinite(out, &scope, "max_lag", group.max_lag);
		push_motion_trace_nonfinite(out, &scope, "max_lag_chain_ratio", group.max_lag_chain_ratio);
		push_motion_trace_nonfinite(out, &scope, "average_lag", group.average_lag);
		push_motion_trace_nonfinite(out, &scope, "final_lag", group.final_lag);
		push_motion_trace_nonfinite(out, &scope, "final_lag_chain_ratio", group.final_lag_chain_ratio);
		push_motion_trace_nonfinite(out, &scope, "recovery_final_lag", group.recovery_final_lag);
		push_motion_trace_nonfinite(out, &scope, "recovery_ratio", group.recovery_ratio);
		push_motion_trace_nonfinite(out, &scope, "initial_stable_offset", group.initial_stable_offset);
		push_motion_trace_nonfinite(out, &scope, "settled_recovery_lag", group.settled_recovery_lag);
		push_motion_trace_nonfinite(out, &scope, "stable_offset", group.stable_offset);
		push_motion_trace_nonfinite(out, &scope, "stable_offset_chain_ratio", group.stable_offset_chain_ratio);
		push_motion_trace_nonfinite(out, &scope, "stable_offset_ratio", group.stable_offset_ratio);
		push_motion_trace_nonfinite(out, &scope, "settled_recovery_ratio", group.settled_recovery_ratio);
		push_motion_trace_nonfinite(out, &scope, "residual_motion", group.residual_motion);
		push_motion_trace_nonfinite(out, &scope, "residual_motion_chain_ratio", group.residual_motion_chain_ratio);
		push_motion_trace_optional_nonfinite(out, &scope, "recovery_half_life_frames", group.recovery_half_life_frames);
		push_motion_trace_nonfinite(out, &scope, "average_rest_response", group.average_rest_response);
		push_motion_trace_nonfinite(out, &scope, "average_shape_preservation", group.average_shape_preservation);
		push_motion_trace_nonfinite(out, &scope, "average_bounce_response", group.average_bounce_response);
		push_motion_trace_nonfinite(out, &scope, "average_parent_motion_follow", group.average_parent_motion_follow);
		push_motion_trace_nonfinite(out, &scope, "average_orientation_follow", group.average_orientation_follow);
		push_motion_trace_nonfinite(out, &scope, "average_max_stretch_response", group.average_max_stretch_response);
		push_motion_trace_nonfinite(
			out,
			&scope,
			"average_stretch_motion_response",
			group.average_stretch_motion_response,
		);
	}
}

fn require_dynamics_motion_trace_evidence(report: &DynamicsMotionTraceAuditReport) -> Result<(), String> {
	if report.missing_motion_evidence.is_empty() {
		Ok(())
	} else {
		Err(format!(
			"UNPhysics motion trace evidence is missing: {}",
			report.missing_motion_evidence.join(", ")
		))
	}
}

fn push_bounded_unique_string(out: &mut Vec<String>, value: String, limit: usize) {
	if out.len() < limit && !out.iter().any(|item| item == &value) {
		out.push(value);
	}
}

fn dynamics_vertex_probe_motion_pose_frame(
	left_upper_arm_z_deg: Option<f32>,
	right_upper_arm_z_deg: Option<f32>,
) -> un_motion_frame::UNMotionFrame {
	fn quatf(q: Quat) -> un_motion_frame::Quatf {
		un_motion_frame::Quatf {
			x: q.x,
			y: q.y,
			z: q.z,
			w: q.w,
		}
	}
	fn bone_sample(bone: un_motion_frame::HumanoidBone, z_deg: f32) -> un_motion_frame::BoneSample {
		un_motion_frame::BoneSample {
			bone,
			transform: un_motion_frame::TransformSample {
				translation: None,
				rotation: Some(quatf(Quat::from_rotation_z(z_deg.to_radians()))),
				scale: None,
				linear_velocity: None,
				angular_velocity: None,
			},
			confidence: 1.0,
			source_index: Some(0),
			state: un_motion_frame::SampleState::Valid,
		}
	}
	let mut bones = Vec::new();
	if let Some(z_deg) = left_upper_arm_z_deg.filter(|value| value.is_finite()) {
		bones.push(bone_sample(un_motion_frame::HumanoidBone::LeftUpperArm, z_deg));
	}
	if let Some(z_deg) = right_upper_arm_z_deg.filter(|value| value.is_finite()) {
		bones.push(bone_sample(un_motion_frame::HumanoidBone::RightUpperArm, z_deg));
	}
	let mut frame = un_motion_frame::UNMotionFrame::new(0);
	frame.header.coordinate_space = un_motion_frame::CoordinateSpace::UNMotion;
	frame.body = Some(un_motion_frame::BodyMotion {
		tracking_state: un_motion_frame::TrackingState::Valid,
		confidence: 1.0,
		humanoid: Some(un_motion_frame::HumanoidPose { root: None, bones }),
	});
	frame
}

fn dynamics_vertex_probe_unmotion_frame_from_json_slice(data: &[u8], path: &Path) -> Result<un_motion_frame::UNMotionFrame, String> {
	if let Ok(frame) = serde_json::from_slice::<un_motion_frame::UNMotionFrame>(data) {
		return Ok(frame);
	}
	let value: serde_json::Value =
		serde_json::from_slice(data).map_err(|err| format!("failed to parse UNMotionFrame JSON `{}`: {err}", path.display()))?;
	let Some(body) = value.get("body") else {
		return Err(format!("UNMotionFrame JSON `{}` has no body", path.display()));
	};
	if !body.get("present").and_then(serde_json::Value::as_bool).unwrap_or(true) {
		return Err(format!("UNMotionFrame JSON `{}` body is not present", path.display()));
	}
	let bones = body
		.get("bones")
		.and_then(serde_json::Value::as_array)
		.ok_or_else(|| format!("UNMotionFrame JSON `{}` body has no bones array", path.display()))?;
	let mut samples = Vec::new();
	for bone in bones {
		let bone_name = bone
			.get("bone")
			.and_then(serde_json::Value::as_str)
			.ok_or_else(|| format!("UNMotionFrame JSON `{}` body bone has no name", path.display()))?;
		let humanoid_bone = serde_json::from_value::<un_motion_frame::HumanoidBone>(serde_json::Value::String(bone_name.to_string()))
			.map_err(|err| {
				format!(
					"UNMotionFrame JSON `{}` has unsupported humanoid bone `{bone_name}`: {err}",
					path.display()
				)
			})?;
		let rotation = bone
			.get("rotation")
			.ok_or_else(|| format!("UNMotionFrame JSON `{}` body bone `{bone_name}` has no rotation", path.display()))?;
		let quat = serde_json::from_value::<un_motion_frame::Quatf>(rotation.clone()).map_err(|err| {
			format!(
				"UNMotionFrame JSON `{}` body bone `{bone_name}` rotation is invalid: {err}",
				path.display()
			)
		})?;
		samples.push(un_motion_frame::BoneSample {
			bone: humanoid_bone,
			transform: un_motion_frame::TransformSample {
				translation: None,
				rotation: Some(quat),
				scale: None,
				linear_velocity: None,
				angular_velocity: None,
			},
			confidence: 1.0,
			source_index: Some(0),
			state: un_motion_frame::SampleState::Valid,
		});
	}
	let mut frame = un_motion_frame::UNMotionFrame::new(value.get("outputSequence").and_then(serde_json::Value::as_u64).unwrap_or(0));
	frame.header.coordinate_space = un_motion_frame::CoordinateSpace::UNMotion;
	frame.body = Some(un_motion_frame::BodyMotion {
		tracking_state: un_motion_frame::TrackingState::Valid,
		confidence: value
			.get("sourceConfidence")
			.and_then(serde_json::Value::as_f64)
			.unwrap_or(1.0)
			.clamp(0.0, 1.0) as f32,
		humanoid: Some(un_motion_frame::HumanoidPose {
			root: None,
			bones: samples,
		}),
	});
	Ok(frame)
}

fn dynamics_vertex_probe_interaction_values(
	doc: &UnaDocument,
	rest_nodes: &[un_avatar_core::UnaSceneNode],
	scene: &UnaSceneSnapshot,
	node_paths: &[Option<String>],
) -> Vec<DynamicsVertexProbeInteractionValue> {
	let runtime = doc.runtime_model();
	let dynamics = runtime.dynamics();
	let world = cli_scene_world_matrices(scene);
	let center_peak_angle_parameters = dynamics_vertex_probe_center_peak_angle_parameters(doc);
	let mut out = Vec::new();
	for group in dynamics.dynamics_groups() {
		if !group.effective_enabled || !dynamics.source_id_resident_in_scene(scene, group.source_id) {
			continue;
		}
		let Some(interaction) = group.interaction else {
			continue;
		};
		if interaction.parameter.is_empty() {
			continue;
		}
		let shape_angle = dynamics_vertex_probe_group_shape_angle(rest_nodes, &scene.nodes, group).unwrap_or(0.0);
		let source_limit_rotation = dynamics_vertex_probe_source_limit_rotation(doc, group.source_id);
		let gravity_angle =
			dynamics_vertex_probe_group_gravity_sensor_angle(rest_nodes, &world, group, node_paths, source_limit_rotation).unwrap_or(0.0);
		let angle = shape_angle.max(gravity_angle);
		let max_angle = dynamics_vertex_probe_interaction_angle_normalizer(group.limit);
		let angle_norm = (angle.to_degrees() / max_angle).clamp(0.0, 1.0);
		let angle_parameter = format!("{}_Angle", interaction.parameter);
		let center_peak_scaled = center_peak_angle_parameters.binary_search(&angle_parameter).is_ok();
		let angle_value = if center_peak_scaled {
			(angle_norm * 0.5).clamp(0.0, 1.0)
		} else {
			angle_norm
		};
		let chain = dynamics_vertex_probe_interaction_chain(group, node_paths)
			.iter()
			.filter_map(|node| node_paths.get(*node).and_then(|path| path.clone()))
			.collect::<Vec<_>>();
		out.push(DynamicsVertexProbeInteractionValue {
			parameter: interaction.parameter.clone(),
			angle_parameter,
			source_id: group.source_id.to_string(),
			angle_value,
			angle_norm,
			angle_deg: angle.to_degrees(),
			shape_angle_deg: shape_angle.to_degrees(),
			gravity_angle_deg: gravity_angle.to_degrees(),
			dominant: if gravity_angle > shape_angle { "gravity" } else { "shape" }.to_string(),
			max_angle_deg: max_angle,
			center_peak_scaled,
			chain,
		});
	}
	out.sort_by(|a, b| a.parameter.cmp(&b.parameter).then_with(|| a.source_id.cmp(&b.source_id)));
	out
}

fn dynamics_vertex_probe_animator_morph_overrides(
	doc: &UnaDocument,
	interaction_values: &[DynamicsVertexProbeInteractionValue],
	node_path: &str,
) -> Vec<DynamicsVertexProbeAnimatorMorphOverride> {
	let mut parameter_values = doc.runtime_model().runtime_parameter_values().clone();
	for value in interaction_values {
		parameter_values.insert(value.angle_parameter.clone(), value.angle_value);
		parameter_values.insert(format!("{}_IsGrabbed", value.parameter), 0.0);
		parameter_values.insert(format!("{}_IsPosed", value.parameter), 0.0);
		parameter_values.insert(format!("{}_Stretch", value.parameter), 0.0);
		parameter_values.insert(format!("{}_Squish", value.parameter), 0.0);
	}
	let mut overrides = BTreeMap::new();
	let Some(animator) = doc.unavatar.as_ref().and_then(|unavatar| unavatar.source.get("animator")) else {
		return Vec::new();
	};
	let Some(controllers) = animator.get("controllers").and_then(serde_json::Value::as_array) else {
		return Vec::new();
	};
	for controller in controllers {
		if controller.get("source").and_then(serde_json::Value::as_str) != Some("modularAvatarMergeAnimator") {
			continue;
		}
		let motion_base_path = controller
			.get("motionBasePath")
			.or_else(|| controller.get("motion_base_path"))
			.and_then(serde_json::Value::as_str)
			.unwrap_or("");
		let defaults = dynamics_vertex_probe_animator_parameter_defaults(controller);
		let Some(layers) = controller.get("layers").and_then(serde_json::Value::as_array) else {
			continue;
		};
		for (layer_index, layer) in layers.iter().enumerate() {
			let layer_weight = if layer_index == 0 {
				1.0
			} else {
				layer.get("defaultWeight").and_then(serde_json::Value::as_f64).unwrap_or(1.0) as f32
			};
			let Some(states) = layer.get("states").and_then(serde_json::Value::as_array) else {
				continue;
			};
			if layer_weight <= 0.0001 || states.len() != 1 {
				continue;
			}
			if let Some(motion) = states[0].get("motion") {
				dynamics_vertex_probe_accumulate_animator_motion_overrides(
					motion,
					motion_base_path,
					&parameter_values,
					&defaults,
					layer_weight,
					&mut overrides,
				);
			}
		}
	}
	let node_suffix = node_path.split_once('/').map(|(_, suffix)| suffix).unwrap_or(node_path);
	let node_leaf = node_path.rsplit('/').next().unwrap_or(node_path);
	let mut out = overrides
		.into_iter()
		.filter(|(key, value)| {
			*value > 0.0001
				&& (key.contains("ArmPit") || key.starts_with(node_path) || key.starts_with(node_suffix) || key.contains(node_leaf))
		})
		.map(|(key, value)| {
			let (target_path, morph_name) = key
				.split_once('\0')
				.map(|(path, morph)| (Some(path.to_string()), morph.to_string()))
				.unwrap_or((None, key.clone()));
			DynamicsVertexProbeAnimatorMorphOverride {
				key,
				target_path,
				morph_name,
				value,
			}
		})
		.collect::<Vec<_>>();
	out.sort_by(|a, b| a.key.cmp(&b.key));
	out
}

fn dynamics_vertex_probe_apply_morph_overrides_to_primitive(
	primitive: &mut un_avatar_core::UnaMeshBuffers,
	overrides: &[DynamicsVertexProbeAnimatorMorphOverride],
	node_path: &str,
) {
	let node_suffix = node_path.split_once('/').map(|(_, suffix)| suffix).unwrap_or(node_path);
	let mut weights = BTreeMap::<String, f32>::new();
	for override_value in overrides {
		let path_matches = override_value.target_path.as_deref().is_none_or(|target_path| {
			target_path == node_path || target_path == node_suffix || node_path.ends_with(&format!("/{target_path}"))
		});
		if path_matches {
			weights.insert(override_value.morph_name.clone(), override_value.value);
		}
	}
	for (target_index, target) in primitive.morph_targets.iter().enumerate() {
		let Some(name) = primitive.morph_target_names.get(target_index) else {
			continue;
		};
		let Some(weight) = weights.get(name).copied().filter(|weight| weight.abs() > 0.0001) else {
			continue;
		};
		for (position, delta) in primitive.positions.iter_mut().zip(target.position_deltas.iter()) {
			position[0] += delta[0] * weight;
			position[1] += delta[1] * weight;
			position[2] += delta[2] * weight;
		}
	}
}

fn dynamics_vertex_probe_animator_parameter_defaults(value: &serde_json::Value) -> BTreeMap<String, f32> {
	let mut out = BTreeMap::new();
	let Some(parameters) = value.get("parameters").and_then(serde_json::Value::as_array) else {
		return out;
	};
	for parameter in parameters {
		let Some(name) = parameter
			.get("name")
			.and_then(serde_json::Value::as_str)
			.filter(|name| !name.is_empty())
		else {
			continue;
		};
		let value = parameter
			.get("defaultFloat")
			.or_else(|| parameter.get("default_float"))
			.and_then(serde_json::Value::as_f64)
			.map(|value| value as f32)
			.unwrap_or(0.0);
		out.insert(name.to_string(), value);
	}
	out
}

fn dynamics_vertex_probe_accumulate_animator_motion_overrides(
	motion: &serde_json::Value,
	motion_base_path: &str,
	parameter_values: &BTreeMap<String, f32>,
	parameter_defaults: &BTreeMap<String, f32>,
	weight: f32,
	out: &mut BTreeMap<String, f32>,
) {
	if weight <= 0.0001 {
		return;
	}
	match motion.get("motionType").and_then(serde_json::Value::as_str) {
		Some("AnimationClip") => {
			let Some(bindings) = motion.get("curveBindings").and_then(serde_json::Value::as_array) else {
				return;
			};
			for binding in bindings {
				let Some(property) = binding.get("propertyName").and_then(serde_json::Value::as_str) else {
					continue;
				};
				let Some(name) = property.strip_prefix("blendShape.").map(str::trim).filter(|name| !name.is_empty()) else {
					continue;
				};
				let Some(raw_value) = binding
					.get("constantValue")
					.or_else(|| binding.get("constant_value"))
					.or_else(|| binding.get("lastValue"))
					.or_else(|| binding.get("last_value"))
					.or_else(|| binding.get("firstValue"))
					.or_else(|| binding.get("first_value"))
					.and_then(serde_json::Value::as_f64)
					.map(|value| value as f32)
				else {
					continue;
				};
				let binding_path = binding.get("path").and_then(serde_json::Value::as_str).unwrap_or("");
				let target_path = dynamics_vertex_probe_animator_resolve_binding_path(motion_base_path, binding_path);
				let key = if target_path.is_empty() {
					name.to_string()
				} else {
					format!("{target_path}\0{name}")
				};
				let value = if raw_value > 1.0 { raw_value / 100.0 } else { raw_value };
				let entry = out.entry(key).or_insert(0.0);
				*entry = (*entry + value * weight).clamp(0.0, 1.0);
			}
		}
		Some("BlendTree") => {
			let blend_type = motion.get("blendType").and_then(serde_json::Value::as_str).unwrap_or("");
			if blend_type != "Simple1D" && blend_type != "1D" {
				return;
			}
			let parameter = motion.get("blendParameter").and_then(serde_json::Value::as_str).unwrap_or("");
			let Some(children) = motion.get("children").and_then(serde_json::Value::as_array) else {
				return;
			};
			let value = parameter_values
				.get(parameter)
				.or_else(|| parameter_defaults.get(parameter))
				.copied()
				.unwrap_or(0.0);
			let thresholds = dynamics_vertex_probe_simple_1d_thresholds(children);
			for (child_index, child) in children.iter().enumerate() {
				let child_weight = dynamics_vertex_probe_simple_1d_child_weight(&thresholds, child_index, value);
				if child_weight > 0.0001 {
					dynamics_vertex_probe_accumulate_animator_motion_overrides(
						child,
						motion_base_path,
						parameter_values,
						parameter_defaults,
						weight * child_weight,
						out,
					);
				}
			}
		}
		_ => {}
	}
}

fn dynamics_vertex_probe_animator_resolve_binding_path(motion_base_path: &str, binding_path: &str) -> String {
	let binding_path = binding_path.trim_matches('/');
	if binding_path.is_empty() {
		return motion_base_path.trim_matches('/').to_string();
	}
	let motion_base_path = motion_base_path.trim_matches('/');
	if motion_base_path.is_empty() || binding_path.starts_with(motion_base_path) {
		binding_path.to_string()
	} else {
		format!("{motion_base_path}/{binding_path}")
	}
}

fn dynamics_vertex_probe_simple_1d_thresholds(children: &[serde_json::Value]) -> Vec<(usize, f32)> {
	let mut out = children
		.iter()
		.enumerate()
		.map(|(index, child)| {
			(
				index,
				child
					.get("threshold")
					.and_then(serde_json::Value::as_f64)
					.map(|value| value as f32)
					.unwrap_or(0.0),
			)
		})
		.collect::<Vec<_>>();
	out.sort_by(|left, right| left.1.partial_cmp(&right.1).unwrap_or(std::cmp::Ordering::Equal));
	out
}

fn dynamics_vertex_probe_simple_1d_child_weight(sorted: &[(usize, f32)], index: usize, value: f32) -> f32 {
	let Some(rank) = sorted.iter().position(|(child_index, _)| *child_index == index) else {
		return 0.0;
	};
	let current = sorted[rank].1;
	if sorted.len() == 1 {
		return 1.0;
	}
	if rank == 0 {
		let next = sorted[1].1;
		if value <= current {
			return 1.0;
		}
		return if next > current {
			((next - value) / (next - current)).clamp(0.0, 1.0)
		} else {
			0.0
		};
	}
	if rank + 1 == sorted.len() {
		let prev = sorted[rank - 1].1;
		if value >= current {
			return 1.0;
		}
		return if current > prev {
			((value - prev) / (current - prev)).clamp(0.0, 1.0)
		} else {
			0.0
		};
	}
	let prev = sorted[rank - 1].1;
	let next = sorted[rank + 1].1;
	if value <= current {
		if current > prev {
			((value - prev) / (current - prev)).clamp(0.0, 1.0)
		} else {
			0.0
		}
	} else if next > current {
		((next - value) / (next - current)).clamp(0.0, 1.0)
	} else {
		0.0
	}
}

fn dynamics_vertex_probe_center_peak_angle_parameters(doc: &UnaDocument) -> Vec<String> {
	let mut out = Vec::new();
	let Some(animator) = doc.unavatar.as_ref().and_then(|unavatar| unavatar.source.get("animator")) else {
		return out;
	};
	dynamics_vertex_probe_collect_center_peak_angle_parameters(animator, &mut out);
	out.sort_unstable();
	out.dedup();
	out
}

fn dynamics_vertex_probe_collect_center_peak_angle_parameters(value: &serde_json::Value, out: &mut Vec<String>) {
	if value.get("motionType").and_then(serde_json::Value::as_str) == Some("BlendTree") {
		let blend_type = value.get("blendType").and_then(serde_json::Value::as_str).unwrap_or("");
		if (blend_type == "Simple1D" || blend_type == "1D")
			&& value
				.get("blendParameter")
				.and_then(serde_json::Value::as_str)
				.is_some_and(|parameter| parameter.ends_with("_Angle") && dynamics_vertex_probe_blend_tree_has_center_peak(value))
		{
			if let Some(parameter) = value.get("blendParameter").and_then(serde_json::Value::as_str) {
				out.push(parameter.to_string());
			}
		}
	}
	for key in ["children", "controllers", "layers", "states"] {
		if let Some(values) = value.get(key).and_then(serde_json::Value::as_array) {
			for child in values {
				dynamics_vertex_probe_collect_center_peak_angle_parameters(child, out);
			}
		}
	}
	if let Some(motion) = value.get("motion") {
		dynamics_vertex_probe_collect_center_peak_angle_parameters(motion, out);
	}
}

fn dynamics_vertex_probe_blend_tree_has_center_peak(value: &serde_json::Value) -> bool {
	let Some(children) = value.get("children").and_then(serde_json::Value::as_array) else {
		return false;
	};
	let mut has_low = false;
	let mut has_center = false;
	let mut has_high = false;
	for child in children {
		let Some(threshold) = child.get("threshold").and_then(serde_json::Value::as_f64) else {
			continue;
		};
		has_low |= (threshold - 0.0).abs() <= 0.001;
		has_center |= (threshold - 0.5).abs() <= 0.001;
		has_high |= (threshold - 1.0).abs() <= 0.001;
	}
	has_low && has_center && has_high
}

fn dynamics_vertex_probe_interaction_angle_normalizer(limit: Option<&un_avatar_core::UnaDynamicsLimit>) -> f32 {
	let Some(limit) = limit else {
		return 90.0;
	};
	let x = limit.max_angle_x.max(0.0);
	let z = limit.max_angle_z.max(0.0);
	if limit.limit_type.to_ascii_lowercase().contains("hinge") {
		x.max(1.0)
	} else {
		x.max(z).max(1.0)
	}
}

fn dynamics_vertex_probe_interaction_chain<'a>(group: un_avatar_core::UnaDynamicsGroup<'a>, node_paths: &[Option<String>]) -> &'a [usize] {
	let chain = group.chain.bone_node_indices;
	let start = group.chain.interaction_start_index.min(chain.len());
	if start == 0 && dynamics_vertex_probe_legacy_interaction_anchor(group, node_paths) {
		return &chain[1..];
	}
	&chain[start..]
}

fn dynamics_vertex_probe_legacy_interaction_anchor(group: un_avatar_core::UnaDynamicsGroup<'_>, node_paths: &[Option<String>]) -> bool {
	if group.interaction.is_none() || group.chain.bone_node_indices.len() < 3 {
		return false;
	}
	let Some(source_path) = group
		.source_id
		.split_once(':')
		.map(|(_, path)| path)
		.filter(|path| !path.is_empty())
	else {
		return false;
	};
	let Some(authored_root_path) = group
		.chain
		.bone_node_indices
		.get(1)
		.and_then(|node| node_paths.get(*node))
		.and_then(|path| path.as_deref())
	else {
		return false;
	};
	authored_root_path == source_path || authored_root_path.ends_with(&format!("/{source_path}"))
}

fn dynamics_vertex_probe_group_shape_angle(
	rest_nodes: &[un_avatar_core::UnaSceneNode],
	nodes: &[un_avatar_core::UnaSceneNode],
	group: un_avatar_core::UnaDynamicsGroup<'_>,
) -> Option<f32> {
	let chain = group.chain.bone_node_indices;
	if chain.len() < 2 {
		return Some(0.0);
	}
	let mut max_angle = 0.0f32;
	let mut measured = false;
	for segment in chain.windows(2) {
		let parent = segment[0];
		let child = segment[1];
		let rest_parent = rest_nodes.get(parent)?;
		let rest_child = rest_nodes.get(child)?;
		let current_parent = nodes.get(parent)?;
		let current_child = nodes.get(child)?;
		let rest_axis = (Mat4::from_cols_array(&rest_parent.transform).inverse() * Mat4::from_cols_array(&rest_child.transform))
			.transform_point3(Vec3::ZERO);
		let current_axis = (Mat4::from_cols_array(&current_parent.transform).inverse() * Mat4::from_cols_array(&current_child.transform))
			.transform_point3(Vec3::ZERO);
		let Some(rest_axis) = rest_axis.try_normalize() else {
			continue;
		};
		let Some(current_axis) = current_axis.try_normalize() else {
			continue;
		};
		max_angle = max_angle.max(rest_axis.angle_between(current_axis));
		measured = true;
	}
	measured.then_some(max_angle)
}

fn dynamics_vertex_probe_group_gravity_sensor_angle(
	rest_nodes: &[un_avatar_core::UnaSceneNode],
	world: &[Mat4],
	group: un_avatar_core::UnaDynamicsGroup<'_>,
	node_paths: &[Option<String>],
	source_limit_rotation: Option<[f32; 3]>,
) -> Option<f32> {
	if group.parameters.gravity_power.abs() <= f32::EPSILON {
		return Some(0.0);
	}
	let gravity_dir = Vec3::from_array(group.parameters.gravity_dir)
		.try_normalize()
		.unwrap_or(Vec3::NEG_Y);
	let chain = dynamics_vertex_probe_interaction_chain(group, node_paths);
	if chain.len() < 2 {
		return Some(0.0);
	}
	let mut max_angle = 0.0f32;
	let mut measured = false;
	for segment in chain.windows(2) {
		let parent = segment[0];
		let child = segment[1];
		let Some(parent_world) = world.get(parent).copied() else {
			continue;
		};
		let Some(rest_child) = rest_nodes.get(child) else {
			continue;
		};
		let (_, parent_rot, _) = parent_world.to_scale_rotation_translation();
		let (_, _, rest_child_translation) = Mat4::from_cols_array(&rest_child.transform).to_scale_rotation_translation();
		let rest_child_translation = if let Some(rotation) = source_limit_rotation {
			Quat::from_euler(
				EulerRot::XYZ,
				rotation[0].to_radians(),
				rotation[1].to_radians(),
				rotation[2].to_radians(),
			) * rest_child_translation
		} else {
			rest_child_translation
		};
		let Some(axis) = (parent_rot.normalize() * rest_child_translation).try_normalize() else {
			continue;
		};
		let gravity_amount = group.parameters.gravity_power.abs().clamp(0.0, 1.0);
		let gravity_target = axis.lerp(gravity_dir * group.parameters.gravity_power.signum(), gravity_amount);
		let Some(gravity_axis) = gravity_target.try_normalize() else {
			continue;
		};
		max_angle = max_angle.max(axis.angle_between(gravity_axis));
		measured = true;
	}
	measured.then_some(max_angle)
}

fn dynamics_vertex_probe_source_limit_rotation(doc: &UnaDocument, source_id: &str) -> Option<[f32; 3]> {
	let source_path = source_id.split_once(':').map(|(_, path)| path).unwrap_or(source_id);
	let entries = doc.unavatar.as_ref()?.source.get("dynamics")?.get("entries")?.as_array()?;
	for entry in entries {
		let id = entry.get("id").and_then(serde_json::Value::as_str).unwrap_or("");
		if id != source_id && id.split_once(':').map(|(_, path)| path).unwrap_or(id) != source_path {
			continue;
		}
		let params = entry.get("sourceParams").or_else(|| entry.get("source_params"))?;
		let values = params.get("limitRotation").or_else(|| params.get("limit_rotation"))?.as_array()?;
		if values.len() < 3 {
			return None;
		}
		return Some([
			values[0].as_f64().unwrap_or(0.0) as f32,
			values[1].as_f64().unwrap_or(0.0) as f32,
			values[2].as_f64().unwrap_or(0.0) as f32,
		]);
	}
	None
}

fn run_dynamics_vertex_probe(
	plugin_dirs: &[PathBuf],
	path: PathBuf,
	input_format: Option<String>,
	wardrobe_set: Option<String>,
	node_contains: &str,
	settle_frames: usize,
	apply_mesh_cloth_assist: bool,
	ignore_authored_colliders: bool,
	ignore_node_constraints: bool,
	pose_left_upper_arm_z_deg: Option<f32>,
	pose_right_upper_arm_z_deg: Option<f32>,
	unmotion_frame_json: Option<PathBuf>,
	tuning: &str,
	json: bool,
) -> Result<(), String> {
	let report = dynamics_vertex_probe_report(
		plugin_dirs,
		&path,
		input_format.as_deref(),
		wardrobe_set.as_deref(),
		node_contains,
		settle_frames,
		apply_mesh_cloth_assist,
		ignore_authored_colliders,
		ignore_node_constraints,
		pose_left_upper_arm_z_deg,
		pose_right_upper_arm_z_deg,
		unmotion_frame_json.as_deref(),
		tuning,
	)?;
	if json {
		write_json_stdout(&report)?;
	} else {
		println!(
			"vertex_probe: node={} mesh={} skin={:?} settle_frames={}{} tuning={} mesh_cloth_assist={} changed_vertices={} ignore_authored_colliders={} runtime_colliders={} collision_projections={} probe_collision_projections={} probe_dynamic_sources={} projection_collider_paths={}",
			report.node_path,
			report.mesh_index,
			report.skin_index,
			report.settle_frames,
			report.pose_left_upper_arm_z_deg
				.map(|value| format!(" left_upper_arm_z={value:.1}deg"))
				.unwrap_or_default(),
			report.tuning,
			report.mesh_cloth_assist_applied,
			report.mesh_cloth_assist_changed_vertices,
			report.authored_colliders_ignored,
			report.runtime_collider_count,
			report.solve_collision_projection_count,
			report.probe_collision_projection_count,
			report.probe_dynamic_source_weight_sums.len(),
			report.solve_collision_projection_collider_path_counts.len()
		);
		for region in &report.regions {
			println!(
				"region {}: vertices={} avg_disp={:.5} max_disp={:.5} avg_delta=({:.5},{:.5},{:.5})",
				region.name,
				region.vertex_count,
				region.average_displacement,
				region.max_displacement,
				region.average_delta[0],
				region.average_delta[1],
				region.average_delta[2]
			);
			let joints = region
				.dominant_joints
				.iter()
				.take(8)
				.map(|entry| format!("{}={}", entry.joint, entry.count))
				.collect::<Vec<_>>()
				.join(", ");
			println!("  dominant: {joints}");
			for morph in region
				.morph_targets
				.iter()
				.filter(|morph| morph.affected_vertices > 0 || morph.default_weight.abs() > 0.0001)
			{
				println!(
					"  morph[{}] {:?}: default_weight={:.3} affected={} avg_delta={:.5} max_delta={:.5}",
					morph.index, morph.name, morph.default_weight, morph.affected_vertices, morph.average_delta, morph.max_delta
				);
			}
		}
		for symmetry in &report.mirror_symmetry {
			println!(
				"mirror {}: left={} right={} avg_l2r={:.5} max_l2r={:.5} avg_r2l={:.5} max_r2l={:.5}",
				symmetry.name,
				symmetry.left_vertex_count,
				symmetry.right_vertex_count,
				symmetry.average_left_to_right_distance,
				symmetry.max_left_to_right_distance,
				symmetry.average_right_to_left_distance,
				symmetry.max_right_to_left_distance
			);
			for sample in symmetry.worst_right_samples.iter().take(6) {
				println!(
					"  right v{} dist={:.5} pos=({:.5},{:.5},{:.5}) nearest=v{} ({:.5},{:.5},{:.5}) joint={} w={:.3}",
					sample.vertex_index,
					sample.mirror_distance,
					sample.position[0],
					sample.position[1],
					sample.position[2],
					sample.nearest_vertex_index,
					sample.nearest_position[0],
					sample.nearest_position[1],
					sample.nearest_position[2],
					sample.dominant_joint,
					sample.dominant_weight
				);
			}
		}
		for summary in report.collider_path_summaries.iter().take(12) {
			println!(
				"collider_path {:?}: shape={} inside_bounds={} candidates={} penetrating={} projections={} sources={} min_margin={:.5} min_distance={:.5} threshold={:.5}",
				summary.collider_path,
				summary.collider_shape,
				summary.inside_bounds,
				summary.candidate_count,
				summary.penetrating_count,
				summary.projection_count,
				summary.source_count,
				summary.min_margin,
				summary.min_distance,
				summary.min_threshold
			);
		}
	}
	Ok(())
}

fn dynamics_vertex_probe_report(
	plugin_dirs: &[PathBuf],
	path: &Path,
	input_format: Option<&str>,
	wardrobe_set: Option<&str>,
	node_contains: &str,
	settle_frames: usize,
	apply_mesh_cloth_assist: bool,
	ignore_authored_colliders: bool,
	ignore_node_constraints: bool,
	pose_left_upper_arm_z_deg: Option<f32>,
	pose_right_upper_arm_z_deg: Option<f32>,
	unmotion_frame_json: Option<&Path>,
	tuning: &str,
) -> Result<DynamicsVertexProbeReport, String> {
	let (mut doc, _import_report, _desc) = import_document_for_cli(plugin_dirs, path, input_format)?;
	if let Some(set_id) = wardrobe_set.filter(|set_id| !set_id.trim().is_empty()) {
		apply_unavatar_wardrobe_set(&mut doc, set_id)?;
	}
	let compare_scene = doc.scene.clone();
	let rest_nodes_for_motion = doc.scene.as_ref().map(|scene| scene.nodes.clone());
	if ignore_node_constraints {
		if let Some(scene) = doc.scene.as_mut() {
			scene.node_constraints.clear();
		}
	}
	if let Some(frame_path) = unmotion_frame_json {
		let rest_nodes = rest_nodes_for_motion
			.as_deref()
			.ok_or_else(|| "document has no scene nodes for motion pose".to_string())?;
		let data = fs::read(frame_path).map_err(|err| format!("failed to read UNMotionFrame JSON `{}`: {err}", frame_path.display()))?;
		let frame = dynamics_vertex_probe_unmotion_frame_from_json_slice(&data, frame_path)?;
		un_avatar_skeleton::apply_un_motion_frame_to_document_with_rest(
			&mut doc,
			&frame,
			un_avatar_skeleton::ApplyUnMotionFrameOpts::default(),
			Some(rest_nodes),
		);
	} else if pose_left_upper_arm_z_deg.is_some() || pose_right_upper_arm_z_deg.is_some() {
		let rest_nodes = rest_nodes_for_motion
			.as_deref()
			.ok_or_else(|| "document has no scene nodes for motion pose".to_string())?;
		let frame = dynamics_vertex_probe_motion_pose_frame(pose_left_upper_arm_z_deg, pose_right_upper_arm_z_deg);
		un_avatar_skeleton::apply_un_motion_frame_to_document_with_rest(
			&mut doc,
			&frame,
			un_avatar_skeleton::ApplyUnMotionFrameOpts::default(),
			Some(rest_nodes),
		);
	}
	let scene = doc.scene.as_ref().ok_or_else(|| "document has no scene".to_string())?;
	let settings = doc.dynamics().ok_or_else(|| "document has no dynamics settings".to_string())?;
	let runtime_model = doc.runtime_model();
	let scene_profile_dynamics = runtime_model.scene_profile_dynamics();
	let runtime_dynamics = scene_profile_dynamics
		.as_ref()
		.map(|runtime| runtime.dynamics)
		.unwrap_or_else(|| settings.runtime_dynamics());
	let node_paths = scene_node_paths_by_index(scene);
	let (_, physics_config) = dynamics_motion_trace_tuning_config(tuning)?;
	let dynamic_nodes = dynamics_mesh_cloth_assist_runtime_dynamic_nodes(scene, runtime_dynamics, &physics_config.categories);
	let effective_visibility = scene.effective_visibility();
	let node_filter = node_contains.trim();
	let (node_index, node_path) = if node_filter.is_empty() {
		node_paths
			.iter()
			.enumerate()
			.filter_map(|(index, path)| {
				if !effective_visibility.get(index).copied().unwrap_or(false) {
					return None;
				}
				let node = scene.nodes.get(index)?;
				let skin = node.skin.and_then(|skin_index| scene.skins.get(skin_index))?;
				let mesh = node.mesh.and_then(|mesh_index| scene.meshes.get(mesh_index))?;
				let path = path.as_ref()?;
				let (dynamic_vertex_count, dynamic_weight_sum) = dynamics_vertex_probe_dynamic_weight_score(mesh, skin, &dynamic_nodes);
				if dynamic_vertex_count == 0 {
					return None;
				}
				let cloth_like = dynamics_mesh_cloth_assist_mesh_matches(Some(path), &[], &physics_config.categories);
				Some((index, path.clone(), cloth_like, dynamic_vertex_count, dynamic_weight_sum))
			})
			.max_by(|a, b| {
				a.2.cmp(&b.2)
					.then_with(|| a.3.cmp(&b.3))
					.then_with(|| a.4.total_cmp(&b.4))
					.then_with(|| b.1.cmp(&a.1))
			})
			.map(|(index, path, _, _, _)| (index, path))
			.ok_or_else(|| "no skinned mesh containing dynamics joints found".to_string())?
	} else {
		select_node_path_containing(&node_paths, &effective_visibility, &node_filter)
			.ok_or_else(|| format!("node containing `{node_contains}` not found"))?
	};
	let node = scene
		.nodes
		.get(node_index)
		.ok_or_else(|| format!("node index {node_index} out of range"))?;
	let mesh_index = node.mesh.ok_or_else(|| format!("node `{node_path}` has no mesh"))?;
	let skin_index = node.skin;
	let skin = skin_index.and_then(|index| scene.skins.get(index));
	let mesh = scene
		.meshes
		.get(mesh_index)
		.ok_or_else(|| format!("mesh index {mesh_index} out of range"))?;
	let primitive = mesh.first().ok_or_else(|| format!("mesh index {mesh_index} has no primitives"))?;
	let mut assisted_primitive;
	let (primitive, mesh_cloth_assist_changed_vertices) = if apply_mesh_cloth_assist {
		assisted_primitive = primitive.clone();
		let config = dynamics_vertex_probe_mesh_cloth_assist_config(&physics_config);
		let changed = dynamics_vertex_probe_apply_mesh_cloth_assist(
			&mut assisted_primitive,
			skin,
			&node_paths,
			&node_path,
			&config,
			&dynamic_nodes,
			&physics_config.categories,
		);
		(&assisted_primitive, changed)
	} else {
		(primitive, 0)
	};

	let mut runtime_colliders = runtime_model
		.scene_profile_dynamics()
		.map(|runtime| {
			build_dynamics_bone_colliders_with_sources(
				runtime.scene,
				runtime.humanoid_profile,
				BoneColliderConfig {
					enabled: false,
					..BoneColliderConfig::default()
				},
				runtime_dynamics,
			)
		})
		.unwrap_or_default();
	if ignore_authored_colliders {
		runtime_colliders.retain(|collider| collider.source_id.is_empty());
	}
	let runtime_collider_count = runtime_colliders.len();
	let mut settled_scene = scene.clone();
	let mut sim = DynamicsSimulator::new_with_runtime_dynamics_and_collider_sources(
		&settled_scene,
		runtime_dynamics,
		runtime_colliders,
		physics_config,
	)
	.ok_or_else(|| "UNPhysics simulator could not be created".to_string())?;
	let settle_frames = settle_frames.min(1200);
	let mut step_profile = DynamicsStepProfile::default();
	for _ in 0..settle_frames {
		let frame_profile = sim.step_runtime_dynamics_profiled(&mut settled_scene, runtime_dynamics, 1.0 / 60.0);
		step_profile.fixed_steps = step_profile.fixed_steps.saturating_add(frame_profile.fixed_steps);
		step_profile.active_groups = frame_profile.active_groups;
		step_profile.active_joints = frame_profile.active_joints;
		step_profile.collision_projection_count = step_profile
			.collision_projection_count
			.saturating_add(frame_profile.collision_projection_count);
		for source_id in frame_profile.collision_projection_source_ids {
			push_bounded_unique_string(&mut step_profile.collision_projection_source_ids, source_id, 16);
		}
		for (source_id, count) in frame_profile.collision_projection_source_counts {
			let entry = step_profile.collision_projection_source_counts.entry(source_id).or_default();
			*entry = entry.saturating_add(count);
		}
		for collider_path in frame_profile.collision_projection_collider_paths {
			push_bounded_unique_string(&mut step_profile.collision_projection_collider_paths, collider_path, 16);
		}
		for (collider_path, count) in frame_profile.collision_projection_collider_path_counts {
			let entry = step_profile
				.collision_projection_collider_path_counts
				.entry(collider_path)
				.or_default();
			*entry = entry.saturating_add(count);
		}
		for (source_id, path_counts) in frame_profile.collision_projection_source_collider_path_counts {
			let source_entry = step_profile
				.collision_projection_source_collider_path_counts
				.entry(source_id)
				.or_default();
			for (collider_path, count) in path_counts {
				let entry = source_entry.entry(collider_path).or_default();
				*entry = entry.saturating_add(count);
			}
		}
		step_profile.world_ms += frame_profile.world_ms;
		step_profile.collider_ms += frame_profile.collider_ms;
		step_profile.solve_ms += frame_profile.solve_ms;
		step_profile.solve_collision_ms += frame_profile.solve_collision_ms;
		step_profile.solve_propagate_ms += frame_profile.solve_propagate_ms;
	}

	let compare_scene = compare_scene.as_ref().unwrap_or(scene);
	let rest_world = cli_scene_world_matrices(compare_scene);
	let motion_world = cli_scene_world_matrices(scene);
	let constraint_node_samples = dynamics_vertex_probe_constraint_node_samples(scene, &node_paths, &rest_world, &motion_world);
	let settled_world = cli_scene_world_matrices(&settled_scene);
	let rest_positions = skinned_positions_for_primitive(compare_scene, node_index, skin, primitive, &rest_world)?;
	let settled_positions = skinned_positions_for_primitive(&settled_scene, node_index, skin, primitive, &settled_world)?;
	let node_samples = dynamics_vertex_probe_node_samples(&node_paths, &rest_world, &settled_world);
	let joint_weight_summaries = dynamics_vertex_probe_joint_weight_summaries(scene, skin, primitive, &node_paths);
	let rest_nodes_for_interaction = rest_nodes_for_motion.as_deref().unwrap_or(compare_scene.nodes.as_slice());
	let interaction_values = dynamics_vertex_probe_interaction_values(&doc, rest_nodes_for_interaction, &settled_scene, &node_paths);
	let interaction_parameters = interaction_values
		.iter()
		.cloned()
		.map(|value| DynamicsVertexProbeInteractionParameter {
			parameter: value.parameter,
			angle_parameter: value.angle_parameter,
			source_id: value.source_id,
			angle_value: value.angle_value,
			angle_norm: value.angle_norm,
			angle_deg: value.angle_deg,
			shape_angle_deg: value.shape_angle_deg,
			gravity_angle_deg: value.gravity_angle_deg,
			dominant: value.dominant,
			max_angle_deg: value.max_angle_deg,
			center_peak_scaled: value.center_peak_scaled,
			chain: value.chain,
		})
		.collect::<Vec<_>>();
	let animator_morph_overrides = dynamics_vertex_probe_animator_morph_overrides(&doc, &interaction_values, &node_path);
	let animator_morph_override_regions = if animator_morph_overrides.is_empty() {
		Vec::new()
	} else {
		let mut morphed_primitive = primitive.clone();
		dynamics_vertex_probe_apply_morph_overrides_to_primitive(&mut morphed_primitive, &animator_morph_overrides, &node_path);
		let morphed_settled_positions =
			skinned_positions_for_primitive(&settled_scene, node_index, skin, &morphed_primitive, &settled_world)?;
		dynamics_vertex_probe_regions(
			scene,
			skin,
			&morphed_primitive,
			&node_paths,
			&rest_positions,
			&morphed_settled_positions,
		)
	};
	let probe_dynamic_source_weight_sums =
		dynamics_vertex_probe_dynamic_source_weight_sums(runtime_dynamics, skin, primitive, &dynamic_nodes);
	let probe_dynamic_source_ids = probe_dynamic_source_weight_sums.keys().take(16).cloned().collect::<Vec<_>>();
	let probe_collision_projection_source_counts =
		probe_collision_projection_source_counts(&step_profile.collision_projection_source_counts, &probe_dynamic_source_weight_sums);
	let probe_collision_projection_count = probe_collision_projection_source_counts.values().copied().sum();
	let probe_collision_projection_source_ids = probe_collision_projection_source_counts
		.keys()
		.take(16)
		.cloned()
		.collect::<Vec<_>>();
	let probe_collision_projection_collider_path_counts = probe_collision_projection_collider_path_counts(
		&step_profile.collision_projection_source_collider_path_counts,
		&probe_dynamic_source_weight_sums,
	);
	let regions = dynamics_vertex_probe_regions(scene, skin, primitive, &node_paths, &rest_positions, &settled_positions);
	let mirror_symmetry = dynamics_vertex_probe_mirror_symmetry(scene, skin, primitive, &node_paths, &settled_positions);
	let collider_summary_world = cli_scene_world_matrices(&settled_scene);
	let collider_tail_samples = sim.tail_samples();
	let collider_path_summaries = dynamics_vertex_probe_collider_path_summaries_for_samples_with_world(
		&collider_summary_world,
		sim.bone_colliders(),
		sim.bone_collider_source_ids(),
		sim.bone_collider_paths(),
		&collider_tail_samples,
		&step_profile.collision_projection_collider_path_counts,
	);
	let probe_collider_path_summaries = if probe_dynamic_source_weight_sums.is_empty() {
		Vec::new()
	} else {
		let probe_tail_samples = collider_tail_samples
			.iter()
			.filter(|sample| probe_dynamic_source_weight_sums.contains_key(&sample.source_id))
			.cloned()
			.collect::<Vec<_>>();
		dynamics_vertex_probe_collider_path_summaries_for_samples_with_world(
			&collider_summary_world,
			sim.bone_colliders(),
			sim.bone_collider_source_ids(),
			sim.bone_collider_paths(),
			&probe_tail_samples,
			&probe_collision_projection_collider_path_counts,
		)
	};
	let probe_tail_samples = if probe_dynamic_source_weight_sums.is_empty() {
		collider_tail_samples.clone()
	} else {
		collider_tail_samples
			.iter()
			.filter(|sample| probe_dynamic_source_weight_sums.contains_key(&sample.source_id))
			.cloned()
			.collect()
	};
	Ok(DynamicsVertexProbeReport {
		path: path.display().to_string(),
		wardrobe_set: wardrobe_set.map(str::to_string),
		tuning: tuning.to_string(),
		node_index,
		node_path,
		mesh_index,
		skin_index,
		settle_frames,
		pose_left_upper_arm_z_deg,
		pose_right_upper_arm_z_deg,
		unmotion_frame_json: unmotion_frame_json.map(|path| path.display().to_string()),
		node_constraints_ignored: ignore_node_constraints,
		authored_colliders_ignored: ignore_authored_colliders,
		runtime_collider_count,
		solve_collision_projection_count: step_profile.collision_projection_count,
		solve_collision_projection_source_ids: step_profile.collision_projection_source_ids,
		solve_collision_projection_source_counts: step_profile.collision_projection_source_counts,
		probe_dynamic_source_ids,
		probe_dynamic_source_weight_sums,
		probe_collision_projection_count,
		probe_collision_projection_source_ids,
		probe_collision_projection_source_counts,
		solve_collision_projection_collider_paths: step_profile.collision_projection_collider_paths,
		solve_collision_projection_collider_path_counts: step_profile.collision_projection_collider_path_counts,
		solve_collision_projection_source_collider_path_counts: step_profile.collision_projection_source_collider_path_counts,
		collider_path_summaries,
		probe_collision_projection_collider_path_counts,
		probe_collider_path_summaries,
		mesh_cloth_assist_applied: apply_mesh_cloth_assist,
		mesh_cloth_assist_changed_vertices,
		node_samples,
		constraint_node_samples,
		probe_tail_samples,
		interaction_parameters,
		animator_morph_overrides,
		animator_morph_override_regions,
		joint_weight_summaries,
		regions,
		mirror_symmetry,
	})
}

#[cfg(test)]
fn dynamics_vertex_probe_collider_path_summaries_for_samples(
	scene: &UnaSceneSnapshot,
	colliders: &[BoneColliderPrimitive],
	collider_source_ids: &[String],
	collider_paths: &[String],
	tail_samples: &[DynamicsTailSample],
	projection_counts: &BTreeMap<String, u32>,
) -> Vec<DynamicsVertexProbeColliderPathSummary> {
	let world = cli_scene_world_matrices(scene);
	dynamics_vertex_probe_collider_path_summaries_for_samples_with_world(
		&world,
		colliders,
		collider_source_ids,
		collider_paths,
		tail_samples,
		projection_counts,
	)
}

fn dynamics_vertex_probe_collider_path_summaries_for_samples_with_world(
	world: &[Mat4],
	colliders: &[BoneColliderPrimitive],
	collider_source_ids: &[String],
	collider_paths: &[String],
	tail_samples: &[DynamicsTailSample],
	projection_counts: &BTreeMap<String, u32>,
) -> Vec<DynamicsVertexProbeColliderPathSummary> {
	#[derive(Default)]
	struct Accum {
		summary: Option<DynamicsVertexProbeColliderPathSummary>,
		source_ids: BTreeSet<String>,
	}

	let all_global = collider_source_ids.iter().all(String::is_empty);
	let mut global_collider_indices = Vec::new();
	let mut source_collider_indices = BTreeMap::<&str, Vec<usize>>::new();
	if !all_global {
		for collider_index in 0..colliders.len() {
			let collider_source_id = collider_source_ids.get(collider_index).map(String::as_str).unwrap_or_default();
			if collider_source_id.is_empty() {
				global_collider_indices.push(collider_index);
			} else {
				source_collider_indices.entry(collider_source_id).or_default().push(collider_index);
			}
		}
	}
	let mut by_path = BTreeMap::<String, Accum>::new();
	for tail in tail_samples {
		let tail_point = Vec3::from_array(tail.curr_tail);
		let mut visit_collider = |collider_index: usize| {
			let Some(collider) = colliders.get(collider_index) else {
				return;
			};
			let Some(contact) = dynamics_vertex_probe_collider_contact(&world, tail, tail_point, collider) else {
				return;
			};
			let collider_path = collider_paths
				.get(collider_index)
				.cloned()
				.filter(|path| !path.is_empty())
				.unwrap_or_else(|| format!("collider_index:{collider_index}"));
			let accum = by_path.entry(collider_path.clone()).or_default();
			let summary = accum.summary.get_or_insert_with(|| DynamicsVertexProbeColliderPathSummary {
				collider_path: collider_path.clone(),
				collider_shape: contact.collider_shape.clone(),
				inside_bounds: contact.inside_bounds,
				candidate_count: 0,
				penetrating_count: 0,
				projection_count: projection_counts.get(&collider_path).copied().unwrap_or_default(),
				source_count: 0,
				min_margin: contact.margin,
				min_distance: contact.distance,
				min_threshold: contact.threshold,
				min_margin_tail: Some(dynamics_vertex_probe_collider_tail_contact(tail, &contact)),
				sample_source_ids: Vec::new(),
			});
			summary.candidate_count += 1;
			if contact.margin < 0.0 {
				summary.penetrating_count += 1;
			}
			if contact.margin < summary.min_margin {
				summary.min_margin = contact.margin;
				summary.min_distance = contact.distance;
				summary.min_threshold = contact.threshold;
				summary.collider_shape = contact.collider_shape.clone();
				summary.inside_bounds = contact.inside_bounds;
				summary.min_margin_tail = Some(dynamics_vertex_probe_collider_tail_contact(tail, &contact));
			}
			if accum.source_ids.insert(tail.source_id.clone()) && summary.sample_source_ids.len() < 8 {
				summary.sample_source_ids.push(tail.source_id.clone());
			}
		};
		if all_global {
			for collider_index in 0..colliders.len() {
				visit_collider(collider_index);
			}
		} else {
			for &collider_index in &global_collider_indices {
				visit_collider(collider_index);
			}
			if let Some(source_indices) = (!tail.source_id.is_empty())
				.then(|| source_collider_indices.get(tail.source_id.as_str()))
				.flatten()
			{
				for &collider_index in source_indices {
					visit_collider(collider_index);
				}
			}
		}
	}
	let mut summaries = by_path
		.into_values()
		.filter_map(|accum| {
			let mut summary = accum.summary?;
			summary.source_count = accum.source_ids.len();
			Some(summary)
		})
		.collect::<Vec<_>>();
	summaries.sort_by(|a, b| {
		b.penetrating_count
			.cmp(&a.penetrating_count)
			.then_with(|| a.min_margin.total_cmp(&b.min_margin))
			.then_with(|| b.candidate_count.cmp(&a.candidate_count))
			.then_with(|| a.collider_path.cmp(&b.collider_path))
	});
	summaries
}

struct DynamicsVertexProbeColliderContact {
	collider_shape: String,
	inside_bounds: bool,
	distance: f32,
	threshold: f32,
	margin: f32,
	closest_pos: [f32; 3],
	collider_a: Option<[f32; 3]>,
	collider_b: Option<[f32; 3]>,
}

fn dynamics_vertex_probe_collider_tail_contact(
	tail: &DynamicsTailSample,
	contact: &DynamicsVertexProbeColliderContact,
) -> DynamicsVertexProbeColliderTailContact {
	let tail_pos = Vec3::from_array(tail.curr_tail);
	let closest = Vec3::from_array(contact.closest_pos);
	let push_dir = (tail_pos - closest).normalize_or_zero();
	DynamicsVertexProbeColliderTailContact {
		source_id: tail.source_id.clone(),
		runtime_index: tail.runtime_index,
		joint_index: tail.joint_index,
		anchor_pos: tail.anchor_pos,
		tail_pos: tail.curr_tail,
		closest_pos: contact.closest_pos,
		collider_a: contact.collider_a,
		collider_b: contact.collider_b,
		push_dir: push_dir.to_array(),
	}
}

fn closest_point_segment(point: Vec3, a: Vec3, b: Vec3) -> Vec3 {
	let ab = b - a;
	let denom = ab.length_squared();
	if denom <= 1e-12 {
		return a;
	}
	let t = ((point - a).dot(ab) / denom).clamp(0.0, 1.0);
	a + ab * t
}

fn dynamics_vertex_probe_collider_contact(
	world: &[Mat4],
	tail: &DynamicsTailSample,
	tail_point: Vec3,
	collider: &BoneColliderPrimitive,
) -> Option<DynamicsVertexProbeColliderContact> {
	let (collider_shape, inside_bounds, distance, threshold, margin, closest_pos, collider_a, collider_b) = match *collider {
		BoneColliderPrimitive::Sphere { node, radius } => {
			let closest = world.get(node)?.transform_point3(Vec3::ZERO);
			let distance = tail_point.distance(closest);
			let threshold = radius.max(0.0) + tail.hit_radius.max(0.0);
			("sphere", false, distance, threshold, distance - threshold, closest, None, None)
		}
		BoneColliderPrimitive::Capsule {
			start_node,
			end_node,
			radius,
		} => {
			let a = world.get(start_node)?.transform_point3(Vec3::ZERO);
			let b = world.get(end_node)?.transform_point3(Vec3::ZERO);
			let closest = closest_point_segment(tail_point, a, b);
			let distance = tail_point.distance(closest);
			let threshold = radius.max(0.0) + tail.hit_radius.max(0.0);
			(
				"capsule",
				false,
				distance,
				threshold,
				distance - threshold,
				closest,
				Some(a),
				Some(b),
			)
		}
		BoneColliderPrimitive::LocalSphere {
			node,
			center,
			radius,
			inside_bounds,
			bones_as_sphere: _,
		} => {
			let (center, radius) = local_sphere_world(world, node, center, radius)?;
			let distance = tail_point.distance(center);
			let hit_radius = tail.hit_radius.max(0.0);
			let threshold = if inside_bounds {
				(radius - hit_radius).max(0.0)
			} else {
				radius.max(0.0) + hit_radius
			};
			let margin = if inside_bounds {
				threshold - distance
			} else {
				distance - threshold
			};
			("local_sphere", inside_bounds, distance, threshold, margin, center, None, None)
		}
		BoneColliderPrimitive::LocalCapsule {
			node,
			center,
			axis,
			half_length,
			radius,
			inside_bounds,
			bones_as_sphere: _,
		} => {
			let (a, b, radius) = local_capsule_world(world, node, center, axis, half_length, radius)?;
			let closest = closest_point_segment(tail_point, a, b);
			let distance = tail_point.distance(closest);
			let hit_radius = tail.hit_radius.max(0.0);
			let threshold = if inside_bounds {
				(radius - hit_radius).max(0.0)
			} else {
				radius.max(0.0) + hit_radius
			};
			let margin = if inside_bounds {
				threshold - distance
			} else {
				distance - threshold
			};
			(
				"local_capsule",
				inside_bounds,
				distance,
				threshold,
				margin,
				closest,
				Some(a),
				Some(b),
			)
		}
		BoneColliderPrimitive::LocalPlane {
			node,
			center,
			normal,
			inside_bounds,
		} => {
			let (point, normal) = local_plane_world(world, node, center, normal)?;
			let signed_distance = (tail_point - point).dot(normal);
			let hit_radius = tail.hit_radius.max(0.0);
			let margin = if inside_bounds {
				-hit_radius - signed_distance
			} else {
				signed_distance - hit_radius
			};
			("local_plane", inside_bounds, signed_distance, hit_radius, margin, point, None, None)
		}
	};
	if !distance.is_finite() {
		return None;
	}
	Some(DynamicsVertexProbeColliderContact {
		collider_shape: collider_shape.to_string(),
		inside_bounds,
		distance,
		threshold,
		margin,
		closest_pos: closest_pos.to_array(),
		collider_a: collider_a.map(|value| value.to_array()),
		collider_b: collider_b.map(|value| value.to_array()),
	})
}

fn dynamics_vertex_probe_node_samples(
	node_paths: &[Option<String>],
	rest_world: &[Mat4],
	settled_world: &[Mat4],
) -> Vec<DynamicsVertexProbeNodeSample> {
	let mut out = Vec::<DynamicsVertexProbeNodeSample>::new();
	for (node_index, path) in node_paths.iter().enumerate() {
		let Some(path) = path else {
			continue;
		};
		let Some(rest) = rest_world.get(node_index).copied() else {
			continue;
		};
		let Some(settled) = settled_world.get(node_index).copied() else {
			continue;
		};
		let rest_translation = rest.transform_point3(Vec3::ZERO);
		let settled_translation = settled.transform_point3(Vec3::ZERO);
		let delta = settled_translation - rest_translation;
		let displacement = delta.length();
		if displacement <= 1e-5 {
			continue;
		}
		out.push(DynamicsVertexProbeNodeSample {
			node_index,
			path: path.clone(),
			rest_translation: rest_translation.to_array(),
			settled_translation: settled_translation.to_array(),
			delta: delta.to_array(),
			displacement,
		});
	}
	out.sort_by(|a, b| {
		b.displacement
			.partial_cmp(&a.displacement)
			.unwrap_or(std::cmp::Ordering::Equal)
			.then_with(|| a.node_index.cmp(&b.node_index))
	});
	out.truncate(96);
	out
}

fn dynamics_vertex_probe_constraint_node_samples(
	scene: &UnaSceneSnapshot,
	node_paths: &[Option<String>],
	rest_world: &[Mat4],
	motion_world: &[Mat4],
) -> Vec<DynamicsVertexProbeNodeSample> {
	let mut node_indices = BTreeSet::new();
	for constraint in &scene.node_constraints {
		node_indices.insert(constraint.target_node);
		node_indices.insert(constraint.source_node);
		for source in &constraint.sources {
			node_indices.insert(source.source_node);
		}
	}
	let mut out = Vec::new();
	for node_index in node_indices {
		let path = node_paths
			.get(node_index)
			.and_then(|path| path.as_ref())
			.cloned()
			.unwrap_or_else(|| format!("#{node_index}"));
		let Some(rest) = rest_world.get(node_index).copied() else {
			continue;
		};
		let Some(motion) = motion_world.get(node_index).copied() else {
			continue;
		};
		let rest_translation = rest.transform_point3(Vec3::ZERO);
		let motion_translation = motion.transform_point3(Vec3::ZERO);
		let delta = motion_translation - rest_translation;
		out.push(DynamicsVertexProbeNodeSample {
			node_index,
			path,
			rest_translation: rest_translation.to_array(),
			settled_translation: motion_translation.to_array(),
			delta: delta.to_array(),
			displacement: delta.length(),
		});
	}
	out.sort_by(|a, b| a.node_index.cmp(&b.node_index));
	out
}

fn select_node_path_containing(node_paths: &[Option<String>], effective_visibility: &[bool], node_filter: &str) -> Option<(usize, String)> {
	let filters = vec![node_filter.to_string()];
	let mut first_match = None;
	for (index, path) in node_paths
		.iter()
		.enumerate()
		.filter_map(|(index, path)| path.as_ref().map(|path| (index, path)))
	{
		if !skeleton_mesh_cloth_assist_mesh_matches_with_categories(path, &filters, &[]) {
			continue;
		}
		if effective_visibility.get(index).copied().unwrap_or(false) {
			return Some((index, path.clone()));
		}
		first_match.get_or_insert((index, path));
	}
	first_match.map(|(index, path)| (index, path.clone()))
}

fn dynamics_vertex_probe_mesh_cloth_assist_config(physics_config: &DynamicsPhysicsConfig) -> DynamicsMeshClothAssistConfig {
	let mut config = physics_config.clone().normalized().mesh_cloth_assist;
	config.enabled = true;
	config
}

fn dynamics_vertex_probe_dynamic_weight_score(
	mesh: &[un_avatar_core::UnaMeshBuffers],
	skin: &un_avatar_core::UnaSkin,
	dynamic_nodes: &[usize],
) -> (usize, f32) {
	let mut vertex_count = 0usize;
	let mut weight_sum = 0.0_f32;
	for primitive in mesh {
		let Some(joints) = primitive.joints.as_ref() else {
			continue;
		};
		let Some(weights) = primitive.weights.as_ref() else {
			continue;
		};
		for (vertex_joints, vertex_weights) in joints.iter().zip(weights.iter()) {
			let mut vertex_dynamic_weight = 0.0_f32;
			for lane in 0..4 {
				let joint_index = vertex_joints[lane] as usize;
				let Some(&node_index) = skin.joint_nodes.get(joint_index) else {
					continue;
				};
				if dynamic_nodes.binary_search(&node_index).is_ok() {
					vertex_dynamic_weight += vertex_weights[lane].max(0.0);
				}
			}
			if vertex_dynamic_weight > 0.001 {
				vertex_count += 1;
				weight_sum += vertex_dynamic_weight;
			}
		}
	}
	(vertex_count, weight_sum)
}

fn dynamics_vertex_probe_dynamic_source_weight_sums(
	runtime_dynamics: un_avatar_core::UnaRuntimeDynamics<'_>,
	skin: Option<&un_avatar_core::UnaSkin>,
	primitive: &un_avatar_core::UnaMeshBuffers,
	dynamic_nodes: &[usize],
) -> BTreeMap<String, f32> {
	let Some(skin) = skin else {
		return BTreeMap::new();
	};
	let Some(joints) = primitive.joints.as_ref() else {
		return BTreeMap::new();
	};
	let Some(weights) = primitive.weights.as_ref() else {
		return BTreeMap::new();
	};
	let mut source_ids_by_node = BTreeMap::<usize, Vec<&str>>::new();
	for group in runtime_dynamics.dynamics_groups().filter(|group| group.effective_enabled) {
		if group.source_id.is_empty() {
			continue;
		}
		for node_index in dynamics_mesh_cloth_assist_deforming_nodes(group.chain.bone_node_indices, group.chain.interaction_start_index) {
			source_ids_by_node.entry(node_index).or_default().push(group.source_id);
		}
	}
	let mut source_weight_sums = BTreeMap::<String, f32>::new();
	for (vertex_joints, vertex_weights) in joints.iter().zip(weights.iter()) {
		for lane in 0..4 {
			let weight = vertex_weights[lane].max(0.0);
			if weight <= 0.001 {
				continue;
			}
			let joint_index = vertex_joints[lane] as usize;
			let Some(&node_index) = skin.joint_nodes.get(joint_index) else {
				continue;
			};
			if dynamic_nodes.binary_search(&node_index).is_err() {
				continue;
			}
			let Some(source_ids) = source_ids_by_node.get(&node_index) else {
				continue;
			};
			for source_id in source_ids {
				let entry = source_weight_sums.entry((*source_id).to_string()).or_default();
				*entry += weight;
			}
		}
	}
	source_weight_sums.retain(|_, weight_sum| *weight_sum > 0.001);
	source_weight_sums
}

fn probe_collision_projection_source_counts(
	all_projection_source_counts: &BTreeMap<String, u32>,
	probe_dynamic_source_weight_sums: &BTreeMap<String, f32>,
) -> BTreeMap<String, u32> {
	probe_dynamic_source_weight_sums
		.keys()
		.filter_map(|source_id| {
			all_projection_source_counts
				.get(source_id)
				.copied()
				.filter(|count| *count > 0)
				.map(|count| (source_id.clone(), count))
		})
		.collect()
}

fn probe_collision_projection_collider_path_counts(
	all_source_path_counts: &BTreeMap<String, BTreeMap<String, u32>>,
	probe_dynamic_source_weight_sums: &BTreeMap<String, f32>,
) -> BTreeMap<String, u32> {
	let mut out = BTreeMap::<String, u32>::new();
	for source_id in probe_dynamic_source_weight_sums.keys() {
		let Some(path_counts) = all_source_path_counts.get(source_id) else {
			continue;
		};
		for (collider_path, count) in path_counts {
			let entry = out.entry(collider_path.clone()).or_default();
			*entry = entry.saturating_add(*count);
		}
	}
	out.retain(|_, count| *count > 0);
	out
}

fn dynamics_vertex_probe_apply_mesh_cloth_assist(
	primitive: &mut un_avatar_core::UnaMeshBuffers,
	skin: Option<&un_avatar_core::UnaSkin>,
	node_paths: &[Option<String>],
	node_path: &str,
	config: &DynamicsMeshClothAssistConfig,
	dynamic_nodes: &[usize],
	categories: &[DynamicsCategoryDefinition],
) -> usize {
	if !config.enabled || config.max_assist_weight <= 0.0 || primitive.positions.is_empty() {
		return 0;
	}
	let Some(skin) = skin else {
		return 0;
	};
	if !dynamics_mesh_cloth_assist_mesh_matches(Some(node_path), &config.mesh_path_contains, categories) {
		return 0;
	}
	let Some(indices) = primitive.indices.as_deref() else {
		return 0;
	};
	if indices.is_empty() {
		return 0;
	}
	let Some(joints) = primitive.joints.as_mut() else {
		return 0;
	};
	let Some(weights) = primitive.weights.as_mut() else {
		return 0;
	};
	let joint_count = skin.joint_nodes.len().min(skin.inverse_bind_matrices.len());
	if joint_count == 0 {
		return 0;
	}
	let joint_roles = dynamics_mesh_cloth_assist_joint_roles(skin, joint_count, Some(dynamic_nodes), |joint_index| {
		dynamics_mesh_cloth_assist_joint_leaf(skin, node_paths, joint_index)
	});
	if !joint_roles.iter().any(|role| *role == DynamicsMeshClothAssistJointRole::Dynamic) {
		return 0;
	}
	let count = primitive.positions.len().min(joints.len()).min(weights.len());
	let mut vertices = (0..count)
		.map(|index| DynamicsVertexProbeMeshClothAssistVertex {
			joints: joints[index],
			weights: weights[index],
		})
		.collect::<Vec<_>>();
	let changed = apply_dynamics_mesh_cloth_assist_to_vertices(&mut vertices, indices, joint_count, config, |joint_index| {
		joint_roles
			.get(joint_index)
			.copied()
			.unwrap_or(DynamicsMeshClothAssistJointRole::Other)
	});
	for (index, vertex) in vertices.into_iter().enumerate() {
		joints[index] = vertex.joints;
		weights[index] = vertex.weights;
	}
	changed
}

struct DynamicsVertexProbeMeshClothAssistVertex {
	joints: [u16; 4],
	weights: [f32; 4],
}

impl DynamicsMeshClothAssistVertex for DynamicsVertexProbeMeshClothAssistVertex {
	fn joints(&self) -> [u16; 4] {
		self.joints
	}

	fn weights(&self) -> [f32; 4] {
		self.weights
	}

	fn set_joints(&mut self, joints: [u16; 4]) {
		self.joints = joints;
	}

	fn set_weights(&mut self, weights: [f32; 4]) {
		self.weights = weights;
	}
}

fn skinned_positions_for_primitive(
	_scene: &UnaSceneSnapshot,
	node_index: usize,
	skin: Option<&un_avatar_core::UnaSkin>,
	primitive: &un_avatar_core::UnaMeshBuffers,
	world: &[Mat4],
) -> Result<Vec<[f32; 3]>, String> {
	let Some(skin) = skin else {
		let node_world = world.get(node_index).copied().unwrap_or(Mat4::IDENTITY);
		return Ok(primitive
			.positions
			.iter()
			.map(|position| node_world.transform_point3(Vec3::from_array(*position)).to_array())
			.collect());
	};
	let joints = primitive
		.joints
		.as_ref()
		.ok_or_else(|| "selected primitive has a skin but no joints".to_string())?;
	let weights = primitive
		.weights
		.as_ref()
		.ok_or_else(|| "selected primitive has a skin but no weights".to_string())?;
	let mesh_world = world.get(node_index).copied().unwrap_or(Mat4::IDENTITY);
	let inv_mesh = mesh_world.inverse();
	let mut palette = Vec::with_capacity(skin.joint_nodes.len().min(skin.inverse_bind_matrices.len()));
	for (joint_index, &node) in skin.joint_nodes.iter().enumerate().take(skin.inverse_bind_matrices.len()) {
		let joint_world = world.get(node).copied().unwrap_or(Mat4::IDENTITY);
		let inverse_bind = Mat4::from_cols_array(&skin.inverse_bind_matrices[joint_index]);
		palette.push(inv_mesh * joint_world * inverse_bind);
	}
	let mut out = Vec::with_capacity(primitive.positions.len());
	for (index, position) in primitive.positions.iter().enumerate() {
		let joints = joints.get(index).copied().unwrap_or([0; 4]);
		let weights = weights.get(index).copied().unwrap_or([1.0, 0.0, 0.0, 0.0]);
		let p = Vec4::new(position[0], position[1], position[2], 1.0);
		let mut skinned = Vec4::ZERO;
		let mut weight_sum = 0.0;
		for slot in 0..4 {
			let weight = weights[slot];
			if weight <= 0.0 {
				continue;
			}
			let joint = joints[slot] as usize;
			let matrix = palette.get(joint).copied().unwrap_or(Mat4::IDENTITY);
			skinned += matrix * p * weight;
			weight_sum += weight;
		}
		if weight_sum <= 0.0 {
			skinned = p;
		}
		out.push([skinned.x, skinned.y, skinned.z]);
	}
	Ok(out)
}

fn dynamics_vertex_probe_regions(
	scene: &UnaSceneSnapshot,
	skin: Option<&un_avatar_core::UnaSkin>,
	primitive: &un_avatar_core::UnaMeshBuffers,
	node_paths: &[Option<String>],
	rest_positions: &[[f32; 3]],
	settled_positions: &[[f32; 3]],
) -> Vec<DynamicsVertexProbeRegionReport> {
	let regions: [(&str, fn([f32; 3]) -> bool); 5] = [
		("all_vertices", |_| true),
		("front_center", |p| p[0].abs() < 0.08 && p[2] > 0.08 && p[1] > 1.0 && p[1] < 1.24),
		("front_left", |p| {
			p[0] > 0.08 && p[0] < 0.16 && p[2] > 0.08 && p[1] > 1.0 && p[1] < 1.24
		}),
		("front_right", |p| {
			p[0] < -0.08 && p[0] > -0.16 && p[2] > 0.08 && p[1] > 1.0 && p[1] < 1.24
		}),
		("upper_front_all", |p| p[2] > 0.08 && p[1] > 1.0 && p[1] < 1.24),
	];
	let mut reports = regions
		.into_iter()
		.map(|(name, predicate)| {
			dynamics_vertex_probe_region(
				scene,
				skin,
				primitive,
				node_paths,
				rest_positions,
				settled_positions,
				name,
				predicate,
			)
		})
		.collect::<Vec<_>>();
	let focused_regions: [(&str, fn([f32; 3]) -> bool); 9] = [
		("front_upper_center", |p| {
			p[0].abs() < 0.08 && p[2] > 0.08 && p[1] > 1.12 && p[1] < 1.20
		}),
		("front_upper_left", |p| {
			p[0] > 0.08 && p[0] < 0.16 && p[2] > 0.08 && p[1] > 1.12 && p[1] < 1.20
		}),
		("front_upper_right", |p| {
			p[0] < -0.08 && p[0] > -0.16 && p[2] > 0.08 && p[1] > 1.12 && p[1] < 1.20
		}),
		("front_lower_left", |p| {
			p[0] > 0.08 && p[0] < 0.16 && p[2] > 0.08 && p[1] > 0.98 && p[1] < 1.08
		}),
		("front_lower_right", |p| {
			p[0] < -0.08 && p[0] > -0.16 && p[2] > 0.08 && p[1] > 0.98 && p[1] < 1.08
		}),
		("upper_side_left", |p| {
			p[0] > 0.14 && p[0] < 0.28 && p[2] > -0.02 && p[2] < 0.12 && p[1] > 1.08 && p[1] < 1.26
		}),
		("upper_side_right", |p| {
			p[0] < -0.14 && p[0] > -0.28 && p[2] > -0.02 && p[2] < 0.12 && p[1] > 1.08 && p[1] < 1.26
		}),
		("upper_back_left", |p| {
			p[0] > 0.12 && p[0] < 0.28 && p[2] > -0.14 && p[2] < 0.02 && p[1] > 1.04 && p[1] < 1.24
		}),
		("upper_back_right", |p| {
			p[0] < -0.12 && p[0] > -0.28 && p[2] > -0.14 && p[2] < 0.02 && p[1] > 1.04 && p[1] < 1.24
		}),
	];
	reports.extend(focused_regions.into_iter().map(|(name, predicate)| {
		dynamics_vertex_probe_region(
			scene,
			skin,
			primitive,
			node_paths,
			rest_positions,
			settled_positions,
			name,
			predicate,
		)
	}));
	reports
}

fn dynamics_vertex_probe_mirror_symmetry(
	scene: &UnaSceneSnapshot,
	skin: Option<&un_avatar_core::UnaSkin>,
	primitive: &un_avatar_core::UnaMeshBuffers,
	node_paths: &[Option<String>],
	positions: &[[f32; 3]],
) -> Vec<DynamicsVertexProbeMirrorSymmetryReport> {
	let regions: [(&str, fn([f32; 3]) -> bool); 5] = [
		("all_vertices", |_| true),
		("cape_shoulders_broad", |p| {
			p[0].abs() > 0.08 && p[0].abs() < 0.42 && p[1] > 0.96 && p[1] < 1.34 && p[2] > -0.24 && p[2] < 0.18
		}),
		("cape_upper_shoulders", |p| {
			p[0].abs() > 0.10 && p[0].abs() < 0.36 && p[1] > 1.08 && p[1] < 1.30 && p[2] > -0.20 && p[2] < 0.14
		}),
		("cape_front_shoulders", |p| {
			p[0].abs() > 0.10 && p[0].abs() < 0.36 && p[1] > 1.00 && p[1] < 1.30 && p[2] > -0.02 && p[2] < 0.18
		}),
		("cape_back_shoulders", |p| {
			p[0].abs() > 0.10 && p[0].abs() < 0.36 && p[1] > 1.00 && p[1] < 1.30 && p[2] > -0.24 && p[2] < 0.02
		}),
	];
	regions
		.into_iter()
		.map(|(name, predicate)| {
			dynamics_vertex_probe_mirror_symmetry_region(scene, skin, primitive, node_paths, positions, name, predicate)
		})
		.collect()
}

fn dynamics_vertex_probe_mirror_symmetry_region(
	scene: &UnaSceneSnapshot,
	skin: Option<&un_avatar_core::UnaSkin>,
	primitive: &un_avatar_core::UnaMeshBuffers,
	node_paths: &[Option<String>],
	positions: &[[f32; 3]],
	name: &str,
	predicate: fn([f32; 3]) -> bool,
) -> DynamicsVertexProbeMirrorSymmetryReport {
	let left_indices = positions
		.iter()
		.enumerate()
		.filter_map(|(index, &position)| (position[0] > 0.002 && predicate(position)).then_some(index))
		.collect::<Vec<_>>();
	let right_indices = positions
		.iter()
		.enumerate()
		.filter_map(|(index, &position)| (position[0] < -0.002 && predicate(position)).then_some(index))
		.collect::<Vec<_>>();
	let left_samples = dynamics_vertex_probe_mirror_samples(scene, skin, primitive, node_paths, positions, &left_indices, &right_indices);
	let right_samples = dynamics_vertex_probe_mirror_samples(scene, skin, primitive, node_paths, positions, &right_indices, &left_indices);
	let (average_left_to_right_distance, max_left_to_right_distance) = dynamics_vertex_probe_mirror_distance_summary(&left_samples);
	let (average_right_to_left_distance, max_right_to_left_distance) = dynamics_vertex_probe_mirror_distance_summary(&right_samples);
	DynamicsVertexProbeMirrorSymmetryReport {
		name: name.to_string(),
		left_vertex_count: left_indices.len(),
		right_vertex_count: right_indices.len(),
		average_left_to_right_distance,
		max_left_to_right_distance,
		average_right_to_left_distance,
		max_right_to_left_distance,
		worst_left_samples: dynamics_vertex_probe_worst_mirror_samples(left_samples),
		worst_right_samples: dynamics_vertex_probe_worst_mirror_samples(right_samples),
	}
}

fn dynamics_vertex_probe_mirror_samples(
	scene: &UnaSceneSnapshot,
	skin: Option<&un_avatar_core::UnaSkin>,
	primitive: &un_avatar_core::UnaMeshBuffers,
	node_paths: &[Option<String>],
	positions: &[[f32; 3]],
	source_indices: &[usize],
	target_indices: &[usize],
) -> Vec<DynamicsVertexProbeMirrorSample> {
	let joints = primitive.joints.as_ref();
	let weights = primitive.weights.as_ref();
	source_indices
		.iter()
		.filter_map(|&source_index| {
			let &position = positions.get(source_index)?;
			let mirrored = Vec3::new(-position[0], position[1], position[2]);
			let (nearest_index, nearest_position, nearest_distance) = target_indices
				.iter()
				.filter_map(|&target_index| {
					let target = Vec3::from_array(*positions.get(target_index)?);
					Some((target_index, target, (target - mirrored).length()))
				})
				.min_by(|a, b| a.2.total_cmp(&b.2))?;
			let (dominant_joint, dominant_weight, influences) = dynamics_vertex_probe_influences(
				scene,
				skin,
				node_paths,
				joints.and_then(|j| j.get(source_index)),
				weights.and_then(|w| w.get(source_index)),
			);
			Some(DynamicsVertexProbeMirrorSample {
				vertex_index: source_index,
				position,
				mirrored_position: mirrored.to_array(),
				nearest_vertex_index: nearest_index,
				nearest_position: nearest_position.to_array(),
				mirror_distance: nearest_distance,
				dominant_joint,
				dominant_weight,
				influences,
			})
		})
		.collect()
}

fn dynamics_vertex_probe_mirror_distance_summary(samples: &[DynamicsVertexProbeMirrorSample]) -> (f32, f32) {
	let mut sum = 0.0_f32;
	let mut max = 0.0_f32;
	for sample in samples {
		sum += sample.mirror_distance;
		max = max.max(sample.mirror_distance);
	}
	(sum / samples.len().max(1) as f32, max)
}

fn dynamics_vertex_probe_worst_mirror_samples(mut samples: Vec<DynamicsVertexProbeMirrorSample>) -> Vec<DynamicsVertexProbeMirrorSample> {
	samples.sort_by(|a, b| {
		b.mirror_distance
			.total_cmp(&a.mirror_distance)
			.then_with(|| a.vertex_index.cmp(&b.vertex_index))
	});
	samples.truncate(16);
	samples
}

fn dynamics_vertex_probe_joint_weight_summaries(
	scene: &UnaSceneSnapshot,
	skin: Option<&un_avatar_core::UnaSkin>,
	primitive: &un_avatar_core::UnaMeshBuffers,
	node_paths: &[Option<String>],
) -> Vec<DynamicsVertexProbeJointWeightSummary> {
	let Some(skin) = skin else {
		return Vec::new();
	};
	let Some(joints) = primitive.joints.as_ref() else {
		return Vec::new();
	};
	let Some(weights) = primitive.weights.as_ref() else {
		return Vec::new();
	};
	let mut summaries = BTreeMap::<usize, (usize, usize, f32, f32, Vec3, Vec3, Vec3)>::new();
	for (vertex_index, (vertex_joints, vertex_weights)) in joints.iter().zip(weights.iter()).enumerate() {
		let position = Vec3::from_array(primitive.positions.get(vertex_index).copied().unwrap_or([0.0; 3]));
		let dominant_joint = vertex_joints
			.iter()
			.copied()
			.zip(vertex_weights.iter().copied())
			.filter(|(_, weight)| *weight > 0.001)
			.max_by(|(_, a), (_, b)| a.total_cmp(b))
			.map(|(joint, _)| joint as usize);
		for slot in 0..4 {
			let weight = vertex_weights[slot];
			if weight <= 0.001 {
				continue;
			}
			let joint_index = vertex_joints[slot] as usize;
			let entry = summaries.entry(joint_index).or_insert((
				0,
				0,
				0.0,
				0.0,
				Vec3::ZERO,
				Vec3::splat(f32::INFINITY),
				Vec3::splat(f32::NEG_INFINITY),
			));
			entry.0 += 1;
			if dominant_joint == Some(joint_index) {
				entry.1 += 1;
			}
			entry.2 += weight;
			entry.3 = entry.3.max(weight);
			entry.4 += position;
			entry.5 = entry.5.min(position);
			entry.6 = entry.6.max(position);
		}
	}
	let mut out = summaries
		.into_iter()
		.map(
			|(joint_index, (vertex_count, dominant_vertex_count, weight_sum, max_weight, position_sum, bounds_min, bounds_max))| {
				let node_index = skin.joint_nodes.get(joint_index).copied();
				let name = node_index
					.and_then(|node| node_paths.get(node).cloned().flatten())
					.or_else(|| node_index.and_then(|node| scene.nodes.get(node).and_then(|node| node.name.clone())))
					.unwrap_or_else(|| format!("joint#{joint_index}"));
				let leaf = name.rsplit('/').next().unwrap_or(&name).to_string();
				let average_position = if vertex_count > 0 {
					position_sum / vertex_count as f32
				} else {
					Vec3::ZERO
				};
				DynamicsVertexProbeJointWeightSummary {
					joint: leaf,
					node_index,
					vertex_count,
					dominant_vertex_count,
					weight_sum,
					max_weight,
					average_weight: weight_sum / vertex_count.max(1) as f32,
					average_position: average_position.to_array(),
					bounds_min: bounds_min.to_array(),
					bounds_max: bounds_max.to_array(),
				}
			},
		)
		.collect::<Vec<_>>();
	out.sort_by(|a, b| {
		b.weight_sum
			.total_cmp(&a.weight_sum)
			.then_with(|| b.vertex_count.cmp(&a.vertex_count))
			.then_with(|| a.joint.cmp(&b.joint))
	});
	out
}

fn dynamics_vertex_probe_region(
	scene: &UnaSceneSnapshot,
	skin: Option<&un_avatar_core::UnaSkin>,
	primitive: &un_avatar_core::UnaMeshBuffers,
	node_paths: &[Option<String>],
	rest_positions: &[[f32; 3]],
	settled_positions: &[[f32; 3]],
	name: &str,
	predicate: fn([f32; 3]) -> bool,
) -> DynamicsVertexProbeRegionReport {
	let joints = primitive.joints.as_ref();
	let weights = primitive.weights.as_ref();
	let mut samples = Vec::new();
	let mut joint_counts = BTreeMap::<String, usize>::new();
	let mut delta_sum = Vec3::ZERO;
	let mut displacement_sum = 0.0;
	let mut max_displacement = 0.0_f32;
	for (index, (&rest, &settled)) in rest_positions.iter().zip(settled_positions.iter()).enumerate() {
		if !predicate(rest) {
			continue;
		}
		let delta = Vec3::from_array(settled) - Vec3::from_array(rest);
		let displacement = delta.length();
		delta_sum += delta;
		displacement_sum += displacement;
		max_displacement = max_displacement.max(displacement);
		let (dominant_joint, dominant_weight, influences) = dynamics_vertex_probe_influences(
			scene,
			skin,
			node_paths,
			joints.and_then(|j| j.get(index)),
			weights.and_then(|w| w.get(index)),
		);
		*joint_counts.entry(dominant_joint.clone()).or_default() += 1;
		samples.push(DynamicsVertexProbeVertexSample {
			vertex_index: index,
			position: rest,
			settled_position: settled,
			delta: delta.to_array(),
			displacement,
			dominant_joint,
			dominant_weight,
			influences,
		});
	}
	let vertex_count = samples.len();
	let denom = vertex_count.max(1) as f32;
	let mut least_moved_samples = samples.clone();
	least_moved_samples.sort_by(|a, b| a.displacement.total_cmp(&b.displacement));
	least_moved_samples.truncate(12);
	let mut most_moved_samples = samples;
	most_moved_samples.sort_by(|a, b| b.displacement.total_cmp(&a.displacement));
	most_moved_samples.truncate(12);
	let mut dominant_joints = joint_counts
		.into_iter()
		.map(|(joint, count)| DynamicsVertexProbeJointCount { joint, count })
		.collect::<Vec<_>>();
	dominant_joints.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.joint.cmp(&b.joint)));
	DynamicsVertexProbeRegionReport {
		name: name.to_string(),
		vertex_count,
		dominant_joints,
		morph_targets: dynamics_vertex_probe_region_morph_targets(primitive, rest_positions, predicate),
		average_displacement: displacement_sum / denom,
		max_displacement,
		average_delta: (delta_sum / denom).to_array(),
		least_moved_samples,
		most_moved_samples,
	}
}

fn dynamics_vertex_probe_region_morph_targets(
	primitive: &un_avatar_core::UnaMeshBuffers,
	rest_positions: &[[f32; 3]],
	predicate: fn([f32; 3]) -> bool,
) -> Vec<DynamicsVertexProbeMorphTargetRegionSummary> {
	const MAX_MORPH_TARGET_REGION_SAMPLES: usize = 12;
	let mut out = Vec::new();
	for (target_index, target) in primitive.morph_targets.iter().enumerate() {
		let name = primitive
			.morph_target_names
			.get(target_index)
			.cloned()
			.unwrap_or_else(|| format!("morph#{target_index}"));
		let mut affected_vertices = 0usize;
		let mut delta_sum = 0.0f32;
		let mut max_delta = 0.0f32;
		for (vertex_index, &rest) in rest_positions.iter().enumerate() {
			if !predicate(rest) {
				continue;
			}
			let delta = target
				.position_deltas
				.get(vertex_index)
				.map(|delta| Vec3::from_array(*delta).length())
				.unwrap_or(0.0);
			if delta <= 0.000001 {
				continue;
			}
			affected_vertices += 1;
			delta_sum += delta;
			max_delta = max_delta.max(delta);
		}
		out.push(DynamicsVertexProbeMorphTargetRegionSummary {
			index: target_index,
			name,
			default_weight: primitive.default_morph_weights.get(target_index).copied().unwrap_or(0.0),
			affected_vertices,
			average_delta: if affected_vertices > 0 {
				delta_sum / affected_vertices as f32
			} else {
				0.0
			},
			max_delta,
		});
	}
	out.sort_by(|a, b| {
		b.affected_vertices
			.cmp(&a.affected_vertices)
			.then_with(|| b.max_delta.total_cmp(&a.max_delta))
			.then_with(|| b.average_delta.total_cmp(&a.average_delta))
			.then_with(|| a.index.cmp(&b.index))
	});
	out.truncate(MAX_MORPH_TARGET_REGION_SAMPLES);
	out
}

fn dynamics_vertex_probe_influences(
	scene: &UnaSceneSnapshot,
	skin: Option<&un_avatar_core::UnaSkin>,
	node_paths: &[Option<String>],
	joints: Option<&[u16; 4]>,
	weights: Option<&[f32; 4]>,
) -> (String, f32, Vec<DynamicsVertexProbeInfluence>) {
	let Some(skin) = skin else {
		return ("<rigid>".to_string(), 1.0, Vec::new());
	};
	let joints = joints.copied().unwrap_or([0; 4]);
	let weights = weights.copied().unwrap_or([1.0, 0.0, 0.0, 0.0]);
	let mut dominant = ("<none>".to_string(), 0.0_f32);
	let mut influences = Vec::new();
	for slot in 0..4 {
		let weight = weights[slot];
		if weight <= 0.001 {
			continue;
		}
		let joint_index = joints[slot] as usize;
		let node = skin.joint_nodes.get(joint_index).copied();
		let name = node
			.and_then(|node| node_paths.get(node).cloned().flatten())
			.or_else(|| node.and_then(|node| scene.nodes.get(node).and_then(|node| node.name.clone())))
			.unwrap_or_else(|| format!("joint#{joint_index}"));
		let leaf = name.rsplit('/').next().unwrap_or(&name).to_string();
		if weight > dominant.1 {
			dominant = (leaf.clone(), weight);
		}
		influences.push(DynamicsVertexProbeInfluence { joint: leaf, weight });
	}
	(dominant.0, dominant.1, influences)
}

fn run_dynamics_motion_trace_audit(
	plugin_dirs: &[PathBuf],
	path: PathBuf,
	input_format: Option<String>,
	wardrobe_set: Option<String>,
	require_motion_evidence: bool,
	frames: usize,
	recovery_frames: Option<usize>,
	tuning: &str,
	json: bool,
) -> Result<(), String> {
	let report = dynamics_motion_trace_report(
		plugin_dirs,
		&path,
		input_format.as_deref(),
		wardrobe_set.as_deref(),
		frames,
		recovery_frames,
		tuning,
	)?;
	let required = if require_motion_evidence {
		require_dynamics_motion_trace_evidence(&report)
	} else {
		Ok(())
	};
	if json {
		write_json_stdout(&report)?;
		return required;
	}
	println!("path: {}", report.path);
	let plug = report
		.import_provider_plugin_id
		.as_ref()
		.map(|p| format!(" ({p})"))
		.unwrap_or_default();
	println!("importer: {}{}", report.import_format_id, plug);
	if let Some(set_id) = &report.active_wardrobe_set {
		println!("active_wardrobe_set: {set_id}");
	}
	println!(
		"motion_trace: frames={} recovery_frames={} tuning={} groups={} joints={}",
		report.frame_count, report.recovery_frame_count, report.tuning, report.group_count, report.joint_count
	);
	for category in report.categories.iter().take(DIAGNOSE_TEXT_LIST_LIMIT) {
		println!(
			"  motion_category[{}]: groups={} joints={} visualGroups={} nonvisualGroups={} skinnedJoints={} meshSubtrees={} max_lag={} avg_lag={} final_lag={} recovery_lag={} recovery_ratio={} initial_stable_offset={} settled_lag={} stable_offset={} stable_ratio={} recovery_state={} settled_ratio={} residual={} residual_chain={} half_life_frames={} response=rest:{}/shape:{}/bounce:{}/follow:{}/orient:{}/stretch:{}/stretchMotion:{}",
			category.category,
			category.group_count,
			category.joint_count,
			category.visual_target_group_count,
			category.nonvisual_group_count,
			category.visible_skinned_joint_count,
			category.visible_mesh_subtree_node_count,
			category.max_lag,
			category.average_lag,
			category.final_lag,
			category.recovery_final_lag,
			category.recovery_ratio,
			category.initial_stable_offset,
			category.settled_recovery_lag,
			category.stable_offset,
			category.stable_offset_ratio,
			category.recovery_state,
			category.settled_recovery_ratio,
			category.residual_motion,
			category.residual_motion_chain_ratio,
			category
				.recovery_half_life_frames
				.map(|value| value.to_string())
				.unwrap_or_else(|| "none".to_string()),
			category.average_rest_response,
			category.average_shape_preservation,
			category.average_bounce_response,
			category.average_parent_motion_follow,
			category.average_orientation_follow,
			category.average_max_stretch_response,
			category.average_stretch_motion_response
		);
	}
	print_omitted_text_items("motion categories", report.categories.len(), DIAGNOSE_TEXT_LIST_LIMIT);
	for group in report.groups.iter().take(DIAGNOSE_TEXT_LIST_LIMIT) {
		println!(
			"  motion_group[{}]: category={} joints={} visual={} skinnedJoints={} meshSubtrees={} interactionMetadataOnly={} max_lag={} avg_lag={} final_lag={} recovery_lag={} recovery_ratio={} initial_stable_offset={} settled_lag={} stable_offset={} stable_ratio={} recovery_state={} settled_ratio={} residual={} residual_chain={} half_life_frames={} response=rest:{}/shape:{}/bounce:{}/follow:{}/orient:{}/stretch:{}/stretchMotion:{}",
			group.source_id,
			group.category,
			group.joint_count,
			group.visual_target,
			group.skinned_joint_count,
			group.mesh_subtree_node_count,
			group.interaction_metadata_only,
			group.max_lag,
			group.average_lag,
			group.final_lag,
			group.recovery_final_lag,
			group.recovery_ratio,
			group.initial_stable_offset,
			group.settled_recovery_lag,
			group.stable_offset,
			group.stable_offset_ratio,
			group.recovery_state,
			group.settled_recovery_ratio,
			group.residual_motion,
			group.residual_motion_chain_ratio,
			group
				.recovery_half_life_frames
				.map(|value| value.to_string())
				.unwrap_or_else(|| "none".to_string()),
			group.average_rest_response,
			group.average_shape_preservation,
			group.average_bounce_response,
			group.average_parent_motion_follow,
			group.average_orientation_follow,
			group.average_max_stretch_response,
			group.average_stretch_motion_response
		);
	}
	print_omitted_text_items("motion groups", report.groups.len(), DIAGNOSE_TEXT_LIST_LIMIT);
	for finding in report.findings.iter().take(DIAGNOSE_TEXT_LIST_LIMIT) {
		println!("motion_finding: {finding}");
	}
	print_omitted_text_items("motion findings", report.findings.len(), DIAGNOSE_TEXT_LIST_LIMIT);
	if !report.finding_kind_counts.is_empty() {
		println!("motion_finding_kind_counts: {:?}", report.finding_kind_counts);
	}
	for finding in report.finding_details.iter().take(DIAGNOSE_TEXT_LIST_LIMIT) {
		if let Some(hint) = &finding.response_override_hint {
			println!(
				"motion_finding_response_hint[{}]: source_id={} rest_response={:?} damping_half_life_ms={:?} stretch_range_scale={:?} stretch_motion={:?}",
				finding.kind,
				hint.source_id,
				hint.rest_response,
				hint.damping_half_life_ms,
				hint.stretch_range_scale,
				hint.stretch_motion
			);
		}
	}
	print_omitted_text_items(
		"motion finding response hints",
		report
			.finding_details
			.iter()
			.filter(|finding| finding.response_override_hint.is_some())
			.count(),
		DIAGNOSE_TEXT_LIST_LIMIT,
	);
	if !report.missing_motion_evidence.is_empty() {
		println!("missing motion evidence: {}", report.missing_motion_evidence.join(", "));
	}
	required
}

fn eye_like_material_name(name: Option<&str>) -> bool {
	let Some(n) = name else {
		return false;
	};
	let l = n.to_ascii_lowercase();
	l.contains("iris")
		|| l.contains("pupil")
		|| l.contains("eyeball")
		|| l.contains("cornea")
		|| l.contains("sight")
		|| l.contains("eyelid")
		|| l.contains("eyelash")
		|| l.contains("eyeline")
		|| l.contains("eyeliner")
		|| l.contains("eyebrow")
		|| l.contains("brow")
		|| l.contains("lash")
		|| l.contains("lid")
		|| l.contains("瞳")
		|| l.contains("虹彩")
		|| l.contains("虹膜")
		|| l.contains("目玉")
		|| l.contains("眼睛")
		|| l.contains("眼球")
		|| l.contains("眼珠")
		|| l.contains("眼白")
		|| l.contains("瞼")
		|| l.contains("まぶた")
		|| l.contains("まつげ")
		|| l.contains("睫")
		|| l.contains("眉")
		|| l.contains("眼睑")
		|| l.contains("眼瞼")
		|| l.contains("眼皮")
		|| l.contains("アイライン")
		|| l.contains("アイラッシュ")
		|| l.contains("eye")
		|| l.contains("highlight")
		|| l.contains("ハイライト")
		|| l.contains("高光")
}

fn bump_count(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
	*map.entry(key.into()).or_insert(0) += 1;
}

fn pixel_format_has_alpha(format: UnaImagePixelFormat) -> bool {
	matches!(
		format,
		UnaImagePixelFormat::R8G8
			| UnaImagePixelFormat::R8G8B8A8
			| UnaImagePixelFormat::R16G16B16A16
			| UnaImagePixelFormat::R16G16B16A16Float
			| UnaImagePixelFormat::R32G32B32A32Float
	)
}

fn texture_alpha_summary(scene: &UnaSceneSnapshot, image_index: Option<usize>) -> Option<DiagnoseTextureAlphaSummary> {
	let image_index = image_index?;
	let image = scene.images.get(image_index)?;
	let pixels = image.rgba8_compat_pixels();
	let mut min_alpha = u8::MAX;
	let mut max_alpha = u8::MIN;
	let mut transparent_pixels = 0usize;
	let mut translucent_pixels = 0usize;
	let mut opaque_pixels = 0usize;
	for pixel in pixels.chunks_exact(4) {
		let alpha = pixel[3];
		min_alpha = min_alpha.min(alpha);
		max_alpha = max_alpha.max(alpha);
		match alpha {
			0 => transparent_pixels += 1,
			255 => opaque_pixels += 1,
			_ => translucent_pixels += 1,
		}
	}
	let total_pixels = transparent_pixels + translucent_pixels + opaque_pixels;
	let coverage = if total_pixels == 0 {
		0.0
	} else {
		(opaque_pixels as f32 + translucent_pixels as f32) / total_pixels as f32
	};
	Some(DiagnoseTextureAlphaSummary {
		image: image_index,
		width: image.width,
		height: image.height,
		pixel_format: image.pixel_format,
		has_alpha_channel: pixel_format_has_alpha(image.pixel_format),
		min_alpha: if total_pixels == 0 { 0 } else { min_alpha },
		max_alpha: if total_pixels == 0 { 0 } else { max_alpha },
		transparent_pixels,
		translucent_pixels,
		opaque_pixels,
		coverage,
	})
}

fn texture_alpha_summary_cached(
	scene: &UnaSceneSnapshot,
	cache: &mut BTreeMap<usize, Option<DiagnoseTextureAlphaSummary>>,
	image_index: Option<usize>,
) -> Option<DiagnoseTextureAlphaSummary> {
	let image_index = image_index?;
	if let Some(summary) = cache.get(&image_index) {
		return summary.clone();
	}
	let summary = texture_alpha_summary(scene, Some(image_index));
	cache.insert(image_index, summary.clone());
	summary
}

fn material_source_shader_is_liltoon(material: &UnaMaterialPbr) -> bool {
	material
		.unavatar_material
		.as_ref()
		.and_then(|m| m.get("sourceShader"))
		.and_then(|v| v.as_str())
		.is_some_and(|shader| shader.to_ascii_lowercase().contains("liltoon"))
		|| material
			.unavatar_material
			.as_ref()
			.and_then(|m| m.get("family"))
			.and_then(|v| v.as_str())
			.is_some_and(|family| family.eq_ignore_ascii_case("liltoon"))
}

fn material_has_source_params(material: &UnaMaterialPbr) -> bool {
	material_source_param_count(material, "floatParams") > 0 || material_source_param_count(material, "colorParams") > 0
}

fn material_source_param_count(material: &UnaMaterialPbr, key: &str) -> usize {
	material
		.unavatar_material
		.as_ref()
		.and_then(|m| m.get(key).or_else(|| m.get(&key.replace("Params", "_params"))))
		.and_then(|v| v.as_object())
		.map_or(0, |params| params.len())
}

fn material_source_float_param(material: &UnaMaterialPbr, name: &str) -> Option<f32> {
	material
		.unavatar_material
		.as_ref()
		.and_then(|m| m.get("floatParams").or_else(|| m.get("float_params")))
		.and_then(|params| params.get(name))
		.and_then(|value| value.as_f64())
		.map(|value| value as f32)
}

fn material_enabled_keywords(material: &UnaMaterialPbr) -> Vec<String> {
	const KEYWORD_FIELDS: &[&str] = &[
		"enabledKeywords",
		"enabled_keywords",
		"shaderKeywords",
		"shader_keywords",
		"keywords",
	];
	let Some(extras) = material.unavatar_material.as_ref() else {
		return Vec::new();
	};
	let mut out = BTreeSet::new();
	for field in KEYWORD_FIELDS {
		match extras.get(*field) {
			Some(serde_json::Value::Array(values)) => {
				for value in values {
					if let Some(keyword) = value.as_str().filter(|keyword| !keyword.is_empty()) {
						out.insert(keyword.to_string());
					}
				}
			}
			Some(serde_json::Value::Object(values)) => {
				for (keyword, value) in values {
					if value.as_bool().unwrap_or(false) && !keyword.is_empty() {
						out.insert(keyword.to_string());
					}
				}
			}
			_ => {}
		}
	}
	out.into_iter().collect()
}

fn material_keyword_contains(material: &UnaMaterialPbr, needle: &str) -> bool {
	let needle = needle.to_ascii_lowercase();
	material_enabled_keywords(material)
		.iter()
		.any(|keyword| keyword.to_ascii_lowercase().contains(&needle))
}

fn material_source_shader_lower(material: &UnaMaterialPbr) -> String {
	material
		.unavatar_material
		.as_ref()
		.and_then(|m| m.get("sourceShader"))
		.and_then(|v| v.as_str())
		.unwrap_or("")
		.to_ascii_lowercase()
}

fn material_liltoon_features(material: &UnaMaterialPbr) -> Vec<String> {
	if !material_source_shader_is_liltoon(material) {
		return Vec::new();
	}
	let shader = material_source_shader_lower(material);
	let mut features = BTreeSet::new();
	if shader.contains("lite") {
		features.insert("lite".to_string());
	}
	if shader.contains("cutout") || matches!(material.alpha_mode, UnaAlphaMode::Mask) {
		features.insert("cutout".to_string());
	}
	if shader.contains("transparent") || matches!(material.alpha_mode, UnaAlphaMode::Blend) {
		features.insert("transparent".to_string());
	}
	if shader.contains("twopass") || shader.contains("two_pass") || material_source_float_param(material, "_PreZWrite").is_some() {
		features.insert("twopass".to_string());
	}
	if shader.contains("outline") || material_source_float_param(material, "_UseOutline").is_some_and(|value| value > 0.5) {
		features.insert("outline".to_string());
	}
	if shader.contains("fur")
		|| material_keyword_contains(material, "fur")
		|| material_source_float_param(material, "_UseFur").is_some_and(|value| value > 0.5)
	{
		features.insert("fur".to_string());
	}
	if shader.contains("refraction")
		|| material_keyword_contains(material, "refraction")
		|| material_source_float_param(material, "_UseRefraction").is_some_and(|value| value > 0.5)
	{
		features.insert("refraction".to_string());
	}
	if shader.contains("gem") {
		features.insert("gem".to_string());
	}
	if material_keyword_contains(material, "alphamask")
		|| material_source_float_param(material, "_AlphaMaskMode").is_some_and(|value| value > 0.5)
	{
		features.insert("alpha_mask".to_string());
	}
	if features.is_empty() {
		features.insert("common".to_string());
	}
	features.into_iter().collect()
}

fn material_transparent_with_z_write(material: &UnaMaterialPbr) -> bool {
	if material.liltoon_like_runtime().is_some() {
		if let Some(value) =
			material_source_float_param(material, "_ZWrite").or_else(|| material_source_float_param(material, "_ZWriteMode"))
		{
			return value > 0.5;
		}
		return material_source_shader_lower(material).contains("twopass");
	}
	material.mtoon_source_profile().is_some_and(|mtoon| mtoon.transparent_with_z_write)
}

fn material_render_float_params(material: &UnaMaterialPbr) -> BTreeMap<String, f32> {
	const PARAMS: &[&str] = &[
		"_TransparentMode",
		"_AlphaMode",
		"_BlendMode",
		"_Cutoff",
		"_SubpassCutoff",
		"_SrcBlend",
		"_DstBlend",
		"_SrcBlendAlpha",
		"_DstBlendAlpha",
		"_ZWrite",
		"_PreZWrite",
		"_Cull",
		"_PreCull",
		"_ColorMask",
		"_PreColorMask",
		"_AlphaToMask",
		"_PreAlphaToMask",
	];
	PARAMS
		.iter()
		.filter_map(|name| material_source_float_param(material, name).map(|value| ((*name).to_string(), value)))
		.collect()
}

fn material_summary(
	index: usize,
	material: &UnaMaterialPbr,
	scene: &UnaSceneSnapshot,
	alpha_cache: &mut BTreeMap<usize, Option<DiagnoseTextureAlphaSummary>>,
) -> DiagnoseMaterialSummary {
	let source_shader = material
		.unavatar_material
		.as_ref()
		.and_then(|m| m.get("sourceShader"))
		.and_then(|v| v.as_str())
		.map(str::to_owned);
	let material_family = material
		.unavatar_material
		.as_ref()
		.and_then(|m| m.get("family"))
		.and_then(|v| v.as_str())
		.map(str::to_owned);
	let render_queue = material
		.unavatar_material
		.as_ref()
		.and_then(|m| m.get("renderQueue"))
		.and_then(|v| v.as_i64())
		.map(|v| v as i32);
	let liltoon_features = material_liltoon_features(material);
	let mtoon = material.mtoon_source_profile().map(|m| DiagnoseMToonSummary {
		transparent_with_z_write: m.transparent_with_z_write,
		shade_color_factor: m.shade_color_factor,
		shade_multiply_texture_index: m.shade_multiply_texture_index,
		shading_shift_factor: m.shading_shift_factor,
		shading_shift_texture_index: m.shading_shift_texture_index,
		shading_toony_factor: m.shading_toony_factor,
		gi_equalization_factor: m.gi_equalization_factor,
		matcap_factor: m.matcap_factor,
		matcap_texture_index: m.matcap_texture_index,
		parametric_rim_color_factor: m.parametric_rim_color_factor,
		rim_multiply_texture_index: m.rim_multiply_texture_index,
		reflection_cube_texture_index: m.reflection_cube_texture_index,
		outline_width_mode: m.outline_width_mode,
		outline_width_factor: m.outline_width_factor,
		outline_width_multiply_texture_index: m.outline_width_multiply_texture_index,
		outline_color_factor: m.outline_color_factor,
		emissive_factor: material.emissive_factor,
		emissive_texture_index: material.emissive_texture_index,
	});
	DiagnoseMaterialSummary {
		index,
		name: material.name.clone(),
		source_shader,
		material_family,
		render_queue,
		source_float_param_count: material_source_param_count(material, "floatParams"),
		source_color_param_count: material_source_param_count(material, "colorParams"),
		source_render_float_params: material_render_float_params(material),
		liltoon_features,
		shading: material.shading,
		alpha_mode: material.alpha_mode,
		alpha_cutoff: material.alpha_cutoff,
		double_sided: material.double_sided,
		cull_mode: material.cull_mode,
		base_color_factor: material.base_color_factor,
		base_color_texture_index: material.base_color_texture_index,
		base_color_texture_alpha: texture_alpha_summary_cached(scene, alpha_cache, material.base_color_texture_index),
		normal_texture_index: material.normal_texture_index,
		normal_texture_scale: material.normal_texture_scale,
		eye_like_name: eye_like_material_name(material.name.as_deref()),
		mtoon,
	}
}

fn scene_node_paths_by_index(scene: &un_avatar_core::UnaSceneSnapshot) -> Vec<Option<String>> {
	fn visit(scene: &un_avatar_core::UnaSceneSnapshot, idx: usize, parent: &str, out: &mut [Option<String>]) {
		let Some(node) = scene.nodes.get(idx) else { return };
		let segment = node.name.as_deref().unwrap_or("");
		let path = if parent.is_empty() {
			segment.to_string()
		} else if segment.is_empty() {
			parent.to_string()
		} else {
			format!("{parent}/{segment}")
		};
		if let Some(slot) = out.get_mut(idx) {
			*slot = (!path.is_empty()).then_some(path.clone());
		}
		for &child in &node.children {
			visit(scene, child, &path, out);
		}
	}

	let mut out = vec![None; scene.nodes.len()];
	for &root in &scene.roots {
		visit(scene, root, "", &mut out);
	}
	out
}

fn scene_parent_indices(scene: &un_avatar_core::UnaSceneSnapshot) -> Vec<Option<usize>> {
	let mut parents = vec![None; scene.nodes.len()];
	for (parent, node) in scene.nodes.iter().enumerate() {
		for &child in &node.children {
			if let Some(slot) = parents.get_mut(child) {
				*slot = Some(parent);
			}
		}
	}
	parents
}

fn scene_effective_visibility(scene: &un_avatar_core::UnaSceneSnapshot) -> Vec<bool> {
	fn visit(scene: &un_avatar_core::UnaSceneSnapshot, idx: usize, parent_visible: bool, out: &mut [bool]) {
		let Some(node) = scene.nodes.get(idx) else { return };
		let visible = parent_visible && node.visible;
		if let Some(slot) = out.get_mut(idx) {
			*slot = visible;
		}
		for &child in &node.children {
			visit(scene, child, visible, out);
		}
	}

	let mut out = vec![false; scene.nodes.len()];
	for &root in &scene.roots {
		visit(scene, root, true, &mut out);
	}
	out
}

fn dynamics_selected_runtime_colliders<'a>(
	runtime_colliders: &'a [&'a un_avatar_core::UnaDynamicsCollider],
	all_runtime_colliders_global: bool,
	group_source_id: &str,
) -> Vec<&'a un_avatar_core::UnaDynamicsCollider> {
	runtime_colliders
		.iter()
		.copied()
		.filter(|collider| {
			all_runtime_colliders_global
				|| collider.source_id.is_empty()
				|| (!group_source_id.is_empty() && collider.source_id == group_source_id)
		})
		.collect()
}

fn dynamics_group_gravity_target_summary(
	scene: &UnaSceneSnapshot,
	world: &[Mat4],
	group: &un_avatar_core::UnaSpringBoneGroup,
) -> (f32, f32) {
	if group.gravity_power.abs() <= f32::EPSILON {
		return (0.0, 0.0);
	}
	let gravity_dir = Vec3::from_array(group.gravity_dir).try_normalize().unwrap_or(Vec3::NEG_Y);
	if gravity_dir.length_squared() < 1e-12 || group.bone_node_indices.len() < 2 {
		return (0.0, 0.0);
	}
	let mut max_angle = 0.0_f32;
	let mut max_amount = 0.0_f32;
	for segment in group.bone_node_indices.windows(2) {
		let parent = segment[0];
		let child = segment[1];
		let Some(parent_world) = world.get(parent).copied() else {
			continue;
		};
		let Some(child_node) = scene.nodes.get(child) else {
			continue;
		};
		let (_, parent_rot, _) = parent_world.to_scale_rotation_translation();
		let local_child = Mat4::from_cols_array(&child_node.transform);
		let (_, _, child_local_translation) = local_child.to_scale_rotation_translation();
		let Some(axis) = (parent_rot.normalize() * child_local_translation).try_normalize() else {
			continue;
		};
		let gravity_amount = dynamics_gravity_falloff_amount(gravity_dir, group.gravity_power.abs(), axis, group.gravity_falloff);
		let Some(gravity_axis) = axis
			.lerp(gravity_dir * group.gravity_power.signum(), gravity_amount)
			.try_normalize()
		else {
			continue;
		};
		max_angle = max_angle.max(axis.angle_between(gravity_axis).to_degrees());
		max_amount = max_amount.max(gravity_amount);
	}
	(max_angle, max_amount)
}

fn dynamics_gravity_falloff_amount(gravity_dir: Vec3, gravity_power: f32, target_axis_world: Vec3, gravity_falloff: f32) -> f32 {
	let gravity_dir = gravity_dir.normalize_or_zero();
	let target_axis_world = target_axis_world.normalize_or_zero();
	if gravity_dir.length_squared() < 1e-12 || target_axis_world.length_squared() < 1e-12 {
		return gravity_power.clamp(0.0, 1.0);
	}
	let perpendicularity = (1.0 - target_axis_world.dot(gravity_dir)).clamp(0.0, 1.0);
	let falloff_min = 1.0 - gravity_falloff.clamp(0.0, 1.0);
	let falloff = falloff_min + (1.0 - falloff_min) * perpendicularity;
	(gravity_power * falloff).clamp(0.0, 1.0)
}

fn dynamics_group_summaries(doc: &UnaDocument) -> Vec<DiagnoseDynamicsGroupSummary> {
	let runtime_model = doc.runtime_model();
	let Some(runtime) = runtime_model.scene_profile_dynamics() else {
		return Vec::new();
	};
	let world_matrices = cli_scene_world_matrices(runtime.scene);
	let groups = runtime.dynamics.groups();
	if groups.is_empty() {
		return Vec::new();
	}
	let node_paths_by_index = scene_node_paths_by_index(runtime.scene);
	let categories = DynamicsPhysicsConfig::default().categories;
	let runtime_colliders = runtime.dynamics.colliders().collect::<Vec<_>>();
	let mut collider_counts_by_source_id = BTreeMap::<String, usize>::new();
	for collider in &runtime_colliders {
		if !collider.source_id.is_empty() {
			*collider_counts_by_source_id.entry(collider.source_id.clone()).or_default() += 1;
		}
	}
	let all_runtime_colliders_global = runtime_colliders.iter().all(|collider| collider.source_id.is_empty());
	let source_collider_summaries = dynamics_source_collider_summaries_by_source_id(doc);
	groups
		.iter()
		.enumerate()
		.map(|(index, group)| {
			let root_node = group.bone_node_indices.first().copied();
			let tip_node = group.bone_node_indices.last().copied();
			let (hit_radius_sample_count, hit_radius_sample_min, hit_radius_sample_max) =
				dynamics_hit_radius_sample_summary(&group.hit_radius_samples);
			let resident = runtime.dynamics.source_id_resident_in_scene(runtime.scene, &group.source_id);
			let source_collider_summary = source_collider_summaries.get(&group.source_id).cloned().unwrap_or_default();
			let selected_runtime_colliders =
				dynamics_selected_runtime_colliders(&runtime_colliders, all_runtime_colliders_global, &group.source_id);
			let (gravity_target_max_angle_deg, gravity_target_max_amount) =
				dynamics_group_gravity_target_summary(runtime.scene, &world_matrices, group);
			DiagnoseDynamicsGroupSummary {
				index,
				source_kind: group.source_kind,
				enabled: resident && runtime.dynamics.group_enabled(group),
				source_enabled: group.enabled,
				runtime_enabled_override: runtime.dynamics.group_enabled_override(group),
				source_id: group.source_id.clone(),
				comment: group.comment.clone(),
				category: runtime
					.dynamics
					.dynamics_group(index)
					.map(|group| classify_dynamics_group_category(runtime.scene, group, &categories))
					.unwrap_or_default(),
				bone_count: group.bone_node_indices.len(),
				source_component_path: source_collider_summary.component_path.clone(),
				source_root_paths: source_collider_summary.root_paths.clone(),
				source_allow_collision: source_collider_summary.allow_collision,
				source_collider_count: source_collider_summary.collider_count,
				source_unknown_shape_collider_count: source_collider_summary.unknown_shape_collider_count,
				source_inside_bounds_collider_count: source_collider_summary.inside_bounds_collider_count,
				source_collider_paths: source_collider_summary.collider_paths.clone(),
				runtime_collider_count: collider_counts_by_source_id.get(&group.source_id).copied().unwrap_or_default(),
				selected_runtime_collider_count: selected_runtime_colliders.len(),
				selected_global_collider_count: selected_runtime_colliders
					.iter()
					.filter(|collider| collider.source_id.is_empty())
					.count(),
				selected_authored_collider_count: selected_runtime_colliders
					.iter()
					.filter(|collider| !collider.source_id.is_empty())
					.count(),
				selected_runtime_collider_paths: selected_runtime_colliders
					.iter()
					.filter_map(|collider| (!collider.collider_path.is_empty()).then(|| collider.collider_path.clone()))
					.take(16)
					.collect(),
				root_node,
				root_path: root_node.and_then(|node| node_paths_by_index.get(node).cloned().flatten()),
				tip_node,
				tip_path: tip_node.and_then(|node| node_paths_by_index.get(node).cloned().flatten()),
				center_node: group.center_node,
				center_path: group.center_node.and_then(|node| node_paths_by_index.get(node).cloned().flatten()),
				stiffness: group.stiffness,
				pull: group.pull,
				spring: group.spring,
				integration_type: group.integration_type,
				drag_force: group.drag_force,
				gravity_power: group.gravity_power,
				gravity_falloff: group.gravity_falloff,
				immobile: group.immobile,
				immobile_type: group.immobile_type,
				gravity_dir: group.gravity_dir,
				gravity_target_max_angle_deg,
				gravity_target_max_amount,
				limit_type: group
					.limit
					.as_ref()
					.and_then(|limit| (!limit.limit_type.is_empty()).then(|| limit.limit_type.clone())),
				limit_rotation: group.limit.as_ref().map(|limit| limit.limit_rotation),
				max_angle_x: group.limit.as_ref().map(|limit| limit.max_angle_x),
				max_angle_z: group.limit.as_ref().map(|limit| limit.max_angle_z),
				max_stretch: group.limit.as_ref().map(|limit| limit.max_stretch),
				max_squish: group.limit.as_ref().map(|limit| limit.max_squish),
				stretch_motion: group.limit.as_ref().and_then(|limit| limit.stretch_motion),
				max_stretch_sample_has_positive: group
					.limit
					.as_ref()
					.is_some_and(|limit| dynamics_limit_samples_have_positive(&limit.max_stretch_samples)),
				max_squish_sample_has_positive: group
					.limit
					.as_ref()
					.is_some_and(|limit| dynamics_limit_samples_have_positive(&limit.max_squish_samples)),
				stretch_motion_sample_has_positive: group
					.limit
					.as_ref()
					.is_some_and(|limit| dynamics_limit_samples_have_positive(&limit.stretch_motion_samples)),
				writeback_mode: group.writeback_mode,
				translation_writeback_candidate_count: una_dynamics_translation_writeback_candidate_count(
					runtime.scene,
					group.writeback_mode,
					&group.bone_node_indices,
				),
				translation_writeback_target_count: una_dynamics_translation_writeback_target_count(
					runtime.scene,
					group.writeback_mode,
					&group.bone_node_indices,
				),
				allow_grabbing: group.interaction.as_ref().and_then(|interaction| interaction.allow_grabbing),
				allow_posing: group.interaction.as_ref().and_then(|interaction| interaction.allow_posing),
				interaction_parameter: group
					.interaction
					.as_ref()
					.map(|interaction| interaction.parameter.clone())
					.unwrap_or_default(),
				hit_radius: group.hit_radius,
				hit_radius_sample_count,
				hit_radius_sample_min,
				hit_radius_sample_max,
			}
		})
		.collect()
}

fn dynamics_response_category_summaries(doc: &UnaDocument) -> Vec<DynamicsResponseCategorySummary> {
	let runtime_model = doc.runtime_model();
	let Some(runtime) = runtime_model.scene_profile_dynamics() else {
		return Vec::new();
	};
	DynamicsSimulator::new_with_runtime_dynamics_and_collider_sources(
		runtime.scene,
		runtime.dynamics,
		Vec::new(),
		DynamicsPhysicsConfig::default(),
	)
	.map(|sim| sim.response_category_summaries())
	.unwrap_or_default()
}

fn dynamics_response_group_summaries(doc: &UnaDocument) -> Vec<DynamicsResponseGroupSummary> {
	let runtime_model = doc.runtime_model();
	let Some(runtime) = runtime_model.scene_profile_dynamics() else {
		return Vec::new();
	};
	let mut groups = DynamicsSimulator::new_with_runtime_dynamics_and_collider_sources(
		runtime.scene,
		runtime.dynamics,
		Vec::new(),
		DynamicsPhysicsConfig::default(),
	)
	.map(|sim| sim.response_group_summaries())
	.unwrap_or_default();
	annotate_dynamics_response_group_visibility(&mut groups, runtime.scene, runtime.dynamics);
	groups
}

fn dynamics_interaction_hook_summaries(doc: &UnaDocument) -> Vec<DiagnoseDynamicsInteractionHookSummary> {
	let runtime_model = doc.runtime_model();
	let Some(runtime) = runtime_model.scene_profile_dynamics() else {
		return Vec::new();
	};
	let groups = runtime.dynamics.groups();
	if groups.is_empty() {
		return Vec::new();
	}
	let node_paths_by_index = scene_node_paths_by_index(runtime.scene);
	groups
		.iter()
		.enumerate()
		.filter_map(|(group_index, group)| {
			let interaction = group.interaction.as_ref()?;
			let allow_grabbing = interaction.allow_grabbing.unwrap_or(false);
			let allow_posing = interaction.allow_posing.unwrap_or(false);
			if !allow_grabbing && !allow_posing && interaction.parameter.is_empty() {
				return None;
			}
			let suffix_parameters = if interaction.parameter.is_empty() {
				Vec::new()
			} else {
				un_avatar_core::UNA_PHYSBONE_PARAMETER_SUFFIXES
					.iter()
					.map(|suffix| format!("{}{}", interaction.parameter, suffix))
					.collect()
			};
			let root_node = group.bone_node_indices.first().copied();
			Some(DiagnoseDynamicsInteractionHookSummary {
				group_index,
				source_kind: group.source_kind,
				enabled: runtime.dynamics.group_enabled(group),
				source_enabled: group.enabled,
				source_id: group.source_id.clone(),
				root_path: root_node.and_then(|node| node_paths_by_index.get(node).cloned().flatten()),
				allow_grabbing,
				allow_posing,
				parameter: interaction.parameter.clone(),
				suffix_parameters,
				metadata_only: interaction.parameter.is_empty(),
			})
		})
		.collect()
}

fn dynamics_collider_summaries(doc: &UnaDocument) -> Vec<DiagnoseDynamicsColliderSummary> {
	let runtime_model = doc.runtime_model();
	let Some(runtime) = runtime_model.scene_profile_dynamics() else {
		return Vec::new();
	};
	let node_paths_by_index = scene_node_paths_by_index(runtime.scene);
	runtime
		.dynamics
		.colliders()
		.enumerate()
		.map(|(index, collider)| DiagnoseDynamicsColliderSummary {
			index,
			source_kind: collider.source_kind,
			source_id: collider.source_id.clone(),
			collider_path: collider.collider_path.clone(),
			node: collider.node,
			node_path: node_paths_by_index.get(collider.node).cloned().flatten(),
			shape: collider.shape.clone(),
			radius: collider.radius,
			height: collider.height,
			position: collider.position,
			rotation: collider.rotation,
			inside_bounds: collider.inside_bounds,
		})
		.collect()
}

fn dynamics_stretch_limit_samples(groups: &[DiagnoseDynamicsGroupSummary]) -> Vec<String> {
	groups
		.iter()
		.filter(|group| dynamics_group_has_length_limit(group))
		.take(4)
		.map(|group| {
			let id = if group.source_id.is_empty() {
				format!("group[{}]", group.index)
			} else {
				group.source_id.clone()
			};
			match &group.root_path {
				Some(root_path) => format!("{id}@{root_path}"),
				None => id,
			}
		})
		.collect()
}

fn dynamics_group_has_length_limit(group: &DiagnoseDynamicsGroupSummary) -> bool {
	let stretch_motion = group
		.stretch_motion
		.filter(|value| value.is_finite())
		.unwrap_or(1.0)
		.clamp(0.0, 1.0);
	let has_length_range = group
		.max_stretch
		.is_some_and(|max_stretch| max_stretch.is_finite() && max_stretch > 0.0)
		|| group
			.max_squish
			.is_some_and(|max_squish| max_squish.is_finite() && max_squish > 0.0)
		|| group.max_stretch_sample_has_positive
		|| group.max_squish_sample_has_positive;
	let has_motion = stretch_motion > 0.0 || group.stretch_motion_sample_has_positive;
	has_motion && has_length_range
}

fn dynamics_effective_stretch_motion(group: &DiagnoseDynamicsGroupSummary) -> f32 {
	group
		.stretch_motion
		.filter(|value| value.is_finite())
		.unwrap_or(1.0)
		.clamp(0.0, 1.0)
}

fn dynamics_large_stretch_range_groups(groups: &[DiagnoseDynamicsGroupSummary]) -> Vec<&DiagnoseDynamicsGroupSummary> {
	groups
		.iter()
		.filter(|group| {
			let max_stretch = group.max_stretch.filter(|value| value.is_finite()).unwrap_or(0.0);
			group.enabled
				&& max_stretch >= 10.0
				&& (dynamics_effective_stretch_motion(group) > 0.0 || group.stretch_motion_sample_has_positive)
		})
		.collect()
}

fn dynamics_large_stretch_range_samples(groups: &[&DiagnoseDynamicsGroupSummary]) -> Vec<String> {
	groups
		.iter()
		.take(4)
		.map(|group| {
			let max_stretch = group.max_stretch.unwrap_or(0.0);
			let stretch_motion = dynamics_effective_stretch_motion(group);
			format!(
				"{} max_stretch={max_stretch:.3} stretch_motion={stretch_motion:.3}",
				dynamics_group_sample_label(group)
			)
		})
		.collect()
}

fn dynamics_limit_samples_have_positive(samples: &[f32]) -> bool {
	samples.iter().any(|value| value.is_finite() && *value > 0.0)
}

fn dynamics_group_samples(groups: &[DiagnoseDynamicsGroupSummary]) -> Vec<String> {
	groups.iter().take(4).map(dynamics_group_sample_label).collect()
}

fn dynamics_unsupported_writeback_groups(groups: &[DiagnoseDynamicsGroupSummary]) -> Vec<&DiagnoseDynamicsGroupSummary> {
	groups
		.iter()
		.filter(|group| {
			group.writeback_mode == un_avatar_core::UnaDynamicsWritebackMode::RotationTranslation
				&& group.translation_writeback_target_count == 0
		})
		.collect()
}

fn dynamics_unsupported_writeback_samples(groups: &[DiagnoseDynamicsGroupSummary]) -> Vec<String> {
	groups
		.iter()
		.filter(|group| {
			group.writeback_mode == un_avatar_core::UnaDynamicsWritebackMode::RotationTranslation
				&& group.translation_writeback_target_count == 0
		})
		.take(4)
		.map(dynamics_group_sample_label)
		.collect()
}

fn dynamics_translation_writeback_candidate_total(groups: &[&DiagnoseDynamicsGroupSummary]) -> usize {
	groups.iter().map(|group| group.translation_writeback_candidate_count).sum()
}

fn dynamics_translation_writeback_target_total(groups: &[&DiagnoseDynamicsGroupSummary]) -> usize {
	groups.iter().map(|group| group.translation_writeback_target_count).sum()
}

fn dynamics_stretch_translation_writeback_group_count(groups: &[DiagnoseDynamicsGroupSummary]) -> usize {
	groups
		.iter()
		.filter(|group| {
			dynamics_group_has_length_limit(group)
				&& group.writeback_mode == un_avatar_core::UnaDynamicsWritebackMode::RotationTranslation
				&& group.translation_writeback_candidate_count > 0
		})
		.count()
}

fn dynamics_stretch_translation_writeback_target_group_count(groups: &[DiagnoseDynamicsGroupSummary]) -> usize {
	groups
		.iter()
		.filter(|group| dynamics_group_has_length_limit(group) && group.translation_writeback_target_count > 0)
		.count()
}

fn dynamics_group_sample_label(group: &DiagnoseDynamicsGroupSummary) -> String {
	let id = if group.source_id.is_empty() {
		format!("group[{}]", group.index)
	} else {
		group.source_id.clone()
	};
	match &group.root_path {
		Some(root_path) => format!("{id}@{root_path}"),
		None => id,
	}
}

fn dynamics_interaction_hook_samples(hooks: &[DiagnoseDynamicsInteractionHookSummary]) -> Vec<String> {
	hooks
		.iter()
		.filter(|hook| hook.metadata_only)
		.take(4)
		.map(|hook| {
			let id = if hook.source_id.is_empty() {
				format!("group[{}]", hook.group_index)
			} else {
				hook.source_id.clone()
			};
			match &hook.root_path {
				Some(root_path) => format!("{id}@{root_path}"),
				None => id,
			}
		})
		.collect()
}

fn dynamics_constraint_ref_samples(constraint_refs: &[DiagnoseDynamicsConstraintRefSummary]) -> Vec<String> {
	constraint_refs
		.iter()
		.take(4)
		.map(|constraint_ref| {
			let id = if constraint_ref.source_id.is_empty() {
				format!("constraint_ref[{}]", constraint_ref.index)
			} else {
				constraint_ref.source_id.clone()
			};
			match &constraint_ref.target_path {
				Some(target_path) => format!("{id}@{target_path}"),
				None => id,
			}
		})
		.collect()
}

fn dynamics_contact_probe_samples(contact_probes: &[DiagnoseContactProbeSummary]) -> Vec<String> {
	contact_probes
		.iter()
		.filter(|probe| probe.would_emit)
		.take(4)
		.map(|probe| {
			let receiver = if probe.receiver_source_id.is_empty() {
				format!("receiver[{}]", probe.receiver_index)
			} else {
				probe.receiver_source_id.clone()
			};
			let sender = if probe.sender_source_id.is_empty() {
				format!("sender[{}]", probe.sender_index)
			} else {
				probe.sender_source_id.clone()
			};
			let target = match &probe.receiver_node_path {
				Some(receiver_path) => format!("{receiver}@{receiver_path}"),
				None => receiver,
			};
			format!("{target}<={sender}:{}", probe.parameter)
		})
		.collect()
}

fn format_warning_samples(samples: &[String]) -> String {
	if samples.is_empty() {
		String::new()
	} else {
		format!(" samples=[{}]", samples.join(", "))
	}
}

fn dynamics_hit_radius_sample_summary(samples: &[f32]) -> (usize, Option<f32>, Option<f32>) {
	let mut min = None::<f32>;
	let mut max = None::<f32>;
	for sample in samples.iter().copied().filter(|sample| sample.is_finite()) {
		min = Some(min.map_or(sample, |value| value.min(sample)));
		max = Some(max.map_or(sample, |value| value.max(sample)));
	}
	(samples.len(), min, max)
}

fn dynamics_contact_summaries(doc: &UnaDocument) -> Vec<DiagnoseDynamicsContactSummary> {
	let runtime_model = doc.runtime_model();
	let Some(runtime) = runtime_model.scene_profile_dynamics() else {
		return Vec::new();
	};
	let node_paths_by_index = scene_node_paths_by_index(runtime.scene);
	runtime
		.dynamics
		.contacts()
		.enumerate()
		.map(|(index, contact)| DiagnoseDynamicsContactSummary {
			index,
			source_kind: contact.source_kind,
			kind: contact.kind.clone(),
			source_id: contact.source_id.clone(),
			node: contact.node,
			node_path: node_paths_by_index.get(contact.node).cloned().flatten(),
			parameter: contact.parameter.clone(),
			collision_tags: contact.collision_tags.clone(),
			shape: contact.shape.clone(),
			radius: contact.radius,
			height: contact.height,
			position: contact.position,
		})
		.collect()
}

fn dynamics_contact_parameter_declaration_summaries(doc: &UnaDocument) -> Vec<DiagnoseContactParameterDeclarationSummary> {
	let runtime_model = doc.runtime_model();
	let Some(runtime) = runtime_model.scene_profile_dynamics() else {
		return Vec::new();
	};
	let node_paths_by_index = scene_node_paths_by_index(runtime.scene);
	runtime
		.dynamics
		.contact_parameter_declarations()
		.into_iter()
		.enumerate()
		.map(|(index, declaration)| DiagnoseContactParameterDeclarationSummary {
			index,
			owner_key: declaration.owner_key,
			source_id: declaration.source_id,
			node: declaration.node,
			node_path: node_paths_by_index.get(declaration.node).cloned().flatten(),
			parameter: declaration.parameter,
			collision_tags: declaration.collision_tags,
		})
		.collect()
}

fn dynamics_contact_probe_summaries(doc: &UnaDocument) -> Vec<DiagnoseContactProbeSummary> {
	let runtime_model = doc.runtime_model();
	let Some(runtime) = runtime_model.scene_profile_dynamics() else {
		return Vec::new();
	};
	let node_paths_by_index = scene_node_paths_by_index(runtime.scene);
	runtime
		.contact_probes()
		.into_iter()
		.enumerate()
		.map(|(index, probe)| DiagnoseContactProbeSummary {
			index,
			receiver_index: probe.receiver_index,
			sender_index: probe.sender_index,
			receiver_source_id: probe.receiver_source_id,
			sender_source_id: probe.sender_source_id,
			receiver_node: probe.receiver_node,
			receiver_node_path: node_paths_by_index.get(probe.receiver_node).cloned().flatten(),
			sender_node: probe.sender_node,
			sender_node_path: node_paths_by_index.get(probe.sender_node).cloned().flatten(),
			parameter: probe.parameter,
			matched_tags: probe.matched_tags,
			tag_match: probe.tag_match,
			overlap: probe.overlap,
			would_emit: probe.would_emit,
			distance: probe.distance,
			threshold: probe.threshold,
			receiver_radius: probe.receiver_radius,
			sender_radius: probe.sender_radius,
			receiver_shape: probe.receiver_shape,
			sender_shape: probe.sender_shape,
			approximation: probe.approximation,
		})
		.collect()
}

fn dynamics_contact_parameter_emission_summaries(doc: &UnaDocument) -> Vec<DiagnoseContactParameterEmissionSummary> {
	if !doc.runtime_model().contact_parameter_emission_enabled() {
		return Vec::new();
	}
	let runtime_model = doc.runtime_model();
	let node_paths_by_index = runtime_model.scene().map(scene_node_paths_by_index).unwrap_or_default();
	runtime_model
		.contact_parameter_emissions()
		.into_iter()
		.enumerate()
		.map(|(index, emission)| DiagnoseContactParameterEmissionSummary {
			index,
			owner_key: emission.owner_key,
			source_id: emission.source_id,
			receiver_index: emission.receiver_index,
			receiver_node: emission.receiver_node,
			receiver_node_path: node_paths_by_index.get(emission.receiver_node).cloned().flatten(),
			parameter: emission.parameter,
			value: emission.value,
			emitted: emission.emitted,
			sender_source_ids: emission.sender_source_ids,
		})
		.collect()
}

fn dynamics_constraint_ref_summaries(doc: &UnaDocument) -> Vec<DiagnoseDynamicsConstraintRefSummary> {
	let runtime_model = doc.runtime_model();
	let Some(runtime) = runtime_model.scene_profile_dynamics() else {
		return Vec::new();
	};
	let node_paths_by_index = scene_node_paths_by_index(runtime.scene);
	runtime
		.dynamics
		.constraint_refs()
		.enumerate()
		.map(|(index, constraint_ref)| {
			let source_paths = constraint_ref
				.source_nodes
				.iter()
				.filter_map(|node| node_paths_by_index.get(*node).cloned().flatten())
				.collect();
			DiagnoseDynamicsConstraintRefSummary {
				index,
				source_kind: constraint_ref.source_kind,
				source_id: constraint_ref.source_id.clone(),
				target_node: constraint_ref.target_node,
				target_path: node_paths_by_index.get(constraint_ref.target_node).cloned().flatten(),
				source_nodes: constraint_ref.source_nodes.clone(),
				source_paths,
				constraint_type: constraint_ref.constraint_type.clone(),
				weight: constraint_ref.weight,
			}
		})
		.collect()
}

fn duplicate_dynamics_source_ids(groups: &[DiagnoseDynamicsGroupSummary]) -> Vec<(String, usize)> {
	let mut counts = BTreeMap::<String, usize>::new();
	for group in groups {
		if group.source_id.is_empty() {
			continue;
		}
		*counts.entry(group.source_id.clone()).or_default() += 1;
	}
	counts.into_iter().filter(|(_, count)| *count > 1).collect()
}

fn duplicate_dynamics_contact_source_ids(contacts: &[DiagnoseDynamicsContactSummary]) -> Vec<(String, usize)> {
	let mut counts = BTreeMap::<String, usize>::new();
	for contact in contacts {
		if contact.source_id.is_empty() {
			continue;
		}
		*counts.entry(contact.source_id.clone()).or_default() += 1;
	}
	counts.into_iter().filter(|(_, count)| *count > 1).collect()
}

fn duplicate_dynamics_constraint_ref_source_ids(constraint_refs: &[DiagnoseDynamicsConstraintRefSummary]) -> Vec<(String, usize)> {
	let mut counts = BTreeMap::<String, usize>::new();
	for constraint_ref in constraint_refs {
		if constraint_ref.source_id.is_empty() {
			continue;
		}
		*counts.entry(constraint_ref.source_id.clone()).or_default() += 1;
	}
	counts.into_iter().filter(|(_, count)| *count > 1).collect()
}

fn dynamics_source_feature_counts(doc: &UnaDocument) -> DynamicsSourceFeatureCounts {
	let Some(unavatar) = doc.unavatar.as_ref() else {
		return DynamicsSourceFeatureCounts::default();
	};
	let Some(dynamics) = unavatar.source.get("dynamics").and_then(|value| value.as_array()) else {
		return DynamicsSourceFeatureCounts::default();
	};
	let mut counts = DynamicsSourceFeatureCounts::default();
	for item in dynamics {
		let source_params = dynamics_source_params(item);
		let limit_type = dynamics_source_value(item, source_params, "limitType", "limit_type")
			.and_then(|value| value.as_str())
			.unwrap_or("");
		let max_angle_x = dynamics_source_value(item, source_params, "maxAngleX", "max_angle_x")
			.and_then(json_number_f64)
			.unwrap_or(0.0);
		let max_angle_z = dynamics_source_value(item, source_params, "maxAngleZ", "max_angle_z")
			.and_then(json_number_f64)
			.unwrap_or(0.0);
		let max_stretch = dynamics_source_value(item, source_params, "maxStretch", "max_stretch")
			.and_then(json_number_f64)
			.unwrap_or(0.0);
		let max_squish = dynamics_source_value(item, source_params, "maxSquish", "max_squish")
			.and_then(json_number_f64)
			.unwrap_or(0.0);
		let stretch_motion = dynamics_source_value(item, source_params, "stretchMotion", "stretch_motion")
			.and_then(json_number_f64)
			.unwrap_or(0.0);
		if !limit_type.is_empty()
			|| max_angle_x.abs() > 0.0
			|| max_angle_z.abs() > 0.0
			|| max_stretch.abs() > 0.0
			|| max_squish.abs() > 0.0
			|| stretch_motion.abs() > 0.0
		{
			counts.limit_count += 1;
		}
		if !limit_type.is_empty() || max_angle_x.abs() > 0.0 || max_angle_z.abs() > 0.0 {
			counts.angle_limit_count += 1;
		}
		if max_stretch.abs() > 0.0 || max_squish.abs() > 0.0 || stretch_motion.abs() > 0.0 {
			counts.stretch_limit_count += 1;
		}
		let radius_curve = dynamics_source_curve_key_count(item, source_params, "radiusCurve", "radius_curve") > 0;
		let angle_limit_curve = dynamics_source_curve_key_count(item, source_params, "maxAngleXCurve", "max_angle_x_curve") > 0
			|| dynamics_source_curve_key_count(item, source_params, "maxAngleZCurve", "max_angle_z_curve") > 0;
		let stretch_limit_curve = dynamics_source_curve_key_count(item, source_params, "maxStretchCurve", "max_stretch_curve") > 0
			|| dynamics_source_curve_key_count(item, source_params, "maxSquishCurve", "max_squish_curve") > 0
			|| dynamics_source_curve_key_count(item, source_params, "stretchMotionCurve", "stretch_motion_curve") > 0;
		let force_curve = dynamics_source_curve_key_count(item, source_params, "pullCurve", "pull_curve") > 0
			|| dynamics_source_curve_key_count(item, source_params, "springCurve", "spring_curve") > 0
			|| dynamics_source_curve_key_count(item, source_params, "stiffnessCurve", "stiffness_curve") > 0
			|| dynamics_source_curve_key_count(item, source_params, "gravityCurve", "gravity_curve") > 0
			|| dynamics_source_curve_key_count(item, source_params, "gravityFalloffCurve", "gravity_falloff_curve") > 0
			|| dynamics_source_curve_key_count(item, source_params, "immobileCurve", "immobile_curve") > 0;
		if radius_curve || angle_limit_curve || stretch_limit_curve || force_curve {
			counts.curve_count += 1;
		}
		if radius_curve {
			counts.radius_curve_count += 1;
		}
		if angle_limit_curve {
			counts.angle_limit_curve_count += 1;
		}
		if stretch_limit_curve {
			counts.stretch_limit_curve_count += 1;
		}
		if dynamics_source_value(item, source_params, "allowCollision", "allow_collision").and_then(|value| value.as_bool()) == Some(false)
		{
			counts.collision_disabled_count += 1;
		}
		if dynamics_source_value(item, source_params, "allowGrabbing", "allow_grabbing").and_then(|value| value.as_bool()) == Some(true) {
			counts.grabbing_enabled_count += 1;
		}
		if dynamics_source_value(item, source_params, "allowPosing", "allow_posing").and_then(|value| value.as_bool()) == Some(true) {
			counts.posing_enabled_count += 1;
		}
		if dynamics_source_value(item, source_params, "parameter", "parameter")
			.and_then(|value| value.as_str())
			.is_some_and(|parameter| !parameter.is_empty())
		{
			counts.interaction_parameter_count += 1;
		}
		if let Some(colliders) = dynamics_source_value(item, source_params, "colliders", "colliders").and_then(|value| value.as_array()) {
			counts.collider_count += colliders.len();
			counts.unknown_shape_collider_count += colliders
				.iter()
				.filter(|collider| !dynamics_source_collider_shape_known(collider))
				.count();
			counts.inside_bounds_collider_count += colliders
				.iter()
				.filter(|collider| {
					collider
						.get("insideBounds")
						.or_else(|| collider.get("inside_bounds"))
						.and_then(|value| value.as_bool())
						== Some(true)
				})
				.count();
		}
	}
	counts
}

fn dynamics_source_collider_summaries_by_source_id(doc: &UnaDocument) -> BTreeMap<String, DynamicsSourceColliderSummary> {
	let Some(unavatar) = doc.unavatar.as_ref() else {
		return BTreeMap::new();
	};
	let Some(dynamics) = unavatar.source.get("dynamics").and_then(|value| value.as_array()) else {
		return BTreeMap::new();
	};
	let mut summaries = BTreeMap::<String, DynamicsSourceColliderSummary>::new();
	for item in dynamics {
		let source_id = item.get("id").and_then(|value| value.as_str()).unwrap_or_default();
		if source_id.is_empty() {
			continue;
		}
		let source_params = dynamics_source_params(item);
		let allow_collision =
			dynamics_source_value(item, source_params, "allowCollision", "allow_collision").and_then(|value| value.as_bool());
		let colliders = dynamics_source_value(item, source_params, "colliders", "colliders").and_then(|value| value.as_array());
		let summary = summaries.entry(source_id.to_string()).or_default();
		summary.component_path = dynamics_node_ref_path(item.get("component")).or_else(|| summary.component_path.clone());
		for root_path in dynamics_source_root_paths(item) {
			if !summary.root_paths.contains(&root_path) {
				summary.root_paths.push(root_path);
			}
		}
		summary.allow_collision = allow_collision.or(summary.allow_collision);
		if let Some(colliders) = colliders {
			summary.collider_count += colliders.len();
			for collider_path in colliders
				.iter()
				.filter_map(|collider| dynamics_node_ref_path(collider.get("component")))
			{
				if summary.collider_paths.len() >= 16 {
					break;
				}
				if !summary.collider_paths.contains(&collider_path) {
					summary.collider_paths.push(collider_path);
				}
			}
			summary.unknown_shape_collider_count += colliders
				.iter()
				.filter(|collider| !dynamics_source_collider_shape_known(collider))
				.count();
			summary.inside_bounds_collider_count += colliders
				.iter()
				.filter(|collider| {
					collider
						.get("insideBounds")
						.or_else(|| collider.get("inside_bounds"))
						.and_then(|value| value.as_bool())
						== Some(true)
				})
				.count();
		}
	}
	summaries
}

fn dynamics_node_ref_path(value: Option<&serde_json::Value>) -> Option<String> {
	value
		.and_then(|value| value.get("path").and_then(|path| path.as_str()))
		.filter(|path| !path.is_empty())
		.map(str::to_string)
}

fn dynamics_source_root_paths(item: &serde_json::Value) -> Vec<String> {
	let Some(roots) = item.get("roots").or_else(|| item.get("root")).or_else(|| item.get("rootNode")) else {
		return Vec::new();
	};
	if let Some(roots) = roots.as_array() {
		roots.iter().filter_map(|root| dynamics_node_ref_path(Some(root))).collect()
	} else {
		dynamics_node_ref_path(Some(roots)).into_iter().collect()
	}
}

fn dynamics_source_collider_shape_known(collider: &serde_json::Value) -> bool {
	let shape = collider
		.get("shapeType")
		.or_else(|| collider.get("shape_type"))
		.or_else(|| collider.get("shape"));
	if matches!(shape.and_then(|shape| shape.as_u64()), Some(0 | 1)) {
		return true;
	}
	let shape = shape.and_then(|shape| shape.as_str()).unwrap_or_default();
	shape == "0"
		|| shape == "1"
		|| shape.eq_ignore_ascii_case("sphere")
		|| shape.eq_ignore_ascii_case("local_sphere")
		|| shape.eq_ignore_ascii_case("capsule")
		|| shape.eq_ignore_ascii_case("local_capsule")
		|| shape.eq_ignore_ascii_case("plane")
		|| shape.eq_ignore_ascii_case("local_plane")
}

fn dynamics_source_collider_audit(doc: &UnaDocument, active_source_ids: Option<&BTreeSet<String>>) -> DynamicsSourceColliderAudit {
	let Some(unavatar) = doc.unavatar.as_ref() else {
		return DynamicsSourceColliderAudit::default();
	};
	let Some(dynamics) = unavatar.source.get("dynamics").and_then(|value| value.as_array()) else {
		return DynamicsSourceColliderAudit::default();
	};
	let mut audit = DynamicsSourceColliderAudit::default();
	for item in dynamics {
		let source_params = dynamics_source_params(item);
		let source_id = item.get("id").and_then(|value| value.as_str()).unwrap_or_default();
		if let Some(active_source_ids) = active_source_ids {
			if !active_source_ids.contains(source_id) {
				continue;
			}
		}
		let label = dynamics_source_label(item);
		let allow_collision =
			dynamics_source_value(item, source_params, "allowCollision", "allow_collision").and_then(|value| value.as_bool());
		let colliders = dynamics_source_value(item, source_params, "colliders", "colliders").and_then(|value| value.as_array());
		let collider_count = colliders.map_or(0, Vec::len);
		if dynamics_source_has_collision_enabled_empty_colliders(allow_collision, collider_count) {
			audit.collision_enabled_empty_collider_count += 1;
			if !source_id.is_empty() {
				audit.collision_enabled_empty_collider_source_ids.push(source_id.to_string());
			}
			if audit.collision_enabled_empty_collider_samples.len() < 8 {
				audit.collision_enabled_empty_collider_samples.push(label.clone());
			}
		}
	}
	audit
}

fn dynamics_source_has_collision_enabled_empty_colliders(allow_collision: Option<bool>, collider_count: usize) -> bool {
	allow_collision == Some(true) && collider_count == 0
}

fn dynamics_source_label(item: &serde_json::Value) -> String {
	item.get("id")
		.and_then(|value| value.as_str())
		.filter(|id| !id.is_empty())
		.or_else(|| item.get("name").and_then(|value| value.as_str()).filter(|name| !name.is_empty()))
		.unwrap_or("<unnamed>")
		.to_string()
}

fn wardrobe_dynamics_enable_targets(doc: &UnaDocument) -> Vec<String> {
	let Some(unavatar) = doc.unavatar.as_ref() else {
		return Vec::new();
	};
	let Some(sets) = unavatar
		.source
		.get("wardrobe")
		.and_then(|wardrobe| wardrobe.get("sets"))
		.and_then(|value| value.as_array())
	else {
		return Vec::new();
	};
	let mut targets = Vec::new();
	for set in sets {
		let Some(operations) = set.get("operations").and_then(|value| value.as_array()) else {
			continue;
		};
		for op in operations {
			let ty = op
				.get("type")
				.or_else(|| op.get("op"))
				.and_then(|value| value.as_str())
				.unwrap_or("");
			if ty != "dynamicsEnable" {
				continue;
			}
			if let Some(target_id) = wardrobe_dynamics_target_id(op) {
				targets.push(target_id.to_string());
			}
		}
	}
	targets
}

fn wardrobe_dynamics_target_id(op: &serde_json::Value) -> Option<&str> {
	let target = op.get("target");
	target
		.and_then(|target| {
			target
				.get("dynamicsId")
				.or_else(|| target.get("dynamics_id"))
				.or_else(|| target.get("sourceId"))
				.or_else(|| target.get("source_id"))
				.or_else(|| target.get("id"))
		})
		.or_else(|| op.get("dynamicsId").or_else(|| op.get("dynamics_id")))
		.or_else(|| op.get("sourceId").or_else(|| op.get("source_id")))
		.or_else(|| op.get("dynamics"))
		.or(target)
		.and_then(|value| value.as_str())
		.filter(|id| !id.is_empty())
}

fn runtime_action_dynamics_enable_targets(actions: Option<&un_avatar_core::UnaRuntimeActionSet>) -> Vec<(String, String)> {
	let Some(actions) = actions else {
		return Vec::new();
	};
	let mut targets = Vec::new();
	for action in &actions.actions {
		for effect in &action.effects {
			if let UnaRuntimeActionEffect::DynamicsEnabled { source_id, .. } = effect {
				if !source_id.is_empty() {
					targets.push((action.id.clone(), source_id.clone()));
				}
			}
		}
	}
	targets
}

fn dynamics_source_params(value: &serde_json::Value) -> Option<&serde_json::Value> {
	value.get("sourceParams").or_else(|| value.get("source_params"))
}

fn dynamics_source_value<'a>(
	value: &'a serde_json::Value,
	source_params: Option<&'a serde_json::Value>,
	camel_key: &str,
	snake_key: &str,
) -> Option<&'a serde_json::Value> {
	source_params
		.and_then(|params| params.get(camel_key).or_else(|| params.get(snake_key)))
		.or_else(|| value.get(camel_key).or_else(|| value.get(snake_key)))
}

fn dynamics_source_curve_key_count(
	value: &serde_json::Value,
	source_params: Option<&serde_json::Value>,
	camel_key: &str,
	snake_key: &str,
) -> usize {
	let Some(curve) = dynamics_source_value(value, source_params, camel_key, snake_key) else {
		return 0;
	};
	curve
		.get("keyCount")
		.or_else(|| curve.get("key_count"))
		.and_then(|value| value.as_u64())
		.or_else(|| curve.get("keys").and_then(|value| value.as_array()).map(|keys| keys.len() as u64))
		.unwrap_or_default() as usize
}

fn json_number_f64(value: &serde_json::Value) -> Option<f64> {
	value.as_f64().or_else(|| value.as_i64().map(|value| value as f64))
}

fn visible_mesh_materials(
	scene: &un_avatar_core::UnaSceneSnapshot,
	mesh_index: usize,
	alpha_cache: &mut BTreeMap<usize, Option<DiagnoseTextureAlphaSummary>>,
) -> Vec<DiagnoseVisibleMaterialSummary> {
	let Some(primitives) = scene.meshes.get(mesh_index) else {
		return Vec::new();
	};
	primitives
		.iter()
		.enumerate()
		.filter_map(|(primitive_index, primitive)| {
			let material_index = primitive.material_index?;
			let material = scene.materials.get(material_index)?;
			let draw_skipped_fully_transparent = matches!(material.alpha_mode, UnaAlphaMode::Mask | UnaAlphaMode::Blend)
				&& material.base_color_factor[3] <= 0.001
				&& texture_alpha_summary_cached(scene, alpha_cache, material.base_color_texture_index)
					.is_some_and(|alpha| alpha.max_alpha == 0);
			let nonzero_morph_weights = primitive
				.default_morph_weights
				.iter()
				.enumerate()
				.filter(|(_, weight)| weight.abs() > 0.000001)
				.map(|(index, &weight)| DiagnoseMorphWeightSummary {
					index,
					name: primitive.morph_target_names.get(index).cloned(),
					weight,
					position_delta_abs_sum: primitive
						.morph_targets
						.get(index)
						.map(|target| target.position_deltas.iter().map(|v| v[0].abs() + v[1].abs() + v[2].abs()).sum())
						.unwrap_or(0.0),
					normal_delta_abs_sum: primitive
						.morph_targets
						.get(index)
						.and_then(|target| target.normal_deltas.as_ref())
						.map(|deltas| deltas.iter().map(|v| v[0].abs() + v[1].abs() + v[2].abs()).sum())
						.unwrap_or(0.0),
				})
				.collect();
			Some(DiagnoseVisibleMaterialSummary {
				primitive: primitive_index,
				index: material_index,
				name: material.name.clone(),
				source_shader: material
					.unavatar_material
					.as_ref()
					.and_then(|m| m.get("sourceShader"))
					.and_then(|v| v.as_str())
					.map(str::to_owned),
				shading: material.shading,
				alpha_mode: material.alpha_mode,
				alpha_cutoff: material.alpha_cutoff,
				transparent_with_z_write: material_transparent_with_z_write(material),
				draw_skipped_fully_transparent,
				morph_target_count: primitive.morph_targets.len(),
				nonzero_morph_weights,
			})
		})
		.collect()
}

fn skin_summaries(scene: &un_avatar_core::UnaSceneSnapshot) -> Vec<DiagnoseSkinSummary> {
	const RENDERER_MAX_BONES: usize = 1024;
	let mut summaries = scene
		.skins
		.iter()
		.enumerate()
		.map(|(index, skin)| {
			let effective_joint_count = skin.joint_nodes.len().min(skin.inverse_bind_matrices.len());
			DiagnoseSkinSummary {
				index,
				joint_count: skin.joint_nodes.len(),
				inverse_bind_count: skin.inverse_bind_matrices.len(),
				effective_joint_count,
				over_renderer_bone_limit: effective_joint_count > RENDERER_MAX_BONES,
				skeleton_node: skin.skeleton_node,
				used_by_node_count: 0,
				primitive_joint_attribute_count: 0,
				primitive_weight_attribute_count: 0,
				mismatched_joint_weight_attribute_count: 0,
				max_joint_index: None,
				out_of_range_joint_attribute_count: 0,
			}
		})
		.collect::<Vec<_>>();
	for node in &scene.nodes {
		let Some(skin_index) = node.skin else { continue };
		let Some(summary) = summaries.get_mut(skin_index) else { continue };
		summary.used_by_node_count += 1;
		let joint_bound = summary.joint_count.min(summary.inverse_bind_count);
		let Some(mesh_index) = node.mesh else { continue };
		let Some(primitives) = scene.meshes.get(mesh_index) else { continue };
		for primitive in primitives {
			if primitive.joints.is_some() != primitive.weights.is_some() {
				summary.mismatched_joint_weight_attribute_count += 1;
			}
			if let Some(joints) = primitive.joints.as_ref() {
				summary.primitive_joint_attribute_count += 1;
				if let Some(max_joint) = joints.iter().flatten().copied().max() {
					summary.max_joint_index = Some(summary.max_joint_index.map_or(max_joint, |current| current.max(max_joint)));
				}
				if joints
					.iter()
					.flatten()
					.any(|joint| (*joint as usize) >= joint_bound || (*joint as usize) >= RENDERER_MAX_BONES)
				{
					summary.out_of_range_joint_attribute_count += 1;
				}
			}
			if primitive.weights.is_some() {
				summary.primitive_weight_attribute_count += 1;
			}
		}
	}
	summaries
}

fn unavatar_wardrobe_sets(doc: &UnaDocument) -> Vec<(String, Option<String>)> {
	doc.unavatar
		.as_ref()
		.and_then(|ext| ext.source.get("wardrobe"))
		.and_then(|wardrobe| wardrobe.get("sets"))
		.and_then(|sets| sets.as_array())
		.map(|sets| {
			sets.iter()
				.filter_map(|set| {
					let id = set.get("id").and_then(|v| v.as_str())?.to_owned();
					let display_name = json_string(set.get("displayName"));
					Some((id, display_name))
				})
				.collect()
		})
		.unwrap_or_default()
}

fn unavatar_base_set_id(doc: &UnaDocument) -> String {
	let Some(wardrobe) = doc.unavatar.as_ref().and_then(|ext| ext.source.get("wardrobe")) else {
		return String::new();
	};
	if let Some(base_set) = wardrobe.get("baseSet").and_then(|v| v.as_str()) {
		return base_set.to_owned();
	}
	wardrobe
		.get("sets")
		.and_then(|sets| sets.as_array())
		.and_then(|sets| {
			sets.iter()
				.find(|set| set.get("default").and_then(|v| v.as_bool()).unwrap_or(false))
				.or_else(|| sets.iter().find(|set| set.get("id").and_then(|v| v.as_str()) == Some("")))
		})
		.and_then(|set| set.get("id").and_then(|v| v.as_str()))
		.unwrap_or("")
		.to_owned()
}

fn wardrobe_probe_for_document(
	set_id: String,
	display_name: Option<String>,
	doc: &UnaDocument,
	apply_report: Option<WardrobeApplyReport>,
	probe_ms: u128,
) -> DiagnoseWardrobeProbeSummary {
	let mut visible_mesh_paths = Vec::new();
	let mut nonzero_morph_weights = Vec::new();
	if let Some(scene) = doc.runtime_model().scene() {
		let effective_visibility = scene_effective_visibility(scene);
		let node_paths_by_index = scene_node_paths_by_index(scene);
		for (node_index, node) in scene.nodes.iter().enumerate() {
			if !effective_visibility.get(node_index).copied().unwrap_or(false) {
				continue;
			}
			if node.mesh.is_some() {
				let path = node_paths_by_index
					.get(node_index)
					.cloned()
					.flatten()
					.or_else(|| node.name.clone())
					.unwrap_or_else(|| format!("#{node_index}"));
				visible_mesh_paths.push(path);
			}
		}
		for (mesh_index, primitives) in scene.meshes.iter().enumerate() {
			for (primitive_index, primitive) in primitives.iter().enumerate() {
				for (weight_index, &weight) in primitive.default_morph_weights.iter().enumerate() {
					if weight.abs() > 0.000001 {
						nonzero_morph_weights.push(DiagnoseWardrobeProbeMorphSummary {
							mesh: mesh_index,
							primitive: primitive_index,
							index: weight_index,
							name: primitive.morph_target_names.get(weight_index).cloned(),
							weight,
						});
					}
				}
			}
		}
	}
	let (
		visibility_applied,
		visibility_missing,
		blendshape_applied,
		blendshape_missing,
		dynamics_applied,
		dynamics_missing,
		material_applied,
		material_missing,
		material_slot_applied,
		material_slot_missing,
		active_asset_groups,
		missing_visibility_paths,
		missing_blendshapes,
		missing_dynamics_ids,
		missing_materials,
		missing_material_slots,
	) = if let Some(report) = apply_report {
		(
			Some(report.visibility_applied),
			Some(report.visibility_missing),
			Some(report.blendshape_applied),
			Some(report.blendshape_missing),
			Some(report.dynamics_applied),
			Some(report.dynamics_missing),
			Some(report.material_applied),
			Some(report.material_missing),
			Some(report.material_slot_applied),
			Some(report.material_slot_missing),
			report.active_asset_groups,
			report.missing_visibility_paths,
			report.missing_blendshapes,
			report.missing_dynamics_ids,
			report.missing_materials,
			report.missing_material_slots,
		)
	} else {
		(
			None,
			None,
			None,
			None,
			None,
			None,
			None,
			None,
			None,
			None,
			Vec::new(),
			Vec::new(),
			Vec::new(),
			Vec::new(),
			Vec::new(),
			Vec::new(),
		)
	};
	DiagnoseWardrobeProbeSummary {
		set_id,
		display_name,
		probe_ms,
		visibility_applied,
		visibility_missing,
		blendshape_applied,
		blendshape_missing,
		dynamics_applied,
		dynamics_missing,
		material_applied,
		material_missing,
		material_slot_applied,
		material_slot_missing,
		active_asset_groups,
		visible_mesh_node_count: visible_mesh_paths.len(),
		visible_mesh_paths,
		nonzero_morph_weight_count: nonzero_morph_weights.len(),
		nonzero_morph_weights,
		missing_visibility_paths,
		missing_blendshapes,
		missing_dynamics_ids,
		missing_materials,
		missing_material_slots,
	}
}

fn build_wardrobe_probes(base_doc: &UnaDocument) -> Result<Vec<DiagnoseWardrobeProbeSummary>, String> {
	if base_doc.unavatar.is_none() {
		return Ok(Vec::new());
	}
	let base_id = unavatar_base_set_id(base_doc);
	let sets = unavatar_wardrobe_sets(base_doc);
	let mut probes = Vec::new();
	let base_display_name = sets
		.iter()
		.find(|(id, _)| id == &base_id)
		.and_then(|(_, display_name)| display_name.clone());
	let started = Instant::now();
	probes.push(wardrobe_probe_for_document(
		base_id.clone(),
		base_display_name,
		base_doc,
		None,
		started.elapsed().as_millis(),
	));
	for (set_id, display_name) in sets {
		if set_id == base_id {
			continue;
		}
		let started = Instant::now();
		let mut doc = base_doc.clone();
		let apply_report = apply_unavatar_wardrobe_set(&mut doc, &set_id)?;
		probes.push(wardrobe_probe_for_document(
			set_id,
			display_name,
			&doc,
			Some(apply_report),
			started.elapsed().as_millis(),
		));
	}
	Ok(probes)
}

fn json_string(value: Option<&serde_json::Value>) -> Option<String> {
	value.and_then(|v| v.as_str()).map(str::to_owned)
}

fn modular_avatar_component_fields(component: &serde_json::Value) -> Option<&serde_json::Value> {
	component.get("fields")
}

fn modular_avatar_component_string(component: &serde_json::Value, names: &[&str]) -> Option<String> {
	names
		.iter()
		.find_map(|name| {
			modular_avatar_component_fields(component)
				.and_then(|fields| fields.get(*name))
				.or_else(|| component.get(*name))
				.and_then(|value| value.as_str())
				.filter(|value| !value.is_empty())
		})
		.map(str::to_owned)
}

fn modular_avatar_component_ref<'a>(component: &'a serde_json::Value, names: &[&str]) -> Option<&'a serde_json::Value> {
	names.iter().find_map(|name| {
		modular_avatar_component_fields(component)
			.and_then(|fields| fields.get(*name))
			.or_else(|| component.get(*name))
	})
}

fn modular_avatar_sub_parameter_names(value: &serde_json::Value) -> Vec<String> {
	value
		.get("subParameters")
		.or_else(|| value.get("sub_parameters"))
		.or_else(|| value.get("SubParameters"))
		.and_then(|value| value.as_array())
		.map(|parameters| {
			parameters
				.iter()
				.filter_map(|parameter| {
					parameter
						.as_str()
						.or_else(|| parameter.get("name").and_then(|value| value.as_str()))
						.or_else(|| parameter.get("Name").and_then(|value| value.as_str()))
				})
				.filter(|value| !value.is_empty())
				.map(str::to_owned)
				.collect()
		})
		.unwrap_or_default()
}

fn modular_avatar_blendshape_sync_summary(
	component: &serde_json::Value,
	component_index: usize,
) -> DiagnoseModularAvatarBlendshapeSyncSummary {
	let bindings = modular_avatar_component_ref(component, &["Bindings", "bindings"])
		.and_then(|value| value.as_array())
		.map(|bindings| {
			bindings
				.iter()
				.filter_map(modular_avatar_blendshape_sync_binding_summary)
				.collect::<Vec<_>>()
		})
		.unwrap_or_default();
	DiagnoseModularAvatarBlendshapeSyncSummary {
		component_index,
		enabled: component.get("enabled").and_then(|value| value.as_bool()).unwrap_or(true),
		target_path: modular_avatar_ref_path(component.get("target")),
		binding_count: bindings.len(),
		bindings,
	}
}

fn modular_avatar_blendshape_sync_binding_summary(
	binding: &serde_json::Value,
) -> Option<DiagnoseModularAvatarBlendshapeSyncBindingSummary> {
	if !binding.is_object() {
		return None;
	}
	let blendshape = binding
		.get("blendshape")
		.or_else(|| binding.get("Blendshape"))
		.and_then(|value| value.as_str())
		.filter(|value| !value.is_empty())?
		.to_string();
	let local_blendshape = binding
		.get("localBlendshape")
		.or_else(|| binding.get("LocalBlendshape"))
		.and_then(|value| value.as_str())
		.filter(|value| !value.is_empty())
		.unwrap_or(&blendshape)
		.to_string();
	let remap_key_count = binding
		.get("remapCurve")
		.or_else(|| binding.get("RemapCurve"))
		.and_then(|curve| curve.get("keyCount").or_else(|| curve.get("key_count")))
		.and_then(|value| value.as_u64())
		.and_then(|value| usize::try_from(value).ok())
		.unwrap_or(0);
	Some(DiagnoseModularAvatarBlendshapeSyncBindingSummary {
		reference_path: modular_avatar_ref_path(binding.get("referenceMesh").or_else(|| binding.get("ReferenceMesh"))),
		blendshape,
		local_blendshape,
		remap_key_count,
	})
}

fn modular_avatar_ref_path(value: Option<&serde_json::Value>) -> Option<String> {
	let value = value?;
	value
		.get("path")
		.or_else(|| value.get("assetPath"))
		.or_else(|| value.get("asset_path"))
		.or_else(|| value.get("referencePath"))
		.or_else(|| value.get("reference_path"))
		.and_then(|path| path.as_str())
		.filter(|path| !path.is_empty())
		.map(str::to_owned)
		.or_else(|| {
			value
				.get("resolvedTarget")
				.or_else(|| value.get("target"))
				.or_else(|| value.get("targetObject"))
				.and_then(|nested| modular_avatar_ref_path(Some(nested)))
		})
}

fn modular_avatar_menu_component_summary(
	component: &serde_json::Value,
	component_index: usize,
	short_type: &str,
) -> DiagnoseModularAvatarMenuComponentSummary {
	let menu_item = modular_avatar_component_ref(component, &["menuItem", "menu_item"]).unwrap_or(component);
	let control = menu_item
		.get("Control")
		.or_else(|| menu_item.get("control"))
		.or_else(|| modular_avatar_component_ref(component, &["Control", "control"]))
		.unwrap_or(menu_item);
	let parameter = control
		.get("parameter")
		.or_else(|| control.get("Parameter"))
		.and_then(|parameter| {
			parameter
				.as_str()
				.or_else(|| parameter.get("name").and_then(|value| value.as_str()))
				.or_else(|| parameter.get("Name").and_then(|value| value.as_str()))
		})
		.filter(|value| !value.is_empty())
		.map(str::to_owned)
		.or_else(|| modular_avatar_component_string(component, &["parameterName", "parameter_name"]));
	DiagnoseModularAvatarMenuComponentSummary {
		component_index,
		menu_key: format!("component:{component_index}"),
		short_type: short_type.to_string(),
		enabled: component.get("enabled").and_then(|value| value.as_bool()).unwrap_or(true),
		source_component_index: None,
		id: modular_avatar_component_string(component, &["id", "componentId", "component_id"]),
		hierarchy_path: modular_avatar_component_string(component, &["hierarchyPath", "hierarchy_path", "componentPath", "component_path"]),
		sibling_index: modular_avatar_component_usize(component, &["siblingIndex", "sibling_index", "transformSiblingIndex", "order"]),
		target_path: modular_avatar_ref_path(component.get("target").or_else(|| component.get("resolvedTarget"))),
		label: modular_avatar_component_string(component, &["label", "Label", "name", "Name", "displayName", "display_name"])
			.or_else(|| modular_avatar_component_string(menu_item, &["label", "Label", "name", "Name", "displayName", "display_name"]))
			.or_else(|| modular_avatar_component_string(control, &["name", "Name", "displayName", "display_name"])),
		control_type: modular_avatar_component_string(control, &["type", "Type", "controlType", "control_type"]),
		parameter,
		sub_parameters: modular_avatar_sub_parameter_names(control),
		value: control
			.get("value")
			.or_else(|| control.get("Value"))
			.and_then(json_number_f64)
			.map(|value| value as f32),
		menu_source: modular_avatar_component_string(component, &["MenuSource", "menuSource", "menu_source"]),
		menu_source_target_path: modular_avatar_ref_path(modular_avatar_component_ref(
			component,
			&[
				"menuSource_otherObjectChildren",
				"menuSourceOtherObjectChildren",
				"targetObject",
				"target_object",
			],
		)),
		menu_to_append_path: modular_avatar_ref_path(modular_avatar_component_ref(component, &["menuToAppend", "menu_to_append"])),
		menu_to_append_control_count: modular_avatar_menu_asset_control_count(modular_avatar_component_ref(
			component,
			&["menuToAppend", "menu_to_append"],
		)),
		install_target_menu_path: modular_avatar_ref_path(modular_avatar_component_ref(
			component,
			&[
				"installTargetMenu",
				"install_target_menu",
				"menuToAppendTarget",
				"menu_to_append_target",
			],
		)),
		install_target_menu_control_count: modular_avatar_menu_asset_control_count(modular_avatar_component_ref(
			component,
			&[
				"installTargetMenu",
				"install_target_menu",
				"menuToAppendTarget",
				"menu_to_append_target",
			],
		)),
		installer_path: modular_avatar_ref_path(modular_avatar_component_ref(
			component,
			&["installer", "Installer", "sourceInstaller", "source_installer"],
		)),
		external_menu_asset_path: None,
		external_menu_control_index: None,
	}
}

fn modular_avatar_external_menu_component_summaries(
	component: &serde_json::Value,
	component_index: usize,
) -> Vec<DiagnoseModularAvatarMenuComponentSummary> {
	let enabled = component.get("enabled").and_then(|value| value.as_bool()).unwrap_or(true);
	let Some(menu_asset) = modular_avatar_component_ref(component, &["menuToAppend", "menu_to_append"]) else {
		return Vec::new();
	};
	let asset_path = modular_avatar_ref_path(Some(menu_asset)).unwrap_or_else(|| format!("external-menu:{component_index}"));
	let Some(controls) = menu_asset.get("controls").and_then(|value| value.as_array()) else {
		return Vec::new();
	};
	controls
		.iter()
		.enumerate()
		.map(|(control_index, control)| {
			let label = modular_avatar_component_string(control, &["name", "Name", "displayName", "display_name"]);
			let hierarchy_path = Some(format!(
				"{}/{}",
				asset_path.trim_matches('/'),
				label.clone().unwrap_or_else(|| format!("control:{control_index}"))
			));
			DiagnoseModularAvatarMenuComponentSummary {
				component_index,
				menu_key: format!("external:{component_index}:{control_index}"),
				short_type: "VRCExpressionsMenuControl".to_string(),
				enabled,
				source_component_index: Some(component_index),
				id: None,
				hierarchy_path,
				sibling_index: Some(control_index),
				target_path: None,
				label,
				control_type: modular_avatar_component_string(control, &["type", "Type", "controlType", "control_type"]),
				parameter: modular_avatar_external_menu_control_parameter(control),
				sub_parameters: modular_avatar_sub_parameter_names(control),
				value: control
					.get("value")
					.or_else(|| control.get("Value"))
					.and_then(json_number_f64)
					.map(|value| value as f32),
				menu_source: Some("VRCExpressionsMenuAsset".to_string()),
				menu_source_target_path: Some(asset_path.clone()),
				menu_to_append_path: None,
				menu_to_append_control_count: None,
				install_target_menu_path: None,
				install_target_menu_control_count: None,
				installer_path: None,
				external_menu_asset_path: Some(asset_path.clone()),
				external_menu_control_index: Some(control_index),
			}
		})
		.collect()
}

fn modular_avatar_external_menu_control_parameter(control: &serde_json::Value) -> Option<String> {
	control
		.get("parameter")
		.or_else(|| control.get("Parameter"))
		.and_then(|parameter| {
			parameter
				.as_str()
				.or_else(|| parameter.get("name").and_then(|value| value.as_str()))
				.or_else(|| parameter.get("Name").and_then(|value| value.as_str()))
		})
		.filter(|value| !value.is_empty())
		.map(str::to_owned)
}

fn modular_avatar_menu_asset_control_count(value: Option<&serde_json::Value>) -> Option<usize> {
	let value = value?;
	value
		.get("controlCount")
		.or_else(|| value.get("control_count"))
		.and_then(|value| value.as_u64())
		.and_then(|value| usize::try_from(value).ok())
		.or_else(|| {
			value
				.get("controls")
				.and_then(|value| value.as_array())
				.map(|controls| controls.len())
		})
}

fn modular_avatar_parameter_summaries(component: &serde_json::Value, component_index: usize) -> Vec<DiagnoseModularAvatarParameterSummary> {
	let parameters = modular_avatar_component_fields(component)
		.and_then(|fields| fields.get("parameters"))
		.or_else(|| component.get("parameters"))
		.and_then(|value| value.as_array());
	parameters
		.into_iter()
		.flatten()
		.filter_map(|parameter| modular_avatar_parameter_summary(parameter, component_index))
		.collect()
}

fn modular_avatar_parameter_summary(
	parameter: &serde_json::Value,
	component_index: usize,
) -> Option<DiagnoseModularAvatarParameterSummary> {
	let name_or_prefix = parameter
		.get("nameOrPrefix")
		.or_else(|| parameter.get("name_or_prefix"))
		.or_else(|| parameter.get("name"))
		.and_then(|value| value.as_str())
		.filter(|value| !value.is_empty())?
		.to_string();
	let remap_to = parameter
		.get("remapTo")
		.or_else(|| parameter.get("remap_to"))
		.and_then(|value| value.as_str())
		.filter(|value| !value.is_empty())
		.map(str::to_string);
	let sync_type = parameter
		.get("syncType")
		.or_else(|| parameter.get("sync_type"))
		.and_then(|value| value.as_str())
		.filter(|value| !value.is_empty())
		.unwrap_or("NotSynced")
		.to_string();
	let default_value = modular_avatar_parameter_default_value(parameter);
	let has_explicit_default_value = json_bool(
		parameter
			.get("hasExplicitDefaultValue")
			.or_else(|| parameter.get("has_explicit_default_value")),
	);
	let has_default_value = has_explicit_default_value || default_value.abs() > MODULAR_AVATAR_PARAMETER_VALUE_EPSILON;
	let override_animator_defaults = json_bool(
		parameter
			.get("overrideAnimatorDefaults")
			.or_else(|| parameter.get("m_overrideAnimatorDefaults"))
			.or_else(|| parameter.get("override_animator_defaults")),
	) || sync_type == "NotSynced" && has_default_value;
	Some(DiagnoseModularAvatarParameterSummary {
		component_index,
		name_or_prefix,
		remap_to,
		internal_parameter: json_bool(parameter.get("internalParameter").or_else(|| parameter.get("internal_parameter"))),
		is_prefix: json_bool(parameter.get("isPrefix").or_else(|| parameter.get("is_prefix"))),
		local_only: json_bool(parameter.get("localOnly").or_else(|| parameter.get("local_only"))) || sync_type == "NotSynced",
		sync_type,
		default_value,
		saved: json_bool(parameter.get("saved")),
		has_explicit_default_value,
		override_animator_defaults,
	})
}

const MODULAR_AVATAR_PARAMETER_VALUE_EPSILON: f32 = 0.000001;

fn modular_avatar_parameter_default_value(parameter: &serde_json::Value) -> f32 {
	parameter
		.get("defaultValue")
		.or_else(|| parameter.get("default_value"))
		.and_then(json_number_f64)
		.unwrap_or(0.0) as f32
}

fn json_bool(value: Option<&serde_json::Value>) -> bool {
	value.and_then(|value| value.as_bool()).unwrap_or(false)
}

fn modular_avatar_menu_graph_candidates(
	components: &[DiagnoseModularAvatarMenuComponentSummary],
) -> Vec<DiagnoseModularAvatarMenuGraphCandidate> {
	let mut candidates = components
		.iter()
		.map(|component| DiagnoseModularAvatarMenuGraphCandidate {
			component_index: component.component_index,
			menu_key: component.menu_key.clone(),
			short_type: component.short_type.clone(),
			kind: modular_avatar_menu_graph_candidate_kind(&component.short_type).to_string(),
			label: component.label.clone(),
			hierarchy_path: component.hierarchy_path.clone(),
			parent_path: component.hierarchy_path.as_deref().and_then(menu_parent_path).map(str::to_string),
			sibling_index: component.sibling_index,
			target_path: component.target_path.clone().or_else(|| component.menu_source_target_path.clone()),
			menu_to_append_path: component.menu_to_append_path.clone(),
			install_target_menu_path: component.install_target_menu_path.clone(),
			installer_path: component.installer_path.clone(),
		})
		.collect::<Vec<_>>();
	candidates.sort_by(|a, b| {
		(
			a.parent_path.as_deref().unwrap_or(""),
			a.sibling_index.unwrap_or(usize::MAX),
			a.component_index,
		)
			.cmp(&(
				b.parent_path.as_deref().unwrap_or(""),
				b.sibling_index.unwrap_or(usize::MAX),
				b.component_index,
			))
	});
	candidates
}

fn modular_avatar_menu_graph_nodes(candidates: &[DiagnoseModularAvatarMenuGraphCandidate]) -> Vec<DiagnoseModularAvatarMenuGraphNode> {
	let path_to_node = candidates
		.iter()
		.enumerate()
		.filter_map(|(index, candidate)| candidate.hierarchy_path.as_ref().map(|path| (path.as_str(), index)))
		.collect::<BTreeMap<_, _>>();
	let mut nodes = candidates
		.iter()
		.enumerate()
		.map(|(node_index, candidate)| {
			let parent_node_index = candidate
				.parent_path
				.as_deref()
				.and_then(|parent_path| path_to_node.get(parent_path).copied());
			DiagnoseModularAvatarMenuGraphNode {
				node_index,
				component_index: candidate.component_index,
				menu_key: candidate.menu_key.clone(),
				short_type: candidate.short_type.clone(),
				kind: candidate.kind.clone(),
				label: candidate.label.clone(),
				hierarchy_path: candidate.hierarchy_path.clone(),
				parent_path: candidate.parent_path.clone(),
				parent_node_index,
				parent_component_index: parent_node_index.map(|index| candidates[index].component_index),
				child_component_indices: Vec::new(),
				menu_to_append_path: candidate.menu_to_append_path.clone(),
				install_target_menu_path: candidate.install_target_menu_path.clone(),
				installer_path: candidate.installer_path.clone(),
			}
		})
		.collect::<Vec<_>>();
	for index in 0..nodes.len() {
		let Some(parent_node_index) = nodes[index].parent_node_index else {
			continue;
		};
		let component_index = nodes[index].component_index;
		if let Some(parent) = nodes.get_mut(parent_node_index) {
			parent.child_component_indices.push(component_index);
		}
	}
	nodes
}

fn modular_avatar_menu_install_edges(
	components: &[DiagnoseModularAvatarMenuComponentSummary],
) -> Vec<DiagnoseModularAvatarMenuInstallEdge> {
	let referenced_installers = components
		.iter()
		.filter(|component| component.short_type == "ModularAvatarMenuInstallTarget")
		.filter_map(|component| component.installer_path.as_deref())
		.collect::<BTreeSet<_>>();
	let mut edges = components
		.iter()
		.filter_map(|component| match component.short_type.as_str() {
			"ModularAvatarMenuInstaller" => {
				let ignored_by_install_target = component
					.hierarchy_path
					.as_deref()
					.is_some_and(|path| referenced_installers.contains(path));
				(component.menu_to_append_path.is_some() || component.install_target_menu_path.is_some()).then(|| {
					DiagnoseModularAvatarMenuInstallEdge {
						source_component_index: component.component_index,
						source_kind: "installer".to_string(),
						target_kind: "target_menu".to_string(),
						source_hierarchy_path: component.hierarchy_path.clone(),
						installer_path: None,
						menu_to_append_path: component.menu_to_append_path.clone(),
						install_target_menu_path: component.install_target_menu_path.clone(),
						ignored_by_install_target,
					}
				})
			}
			"ModularAvatarMenuInstallTarget" => {
				component
					.installer_path
					.as_ref()
					.map(|installer_path| DiagnoseModularAvatarMenuInstallEdge {
						source_component_index: component.component_index,
						source_kind: "install_target".to_string(),
						target_kind: "installer".to_string(),
						source_hierarchy_path: component.hierarchy_path.clone(),
						installer_path: Some(installer_path.clone()),
						menu_to_append_path: None,
						install_target_menu_path: None,
						ignored_by_install_target: false,
					})
			}
			_ => None,
		})
		.collect::<Vec<_>>();
	edges.sort_by(|a, b| (a.source_component_index, a.source_kind.as_str()).cmp(&(b.source_component_index, b.source_kind.as_str())));
	edges
}

fn modular_avatar_menu_graph_candidate_kind(short_type: &str) -> &'static str {
	match short_type {
		"ModularAvatarMenuItem" | "VRCExpressionsMenuControl" => "control",
		"ModularAvatarMenuGroup" => "group",
		"ModularAvatarMenuInstaller" => "installer",
		"ModularAvatarMenuInstallTarget" => "install_target",
		_ => "unknown",
	}
}

fn menu_parent_path(path: &str) -> Option<&str> {
	let path = path.trim_matches('/');
	let (parent, _) = path.rsplit_once('/')?;
	(!parent.is_empty()).then_some(parent)
}

fn modular_avatar_is_menu_metadata_type(short_type: &str) -> bool {
	matches!(
		short_type,
		"ModularAvatarMenuItem"
			| "ModularAvatarMenuGroup"
			| "ModularAvatarMenuInstaller"
			| "ModularAvatarMenuInstallTarget"
			| "VRCExpressionsMenuControl"
	)
}

fn modular_avatar_is_vertex_filter_metadata_type(short_type: &str) -> bool {
	matches!(
		short_type,
		"ModularAvatarMeshCutter"
			| "ModularAvatarShapeChanger"
			| "VertexFilterByAxisComponent"
			| "VertexFilterByBoneComponent"
			| "VertexFilterByMaskComponent"
			| "VertexFilterByShapeComponent"
	)
}

fn modular_avatar_component_number(component: &serde_json::Value, names: &[&str]) -> Option<f32> {
	names
		.iter()
		.find_map(|name| {
			modular_avatar_component_fields(component)
				.and_then(|fields| fields.get(*name))
				.or_else(|| component.get(*name))
				.and_then(json_number_f64)
		})
		.map(|value| value as f32)
}

fn modular_avatar_component_usize(component: &serde_json::Value, names: &[&str]) -> Option<usize> {
	names
		.iter()
		.find_map(|name| {
			modular_avatar_component_fields(component)
				.and_then(|fields| fields.get(*name))
				.or_else(|| component.get(*name))
				.and_then(|value| value.as_u64())
		})
		.and_then(|value| usize::try_from(value).ok())
}

fn json_vec3(value: Option<&serde_json::Value>) -> Option<[f32; 3]> {
	let value = value?;
	if let Some(array) = value.as_array() {
		let x = array.first().and_then(json_number_f64)? as f32;
		let y = array.get(1).and_then(json_number_f64)? as f32;
		let z = array.get(2).and_then(json_number_f64)? as f32;
		return Some([x, y, z]);
	}
	let x = value.get("x").or_else(|| value.get("X")).and_then(json_number_f64)? as f32;
	let y = value.get("y").or_else(|| value.get("Y")).and_then(json_number_f64)? as f32;
	let z = value.get("z").or_else(|| value.get("Z")).and_then(json_number_f64)? as f32;
	Some([x, y, z])
}

fn modular_avatar_vertex_filter_summary(
	component: &serde_json::Value,
	short_type: &str,
) -> Option<DiagnoseModularAvatarVertexFilterSummary> {
	match short_type {
		"VertexFilterByShapeComponent" => Some(DiagnoseModularAvatarVertexFilterSummary {
			kind: "blend_shape".to_string(),
			shapes: modular_avatar_component_ref(component, &["Shapes", "shapes", "m_shapes"])
				.and_then(|value| value.as_array())
				.map(|values| values.iter().filter_map(|value| value.as_str().map(str::to_owned)).collect())
				.unwrap_or_default(),
			threshold: modular_avatar_component_number(component, &["Threshold", "threshold", "m_threshold"]),
			bone_path: None,
			center: None,
			axis: None,
			material_index: None,
			texture: None,
			mode: None,
		}),
		"VertexFilterByBoneComponent" => Some(DiagnoseModularAvatarVertexFilterSummary {
			kind: "bone".to_string(),
			shapes: Vec::new(),
			threshold: modular_avatar_component_number(component, &["Threshold", "threshold", "m_threshold"]),
			bone_path: modular_avatar_ref_path(modular_avatar_component_ref(component, &["Bone", "bone", "m_bone"])),
			center: None,
			axis: None,
			material_index: None,
			texture: None,
			mode: None,
		}),
		"VertexFilterByAxisComponent" => Some(DiagnoseModularAvatarVertexFilterSummary {
			kind: "axis".to_string(),
			shapes: Vec::new(),
			threshold: None,
			bone_path: None,
			center: json_vec3(modular_avatar_component_ref(component, &["Center", "center", "m_center"])),
			axis: json_vec3(modular_avatar_component_ref(component, &["Axis", "axis", "m_axis"])),
			material_index: None,
			texture: None,
			mode: None,
		}),
		"VertexFilterByMaskComponent" => Some(DiagnoseModularAvatarVertexFilterSummary {
			kind: "mask".to_string(),
			shapes: Vec::new(),
			threshold: None,
			bone_path: None,
			center: None,
			axis: None,
			material_index: modular_avatar_component_usize(
				component,
				&["MaterialIndex", "materialIndex", "material_index", "m_materialIndex"],
			),
			texture: modular_avatar_component_string(
				component,
				&["maskTextureAssetId", "MaskTexture", "maskTexture", "mask_texture", "m_maskTexture"],
			),
			mode: modular_avatar_component_string(component, &["DeleteMode", "deleteMode", "delete_mode", "m_deleteMode"]),
		}),
		_ => None,
	}
}

fn modular_avatar_vertex_filter_group_summary(
	component: &serde_json::Value,
	short_type: &str,
) -> Option<DiagnoseModularAvatarVertexFilterGroupSummary> {
	let mut filters = Vec::new();
	if short_type == "ModularAvatarShapeChanger" {
		let threshold = modular_avatar_component_number(component, &["Threshold", "threshold", "m_threshold"]).unwrap_or(0.01);
		if let Some(shapes) = modular_avatar_component_ref(component, &["Shapes", "shapes", "m_shapes"]).and_then(|value| value.as_array())
		{
			for shape in shapes {
				let change_type = shape
					.get("ChangeType")
					.or_else(|| shape.get("changeType"))
					.or_else(|| shape.get("change_type"))
					.and_then(|value| value.as_str())
					.unwrap_or("Delete");
				if change_type != "Delete" {
					continue;
				}
				if let Some(shape_name) = shape
					.get("ShapeName")
					.or_else(|| shape.get("shapeName"))
					.or_else(|| shape.get("shape_name"))
					.and_then(|value| value.as_str())
					.filter(|value| !value.is_empty())
				{
					filters.push(DiagnoseModularAvatarVertexFilterSummary {
						kind: "blend_shape".to_string(),
						shapes: vec![shape_name.to_string()],
						threshold: Some(threshold),
						bone_path: None,
						center: None,
						axis: None,
						material_index: None,
						texture: None,
						mode: None,
					});
				}
			}
		}
	} else if short_type == "ModularAvatarMeshCutter" {
		if let Some(filter_values) = modular_avatar_component_ref(component, &["filters", "Filters", "vertexFilters", "vertex_filters"])
			.and_then(|value| value.as_array())
		{
			for filter in filter_values {
				let filter_type = filter.get("shortType").and_then(|value| value.as_str()).unwrap_or("");
				if let Some(summary) = modular_avatar_vertex_filter_summary(filter, filter_type) {
					filters.push(summary);
				}
			}
		}
	} else if let Some(filter) = modular_avatar_vertex_filter_summary(component, short_type) {
		filters.push(filter);
	}
	if filters.is_empty() && short_type != "ModularAvatarMeshCutter" {
		return None;
	}
	let combine = if short_type == "ModularAvatarMeshCutter" {
		modular_avatar_component_string(component, &["MultiMode", "multiMode", "multi_mode", "m_multiMode"])
			.unwrap_or_else(|| "VertexIntersection".to_string())
	} else {
		"Single".to_string()
	};
	Some(DiagnoseModularAvatarVertexFilterGroupSummary {
		short_type: short_type.to_string(),
		enabled: component.get("enabled").and_then(|value| value.as_bool()).unwrap_or(true),
		id: modular_avatar_component_string(component, &["id", "componentId", "component_id"]),
		target_path: modular_avatar_ref_path(modular_avatar_component_ref(
			component,
			&["Object", "object", "m_object", "target", "resolvedTarget"],
		)),
		combine,
		filter_count: filters.len(),
		filters,
	})
}

fn diagnose_texture_shape_is_cube(shape: Option<&str>) -> bool {
	shape.is_some_and(|shape| shape.eq_ignore_ascii_case("TextureCube") || shape.eq_ignore_ascii_case("Cube"))
}

fn unavatar_summary(ext: &un_avatar_core::UnaUnavatarExtension) -> DiagnoseUnavatarSummary {
	let source = &ext.source;
	let wardrobe = source.get("wardrobe");
	let sets = wardrobe.and_then(|w| w.get("sets")).and_then(|v| v.as_array());
	let modular_avatar_components = source
		.get("modularAvatar")
		.and_then(|modular_avatar| modular_avatar.get("components"))
		.and_then(|components| components.as_array());
	let mut modular_avatar_support_counts = BTreeMap::new();
	let mut modular_avatar_type_counts = BTreeMap::new();
	let mut modular_avatar_disabled_type_counts = BTreeMap::new();
	let mut modular_avatar_disabled_component_count = 0;
	let mut modular_avatar_menu_components = Vec::new();
	let mut modular_avatar_parameters = Vec::new();
	let mut modular_avatar_blendshape_syncs = Vec::new();
	let mut modular_avatar_vertex_filter_groups = Vec::new();
	if let Some(components) = modular_avatar_components {
		for (component_index, component) in components.iter().enumerate() {
			let short_type = component
				.get("shortType")
				.and_then(|value| value.as_str())
				.filter(|value| !value.is_empty())
				.unwrap_or("unknown");
			bump_count(&mut modular_avatar_type_counts, short_type);
			bump_count(
				&mut modular_avatar_support_counts,
				modular_avatar_component_support_kind(short_type),
			);
			if component.get("enabled").and_then(|value| value.as_bool()) == Some(false) {
				bump_count(&mut modular_avatar_support_counts, "disabled");
				bump_count(&mut modular_avatar_disabled_type_counts, short_type);
				modular_avatar_disabled_component_count += 1;
			}
			if modular_avatar_is_menu_metadata_type(short_type) {
				modular_avatar_menu_components.push(modular_avatar_menu_component_summary(component, component_index, short_type));
				if short_type == "ModularAvatarMenuInstaller" {
					modular_avatar_menu_components.extend(modular_avatar_external_menu_component_summaries(component, component_index));
				}
			}
			if short_type == "ModularAvatarParameters" {
				modular_avatar_parameters.extend(modular_avatar_parameter_summaries(component, component_index));
			}
			if short_type == "ModularAvatarBlendshapeSync" {
				modular_avatar_blendshape_syncs.push(modular_avatar_blendshape_sync_summary(component, component_index));
			}
			if modular_avatar_is_vertex_filter_metadata_type(short_type) {
				if let Some(summary) = modular_avatar_vertex_filter_group_summary(component, short_type) {
					modular_avatar_vertex_filter_groups.push(summary);
				}
			}
		}
	}
	let base_set = json_string(wardrobe.and_then(|w| w.get("baseSet")));
	let base = sets.and_then(|sets| {
		sets.iter().find(|set| {
			let set_id = set.get("id").and_then(|v| v.as_str());
			let is_named_base = base_set.as_deref().is_some_and(|base_set| set_id == Some(base_set));
			let is_default = set.get("default").and_then(|v| v.as_bool()).unwrap_or(false);
			let is_empty_id_base = base_set.is_none() && set_id == Some("");
			is_named_base || is_default || is_empty_id_base
		})
	});
	let base_operations = base.and_then(|set| set.get("operations")).and_then(|v| v.as_array());
	let mut base_operation_counts = BTreeMap::new();
	if let Some(base_operations) = base_operations {
		for op in base_operations {
			let ty = op
				.get("type")
				.or_else(|| op.get("op"))
				.and_then(|v| v.as_str())
				.unwrap_or("unknown");
			bump_count(&mut base_operation_counts, ty);
		}
	}
	let wardrobe_set_ids = sets
		.map(|sets| {
			sets.iter()
				.filter_map(|set| set.get("id").and_then(|v| v.as_str()).map(str::to_owned))
				.collect()
		})
		.unwrap_or_default();
	let wardrobe_sets: Vec<DiagnoseUnavatarWardrobeSetSummary> = sets
		.map(|sets| {
			sets.iter()
				.map(|set| {
					let operations = set.get("operations").and_then(|v| v.as_array());
					let mut operation_counts = BTreeMap::new();
					if let Some(operations) = operations {
						for op in operations {
							let ty = op
								.get("type")
								.or_else(|| op.get("op"))
								.and_then(|v| v.as_str())
								.unwrap_or("unknown");
							bump_count(&mut operation_counts, ty);
						}
					}
					let asset_groups = set
						.get("assetGroups")
						.and_then(|v| v.as_array())
						.map(|groups| groups.iter().filter_map(|g| g.as_str().map(str::to_owned)).collect())
						.unwrap_or_default();
					DiagnoseUnavatarWardrobeSetSummary {
						id: set.get("id").and_then(|v| v.as_str()).unwrap_or("").to_owned(),
						display_name: json_string(set.get("displayName")),
						source: json_string(set.get("source")),
						asset_groups,
						operation_count: operations.map(Vec::len).unwrap_or(0),
						operation_counts,
					}
				})
				.collect()
		})
		.unwrap_or_default();
	let asset_group_ids: Vec<String> = wardrobe_sets
		.iter()
		.flat_map(|set| set.asset_groups.iter().cloned())
		.collect::<BTreeSet<_>>()
		.into_iter()
		.collect();
	let modular_avatar_menu_graph_candidates = modular_avatar_menu_graph_candidates(&modular_avatar_menu_components);
	let modular_avatar_menu_graph_nodes = modular_avatar_menu_graph_nodes(&modular_avatar_menu_graph_candidates);
	let modular_avatar_menu_install_edges = modular_avatar_menu_install_edges(&modular_avatar_menu_components);
	let modular_avatar_component_count = modular_avatar_components.map(Vec::len).unwrap_or(0);
	let modular_avatar_support_counts_alias = modular_avatar_support_counts.clone();
	let modular_avatar_type_counts_alias = modular_avatar_type_counts.clone();
	let modular_avatar_disabled_type_counts_alias = modular_avatar_disabled_type_counts.clone();

	DiagnoseUnavatarSummary {
		spec_version: ext.spec_version.clone(),
		generator: json_string(source.get("generator")),
		manifest_name: json_string(source.get("manifest").and_then(|m| m.get("name"))),
		source_type: json_string(source.get("manifest").and_then(|m| m.get("sourceType"))),
		extension_node_count: source.get("nodes").and_then(|v| v.as_array()).map(Vec::len).unwrap_or(0),
		variant_count: source.get("variants").and_then(|v| v.as_array()).map(Vec::len).unwrap_or(0),
		dynamics_entry_count: source.get("dynamics").and_then(|v| v.as_array()).map(Vec::len).unwrap_or(0),
		modular_avatar_component_count,
		modular_avatar_component_count_alias: modular_avatar_component_count,
		modular_avatar_support_counts,
		modular_avatar_support_counts_alias,
		modular_avatar_type_counts,
		modular_avatar_type_counts_alias,
		modular_avatar_disabled_type_counts,
		modular_avatar_disabled_type_counts_alias,
		modular_avatar_disabled_component_count,
		modular_avatar_disabled_component_count_alias: modular_avatar_disabled_component_count,
		modular_avatar_menu_component_count: modular_avatar_menu_components.len(),
		modular_avatar_menu_components,
		modular_avatar_menu_graph_candidate_count: modular_avatar_menu_graph_candidates.len(),
		modular_avatar_menu_graph_candidates,
		modular_avatar_menu_graph_node_count: modular_avatar_menu_graph_nodes.len(),
		modular_avatar_menu_graph_nodes,
		modular_avatar_menu_install_edge_count: modular_avatar_menu_install_edges.len(),
		modular_avatar_menu_install_edges,
		modular_avatar_parameter_count: modular_avatar_parameters.len(),
		modular_avatar_parameters,
		modular_avatar_blendshape_sync_count: modular_avatar_blendshape_syncs.len(),
		modular_avatar_blendshape_syncs,
		modular_avatar_vertex_filter_group_count: modular_avatar_vertex_filter_groups.len(),
		modular_avatar_vertex_filter_groups,
		base_set,
		wardrobe_set_count: sets.map(Vec::len).unwrap_or(0),
		wardrobe_set_ids,
		asset_group_count: asset_group_ids.len(),
		asset_group_ids,
		wardrobe_sets,
		base_operation_count: base_operations.map(Vec::len).unwrap_or(0),
		base_operation_counts,
	}
}

fn zero_usize(value: &usize) -> bool {
	*value == 0
}

fn build_diagnose_report(
	path: &Path,
	import_format_id: String,
	provider_plugin_id: Option<String>,
	timings: DiagnoseTimingSummary,
	import_report: ImportReport,
	doc: UnaDocument,
	wardrobe_probes: Vec<DiagnoseWardrobeProbeSummary>,
) -> DiagnoseReport {
	let mut warnings = Vec::new();
	for diagnostic in &import_report.diagnostics {
		if diagnostic.severity == un_avatar_core::ReportSeverity::Warning {
			warnings.push(format!("import warning: {}", diagnostic.text));
		}
	}
	for lost in &import_report.lost_features {
		let detail = lost.detail.as_deref().unwrap_or("");
		if detail.is_empty() {
			warnings.push(format!("import lost feature: {}", lost.feature));
		} else {
			warnings.push(format!("import lost feature: {} ({detail})", lost.feature));
		}
	}
	for approximation in &import_report.approximations {
		let detail = approximation.detail.as_deref().unwrap_or("");
		if detail.is_empty() {
			warnings.push(format!("import approximation: {}", approximation.feature));
		} else {
			warnings.push(format!("import approximation: {} ({detail})", approximation.feature));
		}
	}
	let runtime_model = doc.runtime_model();
	let scene = if let Some(sc) = runtime_model.scene() {
		let mut shading_counts = BTreeMap::new();
		let mut alpha_counts = BTreeMap::new();
		let mut eye_like_material_indices = Vec::new();
		let mut materials = Vec::new();
		let mut texture_alpha_cache = BTreeMap::new();
		let mut liltoon_material_count = 0usize;
		let mut liltoon_missing_render_queue = 0usize;
		let mut liltoon_missing_source_params = 0usize;
		let mut liltoon_feature_counts = BTreeMap::new();
		let mut suspicious_liltoon_masks = Vec::new();
		let mut fully_transparent_visible_materials = Vec::new();
		for (i, material) in sc.materials.iter().enumerate() {
			bump_count(&mut shading_counts, format!("{:?}", material.shading));
			bump_count(&mut alpha_counts, format!("{:?}", material.alpha_mode));
			if material_source_shader_is_liltoon(material) {
				liltoon_material_count += 1;
				for feature in material_liltoon_features(material) {
					bump_count(&mut liltoon_feature_counts, feature);
				}
				if material
					.unavatar_material
					.as_ref()
					.and_then(|m| m.get("renderQueue").or_else(|| m.get("render_queue")))
					.is_none()
				{
					liltoon_missing_render_queue += 1;
				}
				if !material_has_source_params(material) {
					liltoon_missing_source_params += 1;
				}
				if material.alpha_mode == UnaAlphaMode::Mask && material.alpha_cutoff > 0.01 {
					suspicious_liltoon_masks.push(i);
				}
			}
			if matches!(material.alpha_mode, UnaAlphaMode::Mask | UnaAlphaMode::Blend)
				&& material.base_color_factor[3] <= 0.001
				&& texture_alpha_summary_cached(sc, &mut texture_alpha_cache, material.base_color_texture_index)
					.is_some_and(|alpha| alpha.max_alpha == 0)
			{
				fully_transparent_visible_materials.push(i);
			}
			if eye_like_material_name(material.name.as_deref()) {
				eye_like_material_indices.push(i);
				if material.alpha_mode == UnaAlphaMode::Mask && material.base_color_factor[3] <= 0.001 {
					warnings.push(format!(
						"eye-like material[{i}] is MASK with near-zero material alpha; consider --relax-iris-alpha"
					));
				}
			}
			materials.push(material_summary(i, material, sc, &mut texture_alpha_cache));
		}
		if liltoon_material_count > 0 && liltoon_missing_render_queue > 0 {
			warnings.push(format!(
				"lilToon material source payload is missing renderQueue on {liltoon_missing_render_queue}/{liltoon_material_count} materials; re-export with the current Unity exporter to improve alpha/order diagnostics"
			));
		}
		if liltoon_material_count > 0 && liltoon_missing_source_params > 0 {
			warnings.push(format!(
				"lilToon material source payload is missing floatParams/colorParams on {liltoon_missing_source_params}/{liltoon_material_count} materials; re-export with the current Unity exporter before UNToon compatibility tuning"
			));
		}
		if !suspicious_liltoon_masks.is_empty() {
			warnings.push(format!(
				"lilToon materials with MASK alpha and cutoff > 0.01: {:?}; verify these are actual Cutout materials, not ordinary Opaque shaders with _Cutoff",
				suspicious_liltoon_masks
			));
		}
		let high_risk_hits = ["fur", "refraction", "gem", "twopass"]
			.iter()
			.filter_map(|feature| {
				liltoon_feature_counts
					.get(*feature)
					.copied()
					.filter(|count| *count > 0)
					.map(|count| (*feature, count))
			})
			.collect::<Vec<_>>();
		if !high_risk_hits.is_empty() {
			warnings.push(format!(
				"lilToon high-variance shader features present: {:?}; verify these against Unity because they depend on extra passes or screen/environment inputs",
				high_risk_hits
			));
		}
		if !fully_transparent_visible_materials.is_empty() {
			warnings.push(format!(
				"fully transparent alpha materials are present: {:?}; renderer may skip these draws unless used as authoring helpers",
				fully_transparent_visible_materials
			));
		}
		if doc.vrm.is_some()
			&& !sc.materials.is_empty()
			&& !sc
				.materials
				.iter()
				.any(|m| m.runtime_toon_model() == Some(UnaRuntimeToonModel::UNToon))
		{
			warnings.push("VRM document has no UNToon runtime materials after import".to_string());
		}
		let mut image_source_mime_counts = BTreeMap::new();
		let mut image_source_color_space_counts = BTreeMap::new();
		let mut image_source_texture_type_counts = BTreeMap::new();
		let mut image_source_texture_shape_counts = BTreeMap::new();
		let mut image_source_layout_counts = BTreeMap::new();
		let mut image_pixel_format_counts = BTreeMap::new();
		let mut image_source_count = 0usize;
		let mut image_source_bytes = 0u64;
		let mut largest_image_sources = Vec::new();
		for (index, image) in sc.images.iter().enumerate() {
			bump_count(&mut image_pixel_format_counts, format!("{:?}", image.pixel_format));
			if let Some(source) = sc.image_sources.get(index).and_then(Option::as_ref) {
				let is_cube_source = diagnose_texture_shape_is_cube(source.texture_shape.as_deref());
				largest_image_sources.push(DiagnoseImageSourceSummary {
					index,
					name: source.name.clone(),
					mime_type: source.mime_type.clone(),
					uri: source.uri.clone(),
					source_pixel_format: source.source_pixel_format.clone(),
					channels: source.channels.clone(),
					color_space: source.color_space.clone(),
					texture_type: source.texture_type.clone(),
					texture_shape: source.texture_shape.clone(),
					source_layout: is_cube_source.then(|| source.source_layout.clone()).flatten(),
					unity_generate_cubemap: is_cube_source.then(|| source.unity_generate_cubemap.clone()).flatten(),
					srgb: source.srgb,
					byte_length: source.byte_length,
					pixel_format: image.pixel_format,
					width: image.width,
					height: image.height,
				});
			}
		}
		largest_image_sources.sort_by(|a, b| b.byte_length.cmp(&a.byte_length).then_with(|| a.index.cmp(&b.index)));
		largest_image_sources.truncate(12);
		for source in sc.image_sources.iter().flatten() {
			image_source_count += 1;
			image_source_bytes = image_source_bytes.saturating_add(source.byte_length);
			bump_count(
				&mut image_source_mime_counts,
				source.mime_type.as_deref().unwrap_or("unknown").to_string(),
			);
			bump_count(
				&mut image_source_color_space_counts,
				source.color_space.as_deref().unwrap_or("unknown").to_string(),
			);
			bump_count(
				&mut image_source_texture_type_counts,
				source.texture_type.as_deref().unwrap_or("unknown").to_string(),
			);
			bump_count(
				&mut image_source_texture_shape_counts,
				source.texture_shape.as_deref().unwrap_or("unknown").to_string(),
			);
			bump_count(
				&mut image_source_layout_counts,
				source.source_layout.as_deref().unwrap_or("unknown").to_string(),
			);
		}
		let effective_visibility = scene_effective_visibility(sc);
		let node_paths_by_index = scene_node_paths_by_index(sc);
		let mut visible_material_indices = Vec::new();
		let mut visible_shading_counts = BTreeMap::new();
		let mut visible_alpha_counts = BTreeMap::new();
		let visible_mesh_nodes = sc
			.nodes
			.iter()
			.enumerate()
			.filter(|(idx, _)| effective_visibility.get(*idx).copied().unwrap_or(false))
			.filter_map(|(idx, node)| {
				if let Some(mesh) = node.mesh {
					if let Some(primitives) = sc.meshes.get(mesh) {
						for primitive in primitives {
							let Some(material_index) = primitive.material_index else { continue };
							let Some(material) = sc.materials.get(material_index) else {
								continue;
							};
							visible_material_indices.push(material_index);
							bump_count(&mut visible_shading_counts, format!("{:?}", material.shading));
							bump_count(&mut visible_alpha_counts, format!("{:?}", material.alpha_mode));
						}
					}
				}
				node.mesh.map(|mesh| DiagnoseVisibleMeshNodeSummary {
					node: idx,
					name: node.name.clone(),
					path: node_paths_by_index.get(idx).cloned().flatten(),
					source_node_id: node.source_node_id.clone(),
					resolved_node_id: node.resolved_node_id.clone(),
					mesh,
					skin: node.skin,
					materials: visible_mesh_materials(sc, mesh, &mut texture_alpha_cache),
				})
			})
			.collect();
		visible_material_indices.sort_unstable();
		visible_material_indices.dedup();
		let skins = skin_summaries(sc);
		let skin_over_limit = skins
			.iter()
			.filter(|skin| skin.over_renderer_bone_limit)
			.map(|skin| skin.index)
			.collect::<Vec<_>>();
		if !skin_over_limit.is_empty() {
			warnings.push(format!(
				"skins exceed renderer bone palette limit: {:?}; affected vertices will be clamped unless the renderer limit or skin split is improved",
				skin_over_limit
			));
		}
		let skin_mismatched_attrs = skins
			.iter()
			.filter(|skin| skin.mismatched_joint_weight_attribute_count > 0)
			.map(|skin| (skin.index, skin.mismatched_joint_weight_attribute_count))
			.collect::<Vec<_>>();
		if !skin_mismatched_attrs.is_empty() {
			warnings.push(format!(
				"skins have primitives with mismatched JOINTS/WEIGHTS attributes: {:?}; verify exporter and source mesh skinning data",
				skin_mismatched_attrs
			));
		}
		let skin_out_of_range = skins
			.iter()
			.filter(|skin| skin.out_of_range_joint_attribute_count > 0)
			.map(|skin| (skin.index, skin.out_of_range_joint_attribute_count))
			.collect::<Vec<_>>();
		if !skin_out_of_range.is_empty() {
			warnings.push(format!(
				"skins have primitives with joint indices outside effective palette: {:?}; renderer clamps these vertices",
				skin_out_of_range
			));
		}
		let mut node_constraint_kind_counts = BTreeMap::new();
		let mut parent_node_constraint_source_count = 0usize;
		let mut parent_node_constraint_multi_source_count = 0usize;
		for constraint in &sc.node_constraints {
			let kind = diagnose_node_constraint_kind(&constraint.kind);
			bump_count(&mut node_constraint_kind_counts, kind.to_string());
			if matches!(constraint.kind, UnaNodeConstraintKind::Parent { .. }) {
				let source_count = if constraint.sources.is_empty() {
					1
				} else {
					constraint.sources.len()
				};
				parent_node_constraint_source_count += source_count;
				if source_count > 1 {
					parent_node_constraint_multi_source_count += 1;
				}
			}
		}
		let asset_ownership_counts = sc.asset_group_ownership_counts();
		let scoped_asset_selection = doc.scoped_asset_selection();
		let asset_group_ownership = sc
			.asset_group_ownership
			.iter()
			.map(|group| DiagnoseAssetGroupOwnershipSummary {
				group_id: group.group_id.clone(),
				mesh_primitives: group.mesh_primitives.clone(),
				materials: group.materials.clone(),
				images: group.images.clone(),
				dynamics_source_ids: group.dynamics_source_ids.clone(),
			})
			.collect();
		DiagnoseSceneSummary {
			has_scene: true,
			mesh_count: sc.meshes.len(),
			primitive_count: sc.meshes.iter().map(Vec::len).sum(),
			morph_target_count: sc.meshes.iter().flatten().map(|primitive| primitive.morph_targets.len()).sum(),
			node_count: sc.nodes.len(),
			hidden_node_count: sc.nodes.iter().filter(|node| !node.visible).count(),
			skin_count: sc.skins.len(),
			image_count: sc.images.len(),
			image_source_count,
			image_source_bytes,
			image_source_mime_counts,
			image_source_color_space_counts,
			image_source_texture_type_counts,
			image_source_texture_shape_counts,
			image_source_layout_counts,
			image_pixel_format_counts,
			non_rgba8_image_count: sc
				.images
				.iter()
				.filter(|image| image.pixel_format != UnaImagePixelFormat::R8G8B8A8)
				.count(),
			largest_image_sources,
			material_count: sc.materials.len(),
			liltoon_feature_counts,
			node_constraint_count: sc.node_constraints.len(),
			node_constraint_kind_counts,
			parent_node_constraint_source_count,
			parent_node_constraint_multi_source_count,
			asset_group_ownership_count: asset_ownership_counts.groups,
			asset_group_owned_mesh_primitive_count: asset_ownership_counts.mesh_primitives,
			asset_group_owned_material_count: asset_ownership_counts.materials,
			asset_group_owned_image_count: asset_ownership_counts.images,
			asset_group_owned_dynamics_count: asset_ownership_counts.dynamics,
			asset_group_ownership,
			scoped_active_asset_group_count: scoped_asset_selection.owned_active_groups.len(),
			scoped_missing_active_asset_groups: scoped_asset_selection.missing_active_asset_groups,
			scoped_resident_mesh_primitive_count: scoped_asset_selection.mesh_primitives.len(),
			scoped_resident_material_count: scoped_asset_selection.materials.len(),
			scoped_resident_image_count: scoped_asset_selection.images.len(),
			scoped_resident_dynamics_count: scoped_asset_selection.dynamics_source_ids.len(),
			shading_counts,
			alpha_counts,
			visible_shading_counts,
			visible_alpha_counts,
			visible_material_indices,
			eye_like_material_indices,
			skins,
			materials,
			visible_mesh_nodes,
		}
	} else {
		warnings.push("imported document has no scene".to_string());
		let scoped_asset_selection = doc.scoped_asset_selection();
		DiagnoseSceneSummary {
			has_scene: false,
			mesh_count: 0,
			primitive_count: 0,
			morph_target_count: 0,
			node_count: 0,
			hidden_node_count: 0,
			skin_count: 0,
			image_count: 0,
			image_source_count: 0,
			image_source_bytes: 0,
			image_source_mime_counts: BTreeMap::new(),
			image_source_color_space_counts: BTreeMap::new(),
			image_source_texture_type_counts: BTreeMap::new(),
			image_source_texture_shape_counts: BTreeMap::new(),
			image_source_layout_counts: BTreeMap::new(),
			image_pixel_format_counts: BTreeMap::new(),
			non_rgba8_image_count: 0,
			largest_image_sources: Vec::new(),
			material_count: 0,
			liltoon_feature_counts: BTreeMap::new(),
			node_constraint_count: 0,
			node_constraint_kind_counts: BTreeMap::new(),
			parent_node_constraint_source_count: 0,
			parent_node_constraint_multi_source_count: 0,
			asset_group_ownership_count: 0,
			asset_group_owned_mesh_primitive_count: 0,
			asset_group_owned_material_count: 0,
			asset_group_owned_image_count: 0,
			asset_group_owned_dynamics_count: 0,
			asset_group_ownership: Vec::new(),
			scoped_active_asset_group_count: scoped_asset_selection.owned_active_groups.len(),
			scoped_missing_active_asset_groups: scoped_asset_selection.missing_active_asset_groups,
			scoped_resident_mesh_primitive_count: scoped_asset_selection.mesh_primitives.len(),
			scoped_resident_material_count: scoped_asset_selection.materials.len(),
			scoped_resident_image_count: scoped_asset_selection.images.len(),
			scoped_resident_dynamics_count: scoped_asset_selection.dynamics_source_ids.len(),
			shading_counts: BTreeMap::new(),
			alpha_counts: BTreeMap::new(),
			visible_shading_counts: BTreeMap::new(),
			visible_alpha_counts: BTreeMap::new(),
			visible_material_indices: Vec::new(),
			eye_like_material_indices: Vec::new(),
			skins: Vec::new(),
			materials: Vec::new(),
			visible_mesh_nodes: Vec::new(),
		}
	};

	let runtime_model = doc.runtime_model();
	let humanoid = runtime_model.humanoid_profile().map(|profile| {
		let keys: Vec<String> = profile.bone_node_indices.keys().cloned().collect();
		DiagnoseHumanoidSummary {
			bone_count: profile.bone_node_indices.len(),
			left_eye_node: profile.bone_node_indices.get("lefteye").copied(),
			right_eye_node: profile.bone_node_indices.get("righteye").copied(),
			keys,
		}
	});
	if doc.vrm.is_some() && humanoid.is_none() {
		warnings.push("VRM document has no humanoid profile".to_string());
	}

	let expression_apply_probe = expression_apply_probe(&doc);
	let expressions = runtime_model.expression_catalog().map(|catalog| DiagnoseExpressionSummary {
		preset_count: catalog.presets.len(),
		presets: catalog
			.presets
			.iter()
			.map(|preset| DiagnoseExpressionPresetSummary {
				name: preset.name.clone(),
				bind_count: preset.binds.len(),
			})
			.collect(),
		apply_probe: expression_apply_probe,
	});
	let actions = runtime_model.runtime_actions().map(|actions| DiagnoseActionSummary {
		action_count: actions.actions.len(),
		trigger_count: actions.actions.iter().map(|action| action.triggers.len()).sum(),
		effect_count: actions.actions.iter().map(|action| action.effects.len()).sum(),
		trigger_kinds: runtime_action_trigger_kind_counts(actions.actions.iter().flat_map(|action| action.triggers.iter())),
		effect_kinds: runtime_action_effect_kind_counts(actions.actions.iter().flat_map(|action| action.effects.iter())),
		target_write_collisions: actions.evaluation_target_write_collisions(),
		restore_readiness: runtime_model.runtime_action_set_restore_readiness(actions),
		restore_baseline_candidates: runtime_model.runtime_action_set_restore_baseline_candidates(actions),
		restore_baseline_capture_plan: runtime_model.runtime_action_set_restore_baseline_capture_plan(actions),
		restore_apply_plan: runtime_model.runtime_action_set_restore_apply_plan(actions),
		actions: actions
			.actions
			.iter()
			.map(|action| DiagnoseActionItemSummary {
				id: action.id.clone(),
				label: action.label.clone(),
				trigger_count: action.triggers.len(),
				condition_count: action.conditions.len(),
				effect_count: action.effects.len(),
				trigger_kinds: runtime_action_trigger_kind_counts(action.triggers.iter()),
				parameter_triggers: runtime_action_parameter_triggers(action.triggers.iter()),
				condition_parameter_names: action.condition_parameter_names(),
				current_condition_state: action
					.current_parameter_condition_state(runtime_model.scene(), runtime_model.runtime_parameter_values())
					.map(str::to_string),
				conditions: runtime_action_conditions(action.conditions.iter()),
				effect_kinds: runtime_action_effect_kind_counts(action.effects.iter()),
				target_writes: action.evaluation_target_writes(),
				node_visibility_effects: runtime_action_node_visibility_effects(action.effects.iter()),
				material_property_effects: runtime_action_material_property_effects(action.effects.iter()),
				material_slot_effects: runtime_action_material_slot_effects(action.effects.iter()),
				expression_weight_effects: runtime_action_expression_weight_effects(action.effects.iter()),
				dynamics_enabled_effects: runtime_action_dynamics_enabled_effects(action.effects.iter()),
			})
			.collect(),
	});

	let runtime = DiagnoseRuntimeSummary {
		source_kind: runtime_model.source_kind(),
		humanoid_basis: runtime_model.humanoid_basis(),
		active_wardrobe_set: runtime_model.active_wardrobe_set().map(str::to_owned),
		active_asset_groups: runtime_model.active_asset_groups().to_vec(),
		last_action_id: runtime_model.last_action_id().map(str::to_owned),
		parameter_values: runtime_model.runtime_parameter_values().clone(),
		parameter_definitions: runtime_model.runtime_parameter_definitions(),
		parameter_conflicts: runtime_model.runtime_parameter_conflicts(),
		resolver_cache_key: runtime_model.resolver_cache_key(),
	};
	let runtime_dynamics = runtime_model.dynamics();
	let dynamics_groups = dynamics_group_summaries(&doc);
	let dynamics_response_categories = dynamics_response_category_summaries(&doc);
	let dynamics_response_groups = dynamics_response_group_summaries(&doc);
	let dynamics_colliders = dynamics_collider_summaries(&doc);
	let dynamics_contacts = dynamics_contact_summaries(&doc);
	let dynamics_contact_parameter_declarations = dynamics_contact_parameter_declaration_summaries(&doc);
	let dynamics_contact_probes = dynamics_contact_probe_summaries(&doc);
	let dynamics_contact_parameter_emissions = dynamics_contact_parameter_emission_summaries(&doc);
	let dynamics_constraint_refs = dynamics_constraint_ref_summaries(&doc);
	let dynamics_interaction_hooks = dynamics_interaction_hook_summaries(&doc);
	for (source_id, count) in duplicate_dynamics_source_ids(&dynamics_groups) {
		warnings.push(format!(
			"dynamics source_id {source_id:?} lowers to {count} runtime groups; wardrobe/action dynamics toggles may affect multiple chains"
		));
	}
	for (source_id, count) in duplicate_dynamics_contact_source_ids(&dynamics_contacts) {
		warnings.push(format!(
			"dynamics contact source_id {source_id:?} lowers to {count} runtime contacts; contact diagnostics may merge distinct VRC Contact components"
		));
	}
	for (source_id, count) in duplicate_dynamics_constraint_ref_source_ids(&dynamics_constraint_refs) {
		warnings.push(format!(
			"dynamics constraint_ref source_id {source_id:?} lowers to {count} runtime constraint refs; constraint diagnostics may merge distinct constraint sources"
		));
	}
	let dynamics_source_ids = dynamics_groups
		.iter()
		.filter_map(|group| (!group.source_id.is_empty()).then(|| group.source_id.clone()))
		.collect::<BTreeSet<_>>();
	let active_dynamics_source_ids = dynamics_groups
		.iter()
		.filter(|group| group.enabled)
		.filter_map(|group| (!group.source_id.is_empty()).then(|| group.source_id.clone()))
		.collect::<BTreeSet<_>>();
	for target_id in wardrobe_dynamics_enable_targets(&doc) {
		if !dynamics_source_ids.contains(&target_id) {
			warnings.push(format!(
				"wardrobe dynamicsEnable target {target_id:?} does not match any runtime dynamics group source_id"
			));
		}
	}
	for (action_id, target_id) in runtime_action_dynamics_enable_targets(runtime_model.runtime_actions()) {
		if !dynamics_source_ids.contains(&target_id) {
			warnings.push(format!(
				"runtime action {action_id:?} DynamicsEnabled target {target_id:?} does not match any runtime dynamics group source_id"
			));
		}
	}
	let dynamics_source_features = dynamics_source_feature_counts(&doc);
	let dynamics_source_collider_audit = dynamics_source_collider_audit(&doc, Some(&active_dynamics_source_ids));
	if dynamics_source_features.unknown_shape_collider_count > 0 {
		warnings.push(format!(
			"raw dynamics source colliders include {} unknown shape collider(s); unsupported PhysBone collider shapes will not affect the solver",
			dynamics_source_features.unknown_shape_collider_count
		));
	}
	if dynamics_source_collider_audit.collision_enabled_empty_collider_count > 0 {
		let source_local_empty_ids = dynamics_source_collider_audit
			.collision_enabled_empty_collider_source_ids
			.iter()
			.cloned()
			.collect::<BTreeSet<_>>();
		let runtime_no_contact_groups = dynamics_groups
			.iter()
			.filter(|group| source_local_empty_ids.contains(&group.source_id))
			.filter(|group| group.selected_runtime_collider_count == 0 && group.hit_radius <= 0.0)
			.collect::<Vec<_>>();
		let runtime_no_contact_source_ids = runtime_no_contact_groups
			.iter()
			.map(|group| group.source_id.clone())
			.collect::<BTreeSet<_>>();
		let runtime_no_contact_samples = runtime_no_contact_source_ids.iter().take(8).cloned().collect::<Vec<_>>();
		let runtime_no_contact_samples = format_warning_samples(&runtime_no_contact_samples);
		let source_local_empty_samples = format_warning_samples(&dynamics_source_collider_audit.collision_enabled_empty_collider_samples);
		warnings.push(format!(
			"active VRC PhysBone sources set allowCollision=true but define no source-local colliders; with hitRadius=0 these lower to no runtime contact candidates. sources={} runtime_no_contact_sources={} runtime_no_contact_groups={} no_contact{} source_local_empty{}",
			dynamics_source_collider_audit.collision_enabled_empty_collider_count,
			runtime_no_contact_source_ids.len(),
			runtime_no_contact_groups.len(),
			runtime_no_contact_samples,
			source_local_empty_samples
		));
	}
	let dynamics_counts = runtime_dynamics.counts();
	let rotation_translation_writeback_groups = dynamics_groups
		.iter()
		.filter(|group| group.writeback_mode == un_avatar_core::UnaDynamicsWritebackMode::RotationTranslation)
		.collect::<Vec<_>>();
	let unsupported_writeback_groups = dynamics_unsupported_writeback_groups(&dynamics_groups);
	let rotation_translation_writeback_group_count = rotation_translation_writeback_groups.len();
	let translation_writeback_candidate_count = dynamics_translation_writeback_candidate_total(&rotation_translation_writeback_groups);
	let translation_writeback_target_count = dynamics_translation_writeback_target_total(&rotation_translation_writeback_groups);
	let effective_stretch_limit_group_count = dynamics_groups
		.iter()
		.filter(|group| dynamics_group_has_length_limit(group))
		.count();
	let stretch_translation_writeback_group_count = dynamics_stretch_translation_writeback_group_count(&dynamics_groups);
	let stretch_translation_writeback_target_group_count = dynamics_stretch_translation_writeback_target_group_count(&dynamics_groups);
	if dynamics_counts.groups > 0 && dynamics_counts.enabled_groups == 0 {
		let samples = dynamics_group_samples(&dynamics_groups);
		warnings.push(format!(
			"dynamics groups are present but none are currently enabled; groups={} source_enabled_groups={} runtime_enabled_overrides={}{}",
			dynamics_counts.groups,
			dynamics_counts.source_enabled_groups,
			dynamics_counts.runtime_enabled_overrides,
			format_warning_samples(&samples)
		));
	}
	if dynamics_source_features.stretch_limit_count > 0 || effective_stretch_limit_group_count > 0 {
		let samples = dynamics_stretch_limit_samples(&dynamics_groups);
		if effective_stretch_limit_group_count > 0 {
			warnings.push(format!(
				"dynamics stretch limits are supported as simulation stretch; safe targets also use translation writeback while targetless groups keep node translations unchanged; source_stretch_limits={} runtime_stretch_limit_groups={} writeback_target_groups={}{}",
				dynamics_source_features.stretch_limit_count,
				effective_stretch_limit_group_count,
				stretch_translation_writeback_target_group_count,
				format_warning_samples(&samples)
			));
		} else {
			warnings.push(format!(
				"dynamics stretch/squish limits are authored in source data but currently have no effective runtime length range, usually because scalar/curve stretchMotion is zero or no positive stretch/squish range was lowered; source_stretch_limits={} runtime_stretch_limit_groups=0",
				dynamics_source_features.stretch_limit_count
			));
		}
	}
	let large_stretch_range_groups = dynamics_large_stretch_range_groups(&dynamics_groups);
	if !large_stretch_range_groups.is_empty() {
		let samples = dynamics_large_stretch_range_samples(&large_stretch_range_groups);
		warnings.push(format!(
			"dynamics stretch range has very large authored/effective multiplier; this is allowed source intent but can produce large chain-relative lag and should be verified numerically. groups={}{}",
			large_stretch_range_groups.len(),
			format_warning_samples(&samples)
		));
	}
	if !unsupported_writeback_groups.is_empty() {
		let samples = dynamics_unsupported_writeback_samples(&dynamics_groups);
		let unsupported_candidate_count = dynamics_translation_writeback_candidate_total(&unsupported_writeback_groups);
		let unsupported_target_count = dynamics_translation_writeback_target_total(&unsupported_writeback_groups);
		warnings.push(format!(
			"dynamics rotation_translation writeback has no safe translation target in the current solver; groups={} candidate_joints={} target_joints={}{}",
			unsupported_writeback_groups.len(),
			unsupported_candidate_count,
			unsupported_target_count,
			format_warning_samples(&samples)
		));
	}
	if dynamics_source_features.radius_curve_count > 0 {
		if dynamics_counts.groups > 0 {
			warnings.push(format!(
				"dynamics radius curves are approximated as per-joint hit radius in the current solver; source_radius_curves={}",
				dynamics_source_features.radius_curve_count
			));
		} else {
			warnings.push(format!(
				"dynamics radius curves are metadata-only in the current solver because no runtime dynamics groups were lowered; source_radius_curves={}",
				dynamics_source_features.radius_curve_count
			));
		}
	}
	let metadata_only_interaction_hook_count = dynamics_interaction_hooks.iter().filter(|hook| hook.metadata_only).count();
	let source_only_interaction_hook_count = if metadata_only_interaction_hook_count == 0 && dynamics_interaction_hooks.is_empty() {
		dynamics_source_features.grabbing_enabled_count + dynamics_source_features.posing_enabled_count
	} else {
		0
	};
	let metadata_only_or_source_only_interaction_hook_count = metadata_only_interaction_hook_count + source_only_interaction_hook_count;
	if metadata_only_or_source_only_interaction_hook_count > 0 {
		let samples = dynamics_interaction_hook_samples(&dynamics_interaction_hooks);
		warnings.push(format!(
			"dynamics grabbing/posing interaction hooks without parameters are metadata-only in the current solver; hooks={}{}",
			metadata_only_or_source_only_interaction_hook_count,
			format_warning_samples(&samples)
		));
	}
	if dynamics_counts.vrc_constraint_refs > 0 {
		let samples = dynamics_constraint_ref_samples(&dynamics_constraint_refs);
		warnings.push(format!(
			"dynamics VRC constraint refs are metadata/reset refs only in the current solver; vrc_constraint_refs={}{}",
			dynamics_counts.vrc_constraint_refs,
			format_warning_samples(&samples)
		));
	}
	let contact_probe_would_emit_count = dynamics_contact_probes.iter().filter(|probe| probe.would_emit).count();
	let contact_parameter_emission_enabled = doc.runtime_model().contact_parameter_emission_enabled();
	if contact_probe_would_emit_count > 0 && !contact_parameter_emission_enabled {
		let samples = dynamics_contact_probe_samples(&dynamics_contact_probes);
		warnings.push(format!(
			"dynamics contact probes would emit {contact_probe_would_emit_count} parameter value(s), but contact parameter emission is disabled{}",
			format_warning_samples(&samples)
		));
	}
	let dynamics = DiagnoseDynamicsSummary {
		group_count: dynamics_counts.groups,
		vrm_spring_bone_group_count: dynamics_counts.vrm_spring_bone_groups,
		vrc_physbone_group_count: dynamics_counts.vrc_physbone_groups,
		unknown_group_count: dynamics_counts.unknown_groups,
		limit_group_count: dynamics_counts.limit_groups,
		angle_limit_group_count: dynamics_counts.angle_limit_groups,
		stretch_limit_group_count: effective_stretch_limit_group_count,
		rotation_translation_writeback_group_count,
		translation_writeback_candidate_count,
		translation_writeback_target_count,
		stretch_translation_writeback_group_count,
		stretch_translation_writeback_target_group_count,
		grabbing_enabled_group_count: dynamics_counts.grabbing_enabled_groups,
		posing_enabled_group_count: dynamics_counts.posing_enabled_groups,
		collider_count: dynamics_counts.colliders,
		vrm_spring_bone_collider_count: dynamics_counts.vrm_spring_bone_colliders,
		vrc_physbone_collider_count: dynamics_counts.vrc_physbone_colliders,
		unknown_collider_count: dynamics_counts.unknown_colliders,
		contact_count: dynamics_counts.contacts,
		vrc_contact_sender_count: dynamics_counts.vrc_contact_senders,
		vrc_contact_receiver_count: dynamics_counts.vrc_contact_receivers,
		contact_parameter_declaration_count: dynamics_counts.contact_parameter_declarations,
		contact_parameter_emission_enabled,
		contact_probe_count: dynamics_contact_probes.len(),
		contact_probe_would_emit_count,
		contact_parameter_emission_count: dynamics_contact_parameter_emissions.len(),
		contact_parameter_emitted_count: dynamics_contact_parameter_emissions
			.iter()
			.filter(|emission| emission.emitted)
			.count(),
		contact_parameter_reset_to_zero_count: dynamics_contact_parameter_emissions
			.iter()
			.filter(|emission| !emission.emitted)
			.count(),
		constraint_ref_count: dynamics_counts.constraint_refs,
		vrc_constraint_ref_count: dynamics_counts.vrc_constraint_refs,
		source_limit_count: dynamics_source_features.limit_count,
		source_angle_limit_count: dynamics_source_features.angle_limit_count,
		source_stretch_limit_count: dynamics_source_features.stretch_limit_count,
		source_curve_count: dynamics_source_features.curve_count,
		source_radius_curve_count: dynamics_source_features.radius_curve_count,
		source_angle_limit_curve_count: dynamics_source_features.angle_limit_curve_count,
		source_stretch_limit_curve_count: dynamics_source_features.stretch_limit_curve_count,
		source_collider_count: dynamics_source_features.collider_count,
		source_unknown_shape_collider_count: dynamics_source_features.unknown_shape_collider_count,
		source_collision_disabled_count: dynamics_source_features.collision_disabled_count,
		source_inside_bounds_collider_count: dynamics_source_features.inside_bounds_collider_count,
		source_grabbing_enabled_count: dynamics_source_features.grabbing_enabled_count,
		source_posing_enabled_count: dynamics_source_features.posing_enabled_count,
		source_interaction_parameter_count: dynamics_source_features.interaction_parameter_count,
		colliders: dynamics_colliders,
		contacts: dynamics_contacts,
		contact_parameter_declarations: dynamics_contact_parameter_declarations,
		contact_probes: dynamics_contact_probes,
		contact_parameter_emissions: dynamics_contact_parameter_emissions,
		constraint_refs: dynamics_constraint_refs,
		interaction_hooks: dynamics_interaction_hooks,
		groups: dynamics_groups,
		response_categories: dynamics_response_categories,
		response_groups: dynamics_response_groups,
	};
	let vrm = doc.vrm.as_ref().map(|vrm| DiagnoseVrmSummary {
		spec_version: vrm.spec_version.clone(),
		mtoon_materials_v0: vrm.mtoon_materials_v0.len(),
		mtoon_material_indices_v1: vrm.mtoon_material_indices_v1.clone(),
		spring_group_count: dynamics.vrm_spring_bone_group_count,
	});
	let unavatar = doc.unavatar.as_ref().map(unavatar_summary);
	let menu_action_candidates = diagnose_menu_action_candidates(unavatar.as_ref(), runtime_model.runtime_actions());
	let menu_wardrobe_candidates = diagnose_menu_wardrobe_candidates(unavatar.as_ref(), &menu_action_candidates);
	if let Some(unavatar) = &unavatar {
		for set in &unavatar.wardrobe_sets {
			let is_base_set = unavatar.base_set.as_deref() == Some(set.id.as_str());
			if !is_base_set && set.operation_count > 0 && set.asset_groups.is_empty() {
				warnings.push(format!(
					"wardrobe set {:?} has {} operation(s) but no assetGroups; lazy GPU upload cannot scope this set yet",
					set.id, set.operation_count
				));
			}
		}
	}
	if let Some(unavatar) = &unavatar {
		if unavatar.dynamics_entry_count > 0 && dynamics.group_count == 0 {
			warnings.push(format!(
				".unavatar has {} raw dynamics entries but no runtime dynamics groups; check dynamics root node references and importer lowering",
				unavatar.dynamics_entry_count
			));
		}
	}

	DiagnoseReport {
		path: path.to_string_lossy().to_string(),
		import_format_id,
		import_provider_plugin_id: provider_plugin_id,
		timings,
		import_report,
		runtime,
		scene,
		humanoid,
		expressions,
		actions,
		menu_action_candidates,
		menu_wardrobe_candidates,
		dynamics,
		vrm,
		unavatar,
		wardrobe_probes,
		warnings,
	}
}

fn expression_apply_probe(doc: &UnaDocument) -> Option<DiagnoseExpressionApplyProbe> {
	doc.runtime_model().expression_catalog()?;
	let mut doc = expression_probe_document(doc);
	let mut frame = un_motion_frame::UNMotionFrame::new(0);
	frame.face = Some(un_motion_frame::FaceMotion {
		tracking_state: un_motion_frame::TrackingState::Valid,
		confidence: 1.0,
		head: None,
		expressions: vec![
			un_motion_frame::ExpressionSample {
				name: "jawOpen".to_string(),
				value: 0.6,
				confidence: 1.0,
				source_index: None,
				state: un_motion_frame::SampleState::Valid,
			},
			un_motion_frame::ExpressionSample {
				name: "mouthPucker".to_string(),
				value: 0.4,
				confidence: 1.0,
				source_index: None,
				state: un_motion_frame::SampleState::Valid,
			},
			un_motion_frame::ExpressionSample {
				name: "mouthSmileLeft".to_string(),
				value: 0.8,
				confidence: 1.0,
				source_index: None,
				state: un_motion_frame::SampleState::Valid,
			},
			un_motion_frame::ExpressionSample {
				name: "eyeBlinkLeft".to_string(),
				value: 0.7,
				confidence: 1.0,
				source_index: None,
				state: un_motion_frame::SampleState::Valid,
			},
			un_motion_frame::ExpressionSample {
				name: "browDownLeft".to_string(),
				value: 0.5,
				confidence: 1.0,
				source_index: None,
				state: un_motion_frame::SampleState::Valid,
			},
		],
	});
	un_avatar_skeleton::apply_un_motion_frame_to_document(&mut doc, &frame, un_avatar_skeleton::ApplyUnMotionFrameOpts::default());
	let weights: BTreeMap<String, f32> = doc
		.runtime_model()
		.expression_weights()?
		.preset_weights
		.iter()
		.filter_map(|(name, value)| if *value > 0.0001 { Some((name.clone(), *value)) } else { None })
		.collect();
	let mut active_morph_slots = Vec::new();
	let runtime_model = doc.runtime_model();
	if let (Some(runtime), Some(expression_weights)) = (runtime_model.scene_expression_catalog(), runtime_model.expression_weights()) {
		for (mesh_i, primitives) in runtime.scene.meshes.iter().enumerate() {
			for (prim_i, primitive) in primitives.iter().enumerate() {
				let morphs = morph_weights_for_primitive(primitive, runtime.expression_catalog, Some(expression_weights), mesh_i, prim_i);
				let active_count = morphs.iter().filter(|value| **value > 0.0001).count();
				if active_count > 0 {
					let max_weight = morphs.iter().copied().fold(0.0f32, f32::max);
					active_morph_slots.push(DiagnoseExpressionMorphSlot {
						mesh: mesh_i,
						primitive: prim_i,
						active_count,
						max_weight,
					});
				}
			}
		}
	}
	Some(DiagnoseExpressionApplyProbe {
		weights,
		active_morph_slots,
	})
}

fn expression_probe_document(doc: &UnaDocument) -> UnaDocument {
	let scene = doc.scene.as_ref().map(|scene| UnaSceneSnapshot {
		meshes: scene.meshes.clone(),
		materials: scene.materials.clone(),
		images: Vec::new(),
		lighting: scene.lighting.clone(),
		image_sources: Vec::new(),
		skins: scene.skins.clone(),
		nodes: scene.nodes.clone(),
		roots: scene.roots.clone(),
		node_constraints: scene.node_constraints.clone(),
		asset_group_ownership: scene.asset_group_ownership.clone(),
	});
	UnaDocument {
		scene,
		unavatar: None,
		vrm: None,
		humanoid_profile: doc.humanoid_profile.clone(),
		expression_catalog: doc.expression_catalog.clone(),
		expression_weights: doc.expression_weights.clone(),
		runtime_actions: None,
		runtime_state: doc.runtime_state.clone(),
		spring_bones: None,
	}
}

fn runtime_action_effect_kind(effect: &UnaRuntimeActionEffect) -> &'static str {
	match effect {
		UnaRuntimeActionEffect::WardrobeSet { .. } => "wardrobe_set",
		UnaRuntimeActionEffect::NodeVisibility { .. } => "node_visibility",
		UnaRuntimeActionEffect::ExpressionWeight { .. } => "expression_weight",
		UnaRuntimeActionEffect::MaterialColor { .. } => "material_color",
		UnaRuntimeActionEffect::MaterialScalar { .. } => "material_scalar",
		UnaRuntimeActionEffect::MaterialSlot { .. } => "material_slot",
		UnaRuntimeActionEffect::DynamicsEnabled { .. } => "dynamics_enabled",
	}
}

fn runtime_action_trigger_kind(trigger: &UnaRuntimeActionTrigger) -> &'static str {
	match trigger {
		UnaRuntimeActionTrigger::SupervisorCommand { .. } => "supervisor_command",
		UnaRuntimeActionTrigger::ExpressionMenu { .. } => "expression_menu",
		UnaRuntimeActionTrigger::KeyboardShortcut { .. } => "keyboard_shortcut",
		UnaRuntimeActionTrigger::AnimationEvent { .. } => "animation_event",
		UnaRuntimeActionTrigger::ParameterValue { .. } => "parameter_value",
	}
}

fn runtime_action_trigger_kind_counts<'a>(triggers: impl IntoIterator<Item = &'a UnaRuntimeActionTrigger>) -> BTreeMap<String, usize> {
	let mut counts = BTreeMap::new();
	for trigger in triggers {
		*counts.entry(runtime_action_trigger_kind(trigger).to_string()).or_insert(0) += 1;
	}
	counts
}

fn runtime_action_parameter_triggers<'a>(
	triggers: impl IntoIterator<Item = &'a UnaRuntimeActionTrigger>,
) -> Vec<DiagnoseActionParameterTrigger> {
	triggers
		.into_iter()
		.filter_map(|trigger| match trigger {
			UnaRuntimeActionTrigger::ParameterValue { name, value } => Some(DiagnoseActionParameterTrigger {
				name: name.clone(),
				value: *value,
			}),
			_ => None,
		})
		.collect()
}

fn runtime_action_conditions<'a>(
	conditions: impl IntoIterator<Item = &'a un_avatar_core::UnaRuntimeActionCondition>,
) -> Vec<DiagnoseActionConditionSummary> {
	conditions
		.into_iter()
		.map(|condition| DiagnoseActionConditionSummary {
			source_component_id: condition.source_component_id.clone(),
			source_node_id: condition.source_node.as_ref().and_then(|target| target.source_node_id.clone()),
			resolved_node_id: condition.source_node.as_ref().and_then(|target| target.resolved_node_id.clone()),
			path: condition.source_node.as_ref().and_then(|target| target.path.clone()),
			parameter_name: condition.parameter_name.clone(),
			parameter_value: condition.parameter_value,
			sub_parameter_names: condition.sub_parameter_names.clone(),
			inverted: condition.inverted,
			active_parent_count: condition.active_parent_nodes.len(),
		})
		.collect()
}

fn runtime_action_node_visibility_effects<'a>(
	effects: impl IntoIterator<Item = &'a UnaRuntimeActionEffect>,
) -> Vec<DiagnoseActionNodeVisibilityEffect> {
	effects
		.into_iter()
		.filter_map(|effect| match effect {
			UnaRuntimeActionEffect::NodeVisibility { target, visible } => Some(DiagnoseActionNodeVisibilityEffect {
				node_index: target.node_index,
				source_node_id: target.source_node_id.clone(),
				resolved_node_id: target.resolved_node_id.clone(),
				path: target.path.clone(),
				visible: *visible,
			}),
			_ => None,
		})
		.collect()
}

fn runtime_action_material_property_effects<'a>(
	effects: impl IntoIterator<Item = &'a UnaRuntimeActionEffect>,
) -> Vec<DiagnoseActionMaterialPropertyEffect> {
	effects
		.into_iter()
		.filter_map(|effect| match effect {
			UnaRuntimeActionEffect::MaterialColor { target, parameter, color } => Some(DiagnoseActionMaterialPropertyEffect {
				property_kind: "color".to_string(),
				material_index: target.material_index,
				material_name: target.name.clone(),
				parameter: parameter.clone(),
				scalar_value: None,
				color_value: Some(*color),
			}),
			UnaRuntimeActionEffect::MaterialScalar { target, parameter, value } => Some(DiagnoseActionMaterialPropertyEffect {
				property_kind: "scalar".to_string(),
				material_index: target.material_index,
				material_name: target.name.clone(),
				parameter: parameter.clone(),
				scalar_value: Some(*value),
				color_value: None,
			}),
			_ => None,
		})
		.collect()
}

fn runtime_action_material_slot_effects<'a>(
	effects: impl IntoIterator<Item = &'a UnaRuntimeActionEffect>,
) -> Vec<DiagnoseActionMaterialSlotEffect> {
	effects
		.into_iter()
		.filter_map(|effect| match effect {
			UnaRuntimeActionEffect::MaterialSlot { target, material } => Some(DiagnoseActionMaterialSlotEffect {
				node_index: target.node.node_index,
				source_node_id: target.node.source_node_id.clone(),
				resolved_node_id: target.node.resolved_node_id.clone(),
				path: target.node.path.clone(),
				primitive_index: target.primitive_index,
				material_index: material.as_ref().and_then(|material| material.material_index),
				material_name: material.as_ref().and_then(|material| material.name.clone()),
			}),
			_ => None,
		})
		.collect()
}

fn runtime_action_expression_weight_effects<'a>(
	effects: impl IntoIterator<Item = &'a UnaRuntimeActionEffect>,
) -> Vec<DiagnoseActionExpressionWeightEffect> {
	effects
		.into_iter()
		.filter_map(|effect| match effect {
			UnaRuntimeActionEffect::ExpressionWeight { name, weight } => Some(DiagnoseActionExpressionWeightEffect {
				name: name.clone(),
				weight: *weight,
			}),
			_ => None,
		})
		.collect()
}

fn runtime_action_dynamics_enabled_effects<'a>(
	effects: impl IntoIterator<Item = &'a UnaRuntimeActionEffect>,
) -> Vec<DiagnoseActionDynamicsEnabledEffect> {
	effects
		.into_iter()
		.filter_map(|effect| match effect {
			UnaRuntimeActionEffect::DynamicsEnabled { source_id, enabled } => Some(DiagnoseActionDynamicsEnabledEffect {
				source_id: source_id.clone(),
				enabled: *enabled,
			}),
			_ => None,
		})
		.collect()
}

fn runtime_action_effect_kind_counts<'a>(effects: impl IntoIterator<Item = &'a UnaRuntimeActionEffect>) -> BTreeMap<String, usize> {
	let mut counts = BTreeMap::new();
	for effect in effects {
		*counts.entry(runtime_action_effect_kind(effect).to_string()).or_insert(0) += 1;
	}
	counts
}

fn diagnose_menu_action_candidates(
	unavatar: Option<&DiagnoseUnavatarSummary>,
	actions: Option<&un_avatar_core::UnaRuntimeActionSet>,
) -> Vec<DiagnoseMenuActionCandidate> {
	let Some(unavatar) = unavatar else {
		return Vec::new();
	};
	let Some(actions) = actions else {
		return Vec::new();
	};
	let mut candidates = Vec::new();
	for menu in &unavatar.modular_avatar_menu_components {
		let (Some(parameter_name), Some(parameter_value)) = (&menu.parameter, menu.value) else {
			continue;
		};
		let mut matched_any_action = false;
		for action in &actions.actions {
			let mut matched = None;
			for condition in &action.conditions {
				if condition.parameter_name.as_deref() == Some(parameter_name.as_str())
					&& condition
						.parameter_value
						.is_some_and(|value| (value - parameter_value).abs() <= un_avatar_core::UNA_RUNTIME_ACTION_PARAMETER_EPSILON)
				{
					matched = Some(("condition", condition.inverted));
					break;
				}
			}
			if matched.is_none() {
				for trigger in &action.triggers {
					if let UnaRuntimeActionTrigger::ParameterValue { name, value } = trigger {
						if name == parameter_name
							&& (*value - parameter_value).abs() <= un_avatar_core::UNA_RUNTIME_ACTION_PARAMETER_EPSILON
						{
							matched = Some(("trigger", false));
							break;
						}
					}
				}
			}
			let Some((match_kind, inverted)) = matched else {
				continue;
			};
			matched_any_action = true;
			let wardrobe_set_ids = action
				.effects
				.iter()
				.filter_map(|effect| match effect {
					UnaRuntimeActionEffect::WardrobeSet { set_id } => Some(set_id.clone()),
					_ => None,
				})
				.collect::<Vec<_>>();
			candidates.push(DiagnoseMenuActionCandidate {
				menu_component_index: menu.component_index,
				menu_key: menu.menu_key.clone(),
				menu_label: menu.label.clone(),
				parameter_name: parameter_name.clone(),
				parameter_value,
				action_id: action.id.clone(),
				action_label: action.label.clone(),
				match_kind: match_kind.to_string(),
				inverted,
				effect_count: action.effects.len(),
				effect_kinds: runtime_action_effect_kind_counts(action.effects.iter()),
				wardrobe_set_ids,
			});
		}
		if !matched_any_action && metadata_menu_candidate_visible(menu) {
			candidates.push(DiagnoseMenuActionCandidate {
				menu_component_index: menu.component_index,
				menu_key: menu.menu_key.clone(),
				menu_label: menu.label.clone(),
				parameter_name: parameter_name.clone(),
				parameter_value,
				action_id: format!("menu:{}", menu.menu_key),
				action_label: menu
					.label
					.clone()
					.unwrap_or_else(|| format!("{}={parameter_value}", parameter_name)),
				match_kind: "metadata".to_string(),
				inverted: false,
				effect_count: 0,
				effect_kinds: BTreeMap::new(),
				wardrobe_set_ids: Vec::new(),
			});
		}
	}
	candidates.sort_by(|a, b| {
		(
			a.menu_component_index,
			a.menu_key.as_str(),
			a.action_id.as_str(),
			a.match_kind.as_str(),
		)
			.cmp(&(
				b.menu_component_index,
				b.menu_key.as_str(),
				b.action_id.as_str(),
				b.match_kind.as_str(),
			))
	});
	candidates
}

fn metadata_menu_candidate_visible(menu: &DiagnoseModularAvatarMenuComponentSummary) -> bool {
	if menu.control_type.as_deref() == Some("Button") {
		return false;
	}
	let Some(path) = menu.hierarchy_path.as_deref() else {
		return true;
	};
	let segments = path
		.trim_matches('/')
		.split('/')
		.filter(|segment| !segment.is_empty() && *segment != "VRC Menu")
		.collect::<Vec<_>>();
	if segments.len() > 2 {
		return false;
	}
	if segments
		.iter()
		.any(|segment| *segment == "Face_Tracking" || segment.contains("VRCFT") || segment.contains('<'))
	{
		return false;
	}
	if menu
		.label
		.as_deref()
		.is_some_and(|label| label.contains("VRCFT") || label.contains('<'))
	{
		return false;
	}
	true
}

fn menu_graph_node_display_label(node: &DiagnoseModularAvatarMenuGraphNode) -> Option<String> {
	node.label.clone().or_else(|| {
		node.hierarchy_path
			.as_deref()
			.and_then(|path| path.trim_matches('/').rsplit('/').next())
			.filter(|label| !label.is_empty())
			.map(str::to_string)
	})
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct MenuGraphNodePath {
	labels: Vec<String>,
	truncated: bool,
}

fn menu_graph_node_path(nodes: &[DiagnoseModularAvatarMenuGraphNode], node_index: usize) -> MenuGraphNodePath {
	let mut labels = Vec::new();
	let mut seen = BTreeSet::new();
	let mut current_index = Some(node_index);
	while let Some(index) = current_index {
		if index >= nodes.len() {
			labels.reverse();
			return MenuGraphNodePath { labels, truncated: true };
		}
		if !seen.insert(index) {
			labels.reverse();
			return MenuGraphNodePath { labels, truncated: true };
		}
		let node = &nodes[index];
		if let Some(label) = menu_graph_node_display_label(node) {
			labels.push(label);
		}
		current_index = node.parent_node_index;
	}
	labels.reverse();
	MenuGraphNodePath { labels, truncated: false }
}

fn diagnose_menu_wardrobe_candidates(
	unavatar: Option<&DiagnoseUnavatarSummary>,
	menu_action_candidates: &[DiagnoseMenuActionCandidate],
) -> Vec<DiagnoseMenuWardrobeCandidate> {
	let Some(unavatar) = unavatar else {
		return Vec::new();
	};
	let node_by_menu_key = unavatar
		.modular_avatar_menu_graph_nodes
		.iter()
		.enumerate()
		.map(|(index, node)| (node.menu_key.as_str(), index))
		.collect::<BTreeMap<_, _>>();
	let mut candidates = Vec::new();
	for action_candidate in menu_action_candidates {
		if action_candidate.wardrobe_set_ids.is_empty() {
			continue;
		}
		let menu_path = node_by_menu_key
			.get(action_candidate.menu_key.as_str())
			.map(|node_index| menu_graph_node_path(&unavatar.modular_avatar_menu_graph_nodes, *node_index))
			.unwrap_or_else(|| MenuGraphNodePath {
				labels: action_candidate.menu_label.iter().cloned().collect(),
				truncated: false,
			});
		for wardrobe_set_id in &action_candidate.wardrobe_set_ids {
			candidates.push(DiagnoseMenuWardrobeCandidate {
				menu_component_index: action_candidate.menu_component_index,
				menu_key: action_candidate.menu_key.clone(),
				menu_path: menu_path.labels.clone(),
				menu_path_truncated: menu_path.truncated,
				menu_label: action_candidate.menu_label.clone(),
				action_id: action_candidate.action_id.clone(),
				wardrobe_set_id: wardrobe_set_id.clone(),
				match_kind: action_candidate.match_kind.clone(),
				inverted: action_candidate.inverted,
			});
		}
	}
	candidates.sort_by(|a, b| {
		(
			a.menu_component_index,
			a.menu_key.as_str(),
			a.wardrobe_set_id.as_str(),
			a.action_id.as_str(),
		)
			.cmp(&(
				b.menu_component_index,
				b.menu_key.as_str(),
				b.wardrobe_set_id.as_str(),
				b.action_id.as_str(),
			))
	});
	candidates
}

fn print_omitted_text_items(label: &str, total: usize, limit: usize) {
	if total > limit {
		println!("  {label}: showing {limit}/{total}, omitted {}", total - limit);
	}
}

fn run_diagnose(
	plugin_dirs: &[PathBuf],
	path: PathBuf,
	input_format: Option<String>,
	wardrobe_set: Option<String>,
	wardrobe_probe_all: bool,
	visible_materials_only: bool,
	visible_meshes: bool,
	json: bool,
) -> Result<(), String> {
	let reg = io_registry_for_cli(plugin_dirs)?;
	let cached_bytes = cached_binary_import_bytes(&path);
	let importer: &dyn AvatarImporter = if let Some(ref s) = input_format {
		let id = FormatId::new(s.as_str());
		reg.importer_by_id(&id)
			.ok_or_else(|| format!("指定の importer が登録されていません: {s}"))?
	} else {
		let probe = import_probe_for_path(&path, cached_bytes.clone());
		reg.best_importer_for(&probe)
			.ok_or_else(|| "入力に合う importer が見つかりません".to_string())?
	};
	let desc = importer.descriptor();
	let mut ictx = ImportContext {
		asset_root: path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(".")),
		temp_dir: std::env::temp_dir(),
		..ImportContext::dummy()
	};
	let import_started = Instant::now();
	let mut imported = importer
		.import(&mut ictx, import_input_for_path(&path, &desc.id, cached_bytes), ImportOptions)
		.map_err(|e| e.to_string())?;
	let import_ms = import_started.elapsed().as_millis();
	let base_document_for_probes = imported.document.clone();
	let mut wardrobe_apply_ms = 0;
	if let Some(set_id) = wardrobe_set.as_deref().filter(|set_id| !set_id.trim().is_empty()) {
		let started = Instant::now();
		let applied = apply_unavatar_wardrobe_set(&mut imported.document, set_id)?;
		wardrobe_apply_ms = started.elapsed().as_millis();
		imported.report.push_info(format!(
			".unavatar wardrobe set `{set_id}`: visibility_applied={}, visibility_missing={}, blendshape_applied={}, blendshape_missing={}, dynamics_applied={}, dynamics_missing={}, material_applied={}, material_missing={}, material_slot_applied={}, material_slot_missing={}, active_asset_groups={:?}, scoped_active_groups={}, scoped_missing_groups={:?}, scoped_resident=mesh:{} material:{} image:{} dynamics:{}",
			applied.visibility_applied,
			applied.visibility_missing,
			applied.blendshape_applied,
			applied.blendshape_missing,
			applied.dynamics_applied,
			applied.dynamics_missing,
			applied.material_applied,
			applied.material_missing,
			applied.material_slot_applied,
			applied.material_slot_missing,
			applied.active_asset_groups,
			applied.scoped_active_asset_group_count,
			applied.scoped_missing_active_asset_groups,
			applied.scoped_resident_mesh_primitive_count,
			applied.scoped_resident_material_count,
			applied.scoped_resident_image_count,
			applied.scoped_resident_dynamics_count
		));
	}
	let wardrobe_probe_started = Instant::now();
	let wardrobe_probes = if wardrobe_probe_all {
		build_wardrobe_probes(&base_document_for_probes)?
	} else {
		Vec::new()
	};
	let wardrobe_probe_ms = wardrobe_probe_started.elapsed().as_millis();
	let report_build_started = Instant::now();
	let report = build_diagnose_report(
		&path,
		desc.id.0,
		desc.provider_plugin_id,
		DiagnoseTimingSummary {
			import_ms,
			wardrobe_apply_ms,
			wardrobe_probe_ms,
			report_build_ms: 0,
		},
		imported.report,
		imported.document,
		wardrobe_probes,
	);
	let report_build_ms = report_build_started.elapsed().as_millis();
	let report = DiagnoseReport {
		timings: DiagnoseTimingSummary {
			report_build_ms,
			..report.timings
		},
		..report
	};
	if json {
		write_json_stdout(&report)?;
		return Ok(());
	}
	println!("path: {}", report.path);
	println!("importer: {}", report.import_format_id);
	println!(
		"timings: import={}ms wardrobe_apply={}ms wardrobe_probe={}ms report_build={}ms",
		report.timings.import_ms, report.timings.wardrobe_apply_ms, report.timings.wardrobe_probe_ms, report.timings.report_build_ms
	);
	println!(
		"import_report: status={:?} messages={} diagnostics={} approximations={} lost_features={}",
		report.import_report.status,
		report.import_report.messages.len(),
		report.import_report.diagnostics.len(),
		report.import_report.approximations.len(),
		report.import_report.lost_features.len()
	);
	println!(
		"runtime: source={:?} humanoid_basis={:?} active_wardrobe_set={:?} active_asset_groups={:?} last_action_id={:?} parameter_values={} parameter_definitions={} parameter_conflicts={}",
		report.runtime.source_kind,
		report.runtime.humanoid_basis,
		report.runtime.active_wardrobe_set,
		report.runtime.active_asset_groups,
		report.runtime.last_action_id,
		report.runtime.parameter_values.len(),
		report.runtime.parameter_definitions.len(),
		report.runtime.parameter_conflicts.len()
	);
	if !report.runtime.parameter_values.is_empty() {
		println!("runtime.parameters: {:?}", report.runtime.parameter_values);
	}
	for definition in report.runtime.parameter_definitions.iter().take(16) {
		println!(
			"runtime.parameter_definition[{}]: owners={:?} sources={:?} values={:?} current={:?} transient={}",
			definition.name,
			definition.owner_keys,
			definition.source_kinds,
			definition.value_samples,
			definition.current_value,
			definition.transient
		);
	}
	for conflict in report.runtime.parameter_conflicts.iter().take(16) {
		println!(
			"runtime.parameter_conflict[{}]: reason={} owners={:?} sources={:?} values={:?}",
			conflict.name, conflict.reason, conflict.owner_keys, conflict.source_kinds, conflict.value_samples
		);
	}
	println!("runtime.resolver_cache_key: {:?}", report.runtime.resolver_cache_key);
	if let Some(actions) = &report.actions {
		println!(
			"actions: actions={} triggers={} effects={} target_write_collisions={} restore_readiness={} restore_baseline_candidates={} restore_baseline_capture_plan={} restore_apply_plan={} trigger_kinds={:?} effect_kinds={:?}",
			actions.action_count,
			actions.trigger_count,
			actions.effect_count,
			actions.target_write_collisions.len(),
			actions.restore_readiness.len(),
			actions.restore_baseline_candidates.len(),
			actions.restore_baseline_capture_plan.len(),
			actions.restore_apply_plan.len(),
			actions.trigger_kinds,
			actions.effect_kinds
		);
		for collision in actions.target_write_collisions.iter().take(16) {
			println!(
				"action_target_collision: {:?}:{} owners={:?} actions={:?}",
				collision.target_kind, collision.target_key, collision.owner_keys, collision.action_ids
			);
		}
		for readiness in actions.restore_readiness.iter().take(16) {
			println!(
				"action_restore_readiness: {}:{} target={:?}:{} restore_target={} current={} baseline_required={} ready={} reason={}",
				readiness.owner_key,
				readiness.effect_kind,
				readiness.target_kind,
				readiness.target_key,
				readiness.restore_target,
				readiness.current_value_available,
				readiness.baseline_required,
				readiness.ready,
				readiness.reason
			);
		}
		for candidate in actions.restore_baseline_candidates.iter().take(16) {
			println!(
				"action_restore_baseline_candidate: {}:{} target={:?}:{} value={}",
				candidate.owner_key, candidate.effect_kind, candidate.target_kind, candidate.target_key, candidate.baseline_value
			);
		}
		for entry in actions.restore_baseline_capture_plan.iter().take(16) {
			println!(
				"action_restore_baseline_capture: {} target={:?}:{} value={} actions={:?} effects={:?}",
				entry.owner_key,
				entry.target_kind,
				entry.target_key,
				entry.baseline_value,
				entry.source_action_ids,
				entry.source_effect_kinds
			);
		}
		for entry in actions.restore_apply_plan.iter().take(16) {
			println!(
				"action_restore_apply: {} state={:?} target={:?}:{} ready={} reason={} baseline={:?} current={:?}",
				entry.owner_key,
				entry.condition_state,
				entry.target_kind,
				entry.target_key,
				entry.ready,
				entry.reason,
				entry.baseline_value,
				entry.current_value
			);
		}
		for action in actions.actions.iter().take(16) {
			println!(
				"action[{}]: label={:?} triggers={} conditions={} effects={} condition_state={:?} condition_parameters={:?} trigger_kinds={:?} effect_kinds={:?}",
				action.id,
				action.label,
				action.trigger_count,
				action.condition_count,
				action.effect_count,
				action.current_condition_state,
				action.condition_parameter_names,
				action.trigger_kinds,
				action.effect_kinds
			);
			if !action.parameter_triggers.is_empty() {
				let triggers = action
					.parameter_triggers
					.iter()
					.map(|trigger| format!("{}={}", trigger.name, trigger.value))
					.collect::<Vec<_>>()
					.join(", ");
				println!("action[{}].parameter_triggers: {}", action.id, triggers);
			}
			if !action.conditions.is_empty() {
				let conditions = action
					.conditions
					.iter()
					.map(|condition| {
						let target = condition
							.source_node_id
							.as_deref()
							.or(condition.resolved_node_id.as_deref())
							.or(condition.path.as_deref())
							.unwrap_or("?");
						format!(
							"component={:?} target={} parameter={:?}:{:?} sub_parameters={:?} inverted={} active_parents={}",
							condition.source_component_id,
							target,
							condition.parameter_name,
							condition.parameter_value,
							condition.sub_parameter_names,
							condition.inverted,
							condition.active_parent_count
						)
					})
					.collect::<Vec<_>>()
					.join(", ");
				println!("action[{}].conditions: {}", action.id, conditions);
			}
			if !action.target_writes.is_empty() {
				let writes = action
					.target_writes
					.iter()
					.take(8)
					.map(|write| format!("{}:{}={}", write.owner_key, write.effect_kind, write.target_key))
					.collect::<Vec<_>>()
					.join(", ");
				println!("action[{}].target_writes: {}", action.id, writes);
			}
			if !action.node_visibility_effects.is_empty() {
				let effects = action
					.node_visibility_effects
					.iter()
					.map(|effect| {
						let target = effect
							.source_node_id
							.as_deref()
							.or(effect.resolved_node_id.as_deref())
							.or(effect.path.as_deref())
							.map(str::to_string)
							.unwrap_or_else(|| effect.node_index.map_or("?".to_string(), |index| format!("#{index}")));
						format!("{target}={}", effect.visible)
					})
					.collect::<Vec<_>>()
					.join(", ");
				println!("action[{}].node_visibility: {}", action.id, effects);
			}
			if !action.material_property_effects.is_empty() {
				let effects = action
					.material_property_effects
					.iter()
					.map(|effect| {
						let material = effect
							.material_name
							.as_deref()
							.map(str::to_string)
							.or_else(|| effect.material_index.map(|index| format!("#{index}")))
							.unwrap_or_else(|| "?".to_string());
						let value = effect
							.scalar_value
							.map(|value| value.to_string())
							.or_else(|| effect.color_value.map(|value| format!("{value:?}")))
							.unwrap_or_else(|| "?".to_string());
						format!("{}:{}[{}]={}", effect.property_kind, material, effect.parameter, value)
					})
					.collect::<Vec<_>>()
					.join(", ");
				println!("action[{}].material_properties: {}", action.id, effects);
			}
			if !action.material_slot_effects.is_empty() {
				let effects = action
					.material_slot_effects
					.iter()
					.map(|effect| {
						let target = effect
							.source_node_id
							.as_deref()
							.or(effect.resolved_node_id.as_deref())
							.or(effect.path.as_deref())
							.map(str::to_string)
							.unwrap_or_else(|| effect.node_index.map_or("?".to_string(), |index| format!("#{index}")));
						let primitive = effect.primitive_index.map_or("*".to_string(), |index| index.to_string());
						let material = effect
							.material_name
							.as_deref()
							.map(str::to_string)
							.or_else(|| effect.material_index.map(|index| format!("#{index}")))
							.unwrap_or_else(|| "null".to_string());
						format!("{target}[{primitive}]={material}")
					})
					.collect::<Vec<_>>()
					.join(", ");
				println!("action[{}].material_slots: {}", action.id, effects);
			}
			if !action.expression_weight_effects.is_empty() {
				let effects = action
					.expression_weight_effects
					.iter()
					.map(|effect| format!("{}={}", effect.name, effect.weight))
					.collect::<Vec<_>>()
					.join(", ");
				println!("action[{}].expression_weights: {}", action.id, effects);
			}
			if !action.dynamics_enabled_effects.is_empty() {
				let effects = action
					.dynamics_enabled_effects
					.iter()
					.map(|effect| format!("{}={}", effect.source_id, effect.enabled))
					.collect::<Vec<_>>()
					.join(", ");
				println!("action[{}].dynamics_enabled: {}", action.id, effects);
			}
		}
	} else {
		println!("actions: none");
	}
	for candidate in report.menu_action_candidates.iter().take(16) {
		println!(
			"menu_action_candidate[#{} -> {}]: label={:?} parameter={}:{} match={} inverted={} effects={} {:?} wardrobe_sets={:?}",
			candidate.menu_component_index,
			candidate.action_id,
			candidate.menu_label,
			candidate.parameter_name,
			candidate.parameter_value,
			candidate.match_kind,
			candidate.inverted,
			candidate.effect_count,
			candidate.effect_kinds,
			candidate.wardrobe_set_ids
		);
	}
	for candidate in report.menu_wardrobe_candidates.iter().take(16) {
		println!(
			"menu_wardrobe_candidate[#{} -> {}]: path={:?} truncated={} label={:?} action={} match={} inverted={}",
			candidate.menu_component_index,
			candidate.wardrobe_set_id,
			candidate.menu_path,
			candidate.menu_path_truncated,
			candidate.menu_label,
			candidate.action_id,
			candidate.match_kind,
			candidate.inverted
		);
	}
	if let Some(vrm) = &report.vrm {
		println!(
			"vrm: spec={} mtoon_v0={} mtoon_v1={:?} spring_groups={}",
			vrm.spec_version, vrm.mtoon_materials_v0, vrm.mtoon_material_indices_v1, vrm.spring_group_count
		);
	} else {
		println!("vrm: none");
	}
	println!(
		"dynamics: groups={} vrm_spring={} vrc_physbone={} unknown={} limit_groups={} angle_limit_groups={} stretch_limit_groups={} rotation_translation_writeback_groups={} translation_writeback_candidates={} translation_writeback_targets={} stretch_translation_writeback_groups={} stretch_translation_writeback_target_groups={} grabbing_groups={} posing_groups={} colliders={} collider_vrm_spring={} collider_vrc_physbone={} collider_unknown={} contacts={} contact_senders={} contact_receivers={} contact_parameter_declarations={} contact_parameter_emission={} contact_probes={} contact_probe_would_emit={} contact_parameter_emissions={} contact_parameter_emitted={} contact_parameter_reset_to_zero={} constraint_refs={} vrc_constraint_refs={} source_limits={} source_angle_limits={} source_stretch_limits={} source_curves={} source_radius_curves={} source_angle_limit_curves={} source_stretch_limit_curves={} source_colliders={} source_unknown_shape_colliders={} source_collision_disabled={} source_inside_bounds_colliders={} source_grabbing={} source_posing={} source_interaction_parameters={}",
		report.dynamics.group_count,
		report.dynamics.vrm_spring_bone_group_count,
		report.dynamics.vrc_physbone_group_count,
		report.dynamics.unknown_group_count,
		report.dynamics.limit_group_count,
		report.dynamics.angle_limit_group_count,
		report.dynamics.stretch_limit_group_count,
		report.dynamics.rotation_translation_writeback_group_count,
		report.dynamics.translation_writeback_candidate_count,
		report.dynamics.translation_writeback_target_count,
		report.dynamics.stretch_translation_writeback_group_count,
		report.dynamics.stretch_translation_writeback_target_group_count,
		report.dynamics.grabbing_enabled_group_count,
		report.dynamics.posing_enabled_group_count,
		report.dynamics.collider_count,
		report.dynamics.vrm_spring_bone_collider_count,
		report.dynamics.vrc_physbone_collider_count,
		report.dynamics.unknown_collider_count,
		report.dynamics.contact_count,
		report.dynamics.vrc_contact_sender_count,
		report.dynamics.vrc_contact_receiver_count,
		report.dynamics.contact_parameter_declaration_count,
		report.dynamics.contact_parameter_emission_enabled,
		report.dynamics.contact_probe_count,
		report.dynamics.contact_probe_would_emit_count,
		report.dynamics.contact_parameter_emission_count,
		report.dynamics.contact_parameter_emitted_count,
		report.dynamics.contact_parameter_reset_to_zero_count,
		report.dynamics.constraint_ref_count,
		report.dynamics.vrc_constraint_ref_count,
		report.dynamics.source_limit_count,
		report.dynamics.source_angle_limit_count,
		report.dynamics.source_stretch_limit_count,
		report.dynamics.source_curve_count,
		report.dynamics.source_radius_curve_count,
		report.dynamics.source_angle_limit_curve_count,
		report.dynamics.source_stretch_limit_curve_count,
		report.dynamics.source_collider_count,
		report.dynamics.source_unknown_shape_collider_count,
		report.dynamics.source_collision_disabled_count,
		report.dynamics.source_inside_bounds_collider_count,
		report.dynamics.source_grabbing_enabled_count,
		report.dynamics.source_posing_enabled_count,
		report.dynamics.source_interaction_parameter_count
	);
	let response_range = |average: f32, min: f32, max: f32| format!("{average}[{min}..{max}]");
	let damping_label = |value: Option<f32>| value.map(|value| value.to_string()).unwrap_or_else(|| "-".to_string());
	for category in &report.dynamics.response_categories {
		println!(
			"  dynamics_response_category[{}]: groups={} joints={} matched={} exact={} xpbd={} compliance={} rest={} shape={} bounce={} stretch={} squish={} stretchMotion={} drag={} damp={} follow={} orient={}",
			category.category,
			category.group_count,
			category.joint_count,
			category.matched_override_group_count,
			category.group_override_group_count,
			category.xpbd_group_count,
			category.average_xpbd_compliance,
			response_range(category.average_rest_response, category.min_rest_response, category.max_rest_response),
			response_range(
				category.average_shape_preservation,
				category.min_shape_preservation,
				category.max_shape_preservation
			),
			response_range(
				category.average_bounce_response,
				category.min_bounce_response,
				category.max_bounce_response
			),
			response_range(
				category.average_max_stretch_response,
				category.min_max_stretch_response,
				category.max_max_stretch_response
			),
			response_range(
				category.average_max_squish_response,
				category.min_max_squish_response,
				category.max_max_squish_response
			),
			response_range(
				category.average_stretch_motion_response,
				category.min_stretch_motion_response,
				category.max_stretch_motion_response
			),
			category.average_drag_force,
			damping_label(category.average_damping_half_life_ms),
			response_range(
				category.average_parent_motion_follow,
				category.min_parent_motion_follow,
				category.max_parent_motion_follow
			),
			category.average_orientation_follow
		);
	}
	for group in report.dynamics.response_groups.iter().take(DIAGNOSE_DYNAMICS_GROUP_TEXT_LIMIT) {
		let matched_overrides = if group.matched_overrides.is_empty() {
			String::new()
		} else {
			format!(" overrides={}", group.matched_overrides.join(","))
		};
		let invalid_match_regexes = if group.invalid_match_regexes.is_empty() {
			String::new()
		} else {
			format!(" invalid_regex={}", group.invalid_match_regexes.join(" | "))
		};
		let group_override = if group.group_override_applied { " exact_override=true" } else { "" };
		println!(
			"  dynamics_response_group[{}]: category={} visual={} visibleJoints={} visibleMeshSubtrees={} joints={} solver={:?} compliance={} rest={} shape={} bounce={} stretch={} squish={} stretchMotion={} drag={} damp={} follow={} orient={}{}{}{}",
			group.source_id,
			group.category,
			group.visual_target,
			group.skinned_joint_count,
			group.mesh_subtree_node_count,
			group.joint_count,
			group.solver,
			group.xpbd_compliance,
			response_range(group.average_rest_response, group.min_rest_response, group.max_rest_response),
			response_range(
				group.average_shape_preservation,
				group.min_shape_preservation,
				group.max_shape_preservation
			),
			response_range(group.average_bounce_response, group.min_bounce_response, group.max_bounce_response),
			response_range(
				group.average_max_stretch_response,
				group.min_max_stretch_response,
				group.max_max_stretch_response
			),
			response_range(
				group.average_max_squish_response,
				group.min_max_squish_response,
				group.max_max_squish_response
			),
			response_range(
				group.average_stretch_motion_response,
				group.min_stretch_motion_response,
				group.max_stretch_motion_response
			),
			group.average_drag_force,
			damping_label(group.average_damping_half_life_ms),
			response_range(
				group.average_parent_motion_follow,
				group.min_parent_motion_follow,
				group.max_parent_motion_follow
			),
			group.average_orientation_follow,
			matched_overrides,
			group_override,
			invalid_match_regexes
		);
	}
	for collider in report.dynamics.colliders.iter().take(DIAGNOSE_TEXT_LIST_LIMIT) {
		println!(
			"  dynamics_collider[{}]: source={:?} id={:?} node={:?} shape={:?} radius={} height={} position={:?} rotation={:?} inside_bounds={}",
			collider.index,
			collider.source_kind,
			collider.source_id,
			collider.node_path.as_deref().unwrap_or("#"),
			collider.shape,
			collider.radius,
			collider.height,
			collider.position,
			collider.rotation,
			collider.inside_bounds
		);
	}
	print_omitted_text_items("dynamics_collider", report.dynamics.colliders.len(), DIAGNOSE_TEXT_LIST_LIMIT);
	for group in report.dynamics.groups.iter().take(DIAGNOSE_DYNAMICS_GROUP_TEXT_LIMIT) {
		let limit = match (
			&group.limit_type,
			group.max_angle_x,
			group.max_angle_z,
			group.max_stretch,
			group.max_squish,
			group.stretch_motion,
		) {
			(None, None, None, None, None, None) => String::new(),
			(limit_type, max_angle_x, max_angle_z, max_stretch, max_squish, stretch_motion) => format!(
				" limit={:?}/x={:?}/z={:?}/stretch={:?}/squish={:?}/stretchMotion={:?}",
				limit_type.as_deref(),
				max_angle_x,
				max_angle_z,
				max_stretch,
				max_squish,
				stretch_motion
			),
		};
		let interaction = match (group.allow_grabbing, group.allow_posing) {
			(None, None) => String::new(),
			(allow_grabbing, allow_posing) => format!(" interaction=grab:{allow_grabbing:?}/pose:{allow_posing:?}"),
		};
		println!(
			"  dynamics_group[{}]: source={:?} enabled={} source_enabled={} runtime_override={:?} id={:?} bones={} root={:?} tip={:?} center={:?} writeback={:?} translation_candidates={} translation_targets={} integration={:?} source_pull={} source_bounce={} source_shape={} drag={} gravity={} radius={}{}{} comment={:?}",
			group.index,
			group.source_kind,
			group.enabled,
			group.source_enabled,
			group.runtime_enabled_override,
			group.source_id,
			group.bone_count,
			group.root_path.as_deref().or(group.root_node.map(|_| "#")),
			group.tip_path.as_deref().or(group.tip_node.map(|_| "#")),
			group.center_path.as_deref().or(group.center_node.map(|_| "#")),
			group.writeback_mode,
			group.translation_writeback_candidate_count,
			group.translation_writeback_target_count,
			group.integration_type,
			group.pull,
			group.spring,
			group.stiffness,
			group.drag_force,
			group.gravity_power,
			group.hit_radius,
			limit,
			interaction,
			group.comment
		);
	}
	print_omitted_text_items("dynamics_group", report.dynamics.groups.len(), DIAGNOSE_DYNAMICS_GROUP_TEXT_LIMIT);
	for hook in report.dynamics.interaction_hooks.iter().take(DIAGNOSE_TEXT_LIST_LIMIT) {
		let suffix_preview = hook.suffix_parameters.iter().take(3).cloned().collect::<Vec<_>>();
		println!(
			"  dynamics_interaction_hook[group={}]: source={:?} enabled={} id={:?} root={:?} grab={} pose={} parameter={:?} suffix_count={} suffix_preview={:?} metadata_only={}",
			hook.group_index,
			hook.source_kind,
			hook.enabled,
			hook.source_id,
			hook.root_path.as_deref().unwrap_or("#"),
			hook.allow_grabbing,
			hook.allow_posing,
			hook.parameter,
			hook.suffix_parameters.len(),
			suffix_preview,
			hook.metadata_only
		);
	}
	print_omitted_text_items(
		"dynamics_interaction_hook",
		report.dynamics.interaction_hooks.len(),
		DIAGNOSE_TEXT_LIST_LIMIT,
	);
	for contact in report.dynamics.contacts.iter().take(DIAGNOSE_TEXT_LIST_LIMIT) {
		println!(
			"  dynamics_contact[{}]: source={:?} kind={:?} id={:?} node={:?} parameter={:?} tags={:?} shape={:?} radius={} height={} position={:?}",
			contact.index,
			contact.source_kind,
			contact.kind,
			contact.source_id,
			contact.node_path.as_deref().unwrap_or("#"),
			contact.parameter,
			contact.collision_tags,
			contact.shape,
			contact.radius,
			contact.height,
			contact.position
		);
	}
	print_omitted_text_items("dynamics_contact", report.dynamics.contacts.len(), DIAGNOSE_TEXT_LIST_LIMIT);
	for declaration in report.dynamics.contact_parameter_declarations.iter().take(DIAGNOSE_TEXT_LIST_LIMIT) {
		println!(
			"  dynamics_contact_parameter[{}]: owner={:?} source_id={:?} node={:?} parameter={:?} tags={:?}",
			declaration.index,
			declaration.owner_key,
			declaration.source_id,
			declaration.node_path.as_deref().unwrap_or("#"),
			declaration.parameter,
			declaration.collision_tags
		);
	}
	print_omitted_text_items(
		"dynamics_contact_parameter",
		report.dynamics.contact_parameter_declarations.len(),
		DIAGNOSE_TEXT_LIST_LIMIT,
	);
	for probe in report.dynamics.contact_probes.iter().take(DIAGNOSE_TEXT_LIST_LIMIT) {
		println!(
			"  dynamics_contact_probe[{}]: receiver={} sender={} parameter={:?} tags={:?} tag_match={} overlap={} would_emit={} distance={} threshold={} radii={}/{} approx={}",
			probe.index,
			probe.receiver_node_path.as_deref().unwrap_or("#"),
			probe.sender_node_path.as_deref().unwrap_or("#"),
			probe.parameter,
			probe.matched_tags,
			probe.tag_match,
			probe.overlap,
			probe.would_emit,
			probe.distance,
			probe.threshold,
			probe.receiver_radius,
			probe.sender_radius,
			probe.approximation
		);
	}
	print_omitted_text_items(
		"dynamics_contact_probe",
		report.dynamics.contact_probes.len(),
		DIAGNOSE_TEXT_LIST_LIMIT,
	);
	for emission in report.dynamics.contact_parameter_emissions.iter().take(DIAGNOSE_TEXT_LIST_LIMIT) {
		println!(
			"  dynamics_contact_parameter_emission[{}]: owner={:?} receiver={} parameter={:?} value={} emitted={} senders={:?}",
			emission.index,
			emission.owner_key,
			emission.receiver_node_path.as_deref().unwrap_or("#"),
			emission.parameter,
			emission.value,
			emission.emitted,
			emission.sender_source_ids
		);
	}
	print_omitted_text_items(
		"dynamics_contact_parameter_emission",
		report.dynamics.contact_parameter_emissions.len(),
		DIAGNOSE_TEXT_LIST_LIMIT,
	);
	for constraint_ref in report.dynamics.constraint_refs.iter().take(DIAGNOSE_TEXT_LIST_LIMIT) {
		println!(
			"  dynamics_constraint_ref[{}]: source={:?} id={:?} type={:?} target={:?} sources={:?} weight={}",
			constraint_ref.index,
			constraint_ref.source_kind,
			constraint_ref.source_id,
			constraint_ref.constraint_type,
			constraint_ref.target_path.as_deref().unwrap_or("#"),
			if constraint_ref.source_paths.is_empty() {
				constraint_ref
					.source_nodes
					.iter()
					.map(|node| format!("#{node}"))
					.collect::<Vec<_>>()
			} else {
				constraint_ref.source_paths.clone()
			},
			constraint_ref.weight
		);
	}
	print_omitted_text_items(
		"dynamics_constraint_ref",
		report.dynamics.constraint_refs.len(),
		DIAGNOSE_TEXT_LIST_LIMIT,
	);
	if let Some(unavatar) = &report.unavatar {
		println!(
			"unavatar: spec={} generator={:?} name={:?} source={:?} raw_dynamics={} modular_avatar_components={} support={:?} types={:?} disabled_types={:?} menu_components={} blendshape_syncs={} vertex_filter_groups={}",
			unavatar.spec_version,
			unavatar.generator,
			unavatar.manifest_name,
			unavatar.source_type,
			unavatar.dynamics_entry_count,
			unavatar.modular_avatar_component_count,
			unavatar.modular_avatar_support_counts,
			unavatar.modular_avatar_type_counts,
			unavatar.modular_avatar_disabled_type_counts,
			unavatar.modular_avatar_menu_component_count,
			unavatar.modular_avatar_blendshape_sync_count,
			unavatar.modular_avatar_vertex_filter_group_count
		);
		for menu in unavatar.modular_avatar_menu_components.iter().take(16) {
			println!(
				"unavatar.ma_menu[{}#{}]: enabled={} label={:?} type={:?} parameter={:?} value={:?} hierarchy={:?} sibling={:?} target={:?} menu_source={:?} source_target={:?} menu_to_append={:?} menu_to_append_controls={:?} install_target_menu={:?} install_target_menu_controls={:?} installer={:?}",
				menu.short_type,
				menu.component_index,
				menu.enabled,
				menu.label,
				menu.control_type,
				menu.parameter,
				menu.value,
				menu.hierarchy_path,
				menu.sibling_index,
				menu.target_path,
				menu.menu_source,
				menu.menu_source_target_path,
				menu.menu_to_append_path,
				menu.menu_to_append_control_count,
				menu.install_target_menu_path,
				menu.install_target_menu_control_count,
				menu.installer_path
			);
		}
		for candidate in unavatar.modular_avatar_menu_graph_candidates.iter().take(16) {
			println!(
				"unavatar.ma_menu_candidate[#{}]: kind={} label={:?} hierarchy={:?} parent={:?} sibling={:?} target={:?} menu_to_append={:?} install_target_menu={:?} installer={:?}",
				candidate.component_index,
				candidate.kind,
				candidate.label,
				candidate.hierarchy_path,
				candidate.parent_path,
				candidate.sibling_index,
				candidate.target_path,
				candidate.menu_to_append_path,
				candidate.install_target_menu_path,
				candidate.installer_path
			);
		}
		for node in unavatar.modular_avatar_menu_graph_nodes.iter().take(16) {
			println!(
				"unavatar.ma_menu_graph_node[{}:#{}]: kind={} label={:?} hierarchy={:?} parent_node={:?} parent_component={:?} children={:?} menu_to_append={:?} install_target_menu={:?} installer={:?}",
				node.node_index,
				node.component_index,
				node.kind,
				node.label,
				node.hierarchy_path,
				node.parent_node_index,
				node.parent_component_index,
				node.child_component_indices,
				node.menu_to_append_path,
				node.install_target_menu_path,
				node.installer_path
			);
		}
		for edge in unavatar.modular_avatar_menu_install_edges.iter().take(16) {
			println!(
				"unavatar.ma_menu_install_edge[#{}]: source={} target={} hierarchy={:?} installer={:?} menu_to_append={:?} install_target_menu={:?} ignored_by_install_target={}",
				edge.source_component_index,
				edge.source_kind,
				edge.target_kind,
				edge.source_hierarchy_path,
				edge.installer_path,
				edge.menu_to_append_path,
				edge.install_target_menu_path,
				edge.ignored_by_install_target
			);
		}
		for parameter in unavatar.modular_avatar_parameters.iter().take(16) {
			println!(
				"unavatar.ma_parameter[#{}]: name={} remap={:?} internal={} prefix={} sync={} local_only={} default={} explicit_default={} saved={} override_animator_defaults={}",
				parameter.component_index,
				parameter.name_or_prefix,
				parameter.remap_to,
				parameter.internal_parameter,
				parameter.is_prefix,
				parameter.sync_type,
				parameter.local_only,
				parameter.default_value,
				parameter.has_explicit_default_value,
				parameter.saved,
				parameter.override_animator_defaults
			);
		}
		for sync in unavatar.modular_avatar_blendshape_syncs.iter().take(16) {
			println!(
				"unavatar.ma_blendshape_sync[#{}]: enabled={} target={:?} bindings={}",
				sync.component_index, sync.enabled, sync.target_path, sync.binding_count
			);
			for binding in sync.bindings.iter().take(8) {
				println!(
					"  binding reference={:?} blendshape={} local={} remap_keys={}",
					binding.reference_path, binding.blendshape, binding.local_blendshape, binding.remap_key_count
				);
			}
		}
		for group in unavatar.modular_avatar_vertex_filter_groups.iter().take(16) {
			println!(
				"unavatar.ma_vertex_filter[{}]: enabled={} target={:?} combine={} filters={}",
				group.short_type, group.enabled, group.target_path, group.combine, group.filter_count
			);
			for filter in group.filters.iter().take(8) {
				println!(
					"  filter kind={} shapes={:?} threshold={:?} bone={:?} center={:?} axis={:?} material={:?} texture={:?} mode={:?}",
					filter.kind,
					filter.shapes,
					filter.threshold,
					filter.bone_path,
					filter.center,
					filter.axis,
					filter.material_index,
					filter.texture,
					filter.mode
				);
			}
		}
		println!(
			"wardrobe: base={:?} sets={} {:?} asset_groups={} {:?} base_ops={} {:?} extension_nodes={} variants={}",
			unavatar.base_set,
			unavatar.wardrobe_set_count,
			unavatar.wardrobe_set_ids,
			unavatar.asset_group_count,
			unavatar.asset_group_ids,
			unavatar.base_operation_count,
			unavatar.base_operation_counts,
			unavatar.extension_node_count,
			unavatar.variant_count
		);
		for set in &unavatar.wardrobe_sets {
			println!(
				"wardrobe_set[{}]: name={:?} source={:?} ops={} {:?} groups={:?}",
				set.id, set.display_name, set.source, set.operation_count, set.operation_counts, set.asset_groups
			);
		}
		for probe in &report.wardrobe_probes {
			println!(
				"wardrobe_probe[{}]: name={:?} probe={}ms visible_meshes={} nonzero_morphs={} active_asset_groups={:?} apply=vis {:?}/{:?} blend {:?}/{:?} dyn {:?}/{:?} mat {:?}/{:?} slot {:?}/{:?} missing=vis {} blend {} dyn {} mat {} slot {}",
				probe.set_id,
				probe.display_name,
				probe.probe_ms,
				probe.visible_mesh_node_count,
				probe.nonzero_morph_weight_count,
				probe.active_asset_groups,
				probe.visibility_applied,
				probe.visibility_missing,
				probe.blendshape_applied,
				probe.blendshape_missing,
				probe.dynamics_applied,
				probe.dynamics_missing,
				probe.material_applied,
				probe.material_missing,
				probe.material_slot_applied,
				probe.material_slot_missing,
				probe.missing_visibility_paths.len(),
				probe.missing_blendshapes.len(),
				probe.missing_dynamics_ids.len(),
				probe.missing_materials.len(),
				probe.missing_material_slots.len()
			);
			for path in probe.visible_mesh_paths.iter().take(24) {
				println!("  visible: {path}");
			}
			if probe.visible_mesh_paths.len() > 24 {
				println!("  visible: ... {} more", probe.visible_mesh_paths.len() - 24);
			}
			for morph in probe.nonzero_morph_weights.iter().take(12) {
				println!(
					"  morph: mesh={} primitive={} index={} name={:?} weight={}",
					morph.mesh, morph.primitive, morph.index, morph.name, morph.weight
				);
			}
			if probe.nonzero_morph_weights.len() > 12 {
				println!("  morph: ... {} more", probe.nonzero_morph_weights.len() - 12);
			}
		}
	} else {
		println!("unavatar: none");
	}
	println!(
		"scene: meshes={} primitives={} morph_targets={} nodes={} hidden_nodes={} skins={} images={} materials={}",
		report.scene.mesh_count,
		report.scene.primitive_count,
		report.scene.morph_target_count,
		report.scene.node_count,
		report.scene.hidden_node_count,
		report.scene.skin_count,
		report.scene.image_count,
		report.scene.material_count
	);
	for skin in &report.scene.skins {
		println!(
			"skin[{}]: joints={} inverse_binds={} effective={} over_renderer_limit={} skeleton={:?} used_nodes={} prim_joints={} prim_weights={} mismatched_attrs={} max_joint={:?} out_of_range_prim_joints={}",
			skin.index,
			skin.joint_count,
			skin.inverse_bind_count,
			skin.effective_joint_count,
			skin.over_renderer_bone_limit,
			skin.skeleton_node,
			skin.used_by_node_count,
			skin.primitive_joint_attribute_count,
			skin.primitive_weight_attribute_count,
			skin.mismatched_joint_weight_attribute_count,
			skin.max_joint_index,
			skin.out_of_range_joint_attribute_count
		);
	}
	println!(
		"node_constraints: {} kinds={:?} parent_sources={} parent_multi_source={}",
		report.scene.node_constraint_count,
		report.scene.node_constraint_kind_counts,
		report.scene.parent_node_constraint_source_count,
		report.scene.parent_node_constraint_multi_source_count
	);
	println!(
		"asset_ownership: groups={} mesh_primitives={} materials={} images={} dynamics={}",
		report.scene.asset_group_ownership_count,
		report.scene.asset_group_owned_mesh_primitive_count,
		report.scene.asset_group_owned_material_count,
		report.scene.asset_group_owned_image_count,
		report.scene.asset_group_owned_dynamics_count
	);
	println!(
		"scoped_assets: active_groups={} missing={:?} mesh_primitives={} materials={} images={} dynamics={}",
		report.scene.scoped_active_asset_group_count,
		report.scene.scoped_missing_active_asset_groups,
		report.scene.scoped_resident_mesh_primitive_count,
		report.scene.scoped_resident_material_count,
		report.scene.scoped_resident_image_count,
		report.scene.scoped_resident_dynamics_count
	);
	for group in report.scene.asset_group_ownership.iter().take(16) {
		println!(
			"asset_ownership[{}]: mesh_primitives={} {} materials={} {} images={} {} dynamics={} {}",
			group.group_id,
			group.mesh_primitives.len(),
			debug_preview(&group.mesh_primitives, 8),
			group.materials.len(),
			debug_preview(&group.materials, 12),
			group.images.len(),
			debug_preview(&group.images, 12),
			group.dynamics_source_ids.len(),
			debug_preview(&group.dynamics_source_ids, 8)
		);
	}
	println!(
		"image_sources: {} / {} images, {} bytes, MIME {:?}",
		report.scene.image_source_count, report.scene.image_count, report.scene.image_source_bytes, report.scene.image_source_mime_counts
	);
	println!(
		"image_source_metadata: color_space {:?}, texture_type {:?}, texture_shape {:?}, source_layout {:?}",
		report.scene.image_source_color_space_counts,
		report.scene.image_source_texture_type_counts,
		report.scene.image_source_texture_shape_counts,
		report.scene.image_source_layout_counts
	);
	println!(
		"image_pixel_formats: {:?}, non_rgba8={}",
		report.scene.image_pixel_format_counts, report.scene.non_rgba8_image_count
	);
	if !report.scene.largest_image_sources.is_empty() {
		println!("largest_image_sources:");
		for source in &report.scene.largest_image_sources {
			println!(
				"  image[{}]: {}x{} {:?} {} bytes mime={:?} source_format={:?} channels={:?} color_space={:?} texture_type={:?} texture_shape={:?} source_layout={:?} unity_generate_cubemap={:?} srgb={:?} name={:?} uri={:?}",
				source.index,
				source.width,
				source.height,
				source.pixel_format,
				source.byte_length,
				source.mime_type,
				source.source_pixel_format,
				source.channels,
				source.color_space,
				source.texture_type,
				source.texture_shape,
				source.source_layout,
				source.unity_generate_cubemap,
				source.srgb,
				source.name,
				source.uri
			);
		}
	}
	println!("shading: {:?}", report.scene.shading_counts);
	println!("alpha: {:?}", report.scene.alpha_counts);
	println!("liltoon_features: {:?}", report.scene.liltoon_feature_counts);
	println!("visible_shading: {:?}", report.scene.visible_shading_counts);
	println!("visible_alpha: {:?}", report.scene.visible_alpha_counts);
	println!("visible_materials: {:?}", report.scene.visible_material_indices);
	let visible_material_indices: BTreeSet<usize> = report.scene.visible_material_indices.iter().copied().collect();
	if visible_materials_only {
		println!("materials: visible only ({} unique indices)", visible_material_indices.len());
	}
	if let Some(h) = &report.humanoid {
		println!(
			"humanoid: bones={} left_eye={:?} right_eye={:?}",
			h.bone_count, h.left_eye_node, h.right_eye_node
		);
	} else {
		println!("humanoid: none");
	}
	if let Some(e) = &report.expressions {
		println!("expressions: presets={}", e.preset_count);
	} else {
		println!("expressions: none");
	}
	if visible_meshes {
		println!("visible_mesh_nodes:");
		for node in &report.scene.visible_mesh_nodes {
			println!(
				"  node[{}]: mesh={} skin={:?} path={:?} name={:?}",
				node.node, node.mesh, node.skin, node.path, node.name
			);
			for material in &node.materials {
				println!(
					"    prim[{}]: material[{}] name={:?} source={:?} shading={:?} alpha={:?} cutoff={} zwrite={} skipped={} morph_targets={} nonzero_morphs={}",
					material.primitive,
					material.index,
					material.name,
					material.source_shader,
					material.shading,
					material.alpha_mode,
					material.alpha_cutoff,
					material.transparent_with_z_write,
					material.draw_skipped_fully_transparent,
					material.morph_target_count,
					material.nonzero_morph_weights.len()
				);
				for morph in material.nonzero_morph_weights.iter().take(8) {
					println!(
						"      morph[{}]: name={:?} weight={} pos_delta_sum={} nrm_delta_sum={}",
						morph.index, morph.name, morph.weight, morph.position_delta_abs_sum, morph.normal_delta_abs_sum
					);
				}
				if material.nonzero_morph_weights.len() > 8 {
					println!("      morph: ... {} more", material.nonzero_morph_weights.len() - 8);
				}
			}
		}
	}
	for material in report
		.scene
		.materials
		.iter()
		.filter(|material| !visible_materials_only || visible_material_indices.contains(&material.index))
	{
		println!(
			"material[{}]: name={:?} source={:?}/{:?} rq={:?} source_params=float:{} color:{} shading={:?} alpha={:?} cutoff={} cull={:?} double_sided={} tex={:?} normal={:?}/{} eye_like={}",
			material.index,
			material.name,
			material.material_family,
			material.source_shader,
			material.render_queue,
			material.source_float_param_count,
			material.source_color_param_count,
			material.shading,
			material.alpha_mode,
			material.alpha_cutoff,
			material.cull_mode,
			material.double_sided,
			material.base_color_texture_index,
			material.normal_texture_index,
			material.normal_texture_scale,
			material.eye_like_name
		);
		if let Some(mtoon) = &material.mtoon {
			println!(
				"  mtoon: zwrite={} shade={:?} shade_tex={:?} shift={} toony={} rim={:?} matcap_tex={:?} reflection_tex={:?} outline={:?}/{} emissive={:?}",
				mtoon.transparent_with_z_write,
				mtoon.shade_color_factor,
				mtoon.shade_multiply_texture_index,
				mtoon.shading_shift_factor,
				mtoon.shading_toony_factor,
				mtoon.parametric_rim_color_factor,
				mtoon.matcap_texture_index,
				mtoon.reflection_cube_texture_index,
				mtoon.outline_width_mode,
				mtoon.outline_width_factor,
				mtoon.emissive_factor
			);
		}
		if !material.source_render_float_params.is_empty() {
			println!("  liltoon_render_state: {:?}", material.source_render_float_params);
		}
	}
	for warning in &report.warnings {
		println!("warning: {warning}");
	}
	Ok(())
}

fn run_vmc(command: VmcCommands) -> Result<(), String> {
	use std::net::SocketAddr;

	match command {
		VmcCommands::Listen { port, frame } => {
			let addr = SocketAddr::from(([0, 0, 0, 0], port));
			let mut marionette = un_avatar_vmc::VmcMarionette::bind(addr).map_err(|e| format!("bind {addr}: {e}"))?;
			eprintln!("un-avatar vmc listen: UDP {addr} (Ctrl+Cで終了)");
			let mut seq = 0u64;
			loop {
				match marionette.recv_and_apply() {
					Ok((_from, _n, events)) => {
						seq = seq.wrapping_add(1);
						if frame {
							let line = serde_json::to_string(&marionette.assemble_frame(seq, un_avatar_vmc::wall_clock_ns()))
								.map_err(|e| e.to_string())?;
							println!("{line}");
						} else {
							for ev in events {
								let line = serde_json::to_string(&ev).map_err(|e| e.to_string())?;
								println!("{line}");
							}
						}
					}
					Err(un_avatar_vmc::RecvApplyError::Io(e)) => return Err(format!("recv: {e}")),
					Err(un_avatar_vmc::RecvApplyError::Decode {
						from,
						nbytes,
						err,
						payload_head_hex,
					}) => {
						eprintln!("un-avatar vmc listen: decode from {from} nbytes={nbytes}: {err}; hex_head={payload_head_hex}");
					}
				}
			}
		}
	}
}

fn run_convert(
	plugin_dirs: &[PathBuf],
	input: PathBuf,
	output: PathBuf,
	input_format: Option<String>,
	output_format: Option<String>,
	json_report: Option<PathBuf>,
) -> Result<(), String> {
	let reg = io_registry_for_cli(plugin_dirs)?;
	let cached_bytes = cached_binary_import_bytes(&input);
	let probe = import_probe_for_path(&input, cached_bytes.clone());
	let importer: &dyn AvatarImporter = if let Some(ref s) = input_format {
		let id = FormatId::new(s.as_str());
		reg.importer_by_id(&id)
			.ok_or_else(|| format!("指定の importer が登録されていません: {s}"))?
	} else {
		reg.best_importer_for(&probe).ok_or_else(|| {
			"入力に合う importer が見つかりません（VRM / glTF / .unavatar、`--plugin-dir`、または --input-format を確認）".to_string()
		})?
	};
	let mut ictx = ImportContext {
		asset_root: input.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(".")),
		temp_dir: std::env::temp_dir(),
		..ImportContext::dummy()
	};
	let import_desc = importer.descriptor();
	let imported = importer
		.import(
			&mut ictx,
			import_input_for_path(&input, &import_desc.id, cached_bytes),
			ImportOptions,
		)
		.map_err(|e| e.to_string())?;
	let exporter: &dyn AvatarExporter = if let Some(ref s) = output_format {
		let id = FormatId::new(s.as_str());
		let exp = reg
			.exporter_by_id(&id)
			.ok_or_else(|| format!("指定の exporter が登録されていません: {s}"))?;
		if exp.can_export(&imported.document, &ExportOptions) != ExportCapability::Supported {
			return Err(format!("exporter {s} はこのドキュメントを書き出せません"));
		}
		exp
	} else {
		reg.best_exporter_for(&imported.document, &output)
			.ok_or_else(|| "出力に使える exporter が見つかりません（拡張子、`--plugin-dir`、または --output-format を確認）".to_string())?
	};
	let export_desc = exporter.descriptor();
	let mut ectx = ExportContext {
		output_root: output.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(".")),
		temp_dir: std::env::temp_dir(),
	};
	let export_result = exporter
		.export(&mut ectx, &imported.document, ExportOutput::Path(output), ExportOptions)
		.map_err(|e| e.to_string())?;
	if let Some(ref path) = json_report {
		let bundle = ConvertJsonReport {
			import_format_id: import_desc.id.0.clone(),
			export_format_id: export_desc.id.0.clone(),
			import_provider_plugin_id: import_desc.provider_plugin_id.clone(),
			export_provider_plugin_id: export_desc.provider_plugin_id.clone(),
			import_report: imported.report,
			export_report: export_result.report,
		};
		write_convert_json_report(path, &bundle)?;
	}
	Ok(())
}

fn debug_preview<T: std::fmt::Debug>(items: &[T], limit: usize) -> String {
	if items.len() <= limit {
		return format!("{items:?}");
	}
	format!("{:?} ... (+{} more)", &items[..limit], items.len() - limit)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn test_scene_node(name: &str, transform: [f32; 16], children: Vec<usize>) -> un_avatar_core::UnaSceneNode {
		un_avatar_core::UnaSceneNode {
			name: Some(name.to_string()),
			source_node_id: None,
			resolved_node_id: None,
			visible: true,
			transform,
			children,
			mesh: None,
			skin: None,
			probe_anchor_node: None,
			local_bounds: None,
		}
	}

	fn translation_mat4(x: f32, y: f32, z: f32) -> [f32; 16] {
		let mut m = test_identity_mat4();
		m[12] = x;
		m[13] = y;
		m[14] = z;
		m
	}

	fn test_identity_mat4() -> [f32; 16] {
		[
			1.0, 0.0, 0.0, 0.0, //
			0.0, 1.0, 0.0, 0.0, //
			0.0, 0.0, 1.0, 0.0, //
			0.0, 0.0, 0.0, 1.0,
		]
	}

	#[test]
	fn dynamics_vertex_probe_node_samples_are_displacement_based_not_path_whitelisted() {
		let node_paths = vec![
			Some("Avatar/GenericAccessory".to_string()),
			Some("Avatar/StillNode".to_string()),
			Some("Avatar/OtherDynamic".to_string()),
		];
		let rest_world = vec![Mat4::IDENTITY, Mat4::IDENTITY, Mat4::IDENTITY];
		let settled_world = vec![
			Mat4::from_translation(Vec3::new(0.01, 0.0, 0.0)),
			Mat4::IDENTITY,
			Mat4::from_translation(Vec3::new(0.03, 0.0, 0.0)),
		];

		let samples = dynamics_vertex_probe_node_samples(&node_paths, &rest_world, &settled_world);

		assert_eq!(samples.len(), 2);
		assert_eq!(samples[0].path, "Avatar/OtherDynamic");
		assert_eq!(samples[1].path, "Avatar/GenericAccessory");
		assert!(samples.iter().all(|sample| sample.path != "Avatar/StillNode"));
	}

	#[test]
	fn dynamics_source_collider_audit_flags_collision_enabled_sources_without_colliders() {
		let doc = UnaDocument {
			unavatar: Some(un_avatar_core::UnaUnavatarExtension {
				spec_version: "2.0".to_string(),
				source: serde_json::json!({
					"dynamics": [
						{
							"id": "physbone:Avatar/PB/LooseAccessory",
							"sourceParams": {
								"allowCollision": true,
								"colliders": []
							}
						},
						{
							"id": "physbone:Avatar/PB/ClothPanel/ClothPanel_L",
							"sourceParams": {
								"allowCollision": true,
								"colliders": [
									{"component": {"path": "Avatar/PB/BodyColliders/Chest"}},
									{"component": {"path": "Avatar/PB/BodyColliders/Spine"}}
								]
							}
						},
						{
							"id": "physbone:Avatar/Hair",
							"sourceParams": {
								"allowCollision": true,
								"colliders": []
							}
						}
					]
				}),
			}),
			..UnaDocument::default()
		};
		let audit = dynamics_source_collider_audit(&doc, None);
		assert_eq!(audit.collision_enabled_empty_collider_count, 2);
		assert_eq!(
			audit.collision_enabled_empty_collider_source_ids,
			vec!["physbone:Avatar/PB/LooseAccessory", "physbone:Avatar/Hair"]
		);
		assert_eq!(
			audit.collision_enabled_empty_collider_samples,
			vec!["physbone:Avatar/PB/LooseAccessory", "physbone:Avatar/Hair"]
		);
	}

	#[test]
	fn dynamics_source_empty_collider_audit_uses_source_payload_not_path_shape() {
		assert!(dynamics_source_has_collision_enabled_empty_colliders(Some(true), 0));
		assert!(!dynamics_source_has_collision_enabled_empty_colliders(Some(true), 1));
		assert!(!dynamics_source_has_collision_enabled_empty_colliders(Some(false), 0));
		assert!(!dynamics_source_has_collision_enabled_empty_colliders(None, 0));
	}

	#[test]
	fn dynamics_source_collider_audit_can_filter_to_active_sources() {
		let doc = UnaDocument {
			unavatar: Some(un_avatar_core::UnaUnavatarExtension {
				spec_version: "2.0".to_string(),
				source: serde_json::json!({
					"dynamics": [
						{
							"id": "physbone:InactiveOutfit/PB/LooseAccessory",
							"sourceParams": {
								"allowCollision": true,
								"colliders": []
							}
						},
						{
							"id": "physbone:ActiveOutfit/PB/ClothPanel/ClothPanel_L",
							"sourceParams": {
								"allowCollision": true,
								"colliders": [
									{"component": {"path": "ActiveOutfit/PB/BodyColliders/Chest"}},
									{"component": {"path": "ActiveOutfit/PB/BodyColliders/Spine"}}
								]
							}
						}
					]
				}),
			}),
			..UnaDocument::default()
		};
		let active_source_ids = BTreeSet::from(["physbone:ActiveOutfit/PB/ClothPanel/ClothPanel_L".to_string()]);
		let audit = dynamics_source_collider_audit(&doc, Some(&active_source_ids));
		assert_eq!(audit.collision_enabled_empty_collider_count, 0);
		assert!(audit.collision_enabled_empty_collider_source_ids.is_empty());
	}

	#[test]
	fn dynamics_vertex_probe_morph_samples_are_effect_based_not_name_whitelisted() {
		let primitive = un_avatar_core::UnaMeshBuffers {
			name: None,
			vertex_payload_id: None,
			positions: vec![[0.0, 0.0, 0.0], [0.1, 0.0, 0.0], [0.2, 0.0, 0.0]],
			normals: None,
			tangents: None,
			tex_coords_0: None,
			tex_coords_1: None,
			tex_coords_2: None,
			tex_coords_3: None,
			colors_0: None,
			joints: None,
			weights: None,
			indices: None,
			material_index: None,
			morph_targets: vec![
				un_avatar_core::UnaMorphTargetDeltas {
					position_deltas: vec![[0.001, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]],
					normal_deltas: None,
					tangent_deltas: None,
				},
				un_avatar_core::UnaMorphTargetDeltas {
					position_deltas: vec![[0.02, 0.0, 0.0], [0.03, 0.0, 0.0], [0.0, 0.0, 0.0]],
					normal_deltas: None,
					tangent_deltas: None,
				},
			],
			morph_target_names: vec!["Breast_Fix".to_string(), "GenericWide".to_string()],
			default_morph_weights: vec![0.0, 0.0],
		};

		let samples = dynamics_vertex_probe_region_morph_targets(&primitive, &primitive.positions, |_| true);

		assert_eq!(samples.len(), 2);
		assert_eq!(samples[0].name, "GenericWide");
		assert_eq!(samples[0].affected_vertices, 2);
		assert_eq!(samples[1].name, "Breast_Fix");
	}

	#[test]
	fn dynamics_vertex_probe_collider_path_summaries_are_source_scoped() {
		let scene = un_avatar_core::UnaSceneSnapshot {
			nodes: vec![
				test_scene_node("root", test_identity_mat4(), vec![1, 2]),
				test_scene_node("source_a", translation_mat4(0.0, 0.0, 0.0), Vec::new()),
				test_scene_node("source_b", translation_mat4(1.0, 0.0, 0.0), Vec::new()),
			],
			roots: vec![0],
			..Default::default()
		};
		let colliders = vec![
			BoneColliderPrimitive::Sphere { node: 1, radius: 0.1 },
			BoneColliderPrimitive::Sphere { node: 2, radius: 0.1 },
		];
		let collider_source_ids = vec!["physbone:a".to_string(), "physbone:b".to_string()];
		let collider_paths = vec!["BodyColliders/A".to_string(), "BodyColliders/B".to_string()];
		let tail_samples = vec![DynamicsTailSample {
			source_id: "physbone:a".to_string(),
			curr_tail: [0.05, 0.0, 0.0],
			hit_radius: 0.02,
			..Default::default()
		}];

		let summaries = dynamics_vertex_probe_collider_path_summaries_for_samples(
			&scene,
			&colliders,
			&collider_source_ids,
			&collider_paths,
			&tail_samples,
			&BTreeMap::new(),
		);

		assert_eq!(summaries.len(), 1);
		assert_eq!(summaries[0].collider_path, "BodyColliders/A");
		assert_eq!(summaries[0].candidate_count, 1);
		assert_eq!(summaries[0].penetrating_count, 1);
		assert_eq!(summaries[0].source_count, 1);
		assert_eq!(summaries[0].sample_source_ids, vec!["physbone:a"]);
	}

	#[test]
	fn dynamics_vertex_probe_collider_path_summaries_keep_global_colliders_with_scoped_sources() {
		let scene = un_avatar_core::UnaSceneSnapshot {
			nodes: vec![
				test_scene_node("root", test_identity_mat4(), vec![1, 2, 3]),
				test_scene_node("global", translation_mat4(0.0, 0.0, 0.0), Vec::new()),
				test_scene_node("source_a", translation_mat4(0.0, 0.0, 0.0), Vec::new()),
				test_scene_node("source_b", translation_mat4(1.0, 0.0, 0.0), Vec::new()),
			],
			roots: vec![0],
			..Default::default()
		};
		let colliders = vec![
			BoneColliderPrimitive::Sphere { node: 1, radius: 0.1 },
			BoneColliderPrimitive::Sphere { node: 2, radius: 0.1 },
			BoneColliderPrimitive::Sphere { node: 3, radius: 0.1 },
		];
		let collider_source_ids = vec!["".to_string(), "physbone:a".to_string(), "physbone:b".to_string()];
		let collider_paths = vec![
			"BodyColliders/Global".to_string(),
			"BodyColliders/A".to_string(),
			"BodyColliders/B".to_string(),
		];
		let tail_samples = vec![DynamicsTailSample {
			source_id: "physbone:a".to_string(),
			curr_tail: [0.05, 0.0, 0.0],
			hit_radius: 0.02,
			..Default::default()
		}];

		let summaries = dynamics_vertex_probe_collider_path_summaries_for_samples(
			&scene,
			&colliders,
			&collider_source_ids,
			&collider_paths,
			&tail_samples,
			&BTreeMap::new(),
		);

		let paths = summaries.iter().map(|summary| summary.collider_path.as_str()).collect::<Vec<_>>();
		assert_eq!(paths, vec!["BodyColliders/A", "BodyColliders/Global"]);
		assert!(summaries.iter().all(|summary| summary.candidate_count == 1));
	}

	#[test]
	fn dynamics_vertex_probe_collider_path_summaries_respect_inside_bounds_spheres() {
		let scene = un_avatar_core::UnaSceneSnapshot {
			nodes: vec![
				test_scene_node("root", test_identity_mat4(), vec![1]),
				test_scene_node("bounds", translation_mat4(0.0, 0.0, 0.0), Vec::new()),
			],
			roots: vec![0],
			..Default::default()
		};
		let colliders = vec![BoneColliderPrimitive::LocalSphere {
			node: 1,
			center: [0.0, 0.0, 0.0],
			radius: 0.1,
			inside_bounds: true,
			bones_as_sphere: true,
		}];
		let collider_source_ids = vec!["physbone:a".to_string()];
		let collider_paths = vec!["BodyColliders/InsideSphere".to_string()];
		let tail_samples = vec![
			DynamicsTailSample {
				source_id: "physbone:a".to_string(),
				curr_tail: [0.03, 0.0, 0.0],
				hit_radius: 0.02,
				..Default::default()
			},
			DynamicsTailSample {
				source_id: "physbone:a".to_string(),
				curr_tail: [0.20, 0.0, 0.0],
				hit_radius: 0.02,
				..Default::default()
			},
		];

		let summaries = dynamics_vertex_probe_collider_path_summaries_for_samples(
			&scene,
			&colliders,
			&collider_source_ids,
			&collider_paths,
			&tail_samples,
			&BTreeMap::new(),
		);

		assert_eq!(summaries.len(), 1);
		assert!(summaries[0].inside_bounds);
		assert_eq!(summaries[0].candidate_count, 2);
		assert_eq!(summaries[0].penetrating_count, 1);
		assert!(summaries[0].min_margin < 0.0);
		assert!((summaries[0].min_threshold - 0.08).abs() < 1e-5);
	}

	#[test]
	fn dynamics_vertex_probe_collider_path_summaries_respect_inside_bounds_planes() {
		let scene = un_avatar_core::UnaSceneSnapshot {
			nodes: vec![
				test_scene_node("root", test_identity_mat4(), vec![1]),
				test_scene_node("plane", translation_mat4(0.0, 0.0, 0.0), Vec::new()),
			],
			roots: vec![0],
			..Default::default()
		};
		let colliders = vec![BoneColliderPrimitive::LocalPlane {
			node: 1,
			center: [0.0, 0.0, 0.0],
			normal: [0.0, 1.0, 0.0],
			inside_bounds: true,
		}];
		let collider_source_ids = vec!["physbone:a".to_string()];
		let collider_paths = vec!["BodyColliders/InsidePlane".to_string()];
		let tail_samples = vec![
			DynamicsTailSample {
				source_id: "physbone:a".to_string(),
				curr_tail: [0.0, -0.05, 0.0],
				hit_radius: 0.0,
				..Default::default()
			},
			DynamicsTailSample {
				source_id: "physbone:a".to_string(),
				curr_tail: [0.0, 0.05, 0.0],
				hit_radius: 0.0,
				..Default::default()
			},
		];

		let summaries = dynamics_vertex_probe_collider_path_summaries_for_samples(
			&scene,
			&colliders,
			&collider_source_ids,
			&collider_paths,
			&tail_samples,
			&BTreeMap::new(),
		);

		assert_eq!(summaries.len(), 1);
		assert!(summaries[0].inside_bounds);
		assert_eq!(summaries[0].candidate_count, 2);
		assert_eq!(summaries[0].penetrating_count, 1);
		assert!(summaries[0].min_margin < 0.0);
		assert!(summaries[0].min_distance > 0.0);
	}

	#[test]
	fn dynamics_vertex_probe_collider_path_summaries_include_projection_counts() {
		let scene = un_avatar_core::UnaSceneSnapshot {
			nodes: vec![
				test_scene_node("root", test_identity_mat4(), vec![1]),
				test_scene_node("source_a", translation_mat4(0.0, 0.0, 0.0), Vec::new()),
			],
			roots: vec![0],
			..Default::default()
		};
		let colliders = vec![BoneColliderPrimitive::Sphere { node: 1, radius: 0.1 }];
		let collider_source_ids = vec!["physbone:a".to_string()];
		let collider_paths = vec!["BodyColliders/A".to_string()];
		let tail_samples = vec![DynamicsTailSample {
			source_id: "physbone:a".to_string(),
			curr_tail: [0.05, 0.0, 0.0],
			hit_radius: 0.02,
			..Default::default()
		}];
		let projection_counts = BTreeMap::from([("BodyColliders/A".to_string(), 7)]);

		let summaries = dynamics_vertex_probe_collider_path_summaries_for_samples(
			&scene,
			&colliders,
			&collider_source_ids,
			&collider_paths,
			&tail_samples,
			&projection_counts,
		);

		assert_eq!(summaries.len(), 1);
		assert_eq!(summaries[0].projection_count, 7);
	}

	#[test]
	fn menu_graph_node_path_reports_truncated_cycles() {
		let nodes = vec![
			DiagnoseModularAvatarMenuGraphNode {
				node_index: 0,
				component_index: 10,
				menu_key: "component:10".to_string(),
				short_type: "ModularAvatarMenuGroup".to_string(),
				kind: "group".to_string(),
				label: Some("A".to_string()),
				hierarchy_path: Some("Root/A".to_string()),
				parent_path: Some("Root/B".to_string()),
				parent_node_index: Some(1),
				parent_component_index: Some(11),
				child_component_indices: Vec::new(),
				menu_to_append_path: None,
				install_target_menu_path: None,
				installer_path: None,
			},
			DiagnoseModularAvatarMenuGraphNode {
				node_index: 1,
				component_index: 11,
				menu_key: "component:11".to_string(),
				short_type: "ModularAvatarMenuGroup".to_string(),
				kind: "group".to_string(),
				label: Some("B".to_string()),
				hierarchy_path: Some("Root/B".to_string()),
				parent_path: Some("Root/A".to_string()),
				parent_node_index: Some(0),
				parent_component_index: Some(10),
				child_component_indices: Vec::new(),
				menu_to_append_path: None,
				install_target_menu_path: None,
				installer_path: None,
			},
		];

		let path = menu_graph_node_path(&nodes, 0);
		assert!(path.truncated);
		assert_eq!(path.labels, vec!["B".to_string(), "A".to_string()]);
	}

	#[test]
	fn parse_plugin_path_trims_and_skips_empty() {
		let raw = if cfg!(windows) {
			OsStr::new(" a ; ;b ")
		} else {
			OsStr::new(" a : :b ")
		};
		let v = parse_plugin_path_list(raw);
		assert_eq!(v, vec![PathBuf::from("a"), PathBuf::from("b")]);
	}

	#[test]
	fn merge_unique_plugin_dirs_preserves_order_and_dedups() {
		let a = PathBuf::from("/x/a");
		let b = PathBuf::from("/x/b");
		let merged = merge_unique_plugin_dirs(vec![a.clone(), b.clone()], &[a.clone(), PathBuf::from("/x/c")]);
		assert_eq!(merged, vec![a, b, PathBuf::from("/x/c")]);
	}

	#[test]
	fn normalize_cli_args_treats_path_as_diagnose_shorthand() {
		let args = ["un-avatar", "target/tmp/model.vrm", "--json"].map(OsString::from);
		let normalized = normalize_cli_args(args);
		assert_eq!(
			normalized,
			vec![
				OsString::from("un-avatar"),
				OsString::from("diagnose"),
				OsString::from("target/tmp/model.vrm"),
				OsString::from("--json"),
			]
		);
	}

	#[test]
	fn normalize_cli_args_preserves_explicit_commands_and_global_plugin_dir() {
		let args = ["un-avatar", "--plugin-dir", "plugins/sample-io-plugin", "formats", "list"].map(OsString::from);
		let normalized = normalize_cli_args(args.clone());
		assert_eq!(normalized, args);
	}

	#[test]
	fn normalize_cli_args_preserves_vertex_probe_command() {
		let args = ["un-avatar", "dynamics-vertex-probe", "target/tmp/model.unavatar", "--json"].map(OsString::from);
		let normalized = normalize_cli_args(args.clone());
		assert_eq!(normalized, args);
		assert!(is_known_command(OsStr::new("dynamics-vertex-probe")));
	}

	#[test]
	fn dynamics_scan_counts_unavatar_source_params_without_importing_assets() {
		let path = std::env::temp_dir().join(format!("un-avatar-dynamics-scan-{}.gltf", std::process::id()));
		let json = r#"{
			"asset": {"version": "2.0"},
			"extensions": {
				"UN_avatar": {
					"dynamics": {
						"groups": [
							{"sourceParams": {
								"pull": 0.25,
								"pullCurve": [],
								"spring": 0.35,
								"springCurve": [],
								"momentum": 0.45,
								"momentumCurve": [],
								"stiffness": 0.55,
								"stiffnessCurve": [],
								"gravityFalloff": 0.65,
								"gravityFalloffCurve": [],
								"immobile": 0.75,
								"immobileCurve": [],
								"immobileType": "world",
								"integrationType": "vrcAdvanced",
								"limitRotation": [10, 20, 30]
							}},
							{"sourceParams": {
								"pull": 0.1,
								"pullCurve": [],
								"spring": 0.2,
								"springCurve": [],
								"momentum": 0.3,
								"momentumCurve": [],
								"stiffness": 0.4,
								"stiffnessCurve": [],
								"gravityFalloff": 0.5,
								"gravityFalloffCurve": [],
								"immobile": 0.6,
								"immobileCurve": [],
								"immobileType": "allMotion",
								"integrationType": "standard",
								"limitRotation": [0, 0, 0]
							}}
						]
					}
				}
			}
		}"#;
		fs::write(&path, json).unwrap();
		let report = dynamics_scan_report(&path).unwrap();
		let _ = fs::remove_file(&path);

		assert_eq!(report.extension_keys, vec!["UN_avatar".to_string()]);
		assert_eq!(report.source_params_count, 2);
		assert!(report.missing_required_source_params.is_empty());
		assert_eq!(report.required_source_param_counts["pull"], 2);
		assert_eq!(report.required_source_param_counts["limitRotation"], 2);
		assert_eq!(report.curve_counts["pullCurve"], 2);
		let pull_range = report.numeric_ranges["pull"];
		assert_eq!(pull_range.count, 2);
		assert_eq!(pull_range.min, 0.1);
		assert_eq!(pull_range.max, 0.25);
	}

	#[test]
	fn dynamics_scan_reports_missing_current_exporter_terms() {
		let path = std::env::temp_dir().join(format!("un-avatar-dynamics-scan-missing-{}.gltf", std::process::id()));
		let json = r#"{
			"asset": {"version": "2.0"},
			"extensions": {
				"UN_avatar": {
					"dynamics": {
						"groups": [
							{"sourceParams": {
								"pull": 0.25,
								"spring": 0.35
							}}
						]
					}
				}
			}
		}"#;
		fs::write(&path, json).unwrap();
		let report = dynamics_scan_report(&path).unwrap();
		let _ = fs::remove_file(&path);
		let err = require_current_exporter_dynamics_scan(&report).unwrap_err();

		assert_eq!(report.source_params_count, 1);
		assert!(report
			.missing_required_source_params
			.iter()
			.any(|missing| missing == "pullCurve=0/1"));
		assert!(report
			.missing_required_source_params
			.iter()
			.any(|missing| missing == "limitRotation=0/1"));
		assert!(err.contains("pullCurve=0/1"));
	}

	#[test]
	fn dynamics_import_audit_reports_imported_runtime_evidence() {
		let path = std::env::temp_dir().join(format!("un-avatar-dynamics-import-audit-{}.gltf", std::process::id()));
		let json = r#"{
			"asset": {"version": "2.0"},
			"scene": 0,
			"scenes": [{"nodes": [0]}],
			"nodes": [
				{"name": "Avatar", "children": [1, 3], "translation": [0.0, 0.0, 0.0]},
				{"name": "Root", "children": [2], "translation": [0.0, 0.0, 0.0]},
				{"name": "Tip", "translation": [0.0, 0.2, 0.0]},
				{"name": "ClothRoot", "children": [4], "translation": [0.2, 0.0, 0.0]},
				{"name": "ClothTip", "translation": [0.0, 0.2, 0.0]}
			],
			"extensions": {
				"UN_avatar": {
					"specVersion": "0.1-preview",
					"nodes": [
						{"nodeId": "node_root", "path": "Root"},
						{"nodeId": "node_tip", "path": "Root/Tip"},
						{"nodeId": "node_cloth_root", "path": "ClothRoot"},
						{"nodeId": "node_cloth_tip", "path": "ClothRoot/ClothTip"}
					],
					"dynamics": [
						{
							"id": "audit_hair",
							"source": "vrc_physbone",
							"roots": [{"nodeId": "node_root", "path": "Root"}],
							"drag": 0.2,
							"gravity": [0.0, -0.2, 0.0],
							"radius": 0.03,
							"sourceParams": {
								"integrationType": "Advanced",
								"pull": 0.25,
								"pullCurve": {"keys": [{"time": 0.0, "value": 1.0}, {"time": 1.0, "value": 0.5}]},
								"spring": 0.15,
								"springCurve": {"keys": []},
								"momentum": 0.35,
								"momentumCurve": {"keys": [{"time": 0.0, "value": 1.0}, {"time": 1.0, "value": 0.5}]},
								"stiffness": 0.45,
								"stiffnessCurve": {"keys": [{"time": 0.0, "value": 1.0}, {"time": 1.0, "value": 0.25}]},
								"gravityFalloff": 0.6,
								"gravityFalloffCurve": {"keys": [{"time": 0.0, "value": 1.0}, {"time": 1.0, "value": 0.5}]},
								"immobile": 0.35,
								"immobileCurve": {"keys": [{"time": 0.0, "value": 1.0}, {"time": 1.0, "value": 0.5}]},
								"immobileType": 1,
								"limitRotation": [10.0, 20.0, 30.0]
							}
						},
						{
							"id": "audit_cloth",
							"source": "vrc_physbone",
							"roots": [{"nodeId": "node_cloth_root", "path": "ClothRoot"}],
							"drag": 0.2,
							"gravity": [0.0, -0.2, 0.0],
							"radius": 0.03,
							"sourceParams": {
								"integrationType": "Advanced",
								"pull": 0.25,
								"pullCurve": {"keys": []},
								"spring": 0.15,
								"springCurve": {"keys": []},
								"momentum": 0.35,
								"momentumCurve": {"keys": []},
								"stiffness": 0.45,
								"stiffnessCurve": {"keys": []},
								"gravityFalloff": 0.6,
								"gravityFalloffCurve": {"keys": []},
								"immobile": 0.35,
								"immobileCurve": {"keys": []},
								"immobileType": 1,
								"limitType": "Polar",
								"limitRotation": [10.0, 20.0, 30.0],
								"maxAngleX": 45.0,
								"maxAngleZ": 90.0
							}
						}
					]
				}
			}
		}"#;
		fs::write(&path, json).unwrap();
		let report = dynamics_import_audit_report(&[], &path, None, None).unwrap();
		let _ = fs::remove_file(&path);

		assert_eq!(report.source_params_count, 2);
		assert_eq!(report.group_count, 2);
		assert_eq!(report.enabled_group_count, 2);
		assert_eq!(report.chain_joint_count, 4);
		assert_eq!(report.response_group_count, 2);
		assert_eq!(report.source_angle_limit_group_count, 2);
		assert_eq!(report.active_angle_limit_group_count, 2);
		assert_eq!(report.hard_angle_constraint_group_count, 1);
		assert_eq!(report.cloth_angle_limit_metadata_only_count, 1);
		assert_eq!(report.node_constraint_count, 0);
		assert_eq!(report.parent_node_constraint_count, 0);
		assert_eq!(report.parent_node_constraint_source_count, 0);
		assert_eq!(report.parent_node_constraint_multi_source_count, 0);
		assert!(report.missing_runtime_evidence.is_empty());
		assert_eq!(report.runtime_ranges["pull"].min, 0.25);
		assert!((report.runtime_ranges["spring"].max - 0.35).abs() < 1e-6);
		assert_eq!(report.sample_counts["pullSamples"], 2);
		assert_eq!(report.sample_counts["stiffnessSamples"], 2);
	}

	#[test]
	fn dynamics_import_node_constraint_counts_match_diagnose_parent_policy() {
		let scene = UnaSceneSnapshot {
			node_constraints: vec![
				un_avatar_core::UnaNodeConstraint {
					target_node: 1,
					source_node: 0,
					weight: 1.0,
					kind: UnaNodeConstraintKind::Parent {
						translate_x: true,
						translate_y: true,
						translate_z: true,
						rotate_x: true,
						rotate_y: true,
						rotate_z: true,
						translation_at_rest: [0.0; 3],
						rotation_at_rest: [0.0; 3],
					},
					sources: Vec::new(),
				},
				un_avatar_core::UnaNodeConstraint {
					target_node: 2,
					source_node: 0,
					weight: 1.0,
					kind: UnaNodeConstraintKind::Parent {
						translate_x: true,
						translate_y: true,
						translate_z: true,
						rotate_x: true,
						rotate_y: true,
						rotate_z: true,
						translation_at_rest: [0.0; 3],
						rotation_at_rest: [0.0; 3],
					},
					sources: vec![
						un_avatar_core::UnaNodeConstraintSource {
							source_node: 0,
							weight: 0.5,
							translation_offset: [0.0; 3],
							rotation_offset: [0.0; 3],
						},
						un_avatar_core::UnaNodeConstraintSource {
							source_node: 3,
							weight: 0.5,
							translation_offset: [0.0; 3],
							rotation_offset: [0.0; 3],
						},
					],
				},
				un_avatar_core::UnaNodeConstraint {
					target_node: 4,
					source_node: 0,
					weight: 1.0,
					kind: UnaNodeConstraintKind::Rotation,
					sources: Vec::new(),
				},
			],
			..Default::default()
		};

		assert_eq!(dynamics_import_node_constraint_counts(Some(&scene)), (3, 2, 3, 1));
		assert_eq!(dynamics_import_node_constraint_counts(None), (0, 0, 0, 0));
	}

	#[test]
	fn dynamics_import_missing_runtime_evidence_distinguishes_raw_and_scoped_groups() {
		assert_eq!(
			dynamics_import_missing_runtime_evidence(3, 2, 0, 0, 0),
			Vec::<String>::new(),
			"active wardrobe with no resident dynamics should not fail raw sourceParams evidence"
		);
		let missing_raw = dynamics_import_missing_runtime_evidence(3, 0, 0, 0, 0);
		assert_eq!(
			missing_raw,
			vec!["sourceParams=3 but imported runtime dynamics groups=0".to_string()]
		);
		let missing_chain = dynamics_import_missing_runtime_evidence(3, 2, 2, 0, 2);
		assert_eq!(
			missing_chain,
			vec!["imported dynamics groups=2 but chain_joint_count=0".to_string()]
		);
		let missing_response = dynamics_import_missing_runtime_evidence(3, 2, 2, 2, 0);
		assert_eq!(
			missing_response,
			vec!["imported dynamics groups=2 but simulator response_group_count=0".to_string()]
		);
	}

	#[test]
	fn dynamics_import_group_sample_score_uses_structure_not_model_names() {
		let plain = un_avatar_core::UnaSpringBoneGroup {
			enabled: true,
			source_id: "physbone:Fixture/PB/PlainPanel".to_string(),
			bone_node_indices: vec![0, 1],
			..Default::default()
		};
		let structured = un_avatar_core::UnaSpringBoneGroup {
			enabled: true,
			source_id: "custom:generic_drape".to_string(),
			pull_samples: vec![0.2, 0.1],
			limit: Some(un_avatar_core::UnaDynamicsLimit {
				max_stretch: 0.25,
				..Default::default()
			}),
			bone_node_indices: vec![0, 1, 2, 3],
			..Default::default()
		};

		assert!(dynamics_import_group_sample_score(&structured, false) > dynamics_import_group_sample_score(&plain, false));
		assert!(dynamics_import_group_sample_score(&plain, true) > dynamics_import_group_sample_score(&plain, false));
	}

	#[test]
	fn dynamics_vertex_probe_mesh_cloth_assist_uses_shared_general_cloth_filter() {
		let physics_config = DynamicsPhysicsConfig::default().normalized();
		let config = dynamics_vertex_probe_mesh_cloth_assist_config(&physics_config);
		let categories = physics_config.categories;

		assert!(config.mesh_path_contains.is_empty());
		assert!(dynamics_mesh_cloth_assist_mesh_matches(
			Some("Avatar/LongCoatPanel"),
			&config.mesh_path_contains,
			&categories
		));
		assert!(dynamics_mesh_cloth_assist_mesh_matches(
			Some("Avatar/SleeveFrill"),
			&config.mesh_path_contains,
			&categories
		));
		assert!(dynamics_mesh_cloth_assist_mesh_matches(
			Some("Avatar/ブラウス_裾_L"),
			&config.mesh_path_contains,
			&categories
		));
		assert!(!dynamics_mesh_cloth_assist_mesh_matches(
			Some("Avatar/BodyMesh"),
			&config.mesh_path_contains,
			&categories
		));
		assert!(!dynamics_mesh_cloth_assist_mesh_matches(
			None,
			&config.mesh_path_contains,
			&categories
		));
	}

	#[test]
	fn dynamics_vertex_probe_mesh_cloth_assist_uses_profile_thresholds() {
		let physics_config = DynamicsPhysicsConfig {
			mesh_cloth_assist: DynamicsMeshClothAssistConfig {
				enabled: false,
				body_dominance_threshold: 0.72,
				min_existing_dynamic_weight: 0.03,
				seed_missing_dynamic_influence: false,
				max_assist_weight: 0.5,
				mesh_path_contains: vec!["sleeve panel".to_string()],
			},
			..Default::default()
		};

		let config = dynamics_vertex_probe_mesh_cloth_assist_config(&physics_config);

		assert!(config.enabled);
		assert_eq!(config.body_dominance_threshold, 0.72);
		assert_eq!(config.min_existing_dynamic_weight, 0.03);
		assert!(!config.seed_missing_dynamic_influence);
		assert_eq!(config.max_assist_weight, 0.5);
		assert_eq!(config.mesh_path_contains, vec!["sleeve_panel"]);
	}

	#[test]
	fn dynamics_mesh_cloth_assist_joint_roles_prefer_runtime_dynamic_membership() {
		let identity = test_identity_mat4();
		let skin = un_avatar_core::UnaSkin {
			joint_nodes: vec![0, 1, 2],
			inverse_bind_matrices: vec![identity, identity, identity],
			skeleton_node: None,
		};
		let node_paths = vec![
			Some("Avatar/Chest".to_string()),
			Some("Avatar/AccessoryRoot".to_string()),
			Some("Avatar/Cloth_Static".to_string()),
		];
		let runtime_dynamic_nodes = vec![1usize];

		let roles = dynamics_mesh_cloth_assist_joint_roles(&skin, 3, Some(&runtime_dynamic_nodes), |joint_index| {
			dynamics_mesh_cloth_assist_joint_leaf(&skin, &node_paths, joint_index)
		});

		assert_eq!(roles[0], DynamicsMeshClothAssistJointRole::Body);
		assert_eq!(roles[1], DynamicsMeshClothAssistJointRole::Dynamic);
		assert_eq!(roles[2], DynamicsMeshClothAssistJointRole::StaticCloth);
	}

	#[test]
	fn dynamics_mesh_cloth_assist_joint_roles_keep_cloth_alias_fallback_without_runtime_membership() {
		let identity = test_identity_mat4();
		let skin = un_avatar_core::UnaSkin {
			joint_nodes: vec![0, 1],
			inverse_bind_matrices: vec![identity, identity],
			skeleton_node: None,
		};
		let node_paths = vec![Some("Avatar/Chest".to_string()), Some("Avatar/ブラウス_裾_L".to_string())];

		let roles = dynamics_mesh_cloth_assist_joint_roles(&skin, 2, None, |joint_index| {
			dynamics_mesh_cloth_assist_joint_leaf(&skin, &node_paths, joint_index)
		});

		assert_eq!(roles[0], DynamicsMeshClothAssistJointRole::Body);
		assert_eq!(roles[1], DynamicsMeshClothAssistJointRole::Dynamic);
	}

	#[test]
	fn dynamics_import_mesh_cloth_assist_classifies_seed_candidates_from_connected_dynamic_evidence() {
		let identity = test_identity_mat4();
		let scene = un_avatar_core::UnaSceneSnapshot {
			nodes: vec![
				test_scene_node("Avatar", identity, vec![1, 2, 3, 4]),
				un_avatar_core::UnaSceneNode {
					name: Some("Cloth_Panel_Mesh".to_string()),
					transform: identity,
					mesh: Some(0),
					skin: Some(0),
					..test_scene_node("Cloth_Panel_Mesh", identity, Vec::new())
				},
				test_scene_node("Chest", identity, Vec::new()),
				test_scene_node("Cloth_Static_L", translation_mat4(0.0, 0.0, 0.0), Vec::new()),
				test_scene_node("Cloth_Dyn_L", translation_mat4(0.1, 0.0, 0.0), Vec::new()),
			],
			meshes: vec![vec![un_avatar_core::UnaMeshBuffers {
				name: Some("cloth".to_string()),
				vertex_payload_id: None,
				positions: vec![[0.0, 0.0, 0.0], [0.1, 0.0, 0.0], [0.2, 0.0, 0.0]],
				normals: None,
				tangents: None,
				tex_coords_0: None,
				tex_coords_1: None,
				tex_coords_2: None,
				tex_coords_3: None,
				colors_0: None,
				joints: Some(vec![[0, 1, 0, 0], [0, 1, 2, 0], [0, 1, 2, 0]]),
				weights: Some(vec![[0.94, 0.06, 0.0, 0.0], [0.56, 0.39, 0.05, 0.0], [0.30, 0.60, 0.10, 0.0]]),
				indices: Some(vec![0, 1, 2]),
				material_index: None,
				morph_targets: Vec::new(),
				morph_target_names: Vec::new(),
				default_morph_weights: Vec::new(),
			}]],
			skins: vec![un_avatar_core::UnaSkin {
				joint_nodes: vec![2, 3, 4],
				inverse_bind_matrices: vec![identity, identity, translation_mat4(0.1, 0.0, 0.0)],
				skeleton_node: None,
			}],
			roots: vec![0],
			..Default::default()
		};
		let groups = vec![un_avatar_core::UnaDynamicsSourceGroup {
			enabled: true,
			source_id: "physbone:test/Cloth_Dyn_L".to_string(),
			category: "cloth".to_string(),
			bone_node_indices: vec![2, 3, 4],
			..Default::default()
		}];
		let node_paths = vec![
			Some("Avatar".to_string()),
			Some("Avatar/Cloth_Panel_Mesh".to_string()),
			Some("Avatar/Chest".to_string()),
			Some("Avatar/Cloth_Static_L".to_string()),
			Some("Avatar/Cloth_Dyn_L".to_string()),
		];

		let samples = dynamics_import_mesh_cloth_assist_samples(&scene, &groups, &node_paths);

		let sample = samples.iter().find(|sample| sample.region == "all").unwrap();
		assert_eq!(sample.candidate_count, 2);
		assert_eq!(sample.existing_dynamic_candidate_count, 1);
		assert_eq!(sample.static_cloth_bridge_candidate_count, 2);
		assert_eq!(sample.seed_candidate_count, 1);
		assert!(sample.static_cloth_weight_sum > 0.0);
		assert!(sample.seeded_assist_weight_sum > 0.0);
		assert_eq!(sample.dynamic_targets[0].path.as_deref(), Some("Avatar/Cloth_Dyn_L"));
	}

	#[test]
	fn dynamics_import_mesh_cloth_assist_neighbor_uses_strongest_dynamic_joint_weight() {
		let profiles = vec![
			DynamicsImportMeshClothAssistVertexProfile::default(),
			DynamicsImportMeshClothAssistVertexProfile {
				dynamic_weight: 0.09,
				strongest_dynamic_joint: Some(2),
				strongest_dynamic_weight: 0.05,
				..Default::default()
			},
		];

		let (neighbor_dynamic_max, neighbor_dynamic_joint) =
			dynamics_import_mesh_cloth_assist_neighbor_dynamic(2, Some(&[0, 1, 1]), &profiles);

		assert!((neighbor_dynamic_max[0] - 0.05).abs() < 0.0001);
		assert_eq!(neighbor_dynamic_joint[0], Some(2));
	}

	#[test]
	fn dynamics_import_mesh_cloth_assist_requires_stronger_neighbor_dynamic_evidence() {
		let identity = test_identity_mat4();
		let scene = un_avatar_core::UnaSceneSnapshot {
			nodes: vec![
				test_scene_node("Avatar", identity, vec![1, 2, 3, 4]),
				un_avatar_core::UnaSceneNode {
					name: Some("Cloth_Panel_Mesh".to_string()),
					transform: identity,
					mesh: Some(0),
					skin: Some(0),
					..test_scene_node("Cloth_Panel_Mesh", identity, Vec::new())
				},
				test_scene_node("Chest", identity, Vec::new()),
				test_scene_node("Cloth_Static_L", translation_mat4(0.0, 0.0, 0.0), Vec::new()),
				test_scene_node("Cloth_Dyn_L", translation_mat4(0.1, 0.0, 0.0), Vec::new()),
			],
			meshes: vec![vec![un_avatar_core::UnaMeshBuffers {
				name: Some("cloth".to_string()),
				vertex_payload_id: None,
				positions: vec![[0.0, 0.0, 0.0], [0.1, 0.0, 0.0], [0.2, 0.0, 0.0]],
				normals: None,
				tangents: None,
				tex_coords_0: None,
				tex_coords_1: None,
				tex_coords_2: None,
				tex_coords_3: None,
				colors_0: None,
				joints: Some(vec![[0, 1, 2, 0], [0, 1, 2, 0], [0, 1, 2, 0]]),
				weights: Some(vec![[0.56, 0.39, 0.05, 0.0]; 3]),
				indices: Some(vec![0, 1, 2]),
				material_index: None,
				morph_targets: Vec::new(),
				morph_target_names: Vec::new(),
				default_morph_weights: Vec::new(),
			}]],
			skins: vec![un_avatar_core::UnaSkin {
				joint_nodes: vec![2, 3, 4],
				inverse_bind_matrices: vec![identity; 3],
				skeleton_node: None,
			}],
			roots: vec![0],
			..Default::default()
		};
		let groups = vec![un_avatar_core::UnaDynamicsSourceGroup {
			enabled: true,
			source_id: "physbone:test/Cloth_Dyn_L".to_string(),
			category: "cloth".to_string(),
			bone_node_indices: vec![2, 3, 4],
			..Default::default()
		}];
		let node_paths = vec![
			Some("Avatar".to_string()),
			Some("Avatar/Cloth_Panel_Mesh".to_string()),
			Some("Avatar/Chest".to_string()),
			Some("Avatar/Cloth_Static_L".to_string()),
			Some("Avatar/Cloth_Dyn_L".to_string()),
		];

		let samples = dynamics_import_mesh_cloth_assist_samples(&scene, &groups, &node_paths);

		assert!(samples.is_empty());
	}

	#[test]
	fn dynamics_import_mesh_cloth_assist_does_not_seed_without_connected_dynamic_evidence() {
		let identity = test_identity_mat4();
		let scene = un_avatar_core::UnaSceneSnapshot {
			nodes: vec![
				test_scene_node("Avatar", identity, vec![1, 2, 3, 4]),
				un_avatar_core::UnaSceneNode {
					name: Some("Cloth_Panel_Mesh".to_string()),
					transform: identity,
					mesh: Some(0),
					skin: Some(0),
					..test_scene_node("Cloth_Panel_Mesh", identity, Vec::new())
				},
				test_scene_node("Chest", identity, Vec::new()),
				test_scene_node("Cloth_Static_L", translation_mat4(0.0, 0.0, 0.0), Vec::new()),
				test_scene_node("Cloth_Dyn_L", translation_mat4(0.1, 0.0, 0.0), Vec::new()),
			],
			meshes: vec![vec![un_avatar_core::UnaMeshBuffers {
				name: Some("cloth".to_string()),
				vertex_payload_id: None,
				positions: vec![[0.0, 0.0, 0.0]],
				normals: None,
				tangents: None,
				tex_coords_0: None,
				tex_coords_1: None,
				tex_coords_2: None,
				tex_coords_3: None,
				colors_0: None,
				joints: Some(vec![[0, 1, 0, 0]]),
				weights: Some(vec![[0.94, 0.06, 0.0, 0.0]]),
				indices: Some(vec![0, 0, 0]),
				material_index: None,
				morph_targets: Vec::new(),
				morph_target_names: Vec::new(),
				default_morph_weights: Vec::new(),
			}]],
			skins: vec![un_avatar_core::UnaSkin {
				joint_nodes: vec![2, 3, 4],
				inverse_bind_matrices: vec![identity, identity, translation_mat4(0.1, 0.0, 0.0)],
				skeleton_node: None,
			}],
			roots: vec![0],
			..Default::default()
		};
		let groups = vec![un_avatar_core::UnaDynamicsSourceGroup {
			enabled: true,
			source_id: "physbone:test/Cloth_Dyn_L".to_string(),
			category: "cloth".to_string(),
			bone_node_indices: vec![2, 3, 4],
			..Default::default()
		}];
		let node_paths = vec![
			Some("Avatar".to_string()),
			Some("Avatar/Cloth_Panel_Mesh".to_string()),
			Some("Avatar/Chest".to_string()),
			Some("Avatar/Cloth_Static_L".to_string()),
			Some("Avatar/Cloth_Dyn_L".to_string()),
		];

		let samples = dynamics_import_mesh_cloth_assist_samples(&scene, &groups, &node_paths);

		assert!(samples.iter().all(|sample| sample.seed_candidate_count == 0));
	}

	#[test]
	fn dynamics_vertex_probe_mesh_cloth_assist_does_not_seed_body_only_vertex() {
		let identity = test_identity_mat4();
		let skin = un_avatar_core::UnaSkin {
			joint_nodes: vec![0, 1],
			inverse_bind_matrices: vec![identity, identity],
			skeleton_node: None,
		};
		let node_paths = vec![Some("Avatar/Chest".to_string()), Some("Avatar/Cloth_Dyn".to_string())];
		let dynamic_nodes = vec![1usize];
		let physics_config = DynamicsPhysicsConfig::default().normalized();
		let config = dynamics_vertex_probe_mesh_cloth_assist_config(&physics_config);
		let mut primitive = un_avatar_core::UnaMeshBuffers {
			name: Some("cloth".to_string()),
			vertex_payload_id: None,
			positions: vec![[0.0, 0.0, 0.0]],
			normals: None,
			tangents: None,
			tex_coords_0: None,
			tex_coords_1: None,
			tex_coords_2: None,
			tex_coords_3: None,
			colors_0: None,
			joints: Some(vec![[0, 0, 0, 0]]),
			weights: Some(vec![[1.0, 0.0, 0.0, 0.0]]),
			indices: Some(vec![0, 0, 0]),
			material_index: None,
			morph_targets: Vec::new(),
			morph_target_names: Vec::new(),
			default_morph_weights: Vec::new(),
		};
		let categories = DynamicsPhysicsConfig::default().normalized().categories;

		let changed = dynamics_vertex_probe_apply_mesh_cloth_assist(
			&mut primitive,
			Some(&skin),
			&node_paths,
			"Avatar/Cloth_Mesh",
			&config,
			&dynamic_nodes,
			&categories,
		);

		assert_eq!(changed, 0);
		assert_eq!(primitive.joints.as_ref().unwrap()[0], [0, 0, 0, 0]);
	}

	#[test]
	fn dynamics_vertex_probe_mesh_cloth_assist_uses_connected_dynamic_evidence() {
		let identity = test_identity_mat4();
		let skin = un_avatar_core::UnaSkin {
			joint_nodes: vec![0, 1, 2],
			inverse_bind_matrices: vec![identity, identity, identity],
			skeleton_node: None,
		};
		let node_paths = vec![
			Some("Avatar/Chest".to_string()),
			Some("Avatar/Cloth_Static_L".to_string()),
			Some("Avatar/Cloth_Dyn_L".to_string()),
		];
		let dynamic_nodes = vec![2usize];
		let physics_config = DynamicsPhysicsConfig::default().normalized();
		let config = dynamics_vertex_probe_mesh_cloth_assist_config(&physics_config);
		let mut primitive = un_avatar_core::UnaMeshBuffers {
			name: Some("cloth".to_string()),
			vertex_payload_id: None,
			positions: vec![[0.0, 0.0, 0.0], [0.1, 0.0, 0.0], [0.2, 0.0, 0.0], [0.3, 0.0, 0.0]],
			normals: None,
			tangents: None,
			tex_coords_0: None,
			tex_coords_1: None,
			tex_coords_2: None,
			tex_coords_3: None,
			colors_0: None,
			joints: Some(vec![[0, 1, 0, 0], [0, 1, 2, 0], [0, 1, 2, 0], [0, 1, 2, 0]]),
			weights: Some(vec![
				[0.78, 0.22, 0.0, 0.0],
				[0.58, 0.418, 0.002, 0.0],
				[0.30, 0.684, 0.016, 0.0],
				[0.16, 0.768, 0.072, 0.0],
			]),
			indices: Some(vec![0, 1, 2, 1, 2, 3]),
			material_index: None,
			morph_targets: Vec::new(),
			morph_target_names: Vec::new(),
			default_morph_weights: Vec::new(),
		};
		let categories = DynamicsPhysicsConfig::default().normalized().categories;

		let changed = dynamics_vertex_probe_apply_mesh_cloth_assist(
			&mut primitive,
			Some(&skin),
			&node_paths,
			"Avatar/Cloth_Mesh",
			&config,
			&dynamic_nodes,
			&categories,
		);

		assert!(changed >= 2);
		let first_dynamic = primitive.joints.as_ref().unwrap()[0]
			.iter()
			.zip(primitive.weights.as_ref().unwrap()[0].iter())
			.filter_map(|(&joint, &weight)| (joint == 2).then_some(weight))
			.sum::<f32>();
		assert!(
			first_dynamic >= config.min_existing_dynamic_weight,
			"probe cloth assist should propagate dynamic evidence over connected topology, got {first_dynamic}"
		);
	}

	#[test]
	fn dynamics_mesh_cloth_assist_source_dynamic_nodes_exclude_non_cloth_accessories() {
		let identity = test_identity_mat4();
		let scene = un_avatar_core::UnaSceneSnapshot {
			nodes: vec![
				test_scene_node("Avatar", identity, vec![1, 2, 3]),
				test_scene_node("Chest", identity, Vec::new()),
				test_scene_node("Cloth_Dyn", identity, Vec::new()),
				test_scene_node("Pendant_Body", identity, Vec::new()),
			],
			roots: vec![0],
			..Default::default()
		};
		let groups = vec![
			un_avatar_core::UnaDynamicsSourceGroup {
				enabled: true,
				source_id: "physbone:test/Cloth_Dyn".to_string(),
				category: "cloth".to_string(),
				bone_node_indices: vec![1, 1, 2],
				..Default::default()
			},
			un_avatar_core::UnaDynamicsSourceGroup {
				enabled: true,
				source_id: "physbone:test/Pendant_Body".to_string(),
				category: "accessory".to_string(),
				bone_node_indices: vec![1, 1, 3],
				..Default::default()
			},
		];

		let dynamic_nodes = dynamics_mesh_cloth_assist_source_dynamic_nodes(&scene, &groups);

		assert!(dynamic_nodes.contains(&2));
		assert!(!dynamic_nodes.contains(&3));
	}

	#[test]
	fn dynamics_mesh_cloth_assist_runtime_dynamic_nodes_use_profile_categories() {
		let identity = test_identity_mat4();
		let scene = un_avatar_core::UnaSceneSnapshot {
			nodes: vec![
				test_scene_node("Avatar", identity, vec![1, 2, 3]),
				test_scene_node("Anchor", identity, Vec::new()),
				test_scene_node("PanelDyn", identity, Vec::new()),
				test_scene_node("AccessoryDyn", identity, Vec::new()),
			],
			roots: vec![0],
			..Default::default()
		};
		let settings = un_avatar_core::UnaDynamicsSettings {
			groups: vec![
				un_avatar_core::UnaDynamicsSourceGroup {
					enabled: true,
					source_id: "physbone:Fixture/PanelRig".to_string(),
					bone_node_indices: vec![1, 1, 2],
					..Default::default()
				},
				un_avatar_core::UnaDynamicsSourceGroup {
					enabled: true,
					source_id: "physbone:Fixture/AccessoryRig".to_string(),
					category: "accessory".to_string(),
					bone_node_indices: vec![1, 1, 3],
					..Default::default()
				},
			],
			..Default::default()
		};
		let categories = vec![un_avatar_skeleton::DynamicsCategoryDefinition {
			id: "cloth".to_string(),
			matches: vec!["panel_rig".to_string()],
			..Default::default()
		}];

		let dynamic_nodes = dynamics_mesh_cloth_assist_runtime_dynamic_nodes(&scene, settings.runtime_dynamics(), &categories);

		assert!(dynamic_nodes.contains(&2));
		assert!(!dynamic_nodes.contains(&3));
	}

	#[test]
	fn dynamics_source_collider_shape_known_is_exact() {
		assert!(dynamics_source_collider_shape_known(&serde_json::json!({"shape": "sphere"})));
		assert!(dynamics_source_collider_shape_known(&serde_json::json!({"shape": "local_sphere"})));
		assert!(dynamics_source_collider_shape_known(&serde_json::json!({"shape": "capsule"})));
		assert!(dynamics_source_collider_shape_known(&serde_json::json!({"shape": "local_capsule"})));
		assert!(dynamics_source_collider_shape_known(&serde_json::json!({"shape": "plane"})));
		assert!(dynamics_source_collider_shape_known(&serde_json::json!({"shape": "local_plane"})));
		assert!(dynamics_source_collider_shape_known(&serde_json::json!({"shapeType": 0})));
		assert!(dynamics_source_collider_shape_known(&serde_json::json!({"shape_type": "1"})));
		assert!(!dynamics_source_collider_shape_known(&serde_json::json!({"shape": "not_a_sphere"})));
		assert!(!dynamics_source_collider_shape_known(&serde_json::json!({"shape": "capsule_hint"})));
	}

	#[test]
	fn vertex_probe_node_filter_prefers_visible_matching_node() {
		let node_paths = vec![
			Some("Avatar/Inactive/Cloth_Mesh".to_string()),
			Some("Avatar/Active/Cloth_Mesh".to_string()),
		];
		let effective_visibility = vec![false, true];

		let selected = select_node_path_containing(&node_paths, &effective_visibility, "cloth mesh").unwrap();

		assert_eq!(selected.0, 1);
		assert_eq!(selected.1, "Avatar/Active/Cloth_Mesh");
	}

	#[test]
	fn vertex_probe_node_filter_uses_token_matching() {
		let node_paths = vec![
			Some("Avatar/Accessories/Earring_L".to_string()),
			Some("Avatar/Hair/Ear_01_L".to_string()),
			Some("Avatar/Outer/LongCoatPanel".to_string()),
		];
		let effective_visibility = vec![true, true, true];

		let selected = select_node_path_containing(&node_paths, &effective_visibility, "ear").unwrap();
		let compact_selected = select_node_path_containing(&node_paths, &effective_visibility, "long coat").unwrap();

		assert_eq!(selected.0, 1);
		assert_eq!(selected.1, "Avatar/Hair/Ear_01_L");
		assert_eq!(compact_selected.0, 2);
		assert_eq!(compact_selected.1, "Avatar/Outer/LongCoatPanel");
	}

	#[test]
	fn dynamics_vertex_probe_dynamic_weight_score_uses_authored_vertex_weights() {
		let mesh = vec![un_avatar_core::UnaMeshBuffers {
			name: Some("ClothPanel".to_string()),
			vertex_payload_id: None,
			positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
			normals: None,
			tangents: None,
			tex_coords_0: None,
			tex_coords_1: None,
			tex_coords_2: None,
			tex_coords_3: None,
			colors_0: None,
			joints: Some(vec![[0, 1, 0, 0], [0, 1, 0, 0], [0, 0, 0, 0]]),
			weights: Some(vec![[0.9, 0.1, 0.0, 0.0], [1.0, 0.0005, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]]),
			indices: Some(vec![0, 1, 2]),
			material_index: None,
			morph_targets: Vec::new(),
			morph_target_names: Vec::new(),
			default_morph_weights: Vec::new(),
		}];
		let skin = un_avatar_core::UnaSkin {
			joint_nodes: vec![10, 20],
			inverse_bind_matrices: vec![test_identity_mat4(), test_identity_mat4()],
			skeleton_node: None,
		};
		let dynamic_nodes = vec![20usize];

		let (vertex_count, weight_sum) = dynamics_vertex_probe_dynamic_weight_score(&mesh, &skin, &dynamic_nodes);

		assert_eq!(vertex_count, 1);
		assert!((weight_sum - 0.1).abs() < 0.0001);
	}

	#[test]
	fn dynamics_vertex_probe_probe_projection_sources_follow_weighted_dynamic_sources() {
		let primitive = un_avatar_core::UnaMeshBuffers {
			name: Some("ClothPanel".to_string()),
			vertex_payload_id: None,
			positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
			normals: None,
			tangents: None,
			tex_coords_0: None,
			tex_coords_1: None,
			tex_coords_2: None,
			tex_coords_3: None,
			colors_0: None,
			joints: Some(vec![[0, 1, 0, 0], [0, 2, 0, 0]]),
			weights: Some(vec![[0.8, 0.2, 0.0, 0.0], [0.9, 0.1, 0.0, 0.0]]),
			indices: Some(vec![0, 1]),
			material_index: None,
			morph_targets: Vec::new(),
			morph_target_names: Vec::new(),
			default_morph_weights: Vec::new(),
		};
		let skin = un_avatar_core::UnaSkin {
			joint_nodes: vec![10, 20, 30],
			inverse_bind_matrices: vec![test_identity_mat4(), test_identity_mat4(), test_identity_mat4()],
			skeleton_node: None,
		};
		let settings = un_avatar_core::UnaDynamicsSettings {
			groups: vec![
				un_avatar_core::UnaDynamicsSourceGroup {
					enabled: true,
					source_id: "physbone:Fixture/WeightedCloth".to_string(),
					bone_node_indices: vec![10, 10, 20],
					..Default::default()
				},
				un_avatar_core::UnaDynamicsSourceGroup {
					enabled: true,
					source_id: "physbone:Fixture/OtherCloth".to_string(),
					bone_node_indices: vec![10, 10, 30],
					..Default::default()
				},
			],
			..Default::default()
		};
		let dynamic_nodes = vec![20usize];
		let source_weights =
			dynamics_vertex_probe_dynamic_source_weight_sums(settings.runtime_dynamics(), Some(&skin), &primitive, &dynamic_nodes);
		let all_projection_counts = BTreeMap::from([
			("physbone:Fixture/WeightedCloth".to_string(), 7),
			("physbone:Fixture/OtherCloth".to_string(), 11),
		]);
		let all_source_path_counts = BTreeMap::from([
			(
				"physbone:Fixture/WeightedCloth".to_string(),
				BTreeMap::from([("Body/Torso".to_string(), 7)]),
			),
			(
				"physbone:Fixture/OtherCloth".to_string(),
				BTreeMap::from([("Body/Other".to_string(), 11)]),
			),
		]);

		let probe_projection_counts = probe_collision_projection_source_counts(&all_projection_counts, &source_weights);
		let probe_path_counts = probe_collision_projection_collider_path_counts(&all_source_path_counts, &source_weights);

		assert_eq!(source_weights.len(), 1);
		assert!((source_weights["physbone:Fixture/WeightedCloth"] - 0.2).abs() < 0.0001);
		assert_eq!(probe_projection_counts.len(), 1);
		assert_eq!(probe_projection_counts["physbone:Fixture/WeightedCloth"], 7);
		assert!(!probe_projection_counts.contains_key("physbone:Fixture/OtherCloth"));
		assert_eq!(probe_path_counts.len(), 1);
		assert_eq!(probe_path_counts["Body/Torso"], 7);
		assert!(!probe_path_counts.contains_key("Body/Other"));
	}

	#[test]
	fn dynamics_motion_trace_audit_reports_recovery_and_residual_motion() {
		let path = std::env::temp_dir().join(format!("un-avatar-dynamics-motion-trace-{}.gltf", std::process::id()));
		let json = r#"{
			"asset": {"version": "2.0"},
			"scene": 0,
			"scenes": [{"nodes": [0]}],
			"nodes": [
				{"name": "Avatar", "children": [1], "translation": [0.0, 0.0, 0.0]},
				{"name": "HairRoot", "children": [2], "translation": [0.0, 0.0, 0.0]},
				{"name": "HairMid", "children": [3], "translation": [0.0, 0.5, 0.0]},
				{"name": "HairTip", "translation": [0.0, 0.5, 0.0]}
			],
			"extensions": {
				"UN_avatar": {
					"specVersion": "0.1-preview",
					"nodes": [
						{"nodeId": "node_root", "path": "HairRoot"},
						{"nodeId": "node_mid", "path": "HairRoot/HairMid"},
						{"nodeId": "node_tip", "path": "HairRoot/HairMid/HairTip"}
					],
					"dynamics": [{
						"id": "audit_hair",
						"source": "vrc_physbone",
						"roots": [{"nodeId": "node_root", "path": "HairRoot"}],
						"drag": 0.2,
						"gravity": [0.0, 0.0, 0.0],
						"radius": 0.01,
						"sourceParams": {
							"integrationType": "Advanced",
							"pull": 0.18,
							"pullCurve": {"keys": []},
							"spring": 0.6,
							"springCurve": {"keys": []},
							"momentum": 0.6,
							"momentumCurve": {"keys": []},
							"stiffness": 0.1,
							"stiffnessCurve": {"keys": []},
							"gravityFalloff": 0.0,
							"gravityFalloffCurve": {"keys": []},
							"immobile": 0.0,
							"immobileCurve": {"keys": []},
							"immobileType": 0,
							"limitRotation": [0.0, 0.0, 0.0]
						}
					}, {
						"id": "audit_disabled",
						"source": "vrc_physbone",
						"roots": [{"nodeId": "node_root", "path": "HairRoot"}],
						"enabled": false,
						"drag": 0.2,
						"gravity": [0.0, 0.0, 0.0],
						"radius": 0.01,
						"sourceParams": {
							"integrationType": "Advanced",
							"pull": 0.18,
							"pullCurve": {"keys": []},
							"spring": 0.6,
							"springCurve": {"keys": []},
							"momentum": 0.6,
							"momentumCurve": {"keys": []},
							"stiffness": 0.1,
							"stiffnessCurve": {"keys": []},
							"gravityFalloff": 0.0,
							"gravityFalloffCurve": {"keys": []},
							"immobile": 0.0,
							"immobileCurve": {"keys": []},
							"immobileType": 0,
							"limitRotation": [0.0, 0.0, 0.0]
						}
					}]
				}
			}
		}"#;
		fs::write(&path, json).unwrap();
		let report = dynamics_motion_trace_report(&[], &path, None, None, 12, Some(18), "authored").unwrap();
		let rest_high_report = dynamics_motion_trace_report(&[], &path, None, None, 12, Some(18), "rest-high").unwrap();
		let _ = fs::remove_file(&path);

		assert_eq!(report.frame_count, 12);
		assert_eq!(report.recovery_frame_count, 18);
		assert_eq!(report.tuning, "authored");
		assert_eq!(report.group_count, 1);
		assert_eq!(report.joint_count, 3);
		assert!(report.missing_motion_evidence.is_empty());
		let category = report.categories.iter().find(|category| category.category == "hair").unwrap();
		assert_eq!(category.visual_target_group_count, 0);
		assert_eq!(category.nonvisual_group_count, 1);
		assert_eq!(category.visible_skinned_joint_count, 0);
		assert_eq!(category.visible_mesh_subtree_node_count, 0);
		assert!(category.max_lag > 0.0);
		assert!(category.recovery_final_lag.is_finite());
		assert!(category.recovery_ratio.is_finite());
		assert!(category.settled_recovery_lag.is_finite());
		assert!(category.stable_offset.is_finite());
		assert!(category.stable_offset_ratio.is_finite());
		assert!(!category.recovery_state.is_empty());
		assert!(category.settled_recovery_ratio.is_finite());
		assert!(category.residual_motion.is_finite());
		assert!(category.residual_motion_chain_ratio.is_finite());
		assert!(category.recovery_half_life_frames.map_or(true, f32::is_finite));
		assert!(category.average_rest_response > 0.0);
		assert!(category.average_parent_motion_follow >= 0.0);
		assert_eq!(report.groups.len(), 1);
		assert_eq!(report.groups[0].source_id, "audit_hair");
		assert_eq!(report.groups[0].category, "hair");
		assert!(!report.groups[0].visual_target);
		assert_eq!(report.groups[0].skinned_joint_count, 0);
		assert_eq!(report.groups[0].mesh_subtree_node_count, 0);
		assert!(report.groups[0].max_lag > 0.0);
		assert!(report.groups[0].settled_recovery_lag.is_finite());
		assert!(report.groups[0].stable_offset.is_finite());
		assert!(report.groups[0].stable_offset_ratio.is_finite());
		assert!(report.groups[0].residual_motion_chain_ratio.is_finite());
		assert!(!report.groups[0].recovery_state.is_empty());
		assert!(report.groups[0].recovery_half_life_frames.map_or(true, f32::is_finite));
		assert!(report.groups[0].average_rest_response > 0.0);
		assert!(report.groups[0].average_parent_motion_follow >= 0.0);
		assert_eq!(rest_high_report.tuning, "rest-high");
		assert!(rest_high_report.groups[0].average_rest_response > report.groups[0].average_rest_response);
	}

	#[test]
	fn dynamics_motion_trace_evidence_flags_nonfinite_values() {
		let category = DynamicsMotionTraceCategorySummary {
			category: "cloth".to_string(),
			group_count: 1,
			joint_count: 1,
			visual_target_group_count: 1,
			nonvisual_group_count: 0,
			visible_skinned_joint_count: 1,
			visible_mesh_subtree_node_count: 0,
			average_chain_rest_length: 1.0,
			max_lag: f32::NAN,
			max_lag_chain_ratio: 0.0,
			average_lag: 0.0,
			final_lag: 0.0,
			final_lag_chain_ratio: 0.0,
			recovery_final_lag: 0.0,
			recovery_ratio: 0.0,
			initial_stable_offset: 0.0,
			settled_recovery_lag: 0.0,
			stable_offset: 0.0,
			stable_offset_chain_ratio: 0.0,
			stable_offset_ratio: 0.0,
			recovery_state: "settled".to_string(),
			settled_recovery_ratio: 0.0,
			residual_motion: 0.0,
			residual_motion_chain_ratio: 0.0,
			recovery_half_life_frames: Some(f32::INFINITY),
			average_rest_response: 0.1,
			average_shape_preservation: 0.1,
			average_bounce_response: 0.1,
			average_parent_motion_follow: 0.1,
			average_orientation_follow: 0.1,
			average_max_stretch_response: 0.0,
			average_stretch_motion_response: 0.0,
		};
		let group = DynamicsMotionTraceGroupSummary {
			source_id: "physbone:test".to_string(),
			category: "cloth".to_string(),
			joint_count: 1,
			visual_target: true,
			skinned_joint_count: 1,
			mesh_subtree_node_count: 0,
			interaction_metadata_only: false,
			chain_rest_length: 1.0,
			max_lag: 0.0,
			max_lag_chain_ratio: 0.0,
			average_lag: 0.0,
			final_lag: 0.0,
			final_lag_chain_ratio: 0.0,
			recovery_final_lag: 0.0,
			recovery_ratio: 0.0,
			initial_stable_offset: 0.0,
			settled_recovery_lag: 0.0,
			stable_offset: 0.0,
			stable_offset_chain_ratio: 0.0,
			stable_offset_ratio: 0.0,
			recovery_state: "settled".to_string(),
			settled_recovery_ratio: 0.0,
			residual_motion: f32::INFINITY,
			residual_motion_chain_ratio: 0.0,
			recovery_half_life_frames: None,
			average_rest_response: 0.1,
			average_shape_preservation: 0.1,
			average_bounce_response: 0.1,
			average_parent_motion_follow: 0.1,
			average_orientation_follow: 0.1,
			average_max_stretch_response: 0.0,
			average_stretch_motion_response: 0.0,
		};
		let mut evidence = Vec::new();

		collect_motion_trace_numeric_evidence(&[category], &[group], &mut evidence);

		assert!(evidence.iter().any(|item| item.contains("motion category cloth max_lag")));
		assert!(evidence
			.iter()
			.any(|item| item.contains("motion category cloth recovery_half_life_frames")));
		assert!(evidence
			.iter()
			.any(|item| item.contains("motion group physbone:test residual_motion")));
	}

	#[test]
	fn motion_trace_sort_prefers_finite_values_before_nonfinite_values() {
		let mut values = [f32::NAN, 0.4, f32::INFINITY, 0.9, 0.1];

		values.sort_by(|left, right| motion_trace_desc_finite_cmp(*left, *right));

		assert_eq!(values[0], 0.9);
		assert_eq!(values[1], 0.4);
		assert_eq!(values[2], 0.1);
		assert!(!values[3].is_finite());
		assert!(!values[4].is_finite());
	}

	#[test]
	fn dynamics_motion_trace_findings_flag_large_stretch_and_unsettled_recovery() {
		let group = DynamicsMotionTraceGroupSummary {
			source_id: "physbone:hand".to_string(),
			category: "other".to_string(),
			joint_count: 1,
			visual_target: true,
			skinned_joint_count: 1,
			mesh_subtree_node_count: 0,
			interaction_metadata_only: false,
			chain_rest_length: 0.05,
			max_lag: 0.65,
			max_lag_chain_ratio: 13.0,
			average_lag: 0.3,
			final_lag: 0.65,
			final_lag_chain_ratio: 13.0,
			recovery_final_lag: 0.2,
			recovery_ratio: 0.6,
			initial_stable_offset: 0.65,
			settled_recovery_lag: 0.18,
			stable_offset: 0.18,
			stable_offset_chain_ratio: 3.6,
			stable_offset_ratio: 0.27,
			recovery_state: "moving".to_string(),
			settled_recovery_ratio: 0.7,
			residual_motion: 0.01,
			residual_motion_chain_ratio: 0.2,
			recovery_half_life_frames: Some(14.0),
			average_rest_response: 0.01,
			average_shape_preservation: 0.0,
			average_bounce_response: 0.2,
			average_parent_motion_follow: 0.4,
			average_orientation_follow: 0.0,
			average_max_stretch_response: 100.0,
			average_stretch_motion_response: 0.5,
		};

		let findings = collect_motion_trace_finding_details(&[], &[group]);
		let counts = motion_trace_finding_kind_counts(&findings);

		assert!(findings.iter().any(|item| item.kind == "large_stretch"));
		assert!(findings.iter().any(|item| item.kind == "high_chain_lag"));
		assert!(findings.iter().any(|item| item.kind == "moving_after_recovery"));
		assert_eq!(counts.get("large_stretch"), Some(&1));
		assert_eq!(counts.get("high_chain_lag"), Some(&1));
		assert_eq!(counts.get("moving_after_recovery"), Some(&1));
		let stretch = findings.iter().find(|item| item.kind == "large_stretch").unwrap();
		let hint = stretch.response_override_hint.as_ref().unwrap();
		assert_eq!(hint.source_id, "physbone:hand");
		assert_eq!(hint.stretch_range_scale, Some(0.25));
		assert_eq!(hint.stretch_motion, Some(0.1));
	}

	#[test]
	fn dynamics_motion_trace_findings_classify_nonvisual_control_motion_without_override_hint() {
		let group = DynamicsMotionTraceGroupSummary {
			source_id: "physbone:control".to_string(),
			category: "other".to_string(),
			joint_count: 1,
			visual_target: false,
			skinned_joint_count: 0,
			mesh_subtree_node_count: 0,
			interaction_metadata_only: true,
			chain_rest_length: 0.05,
			max_lag: 0.65,
			max_lag_chain_ratio: 13.0,
			average_lag: 0.3,
			final_lag: 0.65,
			final_lag_chain_ratio: 13.0,
			recovery_final_lag: 0.0,
			recovery_ratio: 1.0,
			initial_stable_offset: 0.65,
			settled_recovery_lag: 0.0,
			stable_offset: 0.0,
			stable_offset_chain_ratio: 0.0,
			stable_offset_ratio: 0.0,
			recovery_state: "settled".to_string(),
			settled_recovery_ratio: 1.0,
			residual_motion: 0.0,
			residual_motion_chain_ratio: 0.0,
			recovery_half_life_frames: Some(2.0),
			average_rest_response: 0.01,
			average_shape_preservation: 0.0,
			average_bounce_response: 0.2,
			average_parent_motion_follow: 0.4,
			average_orientation_follow: 0.0,
			average_max_stretch_response: 100.0,
			average_stretch_motion_response: 0.5,
		};

		let findings = collect_motion_trace_finding_details(&[], &[group]);

		assert_eq!(findings.len(), 1);
		assert_eq!(findings[0].kind, "nonvisual_control_motion");
		assert!(findings[0].response_override_hint.is_none());
	}

	#[test]
	fn dynamics_motion_trace_category_findings_keep_visibility_classification() {
		let category = DynamicsMotionTraceCategorySummary {
			category: "cloth".to_string(),
			group_count: 2,
			joint_count: 4,
			visual_target_group_count: 1,
			nonvisual_group_count: 1,
			visible_skinned_joint_count: 3,
			visible_mesh_subtree_node_count: 1,
			average_chain_rest_length: 0.5,
			max_lag: 0.2,
			max_lag_chain_ratio: 0.4,
			average_lag: 0.1,
			final_lag: 0.1,
			final_lag_chain_ratio: 0.2,
			recovery_final_lag: 0.1,
			recovery_ratio: 0.5,
			initial_stable_offset: 0.2,
			settled_recovery_lag: 0.15,
			stable_offset: 0.15,
			stable_offset_chain_ratio: 0.3,
			stable_offset_ratio: 0.75,
			recovery_state: "settled_offset".to_string(),
			settled_recovery_ratio: 0.5,
			residual_motion: 0.0,
			residual_motion_chain_ratio: 0.0,
			recovery_half_life_frames: Some(12.0),
			average_rest_response: 0.2,
			average_shape_preservation: 0.5,
			average_bounce_response: 0.1,
			average_parent_motion_follow: 0.4,
			average_orientation_follow: 0.2,
			average_max_stretch_response: 0.0,
			average_stretch_motion_response: 0.0,
		};

		let findings = collect_motion_trace_finding_details(&[category], &[]);

		assert_eq!(findings.len(), 1);
		assert_eq!(findings[0].kind, "category_recovery_state");
		assert_eq!(findings[0].visual_target, Some(true));
		assert_eq!(findings[0].skinned_joint_count, Some(3));
		assert_eq!(findings[0].mesh_subtree_node_count, Some(1));
	}

	#[test]
	fn bounded_unique_string_samples_keep_first_unique_values() {
		let mut samples = Vec::new();

		push_bounded_unique_string(&mut samples, "a".to_string(), 2);
		push_bounded_unique_string(&mut samples, "a".to_string(), 2);
		push_bounded_unique_string(&mut samples, "b".to_string(), 2);
		push_bounded_unique_string(&mut samples, "c".to_string(), 2);

		assert_eq!(samples, vec!["a".to_string(), "b".to_string()]);
	}

	#[test]
	fn motion_trace_visual_target_counts_use_only_visible_mesh_skins() {
		let identity = test_identity_mat4();
		let mut hidden_parent = test_scene_node("Hidden", identity, vec![4]);
		hidden_parent.visible = false;
		let mut visible_mesh = test_scene_node("VisibleMesh", identity, Vec::new());
		visible_mesh.mesh = Some(0);
		visible_mesh.skin = Some(0);
		let mut hidden_mesh = test_scene_node("HiddenMesh", identity, Vec::new());
		hidden_mesh.mesh = Some(1);
		hidden_mesh.skin = Some(1);
		let scene = un_avatar_core::UnaSceneSnapshot {
			nodes: vec![
				test_scene_node("Root", identity, vec![1, 3, 5]),
				test_scene_node("VisibleJoint", identity, vec![2]),
				visible_mesh,
				hidden_parent,
				hidden_mesh,
				test_scene_node("UnusedSkinJoint", identity, Vec::new()),
			],
			roots: vec![0],
			meshes: vec![
				vec![un_avatar_core::UnaMeshBuffers {
					name: None,
					vertex_payload_id: None,
					positions: vec![[0.0, 0.0, 0.0]],
					normals: None,
					tangents: None,
					tex_coords_0: None,
					tex_coords_1: None,
					tex_coords_2: None,
					tex_coords_3: None,
					colors_0: None,
					joints: Some(vec![[0, 1, 0, 0]]),
					weights: Some(vec![[1.0, 0.0, 0.0, 0.0]]),
					indices: None,
					material_index: None,
					morph_targets: Vec::new(),
					morph_target_names: Vec::new(),
					default_morph_weights: Vec::new(),
				}],
				vec![un_avatar_core::UnaMeshBuffers {
					name: None,
					vertex_payload_id: None,
					positions: vec![[0.0, 0.0, 0.0]],
					normals: None,
					tangents: None,
					tex_coords_0: None,
					tex_coords_1: None,
					tex_coords_2: None,
					tex_coords_3: None,
					colors_0: None,
					joints: Some(vec![[0, 0, 0, 0]]),
					weights: Some(vec![[1.0, 0.0, 0.0, 0.0]]),
					indices: None,
					material_index: None,
					morph_targets: Vec::new(),
					morph_target_names: Vec::new(),
					default_morph_weights: Vec::new(),
				}],
			],
			skins: vec![
				un_avatar_core::UnaSkin {
					joint_nodes: vec![1, 5],
					..Default::default()
				},
				un_avatar_core::UnaSkin {
					joint_nodes: vec![3],
					..Default::default()
				},
			],
			..Default::default()
		};

		let context = DynamicsVisualTargetContext::for_scene(&scene);

		assert_eq!(context.group_counts(&[1]), (1, 1));
		assert_eq!(context.group_counts(&[1, 1]), (1, 1));
		assert_eq!(context.group_counts(&[2]), (0, 1));
		assert_eq!(context.group_counts(&[5]), (0, 0));
		assert_eq!(context.group_counts(&[3]), (0, 0));
	}

	#[test]
	fn dynamics_motion_trace_tuning_includes_stretch_terms() {
		let (_, stretch_off) = dynamics_motion_trace_tuning_config("stretch-off").unwrap();
		let (_, stretch_low) = dynamics_motion_trace_tuning_config("stretch-low").unwrap();
		let (_, stretch_high) = dynamics_motion_trace_tuning_config("stretch-high").unwrap();

		assert_eq!(stretch_off.overrides[0].params.stretch_range_scale, Some(0.0));
		assert_eq!(stretch_off.overrides[0].params.stretch_motion, Some(0.0));
		assert_eq!(stretch_low.overrides[0].params.stretch_range_scale, Some(0.25));
		assert_eq!(stretch_low.overrides[0].params.stretch_motion, Some(0.1));
		assert_eq!(stretch_high.overrides[0].params.stretch_range_scale, Some(1.5));
		assert_eq!(stretch_high.overrides[0].params.stretch_motion, Some(0.85));
	}

	#[test]
	fn dynamics_response_audit_detects_soft_and_firm_override_effects() {
		let doc = UnaDocument {
			scene: Some(un_avatar_core::UnaSceneSnapshot {
				nodes: vec![
					test_scene_node("Root", test_identity_mat4(), vec![1]),
					test_scene_node("Tip", translation_mat4(0.0, 1.0, 0.0), Vec::new()),
				],
				roots: vec![0],
				..Default::default()
			}),
			spring_bones: Some(un_avatar_core::UnaSpringBoneSettings {
				groups: vec![un_avatar_core::UnaSpringBoneGroup {
					source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
					enabled: true,
					source_id: "physbone:test-ear".to_string(),
					comment: "test ear".to_string(),
					category: "ears".to_string(),
					stiffness: 0.2,
					pull: 0.2,
					spring: 0.5,
					drag_force: 0.4,
					limit: Some(un_avatar_core::UnaDynamicsLimit {
						max_stretch: 0.4,
						max_squish: 0.1,
						stretch_motion: Some(0.5),
						..Default::default()
					}),
					bone_node_indices: vec![0, 1],
					..Default::default()
				}],
				..Default::default()
			}),
			..Default::default()
		};
		let soft = dynamics_response_audit_mode("soft", &doc, dynamics_response_audit_config(dynamics_soft_audit_params()))
			.expect("soft response");
		let firm = dynamics_response_audit_mode("firm", &doc, dynamics_response_audit_config(dynamics_firm_audit_params()))
			.expect("firm response");

		assert_eq!(soft.group_count, 1);
		assert_eq!(firm.group_count, 1);
		assert_eq!(soft.categories.len(), 1);
		assert_eq!(soft.categories[0].category, "ears");
		assert!(soft.average_rest_response < firm.average_rest_response);
		assert!(soft.average_shape_preservation < firm.average_shape_preservation);
		assert!(soft.average_bounce_response > firm.average_bounce_response);
		assert!(soft.average_max_stretch_response < firm.average_max_stretch_response);
		assert!(soft.average_stretch_motion_response < firm.average_stretch_motion_response);
		assert!(soft.average_parent_motion_follow < firm.average_parent_motion_follow);
		assert!(soft.average_damping_half_life_ms > firm.average_damping_half_life_ms);
	}

	#[test]
	fn io_registry_for_cli_empty_is_vrm_and_gltf() {
		let reg = io_registry_for_cli(&[]).unwrap();
		assert_eq!(reg.importer_descriptors().len(), 2);
		assert_eq!(reg.exporter_descriptors().len(), 0);
	}

	#[test]
	fn diagnose_report_summarizes_materials_and_vrm_hints() {
		let doc = UnaDocument {
			scene: Some(un_avatar_core::UnaSceneSnapshot {
				materials: vec![
					un_avatar_core::UnaMaterialPbr {
						name: Some("Eye_Iris".into()),
						shading: UnaShadingModel::LilToonLike,
						alpha_mode: UnaAlphaMode::Mask,
						..Default::default()
					},
					un_avatar_core::UnaMaterialPbr {
						name: Some("Body".into()),
						shading: UnaShadingModel::LilToonLike,
						alpha_mode: UnaAlphaMode::Opaque,
						..Default::default()
					},
				],
				..Default::default()
			}),
			vrm: Some(un_avatar_core::UnaVrmExtension {
				spec_version: "1.0".into(),
				meta: serde_json::Value::Null,
				humanoid_bones: BTreeMap::new(),
				mtoon_materials_v0: Vec::new(),
				mtoon_material_indices_v1: vec![0, 1],
				source: serde_json::Value::Null,
			}),
			..Default::default()
		};

		let report = build_diagnose_report(
			Path::new("avatar.vrm"),
			"io.un-avatar.vrm".into(),
			None,
			DiagnoseTimingSummary {
				import_ms: 0,
				wardrobe_apply_ms: 0,
				wardrobe_probe_ms: 0,
				report_build_ms: 0,
			},
			ImportReport::default(),
			doc,
			Vec::new(),
		);

		assert_eq!(report.scene.material_count, 2);
		assert_eq!(report.scene.node_constraint_count, 0);
		assert_eq!(report.scene.shading_counts.get("LilToonLike"), Some(&2));
		assert!(report.scene.visible_shading_counts.is_empty());
		assert!(report.scene.visible_alpha_counts.is_empty());
		assert!(report.scene.visible_material_indices.is_empty());
		assert_eq!(report.scene.eye_like_material_indices, vec![0]);
		assert_eq!(report.vrm.as_ref().unwrap().mtoon_material_indices_v1, vec![0, 1]);
		assert!(!report.warnings.iter().any(|w| w.contains("eye-like material[0]")));
	}

	#[test]
	fn diagnose_report_summarizes_unavatar_asset_groups() {
		let mut doc = UnaDocument {
			scene: Some(un_avatar_core::UnaSceneSnapshot {
				asset_group_ownership: vec![un_avatar_core::UnaSceneAssetGroupOwnership {
					group_id: "outfit:jacket".to_string(),
					mesh_primitives: vec![un_avatar_core::UnaMeshPrimitiveKey {
						mesh_index: 0,
						primitive_index: 1,
					}],
					materials: vec![2],
					images: vec![3],
					dynamics_source_ids: vec!["physbone:jacket".to_string()],
				}],
				..Default::default()
			}),
			runtime_actions: Some(un_avatar_core::UnaRuntimeActionSet {
				actions: vec![
					un_avatar_core::UnaRuntimeAction {
						id: "ma:hat".to_string(),
						label: "Hat Toggle".to_string(),
						triggers: Vec::new(),
						conditions: vec![un_avatar_core::UnaRuntimeActionCondition {
							parameter_name: Some("Hat".to_string()),
							parameter_value: Some(1.0),
							..Default::default()
						}],
						effects: vec![
							UnaRuntimeActionEffect::NodeVisibility {
								target: un_avatar_core::UnaRuntimeNodeTarget {
									path: Some("Root/Hat".to_string()),
									..Default::default()
								},
								visible: true,
							},
							UnaRuntimeActionEffect::WardrobeSet { set_id: "hat".to_string() },
						],
					},
					un_avatar_core::UnaRuntimeAction {
						id: "ma:glasses".to_string(),
						label: "Glasses Toggle".to_string(),
						triggers: vec![UnaRuntimeActionTrigger::ParameterValue {
							name: "Glasses".to_string(),
							value: 1.0,
						}],
						conditions: Vec::new(),
						effects: vec![UnaRuntimeActionEffect::WardrobeSet {
							set_id: "glasses".to_string(),
						}],
					},
				],
			}),
			unavatar: Some(un_avatar_core::UnaUnavatarExtension {
				spec_version: "0.1-preview".to_string(),
				source: serde_json::json!({
					"modularAvatar": {
						"components": [{
							"shortType": "ModularAvatarRemoveVertexColor",
							"enabled": true
						}, {
							"shortType": "ModularAvatarMaterialSwap",
							"enabled": true
						}, {
							"shortType": "ModularAvatarMenuItem",
							"enabled": true,
							"id": "menu-hat",
							"hierarchyPath": "Root/HatMenu",
							"siblingIndex": 4,
							"target": {"path": "Root/HatMenu"},
							"fields": {
								"label": "Hat",
								"MenuSource": "Children",
								"menuSource_otherObjectChildren": {"path": "Root/HatMenu/Children"},
								"Control": {
									"Type": "RadialPuppet",
									"Parameter": {"Name": "Hat"},
									"Value": 1,
									"subParameters": [{"name": "HatAngle"}]
								}
							}
						}, {
							"shortType": "ModularAvatarParameters",
							"enabled": true,
							"fields": {
								"parameters": [{
									"nameOrPrefix": "Hat",
									"remapTo": "Hat_Remapped",
									"syncType": "Bool",
									"localOnly": false,
									"defaultValue": 1,
									"saved": true,
									"hasExplicitDefaultValue": true
								}, {
									"nameOrPrefix": "Local/",
									"isPrefix": true,
									"internalParameter": true,
									"syncType": "NotSynced",
									"localOnly": false,
									"defaultValue": 0.25
								}]
							}
						}, {
							"shortType": "ModularAvatarMenuGroup",
							"enabled": true,
							"id": "group-accessories",
							"hierarchyPath": "Root/Accessories",
							"siblingIndex": 2,
							"targetObject": {"path": "Root/Accessories"}
						}, {
							"shortType": "ModularAvatarMenuItem",
							"enabled": true,
							"id": "menu-glasses",
							"hierarchyPath": "Root/Accessories/Glasses",
							"siblingIndex": 0,
							"target": {"path": "Root/Accessories/Glasses"},
							"fields": {
								"label": "Glasses",
								"Control": {
									"Type": "Toggle",
									"Parameter": {"Name": "Glasses"},
									"Value": 1
								}
							}
						}, {
							"shortType": "ModularAvatarMenuInstaller",
							"enabled": true,
							"id": "installer-root",
							"hierarchyPath": "Root/MenuInstaller",
							"siblingIndex": 1,
							"menuToAppend": {
								"assetPath": "Assets/Menus/Root.asset",
								"controlCount": 2,
								"controls": [{
									"name": "External Hat",
									"type": "Toggle",
									"parameter": "Hat",
									"value": 1
								}]
							},
							"installTargetMenu": {
								"assetPath": "Assets/Menus/Avatar.asset",
								"controlCount": 1
							}
						}, {
							"shortType": "ModularAvatarMenuInstallTarget",
							"enabled": true,
							"id": "install-target-root",
							"hierarchyPath": "Root/InstallTarget",
							"siblingIndex": 3,
							"installer": {"path": "Root/MenuInstaller"}
						}, {
							"shortType": "ModularAvatarBlendshapeSync",
							"enabled": true,
							"target": {"path": "Root/Jacket"},
							"fields": {
								"Bindings": [{
									"referenceMesh": {"resolvedTarget": {"path": "Root/Body"}},
									"blendshape": "Breast_Big",
									"localBlendshape": "Jacket_Breast_Big",
									"remapCurve": {"keyCount": 2}
								}]
							}
						}, {
							"shortType": "ModularAvatarMeshCutter",
							"enabled": false,
							"id": "cut-sleeve",
							"fields": {
								"m_object": {"path": "Root/Body"},
								"m_multiMode": "VertexIntersection",
								"filters": [{
									"shortType": "VertexFilterByShapeComponent",
									"fields": {
										"m_shapes": ["Sleeve"],
										"m_threshold": 0.001
									}
								}]
							}
						}]
					},
					"wardrobe": {
						"baseSet": "base",
						"sets": [{
							"id": "base",
							"operations": [{"op": "subtreeEnabled"}]
						}, {
							"id": "jacket",
							"assetGroups": ["outfit:jacket"],
							"operations": [{"op": "nodeVisibility"}]
						}, {
							"id": "pants",
							"assetGroups": ["outfit:pants"],
							"operations": [{"op": "nodeVisibility"}]
						}, {
							"id": "hat",
							"operations": [{"op": "nodeVisibility"}]
						}]
					}
				}),
			}),
			..Default::default()
		};
		doc.runtime_model_mut()
			.set_active_asset_groups(vec!["outfit:jacket".to_string(), "missing:gloves".to_string()]);
		doc.runtime_model_mut().set_runtime_parameter_value("Hat", 1.0);

		let report = build_diagnose_report(
			Path::new("avatar.unavatar"),
			"io.un-avatar.gltf".into(),
			None,
			DiagnoseTimingSummary {
				import_ms: 0,
				wardrobe_apply_ms: 0,
				wardrobe_probe_ms: 0,
				report_build_ms: 0,
			},
			ImportReport::default(),
			doc,
			Vec::new(),
		);

		let unavatar = report.unavatar.as_ref().unwrap();
		assert_eq!(report.scene.asset_group_ownership_count, 1);
		assert_eq!(report.scene.asset_group_owned_mesh_primitive_count, 1);
		assert_eq!(report.scene.asset_group_owned_material_count, 1);
		assert_eq!(report.scene.asset_group_owned_image_count, 1);
		assert_eq!(report.scene.asset_group_owned_dynamics_count, 1);
		assert_eq!(report.scene.asset_group_ownership.len(), 1);
		assert_eq!(report.scene.asset_group_ownership[0].group_id, "outfit:jacket");
		assert_eq!(report.scene.asset_group_ownership[0].mesh_primitives.len(), 1);
		assert_eq!(report.scene.asset_group_ownership[0].mesh_primitives[0].mesh_index, 0);
		assert_eq!(report.scene.asset_group_ownership[0].mesh_primitives[0].primitive_index, 1);
		assert_eq!(report.scene.asset_group_ownership[0].materials, vec![2]);
		assert_eq!(report.scene.asset_group_ownership[0].images, vec![3]);
		assert_eq!(
			report.scene.asset_group_ownership[0].dynamics_source_ids,
			vec!["physbone:jacket".to_string()]
		);
		assert_eq!(report.scene.scoped_active_asset_group_count, 1);
		assert_eq!(report.scene.scoped_missing_active_asset_groups, vec!["missing:gloves".to_string()]);
		assert_eq!(report.scene.scoped_resident_mesh_primitive_count, 1);
		assert_eq!(report.scene.scoped_resident_material_count, 1);
		assert_eq!(report.scene.scoped_resident_image_count, 1);
		assert_eq!(report.scene.scoped_resident_dynamics_count, 1);
		assert_eq!(unavatar.asset_group_count, 2);
		assert_eq!(unavatar.modular_avatar_component_count, 10);
		assert_eq!(unavatar.modular_avatar_component_count_alias, 10);
		assert_eq!(unavatar.modular_avatar_support_counts.get("resolver"), Some(&1));
		assert_eq!(unavatar.modular_avatar_support_counts_alias.get("resolver"), Some(&1));
		assert_eq!(unavatar.modular_avatar_support_counts.get("approximate"), Some(&2));
		assert_eq!(unavatar.modular_avatar_support_counts_alias.get("approximate"), Some(&2));
		assert_eq!(unavatar.modular_avatar_support_counts.get("runtime_action"), Some(&1));
		assert_eq!(unavatar.modular_avatar_support_counts_alias.get("runtime_action"), Some(&1));
		assert_eq!(unavatar.modular_avatar_support_counts.get("metadata"), Some(&6));
		assert_eq!(unavatar.modular_avatar_support_counts_alias.get("metadata"), Some(&6));
		assert_eq!(unavatar.modular_avatar_support_counts.get("unsupported"), None);
		assert!(unavatar.modular_avatar_support_counts_alias.get("unsupported").is_none());
		assert_eq!(unavatar.modular_avatar_support_counts.get("disabled"), Some(&1));
		assert_eq!(unavatar.modular_avatar_support_counts_alias.get("disabled"), Some(&1));
		let actions = report.actions.as_ref().unwrap();
		let hat_action = actions.actions.iter().find(|action| action.id == "ma:hat").unwrap();
		assert_eq!(hat_action.condition_parameter_names, vec!["Hat".to_string()]);
		assert_eq!(hat_action.current_condition_state.as_deref(), Some("active"));
		assert_eq!(unavatar.modular_avatar_type_counts.get("ModularAvatarRemoveVertexColor"), Some(&1));
		assert_eq!(
			unavatar.modular_avatar_type_counts_alias.get("ModularAvatarRemoveVertexColor"),
			Some(&1)
		);
		assert_eq!(unavatar.modular_avatar_type_counts.get("ModularAvatarMenuItem"), Some(&2));
		assert_eq!(unavatar.modular_avatar_type_counts_alias.get("ModularAvatarMenuItem"), Some(&2));
		assert_eq!(unavatar.modular_avatar_type_counts.get("ModularAvatarParameters"), Some(&1));
		assert_eq!(unavatar.modular_avatar_type_counts_alias.get("ModularAvatarParameters"), Some(&1));
		assert_eq!(unavatar.modular_avatar_type_counts.get("ModularAvatarMenuGroup"), Some(&1));
		assert_eq!(unavatar.modular_avatar_type_counts_alias.get("ModularAvatarMenuGroup"), Some(&1));
		assert_eq!(unavatar.modular_avatar_type_counts.get("ModularAvatarMenuInstaller"), Some(&1));
		assert_eq!(
			unavatar.modular_avatar_type_counts_alias.get("ModularAvatarMenuInstaller"),
			Some(&1)
		);
		assert_eq!(unavatar.modular_avatar_type_counts.get("ModularAvatarMenuInstallTarget"), Some(&1));
		assert_eq!(
			unavatar.modular_avatar_type_counts_alias.get("ModularAvatarMenuInstallTarget"),
			Some(&1)
		);
		assert_eq!(unavatar.modular_avatar_type_counts.get("ModularAvatarMeshCutter"), Some(&1));
		assert_eq!(unavatar.modular_avatar_type_counts_alias.get("ModularAvatarMeshCutter"), Some(&1));
		assert_eq!(
			unavatar.modular_avatar_disabled_type_counts.get("ModularAvatarMeshCutter"),
			Some(&1)
		);
		assert_eq!(
			unavatar.modular_avatar_disabled_type_counts_alias.get("ModularAvatarMeshCutter"),
			Some(&1)
		);
		assert_eq!(unavatar.modular_avatar_disabled_component_count, 1);
		assert_eq!(unavatar.modular_avatar_disabled_component_count_alias, 1);
		assert_eq!(unavatar.modular_avatar_menu_component_count, 6);
		assert_eq!(unavatar.modular_avatar_menu_graph_candidate_count, 6);
		assert_eq!(unavatar.modular_avatar_menu_graph_node_count, 6);
		assert_eq!(unavatar.modular_avatar_menu_install_edge_count, 2);
		assert_eq!(unavatar.modular_avatar_vertex_filter_group_count, 1);
		let menu = &unavatar.modular_avatar_menu_components[0];
		assert_eq!(menu.component_index, 2);
		assert_eq!(menu.short_type, "ModularAvatarMenuItem");
		assert_eq!(menu.id.as_deref(), Some("menu-hat"));
		assert_eq!(menu.hierarchy_path.as_deref(), Some("Root/HatMenu"));
		assert_eq!(menu.sibling_index, Some(4));
		assert_eq!(menu.label.as_deref(), Some("Hat"));
		assert_eq!(menu.control_type.as_deref(), Some("RadialPuppet"));
		assert_eq!(menu.parameter.as_deref(), Some("Hat"));
		assert_eq!(menu.sub_parameters, vec!["HatAngle".to_string()]);
		assert_eq!(menu.value, Some(1.0));
		assert_eq!(menu.target_path.as_deref(), Some("Root/HatMenu"));
		assert_eq!(menu.menu_source.as_deref(), Some("Children"));
		assert_eq!(menu.menu_source_target_path.as_deref(), Some("Root/HatMenu/Children"));
		let installer_candidate = unavatar
			.modular_avatar_menu_graph_candidates
			.iter()
			.find(|candidate| candidate.menu_key == "component:6")
			.unwrap();
		assert_eq!(installer_candidate.component_index, 6);
		assert_eq!(installer_candidate.kind, "installer");
		assert_eq!(installer_candidate.parent_path.as_deref(), Some("Root"));
		assert_eq!(installer_candidate.sibling_index, Some(1));
		let installer_menu = &unavatar.modular_avatar_menu_components[3];
		assert_eq!(installer_menu.menu_to_append_path.as_deref(), Some("Assets/Menus/Root.asset"));
		assert_eq!(installer_menu.menu_to_append_control_count, Some(2));
		assert_eq!(
			installer_menu.install_target_menu_path.as_deref(),
			Some("Assets/Menus/Avatar.asset")
		);
		assert_eq!(installer_menu.install_target_menu_control_count, Some(1));
		assert_eq!(installer_candidate.menu_to_append_path.as_deref(), Some("Assets/Menus/Root.asset"));
		assert_eq!(
			installer_candidate.install_target_menu_path.as_deref(),
			Some("Assets/Menus/Avatar.asset")
		);
		let external_control = &unavatar.modular_avatar_menu_components[4];
		assert_eq!(external_control.short_type, "VRCExpressionsMenuControl");
		assert_eq!(external_control.source_component_index, Some(6));
		assert_eq!(external_control.menu_key, "external:6:0");
		assert_eq!(external_control.label.as_deref(), Some("External Hat"));
		assert_eq!(external_control.parameter.as_deref(), Some("Hat"));
		assert_eq!(
			external_control.external_menu_asset_path.as_deref(),
			Some("Assets/Menus/Root.asset")
		);
		assert_eq!(external_control.external_menu_control_index, Some(0));
		let external_control_candidate = unavatar
			.modular_avatar_menu_graph_candidates
			.iter()
			.find(|candidate| candidate.menu_key == "external:6:0")
			.unwrap();
		assert_eq!(external_control_candidate.kind, "control");
		assert_eq!(external_control_candidate.label.as_deref(), Some("External Hat"));
		let group_candidate = unavatar
			.modular_avatar_menu_graph_candidates
			.iter()
			.find(|candidate| candidate.menu_key == "component:4")
			.unwrap();
		assert_eq!(group_candidate.component_index, 4);
		assert_eq!(group_candidate.kind, "group");
		assert_eq!(group_candidate.hierarchy_path.as_deref(), Some("Root/Accessories"));
		assert_eq!(group_candidate.sibling_index, Some(2));
		assert_eq!(group_candidate.target_path.as_deref(), Some("Root/Accessories"));
		let install_target_candidate = unavatar
			.modular_avatar_menu_graph_candidates
			.iter()
			.find(|candidate| candidate.menu_key == "component:7")
			.unwrap();
		assert_eq!(install_target_candidate.component_index, 7);
		assert_eq!(install_target_candidate.kind, "install_target");
		assert_eq!(install_target_candidate.sibling_index, Some(3));
		assert_eq!(install_target_candidate.installer_path.as_deref(), Some("Root/MenuInstaller"));
		let control_candidate = unavatar
			.modular_avatar_menu_graph_candidates
			.iter()
			.find(|candidate| candidate.menu_key == "component:2")
			.unwrap();
		assert_eq!(control_candidate.component_index, 2);
		assert_eq!(control_candidate.kind, "control");
		assert_eq!(control_candidate.label.as_deref(), Some("Hat"));
		assert_eq!(control_candidate.sibling_index, Some(4));
		let nested_control_candidate = unavatar
			.modular_avatar_menu_graph_candidates
			.iter()
			.find(|candidate| candidate.menu_key == "component:5")
			.unwrap();
		assert_eq!(nested_control_candidate.component_index, 5);
		assert_eq!(nested_control_candidate.kind, "control");
		assert_eq!(nested_control_candidate.label.as_deref(), Some("Glasses"));
		assert_eq!(nested_control_candidate.parent_path.as_deref(), Some("Root/Accessories"));
		let group_node = unavatar
			.modular_avatar_menu_graph_nodes
			.iter()
			.find(|node| node.menu_key == "component:4")
			.unwrap();
		assert_eq!(group_node.component_index, 4);
		assert_eq!(group_node.child_component_indices, vec![5]);
		let nested_node = unavatar
			.modular_avatar_menu_graph_nodes
			.iter()
			.find(|node| node.menu_key == "component:5")
			.unwrap();
		assert_eq!(nested_node.component_index, 5);
		assert!(nested_node.parent_node_index.is_some());
		assert_eq!(nested_node.parent_component_index, Some(4));
		let install_target_node = unavatar
			.modular_avatar_menu_graph_nodes
			.iter()
			.find(|node| node.menu_key == "component:7")
			.unwrap();
		assert_eq!(install_target_node.component_index, 7);
		assert_eq!(install_target_node.kind, "install_target");
		assert_eq!(install_target_node.installer_path.as_deref(), Some("Root/MenuInstaller"));
		let installer_edge = &unavatar.modular_avatar_menu_install_edges[0];
		assert_eq!(installer_edge.source_component_index, 6);
		assert_eq!(installer_edge.source_kind, "installer");
		assert_eq!(installer_edge.target_kind, "target_menu");
		assert_eq!(installer_edge.menu_to_append_path.as_deref(), Some("Assets/Menus/Root.asset"));
		assert_eq!(
			installer_edge.install_target_menu_path.as_deref(),
			Some("Assets/Menus/Avatar.asset")
		);
		assert!(installer_edge.ignored_by_install_target);
		let install_target_edge = &unavatar.modular_avatar_menu_install_edges[1];
		assert_eq!(install_target_edge.source_component_index, 7);
		assert_eq!(install_target_edge.source_kind, "install_target");
		assert_eq!(install_target_edge.target_kind, "installer");
		assert_eq!(install_target_edge.installer_path.as_deref(), Some("Root/MenuInstaller"));
		assert!(!install_target_edge.ignored_by_install_target);
		assert_eq!(report.menu_action_candidates.len(), 3);
		let menu_action = &report.menu_action_candidates[0];
		assert_eq!(menu_action.menu_component_index, 2);
		assert_eq!(menu_action.menu_key, "component:2");
		assert_eq!(menu_action.menu_label.as_deref(), Some("Hat"));
		assert_eq!(menu_action.parameter_name, "Hat");
		assert_eq!(menu_action.parameter_value, 1.0);
		assert_eq!(menu_action.action_id, "ma:hat");
		assert_eq!(menu_action.match_kind, "condition");
		assert!(!menu_action.inverted);
		assert_eq!(menu_action.effect_kinds.get("node_visibility"), Some(&1));
		assert_eq!(menu_action.effect_kinds.get("wardrobe_set"), Some(&1));
		assert_eq!(menu_action.wardrobe_set_ids, vec!["hat".to_string()]);
		let nested_menu_action = &report.menu_action_candidates[1];
		assert_eq!(nested_menu_action.menu_component_index, 5);
		assert_eq!(nested_menu_action.menu_key, "component:5");
		assert_eq!(nested_menu_action.menu_label.as_deref(), Some("Glasses"));
		assert_eq!(nested_menu_action.parameter_name, "Glasses");
		assert_eq!(nested_menu_action.action_id, "ma:glasses");
		assert_eq!(nested_menu_action.match_kind, "trigger");
		assert_eq!(nested_menu_action.wardrobe_set_ids, vec!["glasses".to_string()]);
		let external_menu_action = &report.menu_action_candidates[2];
		assert_eq!(external_menu_action.menu_component_index, 6);
		assert_eq!(external_menu_action.menu_key, "external:6:0");
		assert_eq!(external_menu_action.menu_label.as_deref(), Some("External Hat"));
		assert_eq!(external_menu_action.parameter_name, "Hat");
		assert_eq!(external_menu_action.action_id, "ma:hat");
		assert_eq!(external_menu_action.match_kind, "condition");
		assert_eq!(external_menu_action.wardrobe_set_ids, vec!["hat".to_string()]);
		assert_eq!(report.menu_wardrobe_candidates.len(), 3);
		let wardrobe_candidate = &report.menu_wardrobe_candidates[0];
		assert_eq!(wardrobe_candidate.menu_component_index, 2);
		assert_eq!(wardrobe_candidate.menu_key, "component:2");
		assert_eq!(wardrobe_candidate.menu_path, vec!["Hat".to_string()]);
		assert_eq!(wardrobe_candidate.menu_label.as_deref(), Some("Hat"));
		assert_eq!(wardrobe_candidate.action_id, "ma:hat");
		assert_eq!(wardrobe_candidate.wardrobe_set_id, "hat");
		assert_eq!(wardrobe_candidate.match_kind, "condition");
		assert!(!wardrobe_candidate.inverted);
		let nested_wardrobe_candidate = &report.menu_wardrobe_candidates[1];
		assert_eq!(nested_wardrobe_candidate.menu_component_index, 5);
		assert_eq!(nested_wardrobe_candidate.menu_key, "component:5");
		assert_eq!(
			nested_wardrobe_candidate.menu_path,
			vec!["Accessories".to_string(), "Glasses".to_string()]
		);
		assert_eq!(nested_wardrobe_candidate.menu_label.as_deref(), Some("Glasses"));
		assert_eq!(nested_wardrobe_candidate.action_id, "ma:glasses");
		assert_eq!(nested_wardrobe_candidate.wardrobe_set_id, "glasses");
		assert_eq!(nested_wardrobe_candidate.match_kind, "trigger");
		assert!(!nested_wardrobe_candidate.inverted);
		let external_wardrobe_candidate = &report.menu_wardrobe_candidates[2];
		assert_eq!(external_wardrobe_candidate.menu_component_index, 6);
		assert_eq!(external_wardrobe_candidate.menu_key, "external:6:0");
		assert_eq!(external_wardrobe_candidate.menu_path, vec!["External Hat".to_string()]);
		assert_eq!(external_wardrobe_candidate.menu_label.as_deref(), Some("External Hat"));
		assert_eq!(external_wardrobe_candidate.action_id, "ma:hat");
		assert_eq!(external_wardrobe_candidate.wardrobe_set_id, "hat");
		assert_eq!(unavatar.modular_avatar_parameter_count, 2);
		assert_eq!(unavatar.modular_avatar_parameters[0].component_index, 3);
		assert_eq!(unavatar.modular_avatar_parameters[0].name_or_prefix, "Hat");
		assert_eq!(unavatar.modular_avatar_parameters[0].remap_to.as_deref(), Some("Hat_Remapped"));
		assert_eq!(unavatar.modular_avatar_parameters[0].sync_type, "Bool");
		assert!(!unavatar.modular_avatar_parameters[0].local_only);
		assert_eq!(unavatar.modular_avatar_parameters[0].default_value, 1.0);
		assert!(unavatar.modular_avatar_parameters[0].saved);
		assert!(unavatar.modular_avatar_parameters[0].has_explicit_default_value);
		assert!(unavatar.modular_avatar_parameters[1].is_prefix);
		assert!(unavatar.modular_avatar_parameters[1].internal_parameter);
		assert_eq!(unavatar.modular_avatar_parameters[1].sync_type, "NotSynced");
		assert!(unavatar.modular_avatar_parameters[1].local_only);
		assert!(unavatar.modular_avatar_parameters[1].override_animator_defaults);
		assert_eq!(unavatar.modular_avatar_blendshape_sync_count, 1);
		let sync = &unavatar.modular_avatar_blendshape_syncs[0];
		assert_eq!(sync.target_path.as_deref(), Some("Root/Jacket"));
		assert_eq!(sync.binding_count, 1);
		assert_eq!(sync.bindings[0].reference_path.as_deref(), Some("Root/Body"));
		assert_eq!(sync.bindings[0].blendshape, "Breast_Big");
		assert_eq!(sync.bindings[0].local_blendshape, "Jacket_Breast_Big");
		assert_eq!(sync.bindings[0].remap_key_count, 2);
		assert_eq!(
			unavatar.asset_group_ids,
			vec!["outfit:jacket".to_string(), "outfit:pants".to_string()]
		);
		let vertex_filter = &unavatar.modular_avatar_vertex_filter_groups[0];
		assert_eq!(vertex_filter.short_type, "ModularAvatarMeshCutter");
		assert!(!vertex_filter.enabled);
		assert_eq!(vertex_filter.id.as_deref(), Some("cut-sleeve"));
		assert_eq!(vertex_filter.target_path.as_deref(), Some("Root/Body"));
		assert_eq!(vertex_filter.combine, "VertexIntersection");
		assert_eq!(vertex_filter.filter_count, 1);
		assert_eq!(vertex_filter.filters[0].kind, "blend_shape");
		assert_eq!(vertex_filter.filters[0].shapes, vec!["Sleeve".to_string()]);
		assert_eq!(vertex_filter.filters[0].threshold, Some(0.001));
		assert!(report
			.warnings
			.iter()
			.any(|warning| warning.contains("wardrobe set \"hat\"") && warning.contains("no assetGroups")));
		assert!(!report
			.warnings
			.iter()
			.any(|warning| warning.contains("wardrobe set \"base\"") && warning.contains("no assetGroups")));
	}

	#[test]
	fn modular_avatar_vertex_filter_mask_summary_uses_texture_asset_id() {
		let component = serde_json::json!({
			"shortType": "ModularAvatarMeshCutter",
			"enabled": true,
			"fields": {
				"Object": {"path": "Root/Body"},
				"filters": [{
					"shortType": "VertexFilterByMaskComponent",
					"fields": {
						"m_materialIndex": 2,
						"m_deleteMode": "DeleteWhite",
						"maskTextureAssetId": "texture-asset-7"
					}
				}]
			}
		});

		let summary = modular_avatar_vertex_filter_group_summary(&component, "ModularAvatarMeshCutter").unwrap();

		assert_eq!(summary.filter_count, 1);
		assert_eq!(summary.filters[0].kind, "mask");
		assert_eq!(summary.filters[0].material_index, Some(2));
		assert_eq!(summary.filters[0].mode.as_deref(), Some("DeleteWhite"));
		assert_eq!(summary.filters[0].texture.as_deref(), Some("texture-asset-7"));
	}

	#[test]
	fn diagnose_report_keeps_scoped_missing_groups_without_scene() {
		let mut doc = UnaDocument::default();
		doc.runtime_model_mut()
			.set_active_asset_groups(vec!["outfit:coat".to_string(), "physbone:coat".to_string()]);

		let report = build_diagnose_report(
			Path::new("avatar.unavatar"),
			"io.un-avatar.gltf".into(),
			None,
			DiagnoseTimingSummary {
				import_ms: 0,
				wardrobe_apply_ms: 0,
				wardrobe_probe_ms: 0,
				report_build_ms: 0,
			},
			ImportReport::default(),
			doc,
			Vec::new(),
		);

		assert!(!report.scene.has_scene);
		assert_eq!(report.scene.scoped_active_asset_group_count, 0);
		assert_eq!(
			report.scene.scoped_missing_active_asset_groups,
			vec!["outfit:coat".to_string(), "physbone:coat".to_string()]
		);
		assert_eq!(report.scene.scoped_resident_mesh_primitive_count, 0);
		assert_eq!(report.scene.scoped_resident_material_count, 0);
		assert_eq!(report.scene.scoped_resident_image_count, 0);
		assert_eq!(report.scene.scoped_resident_dynamics_count, 0);
	}

	#[test]
	fn diagnose_report_surfaces_import_warnings_and_lost_features() {
		let mut import_report = ImportReport::default();
		import_report.push_warning(".unavatar Modular Avatar unsupported component: type=ModularAvatarMeshCutter, count=1");
		import_report.lost_features.push(un_avatar_core::LostFeature {
			feature: "ModularAvatar.ModularAvatarMeshCutter".to_string(),
			detail: Some("1 unsupported Modular Avatar component(s) were preserved as source payload but not applied".to_string()),
		});
		import_report.approximations.push(un_avatar_core::Approximation {
			feature: "ModularAvatar.ModularAvatarExample.Approximate".to_string(),
			detail: Some("example approximation is not exact".to_string()),
		});

		let report = build_diagnose_report(
			Path::new("avatar.unavatar"),
			"io.un-avatar.gltf".into(),
			None,
			DiagnoseTimingSummary {
				import_ms: 0,
				wardrobe_apply_ms: 0,
				wardrobe_probe_ms: 0,
				report_build_ms: 0,
			},
			import_report,
			UnaDocument::default(),
			Vec::new(),
		);

		assert!(report
			.warnings
			.iter()
			.any(|warning| warning.contains("import warning") && warning.contains("ModularAvatarMeshCutter")));
		assert!(report
			.warnings
			.iter()
			.any(|warning| warning.contains("import lost feature") && warning.contains("ModularAvatar.ModularAvatarMeshCutter")));
		assert!(report
			.warnings
			.iter()
			.any(|warning| warning.contains("import approximation") && warning.contains("ModularAvatar.ModularAvatarExample.Approximate")));
	}

	#[test]
	fn diagnose_report_summarizes_node_visibility_action_targets() {
		let doc = UnaDocument {
			runtime_actions: Some(un_avatar_core::UnaRuntimeActionSet {
				actions: vec![un_avatar_core::UnaRuntimeAction {
					id: "ma:object_toggle:hat".to_string(),
					label: "Hat".to_string(),
					triggers: vec![UnaRuntimeActionTrigger::ParameterValue {
						name: "Hat".to_string(),
						value: 1.0,
					}],
					conditions: Vec::new(),
					effects: vec![UnaRuntimeActionEffect::NodeVisibility {
						target: un_avatar_core::UnaRuntimeNodeTarget {
							node_index: None,
							source_node_id: Some("node_hat".to_string()),
							resolved_node_id: Some("resolved_hat".to_string()),
							path: Some("Root/Hat".to_string()),
						},
						visible: false,
					}],
				}],
			}),
			..Default::default()
		};

		let report = build_diagnose_report(
			Path::new("avatar.unavatar"),
			"io.un-avatar.gltf".into(),
			None,
			DiagnoseTimingSummary {
				import_ms: 0,
				wardrobe_apply_ms: 0,
				wardrobe_probe_ms: 0,
				report_build_ms: 0,
			},
			ImportReport::default(),
			doc,
			Vec::new(),
		);

		let action = &report.actions.as_ref().unwrap().actions[0];
		assert_eq!(action.node_visibility_effects.len(), 1);
		assert_eq!(action.node_visibility_effects[0].source_node_id.as_deref(), Some("node_hat"));
		assert_eq!(action.node_visibility_effects[0].resolved_node_id.as_deref(), Some("resolved_hat"));
		assert_eq!(action.node_visibility_effects[0].path.as_deref(), Some("Root/Hat"));
		assert!(!action.node_visibility_effects[0].visible);
	}

	#[test]
	fn diagnose_report_summarizes_material_slot_action_targets() {
		let doc = UnaDocument {
			runtime_actions: Some(un_avatar_core::UnaRuntimeActionSet {
				actions: vec![un_avatar_core::UnaRuntimeAction {
					id: "ma:material_setter:jacket".to_string(),
					label: "Jacket".to_string(),
					triggers: Vec::new(),
					conditions: Vec::new(),
					effects: vec![UnaRuntimeActionEffect::MaterialSlot {
						target: un_avatar_core::UnaRuntimeMaterialSlotTarget {
							node: un_avatar_core::UnaRuntimeNodeTarget {
								node_index: None,
								source_node_id: Some("node_jacket".to_string()),
								resolved_node_id: None,
								path: Some("Root/Jacket".to_string()),
							},
							primitive_index: Some(1),
						},
						material: Some(un_avatar_core::UnaRuntimeMaterialTarget {
							material_index: None,
							name: Some("Jacket Red".to_string()),
						}),
					}],
				}],
			}),
			..Default::default()
		};

		let report = build_diagnose_report(
			Path::new("avatar.unavatar"),
			"io.un-avatar.gltf".into(),
			None,
			DiagnoseTimingSummary {
				import_ms: 0,
				wardrobe_apply_ms: 0,
				wardrobe_probe_ms: 0,
				report_build_ms: 0,
			},
			ImportReport::default(),
			doc,
			Vec::new(),
		);

		let action = &report.actions.as_ref().unwrap().actions[0];
		assert_eq!(action.material_slot_effects.len(), 1);
		assert_eq!(action.material_slot_effects[0].source_node_id.as_deref(), Some("node_jacket"));
		assert_eq!(action.material_slot_effects[0].path.as_deref(), Some("Root/Jacket"));
		assert_eq!(action.material_slot_effects[0].primitive_index, Some(1));
		assert_eq!(action.material_slot_effects[0].material_name.as_deref(), Some("Jacket Red"));
	}

	#[test]
	fn diagnose_report_summarizes_remaining_action_effect_targets() {
		let doc = UnaDocument {
			runtime_actions: Some(un_avatar_core::UnaRuntimeActionSet {
				actions: vec![un_avatar_core::UnaRuntimeAction {
					id: "variant:coat".to_string(),
					label: "Coat".to_string(),
					triggers: Vec::new(),
					conditions: Vec::new(),
					effects: vec![
						UnaRuntimeActionEffect::MaterialColor {
							target: un_avatar_core::UnaRuntimeMaterialTarget {
								material_index: Some(2),
								name: Some("Coat".to_string()),
							},
							parameter: "_Color".to_string(),
							color: [1.0, 0.5, 0.25, 1.0],
						},
						UnaRuntimeActionEffect::MaterialScalar {
							target: un_avatar_core::UnaRuntimeMaterialTarget {
								material_index: Some(2),
								name: Some("Coat".to_string()),
							},
							parameter: "_Cutoff".to_string(),
							value: 0.4,
						},
						UnaRuntimeActionEffect::ExpressionWeight {
							name: "Smile".to_string(),
							weight: 0.75,
						},
						UnaRuntimeActionEffect::DynamicsEnabled {
							source_id: "physbone:hair".to_string(),
							enabled: false,
						},
					],
				}],
			}),
			..Default::default()
		};

		let report = build_diagnose_report(
			Path::new("avatar.unavatar"),
			"io.un-avatar.gltf".into(),
			None,
			DiagnoseTimingSummary {
				import_ms: 0,
				wardrobe_apply_ms: 0,
				wardrobe_probe_ms: 0,
				report_build_ms: 0,
			},
			ImportReport::default(),
			doc,
			Vec::new(),
		);

		let action = &report.actions.as_ref().unwrap().actions[0];
		let actions = report.actions.as_ref().unwrap();
		assert_eq!(actions.restore_readiness.len(), 4);
		assert!(actions.restore_baseline_candidates.is_empty());
		assert_eq!(actions.restore_readiness[0].owner_key, "action:variant:coat");
		assert_eq!(actions.restore_readiness[0].reason, "target_unresolved_or_unsupported_parameter");
		assert_eq!(actions.restore_readiness[2].reason, "not_restore_target");
		assert_eq!(action.target_writes.len(), 4);
		assert_eq!(action.target_writes[0].owner_key, "action:variant:coat");
		assert_eq!(
			action.target_writes[0].target_kind,
			un_avatar_core::UnaEvaluationTargetKind::MaterialProperty
		);
		assert_eq!(action.target_writes[0].target_key, "Coat:_Color");
		assert_eq!(action.material_property_effects.len(), 2);
		assert_eq!(action.material_property_effects[0].property_kind, "color");
		assert_eq!(action.material_property_effects[0].material_name.as_deref(), Some("Coat"));
		assert_eq!(action.material_property_effects[0].parameter, "_Color");
		assert_eq!(action.material_property_effects[0].color_value, Some([1.0, 0.5, 0.25, 1.0]));
		assert_eq!(action.material_property_effects[1].property_kind, "scalar");
		assert_eq!(action.material_property_effects[1].scalar_value, Some(0.4));
		assert_eq!(action.expression_weight_effects.len(), 1);
		assert_eq!(action.expression_weight_effects[0].name, "Smile");
		assert_eq!(action.expression_weight_effects[0].weight, 0.75);
		assert_eq!(action.dynamics_enabled_effects.len(), 1);
		assert_eq!(action.dynamics_enabled_effects[0].source_id, "physbone:hair");
		assert!(!action.dynamics_enabled_effects[0].enabled);
	}

	#[test]
	fn diagnose_report_summarizes_action_target_write_collisions() {
		let doc = UnaDocument {
			runtime_actions: Some(un_avatar_core::UnaRuntimeActionSet {
				actions: vec![
					un_avatar_core::UnaRuntimeAction {
						id: "hat:on".to_string(),
						effects: vec![UnaRuntimeActionEffect::NodeVisibility {
							target: un_avatar_core::UnaRuntimeNodeTarget {
								path: Some("Root/Hat".to_string()),
								..Default::default()
							},
							visible: true,
						}],
						..Default::default()
					},
					un_avatar_core::UnaRuntimeAction {
						id: "hat:off".to_string(),
						effects: vec![UnaRuntimeActionEffect::NodeVisibility {
							target: un_avatar_core::UnaRuntimeNodeTarget {
								path: Some("Root/Hat".to_string()),
								..Default::default()
							},
							visible: false,
						}],
						..Default::default()
					},
				],
			}),
			..Default::default()
		};

		let report = build_diagnose_report(
			Path::new("avatar.unavatar"),
			"io.un-avatar.gltf".into(),
			None,
			DiagnoseTimingSummary {
				import_ms: 0,
				wardrobe_apply_ms: 0,
				wardrobe_probe_ms: 0,
				report_build_ms: 0,
			},
			ImportReport::default(),
			doc,
			Vec::new(),
		);

		let actions = report.actions.as_ref().unwrap();
		assert_eq!(actions.target_write_collisions.len(), 1);
		assert_eq!(actions.target_write_collisions[0].target_key, "Root/Hat");
		assert_eq!(actions.target_write_collisions[0].action_ids, vec!["hat:off", "hat:on"]);
	}

	#[test]
	fn diagnose_report_warns_about_skinning_palette_risks() {
		fn identity_transform() -> [f32; 16] {
			[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]
		}
		fn primitive(joints: Option<Vec<[u16; 4]>>, weights: Option<Vec<[f32; 4]>>) -> un_avatar_core::UnaMeshBuffers {
			un_avatar_core::UnaMeshBuffers {
				name: None,
				vertex_payload_id: None,
				positions: vec![[0.0, 0.0, 0.0]],
				normals: None,
				tangents: None,
				tex_coords_0: None,
				tex_coords_1: None,
				tex_coords_2: None,
				tex_coords_3: None,
				colors_0: None,
				joints,
				weights,
				indices: None,
				material_index: None,
				morph_targets: Vec::new(),
				morph_target_names: Vec::new(),
				default_morph_weights: Vec::new(),
			}
		}

		let doc = UnaDocument {
			scene: Some(un_avatar_core::UnaSceneSnapshot {
				nodes: vec![un_avatar_core::UnaSceneNode {
					name: None,
					source_node_id: None,
					resolved_node_id: None,
					visible: true,
					transform: identity_transform(),
					children: Vec::new(),
					mesh: Some(0),
					skin: Some(0),
					probe_anchor_node: None,
					local_bounds: None,
				}],
				meshes: vec![vec![
					primitive(Some(vec![[600, 0, 0, 0]]), Some(vec![[1.0, 0.0, 0.0, 0.0]])),
					primitive(Some(vec![[0, 0, 0, 0]]), None),
				]],
				skins: vec![un_avatar_core::UnaSkin {
					joint_nodes: (0..513).collect(),
					inverse_bind_matrices: vec![[0.0; 16]; 513],
					skeleton_node: Some(0),
				}],
				..Default::default()
			}),
			..Default::default()
		};

		let report = build_diagnose_report(
			Path::new("avatar.unavatar"),
			"io.un-avatar.gltf".into(),
			None,
			DiagnoseTimingSummary {
				import_ms: 0,
				wardrobe_apply_ms: 0,
				wardrobe_probe_ms: 0,
				report_build_ms: 0,
			},
			ImportReport::default(),
			doc,
			Vec::new(),
		);

		let skin = &report.scene.skins[0];
		assert_eq!(skin.effective_joint_count, 513);
		assert!(skin.over_renderer_bone_limit);
		assert_eq!(skin.max_joint_index, Some(600));
		assert_eq!(skin.mismatched_joint_weight_attribute_count, 1);
		assert_eq!(skin.out_of_range_joint_attribute_count, 1);
		assert!(report.warnings.iter().any(|w| w.contains("renderer bone palette limit")));
		assert!(report.warnings.iter().any(|w| w.contains("mismatched JOINTS/WEIGHTS")));
		assert!(report.warnings.iter().any(|w| w.contains("outside effective palette")));
	}

	#[test]
	fn diagnose_report_warns_when_unavatar_dynamics_do_not_lower() {
		let doc = UnaDocument {
			scene: Some(un_avatar_core::UnaSceneSnapshot::default()),
			unavatar: Some(un_avatar_core::UnaUnavatarExtension {
				spec_version: "0.1".into(),
				source: serde_json::json!({
					"dynamics": [
						{
							"source": "vrc_physbone",
							"roots": [999],
							"allowGrabbing": true,
							"allowPosing": true,
							"allowCollision": false,
							"sourceParams": {
								"parameter": "HairPB",
								"maxStretch": 0.25,
								"radiusCurve": {"keyCount": 2, "keys": []},
								"maxAngleXCurve": {"keys": [{"time": 0.0, "value": 1.0}]},
								"maxStretchCurve": {"keyCount": 3},
								"colliders": [
									{"shapeType": 0, "insideBounds": true},
									{"shapeType": "1"},
									{"shapeType": "Plane"},
									{"shapeType": "box"}
								]
							}
						}
					]
				}),
			}),
			..Default::default()
		};

		let report = build_diagnose_report(
			Path::new("avatar.unavatar"),
			"io.un-avatar.gltf".into(),
			None,
			DiagnoseTimingSummary {
				import_ms: 0,
				wardrobe_apply_ms: 0,
				wardrobe_probe_ms: 0,
				report_build_ms: 0,
			},
			ImportReport::default(),
			doc,
			Vec::new(),
		);

		assert_eq!(report.unavatar.as_ref().unwrap().dynamics_entry_count, 1);
		assert_eq!(report.dynamics.group_count, 0);
		assert_eq!(report.dynamics.source_limit_count, 1);
		assert_eq!(report.dynamics.source_angle_limit_count, 0);
		assert_eq!(report.dynamics.source_stretch_limit_count, 1);
		assert_eq!(report.dynamics.source_curve_count, 1);
		assert_eq!(report.dynamics.source_radius_curve_count, 1);
		assert_eq!(report.dynamics.source_angle_limit_curve_count, 1);
		assert_eq!(report.dynamics.source_stretch_limit_curve_count, 1);
		assert_eq!(report.dynamics.source_collider_count, 4);
		assert_eq!(report.dynamics.source_unknown_shape_collider_count, 1);
		assert_eq!(report.dynamics.source_collision_disabled_count, 1);
		assert_eq!(report.dynamics.source_inside_bounds_collider_count, 1);
		assert_eq!(report.dynamics.source_grabbing_enabled_count, 1);
		assert_eq!(report.dynamics.source_posing_enabled_count, 1);
		assert_eq!(report.dynamics.source_interaction_parameter_count, 1);
		assert!(report
			.warnings
			.iter()
			.any(|w| w.contains("raw dynamics entries but no runtime dynamics groups")));
		assert!(report
			.warnings
			.iter()
			.any(|w| w.contains("raw dynamics source colliders include 1 unknown shape collider")));
		assert!(report
			.warnings
			.iter()
			.any(|w| w.contains("dynamics stretch/squish limits are authored in source data")
				&& w.contains("runtime_stretch_limit_groups=0")));
		assert!(report
			.warnings
			.iter()
			.any(|w| w.contains("dynamics radius curves are metadata-only in the current solver")));
		assert!(report
			.warnings
			.iter()
			.any(|w| w.contains("dynamics grabbing/posing interaction hooks without parameters are metadata-only in the current solver")));
	}

	#[test]
	fn diagnose_report_warns_that_lowered_radius_curves_are_approximated() {
		let doc = UnaDocument {
			scene: Some(un_avatar_core::UnaSceneSnapshot {
				nodes: vec![
					test_scene_node("root", test_identity_mat4(), vec![1]),
					test_scene_node("tip", translation_mat4(0.0, 1.0, 0.0), Vec::new()),
				],
				roots: vec![0],
				..Default::default()
			}),
			spring_bones: Some(un_avatar_core::UnaSpringBoneSettings {
				groups: vec![un_avatar_core::UnaSpringBoneGroup {
					source_kind: UnaDynamicsSourceKind::VrcPhysBone,
					enabled: true,
					source_id: "physbone:hair".into(),
					bone_node_indices: vec![0, 1],
					hit_radius: 0.03,
					hit_radius_samples: vec![0.015],
					stiffness: 0.2,
					stiffness_samples: Vec::new(),
					..Default::default()
				}],
				..Default::default()
			}),
			unavatar: Some(un_avatar_core::UnaUnavatarExtension {
				spec_version: "0.1".into(),
				source: serde_json::json!({
					"dynamics": [{
						"id": "physbone:hair",
						"source": "vrc_physbone",
						"roots": [{"nodeId": "root"}],
						"sourceParams": {
							"radiusCurve": {
								"keys": [
									{"time": 0.0, "value": 1.0},
									{"time": 1.0, "value": 0.5}
								]
							}
						}
					}]
				}),
			}),
			..Default::default()
		};

		let report = build_diagnose_report(
			Path::new("avatar.unavatar"),
			"io.un-avatar.gltf".into(),
			None,
			DiagnoseTimingSummary {
				import_ms: 0,
				wardrobe_apply_ms: 0,
				wardrobe_probe_ms: 0,
				report_build_ms: 0,
			},
			ImportReport::default(),
			doc,
			Vec::new(),
		);

		assert_eq!(report.dynamics.source_radius_curve_count, 1);
		assert_eq!(report.dynamics.groups[0].category, "hair");
		assert_eq!(report.dynamics.response_categories.len(), 1);
		assert_eq!(report.dynamics.response_categories[0].category, "hair");
		assert!(report.dynamics.response_categories[0].average_shape_preservation > 0.0);
		assert!(report.dynamics.response_categories[0].average_orientation_follow >= 0.0);
		assert_eq!(report.dynamics.response_groups.len(), 1);
		assert_eq!(report.dynamics.response_groups[0].source_id, "physbone:hair");
		assert_eq!(report.dynamics.response_groups[0].category, "hair");
		assert!(report.dynamics.response_groups[0].average_shape_preservation > 0.0);
		assert_eq!(report.dynamics.groups[0].hit_radius_sample_count, 1);
		assert_eq!(report.dynamics.groups[0].hit_radius_sample_min, Some(0.015));
		assert_eq!(report.dynamics.groups[0].hit_radius_sample_max, Some(0.015));
		assert!(report
			.warnings
			.iter()
			.any(|w| w.contains("dynamics radius curves are approximated as per-joint hit radius")));
		assert!(!report
			.warnings
			.iter()
			.any(|w| w.contains("dynamics radius curves are metadata-only in the current solver")));
	}

	#[test]
	fn diagnose_report_warns_on_duplicate_dynamics_source_ids() {
		let doc = UnaDocument {
			scene: Some(un_avatar_core::UnaSceneSnapshot {
				nodes: vec![
					test_scene_node("root", test_identity_mat4(), vec![1, 2]),
					test_scene_node("receiver", translation_mat4(0.0, 0.0, 0.0), Vec::new()),
					test_scene_node("sender", translation_mat4(0.07, 0.0, 0.0), Vec::new()),
				],
				roots: vec![0],
				..Default::default()
			}),
			spring_bones: Some(un_avatar_core::UnaSpringBoneSettings {
				groups: vec![
					un_avatar_core::UnaSpringBoneGroup {
						source_kind: UnaDynamicsSourceKind::VrcPhysBone,
						enabled: true,
						source_id: "physbone:hair".into(),
						bone_node_indices: vec![0, 1],
						writeback_mode: un_avatar_core::UnaDynamicsWritebackMode::RotationTranslation,
						limit: Some(un_avatar_core::UnaDynamicsLimit {
							limit_type: "angle".into(),
							limit_rotation: [0.0, 0.0, 0.0],
							max_angle_x: 45.0,
							max_angle_z: 20.0,
							max_stretch: 0.1,
							max_squish: 0.0,
							stretch_motion: None,
							max_stretch_samples: Vec::new(),
							max_squish_samples: Vec::new(),
							stretch_motion_samples: Vec::new(),
						}),
						interaction: Some(un_avatar_core::UnaDynamicsInteraction {
							allow_grabbing: Some(true),
							allow_posing: Some(false),
							parameter: "HairPB".to_string(),
						}),
						..Default::default()
					},
					un_avatar_core::UnaSpringBoneGroup {
						source_kind: UnaDynamicsSourceKind::VrcPhysBone,
						enabled: true,
						source_id: "physbone:hair".into(),
						bone_node_indices: vec![2, 3],
						interaction: Some(un_avatar_core::UnaDynamicsInteraction {
							allow_grabbing: Some(false),
							allow_posing: Some(true),
							parameter: String::new(),
						}),
						..Default::default()
					},
				],
				colliders: Vec::new(),
				contacts: vec![
					un_avatar_core::UnaDynamicsContact {
						source_kind: UnaDynamicsSourceKind::VrcPhysBone,
						source_id: "contact:hand".into(),
						node: 1,
						kind: un_avatar_core::UnaDynamicsContactKind::Receiver,
						parameter: "ContactHand".into(),
						collision_tags: vec!["Hand".into(), "Interact".into()],
						radius: 0.05,
						..Default::default()
					},
					un_avatar_core::UnaDynamicsContact {
						source_kind: UnaDynamicsSourceKind::VrcPhysBone,
						source_id: "contact:hand".into(),
						node: 2,
						kind: un_avatar_core::UnaDynamicsContactKind::Sender,
						collision_tags: vec!["Hand".into()],
						radius: 0.04,
						..Default::default()
					},
				],
				constraint_refs: vec![
					un_avatar_core::UnaDynamicsConstraintRef {
						source_kind: UnaDynamicsSourceKind::VrcPhysBone,
						source_id: "constraint:parent".into(),
						target_node: 1,
						source_nodes: vec![0],
						constraint_type: "parent".into(),
						weight: 0.75,
						..Default::default()
					},
					un_avatar_core::UnaDynamicsConstraintRef {
						source_kind: UnaDynamicsSourceKind::VrcPhysBone,
						source_id: "constraint:parent".into(),
						target_node: 2,
						source_nodes: vec![1],
						constraint_type: "rotation".into(),
						weight: 0.25,
						..Default::default()
					},
				],
				..Default::default()
			}),
			runtime_state: un_avatar_core::UnaRuntimeState {
				dynamics_enabled_overrides: BTreeMap::from([("physbone:hair".to_string(), false)]),
				..Default::default()
			},
			..Default::default()
		};

		let report = build_diagnose_report(
			Path::new("avatar.unavatar"),
			"io.un-avatar.gltf".into(),
			None,
			DiagnoseTimingSummary {
				import_ms: 0,
				wardrobe_apply_ms: 0,
				wardrobe_probe_ms: 0,
				report_build_ms: 0,
			},
			ImportReport::default(),
			doc,
			Vec::new(),
		);

		assert!(report
			.warnings
			.iter()
			.any(|w| w.contains("dynamics source_id \"physbone:hair\" lowers to 2 runtime groups")));
		assert!(report
			.warnings
			.iter()
			.any(|w| w.contains("dynamics contact source_id \"contact:hand\" lowers to 2 runtime contacts")));
		assert!(report
			.warnings
			.iter()
			.any(|w| w.contains("dynamics constraint_ref source_id \"constraint:parent\" lowers to 2 runtime constraint refs")));
		assert!(report
			.warnings
			.iter()
			.any(|w| w.contains("dynamics groups are present but none are currently enabled") && w.contains("samples=[physbone:hair@root")));
		assert!(report
			.warnings
			.iter()
			.any(|w| w.contains("dynamics stretch limits are supported as simulation stretch")
				&& w.contains("writeback_target_groups=1")
				&& w.contains("physbone:hair@root")));
		assert_eq!(report.dynamics.rotation_translation_writeback_group_count, 1);
		assert_eq!(report.dynamics.translation_writeback_candidate_count, 1);
		assert_eq!(report.dynamics.translation_writeback_target_count, 1);
		assert_eq!(report.dynamics.stretch_translation_writeback_group_count, 1);
		assert_eq!(report.dynamics.stretch_translation_writeback_target_group_count, 1);
		assert!(report.warnings.iter().any(|w| w
			.contains("dynamics contact probes would emit 1 parameter value(s), but contact parameter emission is disabled")
			&& w.contains("samples=[contact:hand@root/receiver<=contact:hand:ContactHand]")));
		assert!(report.warnings.iter().any(|w| w
			.contains("dynamics grabbing/posing interaction hooks without parameters are metadata-only in the current solver")
			&& w.contains("samples=[physbone:hair@root")));
		assert!(report.warnings.iter().any(|w| w
			.contains("dynamics VRC constraint refs are metadata/reset refs only in the current solver")
			&& w.contains("samples=[constraint:parent@root/receiver")));
		assert_eq!(report.dynamics.groups.len(), 2);
		assert!(report.dynamics.groups.iter().all(|group| group.source_enabled));
		assert!(report.dynamics.groups.iter().all(|group| !group.enabled));
		assert!(report
			.dynamics
			.groups
			.iter()
			.all(|group| group.runtime_enabled_override == Some(false)));
		assert_eq!(report.dynamics.groups[0].interaction_parameter, "HairPB");
		assert_eq!(report.dynamics.interaction_hooks.len(), 2);
		assert_eq!(report.dynamics.interaction_hooks[0].group_index, 0);
		assert_eq!(report.dynamics.interaction_hooks[0].parameter, "HairPB");
		assert!(report.dynamics.interaction_hooks[0].allow_grabbing);
		assert!(!report.dynamics.interaction_hooks[0].allow_posing);
		assert!(!report.dynamics.interaction_hooks[0].metadata_only);
		assert!(report.dynamics.interaction_hooks[0]
			.suffix_parameters
			.iter()
			.any(|parameter| parameter == "HairPB_IsGrabbed"));
		assert_eq!(report.dynamics.interaction_hooks[1].group_index, 1);
		assert!(report.dynamics.interaction_hooks[1].allow_posing);
		assert!(report.dynamics.interaction_hooks[1].suffix_parameters.is_empty());
		assert!(report.dynamics.interaction_hooks[1].metadata_only);
		assert_eq!(report.dynamics.limit_group_count, 1);
		assert_eq!(report.dynamics.angle_limit_group_count, 1);
		assert_eq!(report.dynamics.stretch_limit_group_count, 1);
		assert_eq!(report.dynamics.grabbing_enabled_group_count, 1);
		assert_eq!(report.dynamics.posing_enabled_group_count, 1);
		assert_eq!(report.dynamics.contact_count, 2);
		assert_eq!(report.dynamics.vrc_contact_receiver_count, 1);
		assert_eq!(report.dynamics.vrc_contact_sender_count, 1);
		assert_eq!(report.dynamics.contacts.len(), 2);
		assert_eq!(report.dynamics.contacts[0].kind, un_avatar_core::UnaDynamicsContactKind::Receiver);
		assert_eq!(report.dynamics.contacts[0].parameter, "ContactHand");
		assert_eq!(report.dynamics.contacts[0].collision_tags, vec!["Hand", "Interact"]);
		assert_eq!(report.dynamics.contacts[0].radius, 0.05);
		assert_eq!(report.dynamics.contact_parameter_declaration_count, 1);
		assert_eq!(report.dynamics.contact_parameter_declarations.len(), 1);
		assert_eq!(report.dynamics.contact_parameter_declarations[0].owner_key, "contact:hand");
		assert_eq!(report.dynamics.contact_parameter_declarations[0].source_id, "contact:hand");
		assert_eq!(report.dynamics.contact_parameter_declarations[0].parameter, "ContactHand");
		assert_eq!(
			report.dynamics.contact_parameter_declarations[0].collision_tags,
			vec!["Hand", "Interact"]
		);
		assert_eq!(report.dynamics.contact_probe_count, 1);
		assert_eq!(report.dynamics.contact_probe_would_emit_count, 1);
		assert_eq!(report.dynamics.contact_probes.len(), 1);
		assert!(report.dynamics.contact_probes[0].tag_match);
		assert!(report.dynamics.contact_probes[0].overlap);
		assert!(report.dynamics.contact_probes[0].would_emit);
		assert_eq!(report.dynamics.contact_probes[0].parameter, "ContactHand");
		assert_eq!(report.dynamics.contact_probes[0].matched_tags, vec!["Hand"]);
		assert_eq!(report.dynamics.constraint_ref_count, 2);
		assert_eq!(report.dynamics.vrc_constraint_ref_count, 2);
		assert_eq!(report.dynamics.constraint_refs.len(), 2);
		assert_eq!(report.dynamics.constraint_refs[0].source_id, "constraint:parent");
		assert_eq!(report.dynamics.constraint_refs[0].target_node, 1);
		assert_eq!(report.dynamics.constraint_refs[0].source_nodes, vec![0]);
		assert_eq!(report.dynamics.constraint_refs[0].constraint_type, "parent");
		assert_eq!(report.dynamics.constraint_refs[0].weight, 0.75);
	}

	#[test]
	fn diagnose_report_warns_on_large_stretch_range() {
		let doc = UnaDocument {
			scene: Some(un_avatar_core::UnaSceneSnapshot {
				nodes: vec![
					test_scene_node("root", test_identity_mat4(), vec![1]),
					test_scene_node("tip", translation_mat4(0.0, 0.05, 0.0), Vec::new()),
				],
				roots: vec![0],
				..Default::default()
			}),
			spring_bones: Some(un_avatar_core::UnaSpringBoneSettings {
				groups: vec![un_avatar_core::UnaSpringBoneGroup {
					source_kind: UnaDynamicsSourceKind::VrcPhysBone,
					enabled: true,
					source_id: "physbone:large-stretch".into(),
					bone_node_indices: vec![0, 1],
					writeback_mode: un_avatar_core::UnaDynamicsWritebackMode::RotationTranslation,
					limit: Some(un_avatar_core::UnaDynamicsLimit {
						limit_type: "angle".into(),
						limit_rotation: [0.0, 0.0, 0.0],
						max_angle_x: 0.0,
						max_angle_z: 0.0,
						max_stretch: 20.0,
						max_squish: 0.0,
						stretch_motion: Some(0.5),
						max_stretch_samples: Vec::new(),
						max_squish_samples: Vec::new(),
						stretch_motion_samples: Vec::new(),
					}),
					..Default::default()
				}],
				..Default::default()
			}),
			..Default::default()
		};

		let report = build_diagnose_report(
			Path::new("avatar.unavatar"),
			"io.un-avatar.gltf".into(),
			None,
			DiagnoseTimingSummary {
				import_ms: 0,
				wardrobe_apply_ms: 0,
				wardrobe_probe_ms: 0,
				report_build_ms: 0,
			},
			ImportReport::default(),
			doc,
			Vec::new(),
		);

		assert!(report.warnings.iter().any(|w| {
			w.contains("dynamics stretch range has very large authored/effective multiplier")
				&& w.contains("physbone:large-stretch@root")
				&& w.contains("max_stretch=20.000")
				&& w.contains("stretch_motion=0.500")
		}));
	}

	#[test]
	fn diagnose_report_warns_on_missing_wardrobe_dynamics_target() {
		let doc = UnaDocument {
			scene: Some(un_avatar_core::UnaSceneSnapshot::default()),
			spring_bones: Some(un_avatar_core::UnaSpringBoneSettings {
				groups: vec![un_avatar_core::UnaSpringBoneGroup {
					source_kind: UnaDynamicsSourceKind::VrcPhysBone,
					enabled: false,
					source_id: "physbone:hair".into(),
					bone_node_indices: vec![0, 1],
					..Default::default()
				}],
				..Default::default()
			}),
			unavatar: Some(un_avatar_core::UnaUnavatarExtension {
				spec_version: "0.1".into(),
				source: serde_json::json!({
					"dynamics": [
						{"id": "physbone:hair", "source": "vrc_physbone", "roots": [0]},
						{"id": "physbone:raw-only", "source": "vrc_physbone", "roots": [0], "enabled": false}
					],
					"wardrobe": {
						"sets": [{
							"id": "base",
							"operations": [
								{"type": "dynamicsEnable", "target": {"dynamicsId": "physbone:hair"}, "enabled": true},
								{"type": "dynamicsEnable", "target": {"dynamicsId": "physbone:raw-only"}, "enabled": true},
								{"type": "dynamicsEnable", "target": {"dynamicsId": "physbone:missing"}, "enabled": false}
							]
						}]
					}
				}),
			}),
			runtime_actions: Some(un_avatar_core::UnaRuntimeActionSet {
				actions: vec![un_avatar_core::UnaRuntimeAction {
					id: "action:dynamics".into(),
					label: "Dynamics".into(),
					triggers: Vec::new(),
					conditions: Vec::new(),
					effects: vec![
						UnaRuntimeActionEffect::DynamicsEnabled {
							source_id: "physbone:hair".into(),
							enabled: true,
						},
						UnaRuntimeActionEffect::DynamicsEnabled {
							source_id: "physbone:action-missing".into(),
							enabled: false,
						},
					],
				}],
			}),
			..Default::default()
		};

		let report = build_diagnose_report(
			Path::new("avatar.unavatar"),
			"io.un-avatar.gltf".into(),
			None,
			DiagnoseTimingSummary {
				import_ms: 0,
				wardrobe_apply_ms: 0,
				wardrobe_probe_ms: 0,
				report_build_ms: 0,
			},
			ImportReport::default(),
			doc,
			Vec::new(),
		);

		assert!(report
			.warnings
			.iter()
			.any(|w| w.contains("wardrobe dynamicsEnable target \"physbone:missing\"")));
		assert!(report
			.warnings
			.iter()
			.any(|w| w.contains("wardrobe dynamicsEnable target \"physbone:raw-only\"")));
		assert!(report
			.warnings
			.iter()
			.any(|w| { w.contains("runtime action \"action:dynamics\" DynamicsEnabled target \"physbone:action-missing\"") }));
		assert!(!report
			.warnings
			.iter()
			.any(|w| w.contains("wardrobe dynamicsEnable target \"physbone:hair\"")));
		assert!(!report
			.warnings
			.iter()
			.any(|w| { w.contains("runtime action \"action:dynamics\" DynamicsEnabled target \"physbone:hair\"") }));
	}

	#[test]
	fn convert_json_report_serializes() {
		let mut import_report = ImportReport::default();
		import_report.push_info("import line");
		let mut export_report = ExportReport::default();
		export_report.push_info("export line");
		let bundle = ConvertJsonReport {
			import_format_id: "io.un-avatar.gltf".into(),
			export_format_id: "io.un-avatar.example.avatar".into(),
			import_provider_plugin_id: None,
			export_provider_plugin_id: None,
			import_report,
			export_report,
		};
		let v = serde_json::to_value(&bundle).unwrap();
		assert!(v.get("import_report").is_some());
		assert!(v.get("export_report").is_some());
		assert_eq!(v["import_format_id"], "io.un-avatar.gltf");
		assert!(v.get("import_provider_plugin_id").is_none());
		assert!(v.get("export_provider_plugin_id").is_none());
		assert!(v["import_report"]["diagnostics"].is_array());
		assert_eq!(v["import_report"]["diagnostics"][0]["severity"], "info");
		assert!(v["export_report"]["diagnostics"].is_array());
	}

	#[test]
	fn convert_json_report_includes_provider_ids_when_set() {
		let bundle = ConvertJsonReport {
			import_format_id: "io.un-avatar.example.avatar".into(),
			export_format_id: "io.un-avatar.example.avatar".into(),
			import_provider_plugin_id: Some("network.usagi.un_avatar.plugin.sample_io".into()),
			export_provider_plugin_id: None,
			import_report: ImportReport::default(),
			export_report: ExportReport::default(),
		};
		let v = serde_json::to_value(&bundle).unwrap();
		assert_eq!(v["import_provider_plugin_id"], "network.usagi.un_avatar.plugin.sample_io");
		assert!(v.get("export_provider_plugin_id").is_none());
	}

	#[test]
	fn validate_report_json_skips_provider_when_none() {
		let r = ValidateReport {
			valid: true,
			path: "p".into(),
			error: None,
			format_id: Some("io.un-avatar.gltf".into()),
			provider_plugin_id: None,
		};
		let v = serde_json::to_value(&r).unwrap();
		assert!(v.get("provider_plugin_id").is_none());
		assert_eq!(v["format_id"], "io.un-avatar.gltf");
	}

	#[test]
	fn validate_report_json_includes_provider_when_set() {
		let r = ValidateReport {
			valid: true,
			path: "p".into(),
			error: None,
			format_id: Some("io.un-avatar.example.avatar".into()),
			provider_plugin_id: Some("network.usagi.un_avatar.plugin.sample_io".into()),
		};
		let v = serde_json::to_value(&r).unwrap();
		assert_eq!(v["provider_plugin_id"], "network.usagi.un_avatar.plugin.sample_io");
	}

	#[test]
	fn formats_list_json_sets_provider_plugin_id_for_stdio_importer() {
		let plugins = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/sample-io-plugin");
		let reg = io_registry_for_cli(&[plugins]).unwrap();
		let out = FormatsListJson {
			importers: reg.importer_descriptors(),
			exporters: reg.exporter_descriptors(),
		};
		let v = serde_json::to_value(&out).unwrap();
		let imp = v["importers"]
			.as_array()
			.unwrap()
			.iter()
			.find(|x| x["id"] == "io.un-avatar.example.avatar")
			.expect("sample importer row");
		assert_eq!(imp["provider_plugin_id"], "network.usagi.un_avatar.plugin.sample_io");
		let exp = v["exporters"]
			.as_array()
			.unwrap()
			.iter()
			.find(|x| x["id"] == "io.un-avatar.example.avatar")
			.expect("sample exporter row");
		assert_eq!(exp["provider_plugin_id"], "network.usagi.un_avatar.plugin.sample_io");
	}

	#[test]
	fn formats_list_json_contains_builtin_importers() {
		let reg = io_registry_for_cli(&[]).unwrap();
		let out = FormatsListJson {
			importers: reg.importer_descriptors(),
			exporters: reg.exporter_descriptors(),
		};
		let v = serde_json::to_value(&out).unwrap();
		let ids: Vec<_> = v["importers"]
			.as_array()
			.unwrap()
			.iter()
			.map(|x| x["id"].as_str().unwrap())
			.collect();
		assert!(ids.contains(&"io.un-avatar.vrm"));
		assert!(ids.contains(&"io.un-avatar.gltf"));
	}

	#[test]
	fn formats_probe_json_includes_provider_for_sample_plugin_path() {
		let plugins = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/sample-io-plugin");
		let reg = io_registry_for_cli(&[plugins]).unwrap();
		let v = serde_json::to_value(super::build_formats_probe_json(&reg, std::path::Path::new("x.exampleavatar"))).unwrap();
		let arr = v["importers"].as_array().expect("array");
		let row = arr
			.iter()
			.find(|x| x["format_id"] == "io.un-avatar.example.avatar")
			.expect("sample row");
		assert!(row["confidence"].as_u64().unwrap() > 0);
		assert_eq!(row["provider_plugin_id"], "network.usagi.un_avatar.plugin.sample_io");
		assert_eq!(v["best_importer"], "io.un-avatar.example.avatar");
		assert_eq!(v["best_importer_provider_plugin_id"], "network.usagi.un_avatar.plugin.sample_io");

		let ex = v["exporters"].as_array().expect("exporters");
		let erow = ex
			.iter()
			.find(|x| x["format_id"] == "io.un-avatar.example.avatar")
			.expect("sample exporter row");
		assert_eq!(erow["confidence"], 120);
		assert_eq!(erow["provider_plugin_id"], "network.usagi.un_avatar.plugin.sample_io");
		assert_eq!(v["best_exporter"], "io.un-avatar.example.avatar");
		assert_eq!(v["best_exporter_provider_plugin_id"], "network.usagi.un_avatar.plugin.sample_io");
	}
}
