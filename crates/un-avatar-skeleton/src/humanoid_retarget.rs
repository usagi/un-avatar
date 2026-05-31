//! Humanoid リターゲット：`UNMotionFrame` → `UnaSceneSnapshot` ノード局所行列。

use glam::{EulerRot, Mat4, Quat, Vec3};
use un_avatar_core::{
	UnaDocument, UnaNodeConstraint, UnaNodeConstraintAimAxis, UnaNodeConstraintAxis, UnaNodeConstraintKind, UnaSceneNode, UnaSceneSnapshot,
};
use un_avatar_types::HumanoidProfile;
use un_motion_frame::{CoordinateSpace, Finger, HandMotion, HumanoidBone, HumanoidPose, SampleState, TransformSample, UNMotionFrame};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TargetHumanoidBasis {
	Vrm0,
	Vrm1,
	Native,
}

fn target_humanoid_basis(document: &UnaDocument) -> TargetHumanoidBasis {
	let Some(vrm) = document.vrm.as_ref() else {
		return TargetHumanoidBasis::Vrm0;
	};
	if vrm.spec_version.starts_with('1') {
		TargetHumanoidBasis::Vrm1
	} else {
		TargetHumanoidBasis::Vrm0
	}
}

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

fn convert_rotation_from_coordinate_space(rotation: Quat, coordinate_space: CoordinateSpace, target_basis: TargetHumanoidBasis) -> Quat {
	match coordinate_space {
		CoordinateSpace::Vmc => match target_basis {
			TargetHumanoidBasis::Vrm0 => Quat::from_xyzw(-rotation.x, -rotation.y, rotation.z, rotation.w),
			TargetHumanoidBasis::Vrm1 => Quat::from_xyzw(rotation.x, -rotation.y, -rotation.z, rotation.w),
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
			TargetHumanoidBasis::Vrm1 => Vec3::new(-translation.x, translation.y, translation.z),
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

fn transform_humanoid_sample_rotation(t: &TransformSample, coordinate_space: CoordinateSpace, target_basis: TargetHumanoidBasis) -> Quat {
	if coordinate_space == CoordinateSpace::UNMotion {
		let mut rotation = t
			.rotation
			.as_ref()
			.map(|quat| Quat::from_xyzw(quat.x, quat.y, quat.z, quat.w))
			.unwrap_or(Quat::IDENTITY);
		rotation = convert_rotation_from_coordinate_space(rotation, CoordinateSpace::Vmc, target_basis);
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
		return convert_translation_from_coordinate_space(translation, CoordinateSpace::Vmc, target_basis);
	}
	transform_sample_translation(t, coordinate_space, target_basis)
}

fn apply_transform_to_profile_node(
	profile: &HumanoidProfile,
	nodes: &mut [UnaSceneNode],
	rest_nodes: Option<&[UnaSceneNode]>,
	key: &str,
	transform: &TransformSample,
	coordinate_space: CoordinateSpace,
	target_basis: TargetHumanoidBasis,
) {
	let Some(ni) = profile_node_index(profile, key) else {
		return;
	};
	if let Some(node) = nodes.get_mut(ni) {
		let base_node = rest_nodes.and_then(|rest| rest.get(ni)).unwrap_or(node);
		let (base_scale, base_rotation, base_translation) = node_scale_rotation_translation(base_node);
		let sample_rotation = transform_sample_rotation(transform, coordinate_space, target_basis);
		node.transform =
			Mat4::from_scale_rotation_translation(base_scale, base_rotation * sample_rotation, base_translation).to_cols_array();
	}
}

fn profile_node_index(profile: &HumanoidProfile, key: &str) -> Option<usize> {
	profile.bone_node_indices.get(key).copied().or_else(|| {
		let target = normalize_profile_match_key(key);
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
	side_prefix: &str,
	rest_nodes: Option<&[UnaSceneNode]>,
	coordinate_space: CoordinateSpace,
	target_basis: TargetHumanoidBasis,
) {
	if hand.tracking_state == un_motion_frame::TrackingState::Lost {
		return;
	}
	if let Some(wrist) = hand.wrist.as_ref() {
		apply_transform_to_profile_node(
			profile,
			nodes,
			rest_nodes,
			&format!("{side_prefix}hand"),
			wrist,
			coordinate_space,
			target_basis,
		);
	}
	for finger in &hand.fingers {
		let finger_key = match finger.finger {
			Finger::Thumb => "thumb",
			Finger::Index => "index",
			Finger::Middle => "middle",
			Finger::Ring => "ring",
			Finger::Little => "little",
		};
		for (index, joint) in finger.joints.iter().enumerate() {
			let segment = match index {
				0 => "proximal",
				1 => "intermediate",
				2 => "distal",
				_ => continue,
			};
			apply_transform_to_profile_node(
				profile,
				nodes,
				rest_nodes,
				&format!("{side_prefix}{finger_key}{segment}"),
				joint,
				coordinate_space,
				target_basis,
			);
		}
	}
}

fn node_scale_rotation_translation(node: &UnaSceneNode) -> (Vec3, Quat, Vec3) {
	let (scale, rotation, translation) = Mat4::from_cols_array(&node.transform).to_scale_rotation_translation();
	(scale, rotation, translation)
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
	for &root in roots {
		visit(nodes, root, Mat4::IDENTITY, &mut world);
	}
	world
}

fn scene_parent_indices(nodes: &[UnaSceneNode]) -> Vec<Option<usize>> {
	let mut parents = vec![None; nodes.len()];
	for (parent, node) in nodes.iter().enumerate() {
		for &child in &node.children {
			if child < parents.len() {
				parents[child] = Some(parent);
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
	parents: &[Option<usize>],
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
		.and_then(|parent| parent.map(|p| world[p]))
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
				let parents = parents.get_or_insert_with(|| scene_parent_indices(nodes));
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
		coordinate_space,
		target_basis,
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
	coordinate_space: CoordinateSpace,
	target_basis: TargetHumanoidBasis,
	eye_clamp_deg: Option<f32>,
	apply_root_translation: bool,
) {
	if let (Some(ref root_t), Some(&ri)) = (&pose.root, roots.first()) {
		if let Some(node) = nodes.get_mut(ri) {
			if let Some(base_node) = rest_nodes.and_then(|rest| rest.get(ri)) {
				let (base_scale, base_rotation, base_translation) = node_scale_rotation_translation(base_node);
				let sample_rotation = transform_humanoid_sample_rotation(root_t, coordinate_space, target_basis);
				// translation は opt-in 時のみ rest に加算する。OFF 時は rest pose の base_translation を温存。
				let translation = if apply_root_translation {
					base_translation + transform_humanoid_sample_translation(root_t, coordinate_space, target_basis)
				} else {
					base_translation
				};
				node.transform =
					Mat4::from_scale_rotation_translation(base_scale, base_rotation * sample_rotation, translation).to_cols_array();
			} else if apply_root_translation {
				node.transform = Mat4::from_rotation_translation(
					transform_humanoid_sample_rotation(root_t, coordinate_space, target_basis),
					transform_humanoid_sample_translation(root_t, coordinate_space, target_basis),
				)
				.to_cols_array();
			} else {
				// rest_nodes が無く apply_root_translation OFF のときは、rotation のみ書き戻し translation は既存値を温存。
				let local = Mat4::from_cols_array(&node.transform);
				let (base_scale, _base_rot, base_translation) = local.to_scale_rotation_translation();
				let sample_rotation = transform_humanoid_sample_rotation(root_t, coordinate_space, target_basis);
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
		let key = humanoid_bone_profile_key(sample.bone);
		let Some(&ni) = profile.bone_node_indices.get(key) else {
			continue;
		};
		if let Some(node) = nodes.get_mut(ni) {
			let base_node = rest_nodes.and_then(|rest| rest.get(ni)).unwrap_or(node);
			let (base_scale, base_rotation, base_translation) = node_scale_rotation_translation(base_node);
			let mut sample_rotation = transform_humanoid_sample_rotation(&sample.transform, coordinate_space, target_basis);
			if let Some(deg) = eye_clamp_deg {
				if matches!(sample.bone, HumanoidBone::LeftEye | HumanoidBone::RightEye) {
					sample_rotation = clamp_eye_rotation(sample_rotation, deg);
				}
			}
			node.transform =
				Mat4::from_scale_rotation_translation(base_scale, base_rotation * sample_rotation, base_translation).to_cols_array();
		}
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
	let target_basis = target_humanoid_basis(document);
	let Some(ref mut scene) = document.scene else {
		return;
	};
	let Some(ref profile) = document.humanoid_profile else {
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
				frame.header.coordinate_space,
				target_basis,
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
			frame.header.coordinate_space,
			target_basis,
		);
	}
	if let Some(ref hand) = frame.right_hand {
		apply_hand_motion_to_scene(
			profile,
			&mut scene.nodes,
			hand,
			"right",
			rest_nodes,
			frame.header.coordinate_space,
			target_basis,
		);
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
	if opts.apply_expressions {
		if let Some(ref face) = frame.face {
			if let Some(cat) = document.expression_catalog.as_ref() {
				let ew = document.expression_weights.get_or_insert_with(Default::default);
				for ex in &face.expressions {
					// 完全一致（ASCII case 無視）優先、見つからなければ ARKit BlendShape の表記揺れに耐性のある
					// 正規化マッチ（区切り文字除去 + 全部小文字）でリトライする。
					// 例: VMC `mouthSmileLeft` / `MouthSmileLeft` / `Mouth_Smile_Left` を同じ preset へ。
					let preset = cat
						.presets
						.iter()
						.find(|p| p.name.eq_ignore_ascii_case(ex.name.as_str()))
						.or_else(|| {
							let target = normalize_expression_match_key(&ex.name);
							cat.presets.iter().find(|p| normalize_expression_match_key(&p.name) == target)
						});
					if let Some(preset) = preset {
						let value = ex.value.clamp(0.0, 1.0);
						if let Some(weight) = ew.preset_weights.get_mut(&preset.name) {
							*weight = value;
						} else {
							ew.preset_weights.insert(preset.name.clone(), value);
						}
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
			name: None,
			visible: true,
			transform: Mat4::IDENTITY.to_cols_array(),
			children: vec![],
			mesh: None,
			skin: None,
		}
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
			skins: vec![],
			nodes: vec![unknown_node()],
			roots: vec![0],
			node_constraints: vec![],
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
		assert!(
			(head_rotation.dot(expected_head).abs() - 1.0).abs() < 1e-5,
			"expected head {:?}, got {:?}",
			expected_head,
			head_rotation
		);
		assert!(
			(finger_applied.dot(finger_rotation).abs() - 1.0).abs() < 1e-5,
			"expected finger {:?}, got {:?}",
			finger_rotation,
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
		assert!(
			(hand_applied.dot(expected_hand).abs() - 1.0).abs() < 1e-5,
			"hand motion without wrist must not reset body-owned RightHand rotation"
		);
		assert!(
			(finger_applied.dot(finger_rotation).abs() - 1.0).abs() < 1e-5,
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
			skins: vec![],
			nodes: rest_nodes.clone(),
			roots: vec![0],
			node_constraints: vec![],
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
			skins: vec![],
			nodes: rest_nodes.clone(),
			roots: vec![0],
			node_constraints: vec![],
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
			skins: vec![],
			nodes: rest_nodes.clone(),
			roots: vec![],
			node_constraints: vec![],
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
			skins: vec![],
			nodes: rest_nodes.clone(),
			roots: vec![0],
			node_constraints: vec![],
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

	/// Phase 2 で `un-motion-frame-zenoh` 経由で受信する UNMotionFrame は
	/// `coordinate_space = UNMotion` を持つ想定。UNMotion canonical basis (RH, +Y up, +Z forward) は
	/// UN Avatar 内部表現と同一なので、bone rotation も root translation も **そのまま** 適用される
	/// (VMC のような z-flip / x-flip 補正は走らない) ことを回帰テストとして固定する。
	#[test]
	fn unmotion_coordinate_space_applies_rotation_and_translation_as_native() {
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
			skins: vec![],
			nodes: rest_nodes.clone(),
			roots: vec![0],
			node_constraints: vec![],
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
		// Humanoid body/root は、VMC で正しく解釈される UNMotionFrame のボーン基底を VRM0 scene へ合わせる。
		let root_transform = Mat4::from_cols_array(&scene.nodes[0].transform);
		assert!(
			(root_transform.w_axis.truncate() - Vec3::new(1.0, 2.0, -3.0)).length() < 1e-5,
			"UNMotion humanoid root translation must use the VRM0 target basis; got {:?}",
			root_transform.w_axis.truncate()
		);
		// Body bone rotation: UNMotionFrame のボーン回転は VMC target basis と同じ変換で VRM0 rest に合成する。
		let applied = Mat4::from_cols_array(&scene.nodes[1].transform);
		let expected_source = Quat::from_xyzw(-source_rotation.x, -source_rotation.y, source_rotation.z, source_rotation.w);
		let expected_rotation = rest_rotation * expected_source;
		let expected = Mat4::from_scale_rotation_translation(Vec3::ONE, expected_rotation, Vec3::new(0.2, 1.0, 0.0));
		assert!(
			(applied - expected).abs_diff_eq(Mat4::ZERO, 1e-5),
			"UNMotion humanoid bone rotation must use the VRM0 target basis (got {:?}, expected {:?})",
			applied,
			expected
		);
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
			skins: vec![],
			nodes: vec![],
			roots: vec![],
			node_constraints: vec![],
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
			skins: vec![],
			nodes: vec![],
			roots: vec![],
			node_constraints: vec![],
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
			skins: vec![],
			nodes: vec![],
			roots: vec![],
			node_constraints: vec![],
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
	fn perfect_sync_input_does_not_drive_basic_vrm_expressions_when_preset_is_missing() {
		let mut document = UnaDocument::default();
		document.scene = Some(un_avatar_core::UnaSceneSnapshot {
			meshes: vec![],
			materials: vec![],
			images: vec![],
			skins: vec![],
			nodes: vec![],
			roots: vec![],
			node_constraints: vec![],
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
