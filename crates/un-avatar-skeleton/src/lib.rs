//! UN Avatar — スケルトン・Humanoid・リターゲット（bootstrap）。
//!
//! 設計: `docs/crate-io-plugin-plan.md` §4.4

#![forbid(unsafe_code)]

mod bone_colliders;
mod humanoid_retarget;
mod spring_bones;

pub use bone_colliders::{
	build_bone_colliders, build_dynamics_bone_colliders, build_dynamics_bone_colliders_with_sources, build_runtime_bone_colliders,
	collider_stats, local_capsule_world, local_sphere_world, BoneColliderConfig, BoneColliderPartRadiiMm, BoneColliderPrimitive,
	BoneColliderSource, BoneColliderStats, RuntimeBoneColliderPrimitive,
};
pub use humanoid_retarget::{
	apply_humanoid_pose_to_scene, apply_humanoid_pose_to_scene_with_rest, apply_node_constraints_to_scene,
	apply_un_motion_frame_to_document, apply_un_motion_frame_to_document_with_context, apply_un_motion_frame_to_document_with_rest,
	humanoid_bone_profile_key, ApplyUnMotionFrameOpts, HumanoidRetargetContext,
};
pub use spring_bones::{
	DynamicsCategoryDefinition, DynamicsCategoryOverride, DynamicsPhysicsConfig, DynamicsPhysicsParams, DynamicsSimulator, DynamicsSolver,
	DynamicsStepProfile, DynamicsTimeMode, SpringBoneCategoryDefinition, SpringBoneCategoryOverride, SpringBonePhysicsConfig,
	SpringBonePhysicsParams, SpringBoneSimulator, SpringBoneSolver, SpringBoneTimeMode,
};
pub use un_avatar_types::HumanoidProfile;

/// Skeleton / リターゲット用プレースホルダ。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SkeletonStub;
