//! wgpu デバイス・スワップチェーン・深度・プロシージャル空スカイ（カメラ／ライトのユニフォーム検証用）。

use std::{
	borrow::Cow,
	collections::{BTreeMap, BTreeSet},
	fmt::Write as _,
	net::SocketAddr,
	sync::{
		atomic::{AtomicU64, AtomicU8, Ordering},
		Arc, Mutex, RwLock,
	},
	time::{Duration, Instant},
};

use glam::{Mat4, Vec3, Vec4};
use serde_json::Value;
use un_avatar_core::{
	UnaDocument, UnaEvaluationTargetKind, UnaExpressionCatalog, UnaRuntimeActionEffect, UnaRuntimeActionQuery, UnaRuntimeActionTrigger,
	UnaRuntimeDynamicsCounts, UnaRuntimeResolverCacheKey, UnaSceneNode, UnaSceneSnapshot,
};
use un_avatar_skeleton::{
	build_dynamics_bone_colliders, collider_stats, local_capsule_world, local_sphere_world, BoneColliderConfig, BoneColliderPrimitive,
	BoneColliderSource, BoneColliderStats, DynamicsPhysicsConfig, DynamicsSimulator,
};
use winit::window::Window;

use crate::{
	camera::OrbitCamera,
	debug_dump::log_material_skin_report,
	debug_log::DebugLog,
	mesh_pass::{
		AvatarOutlineOptions, AvatarOutlinePolicy, MeshShaderVariantTier, SceneMeshActiveResidencyGaps, SceneMeshAssetResidencyCounts,
		SceneMeshAssetResidencyRefresh, SceneMeshBuildProgress, SceneMeshLoadOpts, SceneMeshRuntimeRequirements, SceneMeshes,
		TextureUploadSummary,
	},
	options::{
		AudioLinkOptions, AudioLinkSource, BloomOptions, ColorGradingLook, ContactShadowOptions, EnvironmentColorOptions, LightingOptions,
	},
	post_process::PostProcess,
	AaMode, BlockCompressionEncoder, RenderBackend, SpoutWindowOptions, TextureCompressionAdvancedOptions, TextureCompressionMode,
	WindowDebugOptions,
};

const SHADER_SKY: &str = include_str!("../shaders/sky.wgsl");
const SHADER_AXES: &str = include_str!("../shaders/axes.wgsl");
const SHADER_BONE_COLLIDERS: &str = include_str!("../shaders/bone_colliders.wgsl");
const SHADER_STARTUP_SPLASH: &str = include_str!("../shaders/startup_splash.wgsl");
const SHADER_CONTACT_SHADOW: &str = include_str!("../shaders/contact_shadow.wgsl");

pub(crate) const BASELINE_FALLBACK_SAMPLED_TEXTURES_PER_STAGE: u32 = 16;
pub(crate) const BASELINE_FALLBACK_SAMPLERS_PER_STAGE: u32 = 16;
pub(crate) const HIGH_CAPABILITY_LILTOON_SAMPLED_TEXTURES_PER_STAGE: u32 = 56;
pub(crate) const HIGH_CAPABILITY_LILTOON_SAMPLERS_PER_STAGE: u32 = 19;
const WARDROBE_RESIDENCY_GAP_INDEX_STATUS_LIMIT: usize = 64;
const DYNAMICS_GROUP_STATUS_LIMIT: usize = 64;
const DYNAMICS_COLLIDER_STATUS_LIMIT: usize = 64;
const CONTACT_PARAMETER_DECLARATION_STATUS_LIMIT: usize = 64;
const CONTACT_PROBE_STATUS_LIMIT: usize = 64;
const DYNAMICS_CONSTRAINT_REF_STATUS_LIMIT: usize = 64;
const WARDROBE_ASSET_UPLOAD_MODE_RESOURCE_SCOPED: &str = "draw-scoped-resource-scoped";
const CAMERA_NEAR_CLIP_M: f32 = 0.01;
const CAMERA_FAR_CLIP_M: f32 = 200.0;

#[derive(Clone, Debug)]
pub(crate) struct MeshShaderResourcePlan {
	pub(crate) tier: MeshShaderVariantTier,
	pub(crate) required_limits: wgpu::Limits,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RuntimeActionActivation {
	pub(crate) action_id: String,
	pub(crate) active_wardrobe_set: Option<String>,
	pub(crate) parameter_values: BTreeMap<String, f32>,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct RuntimeWardrobeActionStatus {
	pub(crate) action_id: String,
	pub(crate) label: String,
	pub(crate) set_id: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) expression_menu_path: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) supervisor_command: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) parameter_name: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) parameter_value: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct RuntimeActionStatus {
	pub(crate) action_id: String,
	pub(crate) label: String,
	#[serde(default)]
	pub(crate) effect_count: usize,
	#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
	pub(crate) effect_kinds: BTreeMap<String, usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) wardrobe_set_id: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) expression_menu_path: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) supervisor_command: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) parameter_name: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) parameter_value: Option<f32>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) condition_parameter_names: Vec<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) current_condition_state: Option<String>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) target_writes: Vec<un_avatar_core::UnaEvaluationRuntimeActionTargetWrite>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) node_visibility_effects: Vec<RuntimeActionNodeVisibilityEffectStatus>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) material_property_effects: Vec<RuntimeActionMaterialPropertyEffectStatus>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) material_slot_effects: Vec<RuntimeActionMaterialSlotEffectStatus>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) expression_weight_effects: Vec<RuntimeActionExpressionWeightEffectStatus>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) dynamics_enabled_effects: Vec<RuntimeActionDynamicsEnabledEffectStatus>,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct RuntimeActionNodeVisibilityEffectStatus {
	pub(crate) node_index: Option<usize>,
	pub(crate) source_node_id: Option<String>,
	pub(crate) resolved_node_id: Option<String>,
	pub(crate) path: Option<String>,
	pub(crate) visible: bool,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct RuntimeActionMaterialPropertyEffectStatus {
	pub(crate) property_kind: String,
	pub(crate) material_index: Option<usize>,
	pub(crate) material_name: Option<String>,
	pub(crate) parameter: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) scalar_value: Option<f32>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) color_value: Option<[f32; 4]>,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct RuntimeActionMaterialSlotEffectStatus {
	pub(crate) node_index: Option<usize>,
	pub(crate) source_node_id: Option<String>,
	pub(crate) resolved_node_id: Option<String>,
	pub(crate) path: Option<String>,
	pub(crate) primitive_index: Option<usize>,
	pub(crate) material_index: Option<usize>,
	pub(crate) material_name: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct RuntimeActionExpressionWeightEffectStatus {
	pub(crate) name: String,
	pub(crate) weight: f32,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct RuntimeActionDynamicsEnabledEffectStatus {
	pub(crate) source_id: String,
	pub(crate) enabled: bool,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct RuntimeMenuActionCandidateStatus {
	pub(crate) menu_component_index: usize,
	pub(crate) menu_key: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) menu_label: Option<String>,
	pub(crate) parameter_name: String,
	pub(crate) parameter_value: f32,
	pub(crate) action_id: String,
	pub(crate) action_label: String,
	pub(crate) match_kind: String,
	pub(crate) inverted: bool,
	pub(crate) effect_count: usize,
	#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
	pub(crate) effect_kinds: BTreeMap<String, usize>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) wardrobe_set_ids: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub(crate) struct RuntimeMenuWardrobeCandidateStatus {
	pub(crate) menu_component_index: usize,
	pub(crate) menu_key: String,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) menu_path: Vec<String>,
	#[serde(default, skip_serializing_if = "std::ops::Not::not")]
	pub(crate) menu_path_truncated: bool,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) menu_label: Option<String>,
	pub(crate) action_id: String,
	pub(crate) wardrobe_set_id: String,
	pub(crate) match_kind: String,
	pub(crate) inverted: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub(crate) struct RuntimeContactParameterDeclarationStatus {
	pub(crate) owner_key: String,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) source_id: String,
	pub(crate) node: usize,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) node_path: Option<String>,
	pub(crate) parameter: String,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) collision_tags: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct RuntimeContactProbeStatus {
	pub(crate) index: usize,
	pub(crate) receiver_index: usize,
	pub(crate) sender_index: usize,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) receiver_source_id: String,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) sender_source_id: String,
	pub(crate) receiver_node: usize,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) receiver_node_path: Option<String>,
	pub(crate) sender_node: usize,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) sender_node_path: Option<String>,
	pub(crate) parameter: String,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) matched_tags: Vec<String>,
	pub(crate) tag_match: bool,
	pub(crate) overlap: bool,
	pub(crate) would_emit: bool,
	pub(crate) distance: f32,
	pub(crate) threshold: f32,
	pub(crate) receiver_radius: f32,
	pub(crate) sender_radius: f32,
	pub(crate) receiver_shape: un_avatar_core::UnaDynamicsColliderShape,
	pub(crate) sender_shape: un_avatar_core::UnaDynamicsColliderShape,
	pub(crate) approximation: String,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct RuntimeContactParameterEmissionStatus {
	pub(crate) owner_key: String,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) source_id: String,
	pub(crate) receiver_index: usize,
	pub(crate) receiver_node: usize,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) receiver_node_path: Option<String>,
	pub(crate) parameter: String,
	pub(crate) value: f32,
	pub(crate) emitted: bool,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) sender_source_ids: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RuntimeContactProbeStatusSummary {
	pub(crate) count: u32,
	pub(crate) would_emit_count: u32,
	pub(crate) probes: Vec<RuntimeContactProbeStatus>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RuntimeContactParameterEmissionStatusSummary {
	pub(crate) count: u32,
	pub(crate) emitted_count: u32,
	pub(crate) reset_to_zero_count: u32,
	pub(crate) emissions: Vec<RuntimeContactParameterEmissionStatus>,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct RuntimeDynamicsGroupStatus {
	pub(crate) index: usize,
	pub(crate) source_kind: un_avatar_core::UnaDynamicsSourceKind,
	pub(crate) authored_enabled: bool,
	pub(crate) effective_enabled: bool,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) source_id: String,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) comment: String,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) category: String,
	pub(crate) bone_count: usize,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) root_node: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) root_path: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) tip_node: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) tip_path: Option<String>,
	pub(crate) stiffness: f32,
	pub(crate) drag_force: f32,
	pub(crate) gravity_power: f32,
	pub(crate) gravity_dir: [f32; 3],
	pub(crate) hit_radius: f32,
	pub(crate) hit_radius_sample_count: usize,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) hit_radius_sample_min: Option<f32>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) hit_radius_sample_max: Option<f32>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) center_node: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) center_path: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) limit_type: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) max_angle_x: Option<f32>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) max_angle_z: Option<f32>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) max_stretch: Option<f32>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) allow_grabbing: Option<bool>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) allow_posing: Option<bool>,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) interaction_parameter: String,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct RuntimeDynamicsInteractionHookStatus {
	pub(crate) group_index: usize,
	pub(crate) source_kind: un_avatar_core::UnaDynamicsSourceKind,
	pub(crate) authored_enabled: bool,
	pub(crate) effective_enabled: bool,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) source_id: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) root_path: Option<String>,
	pub(crate) allow_grabbing: bool,
	pub(crate) allow_posing: bool,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) parameter: String,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) suffix_parameters: Vec<String>,
	pub(crate) metadata_only: bool,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct RuntimeDynamicsColliderStatus {
	pub(crate) index: usize,
	pub(crate) source_kind: un_avatar_core::UnaDynamicsSourceKind,
	pub(crate) node: usize,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) node_path: Option<String>,
	pub(crate) shape: un_avatar_core::UnaDynamicsColliderShape,
	pub(crate) radius: f32,
	pub(crate) height: f32,
	pub(crate) position: [f32; 3],
	pub(crate) rotation: [f32; 4],
	pub(crate) inside_bounds: bool,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct RuntimeDynamicsConstraintRefStatus {
	pub(crate) index: usize,
	pub(crate) source_kind: un_avatar_core::UnaDynamicsSourceKind,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) source_id: String,
	pub(crate) target_node: usize,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) target_path: Option<String>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) source_nodes: Vec<usize>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) source_paths: Vec<String>,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) constraint_type: String,
	pub(crate) weight: f32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub(crate) struct WardrobeAssetUploadPlan {
	pub(crate) mode: String,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) active_asset_groups: Vec<String>,
	pub(crate) declared_asset_group_count: usize,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) declared_asset_groups: Vec<String>,
	pub(crate) owned_asset_group_count: usize,
	pub(crate) owned_mesh_primitive_count: usize,
	pub(crate) owned_material_count: usize,
	pub(crate) owned_image_count: usize,
	pub(crate) owned_dynamics_count: usize,
	pub(crate) resident_mesh_primitive_count: usize,
	pub(crate) resident_material_count: usize,
	pub(crate) resident_image_count: usize,
	pub(crate) resident_dynamics_count: usize,
	pub(crate) total_draw_mesh_primitive_count: usize,
	pub(crate) resident_draw_mesh_primitive_count: usize,
	pub(crate) inactive_draw_mesh_primitive_count: usize,
	pub(crate) total_draw_mesh_buffer_bytes: u64,
	pub(crate) resident_draw_mesh_buffer_bytes: u64,
	pub(crate) inactive_draw_mesh_buffer_bytes: u64,
	pub(crate) total_image_texture_count: usize,
	pub(crate) resident_image_texture_count: usize,
	pub(crate) inactive_image_texture_count: usize,
	pub(crate) draws_using_inactive_image_texture_count: usize,
	pub(crate) active_draws_using_inactive_image_texture_count: usize,
	pub(crate) inactive_image_textures_used_by_active_draw_count: usize,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) inactive_image_textures_used_by_active_draw: Vec<usize>,
	pub(crate) inactive_image_textures_used_by_active_draw_truncated: bool,
	pub(crate) total_material_slot_count: usize,
	pub(crate) resident_material_slot_count: usize,
	pub(crate) inactive_material_slot_count: usize,
	pub(crate) active_draws_using_inactive_material_slot_count: usize,
	pub(crate) inactive_material_slots_used_by_active_draw_count: usize,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) inactive_material_slots_used_by_active_draw: Vec<usize>,
	pub(crate) inactive_material_slots_used_by_active_draw_truncated: bool,
	pub(crate) pending_image_texture_upload_count: usize,
	pub(crate) pending_material_slot_upload_count: usize,
	pub(crate) last_residency_refresh_active_draw_change_count: usize,
	pub(crate) last_residency_refresh_image_load_count: usize,
	pub(crate) last_residency_refresh_image_unload_count: usize,
	pub(crate) last_residency_refresh_material_load_count: usize,
	pub(crate) last_residency_refresh_material_unload_count: usize,
	pub(crate) last_mesh_buffer_scoped_load_count: usize,
	pub(crate) last_mesh_buffer_scoped_unload_count: usize,
	pub(crate) last_image_texture_scoped_load_count: usize,
	pub(crate) last_image_texture_scoped_unload_count: usize,
	pub(crate) last_cubemap_scoped_load_count: usize,
	pub(crate) last_cubemap_scoped_unload_count: usize,
	pub(crate) last_material_slot_scoped_upload_count: usize,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) missing_active_asset_groups: Vec<String>,
	pub(crate) inactive_owned_asset_group_count: usize,
	pub(crate) scoped_draw_supported: bool,
	pub(crate) scoped_upload_supported: bool,
	pub(crate) all_resident: bool,
	pub(crate) active_residency_gaps_detected: bool,
	pub(crate) residency_gap_index_status_limit: usize,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) reason: String,
}

pub(crate) fn wardrobe_asset_upload_plan_is_default(plan: &WardrobeAssetUploadPlan) -> bool {
	plan == &WardrobeAssetUploadPlan::default()
}

struct WardrobeResidencyGapIndexStatus {
	indices: Vec<usize>,
	truncated: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct WardrobeScopedUploadWork {
	image_texture_indices: Vec<usize>,
	material_slot_indices: Vec<usize>,
	active_draws_using_inactive_image_texture_count: usize,
	active_draws_using_inactive_material_slot_count: usize,
}

impl WardrobeScopedUploadWork {
	fn has_pending_uploads(&self) -> bool {
		!self.image_texture_indices.is_empty() || !self.material_slot_indices.is_empty()
	}
}

fn wardrobe_scoped_upload_work_for_active_gaps(active_gaps: Option<SceneMeshActiveResidencyGaps>) -> WardrobeScopedUploadWork {
	let Some(active_gaps) = active_gaps else {
		return WardrobeScopedUploadWork::default();
	};
	WardrobeScopedUploadWork {
		image_texture_indices: active_gaps.inactive_image_texture_indices,
		material_slot_indices: active_gaps.inactive_material_slot_indices,
		active_draws_using_inactive_image_texture_count: active_gaps.active_draws_using_inactive_image_texture_count,
		active_draws_using_inactive_material_slot_count: active_gaps.active_draws_using_inactive_material_slot_count,
	}
}

fn wardrobe_asset_upload_plan_for_document(document: &UnaDocument) -> WardrobeAssetUploadPlan {
	let mut declared_asset_groups = document
		.unavatar
		.as_ref()
		.and_then(|unavatar| unavatar.source.get("wardrobe"))
		.and_then(|wardrobe| wardrobe.get("sets"))
		.and_then(|sets| sets.as_array())
		.map(|sets| {
			sets.iter()
				.flat_map(|set| {
					set.get("assetGroups")
						.or_else(|| set.get("asset_groups"))
						.and_then(|groups| groups.as_array())
						.into_iter()
						.flatten()
						.filter_map(|group| group.as_str().filter(|group| !group.is_empty()).map(str::to_owned))
				})
				.collect::<Vec<_>>()
		})
		.unwrap_or_default();
	declared_asset_groups.sort();
	declared_asset_groups.dedup();
	let active_asset_groups = document.runtime_model().active_asset_groups().to_vec();
	let has_declared_groups = !declared_asset_groups.is_empty();
	let ownership = document
		.scene
		.as_ref()
		.map(|scene| scene.asset_group_ownership_counts())
		.unwrap_or_default();
	let has_ownership = ownership.groups > 0;
	let source_asset_work = document.scoped_asset_selection();
	let inactive_owned_asset_group_count = ownership.groups.saturating_sub(source_asset_work.owned_active_groups.len());
	let has_active_asset_groups = !active_asset_groups.is_empty();
	WardrobeAssetUploadPlan {
		mode: if has_declared_groups {
			if has_ownership && has_active_asset_groups {
				WARDROBE_ASSET_UPLOAD_MODE_RESOURCE_SCOPED.to_string()
			} else {
				"all-resident".to_string()
			}
		} else {
			"unscoped".to_string()
		},
		active_asset_groups,
		declared_asset_group_count: declared_asset_groups.len(),
		declared_asset_groups,
		owned_asset_group_count: ownership.groups,
		owned_mesh_primitive_count: ownership.mesh_primitives,
		owned_material_count: ownership.materials,
		owned_image_count: ownership.images,
		owned_dynamics_count: ownership.dynamics,
		resident_mesh_primitive_count: source_asset_work.mesh_primitives.len(),
		resident_material_count: source_asset_work.materials.len(),
		resident_image_count: source_asset_work.images.len(),
		resident_dynamics_count: source_asset_work.dynamics_source_ids.len(),
		total_draw_mesh_primitive_count: 0,
		resident_draw_mesh_primitive_count: 0,
		inactive_draw_mesh_primitive_count: 0,
		total_draw_mesh_buffer_bytes: 0,
		resident_draw_mesh_buffer_bytes: 0,
		inactive_draw_mesh_buffer_bytes: 0,
		total_image_texture_count: 0,
		resident_image_texture_count: 0,
		inactive_image_texture_count: 0,
		draws_using_inactive_image_texture_count: 0,
		active_draws_using_inactive_image_texture_count: 0,
		inactive_image_textures_used_by_active_draw_count: 0,
		inactive_image_textures_used_by_active_draw: Vec::new(),
		inactive_image_textures_used_by_active_draw_truncated: false,
		total_material_slot_count: 0,
		resident_material_slot_count: 0,
		inactive_material_slot_count: 0,
		active_draws_using_inactive_material_slot_count: 0,
		inactive_material_slots_used_by_active_draw_count: 0,
		inactive_material_slots_used_by_active_draw: Vec::new(),
		inactive_material_slots_used_by_active_draw_truncated: false,
		pending_image_texture_upload_count: 0,
		pending_material_slot_upload_count: 0,
		last_residency_refresh_active_draw_change_count: 0,
		last_residency_refresh_image_load_count: 0,
		last_residency_refresh_image_unload_count: 0,
		last_residency_refresh_material_load_count: 0,
		last_residency_refresh_material_unload_count: 0,
		last_mesh_buffer_scoped_load_count: 0,
		last_mesh_buffer_scoped_unload_count: 0,
		last_image_texture_scoped_load_count: 0,
		last_image_texture_scoped_unload_count: 0,
		last_cubemap_scoped_load_count: 0,
		last_cubemap_scoped_unload_count: 0,
		last_material_slot_scoped_upload_count: 0,
		missing_active_asset_groups: source_asset_work.missing_active_asset_groups,
		inactive_owned_asset_group_count,
		scoped_draw_supported: false,
		scoped_upload_supported: has_declared_groups && has_ownership && has_active_asset_groups,
		all_resident: !(has_declared_groups && has_ownership && has_active_asset_groups),
		active_residency_gaps_detected: false,
		residency_gap_index_status_limit: 0,
		reason: if has_declared_groups && has_ownership && has_active_asset_groups {
			"wardrobe asset ownership metadata scopes renderer draw/material/texture residency for active asset groups; mesh buffers, image textures, and cubemap resources are scoped"
				.to_string()
		} else if has_declared_groups && has_ownership {
			"wardrobe asset ownership metadata is present, but no active asset groups are selected; GPU resources remain all-resident"
				.to_string()
		} else if has_declared_groups {
			"wardrobe sets declare assetGroups, but mesh/texture/material assets do not yet carry group ownership metadata".to_string()
		} else {
			"wardrobe sets do not declare assetGroups".to_string()
		},
	}
}

fn wardrobe_asset_upload_plan_with_draw_counts(
	mut plan: WardrobeAssetUploadPlan,
	draw_counts: Option<SceneMeshAssetResidencyCounts>,
) -> WardrobeAssetUploadPlan {
	let Some(draw_counts) = draw_counts else {
		return plan;
	};
	plan.residency_gap_index_status_limit = WARDROBE_RESIDENCY_GAP_INDEX_STATUS_LIMIT;
	plan.total_draw_mesh_primitive_count = draw_counts.total_draw_mesh_primitive_count;
	plan.resident_draw_mesh_primitive_count = draw_counts.resident_draw_mesh_primitive_count;
	plan.inactive_draw_mesh_primitive_count = draw_counts.inactive_draw_mesh_primitive_count;
	plan.total_draw_mesh_buffer_bytes = draw_counts.total_draw_mesh_buffer_bytes;
	plan.resident_draw_mesh_buffer_bytes = draw_counts.resident_draw_mesh_buffer_bytes;
	plan.inactive_draw_mesh_buffer_bytes = draw_counts.inactive_draw_mesh_buffer_bytes;
	plan.total_image_texture_count = draw_counts.total_image_texture_count;
	plan.resident_image_texture_count = draw_counts.resident_image_texture_count;
	plan.inactive_image_texture_count = draw_counts.inactive_image_texture_count;
	plan.draws_using_inactive_image_texture_count = draw_counts.draws_using_inactive_image_texture_count;
	plan.active_draws_using_inactive_image_texture_count = draw_counts.active_draws_using_inactive_image_texture_count;
	plan.inactive_image_textures_used_by_active_draw_count = draw_counts.inactive_image_textures_used_by_active_draw_count;
	let inactive_image_textures_used_by_active_draw =
		wardrobe_residency_gap_index_status_list(draw_counts.inactive_image_textures_used_by_active_draw);
	plan.inactive_image_textures_used_by_active_draw = inactive_image_textures_used_by_active_draw.indices;
	plan.inactive_image_textures_used_by_active_draw_truncated = inactive_image_textures_used_by_active_draw.truncated;
	plan.total_material_slot_count = draw_counts.total_material_slot_count;
	plan.resident_material_slot_count = draw_counts.resident_material_slot_count;
	plan.inactive_material_slot_count = draw_counts.inactive_material_slot_count;
	plan.active_draws_using_inactive_material_slot_count = draw_counts.active_draws_using_inactive_material_slot_count;
	plan.inactive_material_slots_used_by_active_draw_count = draw_counts.inactive_material_slots_used_by_active_draw_count;
	let inactive_material_slots_used_by_active_draw =
		wardrobe_residency_gap_index_status_list(draw_counts.inactive_material_slots_used_by_active_draw);
	plan.inactive_material_slots_used_by_active_draw = inactive_material_slots_used_by_active_draw.indices;
	plan.inactive_material_slots_used_by_active_draw_truncated = inactive_material_slots_used_by_active_draw.truncated;
	plan.pending_image_texture_upload_count = draw_counts.inactive_image_textures_used_by_active_draw_count;
	plan.pending_material_slot_upload_count = draw_counts.inactive_material_slots_used_by_active_draw_count;
	plan.scoped_draw_supported =
		draw_counts.inactive_draw_mesh_primitive_count > 0 || plan.mode == WARDROBE_ASSET_UPLOAD_MODE_RESOURCE_SCOPED;
	plan.active_residency_gaps_detected =
		draw_counts.active_draws_using_inactive_image_texture_count > 0 || draw_counts.active_draws_using_inactive_material_slot_count > 0;
	plan
}

fn wardrobe_residency_gap_index_status_list(mut indices: Vec<usize>) -> WardrobeResidencyGapIndexStatus {
	let truncated = indices.len() > WARDROBE_RESIDENCY_GAP_INDEX_STATUS_LIMIT;
	indices.truncate(WARDROBE_RESIDENCY_GAP_INDEX_STATUS_LIMIT);
	WardrobeResidencyGapIndexStatus { indices, truncated }
}

#[cfg(test)]
fn runtime_action_id_for_parameter(
	actions: &un_avatar_core::UnaRuntimeActionSet,
	scene: Option<&un_avatar_core::UnaSceneSnapshot>,
	name: &str,
	value: f32,
) -> Option<String> {
	runtime_action_ids_for_parameter(actions, scene, name, value).into_iter().next()
}

fn runtime_action_ids_for_parameter(
	actions: &un_avatar_core::UnaRuntimeActionSet,
	scene: Option<&un_avatar_core::UnaSceneSnapshot>,
	name: &str,
	value: f32,
) -> Vec<String> {
	let condition_matches = actions
		.actions
		.iter()
		.filter(|action| action.parameter_condition_state_in_scene(scene, name, value) == Some(true))
		.map(|action| action.id.clone())
		.collect::<Vec<_>>();
	if !condition_matches.is_empty() {
		return condition_matches;
	}
	actions
		.actions
		.iter()
		.filter(|action| {
			action.parameter_condition_state_in_scene(scene, name, value).is_none()
				&& action.matches_query(UnaRuntimeActionQuery {
					parameter_name: Some(name),
					parameter_value: Some(value),
					..Default::default()
				})
		})
		.map(|action| action.id.clone())
		.collect()
}

fn runtime_action_ids_for_parameter_values(
	actions: &un_avatar_core::UnaRuntimeActionSet,
	scene: Option<&un_avatar_core::UnaSceneSnapshot>,
	parameter_values: &BTreeMap<String, f32>,
) -> Vec<String> {
	let mut seen = BTreeSet::new();
	let mut ids = Vec::new();
	for (name, value) in parameter_values {
		for id in runtime_action_ids_for_parameter(actions, scene, name, *value) {
			if seen.insert(id.clone()) {
				ids.push(id);
			}
		}
	}
	ids
}

fn wardrobe_action_statuses(actions: &un_avatar_core::UnaRuntimeActionSet) -> Vec<RuntimeWardrobeActionStatus> {
	let mut statuses = Vec::new();
	for action in &actions.actions {
		let Some(set_id) = action.effects.iter().find_map(|effect| match effect {
			UnaRuntimeActionEffect::WardrobeSet { set_id } => Some(set_id.clone()),
			_ => None,
		}) else {
			continue;
		};
		let mut status = RuntimeWardrobeActionStatus {
			action_id: action.id.clone(),
			label: action.label.clone(),
			set_id,
			..Default::default()
		};
		for trigger in &action.triggers {
			match trigger {
				UnaRuntimeActionTrigger::ExpressionMenu { path } if status.expression_menu_path.is_none() => {
					status.expression_menu_path = Some(path.clone());
				}
				UnaRuntimeActionTrigger::SupervisorCommand { command } if status.supervisor_command.is_none() => {
					status.supervisor_command = Some(command.clone());
				}
				UnaRuntimeActionTrigger::ParameterValue { name, value } if status.parameter_name.is_none() => {
					status.parameter_name = Some(name.clone());
					status.parameter_value = Some(*value);
				}
				_ => {}
			}
		}
		statuses.push(status);
	}
	statuses.sort_by(|a, b| a.label.cmp(&b.label).then_with(|| a.action_id.cmp(&b.action_id)));
	statuses
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

fn runtime_action_effect_kind_counts<'a>(effects: impl IntoIterator<Item = &'a UnaRuntimeActionEffect>) -> BTreeMap<String, usize> {
	let mut counts = BTreeMap::new();
	for effect in effects {
		*counts.entry(runtime_action_effect_kind(effect).to_string()).or_insert(0) += 1;
	}
	counts
}

fn runtime_action_node_visibility_effects<'a>(
	effects: impl IntoIterator<Item = &'a UnaRuntimeActionEffect>,
) -> Vec<RuntimeActionNodeVisibilityEffectStatus> {
	effects
		.into_iter()
		.filter_map(|effect| match effect {
			UnaRuntimeActionEffect::NodeVisibility { target, visible } => Some(RuntimeActionNodeVisibilityEffectStatus {
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
) -> Vec<RuntimeActionMaterialPropertyEffectStatus> {
	effects
		.into_iter()
		.filter_map(|effect| match effect {
			UnaRuntimeActionEffect::MaterialColor { target, parameter, color } => Some(RuntimeActionMaterialPropertyEffectStatus {
				property_kind: "color".to_string(),
				material_index: target.material_index,
				material_name: target.name.clone(),
				parameter: parameter.clone(),
				scalar_value: None,
				color_value: Some(*color),
			}),
			UnaRuntimeActionEffect::MaterialScalar { target, parameter, value } => Some(RuntimeActionMaterialPropertyEffectStatus {
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
) -> Vec<RuntimeActionMaterialSlotEffectStatus> {
	effects
		.into_iter()
		.filter_map(|effect| match effect {
			UnaRuntimeActionEffect::MaterialSlot { target, material } => Some(RuntimeActionMaterialSlotEffectStatus {
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
) -> Vec<RuntimeActionExpressionWeightEffectStatus> {
	effects
		.into_iter()
		.filter_map(|effect| match effect {
			UnaRuntimeActionEffect::ExpressionWeight { name, weight } => Some(RuntimeActionExpressionWeightEffectStatus {
				name: name.clone(),
				weight: *weight,
			}),
			_ => None,
		})
		.collect()
}

fn runtime_action_dynamics_enabled_effects<'a>(
	effects: impl IntoIterator<Item = &'a UnaRuntimeActionEffect>,
) -> Vec<RuntimeActionDynamicsEnabledEffectStatus> {
	effects
		.into_iter()
		.filter_map(|effect| match effect {
			UnaRuntimeActionEffect::DynamicsEnabled { source_id, enabled } => Some(RuntimeActionDynamicsEnabledEffectStatus {
				source_id: source_id.clone(),
				enabled: *enabled,
			}),
			_ => None,
		})
		.collect()
}

fn runtime_action_statuses(
	actions: &un_avatar_core::UnaRuntimeActionSet,
	scene: Option<&UnaSceneSnapshot>,
	parameter_values: &BTreeMap<String, f32>,
) -> Vec<RuntimeActionStatus> {
	let mut statuses = Vec::new();
	for action in &actions.actions {
		let mut status = RuntimeActionStatus {
			action_id: action.id.clone(),
			label: action.label.clone(),
			effect_count: action.effects.len(),
			effect_kinds: runtime_action_effect_kind_counts(action.effects.iter()),
			condition_parameter_names: action.condition_parameter_names(),
			current_condition_state: action
				.current_parameter_condition_state(scene, parameter_values)
				.map(str::to_string),
			target_writes: action.evaluation_target_writes(),
			node_visibility_effects: runtime_action_node_visibility_effects(action.effects.iter()),
			material_property_effects: runtime_action_material_property_effects(action.effects.iter()),
			material_slot_effects: runtime_action_material_slot_effects(action.effects.iter()),
			expression_weight_effects: runtime_action_expression_weight_effects(action.effects.iter()),
			dynamics_enabled_effects: runtime_action_dynamics_enabled_effects(action.effects.iter()),
			..Default::default()
		};
		for trigger in &action.triggers {
			match trigger {
				UnaRuntimeActionTrigger::ExpressionMenu { path } if status.expression_menu_path.is_none() => {
					status.expression_menu_path = Some(path.clone());
				}
				UnaRuntimeActionTrigger::SupervisorCommand { command } if status.supervisor_command.is_none() => {
					status.supervisor_command = Some(command.clone());
				}
				UnaRuntimeActionTrigger::ParameterValue { name, value } if status.parameter_name.is_none() => {
					status.parameter_name = Some(name.clone());
					status.parameter_value = Some(*value);
				}
				_ => {}
			}
		}
		for effect in &action.effects {
			if let UnaRuntimeActionEffect::WardrobeSet { set_id } = effect {
				status.wardrobe_set_id = Some(set_id.clone());
				break;
			}
		}
		statuses.push(status);
	}
	statuses.sort_by(|a, b| a.label.cmp(&b.label).then_with(|| a.action_id.cmp(&b.action_id)));
	statuses
}

#[derive(Clone, Debug)]
struct RuntimeMenuComponentSummary {
	component_index: usize,
	menu_key: String,
	#[allow(clippy::struct_field_names)]
	hierarchy_path: Option<String>,
	sibling_index: Option<usize>,
	label: Option<String>,
	parameter_name: Option<String>,
	value: Option<f32>,
}

#[derive(Clone, Debug)]
struct RuntimeMenuGraphCandidate {
	component_index: usize,
	menu_key: String,
	label: Option<String>,
	hierarchy_path: Option<String>,
	sibling_index: Option<usize>,
}

#[derive(Clone, Debug)]
struct RuntimeMenuGraphNode {
	menu_key: String,
	label: Option<String>,
	hierarchy_path: Option<String>,
	parent_node_index: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct RuntimeMenuGraphNodePath {
	labels: Vec<String>,
	truncated: bool,
}

fn menu_action_candidates_from_runtime(
	unavatar: Option<&un_avatar_core::UnaUnavatarExtension>,
	actions: &un_avatar_core::UnaRuntimeActionSet,
) -> Option<Vec<RuntimeMenuActionCandidateStatus>> {
	let Some(unavatar) = unavatar else {
		return Some(Vec::new());
	};
	let menu_components = modular_avatar_menu_components(unavatar);
	if menu_components.is_empty() {
		return Some(Vec::new());
	}
	let mut candidates = Vec::new();
	for menu in &menu_components {
		let (Some(parameter_name), Some(parameter_value)) = (&menu.parameter_name, menu.value) else {
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
			candidates.push(RuntimeMenuActionCandidateStatus {
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
	Some(candidates)
}

fn menu_wardrobe_candidates_from_runtime(
	unavatar: Option<&un_avatar_core::UnaUnavatarExtension>,
	action_candidates: &[RuntimeMenuActionCandidateStatus],
) -> Vec<RuntimeMenuWardrobeCandidateStatus> {
	let Some(modular_avatar_components) = unavatar.map(modular_avatar_menu_components) else {
		return Vec::new();
	};
	let menu_components = modular_avatar_menu_graph_candidates(&modular_avatar_components);
	let nodes = modular_avatar_menu_graph_nodes(&menu_components);
	if nodes.is_empty() {
		return Vec::new();
	}
	let menu_path_by_key = nodes
		.iter()
		.enumerate()
		.map(|(index, node)| (node.menu_key.as_str(), menu_graph_node_path(&nodes, index)))
		.collect::<BTreeMap<_, _>>();
	let mut candidates = Vec::new();
	for action_candidate in action_candidates {
		if action_candidate.wardrobe_set_ids.is_empty() {
			continue;
		}
		let menu_path = menu_path_by_key
			.get(action_candidate.menu_key.as_str())
			.cloned()
			.unwrap_or_else(|| RuntimeMenuGraphNodePath {
				labels: action_candidate.menu_label.iter().cloned().collect(),
				truncated: false,
			});
		for wardrobe_set_id in &action_candidate.wardrobe_set_ids {
			candidates.push(RuntimeMenuWardrobeCandidateStatus {
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

fn contact_parameter_declaration_statuses(doc: &UnaDocument) -> Vec<RuntimeContactParameterDeclarationStatus> {
	let runtime_model = doc.runtime_model();
	let node_paths_by_index = runtime_model.scene().map(scene_node_paths_by_index).unwrap_or_default();
	runtime_model
		.dynamics()
		.contact_parameter_declarations()
		.into_iter()
		.take(CONTACT_PARAMETER_DECLARATION_STATUS_LIMIT)
		.map(|declaration| RuntimeContactParameterDeclarationStatus {
			owner_key: declaration.owner_key,
			source_id: declaration.source_id,
			node: declaration.node,
			node_path: node_paths_by_index.get(declaration.node).cloned().flatten(),
			parameter: declaration.parameter,
			collision_tags: declaration.collision_tags,
		})
		.collect()
}

fn contact_probe_status_summary(doc: &UnaDocument) -> RuntimeContactProbeStatusSummary {
	let Some(runtime) = doc.runtime_model().scene_profile_dynamics() else {
		return RuntimeContactProbeStatusSummary::default();
	};
	let node_paths_by_index = scene_node_paths_by_index(runtime.scene);
	let probes = runtime.contact_probes();
	let count = probes.len() as u32;
	let would_emit_count = probes.iter().filter(|probe| probe.would_emit).count() as u32;
	let probes = probes
		.into_iter()
		.enumerate()
		.take(CONTACT_PROBE_STATUS_LIMIT)
		.map(|(index, probe)| RuntimeContactProbeStatus {
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
		.collect();
	RuntimeContactProbeStatusSummary {
		count,
		would_emit_count,
		probes,
	}
}

fn contact_parameter_emission_status_summary(doc: &UnaDocument) -> RuntimeContactParameterEmissionStatusSummary {
	let runtime_model = doc.runtime_model();
	let node_paths_by_index = runtime_model.scene().map(scene_node_paths_by_index).unwrap_or_default();
	let emissions = runtime_model.contact_parameter_emissions();
	let count = emissions.len() as u32;
	let emitted_count = emissions.iter().filter(|emission| emission.emitted).count() as u32;
	let reset_to_zero_count = emissions.iter().filter(|emission| !emission.emitted).count() as u32;
	let emissions = emissions
		.into_iter()
		.take(CONTACT_PROBE_STATUS_LIMIT)
		.map(|emission| RuntimeContactParameterEmissionStatus {
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
		.collect();
	RuntimeContactParameterEmissionStatusSummary {
		count,
		emitted_count,
		reset_to_zero_count,
		emissions,
	}
}

fn dynamics_group_statuses(doc: &UnaDocument) -> Vec<RuntimeDynamicsGroupStatus> {
	let runtime_model = doc.runtime_model();
	let node_paths_by_index = runtime_model.scene().map(scene_node_paths_by_index).unwrap_or_default();
	runtime_model
		.dynamics()
		.dynamics_groups()
		.enumerate()
		.take(DYNAMICS_GROUP_STATUS_LIMIT)
		.map(|(index, group)| {
			let root_node = group.chain.bone_node_indices.first().copied();
			let tip_node = group.chain.bone_node_indices.last().copied();
			let center_node = group.parameters.center_node;
			let (hit_radius_sample_count, hit_radius_sample_min, hit_radius_sample_max) =
				dynamics_hit_radius_sample_summary(group.chain.hit_radius_samples);
			let limit_type = group
				.limit
				.and_then(|limit| (!limit.limit_type.is_empty()).then(|| limit.limit_type.clone()));
			RuntimeDynamicsGroupStatus {
				index,
				source_kind: group.source_kind,
				authored_enabled: group.authored_enabled,
				effective_enabled: group.effective_enabled,
				source_id: group.source_id.to_string(),
				comment: group.comment.to_string(),
				category: group.category.to_string(),
				bone_count: group.chain.bone_node_indices.len(),
				root_node,
				root_path: root_node.and_then(|node| node_paths_by_index.get(node).cloned().flatten()),
				tip_node,
				tip_path: tip_node.and_then(|node| node_paths_by_index.get(node).cloned().flatten()),
				stiffness: group.parameters.stiffness,
				drag_force: group.parameters.drag_force,
				gravity_power: group.parameters.gravity_power,
				gravity_dir: group.parameters.gravity_dir,
				hit_radius: group.parameters.hit_radius,
				hit_radius_sample_count,
				hit_radius_sample_min,
				hit_radius_sample_max,
				center_node,
				center_path: center_node.and_then(|node| node_paths_by_index.get(node).cloned().flatten()),
				limit_type,
				max_angle_x: group.limit.map(|limit| limit.max_angle_x),
				max_angle_z: group.limit.map(|limit| limit.max_angle_z),
				max_stretch: group.limit.map(|limit| limit.max_stretch),
				allow_grabbing: group.interaction.and_then(|interaction| interaction.allow_grabbing),
				allow_posing: group.interaction.and_then(|interaction| interaction.allow_posing),
				interaction_parameter: group
					.interaction
					.map(|interaction| interaction.parameter.clone())
					.unwrap_or_default(),
			}
		})
		.collect()
}

fn dynamics_interaction_hook_statuses(doc: &UnaDocument) -> Vec<RuntimeDynamicsInteractionHookStatus> {
	let runtime_model = doc.runtime_model();
	let node_paths_by_index = runtime_model.scene().map(scene_node_paths_by_index).unwrap_or_default();
	runtime_model
		.dynamics()
		.dynamics_groups()
		.enumerate()
		.take(DYNAMICS_GROUP_STATUS_LIMIT)
		.filter_map(|(group_index, group)| {
			let interaction = group.interaction?;
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
			let root_node = group.chain.bone_node_indices.first().copied();
			Some(RuntimeDynamicsInteractionHookStatus {
				group_index,
				source_kind: group.source_kind,
				authored_enabled: group.authored_enabled,
				effective_enabled: group.effective_enabled,
				source_id: group.source_id.to_string(),
				root_path: root_node.and_then(|node| node_paths_by_index.get(node).cloned().flatten()),
				allow_grabbing,
				allow_posing,
				parameter: interaction.parameter.clone(),
				suffix_parameters,
				metadata_only: true,
			})
		})
		.collect()
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

fn dynamics_collider_statuses(doc: &UnaDocument) -> Vec<RuntimeDynamicsColliderStatus> {
	let runtime_model = doc.runtime_model();
	let node_paths_by_index = runtime_model.scene().map(scene_node_paths_by_index).unwrap_or_default();
	runtime_model
		.dynamics()
		.colliders()
		.enumerate()
		.take(DYNAMICS_COLLIDER_STATUS_LIMIT)
		.map(|(index, collider)| RuntimeDynamicsColliderStatus {
			index,
			source_kind: collider.source_kind,
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

fn dynamics_constraint_ref_statuses(doc: &UnaDocument) -> Vec<RuntimeDynamicsConstraintRefStatus> {
	let runtime_model = doc.runtime_model();
	let node_paths_by_index = runtime_model.scene().map(scene_node_paths_by_index).unwrap_or_default();
	runtime_model
		.dynamics()
		.constraint_refs()
		.enumerate()
		.take(DYNAMICS_CONSTRAINT_REF_STATUS_LIMIT)
		.map(|(index, constraint_ref)| RuntimeDynamicsConstraintRefStatus {
			index,
			source_kind: constraint_ref.source_kind,
			source_id: constraint_ref.source_id.clone(),
			target_node: constraint_ref.target_node,
			target_path: node_paths_by_index.get(constraint_ref.target_node).cloned().flatten(),
			source_nodes: constraint_ref.source_nodes.clone(),
			source_paths: constraint_ref
				.source_nodes
				.iter()
				.filter_map(|node| node_paths_by_index.get(*node).cloned().flatten())
				.collect(),
			constraint_type: constraint_ref.constraint_type.clone(),
			weight: constraint_ref.weight,
		})
		.collect()
}

fn scene_node_paths_by_index(scene: &UnaSceneSnapshot) -> Vec<Option<String>> {
	fn visit(scene: &UnaSceneSnapshot, idx: usize, parent: &str, out: &mut [Option<String>]) {
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
	for &root in scene.resolved_roots().iter() {
		visit(scene, root, "", &mut out);
	}
	out
}

fn modular_avatar_menu_components(unavatar: &un_avatar_core::UnaUnavatarExtension) -> Vec<RuntimeMenuComponentSummary> {
	let source = &unavatar.source;
	let Some(components) = source
		.get("modularAvatar")
		.and_then(|modular_avatar| modular_avatar.get("components"))
		.and_then(|components| components.as_array())
	else {
		return Vec::new();
	};
	let mut menu_components = Vec::new();
	for (component_index, component) in components.iter().enumerate() {
		let short_type = component
			.get("shortType")
			.and_then(|value| value.as_str())
			.filter(|value| !value.is_empty())
			.unwrap_or("unknown");
		if !modular_avatar_is_menu_metadata_type(short_type) {
			continue;
		}
		menu_components.push(modular_avatar_menu_component_summary(component, component_index));
		if short_type == "ModularAvatarMenuInstaller" {
			menu_components.extend(modular_avatar_external_menu_component_summaries(component, component_index));
		}
	}
	menu_components
}

fn modular_avatar_menu_component_summary(component: &Value, component_index: usize) -> RuntimeMenuComponentSummary {
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
	RuntimeMenuComponentSummary {
		component_index,
		menu_key: format!("component:{component_index}"),
		hierarchy_path: modular_avatar_component_string(component, &["hierarchyPath", "hierarchy_path", "componentPath", "component_path"]),
		sibling_index: modular_avatar_component_usize(component, &["siblingIndex", "sibling_index", "transformSiblingIndex", "order"]),
		label: modular_avatar_component_string(component, &["label", "Label", "name", "Name", "displayName", "display_name"])
			.or_else(|| modular_avatar_component_string(menu_item, &["label", "Label", "name", "Name", "displayName", "display_name"]))
			.or_else(|| modular_avatar_component_string(control, &["name", "Name", "displayName", "display_name"])),
		parameter_name: parameter,
		value: control
			.get("value")
			.or_else(|| control.get("Value"))
			.and_then(json_number_f64)
			.map(|value| value as f32),
	}
}

fn modular_avatar_external_menu_component_summaries(component: &Value, component_index: usize) -> Vec<RuntimeMenuComponentSummary> {
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
			RuntimeMenuComponentSummary {
				component_index,
				menu_key: format!("external:{component_index}:{control_index}"),
				hierarchy_path: Some(format!(
					"{}/{}",
					asset_path.trim_matches('/'),
					label.clone().unwrap_or_else(|| format!("control:{control_index}"))
				)),
				sibling_index: Some(control_index),
				label,
				parameter_name: modular_avatar_external_menu_control_parameter(control),
				value: control
					.get("value")
					.or_else(|| control.get("Value"))
					.and_then(json_number_f64)
					.map(|value| value as f32),
			}
		})
		.collect()
}

fn modular_avatar_external_menu_control_parameter(control: &Value) -> Option<String> {
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

fn modular_avatar_ref_path(value: Option<&Value>) -> Option<String> {
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

fn modular_avatar_menu_graph_candidates(components: &[RuntimeMenuComponentSummary]) -> Vec<RuntimeMenuGraphCandidate> {
	let mut candidates = components
		.iter()
		.map(|component| RuntimeMenuGraphCandidate {
			component_index: component.component_index,
			menu_key: component.menu_key.clone(),
			label: component.label.clone(),
			hierarchy_path: component.hierarchy_path.clone(),
			sibling_index: component.sibling_index,
		})
		.collect::<Vec<_>>();
	candidates.sort_by(|a, b| {
		(
			a.hierarchy_path.as_deref().and_then(menu_parent_path).unwrap_or(""),
			a.sibling_index.unwrap_or(usize::MAX),
			a.component_index,
		)
			.cmp(&(
				b.hierarchy_path.as_deref().and_then(menu_parent_path).unwrap_or(""),
				b.sibling_index.unwrap_or(usize::MAX),
				b.component_index,
			))
	});
	candidates
}

fn modular_avatar_menu_graph_nodes(candidates: &[RuntimeMenuGraphCandidate]) -> Vec<RuntimeMenuGraphNode> {
	let path_to_node = candidates
		.iter()
		.enumerate()
		.filter_map(|(index, candidate)| candidate.hierarchy_path.as_ref().map(|path| (path.as_str(), index)))
		.collect::<BTreeMap<_, _>>();
	let mut nodes = candidates
		.iter()
		.enumerate()
		.map(|(_node_index, candidate)| {
			let parent_node_index = candidate
				.hierarchy_path
				.as_deref()
				.and_then(menu_parent_path)
				.and_then(|parent_path| path_to_node.get(parent_path).copied());
			RuntimeMenuGraphNode {
				menu_key: candidate.menu_key.clone(),
				label: candidate.label.clone(),
				hierarchy_path: candidate.hierarchy_path.clone(),
				parent_node_index,
			}
		})
		.collect::<Vec<_>>();
	for index in 0..nodes.len() {
		let Some(parent_node_index) = nodes[index].parent_node_index else {
			continue;
		};
		if parent_node_index >= nodes.len() {
			nodes[index].parent_node_index = None;
		}
	}
	nodes
}

fn menu_graph_node_display_label(node: &RuntimeMenuGraphNode) -> Option<String> {
	node.label.clone().or_else(|| {
		node.hierarchy_path
			.as_deref()
			.and_then(|path| path.trim_matches('/').rsplit('/').next())
			.filter(|label| !label.is_empty())
			.map(str::to_string)
	})
}

fn menu_graph_node_path(nodes: &[RuntimeMenuGraphNode], node_index: usize) -> RuntimeMenuGraphNodePath {
	let mut labels = Vec::new();
	let mut seen = BTreeSet::new();
	let mut current_index = Some(node_index);
	while let Some(index) = current_index {
		if index >= nodes.len() {
			labels.reverse();
			return RuntimeMenuGraphNodePath { labels, truncated: true };
		}
		if !seen.insert(index) {
			labels.reverse();
			return RuntimeMenuGraphNodePath { labels, truncated: true };
		}
		let node = &nodes[index];
		if let Some(label) = menu_graph_node_display_label(node) {
			labels.push(label);
		}
		current_index = node.parent_node_index;
	}
	labels.reverse();
	RuntimeMenuGraphNodePath { labels, truncated: false }
}

fn modular_avatar_component_fields(component: &Value) -> Option<&Value> {
	component.get("fields")
}

fn modular_avatar_component_string(component: &Value, names: &[&str]) -> Option<String> {
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

fn modular_avatar_component_ref<'a>(component: &'a Value, names: &[&str]) -> Option<&'a Value> {
	names.iter().find_map(|name| {
		modular_avatar_component_fields(component)
			.and_then(|fields| fields.get(*name))
			.or_else(|| component.get(*name))
	})
}

fn modular_avatar_component_usize(component: &Value, names: &[&str]) -> Option<usize> {
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

fn json_number_f64(value: &Value) -> Option<f64> {
	value.as_f64().or_else(|| value.as_i64().map(|value| value as f64))
}

fn modular_avatar_is_menu_metadata_type(short_type: &str) -> bool {
	matches!(
		short_type,
		"ModularAvatarMenuItem" | "ModularAvatarMenuGroup" | "ModularAvatarMenuInstaller" | "ModularAvatarMenuInstallTarget"
	)
}

fn menu_parent_path(path: &str) -> Option<&str> {
	let path = path.trim_matches('/');
	let (parent, _) = path.rsplit_once('/')?;
	(!parent.is_empty()).then_some(parent)
}

pub(crate) fn mesh_shader_variant_tier_for_limits(adapter_limits: &wgpu::Limits) -> MeshShaderVariantTier {
	if adapter_limits.max_sampled_textures_per_shader_stage >= HIGH_CAPABILITY_LILTOON_SAMPLED_TEXTURES_PER_STAGE
		&& adapter_limits.max_samplers_per_shader_stage >= HIGH_CAPABILITY_LILTOON_SAMPLERS_PER_STAGE
	{
		MeshShaderVariantTier::HighCapability
	} else {
		MeshShaderVariantTier::BaselineFallback
	}
}

pub(crate) fn mesh_shader_resource_plan_for_adapter(adapter_limits: &wgpu::Limits) -> MeshShaderResourcePlan {
	let tier = mesh_shader_variant_tier_for_limits(adapter_limits);
	let mut required_limits = wgpu::Limits::downlevel_defaults().using_resolution(adapter_limits.clone());
	required_limits.max_texture_dimension_2d = required_limits.max_texture_dimension_2d.max(4096);
	apply_mesh_shader_resource_limits(&mut required_limits, adapter_limits, tier);
	MeshShaderResourcePlan { tier, required_limits }
}

fn apply_mesh_shader_resource_limits(limits: &mut wgpu::Limits, adapter_limits: &wgpu::Limits, tier: MeshShaderVariantTier) {
	match tier {
		MeshShaderVariantTier::HighCapability => {
			limits.max_sampled_textures_per_shader_stage = limits
				.max_sampled_textures_per_shader_stage
				.max(HIGH_CAPABILITY_LILTOON_SAMPLED_TEXTURES_PER_STAGE)
				.min(adapter_limits.max_sampled_textures_per_shader_stage);
			limits.max_samplers_per_shader_stage = limits
				.max_samplers_per_shader_stage
				.max(HIGH_CAPABILITY_LILTOON_SAMPLERS_PER_STAGE)
				.min(adapter_limits.max_samplers_per_shader_stage);
		}
		MeshShaderVariantTier::BaselineFallback => {
			limits.max_sampled_textures_per_shader_stage = limits
				.max_sampled_textures_per_shader_stage
				.max(BASELINE_FALLBACK_SAMPLED_TEXTURES_PER_STAGE)
				.min(adapter_limits.max_sampled_textures_per_shader_stage);
			limits.max_samplers_per_shader_stage = limits
				.max_samplers_per_shader_stage
				.max(BASELINE_FALLBACK_SAMPLERS_PER_STAGE)
				.min(adapter_limits.max_samplers_per_shader_stage);
		}
	}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DynamicsRuntimeCounts {
	pub groups: u32,
	pub enabled_groups: u32,
	pub source_enabled_groups: u32,
	pub runtime_enabled_overrides: u32,
	pub vrm_spring_bone_groups: u32,
	pub vrc_physbone_groups: u32,
	pub unknown_groups: u32,
	pub limit_groups: u32,
	pub angle_limit_groups: u32,
	pub stretch_limit_groups: u32,
	pub grabbing_enabled_groups: u32,
	pub posing_enabled_groups: u32,
	pub colliders: u32,
	pub vrm_spring_bone_colliders: u32,
	pub vrc_physbone_colliders: u32,
	pub unknown_colliders: u32,
	pub contacts: u32,
	pub vrc_contact_senders: u32,
	pub vrc_contact_receivers: u32,
	pub contact_parameter_declarations: u32,
	pub constraint_refs: u32,
	pub vrc_constraint_refs: u32,
}

impl From<UnaRuntimeDynamicsCounts> for DynamicsRuntimeCounts {
	fn from(counts: UnaRuntimeDynamicsCounts) -> Self {
		Self {
			groups: counts.groups as u32,
			enabled_groups: counts.enabled_groups as u32,
			source_enabled_groups: counts.source_enabled_groups as u32,
			runtime_enabled_overrides: counts.runtime_enabled_overrides as u32,
			vrm_spring_bone_groups: counts.vrm_spring_bone_groups as u32,
			vrc_physbone_groups: counts.vrc_physbone_groups as u32,
			unknown_groups: counts.unknown_groups as u32,
			limit_groups: counts.limit_groups as u32,
			angle_limit_groups: counts.angle_limit_groups as u32,
			stretch_limit_groups: counts.stretch_limit_groups as u32,
			grabbing_enabled_groups: counts.grabbing_enabled_groups as u32,
			posing_enabled_groups: counts.posing_enabled_groups as u32,
			colliders: counts.colliders as u32,
			vrm_spring_bone_colliders: counts.vrm_spring_bone_colliders as u32,
			vrc_physbone_colliders: counts.vrc_physbone_colliders as u32,
			unknown_colliders: counts.unknown_colliders as u32,
			contacts: counts.contacts as u32,
			vrc_contact_senders: counts.vrc_contact_senders as u32,
			vrc_contact_receivers: counts.vrc_contact_receivers as u32,
			contact_parameter_declarations: counts.contact_parameter_declarations as u32,
			constraint_refs: counts.constraint_refs as u32,
			vrc_constraint_refs: counts.vrc_constraint_refs as u32,
		}
	}
}

fn unmotion_frame_hand_summary(frame: &un_motion_frame::UNMotionFrame, document: &UnaDocument) -> String {
	let body_bones = frame
		.body
		.as_ref()
		.and_then(|body| body.humanoid.as_ref())
		.map(|pose| pose.bones.len())
		.unwrap_or(0);
	let body_has = |bone| {
		frame.body.as_ref().and_then(|body| body.humanoid.as_ref()).is_some_and(|pose| {
			pose.bones
				.iter()
				.any(|sample| sample.bone == bone && sample.state != un_motion_frame::SampleState::Missing)
		})
	};
	let mut left_arm = String::new();
	append_present_labels(
		&mut left_arm,
		&[
			(body_has(un_motion_frame::HumanoidBone::LeftShoulder), "LS"),
			(body_has(un_motion_frame::HumanoidBone::LeftUpperArm), "LU"),
			(body_has(un_motion_frame::HumanoidBone::LeftLowerArm), "LL"),
			(body_has(un_motion_frame::HumanoidBone::LeftHand), "LH"),
		],
	);
	let mut right_arm = String::new();
	append_present_labels(
		&mut right_arm,
		&[
			(body_has(un_motion_frame::HumanoidBone::RightShoulder), "RS"),
			(body_has(un_motion_frame::HumanoidBone::RightUpperArm), "RU"),
			(body_has(un_motion_frame::HumanoidBone::RightLowerArm), "RL"),
			(body_has(un_motion_frame::HumanoidBone::RightHand), "RH"),
		],
	);
	let left_fingers = frame.left_hand.as_ref().map(|h| h.fingers.len()).unwrap_or(0);
	let right_fingers = frame.right_hand.as_ref().map(|h| h.fingers.len()).unwrap_or(0);
	let left_joints = frame
		.left_hand
		.as_ref()
		.map(|h| h.fingers.iter().map(|f| f.joints.len()).sum::<usize>())
		.unwrap_or(0);
	let right_joints = frame
		.right_hand
		.as_ref()
		.map(|h| h.fingers.iter().map(|f| f.joints.len()).sum::<usize>())
		.unwrap_or(0);
	let matched_finger_keys = document
		.humanoid_profile
		.as_ref()
		.map(|profile| {
			profile
				.bone_node_indices
				.keys()
				.filter(|key| {
					let normalized: String = key
						.chars()
						.filter(|ch| ch.is_ascii_alphanumeric())
						.map(|ch| ch.to_ascii_lowercase())
						.collect();
					let side = normalized.starts_with("left") || normalized.starts_with("right");
					let finger = ["thumb", "index", "middle", "ring", "little"]
						.iter()
						.any(|part| normalized.contains(part));
					let segment = ["proximal", "intermediate", "distal"].iter().any(|part| normalized.contains(part));
					side && finger && segment
				})
				.count()
		})
		.unwrap_or(0);
	let (finger_targets, matched_finger_targets) = document
		.humanoid_profile
		.as_ref()
		.map(|profile| {
			let left = count_hand_finger_target_matches(profile, frame.left_hand.as_ref(), "left");
			let right = count_hand_finger_target_matches(profile, frame.right_hand.as_ref(), "right");
			(left.0 + right.0, left.1 + right.1)
		})
		.unwrap_or((0, 0));
	format!(
		"space={:?} body_bones={body_bones} left_arm={left_arm} right_arm={right_arm} left_fingers={left_fingers} right_fingers={right_fingers} left_joints={left_joints} right_joints={right_joints} profile_finger_keys={matched_finger_keys} finger_targets={finger_targets} matched_finger_targets={matched_finger_targets}",
		frame.header.coordinate_space
	)
}

fn append_present_labels(out: &mut String, labels: &[(bool, &str)]) {
	for &(present, label) in labels {
		if !present {
			continue;
		}
		if !out.is_empty() {
			out.push(',');
		}
		out.push_str(label);
	}
}

fn format_top_expression_weights(weights: &std::collections::BTreeMap<String, f32>, limit: usize) -> String {
	let mut top: Vec<(&str, f32)> = Vec::with_capacity(limit.min(weights.len()));
	for (key, &weight) in weights {
		let abs_weight = weight.abs();
		let insert_at = top
			.iter()
			.position(|&(_, existing)| abs_weight > existing.abs())
			.unwrap_or(top.len());
		if insert_at < limit {
			top.insert(insert_at, (key.as_str(), weight));
			top.truncate(limit);
		}
	}
	let mut out = String::new();
	for (key, weight) in top {
		if !out.is_empty() {
			out.push_str(", ");
		}
		out.push_str(key);
		out.push('=');
		let _ = write!(out, "{weight:.3}");
	}
	out
}

fn expression_presets_match_catalog(current: &[String], catalog: Option<&UnaExpressionCatalog>) -> bool {
	let Some(catalog) = catalog else {
		return current.is_empty();
	};
	current.len() == catalog.presets.len()
		&& current
			.iter()
			.zip(&catalog.presets)
			.all(|(current, preset)| current == &preset.name)
}

fn expression_preset_names(catalog: Option<&UnaExpressionCatalog>) -> Vec<String> {
	let Some(catalog) = catalog else {
		return Vec::new();
	};
	let mut names = Vec::with_capacity(catalog.presets.len());
	names.extend(catalog.presets.iter().map(|preset| preset.name.clone()));
	names
}

fn humanoid_profile_keys_csv(profile: Option<&un_avatar_skeleton::HumanoidProfile>) -> String {
	let Some(profile) = profile else {
		return String::new();
	};
	let capacity = profile
		.bone_node_indices
		.keys()
		.map(String::len)
		.sum::<usize>()
		.saturating_add(profile.bone_node_indices.len().saturating_sub(1));
	let mut keys = String::with_capacity(capacity);
	for key in profile.bone_node_indices.keys() {
		if !keys.is_empty() {
			keys.push(',');
		}
		keys.push_str(key);
	}
	keys
}

fn active_expression_weights_for_doc(disable_expression_morphs: bool, doc: &UnaDocument) -> Option<&un_avatar_core::UnaExpressionWeights> {
	if disable_expression_morphs {
		None
	} else {
		doc.runtime_model()
			.expression_weights()
			.filter(|weights| !weights.preset_weights.is_empty())
	}
}

fn active_expression_overrides<'a>(
	disable_expression_morphs: bool,
	overrides: &'a std::collections::BTreeMap<String, f32>,
) -> Option<&'a std::collections::BTreeMap<String, f32>> {
	if disable_expression_morphs || overrides.is_empty() {
		None
	} else {
		Some(overrides)
	}
}

fn count_hand_finger_target_matches(
	profile: &un_avatar_skeleton::HumanoidProfile,
	hand: Option<&un_motion_frame::HandMotion>,
	side_prefix: &str,
) -> (usize, usize) {
	let Some(hand) = hand else {
		return (0, 0);
	};
	let mut targets = 0usize;
	let mut matched = 0usize;
	let mut key = String::with_capacity("rightintermediatedistal".len());
	for finger in &hand.fingers {
		let finger_key = match finger.finger {
			un_motion_frame::Finger::Thumb => "thumb",
			un_motion_frame::Finger::Index => "index",
			un_motion_frame::Finger::Middle => "middle",
			un_motion_frame::Finger::Ring => "ring",
			un_motion_frame::Finger::Little => "little",
		};
		for (index, _) in finger.joints.iter().enumerate() {
			let segment = match index {
				0 => "proximal",
				1 => "intermediate",
				2 => "distal",
				_ => continue,
			};
			key.clear();
			key.push_str(side_prefix);
			key.push_str(finger_key);
			key.push_str(segment);
			targets += 1;
			if profile_has_key(profile, &key) {
				matched += 1;
			}
		}
	}
	(targets, matched)
}

fn profile_has_key(profile: &un_avatar_skeleton::HumanoidProfile, key: &str) -> bool {
	profile.bone_node_indices.contains_key(key) || {
		let target = normalize_profile_match_key(key);
		profile
			.bone_node_indices
			.keys()
			.any(|candidate| normalize_profile_match_key(candidate) == target)
	}
}

fn normalize_profile_match_key(name: &str) -> String {
	let mut normalized = String::with_capacity(name.len());
	normalized.extend(
		name.chars()
			.filter(|ch| ch.is_ascii_alphanumeric())
			.map(|ch| ch.to_ascii_lowercase()),
	);
	normalized
}

/// GPU とシェーダに渡すグローバル（WGSL `Globals` と一致。末尾パディングで 256 バイトに揃える）。
#[repr(C)]
#[derive(Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct GlobalsGpu {
	pub(crate) view_proj: [[f32; 4]; 4],
	pub(crate) inv_view_proj: [[f32; 4]; 4],
	pub(crate) light_dir: [f32; 4],
	pub(crate) camera_pos: [f32; 4],
	_pad: [u8; 96],
}

const _: () = assert!(std::mem::size_of::<GlobalsGpu>() == 256);

#[repr(C)]
#[derive(Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct StartupSplashGpu {
	time: f32,
	progress: f32,
	aspect: f32,
	phase: f32,
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct ContactShadowGpu {
	params: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct DebugLineVertex {
	position: [f32; 3],
	color: [f32; 4],
}

pub(crate) struct StartupSplashFrame {
	pub(crate) time_secs: f32,
	pub(crate) progress: f32,
	pub(crate) phase: f32,
}

pub(crate) struct DocumentAttachOptions {
	pub(crate) mesh_diagnostics: SceneMeshLoadOpts,
	pub(crate) texture_max_dimension: Option<u32>,
	pub(crate) texture_compression: TextureCompressionMode,
	pub(crate) block_compression_encoder: crate::options::BlockCompressionEncoder,
	pub(crate) block_compression_cpu_threads: usize,
	pub(crate) mipmap_filter: crate::options::TextureMipmapFilter,
	pub(crate) texture_compression_advanced: TextureCompressionAdvancedOptions,
	pub(crate) texture_compression_bc_supported: bool,
	pub(crate) texture_compression_astc_supported: bool,
	pub(crate) texture_compression_etc2_supported: bool,
	pub(crate) processed_texture_cache: bool,
	pub(crate) enable_spring_bones: bool,
	pub(crate) bone_colliders: BoneColliderConfig,
	pub(crate) spring_bone_physics: DynamicsPhysicsConfig,
	pub(crate) debug_material_dump: bool,
	pub(crate) vmc_address: Option<SocketAddr>,
	pub(crate) unmotion_zenoh: crate::options::UnmotionZenohOptions,
	pub(crate) audio_link: AudioLinkOptions,
	pub(crate) debug_vmc: bool,
}

pub(crate) struct PreparedDocumentScene {
	document: Arc<RwLock<UnaDocument>>,
	rest_nodes: Option<Arc<Vec<UnaSceneNode>>>,
	scene_meshes: Option<SceneMeshes>,
	texture_summary: Option<TextureUploadSummary>,
	dynamics_sim: Option<DynamicsSimulator>,
	bone_colliders: Vec<BoneColliderPrimitive>,
	bone_collider_count: u32,
	bone_collider_source: BoneColliderSource,
	runtime_requirements: SceneMeshRuntimeRequirements,
	expression_presets: Vec<String>,
}

struct MotionRetargetRuntime {
	rest_nodes: Arc<Vec<UnaSceneNode>>,
	context: un_avatar_skeleton::HumanoidRetargetContext,
}

impl MotionRetargetRuntime {
	fn for_document(document: &UnaDocument, rest_nodes: Arc<Vec<UnaSceneNode>>) -> Option<Self> {
		let runtime_model = document.runtime_model();
		if !runtime_model.has_humanoid_scene() {
			return None;
		}
		let context = un_avatar_skeleton::HumanoidRetargetContext::for_runtime_inputs(
			runtime_model.humanoid_retarget_inputs(),
			Some(rest_nodes.as_slice()),
		);
		Some(Self { rest_nodes, context })
	}

	fn apply_frame(
		&self,
		document: &mut UnaDocument,
		frame: &un_motion_frame::UNMotionFrame,
		opts: un_avatar_skeleton::ApplyUnMotionFrameOpts,
	) {
		un_avatar_skeleton::apply_un_motion_frame_to_document_with_context(
			document,
			frame,
			opts,
			Some(self.rest_nodes.as_slice()),
			&self.context,
		);
	}
}

struct RuntimePhysicsBuild {
	dynamics_sim: Option<DynamicsSimulator>,
	debug_bone_colliders: Vec<BoneColliderPrimitive>,
	stats: BoneColliderStats,
}

fn build_runtime_physics_for_document(
	document: &UnaDocument,
	enable_spring_bones: bool,
	bone_collider_config: BoneColliderConfig,
	spring_bone_physics: &DynamicsPhysicsConfig,
) -> RuntimePhysicsBuild {
	let runtime_model = document.runtime_model();
	let scene_profile_dynamics = runtime_model.scene_profile_dynamics();
	let bone_colliders = if let Some(runtime) = scene_profile_dynamics {
		build_dynamics_bone_colliders(runtime.scene, runtime.humanoid_profile, bone_collider_config, runtime.dynamics)
	} else {
		Vec::new()
	};
	let stats = collider_stats(&bone_colliders);
	let dynamics_sim = if enable_spring_bones {
		if let Some(runtime) = scene_profile_dynamics {
			if runtime.dynamics.has_groups() {
				DynamicsSimulator::new_with_runtime_dynamics(
					runtime.scene,
					runtime.dynamics,
					bone_colliders.clone(),
					spring_bone_physics.clone(),
				)
			} else {
				None
			}
		} else {
			None
		}
	} else {
		None
	};
	let debug_bone_colliders = if dynamics_sim.is_some() { Vec::new() } else { bone_colliders };
	RuntimePhysicsBuild {
		dynamics_sim,
		debug_bone_colliders,
		stats,
	}
}

fn reset_runtime_dynamics_nodes_to_rest(
	scene: &mut un_avatar_core::UnaSceneSnapshot,
	dynamics: un_avatar_core::UnaRuntimeDynamics<'_>,
	rest_nodes: &[UnaSceneNode],
) -> bool {
	if !dynamics.has_groups() {
		return false;
	}
	let mut changed = false;
	for node_index in dynamics.reset_node_indices() {
		if let (Some(dst), Some(src)) = (scene.nodes.get_mut(node_index), rest_nodes.get(node_index)) {
			dst.transform = src.transform;
			changed = true;
		}
	}
	changed
}

fn reset_runtime_dynamics_nodes_to_rest_for_source_id(
	scene: &mut un_avatar_core::UnaSceneSnapshot,
	dynamics: un_avatar_core::UnaRuntimeDynamics<'_>,
	rest_nodes: &[UnaSceneNode],
	source_id: &str,
) -> bool {
	let mut changed = false;
	for node_index in dynamics.reset_node_indices_for_source_id(source_id) {
		if let (Some(dst), Some(src)) = (scene.nodes.get_mut(node_index), rest_nodes.get(node_index)) {
			dst.transform = src.transform;
			changed = true;
		}
	}
	changed
}

pub(crate) struct GpuSceneBuildContext {
	device: wgpu::Device,
	queue: wgpu::Queue,
	format: wgpu::TextureFormat,
	aa: AaMode,
	shader_variant_tier: MeshShaderVariantTier,
}

/// `Mat4::perspective_rh` 用の縦方向 FOV（ラジアン）を、対角画角と幅÷高さから求める。
/// 対角画角の既定値はフルサイズ換算 35mm レンズ相当（36×24mm センサーの対角と焦点距離 35mm から
/// `2 * atan(sqrt(36² + 24²) / (2 * 35)) ≈ 63.45°`）を `crate::camera::DEFAULT_DIAGONAL_FOV_DEG` に置く。
fn vertical_fov_from_diagonal(diagonal_rad: f32, aspect_wh: f32) -> f32 {
	let t = (diagonal_rad * 0.5).tan();
	2.0 * (t / (1.0 + aspect_wh * aspect_wh).sqrt()).atan()
}

/// 1 フレームあたりの計測（壁時計間隔・CPU 記録時間・GPU メインパス時間）。
///
/// `gpu_ms` は `Features::TIMESTAMP_QUERY` 対応 GPU では真の GPU 時間（メインパスの開始から終了まで）。
/// 非対応 GPU では 0 を返す。CPU は `desired_maximum_frame_latency` と present_mode で律速されるため
/// 旧実装のブロッキング `device.poll(wait_indefinitely)` は不要。
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameTimings {
	pub wall_since_last_ms: f32,
	pub cpu_record_ms: f32,
	pub gpu_ms: f32,
}

const TS_RING_LEN: usize = 2;
const TS_BYTES_PER_FRAME: u64 = 16;

const TS_STATE_IDLE: u8 = 0;
const TS_STATE_PENDING: u8 = 1;
const TS_STATE_READY: u8 = 2;

fn instance_descriptor_for_backend(backend: RenderBackend) -> wgpu::InstanceDescriptor {
	let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
	descriptor.backends = match backend {
		RenderBackend::Auto => wgpu::Backends::all(),
		RenderBackend::Vulkan => wgpu::Backends::VULKAN,
		RenderBackend::Dx12 => wgpu::Backends::DX12,
	};
	#[cfg(windows)]
	if backend == RenderBackend::Dx12 {
		// DX12 HWND swapchains expose only Opaque alpha; DirectComposition visual swapchains expose real alpha modes.
		descriptor.backend_options.dx12.presentation_system = wgpu::Dx12SwapchainKind::DxgiFromVisual;
	}
	descriptor
}

fn effective_window_backend(backend: RenderBackend) -> RenderBackend {
	#[cfg(windows)]
	{
		// Windows Vulkan HWND surfaces commonly expose only Opaque alpha. The renderer
		// supports runtime transparency toggles, so prefer the DX12 DirectComposition path.
		if backend == RenderBackend::Vulkan {
			return RenderBackend::Dx12;
		}
	}
	backend
}

struct TimestampRingSlot {
	buf: wgpu::Buffer,
	state: Arc<AtomicU8>,
}

/// GPU タイムスタンプによるメインパス計測。`TIMESTAMP_QUERY` 対応時のみ生成される。
struct GpuTimestamps {
	qset: wgpu::QuerySet,
	resolve_buf: wgpu::Buffer,
	rings: [TimestampRingSlot; TS_RING_LEN],
	period_ns: f32,
	write_idx: usize,
	last_gpu_ms: Option<f32>,
}

impl GpuTimestamps {
	fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
		let qset = device.create_query_set(&wgpu::QuerySetDescriptor {
			label: Some("frame-ts"),
			ty: wgpu::QueryType::Timestamp,
			count: 2,
		});
		let resolve_buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("frame-ts-resolve"),
			size: TS_BYTES_PER_FRAME,
			usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
			mapped_at_creation: false,
		});
		let mut rings: [Option<TimestampRingSlot>; TS_RING_LEN] = [None, None];
		for slot in &mut rings {
			*slot = Some(TimestampRingSlot {
				buf: device.create_buffer(&wgpu::BufferDescriptor {
					label: Some("frame-ts-readback"),
					size: TS_BYTES_PER_FRAME,
					usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
					mapped_at_creation: false,
				}),
				state: Arc::new(AtomicU8::new(TS_STATE_IDLE)),
			});
		}
		Self {
			qset,
			resolve_buf,
			rings: [rings[0].take().unwrap(), rings[1].take().unwrap()],
			period_ns: queue.get_timestamp_period(),
			write_idx: 0,
			last_gpu_ms: None,
		}
	}

	/// 完了済みのリングスロットを読み出し、`last_gpu_ms` を更新する。呼び出し側で `device.poll(Poll)` を済ませておくこと。
	fn drain_ready(&mut self) {
		for slot in &self.rings {
			if slot.state.load(Ordering::Acquire) == TS_STATE_READY {
				let raw = {
					let view = slot.buf.slice(..).get_mapped_range();
					let pair: [u64; 2] = bytemuck::pod_read_unaligned(&view[..16]);
					pair
				};
				slot.buf.unmap();
				slot.state.store(TS_STATE_IDLE, Ordering::Release);
				let diff = raw[1].saturating_sub(raw[0]);
				let ms = (diff as f64 * self.period_ns as f64) / 1_000_000.0;
				self.last_gpu_ms = Some(ms as f32);
			}
		}
	}

	/// 今フレームで書き込めるスロットがあれば、メインパスに渡す timestamp_writes と書き込みインデックスを返す。
	fn begin_pass(&self) -> Option<(wgpu::RenderPassTimestampWrites<'_>, usize)> {
		let idx = self.write_idx;
		if self.rings[idx].state.load(Ordering::Acquire) != TS_STATE_IDLE {
			return None;
		}
		Some((
			wgpu::RenderPassTimestampWrites {
				query_set: &self.qset,
				beginning_of_pass_write_index: Some(0),
				end_of_pass_write_index: Some(1),
			},
			idx,
		))
	}

	fn encode_resolve(&self, encoder: &mut wgpu::CommandEncoder, idx: usize) {
		encoder.resolve_query_set(&self.qset, 0..2, &self.resolve_buf, 0);
		encoder.copy_buffer_to_buffer(&self.resolve_buf, 0, &self.rings[idx].buf, 0, TS_BYTES_PER_FRAME);
	}

	fn after_submit(&mut self, idx: usize) {
		let cb_state = Arc::clone(&self.rings[idx].state);
		cb_state.store(TS_STATE_PENDING, Ordering::Release);
		self.rings[idx].buf.slice(..).map_async(wgpu::MapMode::Read, move |result| {
			if result.is_ok() {
				cb_state.store(TS_STATE_READY, Ordering::Release);
			} else {
				cb_state.store(TS_STATE_IDLE, Ordering::Release);
			}
		});
		self.write_idx = (self.write_idx + 1) % TS_RING_LEN;
	}

	fn last_gpu_ms(&self) -> Option<f32> {
		self.last_gpu_ms
	}
}

/// `primary_motion_source` 共有 atomic で使う数値表現。
///
/// `crate::options::PrimaryMotionSource` の repr ではなく、別途固定値を用意することで
/// `PrimaryMotionSource` 側に repr を強制しなくて済む (Serialize の互換も保ちやすい)。
const PRIMARY_VMC: u8 = 0;
const PRIMARY_UNMOTION_ZENOH: u8 = 1;

fn primary_motion_source_to_u8(source: crate::options::PrimaryMotionSource) -> u8 {
	match source {
		crate::options::PrimaryMotionSource::Vmc => PRIMARY_VMC,
		crate::options::PrimaryMotionSource::UnmotionZenoh => PRIMARY_UNMOTION_ZENOH,
	}
}

fn primary_motion_source_from_u8(value: u8) -> crate::options::PrimaryMotionSource {
	match value {
		PRIMARY_UNMOTION_ZENOH => crate::options::PrimaryMotionSource::UnmotionZenoh,
		_ => crate::options::PrimaryMotionSource::Vmc,
	}
}

#[derive(Default)]
struct MotionControlBuffer {
	state: Mutex<MotionControlBufferState>,
}

impl MotionControlBuffer {
	fn push_frame(&self, frame: un_motion_frame::UNMotionFrame) {
		if let Ok(mut state) = self.state.lock() {
			let write_idx = state.write_idx;
			state.buffers[write_idx].push_frame(frame);
		}
	}

	fn take_pending_frames_into(&self, out: &mut Vec<un_motion_frame::UNMotionFrame>) {
		out.clear();
		let Ok(mut state) = self.state.lock() else {
			return;
		};
		let read_idx = state.write_idx;
		let next_write_idx = 1 - read_idx;
		state.write_idx = next_write_idx;
		state.buffers[next_write_idx].clear();
		state.buffers[read_idx].take_frames_into(out);
	}
}

#[derive(Default)]
struct MotionControlBufferState {
	write_idx: usize,
	buffers: [MotionFrameAccumulator; 2],
}

#[derive(Default)]
struct MotionFrameAccumulator {
	buckets: Vec<MotionFrameBucket>,
	sequence: u64,
	has_pending: bool,
}

impl MotionFrameAccumulator {
	fn clear(&mut self) {
		for bucket in &mut self.buckets {
			bucket.clear_samples();
		}
		self.has_pending = false;
	}

	fn push_frame(&mut self, frame: un_motion_frame::UNMotionFrame) {
		let bucket = self.bucket_for_frame(&frame);
		bucket.merge_frame(frame);
		self.has_pending = true;
	}

	fn take_frames_into(&mut self, frames: &mut Vec<un_motion_frame::UNMotionFrame>) {
		if !self.has_pending || self.buckets.is_empty() {
			return;
		}
		frames.reserve(self.buckets.len());
		for bucket in &mut self.buckets {
			self.sequence = self.sequence.wrapping_add(1);
			if let Some(frame) = bucket.take_frame(self.sequence) {
				frames.push(frame);
			}
		}
		self.has_pending = false;
	}

	fn bucket_for_frame(&mut self, frame: &un_motion_frame::UNMotionFrame) -> &mut MotionFrameBucket {
		if let Some(index) = self.buckets.iter().position(|bucket| bucket.matches(frame)) {
			return &mut self.buckets[index];
		}
		self.buckets.push(MotionFrameBucket::from_frame_space(frame));
		self.buckets.last_mut().expect("bucket just pushed")
	}
}

struct MotionFrameBucket {
	header: un_motion_frame::MotionHeader,
	sources: Vec<un_motion_frame::MotionSourceInfo>,
	metadata: un_motion_frame::MotionMetadata,
	body_tracking_state: un_motion_frame::TrackingState,
	body_confidence: f32,
	body_root: Option<un_motion_frame::TransformSample>,
	body_bones: Vec<un_motion_frame::BoneSample>,
	face_tracking_state: un_motion_frame::TrackingState,
	face_confidence: f32,
	face_head: Option<un_motion_frame::TransformSample>,
	expressions: Vec<un_motion_frame::ExpressionSample>,
	eyes: Option<un_motion_frame::EyeMotion>,
	left_tracking_state: un_motion_frame::TrackingState,
	left_confidence: f32,
	left_wrist: Option<un_motion_frame::TransformSample>,
	left_fingers: Vec<un_motion_frame::FingerPose>,
	right_tracking_state: un_motion_frame::TrackingState,
	right_confidence: f32,
	right_wrist: Option<un_motion_frame::TransformSample>,
	right_fingers: Vec<un_motion_frame::FingerPose>,
	signals: Vec<un_motion_frame::MotionSignal>,
}

impl MotionFrameBucket {
	fn from_frame_space(frame: &un_motion_frame::UNMotionFrame) -> Self {
		let body_bone_capacity = frame
			.body
			.as_ref()
			.and_then(|body| body.humanoid.as_ref())
			.map_or(0, |humanoid| humanoid.bones.len());
		let expression_capacity = frame.face.as_ref().map_or(0, |face| face.expressions.len());
		let left_finger_capacity = frame.left_hand.as_ref().map_or(0, |hand| hand.fingers.len());
		let right_finger_capacity = frame.right_hand.as_ref().map_or(0, |hand| hand.fingers.len());
		Self {
			header: un_motion_frame::MotionHeader::new(0),
			sources: Vec::with_capacity(frame.sources.len()),
			metadata: un_motion_frame::MotionMetadata::default(),
			body_tracking_state: un_motion_frame::TrackingState::Unknown,
			body_confidence: 0.0,
			body_root: None,
			body_bones: Vec::with_capacity(body_bone_capacity),
			face_tracking_state: un_motion_frame::TrackingState::Unknown,
			face_confidence: 0.0,
			face_head: None,
			expressions: Vec::with_capacity(expression_capacity),
			eyes: None,
			left_tracking_state: un_motion_frame::TrackingState::Unknown,
			left_confidence: 0.0,
			left_wrist: None,
			left_fingers: Vec::with_capacity(left_finger_capacity),
			right_tracking_state: un_motion_frame::TrackingState::Unknown,
			right_confidence: 0.0,
			right_wrist: None,
			right_fingers: Vec::with_capacity(right_finger_capacity),
			signals: Vec::with_capacity(frame.signals.len()),
		}
	}

	fn matches(&self, frame: &un_motion_frame::UNMotionFrame) -> bool {
		self.header.coordinate_space == frame.header.coordinate_space
			&& self.header.handedness == frame.header.handedness
			&& self.header.length_unit == frame.header.length_unit
	}

	fn merge_frame(&mut self, frame: un_motion_frame::UNMotionFrame) {
		self.header = frame.header;
		self.metadata = frame.metadata;
		self.sources.extend(frame.sources);
		if let Some(body) = frame.body {
			self.body_tracking_state = body.tracking_state;
			self.body_confidence = body.confidence;
			if let Some(humanoid) = body.humanoid {
				if humanoid.root.is_some() {
					self.body_root = humanoid.root;
				}
				for bone in humanoid.bones {
					upsert_bone_sample(&mut self.body_bones, bone);
				}
			}
		}
		if let Some(face) = frame.face {
			self.face_tracking_state = face.tracking_state;
			self.face_confidence = face.confidence;
			if face.head.is_some() {
				self.face_head = face.head;
			}
			for expression in face.expressions {
				upsert_expression_sample(&mut self.expressions, expression);
			}
		}
		if let Some(eyes) = frame.eyes {
			self.eyes = Some(eyes);
		}
		if let Some(hand) = frame.left_hand {
			self.left_tracking_state = hand.tracking_state;
			self.left_confidence = hand.confidence;
			if hand.wrist.is_some() {
				self.left_wrist = hand.wrist;
			}
			for finger in hand.fingers {
				upsert_finger_pose(&mut self.left_fingers, finger);
			}
		}
		if let Some(hand) = frame.right_hand {
			self.right_tracking_state = hand.tracking_state;
			self.right_confidence = hand.confidence;
			if hand.wrist.is_some() {
				self.right_wrist = hand.wrist;
			}
			for finger in hand.fingers {
				upsert_finger_pose(&mut self.right_fingers, finger);
			}
		}
		for signal in frame.signals {
			upsert_motion_signal(&mut self.signals, signal);
		}
	}

	fn clear_samples(&mut self) {
		self.sources.clear();
		self.metadata = un_motion_frame::MotionMetadata::default();
		self.body_tracking_state = un_motion_frame::TrackingState::Unknown;
		self.body_confidence = 0.0;
		self.body_root = None;
		self.body_bones.clear();
		self.face_tracking_state = un_motion_frame::TrackingState::Unknown;
		self.face_confidence = 0.0;
		self.face_head = None;
		self.expressions.clear();
		self.eyes = None;
		self.left_tracking_state = un_motion_frame::TrackingState::Unknown;
		self.left_confidence = 0.0;
		self.left_wrist = None;
		self.left_fingers.clear();
		self.right_tracking_state = un_motion_frame::TrackingState::Unknown;
		self.right_confidence = 0.0;
		self.right_wrist = None;
		self.right_fingers.clear();
		self.signals.clear();
	}

	fn take_frame(&mut self, sequence: u64) -> Option<un_motion_frame::UNMotionFrame> {
		let mut frame = un_motion_frame::UNMotionFrame::new(sequence);
		self.header.sequence = sequence;
		frame.header = self.header.clone();
		frame.sources.extend(self.sources.drain(..));
		frame.metadata = std::mem::take(&mut self.metadata);
		if self.body_root.is_some() || !self.body_bones.is_empty() {
			frame.body = Some(un_motion_frame::BodyMotion {
				tracking_state: self.body_tracking_state,
				confidence: self.body_confidence,
				humanoid: Some(un_motion_frame::HumanoidPose {
					root: self.body_root.take(),
					bones: self.body_bones.drain(..).collect(),
				}),
			});
		}
		if self.face_head.is_some() || !self.expressions.is_empty() {
			frame.face = Some(un_motion_frame::FaceMotion {
				tracking_state: self.face_tracking_state,
				confidence: self.face_confidence,
				head: self.face_head.take(),
				expressions: self.expressions.drain(..).collect(),
			});
		}
		frame.eyes = self.eyes.take();
		if self.left_wrist.is_some() || !self.left_fingers.is_empty() {
			frame.left_hand = Some(un_motion_frame::HandMotion {
				tracking_state: self.left_tracking_state,
				confidence: self.left_confidence,
				wrist: self.left_wrist.take(),
				fingers: self.left_fingers.drain(..).collect(),
			});
		}
		if self.right_wrist.is_some() || !self.right_fingers.is_empty() {
			frame.right_hand = Some(un_motion_frame::HandMotion {
				tracking_state: self.right_tracking_state,
				confidence: self.right_confidence,
				wrist: self.right_wrist.take(),
				fingers: self.right_fingers.drain(..).collect(),
			});
		}
		frame.signals.extend(self.signals.drain(..));
		if frame.body.is_none()
			&& frame.face.is_none()
			&& frame.eyes.is_none()
			&& frame.left_hand.is_none()
			&& frame.right_hand.is_none()
			&& frame.signals.is_empty()
		{
			return None;
		}
		Some(frame)
	}
}

fn upsert_finger_pose(fingers: &mut Vec<un_motion_frame::FingerPose>, next: un_motion_frame::FingerPose) {
	if let Some(existing) = fingers.iter_mut().find(|finger| finger.finger == next.finger) {
		*existing = next;
	} else {
		fingers.push(next);
	}
}

fn upsert_bone_sample(bones: &mut Vec<un_motion_frame::BoneSample>, next: un_motion_frame::BoneSample) {
	if let Some(existing) = bones.iter_mut().find(|bone| bone.bone == next.bone) {
		*existing = next;
	} else {
		bones.push(next);
	}
}

fn upsert_expression_sample(expressions: &mut Vec<un_motion_frame::ExpressionSample>, next: un_motion_frame::ExpressionSample) {
	if let Some(existing) = expressions.iter_mut().find(|expression| expression.name == next.name) {
		*existing = next;
	} else {
		expressions.push(next);
	}
}

fn upsert_motion_signal(signals: &mut Vec<un_motion_frame::MotionSignal>, next: un_motion_frame::MotionSignal) {
	if let Some(existing) = signals.iter_mut().find(|signal| signal.name == next.name) {
		*existing = next;
	} else {
		signals.push(next);
	}
}

#[cfg(test)]
mod motion_buffer_tests {
	use super::*;

	fn expression_frame(sequence: u64, name: &str, value: f32) -> un_motion_frame::UNMotionFrame {
		let mut frame = un_motion_frame::UNMotionFrame::new(sequence);
		frame.header.coordinate_space = un_motion_frame::CoordinateSpace::UNMotion;
		frame.face = Some(un_motion_frame::FaceMotion {
			tracking_state: un_motion_frame::TrackingState::Valid,
			confidence: 1.0,
			head: None,
			expressions: vec![un_motion_frame::ExpressionSample {
				name: name.to_string(),
				value,
				confidence: 1.0,
				source_index: None,
				state: un_motion_frame::SampleState::Valid,
			}],
		});
		frame
	}

	fn test_node(transform: [f32; 16]) -> UnaSceneNode {
		UnaSceneNode {
			name: None,
			source_node_id: None,
			resolved_node_id: None,
			visible: true,
			transform,
			children: Vec::new(),
			mesh: None,
			skin: None,
			probe_anchor_node: None,
			local_bounds: None,
		}
	}

	#[test]
	fn motion_buffer_keeps_latest_value_per_key_until_frame_read() {
		let buffer = MotionControlBuffer::default();
		buffer.push_frame(expression_frame(1, "Joy", 0.25));
		buffer.push_frame(expression_frame(2, "Joy", 0.75));

		let mut frames = Vec::new();
		buffer.take_pending_frames_into(&mut frames);
		assert_eq!(frames.len(), 1);
		let expressions = &frames[0].face.as_ref().unwrap().expressions;
		assert_eq!(expressions.len(), 1);
		assert_eq!(expressions[0].name, "Joy");
		assert!((expressions[0].value - 0.75).abs() < f32::EPSILON);
		buffer.take_pending_frames_into(&mut frames);
		assert!(frames.is_empty());
	}

	#[test]
	fn motion_buffer_switches_write_side_after_read() {
		let buffer = MotionControlBuffer::default();
		let mut frames = Vec::new();
		buffer.push_frame(expression_frame(1, "Joy", 0.25));
		buffer.take_pending_frames_into(&mut frames);
		assert_eq!(frames.len(), 1);

		buffer.push_frame(expression_frame(2, "Angry", 0.5));
		buffer.take_pending_frames_into(&mut frames);
		assert_eq!(frames.len(), 1);
		let expressions = &frames[0].face.as_ref().unwrap().expressions;
		assert_eq!(expressions.len(), 1);
		assert_eq!(expressions[0].name, "Angry");
	}

	#[test]
	fn motion_frame_accumulator_reuses_buckets_after_read() {
		let mut accumulator = MotionFrameAccumulator::default();
		let mut frames = Vec::new();

		accumulator.push_frame(expression_frame(1, "Joy", 0.25));
		accumulator.take_frames_into(&mut frames);
		assert_eq!(frames.len(), 1);
		assert_eq!(accumulator.buckets.len(), 1);

		frames.clear();
		accumulator.push_frame(expression_frame(2, "Angry", 0.5));
		accumulator.take_frames_into(&mut frames);
		assert_eq!(frames.len(), 1);
		assert_eq!(accumulator.buckets.len(), 1);
		let expressions = &frames[0].face.as_ref().unwrap().expressions;
		assert_eq!(expressions.len(), 1);
		assert_eq!(expressions[0].name, "Angry");
	}

	#[test]
	fn motion_frame_accumulator_skips_empty_reused_buckets() {
		let mut accumulator = MotionFrameAccumulator::default();
		let mut frames = Vec::new();

		accumulator.push_frame(expression_frame(1, "Joy", 0.25));
		accumulator.take_frames_into(&mut frames);
		let sequence_after_frame = accumulator.sequence;
		frames.clear();

		accumulator.take_frames_into(&mut frames);

		assert!(frames.is_empty());
		assert_eq!(accumulator.sequence, sequence_after_frame);
		assert_eq!(accumulator.buckets.len(), 1);
	}

	#[test]
	fn debug_expression_weight_summary_keeps_largest_absolute_weights() {
		let weights = [("Blink".to_string(), 0.25), ("Joy".to_string(), -0.9), ("Angry".to_string(), 0.6)]
			.into_iter()
			.collect();

		assert_eq!(format_top_expression_weights(&weights, 2), "Joy=-0.900, Angry=0.600");
	}

	#[test]
	fn reset_runtime_dynamics_nodes_restores_authored_dynamic_nodes_only() {
		let current = [1.0; 16];
		let rest = [2.0; 16];
		let untouched = [3.0; 16];
		let mut scene = un_avatar_core::UnaSceneSnapshot {
			nodes: vec![test_node(current), test_node(untouched)],
			..Default::default()
		};
		let rest_nodes = vec![test_node(rest), test_node(untouched)];
		let settings = un_avatar_core::UnaSpringBoneSettings {
			groups: vec![un_avatar_core::UnaSpringBoneGroup {
				bone_node_indices: vec![0, 9],
				..Default::default()
			}],
			colliders: Vec::new(),
			..Default::default()
		};

		assert!(reset_runtime_dynamics_nodes_to_rest(
			&mut scene,
			settings.runtime_dynamics(),
			&rest_nodes,
		));
		assert_eq!(scene.nodes[0].transform, rest);
		assert_eq!(scene.nodes[1].transform, untouched);
	}

	#[test]
	fn reset_runtime_dynamics_nodes_to_rest_for_source_id_preserves_other_sources() {
		let hair_current = [1.0; 16];
		let tail_current = [2.0; 16];
		let linked_current = [3.0; 16];
		let rest = [4.0; 16];
		let mut scene = un_avatar_core::UnaSceneSnapshot {
			nodes: vec![test_node(hair_current), test_node(tail_current), test_node(linked_current)],
			..Default::default()
		};
		let rest_nodes = vec![test_node(rest), test_node(rest), test_node(rest)];
		let settings = un_avatar_core::UnaSpringBoneSettings {
			groups: vec![
				un_avatar_core::UnaSpringBoneGroup {
					source_id: "physbone:hair".to_string(),
					bone_node_indices: vec![0],
					..Default::default()
				},
				un_avatar_core::UnaSpringBoneGroup {
					source_id: "physbone:tail".to_string(),
					bone_node_indices: vec![1],
					..Default::default()
				},
			],
			constraint_refs: vec![un_avatar_core::UnaDynamicsConstraintRef {
				target_node: 2,
				source_nodes: vec![0],
				..Default::default()
			}],
			..Default::default()
		};

		assert!(reset_runtime_dynamics_nodes_to_rest_for_source_id(
			&mut scene,
			settings.runtime_dynamics(),
			&rest_nodes,
			"physbone:hair",
		));
		assert_eq!(scene.nodes[0].transform, rest);
		assert_eq!(scene.nodes[1].transform, tail_current);
		assert_eq!(scene.nodes[2].transform, rest);
	}
}

/// IPC / status snapshot 用のカメラ状態（profile 保存・UI 表示用）。
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct CameraStateSnapshot {
	/// target ワールド座標 \[x, y, z\]。
	pub target: [f32; 3],
	/// 緯度・経度は度（UI に出すときに馴染みやすいよう degrees で公開）。
	pub longitude_deg: f32,
	pub latitude_deg: f32,
	/// target からカメラ位置までの距離。
	pub radius: f32,
	/// 対角画角（度）。
	pub diagonal_fov_deg: f32,
}

struct ScreenGrabTarget {
	width: u32,
	height: u32,
	format: wgpu::TextureFormat,
	texture: wgpu::Texture,
	view: wgpu::TextureView,
}

impl ScreenGrabTarget {
	fn new(device: &wgpu::Device, width: u32, height: u32, format: wgpu::TextureFormat) -> Self {
		let width = width.max(1);
		let height = height.max(1);
		let (texture, view) = create_screen_grab_texture(device, width, height, format);
		Self {
			width,
			height,
			format,
			texture,
			view,
		}
	}

	fn resize_to(&mut self, device: &wgpu::Device, width: u32, height: u32, format: wgpu::TextureFormat) {
		let width = width.max(1);
		let height = height.max(1);
		if self.width == width && self.height == height && self.format == format {
			return;
		}
		self.texture.destroy();
		let (texture, view) = create_screen_grab_texture(device, width, height, format);
		self.width = width;
		self.height = height;
		self.format = format;
		self.texture = texture;
		self.view = view;
	}

	fn texture(&self) -> &wgpu::Texture {
		&self.texture
	}

	fn view(&self) -> &wgpu::TextureView {
		&self.view
	}
}

pub(crate) struct GpuState {
	pub(crate) surface: wgpu::Surface<'static>,
	pub(crate) device: wgpu::Device,
	pub(crate) queue: wgpu::Queue,
	pub(crate) config: wgpu::SurfaceConfiguration,
	alpha_modes: Vec<wgpu::CompositeAlphaMode>,
	depth_texture: wgpu::Texture,
	depth_view: wgpu::TextureView,
	uniform_buffer: wgpu::Buffer,
	globals_uploaded: Option<GlobalsGpu>,
	bind_group: wgpu::BindGroup,
	pipeline: wgpu::RenderPipeline,
	axes_pipeline: wgpu::RenderPipeline,
	bone_collider_pipeline: wgpu::RenderPipeline,
	bone_collider_vertex_buffer: Option<wgpu::Buffer>,
	bone_collider_vertex_capacity: usize,
	bone_collider_vertex_count: u32,
	bone_collider_vertices: Vec<DebugLineVertex>,
	startup_splash_pipeline: wgpu::RenderPipeline,
	startup_splash_buffer: wgpu::Buffer,
	startup_splash_bind_group: wgpu::BindGroup,
	contact_shadow_pipeline: wgpu::RenderPipeline,
	contact_shadow_buffer: wgpu::Buffer,
	contact_shadow_bind_group: wgpu::BindGroup,
	document: Option<Arc<RwLock<UnaDocument>>>,
	document_revision: Arc<AtomicU64>,
	applied_document_revision: u64,
	last_runtime_parameter_action_values: BTreeMap<String, f32>,
	/// VMC 受信スレッドが起動済みか。受信データは描画直前に pending buffer から適用する。
	vmc_live: bool,
	scene_meshes: Option<SceneMeshes>,
	shader_variant_tier: MeshShaderVariantTier,
	avatar_outline: AvatarOutlineOptions,
	environment_color: EnvironmentColorOptions,
	lighting: LightingOptions,
	bloom: BloomOptions,
	ssao: crate::SsaoOptions,
	contact_shadow: ContactShadowOptions,
	texture_summary: Option<TextureUploadSummary>,
	last_asset_residency_refresh: SceneMeshAssetResidencyRefresh,
	last_mesh_buffer_scoped_load_count: usize,
	last_mesh_buffer_scoped_unload_count: usize,
	last_image_texture_scoped_load_count: usize,
	last_image_texture_scoped_unload_count: usize,
	last_cubemap_scoped_load_count: usize,
	last_cubemap_scoped_unload_count: usize,
	last_material_slot_scoped_upload_count: usize,
	audio_link_options: AudioLinkOptions,
	audio_link_runtime: Option<crate::audio_link::AudioLinkInputRuntime>,
	dynamics_sim: Option<DynamicsSimulator>,
	runtime_dynamics_enabled: bool,
	runtime_bone_collider_config: BoneColliderConfig,
	runtime_dynamics_physics: DynamicsPhysicsConfig,
	bone_colliders: Vec<BoneColliderPrimitive>,
	aa: AaMode,
	post_process: Option<PostProcess>,
	msaa_target: Option<crate::post_process::MsaaTarget>,
	screen_grab_target: Option<ScreenGrabTarget>,
	#[cfg(windows)]
	spout: Option<crate::spout::SpoutCapture>,
	#[cfg(windows)]
	spout_launch: Option<crate::spout::SpoutLaunchConfig>,
	debug_log: DebugLog,
	debug_scene: bool,
	debug_morph: bool,
	debug_frame_seq: u64,
	animation_time_secs: f32,
	disable_expression_morphs: bool,
	camera: OrbitCamera,
	world_scratch: Vec<Mat4>,
	gpu_timestamps: Option<GpuTimestamps>,
	expression_overrides: std::collections::BTreeMap<String, f32>,
	expression_overrides_revision: u64,
	applied_expression_overrides_revision: u64,
	expression_presets: Vec<String>,
	motion_apply_opts: un_avatar_skeleton::ApplyUnMotionFrameOpts,
	motion_buffer: Arc<MotionControlBuffer>,
	pending_motion_frames: Vec<un_motion_frame::UNMotionFrame>,
	motion_retarget_runtime: Option<MotionRetargetRuntime>,
	rest_nodes: Option<Arc<Vec<UnaSceneNode>>>,
	/// 旧 IPC / status 互換の primary source 値。現在の姿勢適用は key 単位の後着優先。
	primary_motion_source: Arc<AtomicU8>,
	/// UNMotion/Zenoh 受信が live で動いているか (subscriber スレッド起動済み)。
	unmotion_zenoh_live: bool,
	/// UNMotion/Zenoh subscriber が受信したフレーム数。適用前で数える。
	unmotion_zenoh_received_frames: Arc<AtomicU64>,
	/// 現在の profile と可視 material set が外部 AudioLink texture を必要としているか。
	audio_link_texture_needed: bool,
	/// 現在の profile と可視 material set から抽出した renderer runtime 要求。
	runtime_requirements: SceneMeshRuntimeRequirements,
	/// 描画直前に motion buffer から取り出して document に適用したフレーム数。
	motion_applied_frames: Arc<AtomicU64>,
	motion_receiver_generation: Arc<AtomicU64>,
	/// XYZ デバッグ軸描画の表示フラグ。manifest `[debug] show_axes` / CLI `--show-axes` / IPC で切替可能。
	show_axes: bool,
	show_bone_colliders: bool,
	bone_collider_count: u32,
	bone_collider_source: BoneColliderSource,
}

impl GpuState {
	#[allow(clippy::too_many_arguments)]
	pub fn new_shell(
		window: Arc<Window>,
		transparent: bool,
		primary_motion_source: crate::options::PrimaryMotionSource,
		spout_opts: SpoutWindowOptions,
		environment_color: EnvironmentColorOptions,
		lighting: LightingOptions,
		bloom: BloomOptions,
		ssao: crate::SsaoOptions,
		contact_shadow: ContactShadowOptions,
		aa: AaMode,
		render_backend: RenderBackend,
		texture_compression: TextureCompressionMode,
		debug: WindowDebugOptions,
		disable_expression_morphs: bool,
		disable_vmc_eye_look: bool,
		eye_look_at_clamp_deg: Option<f32>,
		apply_vmc_root_translation: bool,
		mesh_diagnostics: SceneMeshLoadOpts,
	) -> Result<Self, String> {
		let debug_log = DebugLog::from_options(&debug).map_err(|e| e.to_string())?;
		let debug_scene = debug.scene;
		let debug_morph = debug.morph;
		let motion_apply_opts = un_avatar_skeleton::ApplyUnMotionFrameOpts {
			apply_expressions: !disable_expression_morphs,
			apply_eye_bones: !disable_vmc_eye_look,
			eye_look_at_clamp_deg,
			apply_root_translation: apply_vmc_root_translation,
		};
		let size = window.inner_size();
		let width = size.width.max(1);
		let height = size.height.max(1);
		let vmc_live = false;
		let unmotion_zenoh_live = false;
		let unmotion_zenoh_received_frames = Arc::new(AtomicU64::new(0));
		let motion_applied_frames = Arc::new(AtomicU64::new(0));
		let document_revision = Arc::new(AtomicU64::new(0));
		let primary_motion_source = Arc::new(AtomicU8::new(primary_motion_source_to_u8(primary_motion_source)));
		let motion_buffer = Arc::new(MotionControlBuffer::default());

		let render_backend = effective_window_backend(render_backend);
		let instance_descriptor = instance_descriptor_for_backend(render_backend);
		let instance = wgpu::Instance::new(instance_descriptor);

		let surface: wgpu::Surface<'static> = instance.create_surface(window).map_err(|e| format!("create_surface: {e}"))?;

		let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
			power_preference: wgpu::PowerPreference::HighPerformance,
			compatible_surface: Some(&surface),
			force_fallback_adapter: false,
		}))
		.map_err(|e| format!("request_adapter: {e}"))?;

		let adapter_limits = adapter.limits();
		let mesh_shader_plan = mesh_shader_resource_plan_for_adapter(&adapter_limits);
		let limits = mesh_shader_plan.required_limits.clone();
		let shader_variant_tier = mesh_shader_plan.tier;
		if shader_variant_tier.is_baseline_fallback() {
			eprintln!(
				"un-avatar-renderer: GPU sampled texture/sampler limits are below the high-capability lilToon-compatible shader target; using baseline fallback variant (adapter sampled={} samplers={}, target sampled={} samplers={})",
				adapter_limits.max_sampled_textures_per_shader_stage,
				adapter_limits.max_samplers_per_shader_stage,
				HIGH_CAPABILITY_LILTOON_SAMPLED_TEXTURES_PER_STAGE,
				HIGH_CAPABILITY_LILTOON_SAMPLERS_PER_STAGE,
			);
		}

		let adapter_features = adapter.features();
		let texture_compression_features = if matches!(texture_compression, TextureCompressionMode::Source | TextureCompressionMode::Compat)
		{
			wgpu::Features::empty()
		} else {
			adapter_features
				& (wgpu::Features::TEXTURE_COMPRESSION_BC
					| wgpu::Features::TEXTURE_COMPRESSION_ASTC
					| wgpu::Features::TEXTURE_COMPRESSION_ETC2)
		};
		let timestamp_features = adapter_features & wgpu::Features::TIMESTAMP_QUERY;
		let texture_format_features = adapter_features & wgpu::Features::TEXTURE_FORMAT_16BIT_NORM;
		let required_features = texture_compression_features | timestamp_features | texture_format_features;

		let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
			label: Some("un-avatar-renderer"),
			required_features,
			required_limits: limits,
			memory_hints: Default::default(),
			..Default::default()
		}))
		.map_err(|e| format!("request_device: {e}"))?;
		device.on_uncaptured_error(Arc::new(|error| {
			eprintln!("un-avatar-renderer: uncaptured wgpu error: {error}");
		}));

		let caps = surface.get_capabilities(&adapter);
		let format = *caps
			.formats
			.first()
			.ok_or_else(|| "get_capabilities: スワップチェーン形式がありません".to_owned())?;

		let alpha_mode = if transparent {
			transparent_alpha_mode(&caps.alpha_modes)
		} else if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
			wgpu::CompositeAlphaMode::Opaque
		} else {
			caps.alpha_modes[0]
		};

		let present_mode = caps
			.present_modes
			.iter()
			.copied()
			.find(|m| *m == wgpu::PresentMode::Fifo)
			.unwrap_or(caps.present_modes[0]);

		let config = wgpu::SurfaceConfiguration {
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
			format,
			width,
			height,
			present_mode,
			alpha_mode,
			view_formats: vec![],
			desired_maximum_frame_latency: 2,
		};

		surface.configure(&device, &config);

		let (depth_texture, depth_view) = create_depth(&device, width, height);

		let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("globals"),
			entries: &[wgpu::BindGroupLayoutEntry {
				binding: 0,
				visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
				ty: wgpu::BindingType::Buffer {
					ty: wgpu::BufferBindingType::Uniform,
					has_dynamic_offset: false,
					min_binding_size: None,
				},
				count: None,
			}],
		});

		let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("globals"),
			size: std::mem::size_of::<GlobalsGpu>() as u64,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});

		let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("globals"),
			layout: &bind_group_layout,
			entries: &[wgpu::BindGroupEntry {
				binding: 0,
				resource: uniform_buffer.as_entire_binding(),
			}],
		});

		let aa_sample_count = aa_sample_count(aa);
		let pipeline = create_sky_pipeline(&device, &bind_group_layout, format, aa_sample_count);
		let axes_pipeline = create_axes_pipeline(&device, &bind_group_layout, format, aa_sample_count);
		let bone_collider_pipeline = create_bone_collider_pipeline(&device, &bind_group_layout, format, aa_sample_count);
		let startup_splash_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("startup_splash"),
			entries: &[wgpu::BindGroupLayoutEntry {
				binding: 0,
				visibility: wgpu::ShaderStages::FRAGMENT,
				ty: wgpu::BindingType::Buffer {
					ty: wgpu::BufferBindingType::Uniform,
					has_dynamic_offset: false,
					min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<StartupSplashGpu>() as u64),
				},
				count: None,
			}],
		});
		let startup_splash_pipeline = create_startup_splash_pipeline(&device, &startup_splash_bind_group_layout, format, aa_sample_count);
		let startup_splash_buffer = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("startup_splash"),
			size: std::mem::size_of::<StartupSplashGpu>() as u64,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		let startup_splash_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("startup_splash"),
			layout: &startup_splash_bind_group_layout,
			entries: &[wgpu::BindGroupEntry {
				binding: 0,
				resource: startup_splash_buffer.as_entire_binding(),
			}],
		});
		let contact_shadow_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("contact_shadow"),
			entries: &[wgpu::BindGroupLayoutEntry {
				binding: 0,
				visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
				ty: wgpu::BindingType::Buffer {
					ty: wgpu::BufferBindingType::Uniform,
					has_dynamic_offset: false,
					min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<ContactShadowGpu>() as u64),
				},
				count: None,
			}],
		});
		let contact_shadow_pipeline = create_contact_shadow_pipeline(
			&device,
			&bind_group_layout,
			&contact_shadow_bind_group_layout,
			format,
			aa_sample_count,
		);
		let contact_shadow_buffer = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("contact_shadow"),
			size: std::mem::size_of::<ContactShadowGpu>() as u64,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		let contact_shadow_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("contact_shadow"),
			layout: &contact_shadow_bind_group_layout,
			entries: &[wgpu::BindGroupEntry {
				binding: 0,
				resource: contact_shadow_buffer.as_entire_binding(),
			}],
		});

		let texture_summary = None;
		let avatar_outline = mesh_diagnostics.avatar_outline;
		let scene_meshes = None;
		let dynamics_sim = None;
		let bone_collider_count = 0;
		let bone_collider_source = BoneColliderSource::Off;

		#[cfg(windows)]
		let spout_launch = if spout_opts.enabled {
			let name = if spout_opts.name.is_empty() {
				"UN Avatar".to_string()
			} else {
				spout_opts.name.clone()
			};
			Some(crate::spout::SpoutLaunchConfig {
				name,
				width: spout_opts.width,
				height: spout_opts.height,
			})
		} else {
			None
		};
		#[cfg(windows)]
		let spout = spout_launch
			.as_ref()
			.and_then(|lc| crate::spout::SpoutCapture::try_new(&device, format, width, height, lc.clone()));
		#[cfg(windows)]
		if spout_opts.enabled && spout.is_none() {
			eprintln!(
				"un-avatar-renderer: Spout2 実バックエンドがこのビルドで利用できません。標準配布は `cargo xtask package` で Spout2 込みビルドを作成します。開発手動ビルドでは `--features spout-sdk` と SPOUT2_SDK_DIR / SPOUT2_LIB_DIR / 起動前 Spout.dll PATH が必要です。"
			);
		}
		#[cfg(not(windows))]
		if spout_opts.enabled {
			eprintln!("un-avatar-renderer: Spout は現状 Windows のみ対応です");
		}

		let (gw, gh) = Self::buffer_dims(width, height, &spout_opts);

		let mut gpu = Self {
			surface,
			device,
			queue,
			config,
			alpha_modes: caps.alpha_modes,
			depth_texture,
			depth_view,
			uniform_buffer,
			globals_uploaded: None,
			bind_group,
			pipeline,
			axes_pipeline,
			bone_collider_pipeline,
			bone_collider_vertex_buffer: None,
			bone_collider_vertex_capacity: 0,
			bone_collider_vertex_count: 0,
			bone_collider_vertices: Vec::new(),
			startup_splash_pipeline,
			startup_splash_buffer,
			startup_splash_bind_group,
			contact_shadow_pipeline,
			contact_shadow_buffer,
			contact_shadow_bind_group,
			document: None,
			document_revision,
			applied_document_revision: 0,
			last_runtime_parameter_action_values: BTreeMap::new(),
			vmc_live,
			scene_meshes,
			shader_variant_tier,
			avatar_outline,
			environment_color,
			lighting,
			bloom,
			ssao,
			contact_shadow,
			texture_summary,
			last_asset_residency_refresh: SceneMeshAssetResidencyRefresh::default(),
			last_mesh_buffer_scoped_load_count: 0,
			last_mesh_buffer_scoped_unload_count: 0,
			last_image_texture_scoped_load_count: 0,
			last_image_texture_scoped_unload_count: 0,
			last_cubemap_scoped_load_count: 0,
			last_cubemap_scoped_unload_count: 0,
			last_material_slot_scoped_upload_count: 0,
			audio_link_options: AudioLinkOptions::default(),
			audio_link_runtime: None,
			dynamics_sim,
			runtime_dynamics_enabled: true,
			runtime_bone_collider_config: BoneColliderConfig::default(),
			runtime_dynamics_physics: DynamicsPhysicsConfig::default(),
			bone_colliders: Vec::new(),
			bone_collider_count,
			bone_collider_source,
			aa,
			post_process: None,
			msaa_target: None,
			screen_grab_target: None,
			#[cfg(windows)]
			spout,
			#[cfg(windows)]
			spout_launch,
			debug_log,
			debug_scene,
			debug_morph,
			debug_frame_seq: 0,
			animation_time_secs: 0.0,
			disable_expression_morphs,
			camera: OrbitCamera::default(),
			world_scratch: Vec::new(),
			gpu_timestamps: None,
			expression_overrides: std::collections::BTreeMap::new(),
			expression_overrides_revision: 0,
			applied_expression_overrides_revision: 0,
			expression_presets: Vec::new(),
			motion_apply_opts,
			motion_buffer,
			pending_motion_frames: Vec::new(),
			motion_retarget_runtime: None,
			rest_nodes: None,
			primary_motion_source,
			unmotion_zenoh_live,
			unmotion_zenoh_received_frames,
			audio_link_texture_needed: false,
			runtime_requirements: SceneMeshRuntimeRequirements::default(),
			motion_applied_frames,
			motion_receiver_generation: Arc::new(AtomicU64::new(0)),
			// XYZ 軸はデフォルト Off（manifest や CLI、UI からの明示指示で表示）。
			show_axes: false,
			show_bone_colliders: false,
		};
		if timestamp_features.contains(wgpu::Features::TIMESTAMP_QUERY) {
			gpu.gpu_timestamps = Some(GpuTimestamps::new(&gpu.device, &gpu.queue));
		}
		gpu.write_globals(gw, gh);
		Ok(gpu)
	}

	pub fn expression_presets(&self) -> &[String] {
		&self.expression_presets
	}

	pub fn set_expression_override(&mut self, name: &str, weight: f32) {
		if !weight.is_finite() {
			return;
		}
		let w = weight.clamp(0.0, 1.0);
		if self
			.expression_overrides
			.get(name)
			.is_some_and(|current| (*current - w).abs() <= f32::EPSILON)
		{
			return;
		}
		self.expression_overrides.insert(name.to_string(), w);
		self.expression_overrides_revision = self.expression_overrides_revision.wrapping_add(1);
	}

	pub fn clear_expression_overrides(&mut self) {
		if self.expression_overrides.is_empty() {
			return;
		}
		self.expression_overrides.clear();
		self.expression_overrides_revision = self.expression_overrides_revision.wrapping_add(1);
	}

	/// VRM 1.0 LookAt 簡易クランプ角度を更新する。`None` でクランプ無効化。
	pub fn set_eye_look_at_clamp_deg(&mut self, clamp_deg: Option<f32>) {
		self.motion_apply_opts.eye_look_at_clamp_deg = clamp_deg.filter(|d| d.is_finite() && *d >= 0.0);
	}

	/// 現在の LookAt クランプ角度を返す（`None` なら無効）。
	pub fn eye_look_at_clamp_deg(&self) -> Option<f32> {
		self.motion_apply_opts.eye_look_at_clamp_deg
	}

	/// VMC `Root.translation` を scene root に加算するか。OFF (既定) ならアバターの位置は rest pose を保つ。
	/// Waidayo 等の calibration の都合で意図せず非ゼロな translation が送られる場合に、アバターが
	/// 前後にズレないようにするためのスイッチ。
	pub fn set_apply_vmc_root_translation(&mut self, enabled: bool) {
		self.motion_apply_opts.apply_root_translation = enabled;
	}

	/// 現在の VMC Root translation 適用フラグ。
	pub fn apply_vmc_root_translation(&self) -> bool {
		self.motion_apply_opts.apply_root_translation
	}

	/// 旧 IPC 互換の primary source 更新。現在の姿勢適用は key 単位の後着優先。
	pub fn set_primary_motion_source(&self, source: crate::options::PrimaryMotionSource) {
		self.primary_motion_source
			.store(primary_motion_source_to_u8(source), Ordering::Relaxed);
	}

	/// 旧 status 互換の primary source 値。
	pub fn primary_motion_source(&self) -> crate::options::PrimaryMotionSource {
		primary_motion_source_from_u8(self.primary_motion_source.load(Ordering::Relaxed))
	}

	/// UNMotion/Zenoh subscriber が起動済みか。`new()` 時の `unmotion_zenoh.enabled` で決定する。
	pub fn unmotion_zenoh_live(&self) -> bool {
		self.unmotion_zenoh_live
	}

	pub fn unmotion_zenoh_received_frames(&self) -> u64 {
		self.unmotion_zenoh_received_frames.load(Ordering::Relaxed)
	}

	pub fn motion_applied_frames(&self) -> u64 {
		self.motion_applied_frames.load(Ordering::Relaxed)
	}

	pub fn dynamics_counts(&self) -> DynamicsRuntimeCounts {
		let Some(doc_arc) = self.document.as_ref() else {
			return DynamicsRuntimeCounts::default();
		};
		let Ok(doc) = doc_arc.read() else {
			return DynamicsRuntimeCounts::default();
		};
		doc.runtime_model().dynamics().counts().into()
	}

	fn refresh_scene_draw_state(&mut self, document_revision_to_apply: Option<u64>) -> bool {
		let (Some(sm), Some(doc_arc)) = (&mut self.scene_meshes, &self.document) else {
			return false;
		};
		let Ok(doc) = doc_arc.read() else {
			return false;
		};
		let runtime_model = doc.runtime_model();
		let Some(runtime) = runtime_model.scene_expression_catalog() else {
			return false;
		};
		crate::scene_transform::write_world_from_nodes(runtime.scene, &mut self.world_scratch);
		let document_changed = document_revision_to_apply.is_some_and(|revision| revision != self.applied_document_revision);
		if document_changed && !expression_presets_match_catalog(&self.expression_presets, runtime.expression_catalog) {
			self.expression_presets = expression_preset_names(runtime.expression_catalog);
		}
		let refresh_scene_morph_defaults = document_changed;
		let expr_weights = active_expression_weights_for_doc(self.disable_expression_morphs, &doc);
		let expression_overrides = active_expression_overrides(self.disable_expression_morphs, &self.expression_overrides);
		if document_changed {
			sm.refresh_draw_materials_from_scene(&self.device, &self.queue, runtime.scene);
			let residency_refresh = sm.refresh_asset_group_residency_with_changes(runtime.scene, runtime_model.active_asset_groups());
			if residency_refresh.has_scoped_resource_changes() && self.debug_log.is_enabled() {
				self.debug_log.line(
					"wardrobe",
					format!(
						"asset residency refresh mesh_load={:?} mesh_unload={:?} image_load={:?} image_unload={:?} material_load={:?} material_unload={:?}",
						residency_refresh.mesh_buffer_load_indices,
						residency_refresh.mesh_buffer_unload_indices,
						residency_refresh.image_texture_load_indices,
						residency_refresh.image_texture_unload_indices,
						residency_refresh.material_slot_load_indices,
						residency_refresh.material_slot_unload_indices
					),
				);
			}
			let (mesh_buffer_load_count, mesh_buffer_unload_count) = sm.apply_mesh_buffer_residency(
				&self.device,
				&self.queue,
				&residency_refresh.mesh_buffer_load_indices,
				&residency_refresh.mesh_buffer_unload_indices,
			);
			self.last_mesh_buffer_scoped_load_count = mesh_buffer_load_count;
			self.last_mesh_buffer_scoped_unload_count = mesh_buffer_unload_count;
			if (mesh_buffer_load_count > 0 || mesh_buffer_unload_count > 0) && self.debug_log.is_enabled() {
				self.debug_log.line(
					"wardrobe",
					format!(
						"mesh buffer scoped load_count={} unload_count={} load={:?} unload={:?}",
						mesh_buffer_load_count,
						mesh_buffer_unload_count,
						residency_refresh.mesh_buffer_load_indices,
						residency_refresh.mesh_buffer_unload_indices
					),
				);
			}
			self.last_asset_residency_refresh = residency_refresh;
			let active_gaps = sm.active_residency_gaps();
			let mut image_load_indices = self
				.last_asset_residency_refresh
				.image_texture_load_indices
				.iter()
				.chain(active_gaps.inactive_image_texture_indices.iter())
				.copied()
				.collect::<Vec<_>>();
			image_load_indices.sort_unstable();
			image_load_indices.dedup();
			let image_load_set = image_load_indices.iter().copied().collect::<BTreeSet<_>>();
			let image_unload_indices = self
				.last_asset_residency_refresh
				.image_texture_unload_indices
				.iter()
				.copied()
				.filter(|index| !image_load_set.contains(index))
				.collect::<Vec<_>>();
			sm.promote_image_texture_residency(&image_load_indices);
			let (image_texture_bind_load_count, image_texture_bind_unload_count, cubemap_load_count, cubemap_unload_count) =
				sm.apply_image_texture_view_residency(&self.device, &self.queue, &image_load_indices, &image_unload_indices);
			self.last_image_texture_scoped_load_count = image_texture_bind_load_count;
			self.last_image_texture_scoped_unload_count = image_texture_bind_unload_count;
			self.last_cubemap_scoped_load_count = cubemap_load_count;
			self.last_cubemap_scoped_unload_count = cubemap_unload_count;
			if (image_texture_bind_load_count > 0
				|| image_texture_bind_unload_count > 0
				|| cubemap_load_count > 0
				|| cubemap_unload_count > 0)
				&& self.debug_log.is_enabled()
			{
				self.debug_log.line(
					"wardrobe",
					format!(
						"image texture scoped load_count={} unload_count={} cubemap_load_count={} cubemap_unload_count={} load={:?} unload={:?}",
						image_texture_bind_load_count,
						image_texture_bind_unload_count,
						cubemap_load_count,
						cubemap_unload_count,
						image_load_indices,
						image_unload_indices
					),
				);
			}
			let material_slot_upload_count = sm.promote_material_slot_residency(&active_gaps.inactive_material_slot_indices);
			self.last_material_slot_scoped_upload_count = material_slot_upload_count;
			if material_slot_upload_count > 0 && self.debug_log.is_enabled() {
				self.debug_log.line(
					"wardrobe",
					format!(
						"material slot scoped upload count={} slots={:?}",
						material_slot_upload_count, active_gaps.inactive_material_slot_indices
					),
				);
			}
			sm.rebuild_material_bind_groups(&self.device);
		}
		sm.update_draw_transforms(
			&self.queue,
			runtime.scene,
			&self.world_scratch,
			expr_weights,
			expression_overrides,
			refresh_scene_morph_defaults,
		);
		let runtime_requirements_after_update = refresh_scene_morph_defaults.then(|| sm.runtime_requirements());
		if let Some(document_revision) = document_revision_to_apply {
			self.applied_document_revision = document_revision;
			self.applied_expression_overrides_revision = self.expression_overrides_revision;
		}
		drop(doc);
		if let Some(requirements) = runtime_requirements_after_update {
			self.apply_runtime_requirements_with_current_audio_link(requirements);
		}
		true
	}

	fn mark_document_changed(&self) {
		self.document_revision.fetch_add(1, Ordering::Release);
	}

	fn invalidate_applied_document_state(&mut self) {
		self.applied_document_revision = 0;
		self.mark_document_changed();
	}

	pub fn audio_link_texture_needed(&self) -> bool {
		self.audio_link_texture_needed
	}

	pub(crate) fn runtime_requirements(&self) -> SceneMeshRuntimeRequirements {
		self.runtime_requirements
	}

	fn apply_runtime_requirements(&mut self, requirements: SceneMeshRuntimeRequirements, audio_link_options: AudioLinkOptions) {
		let audio_link_texture_needed = audio_link_options.source == AudioLinkSource::InputDevice && requirements.audio_link_texture;
		let audio_link_config_changed = self.audio_link_options != audio_link_options;
		let audio_link_need_changed = self.audio_link_texture_needed != audio_link_texture_needed;
		self.audio_link_options = audio_link_options;
		self.audio_link_texture_needed = audio_link_texture_needed;
		self.runtime_requirements = requirements;
		if audio_link_config_changed || audio_link_need_changed {
			self.reconfigure_audio_link_runtime();
		}
	}

	fn apply_runtime_requirements_with_current_audio_link(&mut self, requirements: SceneMeshRuntimeRequirements) {
		let audio_link_texture_needed = self.audio_link_options.source == AudioLinkSource::InputDevice && requirements.audio_link_texture;
		self.runtime_requirements = requirements;
		if self.audio_link_texture_needed != audio_link_texture_needed {
			self.audio_link_texture_needed = audio_link_texture_needed;
			self.reconfigure_audio_link_runtime();
		}
	}

	fn reconfigure_audio_link_runtime(&mut self) {
		if self.audio_link_texture_needed {
			if self.audio_link_runtime.is_none() {
				match crate::audio_link::AudioLinkInputRuntime::start(&self.audio_link_options) {
					Ok(runtime) => {
						self.audio_link_runtime = Some(runtime);
					}
					Err(e) => {
						eprintln!("un-avatar-renderer: AudioLink input disabled: {e}");
						self.audio_link_runtime = None;
					}
				}
			}
		} else {
			self.audio_link_runtime = None;
			if let Some(sm) = &mut self.scene_meshes {
				sm.set_audio_link_external_enabled(false);
			}
		}
	}

	/// XYZ デバッグ軸表示の ON/OFF。
	pub fn set_show_axes(&mut self, enabled: bool) {
		self.show_axes = enabled;
	}

	pub fn set_show_bone_colliders(&mut self, enabled: bool) {
		self.show_bone_colliders = enabled;
	}

	pub fn reconfigure_dynamics(
		&mut self,
		enabled: bool,
		bone_collider_config: BoneColliderConfig,
		spring_bone_physics: DynamicsPhysicsConfig,
	) {
		self.runtime_dynamics_enabled = enabled;
		self.runtime_bone_collider_config = bone_collider_config;
		self.runtime_dynamics_physics = spring_bone_physics;
		self.reset_dynamics_nodes_to_rest();
		self.rebuild_runtime_dynamics();
	}

	fn rebuild_runtime_dynamics(&mut self) {
		let Some(doc_arc) = self.document.as_ref() else {
			self.dynamics_sim = None;
			self.bone_colliders.clear();
			self.bone_collider_count = 0;
			self.bone_collider_source = BoneColliderSource::Off;
			self.bone_collider_vertex_buffer = None;
			self.bone_collider_vertex_capacity = 0;
			self.bone_collider_vertices.clear();
			return;
		};
		let Ok(doc) = doc_arc.read() else {
			return;
		};
		let physics = build_runtime_physics_for_document(
			&doc,
			self.runtime_dynamics_enabled,
			self.runtime_bone_collider_config,
			&self.runtime_dynamics_physics,
		);
		self.bone_collider_count = physics.stats.count;
		self.bone_collider_source = physics.stats.source;
		self.bone_collider_vertex_buffer = None;
		self.bone_collider_vertex_capacity = 0;
		self.bone_collider_vertex_count = 0;
		self.bone_collider_vertices.clear();
		self.dynamics_sim = physics.dynamics_sim;
		self.bone_colliders = physics.debug_bone_colliders;
	}

	fn reset_dynamics_nodes_to_rest(&mut self) {
		let (Some(doc_arc), Some(rest_nodes)) = (self.document.as_ref(), self.rest_nodes.as_ref()) else {
			return;
		};
		let Ok(mut doc) = doc_arc.write() else {
			return;
		};
		let Some(runtime) = doc.runtime_scene_and_dynamics_mut() else {
			return;
		};
		if !reset_runtime_dynamics_nodes_to_rest(runtime.scene, runtime.dynamics.as_readonly(), rest_nodes) {
			return;
		}
		drop(doc);
		self.invalidate_applied_document_state();
	}

	/// Avatar outline effect を実行中 renderer に即時反映する。
	pub fn set_avatar_outline(&mut self, outline: AvatarOutlineOptions) {
		self.avatar_outline = outline;
		if let Some(sm) = &mut self.scene_meshes {
			sm.set_avatar_outline(&self.queue, outline);
		}
	}

	/// Avatar rim light effect を実行中 renderer に即時反映する。
	pub fn set_avatar_rim(&mut self, rim: crate::AvatarRimOptions) {
		if let Some(sm) = &mut self.scene_meshes {
			sm.set_avatar_rim(&self.queue, rim);
		}
	}

	/// Avatar matcap strength を実行中 renderer に即時反映する。
	pub fn set_avatar_matcap(&mut self, matcap: crate::AvatarMatcapOptions) {
		if let Some(sm) = &mut self.scene_meshes {
			sm.set_avatar_matcap(&self.queue, matcap);
		}
	}

	/// Synthetic specular accent を実行中 renderer に即時反映する。
	pub fn set_avatar_specular(&mut self, specular: crate::AvatarSpecularOptions) {
		if let Some(sm) = &mut self.scene_meshes {
			sm.set_avatar_specular(&self.queue, specular);
		}
	}

	/// Authored ambient occlusion strength を実行中 renderer に即時反映する。
	pub fn set_avatar_ambient_occlusion(&mut self, ambient_occlusion: crate::AvatarAmbientOcclusionOptions) {
		if let Some(sm) = &mut self.scene_meshes {
			sm.set_avatar_ambient_occlusion(&self.queue, ambient_occlusion);
		}
	}

	/// Final post color adjustment を実行中 renderer に即時反映する。
	pub fn set_environment_color(&mut self, color: EnvironmentColorOptions) {
		self.environment_color = EnvironmentColorOptions {
			exposure: color.exposure.clamp(-4.0, 4.0),
			contrast: color.contrast.clamp(0.0, 4.0),
			saturation: color.saturation.clamp(0.0, 4.0),
			look: color.look,
			look_intensity: color.look_intensity.clamp(0.0, 1.0),
			temperature: color.temperature.clamp(-1.0, 1.0),
			tint: color.tint.clamp(-1.0, 1.0),
		};
		if matches!(self.environment_color.look, ColorGradingLook::Neutral) {
			self.environment_color.look_intensity = 0.0;
		}
	}

	pub fn set_lighting(&mut self, lighting: LightingOptions) {
		self.lighting = LightingOptions {
			environment: crate::options::EnvironmentLightOptions {
				enabled: lighting.environment.enabled,
				color: [
					lighting.environment.color[0].clamp(0.0, 1.0),
					lighting.environment.color[1].clamp(0.0, 1.0),
					lighting.environment.color[2].clamp(0.0, 1.0),
				],
				intensity: lighting.environment.intensity.clamp(0.0, 2.0),
			},
			directional: crate::options::DirectionalLightOptions {
				enabled: lighting.directional.enabled,
				color: [
					lighting.directional.color[0].clamp(0.0, 1.0),
					lighting.directional.color[1].clamp(0.0, 1.0),
					lighting.directional.color[2].clamp(0.0, 1.0),
				],
				intensity: lighting.directional.intensity.clamp(0.0, 4.0),
				azimuth_deg: lighting.directional.azimuth_deg.clamp(-360.0, 360.0),
				elevation_deg: lighting.directional.elevation_deg.clamp(-89.0, 89.0),
				follow_camera_yaw: lighting.directional.follow_camera_yaw,
				follow_camera_pitch: lighting.directional.follow_camera_pitch,
			},
		};
		self.globals_uploaded = None;
	}

	pub fn set_bloom(&mut self, bloom: BloomOptions) {
		self.bloom = BloomOptions {
			enabled: bloom.enabled,
			strength: bloom.strength.clamp(0.0, 2.0),
			threshold: bloom.threshold.clamp(0.0, 2.0),
			radius: bloom.radius.clamp(0.0, 32.0),
			quality: bloom.quality,
		};
	}

	pub fn set_ssao(&mut self, ssao: crate::SsaoOptions) {
		self.ssao = crate::SsaoOptions {
			enabled: ssao.enabled,
			strength: ssao.strength.clamp(0.0, 1.0),
			radius: ssao.radius.clamp(1.0, 24.0),
			bias: ssao.bias.clamp(0.0, 0.02),
			range: ssao.range.clamp(0.001, 0.2),
		};
	}

	pub fn set_contact_shadow(&mut self, contact_shadow: ContactShadowOptions) {
		self.contact_shadow = ContactShadowOptions {
			enabled: contact_shadow.enabled,
			strength: contact_shadow.strength.clamp(0.0, 1.0),
			radius: contact_shadow.radius.clamp(0.05, 3.0),
			softness: contact_shadow.softness.clamp(0.1, 8.0),
			height: contact_shadow.height.clamp(-1.0, 1.0),
		};
	}

	fn write_contact_shadow_uniform(&self) {
		self.queue.write_buffer(
			&self.contact_shadow_buffer,
			0,
			bytemuck::bytes_of(&ContactShadowGpu {
				params: [
					self.contact_shadow.strength.clamp(0.0, 1.0),
					self.contact_shadow.radius.clamp(0.05, 3.0),
					self.contact_shadow.softness.clamp(0.1, 8.0),
					self.contact_shadow.height.clamp(-1.0, 1.0),
				],
			}),
		);
	}

	fn draw_contact_shadow(&self, pass: &mut wgpu::RenderPass<'_>) {
		pass.set_pipeline(&self.contact_shadow_pipeline);
		pass.set_bind_group(0, &self.bind_group, &[]);
		pass.set_bind_group(1, &self.contact_shadow_bind_group, &[]);
		pass.draw(0..6, 0..1);
	}

	/// XYZ デバッグ軸表示が ON か。
	pub fn show_axes(&self) -> bool {
		self.show_axes
	}

	pub fn show_bone_colliders(&self) -> bool {
		self.show_bone_colliders
	}

	pub fn bone_collider_count(&self) -> u32 {
		self.bone_collider_count
	}

	pub fn bone_collider_source(&self) -> &'static str {
		self.bone_collider_source.as_str()
	}

	fn update_bone_collider_debug_vertices(&mut self) {
		if !self.show_bone_colliders {
			self.bone_collider_vertex_count = 0;
			return;
		}
		let Some(doc_arc) = self.document.as_ref().map(Arc::clone) else {
			self.bone_collider_vertex_count = 0;
			return;
		};
		let Ok(doc) = doc_arc.read() else {
			self.bone_collider_vertex_count = 0;
			return;
		};
		let runtime_model = doc.runtime_model();
		let Some(scene) = runtime_model.scene() else {
			self.bone_collider_vertex_count = 0;
			return;
		};
		crate::scene_transform::write_world_from_nodes(scene, &mut self.world_scratch);
		self.rebuild_bone_collider_debug_vertices_from_world();
	}

	fn rebuild_bone_collider_debug_vertices_from_world(&mut self) {
		self.bone_collider_vertices.clear();
		let colliders = self
			.dynamics_sim
			.as_ref()
			.map(DynamicsSimulator::bone_colliders)
			.unwrap_or(&self.bone_colliders);
		for collider in colliders {
			append_collider_wire_vertices(*collider, &self.world_scratch, &mut self.bone_collider_vertices);
		}
		if self.bone_collider_vertices.is_empty() {
			self.bone_collider_vertex_count = 0;
			return;
		}
		let vertex_count = self.bone_collider_vertices.len();
		self.bone_collider_vertex_count = vertex_count as u32;
		if self.bone_collider_vertex_capacity < vertex_count || self.bone_collider_vertex_buffer.is_none() {
			let next_capacity = vertex_count.next_power_of_two();
			self.bone_collider_vertex_buffer = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
				label: Some("debug_bone_colliders"),
				size: (next_capacity * std::mem::size_of::<DebugLineVertex>()) as u64,
				usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
				mapped_at_creation: false,
			}));
			self.bone_collider_vertex_capacity = next_capacity;
		}
		if let Some(buffer) = &self.bone_collider_vertex_buffer {
			self.queue
				.write_buffer(buffer, 0, bytemuck::cast_slice(&self.bone_collider_vertices));
		}
	}

	/// 対角画角（度）を設定する。範囲外は内部で clamp。
	pub fn set_camera_fov_diagonal_deg(&mut self, deg: f32) {
		self.camera.set_diagonal_fov_deg(deg);
		let (gw, gh) = self.render_pixel_dims();
		self.write_globals(gw, gh);
	}

	/// 現在のカメラパラメータをスナップショットとして返す（IPC/UI からの読み取り用）。
	pub fn camera_state_snapshot(&self) -> CameraStateSnapshot {
		CameraStateSnapshot {
			target: [self.camera.target.x, self.camera.target.y, self.camera.target.z],
			longitude_deg: self.camera.longitude.to_degrees(),
			latitude_deg: self.camera.latitude.to_degrees(),
			radius: self.camera.radius,
			diagonal_fov_deg: self.camera.diagonal_fov_deg,
		}
	}

	/// IPC から渡された target/orbit/fov 値を一度に上書きする（profile からのロード等で使用）。
	pub fn set_camera_state(
		&mut self,
		target: Option<[f32; 3]>,
		longitude_deg: Option<f32>,
		latitude_deg: Option<f32>,
		radius: Option<f32>,
		diagonal_fov_deg: Option<f32>,
	) {
		if let Some([x, y, z]) = target {
			self.camera.target = glam::Vec3::new(x, y, z);
		}
		self.camera
			.set_orbit(longitude_deg.map(f32::to_radians), latitude_deg.map(f32::to_radians), radius);
		if let Some(deg) = diagonal_fov_deg {
			self.camera.set_diagonal_fov_deg(deg);
		}
		let (gw, gh) = self.render_pixel_dims();
		self.write_globals(gw, gh);
	}

	fn buffer_dims(window_w: u32, window_h: u32, spout_opts: &SpoutWindowOptions) -> (u32, u32) {
		#[cfg(windows)]
		if spout_opts.enabled {
			return (
				spout_opts.width.unwrap_or(window_w).max(1),
				spout_opts.height.unwrap_or(window_h).max(1),
			);
		}
		(window_w.max(1), window_h.max(1))
	}

	fn render_pixel_dims(&self) -> (u32, u32) {
		#[cfg(windows)]
		if let Some(ref sp) = self.spout {
			return sp.dimensions();
		}
		(self.config.width.max(1), self.config.height.max(1))
	}

	pub fn orbit_camera_pixels(&mut self, delta_x: f64, delta_y: f64) {
		const ORBIT_RADIANS_PER_PIXEL: f32 = 0.006;
		self.camera
			.orbit(delta_x as f32 * ORBIT_RADIANS_PER_PIXEL, delta_y as f32 * ORBIT_RADIANS_PER_PIXEL);
		let (gw, gh) = self.render_pixel_dims();
		self.write_globals(gw, gh);
	}

	pub fn zoom_camera_wheel(&mut self, wheel_positive_units: f32) {
		self.camera.zoom(wheel_positive_units);
		let (gw, gh) = self.render_pixel_dims();
		self.write_globals(gw, gh);
	}

	pub fn reset_camera(&mut self) {
		self.camera = OrbitCamera::default();
		let (gw, gh) = self.render_pixel_dims();
		self.write_globals(gw, gh);
	}

	/// 視線方向に直交する平面で target を移動する（マウス中ボタンドラッグでのパン用）。
	/// 画面ピクセル基準で `delta_x`/`delta_y` を渡す。
	pub fn pan_camera_pixels(&mut self, delta_x: f64, delta_y: f64) {
		self.camera.pan(delta_x as f32, delta_y as f32);
		let (gw, gh) = self.render_pixel_dims();
		self.write_globals(gw, gh);
	}

	/// orbit (longitude/latitude) のみを初期値に戻す。target/radius は保持。
	pub fn reset_camera_rotation(&mut self) {
		self.camera.reset_rotation();
		let (gw, gh) = self.render_pixel_dims();
		self.write_globals(gw, gh);
	}

	/// target（pan 位置）のみを初期値に戻す。orbit/radius/FOV は保持。
	/// ミドルダブルクリックで「パン操作のリセット」を行う用途。
	pub fn reset_camera_pan(&mut self) {
		self.camera.reset_pan();
		let (gw, gh) = self.render_pixel_dims();
		self.write_globals(gw, gh);
	}

	/// 単発のオフスクリーンレンダリングで PNG を保存する。透過設定をそのまま含む。
	pub fn capture_screenshot(&mut self, path: &std::path::Path, clear_color: wgpu::Color) -> Result<(), String> {
		let (w, h) = self.render_pixel_dims();
		let format = self.config.format;
		let aa_sample_count = aa_sample_count(self.aa);

		// シーンノードがある場合は現在の pose を再アップロードしておく（前フレーム未提出の可能性に備える）。
		self.refresh_scene_draw_state(None);
		self.write_frame_globals(w, h, true);

		let target_tex = self.device.create_texture(&wgpu::TextureDescriptor {
			label: Some("screenshot-target"),
			size: wgpu::Extent3d {
				width: w,
				height: h,
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format,
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
			view_formats: &[],
		});
		let target_view = target_tex.create_view(&wgpu::TextureViewDescriptor::default());

		let mut msaa: Option<crate::post_process::MsaaTarget> = None;
		let mut post: Option<PostProcess> = None;
		let use_msaa = matches!(self.aa, AaMode::Msaa);
		let use_post_aa = matches!(self.aa, AaMode::Fxaa | AaMode::Smaa);
		let use_avatar_outline =
			self.avatar_outline.policy == AvatarOutlinePolicy::Override && self.avatar_outline.width.unwrap_or(0.003) > 0.0;
		let use_color_adjust = !self.environment_color.is_identity();
		let use_bloom = self.bloom.is_enabled();
		let use_ssao = self.ssao.is_enabled();
		let needs_screen_refraction = self.scene_meshes.as_ref().is_some_and(SceneMeshes::needs_screen_refraction);
		let use_post = use_post_aa || use_avatar_outline || use_color_adjust || use_bloom || use_ssao || needs_screen_refraction;
		if use_msaa {
			msaa = Some(crate::post_process::MsaaTarget::new(&self.device, w, h, format, aa_sample_count));
		}
		if use_post {
			post = Some(PostProcess::new(&self.device, w, h, format));
		}
		if needs_screen_refraction {
			if let Some(grab) = &mut self.screen_grab_target {
				grab.resize_to(&self.device, w, h, format);
			} else {
				self.screen_grab_target = Some(ScreenGrabTarget::new(&self.device, w, h, format));
			}
		}
		let (depth_tex, depth_view) = create_depth(&self.device, w, h);
		let draw_scene = self.scene_meshes.as_ref().is_some_and(|m| !m.is_empty());
		let draw_contact_shadow = draw_scene && self.contact_shadow.is_enabled();
		let draw_contact_shadow_in_main = draw_contact_shadow && !use_avatar_outline;
		let mut main_resolve: Option<&wgpu::TextureView> = None;
		let (main_color, main_depth) = if let Some(post) = &post {
			if let Some(msaa) = &msaa {
				main_resolve = Some(post.source_view());
				(msaa.color_view(), msaa.depth_view())
			} else {
				(post.source_view(), post.depth_view())
			}
		} else if let Some(msaa) = &msaa {
			main_resolve = Some(&target_view);
			(msaa.color_view(), msaa.depth_view())
		} else {
			(&target_view, &depth_view)
		};
		// depth_tex は MSAA/PostAA で使われないが、Drop されないよう束縛しておく。
		let _ = &depth_tex;

		let mut encoder = self
			.device
			.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("screenshot") });
		if draw_scene {
			if let Some(sm) = &self.scene_meshes {
				sm.encode_compute_fur_cards(&mut encoder);
			}
		}
		if draw_scene && needs_screen_refraction {
			let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
				label: Some("screenshot-main-opaque"),
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: main_color,
					depth_slice: None,
					resolve_target: main_resolve,
					ops: wgpu::Operations {
						load: wgpu::LoadOp::Clear(clear_color),
						store: wgpu::StoreOp::Store,
					},
				})],
				depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
					view: main_depth,
					depth_ops: Some(wgpu::Operations {
						load: wgpu::LoadOp::Clear(1.0),
						store: wgpu::StoreOp::Store,
					}),
					stencil_ops: None,
				}),
				timestamp_writes: None,
				occlusion_query_set: None,
				multiview_mask: None,
			});
			if let Some(sm) = &self.scene_meshes {
				sm.draw_opaque(&mut pass);
				if draw_contact_shadow_in_main {
					self.write_contact_shadow_uniform();
					self.draw_contact_shadow(&mut pass);
				}
				sm.draw_toon_outlines(&mut pass);
				sm.draw_blended_before_screen_refraction(&mut pass);
			}
			drop(pass);

			if let (Some(post), Some(grab), Some(sm)) = (&post, &self.screen_grab_target, &mut self.scene_meshes) {
				encoder.copy_texture_to_texture(
					wgpu::TexelCopyTextureInfo {
						texture: post.source_texture(),
						mip_level: 0,
						origin: wgpu::Origin3d::ZERO,
						aspect: wgpu::TextureAspect::All,
					},
					wgpu::TexelCopyTextureInfo {
						texture: grab.texture(),
						mip_level: 0,
						origin: wgpu::Origin3d::ZERO,
						aspect: wgpu::TextureAspect::All,
					},
					wgpu::Extent3d {
						width: w.max(1),
						height: h.max(1),
						depth_or_array_layers: 1,
					},
				);
				sm.set_screen_grab_view(&self.device, grab.view());
			}

			let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
				label: Some("screenshot-main-blended"),
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: main_color,
					depth_slice: None,
					resolve_target: main_resolve,
					ops: wgpu::Operations {
						load: wgpu::LoadOp::Load,
						store: wgpu::StoreOp::Store,
					},
				})],
				depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
					view: main_depth,
					depth_ops: Some(wgpu::Operations {
						load: wgpu::LoadOp::Load,
						store: wgpu::StoreOp::Store,
					}),
					stencil_ops: None,
				}),
				timestamp_writes: None,
				occlusion_query_set: None,
				multiview_mask: None,
			});
			if let Some(sm) = &self.scene_meshes {
				sm.draw_blended_after_screen_refraction(&mut pass);
			}
			if self.show_axes {
				pass.set_pipeline(&self.axes_pipeline);
				pass.set_bind_group(0, &self.bind_group, &[]);
				pass.draw(0..6, 0..1);
			}
		} else {
			let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
				label: Some("screenshot-main"),
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: main_color,
					depth_slice: None,
					resolve_target: main_resolve,
					ops: wgpu::Operations {
						load: wgpu::LoadOp::Clear(clear_color),
						store: wgpu::StoreOp::Store,
					},
				})],
				depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
					view: main_depth,
					depth_ops: Some(wgpu::Operations {
						load: wgpu::LoadOp::Clear(1.0),
						store: wgpu::StoreOp::Store,
					}),
					stencil_ops: None,
				}),
				timestamp_writes: None,
				occlusion_query_set: None,
				multiview_mask: None,
			});
			if draw_scene {
				if let Some(sm) = &self.scene_meshes {
					sm.draw_opaque(&mut pass);
					if draw_contact_shadow_in_main {
						self.write_contact_shadow_uniform();
						self.draw_contact_shadow(&mut pass);
					}
					sm.draw_toon_outlines(&mut pass);
					sm.draw_blended(&mut pass);
				}
			} else {
				pass.set_pipeline(&self.pipeline);
				pass.set_bind_group(0, &self.bind_group, &[]);
				pass.draw(0..3, 0..1);
			}
			if self.show_axes && draw_scene {
				pass.set_pipeline(&self.axes_pipeline);
				pass.set_bind_group(0, &self.bind_group, &[]);
				pass.draw(0..6, 0..1);
			}
		}
		if post.is_some() {
			{
				let post = post.as_mut().expect("post target is initialized");
				match self.aa {
					AaMode::Fxaa => post.encode_fxaa(
						&self.device,
						&self.queue,
						&mut encoder,
						&target_view,
						self.environment_color,
						self.bloom,
						self.ssao,
					),
					AaMode::Smaa => post.encode_smaa(
						&self.device,
						&self.queue,
						&mut encoder,
						&target_view,
						self.environment_color,
						self.bloom,
						self.ssao,
					),
					AaMode::Off | AaMode::Msaa => {
						if use_color_adjust || use_bloom || use_ssao {
							post.encode_color_adjust(
								&self.device,
								&self.queue,
								&mut encoder,
								&target_view,
								self.environment_color,
								self.bloom,
								self.ssao,
							);
						} else {
							post.encode_fxaa(
								&self.device,
								&self.queue,
								&mut encoder,
								&target_view,
								self.environment_color,
								self.bloom,
								self.ssao,
							);
						}
					}
				}
			}
			if draw_contact_shadow && use_avatar_outline {
				self.write_contact_shadow_uniform();
				let shadow_depth = post.as_ref().expect("post target is initialized").depth_view();
				let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
					label: Some("screenshot-contact-shadow"),
					color_attachments: &[Some(wgpu::RenderPassColorAttachment {
						view: &target_view,
						depth_slice: None,
						resolve_target: None,
						ops: wgpu::Operations {
							load: wgpu::LoadOp::Load,
							store: wgpu::StoreOp::Store,
						},
					})],
					depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
						view: shadow_depth,
						depth_ops: Some(wgpu::Operations {
							load: wgpu::LoadOp::Load,
							store: wgpu::StoreOp::Store,
						}),
						stencil_ops: None,
					}),
					timestamp_writes: None,
					occlusion_query_set: None,
					multiview_mask: None,
				});
				self.draw_contact_shadow(&mut pass);
			}
			if use_avatar_outline {
				let width_px = self.avatar_outline_width_px_for(w, h);
				let post = post.as_mut().expect("post target is initialized");
				post.encode_avatar_outline(&self.device, &self.queue, &mut encoder, &target_view, self.avatar_outline, width_px);
			}
		}

		let unpadded_bpr = w * 4;
		let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
		let padded_bpr = unpadded_bpr.div_ceil(align) * align;
		let staging_size = (padded_bpr as u64) * (h as u64);
		let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("screenshot-staging"),
			size: staging_size,
			usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
			mapped_at_creation: false,
		});
		encoder.copy_texture_to_buffer(
			wgpu::TexelCopyTextureInfo {
				texture: &target_tex,
				mip_level: 0,
				origin: wgpu::Origin3d::ZERO,
				aspect: wgpu::TextureAspect::All,
			},
			wgpu::TexelCopyBufferInfo {
				buffer: &staging,
				layout: wgpu::TexelCopyBufferLayout {
					offset: 0,
					bytes_per_row: Some(padded_bpr),
					rows_per_image: Some(h),
				},
			},
			wgpu::Extent3d {
				width: w,
				height: h,
				depth_or_array_layers: 1,
			},
		);
		self.queue.submit(std::iter::once(encoder.finish()));

		staging.slice(..).map_async(wgpu::MapMode::Read, |_| ());
		self.device.poll(wgpu::PollType::wait_indefinitely()).ok();

		let mut rgba = vec![0u8; (w as usize) * (h as usize) * 4];
		{
			let view = staging.slice(..).get_mapped_range();
			let row_dst = (w as usize) * 4;
			let row_src = padded_bpr as usize;
			match format {
				wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => {
					for y in 0..(h as usize) {
						let s = y * row_src;
						let d = y * row_dst;
						for x in 0..(w as usize) {
							rgba[d + x * 4] = view[s + x * 4 + 2];
							rgba[d + x * 4 + 1] = view[s + x * 4 + 1];
							rgba[d + x * 4 + 2] = view[s + x * 4];
							rgba[d + x * 4 + 3] = view[s + x * 4 + 3];
						}
					}
				}
				_ => {
					for y in 0..(h as usize) {
						let s = y * row_src;
						let d = y * row_dst;
						rgba[d..d + row_dst].copy_from_slice(&view[s..s + row_dst]);
					}
				}
			}
		}
		staging.unmap();

		if let Some(parent) = path.parent() {
			if !parent.as_os_str().is_empty() {
				std::fs::create_dir_all(parent).map_err(|e| format!("create screenshot dir {}: {e}", parent.display()))?;
			}
		}
		image::save_buffer(path, &rgba, w, h, image::ColorType::Rgba8).map_err(|e| format!("save screenshot {}: {e}", path.display()))
	}

	pub fn set_camera_orbit(&mut self, longitude: Option<f32>, latitude: Option<f32>, radius: Option<f32>) {
		self.camera.set_orbit(longitude, latitude, radius);
		let (gw, gh) = self.render_pixel_dims();
		self.write_globals(gw, gh);
	}

	pub fn set_spout_output(&mut self, enabled: bool, spout_opts: SpoutWindowOptions) -> bool {
		#[cfg(windows)]
		{
			if !enabled {
				self.spout = None;
				self.spout_launch = None;
				let (gw, gh) = self.render_pixel_dims();
				self.write_globals(gw, gh);
				return false;
			}
			let name = if spout_opts.name.is_empty() {
				"UN Avatar".to_string()
			} else {
				spout_opts.name.clone()
			};
			let launch = crate::spout::SpoutLaunchConfig {
				name,
				width: spout_opts.width,
				height: spout_opts.height,
			};
			self.spout = crate::spout::SpoutCapture::try_new(
				&self.device,
				self.config.format,
				self.config.width,
				self.config.height,
				launch.clone(),
			);
			self.spout_launch = Some(launch);
			let (gw, gh) = self.render_pixel_dims();
			self.write_globals(gw, gh);
			self.spout.is_some()
		}
		#[cfg(not(windows))]
		{
			let _ = (enabled, spout_opts);
			false
		}
	}

	pub fn spout_active(&self) -> bool {
		#[cfg(windows)]
		{
			self.spout.is_some()
		}
		#[cfg(not(windows))]
		{
			false
		}
	}

	#[cfg(windows)]
	pub(crate) fn spout_stats(&self) -> Option<crate::spout::SpoutFrameStats> {
		self.spout.as_ref().map(|spout| spout.stats())
	}

	pub(crate) fn texture_summary(&self) -> Option<TextureUploadSummary> {
		self.texture_summary.clone()
	}

	pub(crate) fn active_wardrobe_set(&self) -> Option<String> {
		let doc_arc = self.document.as_ref()?;
		let doc = doc_arc.read().ok()?;
		doc.runtime_model().active_wardrobe_set().map(str::to_owned)
	}

	pub(crate) fn active_asset_groups(&self) -> Vec<String> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Vec::new();
		};
		let Ok(doc) = doc_arc.read() else {
			return Vec::new();
		};
		doc.runtime_model().active_asset_groups().to_vec()
	}

	pub(crate) fn active_wardrobe_residency_gaps(&self) -> Option<SceneMeshActiveResidencyGaps> {
		self.scene_meshes.as_ref().map(SceneMeshes::active_residency_gaps)
	}

	pub(crate) fn wardrobe_asset_upload_plan(&self) -> WardrobeAssetUploadPlan {
		let Some(doc_arc) = self.document.as_ref() else {
			return WardrobeAssetUploadPlan::default();
		};
		let Ok(doc) = doc_arc.read() else {
			return WardrobeAssetUploadPlan::default();
		};
		let active_gaps = self.active_wardrobe_residency_gaps();
		let scoped_upload_work = wardrobe_scoped_upload_work_for_active_gaps(active_gaps.clone());
		let draw_counts = self.scene_meshes.as_ref().map(SceneMeshes::asset_residency_counts);
		let mut plan = wardrobe_asset_upload_plan_with_draw_counts(wardrobe_asset_upload_plan_for_document(&doc), draw_counts);
		plan.pending_image_texture_upload_count = scoped_upload_work.image_texture_indices.len();
		plan.pending_material_slot_upload_count = scoped_upload_work.material_slot_indices.len();
		plan.active_residency_gaps_detected |= scoped_upload_work.has_pending_uploads();
		plan.last_residency_refresh_active_draw_change_count = self.last_asset_residency_refresh.active_draw_state_changed_count;
		plan.last_residency_refresh_image_load_count = self.last_asset_residency_refresh.image_texture_load_indices.len();
		plan.last_residency_refresh_image_unload_count = self.last_asset_residency_refresh.image_texture_unload_indices.len();
		plan.last_residency_refresh_material_load_count = self.last_asset_residency_refresh.material_slot_load_indices.len();
		plan.last_residency_refresh_material_unload_count = self.last_asset_residency_refresh.material_slot_unload_indices.len();
		plan.last_mesh_buffer_scoped_load_count = self.last_mesh_buffer_scoped_load_count;
		plan.last_mesh_buffer_scoped_unload_count = self.last_mesh_buffer_scoped_unload_count;
		plan.last_image_texture_scoped_load_count = self.last_image_texture_scoped_load_count;
		plan.last_image_texture_scoped_unload_count = self.last_image_texture_scoped_unload_count;
		plan.last_cubemap_scoped_load_count = self.last_cubemap_scoped_load_count;
		plan.last_cubemap_scoped_unload_count = self.last_cubemap_scoped_unload_count;
		plan.last_material_slot_scoped_upload_count = self.last_material_slot_scoped_upload_count;
		plan
	}

	pub(crate) fn resolver_cache_key(&self) -> Option<UnaRuntimeResolverCacheKey> {
		let doc_arc = self.document.as_ref()?;
		let doc = doc_arc.read().ok()?;
		Some(doc.runtime_model().resolver_cache_key())
	}

	pub(crate) fn last_action_id(&self) -> Option<String> {
		let doc_arc = self.document.as_ref()?;
		let doc = doc_arc.read().ok()?;
		doc.runtime_model().last_action_id().map(str::to_owned)
	}

	pub(crate) fn runtime_parameter_values(&self) -> BTreeMap<String, f32> {
		let Some(doc_arc) = self.document.as_ref() else {
			return BTreeMap::new();
		};
		let Ok(doc) = doc_arc.read() else {
			return BTreeMap::new();
		};
		doc.runtime_model().runtime_parameter_values().clone()
	}

	pub(crate) fn runtime_parameter_definitions(&self) -> Vec<un_avatar_core::UnaRuntimeParameterDefinition> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Vec::new();
		};
		let Ok(doc) = doc_arc.read() else {
			return Vec::new();
		};
		doc.runtime_model().runtime_parameter_definitions()
	}

	pub(crate) fn runtime_parameter_conflicts(&self) -> Vec<un_avatar_core::UnaRuntimeParameterConflict> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Vec::new();
		};
		let Ok(doc) = doc_arc.read() else {
			return Vec::new();
		};
		doc.runtime_model().runtime_parameter_conflicts()
	}

	pub(crate) fn wardrobe_actions(&self) -> Vec<RuntimeWardrobeActionStatus> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Vec::new();
		};
		let Ok(doc) = doc_arc.read() else {
			return Vec::new();
		};
		doc.runtime_model()
			.runtime_actions()
			.map(wardrobe_action_statuses)
			.unwrap_or_default()
	}

	pub(crate) fn runtime_actions(&self) -> Vec<RuntimeActionStatus> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Vec::new();
		};
		let Ok(doc) = doc_arc.read() else {
			return Vec::new();
		};
		doc.runtime_model()
			.runtime_actions()
			.map(|actions| runtime_action_statuses(actions, doc.runtime_model().scene(), doc.runtime_model().runtime_parameter_values()))
			.unwrap_or_default()
	}

	pub(crate) fn runtime_action_target_write_collisions(&self) -> Vec<un_avatar_core::UnaEvaluationTargetWriteCollision> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Vec::new();
		};
		let Ok(doc) = doc_arc.read() else {
			return Vec::new();
		};
		doc.runtime_model()
			.runtime_actions()
			.map(|actions| actions.evaluation_target_write_collisions())
			.unwrap_or_default()
	}

	pub(crate) fn runtime_action_restore_readiness(&self) -> Vec<un_avatar_core::UnaEvaluationRestoreReadiness> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Vec::new();
		};
		let Ok(doc) = doc_arc.read() else {
			return Vec::new();
		};
		let runtime = doc.runtime_model();
		runtime
			.runtime_actions()
			.map(|actions| runtime.runtime_action_set_restore_readiness(actions))
			.unwrap_or_default()
	}

	pub(crate) fn runtime_action_restore_baseline_candidates(&self) -> Vec<un_avatar_core::UnaEvaluationRestoreBaselineCandidate> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Vec::new();
		};
		let Ok(doc) = doc_arc.read() else {
			return Vec::new();
		};
		let runtime = doc.runtime_model();
		runtime
			.runtime_actions()
			.map(|actions| runtime.runtime_action_set_restore_baseline_candidates(actions))
			.unwrap_or_default()
	}

	pub(crate) fn runtime_action_restore_baseline_capture_plan(&self) -> Vec<un_avatar_core::UnaEvaluationRestoreBaselineEntry> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Vec::new();
		};
		let Ok(doc) = doc_arc.read() else {
			return Vec::new();
		};
		let runtime = doc.runtime_model();
		runtime
			.runtime_actions()
			.map(|actions| runtime.runtime_action_set_restore_baseline_capture_plan(actions))
			.unwrap_or_default()
	}

	pub(crate) fn runtime_action_restore_apply_plan(&self) -> Vec<un_avatar_core::UnaEvaluationRestoreApplyEntry> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Vec::new();
		};
		let Ok(doc) = doc_arc.read() else {
			return Vec::new();
		};
		let runtime = doc.runtime_model();
		runtime
			.runtime_actions()
			.map(|actions| runtime.runtime_action_set_restore_apply_plan(actions))
			.unwrap_or_default()
	}

	pub(crate) fn menu_action_candidates(&self) -> Vec<RuntimeMenuActionCandidateStatus> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Vec::new();
		};
		let Ok(doc) = doc_arc.read() else {
			return Vec::new();
		};
		doc.runtime_model()
			.runtime_actions()
			.and_then(|actions| menu_action_candidates_from_runtime(doc.unavatar.as_ref(), actions))
			.unwrap_or_default()
	}

	pub(crate) fn menu_wardrobe_candidates(&self) -> Vec<RuntimeMenuWardrobeCandidateStatus> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Vec::new();
		};
		let Ok(doc) = doc_arc.read() else {
			return Vec::new();
		};
		let action_candidates = doc
			.runtime_model()
			.runtime_actions()
			.and_then(|actions| menu_action_candidates_from_runtime(doc.unavatar.as_ref(), actions));
		let menu_action_candidates = match action_candidates {
			Some(candidates) => candidates,
			None => return Vec::new(),
		};
		menu_wardrobe_candidates_from_runtime(doc.unavatar.as_ref(), &menu_action_candidates)
	}

	pub(crate) fn contact_parameter_declarations(&self) -> Vec<RuntimeContactParameterDeclarationStatus> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Vec::new();
		};
		let Ok(doc) = doc_arc.read() else {
			return Vec::new();
		};
		contact_parameter_declaration_statuses(&doc)
	}

	pub(crate) fn contact_parameter_emission_enabled(&self) -> bool {
		let Some(doc_arc) = self.document.as_ref() else {
			return false;
		};
		let Ok(doc) = doc_arc.read() else {
			return false;
		};
		doc.runtime_model().contact_parameter_emission_enabled()
	}

	pub(crate) fn contact_probe_status(&self) -> RuntimeContactProbeStatusSummary {
		let Some(doc_arc) = self.document.as_ref() else {
			return RuntimeContactProbeStatusSummary::default();
		};
		let Ok(doc) = doc_arc.read() else {
			return RuntimeContactProbeStatusSummary::default();
		};
		contact_probe_status_summary(&doc)
	}

	pub(crate) fn contact_parameter_emission_status(&self) -> RuntimeContactParameterEmissionStatusSummary {
		let Some(doc_arc) = self.document.as_ref() else {
			return RuntimeContactParameterEmissionStatusSummary::default();
		};
		let Ok(doc) = doc_arc.read() else {
			return RuntimeContactParameterEmissionStatusSummary::default();
		};
		if !doc.runtime_model().contact_parameter_emission_enabled() {
			return RuntimeContactParameterEmissionStatusSummary::default();
		}
		contact_parameter_emission_status_summary(&doc)
	}

	pub(crate) fn apply_contact_parameter_emissions(&mut self) -> Result<BTreeMap<String, f32>, String> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Ok(BTreeMap::new());
		};
		let mut doc = doc_arc.write().map_err(|_| "document: RwLock poisoned".to_string())?;
		let before = doc.runtime_model().runtime_parameter_values().clone();
		let emissions = doc.runtime_model_mut().apply_contact_parameter_emissions();
		if emissions.is_empty() {
			return Ok(BTreeMap::new());
		}
		let after = doc.runtime_model().runtime_parameter_values();
		Ok(emissions
			.into_iter()
			.filter_map(|emission| {
				let value = after.get(&emission.parameter).copied()?;
				if before.get(&emission.parameter).copied() == Some(value) {
					return None;
				}
				Some((emission.parameter, value))
			})
			.collect())
	}

	fn apply_restored_runtime_action_effects(&mut self, restored: &[un_avatar_core::UnaEvaluationRestoreApplyEntry]) {
		if restored.is_empty() {
			return;
		}
		if restored
			.iter()
			.any(|entry| entry.target_kind == UnaEvaluationTargetKind::DynamicsEnabled)
		{
			self.reset_dynamics_nodes_to_rest();
			self.rebuild_runtime_dynamics();
		}
		self.invalidate_applied_document_state();
	}

	pub(crate) fn dynamics_groups(&self) -> Vec<RuntimeDynamicsGroupStatus> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Vec::new();
		};
		let Ok(doc) = doc_arc.read() else {
			return Vec::new();
		};
		dynamics_group_statuses(&doc)
	}

	pub(crate) fn dynamics_interaction_hooks(&self) -> Vec<RuntimeDynamicsInteractionHookStatus> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Vec::new();
		};
		let Ok(doc) = doc_arc.read() else {
			return Vec::new();
		};
		dynamics_interaction_hook_statuses(&doc)
	}

	pub(crate) fn dynamics_colliders(&self) -> Vec<RuntimeDynamicsColliderStatus> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Vec::new();
		};
		let Ok(doc) = doc_arc.read() else {
			return Vec::new();
		};
		dynamics_collider_statuses(&doc)
	}

	pub(crate) fn dynamics_constraint_refs(&self) -> Vec<RuntimeDynamicsConstraintRefStatus> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Vec::new();
		};
		let Ok(doc) = doc_arc.read() else {
			return Vec::new();
		};
		dynamics_constraint_ref_statuses(&doc)
	}

	pub(crate) fn set_runtime_parameter(&mut self, name: &str, value: f32) -> Result<Option<RuntimeActionActivation>, String> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Err("document is not attached".to_string());
		};
		let doc_arc = Arc::clone(doc_arc);
		{
			let mut doc = doc_arc.write().map_err(|_| "document: RwLock poisoned".to_string())?;
			doc.runtime_model_mut().set_runtime_parameter_value(name.to_string(), value);
		}
		let (matching_action_ids, actions_snapshot) = {
			let doc = doc_arc.read().map_err(|_| "document: RwLock poisoned".to_string())?;
			let runtime = doc.runtime_model();
			let Some(actions) = runtime.runtime_actions() else {
				return Ok(None);
			};
			(
				runtime_action_ids_for_parameter(actions, runtime.scene(), name, value),
				actions.clone(),
			)
		};
		let mut last_activation = None;
		for action_id in matching_action_ids {
			last_activation = Some(self.activate_runtime_action(Some(&action_id), None, None, None, None)?);
		}
		if last_activation.is_none() {
			let mut doc = doc_arc.write().map_err(|_| "document: RwLock poisoned".to_string())?;
			let restored = doc.runtime_model_mut().restore_inactive_runtime_action_effects(&actions_snapshot)?;
			drop(doc);
			self.apply_restored_runtime_action_effects(&restored);
		}
		self.last_runtime_parameter_action_values = self.runtime_parameter_values();
		Ok(last_activation)
	}

	pub(crate) fn evaluate_runtime_parameter_actions(&mut self) -> Result<Vec<RuntimeActionActivation>, String> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Ok(Vec::new());
		};
		let doc_arc = Arc::clone(doc_arc);
		let (parameter_values, action_ids, actions_snapshot) = {
			let doc = doc_arc.read().map_err(|_| "document: RwLock poisoned".to_string())?;
			let parameter_values = doc.runtime_model().runtime_parameter_values().clone();
			if parameter_values == self.last_runtime_parameter_action_values {
				return Ok(Vec::new());
			}
			let Some(actions) = doc.runtime_model().runtime_actions() else {
				self.last_runtime_parameter_action_values = parameter_values;
				return Ok(Vec::new());
			};
			(
				parameter_values.clone(),
				runtime_action_ids_for_parameter_values(actions, doc.runtime_model().scene(), &parameter_values),
				actions.clone(),
			)
		};
		self.last_runtime_parameter_action_values = parameter_values;
		if action_ids.is_empty() {
			let mut doc = doc_arc.write().map_err(|_| "document: RwLock poisoned".to_string())?;
			let restored = doc.runtime_model_mut().restore_inactive_runtime_action_effects(&actions_snapshot)?;
			drop(doc);
			self.apply_restored_runtime_action_effects(&restored);
			return Ok(Vec::new());
		}
		let mut activations = Vec::new();
		for action_id in action_ids {
			activations.push(self.activate_runtime_action(Some(&action_id), None, None, None, None)?);
		}
		Ok(activations)
	}

	pub(crate) fn apply_wardrobe_set(&mut self, set_id: &str) -> Result<(), String> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Err("document is not attached".to_string());
		};
		let mut doc = doc_arc.write().map_err(|_| "document: RwLock poisoned".to_string())?;
		crate::model_loader::apply_required_wardrobe_set(&mut doc, set_id)?;
		drop(doc);
		self.reset_dynamics_nodes_to_rest();
		self.rebuild_runtime_dynamics();
		self.invalidate_applied_document_state();
		Ok(())
	}

	fn set_runtime_dynamics_enabled(&mut self, source_id: &str, enabled: bool) -> Result<(), String> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Err("document is not attached".to_string());
		};
		let rest_nodes = self.rest_nodes.as_ref().map(Arc::clone);
		let mut doc = doc_arc.write().map_err(|_| "document: RwLock poisoned".to_string())?;
		let Some(mut runtime) = doc.runtime_scene_and_dynamics_mut() else {
			return Err("document has no runtime scene".to_string());
		};
		if !runtime.dynamics.set_group_enabled_by_source_id(source_id, enabled) {
			return Err(format!("runtime dynamics source_id `{source_id}` not found"));
		}
		if let Some(rest_nodes) = rest_nodes.as_ref() {
			reset_runtime_dynamics_nodes_to_rest_for_source_id(runtime.scene, runtime.dynamics.as_readonly(), rest_nodes, source_id);
		}
		drop(doc);
		self.rebuild_runtime_dynamics();
		self.invalidate_applied_document_state();
		Ok(())
	}

	fn set_runtime_node_visible(&mut self, target: &un_avatar_core::UnaRuntimeNodeTarget, visible: bool) -> Result<(), String> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Err("document is not attached".to_string());
		};
		let mut doc = doc_arc.write().map_err(|_| "document: RwLock poisoned".to_string())?;
		if !doc.runtime_model_mut().set_node_visible(target, visible) {
			return Err(format!("runtime node target not found: {target:?}"));
		}
		drop(doc);
		self.invalidate_applied_document_state();
		Ok(())
	}

	fn set_runtime_material_color(
		&mut self,
		target: &un_avatar_core::UnaRuntimeMaterialTarget,
		parameter: &str,
		color: [f32; 4],
	) -> Result<(), String> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Err("document is not attached".to_string());
		};
		let mut doc = doc_arc.write().map_err(|_| "document: RwLock poisoned".to_string())?;
		doc.runtime_model_mut().set_material_color(target, parameter, color)?;
		drop(doc);
		self.invalidate_applied_document_state();
		Ok(())
	}

	fn set_runtime_material_scalar(
		&mut self,
		target: &un_avatar_core::UnaRuntimeMaterialTarget,
		parameter: &str,
		value: f32,
	) -> Result<(), String> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Err("document is not attached".to_string());
		};
		let mut doc = doc_arc.write().map_err(|_| "document: RwLock poisoned".to_string())?;
		doc.runtime_model_mut().set_material_scalar(target, parameter, value)?;
		drop(doc);
		self.invalidate_applied_document_state();
		Ok(())
	}

	fn set_runtime_material_slot(
		&mut self,
		target: &un_avatar_core::UnaRuntimeMaterialSlotTarget,
		material: Option<&un_avatar_core::UnaRuntimeMaterialTarget>,
	) -> Result<(), String> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Err("document is not attached".to_string());
		};
		let mut doc = doc_arc.write().map_err(|_| "document: RwLock poisoned".to_string())?;
		doc.runtime_model_mut().set_material_slot(target, material)?;
		drop(doc);
		self.invalidate_applied_document_state();
		Ok(())
	}

	pub(crate) fn activate_runtime_action(
		&mut self,
		action_id: Option<&str>,
		command: Option<&str>,
		expression_menu_path: Option<&str>,
		parameter_name: Option<&str>,
		parameter_value: Option<f32>,
	) -> Result<RuntimeActionActivation, String> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Err("document is not attached".to_string());
		};
		let doc_arc = Arc::clone(doc_arc);
		let (resolved_action_id, parameter_values, action, actions_snapshot) = {
			let doc = doc_arc.read().map_err(|_| "document: RwLock poisoned".to_string())?;
			let Some(actions) = doc.runtime_model().runtime_actions() else {
				return Err("document has no runtime actions".to_string());
			};
			let action = actions
				.find_action(UnaRuntimeActionQuery {
					action_id,
					supervisor_command: command,
					expression_menu_path,
					parameter_name,
					parameter_value,
				})
				.ok_or_else(|| "runtime action not found".to_string())?;
			(action.id.clone(), action.parameter_assignments(), action.clone(), actions.clone())
		};
		{
			let mut doc = doc_arc.write().map_err(|_| "document: RwLock poisoned".to_string())?;
			doc.runtime_model_mut()
				.capture_runtime_action_restore_baselines(&un_avatar_core::UnaRuntimeActionSet {
					actions: vec![action.clone()],
				});
		}
		let mut active_wardrobe_set = None;
		for effect in action.effects {
			match effect {
				UnaRuntimeActionEffect::WardrobeSet { set_id } => {
					self.apply_wardrobe_set(&set_id)?;
					active_wardrobe_set = Some(
						crate::model_loader::require_wardrobe_set_id(&set_id)
							.map(str::to_owned)
							.unwrap_or(set_id),
					);
				}
				UnaRuntimeActionEffect::DynamicsEnabled { source_id, enabled } => {
					self.set_runtime_dynamics_enabled(&source_id, enabled)?;
				}
				UnaRuntimeActionEffect::ExpressionWeight { name, weight } => {
					self.set_expression_override(&name, weight);
				}
				UnaRuntimeActionEffect::NodeVisibility { target, visible } => {
					self.set_runtime_node_visible(&target, visible)?;
				}
				UnaRuntimeActionEffect::MaterialColor { target, parameter, color } => {
					self.set_runtime_material_color(&target, &parameter, color)?;
				}
				UnaRuntimeActionEffect::MaterialScalar { target, parameter, value } => {
					self.set_runtime_material_scalar(&target, &parameter, value)?;
				}
				UnaRuntimeActionEffect::MaterialSlot { target, material } => {
					self.set_runtime_material_slot(&target, material.as_ref())?;
				}
			}
		}
		let mut doc = doc_arc.write().map_err(|_| "document: RwLock poisoned".to_string())?;
		doc.runtime_model_mut().set_last_action_id(Some(resolved_action_id.clone()));
		doc.runtime_model_mut().set_runtime_parameter_values(parameter_values.clone());
		let restored = doc.runtime_model_mut().restore_inactive_runtime_action_effects(&actions_snapshot)?;
		drop(doc);
		self.apply_restored_runtime_action_effects(&restored);
		self.last_runtime_parameter_action_values = self.runtime_parameter_values();
		Ok(RuntimeActionActivation {
			action_id: resolved_action_id,
			active_wardrobe_set,
			parameter_values,
		})
	}

	pub(crate) fn scene_build_context(&self) -> GpuSceneBuildContext {
		GpuSceneBuildContext {
			device: self.device.clone(),
			queue: self.queue.clone(),
			format: self.config.format,
			aa: self.aa,
			shader_variant_tier: self.shader_variant_tier,
		}
	}

	pub(crate) fn attach_prepared_document(
		&mut self,
		prepared: PreparedDocumentScene,
		options: DocumentAttachOptions,
	) -> Result<(), String> {
		let DocumentAttachOptions {
			vmc_address,
			unmotion_zenoh,
			audio_link,
			debug_vmc,
			enable_spring_bones,
			bone_colliders,
			spring_bone_physics,
			..
		} = options;
		self.runtime_dynamics_enabled = enable_spring_bones;
		self.runtime_bone_collider_config = bone_colliders;
		self.runtime_dynamics_physics = spring_bone_physics;
		self.expression_presets = prepared.expression_presets;
		self.rest_nodes = prepared.rest_nodes;
		prepared
			.document
			.write()
			.map_err(|_| "document: RwLock poisoned".to_string())?
			.runtime_model_mut()
			.apply_runtime_parameter_initial_values();
		self.document = Some(prepared.document);
		self.invalidate_applied_document_state();
		self.scene_meshes = prepared.scene_meshes;
		self.texture_summary = prepared.texture_summary;
		self.dynamics_sim = prepared.dynamics_sim;
		self.bone_colliders = prepared.bone_colliders;
		self.bone_collider_count = prepared.bone_collider_count;
		self.bone_collider_source = prepared.bone_collider_source;
		self.apply_runtime_requirements(prepared.runtime_requirements, audio_link);
		self.reconfigure_motion_receivers(vmc_address, unmotion_zenoh, debug_vmc)?;
		let (gw, gh) = self.render_pixel_dims();
		self.globals_uploaded = None;
		self.write_globals(gw, gh);
		Ok(())
	}
}

impl GpuSceneBuildContext {
	pub(crate) fn prepare_document_scene(
		self,
		document: Arc<UnaDocument>,
		options: &DocumentAttachOptions,
		mut progress: impl FnMut(SceneMeshBuildProgress),
	) -> Result<PreparedDocumentScene, String> {
		let GpuSceneBuildContext {
			device,
			queue,
			format,
			aa,
			shader_variant_tier,
		} = self;
		let document = Arc::try_unwrap(document).unwrap_or_else(|document| (*document).clone());
		let runtime_model = document.runtime_model();
		let physics = build_runtime_physics_for_document(
			&document,
			options.enable_spring_bones,
			options.bone_colliders,
			&options.spring_bone_physics,
		);
		let needs_rest_nodes = runtime_model.has_humanoid_scene() || physics.dynamics_sim.is_some();
		let rest_nodes = if needs_rest_nodes {
			runtime_model.scene_nodes().map(|nodes| Arc::new(nodes.to_vec()))
		} else {
			None
		};
		let expression_presets = expression_preset_names(runtime_model.expression_catalog());
		let mut scene_meshes = None;
		let mut texture_summary = None;
		let mut runtime_requirements = SceneMeshRuntimeRequirements::default();
		if let Some(runtime) = runtime_model.scene_expression_catalog() {
			if options.debug_material_dump {
				log_material_skin_report(&document);
			}
			let mut gpu_texture_compression = if options.block_compression_encoder == BlockCompressionEncoder::Gpu
				&& !matches!(
					options.texture_compression,
					TextureCompressionMode::Source | TextureCompressionMode::Compat
				) {
				Some(crate::texture_pipeline::create_vulkan_gpu_texture_compression_context()?)
			} else {
				None
			};
			let mut sm = SceneMeshes::new(
				&device,
				&queue,
				format,
				aa_sample_count(aa),
				shader_variant_tier,
				runtime.scene,
				runtime.expression_catalog,
				runtime_model.active_asset_groups(),
				options.mesh_diagnostics.clone(),
				options.texture_max_dimension,
				options.texture_compression,
				options.block_compression_encoder,
				options.block_compression_cpu_threads,
				options.mipmap_filter,
				&options.texture_compression_advanced,
				options.texture_compression_bc_supported,
				options.texture_compression_astc_supported,
				options.texture_compression_etc2_supported,
				options.processed_texture_cache,
				gpu_texture_compression.as_mut(),
				&mut progress,
			)?;
			if !sm.is_empty() {
				texture_summary = Some(sm.texture_summary());
				let world = crate::scene_transform::scene_world_matrices(runtime.scene);
				let expression_weights = active_expression_weights_for_doc(false, &document);
				sm.refresh_asset_group_residency(runtime.scene, runtime_model.active_asset_groups());
				sm.update_draw_transforms(&queue, runtime.scene, &world, expression_weights, None, true);
				runtime_requirements = sm.runtime_requirements();
				if runtime_requirements.audio_link_texture && options.audio_link.source == AudioLinkSource::InputDevice {
					eprintln!("un-avatar-renderer: external AudioLink texture needed by visible material set");
				}
				scene_meshes = Some(sm);
			}
		}
		let document_wrapped = Arc::new(RwLock::new(document));
		Ok(PreparedDocumentScene {
			document: document_wrapped,
			rest_nodes,
			scene_meshes,
			texture_summary,
			dynamics_sim: physics.dynamics_sim,
			bone_colliders: physics.debug_bone_colliders,
			bone_collider_count: physics.stats.count,
			bone_collider_source: physics.stats.source,
			runtime_requirements,
			expression_presets,
		})
	}
}

impl GpuState {
	pub fn reconfigure_motion_receivers(
		&mut self,
		vmc_address: Option<SocketAddr>,
		unmotion_zenoh: crate::options::UnmotionZenohOptions,
		debug_vmc: bool,
	) -> Result<(), String> {
		let generation = self.motion_receiver_generation.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
		if self.vmc_live || self.unmotion_zenoh_live {
			std::thread::sleep(Duration::from_millis(60));
		}
		self.vmc_live = false;
		self.unmotion_zenoh_live = false;
		self.unmotion_zenoh_received_frames.store(0, Ordering::Relaxed);
		self.start_motion_receivers(vmc_address, unmotion_zenoh, debug_vmc, generation)
	}

	fn start_motion_receivers(
		&mut self,
		vmc_address: Option<SocketAddr>,
		unmotion_zenoh: crate::options::UnmotionZenohOptions,
		debug_vmc: bool,
		generation: u64,
	) -> Result<(), String> {
		let Some(doc_arc) = self.document.as_ref().map(Arc::clone) else {
			self.motion_retarget_runtime = None;
			if vmc_address.is_some() {
				eprintln!("un-avatar-renderer: --vmc-address は --gltf でモデルを読み込んだときに指定してください");
			}
			if unmotion_zenoh.enabled {
				eprintln!("un-avatar-renderer: UNMotion/Zenoh 受信を有効化したが、モデル (--gltf) が指定されていません");
			}
			return Ok(());
		};
		let debug_vmc_log = debug_vmc && self.debug_log.is_enabled();
		let (retarget_runtime, humanoid_keys_csv) = {
			let rest_nodes = self.rest_nodes.as_ref().map(Arc::clone);
			let d = doc_arc.read().map_err(|_| "document: RwLock poisoned".to_string())?;
			(
				rest_nodes.and_then(|rest_nodes| MotionRetargetRuntime::for_document(&d, rest_nodes)),
				if debug_vmc_log && vmc_address.is_some() {
					Some(humanoid_profile_keys_csv(d.runtime_model().humanoid_profile()))
				} else {
					None
				},
			)
		};
		let humanoid_ok = retarget_runtime.is_some();
		self.motion_retarget_runtime = retarget_runtime;
		if let Some(addr) = vmc_address {
			if humanoid_ok {
				let humanoid_keys_csv = humanoid_keys_csv.unwrap_or_default();
				let log = self.debug_log.clone();
				let motion_buffer_for_vmc = Arc::clone(&self.motion_buffer);
				let receiver_generation = Arc::clone(&self.motion_receiver_generation);
				std::thread::Builder::new()
					.name("un-avatar-vmc".into())
					.spawn(move || {
						let mut marionette = match un_avatar_vmc::VmcMarionette::bind(addr) {
							Ok(m) => m,
							Err(e) => {
								eprintln!("[un-avatar-vmc] bind FAILED addr={addr}: {e}");
								if debug_vmc_log {
									log.line("vmc", format!("bind_failed {addr}: {e}"));
								}
								return;
							}
						};
						match marionette.local_addr() {
							Ok(local) => eprintln!("[un-avatar-vmc] bind OK requested={addr} local={local}"),
							Err(e) => eprintln!("[un-avatar-vmc] bind OK requested={addr} but local_addr() failed: {e}"),
						}
						if debug_vmc_log {
							log.line("vmc", format!("thread_start bind={addr} humanoid_profile_keys={humanoid_keys_csv}"));
						}
						let mut seq = 0u64;
						let mut recv_i = 0u64;
						while receiver_generation.load(Ordering::Acquire) == generation {
							match marionette.recv_and_apply() {
								Ok((from, n, events)) => {
									if n == 0 {
										continue;
									}
									recv_i = recv_i.wrapping_add(1);
									if recv_i == 1 {
										eprintln!(
											"[un-avatar-vmc] first packet received from={from} nbytes={n} ev_count={}",
											events.len()
										);
									}
									if events.is_empty() {
										continue;
									}
									seq = seq.wrapping_add(1);
									let frame = marionette.assemble_frame(seq, un_avatar_vmc::wall_clock_ns());
									motion_buffer_for_vmc.push_frame(frame);
								}
								Err(un_avatar_vmc::RecvApplyError::Io(e)) => {
									if matches!(
										e.kind(),
										std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut | std::io::ErrorKind::Interrupted
									) {
										continue;
									}
									if debug_vmc_log {
										log.line("vmc", format!("recv_io_error: {e}"));
									}
								}
								Err(un_avatar_vmc::RecvApplyError::Decode {
									from,
									nbytes,
									err,
									ref payload_head_hex,
								}) => {
									if debug_vmc_log {
										log.line(
											"vmc",
											format!("recv_decode_error from={from} nbytes={nbytes} err={err} hex_head={payload_head_hex}"),
										);
									}
								}
							}
						}
						if debug_vmc_log {
							log.line("vmc", "thread_stop generation_changed");
						}
					})
					.map_err(|e| format!("spawn un-avatar-vmc thread failed: {e}"))?;
				self.vmc_live = true;
			} else {
				eprintln!("un-avatar-renderer: --vmc-address は Humanoid とシーンがあるモデルでのみ有効です");
				if debug_vmc_log {
					let humanoid_keys_csv = humanoid_keys_csv.unwrap_or_default();
					self.debug_log.line(
						"vmc",
						format!(
							"marionette thread not started (--vmc-address {addr}): need humanoid_profile + scene (keys_if_any={humanoid_keys_csv})"
						),
					);
				}
			}
		}
		if unmotion_zenoh.enabled {
			if humanoid_ok {
				let strategy = un_motion_frame_zenoh::ZenohTopicStrategy {
					base_key_expr: if unmotion_zenoh.base_key_expr.trim().is_empty() {
						"un-motion/frame".to_string()
					} else {
						unmotion_zenoh.base_key_expr.trim().to_string()
					},
					..un_motion_frame_zenoh::ZenohTopicStrategy::default()
				};
				let log_for_recv = self.debug_log.clone();
				let motion_buffer_for_zenoh = Arc::clone(&self.motion_buffer);
				let received_frames_counter = Arc::clone(&self.unmotion_zenoh_received_frames);
				let receiver_generation = Arc::clone(&self.motion_receiver_generation);
				let key_expr_for_log = strategy.subscribe_key_expr();
				match un_avatar_zenoh::UnAvatarZenohReceiver::declare_zenoh_default(strategy) {
					Ok(receiver) => {
						eprintln!("[un-avatar-zenoh] subscribed key='{key_expr_for_log}'");
						if log_for_recv.is_enabled() {
							log_for_recv.line("unmotion_zenoh", format!("subscribed key={key_expr_for_log}"));
						}
						self.unmotion_zenoh_live = true;
						std::thread::Builder::new()
							.name("un-avatar-zenoh-apply".into())
							.spawn(move || {
								const MAX_ZENOH_APPLY_BATCH: usize = 64;
								let mut received = 0u64;
								while receiver_generation.load(Ordering::Acquire) == generation {
									let frames = receiver.drain_available(MAX_ZENOH_APPLY_BATCH);
									if frames.is_empty() {
										std::thread::sleep(std::time::Duration::from_millis(8));
										continue;
									}
									let batch_len = frames.len();
									received_frames_counter.fetch_add(batch_len as u64, Ordering::Relaxed);
									let mut last_seq = None;
									for frame in frames {
										last_seq = Some(frame.header.sequence);
										motion_buffer_for_zenoh.push_frame(frame);
									}
									received = received.wrapping_add(1);
									if log_for_recv.is_enabled() && (received == 1 || received.is_multiple_of(120)) {
										log_for_recv.line(
											"unmotion_zenoh",
											format!(
												"received batch#{received} frames={batch_len} last_seq={}",
												last_seq.unwrap_or_default()
											),
										);
									}
								}
								if log_for_recv.is_enabled() {
									log_for_recv.line("unmotion_zenoh", "thread_stop generation_changed");
								}
							})
							.map_err(|e| format!("spawn un-avatar-zenoh-apply thread failed: {e}"))?;
					}
					Err(e) => {
						eprintln!("[un-avatar-zenoh] declare failed: {e}");
						if log_for_recv.is_enabled() {
							log_for_recv.line("unmotion_zenoh", format!("declare_failed: {e}"));
						}
					}
				}
			} else {
				eprintln!("un-avatar-renderer: UNMotion/Zenoh 受信は Humanoid とシーンがあるモデルでのみ有効です");
			}
		}
		Ok(())
	}

	fn apply_pending_motion_frames(&mut self) {
		self.motion_buffer.take_pending_frames_into(&mut self.pending_motion_frames);
		if self.pending_motion_frames.is_empty() {
			return;
		}
		let Some(retarget_runtime) = self.motion_retarget_runtime.as_ref() else {
			self.pending_motion_frames.clear();
			return;
		};
		let opts = self.motion_apply_opts;
		let Some(doc_arc) = self.document.as_ref() else {
			self.pending_motion_frames.clear();
			return;
		};
		let Ok(mut document) = doc_arc.write() else {
			self.pending_motion_frames.clear();
			return;
		};
		let should_log = self.debug_log.is_enabled() && self.debug_frame_seq.is_multiple_of(120);
		for frame in &self.pending_motion_frames {
			if should_log {
				self.debug_log.line(
					"motion",
					format!(
						"apply seq={} space={:?} {}",
						frame.header.sequence,
						frame.header.coordinate_space,
						unmotion_frame_hand_summary(frame, &document)
					),
				);
			}
			retarget_runtime.apply_frame(&mut document, frame, opts);
		}
		let applied_frame_count = self.pending_motion_frames.len();
		self.pending_motion_frames.clear();
		self.motion_applied_frames.fetch_add(applied_frame_count as u64, Ordering::Relaxed);
		self.mark_document_changed();
	}

	pub fn resize(&mut self, width: u32, height: u32) {
		if width == 0 || height == 0 {
			return;
		}
		let w = width.max(1);
		let h = height.max(1);
		self.config.width = w;
		self.config.height = h;
		self.surface.configure(&self.device, &self.config);
		self.depth_texture.destroy();
		let (tex, view) = create_depth(&self.device, w, h);
		self.depth_texture = tex;
		self.depth_view = view;
		#[cfg(windows)]
		if let (Some(ref mut sp), Some(ref lc)) = (&mut self.spout, &self.spout_launch) {
			sp.resize_to(&self.device, w, h, lc, self.config.format);
		}
		let (gw, gh) = self.render_pixel_dims();
		self.write_globals(gw, gh);
	}

	pub fn set_transparent(&mut self, transparent: bool) {
		let next = if transparent {
			transparent_alpha_mode(&self.alpha_modes)
		} else if self.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
			wgpu::CompositeAlphaMode::Opaque
		} else {
			self.alpha_modes[0]
		};
		if self.config.alpha_mode == next {
			return;
		}
		self.config.alpha_mode = next;
		self.surface.configure(&self.device, &self.config);
	}

	fn write_globals(&mut self, width: u32, height: u32) {
		self.write_frame_globals(width, height, false);
	}

	fn write_frame_globals(&mut self, width: u32, height: u32, advance_audio_link: bool) {
		let aspect = width.max(1) as f32 / height.max(1) as f32;
		let diagonal_rad = self.camera.diagonal_fov_deg.to_radians();
		let fovy = vertical_fov_from_diagonal(diagonal_rad, aspect);
		let proj = Mat4::perspective_rh(fovy, aspect, CAMERA_NEAR_CLIP_M, CAMERA_FAR_CLIP_M);
		let cam_pos = self.camera.position();
		let look_at = self.camera.target;
		let view = Mat4::look_at_rh(cam_pos, look_at, Vec3::Y);
		let view_proj = proj * view;
		let inv_view_proj = view_proj.inverse();
		let light_dir = self.directional_light_dir(cam_pos, look_at, view);
		let light = Vec4::from((light_dir, 0.0));
		let directional_light_color = self.directional_light_color();
		let environment_light_color = self.environment_light_color();
		let globals = GlobalsGpu {
			view_proj: view_proj.to_cols_array_2d(),
			inv_view_proj: inv_view_proj.to_cols_array_2d(),
			light_dir: light.to_array(),
			camera_pos: Vec4::from((cam_pos, 1.0)).to_array(),
			_pad: [0u8; 96],
		};
		if self.globals_uploaded != Some(globals) {
			self.queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&globals));
			self.globals_uploaded = Some(globals);
		}
		let audio_link_frame = if advance_audio_link && self.audio_link_texture_needed {
			self.audio_link_runtime
				.as_mut()
				.and_then(crate::audio_link::AudioLinkInputRuntime::next_texture_frame)
		} else {
			None
		};
		if let Some(sm) = &mut self.scene_meshes {
			if let Some(frame) = audio_link_frame.as_ref() {
				sm.upload_audio_link_texture(&self.queue, frame);
			} else if !self.audio_link_texture_needed {
				sm.set_audio_link_external_enabled(false);
			}
			sm.prepare_frame(
				&self.queue,
				view_proj,
				view,
				light,
				Vec4::from((cam_pos, 1.0)),
				directional_light_color,
				environment_light_color,
				self.animation_time_secs,
				if self.audio_link_texture_needed {
					sm.audio_link_frame_params()
				} else {
					[0.0; 4]
				},
			);
		}
	}

	fn directional_light_dir(&self, cam_pos: Vec3, look_at: Vec3, _view: Mat4) -> Vec3 {
		let directional = self.lighting.directional;
		if !directional.enabled || directional.intensity <= 0.0 {
			return Vec3::Y;
		}
		let camera_dir = (cam_pos - look_at).try_normalize().unwrap_or(Vec3::Z);
		let camera_yaw = Vec3::new(camera_dir.x, 0.0, camera_dir.z).try_normalize().unwrap_or(Vec3::Z);
		let yaw_basis = if directional.follow_camera_yaw { camera_yaw } else { Vec3::Z };
		let yaw_right = Vec3::new(yaw_basis.z, 0.0, -yaw_basis.x).try_normalize().unwrap_or(Vec3::X);
		let azimuth = directional.azimuth_deg.to_radians();
		let horizontal = (yaw_right * azimuth.sin() + yaw_basis * azimuth.cos())
			.try_normalize()
			.unwrap_or(yaw_basis);
		let camera_pitch = if directional.follow_camera_pitch {
			camera_dir.y.clamp(-1.0, 1.0).asin().to_degrees()
		} else {
			0.0
		};
		let elevation = (directional.elevation_deg + camera_pitch).clamp(-89.0, 89.0).to_radians();
		(horizontal * elevation.cos() + Vec3::Y * elevation.sin())
			.try_normalize()
			.unwrap_or(horizontal)
	}

	fn directional_light_color(&self) -> Vec4 {
		let light = self.lighting.directional;
		let intensity = if light.enabled { light.intensity.clamp(0.0, 4.0) } else { 0.0 };
		Vec4::new(
			light.color[0].clamp(0.0, 1.0),
			light.color[1].clamp(0.0, 1.0),
			light.color[2].clamp(0.0, 1.0),
			intensity,
		)
	}

	fn environment_light_color(&self) -> Vec4 {
		let light = self.lighting.environment;
		let intensity = if light.enabled { light.intensity.clamp(0.0, 2.0) } else { 0.0 };
		Vec4::new(
			light.color[0].clamp(0.0, 1.0),
			light.color[1].clamp(0.0, 1.0),
			light.color[2].clamp(0.0, 1.0),
			intensity,
		)
	}

	fn avatar_outline_width_px_for(&self, width: u32, height: u32) -> f32 {
		let outline_m = self.avatar_outline.width.unwrap_or(0.003).clamp(0.0, 0.05);
		let aspect = width.max(1) as f32 / height.max(1) as f32;
		let diagonal_rad = self.camera.diagonal_fov_deg.to_radians();
		let fovy = vertical_fov_from_diagonal(diagonal_rad, aspect);
		let distance_m = self.camera.radius.max(0.05);
		let pixels_per_meter = height.max(1) as f32 / (2.0 * (fovy * 0.5).tan() * distance_m);
		(outline_m * pixels_per_meter).clamp(0.0, 96.0)
	}

	/// 空シーン（プロシージャルスカイ）を 1 フレーム描画する。`Lost` / `Outdated` 時はリサイズして `None`。
	pub fn render_frame(
		&mut self,
		window: &Window,
		clear_color: wgpu::Color,
		wall_since_last: Duration,
		startup_splash: Option<StartupSplashFrame>,
	) -> Option<FrameTimings> {
		let t_cpu0 = Instant::now();
		// 前フレーム以降に完了した GPU タイムスタンプの readback を進める。
		if self.gpu_timestamps.is_some() {
			self.device.poll(wgpu::PollType::Poll).ok();
			if let Some(ts) = self.gpu_timestamps.as_mut() {
				ts.drain_ready();
			}
		}
		self.animation_time_secs += wall_since_last.as_secs_f32();
		self.debug_frame_seq = self.debug_frame_seq.wrapping_add(1);
		if let (Some(doc_arc), true) = (
			&self.document,
			self.debug_scene && self.debug_log.is_enabled() && self.debug_frame_seq.is_multiple_of(180),
		) {
			if let Ok(g) = doc_arc.read() {
				let runtime_model = g.runtime_model();
				let roots_str = runtime_model
					.scene()
					.map(|s| format!("{:?}", s.roots))
					.unwrap_or_else(|| "none".to_string());
				let keys = humanoid_profile_keys_csv(runtime_model.humanoid_profile());
				self.debug_log.line(
					"scene",
					format!(
						"frame seq={} vmc_live={} scene_roots={} humanoid_keys={}",
						self.debug_frame_seq, self.vmc_live, roots_str, keys
					),
				);
			}
		}
		if let (Some(doc_arc), true) = (
			&self.document,
			self.debug_morph && self.debug_log.is_enabled() && self.debug_frame_seq.is_multiple_of(180),
		) {
			if let Ok(g) = doc_arc.read() {
				let runtime_model = g.runtime_model();
				let n_presets = runtime_model.expression_catalog().map(|c| c.presets.len()).unwrap_or(0);
				if let Some(ew) = runtime_model.expression_weights() {
					let top = format_top_expression_weights(&ew.preset_weights, 16);
					self.debug_log.line(
						"morph",
						format!(
							"frame seq={} catalog_presets={} top_weights=[{}]",
							self.debug_frame_seq, n_presets, top
						),
					);
				} else {
					self.debug_log.line(
						"morph",
						format!(
							"frame seq={} catalog_presets={} no_expression_weights",
							self.debug_frame_seq, n_presets
						),
					);
				}
			}
		}
		let dt = wall_since_last.as_secs_f32();
		self.apply_pending_motion_frames();
		if let (Some(doc_arc), Some(sim)) = (&self.document, &mut self.dynamics_sim) {
			if let Ok(mut doc) = doc_arc.write() {
				if let Some(runtime) = doc.runtime_scene_and_dynamics_mut() {
					sim.step_runtime_dynamics(runtime.scene, runtime.dynamics.as_readonly(), dt);
				}
			}
		}
		if matches!(self.apply_contact_parameter_emissions(), Ok(changed) if !changed.is_empty()) {
			if let Err(e) = self.evaluate_runtime_parameter_actions() {
				eprintln!("un-avatar-renderer: contact parameter action evaluation failed: {e}");
			}
		}
		let (gw, gh) = self.render_pixel_dims();
		self.write_frame_globals(gw, gh, true);

		let frame = match self.surface.get_current_texture() {
			wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
			wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
				let s = window.inner_size();
				self.resize(s.width, s.height);
				return None;
			}
			wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => return None,
			wgpu::CurrentSurfaceTexture::Validation => {
				eprintln!("un-avatar-renderer: get_current_texture: validation error");
				return None;
			}
		};
		let frame_width = frame.texture.width();
		let frame_height = frame.texture.height();
		if frame_width == 0 || frame_height == 0 {
			return None;
		}
		if frame_width != self.config.width || frame_height != self.config.height {
			let s = window.inner_size();
			let width = if s.width == 0 { frame_width } else { s.width };
			let height = if s.height == 0 { frame_height } else { s.height };
			drop(frame);
			self.resize(width, height);
			return None;
		}

		let swap_view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());

		#[cfg(windows)]
		if let (Some(ref mut sp), Some(ref lc)) = (&mut self.spout, &self.spout_launch) {
			sp.resize_to(&self.device, self.config.width, self.config.height, lc, self.config.format);
		}

		let draw_scene = self.scene_meshes.as_ref().is_some_and(|m| !m.is_empty());
		let use_spout = {
			#[cfg(windows)]
			{
				self.spout.is_some() && draw_scene
			}
			#[cfg(not(windows))]
			{
				false
			}
		};
		let use_post_aa = matches!(self.aa, AaMode::Fxaa | AaMode::Smaa);
		let use_avatar_outline =
			self.avatar_outline.policy == AvatarOutlinePolicy::Override && self.avatar_outline.width.unwrap_or(0.003) > 0.0;
		let use_color_adjust = !self.environment_color.is_identity();
		let use_bloom = self.bloom.is_enabled();
		let use_ssao = self.ssao.is_enabled();
		let needs_screen_refraction = self.scene_meshes.as_ref().is_some_and(SceneMeshes::needs_screen_refraction);
		let use_post = use_post_aa || use_avatar_outline || use_color_adjust || use_bloom || use_ssao || needs_screen_refraction;
		let use_msaa = matches!(self.aa, AaMode::Msaa);
		if use_post {
			if let Some(post) = &mut self.post_process {
				post.resize_to(&self.device, gw, gh, self.config.format);
			} else {
				self.post_process = Some(PostProcess::new(&self.device, gw, gh, self.config.format));
			}
		}
		if needs_screen_refraction {
			if let Some(grab) = &mut self.screen_grab_target {
				grab.resize_to(&self.device, gw, gh, self.config.format);
			} else {
				self.screen_grab_target = Some(ScreenGrabTarget::new(&self.device, gw, gh, self.config.format));
			}
		}
		if use_msaa {
			let sample_count = aa_sample_count(self.aa);
			if let Some(msaa) = &mut self.msaa_target {
				msaa.resize_to(&self.device, gw, gh, self.config.format, sample_count);
			} else {
				self.msaa_target = Some(crate::post_process::MsaaTarget::new(
					&self.device,
					gw,
					gh,
					self.config.format,
					sample_count,
				));
			}
		}

		let draw_contact_shadow = draw_scene && self.contact_shadow.is_enabled();
		let draw_contact_shadow_in_main = draw_contact_shadow && !use_avatar_outline;
		let document_revision = self.document_revision.load(Ordering::Acquire);
		let expression_overrides_changed = self.expression_overrides_revision != self.applied_expression_overrides_revision;
		let scene_pose_may_change =
			self.dynamics_sim.is_some() || document_revision != self.applied_document_revision || expression_overrides_changed;
		let mut world_scratch_current = false;
		if draw_scene && scene_pose_may_change {
			world_scratch_current = self.refresh_scene_draw_state(Some(document_revision));
		}
		if self.show_bone_colliders && draw_scene {
			if world_scratch_current {
				self.rebuild_bone_collider_debug_vertices_from_world();
			} else {
				self.update_bone_collider_debug_vertices();
			}
		} else {
			self.bone_collider_vertex_count = 0;
		}

		#[cfg(windows)]
		let final_target_view = if use_spout {
			self.spout.as_ref().unwrap().color_view()
		} else {
			&swap_view
		};
		#[cfg(not(windows))]
		let final_target_view = &swap_view;

		let mut main_resolve_target: Option<&wgpu::TextureView> = None;
		let (main_color, main_depth): (&wgpu::TextureView, &wgpu::TextureView) = if use_post {
			let post = self.post_process.as_ref().expect("post target is initialized");
			if use_msaa {
				let msaa = self.msaa_target.as_ref().expect("msaa target is initialized");
				main_resolve_target = Some(post.source_view());
				(msaa.color_view(), msaa.depth_view())
			} else {
				(post.source_view(), post.depth_view())
			}
		} else if use_msaa {
			let msaa = self.msaa_target.as_ref().expect("msaa target is initialized");
			main_resolve_target = Some(final_target_view);
			(msaa.color_view(), msaa.depth_view())
		} else if use_spout {
			#[cfg(windows)]
			{
				let sp = self.spout.as_ref().unwrap();
				(sp.color_view(), sp.depth_view())
			}
			#[cfg(not(windows))]
			{
				unreachable!()
			}
		} else {
			(&swap_view, &self.depth_view)
		};

		let mut encoder = self
			.device
			.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
		if draw_scene {
			if let Some(sm) = &self.scene_meshes {
				sm.encode_compute_fur_cards(&mut encoder);
			}
		}

		let timestamp_pass = self.gpu_timestamps.as_ref().and_then(|ts| ts.begin_pass());
		let (timestamp_writes, timestamp_write_idx) = match timestamp_pass {
			Some((writes, idx)) => (Some(writes), Some(idx)),
			None => (None, None),
		};
		let scene_clear_color = if use_spout || clear_color.a <= 0.0 {
			wgpu::Color {
				r: 0.0,
				g: 0.0,
				b: 0.0,
				a: 0.0,
			}
		} else {
			clear_color
		};

		if draw_scene && needs_screen_refraction {
			let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
				label: Some("main-opaque"),
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: main_color,
					depth_slice: None,
					resolve_target: main_resolve_target,
					ops: wgpu::Operations {
						load: wgpu::LoadOp::Clear(scene_clear_color),
						store: wgpu::StoreOp::Store,
					},
				})],
				depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
					view: main_depth,
					depth_ops: Some(wgpu::Operations {
						load: wgpu::LoadOp::Clear(1.0),
						store: wgpu::StoreOp::Store,
					}),
					stencil_ops: None,
				}),
				timestamp_writes,
				occlusion_query_set: None,
				multiview_mask: None,
			});
			if let Some(sm) = &self.scene_meshes {
				sm.draw_opaque(&mut pass);
				if draw_contact_shadow_in_main {
					self.write_contact_shadow_uniform();
					self.draw_contact_shadow(&mut pass);
				}
				sm.draw_toon_outlines(&mut pass);
				sm.draw_blended_before_screen_refraction(&mut pass);
			}
			drop(pass);

			if let (Some(post), Some(grab), Some(sm)) = (&self.post_process, &self.screen_grab_target, &mut self.scene_meshes) {
				encoder.copy_texture_to_texture(
					wgpu::TexelCopyTextureInfo {
						texture: post.source_texture(),
						mip_level: 0,
						origin: wgpu::Origin3d::ZERO,
						aspect: wgpu::TextureAspect::All,
					},
					wgpu::TexelCopyTextureInfo {
						texture: grab.texture(),
						mip_level: 0,
						origin: wgpu::Origin3d::ZERO,
						aspect: wgpu::TextureAspect::All,
					},
					wgpu::Extent3d {
						width: gw.max(1),
						height: gh.max(1),
						depth_or_array_layers: 1,
					},
				);
				sm.set_screen_grab_view(&self.device, grab.view());
			}

			let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
				label: Some("main-blended"),
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: main_color,
					depth_slice: None,
					resolve_target: main_resolve_target,
					ops: wgpu::Operations {
						load: wgpu::LoadOp::Load,
						store: wgpu::StoreOp::Store,
					},
				})],
				depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
					view: main_depth,
					depth_ops: Some(wgpu::Operations {
						load: wgpu::LoadOp::Load,
						store: wgpu::StoreOp::Store,
					}),
					stencil_ops: None,
				}),
				timestamp_writes: None,
				occlusion_query_set: None,
				multiview_mask: None,
			});
			if let Some(sm) = &self.scene_meshes {
				sm.draw_blended_after_screen_refraction(&mut pass);
			}
			if self.show_axes {
				pass.set_pipeline(&self.axes_pipeline);
				pass.set_bind_group(0, &self.bind_group, &[]);
				pass.draw(0..6, 0..1);
			}
			if self.show_bone_colliders && self.bone_collider_vertex_count > 0 {
				if let Some(buffer) = &self.bone_collider_vertex_buffer {
					pass.set_pipeline(&self.bone_collider_pipeline);
					pass.set_bind_group(0, &self.bind_group, &[]);
					pass.set_vertex_buffer(0, buffer.slice(..));
					pass.draw(0..self.bone_collider_vertex_count, 0..1);
				}
			}
			if let Some(splash) = startup_splash {
				let aspect = gw.max(1) as f32 / gh.max(1) as f32;
				self.queue.write_buffer(
					&self.startup_splash_buffer,
					0,
					bytemuck::bytes_of(&StartupSplashGpu {
						time: splash.time_secs,
						progress: splash.progress,
						aspect,
						phase: splash.phase,
					}),
				);
				pass.set_pipeline(&self.startup_splash_pipeline);
				pass.set_bind_group(0, &self.startup_splash_bind_group, &[]);
				pass.draw(0..3, 0..1);
			}
		} else {
			let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
				label: Some("main"),
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: main_color,
					depth_slice: None,
					resolve_target: main_resolve_target,
					ops: wgpu::Operations {
						load: wgpu::LoadOp::Clear(scene_clear_color),
						store: wgpu::StoreOp::Store,
					},
				})],
				depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
					view: main_depth,
					depth_ops: Some(wgpu::Operations {
						load: wgpu::LoadOp::Clear(1.0),
						store: wgpu::StoreOp::Store,
					}),
					stencil_ops: None,
				}),
				timestamp_writes,
				occlusion_query_set: None,
				multiview_mask: None,
			});
			if draw_scene {
				if let Some(sm) = &self.scene_meshes {
					sm.draw_opaque(&mut pass);
					if draw_contact_shadow_in_main {
						self.write_contact_shadow_uniform();
						self.draw_contact_shadow(&mut pass);
					}
					sm.draw_toon_outlines(&mut pass);
					sm.draw_blended(&mut pass);
				}
			} else {
				pass.set_pipeline(&self.pipeline);
				pass.set_bind_group(0, &self.bind_group, &[]);
				pass.draw(0..3, 0..1);
			}
			if self.show_axes && draw_scene {
				pass.set_pipeline(&self.axes_pipeline);
				pass.set_bind_group(0, &self.bind_group, &[]);
				pass.draw(0..6, 0..1);
			}
			if self.show_bone_colliders && self.bone_collider_vertex_count > 0 {
				if let Some(buffer) = &self.bone_collider_vertex_buffer {
					pass.set_pipeline(&self.bone_collider_pipeline);
					pass.set_bind_group(0, &self.bind_group, &[]);
					pass.set_vertex_buffer(0, buffer.slice(..));
					pass.draw(0..self.bone_collider_vertex_count, 0..1);
				}
			}
			if let Some(splash) = startup_splash {
				let aspect = gw.max(1) as f32 / gh.max(1) as f32;
				self.queue.write_buffer(
					&self.startup_splash_buffer,
					0,
					bytemuck::bytes_of(&StartupSplashGpu {
						time: splash.time_secs,
						progress: splash.progress,
						aspect,
						phase: splash.phase,
					}),
				);
				pass.set_pipeline(&self.startup_splash_pipeline);
				pass.set_bind_group(0, &self.startup_splash_bind_group, &[]);
				pass.draw(0..3, 0..1);
			}
		}

		if let (Some(ts), Some(idx)) = (self.gpu_timestamps.as_ref(), timestamp_write_idx) {
			ts.encode_resolve(&mut encoder, idx);
		}

		if use_post {
			{
				let post = self.post_process.as_mut().expect("post target is initialized");
				match self.aa {
					AaMode::Fxaa => post.encode_fxaa(
						&self.device,
						&self.queue,
						&mut encoder,
						final_target_view,
						self.environment_color,
						self.bloom,
						self.ssao,
					),
					AaMode::Smaa => post.encode_smaa(
						&self.device,
						&self.queue,
						&mut encoder,
						final_target_view,
						self.environment_color,
						self.bloom,
						self.ssao,
					),
					AaMode::Off | AaMode::Msaa => {
						if use_color_adjust || use_bloom || use_ssao {
							post.encode_color_adjust(
								&self.device,
								&self.queue,
								&mut encoder,
								final_target_view,
								self.environment_color,
								self.bloom,
								self.ssao,
							);
						} else {
							post.encode_fxaa(
								&self.device,
								&self.queue,
								&mut encoder,
								final_target_view,
								self.environment_color,
								self.bloom,
								self.ssao,
							);
						}
					}
				}
			}
			if draw_contact_shadow && use_avatar_outline {
				self.write_contact_shadow_uniform();
				let shadow_depth = self.post_process.as_ref().expect("post target is initialized").depth_view();
				let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
					label: Some("contact-shadow"),
					color_attachments: &[Some(wgpu::RenderPassColorAttachment {
						view: final_target_view,
						depth_slice: None,
						resolve_target: None,
						ops: wgpu::Operations {
							load: wgpu::LoadOp::Load,
							store: wgpu::StoreOp::Store,
						},
					})],
					depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
						view: shadow_depth,
						depth_ops: Some(wgpu::Operations {
							load: wgpu::LoadOp::Load,
							store: wgpu::StoreOp::Store,
						}),
						stencil_ops: None,
					}),
					timestamp_writes: None,
					occlusion_query_set: None,
					multiview_mask: None,
				});
				self.draw_contact_shadow(&mut pass);
			}
			if use_avatar_outline {
				let width_px = self.avatar_outline_width_px_for(gw, gh);
				let post = self.post_process.as_mut().expect("post target is initialized");
				post.encode_avatar_outline(
					&self.device,
					&self.queue,
					&mut encoder,
					final_target_view,
					self.avatar_outline,
					width_px,
				);
			}
		}

		let t_before_submit = Instant::now();
		self.queue.submit(std::iter::once(encoder.finish()));
		if let (Some(ts), Some(idx)) = (self.gpu_timestamps.as_mut(), timestamp_write_idx) {
			ts.after_submit(idx);
		}

		#[cfg(windows)]
		if use_spout {
			let sp = self.spout.as_mut().expect("spout is initialized while active");
			// 1) 前フレーム以降に map が完了したスロットがあれば Spout2 に送る（非ブロッキング）。
			sp.send_mapped_rgba(&self.device);
			// 2) 今フレームの swizzle + readback を encode。リングが空いていれば map を要求する。
			let mut enc2 = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
				label: Some("spout-staging"),
			});
			let staged_slot = sp.copy_to_staging(&mut enc2);
			self.queue.submit(std::iter::once(enc2.finish()));
			if let Some(idx) = staged_slot {
				sp.after_submit_request_map(idx);
			}
			// 3) swap chain にプレビュー用にコピー。
			let mut enc3 = self
				.device
				.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("spout-blit") });
			sp.encode_blit(&mut enc3, &swap_view, clear_color);
			self.queue.submit(std::iter::once(enc3.finish()));
		}

		frame.present();

		Some(FrameTimings {
			wall_since_last_ms: wall_since_last.as_secs_f32() * 1000.0,
			cpu_record_ms: (t_before_submit - t_cpu0).as_secs_f32() * 1000.0,
			gpu_ms: self.gpu_timestamps.as_ref().and_then(|ts| ts.last_gpu_ms()).unwrap_or(0.0),
		})
	}
}

fn create_depth(device: &wgpu::Device, width: u32, height: u32) -> (wgpu::Texture, wgpu::TextureView) {
	let texture = device.create_texture(&wgpu::TextureDescriptor {
		label: Some("un-avatar-depth"),
		size: wgpu::Extent3d {
			width,
			height,
			depth_or_array_layers: 1,
		},
		mip_level_count: 1,
		sample_count: 1,
		dimension: wgpu::TextureDimension::D2,
		format: wgpu::TextureFormat::Depth24Plus,
		usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
		view_formats: &[],
	});
	let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
	(texture, view)
}

fn create_screen_grab_texture(
	device: &wgpu::Device,
	width: u32,
	height: u32,
	format: wgpu::TextureFormat,
) -> (wgpu::Texture, wgpu::TextureView) {
	let texture = device.create_texture(&wgpu::TextureDescriptor {
		label: Some("screen-grab"),
		size: wgpu::Extent3d {
			width,
			height,
			depth_or_array_layers: 1,
		},
		mip_level_count: 1,
		sample_count: 1,
		dimension: wgpu::TextureDimension::D2,
		format,
		usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
		view_formats: &[],
	});
	let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
	(texture, view)
}

fn append_collider_wire_vertices(collider: BoneColliderPrimitive, world: &[Mat4], out: &mut Vec<DebugLineVertex>) {
	const COLOR: [f32; 4] = [1.0, 0.78, 0.12, 0.72];
	match collider {
		BoneColliderPrimitive::Sphere { node, radius } => {
			if let Some(center) = world.get(node).map(|m| m.transform_point3(Vec3::ZERO)) {
				append_wire_sphere(center, radius, COLOR, out);
			}
		}
		BoneColliderPrimitive::Capsule {
			start_node,
			end_node,
			radius,
		} => {
			let (Some(a), Some(b)) = (
				world.get(start_node).map(|m| m.transform_point3(Vec3::ZERO)),
				world.get(end_node).map(|m| m.transform_point3(Vec3::ZERO)),
			) else {
				return;
			};
			push_debug_line(a, b, COLOR, out);
			append_wire_sphere(a, radius, COLOR, out);
			append_wire_sphere(b, radius, COLOR, out);
		}
		BoneColliderPrimitive::LocalSphere { node, center, radius, .. } => {
			if let Some((center, radius)) = local_sphere_world(world, node, center, radius) {
				append_wire_sphere(center, radius, COLOR, out);
			}
		}
		BoneColliderPrimitive::LocalCapsule {
			node,
			center,
			axis,
			half_length,
			radius,
			..
		} => {
			let Some((a, b, radius)) = local_capsule_world(world, node, center, axis, half_length, radius) else {
				return;
			};
			push_debug_line(a, b, COLOR, out);
			append_wire_sphere(a, radius, COLOR, out);
			append_wire_sphere(b, radius, COLOR, out);
		}
	}
}

fn append_wire_sphere(center: Vec3, radius: f32, color: [f32; 4], out: &mut Vec<DebugLineVertex>) {
	if !radius.is_finite() || radius <= 0.0 {
		return;
	}
	const N: usize = 24;
	for plane in 0..3 {
		for i in 0..N {
			let a0 = i as f32 / N as f32 * std::f32::consts::TAU;
			let a1 = (i + 1) as f32 / N as f32 * std::f32::consts::TAU;
			let p0 = circle_point(center, radius, a0, plane);
			let p1 = circle_point(center, radius, a1, plane);
			push_debug_line(p0, p1, color, out);
		}
	}
}

fn circle_point(center: Vec3, radius: f32, angle: f32, plane: usize) -> Vec3 {
	let c = angle.cos() * radius;
	let s = angle.sin() * radius;
	match plane {
		0 => center + Vec3::new(c, s, 0.0),
		1 => center + Vec3::new(c, 0.0, s),
		_ => center + Vec3::new(0.0, c, s),
	}
}

fn push_debug_line(a: Vec3, b: Vec3, color: [f32; 4], out: &mut Vec<DebugLineVertex>) {
	out.push(DebugLineVertex {
		position: a.to_array(),
		color,
	});
	out.push(DebugLineVertex {
		position: b.to_array(),
		color,
	});
}

fn transparent_alpha_mode(alpha_modes: &[wgpu::CompositeAlphaMode]) -> wgpu::CompositeAlphaMode {
	const PREFERRED: [wgpu::CompositeAlphaMode; 4] = [
		wgpu::CompositeAlphaMode::PreMultiplied,
		wgpu::CompositeAlphaMode::PostMultiplied,
		wgpu::CompositeAlphaMode::Inherit,
		wgpu::CompositeAlphaMode::Auto,
	];
	PREFERRED
		.into_iter()
		.find(|mode| alpha_modes.contains(mode))
		.unwrap_or_else(|| alpha_modes.first().copied().unwrap_or(wgpu::CompositeAlphaMode::Opaque))
}

fn aa_sample_count(aa: AaMode) -> u32 {
	match aa {
		AaMode::Msaa => 4,
		AaMode::Off | AaMode::Fxaa | AaMode::Smaa => 1,
	}
}

fn create_sky_pipeline(
	device: &wgpu::Device,
	bind_group_layout: &wgpu::BindGroupLayout,
	surface_format: wgpu::TextureFormat,
	sample_count: u32,
) -> wgpu::RenderPipeline {
	let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
		label: Some("sky"),
		source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER_SKY)),
	});

	let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
		label: Some("sky"),
		bind_group_layouts: &[Some(bind_group_layout)],
		immediate_size: 0,
	});

	device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
		label: Some("sky"),
		layout: Some(&layout),
		cache: None,
		vertex: wgpu::VertexState {
			module: &shader,
			entry_point: Some("vs_main"),
			compilation_options: Default::default(),
			buffers: &[],
		},
		fragment: Some(wgpu::FragmentState {
			module: &shader,
			entry_point: Some("fs_main"),
			compilation_options: Default::default(),
			targets: &[Some(wgpu::ColorTargetState {
				format: surface_format,
				blend: None,
				write_mask: wgpu::ColorWrites::ALL,
			})],
		}),
		primitive: wgpu::PrimitiveState {
			topology: wgpu::PrimitiveTopology::TriangleList,
			..Default::default()
		},
		depth_stencil: Some(wgpu::DepthStencilState {
			format: wgpu::TextureFormat::Depth24Plus,
			depth_write_enabled: Some(true),
			depth_compare: Some(wgpu::CompareFunction::LessEqual),
			stencil: wgpu::StencilState::default(),
			bias: wgpu::DepthBiasState::default(),
		}),
		multisample: wgpu::MultisampleState {
			count: sample_count,
			..Default::default()
		},
		multiview_mask: None,
	})
}

fn create_axes_pipeline(
	device: &wgpu::Device,
	bind_group_layout: &wgpu::BindGroupLayout,
	surface_format: wgpu::TextureFormat,
	sample_count: u32,
) -> wgpu::RenderPipeline {
	let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
		label: Some("debug_axes"),
		source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER_AXES)),
	});

	let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
		label: Some("debug_axes"),
		bind_group_layouts: &[Some(bind_group_layout)],
		immediate_size: 0,
	});

	device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
		label: Some("debug_axes"),
		layout: Some(&layout),
		cache: None,
		vertex: wgpu::VertexState {
			module: &shader,
			entry_point: Some("vs_main"),
			compilation_options: Default::default(),
			buffers: &[],
		},
		fragment: Some(wgpu::FragmentState {
			module: &shader,
			entry_point: Some("fs_main"),
			compilation_options: Default::default(),
			targets: &[Some(wgpu::ColorTargetState {
				format: surface_format,
				blend: None,
				write_mask: wgpu::ColorWrites::ALL,
			})],
		}),
		primitive: wgpu::PrimitiveState {
			topology: wgpu::PrimitiveTopology::LineList,
			..Default::default()
		},
		depth_stencil: Some(wgpu::DepthStencilState {
			format: wgpu::TextureFormat::Depth24Plus,
			depth_write_enabled: Some(false),
			depth_compare: Some(wgpu::CompareFunction::LessEqual),
			stencil: wgpu::StencilState::default(),
			bias: wgpu::DepthBiasState::default(),
		}),
		multisample: wgpu::MultisampleState {
			count: sample_count,
			..Default::default()
		},
		multiview_mask: None,
	})
}

fn create_bone_collider_pipeline(
	device: &wgpu::Device,
	bind_group_layout: &wgpu::BindGroupLayout,
	surface_format: wgpu::TextureFormat,
	sample_count: u32,
) -> wgpu::RenderPipeline {
	let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
		label: Some("debug_bone_colliders"),
		source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER_BONE_COLLIDERS)),
	});
	let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
		label: Some("debug_bone_colliders"),
		bind_group_layouts: &[Some(bind_group_layout)],
		immediate_size: 0,
	});
	device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
		label: Some("debug_bone_colliders"),
		layout: Some(&layout),
		cache: None,
		vertex: wgpu::VertexState {
			module: &shader,
			entry_point: Some("vs_main"),
			compilation_options: Default::default(),
			buffers: &[wgpu::VertexBufferLayout {
				array_stride: std::mem::size_of::<DebugLineVertex>() as u64,
				step_mode: wgpu::VertexStepMode::Vertex,
				attributes: &[
					wgpu::VertexAttribute {
						format: wgpu::VertexFormat::Float32x3,
						offset: 0,
						shader_location: 0,
					},
					wgpu::VertexAttribute {
						format: wgpu::VertexFormat::Float32x4,
						offset: 12,
						shader_location: 1,
					},
				],
			}],
		},
		fragment: Some(wgpu::FragmentState {
			module: &shader,
			entry_point: Some("fs_main"),
			compilation_options: Default::default(),
			targets: &[Some(wgpu::ColorTargetState {
				format: surface_format,
				blend: Some(wgpu::BlendState::ALPHA_BLENDING),
				write_mask: wgpu::ColorWrites::ALL,
			})],
		}),
		primitive: wgpu::PrimitiveState {
			topology: wgpu::PrimitiveTopology::LineList,
			..Default::default()
		},
		depth_stencil: Some(wgpu::DepthStencilState {
			format: wgpu::TextureFormat::Depth24Plus,
			depth_write_enabled: Some(false),
			depth_compare: Some(wgpu::CompareFunction::LessEqual),
			stencil: wgpu::StencilState::default(),
			bias: wgpu::DepthBiasState::default(),
		}),
		multisample: wgpu::MultisampleState {
			count: sample_count,
			..Default::default()
		},
		multiview_mask: None,
	})
}

fn create_contact_shadow_pipeline(
	device: &wgpu::Device,
	globals_layout: &wgpu::BindGroupLayout,
	shadow_layout: &wgpu::BindGroupLayout,
	surface_format: wgpu::TextureFormat,
	sample_count: u32,
) -> wgpu::RenderPipeline {
	let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
		label: Some("contact_shadow"),
		source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER_CONTACT_SHADOW)),
	});

	let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
		label: Some("contact_shadow"),
		bind_group_layouts: &[Some(globals_layout), Some(shadow_layout)],
		immediate_size: 0,
	});

	device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
		label: Some("contact_shadow"),
		layout: Some(&layout),
		cache: None,
		vertex: wgpu::VertexState {
			module: &shader,
			entry_point: Some("vs_main"),
			compilation_options: Default::default(),
			buffers: &[],
		},
		fragment: Some(wgpu::FragmentState {
			module: &shader,
			entry_point: Some("fs_main"),
			compilation_options: Default::default(),
			targets: &[Some(wgpu::ColorTargetState {
				format: surface_format,
				blend: Some(wgpu::BlendState::ALPHA_BLENDING),
				write_mask: wgpu::ColorWrites::ALL,
			})],
		}),
		primitive: wgpu::PrimitiveState {
			topology: wgpu::PrimitiveTopology::TriangleList,
			..Default::default()
		},
		depth_stencil: Some(wgpu::DepthStencilState {
			format: wgpu::TextureFormat::Depth24Plus,
			depth_write_enabled: Some(false),
			depth_compare: Some(wgpu::CompareFunction::Always),
			stencil: wgpu::StencilState::default(),
			bias: wgpu::DepthBiasState::default(),
		}),
		multisample: wgpu::MultisampleState {
			count: sample_count,
			..Default::default()
		},
		multiview_mask: None,
	})
}

fn create_startup_splash_pipeline(
	device: &wgpu::Device,
	bind_group_layout: &wgpu::BindGroupLayout,
	surface_format: wgpu::TextureFormat,
	sample_count: u32,
) -> wgpu::RenderPipeline {
	let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
		label: Some("startup_splash"),
		source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER_STARTUP_SPLASH)),
	});

	let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
		label: Some("startup_splash"),
		bind_group_layouts: &[Some(bind_group_layout)],
		immediate_size: 0,
	});

	device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
		label: Some("startup_splash"),
		layout: Some(&layout),
		cache: None,
		vertex: wgpu::VertexState {
			module: &shader,
			entry_point: Some("vs_main"),
			compilation_options: Default::default(),
			buffers: &[],
		},
		fragment: Some(wgpu::FragmentState {
			module: &shader,
			entry_point: Some("fs_main"),
			compilation_options: Default::default(),
			targets: &[Some(wgpu::ColorTargetState {
				format: surface_format,
				blend: Some(wgpu::BlendState::ALPHA_BLENDING),
				write_mask: wgpu::ColorWrites::ALL,
			})],
		}),
		primitive: wgpu::PrimitiveState {
			topology: wgpu::PrimitiveTopology::TriangleList,
			..Default::default()
		},
		depth_stencil: Some(wgpu::DepthStencilState {
			format: wgpu::TextureFormat::Depth24Plus,
			depth_write_enabled: Some(false),
			depth_compare: Some(wgpu::CompareFunction::Always),
			stencil: wgpu::StencilState::default(),
			bias: wgpu::DepthBiasState::default(),
		}),
		multisample: wgpu::MultisampleState {
			count: sample_count,
			..Default::default()
		},
		multiview_mask: None,
	})
}

#[cfg(test)]
mod tests {
	use std::collections::BTreeMap;

	use super::{
		menu_graph_node_path, mesh_shader_resource_plan_for_adapter, mesh_shader_variant_tier_for_limits, modular_avatar_menu_components,
		runtime_action_id_for_parameter, runtime_action_ids_for_parameter, runtime_action_ids_for_parameter_values,
		runtime_action_statuses, transparent_alpha_mode, wardrobe_action_statuses, wardrobe_asset_upload_plan_for_document,
		wardrobe_asset_upload_plan_with_draw_counts, wardrobe_scoped_upload_work_for_active_gaps, RuntimeMenuGraphNode,
		WardrobeAssetUploadPlan, BASELINE_FALLBACK_SAMPLED_TEXTURES_PER_STAGE, BASELINE_FALLBACK_SAMPLERS_PER_STAGE,
		HIGH_CAPABILITY_LILTOON_SAMPLED_TEXTURES_PER_STAGE, HIGH_CAPABILITY_LILTOON_SAMPLERS_PER_STAGE,
		WARDROBE_ASSET_UPLOAD_MODE_RESOURCE_SCOPED, WARDROBE_RESIDENCY_GAP_INDEX_STATUS_LIMIT,
	};
	use crate::mesh_pass::{MeshShaderVariantTier, SceneMeshActiveResidencyGaps, SceneMeshAssetResidencyCounts};
	use serde_json::json;
	use wgpu::CompositeAlphaMode::{Auto, Opaque, PostMultiplied, PreMultiplied};

	#[test]
	fn menu_graph_node_path_reports_truncated_cycles() {
		let nodes = vec![
			RuntimeMenuGraphNode {
				menu_key: "component:10".to_string(),
				label: Some("A".to_string()),
				hierarchy_path: Some("Root/A".to_string()),
				parent_node_index: Some(1),
			},
			RuntimeMenuGraphNode {
				menu_key: "component:11".to_string(),
				label: Some("B".to_string()),
				hierarchy_path: Some("Root/B".to_string()),
				parent_node_index: Some(0),
			},
		];

		let path = menu_graph_node_path(&nodes, 0);
		assert!(path.truncated);
		assert_eq!(path.labels, vec!["B".to_string(), "A".to_string()]);
	}

	#[test]
	fn modular_avatar_menu_components_expand_external_menu_controls() {
		let unavatar = un_avatar_core::UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: json!({
				"modularAvatar": {
					"components": [{
						"shortType": "ModularAvatarMenuInstaller",
						"enabled": true,
						"hierarchyPath": "Root/MenuInstaller",
						"menuToAppend": {
							"assetPath": "Assets/Menus/Root.asset",
							"controls": [{
								"name": "External Hat",
								"type": "Toggle",
								"parameter": "Hat",
								"value": 1.0
							}]
						}
					}]
				}
			}),
		};

		let components = modular_avatar_menu_components(&unavatar);
		assert_eq!(components.len(), 2);
		assert_eq!(components[0].menu_key, "component:0");
		assert_eq!(components[1].menu_key, "external:0:0");
		assert_eq!(components[1].label.as_deref(), Some("External Hat"));
		assert_eq!(components[1].parameter_name.as_deref(), Some("Hat"));
		assert_eq!(components[1].value, Some(1.0));
	}

	#[test]
	fn runtime_action_parameter_selection_respects_inverted_conditions() {
		let actions = un_avatar_core::UnaRuntimeActionSet {
			actions: vec![
				un_avatar_core::UnaRuntimeAction {
					id: "hat:on".to_string(),
					triggers: vec![un_avatar_core::UnaRuntimeActionTrigger::ParameterValue {
						name: "Hat".to_string(),
						value: 1.0,
					}],
					conditions: vec![un_avatar_core::UnaRuntimeActionCondition {
						parameter_name: Some("Hat".to_string()),
						parameter_value: Some(1.0),
						..Default::default()
					}],
					effects: Vec::new(),
					..Default::default()
				},
				un_avatar_core::UnaRuntimeAction {
					id: "hat:off".to_string(),
					triggers: vec![un_avatar_core::UnaRuntimeActionTrigger::ParameterValue {
						name: "Hat".to_string(),
						value: 1.0,
					}],
					conditions: vec![un_avatar_core::UnaRuntimeActionCondition {
						parameter_name: Some("Hat".to_string()),
						parameter_value: Some(1.0),
						inverted: true,
						..Default::default()
					}],
					effects: Vec::new(),
					..Default::default()
				},
				un_avatar_core::UnaRuntimeAction {
					id: "hat:glow".to_string(),
					conditions: vec![un_avatar_core::UnaRuntimeActionCondition {
						parameter_name: Some("Hat".to_string()),
						parameter_value: Some(1.0),
						..Default::default()
					}],
					effects: Vec::new(),
					..Default::default()
				},
			],
		};

		assert_eq!(
			runtime_action_ids_for_parameter(&actions, None, "Hat", 1.0),
			vec!["hat:on".to_string(), "hat:glow".to_string()]
		);
		assert_eq!(
			runtime_action_id_for_parameter(&actions, None, "Hat", 1.0).as_deref(),
			Some("hat:on")
		);
		assert_eq!(
			runtime_action_id_for_parameter(&actions, None, "Hat", 0.0).as_deref(),
			Some("hat:off")
		);
	}

	#[test]
	fn runtime_action_parameter_selection_evaluates_parameter_snapshot_once_per_action() {
		let actions = un_avatar_core::UnaRuntimeActionSet {
			actions: vec![
				un_avatar_core::UnaRuntimeAction {
					id: "hat:on".to_string(),
					triggers: vec![un_avatar_core::UnaRuntimeActionTrigger::ParameterValue {
						name: "Hat".to_string(),
						value: 1.0,
					}],
					conditions: vec![un_avatar_core::UnaRuntimeActionCondition {
						parameter_name: Some("Hat".to_string()),
						parameter_value: Some(1.0),
						..Default::default()
					}],
					..Default::default()
				},
				un_avatar_core::UnaRuntimeAction {
					id: "shared:glow".to_string(),
					triggers: vec![
						un_avatar_core::UnaRuntimeActionTrigger::ParameterValue {
							name: "Glow".to_string(),
							value: 1.0,
						},
						un_avatar_core::UnaRuntimeActionTrigger::ParameterValue {
							name: "Hat".to_string(),
							value: 1.0,
						},
					],
					..Default::default()
				},
			],
		};
		let parameter_values = BTreeMap::from([("Glow".to_string(), 1.0), ("Hat".to_string(), 1.0)]);

		assert_eq!(
			runtime_action_ids_for_parameter_values(&actions, None, &parameter_values),
			vec!["shared:glow".to_string(), "hat:on".to_string()]
		);
	}

	#[test]
	fn wardrobe_action_statuses_summarize_ui_triggers() {
		let actions = un_avatar_core::UnaRuntimeActionSet {
			actions: vec![
				un_avatar_core::UnaRuntimeAction {
					id: "wardrobe:field".to_string(),
					label: "Field Drape".to_string(),
					triggers: vec![
						un_avatar_core::UnaRuntimeActionTrigger::ExpressionMenu {
							path: "Wardrobe/Field Drape".to_string(),
						},
						un_avatar_core::UnaRuntimeActionTrigger::SupervisorCommand {
							command: "field_drape".to_string(),
						},
						un_avatar_core::UnaRuntimeActionTrigger::ParameterValue {
							name: "Outfit".to_string(),
							value: 2.0,
						},
					],
					effects: vec![un_avatar_core::UnaRuntimeActionEffect::WardrobeSet {
						set_id: "field_drape".to_string(),
					}],
					..Default::default()
				},
				un_avatar_core::UnaRuntimeAction {
					id: "expression:smile".to_string(),
					label: "Smile".to_string(),
					effects: vec![un_avatar_core::UnaRuntimeActionEffect::ExpressionWeight {
						name: "Smile".to_string(),
						weight: 1.0,
					}],
					..Default::default()
				},
			],
		};

		let statuses = wardrobe_action_statuses(&actions);

		assert_eq!(statuses.len(), 1);
		assert_eq!(statuses[0].action_id, "wardrobe:field");
		assert_eq!(statuses[0].label, "Field Drape");
		assert_eq!(statuses[0].set_id, "field_drape");
		assert_eq!(statuses[0].expression_menu_path.as_deref(), Some("Wardrobe/Field Drape"));
		assert_eq!(statuses[0].supervisor_command.as_deref(), Some("field_drape"));
		assert_eq!(statuses[0].parameter_name.as_deref(), Some("Outfit"));
		assert_eq!(statuses[0].parameter_value, Some(2.0));
	}

	#[test]
	fn runtime_action_parameter_selection_checks_active_parent_nodes() {
		let mut scene = un_avatar_core::UnaSceneSnapshot {
			nodes: vec![test_scene_node(vec![1]), test_scene_node(Vec::new())],
			roots: vec![0],
			..Default::default()
		};
		let actions = un_avatar_core::UnaRuntimeActionSet {
			actions: vec![un_avatar_core::UnaRuntimeAction {
				id: "hat:on".to_string(),
				triggers: vec![un_avatar_core::UnaRuntimeActionTrigger::ParameterValue {
					name: "Hat".to_string(),
					value: 1.0,
				}],
				conditions: vec![un_avatar_core::UnaRuntimeActionCondition {
					parameter_name: Some("Hat".to_string()),
					parameter_value: Some(1.0),
					active_parent_nodes: vec![un_avatar_core::UnaRuntimeNodeTarget {
						node_index: Some(0),
						..Default::default()
					}],
					..Default::default()
				}],
				effects: Vec::new(),
				..Default::default()
			}],
		};

		assert_eq!(
			runtime_action_id_for_parameter(&actions, Some(&scene), "Hat", 1.0).as_deref(),
			Some("hat:on")
		);
		scene.nodes[0].visible = false;
		assert_eq!(runtime_action_id_for_parameter(&actions, Some(&scene), "Hat", 1.0), None);
	}

	#[test]
	fn runtime_action_parameter_selection_checks_source_node_active_state() {
		let mut scene = un_avatar_core::UnaSceneSnapshot {
			nodes: vec![test_scene_node(Vec::new())],
			roots: vec![0],
			..Default::default()
		};
		let actions = un_avatar_core::UnaRuntimeActionSet {
			actions: vec![un_avatar_core::UnaRuntimeAction {
				id: "hat:on".to_string(),
				conditions: vec![un_avatar_core::UnaRuntimeActionCondition {
					source_node: Some(un_avatar_core::UnaRuntimeNodeTarget {
						node_index: Some(0),
						..Default::default()
					}),
					parameter_name: Some("Hat".to_string()),
					parameter_value: Some(1.0),
					..Default::default()
				}],
				effects: Vec::new(),
				..Default::default()
			}],
		};

		assert_eq!(
			runtime_action_id_for_parameter(&actions, Some(&scene), "Hat", 1.0).as_deref(),
			Some("hat:on")
		);
		scene.nodes[0].visible = false;
		assert_eq!(runtime_action_id_for_parameter(&actions, Some(&scene), "Hat", 1.0), None);
	}

	#[test]
	fn runtime_action_statuses_report_current_condition_state() {
		let actions = un_avatar_core::UnaRuntimeActionSet {
			actions: vec![un_avatar_core::UnaRuntimeAction {
				id: "hat:on".to_string(),
				label: "Hat On".to_string(),
				conditions: vec![un_avatar_core::UnaRuntimeActionCondition {
					parameter_name: Some("Hat".to_string()),
					parameter_value: Some(1.0),
					..Default::default()
				}],
				effects: Vec::new(),
				..Default::default()
			}],
		};
		let active_values = [("Hat".to_string(), 1.0)].into_iter().collect();
		let inactive_values = [("Hat".to_string(), 0.0)].into_iter().collect();
		let missing_values = Default::default();

		let active = runtime_action_statuses(&actions, None, &active_values);
		let inactive = runtime_action_statuses(&actions, None, &inactive_values);
		let missing = runtime_action_statuses(&actions, None, &missing_values);

		assert_eq!(active[0].condition_parameter_names, vec!["Hat"]);
		assert_eq!(active[0].current_condition_state.as_deref(), Some("active"));
		assert_eq!(inactive[0].current_condition_state.as_deref(), Some("inactive"));
		assert_eq!(missing[0].current_condition_state.as_deref(), Some("missing_parameter"));
	}

	#[test]
	fn runtime_action_statuses_report_effect_targets() {
		let actions = un_avatar_core::UnaRuntimeActionSet {
			actions: vec![un_avatar_core::UnaRuntimeAction {
				id: "variant:coat".to_string(),
				label: "Coat".to_string(),
				effects: vec![
					un_avatar_core::UnaRuntimeActionEffect::NodeVisibility {
						target: un_avatar_core::UnaRuntimeNodeTarget {
							node_index: Some(4),
							path: Some("Avatar/Coat".to_string()),
							..Default::default()
						},
						visible: true,
					},
					un_avatar_core::UnaRuntimeActionEffect::MaterialScalar {
						target: un_avatar_core::UnaRuntimeMaterialTarget {
							material_index: Some(2),
							name: Some("Coat".to_string()),
						},
						parameter: "_Cutoff".to_string(),
						value: 0.4,
					},
					un_avatar_core::UnaRuntimeActionEffect::MaterialSlot {
						target: un_avatar_core::UnaRuntimeMaterialSlotTarget {
							node: un_avatar_core::UnaRuntimeNodeTarget {
								node_index: Some(4),
								path: Some("Avatar/Coat".to_string()),
								..Default::default()
							},
							primitive_index: Some(1),
						},
						material: Some(un_avatar_core::UnaRuntimeMaterialTarget {
							material_index: Some(3),
							name: Some("Coat Accent".to_string()),
						}),
					},
					un_avatar_core::UnaRuntimeActionEffect::ExpressionWeight {
						name: "Smile".to_string(),
						weight: 0.75,
					},
					un_avatar_core::UnaRuntimeActionEffect::DynamicsEnabled {
						source_id: "physbone:hair".to_string(),
						enabled: false,
					},
				],
				..Default::default()
			}],
		};

		let statuses = runtime_action_statuses(&actions, None, &Default::default());

		assert_eq!(statuses[0].target_writes[0].owner_key, "action:variant:coat");
		assert_eq!(
			statuses[0].target_writes[0].target_kind,
			un_avatar_core::UnaEvaluationTargetKind::NodeVisibility
		);
		assert_eq!(statuses[0].node_visibility_effects[0].path.as_deref(), Some("Avatar/Coat"));
		assert_eq!(statuses[0].material_property_effects[0].property_kind, "scalar");
		assert_eq!(statuses[0].material_property_effects[0].parameter, "_Cutoff");
		assert_eq!(statuses[0].material_slot_effects[0].material_name.as_deref(), Some("Coat Accent"));
		assert_eq!(statuses[0].expression_weight_effects[0].name, "Smile");
		assert_eq!(statuses[0].dynamics_enabled_effects[0].source_id, "physbone:hair");
		assert!(!statuses[0].dynamics_enabled_effects[0].enabled);
	}

	#[test]
	fn wardrobe_asset_upload_plan_reports_all_resident_until_assets_are_grouped() {
		let mut document = un_avatar_core::UnaDocument {
			unavatar: Some(un_avatar_core::UnaUnavatarExtension {
				spec_version: "0.1-preview".to_string(),
				source: serde_json::json!({
					"wardrobe": {
						"sets": [{
							"id": "base",
							"assetGroups": ["avatar:base", "texture:red"]
						}, {
							"id": "coat",
							"assetGroups": ["outfit:coat", "texture:red"]
						}]
					}
				}),
			}),
			..Default::default()
		};
		document
			.runtime_model_mut()
			.set_active_asset_groups(vec!["outfit:coat".to_string(), "texture:red".to_string()]);

		let plan = wardrobe_asset_upload_plan_for_document(&document);
		assert_eq!(plan.mode, "all-resident");
		assert_eq!(plan.active_asset_groups, vec!["outfit:coat".to_string(), "texture:red".to_string()]);
		assert_eq!(
			plan.declared_asset_groups,
			vec!["avatar:base".to_string(), "outfit:coat".to_string(), "texture:red".to_string()]
		);
		assert!(!plan.scoped_upload_supported);
		assert!(plan.all_resident);
		assert_eq!(
			plan.missing_active_asset_groups,
			vec!["outfit:coat".to_string(), "texture:red".to_string()]
		);
		assert_eq!(plan.resident_mesh_primitive_count, 0);
		assert_eq!(plan.resident_material_count, 0);
		assert_eq!(plan.resident_image_count, 0);
		assert_eq!(plan.resident_dynamics_count, 0);
		assert!(plan
			.reason
			.contains("mesh/texture/material assets do not yet carry group ownership metadata"));
	}

	#[test]
	fn wardrobe_asset_upload_plan_reports_asset_group_ownership_counts() {
		let mut document = un_avatar_core::UnaDocument {
			scene: Some(un_avatar_core::UnaSceneSnapshot {
				asset_group_ownership: vec![
					un_avatar_core::UnaSceneAssetGroupOwnership {
						group_id: "outfit:coat".to_string(),
						mesh_primitives: vec![un_avatar_core::UnaMeshPrimitiveKey {
							mesh_index: 1,
							primitive_index: 2,
						}],
						materials: vec![3],
						images: vec![4, 5],
						dynamics_source_ids: vec!["physbone:coat".to_string()],
					},
					un_avatar_core::UnaSceneAssetGroupOwnership {
						group_id: "avatar:base".to_string(),
						mesh_primitives: vec![un_avatar_core::UnaMeshPrimitiveKey {
							mesh_index: 0,
							primitive_index: 0,
						}],
						materials: vec![0],
						images: vec![0],
						dynamics_source_ids: Vec::new(),
					},
				],
				..Default::default()
			}),
			unavatar: Some(un_avatar_core::UnaUnavatarExtension {
				spec_version: "0.1-preview".to_string(),
				source: serde_json::json!({
					"wardrobe": {
						"sets": [{
							"id": "coat",
							"assetGroups": ["outfit:coat"]
						}]
					}
				}),
			}),
			..Default::default()
		};
		document
			.runtime_model_mut()
			.set_active_asset_groups(vec!["outfit:coat".to_string()]);

		let plan = wardrobe_asset_upload_plan_for_document(&document);
		assert_eq!(plan.owned_asset_group_count, 2);
		assert_eq!(plan.owned_mesh_primitive_count, 2);
		assert_eq!(plan.owned_material_count, 2);
		assert_eq!(plan.owned_image_count, 3);
		assert_eq!(plan.owned_dynamics_count, 1);
		assert_eq!(plan.resident_mesh_primitive_count, 1);
		assert_eq!(plan.resident_material_count, 1);
		assert_eq!(plan.resident_image_count, 2);
		assert_eq!(plan.resident_dynamics_count, 1);
		assert!(plan.missing_active_asset_groups.is_empty());
		assert_eq!(plan.inactive_owned_asset_group_count, 1);
		assert!(plan.scoped_upload_supported);
		assert!(!plan.all_resident);
		assert_eq!(plan.mode, WARDROBE_ASSET_UPLOAD_MODE_RESOURCE_SCOPED);
		assert!(plan
			.reason
			.contains("mesh buffers, image textures, and cubemap resources are scoped"));
	}

	#[test]
	fn wardrobe_asset_upload_plan_uses_document_scoped_source_assets() {
		let mut document = un_avatar_core::UnaDocument {
			scene: Some(un_avatar_core::UnaSceneSnapshot {
				asset_group_ownership: vec![
					un_avatar_core::UnaSceneAssetGroupOwnership {
						group_id: "outfit:coat".to_string(),
						mesh_primitives: vec![
							un_avatar_core::UnaMeshPrimitiveKey {
								mesh_index: 2,
								primitive_index: 1,
							},
							un_avatar_core::UnaMeshPrimitiveKey {
								mesh_index: 2,
								primitive_index: 1,
							},
						],
						materials: vec![5, 3, 3],
						images: vec![7, 4, 7],
						dynamics_source_ids: vec!["physbone:coat".to_string()],
					},
					un_avatar_core::UnaSceneAssetGroupOwnership {
						group_id: "avatar:base".to_string(),
						mesh_primitives: vec![un_avatar_core::UnaMeshPrimitiveKey {
							mesh_index: 0,
							primitive_index: 0,
						}],
						materials: vec![0],
						images: vec![0],
						dynamics_source_ids: vec!["spring:base".to_string()],
					},
				],
				..Default::default()
			}),
			..Default::default()
		};
		document
			.runtime_model_mut()
			.set_active_asset_groups(vec!["outfit:coat".to_string(), "missing:hat".to_string()]);

		let work = document.scoped_asset_selection();
		assert_eq!(work.owned_active_groups, vec!["outfit:coat".to_string()]);
		assert_eq!(work.missing_active_asset_groups, vec!["missing:hat".to_string()]);
		assert_eq!(
			work.mesh_primitives,
			vec![un_avatar_core::UnaMeshPrimitiveKey {
				mesh_index: 2,
				primitive_index: 1,
			}]
		);
		assert_eq!(work.materials, vec![3, 5]);
		assert_eq!(work.images, vec![4, 7]);
		assert_eq!(work.dynamics_source_ids, vec!["physbone:coat".to_string()]);
	}

	#[test]
	fn wardrobe_asset_upload_plan_can_include_renderer_draw_residency_counts() {
		let plan = wardrobe_asset_upload_plan_with_draw_counts(
			WardrobeAssetUploadPlan {
				mode: WARDROBE_ASSET_UPLOAD_MODE_RESOURCE_SCOPED.to_string(),
				scoped_upload_supported: true,
				all_resident: false,
				..Default::default()
			},
			Some(SceneMeshAssetResidencyCounts {
				total_draw_mesh_primitive_count: 3,
				resident_draw_mesh_primitive_count: 2,
				inactive_draw_mesh_primitive_count: 1,
				total_draw_mesh_buffer_bytes: 3000,
				resident_draw_mesh_buffer_bytes: 2000,
				inactive_draw_mesh_buffer_bytes: 1000,
				total_image_texture_count: 4,
				resident_image_texture_count: 3,
				inactive_image_texture_count: 1,
				draws_using_inactive_image_texture_count: 2,
				active_draws_using_inactive_image_texture_count: 1,
				inactive_image_textures_used_by_active_draw_count: 1,
				inactive_image_textures_used_by_active_draw: vec![3],
				total_material_slot_count: 5,
				resident_material_slot_count: 4,
				inactive_material_slot_count: 1,
				active_draws_using_inactive_material_slot_count: 1,
				inactive_material_slots_used_by_active_draw_count: 1,
				inactive_material_slots_used_by_active_draw: vec![4],
			}),
		);

		assert_eq!(plan.total_draw_mesh_primitive_count, 3);
		assert_eq!(plan.resident_draw_mesh_primitive_count, 2);
		assert_eq!(plan.inactive_draw_mesh_primitive_count, 1);
		assert_eq!(plan.total_draw_mesh_buffer_bytes, 3000);
		assert_eq!(plan.resident_draw_mesh_buffer_bytes, 2000);
		assert_eq!(plan.inactive_draw_mesh_buffer_bytes, 1000);
		assert_eq!(plan.total_image_texture_count, 4);
		assert_eq!(plan.resident_image_texture_count, 3);
		assert_eq!(plan.inactive_image_texture_count, 1);
		assert_eq!(plan.draws_using_inactive_image_texture_count, 2);
		assert_eq!(plan.active_draws_using_inactive_image_texture_count, 1);
		assert_eq!(plan.inactive_image_textures_used_by_active_draw_count, 1);
		assert_eq!(plan.inactive_image_textures_used_by_active_draw, vec![3]);
		assert_eq!(plan.total_material_slot_count, 5);
		assert_eq!(plan.resident_material_slot_count, 4);
		assert_eq!(plan.inactive_material_slot_count, 1);
		assert_eq!(plan.active_draws_using_inactive_material_slot_count, 1);
		assert_eq!(plan.inactive_material_slots_used_by_active_draw_count, 1);
		assert_eq!(plan.inactive_material_slots_used_by_active_draw, vec![4]);
		assert_eq!(plan.pending_image_texture_upload_count, 1);
		assert_eq!(plan.pending_material_slot_upload_count, 1);
		assert!(plan.scoped_draw_supported);
		assert!(plan.scoped_upload_supported);
		assert!(!plan.all_resident);
		assert!(plan.active_residency_gaps_detected);
		assert_eq!(plan.residency_gap_index_status_limit, WARDROBE_RESIDENCY_GAP_INDEX_STATUS_LIMIT);
	}

	#[test]
	fn wardrobe_asset_upload_plan_bounds_renderer_residency_gap_index_lists() {
		let image_indices = (0..WARDROBE_RESIDENCY_GAP_INDEX_STATUS_LIMIT + 2).collect::<Vec<_>>();
		let material_indices = (100..100 + WARDROBE_RESIDENCY_GAP_INDEX_STATUS_LIMIT + 3).collect::<Vec<_>>();
		let plan = wardrobe_asset_upload_plan_with_draw_counts(
			WardrobeAssetUploadPlan {
				mode: WARDROBE_ASSET_UPLOAD_MODE_RESOURCE_SCOPED.to_string(),
				scoped_upload_supported: true,
				all_resident: false,
				..Default::default()
			},
			Some(SceneMeshAssetResidencyCounts {
				active_draws_using_inactive_image_texture_count: 1,
				inactive_image_textures_used_by_active_draw_count: image_indices.len(),
				inactive_image_textures_used_by_active_draw: image_indices,
				active_draws_using_inactive_material_slot_count: 1,
				inactive_material_slots_used_by_active_draw_count: material_indices.len(),
				inactive_material_slots_used_by_active_draw: material_indices,
				..Default::default()
			}),
		);

		assert_eq!(
			plan.inactive_image_textures_used_by_active_draw_count,
			WARDROBE_RESIDENCY_GAP_INDEX_STATUS_LIMIT + 2
		);
		assert_eq!(
			plan.inactive_image_textures_used_by_active_draw.len(),
			WARDROBE_RESIDENCY_GAP_INDEX_STATUS_LIMIT
		);
		assert_eq!(
			plan.inactive_image_textures_used_by_active_draw.last(),
			Some(&(WARDROBE_RESIDENCY_GAP_INDEX_STATUS_LIMIT - 1))
		);
		assert!(plan.inactive_image_textures_used_by_active_draw_truncated);
		assert_eq!(
			plan.inactive_material_slots_used_by_active_draw_count,
			WARDROBE_RESIDENCY_GAP_INDEX_STATUS_LIMIT + 3
		);
		assert_eq!(
			plan.inactive_material_slots_used_by_active_draw.len(),
			WARDROBE_RESIDENCY_GAP_INDEX_STATUS_LIMIT
		);
		assert_eq!(
			plan.inactive_material_slots_used_by_active_draw.last(),
			Some(&(100 + WARDROBE_RESIDENCY_GAP_INDEX_STATUS_LIMIT - 1))
		);
		assert!(plan.inactive_material_slots_used_by_active_draw_truncated);
		assert_eq!(
			plan.pending_image_texture_upload_count,
			WARDROBE_RESIDENCY_GAP_INDEX_STATUS_LIMIT + 2
		);
		assert_eq!(
			plan.pending_material_slot_upload_count,
			WARDROBE_RESIDENCY_GAP_INDEX_STATUS_LIMIT + 3
		);
		assert!(plan.active_residency_gaps_detected);
		assert_eq!(plan.residency_gap_index_status_limit, WARDROBE_RESIDENCY_GAP_INDEX_STATUS_LIMIT);
	}

	#[test]
	fn wardrobe_scoped_upload_work_keeps_full_active_gap_lists() {
		let image_indices = (0..WARDROBE_RESIDENCY_GAP_INDEX_STATUS_LIMIT + 2).collect::<Vec<_>>();
		let material_indices = (100..100 + WARDROBE_RESIDENCY_GAP_INDEX_STATUS_LIMIT + 3).collect::<Vec<_>>();
		let work = wardrobe_scoped_upload_work_for_active_gaps(Some(SceneMeshActiveResidencyGaps {
			inactive_image_texture_indices: image_indices.clone(),
			inactive_material_slot_indices: material_indices.clone(),
			active_draws_using_inactive_image_texture_count: 4,
			active_draws_using_inactive_material_slot_count: 5,
		}));

		assert!(work.has_pending_uploads());
		assert_eq!(work.image_texture_indices, image_indices);
		assert_eq!(work.material_slot_indices, material_indices);
		assert_eq!(work.active_draws_using_inactive_image_texture_count, 4);
		assert_eq!(work.active_draws_using_inactive_material_slot_count, 5);
	}

	fn test_scene_node(children: Vec<usize>) -> un_avatar_core::UnaSceneNode {
		un_avatar_core::UnaSceneNode {
			name: None,
			source_node_id: None,
			resolved_node_id: None,
			visible: true,
			transform: [
				1.0, 0.0, 0.0, 0.0, //
				0.0, 1.0, 0.0, 0.0, //
				0.0, 0.0, 1.0, 0.0, //
				0.0, 0.0, 0.0, 1.0,
			],
			children,
			mesh: None,
			skin: None,
			probe_anchor_node: None,
			local_bounds: None,
		}
	}

	#[test]
	fn high_capability_liltoon_texture_budget_covers_highest_mesh_binding() {
		assert_eq!(HIGH_CAPABILITY_LILTOON_SAMPLED_TEXTURES_PER_STAGE, 56);
	}

	#[test]
	fn mesh_shader_variant_tier_uses_shared_resource_limits() {
		let mut baseline = wgpu::Limits::downlevel_defaults();
		baseline.max_sampled_textures_per_shader_stage = BASELINE_FALLBACK_SAMPLED_TEXTURES_PER_STAGE;
		baseline.max_samplers_per_shader_stage = BASELINE_FALLBACK_SAMPLERS_PER_STAGE;
		assert_eq!(
			mesh_shader_variant_tier_for_limits(&baseline),
			MeshShaderVariantTier::BaselineFallback
		);
		let baseline_plan = mesh_shader_resource_plan_for_adapter(&baseline);
		assert_eq!(baseline_plan.tier, MeshShaderVariantTier::BaselineFallback);
		assert_eq!(
			baseline_plan.required_limits.max_sampled_textures_per_shader_stage,
			BASELINE_FALLBACK_SAMPLED_TEXTURES_PER_STAGE
		);
		assert_eq!(
			baseline_plan.required_limits.max_samplers_per_shader_stage,
			BASELINE_FALLBACK_SAMPLERS_PER_STAGE
		);

		let mut high_capability = baseline.clone();
		high_capability.max_sampled_textures_per_shader_stage = HIGH_CAPABILITY_LILTOON_SAMPLED_TEXTURES_PER_STAGE + 8;
		high_capability.max_samplers_per_shader_stage = HIGH_CAPABILITY_LILTOON_SAMPLERS_PER_STAGE + 4;
		assert_eq!(
			mesh_shader_variant_tier_for_limits(&high_capability),
			MeshShaderVariantTier::HighCapability
		);
		let high_plan = mesh_shader_resource_plan_for_adapter(&high_capability);
		assert_eq!(high_plan.tier, MeshShaderVariantTier::HighCapability);
		assert_eq!(
			high_plan.required_limits.max_sampled_textures_per_shader_stage,
			HIGH_CAPABILITY_LILTOON_SAMPLED_TEXTURES_PER_STAGE
		);
		assert_eq!(
			high_plan.required_limits.max_samplers_per_shader_stage,
			HIGH_CAPABILITY_LILTOON_SAMPLERS_PER_STAGE
		);
	}

	#[test]
	fn transparent_alpha_mode_prefers_explicit_premultiplied_alpha_over_auto() {
		assert_eq!(
			transparent_alpha_mode(&[Auto, Opaque, PostMultiplied, PreMultiplied]),
			PreMultiplied
		);
	}

	#[test]
	fn transparent_alpha_mode_uses_straight_alpha_when_premultiplied_is_missing() {
		assert_eq!(transparent_alpha_mode(&[Auto, Opaque, PostMultiplied]), PostMultiplied);
	}
}
