//! UN Avatar — スケルトン・Humanoid・リターゲット（bootstrap）。
//!
//! 設計: `docs/crate-io-plugin-plan.md` §4.4

#![forbid(unsafe_code)]

mod bone_colliders;
mod humanoid_retarget;
mod spring_bones;

pub use bone_colliders::{
	build_bone_colliders, build_dynamics_bone_colliders, build_dynamics_bone_colliders_with_sources, build_runtime_bone_colliders,
	collider_stats, distance_point_segment, local_capsule_world, local_plane_world, local_sphere_world, BoneColliderConfig,
	BoneColliderPartRadiiMm, BoneColliderPrimitive, BoneColliderSource, BoneColliderStats, RuntimeBoneColliderPrimitive,
};
pub use humanoid_retarget::{
	apply_humanoid_pose_to_scene, apply_humanoid_pose_to_scene_with_rest, apply_node_constraints_to_scene,
	apply_un_motion_frame_to_document, apply_un_motion_frame_to_document_with_context, apply_un_motion_frame_to_document_with_rest,
	humanoid_bone_profile_key, ApplyUnMotionFrameOpts, HumanoidRetargetContext,
};
pub use spring_bones::{
	annotate_dynamics_response_group_visibility, apply_dynamics_mesh_cloth_assist_to_vertices, classify_dynamics_group_category,
	dynamics_group_match_text, dynamics_mesh_cloth_assist_body_joint_matches, dynamics_mesh_cloth_assist_cloth_joint_matches,
	dynamics_mesh_cloth_assist_deforming_nodes, dynamics_mesh_cloth_assist_joint_roles, dynamics_mesh_cloth_assist_mesh_matches,
	dynamics_mesh_cloth_assist_mesh_matches_with_categories, dynamics_mesh_cloth_assist_transfer_candidate, dynamics_normalize_match_text,
	dynamics_normalized_token_filter_matches, dynamics_token_filter_matches, for_each_dynamics_mesh_cloth_assist_neighbor,
	DynamicsCategoryDefinition, DynamicsCategoryOverride, DynamicsColliderAugmentOverride, DynamicsGroupOverride, DynamicsMatchOverride,
	DynamicsMeshClothAssistConfig, DynamicsMeshClothAssistJointRole, DynamicsMeshClothAssistTransferCandidate,
	DynamicsMeshClothAssistTransferKind, DynamicsMeshClothAssistVertex, DynamicsPhysicsConfig, DynamicsPhysicsParams,
	DynamicsResponseCategorySummary, DynamicsResponseGroupSummary, DynamicsSimulator, DynamicsSolver, DynamicsStepProfile,
	DynamicsSurfaceConstraint, DynamicsTailSample, DynamicsTimeMode, DynamicsVisualTargetContext, SpringBoneCategoryDefinition,
	SpringBoneCategoryOverride, SpringBoneGroupOverride, SpringBoneMatchOverride, SpringBonePhysicsConfig, SpringBonePhysicsParams,
	SpringBoneSimulator, SpringBoneSolver, SpringBoneTimeMode,
};
pub use un_avatar_types::HumanoidProfile;

/// Skeleton / リターゲット用プレースホルダ。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SkeletonStub;
