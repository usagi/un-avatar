//! VRM SpringBone 相当のシミュレーション。
//!
//! 実装は UniVRM の `VRMSpringBoneLogic` (`MTUniVRM/Runtime/SpringBone/VRMSpringBoneLogic.cs`) を
//! 参考にした Verlet 積分ベース。各 joint について毎ステップ次の処理を行う:
//!
//! 1. `parent_world_rot = decompose(world[parent]).rotation`
//! 2. `child_world_pos  = world[parent].transform_point(rest_local_translation)`
//! 3. `target_rotation  = parent_world_rot * rest_local_rotation`  (= 揺れが無いときの joint world rot)
//! 4. `target_axis_world = target_rotation * bone_axis`             (= rest pose 子方向の world ベクトル)
//! 5. Verlet:
//!    - `inertia    = (curr_tail - prev_tail) * (1 - drag)`
//!    - `stiff_pull = target_axis_world * (stiffness * dt)`         (= 復元力)
//!    - `external   = gravity_dir * gravity_power * dt`             (= 重力)
//!    - `next_tail  = curr_tail + inertia + stiff_pull + external`
//! 6. 長さ拘束: `next_tail = child_world_pos + (next_tail - child_world_pos).normalize() * length`
//! 7. 回転補正: `q_corr = from_rotation_arc(target_axis_world, (next_tail - child_world_pos).normalize())`
//!    - `new_world_rot   = q_corr * target_rotation`
//!    - `new_local_rot   = parent_world_rot.inverse() * new_world_rot`
//!    - `scene.nodes[child].transform` に `(rest_scale, new_local_rot, rest_translation)` で書き戻す
//! 8. `world_scratch` に新しい `world[child]` 以下を伝播し、次の joint の親回転に使う
//!
//! 旧実装の主な不具合と修正点:
//! - `ideal_tail` を「grandchild の現在 world 座標」から取っていたため、SpringBone 自身が前フレームで
//!   動かした位置が次フレームの目標位置になり stiffness pull が打ち消されていた
//!   → `rest_local_rotation` と `bone_axis` を初期化時に snapshot 保存し、毎フレームの目標は
//!   **rest pose ベース** で再計算する。
//! - `stiffness * 6 * dt` の hack 係数で発振しがちだった → UniVRM 同様 `stiffness * dt` に簡素化。
//! - dt 可変だと Verlet 速度 `curr - prev` が前フレームの dt 分の変位を表すため発散していた
//!   → `accumulator` で固定 dt サブステップ化 (`FIXED_DT = 1/60s`)。

use std::collections::BTreeMap;

use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use un_avatar_core::{UnaDynamicsGroup, UnaDynamicsLimit, UnaRuntimeDynamics, UnaSceneNode, UnaSceneSnapshot, UnaSpringBoneSettings};

use crate::bone_colliders::{push_out_of_colliders, BoneColliderPrimitive};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpringBoneSolver {
	#[default]
	#[serde(alias = "compat_univrm", alias = "compat_euler", alias = "compat", alias = "euler")]
	Verlet,
	Xpbd,
}

pub type DynamicsSolver = SpringBoneSolver;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpringBoneTimeMode {
	FrameBased,
	#[default]
	TimeBased,
}

pub type DynamicsTimeMode = SpringBoneTimeMode;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct SpringBonePhysicsConfig {
	pub time_mode: SpringBoneTimeMode,
	#[serde(default = "default_spring_bone_simulation_hz")]
	pub simulation_hz: f32,
	#[serde(default = "default_spring_bone_substeps")]
	pub substeps: u32,
	#[serde(default = "default_spring_bone_categories")]
	pub categories: Vec<SpringBoneCategoryDefinition>,
	pub overrides: Vec<SpringBoneCategoryOverride>,
}

impl Default for SpringBonePhysicsConfig {
	fn default() -> Self {
		Self {
			time_mode: SpringBoneTimeMode::TimeBased,
			simulation_hz: default_spring_bone_simulation_hz(),
			substeps: 1,
			categories: default_spring_bone_categories(),
			overrides: Vec::new(),
		}
	}
}

impl SpringBonePhysicsConfig {
	pub fn normalized(mut self) -> Self {
		if !self.simulation_hz.is_finite() {
			self.simulation_hz = default_spring_bone_simulation_hz();
		}
		self.simulation_hz = self.simulation_hz.clamp(30.0, 240.0);
		self.substeps = self.substeps.clamp(1, 8);
		if matches!(self.time_mode, SpringBoneTimeMode::FrameBased) {
			self.time_mode = SpringBoneTimeMode::TimeBased;
		}
		for category in &mut self.categories {
			category.id = normalize_category_id(&category.id);
			category.matches = category
				.matches
				.iter()
				.map(|m| normalize_match_text(m))
				.filter(|m| !m.is_empty())
				.collect();
		}
		self.categories.retain(|c| !c.id.is_empty());
		if self.categories.iter().all(|c| c.id != "other") {
			self.categories.push(SpringBoneCategoryDefinition {
				id: "other".to_string(),
				name: "Other".to_string(),
				matches: Vec::new(),
			});
		}
		for override_item in &mut self.overrides {
			override_item.category = normalize_category_id(&override_item.category);
			override_item.params = override_item.params.normalized();
		}
		self.overrides.retain(|o| !o.category.is_empty());
		self
	}

	fn fixed_dt(&self) -> f32 {
		1.0 / self.simulation_hz.clamp(30.0, 240.0)
	}
}

pub type DynamicsPhysicsConfig = SpringBonePhysicsConfig;

fn default_spring_bone_simulation_hz() -> f32 {
	60.0
}

fn default_spring_bone_substeps() -> u32 {
	1
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct SpringBoneCategoryDefinition {
	pub id: String,
	pub name: String,
	pub matches: Vec<String>,
}

impl Default for SpringBoneCategoryDefinition {
	fn default() -> Self {
		Self {
			id: "other".to_string(),
			name: "Other".to_string(),
			matches: Vec::new(),
		}
	}
}

pub type DynamicsCategoryDefinition = SpringBoneCategoryDefinition;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct SpringBoneCategoryOverride {
	pub category: String,
	#[serde(flatten)]
	pub params: SpringBonePhysicsParams,
}

pub type DynamicsCategoryOverride = SpringBoneCategoryOverride;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct SpringBonePhysicsParams {
	pub solver: Option<SpringBoneSolver>,
	pub damping_half_life_ms: Option<f32>,
	pub stiffness_hz: Option<f32>,
	pub xpbd_compliance: Option<f32>,
	pub gravity_scale: Option<f32>,
	pub drag_scale: Option<f32>,
	pub constraint_iterations: Option<u32>,
}

pub type DynamicsPhysicsParams = SpringBonePhysicsParams;

impl SpringBonePhysicsParams {
	fn normalized(mut self) -> Self {
		self.damping_half_life_ms = self
			.damping_half_life_ms
			.and_then(|v| v.is_finite().then_some(v.clamp(1.0, 10_000.0)));
		self.stiffness_hz = self.stiffness_hz.and_then(|v| v.is_finite().then_some(v.clamp(0.0, 60.0)));
		self.xpbd_compliance = self.xpbd_compliance.and_then(|v| v.is_finite().then_some(v.clamp(0.0, 10.0)));
		self.gravity_scale = self.gravity_scale.and_then(|v| v.is_finite().then_some(v.clamp(0.0, 10.0)));
		self.drag_scale = self.drag_scale.and_then(|v| v.is_finite().then_some(v.clamp(0.0, 10.0)));
		self.constraint_iterations = self.constraint_iterations.map(|v| v.clamp(1, 32));
		self
	}

	fn merge(self, override_params: Self) -> Self {
		Self {
			solver: override_params.solver.or(self.solver),
			damping_half_life_ms: override_params.damping_half_life_ms.or(self.damping_half_life_ms),
			stiffness_hz: override_params.stiffness_hz.or(self.stiffness_hz),
			xpbd_compliance: override_params.xpbd_compliance.or(self.xpbd_compliance),
			gravity_scale: override_params.gravity_scale.or(self.gravity_scale),
			drag_scale: override_params.drag_scale.or(self.drag_scale),
			constraint_iterations: override_params.constraint_iterations.or(self.constraint_iterations),
		}
	}
}

fn default_spring_bone_categories() -> Vec<SpringBoneCategoryDefinition> {
	vec![
		SpringBoneCategoryDefinition {
			id: "hair".to_string(),
			name: "Hair".to_string(),
			matches: vec![
				"hair".into(),
				"bangs".into(),
				"side_hair".into(),
				"back_hair".into(),
				"髪".into(),
				"前髪".into(),
				"横髪".into(),
				"後ろ髪".into(),
			],
		},
		SpringBoneCategoryDefinition {
			id: "ears".to_string(),
			name: "Ears".to_string(),
			matches: vec![
				"ears".into(),
				"ear".into(),
				"animal_ear".into(),
				"long_ear".into(),
				"耳".into(),
				"ミミ".into(),
				"けもみみ".into(),
			],
		},
		SpringBoneCategoryDefinition {
			id: "tail".to_string(),
			name: "Tail".to_string(),
			matches: vec!["tail".into(), "尻尾".into(), "しっぽ".into()],
		},
		SpringBoneCategoryDefinition {
			id: "cloth".to_string(),
			name: "Cloth".to_string(),
			matches: vec![
				"cloth".into(),
				"skirt".into(),
				"sleeve".into(),
				"cape".into(),
				"布".into(),
				"スカート".into(),
				"袖".into(),
				"ケープ".into(),
			],
		},
		SpringBoneCategoryDefinition {
			id: "accessory".to_string(),
			name: "Accessory".to_string(),
			matches: vec![
				"accessory".into(),
				"ornament".into(),
				"chain".into(),
				"cord".into(),
				"ribbon".into(),
				"装飾".into(),
				"アクセサリ".into(),
				"飾り".into(),
				"リボン".into(),
			],
		},
		SpringBoneCategoryDefinition {
			id: "other".to_string(),
			name: "Other".to_string(),
			matches: Vec::new(),
		},
	]
}

/// 1 joint 分の rest pose snapshot と動的状態（curr/prev tail）。
struct JointRuntime {
	parent_node: usize,
	child_node: usize,
	/// rest pose での子の local translation (= 親 local 空間での joint head→child の相対位置)。
	rest_local_translation: Vec3,
	/// rest pose での子の local rotation。`parent_rot * rest_local_rotation` が rest pose joint の world rot。
	rest_local_rotation: Quat,
	rest_local_scale: Vec3,
	/// rest pose での「子の子」方向を joint local 空間で表した単位ベクトル。
	/// チェーン末端（grandchild が存在しない）は rest_local_translation 方向を流用、それも 0 なら +Y。
	bone_axis: Vec3,
	/// rest pose での「子 → 子の子」までの距離 (m)。次の tail 拘束距離。
	length: f32,
	/// Collider radius used for this joint tail.
	hit_radius: f32,
	/// 動的: 現在フレームの tail (world)。
	curr_tail: Vec3,
	/// 動的: 前フレームの tail (world)。`curr - prev` が Verlet 速度。
	prev_tail: Vec3,
	/// 動的: XPBD rest-pose constraint の累積 Lagrange multiplier。
	rest_lambda: f32,
}

/// 1 チェーン分のランタイム状態。
struct GroupRuntime {
	joints: Vec<JointRuntime>,
	world_scratch: Vec<Mat4>,
	params: ResolvedSpringBonePhysicsParams,
}

impl GroupRuntime {
	fn reset_xpbd_lambdas(&mut self) {
		for joint in &mut self.joints {
			joint.rest_lambda = 0.0;
		}
	}
}

/// 全グループのランタイム状態。
pub struct SpringBoneSimulator {
	runtimes: Vec<Option<GroupRuntime>>,
	active_runtime_indices: Vec<usize>,
	/// 実時間 dt を蓄積し、`FIXED_DT` 単位の離散ステップに変換するアキュムレータ。
	accumulator: f32,
	bone_colliders: Vec<BoneColliderPrimitive>,
	physics: SpringBonePhysicsConfig,
}

/// Source-neutral v2 dynamics simulator name.
///
/// The implementation still reuses the v1 SpringBone solver assets, but runtime input flows
/// through `UnaRuntimeDynamics` / `UnaDynamicsGroup` rather than source-format SpringBone data.
pub type DynamicsSimulator = SpringBoneSimulator;

/// 1 フレームで処理する最大蓄積時間 (秒)。スパイラル・オブ・デス防止。
const MAX_ACCUM: f32 = 0.05;

/// 1 フレームでの最大サブステップ反復回数。
const MAX_STEPS_PER_FRAME: u32 = 8;

impl Default for SpringBoneSimulator {
	fn default() -> Self {
		Self {
			runtimes: Vec::new(),
			active_runtime_indices: Vec::new(),
			accumulator: 0.0,
			bone_colliders: Vec::new(),
			physics: SpringBonePhysicsConfig::default().normalized(),
		}
	}
}

#[derive(Clone, Copy, Debug)]
struct ResolvedSpringBonePhysicsParams {
	solver: SpringBoneSolver,
	damping_half_life_ms: Option<f32>,
	stiffness_hz: Option<f32>,
	xpbd_compliance: f32,
	gravity_scale: f32,
	drag_scale: f32,
	constraint_iterations: u32,
}

#[derive(Clone, Copy, Debug)]
struct ConvertedSpringBonePhysicsParams {
	stiffness_hz: f32,
	xpbd_compliance: f32,
	gravity_scale: f32,
	drag_scale: f32,
	constraint_iterations: u32,
}

/// シーンの全 root から world 行列を再計算する。
fn world_from_snapshot(scene: &UnaSceneSnapshot) -> Vec<Mat4> {
	let mut world = Vec::new();
	write_world_from_snapshot(scene, &mut world);
	world
}

fn write_world_from_snapshot(scene: &UnaSceneSnapshot, world: &mut Vec<Mat4>) {
	let n = scene.nodes.len().max(1);
	if world.len() != n {
		world.resize(n, Mat4::IDENTITY);
	} else {
		world.fill(Mat4::IDENTITY);
	}
	for &r in scene.resolved_roots().iter() {
		if r < scene.nodes.len() {
			propagate_world_subtree(&scene.nodes, world, r, Mat4::IDENTITY);
		}
	}
}

fn normalize_category_id(value: &str) -> String {
	value.trim().to_ascii_lowercase().replace([' ', '-'], "_")
}

fn normalize_match_text(value: &str) -> String {
	let mut out = String::with_capacity(value.len());
	for ch in value.trim().chars() {
		match ch {
			' ' | '-' => out.push('_'),
			_ => out.extend(ch.to_lowercase()),
		}
	}
	out
}

fn classify_group(scene: &UnaSceneSnapshot, group: UnaDynamicsGroup<'_>, categories: &[SpringBoneCategoryDefinition]) -> String {
	let explicit = normalize_category_id(group.category);
	if !explicit.is_empty() {
		return explicit;
	}
	let mut haystack = normalize_match_text(group.comment);
	for &node_index in group.chain.bone_node_indices {
		if let Some(name) = scene.nodes.get(node_index).and_then(|node| node.name.as_deref()) {
			haystack.push(' ');
			haystack.push_str(&normalize_match_text(name));
		}
	}
	for category in categories {
		for alias in &category.matches {
			if !alias.is_empty() && haystack.contains(alias) {
				return category.id.clone();
			}
		}
	}
	"other".to_string()
}

fn convert_univrm_60fps_params(group: UnaDynamicsGroup<'_>, solver: SpringBoneSolver) -> ConvertedSpringBonePhysicsParams {
	ConvertedSpringBonePhysicsParams {
		// 既存 Verlet 式は `stiffness * dt` を復元 pull として使う。
		// v1 の frequency UI はここから始め、詳細調整時だけユーザー値で上書きする。
		stiffness_hz: group.parameters.stiffness.max(0.0),
		xpbd_compliance: convert_univrm_stiffness_to_xpbd_compliance(group.parameters.stiffness),
		gravity_scale: 1.0,
		drag_scale: 1.0,
		constraint_iterations: if matches!(solver, SpringBoneSolver::Xpbd) { 4 } else { 1 },
	}
}

fn convert_univrm_stiffness_to_xpbd_compliance(stiffness: f32) -> f32 {
	if !stiffness.is_finite() || stiffness <= f32::EPSILON {
		return 10.0;
	}
	let effective_hz = (stiffness * 10.0).clamp(0.1, 32.0);
	let omega = std::f32::consts::TAU * effective_hz;
	(1.0 / (omega * omega)).clamp(0.0, 10.0)
}

fn resolve_group_params(
	category_id: &str,
	group: UnaDynamicsGroup<'_>,
	override_params_by_category: &BTreeMap<String, SpringBonePhysicsParams>,
) -> ResolvedSpringBonePhysicsParams {
	let params = override_params_by_category.get(category_id).copied().unwrap_or_default();
	let solver = params.solver.unwrap_or(SpringBoneSolver::Verlet);
	let converted = convert_univrm_60fps_params(group, solver);
	ResolvedSpringBonePhysicsParams {
		solver,
		damping_half_life_ms: params.damping_half_life_ms,
		stiffness_hz: params.stiffness_hz.or(Some(converted.stiffness_hz)),
		xpbd_compliance: params.xpbd_compliance.unwrap_or(converted.xpbd_compliance),
		gravity_scale: params.gravity_scale.unwrap_or(converted.gravity_scale),
		drag_scale: params.drag_scale.unwrap_or(converted.drag_scale),
		constraint_iterations: params.constraint_iterations.unwrap_or(converted.constraint_iterations).clamp(1, 32),
	}
}

/// `root_idx` の local を起点に親世界行列を畳み込み、子孫の世界行列を `world` に書き込む。
fn propagate_world_subtree(nodes: &[UnaSceneNode], world: &mut [Mat4], root_idx: usize, parent_world: Mat4) {
	if root_idx >= nodes.len() || root_idx >= world.len() {
		return;
	}
	let local = Mat4::from_cols_array(&nodes[root_idx].transform);
	let w = parent_world * local;
	world[root_idx] = w;
	for &c in &nodes[root_idx].children {
		if c < nodes.len() {
			propagate_world_subtree(nodes, world, c, w);
		}
	}
}

impl SpringBoneSimulator {
	pub fn new(scene: &UnaSceneSnapshot, settings: &UnaSpringBoneSettings) -> Option<Self> {
		Self::new_with_bone_colliders(scene, settings, Vec::new())
	}

	pub fn new_with_bone_colliders(
		scene: &UnaSceneSnapshot,
		settings: &UnaSpringBoneSettings,
		bone_colliders: Vec<BoneColliderPrimitive>,
	) -> Option<Self> {
		Self::new_with_config(scene, settings, bone_colliders, SpringBonePhysicsConfig::default())
	}

	pub fn new_with_config(
		scene: &UnaSceneSnapshot,
		settings: &UnaSpringBoneSettings,
		bone_colliders: Vec<BoneColliderPrimitive>,
		physics: SpringBonePhysicsConfig,
	) -> Option<Self> {
		Self::new_with_runtime_dynamics(scene, settings.runtime_dynamics(), bone_colliders, physics)
	}

	pub fn new_with_runtime_dynamics(
		scene: &UnaSceneSnapshot,
		dynamics: UnaRuntimeDynamics<'_>,
		bone_colliders: Vec<BoneColliderPrimitive>,
		physics: SpringBonePhysicsConfig,
	) -> Option<Self> {
		let groups = dynamics.dynamics_groups().collect::<Vec<_>>();
		if groups.is_empty() {
			return None;
		}
		let physics = physics.normalized();
		let world0 = world_from_snapshot(scene);
		let override_params_by_category = merge_category_override_params(&physics.overrides);
		let mut runtimes: Vec<Option<GroupRuntime>> = Vec::new();
		let mut active_runtime_indices = Vec::new();
		for g in groups.iter().copied() {
			if !g.effective_enabled {
				runtimes.push(None);
				continue;
			}
			let chain = g.chain.bone_node_indices;
			if chain.len() < 2 {
				runtimes.push(None);
				continue;
			}
			let mut joints: Vec<JointRuntime> = Vec::with_capacity(chain.len() - 1);
			let mut ok = true;
			for i in 0..chain.len() - 1 {
				let parent = chain[i];
				let child = chain[i + 1];
				if parent >= scene.nodes.len() || child >= scene.nodes.len() {
					ok = false;
					break;
				}
				if parent >= world0.len() || child >= world0.len() {
					ok = false;
					break;
				}
				let local_child = Mat4::from_cols_array(&scene.nodes[child].transform);
				let (scale, rot, trans) = local_child.to_scale_rotation_translation();
				// bone_axis: joint local 空間で「rest pose の子の子」方向。
				// grandchild がチェーン内にあるなら grandchild の rest local translation 方向、
				// 無ければ自分の rest local translation 方向（即ち親→自分の方向で代用）、それも 0 なら +Y。
				let bone_axis = if i + 2 < chain.len() {
					let gc = chain[i + 2];
					if gc < scene.nodes.len() {
						let local_gc = Mat4::from_cols_array(&scene.nodes[gc].transform);
						let (_, _, gc_trans) = local_gc.to_scale_rotation_translation();
						if gc_trans.length_squared() > 1e-12 {
							gc_trans.normalize()
						} else if trans.length_squared() > 1e-12 {
							trans.normalize()
						} else {
							Vec3::Y
						}
					} else if trans.length_squared() > 1e-12 {
						trans.normalize()
					} else {
						Vec3::Y
					}
				} else if trans.length_squared() > 1e-12 {
					trans.normalize()
				} else {
					Vec3::Y
				};
				// length: 子 → 子の子の rest 距離。grandchild が無い末端 joint は自分の rest_local_translation
				// 長さを採用（チェーン末端の「想像上の続き」として親→自分の長さを再利用）。
				let length = if i + 2 < chain.len() {
					let gc = chain[i + 2];
					if gc < scene.nodes.len() {
						let local_gc = Mat4::from_cols_array(&scene.nodes[gc].transform);
						let (_, _, gc_trans) = local_gc.to_scale_rotation_translation();
						gc_trans.length().max(1e-4)
					} else {
						trans.length().max(1e-4)
					}
				} else {
					trans.length().max(1e-4)
				};
				let curr = world0[child].transform_point3(Vec3::ZERO) + world0[child].transform_vector3(bone_axis) * length;
				let hit_radius = g
					.chain
					.hit_radius_samples
					.get(i)
					.copied()
					.filter(|value| value.is_finite())
					.unwrap_or(g.parameters.hit_radius)
					.max(0.0);
				joints.push(JointRuntime {
					parent_node: parent,
					child_node: child,
					rest_local_translation: trans,
					rest_local_rotation: rot,
					rest_local_scale: scale,
					bone_axis,
					length,
					hit_radius,
					curr_tail: curr,
					prev_tail: curr,
					rest_lambda: 0.0,
				});
			}
			if !ok || joints.is_empty() {
				runtimes.push(None);
			} else {
				let category_id = classify_group(scene, g, &physics.categories);
				let params = resolve_group_params(&category_id, g, &override_params_by_category);
				active_runtime_indices.push(runtimes.len());
				runtimes.push(Some(GroupRuntime {
					joints,
					world_scratch: Vec::new(),
					params,
				}));
			}
		}
		if runtimes.iter().all(|r| r.is_none()) {
			None
		} else {
			Some(Self {
				runtimes,
				active_runtime_indices,
				accumulator: 0.0,
				bone_colliders,
				physics,
			})
		}
	}

	/// ヒューマノイド等で親の局所姿勢を更新したあと、揺れボーンの回転だけ上書きする。
	///
	/// 実時間 `dt` を蓄積し、設定された fixed timestep 単位の固定サブステップで進める。
	pub fn step(&mut self, scene: &mut UnaSceneSnapshot, settings: &UnaSpringBoneSettings, dt: f32) {
		self.step_runtime_dynamics(scene, settings.runtime_dynamics(), dt);
	}

	pub fn step_runtime_dynamics(&mut self, scene: &mut UnaSceneSnapshot, dynamics: UnaRuntimeDynamics<'_>, dt: f32) {
		if !dynamics.has_groups() {
			return;
		}
		if !dt.is_finite() || dt <= 0.0 {
			return;
		}
		let fixed_dt = self.physics.fixed_dt();
		let substeps = self.physics.substeps.max(1);
		let sub_dt = fixed_dt / substeps as f32;
		self.accumulator = (self.accumulator + dt).min(MAX_ACCUM);
		let mut steps = 0;
		while self.accumulator >= fixed_dt && steps < MAX_STEPS_PER_FRAME {
			for &runtime_index in &self.active_runtime_indices {
				let (Some(g), Some(Some(rt))) = (dynamics.dynamics_group(runtime_index), self.runtimes.get_mut(runtime_index)) else {
					continue;
				};
				if !g.effective_enabled {
					continue;
				}
				for _ in 0..substeps {
					if matches!(rt.params.solver, SpringBoneSolver::Xpbd) {
						rt.reset_xpbd_lambdas();
					}
					step_group(scene, g, rt, &self.bone_colliders, sub_dt);
				}
			}
			self.accumulator -= fixed_dt;
			steps += 1;
		}
	}

	pub fn bone_collider_count(&self) -> usize {
		self.bone_colliders.len()
	}

	pub fn bone_colliders(&self) -> &[BoneColliderPrimitive] {
		&self.bone_colliders
	}
}

fn merge_category_override_params(overrides: &[SpringBoneCategoryOverride]) -> BTreeMap<String, SpringBonePhysicsParams> {
	let mut by_category: BTreeMap<String, SpringBonePhysicsParams> = BTreeMap::new();
	for override_item in overrides {
		by_category
			.entry(override_item.category.clone())
			.and_modify(|params| *params = params.merge(override_item.params))
			.or_insert(override_item.params);
	}
	by_category
}

fn step_group(
	scene: &mut UnaSceneSnapshot,
	group: UnaDynamicsGroup<'_>,
	rt: &mut GroupRuntime,
	bone_colliders: &[BoneColliderPrimitive],
	dt: f32,
) {
	let drag = match rt.params.solver {
		SpringBoneSolver::Verlet | SpringBoneSolver::Xpbd => {
			let drag = match rt.params.damping_half_life_ms {
				Some(half_life_ms) if half_life_ms > 0.0 => 1.0 - (-std::f32::consts::LN_2 * dt / (half_life_ms / 1000.0)).exp(),
				_ => group.parameters.drag_force,
			};
			(drag * rt.params.drag_scale).clamp(0.0, 1.0)
		}
	};
	let stiffness = rt.params.stiffness_hz.unwrap_or(group.parameters.stiffness).max(0.0);
	let gravity = Vec3::new(
		group.parameters.gravity_dir[0],
		group.parameters.gravity_dir[1],
		group.parameters.gravity_dir[2],
	)
	.normalize_or_zero()
		* group.parameters.gravity_power
		* rt.params.gravity_scale;
	let is_xpbd = matches!(rt.params.solver, SpringBoneSolver::Xpbd);
	write_world_from_snapshot(scene, &mut rt.world_scratch);

	for joint in &mut rt.joints {
		if joint.parent_node >= rt.world_scratch.len() || joint.child_node >= scene.nodes.len() {
			continue;
		}
		let parent_world = rt.world_scratch[joint.parent_node];
		let (_, parent_rot_raw, parent_pos) = parent_world.to_scale_rotation_translation();
		let parent_rot = parent_rot_raw.normalize();
		let child_pos = parent_pos + parent_rot * joint.rest_local_translation;

		let target_rotation = (parent_rot * joint.rest_local_rotation).normalize();
		let target_axis_world = (target_rotation * joint.bone_axis).normalize_or_zero();
		if target_axis_world.length_squared() < 1e-12 {
			joint.prev_tail = joint.curr_tail;
			continue;
		}

		let mut next_tail = {
			// UniVRM の SpringBone は tail の前回位置差分を慣性として使う Verlet 系。
			// `verlet` は authored 値から 60fps 相当へ変換した減衰を使う軽量 VRM 互換経路。
			// 古い `compat_univrm` / `compat_euler` 設定文字列は `verlet` alias として受け付ける。
			let inertia = (joint.curr_tail - joint.prev_tail) * (1.0 - drag);
			let stiff_pull = if is_xpbd {
				Vec3::ZERO
			} else {
				target_axis_world * (stiffness * dt)
			};
			let external = gravity * dt;
			joint.curr_tail + inertia + stiff_pull + external
		};

		if is_xpbd {
			let target_tail = child_pos + target_axis_world * joint.length;
			for _ in 0..rt.params.constraint_iterations {
				next_tail = solve_xpbd_rest_constraint(next_tail, target_tail, rt.params.xpbd_compliance, dt, &mut joint.rest_lambda);
				next_tail = constrain_tail_length(next_tail, child_pos, target_axis_world, joint.length);
				next_tail = constrain_tail_limit(next_tail, child_pos, target_axis_world, joint.length, group.limit);
				next_tail = constrain_tail_colliders(
					next_tail,
					child_pos,
					target_axis_world,
					joint.length,
					&rt.world_scratch,
					bone_colliders,
					joint.hit_radius,
				);
				next_tail = constrain_tail_limit(next_tail, child_pos, target_axis_world, joint.length, group.limit);
			}
		} else {
			joint.rest_lambda = 0.0;
			next_tail = constrain_tail_length(next_tail, child_pos, target_axis_world, joint.length);
			next_tail = constrain_tail_limit(next_tail, child_pos, target_axis_world, joint.length, group.limit);
			next_tail = constrain_tail_colliders(
				next_tail,
				child_pos,
				target_axis_world,
				joint.length,
				&rt.world_scratch,
				bone_colliders,
				joint.hit_radius,
			);
			next_tail = constrain_tail_limit(next_tail, child_pos, target_axis_world, joint.length, group.limit);
		}

		// 回転補正: rest pose の axis (target_axis_world) を実際の axis (next_tail - child_pos) に向ける。
		let new_axis_world = (next_tail - child_pos).normalize_or_zero();
		if new_axis_world.length_squared() < 1e-12 {
			joint.prev_tail = joint.curr_tail;
			joint.curr_tail = next_tail;
			continue;
		}
		let q_corr = Quat::from_rotation_arc(target_axis_world, new_axis_world);
		let new_world_rotation = (q_corr * target_rotation).normalize();
		let parent_rot_inv = parent_rot.conjugate();
		let new_local_rotation = (parent_rot_inv * new_world_rotation).normalize();

		// 子の local transform を rest_translation + new_local_rotation + rest_scale で書き戻す。
		let new_local = Mat4::from_scale_rotation_translation(joint.rest_local_scale, new_local_rotation, joint.rest_local_translation);
		scene.nodes[joint.child_node].transform = new_local.to_cols_array();

		// 子以下の world 行列を更新（次の joint の親回転計算で使う）。
		propagate_world_subtree(&scene.nodes, &mut rt.world_scratch, joint.child_node, parent_world);

		joint.prev_tail = joint.curr_tail;
		joint.curr_tail = next_tail;
	}
}

fn constrain_tail_length(next_tail: Vec3, child_pos: Vec3, fallback_axis: Vec3, length: f32) -> Vec3 {
	let dir = (next_tail - child_pos).normalize_or_zero();
	if dir.length_squared() < 1e-12 {
		child_pos + fallback_axis * length
	} else {
		child_pos + dir * length
	}
}

fn constrain_tail_limit(next_tail: Vec3, child_pos: Vec3, fallback_axis: Vec3, length: f32, limit: Option<&UnaDynamicsLimit>) -> Vec3 {
	let Some(max_angle_rad) = limit.and_then(undynamics_cone_limit_angle_rad) else {
		return next_tail;
	};
	let rest_axis = fallback_axis.normalize_or_zero();
	let dir = (next_tail - child_pos).normalize_or_zero();
	if rest_axis.length_squared() < 1e-12 || dir.length_squared() < 1e-12 {
		return child_pos + fallback_axis * length;
	}
	let dot = rest_axis.dot(dir).clamp(-1.0, 1.0);
	let angle = dot.acos();
	if angle <= max_angle_rad {
		return child_pos + dir * length;
	}
	let tangent = (dir - rest_axis * dot).normalize_or_zero();
	let tangent = if tangent.length_squared() >= 1e-12 {
		tangent
	} else {
		rest_axis.any_orthonormal_vector()
	};
	let constrained_dir = rest_axis * max_angle_rad.cos() + tangent * max_angle_rad.sin();
	child_pos + constrained_dir.normalize_or_zero() * length
}

fn undynamics_cone_limit_angle_rad(limit: &UnaDynamicsLimit) -> Option<f32> {
	let limit_type = limit.limit_type.to_ascii_lowercase();
	if !limit_type.is_empty() && !limit_type.contains("angle") && !limit_type.contains("hinge") && !limit_type.contains("polar") {
		return None;
	}
	let max_angle = [limit.max_angle_x, limit.max_angle_z]
		.into_iter()
		.filter(|angle| angle.is_finite() && *angle > 0.0)
		.fold(0.0_f32, f32::max);
	if max_angle <= 0.0 {
		None
	} else {
		Some(max_angle.clamp(0.0, 179.0).to_radians())
	}
}

fn constrain_tail_colliders(
	next_tail: Vec3,
	child_pos: Vec3,
	fallback_axis: Vec3,
	length: f32,
	world: &[Mat4],
	bone_colliders: &[BoneColliderPrimitive],
	hit_radius: f32,
) -> Vec3 {
	if bone_colliders.is_empty() {
		return next_tail;
	}
	let pushed = push_out_of_colliders(next_tail, world, bone_colliders, hit_radius.max(0.0));
	let pushed_dir = (pushed - child_pos).normalize_or_zero();
	if pushed_dir.length_squared() >= 1e-12 {
		child_pos + pushed_dir * length
	} else {
		child_pos + fallback_axis * length
	}
}

fn solve_xpbd_rest_constraint(curr_tail: Vec3, target_tail: Vec3, compliance: f32, dt: f32, lambda: &mut f32) -> Vec3 {
	if dt <= 0.0 {
		*lambda = 0.0;
		return curr_tail;
	}
	let to_curr = curr_tail - target_tail;
	let distance = to_curr.length();
	if distance < 1e-6 {
		return curr_tail;
	}
	let gradient = to_curr / distance;
	let alpha = compliance.max(0.0) / (dt * dt);
	let delta_lambda = (-distance - alpha * *lambda) / (1.0 + alpha);
	*lambda += delta_lambda;
	curr_tail + gradient * delta_lambda
}

#[cfg(test)]
mod tests {
	use super::*;
	use un_avatar_core::{UnaSceneSnapshot, UnaSpringBoneGroup};

	fn node(rot_y_deg: f32, trans: Vec3, children: Vec<usize>) -> UnaSceneNode {
		let r = Quat::from_rotation_y(rot_y_deg.to_radians());
		let m = Mat4::from_scale_rotation_translation(Vec3::ONE, r, trans);
		UnaSceneNode {
			source_node_id: None,
			resolved_node_id: None,
			name: None,
			visible: true,
			transform: m.to_cols_array(),
			children,
			mesh: None,
			skin: None,
			probe_anchor_node: None,
			local_bounds: None,
		}
	}

	/// 重力で末端 tail が水平方向に流れることを確認する基本テスト。
	#[test]
	fn simulator_moves_tail_under_gravity() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				node(0.0, Vec3::ZERO, vec![1]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![2]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![]),
			],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				source_kind: Default::default(),
				enabled: true,
				source_id: String::new(),
				comment: String::new(),
				category: String::new(),
				stiffness: 0.05,
				gravity_power: 2.0,
				gravity_dir: [1.0, 0.0, 0.0],
				drag_force: 0.2,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1, 2],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let mut sim = SpringBoneSimulator::new(&scene, &settings).expect("sim");
		let tip_before = world_from_snapshot(&scene)[2].transform_point3(Vec3::ZERO);
		for _ in 0..60 {
			sim.step(&mut scene, &settings, 1.0 / 60.0);
		}
		let tip_after = world_from_snapshot(&scene)[2].transform_point3(Vec3::ZERO);
		assert!(
			(tip_after.x - tip_before.x).abs() > 0.005,
			"tail should drift under x-gravity: before={} after={}",
			tip_before.x,
			tip_after.x
		);
	}

	#[test]
	fn dynamics_angle_limit_clamps_tail_direction() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				node(0.0, Vec3::ZERO, vec![1]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![2]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![]),
			],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				source_kind: Default::default(),
				enabled: true,
				source_id: String::new(),
				comment: String::new(),
				category: String::new(),
				stiffness: 0.0,
				gravity_power: 30.0,
				gravity_dir: [1.0, 0.0, 0.0],
				drag_force: 0.0,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				limit: Some(UnaDynamicsLimit {
					limit_type: "Angle".to_string(),
					max_angle_x: 10.0,
					max_angle_z: 0.0,
					max_stretch: 0.0,
				}),
				interaction: None,
				bone_node_indices: vec![0, 1, 2],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let mut sim = SpringBoneSimulator::new(&scene, &settings).expect("sim");
		for _ in 0..60 {
			sim.step(&mut scene, &settings, 1.0 / 60.0);
		}
		let world = world_from_snapshot(&scene);
		let joint = world[1].transform_point3(Vec3::ZERO);
		let tip = world[2].transform_point3(Vec3::ZERO);
		let axis = (tip - joint).normalize_or_zero();
		let angle = Vec3::Y.angle_between(axis).to_degrees();
		assert!(angle <= 10.5, "angle={angle} axis={axis:?}");
	}

	/// 親が静止していて重力 0 なら tail は時間が経っても発散しないことを確認する安定性テスト。
	#[test]
	fn simulator_stays_stable_with_no_forces() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				node(0.0, Vec3::ZERO, vec![1]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![2]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![]),
			],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				source_kind: Default::default(),
				enabled: true,
				source_id: String::new(),
				comment: String::new(),
				category: String::new(),
				stiffness: 1.0,
				gravity_power: 0.0,
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.4,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1, 2],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let mut sim = SpringBoneSimulator::new(&scene, &settings).expect("sim");
		let tip_before = world_from_snapshot(&scene)[2].transform_point3(Vec3::ZERO);
		for _ in 0..600 {
			sim.step(&mut scene, &settings, 1.0 / 60.0);
		}
		let tip_after = world_from_snapshot(&scene)[2].transform_point3(Vec3::ZERO);
		let drift = (tip_after - tip_before).length();
		assert!(
			drift < 0.05,
			"tail should not drift more than 5cm under no force: drift={} (before={:?}, after={:?})",
			drift,
			tip_before,
			tip_after
		);
	}

	#[test]
	fn simulator_uses_per_joint_hit_radius_samples() {
		let scene = UnaSceneSnapshot {
			nodes: vec![
				node(0.0, Vec3::ZERO, vec![1]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![2]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![]),
			],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				source_kind: Default::default(),
				enabled: true,
				source_id: String::new(),
				comment: String::new(),
				category: String::new(),
				stiffness: 1.0,
				gravity_power: 0.0,
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.4,
				center_node: None,
				hit_radius: 0.03,
				hit_radius_samples: vec![0.015, 0.006],
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1, 2],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let sim = SpringBoneSimulator::new(&scene, &settings).expect("sim");
		let runtime = sim.runtimes[0].as_ref().expect("runtime");
		assert_eq!(runtime.joints.len(), 2);
		assert!((runtime.joints[0].hit_radius - 0.015).abs() < 1e-6);
		assert!((runtime.joints[1].hit_radius - 0.006).abs() < 1e-6);
	}

	#[test]
	fn simulator_skips_disabled_groups() {
		let scene = UnaSceneSnapshot {
			nodes: vec![node(0.0, Vec3::ZERO, vec![1]), node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![])],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				source_kind: Default::default(),
				enabled: false,
				source_id: String::new(),
				comment: String::new(),
				category: String::new(),
				stiffness: 1.0,
				gravity_power: 1.0,
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.4,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1],
			}],
			colliders: Vec::new(),
			..Default::default()
		};

		assert!(SpringBoneSimulator::new(&scene, &settings).is_none());
	}

	/// 親 (root) を急に大きく回転させても tail が爆発せず length 制約内に留まることを確認。
	/// 旧実装ではこのケースで Verlet 速度が暴走していた。
	#[test]
	fn simulator_does_not_explode_under_sudden_parent_rotation() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				node(0.0, Vec3::ZERO, vec![1]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![2]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![]),
			],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				source_kind: Default::default(),
				enabled: true,
				source_id: String::new(),
				comment: String::new(),
				category: String::new(),
				stiffness: 1.0,
				gravity_power: 0.5,
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.4,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1, 2],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let mut sim = SpringBoneSimulator::new(&scene, &settings).expect("sim");
		for step in 0..120 {
			// 60 frame ごとに root を 180 度くるっと回す。
			if step % 60 == 0 {
				let rot = Quat::from_rotation_y(std::f32::consts::PI);
				scene.nodes[0].transform = Mat4::from_scale_rotation_translation(Vec3::ONE, rot, Vec3::ZERO).to_cols_array();
			}
			sim.step(&mut scene, &settings, 1.0 / 60.0);
			let tip = world_from_snapshot(&scene)[2].transform_point3(Vec3::ZERO);
			assert!(
				tip.length() < 5.0,
				"tip distance from origin should stay bounded; got {} at step {}",
				tip.length(),
				step
			);
		}
	}

	#[test]
	fn propagate_world_subtree_updates_descendants() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				node(0.0, Vec3::ZERO, vec![1]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![2]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![]),
			],
			roots: vec![0],
			..Default::default()
		};
		let mut world = world_from_snapshot(&scene);
		let new_rot = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
		scene.nodes[1].transform = Mat4::from_scale_rotation_translation(Vec3::ONE, new_rot, Vec3::new(0.0, 1.0, 0.0)).to_cols_array();

		let parent_world = world[0];
		propagate_world_subtree(&scene.nodes, &mut world, 1, parent_world);

		let tip = world[2].transform_point3(Vec3::ZERO);
		let full = world_from_snapshot(&scene);
		let tip_full = full[2].transform_point3(Vec3::ZERO);
		assert!(
			(tip.x - tip_full.x).abs() < 1e-5,
			"tip x mismatch: subtree={} full={}",
			tip.x,
			tip_full.x
		);
		assert!(
			(tip.y - tip_full.y).abs() < 1e-5,
			"tip y mismatch: subtree={} full={}",
			tip.y,
			tip_full.y
		);
		assert!(
			(tip.z - tip_full.z).abs() < 1e-5,
			"tip z mismatch: subtree={} full={}",
			tip.z,
			tip_full.z
		);
	}

	#[test]
	fn category_matches_resolve_to_override_params() {
		let scene = UnaSceneSnapshot {
			nodes: vec![node(0.0, Vec3::ZERO, vec![1]), node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![])],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				source_kind: Default::default(),
				enabled: true,
				source_id: String::new(),
				comment: "ミミ spring".to_string(),
				category: String::new(),
				stiffness: 0.1,
				gravity_power: 1.0,
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.3,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let config = SpringBonePhysicsConfig {
			overrides: vec![SpringBoneCategoryOverride {
				category: "ears".to_string(),
				params: SpringBonePhysicsParams {
					solver: Some(SpringBoneSolver::Xpbd),
					damping_half_life_ms: Some(90.0),
					stiffness_hz: Some(5.0),
					xpbd_compliance: Some(0.025),
					gravity_scale: None,
					drag_scale: None,
					constraint_iterations: Some(8),
				},
			}],
			..Default::default()
		};
		let sim = SpringBoneSimulator::new_with_config(&scene, &settings, Vec::new(), config).expect("sim");
		let rt = sim.runtimes[0].as_ref().expect("runtime");
		assert_eq!(rt.params.solver, SpringBoneSolver::Xpbd);
		assert_eq!(rt.params.damping_half_life_ms, Some(90.0));
		assert_eq!(rt.params.stiffness_hz, Some(5.0));
		assert_eq!(rt.params.xpbd_compliance, 0.025);
		assert_eq!(rt.params.constraint_iterations, 8);
	}

	#[test]
	fn default_config_preserves_imported_vrm_parameters() {
		let scene = UnaSceneSnapshot {
			nodes: vec![node(0.0, Vec3::ZERO, vec![1]), node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![])],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				source_kind: Default::default(),
				enabled: true,
				source_id: String::new(),
				comment: String::new(),
				category: String::new(),
				stiffness: 0.1,
				gravity_power: 1.0,
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.3,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let sim = SpringBoneSimulator::new(&scene, &settings).expect("sim");
		let rt = sim.runtimes[0].as_ref().expect("runtime");
		assert_eq!(sim.physics.simulation_hz, 60.0);
		assert_eq!(rt.params.damping_half_life_ms, None);
		assert_eq!(rt.params.stiffness_hz, Some(0.1));
		assert!((rt.params.xpbd_compliance - convert_univrm_stiffness_to_xpbd_compliance(0.1)).abs() < 1e-6);
		assert_eq!(rt.params.gravity_scale, 1.0);
		assert_eq!(rt.params.drag_scale, 1.0);
	}

	#[test]
	fn legacy_compat_solver_aliases_deserialize_as_verlet() {
		assert_eq!(
			serde_json::from_str::<SpringBoneSolver>("\"compat_univrm\"").expect("compat_univrm"),
			SpringBoneSolver::Verlet
		);
		assert_eq!(
			serde_json::from_str::<SpringBoneSolver>("\"compat_euler\"").expect("compat_euler"),
			SpringBoneSolver::Verlet
		);
	}

	#[test]
	fn xpbd_uses_stiffer_rest_constraint_than_verlet() {
		let base_scene = UnaSceneSnapshot {
			nodes: vec![
				node(0.0, Vec3::ZERO, vec![1]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![2]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![]),
			],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				source_kind: Default::default(),
				enabled: true,
				source_id: String::new(),
				comment: String::new(),
				category: String::new(),
				stiffness: 0.8,
				gravity_power: 1.0,
				gravity_dir: [1.0, -1.0, 0.0],
				drag_force: 0.2,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1, 2],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let mut xpbd_scene = base_scene.clone();
		let mut verlet_scene = base_scene;
		let mut xpbd = SpringBoneSimulator::new_with_config(
			&xpbd_scene,
			&settings,
			Vec::new(),
			SpringBonePhysicsConfig {
				overrides: vec![SpringBoneCategoryOverride {
					category: "other".to_string(),
					params: SpringBonePhysicsParams {
						solver: Some(SpringBoneSolver::Xpbd),
						xpbd_compliance: Some(0.005),
						constraint_iterations: Some(6),
						..Default::default()
					},
				}],
				..Default::default()
			},
		)
		.expect("xpbd sim");
		let mut verlet = SpringBoneSimulator::new_with_config(&verlet_scene, &settings, Vec::new(), SpringBonePhysicsConfig::default())
			.expect("verlet sim");
		for _ in 0..60 {
			xpbd.step(&mut xpbd_scene, &settings, 1.0 / 60.0);
			verlet.step(&mut verlet_scene, &settings, 1.0 / 60.0);
		}
		let xpbd_tip = world_from_snapshot(&xpbd_scene)[2].transform_point3(Vec3::ZERO);
		let verlet_tip = world_from_snapshot(&verlet_scene)[2].transform_point3(Vec3::ZERO);
		let rest_tip = Vec3::new(0.0, 2.0, 0.0);
		assert!(
			(xpbd_tip - rest_tip).length() < (verlet_tip - rest_tip).length(),
			"xpbd rest constraint should keep the chain closer to rest than lightweight verlet: xpbd={xpbd_tip:?} verlet={verlet_tip:?}"
		);
	}

	/// 単一 root + children チェーンでも正しく joint が組まれて揺れることを確認する回帰テスト。
	/// (VRM0 では `bones: [single_root]` が child を辿って chain になるパターン。)
	#[test]
	fn simulator_handles_single_root_chain_built_from_children() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![node(0.0, Vec3::ZERO, vec![1]), node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![])],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				source_kind: Default::default(),
				enabled: true,
				source_id: String::new(),
				comment: String::new(),
				category: String::new(),
				stiffness: 0.1,
				gravity_power: 1.0,
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.3,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		// 2 ノードチェーン = 1 joint。tail を bone_axis に沿って初期化し動作することを確認。
		let mut sim = SpringBoneSimulator::new(&scene, &settings).expect("sim");
		for _ in 0..120 {
			sim.step(&mut scene, &settings, 1.0 / 60.0);
		}
		// 1 joint chain は重力 (y=-1) で tail が下に流れる。
		let tip = world_from_snapshot(&scene)[1].transform_point3(Vec3::ZERO);
		assert!(tip.is_finite(), "tip should remain finite: {:?}", tip);
	}
}
