//! UN Avatar — スケルトン・Humanoid・リターゲット（bootstrap）。
//!
//! 設計: `docs/crate-io-plugin-plan.md` §4.4

#![forbid(unsafe_code)]

mod bone_colliders;
mod humanoid_retarget;
mod spring_bones;

pub use bone_colliders::{
	build_bone_colliders, collider_stats, BoneColliderConfig, BoneColliderPartRadiiMm, BoneColliderPrimitive, BoneColliderSource,
	BoneColliderStats,
};
pub use humanoid_retarget::{
	apply_humanoid_pose_to_scene, apply_humanoid_pose_to_scene_with_rest, apply_node_constraints_to_scene,
	apply_un_motion_frame_to_document, apply_un_motion_frame_to_document_with_rest, humanoid_bone_profile_key, ApplyUnMotionFrameOpts,
};
pub use spring_bones::{
	SpringBoneCategoryDefinition, SpringBoneCategoryOverride, SpringBonePhysicsConfig, SpringBonePhysicsParams, SpringBoneSimulator,
	SpringBoneSolver, SpringBoneTimeMode,
};
pub use un_avatar_types::HumanoidProfile;

/// Skeleton / リターゲット用プレースホルダ。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SkeletonStub;
