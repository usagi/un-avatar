//! wgpu デバイス・スワップチェーン・深度・プロシージャル空スカイ（カメラ／ライトのユニフォーム検証用）。

use std::{
	borrow::Cow,
	collections::{BTreeMap, HashMap},
	fmt::Write as _,
	net::SocketAddr,
	sync::{
		atomic::{AtomicU64, AtomicU8, Ordering},
		Arc, Mutex, RwLock,
	},
	time::{Duration, Instant},
};

use glam::{Mat4, Quat, Vec3, Vec4};
use serde_json::Value;
use un_avatar_core::{
	una_dynamics_translation_writeback_candidate_count, una_dynamics_translation_writeback_target_count, UnaDocument,
	UnaEvaluationTargetKind, UnaExpressionCatalog, UnaNodeConstraintKind, UnaRuntimeActionEffect, UnaRuntimeActionQuery,
	UnaRuntimeActionTrigger, UnaRuntimeDynamics, UnaRuntimeDynamicsCounts, UnaRuntimeNodeTarget, UnaSceneNode, UnaSceneSnapshot,
};
use un_avatar_skeleton::{
	annotate_dynamics_response_group_visibility, build_dynamics_bone_colliders_with_sources, classify_dynamics_group_category,
	collider_stats, distance_point_segment, dynamics_group_match_text, dynamics_mesh_cloth_assist_deforming_nodes,
	dynamics_normalize_match_text, dynamics_normalized_token_filter_matches, local_capsule_world, local_sphere_world, BoneColliderConfig,
	BoneColliderPrimitive, BoneColliderSource, BoneColliderStats, DynamicsPhysicsConfig, DynamicsSimulator, DynamicsStepProfile,
	DynamicsSurfaceConstraint, DynamicsTailSample, DynamicsVisualTargetContext, RuntimeBoneColliderPrimitive,
};
use winit::window::Window;

use crate::{
	camera::OrbitCamera,
	debug_dump::log_material_skin_report,
	debug_log::DebugLog,
	mesh_pass::{
		AvatarOutlineOptions, AvatarOutlinePolicy, DrawTransformUpdateTimings, MeshShaderVariantTier, SceneMeshActiveResidencyGaps,
		SceneMeshAssetResidencyCounts, SceneMeshAssetResidencyRefresh, SceneMeshBuildProgress, SceneMeshLoadOpts,
		SceneMeshRuntimeRequirements, SceneMeshes, TextureUploadSummary,
	},
	model_loader,
	options::{
		AudioLinkOptions, AudioLinkSource, AvatarWindowOptions, BloomOptions, ColorGradingLook, ContactShadowOptions,
		EnvironmentColorOptions, LightingOptions,
	},
	pipeline_cache::PersistentPipelineCache,
	post_process::PostProcess,
	AaMode, BlockCompressionEncoder, RenderBackend, SpoutWindowOptions, TextureCompressionAdvancedOptions, TextureCompressionMode,
	WindowDebugOptions,
};

const SHADER_SKY: &str = include_str!("../shaders/sky.wgsl");
const SHADER_AXES: &str = include_str!("../shaders/axes.wgsl");
const SHADER_BONE_COLLIDERS: &str = include_str!("../shaders/bone_colliders.wgsl");
const SHADER_STARTUP_PROGRESS_OVERLAY: &str = include_str!("../shaders/startup_progress_overlay.wgsl");
const SHADER_WARDROBE_BILLBOARD: &str = include_str!("../shaders/wardrobe_billboard.wgsl");
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

#[derive(Clone, Debug, Default)]
struct AnimatorMorphOverrideCache {
	document_revision: u64,
	parameter_values: BTreeMap<String, f32>,
	overrides: BTreeMap<String, f32>,
	valid: bool,
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
	#[serde(default)]
	pub(crate) available: bool,
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
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) menu_path: Vec<String>,
	#[serde(default, skip_serializing_if = "std::ops::Not::not")]
	pub(crate) menu_path_truncated: bool,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) menu_label: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) control_type: Option<String>,
	pub(crate) parameter_name: String,
	pub(crate) parameter_value: f32,
	pub(crate) action_id: String,
	pub(crate) action_label: String,
	pub(crate) match_kind: String,
	pub(crate) inverted: bool,
	#[serde(default)]
	pub(crate) available: bool,
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
	pub(crate) resident_in_active_assets: bool,
	pub(crate) solver_enabled: bool,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) runtime_enabled_override: Option<bool>,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) source_id: String,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) comment: String,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) category: String,
	pub(crate) bone_count: usize,
	pub(crate) visual_target: bool,
	pub(crate) skinned_joint_count: usize,
	pub(crate) mesh_subtree_node_count: usize,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) root_node: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) root_path: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) tip_node: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) tip_path: Option<String>,
	pub(crate) stiffness: f32,
	pub(crate) pull: f32,
	pub(crate) spring: f32,
	pub(crate) integration_type: un_avatar_core::UnaDynamicsIntegrationType,
	pub(crate) drag_force: f32,
	pub(crate) gravity_power: f32,
	pub(crate) gravity_falloff: f32,
	pub(crate) immobile: f32,
	pub(crate) immobile_type: un_avatar_core::UnaDynamicsImmobileType,
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
	pub(crate) limit_rotation: Option<[f32; 3]>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) max_angle_x: Option<f32>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) max_angle_z: Option<f32>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) max_stretch: Option<f32>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) max_squish: Option<f32>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) stretch_motion: Option<f32>,
	pub(crate) max_stretch_sample_has_positive: bool,
	pub(crate) max_squish_sample_has_positive: bool,
	pub(crate) stretch_motion_sample_has_positive: bool,
	pub(crate) writeback_mode: un_avatar_core::UnaDynamicsWritebackMode,
	pub(crate) translation_writeback_candidate_count: usize,
	pub(crate) translation_writeback_target_count: usize,
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
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) source_id: String,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) collider_path: String,
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
pub(crate) struct RuntimeDynamicsColliderSelectionStatus {
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) source_id: String,
	pub(crate) selected_collider_count: usize,
	pub(crate) global_collider_count: usize,
	pub(crate) authored_collider_count: usize,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) sample_collider_indices: Vec<usize>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) sample_collider_source_ids: Vec<String>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) sample_collider_paths: Vec<String>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) sample_colliders: Vec<String>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) sample_collider_details: Vec<serde_json::Value>,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct RuntimeDynamicsColliderContactStatus {
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) source_id: String,
	pub(crate) runtime_index: usize,
	pub(crate) joint_index: usize,
	pub(crate) parent_node: usize,
	pub(crate) child_node: usize,
	pub(crate) collider_index: Option<usize>,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) collider_path: String,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) collider_shape: String,
	pub(crate) hit_radius: f32,
	pub(crate) collider_radius: f32,
	pub(crate) distance: f32,
	pub(crate) threshold: f32,
	pub(crate) margin: f32,
	pub(crate) inside_bounds: bool,
	pub(crate) penetrating: bool,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct RuntimeDynamicsColliderContactSummary {
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) source_id: String,
	pub(crate) contact_count: usize,
	pub(crate) penetrating_count: usize,
	pub(crate) min_margin: f32,
	pub(crate) min_distance: f32,
	pub(crate) min_threshold: f32,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) closest_collider_path: String,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) closest_collider_shape: String,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct RuntimeDynamicsColliderRuntimeSummary {
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) source_id: String,
	pub(crate) selected_collider_count: usize,
	pub(crate) global_collider_count: usize,
	pub(crate) authored_collider_count: usize,
	pub(crate) contact_count: usize,
	pub(crate) penetrating_count: usize,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) min_margin: Option<f32>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) min_distance: Option<f32>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) min_threshold: Option<f32>,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) closest_collider_path: String,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) closest_collider_shape: String,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) sample_collider_paths: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct RuntimeDynamicsColliderPathContactSummary {
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) collider_path: String,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) collider_shape: String,
	pub(crate) contact_count: usize,
	pub(crate) penetrating_count: usize,
	pub(crate) source_count: usize,
	pub(crate) min_margin: f32,
	pub(crate) min_distance: f32,
	pub(crate) min_threshold: f32,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) sample_source_ids: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct RuntimeDynamicsColliderPathCandidateSummary {
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) collider_path: String,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) collider_shape: String,
	pub(crate) candidate_count: usize,
	pub(crate) penetrating_count: usize,
	pub(crate) source_count: usize,
	pub(crate) min_margin: f32,
	pub(crate) min_distance: f32,
	pub(crate) min_threshold: f32,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) sample_source_ids: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct RuntimeDynamicsColliderPathRuntimeSummary {
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) collider_path: String,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub(crate) collider_shape: String,
	pub(crate) runtime_collider_count: usize,
	pub(crate) candidate_count: usize,
	pub(crate) candidate_penetrating_count: usize,
	pub(crate) source_count: usize,
	pub(crate) contact_count: usize,
	pub(crate) penetrating_count: usize,
	pub(crate) projection_count: u32,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) min_margin: Option<f32>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) min_distance: Option<f32>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub(crate) min_threshold: Option<f32>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) sample_source_ids: Vec<String>,
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
	pub(crate) active_draws_using_inactive_cube_texture_count: usize,
	pub(crate) inactive_cube_textures_used_by_active_draw_count: usize,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) inactive_cube_textures_used_by_active_draw: Vec<usize>,
	pub(crate) inactive_cube_textures_used_by_active_draw_truncated: bool,
	pub(crate) total_material_slot_count: usize,
	pub(crate) resident_material_slot_count: usize,
	pub(crate) inactive_material_slot_count: usize,
	pub(crate) active_draws_using_inactive_material_slot_count: usize,
	pub(crate) inactive_material_slots_used_by_active_draw_count: usize,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) inactive_material_slots_used_by_active_draw: Vec<usize>,
	pub(crate) inactive_material_slots_used_by_active_draw_truncated: bool,
	pub(crate) pending_image_texture_upload_count: usize,
	pub(crate) pending_cube_texture_upload_count: usize,
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
	cube_texture_indices: Vec<usize>,
	material_slot_indices: Vec<usize>,
	active_draws_using_inactive_image_texture_count: usize,
	active_draws_using_inactive_cube_texture_count: usize,
	active_draws_using_inactive_material_slot_count: usize,
}

impl WardrobeScopedUploadWork {
	fn has_pending_uploads(&self) -> bool {
		!self.image_texture_indices.is_empty() || !self.cube_texture_indices.is_empty() || !self.material_slot_indices.is_empty()
	}
}

fn wardrobe_scoped_upload_work_for_active_gaps(active_gaps: Option<SceneMeshActiveResidencyGaps>) -> WardrobeScopedUploadWork {
	let Some(active_gaps) = active_gaps else {
		return WardrobeScopedUploadWork::default();
	};
	WardrobeScopedUploadWork {
		image_texture_indices: active_gaps.inactive_image_texture_indices,
		cube_texture_indices: active_gaps.inactive_cube_texture_indices,
		material_slot_indices: active_gaps.inactive_material_slot_indices,
		active_draws_using_inactive_image_texture_count: active_gaps.active_draws_using_inactive_image_texture_count,
		active_draws_using_inactive_cube_texture_count: active_gaps.active_draws_using_inactive_cube_texture_count,
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
						.filter_map(|group| group.as_str().map(str::to_owned))
				})
				.collect::<Vec<_>>()
		})
		.unwrap_or_default();
	declared_asset_groups.sort();
	declared_asset_groups.dedup();
	let runtime_model = document.runtime_model();
	let active_asset_groups = runtime_model.active_asset_groups().to_vec();
	let has_declared_groups = !declared_asset_groups.is_empty();
	let ownership = document
		.scene
		.as_ref()
		.map(|scene| scene.asset_group_ownership_counts())
		.unwrap_or_default();
	let has_ownership = ownership.groups > 0;
	let source_asset_work = runtime_model.scoped_asset_selection();
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
		active_draws_using_inactive_cube_texture_count: 0,
		inactive_cube_textures_used_by_active_draw_count: 0,
		inactive_cube_textures_used_by_active_draw: Vec::new(),
		inactive_cube_textures_used_by_active_draw_truncated: false,
		total_material_slot_count: 0,
		resident_material_slot_count: 0,
		inactive_material_slot_count: 0,
		active_draws_using_inactive_material_slot_count: 0,
		inactive_material_slots_used_by_active_draw_count: 0,
		inactive_material_slots_used_by_active_draw: Vec::new(),
		inactive_material_slots_used_by_active_draw_truncated: false,
		pending_image_texture_upload_count: 0,
		pending_cube_texture_upload_count: 0,
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
	plan.active_draws_using_inactive_cube_texture_count = draw_counts.active_draws_using_inactive_cube_texture_count;
	plan.inactive_cube_textures_used_by_active_draw_count = draw_counts.inactive_cube_textures_used_by_active_draw_count;
	let inactive_cube_textures_used_by_active_draw =
		wardrobe_residency_gap_index_status_list(draw_counts.inactive_cube_textures_used_by_active_draw);
	plan.inactive_cube_textures_used_by_active_draw = inactive_cube_textures_used_by_active_draw.indices;
	plan.inactive_cube_textures_used_by_active_draw_truncated = inactive_cube_textures_used_by_active_draw.truncated;
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
	plan.pending_cube_texture_upload_count = draw_counts.inactive_cube_textures_used_by_active_draw_count;
	plan.pending_material_slot_upload_count = draw_counts.inactive_material_slots_used_by_active_draw_count;
	plan.scoped_draw_supported =
		draw_counts.inactive_draw_mesh_primitive_count > 0 || plan.mode == WARDROBE_ASSET_UPLOAD_MODE_RESOURCE_SCOPED;
	plan.active_residency_gaps_detected = draw_counts.active_draws_using_inactive_image_texture_count > 0
		|| draw_counts.active_draws_using_inactive_cube_texture_count > 0
		|| draw_counts.active_draws_using_inactive_material_slot_count > 0;
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
	let mut condition_matches = Vec::new();
	let mut trigger_matches = Vec::new();
	let query = UnaRuntimeActionQuery {
		parameter_name: Some(name),
		parameter_value: Some(value),
		..Default::default()
	};
	for action in &actions.actions {
		match action.parameter_condition_state_in_scene(scene, name, value) {
			Some(true) => condition_matches.push(action.id.clone()),
			None if action.matches_query(query) => trigger_matches.push(action.id.clone()),
			_ => {}
		}
	}
	if !condition_matches.is_empty() {
		return condition_matches;
	}
	trigger_matches
}

fn runtime_action_ids_for_parameter_values(
	actions: &un_avatar_core::UnaRuntimeActionSet,
	scene: Option<&un_avatar_core::UnaSceneSnapshot>,
	parameter_values: &BTreeMap<String, f32>,
) -> Vec<String> {
	let mut ids = Vec::new();
	for (name, value) in parameter_values {
		for id in runtime_action_ids_for_parameter(actions, scene, name, *value) {
			if !ids.iter().any(|seen| seen == &id) {
				ids.push(id);
			}
		}
	}
	ids
}

fn runtime_actions_reference_parameter(actions: &un_avatar_core::UnaRuntimeActionSet, name: &str) -> bool {
	actions.actions.iter().any(|action| {
		action
			.conditions
			.iter()
			.any(|condition| condition.parameter_name.as_deref() == Some(name))
			|| action.triggers.iter().any(|trigger| {
				matches!(
					trigger,
					UnaRuntimeActionTrigger::ParameterValue {
						name: trigger_name,
						..
					} if trigger_name == name
				)
			})
	})
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
			available: runtime_action_available(action, scene),
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
	control_type: Option<String>,
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
	scene: Option<&UnaSceneSnapshot>,
) -> Option<Vec<RuntimeMenuActionCandidateStatus>> {
	let Some(unavatar) = unavatar else {
		return Some(Vec::new());
	};
	let menu_components = modular_avatar_menu_components(unavatar);
	if menu_components.is_empty() {
		return Some(Vec::new());
	}
	let menu_graph_candidates = modular_avatar_menu_graph_candidates(&menu_components);
	let menu_graph_nodes = modular_avatar_menu_graph_nodes(&menu_graph_candidates);
	let menu_path_by_key = menu_graph_nodes
		.iter()
		.enumerate()
		.map(|(index, node)| (node.menu_key.as_str(), menu_graph_node_path(&menu_graph_nodes, index)))
		.collect::<BTreeMap<_, _>>();
	let mut candidates = Vec::new();
	for menu in &menu_components {
		let (Some(parameter_name), Some(parameter_value)) = (&menu.parameter_name, menu.value) else {
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
			candidates.push(RuntimeMenuActionCandidateStatus {
				menu_component_index: menu.component_index,
				menu_key: menu.menu_key.clone(),
				menu_path: menu_path_by_key
					.get(menu.menu_key.as_str())
					.map(|path| path.labels.clone())
					.unwrap_or_default(),
				menu_path_truncated: menu_path_by_key.get(menu.menu_key.as_str()).is_some_and(|path| path.truncated),
				menu_label: menu.label.clone(),
				control_type: menu.control_type.clone(),
				parameter_name: parameter_name.clone(),
				parameter_value,
				action_id: action.id.clone(),
				action_label: action.label.clone(),
				match_kind: match_kind.to_string(),
				inverted,
				available: action_menu_conditions_available(action, scene),
				effect_count: action.effects.len(),
				effect_kinds: runtime_action_effect_kind_counts(action.effects.iter()),
				wardrobe_set_ids,
			});
		}
		let menu_path = menu_path_by_key
			.get(menu.menu_key.as_str())
			.map(|path| path.labels.clone())
			.unwrap_or_default();
		if !matched_any_action && metadata_menu_candidate_visible(menu, &menu_path) {
			candidates.push(RuntimeMenuActionCandidateStatus {
				menu_component_index: menu.component_index,
				menu_key: menu.menu_key.clone(),
				menu_path,
				menu_path_truncated: menu_path_by_key.get(menu.menu_key.as_str()).is_some_and(|path| path.truncated),
				menu_label: menu.label.clone(),
				control_type: menu.control_type.clone(),
				parameter_name: parameter_name.clone(),
				parameter_value,
				action_id: format!("menu:{}", menu.menu_key),
				action_label: menu
					.label
					.clone()
					.unwrap_or_else(|| format!("{}={parameter_value}", parameter_name)),
				match_kind: "metadata".to_string(),
				inverted: false,
				available: true,
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
	Some(candidates)
}

fn metadata_menu_candidate_visible(menu: &RuntimeMenuComponentSummary, menu_path: &[String]) -> bool {
	if menu.control_type.as_deref() == Some("Button") {
		return false;
	}
	if menu_path.len() > 2 {
		return false;
	}
	if menu_path
		.iter()
		.any(|segment| segment == "Face_Tracking" || segment.contains("VRCFT") || segment.contains('<'))
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

fn action_menu_conditions_available(action: &un_avatar_core::UnaRuntimeAction, scene: Option<&UnaSceneSnapshot>) -> bool {
	let mut saw_scene_gate = false;
	for condition in &action.conditions {
		if condition.source_node.is_none() && condition.active_parent_nodes.is_empty() {
			continue;
		}
		saw_scene_gate = true;
		if condition.source_node_matches(scene) && condition.active_parent_nodes_match(scene) {
			return true;
		}
	}
	!saw_scene_gate
}

fn runtime_node_target_index(scene: &UnaSceneSnapshot, target: &UnaRuntimeNodeTarget) -> Option<usize> {
	if let Some(index) = target.node_index {
		if index < scene.nodes.len() {
			return Some(index);
		}
	}
	if let Some(source_node_id) = target.source_node_id.as_deref() {
		if let Some(index) = scene
			.nodes
			.iter()
			.position(|node| node.source_node_id.as_deref() == Some(source_node_id))
		{
			return Some(index);
		}
	}
	if let Some(resolved_node_id) = target.resolved_node_id.as_deref() {
		if let Some(index) = scene
			.nodes
			.iter()
			.position(|node| node.resolved_node_id.as_deref() == Some(resolved_node_id))
		{
			return Some(index);
		}
	}
	let path = target.path.as_deref()?;
	scene_node_paths_by_index(scene)
		.into_iter()
		.position(|candidate| candidate.as_deref() == Some(path))
}

fn runtime_node_target_parent_available(scene: &UnaSceneSnapshot, target: &UnaRuntimeNodeTarget) -> bool {
	let Some(index) = runtime_node_target_index(scene, target) else {
		return false;
	};
	for (parent_index, node) in scene.nodes.iter().enumerate() {
		if node.children.contains(&index) {
			return scene.effective_node_visible(parent_index);
		}
	}
	true
}

fn action_effect_targets_available(action: &un_avatar_core::UnaRuntimeAction, scene: Option<&UnaSceneSnapshot>) -> bool {
	let Some(scene) = scene else {
		return true;
	};
	let mut saw_scene_target = false;
	for effect in &action.effects {
		match effect {
			UnaRuntimeActionEffect::NodeVisibility { target, .. } => {
				saw_scene_target = true;
				if runtime_node_target_parent_available(scene, target) {
					return true;
				}
			}
			UnaRuntimeActionEffect::MaterialSlot { target, .. } => {
				saw_scene_target = true;
				if runtime_node_target_parent_available(scene, &target.node) {
					return true;
				}
			}
			_ => {}
		}
	}
	!saw_scene_target
}

fn runtime_action_available(action: &un_avatar_core::UnaRuntimeAction, scene: Option<&UnaSceneSnapshot>) -> bool {
	action_menu_conditions_available(action, scene) && action_effect_targets_available(action, scene)
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
			.unwrap_or_else(|| {
				if !action_candidate.menu_path.is_empty() {
					RuntimeMenuGraphNodePath {
						labels: action_candidate.menu_path.clone(),
						truncated: action_candidate.menu_path_truncated,
					}
				} else {
					RuntimeMenuGraphNodePath {
						labels: action_candidate.menu_label.iter().cloned().collect(),
						truncated: false,
					}
				}
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

fn dynamics_group_statuses_with_limit(
	doc: &UnaDocument,
	categories: &[un_avatar_skeleton::DynamicsCategoryDefinition],
	limit: Option<usize>,
) -> Vec<RuntimeDynamicsGroupStatus> {
	let runtime_model = doc.runtime_model();
	let scene = runtime_model.scene();
	let node_paths_by_index = runtime_model.scene().map(scene_node_paths_by_index).unwrap_or_default();
	let dynamics = runtime_model.dynamics();
	let visual_target_context = scene.map(DynamicsVisualTargetContext::for_scene);
	let iter = dynamics.dynamics_groups().enumerate().map(|(index, group)| {
		let source_group = dynamics.group(index);
		let resident_in_active_assets = scene
			.map(|scene| dynamics.source_id_resident_in_scene(scene, group.source_id))
			.unwrap_or(true);
		let root_node = group.chain.bone_node_indices.first().copied();
		let tip_node = group.chain.bone_node_indices.last().copied();
		let center_node = group.parameters.center_node;
		let (skinned_joint_count, mesh_subtree_node_count) = visual_target_context
			.as_ref()
			.map(|context| context.group_counts(group.chain.bone_node_indices))
			.unwrap_or_default();
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
			resident_in_active_assets,
			solver_enabled: group.effective_enabled && resident_in_active_assets,
			runtime_enabled_override: source_group.and_then(|source_group| dynamics.group_enabled_override(source_group)),
			source_id: group.source_id.to_string(),
			comment: group.comment.to_string(),
			category: scene
				.map(|scene| classify_dynamics_group_category(scene, group, &categories))
				.unwrap_or_else(|| group.category.to_string()),
			bone_count: group.chain.bone_node_indices.len(),
			visual_target: skinned_joint_count > 0 || mesh_subtree_node_count > 0,
			skinned_joint_count,
			mesh_subtree_node_count,
			root_node,
			root_path: root_node.and_then(|node| node_paths_by_index.get(node).cloned().flatten()),
			tip_node,
			tip_path: tip_node.and_then(|node| node_paths_by_index.get(node).cloned().flatten()),
			stiffness: group.parameters.stiffness,
			pull: group.parameters.pull,
			spring: group.parameters.spring,
			integration_type: group.parameters.integration_type,
			drag_force: group.parameters.drag_force,
			gravity_power: group.parameters.gravity_power,
			gravity_falloff: group.parameters.gravity_falloff,
			immobile: group.parameters.immobile,
			immobile_type: group.parameters.immobile_type,
			gravity_dir: group.parameters.gravity_dir,
			hit_radius: group.parameters.hit_radius,
			hit_radius_sample_count,
			hit_radius_sample_min,
			hit_radius_sample_max,
			center_node,
			center_path: center_node.and_then(|node| node_paths_by_index.get(node).cloned().flatten()),
			limit_type,
			limit_rotation: group.limit.map(|limit| limit.limit_rotation),
			max_angle_x: group.limit.map(|limit| limit.max_angle_x),
			max_angle_z: group.limit.map(|limit| limit.max_angle_z),
			max_stretch: group.limit.map(|limit| limit.max_stretch),
			max_squish: group.limit.map(|limit| limit.max_squish),
			stretch_motion: group.limit.and_then(|limit| limit.stretch_motion),
			max_stretch_sample_has_positive: group
				.limit
				.is_some_and(|limit| runtime_limit_samples_have_positive(&limit.max_stretch_samples)),
			max_squish_sample_has_positive: group
				.limit
				.is_some_and(|limit| runtime_limit_samples_have_positive(&limit.max_squish_samples)),
			stretch_motion_sample_has_positive: group
				.limit
				.is_some_and(|limit| runtime_limit_samples_have_positive(&limit.stretch_motion_samples)),
			writeback_mode: group.writeback_mode,
			translation_writeback_candidate_count: scene
				.map(|scene| una_dynamics_translation_writeback_candidate_count(scene, group.writeback_mode, group.chain.bone_node_indices))
				.unwrap_or(0),
			translation_writeback_target_count: scene
				.map(|scene| una_dynamics_translation_writeback_target_count(scene, group.writeback_mode, group.chain.bone_node_indices))
				.unwrap_or(0),
			allow_grabbing: group.interaction.and_then(|interaction| interaction.allow_grabbing),
			allow_posing: group.interaction.and_then(|interaction| interaction.allow_posing),
			interaction_parameter: group
				.interaction
				.map(|interaction| interaction.parameter.clone())
				.unwrap_or_default(),
		}
	});
	match limit {
		Some(limit) => iter.take(limit).collect(),
		None => iter.collect(),
	}
}

fn runtime_limit_samples_have_positive(samples: &[f32]) -> bool {
	samples.iter().any(|value| value.is_finite() && *value > 0.0)
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
				metadata_only: interaction.parameter.is_empty(),
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
	dynamics_collider_statuses_with_limit(doc, Some(DYNAMICS_COLLIDER_STATUS_LIMIT))
}

fn dynamics_collider_statuses_with_limit(doc: &UnaDocument, limit: Option<usize>) -> Vec<RuntimeDynamicsColliderStatus> {
	let runtime_model = doc.runtime_model();
	let node_paths_by_index = runtime_model.scene().map(scene_node_paths_by_index).unwrap_or_default();
	let iter = runtime_model
		.dynamics()
		.colliders()
		.enumerate()
		.map(|(index, collider)| RuntimeDynamicsColliderStatus {
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
		});
	match limit {
		Some(limit) => iter.take(limit).collect(),
		None => iter.collect(),
	}
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

fn scene_parent_indices(scene: &UnaSceneSnapshot) -> Vec<Option<usize>> {
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

fn diagnostic_world_from_scene(scene: &UnaSceneSnapshot) -> Vec<Mat4> {
	fn visit(scene: &UnaSceneSnapshot, idx: usize, parent_world: Mat4, out: &mut [Mat4]) {
		let Some(node) = scene.nodes.get(idx) else { return };
		let world = parent_world * Mat4::from_cols_array(&node.transform);
		if let Some(slot) = out.get_mut(idx) {
			*slot = world;
		}
		for &child in &node.children {
			if child < scene.nodes.len() {
				visit(scene, child, world, out);
			}
		}
	}

	let mut out = vec![Mat4::IDENTITY; scene.nodes.len()];
	for &root in scene.resolved_roots().iter() {
		if root < scene.nodes.len() {
			visit(scene, root, Mat4::IDENTITY, &mut out);
		}
	}
	out
}

fn runtime_dynamics_node_samples(scene: &UnaSceneSnapshot, rest_nodes: Option<&[un_avatar_core::UnaSceneNode]>) -> Vec<serde_json::Value> {
	let node_paths = scene_node_paths_by_index(scene);
	let current_world = diagnostic_world_from_scene(scene);
	let mut rest_scene = scene.clone();
	if let Some(rest_nodes) = rest_nodes {
		if rest_nodes.len() == rest_scene.nodes.len() {
			rest_scene.nodes.clone_from_slice(rest_nodes);
		}
	}
	let rest_world = diagnostic_world_from_scene(&rest_scene);
	let mut out = Vec::new();
	for (node_index, path) in node_paths.iter().enumerate() {
		let Some(path) = path else {
			continue;
		};
		let (Some(current), Some(rest)) = (current_world.get(node_index), rest_world.get(node_index)) else {
			continue;
		};
		let current_translation = current.transform_point3(Vec3::ZERO);
		let rest_translation = rest.transform_point3(Vec3::ZERO);
		let delta = current_translation - rest_translation;
		let (_, current_rotation, _) = current.to_scale_rotation_translation();
		let (_, rest_rotation, _) = rest.to_scale_rotation_translation();
		let rotation_delta = current_rotation * rest_rotation.inverse();
		let (rotation_axis, rotation_angle) = rotation_axis_angle(rotation_delta);
		if delta.length() <= 1e-5 && rotation_angle.abs() <= 0.1_f32.to_radians() {
			continue;
		}
		out.push((
			delta.length().max(rotation_angle.abs() * 0.02),
			serde_json::json!({
				"node_index": node_index,
				"path": path,
				"rest_translation": rest_translation.to_array(),
				"current_translation": current_translation.to_array(),
				"rest_rotation_xyzw": [rest_rotation.x, rest_rotation.y, rest_rotation.z, rest_rotation.w],
				"current_rotation_xyzw": [current_rotation.x, current_rotation.y, current_rotation.z, current_rotation.w],
				"rest_axis_x": rest.transform_vector3(Vec3::X).to_array(),
				"rest_axis_y": rest.transform_vector3(Vec3::Y).to_array(),
				"rest_axis_z": rest.transform_vector3(Vec3::Z).to_array(),
				"current_axis_x": current.transform_vector3(Vec3::X).to_array(),
				"current_axis_y": current.transform_vector3(Vec3::Y).to_array(),
				"current_axis_z": current.transform_vector3(Vec3::Z).to_array(),
				"delta": delta.to_array(),
				"displacement": delta.length(),
				"rotation_delta_axis": rotation_axis.to_array(),
				"rotation_delta_angle_deg": rotation_angle.to_degrees(),
			}),
		));
	}
	out.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
	out.into_iter().take(96).map(|(_, value)| value).collect()
}

fn safe_diagnostic_inverse(matrix: Mat4) -> Mat4 {
	let det = matrix.determinant();
	if det.is_finite() && det.abs() > 1.0e-8 {
		matrix.inverse()
	} else {
		Mat4::IDENTITY
	}
}

fn skin_mesh_used_joints(scene: &UnaSceneSnapshot, mesh_index: usize) -> BTreeMap<usize, (f32, u32)> {
	let mut out = BTreeMap::new();
	let Some(primitives) = scene.meshes.get(mesh_index) else {
		return out;
	};
	for primitive in primitives {
		let (Some(joints), Some(weights)) = (&primitive.joints, &primitive.weights) else {
			continue;
		};
		for (joint_indices, joint_weights) in joints.iter().zip(weights.iter()) {
			for (&joint_index, &weight) in joint_indices.iter().zip(joint_weights.iter()) {
				if weight <= 1.0e-5 {
					continue;
				}
				let entry = out.entry(joint_index as usize).or_insert((0.0, 0));
				entry.0 += weight;
				entry.1 += 1;
			}
		}
	}
	out
}

fn skin_joint_samples(
	scene: &UnaSceneSnapshot,
	rest_nodes: Option<&[un_avatar_core::UnaSceneNode]>,
	dynamic_node_indices: &[usize],
) -> Vec<serde_json::Value> {
	let node_paths = scene_node_paths_by_index(scene);
	let current_world = diagnostic_world_from_scene(scene);
	let mut rest_scene = scene.clone();
	if let Some(rest_nodes) = rest_nodes {
		if rest_nodes.len() == rest_scene.nodes.len() {
			rest_scene.nodes.clone_from_slice(rest_nodes);
		}
	}
	let rest_world = diagnostic_world_from_scene(&rest_scene);
	let mut out = Vec::new();
	for (mesh_node_index, node) in scene.nodes.iter().enumerate() {
		if !node.visible {
			continue;
		}
		let Some(mesh_index) = node.mesh else {
			continue;
		};
		let Some(skin_index) = node.skin else {
			continue;
		};
		let Some(mesh_path) = node_paths.get(mesh_node_index).and_then(Option::as_deref) else {
			continue;
		};
		let Some(skin) = scene.skins.get(skin_index) else {
			continue;
		};
		let used_joints = skin_mesh_used_joints(scene, mesh_index);
		if used_joints.is_empty() {
			continue;
		}
		let current_mesh_world = current_world.get(mesh_node_index).copied().unwrap_or(Mat4::IDENTITY);
		let rest_mesh_world = rest_world.get(mesh_node_index).copied().unwrap_or(Mat4::IDENTITY);
		let current_inv_mesh = safe_diagnostic_inverse(current_mesh_world);
		let rest_inv_mesh = safe_diagnostic_inverse(rest_mesh_world);
		for (joint_index, &node_index) in skin.joint_nodes.iter().enumerate() {
			let Some(joint_path) = node_paths.get(node_index).and_then(Option::as_deref) else {
				continue;
			};
			if dynamic_node_indices.binary_search(&node_index).is_err() {
				continue;
			}
			let Some((weight_sum, weighted_vertex_count)) = used_joints.get(&joint_index).copied() else {
				continue;
			};
			let Some(current_joint_world) = current_world.get(node_index).copied() else {
				continue;
			};
			let Some(rest_joint_world) = rest_world.get(node_index).copied() else {
				continue;
			};
			let ibm = skin
				.inverse_bind_matrices
				.get(joint_index)
				.map(Mat4::from_cols_array)
				.unwrap_or(Mat4::IDENTITY);
			let current_palette = current_inv_mesh * current_joint_world * ibm;
			let rest_palette = rest_inv_mesh * rest_joint_world * ibm;
			let current_palette_cols = current_palette.to_cols_array();
			let rest_palette_cols = rest_palette.to_cols_array();
			let max_palette_abs_delta = current_palette_cols
				.iter()
				.zip(rest_palette_cols.iter())
				.map(|(current, rest)| (current - rest).abs())
				.fold(0.0_f32, f32::max);
			let current_translation = current_joint_world.transform_point3(Vec3::ZERO);
			let rest_translation = rest_joint_world.transform_point3(Vec3::ZERO);
			let delta = current_translation - rest_translation;
			let (_, current_rotation, _) = current_joint_world.to_scale_rotation_translation();
			let (_, rest_rotation, _) = rest_joint_world.to_scale_rotation_translation();
			let rotation_delta = current_rotation * rest_rotation.inverse();
			let (rotation_axis, rotation_angle) = rotation_axis_angle(rotation_delta);
			out.push(serde_json::json!({
				"mesh_node_index": mesh_node_index,
				"mesh_path": mesh_path,
				"mesh_index": mesh_index,
				"skin_index": skin_index,
				"joint_index": joint_index,
				"node_index": node_index,
				"joint_path": joint_path,
				"weight_sum": weight_sum,
				"weighted_vertex_count": weighted_vertex_count,
				"rest_translation": rest_translation.to_array(),
				"current_translation": current_translation.to_array(),
				"displacement": delta.length(),
				"rest_rotation_xyzw": [rest_rotation.x, rest_rotation.y, rest_rotation.z, rest_rotation.w],
				"current_rotation_xyzw": [current_rotation.x, current_rotation.y, current_rotation.z, current_rotation.w],
				"rotation_delta_axis": rotation_axis.to_array(),
				"rotation_delta_angle_deg": rotation_angle.to_degrees(),
				"current_palette": current_palette_cols,
				"max_palette_abs_delta_from_rest": max_palette_abs_delta,
			}));
			if out.len() >= 512 {
				return out;
			}
		}
	}
	out
}

fn visible_nonzero_morph_weights(scene: &UnaSceneSnapshot, limit: usize) -> Vec<serde_json::Value> {
	let node_paths = scene_node_paths_by_index(scene);
	let mut out = Vec::new();
	for (node_index, node) in scene.nodes.iter().enumerate() {
		if !scene.effective_node_visible(node_index) {
			continue;
		}
		let Some(mesh_index) = node.mesh else {
			continue;
		};
		let Some(primitives) = scene.meshes.get(mesh_index) else {
			continue;
		};
		let node_path = node_paths.get(node_index).and_then(Option::as_deref).unwrap_or("<unknown>");
		for (primitive_index, primitive) in primitives.iter().enumerate() {
			for (morph_index, &weight) in primitive.default_morph_weights.iter().enumerate() {
				if weight.abs() <= 1.0e-5 {
					continue;
				}
				out.push(serde_json::json!({
					"node_index": node_index,
					"node_path": node_path,
					"mesh_index": mesh_index,
					"primitive_index": primitive_index,
					"morph_index": morph_index,
					"name": primitive.morph_target_names.get(morph_index),
					"weight": weight,
				}));
				if out.len() >= limit {
					return out;
				}
			}
		}
	}
	out
}

fn rotation_axis_angle(rotation: Quat) -> (Vec3, f32) {
	let normalized = if rotation.is_finite() && rotation.length_squared() > 1.0e-12 {
		rotation.normalize()
	} else {
		Quat::IDENTITY
	};
	let angle = normalized.angle_between(Quat::IDENTITY);
	let axis = normalized.to_axis_angle().0;
	(axis, angle)
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
		control_type: modular_avatar_component_string(control, &["type", "Type", "controlType", "control_type"]),
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
				control_type: modular_avatar_component_string(control, &["type", "Type", "controlType", "control_type"]),
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
	let mut seen = Vec::new();
	let mut current_index = Some(node_index);
	while let Some(index) = current_index {
		if index >= nodes.len() {
			labels.reverse();
			return RuntimeMenuGraphNodePath { labels, truncated: true };
		}
		if seen.contains(&index) {
			labels.reverse();
			return RuntimeMenuGraphNodePath { labels, truncated: true };
		}
		seen.push(index);
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
		"ModularAvatarMenuItem"
			| "ModularAvatarMenuGroup"
			| "ModularAvatarMenuInstaller"
			| "ModularAvatarMenuInstallTarget"
			| "VRCExpressionsMenuControl"
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
	pub surface_constraints: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SceneNodeConstraintCounts {
	pub(crate) total: u32,
	pub(crate) parent: u32,
	pub(crate) parent_sources: u32,
	pub(crate) parent_multi_source: u32,
}

fn scene_node_constraint_counts(scene: &UnaSceneSnapshot) -> SceneNodeConstraintCounts {
	let mut counts = SceneNodeConstraintCounts {
		total: scene.node_constraints.len() as u32,
		..Default::default()
	};
	for constraint in &scene.node_constraints {
		if matches!(constraint.kind, UnaNodeConstraintKind::Parent { .. }) {
			counts.parent += 1;
			let source_count = if constraint.sources.is_empty() {
				1
			} else {
				constraint.sources.len()
			};
			counts.parent_sources += source_count as u32;
			if source_count > 1 {
				counts.parent_multi_source += 1;
			}
		}
	}
	counts
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
			surface_constraints: 0,
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

fn animator_dynamic_morph_target_names(doc: &UnaDocument) -> Vec<String> {
	let mut names = Vec::new();
	let Some(animator) = doc.unavatar.as_ref().and_then(|unavatar| unavatar.source.get("animator")) else {
		return names;
	};
	collect_animator_motion_morph_target_names(animator, &mut names);
	names.sort_unstable();
	names.dedup();
	names
}

fn collect_animator_motion_morph_target_names(value: &serde_json::Value, names: &mut Vec<String>) {
	match value {
		serde_json::Value::Object(map) => {
			if let Some(property) = map.get("propertyName").and_then(serde_json::Value::as_str) {
				if let Some(name) = property.strip_prefix("blendShape.").map(str::trim).filter(|name| !name.is_empty()) {
					names.push(name.to_string());
				}
			}
			for child in map.values() {
				collect_animator_motion_morph_target_names(child, names);
			}
		}
		serde_json::Value::Array(values) => {
			for child in values {
				collect_animator_motion_morph_target_names(child, names);
			}
		}
		_ => {}
	}
}

fn animator_morph_overrides_for_doc(doc: &UnaDocument) -> BTreeMap<String, f32> {
	let mut out = BTreeMap::new();
	let runtime = doc.runtime_model();
	let parameter_values = runtime.runtime_parameter_values();
	let Some(animator) = doc.unavatar.as_ref().and_then(|unavatar| unavatar.source.get("animator")) else {
		return out;
	};
	let Some(controllers) = animator.get("controllers").and_then(Value::as_array) else {
		return out;
	};
	for controller in controllers {
		if controller.get("source").and_then(Value::as_str) != Some("modularAvatarMergeAnimator") {
			continue;
		}
		let motion_base_path = controller
			.get("motionBasePath")
			.or_else(|| controller.get("motion_base_path"))
			.and_then(Value::as_str)
			.unwrap_or("");
		let parameter_defaults = animator_controller_parameter_defaults(controller);
		let Some(layers) = controller.get("layers").and_then(Value::as_array) else {
			continue;
		};
		for (layer_index, layer) in layers.iter().enumerate() {
			let layer_weight = if layer_index == 0 {
				1.0
			} else {
				layer.get("defaultWeight").and_then(Value::as_f64).unwrap_or(1.0) as f32
			};
			if layer_weight <= 0.0001 {
				continue;
			}
			let Some(states) = layer.get("states").and_then(Value::as_array) else {
				continue;
			};
			if states.len() != 1 {
				continue;
			}
			let Some(motion) = states[0].get("motion") else {
				continue;
			};
			accumulate_animator_motion_morph_overrides(
				motion,
				motion_base_path,
				parameter_values,
				&parameter_defaults,
				layer_weight,
				&mut out,
			);
		}
	}
	out
}

fn animator_controller_parameter_defaults(controller: &Value) -> BTreeMap<String, f32> {
	let mut out = BTreeMap::new();
	let Some(parameters) = controller.get("parameters").and_then(Value::as_array) else {
		return out;
	};
	for parameter in parameters {
		let Some(name) = parameter.get("name").and_then(Value::as_str).filter(|name| !name.is_empty()) else {
			continue;
		};
		let value = parameter
			.get("defaultFloat")
			.or_else(|| parameter.get("default_float"))
			.and_then(Value::as_f64)
			.map(|value| value as f32)
			.unwrap_or_else(|| {
				parameter
					.get("defaultInt")
					.or_else(|| parameter.get("default_int"))
					.and_then(Value::as_i64)
					.map(|value| value as f32)
					.unwrap_or(0.0)
			});
		out.insert(name.to_string(), value);
	}
	out
}

fn animator_parameter_value(name: &str, parameter_values: &BTreeMap<String, f32>, parameter_defaults: &BTreeMap<String, f32>) -> f32 {
	parameter_values
		.get(name)
		.or_else(|| parameter_defaults.get(name))
		.copied()
		.unwrap_or(0.0)
}

fn accumulate_animator_motion_morph_overrides(
	motion: &Value,
	motion_base_path: &str,
	parameter_values: &BTreeMap<String, f32>,
	parameter_defaults: &BTreeMap<String, f32>,
	weight: f32,
	out: &mut BTreeMap<String, f32>,
) {
	if weight <= 0.0001 {
		return;
	}
	match motion.get("motionType").and_then(Value::as_str) {
		Some("AnimationClip") => {
			let Some(bindings) = motion.get("curveBindings").and_then(Value::as_array) else {
				return;
			};
			for binding in bindings {
				let Some(property) = binding.get("propertyName").and_then(Value::as_str) else {
					continue;
				};
				let Some(name) = property.strip_prefix("blendShape.").map(str::trim).filter(|name| !name.is_empty()) else {
					continue;
				};
				let Some(value) = animator_curve_binding_value(binding) else {
					continue;
				};
				let binding_path = binding.get("path").and_then(Value::as_str).unwrap_or("");
				let target_path = animator_resolve_binding_path(motion_base_path, binding_path);
				let key = if target_path.is_empty() {
					name.to_string()
				} else {
					format!("{target_path}\0{name}")
				};
				let normalized = if value > 1.0 { value / 100.0 } else { value };
				let entry = out.entry(key).or_insert(0.0);
				*entry = (*entry + normalized * weight).clamp(0.0, 1.0);
			}
		}
		Some("BlendTree") => {
			let blend_type = motion.get("blendType").and_then(Value::as_str).unwrap_or("");
			if blend_type != "Simple1D" && blend_type != "1D" {
				return;
			}
			let parameter = motion.get("blendParameter").and_then(Value::as_str).unwrap_or("");
			let Some(children) = motion.get("children").and_then(Value::as_array) else {
				return;
			};
			let value = animator_parameter_value(parameter, parameter_values, parameter_defaults);
			let sorted_thresholds = simple_1d_blend_child_thresholds(children);
			for (child_index, child) in children.iter().enumerate() {
				let child_weight = simple_1d_blend_child_weight(&sorted_thresholds, child_index, value);
				if child_weight > 0.0001 {
					accumulate_animator_motion_morph_overrides(
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

fn animator_curve_binding_value(binding: &Value) -> Option<f32> {
	binding
		.get("constantValue")
		.or_else(|| binding.get("constant_value"))
		.or_else(|| binding.get("lastValue"))
		.or_else(|| binding.get("last_value"))
		.or_else(|| binding.get("firstValue"))
		.or_else(|| binding.get("first_value"))
		.and_then(Value::as_f64)
		.map(|value| value as f32)
}

fn animator_resolve_binding_path(motion_base_path: &str, binding_path: &str) -> String {
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

fn simple_1d_blend_child_thresholds(children: &[Value]) -> Vec<(usize, f32)> {
	let threshold = |child: &Value| {
		child
			.get("threshold")
			.and_then(Value::as_f64)
			.map(|value| value as f32)
			.unwrap_or(0.0)
	};
	let mut sorted = children
		.iter()
		.enumerate()
		.map(|(child_index, child)| (child_index, threshold(child)))
		.collect::<Vec<_>>();
	sorted.sort_by(|left, right| left.1.partial_cmp(&right.1).unwrap_or(std::cmp::Ordering::Equal));
	sorted
}

fn simple_1d_blend_child_weight(sorted: &[(usize, f32)], index: usize, value: f32) -> f32 {
	if sorted.is_empty() {
		return 0.0;
	}
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

#[cfg(test)]
fn dynamics_interaction_parameter_values_with_context(
	doc: &UnaDocument,
	rest_nodes: Option<&[UnaSceneNode]>,
	node_paths_by_index: &[Option<String>],
	center_peak_angle_parameters: &[String],
) -> BTreeMap<String, f32> {
	dynamics_interaction_parameter_updates_with_context(doc, rest_nodes, node_paths_by_index, center_peak_angle_parameters, None).values
}

struct DynamicsInteractionParameterUpdates {
	values: BTreeMap<String, f32>,
	changed: BTreeMap<String, f32>,
}

fn dynamics_interaction_parameter_updates_with_context(
	doc: &UnaDocument,
	rest_nodes: Option<&[UnaSceneNode]>,
	node_paths_by_index: &[Option<String>],
	center_peak_angle_parameters: &[String],
	before: Option<&BTreeMap<String, f32>>,
) -> DynamicsInteractionParameterUpdates {
	let mut values = BTreeMap::new();
	let mut changed = BTreeMap::new();
	let runtime = doc.runtime_model();
	let Some(scene) = runtime.scene() else {
		return DynamicsInteractionParameterUpdates { values, changed };
	};
	let dynamics = runtime.dynamics();
	let active_dynamics_source_ids = active_dynamics_source_ids_for_scene(doc, scene);
	let mut world = None;
	for group in dynamics.dynamics_groups() {
		if !group.effective_enabled || !dynamics_source_id_resident(group.source_id, active_dynamics_source_ids.as_ref()) {
			continue;
		}
		let Some(interaction) = group.interaction else {
			continue;
		};
		if interaction.parameter.is_empty() {
			continue;
		}
		let shape_angle = dynamics_group_shape_angle(rest_nodes, &scene.nodes, group, &node_paths_by_index).unwrap_or(0.0);
		let world = world.get_or_insert_with(|| crate::scene_transform::scene_world_matrices(scene));
		let gravity_angle = dynamics_group_gravity_sensor_angle(rest_nodes, world, group, &node_paths_by_index).unwrap_or(0.0);
		let angle = shape_angle.max(gravity_angle);
		let max_angle = dynamics_interaction_angle_normalizer(group.limit);
		let angle_parameter = dynamics_interaction_parameter_name(&interaction.parameter, "_Angle");
		let angle_norm = (angle.to_degrees() / max_angle).clamp(0.0, 1.0);
		let angle_value = if center_peak_angle_parameters
			.binary_search_by(|parameter| parameter.as_str().cmp(angle_parameter.as_str()))
			.is_ok()
		{
			(angle_norm * 0.5).clamp(0.0, 1.0)
		} else {
			angle_norm
		};
		insert_dynamics_interaction_parameter_value(&mut values, &mut changed, before, angle_parameter, angle_value);
		insert_dynamics_interaction_parameter_value(
			&mut values,
			&mut changed,
			before,
			dynamics_interaction_parameter_name(&interaction.parameter, "_IsGrabbed"),
			0.0,
		);
		insert_dynamics_interaction_parameter_value(
			&mut values,
			&mut changed,
			before,
			dynamics_interaction_parameter_name(&interaction.parameter, "_IsPosed"),
			0.0,
		);
		insert_dynamics_interaction_parameter_value(
			&mut values,
			&mut changed,
			before,
			dynamics_interaction_parameter_name(&interaction.parameter, "_Stretch"),
			0.0,
		);
		insert_dynamics_interaction_parameter_value(
			&mut values,
			&mut changed,
			before,
			dynamics_interaction_parameter_name(&interaction.parameter, "_Squish"),
			0.0,
		);
	}
	DynamicsInteractionParameterUpdates { values, changed }
}

fn dynamics_interaction_parameter_name(base: &str, suffix: &str) -> String {
	let mut name = String::with_capacity(base.len() + suffix.len());
	name.push_str(base);
	name.push_str(suffix);
	name
}

fn insert_dynamics_interaction_parameter_value(
	values: &mut BTreeMap<String, f32>,
	changed: &mut BTreeMap<String, f32>,
	before: Option<&BTreeMap<String, f32>>,
	name: String,
	value: f32,
) {
	if let Some(before) = before {
		if (before.get(&name).copied().unwrap_or(f32::NAN) - value).abs() > 0.0001 {
			changed.insert(name.clone(), value);
		} else {
			changed.remove(&name);
		}
	}
	values.insert(name, value);
}

#[cfg(test)]
fn dynamics_interaction_parameter_values(doc: &UnaDocument, rest_nodes: Option<&[UnaSceneNode]>) -> BTreeMap<String, f32> {
	let runtime = doc.runtime_model();
	let Some(scene) = runtime.scene() else {
		return BTreeMap::new();
	};
	let node_paths_by_index = scene_node_paths_by_index(scene);
	let center_peak_angle_parameters = animator_center_peak_angle_parameters(doc);
	dynamics_interaction_parameter_values_with_context(doc, rest_nodes, &node_paths_by_index, &center_peak_angle_parameters)
}

fn animator_center_peak_angle_parameters(doc: &UnaDocument) -> Vec<String> {
	let mut out = Vec::new();
	let Some(animator) = doc.unavatar.as_ref().and_then(|unavatar| unavatar.source.get("animator")) else {
		return out;
	};
	collect_center_peak_angle_parameters(animator, &mut out);
	out.sort_unstable();
	out.dedup();
	out
}

fn collect_center_peak_angle_parameters(value: &Value, out: &mut Vec<String>) {
	if let Some(motion_type) = value.get("motionType").and_then(Value::as_str) {
		if motion_type == "BlendTree" {
			let blend_type = value.get("blendType").and_then(Value::as_str).unwrap_or("");
			if (blend_type == "Simple1D" || blend_type == "1D")
				&& value
					.get("blendParameter")
					.and_then(Value::as_str)
					.is_some_and(|parameter| parameter.ends_with("_Angle") && blend_tree_has_center_peak_thresholds(value))
			{
				if let Some(parameter) = value.get("blendParameter").and_then(Value::as_str) {
					out.push(parameter.to_string());
				}
			}
		}
	}
	if let Some(children) = value.get("children").and_then(Value::as_array) {
		for child in children {
			collect_center_peak_angle_parameters(child, out);
		}
	}
	if let Some(controllers) = value.get("controllers").and_then(Value::as_array) {
		for controller in controllers {
			collect_center_peak_angle_parameters(controller, out);
		}
	}
	if let Some(layers) = value.get("layers").and_then(Value::as_array) {
		for layer in layers {
			collect_center_peak_angle_parameters(layer, out);
		}
	}
	if let Some(states) = value.get("states").and_then(Value::as_array) {
		for state in states {
			collect_center_peak_angle_parameters(state, out);
		}
	}
	if let Some(motion) = value.get("motion") {
		collect_center_peak_angle_parameters(motion, out);
	}
}

fn blend_tree_has_center_peak_thresholds(value: &Value) -> bool {
	let Some(children) = value.get("children").and_then(Value::as_array) else {
		return false;
	};
	let mut has_low = false;
	let mut has_center = false;
	let mut has_high = false;
	for child in children {
		let Some(threshold) = child.get("threshold").and_then(Value::as_f64) else {
			continue;
		};
		has_low |= (threshold - 0.0).abs() <= 0.001;
		has_center |= (threshold - 0.5).abs() <= 0.001;
		has_high |= (threshold - 1.0).abs() <= 0.001;
	}
	has_low && has_center && has_high
}

fn dynamics_interaction_parameter_diagnostics(doc: &UnaDocument, rest_nodes: Option<&[UnaSceneNode]>) -> Vec<Value> {
	let runtime = doc.runtime_model();
	let Some(scene) = runtime.scene() else {
		return Vec::new();
	};
	let dynamics = runtime.dynamics();
	let active_dynamics_source_ids = active_dynamics_source_ids_for_scene(doc, scene);
	let node_paths_by_index = scene_node_paths_by_index(scene);
	let world = crate::scene_transform::scene_world_matrices(scene);
	let mut out = Vec::new();
	for group in dynamics.dynamics_groups() {
		if !group.effective_enabled || !dynamics_source_id_resident(group.source_id, active_dynamics_source_ids.as_ref()) {
			continue;
		}
		let Some(interaction) = group.interaction else {
			continue;
		};
		if interaction.parameter.is_empty() {
			continue;
		}
		let shape_angle = dynamics_group_shape_angle(rest_nodes, &scene.nodes, group, &node_paths_by_index).unwrap_or(0.0);
		let gravity_angle = dynamics_group_gravity_sensor_angle(rest_nodes, &world, group, &node_paths_by_index).unwrap_or(0.0);
		let angle = shape_angle.max(gravity_angle);
		let max_angle = dynamics_interaction_angle_normalizer(group.limit);
		let (limit_type, limit_max_angle_x, limit_max_angle_z) = group
			.limit
			.map(|limit| {
				(
					(!limit.limit_type.is_empty()).then(|| limit.limit_type.clone()),
					Some(limit.max_angle_x),
					Some(limit.max_angle_z),
				)
			})
			.unwrap_or((None, None, None));
		let angle_norm = (angle.to_degrees() / max_angle).clamp(0.0, 1.0);
		let chain = dynamics_interaction_chain(group, &node_paths_by_index)
			.iter()
			.filter_map(|node| node_paths_by_index.get(*node).and_then(|path| path.clone()))
			.collect::<Vec<_>>();
		out.push(serde_json::json!({
			"parameter": interaction.parameter,
			"angle_parameter": format!("{}_Angle", interaction.parameter),
			"source_id": group.source_id,
			"source_kind": format!("{:?}", group.source_kind),
			"category": format!("{:?}", group.category),
			"angle_norm": angle_norm,
			"angle_deg": angle.to_degrees(),
			"shape_angle_deg": shape_angle.to_degrees(),
			"gravity_angle_deg": gravity_angle.to_degrees(),
			"dominant": if gravity_angle > shape_angle { "gravity" } else { "shape" },
			"max_angle_deg": max_angle,
			"limit_type": limit_type,
			"limit_max_angle_x_deg": limit_max_angle_x,
			"limit_max_angle_z_deg": limit_max_angle_z,
			"gravity_power": group.parameters.gravity_power,
			"gravity_dir": group.parameters.gravity_dir,
			"chain": chain,
		}));
	}
	out
}

struct ActiveDynamicsSourceIds {
	owned: Vec<String>,
	active: Vec<String>,
}

fn active_dynamics_source_ids_for_scene(doc: &UnaDocument, scene: &UnaSceneSnapshot) -> Option<ActiveDynamicsSourceIds> {
	let active_groups = doc.runtime_state.active_asset_groups.as_slice();
	if active_groups.is_empty() || scene.asset_group_ownership.is_empty() {
		return None;
	}
	let mut owned = Vec::new();
	let mut active = Vec::new();
	for group in &scene.asset_group_ownership {
		let group_active = active_groups.iter().any(|active_group| active_group == &group.group_id);
		for source_id in &group.dynamics_source_ids {
			if source_id.is_empty() {
				continue;
			}
			owned.push(source_id.clone());
			if group_active {
				active.push(source_id.clone());
			}
		}
	}
	if owned.is_empty() {
		return None;
	}
	owned.sort_unstable();
	owned.dedup();
	active.sort_unstable();
	active.dedup();
	Some(ActiveDynamicsSourceIds { owned, active })
}

fn sorted_strings_contains(values: &[String], needle: &str) -> bool {
	values.binary_search_by(|value| value.as_str().cmp(needle)).is_ok()
}

fn sorted_unique_index_union(a: &[usize], b: &[usize]) -> Vec<usize> {
	let mut out = Vec::with_capacity(a.len() + b.len());
	let mut ai = 0;
	let mut bi = 0;
	while ai < a.len() && bi < b.len() {
		match a[ai].cmp(&b[bi]) {
			std::cmp::Ordering::Less => {
				out.push(a[ai]);
				ai += 1;
			}
			std::cmp::Ordering::Equal => {
				out.push(a[ai]);
				ai += 1;
				bi += 1;
			}
			std::cmp::Ordering::Greater => {
				out.push(b[bi]);
				bi += 1;
			}
		}
	}
	out.extend_from_slice(&a[ai..]);
	out.extend_from_slice(&b[bi..]);
	out
}

fn sorted_index_difference(indices: &[usize], excluded: &[usize]) -> Vec<usize> {
	let mut out = Vec::with_capacity(indices.len());
	let mut ei = 0;
	for &index in indices {
		while ei < excluded.len() && excluded[ei] < index {
			ei += 1;
		}
		if excluded.get(ei).copied() != Some(index) {
			out.push(index);
		}
	}
	out
}

fn dynamics_source_id_resident(source_id: &str, source_ids: Option<&ActiveDynamicsSourceIds>) -> bool {
	let Some(source_ids) = source_ids else {
		return true;
	};
	source_id.is_empty() || !sorted_strings_contains(&source_ids.owned, source_id) || sorted_strings_contains(&source_ids.active, source_id)
}

fn dynamics_group_shape_angle(
	rest_nodes: Option<&[UnaSceneNode]>,
	current_nodes: &[UnaSceneNode],
	group: un_avatar_core::UnaDynamicsGroup<'_>,
	node_paths_by_index: &[Option<String>],
) -> Option<f32> {
	let chain = dynamics_interaction_chain(group, node_paths_by_index);
	if chain.len() < 2 {
		return Some(0.0);
	}
	let mut max_angle = 0.0_f32;
	let mut measured = false;
	let rest_nodes = rest_nodes.unwrap_or(current_nodes);
	for segment in chain.windows(2) {
		let root = segment[0];
		let tip = segment[1];
		if root == tip {
			continue;
		}
		let Some(rest_tip) = rest_nodes.get(tip) else {
			continue;
		};
		let Some(current_tip) = current_nodes.get(tip) else {
			continue;
		};
		let rest_local = Mat4::from_cols_array(&rest_tip.transform);
		let current_local = Mat4::from_cols_array(&current_tip.transform);
		let (_, rest_rotation, rest_translation) = rest_local.to_scale_rotation_translation();
		let (_, current_rotation, current_translation) = current_local.to_scale_rotation_translation();
		if rest_rotation.length_squared() > 1e-12 && current_rotation.length_squared() > 1e-12 {
			max_angle = max_angle.max(rest_rotation.normalize().angle_between(current_rotation.normalize()));
			measured = true;
		}
		if let (Some(rest_dir), Some(current_dir)) = (rest_translation.try_normalize(), current_translation.try_normalize()) {
			max_angle = max_angle.max(rest_dir.angle_between(current_dir));
			measured = true;
		}
	}
	measured.then_some(max_angle)
}

fn dynamics_interaction_angle_normalizer(limit: Option<&un_avatar_core::UnaDynamicsLimit>) -> f32 {
	let Some(limit) = limit else {
		return 90.0;
	};
	let x = limit.max_angle_x.max(0.0);
	let z = limit.max_angle_z.max(0.0);
	let positive_min = match (x > 0.0, z > 0.0) {
		(true, true) => x.min(z),
		(true, false) => x,
		(false, true) => z,
		(false, false) => 90.0,
	};
	let limit_type = limit.limit_type.to_ascii_lowercase();
	if limit_type.contains("hinge") {
		positive_min.max(1.0)
	} else {
		x.max(z).max(1.0)
	}
}

fn dynamics_interaction_chain<'a>(group: un_avatar_core::UnaDynamicsGroup<'a>, node_paths_by_index: &[Option<String>]) -> &'a [usize] {
	let chain = group.chain.bone_node_indices;
	let start = group.chain.interaction_start_index.min(chain.len());
	if start == 0 && legacy_interaction_chain_has_prepended_anchor(group, node_paths_by_index) {
		return &chain[1..];
	}
	&chain[start..]
}

fn legacy_interaction_chain_has_prepended_anchor(
	group: un_avatar_core::UnaDynamicsGroup<'_>,
	node_paths_by_index: &[Option<String>],
) -> bool {
	if group.interaction.is_none() {
		return false;
	}
	let chain = group.chain.bone_node_indices;
	if chain.len() < 3 {
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
	let Some(authored_root_path) = chain
		.get(1)
		.and_then(|node| node_paths_by_index.get(*node))
		.and_then(|path| path.as_deref())
	else {
		return false;
	};
	authored_root_path == source_path || authored_root_path.ends_with(&format!("/{source_path}"))
}

fn dynamics_group_gravity_sensor_angle(
	rest_nodes: Option<&[UnaSceneNode]>,
	world: &[Mat4],
	group: un_avatar_core::UnaDynamicsGroup<'_>,
	node_paths_by_index: &[Option<String>],
) -> Option<f32> {
	if group.parameters.gravity_power.abs() <= f32::EPSILON {
		return Some(0.0);
	}
	let gravity_dir = Vec3::from_array(group.parameters.gravity_dir)
		.try_normalize()
		.unwrap_or(Vec3::NEG_Y);
	let rest_nodes = rest_nodes?;
	let chain = dynamics_interaction_chain(group, node_paths_by_index);
	if chain.len() < 2 {
		return Some(0.0);
	}
	let mut max_angle = 0.0_f32;
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
		let rest_child_local = Mat4::from_cols_array(&rest_child.transform);
		let (_, _, rest_child_translation) = rest_child_local.to_scale_rotation_translation();
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

fn humanoid_node_index(profile: &un_avatar_skeleton::HumanoidProfile, keys: &[&str]) -> Option<usize> {
	for key in keys {
		if let Some(index) = profile.bone_node_indices.get(*key).copied() {
			return Some(index);
		}
		let target = normalize_profile_match_key(key);
		if let Some((_, index)) = profile
			.bone_node_indices
			.iter()
			.find(|(candidate, _)| normalize_profile_match_key(candidate) == target)
		{
			return Some(*index);
		}
	}
	None
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

fn runtime_token_filter_matches(value: &str, needles: &[String]) -> bool {
	needles.iter().any(|needle| dynamics_normalized_token_filter_matches(value, needle))
}

fn runtime_physics_source_scope_key(value: &str) -> String {
	let value = value.strip_prefix("physbone:").unwrap_or(value);
	let lower = value.to_ascii_lowercase();
	if let Some(index) = lower.find("/pb/") {
		return normalize_profile_match_key(&value[..index]);
	}
	if let Some(index) = value.find('/') {
		return normalize_profile_match_key(&value[..index]);
	}
	normalize_profile_match_key(value)
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
struct StartupProgressOverlayGpu {
	time: f32,
	progress: f32,
	aspect: f32,
	phase: f32,
	rect_center: [f32; 2],
	rect_half_size: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct WardrobeBillboardGpu {
	view_proj: [[f32; 4]; 4],
	camera_pos: [f32; 4],
	center_size: [f32; 4],
	time_params: [f32; 4],
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

pub(crate) struct StartupProgressOverlayFrame {
	pub(crate) time_secs: f32,
	pub(crate) progress: f32,
	pub(crate) phase: f32,
	pub(crate) rect_center: [f32; 2],
	pub(crate) rect_half_size: [f32; 2],
}

pub(crate) struct WardrobeChangingBillboardFrame {
	pub(crate) time_secs: f32,
	pub(crate) billboard_center: [f32; 3],
	pub(crate) billboard_size: f32,
	pub(crate) billboard_view_proj: [[f32; 4]; 4],
	pub(crate) billboard_camera_pos: [f32; 3],
}

pub(crate) struct RendererStartupPresentation {
	pub(crate) progress_overlay: Option<StartupProgressOverlayFrame>,
}

pub(crate) struct WardrobeTransitionPresentation {
	pub(crate) changing_billboard: WardrobeChangingBillboardFrame,
}

pub(crate) enum RenderedFrameRole {
	/// Normal avatar frames. These are runtime output and may be sent to Spout2.
	RuntimeAvatar,
	/// Renderer-local startup / load / failure presentation. This is never sent to Spout2.
	RendererStartup(RendererStartupPresentation),
	/// OBS-facing wardrobe transition frame. This is runtime output and may be sent to Spout2.
	WardrobeTransition(WardrobeTransitionPresentation),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Spout2FrameDelivery {
	RuntimeOutput,
	SuppressedRendererStartup,
	Unavailable,
}

impl RenderedFrameRole {
	fn spout2_delivery(&self, spout_available: bool) -> Spout2FrameDelivery {
		if !spout_available {
			return Spout2FrameDelivery::Unavailable;
		}
		match self {
			Self::RuntimeAvatar | Self::WardrobeTransition(_) => Spout2FrameDelivery::RuntimeOutput,
			Self::RendererStartup(_) => Spout2FrameDelivery::SuppressedRendererStartup,
		}
	}

	fn wardrobe_transition_billboard(&self) -> Option<&WardrobeChangingBillboardFrame> {
		match self {
			Self::WardrobeTransition(presentation) => Some(&presentation.changing_billboard),
			Self::RuntimeAvatar | Self::RendererStartup(_) => None,
		}
	}

	fn startup_overlay(&self) -> Option<&StartupProgressOverlayFrame> {
		match self {
			Self::RendererStartup(presentation) => presentation.progress_overlay.as_ref(),
			Self::RuntimeAvatar | Self::WardrobeTransition(_) => None,
		}
	}

	fn is_wardrobe_transition_only(&self) -> bool {
		matches!(self, Self::WardrobeTransition(_))
	}
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
	pub(crate) dynamics_enabled: bool,
	pub(crate) bone_colliders: BoneColliderConfig,
	pub(crate) dynamics_physics: DynamicsPhysicsConfig,
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
	expression_presets: Box<[String]>,
	timings: PreparedDocumentSceneTimings,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PreparedDocumentSceneTimings {
	pub(crate) total: Duration,
	pub(crate) document_unwrap: Duration,
	pub(crate) physics: Duration,
	pub(crate) rest_nodes: Duration,
	pub(crate) expressions: Duration,
	pub(crate) mesh_build: Duration,
	pub(crate) initial_draw_state: Duration,
	pub(crate) pipeline_cache_store: Duration,
}

impl PreparedDocumentSceneTimings {
	fn log_slow(self) {
		log_slow_gpu_scene_context_step("prepare document total", self.total);
		log_slow_gpu_scene_context_step("prepare document unwrap", self.document_unwrap);
		log_slow_gpu_scene_context_step("prepare physics", self.physics);
		log_slow_gpu_scene_context_step("prepare rest nodes", self.rest_nodes);
		log_slow_gpu_scene_context_step("prepare expression presets", self.expressions);
		log_slow_gpu_scene_context_step("prepare mesh build", self.mesh_build);
		log_slow_gpu_scene_context_step("prepare initial draw state", self.initial_draw_state);
		log_slow_gpu_scene_context_step("prepare pipeline cache store", self.pipeline_cache_store);
	}
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

#[derive(Clone, Debug)]
struct SurfaceConstraintNode {
	group_index: usize,
	rest_tail: Vec3,
}

#[derive(Clone, Copy, Debug, Default)]
struct SurfaceConstraintPairStats {
	edge_count: usize,
	edge_distance_sum: f32,
	stiffness: f32,
}

#[derive(Clone, Copy, Debug)]
struct SurfaceConstraintVertex {
	vertex_index: usize,
	node_index: usize,
	pos: Vec3,
}

fn build_dynamics_surface_constraints(
	scene: &UnaSceneSnapshot,
	dynamics: UnaRuntimeDynamics<'_>,
	physics: &DynamicsPhysicsConfig,
) -> Vec<DynamicsSurfaceConstraint> {
	let world = scene_world_matrices(scene);
	let surface_nodes = dynamics_surface_constraint_nodes(scene, dynamics, &world, &physics.categories);
	if surface_nodes.is_empty() {
		return Vec::new();
	}
	let topology_enabled = physics.surface_constraint_topology_stiffness > 0.0
		&& physics.surface_constraint_topology_max_edge_distance_m > 0.0
		&& physics.surface_constraint_topology_max_mean_edge_distance_m > 0.0;
	let spatial_enabled = physics.surface_constraint_spatial_stiffness > 0.0 && physics.surface_constraint_spatial_max_distance_m > 0.0;
	if !topology_enabled && !spatial_enabled {
		return Vec::new();
	}
	let mut pair_stats = BTreeMap::<(usize, usize), SurfaceConstraintPairStats>::new();
	for node in &scene.nodes {
		let (Some(mesh_index), Some(skin_index)) = (node.mesh, node.skin) else {
			continue;
		};
		let (Some(primitives), Some(skin)) = (scene.meshes.get(mesh_index), scene.skins.get(skin_index)) else {
			continue;
		};
		for primitive in primitives {
			let (Some(joints), Some(weights), Some(indices)) = (&primitive.joints, &primitive.weights, &primitive.indices) else {
				continue;
			};
			if primitive.positions.is_empty() || joints.len() != weights.len() {
				continue;
			}
			let dominant_nodes = joints
				.iter()
				.zip(weights.iter())
				.map(|(joint_indices, joint_weights)| dominant_surface_constraint_node(joint_indices, joint_weights, skin, &surface_nodes))
				.collect::<Vec<_>>();
			if spatial_enabled {
				accumulate_spatial_surface_seams(
					&primitive.positions,
					&dominant_nodes,
					&surface_nodes,
					&mut pair_stats,
					physics.surface_constraint_spatial_max_distance_m,
					physics.surface_constraint_spatial_stiffness,
				);
			}
			if topology_enabled {
				for triangle in indices.chunks_exact(3) {
					let tri = [triangle[0] as usize, triangle[1] as usize, triangle[2] as usize];
					for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
						let Some(Some(node_a)) = dominant_nodes.get(a) else {
							continue;
						};
						let Some(Some(node_b)) = dominant_nodes.get(b) else {
							continue;
						};
						if node_a == node_b {
							continue;
						}
						let (Some(a_info), Some(b_info)) = (surface_nodes.get(node_a), surface_nodes.get(node_b)) else {
							continue;
						};
						if a_info.group_index == b_info.group_index {
							continue;
						}
						let (Some(pos_a), Some(pos_b)) = (primitive.positions.get(a), primitive.positions.get(b)) else {
							continue;
						};
						let edge_distance = Vec3::from_array(*pos_a).distance(Vec3::from_array(*pos_b));
						if !edge_distance.is_finite() || edge_distance > physics.surface_constraint_topology_max_edge_distance_m {
							continue;
						}
						let key = if node_a < node_b { (*node_a, *node_b) } else { (*node_b, *node_a) };
						let stats = pair_stats.entry(key).or_default();
						stats.edge_count += 1;
						stats.edge_distance_sum += edge_distance;
						stats.stiffness = stats.stiffness.max(physics.surface_constraint_topology_stiffness);
					}
				}
			}
		}
	}
	pair_stats
		.into_iter()
		.filter_map(|((node_a, node_b), stats)| {
			if stats.edge_count < physics.surface_constraint_min_edge_count as usize {
				return None;
			}
			let mean_edge_distance = stats.edge_distance_sum / stats.edge_count as f32;
			if !mean_edge_distance.is_finite() || mean_edge_distance > physics.surface_constraint_topology_max_mean_edge_distance_m {
				return None;
			}
			let rest_distance = surface_nodes
				.get(&node_a)?
				.rest_tail
				.distance(surface_nodes.get(&node_b)?.rest_tail);
			if !rest_distance.is_finite() || rest_distance <= 1e-5 {
				return None;
			}
			Some(DynamicsSurfaceConstraint {
				node_a,
				node_b,
				rest_distance,
				stiffness: stats.stiffness.max(0.35),
			})
		})
		.collect()
}

fn accumulate_spatial_surface_seams(
	positions: &[[f32; 3]],
	dominant_nodes: &[Option<usize>],
	surface_nodes: &BTreeMap<usize, SurfaceConstraintNode>,
	pair_stats: &mut BTreeMap<(usize, usize), SurfaceConstraintPairStats>,
	max_distance_m: f32,
	stiffness: f32,
) {
	const SEAM_CELL_M: f32 = 0.012;
	if positions.is_empty() || dominant_nodes.is_empty() || max_distance_m <= 0.0 || stiffness <= 0.0 {
		return;
	}
	let mut vertices = Vec::new();
	for (vertex_index, node_index) in dominant_nodes.iter().enumerate() {
		let Some(node_index) = *node_index else {
			continue;
		};
		if vertex_index >= positions.len() {
			continue;
		}
		if !surface_nodes.contains_key(&node_index) {
			continue;
		}
		let pos = Vec3::from_array(positions[vertex_index]);
		if !pos.is_finite() {
			continue;
		}
		vertices.push(SurfaceConstraintVertex {
			vertex_index,
			node_index,
			pos,
		});
	}
	if vertices.len() < 2 {
		return;
	}
	let mut grid = HashMap::<(i32, i32, i32), Vec<usize>>::new();
	for (local_index, vertex) in vertices.iter().enumerate() {
		grid.entry(surface_seam_cell(vertex.pos, SEAM_CELL_M))
			.or_default()
			.push(local_index);
	}
	for (local_index, vertex) in vertices.iter().enumerate() {
		let cell = surface_seam_cell(vertex.pos, SEAM_CELL_M);
		for dx in -1..=1 {
			for dy in -1..=1 {
				for dz in -1..=1 {
					let key = (cell.0 + dx, cell.1 + dy, cell.2 + dz);
					let Some(candidates) = grid.get(&key) else {
						continue;
					};
					for &other_index in candidates {
						if other_index <= local_index {
							continue;
						}
						let other = vertices[other_index];
						if other.vertex_index == vertex.vertex_index || other.node_index == vertex.node_index {
							continue;
						}
						let (Some(a_info), Some(b_info)) = (surface_nodes.get(&vertex.node_index), surface_nodes.get(&other.node_index))
						else {
							continue;
						};
						if a_info.group_index == b_info.group_index {
							continue;
						}
						let distance = vertex.pos.distance(other.pos);
						if !distance.is_finite() || distance > max_distance_m {
							continue;
						}
						let key = if vertex.node_index < other.node_index {
							(vertex.node_index, other.node_index)
						} else {
							(other.node_index, vertex.node_index)
						};
						let stats = pair_stats.entry(key).or_default();
						stats.edge_count += 1;
						stats.edge_distance_sum += distance;
						stats.stiffness = stats.stiffness.max(stiffness);
					}
				}
			}
		}
	}
}

fn surface_seam_cell(pos: Vec3, cell_size: f32) -> (i32, i32, i32) {
	(
		(pos.x / cell_size).floor() as i32,
		(pos.y / cell_size).floor() as i32,
		(pos.z / cell_size).floor() as i32,
	)
}

fn dynamics_surface_constraint_nodes(
	scene: &UnaSceneSnapshot,
	dynamics: UnaRuntimeDynamics<'_>,
	world: &[Mat4],
	categories: &[un_avatar_skeleton::DynamicsCategoryDefinition],
) -> BTreeMap<usize, SurfaceConstraintNode> {
	let mut nodes = BTreeMap::new();
	for (group_index, group) in dynamics.dynamics_groups().enumerate() {
		if !group.effective_enabled || !dynamics.source_id_resident_in_scene(scene, group.source_id) {
			continue;
		}
		if classify_dynamics_group_category(scene, group, categories) != "cloth" {
			continue;
		}
		let chain = group.chain.bone_node_indices;
		for chain_index in 1..chain.len() {
			let child = chain[chain_index];
			if child >= scene.nodes.len() || child >= world.len() {
				continue;
			}
			let rest_tail = dynamics_chain_rest_tail(scene, world, chain, chain_index);
			if rest_tail.is_finite() {
				nodes.entry(child).or_insert(SurfaceConstraintNode { group_index, rest_tail });
			}
		}
	}
	nodes
}

fn dynamics_chain_rest_tail(scene: &UnaSceneSnapshot, world: &[Mat4], chain: &[usize], child_chain_index: usize) -> Vec3 {
	let child = chain[child_chain_index];
	let child_world = world[child];
	let local_tail_translation = if child_chain_index + 1 < chain.len() {
		let next = chain[child_chain_index + 1];
		scene
			.nodes
			.get(next)
			.map(|node| Mat4::from_cols_array(&node.transform).to_scale_rotation_translation().2)
			.unwrap_or(Vec3::Y)
	} else {
		scene
			.nodes
			.get(child)
			.map(|node| Mat4::from_cols_array(&node.transform).to_scale_rotation_translation().2)
			.unwrap_or(Vec3::Y)
	};
	let length = local_tail_translation.length().max(1e-4);
	let axis = local_tail_translation.normalize_or_zero();
	let axis = if axis.length_squared() > 1e-12 { axis } else { Vec3::Y };
	child_world.transform_point3(Vec3::ZERO) + child_world.transform_vector3(axis) * length
}

fn dominant_surface_constraint_node(
	joint_indices: &[u16; 4],
	joint_weights: &[f32; 4],
	skin: &un_avatar_core::UnaSkin,
	surface_nodes: &BTreeMap<usize, SurfaceConstraintNode>,
) -> Option<usize> {
	let mut best = None;
	let mut best_weight = 0.0;
	for lane in 0..4 {
		let weight = joint_weights[lane];
		if !weight.is_finite() || weight < 0.25 || weight <= best_weight {
			continue;
		}
		let joint_index = joint_indices[lane] as usize;
		let Some(&node_index) = skin.joint_nodes.get(joint_index) else {
			continue;
		};
		if !surface_nodes.contains_key(&node_index) {
			continue;
		}
		best = Some(node_index);
		best_weight = weight;
	}
	best
}

fn scene_world_matrices(scene: &UnaSceneSnapshot) -> Vec<Mat4> {
	let mut world = vec![Mat4::IDENTITY; scene.nodes.len()];
	for &root in scene.resolved_roots().iter() {
		if root < scene.nodes.len() {
			propagate_scene_world_matrix(&scene.nodes, &mut world, root, Mat4::IDENTITY);
		}
	}
	world
}

fn propagate_scene_world_matrix(nodes: &[UnaSceneNode], world: &mut [Mat4], node_index: usize, parent_world: Mat4) {
	let local = Mat4::from_cols_array(&nodes[node_index].transform);
	let node_world = parent_world * local;
	world[node_index] = node_world;
	for &child in &nodes[node_index].children {
		if child < nodes.len() {
			propagate_scene_world_matrix(nodes, world, child, node_world);
		}
	}
}

fn dynamics_surface_constraint_samples(scene: &UnaSceneSnapshot, constraints: &[DynamicsSurfaceConstraint]) -> Vec<String> {
	let paths = scene_node_paths_by_index(scene);
	constraints
		.iter()
		.take(12)
		.map(|constraint| {
			let a = paths
				.get(constraint.node_a)
				.and_then(|path| path.as_deref())
				.and_then(str_leaf)
				.unwrap_or("<unknown>");
			let b = paths
				.get(constraint.node_b)
				.and_then(|path| path.as_deref())
				.and_then(str_leaf)
				.unwrap_or("<unknown>");
			format!("{a}<->{b}:{:.3}", constraint.rest_distance)
		})
		.collect()
}

fn dynamics_surface_constraint_statuses(scene: &UnaSceneSnapshot, constraints: &[DynamicsSurfaceConstraint]) -> Vec<serde_json::Value> {
	let paths = scene_node_paths_by_index(scene);
	constraints
		.iter()
		.map(|constraint| {
			let path_a = paths.get(constraint.node_a).and_then(|path| path.as_deref()).unwrap_or("<unknown>");
			let path_b = paths.get(constraint.node_b).and_then(|path| path.as_deref()).unwrap_or("<unknown>");
			serde_json::json!({
				"node_a": constraint.node_a,
				"node_b": constraint.node_b,
				"path_a": path_a,
				"path_b": path_b,
				"leaf_a": str_leaf(path_a).unwrap_or("<unknown>"),
				"leaf_b": str_leaf(path_b).unwrap_or("<unknown>"),
				"rest_distance": constraint.rest_distance,
				"stiffness": constraint.stiffness,
			})
		})
		.collect()
}

fn str_leaf(path: &str) -> Option<&str> {
	path.rsplit('/').next().filter(|leaf| !leaf.is_empty())
}

fn dynamics_bone_collider_samples(scene: &UnaSceneSnapshot, colliders: &[RuntimeBoneColliderPrimitive]) -> Vec<String> {
	let paths = scene_node_paths_by_index(scene);
	colliders
		.iter()
		.filter(|collider| {
			!collider.source_id.is_empty()
				&& (!collider.collider_path.is_empty()
					|| matches!(
						collider.primitive,
						BoneColliderPrimitive::Sphere { .. }
							| BoneColliderPrimitive::Capsule { .. }
							| BoneColliderPrimitive::LocalSphere { .. }
							| BoneColliderPrimitive::LocalCapsule { .. }
							| BoneColliderPrimitive::LocalPlane { .. }
					))
		})
		.take(24)
		.map(|collider| format_runtime_bone_collider_sample(&paths, collider))
		.collect()
}

fn dynamics_bone_collider_source_counts(colliders: &[RuntimeBoneColliderPrimitive]) -> Vec<String> {
	let mut counts = BTreeMap::<String, usize>::new();
	for collider in colliders {
		if collider.source_id.is_empty() {
			continue;
		}
		let source_leaf = collider
			.source_id
			.rsplit('/')
			.next()
			.unwrap_or(collider.source_id.as_str())
			.to_string();
		*counts.entry(source_leaf).or_default() += 1;
	}
	counts
		.into_iter()
		.take(80)
		.map(|(source, count)| format!("{source}:{count}"))
		.collect()
}

fn dynamics_group_source_samples(dynamics: UnaRuntimeDynamics<'_>) -> Vec<String> {
	dynamics
		.dynamics_groups()
		.filter(|group| !group.source_id.is_empty())
		.take(96)
		.map(|group| {
			let source_leaf = group.source_id.rsplit('/').next().unwrap_or(group.source_id);
			format!(
				"{source_leaf}:enabled={} chain={}",
				group.effective_enabled,
				group.chain.bone_node_indices.len()
			)
		})
		.collect()
}

fn format_runtime_bone_collider_sample(paths: &[Option<String>], collider: &RuntimeBoneColliderPrimitive) -> String {
	format_bone_collider_sample(paths, None, collider.primitive, &collider.source_id, &collider.collider_path)
}

fn format_bone_collider_sample(
	paths: &[Option<String>],
	index: Option<usize>,
	primitive: BoneColliderPrimitive,
	source_id: &str,
	collider_path: &str,
) -> String {
	let (shape, node) = match primitive {
		BoneColliderPrimitive::Sphere { node, .. }
		| BoneColliderPrimitive::LocalSphere { node, .. }
		| BoneColliderPrimitive::LocalCapsule { node, .. }
		| BoneColliderPrimitive::LocalPlane { node, .. } => (runtime_bone_collider_shape_name(primitive), Some(node)),
		BoneColliderPrimitive::Capsule { start_node, .. } => (runtime_bone_collider_shape_name(primitive), Some(start_node)),
	};
	let node_leaf = node
		.and_then(|node| paths.get(node))
		.and_then(|path| path.as_deref())
		.and_then(str_leaf)
		.unwrap_or("<unknown>");
	let source_leaf = source_id.rsplit('/').next().unwrap_or(source_id);
	let collider_leaf = collider_path.rsplit('/').next().filter(|leaf| !leaf.is_empty());
	match index {
		Some(index) => match collider_leaf {
			Some(collider_leaf) => format!("#{index}:{source_leaf}/{collider_leaf}:{shape}@{node_leaf}"),
			None => format!("#{index}:{source_leaf}:{shape}@{node_leaf}"),
		},
		None => match collider_leaf {
			Some(collider_leaf) => format!("{source_leaf}/{collider_leaf}:{shape}@{node_leaf}"),
			None => format!("{source_leaf}:{shape}@{node_leaf}"),
		},
	}
}

fn runtime_bone_collider_shape_name(collider: BoneColliderPrimitive) -> &'static str {
	match collider {
		BoneColliderPrimitive::Sphere { .. } | BoneColliderPrimitive::LocalSphere { .. } => "sphere",
		BoneColliderPrimitive::Capsule { .. } | BoneColliderPrimitive::LocalCapsule { .. } => "capsule",
		BoneColliderPrimitive::LocalPlane { .. } => "plane",
	}
}

fn runtime_bone_collider_detail(
	paths: &[Option<String>],
	world: &[Mat4],
	index: usize,
	primitive: BoneColliderPrimitive,
	source_id: &str,
	collider_path: &str,
) -> serde_json::Value {
	let node_path = |node: usize| paths.get(node).and_then(|path| path.as_deref()).unwrap_or("<unknown>");
	let world_point = |node: usize, point: [f32; 3]| {
		world
			.get(node)
			.map(|matrix| matrix.transform_point3(Vec3::from_array(point)).to_array())
			.unwrap_or(point)
	};
	let world_vector = |node: usize, vector: [f32; 3]| {
		world
			.get(node)
			.map(|matrix| matrix.transform_vector3(Vec3::from_array(vector)).to_array())
			.unwrap_or(vector)
	};
	match primitive {
		BoneColliderPrimitive::Sphere { node, radius } => serde_json::json!({
			"index": index,
			"source_id": source_id,
			"collider_path": collider_path,
			"shape": "sphere",
			"node": node,
			"node_path": node_path(node),
			"radius": radius,
			"world_center": world_point(node, [0.0, 0.0, 0.0]),
			"inside_bounds": false,
		}),
		BoneColliderPrimitive::Capsule {
			start_node,
			end_node,
			radius,
		} => serde_json::json!({
			"index": index,
			"source_id": source_id,
			"collider_path": collider_path,
			"shape": "capsule",
			"start_node": start_node,
			"start_node_path": node_path(start_node),
			"end_node": end_node,
			"end_node_path": node_path(end_node),
			"radius": radius,
			"world_a": world_point(start_node, [0.0, 0.0, 0.0]),
			"world_b": world_point(end_node, [0.0, 0.0, 0.0]),
			"inside_bounds": false,
		}),
		BoneColliderPrimitive::LocalSphere {
			node,
			center,
			radius,
			inside_bounds,
		} => serde_json::json!({
			"index": index,
			"source_id": source_id,
			"collider_path": collider_path,
			"shape": "local_sphere",
			"node": node,
			"node_path": node_path(node),
			"center": center,
			"radius": radius,
			"world_center": world_point(node, center),
			"inside_bounds": inside_bounds,
		}),
		BoneColliderPrimitive::LocalCapsule {
			node,
			center,
			axis,
			half_length,
			radius,
			inside_bounds,
		} => {
			let axis_vec = Vec3::from_array(axis).normalize_or_zero();
			let a = Vec3::from_array(center) - axis_vec * half_length;
			let b = Vec3::from_array(center) + axis_vec * half_length;
			serde_json::json!({
				"index": index,
				"source_id": source_id,
				"collider_path": collider_path,
				"shape": "local_capsule",
				"node": node,
				"node_path": node_path(node),
				"center": center,
				"axis": axis,
				"half_length": half_length,
				"radius": radius,
				"world_a": world_point(node, a.to_array()),
				"world_b": world_point(node, b.to_array()),
				"world_axis": world_vector(node, axis),
				"inside_bounds": inside_bounds,
			})
		}
		BoneColliderPrimitive::LocalPlane {
			node,
			center,
			normal,
			inside_bounds,
		} => serde_json::json!({
			"index": index,
			"source_id": source_id,
			"collider_path": collider_path,
			"shape": "local_plane",
			"node": node,
			"node_path": node_path(node),
			"center": center,
			"normal": normal,
			"world_point": world_point(node, center),
			"world_normal": world_vector(node, normal),
			"inside_bounds": inside_bounds,
		}),
	}
}

fn dynamics_collider_selection_statuses(
	sim: &DynamicsSimulator,
	node_paths_by_index: &[Option<String>],
	world: &[Mat4],
) -> Vec<RuntimeDynamicsColliderSelectionStatus> {
	let bone_colliders = sim.bone_colliders();
	let bone_collider_source_ids = sim.bone_collider_source_ids();
	let bone_collider_paths = sim.bone_collider_paths();
	sim.collider_selection_summaries()
		.into_iter()
		.map(|summary| {
			let sample_colliders = summary
				.sample_collider_indices
				.iter()
				.filter_map(|index| {
					let primitive = bone_colliders.get(*index).copied()?;
					let source_id = bone_collider_source_ids.get(*index).map(String::as_str).unwrap_or_default();
					let collider_path = bone_collider_paths.get(*index).map(String::as_str).unwrap_or_default();
					Some(format_bone_collider_sample(
						node_paths_by_index,
						Some(*index),
						primitive,
						source_id,
						collider_path,
					))
				})
				.collect();
			let sample_collider_details = summary
				.sample_collider_indices
				.iter()
				.filter_map(|index| {
					let primitive = bone_colliders.get(*index).copied()?;
					let source_id = bone_collider_source_ids.get(*index).map(String::as_str).unwrap_or_default();
					let collider_path = bone_collider_paths.get(*index).map(String::as_str).unwrap_or_default();
					Some(runtime_bone_collider_detail(
						node_paths_by_index,
						world,
						*index,
						primitive,
						source_id,
						collider_path,
					))
				})
				.collect();
			RuntimeDynamicsColliderSelectionStatus {
				source_id: summary.source_id,
				selected_collider_count: summary.selected_collider_count,
				global_collider_count: summary.global_collider_count,
				authored_collider_count: summary.authored_collider_count,
				sample_collider_indices: summary.sample_collider_indices,
				sample_collider_source_ids: summary.sample_collider_source_ids,
				sample_collider_paths: summary.sample_collider_paths,
				sample_colliders,
				sample_collider_details,
			}
		})
		.collect()
}

fn dynamics_collider_details_by_selection_source<'a>(
	collider_selections: &'a [RuntimeDynamicsColliderSelectionStatus],
) -> BTreeMap<&'a str, Vec<&'a serde_json::Value>> {
	let mut colliders_by_source_id = BTreeMap::<&str, Vec<&serde_json::Value>>::new();
	let mut seen = Vec::<(&str, String)>::new();
	for selection in collider_selections {
		for detail in &selection.sample_collider_details {
			let key = (
				selection.source_id.as_str(),
				detail
					.get("index")
					.and_then(Value::as_u64)
					.map(|index| index.to_string())
					.or_else(|| detail.get("collider_path").and_then(Value::as_str).map(str::to_string))
					.unwrap_or_default(),
			);
			if !seen.iter().any(|seen| seen == &key) {
				seen.push(key);
				colliders_by_source_id.entry(selection.source_id.as_str()).or_default().push(detail);
			}
		}
	}
	colliders_by_source_id
}

fn dynamics_collider_contact_statuses(
	tail_samples: &[DynamicsTailSample],
	collider_selections: &[RuntimeDynamicsColliderSelectionStatus],
) -> Vec<RuntimeDynamicsColliderContactStatus> {
	const CONTACT_STATUS_LIMIT: usize = 1024;
	let colliders_by_source_id = dynamics_collider_details_by_selection_source(collider_selections);
	let mut out = Vec::new();
	for tail in tail_samples {
		let Some(colliders) = colliders_by_source_id.get(tail.source_id.as_str()) else {
			continue;
		};
		let tail_point = Vec3::from_array(tail.curr_tail);
		let mut best = None::<RuntimeDynamicsColliderContactStatus>;
		for collider in colliders {
			let Some(contact) = dynamics_collider_contact_status(tail, tail_point, collider) else {
				continue;
			};
			if best.as_ref().is_none_or(|best| contact.margin < best.margin) {
				best = Some(contact);
			}
		}
		if let Some(contact) = best {
			out.push(contact);
		}
		if out.len() >= CONTACT_STATUS_LIMIT {
			break;
		}
	}
	out
}

fn dynamics_collider_contact_summary_statuses(
	contacts: &[RuntimeDynamicsColliderContactStatus],
) -> Vec<RuntimeDynamicsColliderContactSummary> {
	let mut summaries = BTreeMap::<String, RuntimeDynamicsColliderContactSummary>::new();
	for contact in contacts {
		let summary = summaries
			.entry(contact.source_id.clone())
			.or_insert_with(|| RuntimeDynamicsColliderContactSummary {
				source_id: contact.source_id.clone(),
				contact_count: 0,
				penetrating_count: 0,
				min_margin: contact.margin,
				min_distance: contact.distance,
				min_threshold: contact.threshold,
				closest_collider_path: contact.collider_path.clone(),
				closest_collider_shape: contact.collider_shape.clone(),
			});
		summary.contact_count += 1;
		if contact.penetrating {
			summary.penetrating_count += 1;
		}
		if contact.margin < summary.min_margin {
			summary.min_margin = contact.margin;
			summary.min_distance = contact.distance;
			summary.min_threshold = contact.threshold;
			summary.closest_collider_path = contact.collider_path.clone();
			summary.closest_collider_shape = contact.collider_shape.clone();
		}
	}
	summaries.into_values().collect()
}

fn dynamics_collider_runtime_summary_statuses(
	selections: &[RuntimeDynamicsColliderSelectionStatus],
	contact_summaries: &[RuntimeDynamicsColliderContactSummary],
) -> Vec<RuntimeDynamicsColliderRuntimeSummary> {
	let contact_summaries = contact_summaries
		.iter()
		.map(|summary| (summary.source_id.as_str(), summary))
		.collect::<BTreeMap<_, _>>();
	selections
		.iter()
		.map(|selection| {
			let contact = contact_summaries.get(selection.source_id.as_str()).copied();
			RuntimeDynamicsColliderRuntimeSummary {
				source_id: selection.source_id.clone(),
				selected_collider_count: selection.selected_collider_count,
				global_collider_count: selection.global_collider_count,
				authored_collider_count: selection.authored_collider_count,
				contact_count: contact.map_or(0, |summary| summary.contact_count),
				penetrating_count: contact.map_or(0, |summary| summary.penetrating_count),
				min_margin: contact.map(|summary| summary.min_margin),
				min_distance: contact.map(|summary| summary.min_distance),
				min_threshold: contact.map(|summary| summary.min_threshold),
				closest_collider_path: contact.map_or_else(String::new, |summary| summary.closest_collider_path.clone()),
				closest_collider_shape: contact.map_or_else(String::new, |summary| summary.closest_collider_shape.clone()),
				sample_collider_paths: selection.sample_collider_paths.clone(),
			}
		})
		.collect()
}

fn dynamics_collider_path_contact_summary_statuses(
	contacts: &[RuntimeDynamicsColliderContactStatus],
) -> Vec<RuntimeDynamicsColliderPathContactSummary> {
	#[derive(Clone)]
	struct Accum {
		summary: RuntimeDynamicsColliderPathContactSummary,
		source_ids: Vec<String>,
	}
	let mut by_collider = BTreeMap::<String, Accum>::new();
	for contact in contacts {
		let key = if contact.collider_path.is_empty() {
			format!(
				"collider_index:{}",
				contact.collider_index.map_or_else(|| "?".to_string(), |index| index.to_string())
			)
		} else {
			contact.collider_path.clone()
		};
		let accum = by_collider.entry(key).or_insert_with(|| Accum {
			summary: RuntimeDynamicsColliderPathContactSummary {
				collider_path: contact.collider_path.clone(),
				collider_shape: contact.collider_shape.clone(),
				contact_count: 0,
				penetrating_count: 0,
				source_count: 0,
				min_margin: contact.margin,
				min_distance: contact.distance,
				min_threshold: contact.threshold,
				sample_source_ids: Vec::new(),
			},
			source_ids: Vec::new(),
		});
		accum.summary.contact_count += 1;
		if contact.penetrating {
			accum.summary.penetrating_count += 1;
		}
		if contact.margin < accum.summary.min_margin {
			accum.summary.min_margin = contact.margin;
			accum.summary.min_distance = contact.distance;
			accum.summary.min_threshold = contact.threshold;
			accum.summary.collider_shape = contact.collider_shape.clone();
		}
		if !accum.source_ids.iter().any(|source_id| source_id == &contact.source_id) {
			accum.source_ids.push(contact.source_id.clone());
			if accum.summary.sample_source_ids.len() < 8 {
				accum.summary.sample_source_ids.push(contact.source_id.clone());
			}
		}
	}
	by_collider
		.into_values()
		.map(|mut accum| {
			accum.summary.source_count = accum.source_ids.len();
			accum.summary
		})
		.collect()
}

fn dynamics_collider_path_candidate_summary_statuses(
	tail_samples: &[DynamicsTailSample],
	collider_selections: &[RuntimeDynamicsColliderSelectionStatus],
) -> Vec<RuntimeDynamicsColliderPathCandidateSummary> {
	#[derive(Clone)]
	struct Accum {
		summary: RuntimeDynamicsColliderPathCandidateSummary,
		source_ids: Vec<String>,
	}
	let colliders_by_source_id = dynamics_collider_details_by_selection_source(collider_selections);

	let mut by_collider = BTreeMap::<String, Accum>::new();
	for tail in tail_samples {
		let Some(colliders) = colliders_by_source_id.get(tail.source_id.as_str()) else {
			continue;
		};
		let tail_point = Vec3::from_array(tail.curr_tail);
		for collider in colliders {
			let Some(contact) = dynamics_collider_contact_status(tail, tail_point, collider) else {
				continue;
			};
			let key = if contact.collider_path.is_empty() {
				format!(
					"collider_index:{}",
					contact.collider_index.map_or_else(|| "?".to_string(), |index| index.to_string())
				)
			} else {
				contact.collider_path.clone()
			};
			let accum = by_collider.entry(key.clone()).or_insert_with(|| Accum {
				summary: RuntimeDynamicsColliderPathCandidateSummary {
					collider_path: if contact.collider_path.is_empty() {
						key.clone()
					} else {
						contact.collider_path.clone()
					},
					collider_shape: contact.collider_shape.clone(),
					candidate_count: 0,
					penetrating_count: 0,
					source_count: 0,
					min_margin: contact.margin,
					min_distance: contact.distance,
					min_threshold: contact.threshold,
					sample_source_ids: Vec::new(),
				},
				source_ids: Vec::new(),
			});
			accum.summary.candidate_count += 1;
			if contact.penetrating {
				accum.summary.penetrating_count += 1;
			}
			if contact.margin < accum.summary.min_margin {
				accum.summary.min_margin = contact.margin;
				accum.summary.min_distance = contact.distance;
				accum.summary.min_threshold = contact.threshold;
				accum.summary.collider_shape = contact.collider_shape.clone();
			}
			if !accum.source_ids.iter().any(|source_id| source_id == &contact.source_id) {
				accum.source_ids.push(contact.source_id.clone());
				if accum.summary.sample_source_ids.len() < 8 {
					accum.summary.sample_source_ids.push(contact.source_id.clone());
				}
			}
		}
	}
	by_collider
		.into_values()
		.map(|mut accum| {
			accum.summary.source_count = accum.source_ids.len();
			accum.summary
		})
		.collect()
}

fn dynamics_collider_path_runtime_summary_statuses(
	colliders: &[RuntimeDynamicsColliderStatus],
	contact_summaries: &[RuntimeDynamicsColliderPathContactSummary],
	candidate_summaries: &[RuntimeDynamicsColliderPathCandidateSummary],
	projection_counts: &BTreeMap<String, u32>,
) -> Vec<RuntimeDynamicsColliderPathRuntimeSummary> {
	#[derive(Default)]
	struct Accum {
		collider_path: String,
		collider_shape: String,
		runtime_collider_count: usize,
		source_ids: Vec<String>,
		sample_source_ids: Vec<String>,
	}
	fn push_sample_source_ids(accum: &mut Accum, sample_source_ids: &[String]) {
		for source_id in sample_source_ids {
			if accum.sample_source_ids.len() >= 8 {
				break;
			}
			if !accum.sample_source_ids.iter().any(|sample| sample == source_id) {
				accum.sample_source_ids.push(source_id.clone());
			}
		}
	}
	fn ensure_accum<'a>(by_collider: &'a mut BTreeMap<String, Accum>, collider_path: &str, collider_shape: &str) -> &'a mut Accum {
		by_collider.entry(collider_path.to_string()).or_insert_with(|| Accum {
			collider_path: collider_path.to_string(),
			collider_shape: collider_shape.to_string(),
			..Default::default()
		})
	}
	let mut by_collider = BTreeMap::<String, Accum>::new();
	for collider in colliders {
		let key = if collider.collider_path.is_empty() {
			format!("collider_index:{}", collider.index)
		} else {
			collider.collider_path.clone()
		};
		let accum = by_collider.entry(key).or_insert_with(|| Accum {
			collider_path: if collider.collider_path.is_empty() {
				format!("collider_index:{}", collider.index)
			} else {
				collider.collider_path.clone()
			},
			collider_shape: format!("{:?}", collider.shape).to_ascii_lowercase(),
			..Default::default()
		});
		accum.runtime_collider_count += 1;
		if !accum.source_ids.iter().any(|source_id| source_id == &collider.source_id) {
			accum.source_ids.push(collider.source_id.clone());
			if accum.sample_source_ids.len() < 8 {
				accum.sample_source_ids.push(collider.source_id.clone());
			}
		}
	}
	for summary in contact_summaries {
		if summary.collider_path.is_empty() {
			continue;
		}
		let accum = ensure_accum(&mut by_collider, &summary.collider_path, &summary.collider_shape);
		if accum.collider_shape.is_empty() {
			accum.collider_shape = summary.collider_shape.clone();
		}
		push_sample_source_ids(accum, &summary.sample_source_ids);
	}
	for summary in candidate_summaries {
		if summary.collider_path.is_empty() {
			continue;
		}
		let accum = ensure_accum(&mut by_collider, &summary.collider_path, &summary.collider_shape);
		if accum.collider_shape.is_empty() {
			accum.collider_shape = summary.collider_shape.clone();
		}
		push_sample_source_ids(accum, &summary.sample_source_ids);
	}
	for collider_path in projection_counts.keys() {
		if collider_path.is_empty() {
			continue;
		}
		ensure_accum(&mut by_collider, collider_path, "");
	}
	let contact_summaries = contact_summaries
		.iter()
		.map(|summary| (summary.collider_path.as_str(), summary))
		.collect::<BTreeMap<_, _>>();
	let candidate_summaries = candidate_summaries
		.iter()
		.map(|summary| (summary.collider_path.as_str(), summary))
		.collect::<BTreeMap<_, _>>();
	by_collider
		.into_values()
		.map(|accum| {
			let contact = contact_summaries.get(accum.collider_path.as_str()).copied();
			let candidate = candidate_summaries.get(accum.collider_path.as_str()).copied();
			let projection_count = projection_counts.get(accum.collider_path.as_str()).copied().unwrap_or_default();
			let source_count = accum
				.source_ids
				.len()
				.max(contact.map_or(0, |summary| summary.source_count))
				.max(candidate.map_or(0, |summary| summary.source_count));
			RuntimeDynamicsColliderPathRuntimeSummary {
				collider_path: accum.collider_path,
				collider_shape: contact.map_or(accum.collider_shape, |summary| summary.collider_shape.clone()),
				runtime_collider_count: accum.runtime_collider_count,
				candidate_count: candidate.map_or(0, |summary| summary.candidate_count),
				candidate_penetrating_count: candidate.map_or(0, |summary| summary.penetrating_count),
				source_count,
				contact_count: contact.map_or(0, |summary| summary.contact_count),
				penetrating_count: contact.map_or(0, |summary| summary.penetrating_count),
				projection_count,
				min_margin: contact.map(|summary| summary.min_margin),
				min_distance: contact.map(|summary| summary.min_distance),
				min_threshold: contact.map(|summary| summary.min_threshold),
				sample_source_ids: accum.sample_source_ids,
			}
		})
		.collect()
}

fn dynamics_collider_contact_status(
	tail: &DynamicsTailSample,
	tail_point: Vec3,
	collider: &serde_json::Value,
) -> Option<RuntimeDynamicsColliderContactStatus> {
	let shape = collider.get("shape").and_then(Value::as_str).unwrap_or_default();
	let shape_kind = dynamics_collider_shape_kind(shape)?;
	let radius = collider.get("radius").and_then(Value::as_f64).unwrap_or(0.0) as f32;
	let inside_bounds = collider.get("inside_bounds").and_then(Value::as_bool).unwrap_or(false);
	let distance = match shape_kind {
		DynamicsColliderShapeKind::Capsule => {
			let a = json_vec3(collider.get("world_a")?)?;
			let b = json_vec3(collider.get("world_b")?)?;
			distance_point_segment(tail_point, a, b)
		}
		DynamicsColliderShapeKind::Sphere => {
			let center = json_vec3(collider.get("world_center").or_else(|| collider.get("center"))?)?;
			tail_point.distance(center)
		}
		DynamicsColliderShapeKind::Plane => {
			let point = json_vec3(collider.get("world_point")?)?;
			let normal = json_vec3(collider.get("world_normal")?)?.normalize_or_zero();
			(tail_point - point).dot(normal).abs()
		}
	};
	if !distance.is_finite() {
		return None;
	}
	let hit_radius = tail.hit_radius.max(0.0);
	let threshold = match shape_kind {
		DynamicsColliderShapeKind::Plane => hit_radius,
		DynamicsColliderShapeKind::Capsule | DynamicsColliderShapeKind::Sphere => radius.max(0.0) + hit_radius,
	};
	let margin = distance - threshold;
	Some(RuntimeDynamicsColliderContactStatus {
		source_id: tail.source_id.clone(),
		runtime_index: tail.runtime_index,
		joint_index: tail.joint_index,
		parent_node: tail.parent_node,
		child_node: tail.child_node,
		collider_index: collider.get("index").and_then(Value::as_u64).map(|index| index as usize),
		collider_path: collider
			.get("collider_path")
			.and_then(Value::as_str)
			.unwrap_or_default()
			.to_string(),
		collider_shape: shape.to_string(),
		hit_radius,
		collider_radius: radius.max(0.0),
		distance,
		threshold,
		margin,
		inside_bounds,
		penetrating: margin < 0.0,
	})
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DynamicsColliderShapeKind {
	Capsule,
	Sphere,
	Plane,
}

fn dynamics_collider_shape_kind(shape: &str) -> Option<DynamicsColliderShapeKind> {
	match shape.trim().to_ascii_lowercase().replace('-', "_").as_str() {
		"capsule" | "local_capsule" => Some(DynamicsColliderShapeKind::Capsule),
		"sphere" | "local_sphere" => Some(DynamicsColliderShapeKind::Sphere),
		"plane" | "local_plane" => Some(DynamicsColliderShapeKind::Plane),
		_ => None,
	}
}

fn json_vec3(value: &serde_json::Value) -> Option<Vec3> {
	let array = value.as_array()?;
	if array.len() < 3 {
		return None;
	}
	Some(Vec3::new(
		array.first()?.as_f64()? as f32,
		array.get(1)?.as_f64()? as f32,
		array.get(2)?.as_f64()? as f32,
	))
}

fn augment_dynamics_bone_colliders(
	dynamics: UnaRuntimeDynamics<'_>,
	scene: &UnaSceneSnapshot,
	config: &DynamicsPhysicsConfig,
	colliders: &mut Vec<RuntimeBoneColliderPrimitive>,
) {
	if config.collider_augment_overrides.is_empty() {
		return;
	}
	let groups = dynamics.dynamics_groups().collect::<Vec<_>>();
	let group_match_texts = groups
		.iter()
		.map(|group| (*group, dynamics_group_match_text(scene, *group)))
		.collect::<Vec<_>>();
	let mut existing_pairs = colliders
		.iter()
		.map(|collider| (collider.source_id.clone(), collider.collider_path.clone()))
		.collect::<Vec<_>>();
	existing_pairs.sort_unstable();
	existing_pairs.dedup();
	for override_item in &config.collider_augment_overrides {
		let source_needles = override_item
			.source_id_contains
			.iter()
			.map(|needle| dynamics_normalize_match_text(needle))
			.filter(|needle| !needle.is_empty())
			.collect::<Vec<_>>();
		let collider_path_needles = override_item
			.collider_path_contains
			.iter()
			.map(|needle| dynamics_normalize_match_text(needle))
			.filter(|needle| !needle.is_empty())
			.collect::<Vec<_>>();
		if source_needles.is_empty() || collider_path_needles.is_empty() {
			continue;
		}
		let target_source_ids = group_match_texts
			.iter()
			.filter(|(_, match_text)| runtime_token_filter_matches(match_text, &source_needles))
			.map(|(group, _)| {
				let source_id = group.source_id.to_string();
				let scope = runtime_physics_source_scope_key(group.source_id);
				(source_id, scope)
			})
			.collect::<Vec<_>>();
		if target_source_ids.is_empty() {
			continue;
		}
		let candidates = colliders
			.iter()
			.filter(|collider| {
				runtime_token_filter_matches(&dynamics_normalize_match_text(&collider.collider_path), &collider_path_needles)
			})
			.map(|collider| {
				let scope = runtime_physics_source_scope_key(&collider.collider_path);
				(collider.clone(), scope)
			})
			.collect::<Vec<_>>();
		if candidates.is_empty() {
			eprintln!(
				"un-avatar-renderer: dynamics collider augment '{}' skipped: targets={} candidates=0",
				override_item.name,
				target_source_ids.len()
			);
			continue;
		}
		let before_count = colliders.len();
		for (target_source_id, target_scope) in target_source_ids {
			for (candidate, candidate_scope) in &candidates {
				if !target_scope.is_empty() && !candidate_scope.is_empty() && target_scope.as_str() != candidate_scope {
					continue;
				}
				let pair = (target_source_id.clone(), candidate.collider_path.clone());
				let pair_index = match existing_pairs.binary_search(&pair) {
					Ok(_) => continue,
					Err(index) => index,
				};
				let mut augmented = candidate.clone();
				augmented.source_id.clone_from(&target_source_id);
				colliders.push(augmented);
				existing_pairs.insert(pair_index, pair);
			}
		}
		let added = colliders.len().saturating_sub(before_count);
		eprintln!(
			"un-avatar-renderer: dynamics collider augment '{}' targets added={} candidates={}",
			override_item.name,
			added,
			candidates.len()
		);
	}
}

fn build_runtime_physics_for_document(
	document: &UnaDocument,
	dynamics_enabled: bool,
	bone_collider_config: BoneColliderConfig,
	dynamics_physics: &DynamicsPhysicsConfig,
) -> RuntimePhysicsBuild {
	let dynamics_physics = dynamics_physics.clone().normalized();
	let dynamics_physics = &dynamics_physics;
	let runtime_model = document.runtime_model();
	let scene_profile_dynamics = runtime_model.scene_profile_dynamics();
	let mut tagged_bone_colliders = if let Some(runtime) = scene_profile_dynamics {
		build_dynamics_bone_colliders_with_sources(runtime.scene, runtime.humanoid_profile, bone_collider_config, runtime.dynamics)
	} else {
		Vec::new()
	};
	if let Some(runtime) = scene_profile_dynamics {
		augment_dynamics_bone_colliders(runtime.dynamics, runtime.scene, dynamics_physics, &mut tagged_bone_colliders);
	}
	if let Some(runtime) = scene_profile_dynamics {
		let authored = tagged_bone_colliders
			.iter()
			.filter(|collider| !collider.source_id.is_empty())
			.count();
		let global_or_auto = tagged_bone_colliders.len().saturating_sub(authored);
		let samples = dynamics_bone_collider_samples(runtime.scene, &tagged_bone_colliders).join(", ");
		let counts = dynamics_bone_collider_source_counts(&tagged_bone_colliders).join(", ");
		let group_samples = dynamics_group_source_samples(runtime.dynamics).join(", ");
		eprintln!(
			"un-avatar-renderer: dynamics bone colliders built total={} global_or_auto={} authored={} augment_overrides={} source_counts=[{}] group_samples=[{}] samples=[{}]",
			tagged_bone_colliders.len(),
			global_or_auto,
			authored,
			dynamics_physics.collider_augment_overrides.len(),
			counts,
			group_samples,
			samples
		);
	}
	let bone_colliders = tagged_bone_colliders.iter().map(|collider| collider.primitive).collect::<Vec<_>>();
	let stats = collider_stats(&bone_colliders);
	let dynamics_sim = if dynamics_enabled {
		if let Some(runtime) = scene_profile_dynamics {
			if runtime.dynamics.has_groups() {
				let surface_constraints = if dynamics_physics.surface_constraints_enabled {
					build_dynamics_surface_constraints(runtime.scene, runtime.dynamics, dynamics_physics)
				} else {
					Vec::new()
				};
				if !surface_constraints.is_empty() {
					let samples = dynamics_surface_constraint_samples(runtime.scene, &surface_constraints).join(", ");
					eprintln!(
						"un-avatar-renderer: dynamics surface constraints generated count={} samples=[{}]",
						surface_constraints.len(),
						samples
					);
				}
				DynamicsSimulator::new_with_runtime_dynamics_collider_sources_and_surface_constraints(
					runtime.scene,
					runtime.dynamics,
					tagged_bone_colliders,
					dynamics_physics.clone(),
					&surface_constraints,
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
	if let Some(sim) = dynamics_sim.as_ref() {
		eprintln!(
			"un-avatar-renderer: dynamics simulator stats active_groups={} active_joints={} colliders={} surface_constraints={} translation_writeback_candidates={} translation_writeback_targets={}",
			sim.active_group_count(),
			sim.active_joint_count(),
			sim.bone_collider_count(),
			sim.surface_constraint_count(),
			sim.translation_writeback_candidate_count(),
			sim.translation_writeback_target_count()
		);
		let response_categories = sim
			.response_category_summaries()
			.into_iter()
			.map(|summary| {
				format!(
					"{}:groups={} joints={} xpbd={} compliance={:.5} rest={:.3} shape={:.3} bounce={:.3} drag={:.3} follow={:.3} orient={:.3}",
					summary.category,
					summary.group_count,
					summary.joint_count,
					summary.xpbd_group_count,
					summary.average_xpbd_compliance,
					summary.average_rest_response,
					summary.average_shape_preservation,
					summary.average_bounce_response,
					summary.average_drag_force,
					summary.average_parent_motion_follow,
					summary.average_orientation_follow
				)
			})
			.collect::<Vec<_>>()
			.join(", ");
		eprintln!("un-avatar-renderer: dynamics response categories [{response_categories}]");
	}
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

pub(crate) fn restore_runtime_scene_transforms_to_rest(document: &mut UnaDocument, rest_nodes: &[UnaSceneNode]) -> Result<(), String> {
	let mut runtime_model = document.runtime_model_mut();
	let Some((scene, _profile)) = runtime_model.humanoid_scene_mut() else {
		return Ok(());
	};
	if scene.nodes.len() != rest_nodes.len() {
		return Err(format!(
			"rest node count mismatch while preparing wardrobe scene: scene={} rest={}",
			scene.nodes.len(),
			rest_nodes.len()
		));
	}
	for (dst, src) in scene.nodes.iter_mut().zip(rest_nodes.iter()) {
		dst.transform = src.transform;
	}
	Ok(())
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

fn restored_dynamics_source_ids(restored: &[un_avatar_core::UnaEvaluationRestoreApplyEntry]) -> Vec<String> {
	let mut source_ids = Vec::new();
	for entry in restored {
		if entry.target_kind == UnaEvaluationTargetKind::DynamicsEnabled && !entry.target_key.is_empty() {
			source_ids.push(entry.target_key.clone());
		}
	}
	source_ids.sort_unstable();
	source_ids.dedup();
	source_ids
}

fn log_slow_gpu_scene_context_step(label: impl std::fmt::Display, elapsed: Duration) {
	let elapsed_ms = elapsed.as_secs_f64() * 1000.0;
	if elapsed_ms >= 50.0 {
		eprintln!("un-avatar-renderer: gpu scene {label}: {elapsed_ms:.1}ms");
	}
}

pub(crate) struct GpuSceneBuildContext {
	device: wgpu::Device,
	queue: wgpu::Queue,
	format: wgpu::TextureFormat,
	aa: AaMode,
	shader_variant_tier: MeshShaderVariantTier,
	pipeline_cache: PersistentPipelineCache,
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
#[derive(Clone, Debug, Default)]
pub struct FrameTimings {
	pub wall_since_last_ms: f32,
	pub cpu_record_ms: f32,
	pub cpu_total_ms: f32,
	pub motion_apply_ms: f32,
	pub dynamics_step_ms: f32,
	pub dynamics_profile: DynamicsStepProfile,
	pub frame_globals_ms: f32,
	pub surface_acquire_ms: f32,
	pub target_prepare_ms: f32,
	pub draw_state_refresh_ms: f32,
	pub draw_doc_lock_ms: f32,
	pub draw_expression_select_ms: f32,
	pub draw_update_total_ms: f32,
	pub scene_world_ms: f32,
	pub draw_skin_palette_ms: f32,
	pub draw_skin_palette_write_ms: f32,
	pub draw_fur_source_vertices_ms: f32,
	pub draw_expression_values_ms: f32,
	pub draw_morph_weights_ms: f32,
	pub draw_transform_loop_ms: f32,
	pub bone_collider_debug_ms: f32,
	pub command_encode_ms: f32,
	pub submit_present_ms: f32,
	pub spout_cpu_ms: f32,
	pub contact_eval_ms: f32,
	pub runtime_action_eval_ms: f32,
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

fn effective_window_backend(backend: RenderBackend, transparent: bool) -> RenderBackend {
	#[cfg(windows)]
	{
		// Windows Vulkan HWND surfaces commonly expose only Opaque alpha. Keep Vulkan
		// as the default path, and switch to DX12 DirectComposition only when real
		// transparent-window presentation is requested.
		if transparent && backend == RenderBackend::Vulkan {
			return RenderBackend::Dx12;
		}
	}
	backend
}

fn log_effective_window_backend(requested: RenderBackend, effective: RenderBackend, transparent: bool) {
	#[cfg(windows)]
	if transparent && requested == RenderBackend::Vulkan && effective == RenderBackend::Dx12 {
		eprintln!(
			"un-avatar-renderer: transparent window output uses DX12 on Windows; Vulkan pipeline cache/prewarm is unavailable for this renderer startup"
		);
	}
	let _ = (requested, effective, transparent);
}

fn gpu_backend_label(backend: wgpu::Backend) -> &'static str {
	match backend {
		wgpu::Backend::Noop => "noop",
		wgpu::Backend::Vulkan => "vulkan",
		wgpu::Backend::Metal => "metal",
		wgpu::Backend::Dx12 => "dx12",
		wgpu::Backend::Gl => "gl",
		wgpu::Backend::BrowserWebGpu => "webgpu",
	}
}

fn gpu_adapter_selector_from_info(info: &wgpu::AdapterInfo) -> String {
	format!(
		"{}:{:04x}:{:04x}:{}",
		gpu_backend_label(info.backend),
		info.vendor,
		info.device,
		info.name
	)
}

fn gpu_device_selector_from_info(info: &wgpu::AdapterInfo) -> String {
	format!("gpu:{:04x}:{:04x}:{}", info.vendor, info.device, info.name)
}

fn adapter_matches_selector(info: &wgpu::AdapterInfo, selector: &str) -> bool {
	let selector = selector.trim();
	if selector.is_empty() || selector.eq_ignore_ascii_case("auto") {
		return false;
	}
	gpu_device_selector_from_info(info).eq_ignore_ascii_case(selector)
		|| gpu_adapter_selector_from_info(info).eq_ignore_ascii_case(selector)
		|| info.name.eq_ignore_ascii_case(selector)
}

fn request_auto_adapter(instance: &wgpu::Instance, surface: Option<&wgpu::Surface<'static>>) -> Result<wgpu::Adapter, String> {
	pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
		power_preference: wgpu::PowerPreference::HighPerformance,
		compatible_surface: surface,
		force_fallback_adapter: false,
	}))
	.map_err(|e| format!("request_adapter: {e}"))
}

fn adapter_surface_compatible(adapter: &wgpu::Adapter, surface: Option<&wgpu::Surface<'static>>) -> bool {
	let Some(surface) = surface else {
		return true;
	};
	!surface.get_capabilities(adapter).formats.is_empty()
}

fn resolve_adapter(
	instance: &wgpu::Instance,
	backends: wgpu::Backends,
	surface: Option<&wgpu::Surface<'static>>,
	gpu_adapter: Option<&str>,
	context: &str,
) -> Result<wgpu::Adapter, String> {
	let selector = gpu_adapter
		.map(str::trim)
		.filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("auto"));
	if let Some(selector) = selector {
		for adapter in pollster::block_on(instance.enumerate_adapters(backends)) {
			let info = adapter.get_info();
			if adapter_matches_selector(&info, selector) {
				if adapter_surface_compatible(&adapter, surface) {
					eprintln!(
						"un-avatar-renderer: {context} selected GPU adapter {} ({:?}, {}, vendor {:04x}, device {:04x})",
						info.name,
						info.device_type,
						gpu_backend_label(info.backend),
						info.vendor,
						info.device
					);
					return Ok(adapter);
				}
				return Err(format!(
					"{context}: selected GPU '{}' matched {} ({:?}, {}) but is not compatible with this surface",
					selector,
					info.name,
					info.device_type,
					gpu_backend_label(info.backend)
				));
			}
		}
		return Err(format!(
			"{context}: selected GPU '{}' is not available for the requested render backend",
			selector
		));
	}
	request_auto_adapter(instance, surface)
}

pub(crate) fn scene_mesh_load_opts_for_window_options(opts: &AvatarWindowOptions) -> SceneMeshLoadOpts {
	let mut mesh_diagnostics = opts.mesh_diagnostics.clone();
	mesh_diagnostics.force_simple_basecolor |= opts.simple_basecolor_only;
	mesh_diagnostics.disable_mtoon_outlines |= opts.disable_mtoon_outlines;
	mesh_diagnostics.debug_disable_rim_lighting |= opts.debug_disable_rim_lighting;
	mesh_diagnostics.debug_force_shading_shift_zero |= opts.debug_force_shading_shift_zero;
	mesh_diagnostics.debug_disable_matcap |= opts.debug_disable_matcap;
	mesh_diagnostics.debug_disable_emissive |= opts.debug_disable_emissive;
	mesh_diagnostics.debug_disable_shade_color |= opts.debug_disable_shade_color;
	mesh_diagnostics.debug_disable_normal_map |= opts.debug_disable_normal_map;
	mesh_diagnostics.debug_base_texture_only |= opts.debug_base_texture_only;
	mesh_diagnostics.skin_tone_matching |= opts.skin_tone_matching;
	mesh_diagnostics.mesh_cloth_assist = opts.dynamics_physics.mesh_cloth_assist.clone();
	mesh_diagnostics.mesh_cloth_assist_categories = opts.dynamics_physics.categories.clone();
	mesh_diagnostics
}

fn dynamics_deforming_node_indices_for_mesh_assist(
	runtime_model: un_avatar_core::UnaRuntimeModel<'_>,
	categories: &[un_avatar_skeleton::DynamicsCategoryDefinition],
) -> Vec<usize> {
	let Some(runtime) = runtime_model.scene_profile_dynamics() else {
		return Vec::new();
	};
	let mut out = Vec::new();
	let dynamics = runtime.dynamics;
	for group in dynamics.dynamics_groups() {
		if !group.effective_enabled || !dynamics.source_id_resident_in_scene(runtime.scene, group.source_id) {
			continue;
		}
		if classify_dynamics_group_category(runtime.scene, group, categories) != "cloth" {
			continue;
		}
		let chain = group.chain.bone_node_indices;
		if chain.len() < 3 {
			continue;
		}
		out.extend(dynamics_mesh_cloth_assist_deforming_nodes(
			chain,
			group.chain.interaction_start_index,
		));
	}
	out.sort_unstable();
	out.dedup();
	out
}

fn startup_texture_target_size_for_window_options(opts: &AvatarWindowOptions) -> (u32, u32) {
	if opts.spout.enabled {
		(
			opts.spout.width.unwrap_or(opts.window_width).max(1),
			opts.spout.height.unwrap_or(opts.window_height).max(1),
		)
	} else {
		(opts.window_width.max(1), opts.window_height.max(1))
	}
}

#[derive(Clone, Copy)]
pub(crate) enum GpuSceneWarmupPurpose {
	Benchmark,
	PrewarmSceneCache,
}

impl GpuSceneWarmupPurpose {
	fn label(self) -> &'static str {
		match self {
			Self::Benchmark => "gpu scene benchmark",
			Self::PrewarmSceneCache => "scene cache prewarm",
		}
	}
}

pub(crate) fn warmup_gpu_scene_startup(opts: &AvatarWindowOptions, purpose: GpuSceneWarmupPurpose) -> Result<(), String> {
	let Some(path) = opts.gltf_path.as_deref() else {
		return Err(format!("{}: --gltf or manifest avatar_path is required", purpose.label()));
	};
	let label = purpose.label();
	let started = Instant::now();
	let import_started = Instant::now();
	let document = model_loader::load_document_profiled(
		path,
		opts.wardrobe_set.as_deref(),
		&opts.animator_action_ids,
		&opts.animator_action_values,
		opts.contact_parameter_emission,
		opts.processed_texture_cache,
	)
	.map_err(|e| format!("{label}: model import failed: {}: {e}", path.display()))?;
	eprintln!(
		"un-avatar-renderer: {label} import path={} elapsed={:.1}ms",
		path.display(),
		import_started.elapsed().as_secs_f64() * 1000.0
	);

	let requested_render_backend = opts.render_backend;
	let render_backend = effective_window_backend(requested_render_backend, opts.transparent);
	log_effective_window_backend(requested_render_backend, render_backend, opts.transparent);
	let instance_descriptor = instance_descriptor_for_backend(render_backend);
	let backends = instance_descriptor.backends;
	let instance = wgpu::Instance::new(instance_descriptor);
	let adapter_started = Instant::now();
	let adapter = resolve_adapter(&instance, backends, None, opts.gpu_adapter.as_deref(), label).map_err(|e| format!("{label}: {e}"))?;
	let adapter_limits = adapter.limits();
	let mesh_shader_plan = mesh_shader_resource_plan_for_adapter(&adapter_limits);
	let adapter_features = adapter.features();
	let texture_compression_features = if matches!(
		opts.texture_compression,
		TextureCompressionMode::Source | TextureCompressionMode::Compat
	) {
		wgpu::Features::empty()
	} else {
		adapter_features
			& (wgpu::Features::TEXTURE_COMPRESSION_BC | wgpu::Features::TEXTURE_COMPRESSION_ASTC | wgpu::Features::TEXTURE_COMPRESSION_ETC2)
	};
	let pipeline_cache_features = adapter_features & wgpu::Features::PIPELINE_CACHE;
	let required_features =
		texture_compression_features | (adapter_features & wgpu::Features::TEXTURE_FORMAT_16BIT_NORM) | pipeline_cache_features;
	let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
		label: Some("un-avatar-renderer-gpu-scene-benchmark"),
		required_features,
		required_limits: mesh_shader_plan.required_limits,
		memory_hints: Default::default(),
		..Default::default()
	}))
	.map_err(|e| format!("{label}: request_device: {e}"))?;
	device.on_uncaptured_error(Arc::new(|error| {
		eprintln!("un-avatar-renderer: uncaptured wgpu error: {error}");
	}));
	let pipeline_cache = PersistentPipelineCache::load(&device, &adapter.get_info());
	eprintln!(
		"un-avatar-renderer: {label} adapter/device backend={render_backend:?} tier={:?} elapsed={:.1}ms",
		mesh_shader_plan.tier,
		adapter_started.elapsed().as_secs_f64() * 1000.0
	);

	let (target_width, target_height) = startup_texture_target_size_for_window_options(opts);
	let options = DocumentAttachOptions {
		mesh_diagnostics: scene_mesh_load_opts_for_window_options(opts),
		texture_max_dimension: opts.texture_resolution_limit.max_dimension(target_width, target_height),
		texture_compression: opts.texture_compression,
		block_compression_encoder: opts.block_compression_encoder,
		block_compression_cpu_threads: opts.block_compression_cpu_threads,
		mipmap_filter: opts.mipmap_filter,
		texture_compression_advanced: opts.texture_compression_advanced.clone(),
		texture_compression_bc_supported: cfg!(windows)
			&& !matches!(
				opts.texture_compression,
				TextureCompressionMode::Source | TextureCompressionMode::Compat
			),
		texture_compression_astc_supported: false,
		texture_compression_etc2_supported: false,
		processed_texture_cache: opts.processed_texture_cache,
		dynamics_enabled: opts.dynamics_enabled,
		bone_colliders: opts.bone_colliders,
		dynamics_physics: opts.dynamics_physics.clone(),
		debug_material_dump: opts.debug_material_dump,
		vmc_address: opts.vmc_address,
		unmotion_zenoh: opts.unmotion_zenoh.clone(),
		audio_link: opts.audio_link.clone(),
		debug_vmc: opts.debug.vmc,
	};
	let scene_started = Instant::now();
	let context = GpuSceneBuildContext {
		device,
		queue,
		format: wgpu::TextureFormat::Bgra8UnormSrgb,
		aa: opts.aa,
		shader_variant_tier: mesh_shader_plan.tier,
		pipeline_cache,
	};
	let prepared = context.prepare_document_scene(document, &options, |progress| {
		eprintln!(
			"un-avatar-renderer: {label} progress phase={} {}/{} {} ({:.1}ms)",
			progress.phase,
			progress.current,
			progress.total,
			progress.message,
			scene_started.elapsed().as_secs_f64() * 1000.0
		);
	})?;
	drop(prepared);
	eprintln!(
		"un-avatar-renderer: {label} scene elapsed={:.1}ms total={:.1}ms",
		scene_started.elapsed().as_secs_f64() * 1000.0,
		started.elapsed().as_secs_f64() * 1000.0
	);
	Ok(())
}

pub(crate) fn prewarm_shader_pipelines(opts: &AvatarWindowOptions) -> Result<(), String> {
	let started = Instant::now();
	let requested_render_backend = opts.render_backend;
	let render_backend = effective_window_backend(requested_render_backend, opts.transparent);
	log_effective_window_backend(requested_render_backend, render_backend, opts.transparent);
	let instance_descriptor = instance_descriptor_for_backend(render_backend);
	let backends = instance_descriptor.backends;
	let instance = wgpu::Instance::new(instance_descriptor);
	let adapter_started = Instant::now();
	let adapter = resolve_adapter(&instance, backends, None, opts.gpu_adapter.as_deref(), "shader prewarm")
		.map_err(|e| format!("shader prewarm: {e}"))?;
	let adapter_limits = adapter.limits();
	let mesh_shader_plan = mesh_shader_resource_plan_for_adapter(&adapter_limits);
	let adapter_features = adapter.features();
	let pipeline_cache_features = adapter_features & wgpu::Features::PIPELINE_CACHE;
	let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
		label: Some("un-avatar-renderer-shader-prewarm"),
		required_features: pipeline_cache_features,
		required_limits: mesh_shader_plan.required_limits,
		memory_hints: Default::default(),
		..Default::default()
	}))
	.map_err(|e| format!("shader prewarm: request_device: {e}"))?;
	device.on_uncaptured_error(Arc::new(|error| {
		eprintln!("un-avatar-renderer: uncaptured wgpu error during shader prewarm: {error}");
	}));
	let pipeline_cache = PersistentPipelineCache::load(&device, &adapter.get_info());
	eprintln!(
		"un-avatar-renderer: shader prewarm adapter/device backend={render_backend:?} tier={:?} elapsed={:.1}ms",
		mesh_shader_plan.tier,
		adapter_started.elapsed().as_secs_f64() * 1000.0
	);
	let sample_count = aa_sample_count(opts.aa);
	let pipeline_started = Instant::now();
	let summary = SceneMeshes::prewarm_standard_pipelines(
		&device,
		wgpu::TextureFormat::Bgra8UnormSrgb,
		sample_count,
		mesh_shader_plan.tier,
		pipeline_cache.cache(),
		|label| {
			eprintln!(
				"un-avatar-renderer: shader prewarm compiling {label} ({:.1}ms)",
				pipeline_started.elapsed().as_secs_f64() * 1000.0
			);
		},
	);
	queue.submit([]);
	device.poll(wgpu::PollType::wait_indefinitely()).ok();
	pipeline_cache.store();
	eprintln!(
		"un-avatar-renderer: shader prewarm complete shader_modules={} render_pipelines={} compute_pipelines={} pipeline_elapsed={:.1}ms total={:.1}ms",
		summary.shader_modules,
		summary.render_pipelines,
		summary.compute_pipelines,
		pipeline_started.elapsed().as_secs_f64() * 1000.0,
		started.elapsed().as_secs_f64() * 1000.0
	);
	Ok(())
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

fn motion_signal_runtime_parameter_value(signal: &un_motion_frame::MotionSignal) -> Option<f32> {
	if signal.state == un_motion_frame::SampleState::Missing {
		return None;
	}
	match signal.value {
		un_motion_frame::MotionSignalValue::Bool(value) => Some(if value { 1.0 } else { 0.0 }),
		un_motion_frame::MotionSignalValue::Scalar(value) if value.is_finite() => Some(value),
		_ => None,
	}
}

fn motion_signal_runtime_parameter_names(document: &UnaDocument) -> Vec<String> {
	let mut names = document
		.runtime_model()
		.runtime_parameter_definitions()
		.into_iter()
		.filter(|definition| {
			definition
				.source_kinds
				.iter()
				.any(|kind| matches!(kind.as_str(), "action_trigger" | "action_condition" | "modular_avatar_parameter"))
		})
		.map(|definition| definition.name)
		.collect::<Vec<_>>();
	names.sort_unstable();
	names.dedup();
	names
}

fn apply_motion_signal_runtime_parameters_with_names(
	document: &mut UnaDocument,
	frames: &[un_motion_frame::UNMotionFrame],
	parameter_names: &[String],
) -> BTreeMap<String, f32> {
	if parameter_names.is_empty() {
		return BTreeMap::new();
	}
	let before = document.runtime_model().runtime_parameter_values();
	let mut changed = BTreeMap::<String, f32>::new();
	for frame in frames {
		for signal in &frame.signals {
			if parameter_names
				.binary_search_by(|parameter_name| parameter_name.as_str().cmp(signal.name.as_str()))
				.is_err()
			{
				continue;
			}
			let Some(value) = motion_signal_runtime_parameter_value(signal) else {
				continue;
			};
			if before.get(&signal.name).copied() == Some(value) {
				changed.remove(&signal.name);
			} else {
				changed.insert(signal.name.clone(), value);
			}
		}
	}
	if !changed.is_empty() {
		let mut runtime = document.runtime_model_mut();
		for (name, value) in &changed {
			runtime.set_runtime_parameter_value(name.clone(), *value);
		}
	}
	changed
}

#[cfg(test)]
fn apply_motion_signal_runtime_parameters(document: &mut UnaDocument, frames: &[un_motion_frame::UNMotionFrame]) -> BTreeMap<String, f32> {
	let parameter_names = motion_signal_runtime_parameter_names(document);
	apply_motion_signal_runtime_parameters_with_names(document, frames, &parameter_names)
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

	fn signal_frame(
		sequence: u64,
		name: &str,
		value: un_motion_frame::MotionSignalValue,
		state: un_motion_frame::SampleState,
	) -> un_motion_frame::UNMotionFrame {
		let mut frame = un_motion_frame::UNMotionFrame::new(sequence);
		frame.header.coordinate_space = un_motion_frame::CoordinateSpace::UNMotion;
		frame.signals.push(un_motion_frame::MotionSignal {
			name: name.to_string(),
			value,
			confidence: 1.0,
			source_index: None,
			state,
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
	fn motion_signals_update_declared_runtime_parameters_only() {
		let mut document = UnaDocument {
			runtime_actions: Some(un_avatar_core::UnaRuntimeActionSet {
				actions: vec![un_avatar_core::UnaRuntimeAction {
					id: "coat:on".to_string(),
					triggers: vec![un_avatar_core::UnaRuntimeActionTrigger::ParameterValue {
						name: "Coat".to_string(),
						value: 1.0,
					}],
					..Default::default()
				}],
			}),
			..Default::default()
		};
		let frames = vec![
			signal_frame(
				1,
				"Coat",
				un_motion_frame::MotionSignalValue::Bool(true),
				un_motion_frame::SampleState::Valid,
			),
			signal_frame(
				1,
				"Unknown",
				un_motion_frame::MotionSignalValue::Scalar(1.0),
				un_motion_frame::SampleState::Valid,
			),
			signal_frame(
				1,
				"CoatVec",
				un_motion_frame::MotionSignalValue::Vec2(un_motion_frame::Vec2f { x: 1.0, y: 0.0 }),
				un_motion_frame::SampleState::Valid,
			),
		];

		let changed = apply_motion_signal_runtime_parameters(&mut document, &frames);

		assert_eq!(changed, BTreeMap::from([("Coat".to_string(), 1.0)]));
		assert_eq!(document.runtime_model().runtime_parameter_values().get("Coat"), Some(&1.0));
		assert!(!document.runtime_model().runtime_parameter_values().contains_key("Unknown"));
		assert!(!document.runtime_model().runtime_parameter_values().contains_key("CoatVec"));
	}

	#[test]
	fn motion_signals_ignore_physbone_suffix_and_missing_values() {
		let mut document = UnaDocument {
			spring_bones: Some(un_avatar_core::UnaSpringBoneSettings {
				groups: vec![un_avatar_core::UnaSpringBoneGroup {
					source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
					source_id: "physbone:hair".to_string(),
					interaction: Some(un_avatar_core::UnaDynamicsInteraction {
						parameter: "Hair".to_string(),
						..Default::default()
					}),
					..Default::default()
				}],
				..Default::default()
			}),
			runtime_actions: Some(un_avatar_core::UnaRuntimeActionSet {
				actions: vec![un_avatar_core::UnaRuntimeAction {
					id: "hat:on".to_string(),
					triggers: vec![un_avatar_core::UnaRuntimeActionTrigger::ParameterValue {
						name: "Hat".to_string(),
						value: 1.0,
					}],
					..Default::default()
				}],
			}),
			..Default::default()
		};
		let frames = vec![
			signal_frame(
				1,
				"Hair_IsGrabbed",
				un_motion_frame::MotionSignalValue::Bool(true),
				un_motion_frame::SampleState::Valid,
			),
			signal_frame(
				1,
				"Hat",
				un_motion_frame::MotionSignalValue::Scalar(1.0),
				un_motion_frame::SampleState::Missing,
			),
		];

		let changed = apply_motion_signal_runtime_parameters(&mut document, &frames);

		assert!(changed.is_empty());
		assert!(document.runtime_model().runtime_parameter_values().is_empty());
	}

	#[test]
	fn motion_signals_use_last_value_when_filtering_unchanged_parameters() {
		let mut document = UnaDocument {
			runtime_actions: Some(un_avatar_core::UnaRuntimeActionSet {
				actions: vec![un_avatar_core::UnaRuntimeAction {
					id: "coat:on".to_string(),
					triggers: vec![un_avatar_core::UnaRuntimeActionTrigger::ParameterValue {
						name: "Coat".to_string(),
						value: 1.0,
					}],
					..Default::default()
				}],
			}),
			..Default::default()
		};
		document.runtime_model_mut().set_runtime_parameter_value("Coat".to_string(), 0.0);
		let frames = vec![
			signal_frame(
				1,
				"Coat",
				un_motion_frame::MotionSignalValue::Scalar(1.0),
				un_motion_frame::SampleState::Valid,
			),
			signal_frame(
				2,
				"Coat",
				un_motion_frame::MotionSignalValue::Scalar(0.0),
				un_motion_frame::SampleState::Valid,
			),
		];

		let changed = apply_motion_signal_runtime_parameters(&mut document, &frames);

		assert!(changed.is_empty());
		assert_eq!(document.runtime_model().runtime_parameter_values().get("Coat"), Some(&0.0));
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

	#[test]
	fn reset_runtime_dynamics_nodes_to_rest_for_source_id_restores_translation_writeback_target() {
		let root_current = [1.0; 16];
		let mid_current = [2.0; 16];
		let stretched_tip = [3.0; 16];
		let other_current = [4.0; 16];
		let rest = [5.0; 16];
		let mut scene = un_avatar_core::UnaSceneSnapshot {
			nodes: vec![
				test_node(root_current),
				test_node(mid_current),
				test_node(stretched_tip),
				test_node(other_current),
			],
			..Default::default()
		};
		let rest_nodes = vec![test_node(rest), test_node(rest), test_node(rest), test_node(rest)];
		let settings = un_avatar_core::UnaSpringBoneSettings {
			groups: vec![
				un_avatar_core::UnaSpringBoneGroup {
					source_id: "physbone:hair".to_string(),
					bone_node_indices: vec![0, 1, 2],
					writeback_mode: un_avatar_core::UnaDynamicsWritebackMode::RotationTranslation,
					..Default::default()
				},
				un_avatar_core::UnaSpringBoneGroup {
					source_id: "physbone:tail".to_string(),
					bone_node_indices: vec![3],
					..Default::default()
				},
			],
			..Default::default()
		};

		assert!(reset_runtime_dynamics_nodes_to_rest_for_source_id(
			&mut scene,
			settings.runtime_dynamics(),
			&rest_nodes,
			"physbone:hair",
		));
		assert_eq!(scene.nodes[0].transform, rest);
		assert_eq!(scene.nodes[1].transform, rest);
		assert_eq!(scene.nodes[2].transform, rest);
		assert_eq!(scene.nodes[3].transform, other_current);
	}

	#[test]
	fn restored_dynamics_source_ids_are_unique_and_source_scoped() {
		let restored = vec![
			un_avatar_core::UnaEvaluationRestoreApplyEntry {
				owner_key: "action:hair:on".to_string(),
				action_id: "hair:on".to_string(),
				condition_state: Some("inactive".to_string()),
				target_kind: UnaEvaluationTargetKind::DynamicsEnabled,
				target_key: "physbone:hair".to_string(),
				baseline_value: Some(Value::Bool(true)),
				current_value_available: true,
				current_value: Some(Value::Bool(false)),
				ready: true,
				reason: "ready".to_string(),
			},
			un_avatar_core::UnaEvaluationRestoreApplyEntry {
				owner_key: "action:hair:off".to_string(),
				action_id: "hair:off".to_string(),
				condition_state: Some("inactive".to_string()),
				target_kind: UnaEvaluationTargetKind::DynamicsEnabled,
				target_key: "physbone:hair".to_string(),
				baseline_value: Some(Value::Bool(false)),
				current_value_available: true,
				current_value: Some(Value::Bool(true)),
				ready: true,
				reason: "ready".to_string(),
			},
			un_avatar_core::UnaEvaluationRestoreApplyEntry {
				owner_key: "action:hat:on".to_string(),
				action_id: "hat:on".to_string(),
				condition_state: Some("inactive".to_string()),
				target_kind: UnaEvaluationTargetKind::NodeVisibility,
				target_key: "node:hat".to_string(),
				baseline_value: Some(Value::Bool(true)),
				current_value_available: true,
				current_value: Some(Value::Bool(false)),
				ready: true,
				reason: "ready".to_string(),
			},
			un_avatar_core::UnaEvaluationRestoreApplyEntry {
				owner_key: "action:tail:on".to_string(),
				action_id: "tail:on".to_string(),
				condition_state: Some("inactive".to_string()),
				target_kind: UnaEvaluationTargetKind::DynamicsEnabled,
				target_key: "physbone:tail".to_string(),
				baseline_value: Some(Value::Bool(true)),
				current_value_available: true,
				current_value: Some(Value::Bool(false)),
				ready: true,
				reason: "ready".to_string(),
			},
		];

		assert_eq!(
			restored_dynamics_source_ids(&restored),
			vec!["physbone:hair".to_string(), "physbone:tail".to_string()]
		);
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

pub(crate) struct WardrobeBillboardCamera {
	pub(crate) center: [f32; 3],
	pub(crate) size: f32,
	pub(crate) view_proj: [[f32; 4]; 4],
	pub(crate) camera_pos: [f32; 3],
}

impl CameraStateSnapshot {
	pub(crate) fn fallback_wardrobe_billboard_center(self) -> [f32; 3] {
		let target = Vec3::from_array(self.target);
		let radius = self.radius.max(0.05);
		(target + Vec3::Y * (radius * 0.04).clamp(0.04, 0.14)).to_array()
	}

	pub(crate) fn wardrobe_billboard_camera(self, aspect_wh: f32, center: [f32; 3]) -> WardrobeBillboardCamera {
		let target = Vec3::from_array(self.target);
		let lon = self.longitude_deg.to_radians();
		let lat = self.latitude_deg.to_radians();
		let cos_lat = lat.cos();
		let radius = self.radius.max(0.05);
		let camera_pos = target + Vec3::new(radius * cos_lat * lon.sin(), radius * lat.sin(), -radius * cos_lat * lon.cos());
		let aspect = aspect_wh.max(0.01);
		let fovy = vertical_fov_from_diagonal(self.diagonal_fov_deg.to_radians(), aspect);
		let view_proj =
			Mat4::perspective_rh(fovy, aspect, CAMERA_NEAR_CLIP_M, CAMERA_FAR_CLIP_M) * Mat4::look_at_rh(camera_pos, target, Vec3::Y);
		let size = (radius * 0.12).clamp(0.16, 0.34);
		WardrobeBillboardCamera {
			center,
			size,
			view_proj: view_proj.to_cols_array_2d(),
			camera_pos: camera_pos.to_array(),
		}
	}
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

struct ContactShadowResources {
	bind_group_layout: wgpu::BindGroupLayout,
	buffer: wgpu::Buffer,
	bind_group: wgpu::BindGroup,
}

pub(crate) struct GpuState {
	pub(crate) surface: wgpu::Surface<'static>,
	pub(crate) device: wgpu::Device,
	pub(crate) queue: wgpu::Queue,
	pub(crate) config: wgpu::SurfaceConfiguration,
	pipeline_cache: PersistentPipelineCache,
	alpha_modes: Vec<wgpu::CompositeAlphaMode>,
	depth_texture: wgpu::Texture,
	depth_view: wgpu::TextureView,
	uniform_buffer: wgpu::Buffer,
	globals_uploaded: Option<GlobalsGpu>,
	bind_group_layout: wgpu::BindGroupLayout,
	bind_group: wgpu::BindGroup,
	pipeline: wgpu::RenderPipeline,
	axes_pipeline: Option<wgpu::RenderPipeline>,
	bone_collider_pipeline: Option<wgpu::RenderPipeline>,
	bone_collider_vertex_buffer: Option<wgpu::Buffer>,
	bone_collider_vertex_capacity: usize,
	bone_collider_vertex_count: u32,
	bone_collider_vertices: Vec<DebugLineVertex>,
	startup_progress_overlay_pipeline: wgpu::RenderPipeline,
	startup_progress_overlay_buffer: wgpu::Buffer,
	startup_progress_overlay_bind_group: wgpu::BindGroup,
	wardrobe_billboard_pipeline: wgpu::RenderPipeline,
	wardrobe_billboard_buffer: wgpu::Buffer,
	wardrobe_billboard_bind_group: wgpu::BindGroup,
	contact_shadow_resources: Option<ContactShadowResources>,
	contact_shadow_pipeline: Option<wgpu::RenderPipeline>,
	document: Option<Arc<RwLock<UnaDocument>>>,
	document_revision: Arc<AtomicU64>,
	applied_document_revision: u64,
	scene_pose_dirty: bool,
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
	last_draw_doc_lock_ms: f32,
	last_draw_expression_select_ms: f32,
	last_draw_update_total_ms: f32,
	last_scene_world_ms: f32,
	last_draw_transform_timings: DrawTransformUpdateTimings,
	audio_link_options: AudioLinkOptions,
	audio_link_runtime: Option<crate::audio_link::AudioLinkInputRuntime>,
	dynamics_sim: Option<DynamicsSimulator>,
	dynamics_profile_enabled: bool,
	last_dynamics_profile: DynamicsStepProfile,
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
	#[cfg(windows)]
	spout_unavailable_logged: bool,
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
	animator_morph_override_cache: AnimatorMorphOverrideCache,
	expression_presets: Box<[String]>,
	motion_apply_opts: un_avatar_skeleton::ApplyUnMotionFrameOpts,
	motion_buffer: Arc<MotionControlBuffer>,
	pending_motion_frames: Vec<un_motion_frame::UNMotionFrame>,
	motion_runtime_parameter_names: Box<[String]>,
	runtime_scene_node_paths_by_index: Box<[Option<String>]>,
	runtime_center_peak_angle_parameters: Box<[String]>,
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
		gpu_adapter: Option<&str>,
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

		let requested_render_backend = render_backend;
		let render_backend = effective_window_backend(requested_render_backend, transparent);
		log_effective_window_backend(requested_render_backend, render_backend, transparent);
		let instance_descriptor = instance_descriptor_for_backend(render_backend);
		let backends = instance_descriptor.backends;
		let instance = wgpu::Instance::new(instance_descriptor);

		let surface: wgpu::Surface<'static> = instance.create_surface(window).map_err(|e| format!("create_surface: {e}"))?;

		let adapter = resolve_adapter(&instance, backends, Some(&surface), gpu_adapter, "window startup")?;

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
		let pipeline_cache_features = adapter_features & wgpu::Features::PIPELINE_CACHE;
		let required_features = texture_compression_features | timestamp_features | texture_format_features | pipeline_cache_features;

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
		let pipeline_cache = PersistentPipelineCache::load(&device, &adapter.get_info());

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
		let bone_collider_pipeline = None;
		let startup_progress_overlay_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("startup_progress_overlay"),
			entries: &[wgpu::BindGroupLayoutEntry {
				binding: 0,
				visibility: wgpu::ShaderStages::FRAGMENT,
				ty: wgpu::BindingType::Buffer {
					ty: wgpu::BufferBindingType::Uniform,
					has_dynamic_offset: false,
					min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<StartupProgressOverlayGpu>() as u64),
				},
				count: None,
			}],
		});
		let startup_progress_overlay_pipeline =
			create_startup_progress_overlay_pipeline(&device, &startup_progress_overlay_bind_group_layout, format, aa_sample_count);
		let startup_progress_overlay_buffer = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("startup_progress_overlay"),
			size: std::mem::size_of::<StartupProgressOverlayGpu>() as u64,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		let startup_progress_overlay_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("startup_progress_overlay"),
			layout: &startup_progress_overlay_bind_group_layout,
			entries: &[wgpu::BindGroupEntry {
				binding: 0,
				resource: startup_progress_overlay_buffer.as_entire_binding(),
			}],
		});
		let wardrobe_billboard_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("wardrobe_billboard"),
			entries: &[wgpu::BindGroupLayoutEntry {
				binding: 0,
				visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
				ty: wgpu::BindingType::Buffer {
					ty: wgpu::BufferBindingType::Uniform,
					has_dynamic_offset: false,
					min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<WardrobeBillboardGpu>() as u64),
				},
				count: None,
			}],
		});
		let wardrobe_billboard_pipeline = create_wardrobe_billboard_pipeline(
			&device,
			&bind_group_layout,
			&wardrobe_billboard_bind_group_layout,
			format,
			aa_sample_count,
		);
		let wardrobe_billboard_buffer = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("wardrobe_billboard"),
			size: std::mem::size_of::<WardrobeBillboardGpu>() as u64,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		let wardrobe_billboard_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("wardrobe_billboard"),
			layout: &wardrobe_billboard_bind_group_layout,
			entries: &[wgpu::BindGroupEntry {
				binding: 0,
				resource: wardrobe_billboard_buffer.as_entire_binding(),
			}],
		});
		let texture_summary = None;
		let avatar_outline = mesh_diagnostics.avatar_outline;
		let scene_meshes = None;
		let dynamics_sim = None;
		let dynamics_profile_enabled = std::env::var_os("UN_AVATAR_DYNAMICS_PROFILE").is_some();
		let contact_shadow_pipeline = None;
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
		let spout = None;
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
			pipeline_cache,
			alpha_modes: caps.alpha_modes,
			depth_texture,
			depth_view,
			uniform_buffer,
			globals_uploaded: None,
			bind_group_layout,
			bind_group,
			pipeline,
			axes_pipeline: None,
			bone_collider_pipeline,
			bone_collider_vertex_buffer: None,
			bone_collider_vertex_capacity: 0,
			bone_collider_vertex_count: 0,
			bone_collider_vertices: Vec::new(),
			startup_progress_overlay_pipeline,
			startup_progress_overlay_buffer,
			startup_progress_overlay_bind_group,
			wardrobe_billboard_pipeline,
			wardrobe_billboard_buffer,
			wardrobe_billboard_bind_group,
			contact_shadow_resources: None,
			contact_shadow_pipeline,
			document: None,
			document_revision,
			applied_document_revision: 0,
			scene_pose_dirty: false,
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
			last_draw_doc_lock_ms: 0.0,
			last_draw_expression_select_ms: 0.0,
			last_draw_update_total_ms: 0.0,
			last_scene_world_ms: 0.0,
			last_draw_transform_timings: DrawTransformUpdateTimings::default(),
			audio_link_options: AudioLinkOptions::default(),
			audio_link_runtime: None,
			dynamics_sim,
			dynamics_profile_enabled,
			last_dynamics_profile: DynamicsStepProfile::default(),
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
			#[cfg(windows)]
			spout_unavailable_logged: false,
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
			animator_morph_override_cache: AnimatorMorphOverrideCache::default(),
			expression_presets: Box::default(),
			motion_apply_opts,
			motion_buffer,
			pending_motion_frames: Vec::new(),
			motion_runtime_parameter_names: Box::default(),
			runtime_scene_node_paths_by_index: Box::default(),
			runtime_center_peak_angle_parameters: Box::default(),
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
		let mut counts: DynamicsRuntimeCounts = doc.runtime_model().dynamics().counts().into();
		counts.surface_constraints = self.dynamics_sim.as_ref().map_or(0, |sim| sim.surface_constraint_count() as u32);
		counts
	}

	pub(crate) fn scene_node_constraint_counts(&self) -> SceneNodeConstraintCounts {
		let Some(doc_arc) = self.document.as_ref() else {
			return SceneNodeConstraintCounts::default();
		};
		let Ok(doc) = doc_arc.read() else {
			return SceneNodeConstraintCounts::default();
		};
		let Some(scene) = doc.scene.as_ref() else {
			return SceneNodeConstraintCounts::default();
		};
		scene_node_constraint_counts(scene)
	}

	fn refresh_scene_draw_state(&mut self, document_revision_to_apply: Option<u64>) -> bool {
		let (Some(sm), Some(doc_arc)) = (&mut self.scene_meshes, &self.document) else {
			return false;
		};
		let t_doc_lock0 = Instant::now();
		let Ok(doc) = doc_arc.read() else {
			return false;
		};
		self.last_draw_doc_lock_ms = t_doc_lock0.elapsed().as_secs_f32() * 1000.0;
		let runtime_model = doc.runtime_model();
		let Some(runtime) = runtime_model.scene_expression_catalog() else {
			return false;
		};
		let t_world0 = Instant::now();
		crate::scene_transform::write_world_from_nodes(runtime.scene, &mut self.world_scratch);
		self.last_scene_world_ms = t_world0.elapsed().as_secs_f32() * 1000.0;
		let document_changed = document_revision_to_apply.is_some_and(|revision| revision != self.applied_document_revision);
		if document_changed && !expression_presets_match_catalog(&self.expression_presets, runtime.expression_catalog) {
			self.expression_presets = expression_preset_names(runtime.expression_catalog).into_boxed_slice();
		}
		let refresh_scene_morph_defaults = document_changed;
		let t_expr0 = Instant::now();
		let expr_weights = active_expression_weights_for_doc(self.disable_expression_morphs, &doc);
		let expression_overrides = active_expression_overrides(self.disable_expression_morphs, &self.expression_overrides);
		let active_document_revision = document_revision_to_apply.unwrap_or(self.applied_document_revision);
		let animator_morph_overrides = if self.disable_expression_morphs {
			None
		} else {
			let runtime_parameter_values = runtime_model.runtime_parameter_values();
			let cache = &mut self.animator_morph_override_cache;
			if !cache.valid || cache.document_revision != active_document_revision || cache.parameter_values != *runtime_parameter_values {
				cache.overrides = animator_morph_overrides_for_doc(&doc);
				cache.parameter_values.clear();
				cache
					.parameter_values
					.extend(runtime_parameter_values.iter().map(|(key, value)| (key.clone(), *value)));
				cache.document_revision = active_document_revision;
				cache.valid = true;
			}
			(!cache.overrides.is_empty()).then_some(&cache.overrides)
		};
		self.last_draw_expression_select_ms = t_expr0.elapsed().as_secs_f32() * 1000.0;
		if document_changed {
			sm.refresh_draw_visibility_from_scene(runtime.scene);
			sm.refresh_draw_materials_from_scene(&self.device, &self.queue, runtime.scene);
			let mut residency_refresh = sm.refresh_asset_group_residency_with_changes(runtime.scene, runtime_model.active_asset_groups());
			let visible_residency_promotions = sm.promote_visible_draw_residency();
			if !visible_residency_promotions.is_empty() {
				residency_refresh
					.mesh_buffer_load_indices
					.extend(visible_residency_promotions.iter().copied());
				residency_refresh.mesh_buffer_load_indices.sort_unstable();
				residency_refresh.mesh_buffer_load_indices.dedup();
				residency_refresh
					.mesh_buffer_unload_indices
					.retain(|index| visible_residency_promotions.binary_search(index).is_err());
				if self.debug_log.is_enabled() {
					self.debug_log.line(
						"wardrobe",
						format!(
							"visible draw residency promoted count={} draws={:?}",
							visible_residency_promotions.len(),
							visible_residency_promotions
						),
					);
				}
			}
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
				runtime.scene,
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
			let image_load_indices = sorted_unique_index_union(
				&self.last_asset_residency_refresh.image_texture_load_indices,
				&active_gaps.inactive_image_texture_indices,
			);
			let image_unload_indices =
				sorted_index_difference(&self.last_asset_residency_refresh.image_texture_unload_indices, &image_load_indices);
			let cube_load_indices = sorted_unique_index_union(
				&self.last_asset_residency_refresh.cube_texture_load_indices,
				&active_gaps.inactive_cube_texture_indices,
			);
			let cube_unload_indices =
				sorted_index_difference(&self.last_asset_residency_refresh.cube_texture_unload_indices, &cube_load_indices);
			sm.promote_image_texture_residency(&image_load_indices);
			sm.promote_cube_texture_residency(&cube_load_indices);
			let (image_texture_bind_load_count, image_texture_bind_unload_count, cubemap_load_count, cubemap_unload_count) = sm
				.apply_image_texture_view_residency(
					&self.device,
					&self.queue,
					runtime.scene,
					&image_load_indices,
					&image_unload_indices,
					&cube_load_indices,
					&cube_unload_indices,
				);
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
			let ensured_draw_resources = sm.ensure_active_draw_gpu_resources(&self.device, &self.queue, runtime.scene);
			if ensured_draw_resources > 0 && self.debug_log.is_enabled() {
				self.debug_log.line(
					"wardrobe",
					format!("active draw gpu resources ensured count={ensured_draw_resources}"),
				);
			}
		}
		let t_update0 = Instant::now();
		self.last_draw_transform_timings = sm.update_draw_transforms(
			&self.queue,
			runtime.scene,
			&self.world_scratch,
			expr_weights,
			expression_overrides,
			animator_morph_overrides,
			refresh_scene_morph_defaults,
		);
		self.last_draw_update_total_ms = t_update0.elapsed().as_secs_f32() * 1000.0;
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
		self.scene_pose_dirty = true;
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
		if enabled {
			self.ensure_axes_pipeline();
		}
	}

	pub fn set_show_bone_colliders(&mut self, enabled: bool) {
		self.show_bone_colliders = enabled;
	}

	pub fn reconfigure_dynamics(
		&mut self,
		enabled: bool,
		bone_collider_config: BoneColliderConfig,
		dynamics_physics: DynamicsPhysicsConfig,
	) {
		self.runtime_dynamics_enabled = enabled;
		self.runtime_bone_collider_config = bone_collider_config;
		self.runtime_dynamics_physics = dynamics_physics;
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
		let resources = self
			.contact_shadow_resources
			.as_ref()
			.expect("contact shadow resources are initialized");
		self.queue.write_buffer(
			&resources.buffer,
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

	fn ensure_bone_collider_pipeline(&mut self) {
		if self.bone_collider_pipeline.is_none() {
			let sample_count = aa_sample_count(self.aa);
			self.bone_collider_pipeline = Some(create_bone_collider_pipeline(
				&self.device,
				&self.bind_group_layout,
				self.config.format,
				sample_count,
			));
		}
	}

	fn ensure_axes_pipeline(&mut self) {
		if self.axes_pipeline.is_none() {
			let sample_count = aa_sample_count(self.aa);
			self.axes_pipeline = Some(create_axes_pipeline(
				&self.device,
				&self.bind_group_layout,
				self.config.format,
				sample_count,
			));
		}
	}

	fn ensure_contact_shadow_resources(&mut self) {
		if self.contact_shadow_resources.is_some() {
			return;
		}
		let bind_group_layout = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
		let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("contact_shadow"),
			size: std::mem::size_of::<ContactShadowGpu>() as u64,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("contact_shadow"),
			layout: &bind_group_layout,
			entries: &[wgpu::BindGroupEntry {
				binding: 0,
				resource: buffer.as_entire_binding(),
			}],
		});
		self.contact_shadow_resources = Some(ContactShadowResources {
			bind_group_layout,
			buffer,
			bind_group,
		});
	}

	fn ensure_contact_shadow_pipeline(&mut self) {
		self.ensure_contact_shadow_resources();
		if self.contact_shadow_pipeline.is_none() {
			let sample_count = aa_sample_count(self.aa);
			let resources = self
				.contact_shadow_resources
				.as_ref()
				.expect("contact shadow resources are initialized");
			self.contact_shadow_pipeline = Some(create_contact_shadow_pipeline(
				&self.device,
				&self.bind_group_layout,
				&resources.bind_group_layout,
				self.config.format,
				sample_count,
			));
		}
	}

	fn draw_contact_shadow(&self, pass: &mut wgpu::RenderPass<'_>) {
		pass.set_pipeline(
			self.contact_shadow_pipeline
				.as_ref()
				.expect("contact shadow pipeline is initialized"),
		);
		pass.set_bind_group(0, &self.bind_group, &[]);
		pass.set_bind_group(
			1,
			&self
				.contact_shadow_resources
				.as_ref()
				.expect("contact shadow resources are initialized")
				.bind_group,
			&[],
		);
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
		if draw_contact_shadow {
			self.ensure_contact_shadow_pipeline();
		}
		if self.show_axes {
			self.ensure_axes_pipeline();
		}
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
					stencil_ops: Some(stencil_clear_ops()),
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
					stencil_ops: Some(stencil_load_ops()),
				}),
				timestamp_writes: None,
				occlusion_query_set: None,
				multiview_mask: None,
			});
			if let Some(sm) = &self.scene_meshes {
				sm.draw_blended_after_screen_refraction(&mut pass);
			}
			if self.show_axes {
				pass.set_pipeline(self.axes_pipeline.as_ref().expect("axes pipeline is initialized"));
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
					stencil_ops: Some(stencil_clear_ops()),
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
				pass.set_pipeline(self.axes_pipeline.as_ref().expect("axes pipeline is initialized"));
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
						stencil_ops: Some(stencil_load_ops()),
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
				self.spout_unavailable_logged = false;
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
			self.spout_unavailable_logged = self.spout.is_none();
			if self.spout_unavailable_logged {
				log_spout_unavailable();
			}
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

	#[cfg(windows)]
	fn ensure_runtime_spout_output(&mut self) -> bool {
		if self.spout.is_some() {
			return true;
		}
		let Some(launch) = self.spout_launch.clone() else {
			return false;
		};
		self.spout = crate::spout::SpoutCapture::try_new(&self.device, self.config.format, self.config.width, self.config.height, launch);
		if self.spout.is_none() && !self.spout_unavailable_logged {
			self.spout_unavailable_logged = true;
			log_spout_unavailable();
		}
		self.spout.is_some()
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

	pub(crate) fn base_wardrobe_set(&self) -> Option<String> {
		let doc_arc = self.document.as_ref()?;
		let doc = doc_arc.read().ok()?;
		model_loader::base_wardrobe_set_id(&doc)
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
		plan.pending_cube_texture_upload_count = scoped_upload_work.cube_texture_indices.len();
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

	pub(crate) fn document_arc(&self) -> Option<Arc<RwLock<UnaDocument>>> {
		self.document.clone()
	}

	pub(crate) fn wardrobe_billboard_anchor_world(&self, saved_camera: CameraStateSnapshot, anchor: &str, y_offset_m: f32) -> [f32; 3] {
		let fallback = saved_camera.fallback_wardrobe_billboard_center();
		let Some(doc_arc) = self.document.as_ref() else {
			return fallback;
		};
		let Ok(doc) = doc_arc.read() else {
			return fallback;
		};
		let (Some(scene), Some(profile)) = (doc.scene.as_ref(), doc.humanoid_profile.as_ref()) else {
			return fallback;
		};
		let keys = match anchor.trim().to_ascii_lowercase().as_str() {
			"head" => &["head", "neck", "upperchest", "chest"][..],
			"spine" => &["upperchest", "chest", "spine", "hips", "neck"][..],
			_ => &["neck", "upperchest", "chest", "spine", "head"][..],
		};
		let Some(anchor_node) = humanoid_node_index(profile, keys) else {
			return fallback;
		};
		let world = crate::scene_transform::scene_world_matrices(scene);
		let Some(anchor_world) = world.get(anchor_node) else {
			return fallback;
		};
		let position = anchor_world.transform_point3(Vec3::ZERO);
		if !position.is_finite() {
			return fallback;
		}
		let up = anchor_world.transform_vector3(Vec3::Y).try_normalize().unwrap_or(Vec3::Y);
		(position + up * y_offset_m.clamp(-1.0, 1.0)).to_array()
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

	pub(crate) fn dump_runtime_state(&self, path: &std::path::Path) -> Result<(), String> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Err("document is not loaded".to_string());
		};
		let doc = doc_arc.read().map_err(|_| "document lock poisoned".to_string())?;
		let parameter_values = doc.runtime_model().runtime_parameter_values().clone();
		let morph_overrides = animator_morph_overrides_for_doc(&doc);
		let dynamics_interaction_parameters =
			dynamics_interaction_parameter_diagnostics(&doc, self.rest_nodes.as_deref().map(Vec::as_slice));
		let dynamics_groups = dynamics_group_statuses_with_limit(&doc, &self.runtime_dynamics_physics.categories, None);
		let dynamics_colliders = dynamics_collider_statuses_with_limit(&doc, None);
		let dynamics_response_categories = self.dynamics_response_categories();
		let dynamics_response_groups = self.dynamics_response_groups();
		let (node_paths_by_index, scene_world) = doc
			.runtime_model()
			.scene()
			.map(|scene| (scene_node_paths_by_index(scene), scene_world_matrices(scene)))
			.unwrap_or_default();
		let dynamics_surface_constraints = doc
			.runtime_model()
			.scene_profile_dynamics()
			.map(|runtime| {
				let constraints = if self.runtime_dynamics_physics.surface_constraints_enabled {
					build_dynamics_surface_constraints(runtime.scene, runtime.dynamics, &self.runtime_dynamics_physics)
				} else {
					Vec::new()
				};
				dynamics_surface_constraint_statuses(runtime.scene, &constraints)
			})
			.unwrap_or_default();
		let dynamics_collider_selections = self
			.dynamics_sim
			.as_ref()
			.map(|sim| dynamics_collider_selection_statuses(sim, &node_paths_by_index, &scene_world))
			.unwrap_or_default();
		let dynamics_tail_samples = self.dynamics_sim.as_ref().map(DynamicsSimulator::tail_samples).unwrap_or_default();
		let dynamics_collider_contacts = dynamics_collider_contact_statuses(&dynamics_tail_samples, &dynamics_collider_selections);
		let dynamics_collider_contact_summaries = dynamics_collider_contact_summary_statuses(&dynamics_collider_contacts);
		let dynamics_collider_runtime_summaries =
			dynamics_collider_runtime_summary_statuses(&dynamics_collider_selections, &dynamics_collider_contact_summaries);
		let dynamics_collider_path_contact_summaries = dynamics_collider_path_contact_summary_statuses(&dynamics_collider_contacts);
		let dynamics_collider_path_candidate_summaries =
			dynamics_collider_path_candidate_summary_statuses(&dynamics_tail_samples, &dynamics_collider_selections);
		let dynamics_collider_path_runtime_summaries = dynamics_collider_path_runtime_summary_statuses(
			&dynamics_colliders,
			&dynamics_collider_path_contact_summaries,
			&dynamics_collider_path_candidate_summaries,
			&self.last_dynamics_profile.collision_projection_collider_path_counts,
		);
		let dynamics_node_samples = doc
			.scene
			.as_ref()
			.map(|scene| runtime_dynamics_node_samples(scene, self.rest_nodes.as_deref().map(Vec::as_slice)))
			.unwrap_or_default();
		let dynamic_node_indices = doc
			.runtime_model()
			.scene_profile_dynamics()
			.map(|runtime| {
				let mut indices = runtime.dynamics.dynamic_bone_node_indices().collect::<Vec<_>>();
				indices.sort_unstable();
				indices.dedup();
				indices
			})
			.unwrap_or_default();
		let skin_joint_samples = doc
			.scene
			.as_ref()
			.map(|scene| skin_joint_samples(scene, self.rest_nodes.as_deref().map(Vec::as_slice), &dynamic_node_indices))
			.unwrap_or_default();
		let visible_nonzero_morph_weights = doc
			.scene
			.as_ref()
			.map(|scene| visible_nonzero_morph_weights(scene, 512))
			.unwrap_or_default();
		let morph_draws = match (self.scene_meshes.as_ref(), doc.scene.as_ref()) {
			(Some(meshes), Some(scene)) => meshes.diagnostic_morph_state(scene, None, 64),
			_ => serde_json::json!({ "draws": [] }),
		};
		let value = serde_json::json!({
			"runtime_parameters": parameter_values,
			"dynamics_interaction_parameters": dynamics_interaction_parameters,
			"dynamics_groups": dynamics_groups,
			"dynamics_colliders": dynamics_colliders,
			"dynamics_last_profile": self.last_dynamics_profile,
			"dynamics_response_categories": dynamics_response_categories,
			"dynamics_response_groups": dynamics_response_groups,
			"dynamics_collider_selections": dynamics_collider_selections,
			"dynamics_collider_contacts": dynamics_collider_contacts,
			"dynamics_collider_contact_summaries": dynamics_collider_contact_summaries,
			"dynamics_collider_runtime_summaries": dynamics_collider_runtime_summaries,
			"dynamics_collider_path_contact_summaries": dynamics_collider_path_contact_summaries,
			"dynamics_collider_path_candidate_summaries": dynamics_collider_path_candidate_summaries,
			"dynamics_collider_path_runtime_summaries": dynamics_collider_path_runtime_summaries,
			"dynamics_tail_samples": dynamics_tail_samples,
			"dynamics_surface_constraints": dynamics_surface_constraints,
			"dynamics_node_samples": dynamics_node_samples,
			"skin_joint_samples": skin_joint_samples,
			"visible_nonzero_morph_weights": visible_nonzero_morph_weights,
			"animator_morph_overrides": morph_overrides,
			"morph_draws": morph_draws,
		});
		let bytes = serde_json::to_vec_pretty(&value).map_err(|err| err.to_string())?;
		std::fs::write(path, bytes).map_err(|err| err.to_string())
	}

	pub(crate) fn runtime_action_expression_weights(&self, action_id: &str) -> Vec<(String, f32)> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Vec::new();
		};
		let Ok(doc) = doc_arc.read() else {
			return Vec::new();
		};
		let Some(actions) = doc.runtime_model().runtime_actions() else {
			return Vec::new();
		};
		actions
			.actions
			.iter()
			.find(|action| action.id == action_id)
			.map(|action| {
				action
					.effects
					.iter()
					.filter_map(|effect| match effect {
						un_avatar_core::UnaRuntimeActionEffect::ExpressionWeight { name, weight } => Some((name.clone(), *weight)),
						_ => None,
					})
					.collect()
			})
			.unwrap_or_default()
	}

	pub(crate) fn refresh_profile_expression_runtime_actions(
		&mut self,
		enabled_animator_action_ids: &[String],
		animator_action_values: &std::collections::BTreeMap<String, f32>,
	) -> Result<(), String> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Ok(());
		};
		let mut doc = doc_arc.write().map_err(|_| "document: RwLock poisoned".to_string())?;
		crate::model_loader::add_enabled_expression_runtime_actions(&mut doc, enabled_animator_action_ids, animator_action_values);
		Ok(())
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

	pub(crate) fn runtime_action_wardrobe_set_id(&self, action_id: &str) -> Option<String> {
		let doc_arc = self.document.as_ref()?;
		let doc = doc_arc.read().ok()?;
		let actions = doc.runtime_model().runtime_actions()?;
		actions
			.actions
			.iter()
			.find(|action| action.id == action_id)?
			.effects
			.iter()
			.find_map(|effect| match effect {
				un_avatar_core::UnaRuntimeActionEffect::WardrobeSet { set_id } => Some(set_id.clone()),
				_ => None,
			})
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
			.and_then(|actions| menu_action_candidates_from_runtime(doc.unavatar.as_ref(), actions, doc.runtime_model().scene()))
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
			.and_then(|actions| menu_action_candidates_from_runtime(doc.unavatar.as_ref(), actions, doc.runtime_model().scene()));
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
		Ok(doc.runtime_model_mut().apply_contact_parameter_values_with_changes())
	}

	pub(crate) fn apply_dynamics_interaction_parameter_emissions(&mut self) -> Result<BTreeMap<String, f32>, String> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Ok(BTreeMap::new());
		};
		let mut doc = doc_arc.write().map_err(|_| "document: RwLock poisoned".to_string())?;
		let before = doc.runtime_model().runtime_parameter_values();
		let updates = dynamics_interaction_parameter_updates_with_context(
			&doc,
			self.rest_nodes.as_deref().map(Vec::as_slice),
			&self.runtime_scene_node_paths_by_index,
			&self.runtime_center_peak_angle_parameters,
			Some(before),
		);
		if updates.values.is_empty() {
			return Ok(BTreeMap::new());
		}
		doc.runtime_model_mut().set_runtime_parameter_values(updates.values);
		Ok(updates.changed)
	}

	fn apply_restored_runtime_action_effects(&mut self, restored: &[un_avatar_core::UnaEvaluationRestoreApplyEntry]) {
		if restored.is_empty() {
			return;
		}
		let dynamics_source_ids = restored_dynamics_source_ids(restored);
		if !dynamics_source_ids.is_empty() {
			if let (Some(doc_arc), Some(rest_nodes)) = (self.document.as_ref(), self.rest_nodes.as_ref()) {
				if let Ok(mut doc) = doc_arc.write() {
					if let Some(runtime) = doc.runtime_scene_and_dynamics_mut() {
						for source_id in &dynamics_source_ids {
							reset_runtime_dynamics_nodes_to_rest_for_source_id(
								runtime.scene,
								runtime.dynamics.as_readonly(),
								rest_nodes,
								source_id,
							);
						}
					}
				}
			}
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
		dynamics_group_statuses_with_limit(&doc, &self.runtime_dynamics_physics.categories, Some(DYNAMICS_GROUP_STATUS_LIMIT))
	}

	pub(crate) fn dump_scene_nodes(&self, path: &std::path::Path, filter: Option<&str>) -> Result<(), String> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Err("document is not attached".to_string());
		};
		let doc = doc_arc.read().map_err(|_| "document: RwLock poisoned".to_string())?;
		let Some(scene) = doc.runtime_model().scene() else {
			return Err("runtime scene is not available".to_string());
		};
		let paths = scene_node_paths_by_index(scene);
		let parents = scene_parent_indices(scene);
		let world = diagnostic_world_from_scene(scene);
		let filter = filter.map(|value| value.to_ascii_lowercase());
		let nodes = scene
			.nodes
			.iter()
			.enumerate()
			.filter_map(|(index, node)| {
				let node_path = paths.get(index).cloned().flatten();
				if let Some(filter) = filter.as_deref() {
					let name_matches = node.name.as_deref().is_some_and(|name| name.to_ascii_lowercase().contains(filter));
					let path_matches = node_path.as_deref().is_some_and(|path| path.to_ascii_lowercase().contains(filter));
					if !name_matches && !path_matches {
						return None;
					}
				}
				let local = Mat4::from_cols_array(&node.transform);
				let (_, local_rotation, local_translation) = local.to_scale_rotation_translation();
				let world_matrix = world.get(index).copied().unwrap_or(Mat4::IDENTITY);
				let (_, world_rotation, world_translation) = world_matrix.to_scale_rotation_translation();
				let parent_index = parents.get(index).copied().flatten();
				Some(serde_json::json!({
					"index": index,
					"name": node.name.clone(),
					"path": node_path,
					"parent_index": parent_index,
					"parent_path": parent_index.and_then(|parent| paths.get(parent).cloned().flatten()),
					"children": node.children.clone(),
					"mesh": node.mesh,
					"skin": node.skin,
					"visible": node.visible,
					"local_translation": local_translation.to_array(),
					"local_rotation_xyzw": [local_rotation.x, local_rotation.y, local_rotation.z, local_rotation.w],
					"world_translation": world_translation.to_array(),
					"world_rotation_xyzw": [world_rotation.x, world_rotation.y, world_rotation.z, world_rotation.w],
				}))
			})
			.collect::<Vec<_>>();
		let output = serde_json::json!({
			"filter": filter,
			"node_count": scene.nodes.len(),
			"nodes": nodes,
		});
		if let Some(parent) = path.parent() {
			std::fs::create_dir_all(parent).map_err(|e| format!("create dump dir {}: {e}", parent.display()))?;
		}
		let text = serde_json::to_string_pretty(&output).map_err(|e| format!("serialize scene node dump: {e}"))?;
		std::fs::write(path, text).map_err(|e| format!("write scene node dump {}: {e}", path.display()))
	}

	pub(crate) fn dynamics_response_categories(&self) -> Vec<un_avatar_skeleton::DynamicsResponseCategorySummary> {
		let mut categories = self
			.dynamics_sim
			.as_ref()
			.map(DynamicsSimulator::response_category_summaries)
			.unwrap_or_default();
		let Some(doc_arc) = self.document.as_ref() else {
			return categories;
		};
		let Ok(doc) = doc_arc.read() else {
			return categories;
		};
		let runtime_model = doc.runtime_model();
		let Some(runtime) = runtime_model.scene_profile_dynamics() else {
			return categories;
		};
		let visual_target_context = DynamicsVisualTargetContext::for_scene(runtime.scene);
		for group in runtime
			.dynamics
			.dynamics_groups()
			.filter(|group| group.effective_enabled && runtime.dynamics.source_id_resident_in_scene(runtime.scene, group.source_id))
		{
			let category_name = classify_dynamics_group_category(runtime.scene, group, &self.runtime_dynamics_physics.categories);
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
		categories
	}

	pub(crate) fn dynamics_response_groups(&self) -> Vec<un_avatar_skeleton::DynamicsResponseGroupSummary> {
		let mut groups = self
			.dynamics_sim
			.as_ref()
			.map(DynamicsSimulator::response_group_summaries)
			.unwrap_or_default();
		let Some(doc_arc) = self.document.as_ref() else {
			return groups;
		};
		let Ok(doc) = doc_arc.read() else {
			return groups;
		};
		let Some(runtime) = doc.runtime_model().scene_profile_dynamics() else {
			return groups;
		};
		annotate_dynamics_response_group_visibility(&mut groups, runtime.scene, runtime.dynamics);
		groups
	}

	pub(crate) fn dynamics_tuning_warnings(&self) -> Vec<String> {
		self.dynamics_sim
			.as_ref()
			.map(|sim| sim.tuning_warnings().to_vec())
			.unwrap_or_default()
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
		let (matching_action_ids, parameter_is_action_related, actions_snapshot) = {
			let doc = doc_arc.read().map_err(|_| "document: RwLock poisoned".to_string())?;
			let runtime = doc.runtime_model();
			let Some(actions) = runtime.runtime_actions() else {
				return Ok(None);
			};
			(
				runtime_action_ids_for_parameter(actions, runtime.scene(), name, value),
				runtime_actions_reference_parameter(actions, name),
				actions.clone(),
			)
		};
		let mut last_activation = None;
		for action_id in matching_action_ids {
			last_activation = Some(self.activate_runtime_action(Some(&action_id), None, None, None, None)?);
		}
		if last_activation.is_none() {
			self.apply_metadata_expression_menu_parameter(name, value)?;
		}
		if last_activation.is_none() && parameter_is_action_related {
			let mut doc = doc_arc.write().map_err(|_| "document: RwLock poisoned".to_string())?;
			let restored = doc.runtime_model_mut().restore_inactive_runtime_action_effects(&actions_snapshot)?;
			drop(doc);
			self.apply_restored_runtime_action_effects(&restored);
		}
		if last_activation.is_none() {
			self.last_runtime_parameter_action_values = self.runtime_parameter_values();
		}
		Ok(last_activation)
	}

	fn apply_metadata_expression_menu_parameter(&mut self, name: &str, value: f32) -> Result<(), String> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Ok(());
		};
		let doc = doc_arc.read().map_err(|_| "document: RwLock poisoned".to_string())?;
		let Some(actions) = doc.runtime_model().runtime_actions() else {
			return Ok(());
		};
		let Some(unavatar) = doc.unavatar.as_ref() else {
			return Ok(());
		};
		let candidates = menu_action_candidates_from_runtime(Some(unavatar), actions, doc.runtime_model().scene()).unwrap_or_default();
		let active_candidate = candidates.iter().find(|candidate| {
			candidate.match_kind == "metadata"
				&& candidate.parameter_name == name
				&& (candidate.parameter_value - value).abs() <= un_avatar_core::UNA_RUNTIME_ACTION_PARAMETER_EPSILON
		});
		let active_label = active_candidate.and_then(|candidate| candidate.menu_label.clone());
		let mut affected = candidates
			.iter()
			.filter(|candidate| candidate.match_kind == "metadata" && candidate.parameter_name == name)
			.filter_map(|candidate| candidate.menu_label.as_deref())
			.filter(|label| self.expression_presets.iter().any(|preset| preset == label))
			.map(str::to_owned)
			.collect::<Vec<_>>();
		affected.sort_unstable();
		affected.dedup();
		if affected.is_empty() {
			return Ok(());
		}
		if active_label
			.as_ref()
			.is_some_and(|label| !self.expression_presets.iter().any(|preset| preset == label))
		{
			return Ok(());
		}
		drop(doc);
		for label in affected {
			let weight = if active_label.as_deref() == Some(label.as_str()) {
				1.0
			} else {
				0.0
			};
			self.set_expression_override(&label, weight);
		}
		Ok(())
	}

	pub(crate) fn evaluate_runtime_parameter_actions(&mut self) -> Result<Vec<RuntimeActionActivation>, String> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Ok(Vec::new());
		};
		let doc_arc = Arc::clone(doc_arc);
		let (parameter_values, action_ids, actions_snapshot) = {
			let doc = doc_arc.read().map_err(|_| "document: RwLock poisoned".to_string())?;
			let runtime = doc.runtime_model();
			let parameter_values = runtime.runtime_parameter_values();
			if parameter_values == &self.last_runtime_parameter_action_values {
				return Ok(Vec::new());
			}
			let parameter_values = parameter_values.clone();
			let Some(actions) = runtime.runtime_actions() else {
				self.last_runtime_parameter_action_values = parameter_values;
				return Ok(Vec::new());
			};
			let action_ids = runtime_action_ids_for_parameter_values(actions, runtime.scene(), &parameter_values);
			(parameter_values, action_ids, actions.clone())
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

	pub(crate) fn set_runtime_dynamics_enabled(&mut self, source_id: &str, enabled: bool) -> Result<(), String> {
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

	pub(crate) fn set_all_runtime_dynamics_enabled(&mut self, enabled: bool) -> Result<(), String> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Err("document is not attached".to_string());
		};
		let mut doc = doc_arc.write().map_err(|_| "document: RwLock poisoned".to_string())?;
		let Some(mut runtime) = doc.runtime_scene_and_dynamics_mut() else {
			return Err("document has no runtime scene".to_string());
		};
		let count = runtime.dynamics.set_all_groups_enabled(enabled);
		if count == 0 {
			return Err("document has no runtime dynamics source ids".to_string());
		}
		drop(doc);
		self.reset_dynamics_nodes_to_rest();
		self.rebuild_runtime_dynamics();
		self.invalidate_applied_document_state();
		Ok(())
	}

	pub(crate) fn set_current_wardrobe_runtime_dynamics_enabled(&mut self, enabled: bool) -> Result<(), String> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Err("document is not attached".to_string());
		};
		let rest_nodes = self.rest_nodes.as_ref().map(Arc::clone);
		let mut doc = doc_arc.write().map_err(|_| "document: RwLock poisoned".to_string())?;
		let scoped_source_ids = doc.runtime_model().scoped_asset_selection().dynamics_source_ids;
		if scoped_source_ids.is_empty() {
			drop(doc);
			return self.set_all_runtime_dynamics_enabled(enabled);
		}
		let Some(mut runtime) = doc.runtime_scene_and_dynamics_mut() else {
			return Err("document has no runtime scene".to_string());
		};
		let mut changed = 0usize;
		for source_id in &scoped_source_ids {
			if runtime.dynamics.set_group_enabled_by_source_id(source_id, enabled) {
				changed += 1;
				if let Some(rest_nodes) = rest_nodes.as_ref() {
					reset_runtime_dynamics_nodes_to_rest_for_source_id(
						runtime.scene,
						runtime.dynamics.as_readonly(),
						rest_nodes,
						source_id,
					);
				}
			}
		}
		if changed == 0 {
			return Err("current wardrobe has no runtime dynamics source ids".to_string());
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
		let (restored, runtime_parameter_snapshot) = {
			let mut doc = doc_arc.write().map_err(|_| "document: RwLock poisoned".to_string())?;
			doc.runtime_model_mut().set_last_action_id(Some(resolved_action_id.clone()));
			{
				let mut runtime = doc.runtime_model_mut();
				for (name, value) in &parameter_values {
					runtime.set_runtime_parameter_value(name.clone(), *value);
				}
			}
			let restored = doc.runtime_model_mut().restore_inactive_runtime_action_effects(&actions_snapshot)?;
			let runtime_parameter_snapshot = doc.runtime_model().runtime_parameter_values().clone();
			(restored, runtime_parameter_snapshot)
		};
		self.apply_restored_runtime_action_effects(&restored);
		self.last_runtime_parameter_action_values = runtime_parameter_snapshot;
		Ok(RuntimeActionActivation {
			action_id: resolved_action_id,
			active_wardrobe_set,
			parameter_values,
		})
	}

	pub(crate) fn deactivate_runtime_action(&mut self, action_id: &str) -> Result<RuntimeActionActivation, String> {
		let Some(doc_arc) = self.document.as_ref() else {
			return Err("document is not attached".to_string());
		};
		let doc_arc = Arc::clone(doc_arc);
		let action = {
			let doc = doc_arc.read().map_err(|_| "document: RwLock poisoned".to_string())?;
			let Some(actions) = doc.runtime_model().runtime_actions() else {
				return Err("document has no runtime actions".to_string());
			};
			actions
				.actions
				.iter()
				.find(|action| action.id == action_id)
				.cloned()
				.ok_or_else(|| "runtime action not found".to_string())?
		};
		for effect in action.effects {
			if let UnaRuntimeActionEffect::ExpressionWeight { name, .. } = effect {
				self.set_expression_override(&name, 0.0);
			}
		}
		let mut doc = doc_arc.write().map_err(|_| "document: RwLock poisoned".to_string())?;
		doc.runtime_model_mut().set_last_action_id(None);
		drop(doc);
		Ok(RuntimeActionActivation {
			action_id: action_id.to_string(),
			active_wardrobe_set: None,
			parameter_values: BTreeMap::new(),
		})
	}

	pub(crate) fn scene_build_context(&self) -> GpuSceneBuildContext {
		GpuSceneBuildContext {
			device: self.device.clone(),
			queue: self.queue.clone(),
			format: self.config.format,
			aa: self.aa,
			shader_variant_tier: self.shader_variant_tier,
			pipeline_cache: self.pipeline_cache.clone(),
		}
	}

	pub(crate) fn rest_nodes_for_scene_prepare(&self) -> Option<Arc<Vec<UnaSceneNode>>> {
		self.rest_nodes.as_ref().map(Arc::clone)
	}

	pub(crate) fn attach_prepared_document(
		&mut self,
		prepared: PreparedDocumentScene,
		options: DocumentAttachOptions,
	) -> Result<(), String> {
		let attach_start = Instant::now();
		let DocumentAttachOptions {
			vmc_address,
			unmotion_zenoh,
			audio_link,
			debug_vmc,
			dynamics_enabled,
			bone_colliders,
			dynamics_physics,
			..
		} = options;
		let options_elapsed = attach_start.elapsed();
		let prepared_timings = prepared.timings;
		prepared_timings.log_slow();
		self.runtime_dynamics_enabled = dynamics_enabled;
		self.runtime_bone_collider_config = bone_colliders;
		self.runtime_dynamics_physics = dynamics_physics;
		self.expression_presets = prepared.expression_presets;
		self.rest_nodes = prepared.rest_nodes;
		let apply_initial_values_start = Instant::now();
		prepared
			.document
			.write()
			.map_err(|_| "document: RwLock poisoned".to_string())?
			.runtime_model_mut()
			.apply_runtime_parameter_initial_values();
		log_slow_gpu_scene_context_step("attach initial runtime parameters", apply_initial_values_start.elapsed());
		let (motion_runtime_parameter_names, runtime_scene_node_paths_by_index, runtime_center_peak_angle_parameters) = {
			let doc = prepared.document.read().map_err(|_| "document: RwLock poisoned".to_string())?;
			let runtime_model = doc.runtime_model();
			(
				motion_signal_runtime_parameter_names(&doc),
				runtime_model.scene().map(scene_node_paths_by_index).unwrap_or_default(),
				animator_center_peak_angle_parameters(&doc),
			)
		};
		let state_assign_start = Instant::now();
		self.document = Some(prepared.document);
		self.invalidate_applied_document_state();
		self.motion_runtime_parameter_names = motion_runtime_parameter_names.into_boxed_slice();
		self.runtime_scene_node_paths_by_index = runtime_scene_node_paths_by_index.into_boxed_slice();
		self.runtime_center_peak_angle_parameters = runtime_center_peak_angle_parameters.into_boxed_slice();
		self.scene_meshes = prepared.scene_meshes;
		self.texture_summary = prepared.texture_summary;
		self.dynamics_sim = prepared.dynamics_sim;
		self.bone_colliders = prepared.bone_colliders;
		self.bone_collider_count = prepared.bone_collider_count;
		self.bone_collider_source = prepared.bone_collider_source;
		self.apply_runtime_requirements(prepared.runtime_requirements, audio_link);
		log_slow_gpu_scene_context_step("attach prepared state assignment", state_assign_start.elapsed());
		let motion_receiver_start = Instant::now();
		self.reconfigure_motion_receivers(vmc_address, unmotion_zenoh, debug_vmc)?;
		log_slow_gpu_scene_context_step("attach motion receiver reconfigure", motion_receiver_start.elapsed());
		let globals_start = Instant::now();
		let (gw, gh) = self.render_pixel_dims();
		self.globals_uploaded = None;
		self.write_globals(gw, gh);
		log_slow_gpu_scene_context_step("attach globals upload", globals_start.elapsed());
		log_slow_gpu_scene_context_step("attach options destructure", options_elapsed);
		log_slow_gpu_scene_context_step("attach prepared document total", attach_start.elapsed());
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
		let prepare_start = Instant::now();
		let mut timings = PreparedDocumentSceneTimings::default();
		let GpuSceneBuildContext {
			device,
			queue,
			format,
			aa,
			shader_variant_tier,
			pipeline_cache,
		} = self;
		let document_unwrap_start = Instant::now();
		let document = Arc::try_unwrap(document).unwrap_or_else(|document| (*document).clone());
		timings.document_unwrap = document_unwrap_start.elapsed();
		let runtime_model = document.runtime_model();
		let physics_start = Instant::now();
		let physics = build_runtime_physics_for_document(
			&document,
			options.dynamics_enabled,
			options.bone_colliders,
			&options.dynamics_physics,
		);
		timings.physics = physics_start.elapsed();
		let needs_rest_nodes = runtime_model.has_humanoid_scene() || physics.dynamics_sim.is_some();
		let rest_nodes_start = Instant::now();
		let rest_nodes = if needs_rest_nodes {
			runtime_model.scene_nodes().map(|nodes| Arc::new(nodes.to_vec()))
		} else {
			None
		};
		timings.rest_nodes = rest_nodes_start.elapsed();
		let expressions_start = Instant::now();
		let expression_presets = expression_preset_names(runtime_model.expression_catalog());
		let dynamic_morph_target_names = animator_dynamic_morph_target_names(&document);
		timings.expressions = expressions_start.elapsed();
		let mut scene_meshes = None;
		let mut texture_summary = None;
		let mut runtime_requirements = SceneMeshRuntimeRequirements::default();
		if let Some(runtime) = runtime_model.scene_expression_catalog() {
			if options.debug_material_dump {
				log_material_skin_report(&document);
			}
			let gpu_texture_compression_enabled = options.block_compression_encoder == BlockCompressionEncoder::Gpu
				&& !matches!(
					options.texture_compression,
					TextureCompressionMode::Source | TextureCompressionMode::Compat
				);
			let mesh_build_start = Instant::now();
			let mut mesh_load_opts = options.mesh_diagnostics.clone();
			mesh_load_opts.mesh_cloth_assist_categories = options.dynamics_physics.categories.clone();
			mesh_load_opts.dynamic_deforming_node_indices =
				dynamics_deforming_node_indices_for_mesh_assist(runtime_model, &options.dynamics_physics.categories);
			let mut sm = SceneMeshes::new(
				&device,
				&queue,
				format,
				aa_sample_count(aa),
				shader_variant_tier,
				pipeline_cache.cache(),
				runtime.scene,
				runtime.expression_catalog,
				&dynamic_morph_target_names,
				runtime_model.active_asset_groups(),
				mesh_load_opts,
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
				gpu_texture_compression_enabled,
				&mut progress,
			)?;
			timings.mesh_build = mesh_build_start.elapsed();
			if !sm.is_empty() {
				texture_summary = Some(sm.texture_summary());
				progress(SceneMeshBuildProgress {
					phase: "gpu-upload",
					current: 1,
					total: 1,
					message: "Preparing initial scene transforms".to_string(),
				});
				let initial_draw_start = Instant::now();
				let world_start = Instant::now();
				let world = crate::scene_transform::scene_world_matrices(runtime.scene);
				log_slow_gpu_scene_context_step("initial scene world matrices", world_start.elapsed());
				let expression_start = Instant::now();
				let expression_weights = active_expression_weights_for_doc(false, &document);
				log_slow_gpu_scene_context_step("initial expression weights", expression_start.elapsed());
				let residency_start = Instant::now();
				sm.refresh_asset_group_residency(runtime.scene, runtime_model.active_asset_groups());
				sm.promote_visible_draw_residency();
				log_slow_gpu_scene_context_step("initial asset residency refresh", residency_start.elapsed());
				let transform_start = Instant::now();
				let _ = sm.update_draw_transforms(&queue, runtime.scene, &world, expression_weights, None, None, true);
				log_slow_gpu_scene_context_step("initial draw transform upload", transform_start.elapsed());
				log_slow_gpu_scene_context_step("initial draw state preparation", initial_draw_start.elapsed());
				timings.initial_draw_state = initial_draw_start.elapsed();
				runtime_requirements = sm.runtime_requirements();
				if runtime_requirements.audio_link_texture && options.audio_link.source == AudioLinkSource::InputDevice {
					eprintln!("un-avatar-renderer: external AudioLink texture needed by visible material set");
				}
				scene_meshes = Some(sm);
			}
		}
		let pipeline_cache_store_start = Instant::now();
		pipeline_cache.store();
		timings.pipeline_cache_store = pipeline_cache_store_start.elapsed();
		log_slow_gpu_scene_context_step("pipeline cache store", timings.pipeline_cache_store);
		timings.total = prepare_start.elapsed();
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
			expression_presets: expression_presets.into_boxed_slice(),
			timings,
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
		let changed_runtime_parameters = apply_motion_signal_runtime_parameters_with_names(
			&mut document,
			&self.pending_motion_frames,
			&self.motion_runtime_parameter_names,
		);
		if should_log && !changed_runtime_parameters.is_empty() {
			self.debug_log
				.line("motion", format!("runtime_parameters={changed_runtime_parameters:?}"));
		}
		let applied_frame_count = self.pending_motion_frames.len();
		drop(document);
		if !changed_runtime_parameters.is_empty() {
			if let Err(e) = self.evaluate_runtime_parameter_actions() {
				eprintln!("un-avatar-renderer: motion signal parameter action evaluation failed: {e}");
			}
		}
		self.pending_motion_frames.clear();
		self.motion_applied_frames.fetch_add(applied_frame_count as u64, Ordering::Relaxed);
		self.scene_pose_dirty = true;
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

	fn write_wardrobe_billboard_uniform(&self, billboard: &WardrobeChangingBillboardFrame) {
		let center = Vec3::from_array(billboard.billboard_center);
		self.queue.write_buffer(
			&self.wardrobe_billboard_buffer,
			0,
			bytemuck::bytes_of(&WardrobeBillboardGpu {
				view_proj: billboard.billboard_view_proj,
				camera_pos: [
					billboard.billboard_camera_pos[0],
					billboard.billboard_camera_pos[1],
					billboard.billboard_camera_pos[2],
					1.0,
				],
				center_size: [center.x, center.y, center.z, billboard.billboard_size.max(0.01)],
				time_params: [billboard.time_secs, 0.0, 0.0, 0.0],
			}),
		);
	}

	fn draw_wardrobe_billboard<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, billboard: &WardrobeChangingBillboardFrame) {
		self.write_wardrobe_billboard_uniform(billboard);
		pass.set_pipeline(&self.wardrobe_billboard_pipeline);
		pass.set_bind_group(0, &self.bind_group, &[]);
		pass.set_bind_group(1, &self.wardrobe_billboard_bind_group, &[]);
		pass.draw(0..6, 0..1);
	}

	fn draw_startup_progress_overlay<'a>(
		&'a self,
		pass: &mut wgpu::RenderPass<'a>,
		progress_overlay: &StartupProgressOverlayFrame,
		width: u32,
		height: u32,
	) {
		let aspect = width.max(1) as f32 / height.max(1) as f32;
		self.queue.write_buffer(
			&self.startup_progress_overlay_buffer,
			0,
			bytemuck::bytes_of(&StartupProgressOverlayGpu {
				time: progress_overlay.time_secs,
				progress: progress_overlay.progress,
				aspect,
				phase: progress_overlay.phase,
				rect_center: progress_overlay.rect_center,
				rect_half_size: progress_overlay.rect_half_size,
			}),
		);
		pass.set_pipeline(&self.startup_progress_overlay_pipeline);
		pass.set_bind_group(0, &self.startup_progress_overlay_bind_group, &[]);
		pass.draw(0..3, 0..1);
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
		frame_role: RenderedFrameRole,
		window_output_enabled: bool,
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
		let wardrobe_transition_only = frame_role.is_wardrobe_transition_only();
		if let (Some(doc_arc), true) = (
			&self.document,
			!wardrobe_transition_only && self.debug_scene && self.debug_log.is_enabled() && self.debug_frame_seq.is_multiple_of(180),
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
			!wardrobe_transition_only && self.debug_morph && self.debug_log.is_enabled() && self.debug_frame_seq.is_multiple_of(180),
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
		let t_motion0 = Instant::now();
		if !wardrobe_transition_only {
			self.apply_pending_motion_frames();
		}
		let motion_apply_ms = t_motion0.elapsed().as_secs_f32() * 1000.0;
		let t_dynamics0 = Instant::now();
		let mut dynamics_profile = DynamicsStepProfile::default();
		if !wardrobe_transition_only {
			if let (Some(doc_arc), Some(sim)) = (&self.document, &mut self.dynamics_sim) {
				if let Ok(mut doc) = doc_arc.write() {
					if let Some(runtime) = doc.runtime_scene_and_dynamics_mut() {
						if self.dynamics_profile_enabled {
							dynamics_profile = sim.step_runtime_dynamics_profiled(runtime.scene, runtime.dynamics.as_readonly(), dt);
						} else {
							sim.step_runtime_dynamics(runtime.scene, runtime.dynamics.as_readonly(), dt);
						}
					}
				}
			}
		}
		let dynamics_step_ms = t_dynamics0.elapsed().as_secs_f32() * 1000.0;
		let (gw, gh) = self.render_pixel_dims();
		let t_globals0 = Instant::now();
		if !wardrobe_transition_only {
			self.write_frame_globals(gw, gh, true);
		}
		let frame_globals_ms = t_globals0.elapsed().as_secs_f32() * 1000.0;

		let draw_scene = !wardrobe_transition_only && self.scene_meshes.as_ref().is_some_and(|m| !m.is_empty());
		let use_spout = {
			#[cfg(windows)]
			{
				match frame_role.spout2_delivery(self.spout_launch.is_some()) {
					Spout2FrameDelivery::RuntimeOutput => self.ensure_runtime_spout_output(),
					Spout2FrameDelivery::SuppressedRendererStartup | Spout2FrameDelivery::Unavailable => false,
				}
			}
			#[cfg(not(windows))]
			{
				false
			}
		};
		if !window_output_enabled && !use_spout {
			return None;
		}
		let t_surface0 = Instant::now();
		let frame = if window_output_enabled {
			match self.surface.get_current_texture() {
				wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => Some(f),
				wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
					let s = window.inner_size();
					self.resize(s.width, s.height);
					if use_spout {
						None
					} else {
						return None;
					}
				}
				wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
					if use_spout {
						None
					} else {
						return None;
					}
				}
				wgpu::CurrentSurfaceTexture::Validation => {
					eprintln!("un-avatar-renderer: get_current_texture: validation error");
					if use_spout {
						None
					} else {
						return None;
					}
				}
			}
		} else {
			None
		};
		let surface_acquire_ms = t_surface0.elapsed().as_secs_f32() * 1000.0;
		let swap_view = if let Some(frame) = frame.as_ref() {
			let frame_width = frame.texture.width();
			let frame_height = frame.texture.height();
			if frame_width == 0 || frame_height == 0 {
				return None;
			}
			if frame_width != self.config.width || frame_height != self.config.height {
				let s = window.inner_size();
				let width = if s.width == 0 { frame_width } else { s.width };
				let height = if s.height == 0 { frame_height } else { s.height };
				self.resize(width, height);
				return None;
			}
			Some(frame.texture.create_view(&wgpu::TextureViewDescriptor::default()))
		} else {
			None
		};

		#[cfg(windows)]
		if let (Some(ref mut sp), Some(ref lc)) = (&mut self.spout, &self.spout_launch) {
			sp.resize_to(&self.device, self.config.width, self.config.height, lc, self.config.format);
		}
		let use_post_aa = matches!(self.aa, AaMode::Fxaa | AaMode::Smaa);
		let use_avatar_outline =
			self.avatar_outline.policy == AvatarOutlinePolicy::Override && self.avatar_outline.width.unwrap_or(0.003) > 0.0;
		let use_color_adjust = !self.environment_color.is_identity();
		let use_bloom = self.bloom.is_enabled();
		let use_ssao = self.ssao.is_enabled();
		let needs_screen_refraction =
			!wardrobe_transition_only && self.scene_meshes.as_ref().is_some_and(SceneMeshes::needs_screen_refraction);
		let use_post = !wardrobe_transition_only
			&& (use_post_aa || use_avatar_outline || use_color_adjust || use_bloom || use_ssao || needs_screen_refraction);
		let use_msaa = matches!(self.aa, AaMode::Msaa);
		let t_target0 = Instant::now();
		if use_post {
			if let Some(post) = &mut self.post_process {
				post.resize_to(&self.device, gw, gh, self.config.format);
			} else {
				self.post_process = Some(PostProcess::new(&self.device, gw, gh, self.config.format));
			}
		}
		let target_prepare_ms = t_target0.elapsed().as_secs_f32() * 1000.0;
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
		let scene_pose_may_change = self.scene_pose_dirty
			|| self.dynamics_sim.is_some()
			|| document_revision != self.applied_document_revision
			|| expression_overrides_changed;
		let mut world_scratch_current = false;
		let t_draw_state0 = Instant::now();
		if draw_scene && scene_pose_may_change {
			world_scratch_current = self.refresh_scene_draw_state(Some(document_revision));
			if world_scratch_current {
				self.scene_pose_dirty = false;
			}
		}
		let draw_state_refresh_ms = t_draw_state0.elapsed().as_secs_f32() * 1000.0;
		let draw_doc_lock_ms = if world_scratch_current { self.last_draw_doc_lock_ms } else { 0.0 };
		let draw_expression_select_ms = if world_scratch_current {
			self.last_draw_expression_select_ms
		} else {
			0.0
		};
		let draw_update_total_ms = if world_scratch_current {
			self.last_draw_update_total_ms
		} else {
			0.0
		};
		let scene_world_ms = if world_scratch_current { self.last_scene_world_ms } else { 0.0 };
		let draw_transform_timings = if world_scratch_current {
			self.last_draw_transform_timings
		} else {
			DrawTransformUpdateTimings::default()
		};
		let t_collider_debug0 = Instant::now();
		if self.show_bone_colliders && draw_scene {
			if world_scratch_current {
				self.rebuild_bone_collider_debug_vertices_from_world();
			} else {
				self.update_bone_collider_debug_vertices();
			}
		} else {
			self.bone_collider_vertex_count = 0;
		}
		let bone_collider_debug_ms = t_collider_debug0.elapsed().as_secs_f32() * 1000.0;
		if draw_contact_shadow {
			self.ensure_contact_shadow_pipeline();
		}
		if self.show_axes {
			self.ensure_axes_pipeline();
		}
		if self.show_bone_colliders && self.bone_collider_vertex_count > 0 {
			self.ensure_bone_collider_pipeline();
		}

		#[cfg(windows)]
		let final_target_view = if use_spout {
			self.spout.as_ref().unwrap().color_view()
		} else {
			swap_view.as_ref().expect("surface view is available for window output")
		};
		#[cfg(not(windows))]
		let final_target_view = swap_view.as_ref().expect("surface view is available for window output");

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
			(
				swap_view.as_ref().expect("surface view is available for window output"),
				&self.depth_view,
			)
		};

		let t_encode0 = Instant::now();
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
					stencil_ops: Some(stencil_clear_ops()),
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
					stencil_ops: Some(stencil_load_ops()),
				}),
				timestamp_writes: None,
				occlusion_query_set: None,
				multiview_mask: None,
			});
			if let Some(sm) = &self.scene_meshes {
				sm.draw_blended_after_screen_refraction(&mut pass);
			}
			if self.show_axes {
				pass.set_pipeline(self.axes_pipeline.as_ref().expect("axes pipeline is initialized"));
				pass.set_bind_group(0, &self.bind_group, &[]);
				pass.draw(0..6, 0..1);
			}
			if self.show_bone_colliders && self.bone_collider_vertex_count > 0 {
				if let Some(buffer) = &self.bone_collider_vertex_buffer {
					pass.set_pipeline(self.bone_collider_pipeline.as_ref().expect("bone collider pipeline is initialized"));
					pass.set_bind_group(0, &self.bind_group, &[]);
					pass.set_vertex_buffer(0, buffer.slice(..));
					pass.draw(0..self.bone_collider_vertex_count, 0..1);
				}
			}
			if let Some(billboard) = frame_role.wardrobe_transition_billboard() {
				self.draw_wardrobe_billboard(&mut pass, billboard);
			}
			if let Some(progress_overlay) = frame_role.startup_overlay() {
				self.draw_startup_progress_overlay(&mut pass, progress_overlay, gw, gh);
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
					stencil_ops: Some(stencil_clear_ops()),
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
			} else if !wardrobe_transition_only {
				pass.set_pipeline(&self.pipeline);
				pass.set_bind_group(0, &self.bind_group, &[]);
				pass.draw(0..3, 0..1);
			}
			if self.show_axes && draw_scene {
				pass.set_pipeline(self.axes_pipeline.as_ref().expect("axes pipeline is initialized"));
				pass.set_bind_group(0, &self.bind_group, &[]);
				pass.draw(0..6, 0..1);
			}
			if self.show_bone_colliders && self.bone_collider_vertex_count > 0 {
				if let Some(buffer) = &self.bone_collider_vertex_buffer {
					pass.set_pipeline(self.bone_collider_pipeline.as_ref().expect("bone collider pipeline is initialized"));
					pass.set_bind_group(0, &self.bind_group, &[]);
					pass.set_vertex_buffer(0, buffer.slice(..));
					pass.draw(0..self.bone_collider_vertex_count, 0..1);
				}
			}
			if let Some(billboard) = frame_role.wardrobe_transition_billboard() {
				self.draw_wardrobe_billboard(&mut pass, billboard);
			}
			if let Some(progress_overlay) = frame_role.startup_overlay() {
				self.draw_startup_progress_overlay(&mut pass, progress_overlay, gw, gh);
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
						stencil_ops: Some(stencil_load_ops()),
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
		let command_encode_ms = (t_before_submit - t_encode0).as_secs_f32() * 1000.0;
		let mut submit_present_ms = t_before_submit.elapsed().as_secs_f32() * 1000.0;
		if let (Some(ts), Some(idx)) = (self.gpu_timestamps.as_mut(), timestamp_write_idx) {
			ts.after_submit(idx);
		}

		let mut spout_cpu_ms = 0.0;
		#[cfg(windows)]
		if use_spout {
			let t_spout0 = Instant::now();
			let sp = self.spout.as_mut().expect("spout is initialized while active");
			// 1) 前フレーム以降に map が完了したスロットがあれば Spout2 に送る（非ブロッキング）。
			let _ = sp.send_mapped_rgba(&self.device);
			// 2) 今フレームの swizzle + readback を encode。リングが空いていれば map を要求する。
			let mut enc2 = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
				label: Some("spout-staging"),
			});
			let staged_slot = sp.copy_to_staging(&mut enc2);
			self.queue.submit(std::iter::once(enc2.finish()));
			if let Some(idx) = staged_slot {
				sp.after_submit_request_map(idx);
			}
			// 3) swap chain が取れている時だけプレビュー用にコピー。最小化 / occluded 中でも Spout 送信は続ける。
			if let Some(swap_view) = swap_view.as_ref() {
				let mut enc3 = self
					.device
					.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("spout-blit") });
				sp.encode_blit(&mut enc3, swap_view, self.config.width, self.config.height, clear_color);
				self.queue.submit(std::iter::once(enc3.finish()));
			}
			spout_cpu_ms = t_spout0.elapsed().as_secs_f32() * 1000.0;
		}

		if let Some(frame) = frame {
			let t_present0 = Instant::now();
			frame.present();
			submit_present_ms += t_present0.elapsed().as_secs_f32() * 1000.0;
		}
		if self.dynamics_profile_enabled {
			self.last_dynamics_profile = dynamics_profile.clone();
		}

		Some(FrameTimings {
			wall_since_last_ms: wall_since_last.as_secs_f32() * 1000.0,
			cpu_record_ms: (t_before_submit - t_cpu0).as_secs_f32() * 1000.0,
			cpu_total_ms: t_cpu0.elapsed().as_secs_f32() * 1000.0,
			motion_apply_ms,
			dynamics_step_ms,
			dynamics_profile,
			frame_globals_ms,
			surface_acquire_ms,
			target_prepare_ms,
			draw_state_refresh_ms,
			draw_doc_lock_ms,
			draw_expression_select_ms,
			draw_update_total_ms,
			scene_world_ms,
			draw_skin_palette_ms: draw_transform_timings.skin_palette_ms,
			draw_skin_palette_write_ms: draw_transform_timings.skin_palette_write_ms,
			draw_fur_source_vertices_ms: draw_transform_timings.fur_source_vertices_ms,
			draw_expression_values_ms: draw_transform_timings.expression_values_ms,
			draw_morph_weights_ms: draw_transform_timings.morph_weights_ms,
			draw_transform_loop_ms: draw_transform_timings.draw_transform_ms,
			bone_collider_debug_ms,
			command_encode_ms,
			submit_present_ms,
			spout_cpu_ms,
			contact_eval_ms: 0.0,
			runtime_action_eval_ms: 0.0,
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
		format: wgpu::TextureFormat::Depth24PlusStencil8,
		usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
		view_formats: &[],
	});
	let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
	(texture, view)
}

fn stencil_clear_ops() -> wgpu::Operations<u32> {
	wgpu::Operations {
		load: wgpu::LoadOp::Clear(0),
		store: wgpu::StoreOp::Store,
	}
}

fn stencil_load_ops() -> wgpu::Operations<u32> {
	wgpu::Operations {
		load: wgpu::LoadOp::Load,
		store: wgpu::StoreOp::Store,
	}
}

#[cfg(windows)]
fn log_spout_unavailable() {
	eprintln!(
		"un-avatar-renderer: Spout2 実バックエンドがこのビルドで利用できません。標準配布は `cargo xtask package` で Spout2 込みビルドを作成します。開発手動ビルドでは `--features spout-sdk` と SPOUT2_SDK_DIR / SPOUT2_LIB_DIR / 起動前 Spout.dll PATH が必要です。"
	);
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
		BoneColliderPrimitive::LocalPlane { node, center, normal, .. } => {
			let Some(m) = world.get(node) else {
				return;
			};
			let point = m.transform_point3(Vec3::from(center));
			let normal = m.transform_vector3(Vec3::from(normal)).normalize_or_zero();
			if normal.length_squared() < 1e-12 {
				return;
			}
			append_wire_plane(point, normal, COLOR, out);
		}
	}
}

fn append_wire_plane(point: Vec3, normal: Vec3, color: [f32; 4], out: &mut Vec<DebugLineVertex>) {
	let tangent_seed = if normal.x.abs() < 0.9 { Vec3::X } else { Vec3::Z };
	let tangent = normal.cross(tangent_seed).normalize_or_zero();
	let bitangent = normal.cross(tangent).normalize_or_zero();
	if tangent.length_squared() < 1e-12 || bitangent.length_squared() < 1e-12 {
		return;
	}
	let half = 0.12;
	push_debug_line(point - tangent * half, point + tangent * half, color, out);
	push_debug_line(point - bitangent * half, point + bitangent * half, color, out);
	push_debug_line(point, point + normal * half, color, out);
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
			format: wgpu::TextureFormat::Depth24PlusStencil8,
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
			format: wgpu::TextureFormat::Depth24PlusStencil8,
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
			format: wgpu::TextureFormat::Depth24PlusStencil8,
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
			format: wgpu::TextureFormat::Depth24PlusStencil8,
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

fn create_startup_progress_overlay_pipeline(
	device: &wgpu::Device,
	bind_group_layout: &wgpu::BindGroupLayout,
	surface_format: wgpu::TextureFormat,
	sample_count: u32,
) -> wgpu::RenderPipeline {
	let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
		label: Some("startup_progress_overlay"),
		source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER_STARTUP_PROGRESS_OVERLAY)),
	});

	let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
		label: Some("startup_progress_overlay"),
		bind_group_layouts: &[Some(bind_group_layout)],
		immediate_size: 0,
	});

	device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
		label: Some("startup_progress_overlay"),
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
			format: wgpu::TextureFormat::Depth24PlusStencil8,
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

fn create_wardrobe_billboard_pipeline(
	device: &wgpu::Device,
	globals_layout: &wgpu::BindGroupLayout,
	billboard_layout: &wgpu::BindGroupLayout,
	surface_format: wgpu::TextureFormat,
	sample_count: u32,
) -> wgpu::RenderPipeline {
	let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
		label: Some("wardrobe_billboard"),
		source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER_WARDROBE_BILLBOARD)),
	});
	let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
		label: Some("wardrobe_billboard"),
		bind_group_layouts: &[Some(globals_layout), Some(billboard_layout)],
		immediate_size: 0,
	});
	device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
		label: Some("wardrobe_billboard"),
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
			format: wgpu::TextureFormat::Depth24PlusStencil8,
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
		accumulate_spatial_surface_seams, animator_morph_overrides_for_doc, augment_dynamics_bone_colliders,
		dynamics_collider_contact_statuses, dynamics_collider_path_candidate_summary_statuses,
		dynamics_collider_path_runtime_summary_statuses, dynamics_collider_shape_kind, dynamics_group_statuses_with_limit,
		dynamics_interaction_angle_normalizer, dynamics_interaction_parameter_values, effective_window_backend,
		menu_action_candidates_from_runtime, menu_graph_node_path, mesh_shader_resource_plan_for_adapter,
		mesh_shader_variant_tier_for_limits, modular_avatar_menu_components, restore_runtime_scene_transforms_to_rest,
		runtime_action_id_for_parameter, runtime_action_ids_for_parameter, runtime_action_ids_for_parameter_values,
		runtime_action_statuses, scene_node_constraint_counts, sorted_index_difference, sorted_unique_index_union, transparent_alpha_mode,
		wardrobe_action_statuses, wardrobe_asset_upload_plan_for_document, wardrobe_asset_upload_plan_with_draw_counts,
		wardrobe_scoped_upload_work_for_active_gaps, DynamicsColliderShapeKind, RenderedFrameRole, RendererStartupPresentation,
		RuntimeDynamicsColliderPathCandidateSummary, RuntimeDynamicsColliderPathContactSummary, RuntimeDynamicsColliderSelectionStatus,
		RuntimeDynamicsColliderStatus, RuntimeMenuGraphNode, SceneNodeConstraintCounts, Spout2FrameDelivery, StartupProgressOverlayFrame,
		SurfaceConstraintNode, WardrobeAssetUploadPlan, WardrobeChangingBillboardFrame, WardrobeTransitionPresentation,
		BASELINE_FALLBACK_SAMPLED_TEXTURES_PER_STAGE, BASELINE_FALLBACK_SAMPLERS_PER_STAGE,
		HIGH_CAPABILITY_LILTOON_SAMPLED_TEXTURES_PER_STAGE, HIGH_CAPABILITY_LILTOON_SAMPLERS_PER_STAGE,
		WARDROBE_ASSET_UPLOAD_MODE_RESOURCE_SCOPED, WARDROBE_RESIDENCY_GAP_INDEX_STATUS_LIMIT,
	};
	use crate::mesh_pass::{MeshShaderVariantTier, SceneMeshActiveResidencyGaps, SceneMeshAssetResidencyCounts};
	use crate::RenderBackend;
	use glam::{Mat4, Vec3};
	use serde_json::json;
	use un_avatar_core::{UnaDocument, UnaNodeConstraint, UnaNodeConstraintKind, UnaNodeConstraintSource, UnaSceneNode, UnaSceneSnapshot};
	use un_avatar_skeleton::{
		BoneColliderPrimitive, DynamicsColliderAugmentOverride, DynamicsPhysicsConfig, DynamicsTailSample, HumanoidProfile,
		RuntimeBoneColliderPrimitive,
	};
	use wgpu::CompositeAlphaMode::{Auto, Opaque, PostMultiplied, PreMultiplied};

	#[test]
	fn surface_constraints_use_topology_not_cape_names() {
		let surface_nodes = BTreeMap::from([
			(
				10,
				SurfaceConstraintNode {
					group_index: 0,
					rest_tail: Vec3::ZERO,
				},
			),
			(
				20,
				SurfaceConstraintNode {
					group_index: 1,
					rest_tail: Vec3::X,
				},
			),
		]);
		let positions = [[0.0, 0.0, 0.0], [0.006, 0.0, 0.0], [0.0, 0.006, 0.0]];
		let dominant_nodes = vec![Some(10), Some(20), Some(10)];
		let mut pair_stats = BTreeMap::new();

		accumulate_spatial_surface_seams(&positions, &dominant_nodes, &surface_nodes, &mut pair_stats, 0.012, 0.9);

		let stats = pair_stats
			.get(&(10, 20))
			.expect("non-cape seam should be inferred from nearby vertices");
		assert!(stats.edge_count > 0);
		assert!(stats.stiffness >= 0.9);
	}

	#[test]
	fn surface_constraints_skip_disabled_spatial_stiffness() {
		let surface_nodes = BTreeMap::from([
			(
				10,
				SurfaceConstraintNode {
					group_index: 0,
					rest_tail: Vec3::ZERO,
				},
			),
			(
				20,
				SurfaceConstraintNode {
					group_index: 1,
					rest_tail: Vec3::X,
				},
			),
		]);
		let positions = [[0.0, 0.0, 0.0], [0.006, 0.0, 0.0]];
		let dominant_nodes = vec![Some(10), Some(20)];
		let mut pair_stats = BTreeMap::new();

		accumulate_spatial_surface_seams(&positions, &dominant_nodes, &surface_nodes, &mut pair_stats, 0.012, 0.0);

		assert!(pair_stats.is_empty());
	}

	#[test]
	fn dynamics_collider_shape_kind_is_exact_over_substring() {
		assert_eq!(dynamics_collider_shape_kind("capsule"), Some(DynamicsColliderShapeKind::Capsule));
		assert_eq!(
			dynamics_collider_shape_kind("local_capsule"),
			Some(DynamicsColliderShapeKind::Capsule)
		);
		assert_eq!(dynamics_collider_shape_kind("sphere"), Some(DynamicsColliderShapeKind::Sphere));
		assert_eq!(
			dynamics_collider_shape_kind("local_sphere"),
			Some(DynamicsColliderShapeKind::Sphere)
		);
		assert_eq!(dynamics_collider_shape_kind("plane"), Some(DynamicsColliderShapeKind::Plane));
		assert_eq!(dynamics_collider_shape_kind("local_plane"), Some(DynamicsColliderShapeKind::Plane));
		assert_eq!(dynamics_collider_shape_kind("capsule_hint"), None);
		assert_eq!(dynamics_collider_shape_kind("not_a_sphere"), None);
	}

	#[test]
	fn dynamics_collider_summaries_keep_global_colliders_in_source_selection() {
		let tail_samples = vec![DynamicsTailSample {
			source_id: "physbone:cloth".to_string(),
			curr_tail: [0.03, 0.0, 0.0],
			hit_radius: 0.02,
			..Default::default()
		}];
		let collider_selections = vec![RuntimeDynamicsColliderSelectionStatus {
			source_id: "physbone:cloth".to_string(),
			sample_collider_details: vec![
				json!({
					"index": 1,
					"source_id": "",
					"collider_path": "Body/Chest",
					"shape": "local_sphere",
					"radius": 0.05,
					"world_center": [0.0, 0.0, 0.0],
				}),
				json!({
					"index": 2,
					"source_id": "physbone:cloth",
					"collider_path": "Cloth/Side",
					"shape": "local_sphere",
					"radius": 0.01,
					"world_center": [0.2, 0.0, 0.0],
				}),
			],
			..Default::default()
		}];

		let contacts = dynamics_collider_contact_statuses(&tail_samples, &collider_selections);
		assert_eq!(contacts.len(), 1);
		assert_eq!(contacts[0].collider_path, "Body/Chest");

		let summaries = dynamics_collider_path_candidate_summary_statuses(&tail_samples, &collider_selections);
		let paths = summaries.iter().map(|summary| summary.collider_path.as_str()).collect::<Vec<_>>();

		assert!(paths.contains(&"Body/Chest"));
		assert!(paths.contains(&"Cloth/Side"));
		assert_eq!(
			summaries
				.iter()
				.find(|summary| summary.collider_path == "Body/Chest")
				.expect("global collider summary")
				.sample_source_ids,
			vec!["physbone:cloth".to_string()]
		);
	}

	#[test]
	fn dynamics_collider_path_runtime_summary_merges_candidates_and_projections() {
		let colliders = vec![RuntimeDynamicsColliderStatus {
			index: 3,
			source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
			source_id: "physbone:cloth".to_string(),
			collider_path: "Body/Chest".to_string(),
			node: 1,
			node_path: Some("Root/Chest".to_string()),
			shape: un_avatar_core::UnaDynamicsColliderShape::Sphere,
			radius: 0.1,
			height: 0.0,
			position: [0.0; 3],
			rotation: [0.0, 0.0, 0.0, 1.0],
			inside_bounds: false,
		}];
		let contact_summaries = vec![RuntimeDynamicsColliderPathContactSummary {
			collider_path: "Body/Chest".to_string(),
			collider_shape: "local_sphere".to_string(),
			contact_count: 4,
			penetrating_count: 2,
			source_count: 1,
			min_margin: -0.01,
			min_distance: 0.04,
			min_threshold: 0.05,
			sample_source_ids: vec!["physbone:cloth".to_string()],
		}];
		let candidate_summaries = vec![RuntimeDynamicsColliderPathCandidateSummary {
			collider_path: "Body/Chest".to_string(),
			collider_shape: "local_sphere".to_string(),
			candidate_count: 9,
			penetrating_count: 3,
			source_count: 1,
			min_margin: -0.02,
			min_distance: 0.03,
			min_threshold: 0.05,
			sample_source_ids: vec!["physbone:cloth".to_string()],
		}];
		let projection_counts = BTreeMap::from([("Body/Chest".to_string(), 7)]);

		let summaries =
			dynamics_collider_path_runtime_summary_statuses(&colliders, &contact_summaries, &candidate_summaries, &projection_counts);

		assert_eq!(summaries.len(), 1);
		assert_eq!(summaries[0].runtime_collider_count, 1);
		assert_eq!(summaries[0].candidate_count, 9);
		assert_eq!(summaries[0].candidate_penetrating_count, 3);
		assert_eq!(summaries[0].contact_count, 4);
		assert_eq!(summaries[0].penetrating_count, 2);
		assert_eq!(summaries[0].projection_count, 7);
	}

	#[test]
	fn dynamics_collider_path_runtime_summary_keeps_observation_only_paths() {
		let contact_summaries = vec![RuntimeDynamicsColliderPathContactSummary {
			collider_path: "Body/ContactOnly".to_string(),
			collider_shape: "local_capsule".to_string(),
			contact_count: 2,
			penetrating_count: 1,
			source_count: 1,
			min_margin: -0.01,
			min_distance: 0.04,
			min_threshold: 0.05,
			sample_source_ids: vec!["physbone:cloth".to_string()],
		}];
		let candidate_summaries = vec![RuntimeDynamicsColliderPathCandidateSummary {
			collider_path: "Body/CandidateOnly".to_string(),
			collider_shape: "local_sphere".to_string(),
			candidate_count: 3,
			penetrating_count: 1,
			source_count: 1,
			min_margin: -0.02,
			min_distance: 0.03,
			min_threshold: 0.05,
			sample_source_ids: vec!["physbone:tail".to_string()],
		}];
		let projection_counts = BTreeMap::from([("Body/ProjectionOnly".to_string(), 5)]);

		let summaries = dynamics_collider_path_runtime_summary_statuses(&[], &contact_summaries, &candidate_summaries, &projection_counts);

		let contact = summaries
			.iter()
			.find(|summary| summary.collider_path == "Body/ContactOnly")
			.expect("contact-only path should be preserved");
		assert_eq!(contact.runtime_collider_count, 0);
		assert_eq!(contact.contact_count, 2);
		assert_eq!(contact.source_count, 1);
		assert_eq!(contact.sample_source_ids, vec!["physbone:cloth".to_string()]);

		let candidate = summaries
			.iter()
			.find(|summary| summary.collider_path == "Body/CandidateOnly")
			.expect("candidate-only path should be preserved");
		assert_eq!(candidate.runtime_collider_count, 0);
		assert_eq!(candidate.candidate_count, 3);
		assert_eq!(candidate.candidate_penetrating_count, 1);
		assert_eq!(candidate.source_count, 1);
		assert_eq!(candidate.sample_source_ids, vec!["physbone:tail".to_string()]);

		let projection = summaries
			.iter()
			.find(|summary| summary.collider_path == "Body/ProjectionOnly")
			.expect("projection-only path should be preserved");
		assert_eq!(projection.runtime_collider_count, 0);
		assert_eq!(projection.projection_count, 5);
	}

	#[test]
	fn collider_augment_source_match_uses_chain_node_names() {
		let scene = UnaSceneSnapshot {
			nodes: vec![
				test_scene_node_with_transform("Avatar", Mat4::IDENTITY, vec![1, 2]),
				test_scene_node_with_transform("PanelRig", Mat4::from_translation(Vec3::X), vec![2]),
				test_scene_node_with_transform("Tip", Mat4::from_translation(Vec3::Y), Vec::new()),
			],
			roots: vec![0],
			..Default::default()
		};
		let settings = un_avatar_core::UnaDynamicsSettings {
			groups: vec![un_avatar_core::UnaDynamicsSourceGroup {
				enabled: true,
				source_id: "physbone:Avatar/generic".to_string(),
				bone_node_indices: vec![1, 2],
				..Default::default()
			}],
			..Default::default()
		};
		let mut colliders = vec![RuntimeBoneColliderPrimitive {
			source_id: "physbone:donor".to_string(),
			collider_path: "Avatar/PB/Body/Chest Collider".to_string(),
			primitive: BoneColliderPrimitive::LocalSphere {
				node: 1,
				center: [0.0; 3],
				radius: 0.1,
				inside_bounds: false,
			},
		}];
		let config = DynamicsPhysicsConfig {
			collider_augment_overrides: vec![DynamicsColliderAugmentOverride {
				name: "panel chest".to_string(),
				source_id_contains: vec!["panel rig".to_string()],
				collider_path_contains: vec!["chest collider".to_string()],
			}],
			..Default::default()
		}
		.normalized();

		augment_dynamics_bone_colliders(settings.runtime_dynamics(), &scene, &config, &mut colliders);

		assert!(colliders
			.iter()
			.any(|collider| collider.source_id == "physbone:Avatar/generic" && collider.collider_path == "Avatar/PB/Body/Chest Collider"));
	}

	#[test]
	fn collider_augment_source_match_keeps_non_ascii_tokens() {
		let scene = UnaSceneSnapshot {
			nodes: vec![
				test_scene_node_with_transform("Avatar", Mat4::IDENTITY, vec![1, 2]),
				test_scene_node_with_transform("ケープ_制御", Mat4::from_translation(Vec3::X), vec![2]),
				test_scene_node_with_transform("Tip", Mat4::from_translation(Vec3::Y), Vec::new()),
			],
			roots: vec![0],
			..Default::default()
		};
		let settings = un_avatar_core::UnaDynamicsSettings {
			groups: vec![un_avatar_core::UnaDynamicsSourceGroup {
				enabled: true,
				source_id: "physbone:Avatar/generic".to_string(),
				bone_node_indices: vec![1, 2],
				..Default::default()
			}],
			..Default::default()
		};
		let mut colliders = vec![RuntimeBoneColliderPrimitive {
			source_id: "physbone:donor".to_string(),
			collider_path: "Avatar/PB/胸 コライダー".to_string(),
			primitive: BoneColliderPrimitive::LocalSphere {
				node: 1,
				center: [0.0; 3],
				radius: 0.1,
				inside_bounds: false,
			},
		}];
		let config = DynamicsPhysicsConfig {
			collider_augment_overrides: vec![DynamicsColliderAugmentOverride {
				name: "jp cape chest".to_string(),
				source_id_contains: vec!["ケープ".to_string()],
				collider_path_contains: vec!["胸 コライダー".to_string()],
			}],
			..Default::default()
		}
		.normalized();

		augment_dynamics_bone_colliders(settings.runtime_dynamics(), &scene, &config, &mut colliders);

		assert!(colliders
			.iter()
			.any(|collider| collider.source_id == "physbone:Avatar/generic" && collider.collider_path == "Avatar/PB/胸 コライダー"));
	}

	fn test_scene_node_with_transform(name: &str, transform: Mat4, children: Vec<usize>) -> UnaSceneNode {
		UnaSceneNode {
			name: Some(name.to_string()),
			source_node_id: None,
			resolved_node_id: None,
			visible: true,
			transform: transform.to_cols_array(),
			children,
			mesh: None,
			skin: None,
			probe_anchor_node: None,
			local_bounds: None,
		}
	}

	#[test]
	fn dynamics_group_statuses_use_profile_categories() {
		let document = UnaDocument {
			scene: Some(UnaSceneSnapshot {
				nodes: vec![
					test_scene_node_with_transform("Avatar", Mat4::IDENTITY, vec![1, 2]),
					test_scene_node_with_transform("PanelRig", Mat4::from_translation(Vec3::X), vec![2]),
					test_scene_node_with_transform("Tip", Mat4::from_translation(Vec3::Y), Vec::new()),
				],
				roots: vec![0],
				..Default::default()
			}),
			spring_bones: Some(un_avatar_core::UnaDynamicsSettings {
				groups: vec![un_avatar_core::UnaDynamicsSourceGroup {
					enabled: true,
					source_id: "physbone:Fixture/PanelRig".to_string(),
					bone_node_indices: vec![1, 2],
					..Default::default()
				}],
				..Default::default()
			}),
			..Default::default()
		};
		let categories = vec![un_avatar_skeleton::DynamicsCategoryDefinition {
			id: "cloth".to_string(),
			matches: vec!["panel_rig".to_string()],
			..Default::default()
		}];

		let statuses = dynamics_group_statuses_with_limit(&document, &categories, None);

		assert_eq!(statuses.len(), 1);
		assert_eq!(statuses[0].category, "cloth");
	}

	#[test]
	fn scene_node_constraint_counts_report_parent_sources() {
		let scene = UnaSceneSnapshot {
			node_constraints: vec![
				UnaNodeConstraint {
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
						UnaNodeConstraintSource {
							source_node: 0,
							weight: 0.25,
							translation_offset: [0.0; 3],
							rotation_offset: [0.0; 3],
						},
						UnaNodeConstraintSource {
							source_node: 1,
							weight: 0.75,
							translation_offset: [0.0; 3],
							rotation_offset: [0.0; 3],
						},
					],
				},
				UnaNodeConstraint {
					target_node: 3,
					source_node: 0,
					weight: 1.0,
					kind: UnaNodeConstraintKind::Rotation,
					sources: Vec::new(),
				},
			],
			..Default::default()
		};

		assert_eq!(
			scene_node_constraint_counts(&scene),
			SceneNodeConstraintCounts {
				total: 2,
				parent: 1,
				parent_sources: 2,
				parent_multi_source: 1
			}
		);
	}

	#[test]
	fn modular_avatar_blendtree_animator_drives_mesh_local_morph_override_from_base_layer() {
		let mut document = UnaDocument {
			unavatar: Some(un_avatar_core::UnaUnavatarExtension {
				spec_version: "0.1-preview".to_string(),
				source: json!({
					"animator": {
						"controllers": [{
							"name": "Fit_Controller",
							"source": "modularAvatarMergeAnimator",
							"motionBasePath": "AvatarRoot",
							"parameters": [{
								"name": "LeftArmDown_Angle",
								"type": "Float",
								"defaultFloat": 0.0
							}],
							"layers": [{
								"name": "LeftArm_Fit",
								"defaultWeight": 0.0,
								"states": [{
									"name": "Blend Tree",
									"motion": {
										"motionType": "BlendTree",
										"blendType": "Simple1D",
										"blendParameter": "LeftArmDown_Angle",
										"children": [
											{
												"motionType": "AnimationClip",
												"threshold": 0.0,
												"curveBindings": [{
													"path": "ClothPanelMesh",
													"propertyName": "blendShape.(Do not Modify)ArmPit_Fix_L",
													"constantValue": 0.0
												}]
											},
											{
												"motionType": "AnimationClip",
												"threshold": 0.5,
												"curveBindings": [{
													"path": "ClothPanelMesh",
													"propertyName": "blendShape.(Do not Modify)ArmPit_Fix_L",
													"constantValue": 100.0
												}]
											},
											{
												"motionType": "AnimationClip",
												"threshold": 1.0,
												"curveBindings": [{
													"path": "ClothPanelMesh",
													"propertyName": "blendShape.(Do not Modify)ArmPit_Fix_L",
													"constantValue": 0.0
												}]
											}
										]
									}
								}]
							}]
						}]
					}
				}),
			}),
			..Default::default()
		};
		document.runtime_model_mut().set_runtime_parameter_value("LeftArmDown_Angle", 0.5);

		let overrides = animator_morph_overrides_for_doc(&document);

		assert_eq!(
			overrides.get("AvatarRoot/ClothPanelMesh\0(Do not Modify)ArmPit_Fix_L").copied(),
			Some(1.0)
		);
	}

	#[test]
	fn dynamics_angle_animator_center_peak_uses_standard_blend_tree_shape() {
		let mut document = UnaDocument {
			unavatar: Some(un_avatar_core::UnaUnavatarExtension {
				spec_version: "0.1-preview".to_string(),
				source: json!({
					"animator": {
						"controllers": [{
							"name": "Fit_Controller",
							"source": "modularAvatarMergeAnimator",
							"motionBasePath": "AvatarRoot",
							"parameters": [{
								"name": "LeftArmDown_Angle",
								"type": "Float",
								"defaultFloat": 0.0
							}],
							"layers": [{
								"name": "LeftArm_Fit",
								"defaultWeight": 1.0,
								"states": [{
									"name": "Blend Tree",
									"motion": {
										"motionType": "BlendTree",
										"blendType": "Simple1D",
										"blendParameter": "LeftArmDown_Angle",
										"children": [
											{
												"motionType": "AnimationClip",
												"threshold": 0.0,
												"curveBindings": [{
													"path": "ClothPanelMesh",
													"propertyName": "blendShape.(Do not Modify)ArmPit_Fix_L",
													"constantValue": 0.0
												}]
											},
											{
												"motionType": "AnimationClip",
												"threshold": 0.5,
												"curveBindings": [{
													"path": "ClothPanelMesh",
													"propertyName": "blendShape.(Do not Modify)ArmPit_Fix_L",
													"constantValue": 100.0
												}]
											},
											{
												"motionType": "AnimationClip",
												"threshold": 1.0,
												"curveBindings": [{
													"path": "ClothPanelMesh",
													"propertyName": "blendShape.(Do not Modify)ArmPit_Fix_L",
													"constantValue": 0.0
												}]
											}
										]
									}
								}]
							}]
						}]
					}
				}),
			}),
			spring_bones: Some(un_avatar_core::UnaDynamicsSettings {
				groups: vec![un_avatar_core::UnaSpringBoneGroup {
					enabled: true,
					source_id: "physbone:arm".to_string(),
					interaction: Some(un_avatar_core::UnaDynamicsInteraction {
						parameter: "LeftArmDown".to_string(),
						..Default::default()
					}),
					bone_node_indices: vec![0, 1],
					..Default::default()
				}],
				colliders: Vec::new(),
				contacts: Vec::new(),
				constraint_refs: Vec::new(),
			}),
			..Default::default()
		};
		document.runtime_model_mut().set_runtime_parameter_value("LeftArmDown_Angle", 1.0);

		let overrides = animator_morph_overrides_for_doc(&document);

		assert_eq!(
			overrides.get("AvatarRoot/ClothPanelMesh\0(Do not Modify)ArmPit_Fix_L").copied(),
			Some(0.0)
		);

		document.runtime_model_mut().set_runtime_parameter_value("LeftArmDown_Angle", 0.8);
		let overrides = animator_morph_overrides_for_doc(&document);
		assert_eq!(
			overrides.get("AvatarRoot/ClothPanelMesh\0(Do not Modify)ArmPit_Fix_L").copied(),
			Some(0.39999998)
		);

		document.runtime_model_mut().set_runtime_parameter_value("LeftArmDown_Angle", 0.25);
		let overrides = animator_morph_overrides_for_doc(&document);
		assert_eq!(
			overrides.get("AvatarRoot/ClothPanelMesh\0(Do not Modify)ArmPit_Fix_L").copied(),
			Some(0.5)
		);
	}

	#[test]
	fn dynamics_interaction_emits_angle_parameter_from_current_chain_shape() {
		let mut document = UnaDocument {
			scene: Some(UnaSceneSnapshot {
				nodes: vec![
					test_scene_node_with_transform("Root", Mat4::IDENTITY, vec![1]),
					test_scene_node_with_transform("Tip", Mat4::from_translation(Vec3::X), Vec::new()),
				],
				..Default::default()
			}),
			spring_bones: Some(un_avatar_core::UnaDynamicsSettings {
				groups: vec![un_avatar_core::UnaSpringBoneGroup {
					enabled: true,
					source_id: "physbone:test".to_string(),
					interaction: Some(un_avatar_core::UnaDynamicsInteraction {
						parameter: "Test".to_string(),
						..Default::default()
					}),
					limit: Some(un_avatar_core::UnaDynamicsLimit {
						max_angle_x: 90.0,
						max_angle_z: 90.0,
						..Default::default()
					}),
					bone_node_indices: vec![0, 1],
					..Default::default()
				}],
				colliders: Vec::new(),
				contacts: Vec::new(),
				constraint_refs: Vec::new(),
			}),
			..Default::default()
		};
		document.runtime_model_mut().apply_runtime_parameter_initial_values();
		let rest_nodes = vec![
			test_scene_node_with_transform("Root", Mat4::IDENTITY, vec![1]),
			test_scene_node_with_transform("Tip", Mat4::from_translation(Vec3::Y), Vec::new()),
		];

		let values = dynamics_interaction_parameter_values(&document, Some(&rest_nodes));

		assert!(values.get("Test_Angle").copied().unwrap_or(0.0) > 0.99);
		assert_eq!(values.get("Test_IsGrabbed").copied(), Some(0.0));
	}

	#[test]
	fn dynamics_interaction_angle_uses_strongest_chain_segment() {
		let mut document = UnaDocument {
			scene: Some(UnaSceneSnapshot {
				nodes: vec![
					test_scene_node_with_transform("Root", Mat4::IDENTITY, vec![1]),
					test_scene_node_with_transform("Mid", Mat4::from_translation(Vec3::X), vec![2]),
					test_scene_node_with_transform("Tip", Mat4::from_translation(Vec3::new(1.0, 1.0, 0.0)), Vec::new()),
				],
				..Default::default()
			}),
			spring_bones: Some(un_avatar_core::UnaDynamicsSettings {
				groups: vec![un_avatar_core::UnaSpringBoneGroup {
					enabled: true,
					source_id: "physbone:test".to_string(),
					interaction: Some(un_avatar_core::UnaDynamicsInteraction {
						parameter: "Test".to_string(),
						..Default::default()
					}),
					limit: Some(un_avatar_core::UnaDynamicsLimit {
						max_angle_x: 90.0,
						max_angle_z: 90.0,
						..Default::default()
					}),
					bone_node_indices: vec![0, 1, 2],
					..Default::default()
				}],
				colliders: Vec::new(),
				contacts: Vec::new(),
				constraint_refs: Vec::new(),
			}),
			..Default::default()
		};
		document.runtime_model_mut().apply_runtime_parameter_initial_values();
		let rest_nodes = vec![
			test_scene_node_with_transform("Root", Mat4::IDENTITY, vec![1]),
			test_scene_node_with_transform("Mid", Mat4::from_translation(Vec3::Y), vec![2]),
			test_scene_node_with_transform("Tip", Mat4::from_translation(Vec3::new(0.0, 2.0, 0.0)), Vec::new()),
		];

		let values = dynamics_interaction_parameter_values(&document, Some(&rest_nodes));

		assert!(
			values.get("Test_Angle").copied().unwrap_or(0.0) > 0.99,
			"local segment bending must not be diluted by the total root-to-tip vector"
		);
	}

	#[test]
	fn dynamics_interaction_angle_uses_gravity_response_when_chain_has_no_local_deformation() {
		let mut document = UnaDocument {
			scene: Some(UnaSceneSnapshot {
				nodes: vec![
					test_scene_node_with_transform("Root", Mat4::IDENTITY, vec![1]),
					test_scene_node_with_transform("Tip", Mat4::from_translation(Vec3::X), Vec::new()),
				],
				..Default::default()
			}),
			spring_bones: Some(un_avatar_core::UnaDynamicsSettings {
				groups: vec![un_avatar_core::UnaSpringBoneGroup {
					enabled: true,
					source_id: "physbone:test".to_string(),
					interaction: Some(un_avatar_core::UnaDynamicsInteraction {
						parameter: "Test".to_string(),
						..Default::default()
					}),
					limit: Some(un_avatar_core::UnaDynamicsLimit {
						max_angle_x: 90.0,
						max_angle_z: 90.0,
						..Default::default()
					}),
					gravity_dir: [0.0, -1.0, 0.0],
					gravity_power: 1.0,
					bone_node_indices: vec![0, 1],
					..Default::default()
				}],
				colliders: Vec::new(),
				contacts: Vec::new(),
				constraint_refs: Vec::new(),
			}),
			..Default::default()
		};
		document.runtime_model_mut().apply_runtime_parameter_initial_values();
		let rest_nodes = vec![
			test_scene_node_with_transform("Root", Mat4::IDENTITY, vec![1]),
			test_scene_node_with_transform("Tip", Mat4::from_translation(Vec3::X), Vec::new()),
		];

		let values = dynamics_interaction_parameter_values(&document, Some(&rest_nodes));

		assert!(
			values.get("Test_Angle").copied().unwrap_or(0.0) > 0.99,
			"interaction parameters must include the authored gravity response so animator-driven corrective morphs can follow the effective dynamics pose"
		);
	}

	#[test]
	fn dynamics_interaction_angle_uses_center_peak_scale_when_animator_consumes_center_peak_angle() {
		let mut document = UnaDocument {
			unavatar: Some(un_avatar_core::UnaUnavatarExtension {
				spec_version: "0.1-preview".to_string(),
				source: json!({
					"animator": {
						"controllers": [{
							"name": "Fit_Controller",
							"source": "modularAvatarMergeAnimator",
							"motionBasePath": "AvatarRoot",
							"parameters": [{
								"name": "LeftArmDown_Angle",
								"type": "Float",
								"defaultFloat": 0.0
							}],
							"layers": [{
								"name": "LeftArm_Fit",
								"defaultWeight": 1.0,
								"states": [{
									"name": "Blend Tree",
									"motion": {
										"motionType": "BlendTree",
										"blendType": "Simple1D",
										"blendParameter": "LeftArmDown_Angle",
										"children": [
											{
												"motionType": "AnimationClip",
												"threshold": 0.0,
												"curveBindings": [{
													"path": "ClothPanelMesh",
													"propertyName": "blendShape.(Do not Modify)ArmPit_Fix_L",
													"constantValue": 0.0
												}]
											},
											{
												"motionType": "AnimationClip",
												"threshold": 0.5,
												"curveBindings": [{
													"path": "ClothPanelMesh",
													"propertyName": "blendShape.(Do not Modify)ArmPit_Fix_L",
													"constantValue": 100.0
												}]
											},
											{
												"motionType": "AnimationClip",
												"threshold": 1.0,
												"curveBindings": [{
													"path": "ClothPanelMesh",
													"propertyName": "blendShape.(Do not Modify)ArmPit_Fix_L",
													"constantValue": 0.0
												}]
											}
										]
									}
								}]
							}]
						}]
					}
				}),
			}),
			scene: Some(UnaSceneSnapshot {
				nodes: vec![
					test_scene_node_with_transform("Root", Mat4::IDENTITY, vec![1]),
					test_scene_node_with_transform("Tip", Mat4::from_translation(Vec3::X), Vec::new()),
				],
				..Default::default()
			}),
			spring_bones: Some(un_avatar_core::UnaDynamicsSettings {
				groups: vec![un_avatar_core::UnaSpringBoneGroup {
					enabled: true,
					source_id: "physbone:arm".to_string(),
					interaction: Some(un_avatar_core::UnaDynamicsInteraction {
						parameter: "LeftArmDown".to_string(),
						..Default::default()
					}),
					limit: Some(un_avatar_core::UnaDynamicsLimit {
						max_angle_x: 90.0,
						max_angle_z: 90.0,
						..Default::default()
					}),
					gravity_dir: [0.0, -1.0, 0.0],
					gravity_power: 1.0,
					bone_node_indices: vec![0, 1],
					..Default::default()
				}],
				colliders: Vec::new(),
				contacts: Vec::new(),
				constraint_refs: Vec::new(),
			}),
			..Default::default()
		};
		document.runtime_model_mut().apply_runtime_parameter_initial_values();
		let rest_nodes = vec![
			test_scene_node_with_transform("Root", Mat4::IDENTITY, vec![1]),
			test_scene_node_with_transform("Tip", Mat4::from_translation(Vec3::X), Vec::new()),
		];

		let values = dynamics_interaction_parameter_values(&document, Some(&rest_nodes));
		assert_eq!(values.get("LeftArmDown_Angle").copied(), Some(0.5));
		document.runtime_model_mut().set_runtime_parameter_values(values);
		let overrides = animator_morph_overrides_for_doc(&document);

		assert_eq!(
			overrides.get("AvatarRoot/ClothPanelMesh\0(Do not Modify)ArmPit_Fix_L").copied(),
			Some(1.0)
		);
	}

	#[test]
	fn dynamics_interaction_angle_normalizer_uses_narrow_hinge_axis() {
		let limit = un_avatar_core::UnaDynamicsLimit {
			limit_type: "Hinge".to_string(),
			max_angle_x: 90.0,
			max_angle_z: 45.0,
			..Default::default()
		};

		assert_eq!(dynamics_interaction_angle_normalizer(Some(&limit)), 45.0);

		let limit = un_avatar_core::UnaDynamicsLimit {
			limit_type: "Polar".to_string(),
			max_angle_x: 90.0,
			max_angle_z: 45.0,
			..Default::default()
		};

		assert_eq!(dynamics_interaction_angle_normalizer(Some(&limit)), 90.0);
	}

	#[test]
	fn dynamics_interaction_angle_skips_solver_parent_anchor() {
		let mut document = UnaDocument {
			scene: Some(UnaSceneSnapshot {
				nodes: vec![
					test_scene_node_with_transform("Upperarm", Mat4::IDENTITY, vec![1]),
					test_scene_node_with_transform("Arm_Phys", Mat4::from_translation(Vec3::X), vec![2]),
					test_scene_node_with_transform("Arm_Phys Endpoint", Mat4::from_translation(Vec3::NEG_Y), Vec::new()),
				],
				..Default::default()
			}),
			spring_bones: Some(un_avatar_core::UnaDynamicsSettings {
				groups: vec![un_avatar_core::UnaSpringBoneGroup {
					enabled: true,
					source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
					source_id: "physbone:arm".to_string(),
					interaction: Some(un_avatar_core::UnaDynamicsInteraction {
						parameter: "LeftArmDown".to_string(),
						..Default::default()
					}),
					limit: Some(un_avatar_core::UnaDynamicsLimit {
						max_angle_x: 90.0,
						max_angle_z: 90.0,
						..Default::default()
					}),
					gravity_dir: [0.0, -1.0, 0.0],
					gravity_power: 1.0,
					interaction_chain_start_index: 1,
					bone_node_indices: vec![0, 1, 2],
					..Default::default()
				}],
				colliders: Vec::new(),
				contacts: Vec::new(),
				constraint_refs: Vec::new(),
			}),
			..Default::default()
		};
		document.runtime_model_mut().apply_runtime_parameter_initial_values();
		let rest_nodes = document.scene.as_ref().unwrap().nodes.clone();

		let values = dynamics_interaction_parameter_values(&document, Some(&rest_nodes));

		assert!(
			values.get("LeftArmDown_Angle").copied().unwrap_or(1.0) < 0.01,
			"prepended solver anchors must not drive authored PhysBone interaction sensors"
		);
	}

	#[test]
	fn dynamics_interaction_angle_legacy_anchor_skip_uses_interaction_metadata_not_source_kind() {
		let mut document = UnaDocument {
			scene: Some(UnaSceneSnapshot {
				nodes: vec![
					test_scene_node_with_transform("Upperarm", Mat4::IDENTITY, vec![1]),
					test_scene_node_with_transform("Arm_Phys", Mat4::from_translation(Vec3::X), vec![2]),
					test_scene_node_with_transform("Arm_Phys Endpoint", Mat4::from_translation(Vec3::NEG_Y), Vec::new()),
				],
				..Default::default()
			}),
			spring_bones: Some(un_avatar_core::UnaDynamicsSettings {
				groups: vec![un_avatar_core::UnaSpringBoneGroup {
					enabled: true,
					source_id: "custom:Arm_Phys".to_string(),
					interaction: Some(un_avatar_core::UnaDynamicsInteraction {
						parameter: "LeftArmDown".to_string(),
						..Default::default()
					}),
					limit: Some(un_avatar_core::UnaDynamicsLimit {
						max_angle_x: 90.0,
						max_angle_z: 90.0,
						..Default::default()
					}),
					gravity_dir: [0.0, -1.0, 0.0],
					gravity_power: 1.0,
					bone_node_indices: vec![0, 1, 2],
					..Default::default()
				}],
				colliders: Vec::new(),
				contacts: Vec::new(),
				constraint_refs: Vec::new(),
			}),
			..Default::default()
		};
		document.runtime_model_mut().apply_runtime_parameter_initial_values();
		let rest_nodes = document.scene.as_ref().unwrap().nodes.clone();

		let values = dynamics_interaction_parameter_values(&document, Some(&rest_nodes));

		assert!(
			values.get("LeftArmDown_Angle").copied().unwrap_or(1.0) < 0.01,
			"legacy interaction anchor detection should not depend on VRC source kind"
		);
	}

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
	fn modular_avatar_menu_components_include_synthetic_vrc_expression_controls() {
		let unavatar = un_avatar_core::UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: json!({
				"modularAvatar": {
					"components": [{
						"shortType": "VRCExpressionsMenuControl",
						"enabled": true,
						"fields": {
							"hierarchyPath": "VRC Menu/Accessories/Hat",
							"siblingIndex": 2,
							"Control": {
								"name": "Hat",
								"type": "Toggle",
								"parameter": { "name": "Hat" },
								"value": 1.0
							}
						}
					}]
				}
			}),
		};

		let components = modular_avatar_menu_components(&unavatar);
		assert_eq!(components.len(), 1);
		assert_eq!(components[0].menu_key, "component:0");
		assert_eq!(components[0].hierarchy_path.as_deref(), Some("VRC Menu/Accessories/Hat"));
		assert_eq!(components[0].sibling_index, Some(2));
		assert_eq!(components[0].label.as_deref(), Some("Hat"));
		assert_eq!(components[0].parameter_name.as_deref(), Some("Hat"));
		assert_eq!(components[0].value, Some(1.0));
	}

	#[test]
	fn menu_action_candidates_include_metadata_only_vrc_controls() {
		let unavatar = un_avatar_core::UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: json!({
				"modularAvatar": {
					"components": [{
						"shortType": "VRCExpressionsMenuControl",
						"enabled": true,
						"fields": {
							"hierarchyPath": "VRC Menu/Object/Tail",
							"siblingIndex": 1,
							"Control": {
								"name": "Tail",
								"type": "Toggle",
								"parameter": { "name": "Tail" },
								"value": 1.0
							}
						}
					}]
				}
			}),
		};
		let actions = un_avatar_core::UnaRuntimeActionSet { actions: Vec::new() };

		let candidates = menu_action_candidates_from_runtime(Some(&unavatar), &actions, None).unwrap();
		assert_eq!(candidates.len(), 1);
		assert_eq!(candidates[0].menu_key, "component:0");
		assert_eq!(candidates[0].menu_path, vec!["Tail".to_string()]);
		assert_eq!(candidates[0].parameter_name, "Tail");
		assert_eq!(candidates[0].parameter_value, 1.0);
		assert_eq!(candidates[0].action_id, "menu:component:0");
		assert_eq!(candidates[0].match_kind, "metadata");
		assert!(candidates[0].available);
		assert_eq!(candidates[0].effect_count, 0);
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
	fn runtime_action_statuses_gate_scene_target_actions_by_visible_parent() {
		let mut scene = un_avatar_core::UnaSceneSnapshot {
			nodes: vec![test_scene_node(vec![1]), test_scene_node(Vec::new())],
			roots: vec![0],
			..Default::default()
		};
		scene.nodes[0].name = Some("Outfit".to_string());
		scene.nodes[1].name = Some("Hat".to_string());
		scene.nodes[1].visible = false;
		let actions = un_avatar_core::UnaRuntimeActionSet {
			actions: vec![un_avatar_core::UnaRuntimeAction {
				id: "hat:on".to_string(),
				label: "Hat ON".to_string(),
				effects: vec![un_avatar_core::UnaRuntimeActionEffect::NodeVisibility {
					target: un_avatar_core::UnaRuntimeNodeTarget {
						path: Some("Outfit/Hat".to_string()),
						..Default::default()
					},
					visible: true,
				}],
				..Default::default()
			}],
		};

		let statuses = runtime_action_statuses(&actions, Some(&scene), &BTreeMap::new());
		assert!(statuses[0].available);

		scene.nodes[0].visible = false;
		let statuses = runtime_action_statuses(&actions, Some(&scene), &BTreeMap::new());
		assert!(!statuses[0].available);
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
							"assetGroups": ["avatar:base"]
						}, {
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
		assert_eq!(plan.mode, "all-resident");
		assert_eq!(plan.active_asset_groups, vec!["outfit:coat".to_string()]);
		assert_eq!(
			plan.declared_asset_groups,
			vec!["avatar:base".to_string(), "outfit:coat".to_string()]
		);
		assert!(!plan.scoped_upload_supported);
		assert!(plan.all_resident);
		assert_eq!(plan.missing_active_asset_groups, vec!["outfit:coat".to_string()]);
		assert_eq!(plan.resident_mesh_primitive_count, 0);
		assert_eq!(plan.resident_material_count, 0);
		assert_eq!(plan.resident_image_count, 0);
		assert_eq!(plan.resident_dynamics_count, 0);
		assert!(plan
			.reason
			.contains("mesh/texture/material assets do not yet carry group ownership metadata"));
	}

	#[test]
	fn wardrobe_asset_upload_plan_preserves_empty_base_asset_group() {
		let mut document = un_avatar_core::UnaDocument {
			scene: Some(un_avatar_core::UnaSceneSnapshot {
				asset_group_ownership: vec![un_avatar_core::UnaSceneAssetGroupOwnership {
					group_id: String::new(),
					mesh_primitives: vec![un_avatar_core::UnaMeshPrimitiveKey {
						mesh_index: 0,
						primitive_index: 0,
					}],
					materials: vec![0],
					images: vec![0],
					dynamics_source_ids: Vec::new(),
				}],
				..Default::default()
			}),
			unavatar: Some(un_avatar_core::UnaUnavatarExtension {
				spec_version: "0.1-preview".to_string(),
				source: serde_json::json!({
					"wardrobe": {
						"sets": [{
							"id": "base",
							"assetGroups": [""]
						}]
					}
				}),
			}),
			..Default::default()
		};
		document.runtime_model_mut().set_active_asset_groups(vec![String::new()]);

		let plan = wardrobe_asset_upload_plan_for_document(&document);
		assert_eq!(plan.declared_asset_groups, vec![String::new()]);
		assert_eq!(plan.active_asset_groups, vec![String::new()]);
		assert_eq!(plan.mode, WARDROBE_ASSET_UPLOAD_MODE_RESOURCE_SCOPED);
		assert!(!plan.all_resident);
		assert!(plan.scoped_upload_supported);
		assert!(plan.missing_active_asset_groups.is_empty());
		assert_eq!(plan.resident_mesh_primitive_count, 1);
		assert_eq!(plan.resident_material_count, 1);
		assert_eq!(plan.resident_image_count, 1);
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
				active_draws_using_inactive_cube_texture_count: 1,
				inactive_cube_textures_used_by_active_draw_count: 1,
				inactive_cube_textures_used_by_active_draw: vec![6],
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
		assert_eq!(plan.active_draws_using_inactive_cube_texture_count, 1);
		assert_eq!(plan.inactive_cube_textures_used_by_active_draw_count, 1);
		assert_eq!(plan.inactive_cube_textures_used_by_active_draw, vec![6]);
		assert_eq!(plan.total_material_slot_count, 5);
		assert_eq!(plan.resident_material_slot_count, 4);
		assert_eq!(plan.inactive_material_slot_count, 1);
		assert_eq!(plan.active_draws_using_inactive_material_slot_count, 1);
		assert_eq!(plan.inactive_material_slots_used_by_active_draw_count, 1);
		assert_eq!(plan.inactive_material_slots_used_by_active_draw, vec![4]);
		assert_eq!(plan.pending_image_texture_upload_count, 1);
		assert_eq!(plan.pending_cube_texture_upload_count, 1);
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
		let cube_indices = (50..50 + WARDROBE_RESIDENCY_GAP_INDEX_STATUS_LIMIT + 1).collect::<Vec<_>>();
		let material_indices = (100..100 + WARDROBE_RESIDENCY_GAP_INDEX_STATUS_LIMIT + 3).collect::<Vec<_>>();
		let work = wardrobe_scoped_upload_work_for_active_gaps(Some(SceneMeshActiveResidencyGaps {
			inactive_image_texture_indices: image_indices.clone(),
			inactive_cube_texture_indices: cube_indices.clone(),
			inactive_material_slot_indices: material_indices.clone(),
			active_draws_using_inactive_image_texture_count: 4,
			active_draws_using_inactive_cube_texture_count: 3,
			active_draws_using_inactive_material_slot_count: 5,
		}));

		assert!(work.has_pending_uploads());
		assert_eq!(work.image_texture_indices, image_indices);
		assert_eq!(work.cube_texture_indices, cube_indices);
		assert_eq!(work.material_slot_indices, material_indices);
		assert_eq!(work.active_draws_using_inactive_image_texture_count, 4);
		assert_eq!(work.active_draws_using_inactive_cube_texture_count, 3);
		assert_eq!(work.active_draws_using_inactive_material_slot_count, 5);
	}

	#[test]
	fn sorted_unique_index_union_merges_sorted_inputs_without_duplicates() {
		assert_eq!(sorted_unique_index_union(&[1, 3, 8], &[2, 3, 4, 9]), vec![1, 2, 3, 4, 8, 9]);
		assert_eq!(sorted_unique_index_union(&[], &[2, 5]), vec![2, 5]);
		assert_eq!(sorted_unique_index_union(&[1, 4], &[]), vec![1, 4]);
		assert!(sorted_unique_index_union(&[], &[]).is_empty());
	}

	#[test]
	fn sorted_index_difference_removes_sorted_exclusions() {
		assert_eq!(sorted_index_difference(&[0, 1, 3, 5, 8], &[1, 2, 5, 9]), vec![0, 3, 8]);
		assert_eq!(sorted_index_difference(&[1, 2], &[]), vec![1, 2]);
		assert!(sorted_index_difference(&[1, 2], &[0, 1, 2, 3]).is_empty());
		assert!(sorted_index_difference(&[], &[1, 2]).is_empty());
	}

	#[test]
	fn restore_runtime_scene_transforms_to_rest_resets_pose_without_visibility() {
		let mut rest_nodes = vec![test_scene_node(vec![1]), test_scene_node(Vec::new())];
		rest_nodes[0].transform[12] = 1.0;
		rest_nodes[1].transform[13] = 2.0;
		let mut posed_nodes = rest_nodes.clone();
		posed_nodes[0].transform[12] = 10.0;
		posed_nodes[1].transform[13] = 20.0;
		posed_nodes[1].visible = false;
		let mut document = UnaDocument {
			scene: Some(UnaSceneSnapshot {
				nodes: posed_nodes,
				roots: vec![0],
				..Default::default()
			}),
			humanoid_profile: Some(HumanoidProfile::default()),
			..Default::default()
		};

		restore_runtime_scene_transforms_to_rest(&mut document, &rest_nodes).expect("restore rest pose");

		let nodes = document.runtime_model().scene_nodes().expect("scene nodes");
		assert_eq!(nodes[0].transform, rest_nodes[0].transform);
		assert_eq!(nodes[1].transform, rest_nodes[1].transform);
		assert!(!nodes[1].visible);
	}

	#[test]
	fn startup_progress_and_wardrobe_transition_are_distinct_frame_roles() {
		let wardrobe_changing = WardrobeChangingBillboardFrame {
			time_secs: 0.0,
			billboard_center: [0.0, 1.0, 0.0],
			billboard_size: 0.5,
			billboard_view_proj: [[0.0; 4]; 4],
			billboard_camera_pos: [0.0, 0.0, 2.0],
		};
		let startup_overlay = StartupProgressOverlayFrame {
			time_secs: 0.0,
			progress: 0.5,
			phase: 1.0,
			rect_center: [0.5, 0.5],
			rect_half_size: [0.25, 0.1],
		};
		let runtime = RenderedFrameRole::RuntimeAvatar;
		let startup = RenderedFrameRole::RendererStartup(RendererStartupPresentation {
			progress_overlay: Some(startup_overlay),
		});
		let wardrobe = RenderedFrameRole::WardrobeTransition(WardrobeTransitionPresentation {
			changing_billboard: wardrobe_changing,
		});
		assert_eq!(runtime.spout2_delivery(false), Spout2FrameDelivery::Unavailable);
		assert_eq!(runtime.spout2_delivery(true), Spout2FrameDelivery::RuntimeOutput);
		assert_eq!(
			startup.spout2_delivery(true),
			Spout2FrameDelivery::SuppressedRendererStartup,
			"startup presentation is renderer-local and must not be sent to Spout2"
		);
		assert_eq!(
			wardrobe.spout2_delivery(true),
			Spout2FrameDelivery::RuntimeOutput,
			"wardrobe changing billboard is an OBS-facing transition and must remain visible on Spout2"
		);
		assert!(wardrobe.is_wardrobe_transition_only());
		assert!(!startup.is_wardrobe_transition_only());
		assert!(startup.startup_overlay().is_some());
		assert!(startup.wardrobe_transition_billboard().is_none());
		assert!(wardrobe.startup_overlay().is_none());
		assert!(wardrobe.wardrobe_transition_billboard().is_some());
	}

	#[test]
	fn spout_sender_initialization_is_deferred_until_runtime_output() {
		let source = include_str!("gpu.rs");
		let new_body = source
			.split("impl GpuState {")
			.nth(1)
			.and_then(|rest| rest.split("pub fn new_shell(").nth(1))
			.and_then(|rest| rest.split("\n\tpub fn expression_presets").next())
			.expect("GpuState::new_shell body exists");
		let new_windows_spout_block = new_body
			.split("#[cfg(windows)]\n\t\tlet spout_launch")
			.nth(1)
			.and_then(|rest| rest.split("#[cfg(not(windows))]").next())
			.expect("GpuState::new Windows Spout2 block exists");
		assert!(
			new_windows_spout_block.contains("let spout = None;"),
			"GpuState::new_shell must retain Spout2 configuration without creating a sender during renderer-local startup presentation"
		);
		assert!(
			!new_windows_spout_block.contains("SpoutCapture::try_new"),
			"Spout2 sender creation belongs to runtime output frames, not startup initialization"
		);

		let render_frame = source
			.split("pub fn render_frame(")
			.nth(1)
			.and_then(|rest| rest.split("let t_surface0 = Instant::now();").next())
			.expect("render_frame Spout2 policy block exists");
		assert!(
			render_frame.contains("Spout2FrameDelivery::RuntimeOutput => self.ensure_runtime_spout_output()"),
			"runtime avatar and wardrobe transition frames may initialize Spout2 output"
		);
		assert!(
			render_frame.contains("Spout2FrameDelivery::SuppressedRendererStartup | Spout2FrameDelivery::Unavailable => false"),
			"renderer-local startup frames must neither initialize nor send Spout2 output"
		);
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
	fn effective_window_backend_keeps_vulkan_for_opaque_window() {
		assert_eq!(effective_window_backend(RenderBackend::Vulkan, false), RenderBackend::Vulkan);
	}

	#[cfg(windows)]
	#[test]
	fn effective_window_backend_uses_dx12_only_for_transparent_vulkan_window() {
		assert_eq!(effective_window_backend(RenderBackend::Vulkan, true), RenderBackend::Dx12);
		assert_eq!(effective_window_backend(RenderBackend::Dx12, true), RenderBackend::Dx12);
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
