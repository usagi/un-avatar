//! Humanoid リターゲット：`UNMotionFrame` → `UnaSceneSnapshot` ノード局所行列。

use glam::{EulerRot, Mat4, Quat, Vec3};
use std::collections::BTreeMap;
use un_avatar_core::{
	resolved_scene_roots, UnaDocument, UnaHumanoidRuntimeBasis, UnaNodeConstraint, UnaNodeConstraintAimAxis, UnaNodeConstraintAxis,
	UnaNodeConstraintKind, UnaRuntimeRetargetInputs, UnaSceneNode, UnaSceneSnapshot,
};
use un_avatar_types::HumanoidProfile;
use un_motion_frame::{CoordinateSpace, Finger, HandMotion, HumanoidBone, HumanoidPose, SampleState, TransformSample, UNMotionFrame};

type StringIndexLookup = Vec<(String, usize)>;
type ProfileNodeLookup = StringIndexLookup;
type ExpressionLookupEntries = StringIndexLookup;
const NO_PARENT: usize = usize::MAX;

type TargetHumanoidBasis = UnaHumanoidRuntimeBasis;

/// [`HumanoidBone`] を VRM / [`HumanoidProfile`] で使う小文字キーへ（import 時と同じ規約）。
pub fn humanoid_bone_profile_key(bone: HumanoidBone) -> &'static str {
	match bone {
		HumanoidBone::Hips => "hips",
		HumanoidBone::Spine => "spine",
		HumanoidBone::Chest => "chest",
		HumanoidBone::UpperChest => "upperchest",
		HumanoidBone::Neck => "neck",
		HumanoidBone::Head => "head",
		HumanoidBone::LeftShoulder => "leftshoulder",
		HumanoidBone::LeftUpperArm => "leftupperarm",
		HumanoidBone::LeftLowerArm => "leftlowerarm",
		HumanoidBone::LeftHand => "lefthand",
		HumanoidBone::RightShoulder => "rightshoulder",
		HumanoidBone::RightUpperArm => "rightupperarm",
		HumanoidBone::RightLowerArm => "rightlowerarm",
		HumanoidBone::RightHand => "righthand",
		HumanoidBone::LeftUpperLeg => "leftupperleg",
		HumanoidBone::LeftLowerLeg => "leftlowerleg",
		HumanoidBone::LeftFoot => "leftfoot",
		HumanoidBone::LeftToes => "lefttoes",
		HumanoidBone::RightUpperLeg => "rightupperleg",
		HumanoidBone::RightLowerLeg => "rightlowerleg",
		HumanoidBone::RightFoot => "rightfoot",
		HumanoidBone::RightToes => "righttoes",
		HumanoidBone::LeftEye => "lefteye",
		HumanoidBone::RightEye => "righteye",
		HumanoidBone::Jaw => "jaw",
	}
}

fn humanoid_bone_index(bone: HumanoidBone) -> usize {
	match bone {
		HumanoidBone::Hips => 0,
		HumanoidBone::Spine => 1,
		HumanoidBone::Chest => 2,
		HumanoidBone::UpperChest => 3,
		HumanoidBone::Neck => 4,
		HumanoidBone::Head => 5,
		HumanoidBone::LeftShoulder => 6,
		HumanoidBone::LeftUpperArm => 7,
		HumanoidBone::LeftLowerArm => 8,
		HumanoidBone::LeftHand => 9,
		HumanoidBone::RightShoulder => 10,
		HumanoidBone::RightUpperArm => 11,
		HumanoidBone::RightLowerArm => 12,
		HumanoidBone::RightHand => 13,
		HumanoidBone::LeftUpperLeg => 14,
		HumanoidBone::LeftLowerLeg => 15,
		HumanoidBone::LeftFoot => 16,
		HumanoidBone::LeftToes => 17,
		HumanoidBone::RightUpperLeg => 18,
		HumanoidBone::RightLowerLeg => 19,
		HumanoidBone::RightFoot => 20,
		HumanoidBone::RightToes => 21,
		HumanoidBone::LeftEye => 22,
		HumanoidBone::RightEye => 23,
		HumanoidBone::Jaw => 24,
	}
}

const HUMANOID_PROFILE_BONES: &[HumanoidBone] = &[
	HumanoidBone::Hips,
	HumanoidBone::Spine,
	HumanoidBone::Chest,
	HumanoidBone::UpperChest,
	HumanoidBone::Neck,
	HumanoidBone::Head,
	HumanoidBone::LeftShoulder,
	HumanoidBone::LeftUpperArm,
	HumanoidBone::LeftLowerArm,
	HumanoidBone::LeftHand,
	HumanoidBone::RightShoulder,
	HumanoidBone::RightUpperArm,
	HumanoidBone::RightLowerArm,
	HumanoidBone::RightHand,
	HumanoidBone::LeftUpperLeg,
	HumanoidBone::LeftLowerLeg,
	HumanoidBone::LeftFoot,
	HumanoidBone::LeftToes,
	HumanoidBone::RightUpperLeg,
	HumanoidBone::RightLowerLeg,
	HumanoidBone::RightFoot,
	HumanoidBone::RightToes,
	HumanoidBone::LeftEye,
	HumanoidBone::RightEye,
	HumanoidBone::Jaw,
];

const HUMANOID_PROFILE_BONE_COUNT: usize = 25;

fn convert_rotation_from_coordinate_space(rotation: Quat, coordinate_space: CoordinateSpace, target_basis: TargetHumanoidBasis) -> Quat {
	match coordinate_space {
		CoordinateSpace::Vmc => match target_basis {
			TargetHumanoidBasis::Vrm0 => Quat::from_xyzw(-rotation.x, -rotation.y, rotation.z, rotation.w),
			TargetHumanoidBasis::Vrm1 | TargetHumanoidBasis::UnavatarUnity => {
				Quat::from_xyzw(rotation.x, -rotation.y, -rotation.z, rotation.w)
			}
			TargetHumanoidBasis::Native => rotation,
		},
		_ => rotation,
	}
}

fn convert_translation_from_coordinate_space(
	translation: Vec3,
	coordinate_space: CoordinateSpace,
	target_basis: TargetHumanoidBasis,
) -> Vec3 {
	match coordinate_space {
		CoordinateSpace::Vmc => match target_basis {
			TargetHumanoidBasis::Vrm0 => Vec3::new(translation.x, translation.y, -translation.z),
			TargetHumanoidBasis::Vrm1 | TargetHumanoidBasis::UnavatarUnity => Vec3::new(-translation.x, translation.y, translation.z),
			TargetHumanoidBasis::Native => translation,
		},
		_ => translation,
	}
}

fn transform_sample_rotation(t: &TransformSample, coordinate_space: CoordinateSpace, target_basis: TargetHumanoidBasis) -> Quat {
	let mut rotation = t
		.rotation
		.as_ref()
		.map(|quat| Quat::from_xyzw(quat.x, quat.y, quat.z, quat.w))
		.unwrap_or(Quat::IDENTITY);
	rotation = convert_rotation_from_coordinate_space(rotation, coordinate_space, target_basis);
	if rotation.length_squared() > 1e-20 {
		rotation = rotation.normalize();
	} else {
		rotation = Quat::IDENTITY;
	}
	rotation
}

fn transform_sample_translation(t: &TransformSample, coordinate_space: CoordinateSpace, target_basis: TargetHumanoidBasis) -> Vec3 {
	let translation = t
		.translation
		.as_ref()
		.map(|value| Vec3::new(value.x, value.y, value.z))
		.unwrap_or(Vec3::ZERO);
	convert_translation_from_coordinate_space(translation, coordinate_space, target_basis)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UnmotionHumanoidRole {
	Root,
	BodyBone(HumanoidBone),
	HandWrist,
	HandFinger,
}

fn unavatar_unmotion_limb_uses_vrm0_like_axis(role: UnmotionHumanoidRole) -> bool {
	matches!(role, UnmotionHumanoidRole::HandWrist)
}

fn convert_unmotion_humanoid_rotation_to_target(rotation: Quat, target_basis: TargetHumanoidBasis, role: UnmotionHumanoidRole) -> Quat {
	match target_basis {
		TargetHumanoidBasis::Vrm0 if matches!(role, UnmotionHumanoidRole::HandFinger) => rotation,
		TargetHumanoidBasis::Vrm0 if matches!(role, UnmotionHumanoidRole::HandWrist) => rotation,
		TargetHumanoidBasis::Vrm0 => Quat::from_xyzw(-rotation.x, -rotation.y, rotation.z, rotation.w),
		TargetHumanoidBasis::Vrm1 if matches!(role, UnmotionHumanoidRole::HandWrist | UnmotionHumanoidRole::HandFinger) => rotation,
		TargetHumanoidBasis::Vrm1 => Quat::from_xyzw(rotation.x, -rotation.y, -rotation.z, rotation.w),
		TargetHumanoidBasis::UnavatarUnity if matches!(role, UnmotionHumanoidRole::HandFinger) => rotation,
		TargetHumanoidBasis::UnavatarUnity if unavatar_unmotion_limb_uses_vrm0_like_axis(role) => {
			Quat::from_xyzw(-rotation.x, -rotation.y, rotation.z, rotation.w)
		}
		TargetHumanoidBasis::UnavatarUnity => Quat::from_xyzw(rotation.x, -rotation.y, -rotation.z, rotation.w),
		TargetHumanoidBasis::Native => rotation,
	}
}

fn convert_unmotion_humanoid_translation_to_target(translation: Vec3, target_basis: TargetHumanoidBasis) -> Vec3 {
	match target_basis {
		TargetHumanoidBasis::Vrm0 => Vec3::new(translation.x, translation.y, -translation.z),
		TargetHumanoidBasis::Vrm1 | TargetHumanoidBasis::UnavatarUnity => Vec3::new(-translation.x, translation.y, translation.z),
		TargetHumanoidBasis::Native => translation,
	}
}

fn transform_humanoid_sample_rotation(
	t: &TransformSample,
	coordinate_space: CoordinateSpace,
	target_basis: TargetHumanoidBasis,
	role: UnmotionHumanoidRole,
) -> Quat {
	if coordinate_space == CoordinateSpace::UNMotion {
		let mut rotation = t
			.rotation
			.as_ref()
			.map(|quat| Quat::from_xyzw(quat.x, quat.y, quat.z, quat.w))
			.unwrap_or(Quat::IDENTITY);
		rotation = convert_unmotion_humanoid_rotation_to_target(rotation, target_basis, role);
		if rotation.length_squared() > 1e-20 {
			return rotation.normalize();
		}
		return Quat::IDENTITY;
	}
	transform_sample_rotation(t, coordinate_space, target_basis)
}

fn transform_humanoid_sample_translation(
	t: &TransformSample,
	coordinate_space: CoordinateSpace,
	target_basis: TargetHumanoidBasis,
) -> Vec3 {
	if coordinate_space == CoordinateSpace::UNMotion {
		let translation = t
			.translation
			.as_ref()
			.map(|value| Vec3::new(value.x, value.y, value.z))
			.unwrap_or(Vec3::ZERO);
		return convert_unmotion_humanoid_translation_to_target(translation, target_basis);
	}
	transform_sample_translation(t, coordinate_space, target_basis)
}

#[derive(Clone, Copy, Debug)]
struct RetargetFrameContext<'a> {
	coordinate_space: CoordinateSpace,
	target_basis: TargetHumanoidBasis,
	unavatar_adapter: Option<&'a UnavatarRetargetAdapter>,
	runtime: Option<&'a RuntimeRetargetData>,
}

impl<'a> RetargetFrameContext<'a> {
	fn new(
		coordinate_space: CoordinateSpace,
		target_basis: TargetHumanoidBasis,
		unavatar_adapter: Option<&'a UnavatarRetargetAdapter>,
		runtime: Option<&'a RuntimeRetargetData>,
	) -> Self {
		Self {
			coordinate_space,
			target_basis,
			unavatar_adapter,
			runtime,
		}
	}

	fn needs_unavatar_unmotion_adapter(self) -> bool {
		self.unavatar_adapter.is_some()
	}

	fn transform_rotation(self, sample: &TransformSample, role: UnmotionHumanoidRole) -> Quat {
		transform_humanoid_sample_rotation(sample, self.coordinate_space, self.target_basis, role)
	}

	fn transform_translation(self, sample: &TransformSample) -> Vec3 {
		transform_humanoid_sample_translation(sample, self.coordinate_space, self.target_basis)
	}

	fn profile_lookup(self) -> Option<&'a ProfileNodeLookup> {
		self.runtime.map(|runtime| &runtime.profile_lookup)
	}

	fn body_bone_binding(self, bone: HumanoidBone) -> Option<NodeTransformBinding> {
		self.runtime
			.and_then(|runtime| runtime.body_bone_nodes.get(humanoid_bone_index(bone)).copied().flatten())
	}

	fn body_bone_node_index(self, profile: &HumanoidProfile, bone: HumanoidBone) -> Option<usize> {
		if self.has_runtime() {
			return self.body_bone_binding(bone).map(|binding| binding.node_index);
		}
		let key = humanoid_bone_profile_key(bone);
		profile_node_index_with_lookup(profile, self.profile_lookup(), key)
	}

	fn hand_bindings(self, side_prefix: &str) -> Option<&'a HandNodeBindings> {
		let side_index = side_index_from_prefix(side_prefix)?;
		self.runtime.and_then(|runtime| runtime.hand_nodes.get(side_index))
	}

	fn has_runtime(self) -> bool {
		self.runtime.is_some()
	}
}

fn unavatar_unmotion_limb_source_axis_in_target(role: UnmotionHumanoidRole) -> Option<Vec3> {
	match role {
		UnmotionHumanoidRole::BodyBone(
			HumanoidBone::LeftShoulder | HumanoidBone::LeftUpperArm | HumanoidBone::LeftLowerArm | HumanoidBone::LeftHand,
		) => Some(Vec3::X),
		UnmotionHumanoidRole::BodyBone(
			HumanoidBone::RightShoulder | HumanoidBone::RightUpperArm | HumanoidBone::RightLowerArm | HumanoidBone::RightHand,
		) => Some(-Vec3::X),
		UnmotionHumanoidRole::BodyBone(
			HumanoidBone::LeftUpperLeg | HumanoidBone::LeftLowerLeg | HumanoidBone::RightUpperLeg | HumanoidBone::RightLowerLeg,
		) => Some(-Vec3::Y),
		UnmotionHumanoidRole::BodyBone(HumanoidBone::LeftFoot | HumanoidBone::RightFoot) => Some(Vec3::Z),
		_ => None,
	}
}

fn humanoid_successor_profile_key(bone: HumanoidBone) -> Option<&'static str> {
	match bone {
		HumanoidBone::LeftShoulder => Some("leftupperarm"),
		HumanoidBone::LeftUpperArm => Some("leftlowerarm"),
		HumanoidBone::LeftLowerArm => Some("lefthand"),
		HumanoidBone::LeftHand => Some("leftmiddleproximal"),
		HumanoidBone::LeftUpperLeg => Some("leftlowerleg"),
		HumanoidBone::LeftLowerLeg => Some("leftfoot"),
		HumanoidBone::LeftFoot => Some("lefttoes"),
		HumanoidBone::RightShoulder => Some("rightupperarm"),
		HumanoidBone::RightUpperArm => Some("rightlowerarm"),
		HumanoidBone::RightLowerArm => Some("righthand"),
		HumanoidBone::RightHand => Some("rightmiddleproximal"),
		HumanoidBone::RightUpperLeg => Some("rightlowerleg"),
		HumanoidBone::RightLowerLeg => Some("rightfoot"),
		HumanoidBone::RightFoot => Some("righttoes"),
		_ => None,
	}
}

fn rest_child_axis_from_direct_child(nodes: &[UnaSceneNode], node_index: usize, child_index: usize) -> Option<(Quat, Vec3)> {
	let node = nodes.get(node_index)?;
	let (_, rest_rotation, _) = node_scale_rotation_translation(node);
	if !node.children.contains(&child_index) {
		return None;
	}
	let child = nodes.get(child_index)?;
	let (_, _, translation) = node_scale_rotation_translation(child);
	(translation.length_squared() > 1e-8).then(|| (rest_rotation, (rest_rotation * translation).normalize()))
}

fn rest_child_axis_from_direct_child_cached(
	nodes: &[UnaSceneNode],
	cache: Option<&RetargetRestCache>,
	node_index: usize,
	child_index: usize,
) -> Option<(Quat, Vec3)> {
	cache
		.and_then(|cache| cache.direct_child_axis(node_index, child_index))
		.or_else(|| rest_child_axis_from_direct_child(nodes, node_index, child_index))
}

fn rest_first_child_axis_in_parent_cached(
	nodes: &[UnaSceneNode],
	cache: Option<&RetargetRestCache>,
	node_index: usize,
) -> Option<(Quat, Vec3)> {
	let node = nodes.get(node_index)?;
	node.children
		.iter()
		.find_map(|&child| rest_child_axis_from_direct_child_cached(nodes, cache, node_index, child))
}

fn rest_named_child_axis_in_parent(
	nodes: &[UnaSceneNode],
	cache: Option<&RetargetRestCache>,
	node_index: usize,
	pattern: &str,
) -> Option<(Quat, Vec3)> {
	let node = nodes.get(node_index)?;
	node.children.iter().find_map(|&child| {
		let name = nodes.get(child).and_then(|node| node.name.as_deref()).unwrap_or("");
		normalize_profile_match_key(name)
			.contains(pattern)
			.then(|| rest_child_axis_from_direct_child_cached(nodes, cache, node_index, child))
			.flatten()
	})
}

fn rest_humanoid_child_axis_in_parent(
	profile: &HumanoidProfile,
	profile_lookup: Option<&ProfileNodeLookup>,
	nodes: &[UnaSceneNode],
	cache: Option<&RetargetRestCache>,
	node_index: usize,
	role: UnmotionHumanoidRole,
) -> Option<(Quat, Vec3)> {
	if let UnmotionHumanoidRole::BodyBone(bone) = role {
		if let Some(key) = humanoid_successor_profile_key(bone) {
			if let Some(child_index) = profile_node_index_with_lookup(profile, profile_lookup, key) {
				if let Some(axis) = rest_child_axis_from_direct_child_cached(nodes, cache, node_index, child_index) {
					return Some(axis);
				}
			}
		}
		if matches!(bone, HumanoidBone::LeftFoot | HumanoidBone::RightFoot) {
			if let Some(axis) = rest_named_child_axis_in_parent(nodes, cache, node_index, "toe") {
				return Some(axis);
			}
		}
	}
	rest_first_child_axis_in_parent_cached(nodes, cache, node_index)
}

#[derive(Debug)]
struct RetargetRestCache {
	local_rotations: Vec<Quat>,
	local_translations: Vec<Vec3>,
	parents: Vec<usize>,
	world_rotations: Vec<Quat>,
}

#[derive(Clone, Copy, Debug)]
struct NodeRestTransform {
	scale: Vec3,
	rotation: Quat,
	translation: Vec3,
}

impl NodeRestTransform {
	fn from_node(node: &UnaSceneNode) -> Self {
		let (scale, rotation, translation) = node_scale_rotation_translation(node);
		Self {
			scale,
			rotation,
			translation,
		}
	}
}

#[derive(Clone, Copy, Debug)]
struct NodeTransformBinding {
	node_index: usize,
	base: NodeRestTransform,
}

impl NodeTransformBinding {
	fn from_profile_key(
		profile: &HumanoidProfile,
		profile_lookup: &ProfileNodeLookup,
		scene: &UnaSceneSnapshot,
		rest_nodes: Option<&[UnaSceneNode]>,
		key: &str,
	) -> Option<Self> {
		let node_index = profile_node_index_with_lookup(profile, Some(profile_lookup), key)?;
		let base = node_base_transform_from_scene(scene, rest_nodes, node_index)?;
		Some(Self { node_index, base })
	}
}

#[derive(Clone, Copy, Debug)]
struct FingerNodeBinding {
	node: NodeTransformBinding,
	successor_node_index: Option<usize>,
}

#[derive(Clone, Copy, Debug, Default)]
struct HandNodeBindings {
	wrist: Option<NodeTransformBinding>,
	fingers: [[Option<FingerNodeBinding>; 3]; 5],
}

#[derive(Debug, Default)]
struct ExpressionNameLookup {
	preset_names: Vec<String>,
	exact_ascii_casefold: ExpressionLookupEntries,
	normalized: ExpressionLookupEntries,
}

impl ExpressionNameLookup {
	fn is_empty(&self) -> bool {
		self.preset_names.is_empty()
	}

	fn preset_name_for(&self, name: &str) -> Option<&str> {
		let index = lookup_entry_index(&self.exact_ascii_casefold, &name.to_ascii_lowercase()).or_else(|| {
			let target = normalize_expression_match_key(name);
			lookup_entry_index(&self.normalized, &target)
		})?;
		self.preset_names.get(index).map(String::as_str)
	}
}

#[derive(Debug, Default)]
struct RuntimeRetargetData {
	profile_lookup: ProfileNodeLookup,
	root_base: Option<NodeTransformBinding>,
	body_bone_nodes: [Option<NodeTransformBinding>; HUMANOID_PROFILE_BONE_COUNT],
	hand_nodes: [HandNodeBindings; 2],
	expression_lookup: ExpressionNameLookup,
}

#[derive(Clone, Copy)]
struct RetargetCompileInput<'a> {
	profile: Option<&'a HumanoidProfile>,
	scene: Option<&'a UnaSceneSnapshot>,
	rest_nodes: Option<&'a [UnaSceneNode]>,
	profile_lookup: &'a ProfileNodeLookup,
}

impl RetargetRestCache {
	fn new(nodes: &[UnaSceneNode], roots: &[usize]) -> Self {
		let (local_rotations, local_translations): (Vec<_>, Vec<_>) = nodes
			.iter()
			.map(|node| {
				let (_, rotation, translation) = node_scale_rotation_translation(node);
				(rotation, translation)
			})
			.unzip();
		let parents = compact_scene_parent_indices(nodes);
		let world_rotations = scene_world_matrices(nodes, roots)
			.into_iter()
			.map(|matrix| matrix.to_scale_rotation_translation().1)
			.collect();
		Self {
			local_rotations,
			local_translations,
			parents,
			world_rotations,
		}
	}

	fn parent_world_rotation(&self, node_index: usize) -> Quat {
		self.parents
			.get(node_index)
			.and_then(|&parent| (parent != NO_PARENT).then_some(parent))
			.and_then(|parent| self.world_rotations.get(parent).copied())
			.unwrap_or(Quat::IDENTITY)
	}

	fn direct_child_axis(&self, node_index: usize, child_index: usize) -> Option<(Quat, Vec3)> {
		if self.parents.get(child_index).copied() != Some(node_index) {
			return None;
		}
		let rest_rotation = *self.local_rotations.get(node_index)?;
		let translation = *self.local_translations.get(child_index)?;
		(translation.length_squared() > 1e-8).then(|| (rest_rotation, (rest_rotation * translation).normalize()))
	}
}

#[derive(Debug)]
struct UnavatarRetargetAdapter {
	rest_cache: RetargetRestCache,
	rest_axes: Vec<(usize, (Quat, Vec3))>,
}

impl UnavatarRetargetAdapter {
	fn new(profile: Option<&HumanoidProfile>, nodes: &[UnaSceneNode], roots: &[usize], profile_lookup: &ProfileNodeLookup) -> Self {
		let rest_cache = RetargetRestCache::new(nodes, roots);
		let rest_axes = precompute_unavatar_rest_axes(profile, nodes, &rest_cache, profile_lookup);
		Self { rest_cache, rest_axes }
	}

	fn rest_axis(&self, node_index: usize) -> Option<(Quat, Vec3)> {
		self.rest_axes
			.binary_search_by_key(&node_index, |(node_index, _)| *node_index)
			.ok()
			.map(|index| self.rest_axes[index].1)
	}
}

#[derive(Debug)]
pub struct HumanoidRetargetContext {
	target_basis: TargetHumanoidBasis,
	runtime: RuntimeRetargetData,
	unavatar_adapter: Option<UnavatarRetargetAdapter>,
}

impl HumanoidRetargetContext {
	pub fn for_document(document: &UnaDocument, rest_nodes: Option<&[UnaSceneNode]>) -> Self {
		Self::for_runtime_inputs(document.runtime_model().humanoid_retarget_inputs(), rest_nodes)
	}

	pub fn for_runtime_inputs(inputs: UnaRuntimeRetargetInputs<'_>, rest_nodes: Option<&[UnaSceneNode]>) -> Self {
		let target_basis = inputs.humanoid_basis;
		let profile = inputs.profile;
		let scene = inputs.scene;
		let profile_lookup = profile.map(precompute_profile_lookup).unwrap_or_default();
		let compile_input = RetargetCompileInput {
			profile,
			scene,
			rest_nodes,
			profile_lookup: &profile_lookup,
		};
		let root_base = precompute_root_base(compile_input);
		let body_bone_nodes = precompute_body_bone_nodes(compile_input);
		let hand_nodes = precompute_hand_nodes(compile_input);
		let expression_lookup = precompute_expression_lookup(inputs.expression_catalog);
		let runtime = RuntimeRetargetData {
			profile_lookup,
			root_base,
			body_bone_nodes,
			hand_nodes,
			expression_lookup,
		};
		let unavatar_adapter = if target_basis == TargetHumanoidBasis::UnavatarUnity {
			scene.map(|scene| {
				let rest = rest_nodes.unwrap_or(&scene.nodes);
				UnavatarRetargetAdapter::new(profile, rest, &scene.roots, &runtime.profile_lookup)
			})
		} else {
			None
		};
		Self {
			target_basis,
			runtime,
			unavatar_adapter,
		}
	}

	fn frame_context(&self, coordinate_space: CoordinateSpace) -> RetargetFrameContext<'_> {
		let unavatar_adapter = if coordinate_space == CoordinateSpace::UNMotion && self.target_basis == TargetHumanoidBasis::UnavatarUnity {
			self.unavatar_adapter.as_ref()
		} else {
			None
		};
		RetargetFrameContext::new(coordinate_space, self.target_basis, unavatar_adapter, Some(&self.runtime))
	}
}

fn precompute_profile_lookup(profile: &HumanoidProfile) -> ProfileNodeLookup {
	let mut lookup: ProfileNodeLookup = profile
		.bone_node_indices
		.iter()
		.map(|(key, &index)| (normalize_profile_match_key(key), index))
		.collect();
	sort_dedup_string_index_lookup(&mut lookup);
	lookup
}

fn profile_lookup_node_index(lookup: &ProfileNodeLookup, key: &str) -> Option<usize> {
	string_index_lookup_value(lookup, key)
}

fn lookup_entry_index(entries: &ExpressionLookupEntries, key: &str) -> Option<usize> {
	string_index_lookup_value(entries, key)
}

fn string_index_lookup_value(entries: &StringIndexLookup, key: &str) -> Option<usize> {
	entries
		.binary_search_by(|(candidate, _)| candidate.as_str().cmp(key))
		.ok()
		.map(|index| entries[index].1)
}

fn sort_dedup_string_index_lookup(entries: &mut StringIndexLookup) {
	entries.sort_by(|(a, _), (b, _)| a.cmp(b));
	entries.dedup_by(|(a, _), (b, _)| a == b);
}

fn precompute_root_base(input: RetargetCompileInput<'_>) -> Option<NodeTransformBinding> {
	let Some(scene) = input.scene else {
		return None;
	};
	let roots = scene.resolved_roots();
	let node_index = *roots.first()?;
	node_base_transform_from_scene(scene, input.rest_nodes, node_index).map(|base| NodeTransformBinding { node_index, base })
}

fn node_base_transform_from_scene(
	scene: &UnaSceneSnapshot,
	rest_nodes: Option<&[UnaSceneNode]>,
	node_index: usize,
) -> Option<NodeRestTransform> {
	let nodes = rest_nodes.unwrap_or(&scene.nodes);
	nodes.get(node_index).map(NodeRestTransform::from_node)
}

fn precompute_body_bone_nodes(input: RetargetCompileInput<'_>) -> [Option<NodeTransformBinding>; HUMANOID_PROFILE_BONE_COUNT] {
	debug_assert_eq!(HUMANOID_PROFILE_BONES.len(), HUMANOID_PROFILE_BONE_COUNT);
	let mut nodes = [None; HUMANOID_PROFILE_BONE_COUNT];
	let (Some(profile), Some(scene)) = (input.profile, input.scene) else {
		return nodes;
	};
	for &bone in HUMANOID_PROFILE_BONES {
		let key = humanoid_bone_profile_key(bone);
		nodes[humanoid_bone_index(bone)] =
			NodeTransformBinding::from_profile_key(profile, input.profile_lookup, scene, input.rest_nodes, key);
	}
	nodes
}

fn precompute_hand_nodes(input: RetargetCompileInput<'_>) -> [HandNodeBindings; 2] {
	let mut hands = [HandNodeBindings::default(); 2];
	let (Some(profile), Some(scene)) = (input.profile, input.scene) else {
		return hands;
	};
	for (side_index, side_prefix) in ["left", "right"].into_iter().enumerate() {
		if let Some(key) = hand_profile_key(side_prefix) {
			hands[side_index].wrist = NodeTransformBinding::from_profile_key(profile, input.profile_lookup, scene, input.rest_nodes, key);
		}
	}
	for &(side_prefix, finger_key, segment) in FINGER_PROFILE_SEGMENTS {
		let Some(key) = finger_profile_key(side_prefix, finger_key, segment) else {
			continue;
		};
		let Some(node) = NodeTransformBinding::from_profile_key(profile, input.profile_lookup, scene, input.rest_nodes, key) else {
			continue;
		};
		let Some(side_index) = side_index_from_prefix(side_prefix) else {
			continue;
		};
		let Some(finger_index) = finger_index_from_key(finger_key) else {
			continue;
		};
		let Some(segment_index) = segment_index_from_key(segment) else {
			continue;
		};
		let successor_node_index = finger_successor_profile_key(side_prefix, finger_key, segment)
			.and_then(|successor_key| profile_node_index_with_lookup(profile, Some(input.profile_lookup), successor_key));
		hands[side_index].fingers[finger_index][segment_index] = Some(FingerNodeBinding {
			node,
			successor_node_index,
		});
	}
	hands
}

fn precompute_expression_lookup(catalog: Option<&un_avatar_core::UnaExpressionCatalog>) -> ExpressionNameLookup {
	let Some(catalog) = catalog else {
		return ExpressionNameLookup::default();
	};
	let mut lookup = ExpressionNameLookup::default();
	for preset in &catalog.presets {
		let index = lookup.preset_names.len();
		lookup.preset_names.push(preset.name.clone());
		lookup.exact_ascii_casefold.push((preset.name.to_ascii_lowercase(), index));
		lookup.normalized.push((normalize_expression_match_key(&preset.name), index));
	}
	sort_dedup_string_index_lookup(&mut lookup.exact_ascii_casefold);
	sort_dedup_string_index_lookup(&mut lookup.normalized);
	lookup
}

fn precompute_unavatar_rest_axes(
	profile: Option<&HumanoidProfile>,
	nodes: &[UnaSceneNode],
	cache: &RetargetRestCache,
	profile_lookup: &ProfileNodeLookup,
) -> Vec<(usize, (Quat, Vec3))> {
	let Some(profile) = profile else {
		return Vec::new();
	};
	let mut axes = BTreeMap::new();
	for bone in [
		HumanoidBone::LeftShoulder,
		HumanoidBone::LeftUpperArm,
		HumanoidBone::LeftLowerArm,
		HumanoidBone::LeftHand,
		HumanoidBone::RightShoulder,
		HumanoidBone::RightUpperArm,
		HumanoidBone::RightLowerArm,
		HumanoidBone::RightHand,
		HumanoidBone::LeftUpperLeg,
		HumanoidBone::LeftLowerLeg,
		HumanoidBone::LeftFoot,
		HumanoidBone::RightUpperLeg,
		HumanoidBone::RightLowerLeg,
		HumanoidBone::RightFoot,
	] {
		let key = humanoid_bone_profile_key(bone);
		let Some(node_index) = profile_node_index_with_lookup(profile, Some(profile_lookup), key) else {
			continue;
		};
		if let Some(axis) = rest_humanoid_child_axis_in_parent(
			profile,
			Some(profile_lookup),
			nodes,
			Some(cache),
			node_index,
			UnmotionHumanoidRole::BodyBone(bone),
		) {
			axes.insert(node_index, axis);
		}
	}
	for (side_prefix, finger_key, segment) in FINGER_PROFILE_SEGMENTS {
		let Some(key) = finger_profile_key(side_prefix, finger_key, segment) else {
			continue;
		};
		let Some(node_index) = profile_node_index_with_lookup(profile, Some(profile_lookup), key) else {
			continue;
		};
		let axis = finger_successor_profile_key(side_prefix, finger_key, segment)
			.and_then(|successor_key| profile_node_index_with_lookup(profile, Some(profile_lookup), successor_key))
			.and_then(|child_index| rest_child_axis_from_direct_child_cached(nodes, Some(cache), node_index, child_index))
			.or_else(|| rest_first_child_axis_in_parent_cached(nodes, Some(cache), node_index));
		if let Some(axis) = axis {
			axes.insert(node_index, axis);
		}
	}
	axes.into_iter().collect()
}

fn adapt_unavatar_unmotion_limb_axis(
	profile: &HumanoidProfile,
	rotation: Quat,
	nodes: &[UnaSceneNode],
	rest_nodes: Option<&[UnaSceneNode]>,
	frame_ctx: RetargetFrameContext<'_>,
	node_index: usize,
	role: UnmotionHumanoidRole,
) -> Quat {
	if !frame_ctx.needs_unavatar_unmotion_adapter() {
		return rotation;
	}
	let Some(source_axis) = unavatar_unmotion_limb_source_axis_in_target(role) else {
		return rotation;
	};
	let rest = rest_nodes.unwrap_or(nodes);
	let Some(adapter) = frame_ctx.unavatar_adapter else {
		return rotation;
	};
	let cache = &adapter.rest_cache;
	let axis = adapter
		.rest_axis(node_index)
		.or_else(|| rest_humanoid_child_axis_in_parent(profile, frame_ctx.profile_lookup(), rest, Some(cache), node_index, role));
	let Some((rest_rotation, target_axis)) = axis else {
		return rotation;
	};
	let parent_world_rotation = cache.parent_world_rotation(node_index);
	let source_axis_in_parent = (parent_world_rotation.inverse() * source_axis).normalize_or_zero();
	if source_axis_in_parent.length_squared() < 1e-10 {
		return rotation;
	}
	let rotation_in_parent = parent_world_rotation.inverse() * rotation * parent_world_rotation;
	let adapter = Quat::from_rotation_arc(target_axis, source_axis_in_parent);
	let parent_space_delta = (rotation_in_parent * adapter).normalize();
	(rest_rotation.inverse() * parent_space_delta * rest_rotation).normalize()
}

fn unavatar_unmotion_finger_source_axis_in_target(side_prefix: &str, finger_key: &str, segment: &str) -> Vec3 {
	if finger_key == "thumb" && segment == "proximal" {
		const THUMB_REST_OPEN_RAD: f32 = 0.31;
		let side = if side_prefix == "left" { 1.0 } else { -1.0 };
		Vec3::new(side * THUMB_REST_OPEN_RAD.cos(), 0.0, THUMB_REST_OPEN_RAD.sin())
	} else if finger_key == "thumb" {
		const THUMB_FLEXION_REST_OPEN_RAD: f32 = 0.33;
		let side = if side_prefix == "left" { 1.0 } else { -1.0 };
		Vec3::new(side * THUMB_FLEXION_REST_OPEN_RAD.cos(), 0.0, -THUMB_FLEXION_REST_OPEN_RAD.sin())
	} else if side_prefix == "left" {
		Vec3::X
	} else {
		-Vec3::X
	}
}

const FINGER_PROFILE_SEGMENTS: &[(&str, &str, &str)] = &[
	("left", "thumb", "proximal"),
	("left", "thumb", "intermediate"),
	("left", "thumb", "distal"),
	("left", "index", "proximal"),
	("left", "index", "intermediate"),
	("left", "index", "distal"),
	("left", "middle", "proximal"),
	("left", "middle", "intermediate"),
	("left", "middle", "distal"),
	("left", "ring", "proximal"),
	("left", "ring", "intermediate"),
	("left", "ring", "distal"),
	("left", "little", "proximal"),
	("left", "little", "intermediate"),
	("left", "little", "distal"),
	("right", "thumb", "proximal"),
	("right", "thumb", "intermediate"),
	("right", "thumb", "distal"),
	("right", "index", "proximal"),
	("right", "index", "intermediate"),
	("right", "index", "distal"),
	("right", "middle", "proximal"),
	("right", "middle", "intermediate"),
	("right", "middle", "distal"),
	("right", "ring", "proximal"),
	("right", "ring", "intermediate"),
	("right", "ring", "distal"),
	("right", "little", "proximal"),
	("right", "little", "intermediate"),
	("right", "little", "distal"),
];

fn finger_profile_key(side_prefix: &str, finger_key: &str, segment: &str) -> Option<&'static str> {
	match (side_prefix, finger_key, segment) {
		("left", "thumb", "proximal") => Some("leftthumbproximal"),
		("left", "thumb", "intermediate") => Some("leftthumbintermediate"),
		("left", "thumb", "distal") => Some("leftthumbdistal"),
		("left", "index", "proximal") => Some("leftindexproximal"),
		("left", "index", "intermediate") => Some("leftindexintermediate"),
		("left", "index", "distal") => Some("leftindexdistal"),
		("left", "middle", "proximal") => Some("leftmiddleproximal"),
		("left", "middle", "intermediate") => Some("leftmiddleintermediate"),
		("left", "middle", "distal") => Some("leftmiddledistal"),
		("left", "ring", "proximal") => Some("leftringproximal"),
		("left", "ring", "intermediate") => Some("leftringintermediate"),
		("left", "ring", "distal") => Some("leftringdistal"),
		("left", "little", "proximal") => Some("leftlittleproximal"),
		("left", "little", "intermediate") => Some("leftlittleintermediate"),
		("left", "little", "distal") => Some("leftlittledistal"),
		("right", "thumb", "proximal") => Some("rightthumbproximal"),
		("right", "thumb", "intermediate") => Some("rightthumbintermediate"),
		("right", "thumb", "distal") => Some("rightthumbdistal"),
		("right", "index", "proximal") => Some("rightindexproximal"),
		("right", "index", "intermediate") => Some("rightindexintermediate"),
		("right", "index", "distal") => Some("rightindexdistal"),
		("right", "middle", "proximal") => Some("rightmiddleproximal"),
		("right", "middle", "intermediate") => Some("rightmiddleintermediate"),
		("right", "middle", "distal") => Some("rightmiddledistal"),
		("right", "ring", "proximal") => Some("rightringproximal"),
		("right", "ring", "intermediate") => Some("rightringintermediate"),
		("right", "ring", "distal") => Some("rightringdistal"),
		("right", "little", "proximal") => Some("rightlittleproximal"),
		("right", "little", "intermediate") => Some("rightlittleintermediate"),
		("right", "little", "distal") => Some("rightlittledistal"),
		_ => None,
	}
}

fn finger_successor_profile_key(side_prefix: &str, finger_key: &str, segment: &str) -> Option<&'static str> {
	let next_segment = match segment {
		"proximal" => "intermediate",
		"intermediate" => "distal",
		_ => return None,
	};
	finger_profile_key(side_prefix, finger_key, next_segment)
}

fn hand_profile_key(side_prefix: &str) -> Option<&'static str> {
	match side_prefix {
		"left" => Some("lefthand"),
		"right" => Some("righthand"),
		_ => None,
	}
}

fn side_index_from_prefix(side_prefix: &str) -> Option<usize> {
	match side_prefix {
		"left" => Some(0),
		"right" => Some(1),
		_ => None,
	}
}

fn finger_index_from_key(finger_key: &str) -> Option<usize> {
	match finger_key {
		"thumb" => Some(0),
		"index" => Some(1),
		"middle" => Some(2),
		"ring" => Some(3),
		"little" => Some(4),
		_ => None,
	}
}

fn finger_index_and_key(finger: Finger) -> (usize, &'static str) {
	match finger {
		Finger::Thumb => (0, "thumb"),
		Finger::Index => (1, "index"),
		Finger::Middle => (2, "middle"),
		Finger::Ring => (3, "ring"),
		Finger::Little => (4, "little"),
	}
}

fn segment_index_from_key(segment: &str) -> Option<usize> {
	match segment {
		"proximal" => Some(0),
		"intermediate" => Some(1),
		"distal" => Some(2),
		_ => None,
	}
}

fn segment_key_from_index(index: usize) -> Option<&'static str> {
	match index {
		0 => Some("proximal"),
		1 => Some("intermediate"),
		2 => Some("distal"),
		_ => None,
	}
}

fn hand_wrist_adapter_role(side_prefix: &str) -> UnmotionHumanoidRole {
	if side_prefix == "left" {
		UnmotionHumanoidRole::BodyBone(HumanoidBone::LeftHand)
	} else {
		UnmotionHumanoidRole::BodyBone(HumanoidBone::RightHand)
	}
}

#[allow(clippy::too_many_arguments)]
fn adapt_unavatar_unmotion_finger_axis(
	mut rotation: Quat,
	nodes: &[UnaSceneNode],
	rest_nodes: Option<&[UnaSceneNode]>,
	frame_ctx: RetargetFrameContext<'_>,
	node_index: usize,
	side_prefix: &str,
	finger_key: &str,
	segment: &str,
	successor_node_index: Option<usize>,
) -> Quat {
	if !frame_ctx.needs_unavatar_unmotion_adapter() {
		return rotation;
	}
	if rotation.angle_between(Quat::IDENTITY) < 1e-5 {
		return Quat::IDENTITY;
	}
	if finger_key == "thumb" && segment == "proximal" {
		rotation = Quat::from_xyzw(rotation.x, rotation.y, -rotation.z, rotation.w).normalize();
	} else if finger_key == "thumb" {
		rotation = rotation.normalize();
	} else {
		rotation = Quat::from_xyzw(rotation.x, rotation.y, -rotation.z, rotation.w).normalize();
	}
	if rotation.angle_between(Quat::IDENTITY) < 1e-5 {
		return Quat::IDENTITY;
	}
	let rest = rest_nodes.unwrap_or(nodes);
	let Some(adapter) = frame_ctx.unavatar_adapter else {
		return rotation;
	};
	let cache = &adapter.rest_cache;
	let axis = adapter.rest_axis(node_index).or_else(|| {
		successor_node_index
			.and_then(|child_index| rest_child_axis_from_direct_child_cached(rest, Some(cache), node_index, child_index))
			.or_else(|| rest_first_child_axis_in_parent_cached(rest, Some(cache), node_index))
	});
	let Some((rest_rotation, target_axis)) = axis else {
		return rotation;
	};
	let parent_world_rotation = cache.parent_world_rotation(node_index);
	let source_axis = unavatar_unmotion_finger_source_axis_in_target(side_prefix, finger_key, segment);
	let source_axis_in_parent = (parent_world_rotation.inverse() * source_axis).normalize_or_zero();
	if source_axis_in_parent.length_squared() < 1e-10 {
		return rotation;
	}
	let rotation_in_parent = parent_world_rotation.inverse() * rotation * parent_world_rotation;
	let parent_space_delta = if finger_key == "thumb" {
		rotation_in_parent.normalize()
	} else {
		let desired_axis = (rotation_in_parent * source_axis_in_parent).normalize_or_zero();
		if desired_axis.length_squared() < 1e-10 {
			return rotation;
		}
		Quat::from_rotation_arc(target_axis, desired_axis).normalize()
	};
	(rest_rotation.inverse() * parent_space_delta * rest_rotation).normalize()
}

fn apply_humanoid_transform_to_profile_node(
	profile: &HumanoidProfile,
	nodes: &mut [UnaSceneNode],
	rest_nodes: Option<&[UnaSceneNode]>,
	frame_ctx: RetargetFrameContext<'_>,
	key: &str,
	transform: &TransformSample,
	role: UnmotionHumanoidRole,
	adapter_role: UnmotionHumanoidRole,
) {
	let Some(ni) = profile_node_index_with_lookup(profile, frame_ctx.profile_lookup(), key) else {
		return;
	};
	apply_humanoid_transform_to_node_index(
		profile,
		nodes,
		rest_nodes,
		frame_ctx,
		ni,
		transform,
		role,
		adapter_role,
		None,
		true,
		None,
	);
}

#[allow(clippy::too_many_arguments)]
fn apply_humanoid_transform_to_node_index(
	profile: &HumanoidProfile,
	nodes: &mut [UnaSceneNode],
	rest_nodes: Option<&[UnaSceneNode]>,
	frame_ctx: RetargetFrameContext<'_>,
	ni: usize,
	transform: &TransformSample,
	role: UnmotionHumanoidRole,
	adapter_role: UnmotionHumanoidRole,
	compiled_base: Option<NodeRestTransform>,
	apply_translation: bool,
	eye_clamp_deg: Option<f32>,
) {
	let mut sample_rotation = frame_ctx.transform_rotation(transform, role);
	sample_rotation = adapt_unavatar_unmotion_limb_axis(profile, sample_rotation, nodes, rest_nodes, frame_ctx, ni, adapter_role);
	if let Some(node) = nodes.get_mut(ni) {
		if let Some(deg) = eye_clamp_deg {
			sample_rotation = clamp_eye_rotation(sample_rotation, deg);
		}
		write_retargeted_local_transform(
			ni,
			node,
			rest_nodes,
			frame_ctx,
			compiled_base,
			sample_rotation,
			transform,
			apply_translation,
		);
	}
}

#[allow(clippy::too_many_arguments)]
fn apply_finger_transform_to_profile_node(
	nodes: &mut [UnaSceneNode],
	rest_nodes: Option<&[UnaSceneNode]>,
	frame_ctx: RetargetFrameContext<'_>,
	node_index: usize,
	successor_node_index: Option<usize>,
	compiled_base: Option<NodeRestTransform>,
	side_prefix: &str,
	finger_key: &str,
	segment: &str,
	transform: &TransformSample,
) {
	let mut sample_rotation = frame_ctx.transform_rotation(transform, UnmotionHumanoidRole::HandFinger);
	sample_rotation = adapt_unavatar_unmotion_finger_axis(
		sample_rotation,
		nodes,
		rest_nodes,
		frame_ctx,
		node_index,
		side_prefix,
		finger_key,
		segment,
		successor_node_index,
	);
	if let Some(node) = nodes.get_mut(node_index) {
		write_retargeted_local_transform(
			node_index,
			node,
			rest_nodes,
			frame_ctx,
			compiled_base,
			sample_rotation,
			transform,
			true,
		);
	}
}

fn write_retargeted_local_transform(
	node_index: usize,
	node: &mut UnaSceneNode,
	rest_nodes: Option<&[UnaSceneNode]>,
	frame_ctx: RetargetFrameContext<'_>,
	compiled_base: Option<NodeRestTransform>,
	sample_rotation: Quat,
	transform: &TransformSample,
	apply_translation: bool,
) {
	let base = compiled_base.unwrap_or_else(|| base_node_transform(node_index, node, rest_nodes));
	let sample_translation = if apply_translation {
		frame_ctx.transform_translation(transform)
	} else {
		Vec3::ZERO
	};
	node.transform =
		Mat4::from_scale_rotation_translation(base.scale, base.rotation * sample_rotation, base.translation + sample_translation)
			.to_cols_array();
}

fn profile_node_index_with_lookup(profile: &HumanoidProfile, lookup: Option<&ProfileNodeLookup>, key: &str) -> Option<usize> {
	profile.bone_node_indices.get(key).copied().or_else(|| {
		let target = normalize_profile_match_key(key);
		if let Some(lookup) = lookup {
			return profile_lookup_node_index(lookup, &target);
		}
		profile
			.bone_node_indices
			.iter()
			.find(|(candidate, _)| normalize_profile_match_key(candidate) == target)
			.map(|(_, index)| *index)
	})
}

fn normalize_profile_match_key(name: &str) -> String {
	name.chars()
		.filter(|ch| ch.is_ascii_alphanumeric())
		.map(|ch| ch.to_ascii_lowercase())
		.collect()
}

fn apply_hand_motion_to_scene(
	profile: &HumanoidProfile,
	nodes: &mut [UnaSceneNode],
	hand: &HandMotion,
	side_prefix: &'static str,
	rest_nodes: Option<&[UnaSceneNode]>,
	frame_ctx: RetargetFrameContext<'_>,
	apply_wrist: bool,
) {
	if hand.tracking_state == un_motion_frame::TrackingState::Lost {
		return;
	}
	let hand_bindings = frame_ctx.hand_bindings(side_prefix);
	if apply_wrist {
		if let Some(wrist) = hand.wrist.as_ref() {
			let adapter_role = hand_wrist_adapter_role(side_prefix);
			if let Some(binding) = hand_bindings.and_then(|hand| hand.wrist) {
				apply_humanoid_transform_to_node_index(
					profile,
					nodes,
					rest_nodes,
					frame_ctx,
					binding.node_index,
					wrist,
					UnmotionHumanoidRole::HandWrist,
					adapter_role,
					Some(binding.base),
					true,
					None,
				);
			} else if let Some(key) = hand_profile_key(side_prefix) {
				apply_humanoid_transform_to_profile_node(
					profile,
					nodes,
					rest_nodes,
					frame_ctx,
					key,
					wrist,
					UnmotionHumanoidRole::HandWrist,
					adapter_role,
				);
			}
		}
	}
	for finger in &hand.fingers {
		let (finger_index, finger_key) = finger_index_and_key(finger.finger);
		for (index, joint) in finger.joints.iter().enumerate() {
			let Some(segment) = segment_key_from_index(index) else {
				continue;
			};
			let Some(binding) = hand_bindings.and_then(|hand| hand.fingers[finger_index][index]) else {
				continue;
			};
			apply_finger_transform_to_profile_node(
				nodes,
				rest_nodes,
				frame_ctx,
				binding.node.node_index,
				binding.successor_node_index,
				Some(binding.node.base),
				side_prefix,
				finger_key,
				segment,
				joint,
			);
		}
	}
}

#[derive(Clone, Copy, Debug, Default)]
struct BodyPoseOwnership {
	left_hand: bool,
	right_hand: bool,
}

impl BodyPoseOwnership {
	fn from_pose(pose: Option<&HumanoidPose>) -> Self {
		let Some(pose) = pose else {
			return Self::default();
		};
		let mut ownership = Self::default();
		for sample in &pose.bones {
			if sample.state == SampleState::Missing {
				continue;
			}
			match sample.bone {
				HumanoidBone::LeftHand => ownership.left_hand = true,
				HumanoidBone::RightHand => ownership.right_hand = true,
				_ => {}
			}
			if ownership.left_hand && ownership.right_hand {
				break;
			}
		}
		ownership
	}
}

fn node_scale_rotation_translation(node: &UnaSceneNode) -> (Vec3, Quat, Vec3) {
	let (scale, rotation, translation) = Mat4::from_cols_array(&node.transform).to_scale_rotation_translation();
	(scale, rotation, translation)
}

fn base_node_transform(node_index: usize, node: &UnaSceneNode, rest_nodes: Option<&[UnaSceneNode]>) -> NodeRestTransform {
	let base_node = rest_nodes.and_then(|rest| rest.get(node_index)).unwrap_or(node);
	NodeRestTransform::from_node(base_node)
}

fn root_base_transform(
	node_index: usize,
	node: &UnaSceneNode,
	rest_nodes: Option<&[UnaSceneNode]>,
	frame_ctx: RetargetFrameContext<'_>,
) -> Option<NodeRestTransform> {
	frame_ctx
		.runtime
		.and_then(|runtime| runtime.root_base)
		.filter(|binding| binding.node_index == node_index)
		.map(|binding| binding.base)
		.or_else(|| rest_nodes.and_then(|rest| rest.get(node_index)).map(NodeRestTransform::from_node))
		.or_else(|| frame_ctx.has_runtime().then(|| NodeRestTransform::from_node(node)))
}

fn constraint_axis(axis: UnaNodeConstraintAxis) -> Vec3 {
	match axis {
		UnaNodeConstraintAxis::X => Vec3::X,
		UnaNodeConstraintAxis::Y => Vec3::Y,
		UnaNodeConstraintAxis::Z => Vec3::Z,
	}
}

fn constraint_aim_axis(axis: UnaNodeConstraintAimAxis) -> Vec3 {
	match axis {
		UnaNodeConstraintAimAxis::PositiveX => Vec3::X,
		UnaNodeConstraintAimAxis::NegativeX => -Vec3::X,
		UnaNodeConstraintAimAxis::PositiveY => Vec3::Y,
		UnaNodeConstraintAimAxis::NegativeY => -Vec3::Y,
		UnaNodeConstraintAimAxis::PositiveZ => Vec3::Z,
		UnaNodeConstraintAimAxis::NegativeZ => -Vec3::Z,
	}
}

fn scene_world_matrices(nodes: &[UnaSceneNode], roots: &[usize]) -> Vec<Mat4> {
	let mut world = vec![Mat4::IDENTITY; nodes.len().max(1)];
	fn visit(nodes: &[UnaSceneNode], idx: usize, parent: Mat4, world: &mut [Mat4]) {
		if idx >= nodes.len() {
			return;
		}
		let local = Mat4::from_cols_array(&nodes[idx].transform);
		let w = parent * local;
		world[idx] = w;
		for &child in &nodes[idx].children {
			visit(nodes, child, w, world);
		}
	}
	for &root in resolved_scene_roots(nodes, roots).iter() {
		visit(nodes, root, Mat4::IDENTITY, &mut world);
	}
	world
}

fn compact_scene_parent_indices(nodes: &[UnaSceneNode]) -> Vec<usize> {
	let mut parents = vec![NO_PARENT; nodes.len()];
	for (parent, node) in nodes.iter().enumerate() {
		for &child in &node.children {
			if child < parents.len() {
				parents[child] = parent;
			}
		}
	}
	parents
}

fn set_node_rotation(node: &mut UnaSceneNode, rotation: Quat) {
	let (scale, _old_rotation, translation) = node_scale_rotation_translation(node);
	node.transform = Mat4::from_scale_rotation_translation(scale, rotation.normalize(), translation).to_cols_array();
}

fn apply_rotation_constraint(nodes: &mut [UnaSceneNode], rest_nodes: &[UnaSceneNode], c: &UnaNodeConstraint) {
	let Some(src) = nodes.get(c.source_node) else { return };
	let Some(src_rest) = rest_nodes.get(c.source_node) else { return };
	let Some(dst_rest) = rest_nodes.get(c.target_node) else { return };
	let (_, src_rotation, _) = node_scale_rotation_translation(src);
	let (_, src_rest_rotation, _) = node_scale_rotation_translation(src_rest);
	let (_, dst_rest_rotation, _) = node_scale_rotation_translation(dst_rest);
	let src_delta = src_rest_rotation.inverse() * src_rotation;
	let target = dst_rest_rotation * src_delta;
	let result = dst_rest_rotation.slerp(target.normalize(), c.weight.clamp(0.0, 1.0));
	if let Some(dst) = nodes.get_mut(c.target_node) {
		set_node_rotation(dst, result);
	}
}

fn apply_roll_constraint(nodes: &mut [UnaSceneNode], rest_nodes: &[UnaSceneNode], c: &UnaNodeConstraint, axis: UnaNodeConstraintAxis) {
	let Some(src) = nodes.get(c.source_node) else { return };
	let Some(src_rest) = rest_nodes.get(c.source_node) else { return };
	let Some(dst_rest) = rest_nodes.get(c.target_node) else { return };
	let (_, src_rotation, _) = node_scale_rotation_translation(src);
	let (_, src_rest_rotation, _) = node_scale_rotation_translation(src_rest);
	let (_, dst_rest_rotation, _) = node_scale_rotation_translation(dst_rest);
	let axis = constraint_axis(axis);
	let src_delta = src_rest_rotation.inverse() * src_rotation;
	let src_delta_in_parent = src_rest_rotation * src_delta * src_rest_rotation.inverse();
	let src_delta_in_dst = dst_rest_rotation.inverse() * src_delta_in_parent * dst_rest_rotation;
	let to_vec = (src_delta_in_dst * axis).normalize_or_zero();
	if to_vec.length_squared() < 1e-10 {
		return;
	}
	let from_to = Quat::from_rotation_arc(axis, to_vec);
	let target = dst_rest_rotation * from_to.inverse() * src_delta_in_dst;
	let result = dst_rest_rotation.slerp(target.normalize(), c.weight.clamp(0.0, 1.0));
	if let Some(dst) = nodes.get_mut(c.target_node) {
		set_node_rotation(dst, result);
	}
}

fn apply_aim_constraint(
	nodes: &mut [UnaSceneNode],
	roots: &[usize],
	rest_nodes: &[UnaSceneNode],
	parents: &[usize],
	c: &UnaNodeConstraint,
	axis: UnaNodeConstraintAimAxis,
) {
	if c.source_node >= nodes.len() || c.target_node >= nodes.len() || c.target_node >= rest_nodes.len() {
		return;
	}
	let world = scene_world_matrices(nodes, roots);
	let src_pos = world[c.source_node].transform_point3(Vec3::ZERO);
	let dst_pos = world[c.target_node].transform_point3(Vec3::ZERO);
	let to_vec = (src_pos - dst_pos).normalize_or_zero();
	if to_vec.length_squared() < 1e-10 {
		return;
	}
	let parent_world_rotation = parents
		.get(c.target_node)
		.and_then(|&parent| (parent != NO_PARENT).then_some(parent))
		.and_then(|parent| world.get(parent).copied())
		.map(|m| m.to_scale_rotation_translation().1)
		.unwrap_or(Quat::IDENTITY);
	let (_, dst_rest_rotation, _) = node_scale_rotation_translation(&rest_nodes[c.target_node]);
	let axis = constraint_aim_axis(axis);
	let from_vec = (parent_world_rotation * dst_rest_rotation * axis).normalize_or_zero();
	if from_vec.length_squared() < 1e-10 {
		return;
	}
	let from_to = Quat::from_rotation_arc(from_vec, to_vec);
	let target = parent_world_rotation.inverse() * from_to * parent_world_rotation * dst_rest_rotation;
	let result = dst_rest_rotation.slerp(target.normalize(), c.weight.clamp(0.0, 1.0));
	if let Some(dst) = nodes.get_mut(c.target_node) {
		set_node_rotation(dst, result);
	}
}

/// VRM 1 `VRMC_node_constraint` を現在のノード姿勢へ適用する。
///
/// `rest_nodes` は importer 直後のノード列を渡す。制約は rest rotation からの差分として評価するため、
/// VMC などで Humanoid ボーンを書き換えた直後に呼ぶ。
pub fn apply_node_constraints_to_scene(
	nodes: &mut [UnaSceneNode],
	roots: &[usize],
	constraints: &[UnaNodeConstraint],
	rest_nodes: &[UnaSceneNode],
) {
	if constraints.is_empty() || nodes.is_empty() || rest_nodes.is_empty() {
		return;
	}
	let mut parents = None;
	for c in constraints {
		match c.kind {
			UnaNodeConstraintKind::Rotation => apply_rotation_constraint(nodes, rest_nodes, c),
			UnaNodeConstraintKind::Roll { axis } => apply_roll_constraint(nodes, rest_nodes, c, axis),
			UnaNodeConstraintKind::Aim { axis } => {
				let parents = parents.get_or_insert_with(|| compact_scene_parent_indices(nodes));
				apply_aim_constraint(nodes, roots, rest_nodes, parents, c, axis);
			}
		}
	}
}

/// [`apply_un_motion_frame_to_document`] のオプション（目周り切り分けなど）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ApplyUnMotionFrameOpts {
	/// `frame.face` の式を `expression_weights` に反映するか。
	pub apply_expressions: bool,
	/// Humanoid の LeftEye / RightEye をノードへ書き込むか（VMC の視線相当）。
	pub apply_eye_bones: bool,
	/// 視線（LeftEye / RightEye）の yaw/pitch クランプ角度（度）。
	/// `Some(deg)` のとき、VRM 1.0 LookAt curve の単純化として ±deg にクランプしてから書き込む。
	/// `None` のときクランプしない（従来動作）。VRM 1.0 標準のデフォルトは 30 度。
	pub eye_look_at_clamp_deg: Option<f32>,
	/// `pose.root` の **translation** を scene の最初の root へ加算するか。既定 `false`。
	///
	/// VMC `/VMC/Ext/Root/Pos` は studio 空間でのアバター root の world transform を送る仕様だが、
	/// 顔だけ追従する Waidayo 系などが calibration の都合で非ゼロな translation を送ってくることがあり、
	/// `roots.first()` が実 armature root (model1.vrm の `Root` 等) のモデルではアバター全体が
	/// 前後にズレて表示される。Sender に依存しないデフォルトとして OFF にし、
	/// 体ごと位置を動かしたいユースケースで明示的に ON にする。**rotation は常に適用する** (頭向きや
	/// 体全体の向きには元来必要)。
	pub apply_root_translation: bool,
}

impl Default for ApplyUnMotionFrameOpts {
	fn default() -> Self {
		Self {
			apply_expressions: true,
			apply_eye_bones: true,
			// LookAt クランプはユーザーの判断（"VRM 1.0 LookAt curve 簡易版"）で明示的に有効化する用途のため、
			// デフォルトでは OFF。manifest `[motion.look_at] enabled = true clamp_deg = 30` で明示的に ON にする。
			eye_look_at_clamp_deg: None,
			apply_root_translation: false,
		}
	}
}

/// [`HumanoidPose`] を、プロファイルとシーンに応じてノードの `transform` に書き込む。
///
/// - `humanoid.root` は（シーンに複数ルートがあっても）**先頭のルート**へ適用する。
/// - [`SampleState::Missing`] のボーンはスキップする。
/// - `skip_eye_bones` が `true` のとき LeftEye / RightEye は適用しない。
pub fn apply_humanoid_pose_to_scene(
	profile: &HumanoidProfile,
	nodes: &mut [UnaSceneNode],
	roots: &[usize],
	pose: &HumanoidPose,
	skip_eye_bones: bool,
) {
	apply_humanoid_pose_to_scene_with_rest(profile, nodes, roots, pose, skip_eye_bones, None);
}

/// [`HumanoidPose`] を rest pose を基準に適用する。
///
/// VMC の Humanoid bone rotation は、そのまま glTF node local rotation に置くと VRM/glTF 側の rest orientation を消してしまう。
/// そのため rest node の `rotation * vmc_rotation` として適用し、translation / scale は rest pose を保持する。
pub fn apply_humanoid_pose_to_scene_with_rest(
	profile: &HumanoidProfile,
	nodes: &mut [UnaSceneNode],
	roots: &[usize],
	pose: &HumanoidPose,
	skip_eye_bones: bool,
	rest_nodes: Option<&[UnaSceneNode]>,
) {
	apply_humanoid_pose_to_scene_with_rest_in_space(
		profile,
		nodes,
		roots,
		pose,
		skip_eye_bones,
		rest_nodes,
		CoordinateSpace::UNMotion,
		TargetHumanoidBasis::Native,
	);
}

/// VRM 1.0 LookAt curve の単純化として、視線 bone の yaw/pitch を `clamp_deg` に制限する。
///
/// 入力 rotation は VMC 由来の eye bone local rotation。`EulerRot::YXZ`（yaw → pitch → roll）で分解し、
/// yaw / pitch のみクランプして再合成する。実 VRM の curve は別途範囲設定が可能だが、ここでは
/// VRM 1.0 spec デフォルトの 30°（呼び出し側が指定する単一値）を採用する。
pub fn clamp_eye_rotation(rot: Quat, clamp_deg: f32) -> Quat {
	let max = clamp_deg.to_radians().abs();
	let (yaw, pitch, roll) = rot.to_euler(EulerRot::YXZ);
	let y = yaw.clamp(-max, max);
	let p = pitch.clamp(-max, max);
	Quat::from_euler(EulerRot::YXZ, y, p, roll)
}

#[allow(clippy::too_many_arguments)]
fn apply_humanoid_pose_to_scene_with_rest_in_space(
	profile: &HumanoidProfile,
	nodes: &mut [UnaSceneNode],
	roots: &[usize],
	pose: &HumanoidPose,
	skip_eye_bones: bool,
	rest_nodes: Option<&[UnaSceneNode]>,
	coordinate_space: CoordinateSpace,
	target_basis: TargetHumanoidBasis,
) {
	apply_humanoid_pose_to_scene_with_rest_in_space_full(
		profile,
		nodes,
		roots,
		pose,
		skip_eye_bones,
		rest_nodes,
		RetargetFrameContext::new(coordinate_space, target_basis, None, None),
		None,
		// 旧来動作（テスト互換）。VMC 経由のドキュメント適用は新しい opts.apply_root_translation を経由する。
		true,
	);
}

#[allow(clippy::too_many_arguments)]
fn apply_humanoid_pose_to_scene_with_rest_in_space_full(
	profile: &HumanoidProfile,
	nodes: &mut [UnaSceneNode],
	roots: &[usize],
	pose: &HumanoidPose,
	skip_eye_bones: bool,
	rest_nodes: Option<&[UnaSceneNode]>,
	frame_ctx: RetargetFrameContext<'_>,
	eye_clamp_deg: Option<f32>,
	apply_root_translation: bool,
) {
	let resolved_roots = resolved_scene_roots(nodes, roots);
	if let (Some(ref root_t), Some(&ri)) = (&pose.root, resolved_roots.first()) {
		if let Some(node) = nodes.get_mut(ri) {
			if let Some(base) = root_base_transform(ri, node, rest_nodes, frame_ctx) {
				let sample_rotation = frame_ctx.transform_rotation(root_t, UnmotionHumanoidRole::Root);
				// translation は opt-in 時のみ rest に加算する。OFF 時は rest pose の base_translation を温存。
				let translation = if apply_root_translation {
					base.translation + frame_ctx.transform_translation(root_t)
				} else {
					base.translation
				};
				node.transform =
					Mat4::from_scale_rotation_translation(base.scale, base.rotation * sample_rotation, translation).to_cols_array();
			} else if apply_root_translation {
				node.transform = Mat4::from_rotation_translation(
					frame_ctx.transform_rotation(root_t, UnmotionHumanoidRole::Root),
					frame_ctx.transform_translation(root_t),
				)
				.to_cols_array();
			} else {
				// rest_nodes が無く apply_root_translation OFF のときは、rotation のみ書き戻し translation は既存値を温存。
				let local = Mat4::from_cols_array(&node.transform);
				let (base_scale, _base_rot, base_translation) = local.to_scale_rotation_translation();
				let sample_rotation = frame_ctx.transform_rotation(root_t, UnmotionHumanoidRole::Root);
				node.transform = Mat4::from_scale_rotation_translation(base_scale, sample_rotation, base_translation).to_cols_array();
			}
		}
	}

	for sample in &pose.bones {
		if sample.state == SampleState::Missing {
			continue;
		}
		if skip_eye_bones && matches!(sample.bone, HumanoidBone::LeftEye | HumanoidBone::RightEye) {
			continue;
		}
		let binding = frame_ctx.body_bone_binding(sample.bone);
		let ni = frame_ctx.body_bone_node_index(profile, sample.bone);
		let Some(ni) = ni else {
			continue;
		};
		apply_humanoid_transform_to_node_index(
			profile,
			nodes,
			rest_nodes,
			frame_ctx,
			ni,
			&sample.transform,
			UnmotionHumanoidRole::BodyBone(sample.bone),
			UnmotionHumanoidRole::BodyBone(sample.bone),
			binding.map(|binding| binding.base),
			false,
			eye_clamp_deg.filter(|_| matches!(sample.bone, HumanoidBone::LeftEye | HumanoidBone::RightEye)),
		);
	}
}

/// [`UNMotionFrame`] のボディ／表情を [`UnaDocument`] に反映する（シーン・式ウェイト）。
pub fn apply_un_motion_frame_to_document(document: &mut UnaDocument, frame: &UNMotionFrame, opts: ApplyUnMotionFrameOpts) {
	apply_un_motion_frame_to_document_with_rest(document, frame, opts, None);
}

/// [`UNMotionFrame`] を rest pose を基準に [`UnaDocument`] へ反映する。
pub fn apply_un_motion_frame_to_document_with_rest(
	document: &mut UnaDocument,
	frame: &UNMotionFrame,
	opts: ApplyUnMotionFrameOpts,
	rest_nodes: Option<&[UnaSceneNode]>,
) {
	let context = HumanoidRetargetContext::for_document(document, rest_nodes);
	apply_un_motion_frame_to_document_with_context(document, frame, opts, rest_nodes, &context);
}

/// Precompiled retarget context を使って [`UNMotionFrame`] を [`UnaDocument`] へ反映する。
pub fn apply_un_motion_frame_to_document_with_context(
	document: &mut UnaDocument,
	frame: &UNMotionFrame,
	opts: ApplyUnMotionFrameOpts,
	rest_nodes: Option<&[UnaSceneNode]>,
	context: &HumanoidRetargetContext,
) {
	let frame_ctx = context.frame_context(frame.header.coordinate_space);
	let body_pose = frame.body.as_ref().and_then(|body| body.humanoid.as_ref());
	let body_pose_ownership = BodyPoseOwnership::from_pose(body_pose);
	{
		let mut runtime_model = document.runtime_model_mut();
		let Some((scene, profile)) = runtime_model.humanoid_scene_mut() else {
			return;
		};
		if let Some(ref body) = frame.body {
			if let Some(ref pose) = body.humanoid {
				apply_humanoid_pose_to_scene_with_rest_in_space_full(
					profile,
					&mut scene.nodes,
					&scene.roots,
					pose,
					!opts.apply_eye_bones,
					rest_nodes,
					frame_ctx,
					if opts.apply_eye_bones { opts.eye_look_at_clamp_deg } else { None },
					opts.apply_root_translation,
				);
			}
		}
		if let Some(ref hand) = frame.left_hand {
			apply_hand_motion_to_scene(
				profile,
				&mut scene.nodes,
				hand,
				"left",
				rest_nodes,
				frame_ctx,
				!body_pose_ownership.left_hand,
			);
		}
		if let Some(ref hand) = frame.right_hand {
			apply_hand_motion_to_scene(
				profile,
				&mut scene.nodes,
				hand,
				"right",
				rest_nodes,
				frame_ctx,
				!body_pose_ownership.right_hand,
			);
		}
		if let Some(ref face) = frame.face {
			if let Some(ref head) = face.head {
				if let Some(binding) = frame_ctx.body_bone_binding(HumanoidBone::Head) {
					apply_humanoid_transform_to_node_index(
						profile,
						&mut scene.nodes,
						rest_nodes,
						frame_ctx,
						binding.node_index,
						head,
						UnmotionHumanoidRole::BodyBone(HumanoidBone::Head),
						UnmotionHumanoidRole::BodyBone(HumanoidBone::Head),
						Some(binding.base),
						true,
						None,
					);
				} else {
					apply_humanoid_transform_to_profile_node(
						profile,
						&mut scene.nodes,
						rest_nodes,
						frame_ctx,
						"head",
						head,
						UnmotionHumanoidRole::BodyBone(HumanoidBone::Head),
						UnmotionHumanoidRole::BodyBone(HumanoidBone::Head),
					);
				}
			}
		}
		if let Some(rest_nodes) = rest_nodes {
			let UnaSceneSnapshot {
				nodes,
				roots,
				node_constraints,
				..
			} = scene;
			apply_node_constraints_to_scene(nodes, roots, node_constraints, rest_nodes);
		}
	}
	if opts.apply_expressions {
		if let Some(ref face) = frame.face {
			if context.runtime.expression_lookup.is_empty() {
				return;
			}
			let mut runtime_model = document.runtime_model_mut();
			for ex in &face.expressions {
				// 完全一致（ASCII case 無視）優先、見つからなければ ARKit BlendShape の表記揺れに耐性のある
				// 正規化マッチ（区切り文字除去 + 全部小文字）でリトライする。
				// 例: VMC `mouthSmileLeft` / `MouthSmileLeft` / `Mouth_Smile_Left` を同じ preset へ。
				if let Some(preset_name) = context.runtime.expression_lookup.preset_name_for(&ex.name) {
					let value = ex.value.clamp(0.0, 1.0);
					let ew = runtime_model.expression_weights_mut();
					if let Some(weight) = ew.preset_weights.get_mut(preset_name) {
						*weight = value;
					} else {
						ew.preset_weights.insert(preset_name.to_owned(), value);
					}
				}
			}
		}
	}
}

/// VRM/ARKit BlendShape 名のマッチング用キー正規化。
/// ASCII 英数字以外（`_`, `-`, 空白など）を全て削り、ASCII 小文字に揃える。
/// `MouthSmileLeft` / `mouthSmileLeft` / `Mouth_Smile_Left` / `Mouth Smile Left` を `mouthsmileleft` に揃え、
/// VRM 0 の PerfectSync 系（PascalCase）と Waidayo 等の ARKit camelCase 送信を相互適用可能にする。
pub fn normalize_expression_match_key(name: &str) -> String {
	name.chars()
		.filter(|c| c.is_ascii_alphanumeric())
		.map(|c| c.to_ascii_lowercase())
		.collect()
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
	use super::*;
	use un_avatar_types::HumanoidProfile;
	use un_motion_frame::{
		BoneSample, FaceMotion, Finger, FingerPose, HandMotion, Quatf, SampleState, TrackingState, UNMotionFrame, Vec3f,
	};

	#[test]
	fn clamp_eye_rotation_limits_yaw_and_pitch() {
		// 60° の yaw を 30° で頭打ちにする
		let yaw_60 = Quat::from_axis_angle(Vec3::Y, 60f32.to_radians());
		let clamped = clamp_eye_rotation(yaw_60, 30.0);
		let (yaw, pitch, roll) = clamped.to_euler(EulerRot::YXZ);
		assert!((yaw - 30f32.to_radians()).abs() < 1e-4, "yaw should be clamped to 30°, got {yaw}");
		assert!(pitch.abs() < 1e-4);
		assert!(roll.abs() < 1e-4);

		// -60° の pitch を ±30° でクランプ
		let pitch_neg60 = Quat::from_axis_angle(Vec3::X, -60f32.to_radians());
		let clamped = clamp_eye_rotation(pitch_neg60, 30.0);
		let (yaw, pitch, roll) = clamped.to_euler(EulerRot::YXZ);
		assert!(yaw.abs() < 1e-4);
		assert!(
			(pitch + 30f32.to_radians()).abs() < 1e-4,
			"pitch should be clamped to -30°, got {pitch}"
		);
		assert!(roll.abs() < 1e-4);

		// 範囲内（10°）はそのまま
		let yaw_10 = Quat::from_axis_angle(Vec3::Y, 10f32.to_radians());
		let clamped = clamp_eye_rotation(yaw_10, 30.0);
		let (yaw, _, _) = clamped.to_euler(EulerRot::YXZ);
		assert!((yaw - 10f32.to_radians()).abs() < 1e-4);
	}

	fn unknown_node() -> UnaSceneNode {
		UnaSceneNode {
			source_node_id: None,
			resolved_node_id: None,
			name: None,
			visible: true,
			transform: Mat4::IDENTITY.to_cols_array(),
			children: vec![],
			mesh: None,
			skin: None,
			probe_anchor_node: None,
			local_bounds: None,
		}
	}

	fn quatf(q: Quat) -> Quatf {
		Quatf {
			x: q.x,
			y: q.y,
			z: q.z,
			w: q.w,
		}
	}

	fn valid_bone_sample(bone: HumanoidBone, rotation: Quat) -> BoneSample {
		BoneSample {
			bone,
			transform: TransformSample {
				translation: None,
				rotation: Some(quatf(rotation)),
				scale: None,
				linear_velocity: None,
				angular_velocity: None,
			},
			confidence: 1.0,
			source_index: Some(0),
			state: SampleState::Valid,
		}
	}

	fn unmotion_body_frame(bone: HumanoidBone, rotation: Quat) -> UNMotionFrame {
		let mut frame = UNMotionFrame::new(0);
		frame.header.coordinate_space = CoordinateSpace::UNMotion;
		frame.body = Some(un_motion_frame::BodyMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			humanoid: Some(HumanoidPose {
				root: None,
				bones: vec![valid_bone_sample(bone, rotation)],
			}),
		});
		frame
	}

	fn normalized_world_bone_axis(scene: &UnaSceneSnapshot, parent: usize, child: usize) -> Vec3 {
		let world = scene_world_matrices(&scene.nodes, &scene.roots);
		let from = world[parent].transform_point3(Vec3::ZERO);
		let to = world[child].transform_point3(Vec3::ZERO);
		(to - from).normalize()
	}

	fn apply_left_upper_arm_sample(mut document: UnaDocument, rest_nodes: Vec<UnaSceneNode>, source_rotation: Quat) -> UnaSceneSnapshot {
		let frame = unmotion_body_frame(HumanoidBone::LeftUpperArm, source_rotation);
		apply_un_motion_frame_to_document_with_rest(&mut document, &frame, ApplyUnMotionFrameOpts::default(), Some(&rest_nodes));
		document.scene.unwrap()
	}

	#[test]
	fn applies_bone_and_single_root() {
		let mut nodes = vec![unknown_node(); 3];
		nodes[2].name = Some("hips".into());
		let roots = vec![0usize];
		let profile = HumanoidProfile {
			bone_node_indices: [("hips".to_string(), 2)].into_iter().collect(),
		};
		let pose = HumanoidPose {
			root: Some(TransformSample {
				translation: Some(Vec3f { x: 1.0, y: 0.0, z: 0.0 }),
				rotation: None,
				scale: None,
				linear_velocity: None,
				angular_velocity: None,
			}),
			bones: vec![BoneSample {
				bone: HumanoidBone::Hips,
				transform: TransformSample {
					translation: Some(Vec3f { x: 0.0, y: 2.0, z: 0.0 }),
					rotation: Some(Quatf {
						x: 0.0,
						y: 0.0,
						z: 0.0,
						w: 1.0,
					}),
					scale: None,
					linear_velocity: None,
					angular_velocity: None,
				},
				confidence: 1.0,
				source_index: None,
				state: SampleState::Valid,
			}],
		};
		apply_humanoid_pose_to_scene(&profile, &mut nodes, &roots, &pose, false);
		let m0 = Mat4::from_cols_array_2d(&[
			nodes[0].transform[0..4].try_into().unwrap(),
			nodes[0].transform[4..8].try_into().unwrap(),
			nodes[0].transform[8..12].try_into().unwrap(),
			nodes[0].transform[12..16].try_into().unwrap(),
		]);
		assert!((m0.w_axis.truncate() - Vec3::new(1.0, 0.0, 0.0)).length() < 1e-5);
		let m2 = Mat4::from_cols_array_2d(&[
			nodes[2].transform[0..4].try_into().unwrap(),
			nodes[2].transform[4..8].try_into().unwrap(),
			nodes[2].transform[8..12].try_into().unwrap(),
			nodes[2].transform[12..16].try_into().unwrap(),
		]);
		assert!(m2.w_axis.truncate().length() < 1e-5);
	}

	#[test]
	fn root_pose_falls_back_to_parentless_scene_root() {
		let mut nodes = vec![unknown_node(); 2];
		nodes[0].children.push(1);
		let profile = HumanoidProfile::default();
		let pose = HumanoidPose {
			root: Some(TransformSample {
				translation: Some(Vec3f { x: 1.0, y: 0.0, z: 0.0 }),
				rotation: None,
				scale: None,
				linear_velocity: None,
				angular_velocity: None,
			}),
			bones: Vec::new(),
		};

		apply_humanoid_pose_to_scene(&profile, &mut nodes, &[], &pose, false);

		let root = Mat4::from_cols_array(&nodes[0].transform);
		let child = Mat4::from_cols_array(&nodes[1].transform);
		assert!((root.w_axis.truncate() - Vec3::new(1.0, 0.0, 0.0)).length() < 1e-5);
		assert!(child.w_axis.truncate().length() < 1e-5);
	}

	#[test]
	fn skips_missing_sample() {
		let mut nodes = vec![unknown_node(); 2];
		let roots: Vec<usize> = vec![];
		let profile = HumanoidProfile {
			bone_node_indices: [("hips".to_string(), 1)].into_iter().collect(),
		};
		let before = nodes[1].transform;
		let pose = HumanoidPose {
			root: None,
			bones: vec![BoneSample {
				bone: HumanoidBone::Hips,
				transform: TransformSample {
					translation: Some(Vec3f { x: 99.0, y: 0.0, z: 0.0 }),
					rotation: None,
					scale: None,
					linear_velocity: None,
					angular_velocity: None,
				},
				confidence: 0.0,
				source_index: None,
				state: SampleState::Missing,
			}],
		};
		apply_humanoid_pose_to_scene(&profile, &mut nodes, &roots, &pose, false);
		assert_eq!(nodes[1].transform, before);
	}

	#[test]
	fn bone_pose_preserves_existing_local_translation() {
		let mut nodes = vec![unknown_node(); 2];
		nodes[1].transform = Mat4::from_translation(Vec3::new(0.25, 1.22, -0.03)).to_cols_array();
		let profile = HumanoidProfile {
			bone_node_indices: [("leftlowerarm".to_string(), 1)].into_iter().collect(),
		};
		let pose = HumanoidPose {
			root: None,
			bones: vec![BoneSample {
				bone: HumanoidBone::LeftLowerArm,
				transform: TransformSample {
					translation: Some(Vec3f { x: 0.0, y: 0.0, z: 0.0 }),
					rotation: Some(Quatf {
						x: 0.0,
						y: 0.0,
						z: 0.0,
						w: 1.0,
					}),
					scale: None,
					linear_velocity: None,
					angular_velocity: None,
				},
				confidence: 1.0,
				source_index: None,
				state: SampleState::Valid,
			}],
		};

		apply_humanoid_pose_to_scene(&profile, &mut nodes, &[], &pose, false);

		let m = Mat4::from_cols_array(&nodes[1].transform);
		assert!((m.w_axis.truncate() - Vec3::new(0.25, 1.22, -0.03)).length() < 1e-5);
	}

	#[test]
	fn rest_pose_rotation_is_preserved_for_identity_vmc_rotation() {
		let rest_rotation = Quat::from_rotation_z(0.35) * Quat::from_rotation_y(-0.7);
		let rest_transform = Mat4::from_scale_rotation_translation(Vec3::ONE, rest_rotation, Vec3::new(0.25, 1.22, -0.03));
		let rest_nodes = vec![
			unknown_node(),
			UnaSceneNode {
				transform: rest_transform.to_cols_array(),
				..unknown_node()
			},
		];
		let mut nodes = rest_nodes.clone();
		let profile = HumanoidProfile {
			bone_node_indices: [("leftlowerarm".to_string(), 1)].into_iter().collect(),
		};
		let pose = HumanoidPose {
			root: None,
			bones: vec![BoneSample {
				bone: HumanoidBone::LeftLowerArm,
				transform: TransformSample {
					translation: Some(Vec3f { x: 0.0, y: 0.0, z: 0.0 }),
					rotation: Some(Quatf {
						x: 0.0,
						y: 0.0,
						z: 0.0,
						w: 1.0,
					}),
					scale: None,
					linear_velocity: None,
					angular_velocity: None,
				},
				confidence: 1.0,
				source_index: None,
				state: SampleState::Valid,
			}],
		};

		apply_humanoid_pose_to_scene_with_rest(&profile, &mut nodes, &[], &pose, false, Some(&rest_nodes));

		let applied = Mat4::from_cols_array(&nodes[1].transform);
		assert!((applied - rest_transform).abs_diff_eq(Mat4::ZERO, 1e-5));
	}

	#[test]
	fn vmc_coordinate_space_flips_z_for_vrm0_root_translation() {
		let mut document = UnaDocument::default();
		document.scene = Some(un_avatar_core::UnaSceneSnapshot {
			meshes: vec![],
			materials: vec![],
			images: vec![],
			image_sources: vec![],
			skins: vec![],
			nodes: vec![unknown_node()],
			roots: vec![0],
			node_constraints: vec![],
			asset_group_ownership: vec![],
		});
		document.humanoid_profile = Some(HumanoidProfile::default());
		let mut frame = UNMotionFrame::new(0);
		frame.header.coordinate_space = CoordinateSpace::Vmc;
		frame.body = Some(un_motion_frame::BodyMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			humanoid: Some(HumanoidPose {
				root: Some(TransformSample {
					translation: Some(Vec3f { x: 1.0, y: 2.0, z: 3.0 }),
					rotation: None,
					scale: None,
					linear_velocity: None,
					angular_velocity: None,
				}),
				bones: vec![],
			}),
		});

		// 既定では apply_root_translation=false なので translation は反映されない (rest = identity)。
		apply_un_motion_frame_to_document(&mut document, &frame, ApplyUnMotionFrameOpts::default());
		let scene = document.scene.clone().unwrap();
		let applied = Mat4::from_cols_array(&scene.nodes[0].transform);
		assert!(
			applied.w_axis.truncate().length() < 1e-5,
			"with apply_root_translation=false, root translation must be ignored; got {:?}",
			applied.w_axis.truncate()
		);

		// opt-in した場合は VRM0 z-flip 込みで (1, 2, -3) になる。
		apply_un_motion_frame_to_document(
			&mut document,
			&frame,
			ApplyUnMotionFrameOpts {
				apply_root_translation: true,
				..ApplyUnMotionFrameOpts::default()
			},
		);
		let scene = document.scene.unwrap();
		let applied = Mat4::from_cols_array(&scene.nodes[0].transform);
		assert!(
			(applied.w_axis.truncate() - Vec3::new(1.0, 2.0, -3.0)).length() < 1e-5,
			"with apply_root_translation=true, root translation must be applied with VRM0 z-flip; got {:?}",
			applied.w_axis.truncate()
		);
	}

	#[test]
	fn applies_typed_hand_finger_motion_to_profile_nodes() {
		let rest_nodes = vec![unknown_node()];
		let mut document = UnaDocument {
			scene: Some(un_avatar_core::UnaSceneSnapshot {
				nodes: rest_nodes.clone(),
				roots: vec![0],
				..Default::default()
			}),
			humanoid_profile: Some(HumanoidProfile {
				bone_node_indices: [("rightindexintermediate".to_string(), 0)].into_iter().collect(),
			}),
			..Default::default()
		};
		let mut frame = UNMotionFrame::new(0);
		frame.right_hand = Some(HandMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			wrist: None,
			fingers: vec![FingerPose {
				finger: Finger::Index,
				joints: vec![
					TransformSample {
						translation: None,
						rotation: Some(Quatf {
							x: 0.0,
							y: 0.0,
							z: 0.0,
							w: 1.0,
						}),
						scale: None,
						linear_velocity: None,
						angular_velocity: None,
					},
					TransformSample {
						translation: None,
						rotation: Some(Quatf {
							x: 0.0,
							y: 0.0,
							z: 0.24740396,
							w: 0.9689124,
						}),
						scale: None,
						linear_velocity: None,
						angular_velocity: None,
					},
				],
				confidence: 1.0,
			}],
		});

		apply_un_motion_frame_to_document_with_rest(&mut document, &frame, ApplyUnMotionFrameOpts::default(), Some(&rest_nodes));

		let node = &document.scene.as_ref().unwrap().nodes[0];
		let (_, rotation, _) = Mat4::from_cols_array(&node.transform).to_scale_rotation_translation();
		assert!(rotation.angle_between(Quat::from_rotation_z(0.5)) < 1e-4);
	}

	#[test]
	fn applies_typed_hand_finger_motion_to_normalized_profile_keys() {
		let rest_nodes = vec![unknown_node()];
		let mut document = UnaDocument {
			scene: Some(un_avatar_core::UnaSceneSnapshot {
				nodes: rest_nodes.clone(),
				roots: vec![0],
				..Default::default()
			}),
			humanoid_profile: Some(HumanoidProfile {
				bone_node_indices: [("right_index_intermediate".to_string(), 0)].into_iter().collect(),
			}),
			..Default::default()
		};
		let mut frame = UNMotionFrame::new(0);
		frame.right_hand = Some(HandMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			wrist: None,
			fingers: vec![FingerPose {
				finger: Finger::Index,
				joints: vec![
					TransformSample {
						translation: None,
						rotation: Some(Quatf {
							x: 0.0,
							y: 0.0,
							z: 0.0,
							w: 1.0,
						}),
						scale: None,
						linear_velocity: None,
						angular_velocity: None,
					},
					TransformSample {
						translation: None,
						rotation: Some(Quatf {
							x: 0.0,
							y: 0.0,
							z: 0.24740396,
							w: 0.9689124,
						}),
						scale: None,
						linear_velocity: None,
						angular_velocity: None,
					},
				],
				confidence: 1.0,
			}],
		});

		apply_un_motion_frame_to_document_with_rest(&mut document, &frame, ApplyUnMotionFrameOpts::default(), Some(&rest_nodes));

		let node = &document.scene.as_ref().unwrap().nodes[0];
		let (_, rotation, _) = Mat4::from_cols_array(&node.transform).to_scale_rotation_translation();
		assert!(rotation.angle_between(Quat::from_rotation_z(0.5)) < 1e-4);
	}

	#[test]
	fn applies_body_motion_to_normalized_profile_keys() {
		let rest_nodes = vec![
			unknown_node(),
			UnaSceneNode {
				transform: Mat4::IDENTITY.to_cols_array(),
				..unknown_node()
			},
		];
		let mut document = UnaDocument {
			scene: Some(un_avatar_core::UnaSceneSnapshot {
				nodes: rest_nodes.clone(),
				roots: vec![0],
				..Default::default()
			}),
			humanoid_profile: Some(HumanoidProfile {
				bone_node_indices: [("left_upper_arm".to_string(), 1)].into_iter().collect(),
			}),
			..Default::default()
		};
		let rotation = Quat::from_rotation_z(0.35);
		let frame = unmotion_body_frame(HumanoidBone::LeftUpperArm, rotation);

		apply_un_motion_frame_to_document_with_rest(&mut document, &frame, ApplyUnMotionFrameOpts::default(), Some(&rest_nodes));

		let node = &document.scene.as_ref().unwrap().nodes[1];
		let (_, applied, _) = Mat4::from_cols_array(&node.transform).to_scale_rotation_translation();
		assert!(
			applied.angle_between(rotation) < 1e-4,
			"body motion should resolve normalized profile keys; got {applied:?}"
		);
	}

	#[test]
	fn vmc_coordinate_space_converts_typed_hand_fingers() {
		let rest_rotation = Quat::from_rotation_y(0.25);
		let rest_transform = Mat4::from_scale_rotation_translation(Vec3::ONE, rest_rotation, Vec3::new(0.2, 1.0, 0.0));
		let rest_nodes = vec![
			unknown_node(),
			UnaSceneNode {
				transform: rest_transform.to_cols_array(),
				..unknown_node()
			},
		];
		let mut document = UnaDocument {
			scene: Some(un_avatar_core::UnaSceneSnapshot {
				nodes: rest_nodes.clone(),
				roots: vec![0],
				..Default::default()
			}),
			humanoid_profile: Some(HumanoidProfile {
				bone_node_indices: [("rightindexintermediate".to_string(), 1)].into_iter().collect(),
			}),
			..Default::default()
		};
		let vmc_rotation = Quat::from_rotation_x(0.5);
		let mut frame = UNMotionFrame::new(0);
		frame.header.coordinate_space = CoordinateSpace::Vmc;
		frame.right_hand = Some(HandMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			wrist: None,
			fingers: vec![FingerPose {
				finger: Finger::Index,
				joints: vec![
					TransformSample {
						translation: None,
						rotation: Some(Quatf {
							x: 0.0,
							y: 0.0,
							z: 0.0,
							w: 1.0,
						}),
						scale: None,
						linear_velocity: None,
						angular_velocity: None,
					},
					TransformSample {
						translation: None,
						rotation: Some(Quatf {
							x: vmc_rotation.x,
							y: vmc_rotation.y,
							z: vmc_rotation.z,
							w: vmc_rotation.w,
						}),
						scale: None,
						linear_velocity: None,
						angular_velocity: None,
					},
				],
				confidence: 1.0,
			}],
		});

		apply_un_motion_frame_to_document_with_rest(&mut document, &frame, ApplyUnMotionFrameOpts::default(), Some(&rest_nodes));

		let node = &document.scene.as_ref().unwrap().nodes[1];
		let (_, rotation, translation) = Mat4::from_cols_array(&node.transform).to_scale_rotation_translation();
		let expected_rotation = rest_rotation * Quat::from_rotation_x(-0.5);
		assert!(rotation.angle_between(expected_rotation) < 1e-4);
		assert!((translation - Vec3::new(0.2, 1.0, 0.0)).length() < 1e-5);
	}

	#[test]
	fn partial_unmotion_frames_compose_without_resetting_previous_bones() {
		let rest_nodes = vec![
			unknown_node(),
			UnaSceneNode {
				transform: Mat4::from_translation(Vec3::new(0.0, 1.5, 0.0)).to_cols_array(),
				..unknown_node()
			},
			UnaSceneNode {
				transform: Mat4::from_translation(Vec3::new(0.2, 1.0, 0.0)).to_cols_array(),
				..unknown_node()
			},
		];
		let mut document = UnaDocument {
			scene: Some(un_avatar_core::UnaSceneSnapshot {
				nodes: rest_nodes.clone(),
				roots: vec![0],
				..Default::default()
			}),
			humanoid_profile: Some(HumanoidProfile {
				bone_node_indices: [("head".to_string(), 1), ("rightindexintermediate".to_string(), 2)]
					.into_iter()
					.collect(),
			}),
			..Default::default()
		};

		let mut head_frame = UNMotionFrame::new(1);
		head_frame.header.coordinate_space = CoordinateSpace::Vmc;
		let head_vmc_rotation = Quat::from_rotation_x(0.4);
		head_frame.body = Some(un_motion_frame::BodyMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			humanoid: Some(HumanoidPose {
				root: None,
				bones: vec![BoneSample {
					bone: HumanoidBone::Head,
					transform: TransformSample {
						translation: None,
						rotation: Some(Quatf {
							x: head_vmc_rotation.x,
							y: head_vmc_rotation.y,
							z: head_vmc_rotation.z,
							w: head_vmc_rotation.w,
						}),
						scale: None,
						linear_velocity: None,
						angular_velocity: None,
					},
					confidence: 1.0,
					source_index: Some(0),
					state: SampleState::Valid,
				}],
			}),
		});

		let mut hand_frame = UNMotionFrame::new(2);
		hand_frame.header.coordinate_space = CoordinateSpace::UNMotion;
		let finger_rotation = Quat::from_rotation_z(0.5);
		hand_frame.right_hand = Some(HandMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			wrist: None,
			fingers: vec![FingerPose {
				finger: Finger::Index,
				joints: vec![
					TransformSample {
						translation: None,
						rotation: Some(Quatf {
							x: 0.0,
							y: 0.0,
							z: 0.0,
							w: 1.0,
						}),
						scale: None,
						linear_velocity: None,
						angular_velocity: None,
					},
					TransformSample {
						translation: None,
						rotation: Some(Quatf {
							x: finger_rotation.x,
							y: finger_rotation.y,
							z: finger_rotation.z,
							w: finger_rotation.w,
						}),
						scale: None,
						linear_velocity: None,
						angular_velocity: None,
					},
				],
				confidence: 1.0,
			}],
		});

		apply_un_motion_frame_to_document_with_rest(&mut document, &head_frame, ApplyUnMotionFrameOpts::default(), Some(&rest_nodes));
		apply_un_motion_frame_to_document_with_rest(&mut document, &hand_frame, ApplyUnMotionFrameOpts::default(), Some(&rest_nodes));

		let scene = document.scene.as_ref().unwrap();
		let (_, head_rotation, _) = Mat4::from_cols_array(&scene.nodes[1].transform).to_scale_rotation_translation();
		let (_, finger_applied, _) = Mat4::from_cols_array(&scene.nodes[2].transform).to_scale_rotation_translation();
		let expected_head = Quat::from_rotation_x(-0.4);
		let expected_finger = finger_rotation;
		assert!(
			(head_rotation.dot(expected_head).abs() - 1.0).abs() < 1e-5,
			"expected head {:?}, got {:?}",
			expected_head,
			head_rotation
		);
		assert!(
			(finger_applied.dot(expected_finger).abs() - 1.0).abs() < 1e-5,
			"expected finger {:?}, got {:?}",
			expected_finger,
			finger_applied
		);
	}

	#[test]
	fn hand_motion_without_wrist_preserves_body_hand_bone_rotation() {
		let rest_nodes = vec![
			unknown_node(),
			UnaSceneNode {
				transform: Mat4::from_translation(Vec3::new(0.2, 1.0, 0.0)).to_cols_array(),
				..unknown_node()
			},
			UnaSceneNode {
				transform: Mat4::from_translation(Vec3::new(0.25, 1.05, 0.0)).to_cols_array(),
				..unknown_node()
			},
		];
		let mut document = UnaDocument {
			scene: Some(un_avatar_core::UnaSceneSnapshot {
				nodes: rest_nodes.clone(),
				roots: vec![0],
				..Default::default()
			}),
			humanoid_profile: Some(HumanoidProfile {
				bone_node_indices: [("righthand".to_string(), 1), ("rightindexintermediate".to_string(), 2)]
					.into_iter()
					.collect(),
			}),
			..Default::default()
		};
		let hand_bone_rotation = Quat::from_rotation_y(0.35);
		let finger_rotation = Quat::from_rotation_z(-0.45);
		let mut frame = UNMotionFrame::new(3);
		frame.header.coordinate_space = CoordinateSpace::UNMotion;
		frame.body = Some(un_motion_frame::BodyMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			humanoid: Some(HumanoidPose {
				root: None,
				bones: vec![BoneSample {
					bone: HumanoidBone::RightHand,
					transform: TransformSample {
						translation: None,
						rotation: Some(Quatf {
							x: hand_bone_rotation.x,
							y: hand_bone_rotation.y,
							z: hand_bone_rotation.z,
							w: hand_bone_rotation.w,
						}),
						scale: None,
						linear_velocity: None,
						angular_velocity: None,
					},
					confidence: 1.0,
					source_index: Some(0),
					state: SampleState::Valid,
				}],
			}),
		});
		frame.right_hand = Some(HandMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			wrist: None,
			fingers: vec![FingerPose {
				finger: Finger::Index,
				joints: vec![
					TransformSample {
						translation: None,
						rotation: Some(Quatf {
							x: 0.0,
							y: 0.0,
							z: 0.0,
							w: 1.0,
						}),
						scale: None,
						linear_velocity: None,
						angular_velocity: None,
					},
					TransformSample {
						translation: None,
						rotation: Some(Quatf {
							x: finger_rotation.x,
							y: finger_rotation.y,
							z: finger_rotation.z,
							w: finger_rotation.w,
						}),
						scale: None,
						linear_velocity: None,
						angular_velocity: None,
					},
				],
				confidence: 1.0,
			}],
		});

		apply_un_motion_frame_to_document_with_rest(&mut document, &frame, ApplyUnMotionFrameOpts::default(), Some(&rest_nodes));

		let scene = document.scene.as_ref().unwrap();
		let (_, hand_applied, _) = Mat4::from_cols_array(&scene.nodes[1].transform).to_scale_rotation_translation();
		let (_, finger_applied, _) = Mat4::from_cols_array(&scene.nodes[2].transform).to_scale_rotation_translation();
		let expected_hand = Quat::from_xyzw(
			-hand_bone_rotation.x,
			-hand_bone_rotation.y,
			hand_bone_rotation.z,
			hand_bone_rotation.w,
		);
		let expected_finger = finger_rotation;
		assert!(
			(hand_applied.dot(expected_hand).abs() - 1.0).abs() < 1e-5,
			"hand motion without wrist must not reset body-owned RightHand rotation"
		);
		assert!(
			(finger_applied.dot(expected_finger).abs() - 1.0).abs() < 1e-5,
			"finger joints should still apply"
		);
	}

	/// 回帰テスト: VMC が non-zero な Root translation を送ってきても、
	/// `apply_root_translation = false` (既定) の場合は rest の base_translation を温存する。
	/// model1.vrm の "Root" ノードがアバター armature root のケースを再現する。
	#[test]
	fn vmc_root_translation_does_not_shift_avatar_by_default() {
		let rest_root = Mat4::IDENTITY;
		let rest_nodes = vec![UnaSceneNode {
			transform: rest_root.to_cols_array(),
			..unknown_node()
		}];
		let mut document = UnaDocument::default();
		document.scene = Some(un_avatar_core::UnaSceneSnapshot {
			meshes: vec![],
			materials: vec![],
			images: vec![],
			image_sources: vec![],
			skins: vec![],
			nodes: rest_nodes.clone(),
			roots: vec![0],
			node_constraints: vec![],
			asset_group_ownership: vec![],
		});
		document.humanoid_profile = Some(HumanoidProfile::default());
		let mut frame = UNMotionFrame::new(0);
		frame.header.coordinate_space = CoordinateSpace::Vmc;
		frame.body = Some(un_motion_frame::BodyMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			humanoid: Some(HumanoidPose {
				root: Some(TransformSample {
					translation: Some(Vec3f { x: 0.0, y: 0.0, z: -1.0 }),
					rotation: None,
					scale: None,
					linear_velocity: None,
					angular_velocity: None,
				}),
				bones: vec![],
			}),
		});

		apply_un_motion_frame_to_document_with_rest(&mut document, &frame, ApplyUnMotionFrameOpts::default(), Some(&rest_nodes));
		let scene = document.scene.unwrap();
		let applied = Mat4::from_cols_array(&scene.nodes[0].transform);
		let pos = applied.w_axis.truncate();
		assert!(
			pos.length() < 1e-5,
			"VMC Root.translation must NOT shift the avatar when apply_root_translation=false; got {:?}",
			pos
		);
	}

	#[test]
	fn vmc_root_motion_preserves_rest_root_rotation() {
		let rest_root = Mat4::from_rotation_y(std::f32::consts::PI);
		let rest_nodes = vec![UnaSceneNode {
			transform: rest_root.to_cols_array(),
			..unknown_node()
		}];
		let mut document = UnaDocument::default();
		document.scene = Some(un_avatar_core::UnaSceneSnapshot {
			meshes: vec![],
			materials: vec![],
			images: vec![],
			image_sources: vec![],
			skins: vec![],
			nodes: rest_nodes.clone(),
			roots: vec![0],
			node_constraints: vec![],
			asset_group_ownership: vec![],
		});
		document.humanoid_profile = Some(HumanoidProfile::default());
		let mut frame = UNMotionFrame::new(0);
		frame.header.coordinate_space = CoordinateSpace::Vmc;
		frame.body = Some(un_motion_frame::BodyMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			humanoid: Some(HumanoidPose {
				root: Some(TransformSample {
					translation: Some(Vec3f { x: 0.0, y: 0.0, z: 0.0 }),
					rotation: None,
					scale: None,
					linear_velocity: None,
					angular_velocity: None,
				}),
				bones: vec![],
			}),
		});

		apply_un_motion_frame_to_document_with_rest(&mut document, &frame, ApplyUnMotionFrameOpts::default(), Some(&rest_nodes));

		let scene = document.scene.unwrap();
		let applied = Mat4::from_cols_array(&scene.nodes[0].transform);
		assert!(applied.abs_diff_eq(rest_root, 1e-5));
	}

	#[test]
	fn vmc_coordinate_space_converts_quaternion_before_rest_rotation() {
		let rest_rotation = Quat::from_rotation_y(0.25);
		let rest_transform = Mat4::from_scale_rotation_translation(Vec3::ONE, rest_rotation, Vec3::new(0.2, 1.0, 0.0));
		let rest_nodes = vec![
			unknown_node(),
			UnaSceneNode {
				transform: rest_transform.to_cols_array(),
				..unknown_node()
			},
		];
		let mut document = UnaDocument::default();
		document.scene = Some(un_avatar_core::UnaSceneSnapshot {
			meshes: vec![],
			materials: vec![],
			images: vec![],
			image_sources: vec![],
			skins: vec![],
			nodes: rest_nodes.clone(),
			roots: vec![],
			node_constraints: vec![],
			asset_group_ownership: vec![],
		});
		document.humanoid_profile = Some(HumanoidProfile {
			bone_node_indices: [("leftlowerarm".to_string(), 1)].into_iter().collect(),
		});
		let vmc_rotation = Quat::from_rotation_x(0.5);
		let mut frame = UNMotionFrame::new(0);
		frame.header.coordinate_space = CoordinateSpace::Vmc;
		frame.body = Some(un_motion_frame::BodyMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			humanoid: Some(HumanoidPose {
				root: None,
				bones: vec![BoneSample {
					bone: HumanoidBone::LeftLowerArm,
					transform: TransformSample {
						translation: None,
						rotation: Some(Quatf {
							x: vmc_rotation.x,
							y: vmc_rotation.y,
							z: vmc_rotation.z,
							w: vmc_rotation.w,
						}),
						scale: None,
						linear_velocity: None,
						angular_velocity: None,
					},
					confidence: 1.0,
					source_index: Some(0),
					state: SampleState::Valid,
				}],
			}),
		});

		apply_un_motion_frame_to_document_with_rest(&mut document, &frame, ApplyUnMotionFrameOpts::default(), Some(&rest_nodes));

		let scene = document.scene.unwrap();
		let applied = Mat4::from_cols_array(&scene.nodes[1].transform);
		let expected_rotation = rest_rotation * Quat::from_rotation_x(-0.5);
		let expected = Mat4::from_scale_rotation_translation(Vec3::ONE, expected_rotation, Vec3::new(0.2, 1.0, 0.0));
		assert!((applied - expected).abs_diff_eq(Mat4::ZERO, 1e-5));
	}

	#[test]
	fn vmc_coordinate_space_uses_vrm1_basis_when_document_is_vrm1() {
		let rest_nodes = vec![
			unknown_node(),
			UnaSceneNode {
				transform: Mat4::from_translation(Vec3::new(0.2, 1.0, 0.0)).to_cols_array(),
				..unknown_node()
			},
		];
		let mut document = UnaDocument::default();
		document.vrm = Some(un_avatar_core::UnaVrmExtension {
			spec_version: "1.0".to_string(),
			meta: serde_json::Value::Null,
			humanoid_bones: Default::default(),
			mtoon_materials_v0: vec![],
			mtoon_material_indices_v1: vec![],
			source: serde_json::Value::Null,
		});
		document.scene = Some(un_avatar_core::UnaSceneSnapshot {
			meshes: vec![],
			materials: vec![],
			images: vec![],
			image_sources: vec![],
			skins: vec![],
			nodes: rest_nodes.clone(),
			roots: vec![0],
			node_constraints: vec![],
			asset_group_ownership: vec![],
		});
		document.humanoid_profile = Some(HumanoidProfile {
			bone_node_indices: [("leftlowerarm".to_string(), 1)].into_iter().collect(),
		});
		let vmc_rotation = Quat::from_rotation_x(0.5);
		let mut frame = UNMotionFrame::new(0);
		frame.header.coordinate_space = CoordinateSpace::Vmc;
		frame.body = Some(un_motion_frame::BodyMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			humanoid: Some(HumanoidPose {
				root: Some(TransformSample {
					translation: Some(Vec3f { x: 1.0, y: 2.0, z: 3.0 }),
					rotation: None,
					scale: None,
					linear_velocity: None,
					angular_velocity: None,
				}),
				bones: vec![BoneSample {
					bone: HumanoidBone::LeftLowerArm,
					transform: TransformSample {
						translation: None,
						rotation: Some(Quatf {
							x: vmc_rotation.x,
							y: vmc_rotation.y,
							z: vmc_rotation.z,
							w: vmc_rotation.w,
						}),
						scale: None,
						linear_velocity: None,
						angular_velocity: None,
					},
					confidence: 1.0,
					source_index: Some(0),
					state: SampleState::Valid,
				}],
			}),
		});

		apply_un_motion_frame_to_document_with_rest(
			&mut document,
			&frame,
			ApplyUnMotionFrameOpts {
				apply_root_translation: true,
				..ApplyUnMotionFrameOpts::default()
			},
			Some(&rest_nodes),
		);

		let scene = document.scene.unwrap();
		let root_transform = Mat4::from_cols_array(&scene.nodes[0].transform);
		assert!((root_transform.w_axis.truncate() - Vec3::new(-1.0, 2.0, 3.0)).length() < 1e-5);
		let applied = Mat4::from_cols_array(&scene.nodes[1].transform);
		let expected = Mat4::from_scale_rotation_translation(Vec3::ONE, Quat::from_rotation_x(0.5), Vec3::new(0.2, 1.0, 0.0));
		assert!((applied - expected).abs_diff_eq(Mat4::ZERO, 1e-5));
	}

	/// 1.0.0 互換: VRM0 の Humanoid body/root へ入る UNMotion は VRM0 target basis へ変換して合成する。
	#[test]
	fn vrm0_unmotion_humanoid_body_uses_reference_target_basis() {
		let rest_rotation = Quat::from_rotation_y(0.25);
		let rest_transform = Mat4::from_scale_rotation_translation(Vec3::ONE, rest_rotation, Vec3::new(0.2, 1.0, 0.0));
		let rest_nodes = vec![
			unknown_node(),
			UnaSceneNode {
				transform: rest_transform.to_cols_array(),
				..unknown_node()
			},
		];
		let mut document = UnaDocument::default();
		document.scene = Some(un_avatar_core::UnaSceneSnapshot {
			meshes: vec![],
			materials: vec![],
			images: vec![],
			image_sources: vec![],
			skins: vec![],
			nodes: rest_nodes.clone(),
			roots: vec![0],
			node_constraints: vec![],
			asset_group_ownership: vec![],
		});
		document.humanoid_profile = Some(HumanoidProfile {
			bone_node_indices: [("leftlowerarm".to_string(), 1)].into_iter().collect(),
		});
		let source_rotation = Quat::from_rotation_x(0.5);
		let mut frame = UNMotionFrame::new(0);
		frame.header.coordinate_space = CoordinateSpace::UNMotion;
		frame.body = Some(un_motion_frame::BodyMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			humanoid: Some(HumanoidPose {
				root: Some(TransformSample {
					translation: Some(Vec3f { x: 1.0, y: 2.0, z: 3.0 }),
					rotation: None,
					scale: None,
					linear_velocity: None,
					angular_velocity: None,
				}),
				bones: vec![BoneSample {
					bone: HumanoidBone::LeftLowerArm,
					transform: TransformSample {
						translation: None,
						rotation: Some(Quatf {
							x: source_rotation.x,
							y: source_rotation.y,
							z: source_rotation.z,
							w: source_rotation.w,
						}),
						scale: None,
						linear_velocity: None,
						angular_velocity: None,
					},
					confidence: 1.0,
					source_index: Some(0),
					state: SampleState::Valid,
				}],
			}),
		});

		apply_un_motion_frame_to_document_with_rest(
			&mut document,
			&frame,
			ApplyUnMotionFrameOpts {
				apply_root_translation: true,
				..ApplyUnMotionFrameOpts::default()
			},
			Some(&rest_nodes),
		);

		let scene = document.scene.unwrap();
		let root_transform = Mat4::from_cols_array(&scene.nodes[0].transform);
		assert!(
			(root_transform.w_axis.truncate() - Vec3::new(1.0, 2.0, -3.0)).length() < 1e-5,
			"VRM0 UNMotion humanoid root translation must use the 1.0.0 target-basis conversion; got {:?}",
			root_transform.w_axis.truncate()
		);
		let applied = Mat4::from_cols_array(&scene.nodes[1].transform);
		let expected_source = Quat::from_xyzw(-source_rotation.x, -source_rotation.y, source_rotation.z, source_rotation.w);
		let expected_rotation = rest_rotation * expected_source;
		let expected = Mat4::from_scale_rotation_translation(Vec3::ONE, expected_rotation, Vec3::new(0.2, 1.0, 0.0));
		assert!(
			(applied - expected).abs_diff_eq(Mat4::ZERO, 1e-5),
			"VRM0 UNMotion humanoid bone rotation must use the 1.0.0 target-basis conversion (got {:?}, expected {:?})",
			applied,
			expected
		);
	}

	#[test]
	fn unavatar_unmotion_arm_limb_uses_unity_limb_axis_basis() {
		let rest_rotation = Quat::from_rotation_y(0.25);
		let rest_transform = Mat4::from_scale_rotation_translation(Vec3::ONE, rest_rotation, Vec3::new(0.2, 1.0, 0.0));
		let rest_nodes = vec![
			unknown_node(),
			UnaSceneNode {
				transform: rest_transform.to_cols_array(),
				..unknown_node()
			},
		];
		let mut document = UnaDocument::default();
		document.unavatar = Some(un_avatar_core::UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::Value::Null,
		});
		document.scene = Some(un_avatar_core::UnaSceneSnapshot {
			meshes: vec![],
			materials: vec![],
			images: vec![],
			image_sources: vec![],
			skins: vec![],
			nodes: rest_nodes.clone(),
			roots: vec![0],
			node_constraints: vec![],
			asset_group_ownership: vec![],
		});
		document.humanoid_profile = Some(HumanoidProfile {
			bone_node_indices: [("leftlowerarm".to_string(), 1)].into_iter().collect(),
		});
		let source_rotation = Quat::from_rotation_z(0.5);
		let mut frame = UNMotionFrame::new(0);
		frame.header.coordinate_space = CoordinateSpace::UNMotion;
		frame.body = Some(un_motion_frame::BodyMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			humanoid: Some(HumanoidPose {
				root: None,
				bones: vec![BoneSample {
					bone: HumanoidBone::LeftLowerArm,
					transform: TransformSample {
						translation: None,
						rotation: Some(Quatf {
							x: source_rotation.x,
							y: source_rotation.y,
							z: source_rotation.z,
							w: source_rotation.w,
						}),
						scale: None,
						linear_velocity: None,
						angular_velocity: None,
					},
					confidence: 1.0,
					source_index: Some(0),
					state: SampleState::Valid,
				}],
			}),
		});

		apply_un_motion_frame_to_document_with_rest(&mut document, &frame, ApplyUnMotionFrameOpts::default(), Some(&rest_nodes));

		let scene = document.scene.unwrap();
		let applied = Mat4::from_cols_array(&scene.nodes[1].transform);
		let source_in_limb_axis = Quat::from_xyzw(source_rotation.x, -source_rotation.y, -source_rotation.z, source_rotation.w);
		let expected_rotation = rest_rotation * source_in_limb_axis;
		let expected = Mat4::from_scale_rotation_translation(Vec3::ONE, expected_rotation, Vec3::new(0.2, 1.0, 0.0));
		assert!(
			(applied - expected).abs_diff_eq(Mat4::ZERO, 1e-5),
			".unavatar UNMotion arm limb rotation must use the Unity humanoid limb-axis correction"
		);
	}

	#[test]
	fn unavatar_unmotion_lower_arm_adapts_canonical_arm_axis_to_converted_rest_child_axis() {
		let rest_nodes = vec![
			unknown_node(),
			UnaSceneNode {
				transform: Mat4::IDENTITY.to_cols_array(),
				children: vec![2],
				..unknown_node()
			},
			UnaSceneNode {
				transform: Mat4::from_translation(Vec3::Y).to_cols_array(),
				..unknown_node()
			},
		];
		let mut document = UnaDocument::default();
		document.unavatar = Some(un_avatar_core::UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::Value::Null,
		});
		document.scene = Some(un_avatar_core::UnaSceneSnapshot {
			meshes: vec![],
			materials: vec![],
			images: vec![],
			image_sources: vec![],
			skins: vec![],
			nodes: rest_nodes.clone(),
			roots: vec![0],
			node_constraints: vec![],
			asset_group_ownership: vec![],
		});
		document.humanoid_profile = Some(HumanoidProfile {
			bone_node_indices: [("leftlowerarm".to_string(), 1)].into_iter().collect(),
		});
		let source_rotation = Quat::from_rotation_arc(-Vec3::X, Vec3::Z);
		let mut frame = UNMotionFrame::new(0);
		frame.header.coordinate_space = CoordinateSpace::UNMotion;
		frame.body = Some(un_motion_frame::BodyMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			humanoid: Some(HumanoidPose {
				root: None,
				bones: vec![BoneSample {
					bone: HumanoidBone::LeftLowerArm,
					transform: TransformSample {
						translation: None,
						rotation: Some(Quatf {
							x: source_rotation.x,
							y: source_rotation.y,
							z: source_rotation.z,
							w: source_rotation.w,
						}),
						scale: None,
						linear_velocity: None,
						angular_velocity: None,
					},
					confidence: 1.0,
					source_index: Some(0),
					state: SampleState::Valid,
				}],
			}),
		});

		apply_un_motion_frame_to_document_with_rest(&mut document, &frame, ApplyUnMotionFrameOpts::default(), Some(&rest_nodes));

		let scene = document.scene.unwrap();
		let (_, applied, _) = Mat4::from_cols_array(&scene.nodes[1].transform).to_scale_rotation_translation();
		let child_axis_after_rotation = applied * Vec3::Y;
		let expected_axis = Vec3::Z;
		assert!(
			(child_axis_after_rotation - expected_axis).length() < 1e-5,
			".unavatar lower arm rest child axis should follow the UNMotion canonical arm axis after target-basis conversion; got {:?}",
			child_axis_after_rotation
		);
	}

	#[test]
	fn unavatar_unmotion_arm_axis_adapter_includes_rest_rotation() {
		let rest_rotation = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
		let rest_nodes = vec![
			unknown_node(),
			UnaSceneNode {
				transform: Mat4::from_rotation_translation(rest_rotation, Vec3::ZERO).to_cols_array(),
				children: vec![2],
				..unknown_node()
			},
			UnaSceneNode {
				transform: Mat4::from_translation(Vec3::Y).to_cols_array(),
				..unknown_node()
			},
		];
		let mut document = UnaDocument::default();
		document.unavatar = Some(un_avatar_core::UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::Value::Null,
		});
		document.scene = Some(un_avatar_core::UnaSceneSnapshot {
			meshes: vec![],
			materials: vec![],
			images: vec![],
			image_sources: vec![],
			skins: vec![],
			nodes: rest_nodes.clone(),
			roots: vec![0],
			node_constraints: vec![],
			asset_group_ownership: vec![],
		});
		document.humanoid_profile = Some(HumanoidProfile {
			bone_node_indices: [("leftupperarm".to_string(), 1)].into_iter().collect(),
		});
		let source_rotation = Quat::from_rotation_arc(-Vec3::X, Vec3::Z);
		let mut frame = UNMotionFrame::new(0);
		frame.header.coordinate_space = CoordinateSpace::UNMotion;
		frame.body = Some(un_motion_frame::BodyMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			humanoid: Some(HumanoidPose {
				root: None,
				bones: vec![BoneSample {
					bone: HumanoidBone::LeftUpperArm,
					transform: TransformSample {
						translation: None,
						rotation: Some(Quatf {
							x: source_rotation.x,
							y: source_rotation.y,
							z: source_rotation.z,
							w: source_rotation.w,
						}),
						scale: None,
						linear_velocity: None,
						angular_velocity: None,
					},
					confidence: 1.0,
					source_index: Some(0),
					state: SampleState::Valid,
				}],
			}),
		});

		apply_un_motion_frame_to_document_with_rest(&mut document, &frame, ApplyUnMotionFrameOpts::default(), Some(&rest_nodes));

		let scene = document.scene.unwrap();
		let (_, applied, _) = Mat4::from_cols_array(&scene.nodes[1].transform).to_scale_rotation_translation();
		let child_axis_after_rotation = applied * Vec3::Y;
		assert!(
			(child_axis_after_rotation - Vec3::Z).length() < 1e-5,
			".unavatar arm axis adapter must account for rest rotation; got {:?}",
			child_axis_after_rotation
		);
	}

	#[test]
	fn unavatar_unmotion_shoulder_axis_uses_humanoid_successor_not_first_decoration_child() {
		let rest_nodes = vec![
			unknown_node(),
			UnaSceneNode {
				transform: Mat4::IDENTITY.to_cols_array(),
				children: vec![2, 3],
				..unknown_node()
			},
			UnaSceneNode {
				name: Some("Shoulder_Ribbon_FrontRoot_L".to_string()),
				transform: Mat4::from_translation(Vec3::X).to_cols_array(),
				..unknown_node()
			},
			UnaSceneNode {
				name: Some("Upperarm_L".to_string()),
				transform: Mat4::from_translation(Vec3::Y).to_cols_array(),
				..unknown_node()
			},
		];
		let mut document = UnaDocument::default();
		document.unavatar = Some(un_avatar_core::UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::Value::Null,
		});
		document.scene = Some(un_avatar_core::UnaSceneSnapshot {
			meshes: vec![],
			materials: vec![],
			images: vec![],
			image_sources: vec![],
			skins: vec![],
			nodes: rest_nodes.clone(),
			roots: vec![0],
			node_constraints: vec![],
			asset_group_ownership: vec![],
		});
		document.humanoid_profile = Some(HumanoidProfile {
			bone_node_indices: [("leftshoulder".to_string(), 1), ("leftupperarm".to_string(), 3)]
				.into_iter()
				.collect(),
		});
		let source_rotation = Quat::from_rotation_arc(-Vec3::X, Vec3::Z);
		let frame = unmotion_body_frame(HumanoidBone::LeftShoulder, source_rotation);

		apply_un_motion_frame_to_document_with_rest(&mut document, &frame, ApplyUnMotionFrameOpts::default(), Some(&rest_nodes));

		let scene = document.scene.unwrap();
		let (_, applied, _) = Mat4::from_cols_array(&scene.nodes[1].transform).to_scale_rotation_translation();
		let humanoid_child_axis_after_rotation = applied * Vec3::Y;
		assert!(
			(humanoid_child_axis_after_rotation - Vec3::Z).length() < 1e-5,
			".unavatar shoulder adapter must use the Humanoid successor child axis, not the first decoration child; got {:?}",
			humanoid_child_axis_after_rotation
		);
	}

	#[test]
	fn unmotion_left_upper_arm_matches_vrm0_vrm1_and_equivalent_unavatar_world_axis() {
		let source_rotation = Quat::from_rotation_arc(-Vec3::X, Vec3::Z);

		let vrm0_rest_nodes = vec![
			UnaSceneNode {
				transform: Mat4::from_rotation_y(std::f32::consts::PI).to_cols_array(),
				children: vec![1],
				..unknown_node()
			},
			UnaSceneNode {
				transform: Mat4::from_translation(-Vec3::X).to_cols_array(),
				children: vec![2],
				..unknown_node()
			},
			UnaSceneNode {
				transform: Mat4::from_translation(-Vec3::X).to_cols_array(),
				..unknown_node()
			},
		];
		let mut vrm0_document = UnaDocument {
			scene: Some(UnaSceneSnapshot {
				nodes: vrm0_rest_nodes.clone(),
				roots: vec![0],
				..Default::default()
			}),
			humanoid_profile: Some(HumanoidProfile {
				bone_node_indices: [("leftupperarm".to_string(), 1)].into_iter().collect(),
			}),
			..Default::default()
		};
		vrm0_document.vrm = Some(un_avatar_core::UnaVrmExtension {
			spec_version: "0.0".to_string(),
			meta: serde_json::Value::Null,
			humanoid_bones: Default::default(),
			mtoon_materials_v0: vec![],
			mtoon_material_indices_v1: vec![],
			source: serde_json::Value::Null,
		});
		let vrm0_scene = apply_left_upper_arm_sample(vrm0_document, vrm0_rest_nodes, source_rotation);
		let vrm0_axis = normalized_world_bone_axis(&vrm0_scene, 1, 2);

		let vrm1_rest_nodes = vec![
			UnaSceneNode {
				transform: Mat4::IDENTITY.to_cols_array(),
				children: vec![1],
				..unknown_node()
			},
			UnaSceneNode {
				transform: Mat4::from_translation(Vec3::X).to_cols_array(),
				children: vec![2],
				..unknown_node()
			},
			UnaSceneNode {
				transform: Mat4::from_translation(Vec3::X).to_cols_array(),
				..unknown_node()
			},
		];
		let vrm1_document = UnaDocument {
			vrm: Some(un_avatar_core::UnaVrmExtension {
				spec_version: "1.0".to_string(),
				meta: serde_json::Value::Null,
				humanoid_bones: Default::default(),
				mtoon_materials_v0: vec![],
				mtoon_material_indices_v1: vec![],
				source: serde_json::Value::Null,
			}),
			scene: Some(UnaSceneSnapshot {
				nodes: vrm1_rest_nodes.clone(),
				roots: vec![0],
				..Default::default()
			}),
			humanoid_profile: Some(HumanoidProfile {
				bone_node_indices: [("leftupperarm".to_string(), 1)].into_iter().collect(),
			}),
			..Default::default()
		};
		let vrm1_scene = apply_left_upper_arm_sample(vrm1_document, vrm1_rest_nodes, source_rotation);
		let vrm1_axis = normalized_world_bone_axis(&vrm1_scene, 1, 2);

		let unavatar_rest_nodes = vec![
			UnaSceneNode {
				transform: Mat4::IDENTITY.to_cols_array(),
				children: vec![1],
				..unknown_node()
			},
			UnaSceneNode {
				transform: Mat4::from_rotation_z(-std::f32::consts::FRAC_PI_2).to_cols_array(),
				children: vec![2],
				..unknown_node()
			},
			UnaSceneNode {
				transform: Mat4::from_translation(Vec3::Y).to_cols_array(),
				children: vec![3],
				..unknown_node()
			},
			UnaSceneNode {
				transform: Mat4::from_translation(Vec3::Y).to_cols_array(),
				..unknown_node()
			},
		];
		let unavatar_document = UnaDocument {
			unavatar: Some(un_avatar_core::UnaUnavatarExtension {
				spec_version: "0.1-preview".to_string(),
				source: serde_json::Value::Null,
			}),
			scene: Some(UnaSceneSnapshot {
				nodes: unavatar_rest_nodes.clone(),
				roots: vec![0],
				..Default::default()
			}),
			humanoid_profile: Some(HumanoidProfile {
				bone_node_indices: [("leftupperarm".to_string(), 2)].into_iter().collect(),
			}),
			..Default::default()
		};
		let unavatar_scene = apply_left_upper_arm_sample(unavatar_document, unavatar_rest_nodes, source_rotation);
		let unavatar_axis = normalized_world_bone_axis(&unavatar_scene, 2, 3);

		assert!(
			(vrm0_axis - vrm1_axis).length() < 1e-5,
			"VRM0 and VRM1 should agree in world space after root/basis normalization: vrm0={:?} vrm1={:?}",
			vrm0_axis,
			vrm1_axis
		);
		assert!(
			(unavatar_axis - vrm1_axis).length() < 1e-5,
			".unavatar +Y local arm chain equivalent to VRM1 +X should produce the same world axis: unavatar={:?} vrm1={:?}",
			unavatar_axis,
			vrm1_axis
		);
		assert!((vrm1_axis - Vec3::Z).length() < 1e-5);
	}

	#[test]
	fn unavatar_unmotion_upper_arm_raise_matches_vrm1_world_axis() {
		let source_rotation = Quat::from_rotation_arc(-Vec3::X, Vec3::Y);

		let vrm1_rest_nodes = vec![
			UnaSceneNode {
				transform: Mat4::IDENTITY.to_cols_array(),
				children: vec![1],
				..unknown_node()
			},
			UnaSceneNode {
				transform: Mat4::from_translation(Vec3::X).to_cols_array(),
				children: vec![2],
				..unknown_node()
			},
			UnaSceneNode {
				transform: Mat4::from_translation(Vec3::X).to_cols_array(),
				..unknown_node()
			},
		];
		let vrm1_document = UnaDocument {
			vrm: Some(un_avatar_core::UnaVrmExtension {
				spec_version: "1.0".to_string(),
				meta: serde_json::Value::Null,
				humanoid_bones: Default::default(),
				mtoon_materials_v0: vec![],
				mtoon_material_indices_v1: vec![],
				source: serde_json::Value::Null,
			}),
			scene: Some(UnaSceneSnapshot {
				nodes: vrm1_rest_nodes.clone(),
				roots: vec![0],
				..Default::default()
			}),
			humanoid_profile: Some(HumanoidProfile {
				bone_node_indices: [("leftupperarm".to_string(), 1)].into_iter().collect(),
			}),
			..Default::default()
		};
		let vrm1_scene = apply_left_upper_arm_sample(vrm1_document, vrm1_rest_nodes, source_rotation);
		let vrm1_axis = normalized_world_bone_axis(&vrm1_scene, 1, 2);

		let unavatar_rest_nodes = vec![
			UnaSceneNode {
				transform: Mat4::IDENTITY.to_cols_array(),
				children: vec![1],
				..unknown_node()
			},
			UnaSceneNode {
				transform: Mat4::IDENTITY.to_cols_array(),
				children: vec![2],
				..unknown_node()
			},
			UnaSceneNode {
				transform: Mat4::from_translation(Vec3::Y).to_cols_array(),
				children: vec![3],
				..unknown_node()
			},
			UnaSceneNode {
				transform: Mat4::from_translation(Vec3::Y).to_cols_array(),
				..unknown_node()
			},
		];
		let unavatar_document = UnaDocument {
			unavatar: Some(un_avatar_core::UnaUnavatarExtension {
				spec_version: "0.1-preview".to_string(),
				source: serde_json::Value::Null,
			}),
			scene: Some(UnaSceneSnapshot {
				nodes: unavatar_rest_nodes.clone(),
				roots: vec![0],
				..Default::default()
			}),
			humanoid_profile: Some(HumanoidProfile {
				bone_node_indices: [("leftupperarm".to_string(), 2)].into_iter().collect(),
			}),
			..Default::default()
		};
		let unavatar_scene = apply_left_upper_arm_sample(unavatar_document, unavatar_rest_nodes, source_rotation);
		let unavatar_axis = normalized_world_bone_axis(&unavatar_scene, 2, 3);

		assert!(
			(unavatar_axis - vrm1_axis).length() < 1e-5,
			".unavatar +Y upper-arm raise must match VRM1 +X upper-arm raise: unavatar={:?} vrm1={:?}",
			unavatar_axis,
			vrm1_axis
		);
		assert!((vrm1_axis - Vec3::Y).length() < 1e-5);
	}

	#[test]
	fn unavatar_unmotion_hand_axis_uses_middle_finger_not_first_decoration_child() {
		let rest_nodes = vec![
			unknown_node(),
			UnaSceneNode {
				transform: Mat4::IDENTITY.to_cols_array(),
				children: vec![2, 3],
				..unknown_node()
			},
			UnaSceneNode {
				name: Some("coat_hand_root_L".to_string()),
				transform: Mat4::from_translation(-Vec3::Y).to_cols_array(),
				..unknown_node()
			},
			UnaSceneNode {
				name: Some("Middle Proximal_L".to_string()),
				transform: Mat4::from_translation(Vec3::Y).to_cols_array(),
				..unknown_node()
			},
		];
		let mut document = UnaDocument::default();
		document.unavatar = Some(un_avatar_core::UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::Value::Null,
		});
		document.scene = Some(un_avatar_core::UnaSceneSnapshot {
			meshes: vec![],
			materials: vec![],
			images: vec![],
			image_sources: vec![],
			skins: vec![],
			nodes: rest_nodes.clone(),
			roots: vec![0],
			node_constraints: vec![],
			asset_group_ownership: vec![],
		});
		document.humanoid_profile = Some(HumanoidProfile {
			bone_node_indices: [("lefthand".to_string(), 1), ("leftmiddleproximal".to_string(), 3)]
				.into_iter()
				.collect(),
		});
		let source_rotation = Quat::from_rotation_arc(-Vec3::X, Vec3::Z);
		let frame = unmotion_body_frame(HumanoidBone::LeftHand, source_rotation);

		apply_un_motion_frame_to_document_with_rest(&mut document, &frame, ApplyUnMotionFrameOpts::default(), Some(&rest_nodes));

		let scene = document.scene.unwrap();
		let (_, applied, _) = Mat4::from_cols_array(&scene.nodes[1].transform).to_scale_rotation_translation();
		let middle_axis_after_rotation = applied * Vec3::Y;
		assert!(
			(middle_axis_after_rotation - Vec3::Z).length() < 1e-5,
			".unavatar hand adapter must use the middle-finger Humanoid successor, not the first decoration child; got {:?}",
			middle_axis_after_rotation
		);
	}

	#[test]
	fn unavatar_unmotion_leg_axis_adapts_plus_y_chain_to_canonical_down_axis() {
		let rest_nodes = vec![
			unknown_node(),
			UnaSceneNode {
				transform: Mat4::IDENTITY.to_cols_array(),
				children: vec![2],
				..unknown_node()
			},
			UnaSceneNode {
				transform: Mat4::from_translation(Vec3::Y).to_cols_array(),
				..unknown_node()
			},
		];
		let mut document = UnaDocument::default();
		document.unavatar = Some(un_avatar_core::UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::Value::Null,
		});
		document.scene = Some(un_avatar_core::UnaSceneSnapshot {
			meshes: vec![],
			materials: vec![],
			images: vec![],
			image_sources: vec![],
			skins: vec![],
			nodes: rest_nodes.clone(),
			roots: vec![0],
			node_constraints: vec![],
			asset_group_ownership: vec![],
		});
		document.humanoid_profile = Some(HumanoidProfile {
			bone_node_indices: [("leftupperleg".to_string(), 1), ("leftlowerleg".to_string(), 2)]
				.into_iter()
				.collect(),
		});
		let frame = unmotion_body_frame(HumanoidBone::LeftUpperLeg, Quat::IDENTITY);

		apply_un_motion_frame_to_document_with_rest(&mut document, &frame, ApplyUnMotionFrameOpts::default(), Some(&rest_nodes));

		let scene = document.scene.unwrap();
		let (_, applied, _) = Mat4::from_cols_array(&scene.nodes[1].transform).to_scale_rotation_translation();
		let lower_leg_axis = applied * Vec3::Y;
		assert!(
			(lower_leg_axis + Vec3::Y).length() < 1e-5,
			".unavatar +Y leg chain must match the canonical UNMotion down axis; got {:?}",
			lower_leg_axis
		);
	}

	#[test]
	fn unavatar_unmotion_foot_axis_uses_toe_child_not_first_decoration_child() {
		let rest_nodes = vec![
			unknown_node(),
			UnaSceneNode {
				transform: Mat4::IDENTITY.to_cols_array(),
				children: vec![2, 3],
				..unknown_node()
			},
			UnaSceneNode {
				name: Some("Leg_frills_Root_L".to_string()),
				transform: Mat4::from_translation(Vec3::X).to_cols_array(),
				..unknown_node()
			},
			UnaSceneNode {
				name: Some("Toe_L".to_string()),
				transform: Mat4::from_translation(Vec3::Y).to_cols_array(),
				..unknown_node()
			},
		];
		let mut document = UnaDocument::default();
		document.unavatar = Some(un_avatar_core::UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::Value::Null,
		});
		document.scene = Some(un_avatar_core::UnaSceneSnapshot {
			meshes: vec![],
			materials: vec![],
			images: vec![],
			image_sources: vec![],
			skins: vec![],
			nodes: rest_nodes.clone(),
			roots: vec![0],
			node_constraints: vec![],
			asset_group_ownership: vec![],
		});
		document.humanoid_profile = Some(HumanoidProfile {
			bone_node_indices: [("leftfoot".to_string(), 1)].into_iter().collect(),
		});
		let frame = unmotion_body_frame(HumanoidBone::LeftFoot, Quat::IDENTITY);

		apply_un_motion_frame_to_document_with_rest(&mut document, &frame, ApplyUnMotionFrameOpts::default(), Some(&rest_nodes));

		let scene = document.scene.unwrap();
		let (_, applied, _) = Mat4::from_cols_array(&scene.nodes[1].transform).to_scale_rotation_translation();
		let toe_axis = applied * Vec3::Y;
		assert!(
			(toe_axis - Vec3::Z).length() < 1e-5,
			".unavatar foot adapter must use the toe child, not the first decoration child; got {:?}",
			toe_axis
		);
	}

	#[test]
	fn body_hand_rotation_is_not_overwritten_by_hand_wrist_when_body_owns_hand_bone() {
		let rest_nodes = vec![
			unknown_node(),
			UnaSceneNode {
				transform: Mat4::IDENTITY.to_cols_array(),
				..unknown_node()
			},
		];
		let mut document = UnaDocument::default();
		document.scene = Some(un_avatar_core::UnaSceneSnapshot {
			meshes: vec![],
			materials: vec![],
			images: vec![],
			image_sources: vec![],
			skins: vec![],
			nodes: rest_nodes.clone(),
			roots: vec![0],
			node_constraints: vec![],
			asset_group_ownership: vec![],
		});
		document.humanoid_profile = Some(HumanoidProfile {
			bone_node_indices: [("lefthand".to_string(), 1)].into_iter().collect(),
		});
		let body_hand_rotation = Quat::from_rotation_y(0.4);
		let wrist_rotation = Quat::from_rotation_y(-1.0);
		let mut frame = UNMotionFrame::new(0);
		frame.header.coordinate_space = CoordinateSpace::UNMotion;
		frame.body = Some(un_motion_frame::BodyMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			humanoid: Some(HumanoidPose {
				root: None,
				bones: vec![BoneSample {
					bone: HumanoidBone::LeftHand,
					transform: TransformSample {
						translation: None,
						rotation: Some(Quatf {
							x: body_hand_rotation.x,
							y: body_hand_rotation.y,
							z: body_hand_rotation.z,
							w: body_hand_rotation.w,
						}),
						scale: None,
						linear_velocity: None,
						angular_velocity: None,
					},
					confidence: 1.0,
					source_index: Some(0),
					state: SampleState::Valid,
				}],
			}),
		});
		frame.left_hand = Some(HandMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			wrist: Some(TransformSample {
				translation: None,
				rotation: Some(Quatf {
					x: wrist_rotation.x,
					y: wrist_rotation.y,
					z: wrist_rotation.z,
					w: wrist_rotation.w,
				}),
				scale: None,
				linear_velocity: None,
				angular_velocity: None,
			}),
			fingers: Vec::new(),
		});

		apply_un_motion_frame_to_document_with_rest(&mut document, &frame, ApplyUnMotionFrameOpts::default(), Some(&rest_nodes));

		let scene = document.scene.unwrap();
		let (_, applied, _) = Mat4::from_cols_array(&scene.nodes[1].transform).to_scale_rotation_translation();
		let expected = Quat::from_xyzw(
			-body_hand_rotation.x,
			-body_hand_rotation.y,
			body_hand_rotation.z,
			body_hand_rotation.w,
		);
		assert!(
			(applied.dot(expected).abs() - 1.0).abs() < 1e-5,
			"body-owned LeftHand rotation must not be overwritten by HandMotion.wrist"
		);
	}

	#[test]
	fn unavatar_hand_wrist_fallback_uses_body_hand_axis_adapter_per_side() {
		for (side_prefix, hand_key, middle_key, source_axis) in [
			("left", "lefthand", "leftmiddleproximal", Vec3::X),
			("right", "righthand", "rightmiddleproximal", -Vec3::X),
		] {
			let rest_nodes = vec![
				unknown_node(),
				UnaSceneNode {
					transform: Mat4::IDENTITY.to_cols_array(),
					children: vec![2],
					..unknown_node()
				},
				UnaSceneNode {
					transform: Mat4::from_translation(Vec3::Y).to_cols_array(),
					..unknown_node()
				},
			];
			let mut document = UnaDocument::default();
			document.unavatar = Some(un_avatar_core::UnaUnavatarExtension {
				spec_version: "0.1-preview".to_string(),
				source: serde_json::Value::Null,
			});
			document.scene = Some(un_avatar_core::UnaSceneSnapshot {
				meshes: vec![],
				materials: vec![],
				images: vec![],
				image_sources: vec![],
				skins: vec![],
				nodes: rest_nodes.clone(),
				roots: vec![0],
				node_constraints: vec![],
				asset_group_ownership: vec![],
			});
			document.humanoid_profile = Some(HumanoidProfile {
				bone_node_indices: [(hand_key.to_string(), 1), (middle_key.to_string(), 2)].into_iter().collect(),
			});
			let mut frame = UNMotionFrame::new(0);
			frame.header.coordinate_space = CoordinateSpace::UNMotion;
			let hand = HandMotion {
				tracking_state: TrackingState::Valid,
				confidence: 1.0,
				wrist: Some(TransformSample {
					translation: None,
					rotation: Some(quatf(Quat::IDENTITY)),
					scale: None,
					linear_velocity: None,
					angular_velocity: None,
				}),
				fingers: Vec::new(),
			};
			if side_prefix == "left" {
				frame.left_hand = Some(hand);
			} else {
				frame.right_hand = Some(hand);
			}

			apply_un_motion_frame_to_document_with_rest(&mut document, &frame, ApplyUnMotionFrameOpts::default(), Some(&rest_nodes));

			let scene = document.scene.unwrap();
			let (_, applied, _) = Mat4::from_cols_array(&scene.nodes[1].transform).to_scale_rotation_translation();
			let middle_axis = applied * Vec3::Y;
			assert!(
				(middle_axis - source_axis).length() < 1e-5,
				".unavatar HandMotion.wrist fallback must use the {side_prefix} hand axis adapter; got {:?}",
				middle_axis
			);
		}
	}

	#[test]
	fn unavatar_unmotion_hand_fingers_adapt_plus_y_chain_to_canonical_finger_axis() {
		let rest_nodes = vec![
			unknown_node(),
			UnaSceneNode {
				transform: Mat4::IDENTITY.to_cols_array(),
				children: vec![2],
				..unknown_node()
			},
			UnaSceneNode {
				transform: Mat4::from_translation(Vec3::Y).to_cols_array(),
				..unknown_node()
			},
		];
		let mut document = UnaDocument::default();
		document.unavatar = Some(un_avatar_core::UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::Value::Null,
		});
		document.scene = Some(un_avatar_core::UnaSceneSnapshot {
			meshes: vec![],
			materials: vec![],
			images: vec![],
			image_sources: vec![],
			skins: vec![],
			nodes: rest_nodes.clone(),
			roots: vec![0],
			node_constraints: vec![],
			asset_group_ownership: vec![],
		});
		document.humanoid_profile = Some(HumanoidProfile {
			bone_node_indices: [("rightindexintermediate".to_string(), 1), ("rightindexdistal".to_string(), 2)]
				.into_iter()
				.collect(),
		});
		let source_rotation = Quat::from_rotation_z(-0.5);
		let mut frame = UNMotionFrame::new(0);
		frame.header.coordinate_space = CoordinateSpace::UNMotion;
		frame.right_hand = Some(HandMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			wrist: None,
			fingers: vec![FingerPose {
				finger: Finger::Index,
				joints: vec![
					TransformSample {
						translation: None,
						rotation: Some(Quatf {
							x: 0.0,
							y: 0.0,
							z: 0.0,
							w: 1.0,
						}),
						scale: None,
						linear_velocity: None,
						angular_velocity: None,
					},
					TransformSample {
						translation: None,
						rotation: Some(Quatf {
							x: source_rotation.x,
							y: source_rotation.y,
							z: source_rotation.z,
							w: source_rotation.w,
						}),
						scale: None,
						linear_velocity: None,
						angular_velocity: None,
					},
				],
				confidence: 1.0,
			}],
		});

		apply_un_motion_frame_to_document_with_rest(&mut document, &frame, ApplyUnMotionFrameOpts::default(), Some(&rest_nodes));

		let scene = document.scene.unwrap();
		let (_, applied, _) = Mat4::from_cols_array(&scene.nodes[1].transform).to_scale_rotation_translation();
		let axis_after_curl = applied * Vec3::Y;
		let expected_axis = Quat::from_rotation_z(0.5) * -Vec3::X;
		assert!(
			(axis_after_curl - expected_axis).length() < 1e-5,
			".unavatar +Y finger chain must match the VRM0/1.0.0 right-finger curl axis; got {:?}, expected {:?}",
			axis_after_curl,
			expected_axis
		);
	}

	#[test]
	fn unavatar_thumb_proximal_applies_live_unmotion_relative_curl() {
		let rest_rotation = Quat::from_rotation_z(-0.5) * Quat::from_rotation_x(0.25);
		let rest_nodes = vec![
			unknown_node(),
			UnaSceneNode {
				transform: Mat4::from_quat(rest_rotation).to_cols_array(),
				children: vec![2],
				..unknown_node()
			},
			UnaSceneNode {
				transform: Mat4::from_translation(Vec3::Y).to_cols_array(),
				..unknown_node()
			},
		];
		let mut document = UnaDocument::default();
		document.unavatar = Some(un_avatar_core::UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::Value::Null,
		});
		document.scene = Some(un_avatar_core::UnaSceneSnapshot {
			meshes: vec![],
			materials: vec![],
			images: vec![],
			image_sources: vec![],
			skins: vec![],
			nodes: rest_nodes.clone(),
			roots: vec![0],
			node_constraints: vec![],
			asset_group_ownership: vec![],
		});
		document.humanoid_profile = Some(HumanoidProfile {
			bone_node_indices: [("leftthumbproximal".to_string(), 1), ("leftthumbintermediate".to_string(), 2)]
				.into_iter()
				.collect(),
		});
		let apply_thumb_rotation = |source_rotation: Quat| {
			let mut document = document.clone();
			let mut frame = UNMotionFrame::new(0);
			frame.header.coordinate_space = CoordinateSpace::UNMotion;
			frame.left_hand = Some(HandMotion {
				tracking_state: TrackingState::Valid,
				confidence: 1.0,
				wrist: None,
				fingers: vec![FingerPose {
					finger: Finger::Thumb,
					joints: vec![TransformSample {
						translation: None,
						rotation: Some(quatf(source_rotation)),
						scale: None,
						linear_velocity: None,
						angular_velocity: None,
					}],
					confidence: 1.0,
				}],
			});
			apply_un_motion_frame_to_document_with_rest(&mut document, &frame, ApplyUnMotionFrameOpts::default(), Some(&rest_nodes));
			let scene = document.scene.unwrap();
			let (_, applied, _) = Mat4::from_cols_array(&scene.nodes[1].transform).to_scale_rotation_translation();
			applied * Vec3::Y
		};
		let apply_thumb_y_curl = |curl: f32| apply_thumb_rotation(Quat::from_rotation_y(curl));

		let neutral_axis = apply_thumb_y_curl(0.0);
		let rest_axis = rest_rotation * Vec3::Y;
		assert!(
			(neutral_axis - rest_axis).length() < 1e-5,
			".unavatar thumb proximal must keep Unity rest pose when live UNMotion sends identity; got {neutral_axis:?}"
		);
		let expected_axis = Quat::from_rotation_y(0.5) * rest_axis;
		let actual_axis = apply_thumb_y_curl(0.5);
		assert!(actual_axis.angle_between(expected_axis) < 1e-4);

		let z_curl_axis = apply_thumb_rotation(Quat::from_euler(EulerRot::XYZ, 0.0, 0.0, 0.5));
		assert!(
			z_curl_axis.y < neutral_axis.y,
			".unavatar thumb proximal Z curl must follow VRM0/1.0.0 curl direction instead of opening outward; neutral={neutral_axis:?} curled={z_curl_axis:?}"
		);
	}

	#[test]
	fn unavatar_thumb_intermediate_keeps_live_unmotion_y_sign() {
		let rest_rotation = Quat::from_rotation_z(-0.35) * Quat::from_rotation_x(0.2);
		let rest_nodes = vec![
			unknown_node(),
			UnaSceneNode {
				transform: Mat4::IDENTITY.to_cols_array(),
				children: vec![2],
				..unknown_node()
			},
			UnaSceneNode {
				transform: Mat4::from_scale_rotation_translation(Vec3::ONE, rest_rotation, Vec3::Y).to_cols_array(),
				children: vec![3],
				..unknown_node()
			},
			UnaSceneNode {
				transform: Mat4::from_translation(Vec3::Y).to_cols_array(),
				..unknown_node()
			},
		];
		let mut document = UnaDocument::default();
		document.unavatar = Some(un_avatar_core::UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::Value::Null,
		});
		document.scene = Some(un_avatar_core::UnaSceneSnapshot {
			meshes: vec![],
			materials: vec![],
			images: vec![],
			image_sources: vec![],
			skins: vec![],
			nodes: rest_nodes.clone(),
			roots: vec![0],
			node_constraints: vec![],
			asset_group_ownership: vec![],
		});
		document.humanoid_profile = Some(HumanoidProfile {
			bone_node_indices: [
				("leftthumbproximal".to_string(), 1),
				("leftthumbintermediate".to_string(), 2),
				("leftthumbdistal".to_string(), 3),
			]
			.into_iter()
			.collect(),
		});
		let source_rotation = Quat::from_rotation_y(0.5);
		let mut frame = UNMotionFrame::new(0);
		frame.header.coordinate_space = CoordinateSpace::UNMotion;
		frame.left_hand = Some(HandMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			wrist: None,
			fingers: vec![FingerPose {
				finger: Finger::Thumb,
				joints: vec![
					TransformSample {
						translation: None,
						rotation: Some(quatf(Quat::IDENTITY)),
						scale: None,
						linear_velocity: None,
						angular_velocity: None,
					},
					TransformSample {
						translation: None,
						rotation: Some(quatf(source_rotation)),
						scale: None,
						linear_velocity: None,
						angular_velocity: None,
					},
				],
				confidence: 1.0,
			}],
		});

		apply_un_motion_frame_to_document_with_rest(&mut document, &frame, ApplyUnMotionFrameOpts::default(), Some(&rest_nodes));

		let scene = document.scene.unwrap();
		let (_, applied, _) = Mat4::from_cols_array(&scene.nodes[2].transform).to_scale_rotation_translation();
		let neutral_axis = rest_rotation * Vec3::Y;
		let actual_axis = applied * Vec3::Y;
		let expected_axis = source_rotation * neutral_axis;
		let reversed_axis = Quat::from_rotation_y(-0.5) * neutral_axis;
		assert!(
			actual_axis.angle_between(expected_axis) < actual_axis.angle_between(reversed_axis),
			".unavatar thumb intermediate must preserve live UNMotion Y curl sign; got {actual_axis:?}, expected-sign {expected_axis:?}, reversed-sign {reversed_axis:?}"
		);
	}

	#[test]
	fn unavatar_vmc_humanoid_uses_unity_to_gltf_basis() {
		let rest_rotation = Quat::from_rotation_y(0.25);
		let rest_transform = Mat4::from_scale_rotation_translation(Vec3::ONE, rest_rotation, Vec3::new(0.2, 1.0, 0.0));
		let rest_nodes = vec![
			unknown_node(),
			UnaSceneNode {
				transform: rest_transform.to_cols_array(),
				..unknown_node()
			},
		];
		let mut document = UnaDocument::default();
		document.unavatar = Some(un_avatar_core::UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::Value::Null,
		});
		document.scene = Some(un_avatar_core::UnaSceneSnapshot {
			meshes: vec![],
			materials: vec![],
			images: vec![],
			image_sources: vec![],
			skins: vec![],
			nodes: rest_nodes.clone(),
			roots: vec![0],
			node_constraints: vec![],
			asset_group_ownership: vec![],
		});
		document.humanoid_profile = Some(HumanoidProfile {
			bone_node_indices: [("leftlowerarm".to_string(), 1)].into_iter().collect(),
		});
		let source_rotation = Quat::from_xyzw(0.1, 0.2, 0.3, 0.9).normalize();
		let mut frame = UNMotionFrame::new(0);
		frame.header.coordinate_space = CoordinateSpace::Vmc;
		frame.body = Some(un_motion_frame::BodyMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			humanoid: Some(HumanoidPose {
				root: None,
				bones: vec![BoneSample {
					bone: HumanoidBone::LeftLowerArm,
					transform: TransformSample {
						translation: None,
						rotation: Some(Quatf {
							x: source_rotation.x,
							y: source_rotation.y,
							z: source_rotation.z,
							w: source_rotation.w,
						}),
						scale: None,
						linear_velocity: None,
						angular_velocity: None,
					},
					confidence: 1.0,
					source_index: Some(0),
					state: SampleState::Valid,
				}],
			}),
		});

		apply_un_motion_frame_to_document_with_rest(&mut document, &frame, ApplyUnMotionFrameOpts::default(), Some(&rest_nodes));

		let scene = document.scene.unwrap();
		let applied = Mat4::from_cols_array(&scene.nodes[1].transform);
		let unity_to_gltf_rotation = Quat::from_xyzw(source_rotation.x, -source_rotation.y, -source_rotation.z, source_rotation.w);
		let expected_rotation = rest_rotation * unity_to_gltf_rotation;
		let expected = Mat4::from_scale_rotation_translation(Vec3::ONE, expected_rotation, Vec3::new(0.2, 1.0, 0.0));
		assert!(
			(applied - expected).abs_diff_eq(Mat4::ZERO, 1e-5),
			".unavatar VMC bone rotation must use the Unity exporter's glTF basis conversion"
		);
	}

	#[test]
	fn face_head_transform_drives_head_bone() {
		let rest_nodes = vec![
			unknown_node(),
			UnaSceneNode {
				transform: Mat4::from_translation(Vec3::new(0.0, 1.5, 0.0)).to_cols_array(),
				..unknown_node()
			},
		];
		let mut document = UnaDocument::default();
		document.unavatar = Some(un_avatar_core::UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::Value::Null,
		});
		document.scene = Some(un_avatar_core::UnaSceneSnapshot {
			meshes: vec![],
			materials: vec![],
			images: vec![],
			image_sources: vec![],
			skins: vec![],
			nodes: rest_nodes.clone(),
			roots: vec![0],
			node_constraints: vec![],
			asset_group_ownership: vec![],
		});
		document.humanoid_profile = Some(HumanoidProfile {
			bone_node_indices: [("head".to_string(), 1)].into_iter().collect(),
		});
		let head_rotation = Quat::from_rotation_y(0.3);
		let mut frame = UNMotionFrame::new(0);
		frame.header.coordinate_space = CoordinateSpace::UNMotion;
		frame.face = Some(FaceMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			head: Some(TransformSample {
				translation: None,
				rotation: Some(Quatf {
					x: head_rotation.x,
					y: head_rotation.y,
					z: head_rotation.z,
					w: head_rotation.w,
				}),
				scale: None,
				linear_velocity: None,
				angular_velocity: None,
			}),
			expressions: vec![],
		});

		apply_un_motion_frame_to_document_with_rest(&mut document, &frame, ApplyUnMotionFrameOpts::default(), Some(&rest_nodes));

		let scene = document.scene.unwrap();
		let (_, applied, translation) = Mat4::from_cols_array(&scene.nodes[1].transform).to_scale_rotation_translation();
		let expected = Quat::from_xyzw(head_rotation.x, -head_rotation.y, -head_rotation.z, head_rotation.w);
		assert!((applied.dot(expected).abs() - 1.0).abs() < 1e-5);
		assert!((translation - Vec3::new(0.0, 1.5, 0.0)).length() < 1e-5);
	}

	#[test]
	fn vrm0_face_head_transform_uses_reference_vmc_basis() {
		let rest_nodes = vec![
			unknown_node(),
			UnaSceneNode {
				transform: Mat4::from_translation(Vec3::new(0.0, 1.5, 0.0)).to_cols_array(),
				..unknown_node()
			},
		];
		let mut document = UnaDocument {
			scene: Some(un_avatar_core::UnaSceneSnapshot {
				meshes: vec![],
				materials: vec![],
				images: vec![],
				image_sources: vec![],
				skins: vec![],
				nodes: rest_nodes.clone(),
				roots: vec![0],
				node_constraints: vec![],
				asset_group_ownership: vec![],
			}),
			humanoid_profile: Some(HumanoidProfile {
				bone_node_indices: [("head".to_string(), 1)].into_iter().collect(),
			}),
			..Default::default()
		};
		let head_rotation = Quat::from_xyzw(0.1, 0.2, 0.3, 0.9).normalize();
		let mut frame = UNMotionFrame::new(0);
		frame.header.coordinate_space = CoordinateSpace::UNMotion;
		frame.face = Some(FaceMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			head: Some(TransformSample {
				translation: Some(Vec3f { x: 1.0, y: 2.0, z: 3.0 }),
				rotation: Some(Quatf {
					x: head_rotation.x,
					y: head_rotation.y,
					z: head_rotation.z,
					w: head_rotation.w,
				}),
				scale: None,
				linear_velocity: None,
				angular_velocity: None,
			}),
			expressions: vec![],
		});

		apply_un_motion_frame_to_document_with_rest(&mut document, &frame, ApplyUnMotionFrameOpts::default(), Some(&rest_nodes));

		let scene = document.scene.unwrap();
		let (_, applied, translation) = Mat4::from_cols_array(&scene.nodes[1].transform).to_scale_rotation_translation();
		let expected = Quat::from_xyzw(-head_rotation.x, -head_rotation.y, head_rotation.z, head_rotation.w);
		assert!((applied.dot(expected).abs() - 1.0).abs() < 1e-5);
		assert!((translation - Vec3::new(1.0, 3.5, -3.0)).length() < 1e-5);
	}

	#[test]
	fn node_rotation_constraint_transfers_source_delta() {
		let rest_nodes = vec![
			UnaSceneNode {
				transform: Mat4::IDENTITY.to_cols_array(),
				children: vec![1, 2],
				..unknown_node()
			},
			UnaSceneNode {
				transform: Mat4::IDENTITY.to_cols_array(),
				..unknown_node()
			},
			UnaSceneNode {
				transform: Mat4::from_translation(Vec3::X).to_cols_array(),
				..unknown_node()
			},
		];
		let mut nodes = rest_nodes.clone();
		nodes[1].transform = Mat4::from_rotation_z(0.5).to_cols_array();
		let constraints = vec![un_avatar_core::UnaNodeConstraint {
			target_node: 2,
			source_node: 1,
			weight: 1.0,
			kind: un_avatar_core::UnaNodeConstraintKind::Rotation,
		}];

		apply_node_constraints_to_scene(&mut nodes, &[0], &constraints, &rest_nodes);

		let (_, applied_rotation, translation) = Mat4::from_cols_array(&nodes[2].transform).to_scale_rotation_translation();
		let expected = Quat::from_rotation_z(0.5);
		assert!(applied_rotation.abs_diff_eq(expected, 1e-5) || applied_rotation.abs_diff_eq(-expected, 1e-5));
		assert!((translation - Vec3::X).length() < 1e-5);
	}

	#[test]
	fn frame_updates_expression_weight() {
		let mut document = UnaDocument::default();
		document.scene = Some(un_avatar_core::UnaSceneSnapshot {
			meshes: vec![],
			materials: vec![],
			images: vec![],
			image_sources: vec![],
			skins: vec![],
			nodes: vec![],
			roots: vec![],
			node_constraints: vec![],
			asset_group_ownership: vec![],
		});
		document.humanoid_profile = Some(HumanoidProfile::default());
		document.expression_catalog = Some(un_avatar_core::UnaExpressionCatalog {
			presets: vec![un_avatar_core::UnaExpressionPreset {
				name: "blink".to_string(),
				binds: vec![],
			}],
		});
		document.expression_weights = Some(un_avatar_core::UnaExpressionWeights::default());
		let mut frame = UNMotionFrame::new(0);
		frame.face = Some(FaceMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			head: None,
			expressions: vec![un_motion_frame::ExpressionSample {
				name: "blink".to_string(),
				value: 0.7,
				confidence: 1.0,
				source_index: None,
				state: SampleState::Valid,
			}],
		});
		apply_un_motion_frame_to_document(&mut document, &frame, ApplyUnMotionFrameOpts::default());
		let ew = document.expression_weights.as_ref().unwrap();
		assert!((ew.preset_weights.get("blink").copied().unwrap_or(0.0) - 0.7).abs() < 1e-5);
	}

	#[test]
	fn expression_preset_match_is_ascii_case_insensitive() {
		let mut document = UnaDocument::default();
		document.scene = Some(un_avatar_core::UnaSceneSnapshot {
			meshes: vec![],
			materials: vec![],
			images: vec![],
			image_sources: vec![],
			skins: vec![],
			nodes: vec![],
			roots: vec![],
			node_constraints: vec![],
			asset_group_ownership: vec![],
		});
		document.humanoid_profile = Some(HumanoidProfile::default());
		document.expression_catalog = Some(un_avatar_core::UnaExpressionCatalog {
			presets: vec![un_avatar_core::UnaExpressionPreset {
				name: "blink".to_string(),
				binds: vec![],
			}],
		});
		document.expression_weights = Some(un_avatar_core::UnaExpressionWeights::default());
		let mut frame = UNMotionFrame::new(0);
		frame.face = Some(FaceMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			head: None,
			expressions: vec![un_motion_frame::ExpressionSample {
				name: "BLINK".to_string(),
				value: 0.4,
				confidence: 1.0,
				source_index: None,
				state: SampleState::Valid,
			}],
		});
		apply_un_motion_frame_to_document(&mut document, &frame, ApplyUnMotionFrameOpts::default());
		let ew = document.expression_weights.as_ref().unwrap();
		assert!((ew.preset_weights.get("blink").copied().unwrap_or(0.0) - 0.4).abs() < 1e-5);
	}

	#[test]
	fn expression_match_normalizes_perfect_sync_separator_and_case() {
		// VRM0 PerfectSync 用 BlendShape は PascalCase（"MouthSmileLeft"）で登録される一方、
		// Waidayo の Sub Send Motion は camelCase（"mouthSmileLeft"）、UNMotion 等は snake_case を
		// 送ってくる可能性もある。これらが同じ preset にマッチすることを保証する。
		let mut document = UnaDocument::default();
		document.scene = Some(un_avatar_core::UnaSceneSnapshot {
			meshes: vec![],
			materials: vec![],
			images: vec![],
			image_sources: vec![],
			skins: vec![],
			nodes: vec![],
			roots: vec![],
			node_constraints: vec![],
			asset_group_ownership: vec![],
		});
		document.humanoid_profile = Some(HumanoidProfile::default());
		document.expression_catalog = Some(un_avatar_core::UnaExpressionCatalog {
			presets: vec![un_avatar_core::UnaExpressionPreset {
				name: "MouthSmileLeft".to_string(),
				binds: vec![],
			}],
		});
		document.expression_weights = Some(un_avatar_core::UnaExpressionWeights::default());
		for incoming in ["mouthSmileLeft", "MouthSmileLeft", "Mouth_Smile_Left", "MOUTH-SMILE-LEFT"] {
			let mut frame = UNMotionFrame::new(0);
			frame.face = Some(FaceMotion {
				tracking_state: TrackingState::Valid,
				confidence: 1.0,
				head: None,
				expressions: vec![un_motion_frame::ExpressionSample {
					name: incoming.to_string(),
					value: 0.5,
					confidence: 1.0,
					source_index: None,
					state: SampleState::Valid,
				}],
			});
			document.expression_weights = Some(un_avatar_core::UnaExpressionWeights::default());
			apply_un_motion_frame_to_document(&mut document, &frame, ApplyUnMotionFrameOpts::default());
			let ew = document.expression_weights.as_ref().unwrap();
			assert!(
				(ew.preset_weights.get("MouthSmileLeft").copied().unwrap_or(0.0) - 0.5).abs() < 1e-5,
				"incoming={incoming} should map to preset 'MouthSmileLeft'",
			);
		}
	}

	#[test]
	fn expression_normalized_lookup_keeps_first_matching_preset() {
		let mut document = UnaDocument::default();
		document.scene = Some(un_avatar_core::UnaSceneSnapshot {
			meshes: vec![],
			materials: vec![],
			images: vec![],
			image_sources: vec![],
			skins: vec![],
			nodes: vec![],
			roots: vec![],
			node_constraints: vec![],
			asset_group_ownership: vec![],
		});
		document.humanoid_profile = Some(HumanoidProfile::default());
		document.expression_catalog = Some(un_avatar_core::UnaExpressionCatalog {
			presets: ["MouthSmileLeft", "mouth_smile_left"]
				.into_iter()
				.map(|name| un_avatar_core::UnaExpressionPreset {
					name: name.to_string(),
					binds: vec![],
				})
				.collect(),
		});
		document.expression_weights = Some(un_avatar_core::UnaExpressionWeights::default());
		let mut frame = UNMotionFrame::new(0);
		frame.face = Some(FaceMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			head: None,
			expressions: vec![un_motion_frame::ExpressionSample {
				name: "MOUTH-SMILE-LEFT".to_string(),
				value: 0.6,
				confidence: 1.0,
				source_index: None,
				state: SampleState::Valid,
			}],
		});

		apply_un_motion_frame_to_document(&mut document, &frame, ApplyUnMotionFrameOpts::default());

		let ew = document.expression_weights.as_ref().unwrap();
		assert!((ew.preset_weights.get("MouthSmileLeft").copied().unwrap_or(0.0) - 0.6).abs() < 1e-5);
		assert_eq!(ew.preset_weights.get("mouth_smile_left").copied().unwrap_or(0.0), 0.0);
	}

	#[test]
	fn perfect_sync_input_does_not_drive_basic_vrm_expressions_when_preset_is_missing() {
		let mut document = UnaDocument::default();
		document.scene = Some(un_avatar_core::UnaSceneSnapshot {
			meshes: vec![],
			materials: vec![],
			images: vec![],
			image_sources: vec![],
			skins: vec![],
			nodes: vec![],
			roots: vec![],
			node_constraints: vec![],
			asset_group_ownership: vec![],
		});
		document.humanoid_profile = Some(HumanoidProfile::default());
		document.expression_catalog = Some(un_avatar_core::UnaExpressionCatalog {
			presets: ["a", "u", "blink_l", "blink_r", "joy", "angry", "sorrow", "Surprised"]
				.into_iter()
				.map(|name| un_avatar_core::UnaExpressionPreset {
					name: name.to_string(),
					binds: vec![],
				})
				.collect(),
		});
		document.expression_weights = Some(un_avatar_core::UnaExpressionWeights::default());
		let mut frame = UNMotionFrame::new(0);
		frame.face = Some(FaceMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			head: None,
			expressions: vec![
				un_motion_frame::ExpressionSample {
					name: "jawOpen".to_string(),
					value: 0.6,
					confidence: 1.0,
					source_index: None,
					state: SampleState::Valid,
				},
				un_motion_frame::ExpressionSample {
					name: "mouthPucker".to_string(),
					value: 0.4,
					confidence: 1.0,
					source_index: None,
					state: SampleState::Valid,
				},
				un_motion_frame::ExpressionSample {
					name: "eyeBlinkLeft".to_string(),
					value: 0.7,
					confidence: 1.0,
					source_index: None,
					state: SampleState::Valid,
				},
				un_motion_frame::ExpressionSample {
					name: "mouthSmileRight".to_string(),
					value: 0.8,
					confidence: 1.0,
					source_index: None,
					state: SampleState::Valid,
				},
				un_motion_frame::ExpressionSample {
					name: "browDownLeft".to_string(),
					value: 0.5,
					confidence: 1.0,
					source_index: None,
					state: SampleState::Valid,
				},
			],
		});
		apply_un_motion_frame_to_document(&mut document, &frame, ApplyUnMotionFrameOpts::default());
		let ew = document.expression_weights.as_ref().unwrap();
		assert_eq!(ew.preset_weights.get("a").copied().unwrap_or(0.0), 0.0);
		assert_eq!(ew.preset_weights.get("u").copied().unwrap_or(0.0), 0.0);
		assert_eq!(ew.preset_weights.get("blink_l").copied().unwrap_or(0.0), 0.0);
		assert_eq!(ew.preset_weights.get("joy").copied().unwrap_or(0.0), 0.0);
		assert_eq!(ew.preset_weights.get("angry").copied().unwrap_or(0.0), 0.0);

		let mut frame = UNMotionFrame::new(1);
		frame.face = Some(FaceMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			head: None,
			expressions: vec![un_motion_frame::ExpressionSample {
				name: "jawOpen".to_string(),
				value: 0.2,
				confidence: 1.0,
				source_index: None,
				state: SampleState::Valid,
			}],
		});
		apply_un_motion_frame_to_document(&mut document, &frame, ApplyUnMotionFrameOpts::default());
		let ew = document.expression_weights.as_ref().unwrap();
		assert_eq!(ew.preset_weights.get("a").copied().unwrap_or(0.0), 0.0);
		assert_eq!(ew.preset_weights.get("joy").copied().unwrap_or(0.0), 0.0);

		let mut frame = UNMotionFrame::new(2);
		frame.face = Some(FaceMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.0,
			head: None,
			expressions: vec![
				un_motion_frame::ExpressionSample {
					name: "jawOpen".to_string(),
					value: 1.0,
					confidence: 1.0,
					source_index: None,
					state: SampleState::Valid,
				},
				un_motion_frame::ExpressionSample {
					name: "eyeBlinkLeft".to_string(),
					value: 1.0,
					confidence: 1.0,
					source_index: None,
					state: SampleState::Valid,
				},
			],
		});
		apply_un_motion_frame_to_document(&mut document, &frame, ApplyUnMotionFrameOpts::default());
		let ew = document.expression_weights.as_ref().unwrap();
		assert_eq!(ew.preset_weights.get("a").copied().unwrap_or(0.0), 0.0);
		assert_eq!(ew.preset_weights.get("blink_l").copied().unwrap_or(0.0), 0.0);
	}

	#[test]
	fn normalize_expression_match_key_strips_non_alpha() {
		assert_eq!(normalize_expression_match_key("MouthSmileLeft"), "mouthsmileleft");
		assert_eq!(normalize_expression_match_key("mouthSmileLeft"), "mouthsmileleft");
		assert_eq!(normalize_expression_match_key("Mouth_Smile_Left"), "mouthsmileleft");
		assert_eq!(normalize_expression_match_key("MOUTH-SMILE-LEFT"), "mouthsmileleft");
		assert_eq!(normalize_expression_match_key("blink"), "blink");
	}
}
