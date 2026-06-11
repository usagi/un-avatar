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
use serde::Serialize;
use un_avatar_core::{
	modular_avatar_component_support_kind, morph_weights_for_primitive, UnaAlphaMode, UnaDynamicsSourceKind, UnaHumanoidRuntimeBasis,
	UnaImagePixelFormat, UnaMaterialPbr, UnaRuntimeActionEffect, UnaRuntimeActionTrigger, UnaRuntimeResolverCacheKey, UnaRuntimeSourceKind,
	UnaRuntimeToonModel, UnaSceneSnapshot, UnaShadingModel,
};
use un_avatar_io::{
	path_has_format_extension, AvatarExporter, AvatarImporter, ExportCapability, ExportContext, ExportOptions, ExportOutput, ExportReport,
	FormatDescriptor, FormatId, ImportContext, ImportInput, ImportOptions, ImportProbe, ImportReport, IoRegistry, UnaDocument,
};
use un_avatar_io_gltf::{apply_unavatar_wardrobe_set, register_gltf_importer, WardrobeApplyReport};
use un_avatar_io_una::{io_registry_with_una, read_una_any, UnaFileV0};
use un_avatar_io_vrm::register_vrm_importer;
use un_avatar_plugin_host::{register_stdio_exporters_from_plugin_root, register_stdio_importers_from_plugin_root};

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
	una: UnaFileV0,
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

#[derive(Serialize)]
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
	#[serde(skip_serializing_if = "Vec::is_empty")]
	menu_path: Vec<String>,
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
	constraint_ref_count: usize,
	vrc_constraint_ref_count: usize,
	source_limit_count: usize,
	source_angle_limit_count: usize,
	source_stretch_limit_count: usize,
	source_collision_disabled_count: usize,
	source_inside_bounds_collider_count: usize,
	source_grabbing_enabled_count: usize,
	source_posing_enabled_count: usize,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	contacts: Vec<DiagnoseDynamicsContactSummary>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	contact_parameter_declarations: Vec<DiagnoseContactParameterDeclarationSummary>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	contact_probes: Vec<DiagnoseContactProbeSummary>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	constraint_refs: Vec<DiagnoseDynamicsConstraintRefSummary>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	groups: Vec<DiagnoseDynamicsGroupSummary>,
}

#[derive(Default)]
struct DynamicsSourceFeatureCounts {
	limit_count: usize,
	angle_limit_count: usize,
	stretch_limit_count: usize,
	collision_disabled_count: usize,
	inside_bounds_collider_count: usize,
	grabbing_enabled_count: usize,
	posing_enabled_count: usize,
}

#[derive(Serialize)]
struct DiagnoseDynamicsGroupSummary {
	index: usize,
	source_kind: UnaDynamicsSourceKind,
	enabled: bool,
	source_enabled: bool,
	#[serde(skip_serializing_if = "String::is_empty")]
	source_id: String,
	#[serde(skip_serializing_if = "String::is_empty")]
	comment: String,
	#[serde(skip_serializing_if = "String::is_empty")]
	category: String,
	bone_count: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	root_node: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	root_path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	tip_node: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	tip_path: Option<String>,
	stiffness: f32,
	drag_force: f32,
	gravity_power: f32,
	gravity_dir: [f32; 3],
	#[serde(skip_serializing_if = "Option::is_none")]
	limit_type: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	max_angle_x: Option<f32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	max_angle_z: Option<f32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	max_stretch: Option<f32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	allow_grabbing: Option<bool>,
	#[serde(skip_serializing_if = "Option::is_none")]
	allow_posing: Option<bool>,
	hit_radius: f32,
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
	short_type: String,
	enabled: bool,
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
	install_target_menu_path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	installer_path: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct DiagnoseModularAvatarMenuGraphCandidate {
	component_index: usize,
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
	let mut reg = io_registry_with_una();
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
		("io.un-avatar.vrm" | "io.un-avatar.gltf", Some(bytes)) => ImportInput::Bytes {
			bytes,
			path_hint: Some(path.to_path_buf()),
		},
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
	/// アバターを別形式へ書き出す（現状は UNA v0 のみ）
	Convert {
		/// 入力ファイル、または `.una.d` ディレクトリ
		input: PathBuf,
		/// 出力 `.una` ファイル、または `.una.d` ディレクトリ
		output: PathBuf,
		/// 使う importer の FormatId（例: io.un-avatar.una）。省略時はパスから probe
		#[arg(long, value_name = "FORMAT_ID")]
		input_format: Option<String>,
		/// 使う exporter の FormatId。省略時は出力パスから選択
		#[arg(long, value_name = "FORMAT_ID")]
		output_format: Option<String>,
		/// import/export レポートを JSON で書き出す（`-` で stdout）
		#[arg(long, value_name = "PATH")]
		json_report: Option<PathBuf>,
	},
	/// UNA など、Importer 経由で読めるか検証する（終了コード 0/1）
	Validate {
		/// `.una` ファイルまたは `.una.d` ディレクトリ
		path: PathBuf,
		/// 使う importer の FormatId。省略時はパスから probe
		#[arg(long, value_name = "FORMAT_ID")]
		input_format: Option<String>,
		/// 結果を JSON で stdout に出す（失敗時も出力してから終了コード 1）
		#[arg(long)]
		json: bool,
	},
	/// UNA ファイル／バンドルを読み、スキーマ上の概要を表示する
	Inspect {
		path: PathBuf,
		#[arg(long)]
		json: bool,
	},
	/// Importer 経由でモデルを読み、材質・Humanoid・表情・VRM ヒントを診断する
	Diagnose {
		/// 入力ファイル、または `.una.d` ディレクトリ
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
		"formats" | "convert" | "validate" | "inspect" | "diagnose" | "vmc" | "help"
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
				"vrm" | "glb" | "gltf" | "unavatar" | "una" | "exampleavatar"
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
		Commands::Inspect { path, json } => run_inspect(path, json),
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
		let probe = import_probe_for_path(&path, cached_binary_import_bytes(&path));
		match reg.best_importer_for(&probe) {
			Some(i) => i,
			None => {
				let msg =
					"入力に合う importer が見つかりません（`.una` または `manifest.toml` 付き `.una.d` を指定、`--plugin-dir`、または --input-format）"
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
	};
	let path_display = path.display().to_string();
	let import_input = import_input_for_path(&path, &desc.id, cached_binary_import_bytes(&path));
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

fn run_inspect(path: PathBuf, json: bool) -> Result<(), String> {
	let path_str = path.to_string_lossy().to_string();
	let file = read_una_any(&path).map_err(|e| e.to_string())?;
	if json {
		let out = InspectReport { path: path_str, una: file };
		write_json_stdout(&out)?;
		return Ok(());
	}
	println!("path: {}", path.display());
	println!("format_version: {}", file.format_version);
	println!("scene.empty: {}", file.scene.empty);
	Ok(())
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
	material.mtoon_like_runtime().is_some_and(|mtoon| mtoon.transparent_with_z_write)
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

fn material_summary(index: usize, material: &UnaMaterialPbr, scene: &UnaSceneSnapshot) -> DiagnoseMaterialSummary {
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
	let mtoon = material.mtoon_like_runtime().map(|m| DiagnoseMToonSummary {
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
		base_color_texture_alpha: texture_alpha_summary(scene, material.base_color_texture_index),
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

fn dynamics_group_summaries(doc: &UnaDocument) -> Vec<DiagnoseDynamicsGroupSummary> {
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
		.map(|(index, group)| {
			let root_node = group.bone_node_indices.first().copied();
			let tip_node = group.bone_node_indices.last().copied();
			DiagnoseDynamicsGroupSummary {
				index,
				source_kind: group.source_kind,
				enabled: runtime.dynamics.group_enabled(group),
				source_enabled: group.enabled,
				source_id: group.source_id.clone(),
				comment: group.comment.clone(),
				category: group.category.clone(),
				bone_count: group.bone_node_indices.len(),
				root_node,
				root_path: root_node.and_then(|node| node_paths_by_index.get(node).cloned().flatten()),
				tip_node,
				tip_path: tip_node.and_then(|node| node_paths_by_index.get(node).cloned().flatten()),
				stiffness: group.stiffness,
				drag_force: group.drag_force,
				gravity_power: group.gravity_power,
				gravity_dir: group.gravity_dir,
				limit_type: group
					.limit
					.as_ref()
					.and_then(|limit| (!limit.limit_type.is_empty()).then(|| limit.limit_type.clone())),
				max_angle_x: group.limit.as_ref().map(|limit| limit.max_angle_x),
				max_angle_z: group.limit.as_ref().map(|limit| limit.max_angle_z),
				max_stretch: group.limit.as_ref().map(|limit| limit.max_stretch),
				allow_grabbing: group.interaction.as_ref().and_then(|interaction| interaction.allow_grabbing),
				allow_posing: group.interaction.as_ref().and_then(|interaction| interaction.allow_posing),
				hit_radius: group.hit_radius,
			}
		})
		.collect()
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
		if !limit_type.is_empty() || max_angle_x.abs() > 0.0 || max_angle_z.abs() > 0.0 || max_stretch.abs() > 0.0 {
			counts.limit_count += 1;
		}
		if !limit_type.is_empty() || max_angle_x.abs() > 0.0 || max_angle_z.abs() > 0.0 {
			counts.angle_limit_count += 1;
		}
		if max_stretch.abs() > 0.0 {
			counts.stretch_limit_count += 1;
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
		if let Some(colliders) = dynamics_source_value(item, source_params, "colliders", "colliders").and_then(|value| value.as_array()) {
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

fn json_number_f64(value: &serde_json::Value) -> Option<f64> {
	value.as_f64().or_else(|| value.as_i64().map(|value| value as f64))
}

fn visible_mesh_materials(scene: &un_avatar_core::UnaSceneSnapshot, mesh_index: usize) -> Vec<DiagnoseVisibleMaterialSummary> {
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
				&& texture_alpha_summary(scene, material.base_color_texture_index).is_some_and(|alpha| alpha.max_alpha == 0);
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
	const RENDERER_MAX_BONES: usize = 512;
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
	doc.unavatar
		.as_ref()
		.and_then(|ext| ext.source.get("wardrobe"))
		.and_then(|wardrobe| wardrobe.get("baseSet"))
		.and_then(|v| v.as_str())
		.unwrap_or("base")
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
		short_type: short_type.to_string(),
		enabled: component.get("enabled").and_then(|value| value.as_bool()).unwrap_or(true),
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
		install_target_menu_path: modular_avatar_ref_path(modular_avatar_component_ref(
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
	}
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
	Some(DiagnoseModularAvatarParameterSummary {
		component_index,
		name_or_prefix,
		remap_to,
		internal_parameter: json_bool(parameter.get("internalParameter").or_else(|| parameter.get("internal_parameter"))),
		is_prefix: json_bool(parameter.get("isPrefix").or_else(|| parameter.get("is_prefix"))),
		sync_type,
		local_only: json_bool(parameter.get("localOnly").or_else(|| parameter.get("local_only"))),
		default_value: parameter
			.get("defaultValue")
			.or_else(|| parameter.get("default_value"))
			.and_then(json_number_f64)
			.unwrap_or(0.0) as f32,
		saved: json_bool(parameter.get("saved")),
		has_explicit_default_value: json_bool(
			parameter
				.get("hasExplicitDefaultValue")
				.or_else(|| parameter.get("has_explicit_default_value")),
		),
		override_animator_defaults: json_bool(
			parameter
				.get("overrideAnimatorDefaults")
				.or_else(|| parameter.get("m_overrideAnimatorDefaults"))
				.or_else(|| parameter.get("override_animator_defaults")),
		),
	})
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
		"ModularAvatarMenuItem" => "control",
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
		"ModularAvatarMenuItem" | "ModularAvatarMenuGroup" | "ModularAvatarMenuInstaller" | "ModularAvatarMenuInstallTarget"
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
			texture: modular_avatar_component_string(component, &["MaskTexture", "maskTexture", "mask_texture", "m_maskTexture"]),
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
			let is_named_base = base_set
				.as_deref()
				.is_some_and(|base_set| set.get("id").and_then(|v| v.as_str()) == Some(base_set));
			let is_default = set.get("default").and_then(|v| v.as_bool()).unwrap_or(false);
			is_named_base || is_default
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
				&& texture_alpha_summary(sc, material.base_color_texture_index).is_some_and(|alpha| alpha.max_alpha == 0)
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
			materials.push(material_summary(i, material, sc));
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
				.any(|m| m.runtime_toon_model() == Some(UnaRuntimeToonModel::MToonLike))
		{
			warnings.push("VRM document has no MToonLike materials after import".to_string());
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
					materials: visible_mesh_materials(sc, mesh),
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
		resolver_cache_key: runtime_model.resolver_cache_key(),
	};
	let runtime_dynamics = runtime_model.dynamics();
	let dynamics_groups = dynamics_group_summaries(&doc);
	let dynamics_contacts = dynamics_contact_summaries(&doc);
	let dynamics_contact_parameter_declarations = dynamics_contact_parameter_declaration_summaries(&doc);
	let dynamics_contact_probes = dynamics_contact_probe_summaries(&doc);
	let dynamics_constraint_refs = dynamics_constraint_ref_summaries(&doc);
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
	let dynamics_counts = runtime_dynamics.counts();
	let dynamics = DiagnoseDynamicsSummary {
		group_count: dynamics_counts.groups,
		vrm_spring_bone_group_count: dynamics_counts.vrm_spring_bone_groups,
		vrc_physbone_group_count: dynamics_counts.vrc_physbone_groups,
		unknown_group_count: dynamics_counts.unknown_groups,
		limit_group_count: dynamics_counts.limit_groups,
		angle_limit_group_count: dynamics_counts.angle_limit_groups,
		stretch_limit_group_count: dynamics_counts.stretch_limit_groups,
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
		contact_parameter_emission_enabled: doc.runtime_model().contact_parameter_emission_enabled(),
		contact_probe_count: dynamics_contact_probes.len(),
		contact_probe_would_emit_count: dynamics_contact_probes.iter().filter(|probe| probe.would_emit).count(),
		constraint_ref_count: dynamics_counts.constraint_refs,
		vrc_constraint_ref_count: dynamics_counts.vrc_constraint_refs,
		source_limit_count: dynamics_source_features.limit_count,
		source_angle_limit_count: dynamics_source_features.angle_limit_count,
		source_stretch_limit_count: dynamics_source_features.stretch_limit_count,
		source_collision_disabled_count: dynamics_source_features.collision_disabled_count,
		source_inside_bounds_collider_count: dynamics_source_features.inside_bounds_collider_count,
		source_grabbing_enabled_count: dynamics_source_features.grabbing_enabled_count,
		source_posing_enabled_count: dynamics_source_features.posing_enabled_count,
		contacts: dynamics_contacts,
		contact_parameter_declarations: dynamics_contact_parameter_declarations,
		contact_probes: dynamics_contact_probes,
		constraint_refs: dynamics_constraint_refs,
		groups: dynamics_groups,
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
	let mut doc = doc.clone();
	doc.runtime_model().expression_catalog()?;
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
	}
	candidates.sort_by(|a, b| {
		(a.menu_component_index, a.action_id.as_str(), a.match_kind.as_str()).cmp(&(
			b.menu_component_index,
			b.action_id.as_str(),
			b.match_kind.as_str(),
		))
	});
	candidates
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

fn menu_graph_node_path(nodes: &[DiagnoseModularAvatarMenuGraphNode], node_index: usize) -> Vec<String> {
	let mut labels = Vec::new();
	let mut seen = BTreeSet::new();
	let mut current_index = Some(node_index);
	while let Some(index) = current_index {
		if index >= nodes.len() || !seen.insert(index) {
			break;
		}
		let node = &nodes[index];
		if let Some(label) = menu_graph_node_display_label(node) {
			labels.push(label);
		}
		current_index = node.parent_node_index;
	}
	labels.reverse();
	labels
}

fn diagnose_menu_wardrobe_candidates(
	unavatar: Option<&DiagnoseUnavatarSummary>,
	menu_action_candidates: &[DiagnoseMenuActionCandidate],
) -> Vec<DiagnoseMenuWardrobeCandidate> {
	let Some(unavatar) = unavatar else {
		return Vec::new();
	};
	let node_by_component = unavatar
		.modular_avatar_menu_graph_nodes
		.iter()
		.enumerate()
		.map(|(index, node)| (node.component_index, index))
		.collect::<BTreeMap<_, _>>();
	let mut candidates = Vec::new();
	for action_candidate in menu_action_candidates {
		if action_candidate.wardrobe_set_ids.is_empty() {
			continue;
		}
		let menu_path = node_by_component
			.get(&action_candidate.menu_component_index)
			.map(|node_index| menu_graph_node_path(&unavatar.modular_avatar_menu_graph_nodes, *node_index))
			.unwrap_or_else(|| action_candidate.menu_label.iter().cloned().collect());
		for wardrobe_set_id in &action_candidate.wardrobe_set_ids {
			candidates.push(DiagnoseMenuWardrobeCandidate {
				menu_component_index: action_candidate.menu_component_index,
				menu_path: menu_path.clone(),
				menu_label: action_candidate.menu_label.clone(),
				action_id: action_candidate.action_id.clone(),
				wardrobe_set_id: wardrobe_set_id.clone(),
				match_kind: action_candidate.match_kind.clone(),
				inverted: action_candidate.inverted,
			});
		}
	}
	candidates.sort_by(|a, b| {
		(a.menu_component_index, a.wardrobe_set_id.as_str(), a.action_id.as_str()).cmp(&(
			b.menu_component_index,
			b.wardrobe_set_id.as_str(),
			b.action_id.as_str(),
		))
	});
	candidates
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
	let importer: &dyn AvatarImporter = if let Some(ref s) = input_format {
		let id = FormatId::new(s.as_str());
		reg.importer_by_id(&id)
			.ok_or_else(|| format!("指定の importer が登録されていません: {s}"))?
	} else {
		let probe = import_probe_for_path(&path, cached_binary_import_bytes(&path));
		reg.best_importer_for(&probe)
			.ok_or_else(|| "入力に合う importer が見つかりません".to_string())?
	};
	let desc = importer.descriptor();
	let mut ictx = ImportContext {
		asset_root: path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(".")),
		temp_dir: std::env::temp_dir(),
	};
	let import_started = Instant::now();
	let mut imported = importer
		.import(
			&mut ictx,
			import_input_for_path(&path, &desc.id, cached_binary_import_bytes(&path)),
			ImportOptions,
		)
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
		"runtime: source={:?} humanoid_basis={:?} active_wardrobe_set={:?} active_asset_groups={:?} last_action_id={:?} parameter_values={}",
		report.runtime.source_kind,
		report.runtime.humanoid_basis,
		report.runtime.active_wardrobe_set,
		report.runtime.active_asset_groups,
		report.runtime.last_action_id,
		report.runtime.parameter_values.len()
	);
	if !report.runtime.parameter_values.is_empty() {
		println!("runtime.parameters: {:?}", report.runtime.parameter_values);
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
			"menu_wardrobe_candidate[#{} -> {}]: path={:?} label={:?} action={} match={} inverted={}",
			candidate.menu_component_index,
			candidate.wardrobe_set_id,
			candidate.menu_path,
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
		"dynamics: groups={} vrm_spring={} vrc_physbone={} unknown={} limit_groups={} angle_limit_groups={} stretch_limit_groups={} grabbing_groups={} posing_groups={} colliders={} collider_vrm_spring={} collider_vrc_physbone={} collider_unknown={} contacts={} contact_senders={} contact_receivers={} contact_parameter_declarations={} contact_parameter_emission={} contact_probes={} contact_probe_would_emit={} constraint_refs={} vrc_constraint_refs={} source_limits={} source_angle_limits={} source_stretch_limits={} source_collision_disabled={} source_inside_bounds_colliders={} source_grabbing={} source_posing={}",
		report.dynamics.group_count,
		report.dynamics.vrm_spring_bone_group_count,
		report.dynamics.vrc_physbone_group_count,
		report.dynamics.unknown_group_count,
		report.dynamics.limit_group_count,
		report.dynamics.angle_limit_group_count,
		report.dynamics.stretch_limit_group_count,
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
		report.dynamics.constraint_ref_count,
		report.dynamics.vrc_constraint_ref_count,
		report.dynamics.source_limit_count,
		report.dynamics.source_angle_limit_count,
		report.dynamics.source_stretch_limit_count,
		report.dynamics.source_collision_disabled_count,
		report.dynamics.source_inside_bounds_collider_count,
		report.dynamics.source_grabbing_enabled_count,
		report.dynamics.source_posing_enabled_count
	);
	for group in report.dynamics.groups.iter().take(16) {
		let limit = match (&group.limit_type, group.max_angle_x, group.max_angle_z, group.max_stretch) {
			(None, None, None, None) => String::new(),
			(limit_type, max_angle_x, max_angle_z, max_stretch) => format!(
				" limit={:?}/x={:?}/z={:?}/stretch={:?}",
				limit_type.as_deref(),
				max_angle_x,
				max_angle_z,
				max_stretch
			),
		};
		let interaction = match (group.allow_grabbing, group.allow_posing) {
			(None, None) => String::new(),
			(allow_grabbing, allow_posing) => format!(" interaction=grab:{allow_grabbing:?}/pose:{allow_posing:?}"),
		};
		println!(
			"  dynamics_group[{}]: source={:?} enabled={} source_enabled={} id={:?} bones={} root={:?} tip={:?} stiffness={} drag={} gravity={} radius={}{}{} comment={:?}",
			group.index,
			group.source_kind,
			group.enabled,
			group.source_enabled,
			group.source_id,
			group.bone_count,
			group.root_path.as_deref().or(group.root_node.map(|_| "#")),
			group.tip_path.as_deref().or(group.tip_node.map(|_| "#")),
			group.stiffness,
			group.drag_force,
			group.gravity_power,
			group.hit_radius,
			limit,
			interaction,
			group.comment
		);
	}
	for contact in report.dynamics.contacts.iter().take(16) {
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
	for declaration in report.dynamics.contact_parameter_declarations.iter().take(16) {
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
	for probe in report.dynamics.contact_probes.iter().take(16) {
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
	for constraint_ref in report.dynamics.constraint_refs.iter().take(16) {
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
				"unavatar.ma_menu[{}#{}]: enabled={} label={:?} type={:?} parameter={:?} value={:?} hierarchy={:?} sibling={:?} target={:?} menu_source={:?} source_target={:?} menu_to_append={:?} install_target_menu={:?} installer={:?}",
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
				menu.install_target_menu_path,
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
	println!("node_constraints: {}", report.scene.node_constraint_count);
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
			"入力に合う importer が見つかりません（`.una` または `manifest.toml` 付き `.una.d` を指定、`--plugin-dir`、または --input-format）".to_string()
		})?
	};
	let mut ictx = ImportContext {
		asset_root: input.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(".")),
		temp_dir: std::env::temp_dir(),
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
		reg.best_exporter_for(&imported.document, &output).ok_or_else(|| {
			"出力に使える exporter が見つかりません（`.una` または `.una.d` のパスを指定、または --output-format）".to_string()
		})?
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

	use std::fs;

	use un_avatar_io_una::write_una_path;

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
	fn io_registry_for_cli_empty_is_una_vrm_and_gltf() {
		let reg = io_registry_for_cli(&[]).unwrap();
		assert_eq!(reg.importer_descriptors().len(), 3);
		assert_eq!(reg.exporter_descriptors().len(), 1);
	}

	#[test]
	fn validate_import_pipeline_accepts_default_una_file() {
		let dir = std::env::temp_dir().join(format!("ua-cli-val-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		fs::create_dir_all(&dir).unwrap();
		let path = dir.join("t.una");
		write_una_path(&path, &UnaFileV0::default()).unwrap();
		let reg = io_registry_with_una();
		let probe = ImportProbe {
			path_hint: Some(path.clone()),
			bytes: None,
		};
		let imp = reg.best_importer_for(&probe).expect("importer");
		let mut ctx = ImportContext {
			asset_root: dir.clone(),
			temp_dir: std::env::temp_dir(),
		};
		imp.import(&mut ctx, ImportInput::Path(path), ImportOptions).unwrap();
		let _ = fs::remove_dir_all(&dir);
	}

	#[test]
	fn inspect_reads_una_summary_fields() {
		let dir = std::env::temp_dir().join(format!("ua-cli-inspect-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		fs::create_dir_all(&dir).unwrap();
		let path = dir.join("x.una");
		write_una_path(&path, &UnaFileV0::default()).unwrap();
		let f = read_una_any(&path).unwrap();
		assert_eq!(f.format_version, un_avatar_io_una::UNA_FORMAT_VERSION_V0);
		assert!(f.scene.empty);
		let _ = fs::remove_dir_all(&dir);
	}

	#[test]
	fn diagnose_report_summarizes_materials_and_vrm_hints() {
		let doc = UnaDocument {
			scene: Some(un_avatar_core::UnaSceneSnapshot {
				materials: vec![
					un_avatar_core::UnaMaterialPbr {
						name: Some("Eye_Iris".into()),
						shading: UnaShadingModel::MToonLike,
						alpha_mode: UnaAlphaMode::Mask,
						..Default::default()
					},
					un_avatar_core::UnaMaterialPbr {
						name: Some("Body".into()),
						shading: UnaShadingModel::MToonLike,
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
		assert_eq!(report.scene.shading_counts.get("MToonLike"), Some(&2));
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
									"localOnly": true,
									"defaultValue": 0.25,
									"m_overrideAnimatorDefaults": true
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
							"menuToAppend": {"path": "Menus/Root"},
							"installTargetMenu": {"path": "Avatar/Menu"}
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
							"assetGroups": ["outfit:jacket", "texture:red"],
							"operations": [{"op": "nodeVisibility"}]
						}, {
							"id": "pants",
							"assetGroups": ["outfit:pants", "texture:red"],
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
		assert_eq!(unavatar.asset_group_count, 3);
		assert_eq!(unavatar.modular_avatar_component_count, 10);
		assert_eq!(unavatar.modular_avatar_component_count_alias, 10);
		assert_eq!(unavatar.modular_avatar_support_counts.get("resolver"), Some(&3));
		assert_eq!(unavatar.modular_avatar_support_counts_alias.get("resolver"), Some(&3));
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
		assert_eq!(unavatar.modular_avatar_menu_component_count, 5);
		assert_eq!(unavatar.modular_avatar_menu_graph_candidate_count, 5);
		assert_eq!(unavatar.modular_avatar_menu_graph_node_count, 5);
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
		let installer_candidate = &unavatar.modular_avatar_menu_graph_candidates[0];
		assert_eq!(installer_candidate.component_index, 6);
		assert_eq!(installer_candidate.kind, "installer");
		assert_eq!(installer_candidate.parent_path.as_deref(), Some("Root"));
		assert_eq!(installer_candidate.sibling_index, Some(1));
		assert_eq!(installer_candidate.menu_to_append_path.as_deref(), Some("Menus/Root"));
		assert_eq!(installer_candidate.install_target_menu_path.as_deref(), Some("Avatar/Menu"));
		let group_candidate = &unavatar.modular_avatar_menu_graph_candidates[1];
		assert_eq!(group_candidate.component_index, 4);
		assert_eq!(group_candidate.kind, "group");
		assert_eq!(group_candidate.hierarchy_path.as_deref(), Some("Root/Accessories"));
		assert_eq!(group_candidate.sibling_index, Some(2));
		assert_eq!(group_candidate.target_path.as_deref(), Some("Root/Accessories"));
		let install_target_candidate = &unavatar.modular_avatar_menu_graph_candidates[2];
		assert_eq!(install_target_candidate.component_index, 7);
		assert_eq!(install_target_candidate.kind, "install_target");
		assert_eq!(install_target_candidate.sibling_index, Some(3));
		assert_eq!(install_target_candidate.installer_path.as_deref(), Some("Root/MenuInstaller"));
		let control_candidate = &unavatar.modular_avatar_menu_graph_candidates[3];
		assert_eq!(control_candidate.component_index, 2);
		assert_eq!(control_candidate.kind, "control");
		assert_eq!(control_candidate.label.as_deref(), Some("Hat"));
		assert_eq!(control_candidate.sibling_index, Some(4));
		let nested_control_candidate = &unavatar.modular_avatar_menu_graph_candidates[4];
		assert_eq!(nested_control_candidate.component_index, 5);
		assert_eq!(nested_control_candidate.kind, "control");
		assert_eq!(nested_control_candidate.label.as_deref(), Some("Glasses"));
		assert_eq!(nested_control_candidate.parent_path.as_deref(), Some("Root/Accessories"));
		let group_node = &unavatar.modular_avatar_menu_graph_nodes[1];
		assert_eq!(group_node.component_index, 4);
		assert_eq!(group_node.child_component_indices, vec![5]);
		let nested_node = &unavatar.modular_avatar_menu_graph_nodes[4];
		assert_eq!(nested_node.component_index, 5);
		assert_eq!(nested_node.parent_node_index, Some(1));
		assert_eq!(nested_node.parent_component_index, Some(4));
		let install_target_node = &unavatar.modular_avatar_menu_graph_nodes[2];
		assert_eq!(install_target_node.component_index, 7);
		assert_eq!(install_target_node.kind, "install_target");
		assert_eq!(install_target_node.installer_path.as_deref(), Some("Root/MenuInstaller"));
		let installer_edge = &unavatar.modular_avatar_menu_install_edges[0];
		assert_eq!(installer_edge.source_component_index, 6);
		assert_eq!(installer_edge.source_kind, "installer");
		assert_eq!(installer_edge.target_kind, "target_menu");
		assert_eq!(installer_edge.menu_to_append_path.as_deref(), Some("Menus/Root"));
		assert_eq!(installer_edge.install_target_menu_path.as_deref(), Some("Avatar/Menu"));
		assert!(installer_edge.ignored_by_install_target);
		let install_target_edge = &unavatar.modular_avatar_menu_install_edges[1];
		assert_eq!(install_target_edge.source_component_index, 7);
		assert_eq!(install_target_edge.source_kind, "install_target");
		assert_eq!(install_target_edge.target_kind, "installer");
		assert_eq!(install_target_edge.installer_path.as_deref(), Some("Root/MenuInstaller"));
		assert!(!install_target_edge.ignored_by_install_target);
		assert_eq!(report.menu_action_candidates.len(), 2);
		let menu_action = &report.menu_action_candidates[0];
		assert_eq!(menu_action.menu_component_index, 2);
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
		assert_eq!(nested_menu_action.menu_label.as_deref(), Some("Glasses"));
		assert_eq!(nested_menu_action.parameter_name, "Glasses");
		assert_eq!(nested_menu_action.action_id, "ma:glasses");
		assert_eq!(nested_menu_action.match_kind, "trigger");
		assert_eq!(nested_menu_action.wardrobe_set_ids, vec!["glasses".to_string()]);
		assert_eq!(report.menu_wardrobe_candidates.len(), 2);
		let wardrobe_candidate = &report.menu_wardrobe_candidates[0];
		assert_eq!(wardrobe_candidate.menu_component_index, 2);
		assert_eq!(wardrobe_candidate.menu_path, vec!["Hat".to_string()]);
		assert_eq!(wardrobe_candidate.menu_label.as_deref(), Some("Hat"));
		assert_eq!(wardrobe_candidate.action_id, "ma:hat");
		assert_eq!(wardrobe_candidate.wardrobe_set_id, "hat");
		assert_eq!(wardrobe_candidate.match_kind, "condition");
		assert!(!wardrobe_candidate.inverted);
		let nested_wardrobe_candidate = &report.menu_wardrobe_candidates[1];
		assert_eq!(nested_wardrobe_candidate.menu_component_index, 5);
		assert_eq!(
			nested_wardrobe_candidate.menu_path,
			vec!["Accessories".to_string(), "Glasses".to_string()]
		);
		assert_eq!(nested_wardrobe_candidate.menu_label.as_deref(), Some("Glasses"));
		assert_eq!(nested_wardrobe_candidate.action_id, "ma:glasses");
		assert_eq!(nested_wardrobe_candidate.wardrobe_set_id, "glasses");
		assert_eq!(nested_wardrobe_candidate.match_kind, "trigger");
		assert!(!nested_wardrobe_candidate.inverted);
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
			vec!["outfit:jacket".to_string(), "outfit:pants".to_string(), "texture:red".to_string()]
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
	fn diagnose_report_keeps_scoped_missing_groups_without_scene() {
		let mut doc = UnaDocument::default();
		doc.runtime_model_mut()
			.set_active_asset_groups(vec!["outfit:coat".to_string(), "texture:red".to_string()]);

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
			vec!["outfit:coat".to_string(), "texture:red".to_string()]
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
								"maxStretch": 0.25,
								"colliders": [
									{"insideBounds": true}
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
		assert_eq!(report.dynamics.source_collision_disabled_count, 1);
		assert_eq!(report.dynamics.source_inside_bounds_collider_count, 1);
		assert_eq!(report.dynamics.source_grabbing_enabled_count, 1);
		assert_eq!(report.dynamics.source_posing_enabled_count, 1);
		assert!(report
			.warnings
			.iter()
			.any(|w| w.contains("raw dynamics entries but no runtime dynamics groups")));
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
						limit: Some(un_avatar_core::UnaDynamicsLimit {
							limit_type: "angle".into(),
							max_angle_x: 45.0,
							max_angle_z: 20.0,
							max_stretch: 0.1,
						}),
						interaction: Some(un_avatar_core::UnaDynamicsInteraction {
							allow_grabbing: Some(true),
							allow_posing: Some(false),
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
		assert_eq!(report.dynamics.groups.len(), 2);
		assert!(report.dynamics.groups.iter().all(|group| group.source_enabled));
		assert!(report.dynamics.groups.iter().all(|group| !group.enabled));
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
	fn validate_import_works_with_explicit_format_on_path_without_una_suffix() {
		let dir = std::env::temp_dir().join(format!("ua-cli-val-fmt-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		fs::create_dir_all(&dir).unwrap();
		let path = dir.join("blob");
		write_una_path(&path, &UnaFileV0::default()).unwrap();
		let reg = io_registry_with_una();
		let imp = reg.importer_by_id(&FormatId::new("io.un-avatar.una")).expect("una importer");
		let mut ctx = ImportContext {
			asset_root: dir.clone(),
			temp_dir: std::env::temp_dir(),
		};
		imp.import(&mut ctx, ImportInput::Path(path), ImportOptions).unwrap();
		let _ = fs::remove_dir_all(&dir);
	}

	#[test]
	fn best_exporter_matches_una_d_suffix() {
		let reg = io_registry_with_una();
		let doc = un_avatar_io::UnaDocument::default();
		let out = PathBuf::from("avatar.una.d");
		let e = reg.best_exporter_for(&doc, &out).expect("exporter");
		assert_eq!(e.descriptor().id.0, "io.un-avatar.una");
	}

	#[test]
	fn convert_json_report_serializes() {
		let mut import_report = ImportReport::default();
		import_report.push_info("import line");
		let mut export_report = ExportReport::default();
		export_report.push_info("export line");
		let bundle = ConvertJsonReport {
			import_format_id: "io.un-avatar.una".into(),
			export_format_id: "io.un-avatar.una".into(),
			import_provider_plugin_id: None,
			export_provider_plugin_id: None,
			import_report,
			export_report,
		};
		let v = serde_json::to_value(&bundle).unwrap();
		assert!(v.get("import_report").is_some());
		assert!(v.get("export_report").is_some());
		assert_eq!(v["import_format_id"], "io.un-avatar.una");
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
			export_format_id: "io.un-avatar.una".into(),
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
			format_id: Some("io.un-avatar.una".into()),
			provider_plugin_id: None,
		};
		let v = serde_json::to_value(&r).unwrap();
		assert!(v.get("provider_plugin_id").is_none());
		assert_eq!(v["format_id"], "io.un-avatar.una");
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
		let una = v["importers"]
			.as_array()
			.unwrap()
			.iter()
			.find(|x| x["id"] == "io.un-avatar.una")
			.expect("una");
		assert!(una.get("provider_plugin_id").is_none());
	}

	#[test]
	fn formats_list_json_contains_una() {
		let reg = io_registry_with_una();
		let out = FormatsListJson {
			importers: reg.importer_descriptors(),
			exporters: reg.exporter_descriptors(),
		};
		let v = serde_json::to_value(&out).unwrap();
		assert!(!v["importers"].as_array().unwrap().is_empty());
		assert_eq!(v["importers"][0]["id"], "io.un-avatar.una");
	}

	#[test]
	fn formats_probe_json_has_positive_confidence_for_una_path() {
		let reg = io_registry_with_una();
		let v = serde_json::to_value(super::build_formats_probe_json(&reg, std::path::Path::new("model.una"))).unwrap();
		let arr = v["importers"].as_array().expect("array");
		let row = arr.iter().find(|x| x["format_id"] == "io.un-avatar.una").expect("una row");
		assert!(row["confidence"].as_u64().unwrap() > 0);
		assert!(row.get("provider_plugin_id").is_none());
		assert_eq!(v["best_importer"], "io.un-avatar.una");
		assert!(v.get("best_importer_provider_plugin_id").is_none());

		let ex = v["exporters"].as_array().expect("exporters");
		let erow = ex.iter().find(|x| x["format_id"] == "io.un-avatar.una").expect("una exporter row");
		assert_eq!(erow["confidence"], 120);
		assert!(erow.get("provider_plugin_id").is_none());
		assert_eq!(v["best_exporter"], "io.un-avatar.una");
		assert!(v.get("best_exporter_provider_plugin_id").is_none());
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

	#[test]
	fn io_una_registry_resolves_importer_exporter_by_id() {
		let reg = io_registry_with_una();
		let id = FormatId::new("io.un-avatar.una");
		assert!(reg.importer_by_id(&id).is_some());
		assert!(reg.exporter_by_id(&id).is_some());
	}
}
