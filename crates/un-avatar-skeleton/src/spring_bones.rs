//! UNPhysics / UNDynamics bone dynamics simulation.
//!
//! This module keeps the historic `spring_bones` name as an API compatibility
//! shim. The runtime model is UNDynamics: VRM SpringBone and VRC PhysBone data
//! are lowered into source-neutral response terms, then solved by the same
//! backend. The goal is not numeric compatibility with either source solver.
//! Each joint step uses the following source-neutral pipeline:
//!
//! 1. `parent_world_rot = decompose(world[parent]).rotation`
//! 2. `child_world_pos  = world[parent].transform_point(rest_local_translation)`
//! 3. `target_rotation  = parent_world_rot * rest_local_rotation`  (= 揺れが無いときの joint world rot)
//! 4. `target_axis_world = target_rotation * bone_axis`             (= rest pose 子方向の world ベクトル)
//! 5. 前回の child/target frame から現在の child/target frame へ tail 状態を移し、親 motion を反映する。
//! 6. Verlet / XPBD 共通の UNPhysics response terms:
//!    - `inertia      = (curr_tail - prev_tail) * inertia_retention(damping, bounce_response)`
//!    - `rest_response = (target_tail - curr_tail) * rest_gain`      (= rest pose への復元)
//!    - `bounce_response` preserves some damped inertia but never raises retention above 1
//!    - `orientation  = previous_orientation_tail - curr_tail`       (= 向き保持)
//!    - `target_tail` is rest pose biased by gravity, and may stretch only when a stretch constraint allows it.
//!    - `next_tail    = curr_tail + inertia + rest_response + orientation`
//! 7. 長さ拘束: stretch writeback target が無い場合は rest length、ある場合は limit の範囲へ収める
//! 8. 回転補正: `q_corr = from_rotation_arc(target_axis_world, (next_tail - child_world_pos).normalize())`
//!    - `new_world_rot   = q_corr * target_rotation`
//!    - `new_local_rot   = parent_world_rot.inverse() * new_world_rot`
//!    - `scene.nodes[child].transform` に `(rest_scale, new_local_rot, rest_translation)` で書き戻す
//! 9. `world_scratch` に新しい `world[child]` 以下を伝播し、次の joint の親回転に使う
//!
//! 旧実装から v2.1 UNDynamics へ移行する上で固定した主な性質:
//! - `ideal_tail` を「grandchild の現在 world 座標」から取っていたため、SpringBone 自身が前フレームで
//!   動かした位置が次フレームの目標位置になり stiffness pull が打ち消されていた
//!   → `rest_local_rotation` と `bone_axis` を初期化時に snapshot 保存し、毎フレームの目標は
//!   **rest pose ベース** で再計算する。
//! - source の Pull / Spring / Momentum / Stiffness / Immobile は最終 solver 値ではなく、
//!   `rest_response` / `bounce` / `shape_preservation` / `motion_coupling` へ lower する authored intent。
//! - VRC PhysBone Stiffness 相当値は rest pose 復元力へ混ぜず、局所形状保持
//!   `shape_preservation` として扱う。VRM SpringBone stiffness は UNDynamics view で
//!   rest-pull intent として `pull` へ lower する。
//! - dt 可変だと Verlet 速度 `curr - prev` が前フレームの dt 分の変位を表すため発散しやすい
//!   → accumulator で profile 設定の固定 dt サブステップ化を行う。

use std::{collections::BTreeMap, time::Instant};

use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use un_avatar_core::{
	una_dynamics_translation_writeback_candidate_count, UnaDynamicsGroup, UnaDynamicsImmobileType, UnaDynamicsLimit, UnaDynamicsSettings,
	UnaDynamicsWritebackMode, UnaRuntimeDynamics, UnaSceneNode, UnaSceneSnapshot,
};

use crate::bone_colliders::{
	push_out_of_world_collider, resolve_world_colliders, BoneColliderPrimitive, RuntimeBoneColliderPrimitive, WorldBoneColliderPrimitive,
};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DynamicsSolver {
	#[default]
	#[serde(alias = "compat_univrm", alias = "compat_euler", alias = "compat", alias = "euler")]
	Verlet,
	Xpbd,
}

pub type SpringBoneSolver = DynamicsSolver;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DynamicsTimeMode {
	FrameBased,
	#[default]
	TimeBased,
}

pub type SpringBoneTimeMode = DynamicsTimeMode;

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct DynamicsStepProfile {
	pub fixed_steps: u32,
	pub active_groups: u32,
	pub active_joints: u32,
	pub collision_projection_count: u32,
	pub collision_projection_source_ids: Vec<String>,
	pub collision_projection_source_counts: BTreeMap<String, u32>,
	pub collision_projection_collider_paths: Vec<String>,
	pub collision_projection_collider_path_counts: BTreeMap<String, u32>,
	pub collision_projection_source_collider_path_counts: BTreeMap<String, BTreeMap<String, u32>>,
	pub world_ms: f32,
	pub collider_ms: f32,
	pub solve_ms: f32,
	pub solve_collision_ms: f32,
	pub solve_propagate_ms: f32,
}

impl DynamicsStepProfile {
	fn record_collision_projection(&mut self, source_id: &str, collider_path: Option<&str>) {
		self.collision_projection_count = self.collision_projection_count.saturating_add(1);
		let count = self.collision_projection_source_counts.entry(source_id.to_string()).or_default();
		*count = count.saturating_add(1);
		push_bounded_unique_string(&mut self.collision_projection_source_ids, source_id, 16);
		if let Some(collider_path) = collider_path.filter(|path| !path.is_empty()) {
			let count = self
				.collision_projection_collider_path_counts
				.entry(collider_path.to_string())
				.or_default();
			*count = count.saturating_add(1);
			let source_path_counts = self
				.collision_projection_source_collider_path_counts
				.entry(source_id.to_string())
				.or_default();
			let count = source_path_counts.entry(collider_path.to_string()).or_default();
			*count = count.saturating_add(1);
			push_bounded_unique_string(&mut self.collision_projection_collider_paths, collider_path, 16);
		}
	}
}

fn push_bounded_unique_string(out: &mut Vec<String>, value: &str, limit: usize) {
	if out.len() < limit && !out.iter().any(|item| item == value) {
		out.push(value.to_string());
	}
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct DynamicsPhysicsConfig {
	pub time_mode: DynamicsTimeMode,
	#[serde(default = "default_spring_bone_simulation_hz")]
	pub simulation_hz: f32,
	#[serde(default = "default_spring_bone_substeps")]
	pub substeps: u32,
	pub mesh_cloth_assist: DynamicsMeshClothAssistConfig,
	#[serde(default = "default_dynamics_surface_constraints_enabled")]
	pub surface_constraints_enabled: bool,
	#[serde(default = "default_dynamics_surface_constraint_topology_max_edge_distance_m")]
	pub surface_constraint_topology_max_edge_distance_m: f32,
	#[serde(default = "default_dynamics_surface_constraint_topology_max_mean_edge_distance_m")]
	pub surface_constraint_topology_max_mean_edge_distance_m: f32,
	#[serde(default = "default_dynamics_surface_constraint_spatial_max_distance_m")]
	pub surface_constraint_spatial_max_distance_m: f32,
	#[serde(default = "default_dynamics_surface_constraint_topology_stiffness")]
	pub surface_constraint_topology_stiffness: f32,
	#[serde(default = "default_dynamics_surface_constraint_spatial_stiffness")]
	pub surface_constraint_spatial_stiffness: f32,
	#[serde(default = "default_dynamics_surface_constraint_min_edge_count")]
	pub surface_constraint_min_edge_count: u32,
	#[serde(default = "default_spring_bone_categories")]
	pub categories: Vec<DynamicsCategoryDefinition>,
	pub overrides: Vec<DynamicsCategoryOverride>,
	pub match_overrides: Vec<DynamicsMatchOverride>,
	pub group_overrides: Vec<DynamicsGroupOverride>,
	pub collider_augment_overrides: Vec<DynamicsColliderAugmentOverride>,
}

impl Default for DynamicsPhysicsConfig {
	fn default() -> Self {
		Self {
			time_mode: DynamicsTimeMode::TimeBased,
			simulation_hz: default_spring_bone_simulation_hz(),
			substeps: 1,
			mesh_cloth_assist: DynamicsMeshClothAssistConfig::default(),
			surface_constraints_enabled: default_dynamics_surface_constraints_enabled(),
			surface_constraint_topology_max_edge_distance_m: default_dynamics_surface_constraint_topology_max_edge_distance_m(),
			surface_constraint_topology_max_mean_edge_distance_m: default_dynamics_surface_constraint_topology_max_mean_edge_distance_m(),
			surface_constraint_spatial_max_distance_m: default_dynamics_surface_constraint_spatial_max_distance_m(),
			surface_constraint_topology_stiffness: default_dynamics_surface_constraint_topology_stiffness(),
			surface_constraint_spatial_stiffness: default_dynamics_surface_constraint_spatial_stiffness(),
			surface_constraint_min_edge_count: default_dynamics_surface_constraint_min_edge_count(),
			categories: default_spring_bone_categories(),
			overrides: Vec::new(),
			match_overrides: Vec::new(),
			group_overrides: Vec::new(),
			collider_augment_overrides: Vec::new(),
		}
	}
}

impl DynamicsPhysicsConfig {
	pub fn normalized(mut self) -> Self {
		if !self.simulation_hz.is_finite() {
			self.simulation_hz = default_spring_bone_simulation_hz();
		}
		self.simulation_hz = self.simulation_hz.clamp(30.0, 240.0);
		self.substeps = self.substeps.clamp(1, 8);
		if matches!(self.time_mode, DynamicsTimeMode::FrameBased) {
			self.time_mode = DynamicsTimeMode::TimeBased;
		}
		self.mesh_cloth_assist = self.mesh_cloth_assist.normalized();
		self.surface_constraint_topology_max_edge_distance_m = finite_or(
			self.surface_constraint_topology_max_edge_distance_m,
			default_dynamics_surface_constraint_topology_max_edge_distance_m(),
		)
		.clamp(0.001, 0.2);
		self.surface_constraint_topology_max_mean_edge_distance_m = finite_or(
			self.surface_constraint_topology_max_mean_edge_distance_m,
			default_dynamics_surface_constraint_topology_max_mean_edge_distance_m(),
		)
		.clamp(0.001, self.surface_constraint_topology_max_edge_distance_m);
		self.surface_constraint_spatial_max_distance_m = finite_or(
			self.surface_constraint_spatial_max_distance_m,
			default_dynamics_surface_constraint_spatial_max_distance_m(),
		)
		.clamp(0.001, 0.1);
		self.surface_constraint_topology_stiffness = finite_or(
			self.surface_constraint_topology_stiffness,
			default_dynamics_surface_constraint_topology_stiffness(),
		)
		.clamp(0.0, 1.0);
		self.surface_constraint_spatial_stiffness = finite_or(
			self.surface_constraint_spatial_stiffness,
			default_dynamics_surface_constraint_spatial_stiffness(),
		)
		.clamp(0.0, 1.0);
		self.surface_constraint_min_edge_count = self.surface_constraint_min_edge_count.clamp(1, 64);
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
			self.categories.push(DynamicsCategoryDefinition {
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
		for override_item in &mut self.match_overrides {
			override_item.normalize();
		}
		self.match_overrides.retain(DynamicsMatchOverride::has_matcher);
		for override_item in &mut self.group_overrides {
			override_item.source_id = override_item.source_id.trim().to_string();
			override_item.params = override_item.params.normalized();
		}
		self.group_overrides.retain(|o| !o.source_id.is_empty());
		for override_item in &mut self.collider_augment_overrides {
			override_item.normalize();
		}
		self.collider_augment_overrides.retain(DynamicsColliderAugmentOverride::has_matcher);
		self
	}

	fn fixed_dt(&self) -> f32 {
		1.0 / self.simulation_hz.clamp(30.0, 240.0)
	}
}

pub type SpringBonePhysicsConfig = DynamicsPhysicsConfig;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct DynamicsMeshClothAssistConfig {
	pub enabled: bool,
	/// Vertices dominated by non-dynamic body joints above this weight become assist candidates.
	pub body_dominance_threshold: f32,
	/// Existing dynamic lanes or static cloth bridge lanes below this weight need stronger connected evidence.
	pub min_existing_dynamic_weight: f32,
	/// If no dynamic lane is already present, seed a small influence from a nearby dynamic cloth joint in the same skin.
	pub seed_missing_dynamic_influence: bool,
	/// Maximum total influence that may be reassigned from body joints to nearby dynamic cloth joints.
	pub max_assist_weight: f32,
	/// Mesh/node path filters. Empty means all cloth-like meshes may be diagnosed.
	pub mesh_path_contains: Vec<String>,
}

impl Default for DynamicsMeshClothAssistConfig {
	fn default() -> Self {
		Self {
			enabled: false,
			body_dominance_threshold: 0.55,
			min_existing_dynamic_weight: 0.05,
			seed_missing_dynamic_influence: true,
			max_assist_weight: 0.35,
			mesh_path_contains: Vec::new(),
		}
	}
}

impl DynamicsMeshClothAssistConfig {
	fn normalized(mut self) -> Self {
		self.body_dominance_threshold = finite_or(self.body_dominance_threshold, 0.55).clamp(0.05, 0.99);
		self.min_existing_dynamic_weight = finite_or(self.min_existing_dynamic_weight, 0.05).clamp(0.0, 0.95);
		self.max_assist_weight = finite_or(self.max_assist_weight, 0.35).clamp(0.0, 0.95);
		self.mesh_path_contains = self
			.mesh_path_contains
			.iter()
			.map(|value| normalize_match_text(value))
			.filter(|value| !value.is_empty())
			.collect();
		self
	}
}

const MESH_CLOTH_ASSIST_BODY_JOINT_ALIASES: &[&str] = &[
	"chest",
	"breast",
	"shoulder",
	"upperarm",
	"upper_arm",
	"lowerarm",
	"lower_arm",
	"hips",
	"spine",
];
const MESH_CLOTH_ASSIST_CLOTH_JOINT_ALIASES: &[&str] = &[
	"cape",
	"skirt",
	"cloth",
	"frill",
	"frills",
	"sleeve",
	"shirt",
	"sweater",
	"blouse",
	"dress",
	"coat",
	"longcoat",
	"stocking",
	"stockings",
	"布",
	"スカート",
	"袖",
	"ケープ",
	"シャツ",
	"セーター",
	"ブラウス",
	"ドレス",
	"コート",
	"フリル",
	"靴下",
	"ストッキング",
];

pub fn dynamics_mesh_cloth_assist_mesh_matches(mesh_path: &str, filters: &[String]) -> bool {
	let config = DynamicsPhysicsConfig::default().normalized();
	dynamics_mesh_cloth_assist_mesh_matches_with_categories(mesh_path, filters, &config.categories)
}

pub fn dynamics_mesh_cloth_assist_mesh_matches_with_categories(
	mesh_path: &str,
	filters: &[String],
	categories: &[DynamicsCategoryDefinition],
) -> bool {
	let mesh_path = normalize_match_text(mesh_path);
	if filters.is_empty() {
		return categories
			.iter()
			.find(|category| category.id == "cloth")
			.is_some_and(|category| category.matches.iter().any(|alias| normalized_alias_matches(&mesh_path, alias)));
	}
	filters.iter().any(|filter| explicit_contains_match(&mesh_path, filter))
}

pub fn dynamics_mesh_cloth_assist_body_joint_matches(joint_leaf: &str) -> bool {
	let leaf = normalize_match_text(joint_leaf);
	normalized_text_contains_any(&leaf, MESH_CLOTH_ASSIST_BODY_JOINT_ALIASES)
}

pub fn dynamics_mesh_cloth_assist_cloth_joint_matches(joint_leaf: &str) -> bool {
	let leaf = normalize_match_text(joint_leaf);
	normalized_text_contains_any(&leaf, MESH_CLOTH_ASSIST_CLOTH_JOINT_ALIASES)
}

pub fn dynamics_mesh_cloth_assist_deforming_nodes(
	bone_node_indices: &[usize],
	interaction_chain_start_index: usize,
) -> impl Iterator<Item = usize> + '_ {
	let start = interaction_chain_start_index.max(2).min(bone_node_indices.len());
	bone_node_indices.iter().skip(start).copied()
}

pub fn dynamics_token_filter_matches(value: &str, filter: &str) -> bool {
	explicit_contains_match(value, filter)
}

pub fn dynamics_normalize_match_text(value: &str) -> String {
	normalize_match_text(value)
}

pub fn dynamics_normalized_token_filter_matches(normalized_value: &str, normalized_filter: &str) -> bool {
	normalized_alias_matches(normalized_value, normalized_filter)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DynamicsMeshClothAssistJointRole {
	Body,
	Dynamic,
	StaticCloth,
	Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DynamicsMeshClothAssistTransferKind {
	ExistingDynamicLane,
	SeedMissingDynamicLane,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DynamicsMeshClothAssistTransferCandidate {
	pub kind: DynamicsMeshClothAssistTransferKind,
	pub transfer_weight: f32,
}

pub fn dynamics_mesh_cloth_assist_transfer_candidate(
	config: &DynamicsMeshClothAssistConfig,
	has_dynamic_lane: bool,
	body_weight: f32,
	dynamic_weight: f32,
	static_cloth_weight: f32,
	neighbor_dynamic_weight: f32,
	transferred_total: f32,
) -> Option<DynamicsMeshClothAssistTransferCandidate> {
	let remaining_assist = (config.max_assist_weight - transferred_total).max(0.0);
	if remaining_assist <= 0.000001 || body_weight <= 0.000001 {
		return None;
	}
	if !has_dynamic_lane {
		if !config.seed_missing_dynamic_influence
			|| body_weight < config.body_dominance_threshold
			|| static_cloth_weight < config.min_existing_dynamic_weight
			|| neighbor_dynamic_weight < 0.001
		{
			return None;
		}
		let transfer_weight = neighbor_dynamic_weight.min(remaining_assist).min(body_weight).max(0.0);
		return (transfer_weight > 0.000001).then_some(DynamicsMeshClothAssistTransferCandidate {
			kind: DynamicsMeshClothAssistTransferKind::SeedMissingDynamicLane,
			transfer_weight,
		});
	}

	let dynamic_bridge = static_cloth_weight >= config.min_existing_dynamic_weight;
	let body_dominant = body_weight >= config.body_dominance_threshold;
	let static_cloth_bridge = dynamic_bridge && body_weight >= config.min_existing_dynamic_weight;
	let min_neighbor_delta = if dynamic_bridge {
		0.001
	} else {
		config.min_existing_dynamic_weight
	};
	if (!body_dominant && !static_cloth_bridge)
		|| (!dynamic_bridge && dynamic_weight < config.min_existing_dynamic_weight)
		|| neighbor_dynamic_weight <= dynamic_weight + min_neighbor_delta
	{
		return None;
	}
	let transfer_weight = (neighbor_dynamic_weight - dynamic_weight)
		.min(remaining_assist)
		.min(body_weight)
		.max(0.0);
	(transfer_weight > 0.000001).then_some(DynamicsMeshClothAssistTransferCandidate {
		kind: DynamicsMeshClothAssistTransferKind::ExistingDynamicLane,
		transfer_weight,
	})
}

pub fn dynamics_mesh_cloth_assist_joint_roles<'a>(
	skin: &un_avatar_core::UnaSkin,
	joint_count: usize,
	dynamic_nodes: Option<&[usize]>,
	mut joint_leaf: impl FnMut(usize) -> &'a str,
) -> Vec<DynamicsMeshClothAssistJointRole> {
	let mut roles = vec![DynamicsMeshClothAssistJointRole::Other; joint_count];
	for (joint_index, role) in roles.iter_mut().enumerate() {
		if let Some(nodes) = dynamic_nodes {
			if skin
				.joint_nodes
				.get(joint_index)
				.is_some_and(|node_index| nodes.binary_search(node_index).is_ok())
			{
				*role = DynamicsMeshClothAssistJointRole::Dynamic;
				continue;
			}
		}
		let leaf = joint_leaf(joint_index);
		*role = dynamics_mesh_cloth_assist_alias_joint_role(leaf, dynamic_nodes.is_none());
	}
	roles
}

fn dynamics_mesh_cloth_assist_alias_joint_role(joint_leaf: &str, cloth_alias_is_dynamic: bool) -> DynamicsMeshClothAssistJointRole {
	let leaf = normalize_match_text(joint_leaf);
	let cloth_alias = normalized_text_contains_any(&leaf, MESH_CLOTH_ASSIST_CLOTH_JOINT_ALIASES);
	if cloth_alias_is_dynamic && cloth_alias {
		DynamicsMeshClothAssistJointRole::Dynamic
	} else if normalized_text_contains_any(&leaf, MESH_CLOTH_ASSIST_BODY_JOINT_ALIASES) {
		DynamicsMeshClothAssistJointRole::Body
	} else if cloth_alias {
		DynamicsMeshClothAssistJointRole::StaticCloth
	} else {
		DynamicsMeshClothAssistJointRole::Other
	}
}

pub trait DynamicsMeshClothAssistVertex {
	fn joints(&self) -> [u16; 4];
	fn weights(&self) -> [f32; 4];
	fn set_joints(&mut self, joints: [u16; 4]);
	fn set_weights(&mut self, weights: [f32; 4]);
}

#[derive(Clone, Copy, Default)]
struct DynamicsMeshClothAssistVertexProfile {
	body_lanes: [(usize, f32); 4],
	body_lane_count: usize,
	dynamic_lanes: [(usize, f32); 4],
	dynamic_lane_count: usize,
	dynamic_joint_weights: [(usize, f32); 4],
	dynamic_joint_weight_count: usize,
	body_weight: f32,
	dynamic_weight: f32,
	static_cloth_weight: f32,
}

impl DynamicsMeshClothAssistVertexProfile {
	fn push_body_lane(&mut self, lane: usize, weight: f32) {
		if self.body_lane_count < self.body_lanes.len() {
			self.body_lanes[self.body_lane_count] = (lane, weight);
			self.body_lane_count += 1;
		}
	}

	fn push_dynamic_lane(&mut self, lane: usize, joint_index: usize, weight: f32) {
		if self.dynamic_lane_count < self.dynamic_lanes.len() {
			self.dynamic_lanes[self.dynamic_lane_count] = (lane, weight);
			self.dynamic_lane_count += 1;
		}
		if self.dynamic_joint_weight_count < self.dynamic_joint_weights.len() {
			self.dynamic_joint_weights[self.dynamic_joint_weight_count] = (joint_index, weight);
			self.dynamic_joint_weight_count += 1;
		}
	}

	fn body_lanes(&self) -> impl Iterator<Item = (usize, f32)> + '_ {
		self.body_lanes[..self.body_lane_count].iter().copied()
	}

	fn dynamic_lanes(&self) -> impl Iterator<Item = (usize, f32)> + '_ {
		self.dynamic_lanes[..self.dynamic_lane_count].iter().copied()
	}

	fn dynamic_joint_weights(&self) -> impl Iterator<Item = (usize, f32)> + '_ {
		self.dynamic_joint_weights[..self.dynamic_joint_weight_count].iter().copied()
	}
}

pub fn apply_dynamics_mesh_cloth_assist_to_vertices<V: DynamicsMeshClothAssistVertex>(
	vertices: &mut [V],
	indices: &[u32],
	joint_count: usize,
	config: &DynamicsMeshClothAssistConfig,
	joint_role: impl Fn(usize) -> DynamicsMeshClothAssistJointRole,
) -> usize {
	if !config.enabled || config.max_assist_weight <= 0.0 || vertices.is_empty() || indices.is_empty() || joint_count == 0 {
		return 0;
	}
	let mut profiles = vec![DynamicsMeshClothAssistVertexProfile::default(); vertices.len()];
	let mut has_dynamic_joint = false;
	for joint_index in 0..joint_count {
		if joint_role(joint_index) == DynamicsMeshClothAssistJointRole::Dynamic {
			has_dynamic_joint = true;
			break;
		}
	}
	if !has_dynamic_joint {
		return 0;
	}
	let mut transferred_total = vec![0.0_f32; vertices.len()];
	let mut neighbor_dynamic_max = vec![0.0_f32; vertices.len()];
	let mut neighbor_dynamic_joint = vec![None::<usize>; vertices.len()];
	let mut changed = 0usize;
	for _ in 0..6 {
		for (profile, vertex) in profiles.iter_mut().zip(vertices.iter()) {
			*profile = dynamics_mesh_cloth_assist_vertex_profile(vertex, joint_count, &joint_role);
		}
		neighbor_dynamic_max.fill(0.0);
		neighbor_dynamic_joint.fill(None);
		for_each_dynamics_mesh_cloth_assist_neighbor(indices, vertices.len(), |vertex_index, neighbor_index| {
			update_dynamics_mesh_cloth_assist_neighbor(
				vertex_index,
				&profiles[neighbor_index],
				&mut neighbor_dynamic_max,
				&mut neighbor_dynamic_joint,
			);
		});
		let mut pass_changed = false;
		for vertex_index in 0..vertices.len() {
			let profile = &profiles[vertex_index];
			if profile.body_lane_count == 0 {
				continue;
			}
			let neighbor_dynamic = neighbor_dynamic_max[vertex_index];
			let mut joints = vertices[vertex_index].joints();
			let mut weights = vertices[vertex_index].weights();
			let Some(candidate) = dynamics_mesh_cloth_assist_transfer_candidate(
				config,
				profile.dynamic_lane_count > 0,
				profile.body_weight,
				profile.dynamic_weight,
				profile.static_cloth_weight,
				neighbor_dynamic,
				transferred_total[vertex_index],
			) else {
				continue;
			};
			let transfer = match candidate.kind {
				DynamicsMeshClothAssistTransferKind::SeedMissingDynamicLane => {
					let Some(seed_joint_index) = neighbor_dynamic_joint[vertex_index] else {
						continue;
					};
					let Some(seed_lane) = dynamics_mesh_cloth_assist_seed_lane(&weights, profile) else {
						continue;
					};
					let transfer = candidate.transfer_weight;
					for (lane, weight) in profile.body_lanes() {
						let share = weight / profile.body_weight;
						weights[lane] = (weights[lane] - transfer * share).max(0.0);
					}
					joints[seed_lane] = seed_joint_index.min(u16::MAX as usize) as u16;
					weights[seed_lane] = transfer;
					transfer
				}
				DynamicsMeshClothAssistTransferKind::ExistingDynamicLane => {
					let transfer = candidate.transfer_weight;
					for (lane, weight) in profile.body_lanes() {
						let share = weight / profile.body_weight;
						weights[lane] = (weights[lane] - transfer * share).max(0.0);
					}
					for (lane, weight) in profile.dynamic_lanes() {
						let share = weight / profile.dynamic_weight;
						weights[lane] += transfer * share;
					}
					transfer
				}
			};
			normalize_dynamics_mesh_cloth_assist_weights(&mut weights);
			vertices[vertex_index].set_joints(joints);
			vertices[vertex_index].set_weights(weights);
			if transferred_total[vertex_index] <= 0.000001 {
				changed += 1;
			}
			transferred_total[vertex_index] += transfer;
			pass_changed = true;
		}
		if !pass_changed {
			break;
		}
	}
	changed
}

pub fn for_each_dynamics_mesh_cloth_assist_neighbor(indices: &[u32], vertex_count: usize, mut visit: impl FnMut(usize, usize)) {
	for tri in indices.chunks_exact(3) {
		let a = tri[0] as usize;
		let b = tri[1] as usize;
		let c = tri[2] as usize;
		if a >= vertex_count || b >= vertex_count || c >= vertex_count {
			continue;
		}
		visit(a, b);
		visit(a, c);
		visit(b, a);
		visit(b, c);
		visit(c, a);
		visit(c, b);
	}
}

fn dynamics_mesh_cloth_assist_vertex_profile(
	vertex: &impl DynamicsMeshClothAssistVertex,
	joint_count: usize,
	joint_role: &impl Fn(usize) -> DynamicsMeshClothAssistJointRole,
) -> DynamicsMeshClothAssistVertexProfile {
	let joints = vertex.joints();
	let weights = vertex.weights();
	let mut profile = DynamicsMeshClothAssistVertexProfile::default();
	for lane in 0..4 {
		let joint_index = joints[lane] as usize;
		let weight = weights[lane].max(0.0);
		if joint_index >= joint_count || weight <= 0.000001 {
			continue;
		}
		match joint_role(joint_index) {
			DynamicsMeshClothAssistJointRole::Body => {
				profile.push_body_lane(lane, weight);
				profile.body_weight += weight;
			}
			DynamicsMeshClothAssistJointRole::Dynamic => {
				profile.push_dynamic_lane(lane, joint_index, weight);
				profile.dynamic_weight += weight;
			}
			DynamicsMeshClothAssistJointRole::StaticCloth => {
				profile.static_cloth_weight += weight;
			}
			DynamicsMeshClothAssistJointRole::Other => {}
		}
	}
	profile
}

fn update_dynamics_mesh_cloth_assist_neighbor(
	vertex_index: usize,
	neighbor: &DynamicsMeshClothAssistVertexProfile,
	neighbor_dynamic_max: &mut [f32],
	neighbor_dynamic_joint: &mut [Option<usize>],
) {
	let Some((joint_index, joint_weight)) = neighbor
		.dynamic_joint_weights()
		.max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
	else {
		return;
	};
	if joint_weight <= neighbor_dynamic_max[vertex_index] {
		return;
	}
	neighbor_dynamic_max[vertex_index] = joint_weight;
	neighbor_dynamic_joint[vertex_index] = Some(joint_index);
}

fn dynamics_mesh_cloth_assist_seed_lane(weights: &[f32; 4], profile: &DynamicsMeshClothAssistVertexProfile) -> Option<usize> {
	(0..4).find(|&lane| weights[lane].abs() <= 0.000001).or_else(|| {
		profile
			.body_lanes()
			.min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
			.and_then(|(lane, weight)| (weight <= 0.02).then_some(lane))
	})
}

fn normalize_dynamics_mesh_cloth_assist_weights(weights: &mut [f32; 4]) {
	let sum = weights
		.iter()
		.copied()
		.filter(|value| value.is_finite() && *value > 0.0)
		.sum::<f32>();
	if sum <= 0.000001 {
		*weights = [1.0, 0.0, 0.0, 0.0];
		return;
	}
	for weight in weights {
		*weight = if weight.is_finite() && *weight > 0.0 { *weight / sum } else { 0.0 };
	}
}

fn normalized_text_contains_any(text: &str, aliases: &[&str]) -> bool {
	aliases.iter().any(|alias| normalized_alias_matches(text, alias))
}

fn finite_or(value: f32, fallback: f32) -> f32 {
	if value.is_finite() {
		value
	} else {
		fallback
	}
}

fn default_spring_bone_simulation_hz() -> f32 {
	60.0
}

fn default_spring_bone_substeps() -> u32 {
	1
}

fn default_dynamics_surface_constraints_enabled() -> bool {
	true
}

fn default_dynamics_surface_constraint_topology_max_edge_distance_m() -> f32 {
	0.06
}

fn default_dynamics_surface_constraint_topology_max_mean_edge_distance_m() -> f32 {
	0.03
}

fn default_dynamics_surface_constraint_spatial_max_distance_m() -> f32 {
	0.012
}

fn default_dynamics_surface_constraint_topology_stiffness() -> f32 {
	0.35
}

fn default_dynamics_surface_constraint_spatial_stiffness() -> f32 {
	0.9
}

fn default_dynamics_surface_constraint_min_edge_count() -> u32 {
	3
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct DynamicsCategoryDefinition {
	pub id: String,
	pub name: String,
	pub matches: Vec<String>,
}

impl Default for DynamicsCategoryDefinition {
	fn default() -> Self {
		Self {
			id: "other".to_string(),
			name: "Other".to_string(),
			matches: Vec::new(),
		}
	}
}

pub type SpringBoneCategoryDefinition = DynamicsCategoryDefinition;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct DynamicsCategoryOverride {
	pub category: String,
	#[serde(flatten)]
	pub params: DynamicsPhysicsParams,
}

pub type SpringBoneCategoryOverride = DynamicsCategoryOverride;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct DynamicsMatchOverride {
	pub name: String,
	#[serde(alias = "sourceId", alias = "source")]
	pub source_id: String,
	#[serde(alias = "sourceIdContains", alias = "source_contains", alias = "contains")]
	pub source_id_contains: Vec<String>,
	#[serde(alias = "sourceIdRegex", alias = "source_regex", alias = "regex")]
	pub source_id_regex: Vec<String>,
	#[serde(flatten)]
	pub params: DynamicsPhysicsParams,
}

impl DynamicsMatchOverride {
	fn normalize(&mut self) {
		self.name = self.name.trim().to_string();
		self.source_id = self.source_id.trim().to_string();
		self.source_id_contains = self
			.source_id_contains
			.iter()
			.map(|value| normalize_match_text(value))
			.filter(|value| !value.is_empty())
			.collect();
		self.source_id_regex = self
			.source_id_regex
			.iter()
			.map(|value| value.trim().to_string())
			.filter(|value| !value.is_empty())
			.collect();
		self.params = self.params.normalized();
	}

	fn has_matcher(&self) -> bool {
		!self.source_id.is_empty() || !self.source_id_contains.is_empty() || !self.source_id_regex.is_empty()
	}
}

pub type SpringBoneMatchOverride = DynamicsMatchOverride;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct DynamicsGroupOverride {
	#[serde(alias = "sourceId", alias = "source")]
	pub source_id: String,
	#[serde(flatten)]
	pub params: DynamicsPhysicsParams,
}

pub type SpringBoneGroupOverride = DynamicsGroupOverride;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct DynamicsColliderAugmentOverride {
	pub name: String,
	#[serde(alias = "sourceIdContains", alias = "source_contains", alias = "contains")]
	pub source_id_contains: Vec<String>,
	#[serde(alias = "colliderPathContains", alias = "collider_contains")]
	pub collider_path_contains: Vec<String>,
}

impl DynamicsColliderAugmentOverride {
	fn normalize(&mut self) {
		self.name = self.name.trim().to_string();
		self.source_id_contains = self
			.source_id_contains
			.iter()
			.map(|value| normalize_match_text(value))
			.filter(|value| !value.is_empty())
			.collect();
		self.collider_path_contains = self
			.collider_path_contains
			.iter()
			.map(|value| normalize_match_text(value))
			.filter(|value| !value.is_empty())
			.collect();
	}

	fn has_matcher(&self) -> bool {
		!self.source_id_contains.is_empty() && !self.collider_path_contains.is_empty()
	}
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct DynamicsPhysicsParams {
	pub solver: Option<DynamicsSolver>,
	pub damping_half_life_ms: Option<f32>,
	pub rest_response: Option<f32>,
	/// Legacy profile key. v2.1 treats this as an alias for `rest_response`.
	pub stiffness_hz: Option<f32>,
	pub shape_preservation: Option<f32>,
	pub bounce_scale: Option<f32>,
	pub stretch_range_scale: Option<f32>,
	pub stretch_motion: Option<f32>,
	pub xpbd_compliance: Option<f32>,
	pub gravity_scale: Option<f32>,
	pub motion_coupling: Option<f32>,
	pub drag_scale: Option<f32>,
	pub constraint_iterations: Option<u32>,
}

pub type SpringBonePhysicsParams = DynamicsPhysicsParams;

impl DynamicsPhysicsParams {
	fn normalized(mut self) -> Self {
		self.damping_half_life_ms = self
			.damping_half_life_ms
			.and_then(|v| v.is_finite().then_some(v.clamp(1.0, 10_000.0)));
		self.rest_response = self.rest_response.and_then(|v| v.is_finite().then_some(v.clamp(0.0, 1.0)));
		self.stiffness_hz = self.stiffness_hz.and_then(|v| v.is_finite().then_some(v.clamp(0.0, 60.0)));
		self.shape_preservation = self.shape_preservation.and_then(|v| v.is_finite().then_some(v.clamp(0.0, 1.0)));
		self.bounce_scale = self.bounce_scale.and_then(|v| v.is_finite().then_some(v.clamp(0.0, 4.0)));
		self.stretch_range_scale = self.stretch_range_scale.and_then(|v| v.is_finite().then_some(v.clamp(0.0, 4.0)));
		self.stretch_motion = self.stretch_motion.and_then(|v| v.is_finite().then_some(v.clamp(0.0, 1.0)));
		self.xpbd_compliance = self.xpbd_compliance.and_then(|v| v.is_finite().then_some(v.clamp(0.0, 10.0)));
		self.gravity_scale = self.gravity_scale.and_then(|v| v.is_finite().then_some(v.clamp(0.0, 10.0)));
		self.motion_coupling = self.motion_coupling.and_then(|v| v.is_finite().then_some(v.clamp(0.0, 1.0)));
		self.drag_scale = self.drag_scale.and_then(|v| v.is_finite().then_some(v.clamp(0.0, 10.0)));
		self.constraint_iterations = self.constraint_iterations.map(|v| v.clamp(1, 32));
		self
	}

	fn rest_response_override(self) -> Option<f32> {
		self.rest_response.or_else(|| {
			self.stiffness_hz
				.map(|legacy| if legacy <= 1.0 { legacy } else { (legacy / 60.0).clamp(0.0, 1.0) })
		})
	}

	fn merge(self, override_params: Self) -> Self {
		Self {
			solver: override_params.solver.or(self.solver),
			damping_half_life_ms: override_params.damping_half_life_ms.or(self.damping_half_life_ms),
			rest_response: override_params.rest_response.or(self.rest_response),
			stiffness_hz: override_params.stiffness_hz.or(self.stiffness_hz),
			shape_preservation: override_params.shape_preservation.or(self.shape_preservation),
			bounce_scale: override_params.bounce_scale.or(self.bounce_scale),
			stretch_range_scale: override_params.stretch_range_scale.or(self.stretch_range_scale),
			stretch_motion: override_params.stretch_motion.or(self.stretch_motion),
			xpbd_compliance: override_params.xpbd_compliance.or(self.xpbd_compliance),
			gravity_scale: override_params.gravity_scale.or(self.gravity_scale),
			motion_coupling: override_params.motion_coupling.or(self.motion_coupling),
			drag_scale: override_params.drag_scale.or(self.drag_scale),
			constraint_iterations: override_params.constraint_iterations.or(self.constraint_iterations),
		}
	}
}

fn default_spring_bone_categories() -> Vec<DynamicsCategoryDefinition> {
	vec![
		DynamicsCategoryDefinition {
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
		DynamicsCategoryDefinition {
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
		DynamicsCategoryDefinition {
			id: "tail".to_string(),
			name: "Tail".to_string(),
			matches: vec!["tail".into(), "尻尾".into(), "しっぽ".into()],
		},
		DynamicsCategoryDefinition {
			id: "cloth".to_string(),
			name: "Cloth".to_string(),
			matches: vec![
				"cloth".into(),
				"skirt".into(),
				"sleeve".into(),
				"cape".into(),
				"shirt".into(),
				"sweater".into(),
				"blouse".into(),
				"dress".into(),
				"coat".into(),
				"longcoat".into(),
				"frill".into(),
				"frills".into(),
				"stocking".into(),
				"stockings".into(),
				"布".into(),
				"スカート".into(),
				"袖".into(),
				"ケープ".into(),
				"シャツ".into(),
				"セーター".into(),
				"ブラウス".into(),
				"ドレス".into(),
				"コート".into(),
				"フリル".into(),
				"靴下".into(),
				"ストッキング".into(),
			],
		},
		DynamicsCategoryDefinition {
			id: "accessory".to_string(),
			name: "Accessory".to_string(),
			matches: vec![
				"accessory".into(),
				"ornament".into(),
				"chain".into(),
				"cord".into(),
				"ribbon".into(),
				"accessories".into(),
				"bag".into(),
				"bookbag".into(),
				"earring".into(),
				"earrings".into(),
				"earringroot".into(),
				"earring_root".into(),
				"shoe".into(),
				"shoes".into(),
				"maryjane".into(),
				"mary_jane".into(),
				"footwear".into(),
				"boot".into(),
				"boots".into(),
				"watch".into(),
				"pocket_watch".into(),
				"brooch".into(),
				"broach".into(),
				"hat".into(),
				"hatroot".into(),
				"hat_root".into(),
				"tie".into(),
				"tieroot".into(),
				"tie_root".into(),
				"bowroot".into(),
				"bow_root".into(),
				"bow_tie".into(),
				"bowties".into(),
				"necklace".into(),
				"potion".into(),
				"bottle".into(),
				"cable".into(),
				"nervecable".into(),
				"strings".into(),
				"装飾".into(),
				"アクセサリ".into(),
				"飾り".into(),
				"リボン".into(),
				"鞄".into(),
				"バッグ".into(),
				"時計".into(),
				"ブローチ".into(),
				"靴".into(),
				"ブーツ".into(),
				"帽子".into(),
				"ネクタイ".into(),
				"蝶ネクタイ".into(),
				"首飾り".into(),
				"ネックレス".into(),
				"瓶".into(),
				"ボトル".into(),
				"ケーブル".into(),
				"紐".into(),
			],
		},
		DynamicsCategoryDefinition {
			id: "soft_body".to_string(),
			name: "Soft Body".to_string(),
			matches: vec![
				"breast".into(),
				"bust".into(),
				"butt".into(),
				"cheek".into(),
				"胸".into(),
				"尻".into(),
				"お尻".into(),
				"頬".into(),
			],
		},
		DynamicsCategoryDefinition {
			id: "other".to_string(),
			name: "Other".to_string(),
			matches: Vec::new(),
		},
	]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TailTranslationWritebackTarget {
	ChildNode,
	NextChainNode { node: usize },
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
	/// Number of simulated joints in the source chain. Used by UNPhysics to distinguish
	/// distributed long-chain deflection from a genuinely small local sway.
	chain_joint_count: usize,
	/// Collider radius used for this joint tail.
	hit_radius: f32,
	rest_response: f32,
	shape_preservation: f32,
	bounce_response: f32,
	max_stretch_response: f32,
	max_squish_response: f32,
	stretch_motion_response: f32,
	damping_half_life_ms: Option<f32>,
	drag_force: f32,
	gravity_power: f32,
	gravity_falloff: f32,
	immobile: f32,
	parent_motion_follow: f32,
	motion_frame_node: Option<usize>,
	/// Whether this joint may later use translation writeback without moving a skinned deformation joint.
	translation_writeback_allowed: bool,
	/// The local translation target represented by this tail particle, if stretch writeback is safe.
	translation_writeback_target: Option<TailTranslationWritebackTarget>,
	/// 動的: 現在フレームの tail (world)。
	curr_tail: Vec3,
	/// 動的: 前フレームの tail (world)。`curr - prev` が Verlet 速度。
	prev_tail: Vec3,
	/// Last UNPhysics response velocity term.
	prev_velocity: Vec3,
	/// 前回 step の child head 位置。親/center motion を tail 状態へ移す基準。
	last_child_pos: Vec3,
	/// 前回 step の rest target world rotation。親回転 motion を tail 状態へ移す基準。
	last_target_rotation: Quat,
	/// 前回 step の UNPhysics motion frame。center がある group は center、無ければ child frame。
	last_motion_pos: Vec3,
	last_motion_rotation: Quat,
	/// 動的: XPBD rest-pose constraint の累積 Lagrange multiplier。
	rest_lambda: f32,
}

/// 1 チェーン分のランタイム状態。
struct GroupRuntime {
	dynamics_group_index: usize,
	source_id: String,
	category: String,
	matched_overrides: Vec<String>,
	group_override_applied: bool,
	invalid_match_regexes: Vec<String>,
	joints: Vec<JointRuntime>,
	params: ResolvedDynamicsPhysicsParams,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RuntimeJointHandle {
	runtime_index: usize,
	joint_index: usize,
}

#[derive(Clone, Debug, PartialEq)]
struct RuntimeSurfaceConstraint {
	a: RuntimeJointHandle,
	b: RuntimeJointHandle,
	rest_distance: f32,
	stiffness: f32,
}

#[derive(Clone, Copy)]
enum WorldColliderSelection<'a> {
	All {
		colliders: &'a [WorldBoneColliderPrimitive],
		paths: &'a [String],
	},
	Selected {
		colliders: &'a [WorldBoneColliderPrimitive],
		paths: &'a [String],
		indices: &'a [usize],
	},
}

impl<'a> WorldColliderSelection<'a> {
	fn new(colliders: &'a [WorldBoneColliderPrimitive], paths: &'a [String], all_global: bool, selected_indices: &'a [usize]) -> Self {
		if all_global {
			Self::All { colliders, paths }
		} else {
			Self::Selected {
				colliders,
				paths,
				indices: selected_indices,
			}
		}
	}

	fn is_empty(self) -> bool {
		match self {
			Self::All { colliders, .. } => colliders.is_empty(),
			Self::Selected { indices, .. } => indices.is_empty(),
		}
	}

	fn push_out(self, mut point: Vec3, extra_radius: f32) -> Vec3 {
		let extra_radius = extra_radius.max(0.0);
		match self {
			Self::All { colliders, .. } => {
				for &collider in colliders {
					point = push_out_of_world_collider(point, collider, extra_radius);
				}
			}
			Self::Selected { colliders, indices, .. } => {
				for &index in indices {
					if let Some(&collider) = colliders.get(index) {
						point = push_out_of_world_collider(point, collider, extra_radius);
					}
				}
			}
		}
		point
	}

	fn projected_path(self, mut point: Vec3, extra_radius: f32) -> Option<&'a str> {
		let extra_radius = extra_radius.max(0.0);
		let mut projected_path = None;
		match self {
			Self::All { colliders, paths } => {
				for (index, &collider) in colliders.iter().enumerate() {
					let before = point;
					point = push_out_of_world_collider(point, collider, extra_radius);
					if (point - before).length_squared() > 1e-12 {
						projected_path = paths.get(index).map(String::as_str).filter(|path| !path.is_empty());
					}
				}
			}
			Self::Selected { colliders, paths, indices } => {
				for &index in indices {
					let Some(&collider) = colliders.get(index) else {
						continue;
					};
					let before = point;
					point = push_out_of_world_collider(point, collider, extra_radius);
					if (point - before).length_squared() > 1e-12 {
						projected_path = paths.get(index).map(String::as_str).filter(|path| !path.is_empty());
					}
				}
			}
		}
		projected_path
	}
}

impl GroupRuntime {
	fn reset_xpbd_lambdas(&mut self) {
		for joint in &mut self.joints {
			joint.rest_lambda = 0.0;
		}
	}
}

fn build_runtime_surface_constraints(
	constraints: &[DynamicsSurfaceConstraint],
	runtime_joint_by_child_node: &[Option<RuntimeJointHandle>],
) -> Vec<RuntimeSurfaceConstraint> {
	constraints
		.iter()
		.filter_map(|constraint| {
			let a = runtime_joint_by_child_node.get(constraint.node_a).copied().flatten()?;
			let b = runtime_joint_by_child_node.get(constraint.node_b).copied().flatten()?;
			if a == b {
				return None;
			}
			let rest_distance = constraint.rest_distance;
			if !rest_distance.is_finite() || rest_distance <= 1e-5 {
				return None;
			}
			let stiffness = constraint.stiffness;
			if !stiffness.is_finite() || stiffness <= 0.0 {
				return None;
			}
			Some(RuntimeSurfaceConstraint {
				a,
				b,
				rest_distance,
				stiffness: stiffness.clamp(0.0, 1.0),
			})
		})
		.collect()
}

fn surface_constraint_runtime_indices(constraints: &[RuntimeSurfaceConstraint]) -> Vec<usize> {
	let mut indices = Vec::with_capacity(constraints.len().saturating_mul(2));
	for constraint in constraints {
		indices.push(constraint.a.runtime_index);
		indices.push(constraint.b.runtime_index);
	}
	sort_dedup(&mut indices);
	indices
}

fn sort_dedup<T: Ord>(values: &mut Vec<T>) {
	values.sort_unstable();
	values.dedup();
}

fn visible_mesh_used_skin_joint_indices(scene: &UnaSceneSnapshot, mesh_index: usize) -> Vec<usize> {
	let Some(primitives) = scene.meshes.get(mesh_index) else {
		return Vec::new();
	};
	let mut out = Vec::new();
	for primitive in primitives {
		let (Some(joints), Some(weights)) = (&primitive.joints, &primitive.weights) else {
			continue;
		};
		for (joint_indices, joint_weights) in joints.iter().zip(weights.iter()) {
			for (&joint_index, &weight) in joint_indices.iter().zip(joint_weights.iter()) {
				if weight > 1.0e-5 {
					out.push(joint_index as usize);
				}
			}
		}
	}
	sort_dedup(&mut out);
	out
}

/// 全グループのランタイム状態。
pub struct DynamicsSimulator {
	runtimes: Vec<Option<GroupRuntime>>,
	active_runtime_indices: Vec<usize>,
	active_verlet_runtime_indices: Vec<usize>,
	active_xpbd_runtime_indices: Vec<usize>,
	surface_constraints: Vec<RuntimeSurfaceConstraint>,
	surface_constraint_runtime_indices: Vec<usize>,
	world_scratch: Vec<Mat4>,
	/// 実時間 dt を蓄積し、`FIXED_DT` 単位の離散ステップに変換するアキュムレータ。
	accumulator: f32,
	bone_colliders: Vec<BoneColliderPrimitive>,
	bone_collider_source_ids: Vec<String>,
	all_bone_colliders_global: bool,
	bone_collider_paths: Vec<String>,
	runtime_collider_indices: Vec<Vec<usize>>,
	world_colliders: Vec<WorldBoneColliderPrimitive>,
	post_surface_projections: Vec<(RuntimeJointHandle, Vec3)>,
	physics: DynamicsPhysicsConfig,
	tuning_warnings: Vec<String>,
}

/// Source-neutral v2 dynamics simulator name.
///
/// The implementation still reuses the v1 SpringBone solver assets, but runtime input flows
/// through `UnaRuntimeDynamics` / `UnaDynamicsGroup` rather than source-format SpringBone data.
pub type SpringBoneSimulator = DynamicsSimulator;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DynamicsSurfaceConstraint {
	pub node_a: usize,
	pub node_b: usize,
	pub rest_distance: f32,
	pub stiffness: f32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct DynamicsResponseCategorySummary {
	pub category: String,
	pub group_count: usize,
	pub joint_count: usize,
	pub visual_target_group_count: usize,
	pub nonvisual_group_count: usize,
	pub visible_skinned_joint_count: usize,
	pub visible_mesh_subtree_node_count: usize,
	pub matched_override_group_count: usize,
	pub group_override_group_count: usize,
	pub xpbd_group_count: usize,
	pub average_rest_response: f32,
	pub min_rest_response: f32,
	pub max_rest_response: f32,
	pub average_pull: f32,
	pub average_stiffness: f32,
	pub average_shape_preservation: f32,
	pub min_shape_preservation: f32,
	pub max_shape_preservation: f32,
	pub average_bounce_response: f32,
	pub min_bounce_response: f32,
	pub max_bounce_response: f32,
	pub average_max_stretch_response: f32,
	pub min_max_stretch_response: f32,
	pub max_max_stretch_response: f32,
	pub average_max_squish_response: f32,
	pub min_max_squish_response: f32,
	pub max_max_squish_response: f32,
	pub average_stretch_motion_response: f32,
	pub min_stretch_motion_response: f32,
	pub max_stretch_motion_response: f32,
	pub average_spring: f32,
	pub average_drag_force: f32,
	pub average_gravity_power: f32,
	pub min_gravity_power: f32,
	pub max_gravity_power: f32,
	pub average_gravity_falloff: f32,
	pub average_immobile: f32,
	pub min_immobile: f32,
	pub max_immobile: f32,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub average_damping_half_life_ms: Option<f32>,
	pub average_parent_motion_follow: f32,
	pub min_parent_motion_follow: f32,
	pub max_parent_motion_follow: f32,
	pub average_orientation_follow: f32,
	pub average_xpbd_compliance: f32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct DynamicsResponseGroupSummary {
	pub runtime_index: usize,
	pub dynamics_group_index: usize,
	pub source_id: String,
	pub category: String,
	pub matched_overrides: Vec<String>,
	pub group_override_applied: bool,
	pub invalid_match_regexes: Vec<String>,
	pub joint_count: usize,
	pub visual_target: bool,
	pub skinned_joint_count: usize,
	pub mesh_subtree_node_count: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub root_node: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub tip_node: Option<usize>,
	pub solver: DynamicsSolver,
	pub average_rest_response: f32,
	pub min_rest_response: f32,
	pub max_rest_response: f32,
	pub average_pull: f32,
	pub average_stiffness: f32,
	pub average_shape_preservation: f32,
	pub min_shape_preservation: f32,
	pub max_shape_preservation: f32,
	pub average_bounce_response: f32,
	pub min_bounce_response: f32,
	pub max_bounce_response: f32,
	pub average_max_stretch_response: f32,
	pub min_max_stretch_response: f32,
	pub max_max_stretch_response: f32,
	pub average_max_squish_response: f32,
	pub min_max_squish_response: f32,
	pub max_max_squish_response: f32,
	pub average_stretch_motion_response: f32,
	pub min_stretch_motion_response: f32,
	pub max_stretch_motion_response: f32,
	pub average_spring: f32,
	pub average_drag_force: f32,
	pub average_gravity_power: f32,
	pub min_gravity_power: f32,
	pub max_gravity_power: f32,
	pub average_gravity_falloff: f32,
	pub average_immobile: f32,
	pub min_immobile: f32,
	pub max_immobile: f32,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub average_damping_half_life_ms: Option<f32>,
	pub average_parent_motion_follow: f32,
	pub min_parent_motion_follow: f32,
	pub max_parent_motion_follow: f32,
	pub average_orientation_follow: f32,
	pub xpbd_compliance: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DynamicsVisualTargetContext {
	visible_skinned_joint_nodes: Vec<usize>,
	visible_mesh_nodes: Vec<usize>,
	parent_by_node: Vec<Option<usize>>,
}

impl DynamicsVisualTargetContext {
	pub fn for_scene(scene: &UnaSceneSnapshot) -> Self {
		let mut parent_by_node = vec![None; scene.nodes.len()];
		for (parent_index, node) in scene.nodes.iter().enumerate() {
			for &child_index in &node.children {
				if child_index < parent_by_node.len() && parent_by_node[child_index].is_none() {
					parent_by_node[child_index] = Some(parent_index);
				}
			}
		}
		let mut visible_skinned_joint_nodes = Vec::new();
		let mut visible_mesh_nodes = Vec::new();
		let mut visited = vec![false; scene.nodes.len()];
		for &root in scene.resolved_roots().iter() {
			visit_visual_target_scene(
				scene,
				root,
				true,
				&mut visited,
				&mut visible_mesh_nodes,
				&mut visible_skinned_joint_nodes,
			);
		}
		for node_index in 0..scene.nodes.len() {
			if !visited[node_index] && parent_by_node[node_index].is_none() {
				visit_visual_target_scene(
					scene,
					node_index,
					true,
					&mut visited,
					&mut visible_mesh_nodes,
					&mut visible_skinned_joint_nodes,
				);
			}
		}
		sort_dedup(&mut visible_skinned_joint_nodes);
		Self {
			visible_skinned_joint_nodes,
			visible_mesh_nodes,
			parent_by_node,
		}
	}

	pub fn group_counts(&self, bone_node_indices: &[usize]) -> (usize, usize) {
		let skinned_joint_count = bone_node_indices
			.iter()
			.enumerate()
			.filter(|(index, node)| !bone_node_indices[..*index].contains(node))
			.map(|(_, node)| node)
			.filter(|node| self.visible_skinned_joint_nodes.binary_search(node).is_ok())
			.count();
		let mut mesh_subtree_node_count = 0usize;
		for &mesh_node in &self.visible_mesh_nodes {
			let mut cursor = Some(mesh_node);
			while let Some(node) = cursor {
				if bone_node_indices.contains(&node) {
					mesh_subtree_node_count += 1;
					break;
				}
				cursor = self.parent_by_node.get(node).copied().flatten();
			}
		}
		(skinned_joint_count, mesh_subtree_node_count)
	}
}

pub fn annotate_dynamics_response_group_visibility(
	groups: &mut [DynamicsResponseGroupSummary],
	scene: &UnaSceneSnapshot,
	runtime_dynamics: un_avatar_core::UnaRuntimeDynamics<'_>,
) {
	let visual_target_context = DynamicsVisualTargetContext::for_scene(scene);
	let mut matched_groups = vec![false; groups.len()];
	for group in runtime_dynamics
		.dynamics_groups()
		.filter(|group| group.effective_enabled && runtime_dynamics.source_id_resident_in_scene(scene, group.source_id))
	{
		let (skinned_joint_count, mesh_subtree_node_count) = visual_target_context.group_counts(group.chain.bone_node_indices);
		if let Some((summary_index, summary)) = groups
			.iter_mut()
			.enumerate()
			.find(|(index, summary)| !matched_groups[*index] && summary.source_id == group.source_id)
		{
			matched_groups[summary_index] = true;
			summary.visual_target = skinned_joint_count > 0 || mesh_subtree_node_count > 0;
			summary.skinned_joint_count = skinned_joint_count;
			summary.mesh_subtree_node_count = mesh_subtree_node_count;
		}
	}
}

fn visit_visual_target_scene(
	scene: &UnaSceneSnapshot,
	node_index: usize,
	inherited_visible: bool,
	visited: &mut [bool],
	visible_mesh_nodes: &mut Vec<usize>,
	visible_skinned_joint_nodes: &mut Vec<usize>,
) {
	let Some(node) = scene.nodes.get(node_index) else {
		return;
	};
	if visited.get(node_index).copied().unwrap_or(false) {
		return;
	}
	visited[node_index] = true;
	let visible = inherited_visible && node.visible;
	if visible && node.mesh.is_some() {
		visible_mesh_nodes.push(node_index);
		if let (Some(mesh_index), Some(skin)) = (node.mesh, node.skin.and_then(|skin_index| scene.skins.get(skin_index))) {
			for joint_index in visible_mesh_used_skin_joint_indices(scene, mesh_index) {
				if let Some(&joint_node) = skin.joint_nodes.get(joint_index) {
					visible_skinned_joint_nodes.push(joint_node);
				}
			}
		}
	}
	for &child in &node.children {
		visit_visual_target_scene(scene, child, visible, visited, visible_mesh_nodes, visible_skinned_joint_nodes);
	}
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct DynamicsTailSample {
	pub source_id: String,
	pub runtime_index: usize,
	pub joint_index: usize,
	pub parent_node: usize,
	pub child_node: usize,
	pub anchor_pos: [f32; 3],
	pub curr_tail: [f32; 3],
	pub prev_tail: [f32; 3],
	pub rest_axis_world: [f32; 3],
	pub current_axis_world: [f32; 3],
	pub axis_angle_deg: f32,
	pub prev_velocity: [f32; 3],
	pub length: f32,
	pub hit_radius: f32,
	pub translation_writeback_target: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct DynamicsColliderSelectionSummary {
	pub source_id: String,
	pub selected_collider_count: usize,
	pub global_collider_count: usize,
	pub authored_collider_count: usize,
	pub sample_collider_indices: Vec<usize>,
	pub sample_collider_source_ids: Vec<String>,
	pub sample_collider_paths: Vec<String>,
}

/// 1 フレームで処理する最大蓄積時間 (秒)。スパイラル・オブ・デス防止。
const MAX_ACCUM: f32 = 0.05;

/// 1 フレームでの最大サブステップ反復回数。
const MAX_STEPS_PER_FRAME: u32 = 8;

impl Default for DynamicsSimulator {
	fn default() -> Self {
		Self {
			runtimes: Vec::new(),
			active_runtime_indices: Vec::new(),
			active_verlet_runtime_indices: Vec::new(),
			active_xpbd_runtime_indices: Vec::new(),
			surface_constraints: Vec::new(),
			surface_constraint_runtime_indices: Vec::new(),
			world_scratch: Vec::new(),
			accumulator: 0.0,
			bone_colliders: Vec::new(),
			bone_collider_source_ids: Vec::new(),
			all_bone_colliders_global: true,
			bone_collider_paths: Vec::new(),
			runtime_collider_indices: Vec::new(),
			world_colliders: Vec::new(),
			post_surface_projections: Vec::new(),
			physics: DynamicsPhysicsConfig::default().normalized(),
			tuning_warnings: Vec::new(),
		}
	}
}

#[derive(Clone, Copy, Debug)]
struct ResolvedDynamicsPhysicsParams {
	solver: DynamicsSolver,
	damping_half_life_ms: Option<f32>,
	rest_response_override: Option<f32>,
	shape_preservation_override: Option<f32>,
	bounce_scale: f32,
	source_shape_preservation_scale: f32,
	source_rest_response_scale: f32,
	source_bounce_response_scale: f32,
	source_motion_coupling_scale: f32,
	shape_preservation: f32,
	rest_response: f32,
	bounce_response: f32,
	xpbd_compliance: f32,
	stretch_range_scale: f32,
	stretch_motion_override: Option<f32>,
	gravity_scale: f32,
	gravity_falloff: f32,
	immobile: f32,
	immobile_type: UnaDynamicsImmobileType,
	motion_coupling_override: Option<f32>,
	drag_scale: f32,
	constraint_iterations: u32,
}

#[derive(Clone, Copy, Debug)]
struct DynamicsChainResponseScale {
	pull_scale: f32,
	stiffness_scale: f32,
	spring_scale: f32,
	motion_coupling_scale: f32,
}

impl Default for DynamicsChainResponseScale {
	fn default() -> Self {
		Self {
			pull_scale: 1.0,
			stiffness_scale: 1.0,
			spring_scale: 1.0,
			motion_coupling_scale: 1.0,
		}
	}
}

fn dynamics_chain_response_scale(joint_count: usize) -> DynamicsChainResponseScale {
	let extra = joint_count.saturating_sub(2) as f32;
	if extra <= 0.0 {
		return DynamicsChainResponseScale::default();
	}
	let extra = extra.min(5.0);
	DynamicsChainResponseScale {
		pull_scale: (1.0 - 0.055 * extra).max(0.68),
		stiffness_scale: (1.0 - 0.10 * extra).max(0.50),
		spring_scale: (1.0 + 0.025 * extra).min(1.12),
		motion_coupling_scale: (1.0 - 0.06 * extra).max(0.65),
	}
}

#[derive(Clone, Copy, Debug)]
struct DynamicsJointPositionResponseScale {
	rest_response_scale: f32,
	shape_preservation_scale: f32,
	bounce_response_scale: f32,
	motion_coupling_scale: f32,
}

impl Default for DynamicsJointPositionResponseScale {
	fn default() -> Self {
		Self {
			rest_response_scale: 1.0,
			shape_preservation_scale: 1.0,
			bounce_response_scale: 1.0,
			motion_coupling_scale: 1.0,
		}
	}
}

fn dynamics_joint_position_response_scale(joint_index: usize, joint_count: usize) -> DynamicsJointPositionResponseScale {
	if joint_count <= 1 {
		return DynamicsJointPositionResponseScale::default();
	}
	let t = (joint_index as f32 / (joint_count - 1) as f32).clamp(0.0, 1.0);
	let lerp = |root: f32, tip: f32| root + (tip - root) * t;
	DynamicsJointPositionResponseScale {
		rest_response_scale: lerp(1.04, 0.86),
		shape_preservation_scale: lerp(1.06, 0.68),
		bounce_response_scale: lerp(0.92, 1.14),
		motion_coupling_scale: lerp(1.04, 0.82),
	}
}

#[derive(Clone, Copy, Debug)]
struct ConvertedDynamicsPhysicsParams {
	shape_preservation: f32,
	rest_response: f32,
	bounce_response: f32,
	xpbd_compliance: f32,
	gravity_scale: f32,
	gravity_falloff: f32,
	immobile: f32,
	immobile_type: UnaDynamicsImmobileType,
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
	let mut prev_was_separator = true;
	let mut prev_was_lower_or_digit = false;
	for ch in value.trim().chars() {
		if ch == '_' || !ch.is_alphanumeric() {
			if !prev_was_separator && !out.is_empty() {
				out.push('_');
			}
			prev_was_separator = true;
			prev_was_lower_or_digit = false;
			continue;
		}
		let is_upper = ch.is_uppercase();
		if is_upper && prev_was_lower_or_digit && !prev_was_separator {
			out.push('_');
		}
		for lower in ch.to_lowercase() {
			out.push(lower);
		}
		prev_was_separator = false;
		prev_was_lower_or_digit = ch.is_lowercase() || ch.is_numeric();
	}
	while out.ends_with('_') {
		out.pop();
	}
	out
}

fn compact_match_text(value: &str) -> String {
	value.chars().filter(|ch| *ch != '_').collect()
}

fn source_id_leaf(value: &str) -> &str {
	value.rsplit(['/', ':']).next().unwrap_or(value)
}

pub fn dynamics_group_match_text(scene: &UnaSceneSnapshot, group: UnaDynamicsGroup<'_>) -> String {
	let mut text = normalize_match_text(group.source_id);
	if !group.comment.is_empty() {
		text.push(' ');
		text.push_str(&normalize_match_text(group.comment));
	}
	for &node_index in group.chain.bone_node_indices {
		if let Some(name) = scene.nodes.get(node_index).and_then(|node| node.name.as_deref()) {
			text.push(' ');
			text.push_str(&normalize_match_text(name));
		}
	}
	text
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct DynamicsMatchOverrideInspection {
	applies: bool,
	invalid_regexes: Vec<String>,
}

#[derive(Clone, Debug)]
struct CompiledDynamicsMatchOverride<'a> {
	item: &'a DynamicsMatchOverride,
	label: String,
	regexes: Vec<regex::Regex>,
	invalid_regexes: Vec<String>,
}

impl<'a> CompiledDynamicsMatchOverride<'a> {
	fn new(item: &'a DynamicsMatchOverride) -> Self {
		let label = match_override_label(item);
		let mut regexes = Vec::new();
		let mut invalid_regexes = Vec::new();
		for pattern in &item.source_id_regex {
			match regex::Regex::new(pattern) {
				Ok(regex) => regexes.push(regex),
				Err(err) => invalid_regexes.push(format!("{label}: {err}")),
			}
		}
		Self {
			item,
			label,
			regexes,
			invalid_regexes,
		}
	}

	fn inspect(&self, group: UnaDynamicsGroup<'_>, match_text: &str) -> DynamicsMatchOverrideInspection {
		let mut applies = !self.item.source_id.is_empty() && self.item.source_id == group.source_id;
		if self
			.item
			.source_id_contains
			.iter()
			.any(|needle| dynamics_normalized_token_filter_matches(match_text, needle))
		{
			applies = true;
		}
		if self
			.regexes
			.iter()
			.any(|regex| regex.is_match(group.source_id) || regex.is_match(match_text))
		{
			applies = true;
		}
		DynamicsMatchOverrideInspection {
			applies,
			invalid_regexes: self.invalid_regexes.clone(),
		}
	}
}

fn explicit_contains_match(match_text: &str, needle: &str) -> bool {
	let match_text = normalize_match_text(match_text);
	let needle = normalize_match_text(needle);
	dynamics_normalized_token_filter_matches(&match_text, &needle)
}

fn match_override_label(override_item: &DynamicsMatchOverride) -> String {
	if !override_item.name.is_empty() {
		override_item.name.clone()
	} else if !override_item.source_id.is_empty() {
		override_item.source_id.clone()
	} else if !override_item.source_id_contains.is_empty() {
		format!("contains:{}", override_item.source_id_contains.join(","))
	} else if !override_item.source_id_regex.is_empty() {
		format!("regex:{}", override_item.source_id_regex.join(","))
	} else {
		"unnamed".to_string()
	}
}

fn compile_match_overrides(match_overrides: &[DynamicsMatchOverride]) -> Vec<CompiledDynamicsMatchOverride<'_>> {
	match_overrides.iter().map(CompiledDynamicsMatchOverride::new).collect()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct DynamicsMatchOverrideEvaluation {
	matched_indices: Vec<usize>,
	matched_labels: Vec<String>,
	invalid_regexes: Vec<String>,
}

impl DynamicsMatchOverrideEvaluation {
	fn diagnostics(&self) -> (Vec<String>, Vec<String>) {
		let mut matched_labels = self.matched_labels.clone();
		matched_labels.sort();
		matched_labels.dedup();
		let mut invalid_regexes = self.invalid_regexes.clone();
		invalid_regexes.sort();
		invalid_regexes.dedup();
		(matched_labels, invalid_regexes)
	}
}

fn match_override_evaluation(
	group: UnaDynamicsGroup<'_>,
	match_text: &str,
	match_overrides: &[CompiledDynamicsMatchOverride<'_>],
) -> DynamicsMatchOverrideEvaluation {
	let mut matched = Vec::new();
	let mut invalid_regexes = Vec::new();
	let mut matched_indices = Vec::new();
	for (index, override_item) in match_overrides.iter().enumerate() {
		let inspection = override_item.inspect(group, match_text);
		if inspection.applies {
			matched_indices.push(index);
			matched.push(override_item.label.clone());
		}
		invalid_regexes.extend(inspection.invalid_regexes);
	}
	DynamicsMatchOverrideEvaluation {
		matched_indices,
		matched_labels: matched,
		invalid_regexes,
	}
}

fn category_match_in_text(text: &str, categories: &[DynamicsCategoryDefinition]) -> Option<String> {
	let mut best: Option<(&str, usize)> = None;
	for category in categories {
		for alias in &category.matches {
			if normalized_alias_matches(text, alias) {
				let alias_len = alias.chars().count();
				if best.is_none_or(|(_, best_len)| alias_len > best_len) {
					best = Some((category.id.as_str(), alias_len));
				}
			}
		}
	}
	best.map(|(category, _)| category.to_string())
}

fn normalized_alias_matches(text: &str, alias: &str) -> bool {
	if alias.is_empty() {
		return false;
	}
	let alias_parts = alias.split('_').filter(|part| !part.is_empty()).collect::<Vec<_>>();
	if alias_parts.is_empty() {
		return false;
	}
	let text_parts = text.split(['_', '/', ':', ' ']).filter(|part| !part.is_empty()).collect::<Vec<_>>();
	text_parts
		.windows(alias_parts.len())
		.any(|window| window.iter().copied().eq(alias_parts.iter().copied()))
		|| (alias_parts.len() == 1
			&& text_parts
				.iter()
				.any(|part| normalized_part_matches_alias_with_numeric_suffix(part, alias_parts[0])))
		|| compact_adjacent_parts_match(&text_parts, &compact_match_text(alias))
}

fn compact_adjacent_parts_match(text_parts: &[&str], compact_alias: &str) -> bool {
	if compact_alias.is_empty() {
		return false;
	}
	for start in 0..text_parts.len() {
		let mut combined = String::new();
		for part in &text_parts[start..] {
			combined.push_str(part);
			if combined == compact_alias || normalized_part_matches_alias_with_numeric_suffix(&combined, compact_alias) {
				return true;
			}
			if combined.len() >= compact_alias.len() && !compact_alias.starts_with(&combined) {
				break;
			}
		}
	}
	false
}

fn normalized_part_matches_alias_with_numeric_suffix(part: &str, alias: &str) -> bool {
	let Some(suffix) = part.strip_prefix(alias) else {
		return false;
	};
	!suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit())
}

pub fn classify_dynamics_group_category(
	scene: &UnaSceneSnapshot,
	group: UnaDynamicsGroup<'_>,
	categories: &[DynamicsCategoryDefinition],
) -> String {
	let explicit = normalize_category_id(group.category);
	if !explicit.is_empty() {
		return explicit;
	}
	let mut primary = normalize_match_text(group.comment);
	if !group.source_id.is_empty() {
		primary.push(' ');
		primary.push_str(&normalize_match_text(source_id_leaf(group.source_id)));
	}
	for &node_index in group.chain.bone_node_indices {
		if let Some(name) = scene.nodes.get(node_index).and_then(|node| node.name.as_deref()) {
			primary.push(' ');
			primary.push_str(&normalize_match_text(name));
		}
	}
	if let Some(category) = category_match_in_text(&primary, categories) {
		return category;
	}

	if !group.source_id.is_empty() {
		let source_path = normalize_match_text(group.source_id);
		if let Some(category) = category_match_in_text(&source_path, categories) {
			return category;
		}
	}

	"other".to_string()
}

fn classify_group(scene: &UnaSceneSnapshot, group: UnaDynamicsGroup<'_>, categories: &[DynamicsCategoryDefinition]) -> String {
	classify_dynamics_group_category(scene, group, categories)
}

fn convert_normalized_dynamics_params(group: UnaDynamicsGroup<'_>, solver: DynamicsSolver) -> ConvertedDynamicsPhysicsParams {
	let authored_stiffness = group.parameters.stiffness.max(0.0);
	let pull = if group.parameters.pull.is_finite() && group.parameters.pull > 0.0 {
		group.parameters.pull
	} else {
		authored_stiffness
	};
	ConvertedDynamicsPhysicsParams {
		// The runtime term is UNPhysics rest response. Importers lower each source
		// format's authored restore intent into `pull` before the solver sees it.
		shape_preservation: authored_stiffness,
		rest_response: pull,
		bounce_response: group.parameters.spring.max(0.0),
		xpbd_compliance: convert_unphysics_response_to_xpbd_compliance(pull.max(authored_stiffness)),
		gravity_scale: 1.0,
		gravity_falloff: group.parameters.gravity_falloff.clamp(0.0, 1.0),
		immobile: group.parameters.immobile.clamp(0.0, 1.0),
		immobile_type: group.parameters.immobile_type,
		drag_scale: 1.0,
		constraint_iterations: if matches!(solver, DynamicsSolver::Xpbd) { 4 } else { 1 },
	}
}

fn convert_unphysics_response_to_xpbd_compliance(response: f32) -> f32 {
	if !response.is_finite() || response <= f32::EPSILON {
		return 10.0;
	}
	let effective_hz = (response * 10.0).clamp(0.1, 32.0);
	let omega = std::f32::consts::TAU * effective_hz;
	(1.0 / (omega * omega)).clamp(0.0, 10.0)
}

fn convert_unphysics_rest_response_to_xpbd_compliance(rest_response: f32) -> f32 {
	if !rest_response.is_finite() || rest_response <= f32::EPSILON {
		return 10.0;
	}
	convert_unphysics_response_to_xpbd_compliance(rest_response.clamp(0.0, 1.0))
}

fn resolve_xpbd_compliance(explicit: Option<f32>, rest_response_override: Option<f32>, source_default: f32) -> f32 {
	let rest_response_compliance = rest_response_override.map(convert_unphysics_rest_response_to_xpbd_compliance);
	match (explicit, rest_response_compliance) {
		(Some(explicit), Some(rest_response_compliance)) => explicit.max(rest_response_compliance),
		(Some(explicit), None) => explicit,
		(None, Some(rest_response_compliance)) => rest_response_compliance,
		(None, None) => source_default,
	}
}

fn resolve_group_params(
	category_id: &str,
	group: UnaDynamicsGroup<'_>,
	override_params_by_category: &BTreeMap<String, DynamicsPhysicsParams>,
	match_overrides: &[CompiledDynamicsMatchOverride<'_>],
	match_evaluation: &DynamicsMatchOverrideEvaluation,
	override_params_by_source_id: &BTreeMap<String, DynamicsPhysicsParams>,
) -> ResolvedDynamicsPhysicsParams {
	let params = override_params_by_category
		.get(category_id)
		.copied()
		.unwrap_or_default()
		.merge(
			match_evaluation
				.matched_indices
				.iter()
				.filter_map(|&index| match_overrides.get(index))
				.fold(DynamicsPhysicsParams::default(), |params, override_item| {
					params.merge(override_item.item.params)
				}),
		)
		.merge(override_params_by_source_id.get(group.source_id).copied().unwrap_or_default());
	let solver = params.solver.unwrap_or(DynamicsSolver::Verlet);
	let converted = convert_normalized_dynamics_params(group, solver);
	let joint_count = group.chain.bone_node_indices.len().saturating_sub(1);
	let chain_scale = dynamics_chain_response_scale(joint_count);
	let rest_response_override = params.rest_response_override().map(|value| value.max(0.0));
	let shape_preservation_override = params.shape_preservation.map(|value| value.max(0.0));
	let shape_preservation = shape_preservation_override
		.unwrap_or(converted.shape_preservation * chain_scale.stiffness_scale)
		.max(0.0);
	let rest_response = rest_response_override
		.unwrap_or(converted.rest_response * chain_scale.pull_scale)
		.max(0.0);
	let bounce_scale = params.bounce_scale.unwrap_or(1.0).max(0.0);
	ResolvedDynamicsPhysicsParams {
		solver,
		damping_half_life_ms: params.damping_half_life_ms,
		rest_response_override,
		shape_preservation_override,
		bounce_scale,
		source_shape_preservation_scale: chain_scale.stiffness_scale,
		source_rest_response_scale: chain_scale.pull_scale,
		source_bounce_response_scale: chain_scale.spring_scale,
		source_motion_coupling_scale: chain_scale.motion_coupling_scale,
		shape_preservation,
		rest_response,
		bounce_response: (converted.bounce_response * chain_scale.spring_scale * bounce_scale).max(0.0),
		xpbd_compliance: resolve_xpbd_compliance(params.xpbd_compliance, rest_response_override, converted.xpbd_compliance),
		stretch_range_scale: params.stretch_range_scale.unwrap_or(1.0).max(0.0),
		stretch_motion_override: params.stretch_motion,
		gravity_scale: params.gravity_scale.unwrap_or(converted.gravity_scale),
		gravity_falloff: converted.gravity_falloff,
		immobile: converted.immobile,
		immobile_type: converted.immobile_type,
		motion_coupling_override: params.motion_coupling,
		drag_scale: params.drag_scale.unwrap_or(converted.drag_scale),
		constraint_iterations: params.constraint_iterations.unwrap_or(converted.constraint_iterations).clamp(1, 32),
	}
}

#[derive(Clone, Copy, Debug)]
struct ResolvedJointDynamicsResponse {
	rest_response: f32,
	shape_preservation: f32,
	bounce_response: f32,
	damping_half_life_ms: Option<f32>,
	drag_force: f32,
	gravity_power: f32,
	gravity_falloff: f32,
	immobile: f32,
	parent_motion_follow: f32,
}

fn dynamics_sample(samples: &[f32], joint_index: usize) -> Option<f32> {
	samples.get(joint_index).copied().filter(|value| value.is_finite())
}

fn resolve_joint_response(
	group: UnaDynamicsGroup<'_>,
	params: ResolvedDynamicsPhysicsParams,
	joint_index: usize,
) -> ResolvedJointDynamicsResponse {
	let joint_count = group.chain.bone_node_indices.len().saturating_sub(1);
	let position_scale = dynamics_joint_position_response_scale(joint_index, joint_count);
	let source_stiffness_sample = dynamics_sample(group.chain.stiffness_samples, joint_index);
	let source_pull_sample = dynamics_sample(group.chain.pull_samples, joint_index);
	let legacy_rest_sample_from_stiffness =
		group.parameters.stiffness <= f32::EPSILON && group.chain.pull_samples.is_empty() && source_stiffness_sample.is_some();
	let source_stiffness = if legacy_rest_sample_from_stiffness {
		params.shape_preservation * position_scale.shape_preservation_scale
	} else {
		source_stiffness_sample
			.map(|stiffness| stiffness * params.source_shape_preservation_scale)
			.unwrap_or(params.shape_preservation * position_scale.shape_preservation_scale)
	}
	.max(0.0);
	let source_pull = source_pull_sample
		.or_else(|| legacy_rest_sample_from_stiffness.then_some(source_stiffness_sample).flatten())
		.map(|pull| pull * params.source_rest_response_scale)
		.unwrap_or(params.rest_response * position_scale.rest_response_scale)
		.max(0.0);
	let source_spring = dynamics_sample(group.chain.spring_samples, joint_index)
		.map(|spring| spring.max(0.0) * params.source_bounce_response_scale * params.bounce_scale)
		.unwrap_or(params.bounce_response * position_scale.bounce_response_scale)
		.clamp(0.0, 1.0);
	let shape_preservation = params.shape_preservation_override.unwrap_or(source_stiffness).max(0.0);
	let rest_response = params.rest_response_override.unwrap_or(source_pull).max(0.0);
	let drag_force = (group.parameters.drag_force * params.drag_scale).clamp(0.0, 1.0);
	let gravity_power = dynamics_sample(group.chain.gravity_power_samples, joint_index)
		.unwrap_or(group.parameters.gravity_power)
		.max(0.0)
		* params.gravity_scale;
	let gravity_falloff = dynamics_sample(group.chain.gravity_falloff_samples, joint_index)
		.unwrap_or(params.gravity_falloff)
		.clamp(0.0, 1.0);
	let immobile = dynamics_sample(group.chain.immobile_samples, joint_index)
		.unwrap_or(params.immobile)
		.clamp(0.0, 1.0);
	let source_motion_coupling = match params.immobile_type {
		UnaDynamicsImmobileType::AllMotion => 0.25 + immobile * immobile * 0.65,
		UnaDynamicsImmobileType::World => 0.5,
	};
	let parent_motion_follow = params
		.motion_coupling_override
		.unwrap_or(source_motion_coupling * params.source_motion_coupling_scale * position_scale.motion_coupling_scale)
		.clamp(0.0, 1.0);
	ResolvedJointDynamicsResponse {
		rest_response,
		shape_preservation,
		bounce_response: source_spring,
		damping_half_life_ms: params.damping_half_life_ms,
		drag_force,
		gravity_power,
		gravity_falloff,
		immobile,
		parent_motion_follow,
	}
}

fn tail_translation_writeback_target(
	scene: &UnaSceneSnapshot,
	group: UnaDynamicsGroup<'_>,
	chain: &[usize],
	joint_index: usize,
) -> Option<TailTranslationWritebackTarget> {
	if group.writeback_mode != UnaDynamicsWritebackMode::RotationTranslation {
		return None;
	}
	if joint_index + 2 < chain.len() {
		let anchor = chain[joint_index + 1];
		let target = chain[joint_index + 2];
		if una_dynamics_translation_writeback_candidate_count(scene, group.writeback_mode, &[anchor, target]) > 0 {
			return Some(TailTranslationWritebackTarget::NextChainNode { node: target });
		}
		return None;
	}
	if chain.len() == 2 {
		let anchor = chain[joint_index];
		let target = chain[joint_index + 1];
		if una_dynamics_translation_writeback_candidate_count(scene, group.writeback_mode, &[anchor, target]) > 0 {
			return Some(TailTranslationWritebackTarget::ChildNode);
		}
	}
	None
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

impl DynamicsSimulator {
	pub fn new(scene: &UnaSceneSnapshot, settings: &UnaDynamicsSettings) -> Option<Self> {
		Self::new_with_bone_colliders(scene, settings, Vec::new())
	}

	pub fn new_with_bone_colliders(
		scene: &UnaSceneSnapshot,
		settings: &UnaDynamicsSettings,
		bone_colliders: Vec<BoneColliderPrimitive>,
	) -> Option<Self> {
		Self::new_with_config(scene, settings, bone_colliders, DynamicsPhysicsConfig::default())
	}

	pub fn new_with_config(
		scene: &UnaSceneSnapshot,
		settings: &UnaDynamicsSettings,
		bone_colliders: Vec<BoneColliderPrimitive>,
		physics: DynamicsPhysicsConfig,
	) -> Option<Self> {
		Self::new_with_runtime_dynamics(scene, settings.runtime_dynamics(), bone_colliders, physics)
	}

	pub fn new_with_runtime_dynamics(
		scene: &UnaSceneSnapshot,
		dynamics: UnaRuntimeDynamics<'_>,
		bone_colliders: Vec<BoneColliderPrimitive>,
		physics: DynamicsPhysicsConfig,
	) -> Option<Self> {
		let colliders = bone_colliders
			.into_iter()
			.map(|primitive| RuntimeBoneColliderPrimitive {
				primitive,
				source_id: String::new(),
				collider_path: String::new(),
			})
			.collect();
		Self::new_with_runtime_dynamics_and_collider_sources(scene, dynamics, colliders, physics)
	}

	pub fn new_with_runtime_dynamics_and_collider_sources(
		scene: &UnaSceneSnapshot,
		dynamics: UnaRuntimeDynamics<'_>,
		bone_colliders: Vec<RuntimeBoneColliderPrimitive>,
		physics: DynamicsPhysicsConfig,
	) -> Option<Self> {
		Self::new_with_runtime_dynamics_collider_sources_and_surface_constraints(scene, dynamics, bone_colliders, physics, &[])
	}

	pub fn new_with_runtime_dynamics_collider_sources_and_surface_constraints(
		scene: &UnaSceneSnapshot,
		dynamics: UnaRuntimeDynamics<'_>,
		bone_colliders: Vec<RuntimeBoneColliderPrimitive>,
		physics: DynamicsPhysicsConfig,
		surface_constraints: &[DynamicsSurfaceConstraint],
	) -> Option<Self> {
		let groups = dynamics
			.dynamics_groups()
			.enumerate()
			.filter(|(_, group)| dynamics.source_id_resident_in_scene(scene, group.source_id))
			.collect::<Vec<_>>();
		if groups.is_empty() {
			return None;
		}
		let physics = physics.normalized();
		let world0 = world_from_snapshot(scene);
		let override_params_by_category = merge_category_override_params(&physics.overrides);
		let override_params_by_source_id = merge_group_override_params(&physics.group_overrides);
		let compiled_match_overrides = compile_match_overrides(&physics.match_overrides);
		let mut known_source_ids = groups.iter().map(|(_, group)| group.source_id).collect::<Vec<_>>();
		known_source_ids.sort_unstable();
		known_source_ids.dedup();
		let mut matched_match_indices = Vec::new();
		for (_, group) in groups.iter().copied() {
			let match_text = dynamics_group_match_text(scene, group);
			let match_evaluation = match_override_evaluation(group, &match_text, &compiled_match_overrides);
			matched_match_indices.extend(match_evaluation.matched_indices.iter().copied());
		}
		let mut runtimes: Vec<Option<GroupRuntime>> = Vec::new();
		let mut active_runtime_indices = Vec::new();
		let mut active_verlet_runtime_indices = Vec::new();
		let mut active_xpbd_runtime_indices = Vec::new();
		let mut runtime_joint_by_child_node = vec![None; scene.nodes.len()];
		for (dynamics_group_index, g) in groups.iter().copied() {
			if !g.effective_enabled {
				runtimes.push(None);
				continue;
			}
			let chain = g.chain.bone_node_indices;
			if chain.len() < 2 {
				runtimes.push(None);
				continue;
			}
			let category_id = classify_group(scene, g, &physics.categories);
			let match_text = dynamics_group_match_text(scene, g);
			let match_evaluation = match_override_evaluation(g, &match_text, &compiled_match_overrides);
			let (matched_overrides, invalid_match_regexes) = match_evaluation.diagnostics();
			let group_override_applied = override_params_by_source_id.contains_key(g.source_id);
			let params = resolve_group_params(
				&category_id,
				g,
				&override_params_by_category,
				&compiled_match_overrides,
				&match_evaluation,
				&override_params_by_source_id,
			);
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
				let hit_radius = g
					.chain
					.hit_radius_samples
					.get(i)
					.copied()
					.filter(|value| value.is_finite())
					.unwrap_or(g.parameters.hit_radius)
					.max(0.0);
				let response = resolve_joint_response(g, params, i);
				let mut sampled_limit = sampled_joint_limit(g, i);
				if let Some(limit) = sampled_limit.as_mut() {
					apply_dynamics_limit_overrides(limit, &params);
				}
				let (max_stretch_response, max_squish_response, stretch_motion_response) =
					effective_stretch_response(sampled_limit.as_ref());
				let translation_writeback_allowed =
					una_dynamics_translation_writeback_candidate_count(scene, g.writeback_mode, &[parent, child]) > 0;
				let translation_writeback_target = tail_translation_writeback_target(scene, g, chain, i);
				let (_, parent_rot_raw, parent_pos) = world0[parent].to_scale_rotation_translation();
				let parent_rot = parent_rot_raw.normalize();
				let child_pos = parent_pos + parent_rot * trans;
				let target_rotation = (parent_rot * rot).normalize();
				let is_child_translation_target = translation_writeback_target == Some(TailTranslationWritebackTarget::ChildNode);
				let solver_anchor_pos = if is_child_translation_target { parent_pos } else { child_pos };
				let curr = if is_child_translation_target {
					child_pos
				} else {
					world0[child].transform_point3(Vec3::ZERO) + world0[child].transform_vector3(bone_axis) * length
				};
				let motion_frame_node = g.parameters.center_node.filter(|&node| node < world0.len());
				let (motion_pos, motion_rotation) = motion_frame_from_world(&world0, motion_frame_node, solver_anchor_pos, target_rotation);
				joints.push(JointRuntime {
					parent_node: parent,
					child_node: child,
					rest_local_translation: trans,
					rest_local_rotation: rot,
					rest_local_scale: scale,
					bone_axis,
					length,
					chain_joint_count: chain.len().saturating_sub(1),
					hit_radius,
					rest_response: response.rest_response,
					shape_preservation: response.shape_preservation,
					bounce_response: response.bounce_response,
					max_stretch_response,
					max_squish_response,
					stretch_motion_response,
					damping_half_life_ms: response.damping_half_life_ms,
					drag_force: response.drag_force,
					gravity_power: response.gravity_power,
					gravity_falloff: response.gravity_falloff,
					immobile: response.immobile,
					parent_motion_follow: response.parent_motion_follow,
					motion_frame_node,
					translation_writeback_allowed,
					translation_writeback_target,
					curr_tail: curr,
					prev_tail: curr,
					prev_velocity: Vec3::ZERO,
					last_child_pos: solver_anchor_pos,
					last_target_rotation: target_rotation,
					last_motion_pos: motion_pos,
					last_motion_rotation: motion_rotation,
					rest_lambda: 0.0,
				});
			}
			if !ok || joints.is_empty() {
				runtimes.push(None);
			} else {
				let runtime_index = runtimes.len();
				for (joint_index, joint) in joints.iter().enumerate() {
					if let Some(slot) = runtime_joint_by_child_node.get_mut(joint.child_node) {
						slot.get_or_insert(RuntimeJointHandle {
							runtime_index,
							joint_index,
						});
					}
				}
				active_runtime_indices.push(runtime_index);
				match params.solver {
					DynamicsSolver::Verlet => active_verlet_runtime_indices.push(runtime_index),
					DynamicsSolver::Xpbd => active_xpbd_runtime_indices.push(runtime_index),
				}
				runtimes.push(Some(GroupRuntime {
					dynamics_group_index,
					source_id: g.source_id.to_string(),
					category: category_id,
					matched_overrides,
					group_override_applied,
					invalid_match_regexes,
					joints,
					params,
				}));
			}
		}
		if runtimes.iter().all(|r| r.is_none()) {
			None
		} else {
			let surface_constraints = build_runtime_surface_constraints(surface_constraints, &runtime_joint_by_child_node);
			let surface_constraint_runtime_indices = surface_constraint_runtime_indices(&surface_constraints);
			let tuning_warnings = dynamics_tuning_warnings(
				&compiled_match_overrides,
				&matched_match_indices,
				&physics.group_overrides,
				&known_source_ids,
			);
			let mut resolved_bone_colliders = Vec::with_capacity(bone_colliders.len());
			let mut bone_collider_source_ids = Vec::with_capacity(bone_colliders.len());
			let mut bone_collider_paths = Vec::with_capacity(bone_colliders.len());
			for collider in bone_colliders {
				resolved_bone_colliders.push(collider.primitive);
				bone_collider_source_ids.push(collider.source_id);
				bone_collider_paths.push(collider.collider_path);
			}
			let bone_colliders = resolved_bone_colliders;
			let all_bone_colliders_global = bone_collider_source_ids.iter().all(String::is_empty);
			let mut runtime_collider_indices = vec![Vec::new(); runtimes.len()];
			if !all_bone_colliders_global {
				for (runtime_index, runtime) in runtimes.iter_mut().enumerate() {
					let Some(runtime) = runtime.as_mut() else {
						continue;
					};
					let indices = selected_group_collider_indices(&bone_collider_source_ids, &runtime.source_id);
					runtime_collider_indices[runtime_index] = indices;
				}
			}
			Some(Self {
				runtimes,
				active_runtime_indices,
				active_verlet_runtime_indices,
				active_xpbd_runtime_indices,
				surface_constraints,
				surface_constraint_runtime_indices,
				world_scratch: Vec::new(),
				accumulator: 0.0,
				bone_colliders,
				bone_collider_source_ids,
				all_bone_colliders_global,
				bone_collider_paths,
				runtime_collider_indices,
				world_colliders: Vec::new(),
				post_surface_projections: Vec::new(),
				physics,
				tuning_warnings,
			})
		}
	}

	/// ヒューマノイド等で親の局所姿勢を更新したあと、揺れボーンの回転だけ上書きする。
	///
	/// 実時間 `dt` を蓄積し、設定された fixed timestep 単位の固定サブステップで進める。
	pub fn step(&mut self, scene: &mut UnaSceneSnapshot, settings: &UnaDynamicsSettings, dt: f32) {
		self.step_runtime_dynamics(scene, settings.runtime_dynamics(), dt);
	}

	pub fn step_runtime_dynamics(&mut self, scene: &mut UnaSceneSnapshot, dynamics: UnaRuntimeDynamics<'_>, dt: f32) {
		self.step_runtime_dynamics_inner(scene, dynamics, dt, None);
	}

	pub fn step_runtime_dynamics_profiled(
		&mut self,
		scene: &mut UnaSceneSnapshot,
		dynamics: UnaRuntimeDynamics<'_>,
		dt: f32,
	) -> DynamicsStepProfile {
		let mut profile = DynamicsStepProfile::default();
		self.step_runtime_dynamics_inner(scene, dynamics, dt, Some(&mut profile));
		profile
	}

	fn step_runtime_dynamics_inner(
		&mut self,
		scene: &mut UnaSceneSnapshot,
		dynamics: UnaRuntimeDynamics<'_>,
		dt: f32,
		mut profile: Option<&mut DynamicsStepProfile>,
	) {
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
			let t_world = profile.is_some().then(Instant::now);
			write_world_from_snapshot(scene, &mut self.world_scratch);
			if let (Some(profile), Some(t_world)) = (profile.as_deref_mut(), t_world) {
				profile.world_ms += t_world.elapsed().as_secs_f32() * 1000.0;
			}
			let t_collider = profile.is_some().then(Instant::now);
			resolve_world_colliders(&self.world_scratch, &self.bone_colliders, &mut self.world_colliders);
			if let (Some(profile), Some(t_collider)) = (profile.as_deref_mut(), t_collider) {
				profile.collider_ms += t_collider.elapsed().as_secs_f32() * 1000.0;
			}
			let t_solve = profile.is_some().then(Instant::now);
			for &runtime_index in &self.active_verlet_runtime_indices {
				let Some(dynamics_group_index) = self
					.runtimes
					.get(runtime_index)
					.and_then(Option::as_ref)
					.map(|runtime| runtime.dynamics_group_index)
				else {
					continue;
				};
				let (Some(g), Some(Some(rt))) = (dynamics.dynamics_group(dynamics_group_index), self.runtimes.get_mut(runtime_index))
				else {
					continue;
				};
				if !g.effective_enabled {
					continue;
				}
				let group_world_colliders = WorldColliderSelection::new(
					&self.world_colliders,
					&self.bone_collider_paths,
					self.all_bone_colliders_global,
					self.runtime_collider_indices.get(runtime_index).map(Vec::as_slice).unwrap_or(&[]),
				);
				for _ in 0..substeps {
					step_group_solver::<false>(
						scene,
						g,
						rt,
						&mut self.world_scratch,
						group_world_colliders,
						sub_dt,
						profile.as_deref_mut(),
					);
				}
			}
			for &runtime_index in &self.active_xpbd_runtime_indices {
				let Some(dynamics_group_index) = self
					.runtimes
					.get(runtime_index)
					.and_then(Option::as_ref)
					.map(|runtime| runtime.dynamics_group_index)
				else {
					continue;
				};
				let (Some(g), Some(Some(rt))) = (dynamics.dynamics_group(dynamics_group_index), self.runtimes.get_mut(runtime_index))
				else {
					continue;
				};
				if !g.effective_enabled {
					continue;
				}
				let group_world_colliders = WorldColliderSelection::new(
					&self.world_colliders,
					&self.bone_collider_paths,
					self.all_bone_colliders_global,
					self.runtime_collider_indices.get(runtime_index).map(Vec::as_slice).unwrap_or(&[]),
				);
				for _ in 0..substeps {
					rt.reset_xpbd_lambdas();
					step_group_solver::<true>(
						scene,
						g,
						rt,
						&mut self.world_scratch,
						group_world_colliders,
						sub_dt,
						profile.as_deref_mut(),
					);
				}
			}
			if !self.surface_constraints.is_empty() {
				apply_surface_constraints(scene, &mut self.runtimes, &self.surface_constraints, &mut self.world_scratch);
				let t_collision = profile.is_some().then(Instant::now);
				apply_post_surface_collider_constraints(
					scene,
					&mut self.runtimes,
					&self.surface_constraint_runtime_indices,
					dynamics,
					&mut self.world_scratch,
					&self.world_colliders,
					&self.bone_collider_paths,
					&self.runtime_collider_indices,
					self.all_bone_colliders_global,
					&mut self.post_surface_projections,
				);
				if let (Some(profile), Some(t_collision)) = (profile.as_deref_mut(), t_collision) {
					profile.solve_collision_ms += t_collision.elapsed().as_secs_f32() * 1000.0;
				}
			}
			if let (Some(profile), Some(t_solve)) = (profile.as_deref_mut(), t_solve) {
				profile.solve_ms += t_solve.elapsed().as_secs_f32() * 1000.0;
				profile.fixed_steps = profile.fixed_steps.saturating_add(1);
				profile.active_groups = self.active_runtime_indices.len() as u32;
				profile.active_joints = self.active_joint_count() as u32;
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

	pub fn bone_collider_source_ids(&self) -> &[String] {
		&self.bone_collider_source_ids
	}

	pub fn bone_collider_paths(&self) -> &[String] {
		&self.bone_collider_paths
	}

	pub fn active_group_count(&self) -> usize {
		self.active_runtime_indices.len()
	}

	pub fn active_joint_count(&self) -> usize {
		self.active_runtime_indices
			.iter()
			.filter_map(|&index| self.runtimes.get(index).and_then(Option::as_ref))
			.map(|runtime| runtime.joints.len())
			.sum()
	}

	pub fn surface_constraint_count(&self) -> usize {
		self.surface_constraints.len()
	}

	pub fn tail_samples(&self) -> Vec<DynamicsTailSample> {
		self.active_runtime_indices
			.iter()
			.filter_map(|&runtime_index| {
				self.runtimes
					.get(runtime_index)
					.and_then(Option::as_ref)
					.map(|runtime| (runtime_index, runtime))
			})
			.flat_map(|(runtime_index, runtime)| {
				runtime.joints.iter().enumerate().map(move |(joint_index, joint)| {
					let translation_writeback_target = match joint.translation_writeback_target {
						Some(TailTranslationWritebackTarget::ChildNode) => "child_node".to_string(),
						Some(TailTranslationWritebackTarget::NextChainNode { node }) => format!("next_chain_node:{node}"),
						None => "none".to_string(),
					};
					let rest_axis_world = (joint.last_target_rotation * joint.bone_axis).normalize_or_zero();
					let current_axis_world = (joint.curr_tail - joint.last_child_pos).normalize_or_zero();
					let axis_angle_deg = if rest_axis_world.length_squared() >= 1.0e-12 && current_axis_world.length_squared() >= 1.0e-12 {
						rest_axis_world.angle_between(current_axis_world).to_degrees()
					} else {
						0.0
					};
					DynamicsTailSample {
						source_id: runtime.source_id.clone(),
						runtime_index,
						joint_index,
						parent_node: joint.parent_node,
						child_node: joint.child_node,
						anchor_pos: joint.last_child_pos.to_array(),
						curr_tail: joint.curr_tail.to_array(),
						prev_tail: joint.prev_tail.to_array(),
						rest_axis_world: rest_axis_world.to_array(),
						current_axis_world: current_axis_world.to_array(),
						axis_angle_deg,
						prev_velocity: joint.prev_velocity.to_array(),
						length: joint.length,
						hit_radius: joint.hit_radius,
						translation_writeback_target,
					}
				})
			})
			.collect()
	}

	pub fn collider_selection_summaries(&self) -> Vec<DynamicsColliderSelectionSummary> {
		const SAMPLE_LIMIT: usize = 16;
		self.active_runtime_indices
			.iter()
			.filter_map(|&index| self.runtimes.get(index).and_then(Option::as_ref))
			.map(|runtime| {
				let mut selected_collider_count = 0usize;
				let mut global_collider_count = 0usize;
				let mut authored_collider_count = 0usize;
				let mut sample_collider_indices = Vec::new();
				let mut sample_collider_source_ids = Vec::new();
				let mut sample_collider_paths = Vec::new();
				for (collider_index, source_id) in self.bone_collider_source_ids.iter().enumerate() {
					let selected = self.all_bone_colliders_global
						|| source_id.is_empty()
						|| (!runtime.source_id.is_empty() && source_id == &runtime.source_id);
					if !selected {
						continue;
					}
					selected_collider_count += 1;
					if source_id.is_empty() {
						global_collider_count += 1;
					} else {
						authored_collider_count += 1;
					}
					if sample_collider_indices.len() < SAMPLE_LIMIT {
						sample_collider_indices.push(collider_index);
						sample_collider_source_ids.push(source_id.clone());
						sample_collider_paths.push(
							self.bone_colliders
								.get(collider_index)
								.and_then(|_| self.bone_collider_paths.get(collider_index))
								.cloned()
								.unwrap_or_default(),
						);
					}
				}
				DynamicsColliderSelectionSummary {
					source_id: runtime.source_id.clone(),
					selected_collider_count,
					global_collider_count,
					authored_collider_count,
					sample_collider_indices,
					sample_collider_source_ids,
					sample_collider_paths,
				}
			})
			.collect()
	}

	pub fn response_category_summaries(&self) -> Vec<DynamicsResponseCategorySummary> {
		#[derive(Default)]
		struct Accum {
			group_count: usize,
			joint_count: usize,
			matched_override_group_count: usize,
			group_override_group_count: usize,
			xpbd_group_count: usize,
			xpbd_compliance: f32,
			pull: f32,
			min_pull: f32,
			max_pull: f32,
			shape_preservation: f32,
			min_shape_preservation: f32,
			max_shape_preservation: f32,
			spring: f32,
			min_spring: f32,
			max_spring: f32,
			max_stretch: f32,
			min_max_stretch: f32,
			max_max_stretch: f32,
			max_squish: f32,
			min_max_squish: f32,
			max_max_squish: f32,
			stretch_motion: f32,
			min_stretch_motion: f32,
			max_stretch_motion: f32,
			drag_force: f32,
			gravity_power: f32,
			min_gravity_power: f32,
			max_gravity_power: f32,
			gravity_falloff: f32,
			immobile: f32,
			min_immobile: f32,
			max_immobile: f32,
			damping_half_life_ms: f32,
			damping_half_life_count: usize,
			parent_motion_follow: f32,
			min_parent_motion_follow: f32,
			max_parent_motion_follow: f32,
			orientation_follow: f32,
		}

		impl Accum {
			fn push_joint(&mut self, joint: &JointRuntime) {
				let orientation_follow = joint.shape_preservation * joint.parent_motion_follow;
				if self.joint_count == 0 {
					self.min_pull = joint.rest_response;
					self.max_pull = joint.rest_response;
					self.min_shape_preservation = joint.shape_preservation;
					self.max_shape_preservation = joint.shape_preservation;
					self.min_spring = joint.bounce_response;
					self.max_spring = joint.bounce_response;
					self.min_max_stretch = joint.max_stretch_response;
					self.max_max_stretch = joint.max_stretch_response;
					self.min_max_squish = joint.max_squish_response;
					self.max_max_squish = joint.max_squish_response;
					self.min_stretch_motion = joint.stretch_motion_response;
					self.max_stretch_motion = joint.stretch_motion_response;
					self.min_gravity_power = joint.gravity_power;
					self.max_gravity_power = joint.gravity_power;
					self.min_immobile = joint.immobile;
					self.max_immobile = joint.immobile;
					self.min_parent_motion_follow = joint.parent_motion_follow;
					self.max_parent_motion_follow = joint.parent_motion_follow;
				} else {
					self.min_pull = self.min_pull.min(joint.rest_response);
					self.max_pull = self.max_pull.max(joint.rest_response);
					self.min_shape_preservation = self.min_shape_preservation.min(joint.shape_preservation);
					self.max_shape_preservation = self.max_shape_preservation.max(joint.shape_preservation);
					self.min_spring = self.min_spring.min(joint.bounce_response);
					self.max_spring = self.max_spring.max(joint.bounce_response);
					self.min_max_stretch = self.min_max_stretch.min(joint.max_stretch_response);
					self.max_max_stretch = self.max_max_stretch.max(joint.max_stretch_response);
					self.min_max_squish = self.min_max_squish.min(joint.max_squish_response);
					self.max_max_squish = self.max_max_squish.max(joint.max_squish_response);
					self.min_stretch_motion = self.min_stretch_motion.min(joint.stretch_motion_response);
					self.max_stretch_motion = self.max_stretch_motion.max(joint.stretch_motion_response);
					self.min_gravity_power = self.min_gravity_power.min(joint.gravity_power);
					self.max_gravity_power = self.max_gravity_power.max(joint.gravity_power);
					self.min_immobile = self.min_immobile.min(joint.immobile);
					self.max_immobile = self.max_immobile.max(joint.immobile);
					self.min_parent_motion_follow = self.min_parent_motion_follow.min(joint.parent_motion_follow);
					self.max_parent_motion_follow = self.max_parent_motion_follow.max(joint.parent_motion_follow);
				}
				self.joint_count += 1;
				self.pull += joint.rest_response;
				self.shape_preservation += joint.shape_preservation;
				self.spring += joint.bounce_response;
				self.max_stretch += joint.max_stretch_response;
				self.max_squish += joint.max_squish_response;
				self.stretch_motion += joint.stretch_motion_response;
				self.drag_force += joint.drag_force;
				self.gravity_power += joint.gravity_power;
				self.gravity_falloff += joint.gravity_falloff;
				self.immobile += joint.immobile;
				if let Some(half_life) = joint.damping_half_life_ms {
					self.damping_half_life_ms += half_life;
					self.damping_half_life_count += 1;
				}
				self.parent_motion_follow += joint.parent_motion_follow;
				self.orientation_follow += orientation_follow;
			}
		}

		let mut by_category: BTreeMap<String, Accum> = BTreeMap::new();
		for runtime in self
			.active_runtime_indices
			.iter()
			.filter_map(|&index| self.runtimes.get(index).and_then(Option::as_ref))
		{
			let accum = by_category.entry(runtime.category.clone()).or_default();
			accum.group_count += 1;
			if !runtime.matched_overrides.is_empty() {
				accum.matched_override_group_count += 1;
			}
			if runtime.group_override_applied {
				accum.group_override_group_count += 1;
			}
			if matches!(runtime.params.solver, DynamicsSolver::Xpbd) {
				accum.xpbd_group_count += 1;
				accum.xpbd_compliance += runtime.params.xpbd_compliance;
			}
			for joint in &runtime.joints {
				accum.push_joint(joint);
			}
		}
		by_category
			.into_iter()
			.map(|(category, accum)| {
				let denom = accum.joint_count.max(1) as f32;
				DynamicsResponseCategorySummary {
					category,
					group_count: accum.group_count,
					joint_count: accum.joint_count,
					visual_target_group_count: 0,
					nonvisual_group_count: 0,
					visible_skinned_joint_count: 0,
					visible_mesh_subtree_node_count: 0,
					matched_override_group_count: accum.matched_override_group_count,
					group_override_group_count: accum.group_override_group_count,
					xpbd_group_count: accum.xpbd_group_count,
					average_rest_response: accum.pull / denom,
					min_rest_response: accum.min_pull,
					max_rest_response: accum.max_pull,
					average_pull: accum.pull / denom,
					average_stiffness: accum.shape_preservation / denom,
					average_shape_preservation: accum.shape_preservation / denom,
					min_shape_preservation: accum.min_shape_preservation,
					max_shape_preservation: accum.max_shape_preservation,
					average_bounce_response: accum.spring / denom,
					min_bounce_response: accum.min_spring,
					max_bounce_response: accum.max_spring,
					average_max_stretch_response: accum.max_stretch / denom,
					min_max_stretch_response: accum.min_max_stretch,
					max_max_stretch_response: accum.max_max_stretch,
					average_max_squish_response: accum.max_squish / denom,
					min_max_squish_response: accum.min_max_squish,
					max_max_squish_response: accum.max_max_squish,
					average_stretch_motion_response: accum.stretch_motion / denom,
					min_stretch_motion_response: accum.min_stretch_motion,
					max_stretch_motion_response: accum.max_stretch_motion,
					average_spring: accum.spring / denom,
					average_drag_force: accum.drag_force / denom,
					average_gravity_power: accum.gravity_power / denom,
					min_gravity_power: accum.min_gravity_power,
					max_gravity_power: accum.max_gravity_power,
					average_gravity_falloff: accum.gravity_falloff / denom,
					average_immobile: accum.immobile / denom,
					min_immobile: accum.min_immobile,
					max_immobile: accum.max_immobile,
					average_damping_half_life_ms: (accum.damping_half_life_count > 0)
						.then_some(accum.damping_half_life_ms / accum.damping_half_life_count as f32),
					average_parent_motion_follow: accum.parent_motion_follow / denom,
					min_parent_motion_follow: accum.min_parent_motion_follow,
					max_parent_motion_follow: accum.max_parent_motion_follow,
					average_orientation_follow: accum.orientation_follow / denom,
					average_xpbd_compliance: if accum.xpbd_group_count > 0 {
						accum.xpbd_compliance / accum.xpbd_group_count as f32
					} else {
						0.0
					},
				}
			})
			.collect()
	}

	pub fn response_group_summaries(&self) -> Vec<DynamicsResponseGroupSummary> {
		self.active_runtime_indices
			.iter()
			.filter_map(|&index| self.runtimes.get(index).and_then(Option::as_ref).map(|runtime| (index, runtime)))
			.map(|(runtime_index, runtime)| {
				let mut pull = 0.0;
				let mut min_pull = 0.0;
				let mut max_pull = 0.0;
				let mut shape_preservation = 0.0;
				let mut min_shape_preservation = 0.0;
				let mut max_shape_preservation = 0.0;
				let mut spring = 0.0;
				let mut min_spring = 0.0;
				let mut max_spring = 0.0;
				let mut max_stretch = 0.0;
				let mut min_max_stretch = 0.0;
				let mut max_max_stretch = 0.0;
				let mut max_squish = 0.0;
				let mut min_max_squish = 0.0;
				let mut max_max_squish = 0.0;
				let mut stretch_motion = 0.0;
				let mut min_stretch_motion = 0.0;
				let mut max_stretch_motion = 0.0;
				let mut drag_force = 0.0;
				let mut gravity_power = 0.0;
				let mut min_gravity_power = 0.0;
				let mut max_gravity_power = 0.0;
				let mut gravity_falloff = 0.0;
				let mut immobile = 0.0;
				let mut min_immobile = 0.0;
				let mut max_immobile = 0.0;
				let mut damping_half_life_ms = 0.0;
				let mut damping_half_life_count = 0usize;
				let mut parent_motion_follow = 0.0;
				let mut min_parent_motion_follow = 0.0;
				let mut max_parent_motion_follow = 0.0;
				let mut orientation_follow = 0.0;
				for (i, joint) in runtime.joints.iter().enumerate() {
					if i == 0 {
						min_pull = joint.rest_response;
						max_pull = joint.rest_response;
						min_shape_preservation = joint.shape_preservation;
						max_shape_preservation = joint.shape_preservation;
						min_spring = joint.bounce_response;
						max_spring = joint.bounce_response;
						min_max_stretch = joint.max_stretch_response;
						max_max_stretch = joint.max_stretch_response;
						min_max_squish = joint.max_squish_response;
						max_max_squish = joint.max_squish_response;
						min_stretch_motion = joint.stretch_motion_response;
						max_stretch_motion = joint.stretch_motion_response;
						min_gravity_power = joint.gravity_power;
						max_gravity_power = joint.gravity_power;
						min_immobile = joint.immobile;
						max_immobile = joint.immobile;
						min_parent_motion_follow = joint.parent_motion_follow;
						max_parent_motion_follow = joint.parent_motion_follow;
					} else {
						min_pull = min_pull.min(joint.rest_response);
						max_pull = max_pull.max(joint.rest_response);
						min_shape_preservation = min_shape_preservation.min(joint.shape_preservation);
						max_shape_preservation = max_shape_preservation.max(joint.shape_preservation);
						min_spring = min_spring.min(joint.bounce_response);
						max_spring = max_spring.max(joint.bounce_response);
						min_max_stretch = min_max_stretch.min(joint.max_stretch_response);
						max_max_stretch = max_max_stretch.max(joint.max_stretch_response);
						min_max_squish = min_max_squish.min(joint.max_squish_response);
						max_max_squish = max_max_squish.max(joint.max_squish_response);
						min_stretch_motion = min_stretch_motion.min(joint.stretch_motion_response);
						max_stretch_motion = max_stretch_motion.max(joint.stretch_motion_response);
						min_gravity_power = min_gravity_power.min(joint.gravity_power);
						max_gravity_power = max_gravity_power.max(joint.gravity_power);
						min_immobile = min_immobile.min(joint.immobile);
						max_immobile = max_immobile.max(joint.immobile);
						min_parent_motion_follow = min_parent_motion_follow.min(joint.parent_motion_follow);
						max_parent_motion_follow = max_parent_motion_follow.max(joint.parent_motion_follow);
					}
					pull += joint.rest_response;
					shape_preservation += joint.shape_preservation;
					spring += joint.bounce_response;
					max_stretch += joint.max_stretch_response;
					max_squish += joint.max_squish_response;
					stretch_motion += joint.stretch_motion_response;
					drag_force += joint.drag_force;
					gravity_power += joint.gravity_power;
					gravity_falloff += joint.gravity_falloff;
					immobile += joint.immobile;
					if let Some(half_life) = joint.damping_half_life_ms {
						damping_half_life_ms += half_life;
						damping_half_life_count += 1;
					}
					parent_motion_follow += joint.parent_motion_follow;
					orientation_follow += joint.shape_preservation * joint.parent_motion_follow;
				}
				let denom = runtime.joints.len().max(1) as f32;
				DynamicsResponseGroupSummary {
					runtime_index,
					dynamics_group_index: runtime.dynamics_group_index,
					source_id: runtime.source_id.clone(),
					category: runtime.category.clone(),
					matched_overrides: runtime.matched_overrides.clone(),
					group_override_applied: runtime.group_override_applied,
					invalid_match_regexes: runtime.invalid_match_regexes.clone(),
					joint_count: runtime.joints.len(),
					visual_target: false,
					skinned_joint_count: 0,
					mesh_subtree_node_count: 0,
					root_node: runtime.joints.first().map(|joint| joint.child_node),
					tip_node: runtime.joints.last().map(|joint| joint.child_node),
					solver: runtime.params.solver,
					average_rest_response: pull / denom,
					min_rest_response: min_pull,
					max_rest_response: max_pull,
					average_pull: pull / denom,
					average_stiffness: shape_preservation / denom,
					average_shape_preservation: shape_preservation / denom,
					min_shape_preservation,
					max_shape_preservation,
					average_bounce_response: spring / denom,
					min_bounce_response: min_spring,
					max_bounce_response: max_spring,
					average_max_stretch_response: max_stretch / denom,
					min_max_stretch_response: min_max_stretch,
					max_max_stretch_response: max_max_stretch,
					average_max_squish_response: max_squish / denom,
					min_max_squish_response: min_max_squish,
					max_max_squish_response: max_max_squish,
					average_stretch_motion_response: stretch_motion / denom,
					min_stretch_motion_response: min_stretch_motion,
					max_stretch_motion_response: max_stretch_motion,
					average_spring: spring / denom,
					average_drag_force: drag_force / denom,
					average_gravity_power: gravity_power / denom,
					min_gravity_power,
					max_gravity_power,
					average_gravity_falloff: gravity_falloff / denom,
					average_immobile: immobile / denom,
					min_immobile,
					max_immobile,
					average_damping_half_life_ms: (damping_half_life_count > 0)
						.then_some(damping_half_life_ms / damping_half_life_count as f32),
					average_parent_motion_follow: parent_motion_follow / denom,
					min_parent_motion_follow,
					max_parent_motion_follow,
					average_orientation_follow: orientation_follow / denom,
					xpbd_compliance: if matches!(runtime.params.solver, DynamicsSolver::Xpbd) {
						runtime.params.xpbd_compliance
					} else {
						0.0
					},
				}
			})
			.collect()
	}

	pub fn tuning_warnings(&self) -> &[String] {
		&self.tuning_warnings
	}

	pub fn translation_writeback_candidate_count(&self) -> usize {
		self.runtimes
			.iter()
			.filter_map(Option::as_ref)
			.flat_map(|runtime| runtime.joints.iter())
			.filter(|joint| joint.translation_writeback_allowed)
			.count()
	}

	pub fn translation_writeback_target_count(&self) -> usize {
		self.runtimes
			.iter()
			.filter_map(Option::as_ref)
			.flat_map(|runtime| runtime.joints.iter())
			.filter(|joint| joint.translation_writeback_target.is_some())
			.count()
	}
}

fn merge_category_override_params(overrides: &[DynamicsCategoryOverride]) -> BTreeMap<String, DynamicsPhysicsParams> {
	let mut by_category: BTreeMap<String, DynamicsPhysicsParams> = BTreeMap::new();
	for override_item in overrides {
		by_category
			.entry(override_item.category.clone())
			.and_modify(|params| *params = params.merge(override_item.params))
			.or_insert(override_item.params);
	}
	by_category
}

fn merge_group_override_params(overrides: &[DynamicsGroupOverride]) -> BTreeMap<String, DynamicsPhysicsParams> {
	let mut by_source_id: BTreeMap<String, DynamicsPhysicsParams> = BTreeMap::new();
	for override_item in overrides {
		by_source_id
			.entry(override_item.source_id.clone())
			.and_modify(|params| *params = params.merge(override_item.params))
			.or_insert(override_item.params);
	}
	by_source_id
}

fn dynamics_tuning_warnings(
	match_overrides: &[CompiledDynamicsMatchOverride<'_>],
	matched_match_indices: &[usize],
	group_overrides: &[DynamicsGroupOverride],
	known_source_ids: &[&str],
) -> Vec<String> {
	let mut matched_match_indices = matched_match_indices.to_vec();
	matched_match_indices.sort_unstable();
	matched_match_indices.dedup();
	let mut warnings = Vec::new();
	for (index, override_item) in match_overrides.iter().enumerate() {
		if matched_match_indices.binary_search(&index).is_err() {
			warnings.push(format!(
				"dynamics match override did not match any current source group: {}",
				override_item.label
			));
		}
	}
	for override_item in group_overrides {
		if known_source_ids.binary_search(&override_item.source_id.as_str()).is_err() {
			warnings.push(format!(
				"dynamics exact group override source_id is not present in current model: {}",
				override_item.source_id
			));
		}
	}
	warnings.sort();
	warnings.dedup();
	warnings
}

#[cfg(test)]
fn gravity_with_falloff(gravity_dir: Vec3, gravity_power: f32, target_axis_world: Vec3, gravity_falloff: f32) -> Vec3 {
	let gravity_dir = gravity_dir.normalize_or_zero();
	if gravity_dir.length_squared() < 1e-12 {
		return Vec3::ZERO;
	}
	let target_axis_world = target_axis_world.normalize_or_zero();
	let falloff = if target_axis_world.length_squared() < 1e-12 {
		1.0
	} else {
		let along_gravity = target_axis_world.dot(gravity_dir).clamp(0.0, 1.0);
		1.0 - gravity_falloff.clamp(0.0, 1.0) * along_gravity
	};
	gravity_dir * gravity_power * falloff
}

fn unphysics_response_gain(value: f32, dt: f32) -> f32 {
	if !value.is_finite() || value <= 0.0 || !dt.is_finite() || dt <= 0.0 {
		return 0.0;
	}
	// UNPhysics response values are normalized authored intent, not a raw per-frame
	// snap factor. Shape the low/mid range so profile sliders can make hair, cloth,
	// and long ears visibly soft without requiring values extremely close to zero,
	// while still allowing high values to recover quickly.
	let shaped = value.clamp(0.0, 1.0).powf(1.65);
	let response_hz = 1.0 + shaped * 23.0;
	(1.0 - (-response_hz * dt).exp()).clamp(0.0, 1.0)
}

fn unphysics_displacement_response_gain(
	value: f32,
	dt: f32,
	displacement: f32,
	rest_length: f32,
	parent_motion_follow: f32,
	chain_joint_count: usize,
) -> f32 {
	let base = unphysics_response_gain(value, dt);
	let boost = unphysics_displacement_boost(displacement, rest_length, value, parent_motion_follow, chain_joint_count);
	if base <= 0.0 || boost <= 0.0 {
		return base;
	}
	let boosted_value = (value.clamp(0.0, 1.0) + (1.0 - value.clamp(0.0, 1.0)) * 0.85 * boost).clamp(0.0, 1.0);
	base.max(unphysics_response_gain(boosted_value, dt))
}

fn unphysics_displacement_boost(
	displacement: f32,
	rest_length: f32,
	rest_response: f32,
	parent_motion_follow: f32,
	chain_joint_count: usize,
) -> f32 {
	if !displacement.is_finite()
		|| !rest_length.is_finite()
		|| rest_length <= 1e-6
		|| !rest_response.is_finite()
		|| !parent_motion_follow.is_finite()
	{
		return 0.0;
	}
	let displacement_ratio = (displacement / rest_length).max(0.0);
	let displacement_boost = ((displacement_ratio - 0.35) / 1.25).clamp(0.0, 1.0);
	let soft_response_boost = ((0.22 - rest_response.clamp(0.0, 1.0)) / 0.22).clamp(0.0, 1.0);
	let loose_motion_boost = ((0.40 - parent_motion_follow.clamp(0.0, 1.0)) / 0.40).clamp(0.0, 1.0);
	let local_large_deflection_boost = displacement_boost * soft_response_boost * loose_motion_boost;
	let long_chain_factor = ((chain_joint_count.saturating_sub(4) as f32) / 7.0).clamp(0.0, 1.0);
	let distributed_deflection_boost = ((displacement_ratio - 0.10) / 0.65).clamp(0.0, 1.0);
	let long_chain_boost = long_chain_factor * distributed_deflection_boost * soft_response_boost * loose_motion_boost * 0.82;
	local_large_deflection_boost.max(long_chain_boost)
}

fn unphysics_gravity_rest_target(
	child_pos: Vec3,
	target_axis_world: Vec3,
	rest_length: f32,
	max_length: f32,
	gravity_dir: Vec3,
	gravity_power: f32,
	gravity_falloff: f32,
) -> Vec3 {
	if gravity_power.abs() <= 0.0 || rest_length <= 1e-8 {
		return child_pos + target_axis_world * rest_length;
	}
	let gravity_dir = gravity_dir.normalize_or_zero();
	if gravity_dir.length_squared() < 1e-12 {
		return child_pos + target_axis_world * rest_length;
	}
	let target_vector = target_axis_world.normalize_or_zero() * rest_length;
	if target_vector.length_squared() < 1e-12 {
		return child_pos + target_axis_world * rest_length;
	}
	let gravity_amount = unphysics_gravity_falloff_amount(gravity_dir, gravity_power.abs(), target_axis_world, gravity_falloff);
	let allowed_length = if max_length.is_finite() {
		max_length.max(rest_length)
	} else {
		rest_length
	};
	let target_length = rest_length + (allowed_length - rest_length) * gravity_amount;
	let gravity_vector = gravity_dir * gravity_power.signum() * target_length;
	let bent_vector = target_vector.lerp(gravity_vector, gravity_amount).normalize_or_zero() * target_length;
	child_pos + bent_vector
}

fn unphysics_gravity_falloff_amount(gravity_dir: Vec3, gravity_power: f32, target_axis_world: Vec3, gravity_falloff: f32) -> f32 {
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

fn transform_tail_between_child_frames(
	tail: Vec3,
	from_child_pos: Vec3,
	from_target_rotation: Quat,
	to_child_pos: Vec3,
	to_target_rotation: Quat,
) -> Vec3 {
	let local = from_target_rotation.conjugate() * (tail - from_child_pos);
	to_child_pos + to_target_rotation * local
}

fn motion_frame_from_world(world: &[Mat4], motion_frame_node: Option<usize>, fallback_pos: Vec3, fallback_rotation: Quat) -> (Vec3, Quat) {
	let Some(node) = motion_frame_node else {
		return (fallback_pos, fallback_rotation);
	};
	let Some(matrix) = world.get(node) else {
		return (fallback_pos, fallback_rotation);
	};
	let (_, rotation, pos) = matrix.to_scale_rotation_translation();
	(pos, rotation.normalize())
}

fn apply_parent_motion_to_joint(joint: &mut JointRuntime, child_pos: Vec3, target_rotation: Quat, motion_pos: Vec3, motion_rotation: Quat) {
	let follow_parent_motion = joint.parent_motion_follow.clamp(0.0, 1.0);
	let moved = (motion_pos - joint.last_motion_pos).length_squared() > 1e-12
		|| (1.0 - motion_rotation.dot(joint.last_motion_rotation).abs()) > 1e-6;
	if moved && follow_parent_motion > 0.0 {
		let followed_curr = transform_tail_between_child_frames(
			joint.curr_tail,
			joint.last_motion_pos,
			joint.last_motion_rotation,
			motion_pos,
			motion_rotation,
		);
		let followed_prev = transform_tail_between_child_frames(
			joint.prev_tail,
			joint.last_motion_pos,
			joint.last_motion_rotation,
			motion_pos,
			motion_rotation,
		);
		joint.curr_tail = joint.curr_tail.lerp(followed_curr, follow_parent_motion);
		joint.prev_tail = joint.prev_tail.lerp(followed_prev, follow_parent_motion);
		joint.rest_lambda = 0.0;
	}
	joint.last_child_pos = child_pos;
	joint.last_target_rotation = target_rotation;
	joint.last_motion_pos = motion_pos;
	joint.last_motion_rotation = motion_rotation;
}

fn joint_damping(joint: &JointRuntime, dt: f32) -> f32 {
	match joint.damping_half_life_ms {
		Some(half_life_ms) if half_life_ms > 0.0 => 1.0 - (-std::f32::consts::LN_2 * dt / (half_life_ms / 1000.0)).exp(),
		_ => joint.drag_force,
	}
	.clamp(0.0, 1.0)
}

fn unphysics_inertia_retention(damping: f32, bounce_response: f32) -> f32 {
	let damping = damping.clamp(0.0, 1.0);
	let retained = 1.0 - damping;
	let bounce_response = bounce_response.clamp(0.0, 1.0);
	(retained + damping * bounce_response * 0.6).min(0.995)
}

fn step_group_solver<const XPBD: bool>(
	scene: &mut UnaSceneSnapshot,
	group: UnaDynamicsGroup<'_>,
	rt: &mut GroupRuntime,
	world_scratch: &mut [Mat4],
	bone_colliders: WorldColliderSelection<'_>,
	dt: f32,
	mut profile: Option<&mut DynamicsStepProfile>,
) {
	let gravity_dir = Vec3::new(
		group.parameters.gravity_dir[0],
		group.parameters.gravity_dir[1],
		group.parameters.gravity_dir[2],
	);
	for (joint_index, joint) in rt.joints.iter_mut().enumerate() {
		if joint.parent_node >= world_scratch.len() || joint.child_node >= scene.nodes.len() {
			continue;
		}
		let parent_world = world_scratch[joint.parent_node];
		let (_, parent_rot_raw, parent_pos) = parent_world.to_scale_rotation_translation();
		let parent_rot = parent_rot_raw.normalize();
		let local_child = Mat4::from_cols_array(&scene.nodes[joint.child_node].transform);
		let (_, _, current_local_translation) = local_child.to_scale_rotation_translation();
		let child_local_translation = if group.writeback_mode == UnaDynamicsWritebackMode::RotationTranslation {
			current_local_translation
		} else {
			joint.rest_local_translation
		};
		let child_pos = parent_pos + parent_rot * child_local_translation;
		let is_child_translation_target = joint.translation_writeback_target == Some(TailTranslationWritebackTarget::ChildNode);
		let solver_anchor_pos = if is_child_translation_target { parent_pos } else { child_pos };

		let target_rotation = (parent_rot * joint.rest_local_rotation).normalize();
		let previous_orientation_vector = joint.curr_tail - joint.last_child_pos;
		let (motion_pos, motion_rotation) =
			motion_frame_from_world(world_scratch, joint.motion_frame_node, solver_anchor_pos, target_rotation);
		apply_parent_motion_to_joint(joint, solver_anchor_pos, target_rotation, motion_pos, motion_rotation);
		let target_axis_world = if is_child_translation_target {
			(parent_rot * joint.rest_local_translation.normalize_or_zero()).normalize_or_zero()
		} else {
			(target_rotation * joint.bone_axis).normalize_or_zero()
		};
		if target_axis_world.length_squared() < 1e-12 {
			joint.prev_tail = joint.curr_tail;
			continue;
		}
		let limit_axis_world = target_axis_world;
		let drag = joint_damping(joint, dt);
		let mut sampled_limit = sampled_joint_limit(group, joint_index);
		if let Some(limit) = sampled_limit.as_mut() {
			apply_dynamics_limit_overrides(limit, &rt.params);
			apply_dynamics_limit_category_adjustments(limit, &rt.category);
		}
		let effective_limit = sampled_limit.as_ref().or(group.limit);
		let (min_tail_length, max_tail_length) = tail_length_range(joint.length, effective_limit);
		let target_tail = unphysics_gravity_rest_target(
			solver_anchor_pos,
			target_axis_world,
			joint.length,
			max_tail_length,
			gravity_dir,
			joint.gravity_power,
			joint.gravity_falloff,
		);
		let target_tail = constrain_tail_limit(
			target_tail,
			solver_anchor_pos,
			limit_axis_world,
			tail_distance_or(target_tail, solver_anchor_pos, joint.length),
			effective_limit,
		);
		let rest_offset = target_tail - joint.curr_tail;
		let rest_offset_len = rest_offset.length();
		let displacement_boost = unphysics_displacement_boost(
			rest_offset_len,
			joint.length,
			joint.rest_response,
			joint.parent_motion_follow,
			joint.chain_joint_count,
		);
		let inertia_retention = unphysics_inertia_retention(drag, joint.bounce_response) * (1.0 - 0.16 * displacement_boost);
		let inertia = (joint.curr_tail - joint.prev_tail) * inertia_retention;
		let rest_response = rest_offset
			* unphysics_displacement_response_gain(
				joint.rest_response,
				dt,
				rest_offset_len,
				joint.length,
				joint.parent_motion_follow,
				joint.chain_joint_count,
			);
		let previous_orientation_tail = solver_anchor_pos + previous_orientation_vector.normalize_or_zero() * joint.length;
		let shape_response = joint.shape_preservation * joint.parent_motion_follow;
		let orientation_response = (previous_orientation_tail - joint.curr_tail) * unphysics_response_gain(shape_response, dt);
		let velocity = inertia + rest_response + orientation_response;
		joint.prev_velocity = velocity;
		let unconstrained_tail = joint.curr_tail + velocity;
		let mut next_tail = unconstrained_tail;
		let mut collision_projected = false;

		if XPBD {
			for _ in 0..rt.params.constraint_iterations {
				next_tail = solve_xpbd_rest_constraint(next_tail, target_tail, rt.params.xpbd_compliance, dt, &mut joint.rest_lambda);
				next_tail = constrain_tail_length_range(next_tail, solver_anchor_pos, target_axis_world, min_tail_length, max_tail_length);
				let constrained_length = tail_distance_or(next_tail, solver_anchor_pos, joint.length);
				next_tail = constrain_tail_limit(next_tail, solver_anchor_pos, limit_axis_world, constrained_length, effective_limit);
				let constrained_length = tail_distance_or(next_tail, solver_anchor_pos, joint.length);
				let t_collision = profile.is_some().then(Instant::now);
				let before_collision = next_tail;
				next_tail = constrain_tail_colliders(
					next_tail,
					solver_anchor_pos,
					target_axis_world,
					constrained_length,
					bone_colliders,
					joint.hit_radius,
				);
				let projected = (next_tail - before_collision).length_squared() > 1e-12;
				collision_projected |= projected;
				if let (Some(profile), Some(t_collision)) = (profile.as_deref_mut(), t_collision) {
					if projected {
						let collider_path = bone_colliders.projected_path(before_collision, joint.hit_radius);
						profile.record_collision_projection(group.source_id, collider_path);
					}
					profile.solve_collision_ms += t_collision.elapsed().as_secs_f32() * 1000.0;
				}
			}
		} else {
			joint.rest_lambda = 0.0;
			next_tail = constrain_tail_length_range(next_tail, solver_anchor_pos, target_axis_world, min_tail_length, max_tail_length);
			let constrained_length = tail_distance_or(next_tail, solver_anchor_pos, joint.length);
			next_tail = constrain_tail_limit(next_tail, solver_anchor_pos, limit_axis_world, constrained_length, effective_limit);
			let constrained_length = tail_distance_or(next_tail, solver_anchor_pos, joint.length);
			let t_collision = profile.is_some().then(Instant::now);
			let before_collision = next_tail;
			next_tail = constrain_tail_colliders(
				next_tail,
				solver_anchor_pos,
				target_axis_world,
				constrained_length,
				bone_colliders,
				joint.hit_radius,
			);
			let projected = (next_tail - before_collision).length_squared() > 1e-12;
			collision_projected |= projected;
			if let (Some(profile), Some(t_collision)) = (profile.as_deref_mut(), t_collision) {
				if projected {
					let collider_path = bone_colliders.projected_path(before_collision, joint.hit_radius);
					profile.record_collision_projection(group.source_id, collider_path);
				}
				profile.solve_collision_ms += t_collision.elapsed().as_secs_f32() * 1000.0;
			}
		}

		// 回転補正: rest pose の axis (target_axis_world) を実際の axis (next_tail - child_pos) に向ける。
		let new_axis_world = (next_tail - solver_anchor_pos).normalize_or_zero();
		if new_axis_world.length_squared() < 1e-12 {
			joint.prev_tail = joint.curr_tail;
			joint.curr_tail = next_tail;
			continue;
		}
		let q_corr = Quat::from_rotation_arc(target_axis_world, new_axis_world);
		let new_world_rotation = (q_corr * target_rotation).normalize();
		let parent_rot_inv = parent_rot.conjugate();
		let new_local_rotation = (parent_rot_inv * new_world_rotation).normalize();

		// 子の local transform を現在の translation + new_local_rotation + rest_scale で書き戻す。
		let final_child_local_translation = match joint.translation_writeback_target {
			Some(TailTranslationWritebackTarget::ChildNode) => parent_world.inverse().transform_point3(next_tail),
			_ => child_local_translation,
		};
		let new_local = Mat4::from_scale_rotation_translation(joint.rest_local_scale, new_local_rotation, final_child_local_translation);
		scene.nodes[joint.child_node].transform = new_local.to_cols_array();
		if let Some(TailTranslationWritebackTarget::NextChainNode { node }) = joint.translation_writeback_target {
			if node < scene.nodes.len() {
				let child_world = parent_world * new_local;
				let target_local_translation = child_world.inverse().transform_point3(next_tail);
				let target_local = Mat4::from_cols_array(&scene.nodes[node].transform);
				let (target_scale, target_rotation, _) = target_local.to_scale_rotation_translation();
				scene.nodes[node].transform =
					Mat4::from_scale_rotation_translation(target_scale, target_rotation, target_local_translation).to_cols_array();
			}
		}

		// 子以下の world 行列を更新（次の joint の親回転計算で使う）。
		let t_propagate = profile.is_some().then(Instant::now);
		propagate_world_subtree(&scene.nodes, world_scratch, joint.child_node, parent_world);
		if let (Some(profile), Some(t_propagate)) = (profile.as_deref_mut(), t_propagate) {
			profile.solve_propagate_ms += t_propagate.elapsed().as_secs_f32() * 1000.0;
		}

		// Constraint projection is positional correction, not kinetic energy for the next frame.
		let constraint_correction = next_tail - unconstrained_tail;
		let projected_prev_tail = if collision_projected {
			next_tail
		} else {
			joint.curr_tail + constraint_correction
		};
		joint.prev_tail = if projected_prev_tail.is_finite() {
			projected_prev_tail
		} else {
			joint.curr_tail
		};
		joint.curr_tail = next_tail;
	}
}

fn apply_surface_constraints(
	scene: &mut UnaSceneSnapshot,
	runtimes: &mut [Option<GroupRuntime>],
	constraints: &[RuntimeSurfaceConstraint],
	world_scratch: &mut Vec<Mat4>,
) {
	if constraints.is_empty() {
		return;
	}
	if world_scratch.len() != scene.nodes.len().max(1) {
		write_world_from_snapshot(scene, world_scratch);
	}
	for _ in 0..1 {
		for constraint in constraints {
			let Some(a_tail) = runtime_joint_tail(runtimes, constraint.a) else {
				continue;
			};
			let Some(b_tail) = runtime_joint_tail(runtimes, constraint.b) else {
				continue;
			};
			let delta = b_tail - a_tail;
			let distance = delta.length();
			if !distance.is_finite() || distance <= 1e-5 {
				continue;
			}
			let error = distance - constraint.rest_distance;
			if error.abs() <= 1e-5 {
				continue;
			}
			let correction = delta * (0.5 * constraint.stiffness * error / distance);
			apply_runtime_joint_tail_projection(scene, runtimes, constraint.a, a_tail + correction, world_scratch);
			apply_runtime_joint_tail_projection(scene, runtimes, constraint.b, b_tail - correction, world_scratch);
		}
	}
}

fn apply_post_surface_collider_constraints(
	scene: &mut UnaSceneSnapshot,
	runtimes: &mut [Option<GroupRuntime>],
	surface_runtime_indices: &[usize],
	dynamics: UnaRuntimeDynamics<'_>,
	world_scratch: &mut [Mat4],
	world_colliders: &[WorldBoneColliderPrimitive],
	bone_collider_paths: &[String],
	runtime_collider_indices: &[Vec<usize>],
	all_bone_colliders_global: bool,
	projection_scratch: &mut Vec<(RuntimeJointHandle, Vec3)>,
) {
	if world_colliders.is_empty() {
		return;
	}
	for _ in 0..2 {
		for &runtime_index in surface_runtime_indices {
			let Some(runtime) = runtimes.get(runtime_index).and_then(Option::as_ref) else {
				continue;
			};
			let Some(group) = dynamics.dynamics_group(runtime.dynamics_group_index) else {
				continue;
			};
			if !group.effective_enabled {
				continue;
			}
			let selected_colliders = WorldColliderSelection::new(
				world_colliders,
				bone_collider_paths,
				all_bone_colliders_global,
				runtime_collider_indices.get(runtime_index).map(Vec::as_slice).unwrap_or(&[]),
			);
			if selected_colliders.is_empty() {
				continue;
			}
			projection_scratch.clear();
			for (joint_index, joint) in runtime.joints.iter().enumerate() {
				let Some(next_tail) = post_surface_collider_tail(joint, &*world_scratch, selected_colliders) else {
					continue;
				};
				if (next_tail - joint.curr_tail).length_squared() <= 1e-12 {
					continue;
				}
				projection_scratch.push((
					RuntimeJointHandle {
						runtime_index,
						joint_index,
					},
					next_tail,
				));
			}
			for (handle, next_tail) in projection_scratch.drain(..) {
				if apply_runtime_joint_tail_projection(scene, runtimes, handle, next_tail, world_scratch) {
					clear_runtime_joint_tail_velocity(runtimes, handle);
				}
			}
		}
	}
}

fn post_surface_collider_tail(joint: &JointRuntime, world_scratch: &[Mat4], bone_colliders: WorldColliderSelection<'_>) -> Option<Vec3> {
	if joint.parent_node >= world_scratch.len() || joint.child_node >= world_scratch.len() {
		return None;
	}
	let parent_world = world_scratch[joint.parent_node];
	let (_, parent_rot_raw, parent_pos) = parent_world.to_scale_rotation_translation();
	let parent_rot = parent_rot_raw.normalize();
	let is_child_translation_target = joint.translation_writeback_target == Some(TailTranslationWritebackTarget::ChildNode);
	let child_pos = if is_child_translation_target {
		parent_pos
	} else {
		let child_world = world_scratch[joint.child_node];
		let (_, _, child_pos) = child_world.to_scale_rotation_translation();
		child_pos
	};
	let solver_anchor_pos = if is_child_translation_target { parent_pos } else { child_pos };
	let target_rotation = (parent_rot * joint.rest_local_rotation).normalize();
	let target_axis_world = if is_child_translation_target {
		(parent_rot * joint.rest_local_translation.normalize_or_zero()).normalize_or_zero()
	} else {
		(target_rotation * joint.bone_axis).normalize_or_zero()
	};
	if target_axis_world.length_squared() < 1e-12 {
		return None;
	}
	let length = if joint.translation_writeback_target.is_some() {
		tail_distance_or(joint.curr_tail, solver_anchor_pos, joint.length)
	} else {
		joint.length
	};
	Some(constrain_tail_colliders(
		joint.curr_tail,
		solver_anchor_pos,
		target_axis_world,
		length,
		bone_colliders,
		joint.hit_radius,
	))
}

fn runtime_joint_tail(runtimes: &[Option<GroupRuntime>], handle: RuntimeJointHandle) -> Option<Vec3> {
	runtimes
		.get(handle.runtime_index)
		.and_then(Option::as_ref)
		.and_then(|runtime| runtime.joints.get(handle.joint_index))
		.map(|joint| joint.curr_tail)
}

fn apply_runtime_joint_tail_projection(
	scene: &mut UnaSceneSnapshot,
	runtimes: &mut [Option<GroupRuntime>],
	handle: RuntimeJointHandle,
	next_tail: Vec3,
	world_scratch: &mut [Mat4],
) -> bool {
	if !next_tail.is_finite() {
		return false;
	}
	let Some(Some(runtime)) = runtimes.get_mut(handle.runtime_index) else {
		return false;
	};
	let Some(joint) = runtime.joints.get_mut(handle.joint_index) else {
		return false;
	};
	if joint.parent_node >= world_scratch.len() || joint.child_node >= scene.nodes.len() {
		return false;
	}
	let parent_world = world_scratch[joint.parent_node];
	let (_, parent_rot_raw, parent_pos) = parent_world.to_scale_rotation_translation();
	let parent_rot = parent_rot_raw.normalize();
	let local_child = Mat4::from_cols_array(&scene.nodes[joint.child_node].transform);
	let (_, _, current_local_translation) = local_child.to_scale_rotation_translation();
	let child_pos = parent_pos + parent_rot * current_local_translation;
	let is_child_translation_target = joint.translation_writeback_target == Some(TailTranslationWritebackTarget::ChildNode);
	let solver_anchor_pos = if is_child_translation_target { parent_pos } else { child_pos };
	let target_rotation = (parent_rot * joint.rest_local_rotation).normalize();
	let target_axis_world = if is_child_translation_target {
		(parent_rot * joint.rest_local_translation.normalize_or_zero()).normalize_or_zero()
	} else {
		(target_rotation * joint.bone_axis).normalize_or_zero()
	};
	if target_axis_world.length_squared() < 1e-12 {
		return false;
	}
	let new_axis_world = (next_tail - solver_anchor_pos).normalize_or_zero();
	if new_axis_world.length_squared() < 1e-12 {
		return false;
	}
	let q_corr = Quat::from_rotation_arc(target_axis_world, new_axis_world);
	let new_world_rotation = (q_corr * target_rotation).normalize();
	let parent_rot_inv = parent_rot.conjugate();
	let new_local_rotation = (parent_rot_inv * new_world_rotation).normalize();
	let final_child_local_translation = match joint.translation_writeback_target {
		Some(TailTranslationWritebackTarget::ChildNode) => parent_world.inverse().transform_point3(next_tail),
		_ => current_local_translation,
	};
	let new_local = Mat4::from_scale_rotation_translation(joint.rest_local_scale, new_local_rotation, final_child_local_translation);
	scene.nodes[joint.child_node].transform = new_local.to_cols_array();
	if let Some(TailTranslationWritebackTarget::NextChainNode { node }) = joint.translation_writeback_target {
		if node < scene.nodes.len() {
			let child_world = parent_world * new_local;
			let target_local_translation = child_world.inverse().transform_point3(next_tail);
			let target_local = Mat4::from_cols_array(&scene.nodes[node].transform);
			let (target_scale, target_rotation, _) = target_local.to_scale_rotation_translation();
			scene.nodes[node].transform =
				Mat4::from_scale_rotation_translation(target_scale, target_rotation, target_local_translation).to_cols_array();
		}
	}
	propagate_world_subtree(&scene.nodes, world_scratch, joint.child_node, parent_world);
	let correction = next_tail - joint.curr_tail;
	let projected_prev_tail = joint.prev_tail + correction;
	if projected_prev_tail.is_finite() {
		joint.prev_tail = projected_prev_tail;
	}
	joint.curr_tail = next_tail;
	true
}

fn clear_runtime_joint_tail_velocity(runtimes: &mut [Option<GroupRuntime>], handle: RuntimeJointHandle) {
	let Some(Some(runtime)) = runtimes.get_mut(handle.runtime_index) else {
		return;
	};
	let Some(joint) = runtime.joints.get_mut(handle.joint_index) else {
		return;
	};
	joint.prev_tail = joint.curr_tail;
}

fn constrain_tail_length(next_tail: Vec3, child_pos: Vec3, fallback_axis: Vec3, length: f32) -> Vec3 {
	let dir = (next_tail - child_pos).normalize_or_zero();
	if dir.length_squared() < 1e-12 {
		child_pos + fallback_axis * length
	} else {
		child_pos + dir * length
	}
}

fn effective_stretch_response(limit: Option<&UnaDynamicsLimit>) -> (f32, f32, f32) {
	let Some(limit) = limit else {
		return (0.0, 0.0, 0.0);
	};
	let stretch_motion = limit
		.stretch_motion
		.filter(|value| value.is_finite())
		.unwrap_or(1.0)
		.clamp(0.0, 1.0);
	let max_stretch = if limit.max_stretch.is_finite() {
		limit.max_stretch.max(0.0)
	} else {
		0.0
	};
	let max_squish = if limit.max_squish.is_finite() {
		limit.max_squish.max(0.0).clamp(0.0, 0.95)
	} else {
		0.0
	};
	if max_stretch <= 0.0 && max_squish <= 0.0 {
		return (0.0, 0.0, 0.0);
	}
	(max_stretch, max_squish, stretch_motion)
}

fn tail_length_range(rest_length: f32, limit: Option<&UnaDynamicsLimit>) -> (f32, f32) {
	let (max_stretch, max_squish, stretch_motion) = effective_stretch_response(limit);
	let min_length = (rest_length * (1.0 - max_squish * stretch_motion)).max(1e-4);
	let max_length = (rest_length * (1.0 + max_stretch * stretch_motion)).max(rest_length);
	(min_length.min(max_length), max_length)
}

fn tail_distance_or(next_tail: Vec3, child_pos: Vec3, fallback_length: f32) -> f32 {
	let distance = (next_tail - child_pos).length();
	if distance.is_finite() && distance > 1e-6 {
		distance
	} else {
		fallback_length
	}
}

fn constrain_tail_length_range(next_tail: Vec3, child_pos: Vec3, fallback_axis: Vec3, min_length: f32, max_length: f32) -> Vec3 {
	if max_length <= min_length + 1e-6 {
		return constrain_tail_length(next_tail, child_pos, fallback_axis, min_length);
	}
	let offset = next_tail - child_pos;
	let dir = offset.normalize_or_zero();
	if dir.length_squared() < 1e-12 {
		return child_pos + fallback_axis * min_length;
	}
	let distance = offset.length().clamp(min_length, max_length);
	child_pos + dir * distance
}

fn sampled_joint_limit(group: UnaDynamicsGroup<'_>, joint_index: usize) -> Option<UnaDynamicsLimit> {
	let mut limit = group.limit?.clone();
	if let Some(max_angle_x) = dynamics_sample(group.chain.max_angle_x_samples, joint_index) {
		limit.max_angle_x = max_angle_x.max(0.0);
	}
	if let Some(max_angle_z) = dynamics_sample(group.chain.max_angle_z_samples, joint_index) {
		limit.max_angle_z = max_angle_z.max(0.0);
	}
	if let Some(max_stretch) = dynamics_sample(&limit.max_stretch_samples, joint_index) {
		limit.max_stretch = max_stretch.max(0.0);
	}
	if let Some(max_squish) = dynamics_sample(&limit.max_squish_samples, joint_index) {
		limit.max_squish = max_squish.max(0.0);
	}
	if let Some(stretch_motion) = dynamics_sample(&limit.stretch_motion_samples, joint_index) {
		limit.stretch_motion = Some(stretch_motion.clamp(0.0, 1.0));
	}
	Some(limit)
}

fn apply_dynamics_limit_overrides(limit: &mut UnaDynamicsLimit, params: &ResolvedDynamicsPhysicsParams) {
	limit.max_stretch = (limit.max_stretch * params.stretch_range_scale).max(0.0);
	limit.max_squish = (limit.max_squish * params.stretch_range_scale).max(0.0);
	for value in &mut limit.max_stretch_samples {
		*value = (*value * params.stretch_range_scale).max(0.0);
	}
	for value in &mut limit.max_squish_samples {
		*value = (*value * params.stretch_range_scale).max(0.0);
	}
	if let Some(stretch_motion) = params.stretch_motion_override {
		limit.stretch_motion = Some(stretch_motion.clamp(0.0, 1.0));
		limit.stretch_motion_samples.clear();
	}
}

fn apply_dynamics_limit_category_adjustments(limit: &mut UnaDynamicsLimit, category: &str) {
	if category == "cloth" {
		limit.limit_type.clear();
		limit.max_angle_x = 0.0;
		limit.max_angle_z = 0.0;
	}
}

fn constrain_tail_limit(next_tail: Vec3, child_pos: Vec3, fallback_axis: Vec3, length: f32, limit: Option<&UnaDynamicsLimit>) -> Vec3 {
	let Some(limit) = limit else {
		return next_tail;
	};
	let rest_axis = fallback_axis.normalize_or_zero();
	let dir = (next_tail - child_pos).normalize_or_zero();
	if rest_axis.length_squared() < 1e-12 || dir.length_squared() < 1e-12 {
		return child_pos + fallback_axis * length;
	}
	let constrained_dir = constrain_axis_by_limit_type(dir, rest_axis, limit);
	child_pos + constrained_dir.normalize_or_zero() * length
}

fn constrain_axis_by_limit_type(axis: Vec3, rest_axis: Vec3, limit: &UnaDynamicsLimit) -> Vec3 {
	let limit_type = limit.limit_type.to_ascii_lowercase();
	if limit_type.is_empty() || limit_type == "none" {
		return axis;
	}
	let base_rotation = Quat::from_rotation_arc(Vec3::Y, rest_axis);
	let authored_rotation = Quat::from_euler(
		glam::EulerRot::XYZ,
		limit.limit_rotation[0].to_radians(),
		limit.limit_rotation[1].to_radians(),
		limit.limit_rotation[2].to_radians(),
	);
	let limit_rotation = (base_rotation * authored_rotation).normalize();
	let limit_axis_x = (limit_rotation * Vec3::X).normalize_or_zero();
	let limit_axis_y = (limit_rotation * Vec3::Y).normalize_or_zero();
	let limit_axis_y = if limit_axis_y.length_squared() >= 1e-12 {
		limit_axis_y
	} else {
		rest_axis
	};
	if limit_type.contains("hinge") {
		let projected = (axis - limit_axis_x * axis.dot(limit_axis_x)).normalize_or_zero();
		let axis = if projected.length_squared() < 1e-12 {
			limit_axis_y
		} else {
			projected
		};
		return constrain_axis_angle(axis, limit_axis_y, limit_axis_x, limit.max_angle_x);
	}
	if limit_type.contains("polar") {
		return constrain_axis_polar(axis, limit_axis_x, limit_axis_y, limit.max_angle_x, limit.max_angle_z);
	}
	if limit_type.contains("angle") {
		return constrain_axis_angle(axis, limit_axis_y, limit_axis_x, limit.max_angle_x);
	}
	axis
}

fn constrain_axis_angle(axis: Vec3, rest_axis: Vec3, fallback_rotation_axis: Vec3, max_angle_deg: f32) -> Vec3 {
	let max_angle_rad = max_angle_deg.clamp(0.0, 179.0).to_radians();
	if max_angle_rad <= 0.0 {
		return rest_axis;
	}
	let dot = rest_axis.dot(axis).clamp(-1.0, 1.0);
	let angle = dot.acos();
	if angle <= max_angle_rad {
		return axis;
	}
	let rotation_axis = rest_axis.cross(axis).normalize_or_zero();
	let rotation_axis = if rotation_axis.length_squared() >= 1e-12 {
		rotation_axis
	} else {
		fallback_rotation_axis
	};
	Quat::from_axis_angle(rotation_axis, max_angle_rad) * rest_axis
}

fn constrain_axis_polar(axis: Vec3, limit_axis_x: Vec3, limit_axis_y: Vec3, max_angle_x_deg: f32, max_angle_z_deg: f32) -> Vec3 {
	let limit_axis_z = limit_axis_x.cross(limit_axis_y).normalize_or_zero();
	if limit_axis_z.length_squared() < 1e-12 {
		return constrain_axis_angle(axis, limit_axis_y, limit_axis_x, max_angle_x_deg.max(max_angle_z_deg));
	}
	let local = Vec3::new(axis.dot(limit_axis_x), axis.dot(limit_axis_z), axis.dot(limit_axis_y)).normalize_or_zero();
	if local.length_squared() < 1e-12 {
		return limit_axis_y;
	}
	let x_angle = local
		.x
		.atan2(local.z)
		.clamp(-max_angle_x_deg.to_radians(), max_angle_x_deg.to_radians());
	let z_angle = local
		.y
		.atan2(local.z)
		.clamp(-max_angle_z_deg.to_radians(), max_angle_z_deg.to_radians());
	let clamped_local = Vec3::new(x_angle.tan(), z_angle.tan(), 1.0).normalize_or_zero();
	(limit_axis_x * clamped_local.x + limit_axis_z * clamped_local.y + limit_axis_y * clamped_local.z).normalize_or_zero()
}

fn constrain_tail_colliders(
	next_tail: Vec3,
	child_pos: Vec3,
	fallback_axis: Vec3,
	length: f32,
	bone_colliders: WorldColliderSelection<'_>,
	hit_radius: f32,
) -> Vec3 {
	if bone_colliders.is_empty() {
		return next_tail;
	}
	let pushed = bone_colliders.push_out(next_tail, hit_radius);
	if (pushed - next_tail).length_squared() <= 1e-12 {
		return next_tail;
	}
	let pushed_dir = (pushed - child_pos).normalize_or_zero();
	if pushed_dir.length_squared() >= 1e-12 {
		child_pos + pushed_dir * length
	} else {
		child_pos + fallback_axis * length
	}
}

fn selected_group_collider_indices(source_ids: &[String], group_source_id: &str) -> Vec<usize> {
	source_ids
		.iter()
		.enumerate()
		.filter_map(|(index, source_id)| {
			(source_id.is_empty() || (!group_source_id.is_empty() && source_id == group_source_id)).then_some(index)
		})
		.collect()
}

#[cfg(test)]
fn select_group_world_colliders<'a>(
	world_colliders: &'a [WorldBoneColliderPrimitive],
	collider_paths: &'a [String],
	selected_indices: Option<&[usize]>,
	scratch: &'a mut Vec<WorldBoneColliderPrimitive>,
	path_scratch: Option<&'a mut Vec<String>>,
) -> (&'a [WorldBoneColliderPrimitive], &'a [String]) {
	let Some(selected_indices) = selected_indices else {
		return (world_colliders, collider_paths);
	};
	scratch.clear();
	scratch.reserve(selected_indices.len());
	if let Some(path_scratch) = path_scratch {
		path_scratch.clear();
		path_scratch.reserve(selected_indices.len());
		for &index in selected_indices {
			if let Some(collider) = world_colliders.get(index).copied() {
				scratch.push(collider);
				path_scratch.push(collider_paths.get(index).cloned().unwrap_or_default());
			}
		}
		(scratch.as_slice(), path_scratch.as_slice())
	} else {
		for &index in selected_indices {
			if let Some(collider) = world_colliders.get(index).copied() {
				scratch.push(collider);
			}
		}
		(scratch.as_slice(), &[])
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
	use un_avatar_core::{
		UnaDocument, UnaDynamicsIntegrationType, UnaDynamicsLimit, UnaDynamicsSourceKind, UnaDynamicsWritebackMode, UnaMeshBuffers,
		UnaSceneAssetGroupOwnership, UnaSceneSnapshot, UnaSkin, UnaSpringBoneGroup, UnaSpringBoneSettings,
	};

	#[test]
	fn unphysics_mesh_cloth_assist_config_normalizes() {
		let config = DynamicsPhysicsConfig {
			mesh_cloth_assist: DynamicsMeshClothAssistConfig {
				enabled: true,
				body_dominance_threshold: f32::NAN,
				min_existing_dynamic_weight: -1.0,
				seed_missing_dynamic_influence: true,
				max_assist_weight: 5.0,
				mesh_path_contains: vec![" ClothPanelMesh ".to_string(), "".to_string(), "skirt-panel".to_string()],
			},
			..Default::default()
		}
		.normalized();
		assert!(config.mesh_cloth_assist.enabled);
		assert_eq!(config.mesh_cloth_assist.body_dominance_threshold, 0.55);
		assert_eq!(config.mesh_cloth_assist.min_existing_dynamic_weight, 0.0);
		assert!(config.mesh_cloth_assist.seed_missing_dynamic_influence);
		assert_eq!(config.mesh_cloth_assist.max_assist_weight, 0.95);
		assert_eq!(config.mesh_cloth_assist.mesh_path_contains, vec!["cloth_panel_mesh", "skirt_panel"]);
	}

	#[test]
	fn surface_constraint_config_normalizes_to_source_neutral_bounds() {
		let config = DynamicsPhysicsConfig {
			surface_constraints_enabled: true,
			surface_constraint_topology_max_edge_distance_m: f32::NAN,
			surface_constraint_topology_max_mean_edge_distance_m: 9.0,
			surface_constraint_spatial_max_distance_m: -1.0,
			surface_constraint_topology_stiffness: 2.0,
			surface_constraint_spatial_stiffness: -2.0,
			surface_constraint_min_edge_count: 0,
			..Default::default()
		}
		.normalized();

		assert_eq!(config.surface_constraint_topology_max_edge_distance_m, 0.06);
		assert_eq!(config.surface_constraint_topology_max_mean_edge_distance_m, 0.06);
		assert_eq!(config.surface_constraint_spatial_max_distance_m, 0.001);
		assert_eq!(config.surface_constraint_topology_stiffness, 1.0);
		assert_eq!(config.surface_constraint_spatial_stiffness, 0.0);
		assert_eq!(config.surface_constraint_min_edge_count, 1);
	}

	#[test]
	fn mesh_cloth_assist_joint_matchers_cover_common_separators() {
		assert!(dynamics_mesh_cloth_assist_body_joint_matches("Upper_Arm_L"));
		assert!(dynamics_mesh_cloth_assist_body_joint_matches("LowerArm_R"));
		assert!(dynamics_mesh_cloth_assist_cloth_joint_matches("Sleeve_Frill_L"));
		assert!(dynamics_mesh_cloth_assist_cloth_joint_matches("Blouse_Hem_L"));
		assert!(dynamics_mesh_cloth_assist_cloth_joint_matches("LongCoat_Back"));
		assert!(dynamics_mesh_cloth_assist_cloth_joint_matches("StockingsRoot_L"));
		assert!(dynamics_mesh_cloth_assist_cloth_joint_matches("ブラウス_裾_L"));
		assert!(dynamics_mesh_cloth_assist_cloth_joint_matches("ケープ01"));
		assert!(!dynamics_mesh_cloth_assist_body_joint_matches("PocketWatch_Root"));
		assert!(!dynamics_mesh_cloth_assist_cloth_joint_matches("Chest"));
		assert!(!dynamics_mesh_cloth_assist_cloth_joint_matches("ChairRoot"));
	}

	#[test]
	fn mesh_cloth_assist_deforming_nodes_skip_anchors_and_respect_authored_start() {
		let chain = [10, 11, 12, 13, 14];

		assert_eq!(
			dynamics_mesh_cloth_assist_deforming_nodes(&chain, 0).collect::<Vec<_>>(),
			vec![12, 13, 14]
		);
		assert_eq!(
			dynamics_mesh_cloth_assist_deforming_nodes(&chain, 1).collect::<Vec<_>>(),
			vec![12, 13, 14]
		);
		assert_eq!(
			dynamics_mesh_cloth_assist_deforming_nodes(&chain, 3).collect::<Vec<_>>(),
			vec![13, 14]
		);
		assert!(dynamics_mesh_cloth_assist_deforming_nodes(&chain[..2], 0)
			.collect::<Vec<_>>()
			.is_empty());
	}

	#[test]
	fn mesh_cloth_assist_matchers_reuse_lowercase_inputs() {
		assert!(dynamics_mesh_cloth_assist_mesh_matches("avatar/longcoat", &[]));
		assert!(dynamics_mesh_cloth_assist_mesh_matches("Avatar/LongCoat", &[]));
		assert!(dynamics_mesh_cloth_assist_mesh_matches("Avatar/Long Coat", &[]));
		assert!(dynamics_mesh_cloth_assist_mesh_matches(
			"Avatar/ClothPanelMesh",
			&["clothpanelmesh".to_string()]
		));
		assert!(!dynamics_mesh_cloth_assist_mesh_matches("Avatar/ChairBack", &[]));
	}

	#[test]
	fn mesh_cloth_assist_empty_filter_uses_profile_cloth_category_aliases() {
		let categories = vec![
			DynamicsCategoryDefinition {
				id: "cloth".to_string(),
				name: "Cloth".to_string(),
				matches: vec!["panel".to_string()],
			},
			DynamicsCategoryDefinition {
				id: "other".to_string(),
				name: "Other".to_string(),
				matches: Vec::new(),
			},
		];

		assert!(dynamics_mesh_cloth_assist_mesh_matches_with_categories(
			"Avatar/PanelWing",
			&[],
			&categories
		));
		assert!(!dynamics_mesh_cloth_assist_mesh_matches_with_categories(
			"Avatar/LongCoat",
			&[],
			&categories
		));
	}

	#[test]
	fn mesh_cloth_assist_joint_roles_prefer_runtime_dynamic_membership() {
		let skin = UnaSkin {
			joint_nodes: vec![0, 1, 2],
			inverse_bind_matrices: vec![[1.0; 16]; 3],
			skeleton_node: None,
		};
		let dynamic_nodes = vec![1usize];
		let leaves = ["Chest", "Accessory_Dyn", "Cloth_Static"];

		let roles = dynamics_mesh_cloth_assist_joint_roles(&skin, 3, Some(&dynamic_nodes), |joint_index| leaves[joint_index]);

		assert_eq!(roles[0], DynamicsMeshClothAssistJointRole::Body);
		assert_eq!(roles[1], DynamicsMeshClothAssistJointRole::Dynamic);
		assert_eq!(roles[2], DynamicsMeshClothAssistJointRole::StaticCloth);
	}

	#[test]
	fn mesh_cloth_assist_joint_roles_use_cloth_alias_only_without_runtime_membership() {
		let skin = UnaSkin {
			joint_nodes: vec![0, 1],
			inverse_bind_matrices: vec![[1.0; 16]; 2],
			skeleton_node: None,
		};
		let leaves = ["Chest", "Cloth_Static"];

		let roles = dynamics_mesh_cloth_assist_joint_roles(&skin, 2, None, |joint_index| leaves[joint_index]);

		assert_eq!(roles[0], DynamicsMeshClothAssistJointRole::Body);
		assert_eq!(roles[1], DynamicsMeshClothAssistJointRole::Dynamic);
	}

	#[test]
	fn mesh_cloth_assist_transfer_candidate_caps_remaining_assist() {
		let config = DynamicsMeshClothAssistConfig {
			enabled: true,
			body_dominance_threshold: 0.55,
			min_existing_dynamic_weight: 0.04,
			seed_missing_dynamic_influence: true,
			max_assist_weight: 0.3,
			mesh_path_contains: Vec::new(),
		};

		let candidate = dynamics_mesh_cloth_assist_transfer_candidate(&config, true, 0.8, 0.05, 0.05, 0.25, 0.22)
			.expect("existing dynamic lane transfer");

		assert_eq!(candidate.kind, DynamicsMeshClothAssistTransferKind::ExistingDynamicLane);
		assert!((candidate.transfer_weight - 0.08).abs() < 0.0001);
	}

	#[test]
	fn mesh_cloth_assist_transfer_candidate_requires_seed_opt_in() {
		let config = DynamicsMeshClothAssistConfig {
			enabled: true,
			body_dominance_threshold: 0.55,
			min_existing_dynamic_weight: 0.04,
			seed_missing_dynamic_influence: false,
			max_assist_weight: 0.3,
			mesh_path_contains: Vec::new(),
		};

		assert!(dynamics_mesh_cloth_assist_transfer_candidate(&config, false, 0.8, 0.0, 0.1, 0.2, 0.0).is_none());
	}

	#[test]
	fn mesh_cloth_assist_transfer_candidate_requires_stronger_neighbor() {
		let config = DynamicsMeshClothAssistConfig {
			enabled: true,
			body_dominance_threshold: 0.55,
			min_existing_dynamic_weight: 0.04,
			seed_missing_dynamic_influence: true,
			max_assist_weight: 0.3,
			mesh_path_contains: Vec::new(),
		};

		assert!(dynamics_mesh_cloth_assist_transfer_candidate(&config, true, 0.8, 0.05, 0.05, 0.0505, 0.0).is_none());
	}

	#[derive(Clone, Copy)]
	struct TestClothAssistVertex {
		joints: [u16; 4],
		weights: [f32; 4],
	}

	impl DynamicsMeshClothAssistVertex for TestClothAssistVertex {
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

	#[test]
	fn mesh_cloth_assist_shared_helper_propagates_connected_dynamic_evidence() {
		let mut vertices = vec![
			TestClothAssistVertex {
				joints: [0, 1, 0, 0],
				weights: [0.78, 0.22, 0.0, 0.0],
			},
			TestClothAssistVertex {
				joints: [0, 1, 2, 0],
				weights: [0.58, 0.418, 0.002, 0.0],
			},
			TestClothAssistVertex {
				joints: [0, 1, 2, 0],
				weights: [0.30, 0.684, 0.016, 0.0],
			},
			TestClothAssistVertex {
				joints: [0, 1, 2, 0],
				weights: [0.16, 0.768, 0.072, 0.0],
			},
		];
		let config = DynamicsMeshClothAssistConfig {
			enabled: true,
			body_dominance_threshold: 0.55,
			min_existing_dynamic_weight: 0.04,
			seed_missing_dynamic_influence: true,
			max_assist_weight: 0.3,
			mesh_path_contains: Vec::new(),
		};

		let changed = apply_dynamics_mesh_cloth_assist_to_vertices(&mut vertices, &[0, 1, 2, 1, 2, 3], 3, &config, |joint| match joint {
			0 => DynamicsMeshClothAssistJointRole::Body,
			1 => DynamicsMeshClothAssistJointRole::StaticCloth,
			2 => DynamicsMeshClothAssistJointRole::Dynamic,
			_ => DynamicsMeshClothAssistJointRole::Other,
		});

		assert!(changed >= 2);
		let head_dynamic = vertices[0]
			.joints
			.iter()
			.zip(vertices[0].weights.iter())
			.filter_map(|(&joint, &weight)| (joint == 2).then_some(weight))
			.sum::<f32>();
		assert!(
			head_dynamic >= config.min_existing_dynamic_weight,
			"connected helper should propagate dynamic evidence to the body-dominated cloth head, got {head_dynamic}"
		);
	}

	#[test]
	fn mesh_cloth_assist_uses_strongest_neighbor_dynamic_joint_not_split_sum() {
		let mut vertices = vec![
			TestClothAssistVertex {
				joints: [0, 1, 0, 0],
				weights: [0.78, 0.22, 0.0, 0.0],
			},
			TestClothAssistVertex {
				joints: [0, 1, 2, 3],
				weights: [0.70, 0.23, 0.04, 0.03],
			},
			TestClothAssistVertex {
				joints: [0, 1, 0, 0],
				weights: [0.78, 0.22, 0.0, 0.0],
			},
		];
		let config = DynamicsMeshClothAssistConfig {
			enabled: true,
			body_dominance_threshold: 0.55,
			min_existing_dynamic_weight: 0.02,
			seed_missing_dynamic_influence: true,
			max_assist_weight: 0.3,
			mesh_path_contains: Vec::new(),
		};

		let changed = apply_dynamics_mesh_cloth_assist_to_vertices(&mut vertices, &[0, 1, 2], 4, &config, |joint| match joint {
			0 => DynamicsMeshClothAssistJointRole::Body,
			1 => DynamicsMeshClothAssistJointRole::StaticCloth,
			2 | 3 => DynamicsMeshClothAssistJointRole::Dynamic,
			_ => DynamicsMeshClothAssistJointRole::Other,
		});

		assert_eq!(changed, 2);
		for vertex in [vertices[0], vertices[2]] {
			let seeded_dynamic = vertex
				.joints
				.iter()
				.zip(vertex.weights.iter())
				.filter_map(|(&joint, &weight)| (joint == 2).then_some(weight))
				.sum::<f32>();
			assert!(
				(seeded_dynamic - 0.04).abs() < 0.0001,
				"seeded dynamic weight should follow the strongest adjacent dynamic joint, got {seeded_dynamic}"
			);
		}
	}

	#[test]
	fn mesh_cloth_assist_shared_helper_does_not_seed_without_topology_evidence() {
		let mut vertices = vec![
			TestClothAssistVertex {
				joints: [0, 1, 0, 0],
				weights: [0.78, 0.22, 0.0, 0.0],
			},
			TestClothAssistVertex {
				joints: [0, 1, 0, 0],
				weights: [0.78, 0.22, 0.0, 0.0],
			},
		];
		let config = DynamicsMeshClothAssistConfig {
			enabled: true,
			body_dominance_threshold: 0.55,
			min_existing_dynamic_weight: 0.04,
			seed_missing_dynamic_influence: true,
			max_assist_weight: 0.3,
			mesh_path_contains: Vec::new(),
		};

		let changed = apply_dynamics_mesh_cloth_assist_to_vertices(&mut vertices, &[0, 1, 1], 3, &config, |joint| match joint {
			0 => DynamicsMeshClothAssistJointRole::Body,
			1 => DynamicsMeshClothAssistJointRole::StaticCloth,
			2 => DynamicsMeshClothAssistJointRole::Dynamic,
			_ => DynamicsMeshClothAssistJointRole::Other,
		});

		assert_eq!(changed, 0);
		assert_eq!(vertices[0].joints, [0, 1, 0, 0]);
		assert_eq!(vertices[0].weights, [0.78, 0.22, 0.0, 0.0]);
	}

	#[test]
	fn explicit_contains_match_uses_tokens_without_substring_accidents() {
		assert!(explicit_contains_match("physbone:left_ear", "ear"));
		assert!(explicit_contains_match("avatar/hat_ribbon_tail", "hat ribbon"));
		assert!(explicit_contains_match("avatar/long_coat", "longcoat"));
		assert!(explicit_contains_match("avatar/cape01", "cape"));
		assert!(!explicit_contains_match("physbone:earring_l", "ear"));
		assert!(!explicit_contains_match("avatar/gear_root", "ear"));
		assert!(!explicit_contains_match("avatar/chair_back", "hair"));
	}

	#[test]
	fn public_token_filter_matches_share_override_boundaries() {
		assert!(dynamics_token_filter_matches("Avatar/Hat_Ribbon_Tail_L", "hat ribbon"));
		assert!(dynamics_token_filter_matches("Avatar/ClothPanelMesh", "cloth panel"));
		assert!(!dynamics_token_filter_matches("Avatar/Earring_L", "ear"));
		assert!(!dynamics_token_filter_matches("Avatar/ChairBack", "hair"));
	}

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

	fn weighted_test_primitive(joints: Vec<[u16; 4]>, weights: Vec<[f32; 4]>) -> UnaMeshBuffers {
		UnaMeshBuffers {
			name: None,
			vertex_payload_id: None,
			positions: vec![[0.0, 0.0, 0.0]; joints.len().max(weights.len()).max(1)],
			normals: None,
			tangents: None,
			tex_coords_0: None,
			tex_coords_1: None,
			tex_coords_2: None,
			tex_coords_3: None,
			colors_0: None,
			joints: Some(joints),
			weights: Some(weights),
			indices: None,
			material_index: None,
			morph_targets: Vec::new(),
			morph_target_names: Vec::new(),
			default_morph_weights: Vec::new(),
		}
	}

	#[test]
	fn visual_target_context_counts_only_weighted_visible_skin_joints() {
		let mut mesh_node = node(0.0, Vec3::ZERO, Vec::new());
		mesh_node.mesh = Some(0);
		mesh_node.skin = Some(0);
		let scene = UnaSceneSnapshot {
			nodes: vec![
				node(0.0, Vec3::ZERO, vec![1, 2, 3]),
				node(0.0, Vec3::ZERO, Vec::new()),
				node(0.0, Vec3::ZERO, Vec::new()),
				mesh_node,
			],
			roots: vec![0],
			meshes: vec![vec![weighted_test_primitive(vec![[0, 1, 0, 0]], vec![[1.0, 0.0, 0.0, 0.0]])]],
			skins: vec![UnaSkin {
				joint_nodes: vec![1, 2],
				..Default::default()
			}],
			..Default::default()
		};
		let context = DynamicsVisualTargetContext::for_scene(&scene);

		assert_eq!(context.group_counts(&[1]), (1, 0));
		assert_eq!(context.group_counts(&[2]), (0, 0));
		assert_eq!(context.group_counts(&[3]), (0, 1));
	}

	#[test]
	fn source_tagged_colliders_are_filtered_per_dynamics_group() {
		let colliders = vec![
			WorldBoneColliderPrimitive::Sphere {
				center: Vec3::ZERO,
				radius: 1.0,
				inside_bounds: false,
			},
			WorldBoneColliderPrimitive::Sphere {
				center: Vec3::X,
				radius: 1.0,
				inside_bounds: false,
			},
			WorldBoneColliderPrimitive::Sphere {
				center: Vec3::Y,
				radius: 1.0,
				inside_bounds: false,
			},
		];
		let source_ids = vec![String::new(), "physbone:hair".to_string(), "physbone:skirt".to_string()];
		let paths = vec!["global".to_string(), "hair_col".to_string(), "skirt_col".to_string()];
		let selected_indices = selected_group_collider_indices(&source_ids, "physbone:hair");
		let mut scratch = Vec::new();
		let mut path_scratch = Vec::new();
		let (selected, selected_paths) =
			select_group_world_colliders(&colliders, &paths, Some(&selected_indices), &mut scratch, Some(&mut path_scratch));
		assert_eq!(selected.len(), 2);
		assert_eq!(selected[0], colliders[0]);
		assert_eq!(selected[1], colliders[1]);
		assert_eq!(selected_paths, &["global".to_string(), "hair_col".to_string()]);
	}

	#[test]
	fn select_group_world_colliders_skips_path_scratch_when_not_profiled() {
		let colliders = vec![
			WorldBoneColliderPrimitive::Sphere {
				center: Vec3::ZERO,
				radius: 1.0,
				inside_bounds: false,
			},
			WorldBoneColliderPrimitive::Sphere {
				center: Vec3::Y,
				radius: 1.0,
				inside_bounds: false,
			},
		];
		let source_ids = vec![String::new(), "physbone:hair".to_string()];
		let paths = vec!["global".to_string(), "hair_col".to_string()];
		let selected_indices = selected_group_collider_indices(&source_ids, "physbone:hair");
		let mut scratch = Vec::new();
		let (selected, selected_paths) = select_group_world_colliders(&colliders, &paths, Some(&selected_indices), &mut scratch, None);

		assert_eq!(selected.len(), 2);
		assert!(selected_paths.is_empty());
	}

	#[test]
	fn projected_collider_path_reports_projecting_collider() {
		let colliders = vec![
			WorldBoneColliderPrimitive::Sphere {
				center: Vec3::ZERO,
				radius: 0.05,
				inside_bounds: false,
			},
			WorldBoneColliderPrimitive::Sphere {
				center: Vec3::X,
				radius: 0.25,
				inside_bounds: false,
			},
		];
		let paths = vec!["near_origin".to_string(), "right_col".to_string()];

		let path = WorldColliderSelection::new(&colliders, &paths, true, &[]).projected_path(Vec3::new(0.9, 0.0, 0.0), 0.0);

		assert_eq!(path, Some("right_col"));
	}

	#[test]
	fn dynamics_step_profile_serializes_projection_collider_paths() {
		let mut profile = DynamicsStepProfile::default();
		profile.record_collision_projection("physbone:cloth", Some("BodyColliders/Chest"));
		profile.record_collision_projection("physbone:cloth", Some("BodyColliders/Chest"));
		profile.record_collision_projection("physbone:hair", Some("BodyColliders/Head"));

		let value = serde_json::to_value(&profile).expect("profile json");

		assert_eq!(value["collision_projection_count"], 3);
		assert_eq!(value["collision_projection_source_ids"].as_array().unwrap().len(), 2);
		assert_eq!(value["collision_projection_source_counts"]["physbone:cloth"], 2);
		assert_eq!(value["collision_projection_collider_paths"][0], "BodyColliders/Chest");
		assert_eq!(value["collision_projection_collider_paths"].as_array().unwrap().len(), 2);
		assert_eq!(value["collision_projection_collider_path_counts"]["BodyColliders/Chest"], 2);
		assert_eq!(
			value["collision_projection_source_collider_path_counts"]["physbone:cloth"]["BodyColliders/Chest"],
			2
		);
		assert_eq!(
			value["collision_projection_source_collider_path_counts"]["physbone:hair"]["BodyColliders/Head"],
			1
		);
	}

	#[test]
	fn collider_selection_summary_reports_group_filtered_sources() {
		let mut sim = DynamicsSimulator::default();
		sim.bone_colliders = vec![
			BoneColliderPrimitive::Sphere { node: 0, radius: 1.0 },
			BoneColliderPrimitive::Sphere { node: 1, radius: 1.0 },
			BoneColliderPrimitive::Sphere { node: 2, radius: 1.0 },
		];
		sim.bone_collider_source_ids = vec![String::new(), "physbone:hair".to_string(), "physbone:skirt".to_string()];
		sim.all_bone_colliders_global = false;
		sim.bone_collider_paths = vec![
			"global".to_string(),
			"root/BodyColliders/Hair".to_string(),
			"root/BodyColliders/Skirt".to_string(),
		];
		sim.runtimes = vec![Some(GroupRuntime {
			dynamics_group_index: 0,
			source_id: "physbone:hair".to_string(),
			category: "hair".to_string(),
			matched_overrides: Vec::new(),
			group_override_applied: false,
			invalid_match_regexes: Vec::new(),
			joints: Vec::new(),
			params: ResolvedDynamicsPhysicsParams {
				solver: DynamicsSolver::Verlet,
				damping_half_life_ms: None,
				rest_response_override: None,
				shape_preservation_override: None,
				bounce_scale: 1.0,
				source_shape_preservation_scale: 1.0,
				source_rest_response_scale: 1.0,
				source_bounce_response_scale: 1.0,
				source_motion_coupling_scale: 1.0,
				shape_preservation: 0.0,
				rest_response: 0.0,
				bounce_response: 0.0,
				xpbd_compliance: 0.0,
				stretch_range_scale: 1.0,
				stretch_motion_override: None,
				gravity_scale: 1.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: UnaDynamicsImmobileType::default(),
				motion_coupling_override: None,
				drag_scale: 1.0,
				constraint_iterations: 1,
			},
		})];
		sim.active_runtime_indices = vec![0];

		let summaries = sim.collider_selection_summaries();
		assert_eq!(summaries.len(), 1);
		assert_eq!(summaries[0].selected_collider_count, 2);
		assert_eq!(summaries[0].global_collider_count, 1);
		assert_eq!(summaries[0].authored_collider_count, 1);
		assert_eq!(summaries[0].sample_collider_indices, vec![0, 1]);
		assert_eq!(summaries[0].sample_collider_source_ids, vec!["", "physbone:hair"]);
		assert_eq!(summaries[0].sample_collider_paths, vec!["global", "root/BodyColliders/Hair"]);
	}

	#[test]
	fn tail_collision_uses_particle_endpoint_not_bone_segment() {
		let colliders = vec![WorldBoneColliderPrimitive::Sphere {
			center: Vec3::ZERO,
			radius: 0.25,
			inside_bounds: false,
		}];
		let child_pos = Vec3::new(-1.0, 0.0, 0.0);
		let next_tail = Vec3::new(1.0, 0.0, 0.0);
		let pushed = constrain_tail_colliders(
			next_tail,
			child_pos,
			Vec3::X,
			2.0,
			WorldColliderSelection::new(&colliders, &[], true, &[]),
			0.0,
		);
		assert_eq!(pushed, next_tail);
	}

	#[test]
	fn surface_constraints_pull_connected_cross_group_tails_toward_rest_distance() {
		let scene = UnaSceneSnapshot {
			nodes: vec![
				node(0.0, Vec3::ZERO, vec![1, 3]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![2]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![]),
				node(0.0, Vec3::new(1.0, 1.0, 0.0), vec![4]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![]),
			],
			roots: vec![0],
			..Default::default()
		};
		let group = |source_id: &str, chain: Vec<usize>, gravity_x: f32| UnaSpringBoneGroup {
			interaction_chain_start_index: 0,
			source_kind: UnaDynamicsSourceKind::VrcPhysBone,
			enabled: true,
			source_id: source_id.to_string(),
			pull: 0.0,
			spring: 0.0,
			gravity_power: 2.0,
			gravity_dir: [gravity_x, 0.0, 0.0],
			drag_force: 0.0,
			bone_node_indices: chain,
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![group("left", vec![0, 1, 2], -1.0), group("right", vec![0, 3, 4], 1.0)],
			..Default::default()
		};
		let physics = DynamicsPhysicsConfig {
			simulation_hz: 60.0,
			..Default::default()
		}
		.normalized();
		let measure = |sim: &DynamicsSimulator| {
			let tails = sim
				.runtimes
				.iter()
				.filter_map(Option::as_ref)
				.map(|runtime| runtime.joints[0].curr_tail)
				.collect::<Vec<_>>();
			tails[0].distance(tails[1])
		};
		let offset_second_tail = |sim: &mut DynamicsSimulator, offset: Vec3| {
			let runtime = sim.runtimes.iter_mut().filter_map(Option::as_mut).nth(1).expect("second runtime");
			let joint = runtime.joints.get_mut(0).expect("joint");
			joint.curr_tail += offset;
			joint.prev_tail += offset;
		};
		let mut free_scene = scene.clone();
		let mut free_sim = DynamicsSimulator::new_with_config(&free_scene, &settings, Vec::new(), physics.clone()).expect("free sim");
		let mut constrained_scene = scene.clone();
		let mut constrained_sim = DynamicsSimulator::new_with_runtime_dynamics_collider_sources_and_surface_constraints(
			&constrained_scene,
			settings.runtime_dynamics(),
			Vec::new(),
			physics,
			&[DynamicsSurfaceConstraint {
				node_a: 1,
				node_b: 3,
				rest_distance: 1.0,
				stiffness: 1.0,
			}],
		)
		.expect("constrained sim");
		assert_eq!(constrained_sim.surface_constraint_count(), 1);
		offset_second_tail(&mut free_sim, Vec3::new(0.5, 0.0, 0.0));
		offset_second_tail(&mut constrained_sim, Vec3::new(0.5, 0.0, 0.0));
		free_sim.step(&mut free_scene, &settings, 1.0 / 60.0);
		constrained_sim.step(&mut constrained_scene, &settings, 1.0 / 60.0);
		let free_distance = measure(&free_sim);
		let constrained_distance = measure(&constrained_sim);
		assert!(
			constrained_distance < free_distance - 0.1,
			"constraint should reduce cross-chain tail separation: free={free_distance} constrained={constrained_distance}"
		);
		assert!(
			(constrained_distance - 1.0).abs() < (free_distance - 1.0).abs(),
			"constraint should keep the connected surface closer to rest distance: free={free_distance} constrained={constrained_distance}"
		);
	}

	#[test]
	fn post_surface_collider_projection_keeps_surface_constraints_out_of_colliders() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				node(0.0, Vec3::ZERO, vec![1, 3]),
				node(0.0, Vec3::ZERO, vec![2]),
				node(0.0, Vec3::X, vec![]),
				node(0.0, Vec3::new(4.0, 0.0, 0.0), vec![4]),
				node(0.0, Vec3::X, vec![]),
			],
			roots: vec![0],
			..Default::default()
		};
		let group = |source_id: &str, chain: Vec<usize>| UnaSpringBoneGroup {
			interaction_chain_start_index: 0,
			source_kind: UnaDynamicsSourceKind::VrcPhysBone,
			enabled: true,
			source_id: source_id.to_string(),
			pull: 0.0,
			spring: 0.0,
			stiffness: 0.0,
			gravity_power: 0.0,
			drag_force: 0.0,
			bone_node_indices: chain,
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![group("left", vec![0, 1, 2]), group("right", vec![0, 3, 4])],
			..Default::default()
		};
		let sphere_center = Vec3::new(2.3, 0.0, 0.0);
		let sphere_radius = 0.4;
		let surface_only_left_tail = Vec3::new(2.5, 0.0, 0.0);
		assert!(surface_only_left_tail.distance(sphere_center) < sphere_radius);
		let colliders = vec![RuntimeBoneColliderPrimitive {
			primitive: BoneColliderPrimitive::LocalSphere {
				node: 0,
				center: sphere_center.to_array(),
				radius: sphere_radius,
				inside_bounds: false,
			},
			source_id: String::new(),
			collider_path: "global/sphere".to_string(),
		}];
		let physics = DynamicsPhysicsConfig {
			simulation_hz: 60.0,
			..Default::default()
		}
		.normalized();
		let mut sim = DynamicsSimulator::new_with_runtime_dynamics_collider_sources_and_surface_constraints(
			&scene,
			settings.runtime_dynamics(),
			colliders,
			physics,
			&[DynamicsSurfaceConstraint {
				node_a: 1,
				node_b: 3,
				rest_distance: 1.0,
				stiffness: 1.0,
			}],
		)
		.expect("sim");
		assert_eq!(sim.surface_constraint_count(), 1);

		sim.step(&mut scene, &settings, 1.0 / 60.0);

		let left_joint = &sim.runtimes[0].as_ref().expect("left runtime").joints[0];
		let collision_radius = sphere_radius + left_joint.hit_radius;
		assert!(
			left_joint.curr_tail.distance(sphere_center) >= collision_radius - 1e-4,
			"post-surface collider projection should leave the corrected tail outside the collider: tail={:?}",
			left_joint.curr_tail
		);
		assert!(
			left_joint.curr_tail.distance(left_joint.prev_tail) <= 1e-6,
			"post-surface collider projection should not become next-frame velocity"
		);
	}

	#[test]
	fn simulator_filters_inactive_asset_group_dynamics() {
		let scene = UnaSceneSnapshot {
			nodes: vec![
				node(0.0, Vec3::ZERO, vec![1]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![2]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![]),
			],
			roots: vec![0],
			asset_group_ownership: vec![
				UnaSceneAssetGroupOwnership {
					group_id: "outfit:white".to_string(),
					dynamics_source_ids: vec!["physbone:white-cloth-panel".to_string()],
					..Default::default()
				},
				UnaSceneAssetGroupOwnership {
					group_id: "outfit:black".to_string(),
					dynamics_source_ids: vec!["physbone:black-cloth-panel".to_string()],
					..Default::default()
				},
			],
			..Default::default()
		};
		let cloth_panel_group = |source_id: &str| UnaSpringBoneGroup {
			interaction_chain_start_index: 0,
			source_kind: UnaDynamicsSourceKind::VrcPhysBone,
			enabled: true,
			source_id: source_id.to_string(),
			pull: 0.2,
			spring: 0.2,
			gravity_power: 1.0,
			gravity_dir: [0.0, -1.0, 0.0],
			drag_force: 0.4,
			bone_node_indices: vec![0, 1, 2],
			..Default::default()
		};
		let mut document = UnaDocument {
			scene: Some(scene),
			spring_bones: Some(UnaSpringBoneSettings {
				groups: vec![
					cloth_panel_group("physbone:white-cloth-panel"),
					cloth_panel_group("physbone:black-cloth-panel"),
				],
				..Default::default()
			}),
			..Default::default()
		};
		document
			.runtime_model_mut()
			.set_active_asset_groups(vec!["outfit:black".to_string()]);

		let scene = document.scene.as_ref().expect("scene");
		let dynamics = document.runtime_model().dynamics();
		let sim = DynamicsSimulator::new_with_runtime_dynamics_and_collider_sources(
			scene,
			dynamics,
			Vec::new(),
			DynamicsPhysicsConfig::default(),
		)
		.expect("sim");
		let summaries = sim.response_group_summaries();
		assert_eq!(summaries.len(), 1);
		assert_eq!(summaries[0].source_id, "physbone:black-cloth-panel");
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
				interaction_chain_start_index: 0,
				source_kind: Default::default(),
				enabled: true,
				source_id: String::new(),
				comment: String::new(),
				category: String::new(),
				stiffness: 0.05,
				pull: 0.05,
				spring: 0.0,
				integration_type: Default::default(),
				gravity_power: 2.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [1.0, 0.0, 0.0],
				drag_force: 0.2,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1, 2],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let mut sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
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
	fn unphysics_rest_response_drives_gravity_bias_without_source_kind_branch() {
		fn drift_after_rest_response(rest_response: f32) -> f32 {
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
					interaction_chain_start_index: 0,
					source_kind: Default::default(),
					enabled: true,
					source_id: String::new(),
					comment: String::new(),
					category: String::new(),
					stiffness: 0.0,
					pull: rest_response,
					spring: 0.0,
					integration_type: Default::default(),
					gravity_power: 2.0,
					gravity_falloff: 0.0,
					immobile: 0.0,
					immobile_type: Default::default(),
					gravity_dir: [1.0, 0.0, 0.0],
					drag_force: 0.2,
					center_node: None,
					hit_radius: 0.0,
					hit_radius_samples: Vec::new(),
					stiffness_samples: Vec::new(),
					pull_samples: Vec::new(),
					spring_samples: Vec::new(),
					gravity_power_samples: Vec::new(),
					gravity_falloff_samples: Vec::new(),
					immobile_samples: Vec::new(),
					max_angle_x_samples: Vec::new(),
					max_angle_z_samples: Vec::new(),
					writeback_mode: UnaDynamicsWritebackMode::RotationTranslation,
					limit: Some(UnaDynamicsLimit {
						limit_type: String::new(),
						limit_rotation: [0.0, 0.0, 0.0],
						max_angle_x: 0.0,
						max_angle_z: 0.0,
						max_stretch: 1.0,
						max_squish: 0.0,
						stretch_motion: None,
						max_stretch_samples: Vec::new(),
						max_squish_samples: Vec::new(),
						stretch_motion_samples: Vec::new(),
					}),
					interaction: None,
					bone_node_indices: vec![0, 1, 2],
				}],
				colliders: Vec::new(),
				..Default::default()
			};
			let mut sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
			let tip_before = world_from_snapshot(&scene)[2].transform_point3(Vec3::ZERO);
			for _ in 0..60 {
				sim.step(&mut scene, &settings, 1.0 / 60.0);
			}
			let tip_after = world_from_snapshot(&scene)[2].transform_point3(Vec3::ZERO);
			(tip_after.x - tip_before.x).abs()
		}

		let loose = drift_after_rest_response(0.0);
		let pulled = drift_after_rest_response(2.0);
		assert!(
			pulled > loose,
			"UNPhysics rest_response should make gravity_bias reachable without checking source kind: loose={loose} pulled={pulled}"
		);
	}

	#[test]
	fn unphysics_solver_response_depends_on_normalized_terms_not_source_kind() {
		fn tip_after_source_kind(source_kind: un_avatar_core::UnaDynamicsSourceKind) -> Vec3 {
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
					interaction_chain_start_index: 0,
					source_kind,
					enabled: true,
					source_id: format!("source:{source_kind:?}"),
					comment: String::new(),
					category: "tail".to_string(),
					stiffness: 0.18,
					pull: 0.32,
					spring: 0.28,
					integration_type: Default::default(),
					gravity_power: 1.6,
					gravity_falloff: 0.25,
					immobile: 0.35,
					immobile_type: Default::default(),
					gravity_dir: [1.0, -0.2, 0.0],
					drag_force: 0.22,
					center_node: None,
					hit_radius: 0.0,
					hit_radius_samples: Vec::new(),
					stiffness_samples: vec![0.20, 0.10],
					pull_samples: vec![0.34, 0.22],
					spring_samples: vec![0.25, 0.31],
					gravity_power_samples: vec![1.0, 0.7],
					gravity_falloff_samples: vec![0.0, 0.35],
					immobile_samples: vec![0.2, 0.5],
					max_angle_x_samples: Vec::new(),
					max_angle_z_samples: Vec::new(),
					writeback_mode: Default::default(),
					limit: None,
					interaction: None,
					bone_node_indices: vec![0, 1, 2],
				}],
				colliders: Vec::new(),
				..Default::default()
			};
			let mut sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
			for _ in 0..72 {
				sim.step(&mut scene, &settings, 1.0 / 60.0);
			}
			world_from_snapshot(&scene)[2].transform_point3(Vec3::ZERO)
		}

		let from_vrm = tip_after_source_kind(un_avatar_core::UnaDynamicsSourceKind::VrmSpringBone);
		let from_vrc = tip_after_source_kind(un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone);
		assert!(
			(from_vrm - from_vrc).length() < 1e-6,
			"normalized UNDynamics terms must produce source-neutral solver output: vrm={from_vrm:?} vrc={from_vrc:?}"
		);
	}

	#[test]
	fn normalized_gravity_falloff_reduces_aligned_gravity_without_source_kind_branch() {
		let full_gravity = gravity_with_falloff(Vec3::Y, 2.0, Vec3::Y, 0.0);
		let fallen_off = gravity_with_falloff(Vec3::Y, 2.0, Vec3::Y, 1.0);
		let perpendicular = gravity_with_falloff(Vec3::Y, 2.0, Vec3::X, 1.0);
		assert_eq!(full_gravity, Vec3::Y * 2.0);
		assert_eq!(fallen_off, Vec3::ZERO);
		assert_eq!(perpendicular, Vec3::Y * 2.0);
	}

	#[test]
	fn unphysics_response_gain_is_shaped_and_frame_rate_independent() {
		let low = unphysics_response_gain(0.10, 1.0 / 60.0);
		let mid = unphysics_response_gain(0.30, 1.0 / 60.0);
		let high = unphysics_response_gain(0.80, 1.0 / 60.0);
		assert!(low > 0.0 && low < mid && mid < high);
		assert!(
			mid < 0.30,
			"UNPhysics response should not treat normalized mid-range values as direct per-frame snap: mid={mid}"
		);
		let full_step = unphysics_response_gain(0.30, 1.0 / 60.0);
		let half_step = unphysics_response_gain(0.30, 1.0 / 120.0);
		let combined_half_steps = 1.0 - (1.0 - half_step) * (1.0 - half_step);
		assert!((combined_half_steps - full_step).abs() < 1e-5);
		assert!(unphysics_response_gain(2.0, 1.0 / 60.0) < 1.0);
	}

	#[test]
	fn unphysics_rest_response_is_not_treated_as_hz_rate() {
		let start = Vec3::new(1.0, 0.0, 0.0);
		let target = Vec3::ZERO;
		let rest_response = 0.183;
		let correct_step = start + (target - start) * unphysics_response_gain(rest_response, 1.0 / 60.0);
		let hz_like_step = start + (target - start) * (rest_response / 60.0);
		assert!(
			correct_step.distance(target) < hz_like_step.distance(target),
			"UNPhysics normalized rest_response should not be damped as a Hz-like authored rate: correct={correct_step:?} hz_like={hz_like_step:?}"
		);
	}

	#[test]
	fn unphysics_displacement_response_boosts_large_deflection_only() {
		let base = unphysics_response_gain(0.08, 1.0 / 60.0);
		let small = unphysics_displacement_response_gain(0.08, 1.0 / 60.0, 0.1, 1.0, 0.20, 2);
		let large = unphysics_displacement_response_gain(0.08, 1.0 / 60.0, 2.0, 1.0, 0.20, 2);
		assert_eq!(unphysics_displacement_boost(0.1, 1.0, 0.08, 0.20, 2), 0.0);
		assert!(unphysics_displacement_boost(2.0, 1.0, 0.08, 0.20, 2) > 0.0);
		assert_eq!(
			unphysics_displacement_boost(2.0, 1.0, 0.30, 0.20, 2),
			0.0,
			"already firm authored response should not receive hidden large-deflection boost"
		);
		assert_eq!(
			unphysics_displacement_boost(2.0, 1.0, 0.08, 0.70, 2),
			0.0,
			"firm parent-motion follow should not receive hidden large-deflection boost"
		);
		assert!((small - base).abs() < 1e-6, "small deflections should keep authored softness");
		assert!(
			large > base * 2.0,
			"large deflections should get nonlinear UNPhysics recovery: base={base} large={large}"
		);
		assert!(large < 1.0);
	}

	#[test]
	fn unphysics_long_chain_distributed_deflection_gets_recovery_floor() {
		let short = unphysics_displacement_boost(0.25, 1.0, 0.08, 0.20, 2);
		let long = unphysics_displacement_boost(0.25, 1.0, 0.08, 0.20, 12);
		let firm_long = unphysics_displacement_boost(0.25, 1.0, 0.30, 0.20, 12);
		let followed_long = unphysics_displacement_boost(0.25, 1.0, 0.08, 0.70, 12);
		assert_eq!(short, 0.0, "short local sway should not be auto-hardened");
		assert!(long > 0.0, "long chains need a recovery floor for distributed deflection");
		assert_eq!(firm_long, 0.0, "firm long chains already have enough rest response");
		assert_eq!(followed_long, 0.0, "well-followed long chains should not get hidden recovery");
	}

	#[test]
	fn unphysics_bounce_retention_never_amplifies_inertia() {
		let no_bounce = unphysics_inertia_retention(0.12, 0.0);
		let high_bounce = unphysics_inertia_retention(0.12, 1.0);
		let near_undamped = unphysics_inertia_retention(0.01, 1.0);
		assert!(high_bounce > no_bounce, "bounce should preserve more residual motion");
		assert!(
			high_bounce < 1.0 && near_undamped < 1.0,
			"bounce must not create energy by pushing inertia retention over 1: high={high_bounce} near={near_undamped}"
		);
	}

	#[test]
	fn unphysics_bounce_response_summary_reports_solver_effective_range() {
		let scene = UnaSceneSnapshot {
			nodes: vec![node(0.0, Vec3::ZERO, vec![1]), node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![])],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				interaction_chain_start_index: 0,
				enabled: true,
				source_id: "physbone:high-bounce".to_string(),
				category: "hair".to_string(),
				pull: 0.2,
				spring: 2.0,
				drag_force: 0.2,
				bone_node_indices: vec![0, 1],
				..Default::default()
			}],
			..Default::default()
		};
		let sim = DynamicsSimulator::new_with_config(
			&scene,
			&settings,
			Vec::new(),
			DynamicsPhysicsConfig {
				overrides: vec![DynamicsCategoryOverride {
					category: "hair".to_string(),
					params: DynamicsPhysicsParams {
						bounce_scale: Some(2.0),
						..Default::default()
					},
				}],
				..Default::default()
			},
		)
		.expect("sim");
		let runtime = sim.runtimes[0].as_ref().expect("runtime");
		assert_eq!(runtime.joints[0].bounce_response, 1.0);
		let summary = sim.response_group_summaries().into_iter().next().expect("summary");
		assert_eq!(summary.average_bounce_response, 1.0);
		assert_eq!(summary.max_bounce_response, 1.0);
	}

	#[test]
	fn unphysics_animal_ears_sway_then_recover_from_vrc_source_terms() {
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
				interaction_chain_start_index: 0,
				source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
				enabled: true,
				source_id: String::new(),
				comment: "animal ears".to_string(),
				category: "ears".to_string(),
				stiffness: 0.228,
				pull: 0.183,
				spring: 0.75,
				integration_type: UnaDynamicsIntegrationType::VrcAdvanced,
				gravity_power: 0.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.4,
				center_node: None,
				hit_radius: 0.02,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1, 2],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let mut sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
		scene.nodes[0].transform = Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2).to_cols_array();
		sim.step(&mut scene, &settings, 1.0 / 60.0);
		let after_turn = world_from_snapshot(&scene)[2].transform_point3(Vec3::ZERO);
		let rest_after_turn = Vec3::new(-2.0, 0.0, 0.0);
		let initial_deflection = after_turn.distance(rest_after_turn);
		assert!(
			initial_deflection > 0.1,
			"animal ears should still visibly sway after head rotation: after_turn={after_turn:?}"
		);

		for _ in 0..120 {
			sim.step(&mut scene, &settings, 1.0 / 60.0);
		}
		let recovered = world_from_snapshot(&scene)[2].transform_point3(Vec3::ZERO);
		assert!(
			recovered.distance(rest_after_turn) < initial_deflection * 0.5,
			"UNPhysics rest_response should recover animal ears toward rest instead of leaving them sluggishly folded: initial={initial_deflection} recovered={recovered:?}"
		);
	}

	#[test]
	fn unphysics_animal_ears_lag_during_smooth_head_turn_from_vrc_source_terms() {
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
				interaction_chain_start_index: 0,
				source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
				enabled: true,
				source_id: String::new(),
				comment: "animal ears".to_string(),
				category: "ears".to_string(),
				stiffness: 0.228,
				pull: 0.183,
				spring: 0.75,
				integration_type: UnaDynamicsIntegrationType::VrcAdvanced,
				gravity_power: 0.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.4,
				center_node: None,
				hit_radius: 0.02,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1, 2],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let mut sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
		let mut max_lag = 0.0_f32;
		for frame in 1..=30 {
			let angle = std::f32::consts::FRAC_PI_2 * frame as f32 / 30.0;
			scene.nodes[0].transform = Mat4::from_rotation_z(angle).to_cols_array();
			sim.step(&mut scene, &settings, 1.0 / 60.0);
			let tip = world_from_snapshot(&scene)[2].transform_point3(Vec3::ZERO);
			let rest = Mat4::from_rotation_z(angle).transform_point3(Vec3::new(0.0, 2.0, 0.0));
			max_lag = max_lag.max(tip.distance(rest));
		}
		assert!(
			max_lag > 0.06,
			"smooth head motion should still produce visible UNPhysics lag instead of rigid rest-pose tracking: max_lag={max_lag}"
		);
	}

	#[test]
	fn unphysics_motion_coupling_override_changes_head_motion_lag() {
		fn max_lag_with_motion_coupling(motion_coupling: f32) -> f32 {
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
					interaction_chain_start_index: 0,
					source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
					enabled: true,
					source_id: String::new(),
					comment: "profile-adjustable ears".to_string(),
					category: "ears".to_string(),
					stiffness: 0.12,
					pull: 0.10,
					spring: 0.65,
					integration_type: UnaDynamicsIntegrationType::VrcAdvanced,
					gravity_power: 0.0,
					gravity_falloff: 0.0,
					immobile: 0.0,
					immobile_type: Default::default(),
					gravity_dir: [0.0, -1.0, 0.0],
					drag_force: 0.35,
					center_node: None,
					hit_radius: 0.0,
					hit_radius_samples: Vec::new(),
					stiffness_samples: Vec::new(),
					pull_samples: Vec::new(),
					spring_samples: Vec::new(),
					gravity_power_samples: Vec::new(),
					gravity_falloff_samples: Vec::new(),
					immobile_samples: Vec::new(),
					max_angle_x_samples: Vec::new(),
					max_angle_z_samples: Vec::new(),
					writeback_mode: Default::default(),
					limit: None,
					interaction: None,
					bone_node_indices: vec![0, 1, 2],
				}],
				..Default::default()
			};
			let config = DynamicsPhysicsConfig {
				overrides: vec![DynamicsCategoryOverride {
					category: "ears".to_string(),
					params: DynamicsPhysicsParams {
						solver: Some(DynamicsSolver::Verlet),
						rest_response: Some(0.10),
						bounce_scale: Some(0.6),
						motion_coupling: Some(motion_coupling),
						..Default::default()
					},
				}],
				..Default::default()
			};
			let mut sim = DynamicsSimulator::new_with_config(&scene, &settings, Vec::new(), config).expect("sim");
			let mut max_lag = 0.0_f32;
			for frame in 1..=30 {
				let angle = std::f32::consts::FRAC_PI_2 * frame as f32 / 30.0;
				scene.nodes[0].transform = Mat4::from_rotation_z(angle).to_cols_array();
				sim.step(&mut scene, &settings, 1.0 / 60.0);
				let tip = world_from_snapshot(&scene)[2].transform_point3(Vec3::ZERO);
				let rest = Mat4::from_rotation_z(angle).transform_point3(Vec3::new(0.0, 2.0, 0.0));
				max_lag = max_lag.max(tip.distance(rest));
			}
			max_lag
		}

		let loose = max_lag_with_motion_coupling(0.15);
		let firm = max_lag_with_motion_coupling(0.90);
		assert!(
			loose > firm + 0.10,
			"lower motion_coupling should visibly increase head-motion lag: loose={loose} firm={firm}"
		);
	}

	#[test]
	fn unphysics_rest_response_override_changes_recovery_speed() {
		fn remaining_deflection_after_recovery(rest_response: f32) -> f32 {
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
					interaction_chain_start_index: 0,
					source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
					enabled: true,
					source_id: String::new(),
					comment: "profile-adjustable ears".to_string(),
					category: "ears".to_string(),
					stiffness: 0.12,
					pull: 0.10,
					spring: 0.45,
					integration_type: UnaDynamicsIntegrationType::VrcAdvanced,
					gravity_power: 0.0,
					gravity_falloff: 0.0,
					immobile: 0.0,
					immobile_type: Default::default(),
					gravity_dir: [0.0, -1.0, 0.0],
					drag_force: 0.35,
					center_node: None,
					hit_radius: 0.0,
					hit_radius_samples: Vec::new(),
					stiffness_samples: Vec::new(),
					pull_samples: Vec::new(),
					spring_samples: Vec::new(),
					gravity_power_samples: Vec::new(),
					gravity_falloff_samples: Vec::new(),
					immobile_samples: Vec::new(),
					max_angle_x_samples: Vec::new(),
					max_angle_z_samples: Vec::new(),
					writeback_mode: Default::default(),
					limit: None,
					interaction: None,
					bone_node_indices: vec![0, 1, 2],
				}],
				..Default::default()
			};
			let config = DynamicsPhysicsConfig {
				overrides: vec![DynamicsCategoryOverride {
					category: "ears".to_string(),
					params: DynamicsPhysicsParams {
						solver: Some(DynamicsSolver::Verlet),
						rest_response: Some(rest_response),
						bounce_scale: Some(0.4),
						motion_coupling: Some(0.35),
						..Default::default()
					},
				}],
				..Default::default()
			};
			let mut sim = DynamicsSimulator::new_with_config(&scene, &settings, Vec::new(), config).expect("sim");
			scene.nodes[0].transform = Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2).to_cols_array();
			sim.step(&mut scene, &settings, 1.0 / 60.0);
			let rest_after_turn = Vec3::new(-2.0, 0.0, 0.0);
			for _ in 0..45 {
				sim.step(&mut scene, &settings, 1.0 / 60.0);
			}
			let tip = world_from_snapshot(&scene)[2].transform_point3(Vec3::ZERO);
			tip.distance(rest_after_turn)
		}

		let soft = remaining_deflection_after_recovery(0.02);
		let firm = remaining_deflection_after_recovery(0.30);
		assert!(
			firm < soft * 0.6,
			"higher rest_response should recover substantially faster: soft={soft} firm={firm}"
		);
	}

	#[test]
	fn unphysics_source_stiffness_does_not_bypass_soft_pull_and_motion_coupling() {
		fn lag_after_smooth_turn(source_stiffness: f32) -> f32 {
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
					interaction_chain_start_index: 0,
					source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
					enabled: true,
					source_id: String::new(),
					comment: "soft authored pull with high source stiffness".to_string(),
					category: "ears".to_string(),
					stiffness: source_stiffness,
					pull: 0.02,
					spring: 0.0,
					integration_type: UnaDynamicsIntegrationType::VrcAdvanced,
					gravity_power: 0.0,
					gravity_falloff: 0.0,
					immobile: 0.0,
					immobile_type: Default::default(),
					gravity_dir: [0.0, -1.0, 0.0],
					drag_force: 0.30,
					center_node: None,
					hit_radius: 0.0,
					hit_radius_samples: Vec::new(),
					stiffness_samples: Vec::new(),
					pull_samples: Vec::new(),
					spring_samples: Vec::new(),
					gravity_power_samples: Vec::new(),
					gravity_falloff_samples: Vec::new(),
					immobile_samples: Vec::new(),
					max_angle_x_samples: Vec::new(),
					max_angle_z_samples: Vec::new(),
					writeback_mode: Default::default(),
					limit: None,
					interaction: None,
					bone_node_indices: vec![0, 1, 2],
				}],
				..Default::default()
			};
			let config = DynamicsPhysicsConfig {
				overrides: vec![DynamicsCategoryOverride {
					category: "ears".to_string(),
					params: DynamicsPhysicsParams {
						solver: Some(DynamicsSolver::Verlet),
						shape_preservation: Some(0.02),
						motion_coupling: Some(0.15),
						..Default::default()
					},
				}],
				..Default::default()
			};
			let mut sim = DynamicsSimulator::new_with_config(&scene, &settings, Vec::new(), config).expect("sim");
			let mut max_lag = 0.0_f32;
			for frame in 1..=30 {
				let angle = std::f32::consts::FRAC_PI_2 * frame as f32 / 30.0;
				scene.nodes[0].transform = Mat4::from_rotation_z(angle).to_cols_array();
				sim.step(&mut scene, &settings, 1.0 / 60.0);
				let tip = world_from_snapshot(&scene)[2].transform_point3(Vec3::ZERO);
				let rest = Mat4::from_rotation_z(angle).transform_point3(Vec3::new(0.0, 2.0, 0.0));
				max_lag = max_lag.max(tip.distance(rest));
			}
			max_lag
		}

		let soft_shape = lag_after_smooth_turn(0.02);
		let high_source_stiffness = lag_after_smooth_turn(1.0);
		assert!(
			(high_source_stiffness - soft_shape).abs() < 0.05,
			"high source stiffness must not create hidden rigid tracking when pull and motion_coupling are soft: soft={soft_shape} high={high_source_stiffness}"
		);
	}

	#[test]
	fn unphysics_gravity_falloff_reduces_aligned_rest_gravity() {
		let child = Vec3::ZERO;
		let axis = Vec3::Y;
		let gravity = Vec3::Y;
		let full = unphysics_gravity_rest_target(child, axis, 1.0, 1.0, gravity, 1.0, 0.0);
		let fallen_off = unphysics_gravity_rest_target(child, axis, 1.0, 1.0, gravity, 1.0, 1.0);
		assert!(
			fallen_off.distance(axis) < full.distance(axis) + 1e-6,
			"falloff should not increase aligned gravity displacement: full={full:?} fallen_off={fallen_off:?}"
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
				interaction_chain_start_index: 0,
				source_kind: Default::default(),
				enabled: true,
				source_id: String::new(),
				comment: String::new(),
				category: String::new(),
				stiffness: 0.0,
				pull: 1.0,
				spring: 0.0,
				integration_type: Default::default(),
				gravity_power: 30.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [1.0, 0.0, 0.0],
				drag_force: 0.0,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: Some(UnaDynamicsLimit {
					limit_type: "Angle".to_string(),
					limit_rotation: [0.0, 0.0, 0.0],
					max_angle_x: 10.0,
					max_angle_z: 0.0,
					max_stretch: 0.0,
					max_squish: 0.0,
					stretch_motion: None,
					max_stretch_samples: Vec::new(),
					max_squish_samples: Vec::new(),
					stretch_motion_samples: Vec::new(),
				}),
				interaction: None,
				bone_node_indices: vec![0, 1, 2],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let mut sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
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

	#[test]
	fn dynamics_angle_limit_uses_per_joint_curve_samples() {
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				interaction_chain_start_index: 0,
				source_kind: Default::default(),
				enabled: true,
				source_id: String::new(),
				comment: String::new(),
				category: String::new(),
				stiffness: 0.0,
				pull: 1.0,
				spring: 0.0,
				integration_type: Default::default(),
				gravity_power: 0.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.0,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: vec![5.0, 35.0],
				max_angle_z_samples: vec![7.0, 45.0],
				writeback_mode: Default::default(),
				limit: Some(UnaDynamicsLimit {
					limit_type: "Polar".to_string(),
					limit_rotation: [0.0, 0.0, 0.0],
					max_angle_x: 90.0,
					max_angle_z: 90.0,
					max_stretch: 0.0,
					max_squish: 0.0,
					stretch_motion: None,
					max_stretch_samples: Vec::new(),
					max_squish_samples: Vec::new(),
					stretch_motion_samples: Vec::new(),
				}),
				interaction: None,
				bone_node_indices: vec![0, 1, 2],
			}],
			..Default::default()
		};
		let dynamics = settings.runtime_dynamics();
		let group = dynamics.dynamics_group(0).expect("group");
		let root_limit = sampled_joint_limit(group, 0).expect("root limit");
		let tip_limit = sampled_joint_limit(group, 1).expect("tip limit");
		assert_eq!(root_limit.max_angle_x, 5.0);
		assert_eq!(root_limit.max_angle_z, 7.0);
		assert_eq!(tip_limit.max_angle_x, 35.0);
		assert_eq!(tip_limit.max_angle_z, 45.0);
	}

	#[test]
	fn dynamics_hinge_limit_projects_motion_to_hinge_plane() {
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
				interaction_chain_start_index: 0,
				source_kind: Default::default(),
				enabled: true,
				source_id: String::new(),
				comment: String::new(),
				category: String::new(),
				stiffness: 0.0,
				pull: 1.0,
				spring: 0.0,
				integration_type: Default::default(),
				gravity_power: 30.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [1.0, 0.0, 0.0],
				drag_force: 0.0,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: Some(UnaDynamicsLimit {
					limit_type: "Hinge".to_string(),
					limit_rotation: [0.0, 0.0, 0.0],
					max_angle_x: 90.0,
					max_angle_z: 0.0,
					max_stretch: 0.0,
					max_squish: 0.0,
					stretch_motion: None,
					max_stretch_samples: Vec::new(),
					max_squish_samples: Vec::new(),
					stretch_motion_samples: Vec::new(),
				}),
				interaction: None,
				bone_node_indices: vec![0, 1, 2],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let mut sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
		for _ in 0..60 {
			sim.step(&mut scene, &settings, 1.0 / 60.0);
		}
		let world = world_from_snapshot(&scene);
		let joint = world[1].transform_point3(Vec3::ZERO);
		let tip = world[2].transform_point3(Vec3::ZERO);
		let axis = (tip - joint).normalize_or_zero();
		assert!(
			axis.x.abs() < 1e-4,
			"Hinge limit should project motion onto the hinge plane instead of allowing cone drift: axis={axis:?}"
		);
	}

	#[test]
	fn dynamics_angle_limit_rotation_changes_center_axis() {
		let limit = UnaDynamicsLimit {
			limit_type: "Angle".to_string(),
			limit_rotation: [0.0, 0.0, 90.0],
			max_angle_x: 0.0,
			max_angle_z: 0.0,
			max_stretch: 0.0,
			max_squish: 0.0,
			stretch_motion: None,
			max_stretch_samples: Vec::new(),
			max_squish_samples: Vec::new(),
			stretch_motion_samples: Vec::new(),
		};
		let constrained = constrain_axis_by_limit_type(Vec3::Y, Vec3::Y, &limit);
		assert!(
			constrained.distance(Vec3::NEG_X) < 1e-4,
			"limit Rotation should rotate the center axis used by Angle limits: constrained={constrained:?}"
		);
	}

	#[test]
	fn dynamics_polar_limit_uses_authored_x_and_z_axes() {
		let limit = UnaDynamicsLimit {
			limit_type: "Polar".to_string(),
			limit_rotation: [0.0, 0.0, 0.0],
			max_angle_x: 30.0,
			max_angle_z: 70.0,
			max_stretch: 0.0,
			max_squish: 0.0,
			stretch_motion: None,
			max_stretch_samples: Vec::new(),
			max_squish_samples: Vec::new(),
			stretch_motion_samples: Vec::new(),
		};
		let x_constrained = constrain_axis_by_limit_type((Vec3::X * 10.0 + Vec3::Y).normalize(), Vec3::Y, &limit);
		let z_constrained = constrain_axis_by_limit_type((Vec3::Z * 10.0 + Vec3::Y).normalize(), Vec3::Y, &limit);
		let x_angle = x_constrained.x.atan2(x_constrained.y).abs().to_degrees();
		let z_angle = z_constrained.z.atan2(z_constrained.y).abs().to_degrees();
		assert!(
			(x_angle - 30.0).abs() < 1e-3,
			"Polar maxAngleX should clamp motion along local X: angle={x_angle} axis={x_constrained:?}"
		);
		assert!(
			(z_angle - 70.0).abs() < 1e-3,
			"Polar maxAngleZ should clamp motion along local Z: angle={z_angle} axis={z_constrained:?}"
		);
	}

	#[test]
	fn dynamics_cloth_category_clears_angular_limits_but_keeps_stretch_terms() {
		let mut cloth_limit = UnaDynamicsLimit {
			limit_type: "Polar".to_string(),
			limit_rotation: [0.0, 0.0, 0.0],
			max_angle_x: 45.0,
			max_angle_z: 90.0,
			max_stretch: 0.2,
			max_squish: 0.1,
			stretch_motion: Some(0.5),
			max_stretch_samples: vec![0.2],
			max_squish_samples: vec![0.1],
			stretch_motion_samples: vec![0.5],
		};
		apply_dynamics_limit_category_adjustments(&mut cloth_limit, "cloth");
		assert!(cloth_limit.limit_type.is_empty());
		assert_eq!(cloth_limit.max_angle_x, 0.0);
		assert_eq!(cloth_limit.max_angle_z, 0.0);
		assert_eq!(cloth_limit.max_stretch, 0.2);
		assert_eq!(cloth_limit.max_squish, 0.1);
		assert_eq!(cloth_limit.stretch_motion, Some(0.5));

		let mut hair_limit = cloth_limit.clone();
		hair_limit.limit_type = "Polar".to_string();
		hair_limit.max_angle_x = 45.0;
		hair_limit.max_angle_z = 90.0;
		apply_dynamics_limit_category_adjustments(&mut hair_limit, "hair");
		assert_eq!(hair_limit.limit_type, "Polar");
		assert_eq!(hair_limit.max_angle_x, 45.0);
		assert_eq!(hair_limit.max_angle_z, 90.0);
	}

	#[test]
	fn unphysics_constraint_projection_does_not_become_next_frame_inertia() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![node(0.0, Vec3::ZERO, vec![1]), node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![])],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				enabled: true,
				source_id: "constraint-projection".to_string(),
				category: "hair".to_string(),
				pull: 0.0,
				spring: 0.0,
				drag_force: 1.0,
				bone_node_indices: vec![0, 1],
				limit: Some(UnaDynamicsLimit {
					limit_type: "Angle".to_string(),
					limit_rotation: [0.0, 0.0, 0.0],
					max_angle_x: 0.0,
					max_angle_z: 0.0,
					max_stretch: 0.0,
					max_squish: 0.0,
					stretch_motion: None,
					max_stretch_samples: Vec::new(),
					max_squish_samples: Vec::new(),
					stretch_motion_samples: Vec::new(),
				}),
				..Default::default()
			}],
			..Default::default()
		};
		let mut sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
		scene.nodes[0].transform = Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2).to_cols_array();
		sim.step(&mut scene, &settings, 1.0 / 60.0);
		let joint = &sim.runtimes[0].as_ref().expect("runtime").joints[0];
		let projected_velocity = joint.curr_tail.distance(joint.prev_tail);
		assert!(
			projected_velocity < 1e-4,
			"constraint projection should not be stored as inertia for the next frame: prev={:?} curr={:?} velocity={projected_velocity}",
			joint.prev_tail,
			joint.curr_tail
		);
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
				interaction_chain_start_index: 0,
				source_kind: Default::default(),
				enabled: true,
				source_id: String::new(),
				comment: String::new(),
				category: String::new(),
				stiffness: 1.0,
				pull: 1.0,
				spring: 0.0,
				integration_type: Default::default(),
				gravity_power: 0.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.4,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1, 2],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let mut sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
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
	fn simulator_uses_per_joint_dynamics_samples() {
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
				interaction_chain_start_index: 0,
				source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
				enabled: true,
				source_id: String::new(),
				comment: String::new(),
				category: String::new(),
				stiffness: 1.0,
				pull: 1.0,
				spring: 0.0,
				integration_type: Default::default(),
				gravity_power: 0.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.4,
				center_node: None,
				hit_radius: 0.03,
				hit_radius_samples: vec![0.015, 0.006],
				stiffness_samples: vec![0.7, 0.8],
				pull_samples: vec![0.1, 0.2],
				spring_samples: vec![0.3, 0.4],
				gravity_power_samples: vec![0.5, 0.6],
				gravity_falloff_samples: vec![0.7, 0.8],
				immobile_samples: vec![0.9, 1.0],
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1, 2],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
		let runtime = sim.runtimes[0].as_ref().expect("runtime");
		assert_eq!(runtime.joints.len(), 2);
		assert!((runtime.joints[0].hit_radius - 0.015).abs() < 1e-6);
		assert!((runtime.joints[1].hit_radius - 0.006).abs() < 1e-6);
		assert!((runtime.joints[0].shape_preservation - 0.7).abs() < 1e-6);
		assert!((runtime.joints[1].shape_preservation - 0.8).abs() < 1e-6);
		assert!((runtime.joints[0].rest_response - 0.1).abs() < 1e-6);
		assert!((runtime.joints[1].rest_response - 0.2).abs() < 1e-6);
		assert!((runtime.joints[0].bounce_response - 0.3).abs() < 1e-6);
		assert!((runtime.joints[1].bounce_response - 0.4).abs() < 1e-6);
		assert!((runtime.joints[0].gravity_power - 0.5).abs() < 1e-6);
		assert!((runtime.joints[1].gravity_power - 0.6).abs() < 1e-6);
		assert!((runtime.joints[0].gravity_falloff - 0.7).abs() < 1e-6);
		assert!((runtime.joints[1].gravity_falloff - 0.8).abs() < 1e-6);
		assert!((runtime.joints[0].parent_motion_follow - 0.80756).abs() < 1e-6);
		assert!((runtime.joints[1].parent_motion_follow - 0.738).abs() < 1e-6);
	}

	#[test]
	fn unphysics_category_does_not_apply_hidden_solver_response() {
		let scene = UnaSceneSnapshot {
			nodes: vec![node(0.0, Vec3::ZERO, vec![1]), node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![])],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![
				UnaSpringBoneGroup {
					interaction_chain_start_index: 0,
					source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
					enabled: true,
					source_id: String::new(),
					comment: "Hair_Main".to_string(),
					category: String::new(),
					stiffness: 0.5,
					pull: 0.4,
					spring: 0.3,
					integration_type: Default::default(),
					gravity_power: 0.0,
					gravity_falloff: 0.0,
					immobile: 0.0,
					immobile_type: Default::default(),
					gravity_dir: [0.0, -1.0, 0.0],
					drag_force: 0.4,
					center_node: None,
					hit_radius: 0.0,
					hit_radius_samples: Vec::new(),
					stiffness_samples: Vec::new(),
					pull_samples: Vec::new(),
					spring_samples: Vec::new(),
					gravity_power_samples: Vec::new(),
					gravity_falloff_samples: Vec::new(),
					immobile_samples: Vec::new(),
					max_angle_x_samples: Vec::new(),
					max_angle_z_samples: Vec::new(),
					writeback_mode: Default::default(),
					limit: None,
					interaction: None,
					bone_node_indices: vec![0, 1],
				},
				UnaSpringBoneGroup {
					interaction_chain_start_index: 0,
					source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
					enabled: true,
					source_id: String::new(),
					comment: "Cloth panel".to_string(),
					category: String::new(),
					stiffness: 0.5,
					pull: 0.4,
					spring: 0.3,
					integration_type: Default::default(),
					gravity_power: 0.0,
					gravity_falloff: 0.0,
					immobile: 0.0,
					immobile_type: Default::default(),
					gravity_dir: [0.0, -1.0, 0.0],
					drag_force: 0.4,
					center_node: None,
					hit_radius: 0.0,
					hit_radius_samples: Vec::new(),
					stiffness_samples: Vec::new(),
					pull_samples: Vec::new(),
					spring_samples: Vec::new(),
					gravity_power_samples: Vec::new(),
					gravity_falloff_samples: Vec::new(),
					immobile_samples: Vec::new(),
					max_angle_x_samples: Vec::new(),
					max_angle_z_samples: Vec::new(),
					writeback_mode: Default::default(),
					limit: None,
					interaction: None,
					bone_node_indices: vec![0, 1],
				},
			],
			colliders: Vec::new(),
			..Default::default()
		};
		let sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
		let hair = sim.runtimes[0].as_ref().expect("hair runtime");
		let cloth = sim.runtimes[1].as_ref().expect("cloth runtime");
		assert_eq!(hair.category, "hair");
		assert_eq!(cloth.category, "cloth");
		for runtime in [hair, cloth] {
			assert!((runtime.params.rest_response - 0.4).abs() < 1e-6);
			assert!((runtime.params.shape_preservation - 0.5).abs() < 1e-6);
			assert!((runtime.params.bounce_response - 0.3).abs() < 1e-6);
		}
	}

	#[test]
	fn unphysics_long_chain_shaping_is_category_independent() {
		let scene = UnaSceneSnapshot {
			nodes: vec![
				node(0.0, Vec3::ZERO, vec![1]),
				node(0.0, Vec3::new(0.0, 0.8, 0.0), vec![2]),
				node(0.0, Vec3::new(0.0, 0.8, 0.0), vec![3]),
				node(0.0, Vec3::new(0.0, 0.8, 0.0), vec![]),
				node(0.0, Vec3::new(2.0, 0.0, 0.0), vec![5]),
				node(0.0, Vec3::new(0.0, 0.8, 0.0), vec![6]),
				node(0.0, Vec3::new(0.0, 0.8, 0.0), vec![7]),
				node(0.0, Vec3::new(0.0, 0.8, 0.0), vec![]),
			],
			roots: vec![0, 4],
			..Default::default()
		};
		let group = |category: &str, chain: Vec<usize>| UnaSpringBoneGroup {
			interaction_chain_start_index: 0,
			source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
			enabled: true,
			source_id: format!("physbone:{category}"),
			comment: category.to_string(),
			category: category.to_string(),
			stiffness: 0.4,
			pull: 0.35,
			spring: 0.25,
			integration_type: UnaDynamicsIntegrationType::VrcAdvanced,
			gravity_power: 0.0,
			gravity_falloff: 0.0,
			immobile: 0.5,
			immobile_type: UnaDynamicsImmobileType::AllMotion,
			gravity_dir: [0.0, -1.0, 0.0],
			drag_force: 0.35,
			center_node: None,
			hit_radius: 0.0,
			hit_radius_samples: Vec::new(),
			stiffness_samples: Vec::new(),
			pull_samples: Vec::new(),
			spring_samples: Vec::new(),
			gravity_power_samples: Vec::new(),
			gravity_falloff_samples: Vec::new(),
			immobile_samples: Vec::new(),
			max_angle_x_samples: Vec::new(),
			max_angle_z_samples: Vec::new(),
			writeback_mode: Default::default(),
			limit: None,
			interaction: None,
			bone_node_indices: chain,
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![group("hair", vec![0, 1, 2, 3]), group("cloth", vec![4, 5, 6, 7])],
			..Default::default()
		};
		let sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
		let hair = sim.runtimes[0].as_ref().expect("hair runtime");
		let cloth = sim.runtimes[1].as_ref().expect("cloth runtime");
		assert_eq!(hair.category, "hair");
		assert_eq!(cloth.category, "cloth");
		assert_eq!(hair.params.rest_response, cloth.params.rest_response);
		assert_eq!(hair.params.shape_preservation, cloth.params.shape_preservation);
		assert_eq!(hair.params.bounce_response, cloth.params.bounce_response);
		for (hair_joint, cloth_joint) in hair.joints.iter().zip(&cloth.joints) {
			assert_eq!(hair_joint.rest_response, cloth_joint.rest_response);
			assert_eq!(hair_joint.shape_preservation, cloth_joint.shape_preservation);
			assert_eq!(hair_joint.bounce_response, cloth_joint.bounce_response);
			assert_eq!(hair_joint.parent_motion_follow, cloth_joint.parent_motion_follow);
		}
	}

	#[test]
	fn unphysics_moderate_immobile_keeps_parent_motion_follow_loose() {
		let scene = UnaSceneSnapshot {
			nodes: vec![node(0.0, Vec3::ZERO, vec![1]), node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![])],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				interaction_chain_start_index: 0,
				source_kind: Default::default(),
				enabled: true,
				source_id: "physbone:test/cloth-panel".to_string(),
				comment: "Cloth panel".to_string(),
				category: "cloth".to_string(),
				stiffness: 0.0,
				pull: 0.2,
				spring: 0.2,
				integration_type: Default::default(),
				gravity_power: 0.0,
				gravity_falloff: 0.0,
				immobile: 0.5,
				immobile_type: UnaDynamicsImmobileType::AllMotion,
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.4,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1],
			}],
			..Default::default()
		};
		let sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
		let runtime = sim.runtimes[0].as_ref().expect("runtime");
		let joint = &runtime.joints[0];
		assert_eq!(runtime.category, "cloth");
		assert!(
			joint.parent_motion_follow < 0.45,
			"cloth should keep parent motion coupling loose enough for inertia: follow={}",
			joint.parent_motion_follow
		);
		assert!((joint.parent_motion_follow - 0.4125).abs() < 1e-6);
	}

	#[test]
	fn unphysics_cloth_profile_softness_changes_solver_output() {
		fn cloth_tip_lag(rest_response: f32, shape_preservation: f32, motion_coupling: f32, damping_half_life_ms: f32) -> f32 {
			let mut scene = UnaSceneSnapshot {
				nodes: vec![
					node(0.0, Vec3::ZERO, vec![1]),
					node(0.0, Vec3::new(0.0, 0.75, 0.0), vec![2]),
					node(0.0, Vec3::new(0.0, 0.75, 0.0), vec![3]),
					node(0.0, Vec3::new(0.0, 0.75, 0.0), vec![]),
				],
				roots: vec![0],
				..Default::default()
			};
			let settings = UnaSpringBoneSettings {
				groups: vec![UnaSpringBoneGroup {
					interaction_chain_start_index: 0,
					source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
					enabled: true,
					source_id: "physbone:test/cloth-panel".to_string(),
					comment: "Cloth panel".to_string(),
					category: "cloth".to_string(),
					stiffness: 0.20,
					pull: 0.22,
					spring: 0.25,
					integration_type: UnaDynamicsIntegrationType::VrcAdvanced,
					gravity_power: 0.0,
					gravity_falloff: 0.0,
					immobile: 0.5,
					immobile_type: UnaDynamicsImmobileType::AllMotion,
					gravity_dir: [0.0, -1.0, 0.0],
					drag_force: 0.30,
					center_node: None,
					hit_radius: 0.0,
					hit_radius_samples: Vec::new(),
					stiffness_samples: Vec::new(),
					pull_samples: Vec::new(),
					spring_samples: Vec::new(),
					gravity_power_samples: Vec::new(),
					gravity_falloff_samples: Vec::new(),
					immobile_samples: Vec::new(),
					max_angle_x_samples: Vec::new(),
					max_angle_z_samples: Vec::new(),
					writeback_mode: Default::default(),
					limit: None,
					interaction: None,
					bone_node_indices: vec![0, 1, 2, 3],
				}],
				..Default::default()
			};
			let config = DynamicsPhysicsConfig {
				overrides: vec![DynamicsCategoryOverride {
					category: "cloth".to_string(),
					params: DynamicsPhysicsParams {
						solver: Some(DynamicsSolver::Verlet),
						rest_response: Some(rest_response),
						shape_preservation: Some(shape_preservation),
						motion_coupling: Some(motion_coupling),
						bounce_scale: Some(0.35),
						damping_half_life_ms: Some(damping_half_life_ms),
						..Default::default()
					},
				}],
				..Default::default()
			};
			let mut sim = DynamicsSimulator::new_with_config(&scene, &settings, Vec::new(), config).expect("sim");
			let mut max_lag = 0.0_f32;
			for frame in 1..=36 {
				let angle = std::f32::consts::FRAC_PI_2 * frame as f32 / 36.0;
				scene.nodes[0].transform = Mat4::from_rotation_z(angle).to_cols_array();
				sim.step(&mut scene, &settings, 1.0 / 60.0);
				let tip = world_from_snapshot(&scene)[3].transform_point3(Vec3::ZERO);
				let rest = Mat4::from_rotation_z(angle).transform_point3(Vec3::new(0.0, 2.25, 0.0));
				max_lag = max_lag.max(tip.distance(rest));
			}
			max_lag
		}

		let soft = cloth_tip_lag(0.035, 0.01, 0.10, 260.0);
		let firm = cloth_tip_lag(0.30, 0.20, 0.85, 70.0);
		assert!(
			soft > firm + 0.20,
			"cloth profile softness should visibly change solver output: soft={soft} firm={firm}"
		);
	}

	#[test]
	fn unphysics_ears_preset_intent_changes_lag_recovery_and_residual_motion() {
		#[derive(Clone, Copy)]
		struct Preset {
			rest_response: f32,
			shape_preservation: f32,
			motion_coupling: f32,
			damping_half_life_ms: f32,
			bounce_scale: f32,
		}

		#[derive(Clone, Copy)]
		struct Metrics {
			max_lag: f32,
			recovered_deflection: f32,
			residual_velocity: f32,
		}

		fn metrics_for_preset(preset: Preset) -> Metrics {
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
					interaction_chain_start_index: 0,
					source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
					enabled: true,
					source_id: "physbone:test/ears".to_string(),
					comment: "ears".to_string(),
					category: "ears".to_string(),
					stiffness: 0.12,
					pull: 0.10,
					spring: 0.75,
					integration_type: UnaDynamicsIntegrationType::VrcAdvanced,
					gravity_power: 0.0,
					gravity_falloff: 0.0,
					immobile: 0.0,
					immobile_type: Default::default(),
					gravity_dir: [0.0, -1.0, 0.0],
					drag_force: 0.25,
					center_node: None,
					hit_radius: 0.0,
					hit_radius_samples: Vec::new(),
					stiffness_samples: Vec::new(),
					pull_samples: Vec::new(),
					spring_samples: Vec::new(),
					gravity_power_samples: Vec::new(),
					gravity_falloff_samples: Vec::new(),
					immobile_samples: Vec::new(),
					max_angle_x_samples: Vec::new(),
					max_angle_z_samples: Vec::new(),
					writeback_mode: Default::default(),
					limit: None,
					interaction: None,
					bone_node_indices: vec![0, 1, 2],
				}],
				..Default::default()
			};
			let config = DynamicsPhysicsConfig {
				overrides: vec![DynamicsCategoryOverride {
					category: "ears".to_string(),
					params: DynamicsPhysicsParams {
						solver: Some(DynamicsSolver::Verlet),
						rest_response: Some(preset.rest_response),
						shape_preservation: Some(preset.shape_preservation),
						motion_coupling: Some(preset.motion_coupling),
						damping_half_life_ms: Some(preset.damping_half_life_ms),
						bounce_scale: Some(preset.bounce_scale),
						..Default::default()
					},
				}],
				..Default::default()
			};
			let mut sim = DynamicsSimulator::new_with_config(&scene, &settings, Vec::new(), config).expect("sim");
			let mut max_lag = 0.0_f32;
			for frame in 1..=30 {
				let angle = std::f32::consts::FRAC_PI_2 * frame as f32 / 30.0;
				scene.nodes[0].transform = Mat4::from_rotation_z(angle).to_cols_array();
				sim.step(&mut scene, &settings, 1.0 / 60.0);
				let tip = world_from_snapshot(&scene)[2].transform_point3(Vec3::ZERO);
				let rest = Mat4::from_rotation_z(angle).transform_point3(Vec3::new(0.0, 2.0, 0.0));
				max_lag = max_lag.max(tip.distance(rest));
			}
			let rest_after_turn = Vec3::new(-2.0, 0.0, 0.0);
			for _ in 0..45 {
				sim.step(&mut scene, &settings, 1.0 / 60.0);
			}
			let tip = world_from_snapshot(&scene)[2].transform_point3(Vec3::ZERO);
			let recovered_deflection = tip.distance(rest_after_turn);
			let residual_velocity = sim.runtimes[0]
				.as_ref()
				.expect("runtime")
				.joints
				.last()
				.expect("joint")
				.prev_velocity
				.length();
			Metrics {
				max_lag,
				recovered_deflection,
				residual_velocity,
			}
		}

		let soft = metrics_for_preset(Preset {
			rest_response: 0.02,
			shape_preservation: 0.015,
			motion_coupling: 0.30,
			damping_half_life_ms: 160.0,
			bounce_scale: 0.45,
		});
		let natural = metrics_for_preset(Preset {
			rest_response: 0.08,
			shape_preservation: 0.06,
			motion_coupling: 0.50,
			damping_half_life_ms: 95.0,
			bounce_scale: 0.70,
		});
		let snappy = metrics_for_preset(Preset {
			rest_response: 0.18,
			shape_preservation: 0.15,
			motion_coupling: 0.75,
			damping_half_life_ms: 55.0,
			bounce_scale: 0.50,
		});

		assert!(
			soft.max_lag > natural.max_lag && natural.max_lag > snappy.max_lag,
			"UNPhysics ears presets should monotonically reduce head-motion lag: soft={:?} natural={:?} snappy={:?}",
			(soft.max_lag, soft.recovered_deflection, soft.residual_velocity),
			(natural.max_lag, natural.recovered_deflection, natural.residual_velocity),
			(snappy.max_lag, snappy.recovered_deflection, snappy.residual_velocity)
		);
		assert!(
			natural.recovered_deflection < 0.01 && snappy.recovered_deflection <= soft.recovered_deflection * 1.1,
			"UNPhysics ears presets should recover close to rest while bounce remains bounded: soft={:?} natural={:?} snappy={:?}",
			(soft.max_lag, soft.recovered_deflection, soft.residual_velocity),
			(natural.max_lag, natural.recovered_deflection, natural.residual_velocity),
			(snappy.max_lag, snappy.recovered_deflection, snappy.residual_velocity)
		);
		assert!(
			soft.residual_velocity > 0.0 && snappy.residual_velocity < natural.residual_velocity,
			"ears presets should keep soft motion alive while snappy settles faster than natural: soft={:?} natural={:?} snappy={:?}",
			(soft.max_lag, soft.recovered_deflection, soft.residual_velocity),
			(natural.max_lag, natural.recovered_deflection, natural.residual_velocity),
			(snappy.max_lag, snappy.recovered_deflection, snappy.residual_velocity)
		);
	}

	#[test]
	fn unphysics_long_chain_source_intent_is_softened_without_scaling_profile_override() {
		let scene = UnaSceneSnapshot {
			nodes: vec![
				node(0.0, Vec3::ZERO, vec![1, 8]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![2]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![3]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![4]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![5]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![6]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![7]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![]),
				node(0.0, Vec3::new(1.0, 0.0, 0.0), vec![9]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![]),
			],
			roots: vec![0],
			..Default::default()
		};
		let cloth_group = |source_id: &str, chain: Vec<usize>| UnaSpringBoneGroup {
			interaction_chain_start_index: 0,
			source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
			enabled: true,
			source_id: source_id.to_string(),
			comment: "cloth".to_string(),
			category: "cloth".to_string(),
			stiffness: 0.40,
			pull: 0.40,
			spring: 0.40,
			integration_type: UnaDynamicsIntegrationType::VrcAdvanced,
			gravity_power: 0.0,
			gravity_falloff: 0.0,
			immobile: 0.5,
			immobile_type: UnaDynamicsImmobileType::AllMotion,
			gravity_dir: [0.0, -1.0, 0.0],
			drag_force: 0.4,
			center_node: None,
			hit_radius: 0.0,
			hit_radius_samples: Vec::new(),
			stiffness_samples: Vec::new(),
			pull_samples: Vec::new(),
			spring_samples: Vec::new(),
			gravity_power_samples: Vec::new(),
			gravity_falloff_samples: Vec::new(),
			immobile_samples: Vec::new(),
			max_angle_x_samples: Vec::new(),
			max_angle_z_samples: Vec::new(),
			writeback_mode: Default::default(),
			limit: None,
			interaction: None,
			bone_node_indices: chain,
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![
				cloth_group("physbone:long-cloth", vec![0, 1, 2, 3, 4, 5, 6, 7]),
				cloth_group("physbone:short-cloth", vec![8, 9]),
			],
			..Default::default()
		};
		let sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
		let long_runtime = sim.runtimes[0].as_ref().expect("long runtime");
		let short_runtime = sim.runtimes[1].as_ref().expect("short runtime");
		assert!(
			long_runtime.params.rest_response < short_runtime.params.rest_response * 0.9,
			"long cloth source pull should be softened: long={} short={}",
			long_runtime.params.rest_response,
			short_runtime.params.rest_response
		);
		assert!(
			long_runtime.params.rest_response > short_runtime.params.rest_response * 0.65,
			"long cloth source pull should retain a recovery floor: long={} short={}",
			long_runtime.params.rest_response,
			short_runtime.params.rest_response
		);
		assert!(
			long_runtime.params.shape_preservation < short_runtime.params.shape_preservation * 0.85,
			"long cloth source shape should be softened: long={} short={}",
			long_runtime.params.shape_preservation,
			short_runtime.params.shape_preservation
		);
		assert!(
			long_runtime.joints[0].parent_motion_follow < short_runtime.joints[0].parent_motion_follow,
			"long cloth should keep more inertia than short cloth: long={} short={}",
			long_runtime.joints[0].parent_motion_follow,
			short_runtime.joints[0].parent_motion_follow
		);

		let config = DynamicsPhysicsConfig {
			overrides: vec![DynamicsCategoryOverride {
				category: "cloth".to_string(),
				params: DynamicsPhysicsParams {
					rest_response: Some(0.22),
					shape_preservation: Some(0.11),
					motion_coupling: Some(0.33),
					..Default::default()
				},
			}],
			..Default::default()
		};
		let sim = DynamicsSimulator::new_with_config(&scene, &settings, Vec::new(), config).expect("sim");
		let long_runtime = sim.runtimes[0].as_ref().expect("long runtime");
		let short_runtime = sim.runtimes[1].as_ref().expect("short runtime");
		for runtime in [long_runtime, short_runtime] {
			assert!((runtime.params.rest_response - 0.22).abs() < 1e-6);
			assert!((runtime.params.shape_preservation - 0.11).abs() < 1e-6);
			assert!((runtime.joints[0].parent_motion_follow - 0.33).abs() < 1e-6);
		}
	}

	#[test]
	fn unphysics_long_cloth_without_curves_gets_tip_softness_distribution() {
		let scene = UnaSceneSnapshot {
			nodes: vec![
				node(0.0, Vec3::ZERO, vec![1]),
				node(0.0, Vec3::new(0.0, 0.6, 0.0), vec![2]),
				node(0.0, Vec3::new(0.0, 0.6, 0.0), vec![3]),
				node(0.0, Vec3::new(0.0, 0.6, 0.0), vec![4]),
				node(0.0, Vec3::new(0.0, 0.6, 0.0), vec![]),
			],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				interaction_chain_start_index: 0,
				source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
				enabled: true,
				source_id: "physbone:test/cloth-panel".to_string(),
				comment: "Cloth panel".to_string(),
				category: "cloth".to_string(),
				stiffness: 0.4,
				pull: 0.35,
				spring: 0.25,
				integration_type: UnaDynamicsIntegrationType::VrcAdvanced,
				gravity_power: 0.0,
				gravity_falloff: 0.0,
				immobile: 0.5,
				immobile_type: UnaDynamicsImmobileType::AllMotion,
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.35,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1, 2, 3, 4],
			}],
			..Default::default()
		};
		let sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
		let runtime = sim.runtimes[0].as_ref().expect("runtime");
		let root = runtime.joints.first().expect("root joint");
		let tip = runtime.joints.last().expect("tip joint");
		assert!(
			tip.rest_response < root.rest_response,
			"long cloth tip should restore more softly than root: root={} tip={}",
			root.rest_response,
			tip.rest_response
		);
		assert!(
			tip.shape_preservation < root.shape_preservation,
			"long cloth tip should preserve less shape than root: root={} tip={}",
			root.shape_preservation,
			tip.shape_preservation
		);
		assert!(
			tip.parent_motion_follow < root.parent_motion_follow,
			"long cloth tip should keep more local inertia than root: root={} tip={}",
			root.parent_motion_follow,
			tip.parent_motion_follow
		);
		assert!(
			tip.bounce_response > root.bounce_response,
			"long cloth tip should retain more residual motion than root: root={} tip={}",
			root.bounce_response,
			tip.bounce_response
		);
	}

	#[test]
	fn unphysics_category_does_not_scale_profile_override() {
		let scene = UnaSceneSnapshot {
			nodes: vec![node(0.0, Vec3::ZERO, vec![1]), node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![])],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				interaction_chain_start_index: 0,
				source_kind: Default::default(),
				enabled: true,
				source_id: String::new(),
				comment: "Cloth panel".to_string(),
				category: String::new(),
				stiffness: 0.5,
				pull: 0.4,
				spring: 0.3,
				integration_type: Default::default(),
				gravity_power: 0.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.4,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let config = DynamicsPhysicsConfig {
			overrides: vec![DynamicsCategoryOverride {
				category: "cloth".to_string(),
				params: DynamicsPhysicsParams {
					rest_response: Some(0.12),
					shape_preservation: Some(0.10),
					bounce_scale: Some(0.5),
					motion_coupling: Some(0.25),
					..Default::default()
				},
			}],
			..Default::default()
		};
		let sim = DynamicsSimulator::new_with_config(&scene, &settings, Vec::new(), config).expect("sim");
		let runtime = sim.runtimes[0].as_ref().expect("runtime");
		assert_eq!(runtime.category, "cloth");
		assert!((runtime.params.rest_response - 0.12).abs() < 1e-6);
		assert!((runtime.params.shape_preservation - 0.10).abs() < 1e-6);
		assert!((runtime.params.bounce_response - 0.15).abs() < 1e-6);
		assert!((runtime.joints[0].parent_motion_follow - 0.25).abs() < 1e-6);
	}

	#[test]
	fn unphysics_vrm_springbone_stiffness_lowers_to_rest_response_not_shape_preservation() {
		let scene = UnaSceneSnapshot {
			nodes: vec![node(0.0, Vec3::ZERO, vec![1]), node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![])],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				interaction_chain_start_index: 0,
				source_kind: un_avatar_core::UnaDynamicsSourceKind::VrmSpringBone,
				enabled: true,
				source_id: "spring:hair".to_string(),
				comment: "hair".to_string(),
				category: "hair".to_string(),
				stiffness: 0.0,
				pull: 0.8,
				spring: 0.0,
				integration_type: Default::default(),
				gravity_power: 0.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.4,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1],
			}],
			..Default::default()
		};
		let sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
		let runtime = sim.runtimes[0].as_ref().expect("runtime");
		assert_eq!(runtime.category, "hair");
		assert!((runtime.params.rest_response - 0.8).abs() < 1e-6);
		assert_eq!(runtime.params.shape_preservation, 0.0);
		assert_eq!(runtime.joints[0].shape_preservation, 0.0);
	}

	#[test]
	fn unphysics_vrm_springbone_stiffness_samples_lower_to_pull_samples_not_shape_samples() {
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
				interaction_chain_start_index: 0,
				source_kind: un_avatar_core::UnaDynamicsSourceKind::VrmSpringBone,
				enabled: true,
				source_id: "spring:tail".to_string(),
				comment: "tail".to_string(),
				category: "tail".to_string(),
				stiffness: 0.0,
				pull: 0.5,
				spring: 0.0,
				integration_type: Default::default(),
				gravity_power: 0.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.4,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: vec![0.2, 0.8],
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1, 2],
			}],
			..Default::default()
		};
		let sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
		let runtime = sim.runtimes[0].as_ref().expect("runtime");
		assert_eq!(runtime.joints.len(), 2);
		assert!((runtime.joints[0].rest_response - 0.2).abs() < 1e-6);
		assert!((runtime.joints[1].rest_response - 0.8).abs() < 1e-6);
		assert_eq!(runtime.joints[0].shape_preservation, 0.0);
		assert_eq!(runtime.joints[1].shape_preservation, 0.0);
	}

	#[test]
	fn unphysics_profile_override_applies_after_per_joint_source_samples() {
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
				interaction_chain_start_index: 0,
				source_kind: Default::default(),
				enabled: true,
				source_id: "physbone:test-ears".to_string(),
				comment: "ears".to_string(),
				category: "ears".to_string(),
				stiffness: 1.0,
				pull: 1.0,
				spring: 1.0,
				integration_type: Default::default(),
				gravity_power: 0.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.4,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: vec![0.7, 0.8],
				pull_samples: vec![0.1, 0.2],
				spring_samples: vec![0.3, 0.4],
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: vec![0.2, 0.4],
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1, 2],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let config = DynamicsPhysicsConfig {
			overrides: vec![DynamicsCategoryOverride {
				category: "ears".to_string(),
				params: DynamicsPhysicsParams {
					solver: Some(DynamicsSolver::Verlet),
					damping_half_life_ms: Some(120.0),
					rest_response: Some(0.25),
					shape_preservation: Some(0.18),
					bounce_scale: Some(0.5),
					motion_coupling: Some(0.3),
					drag_scale: Some(0.25),
					..Default::default()
				},
			}],
			..Default::default()
		};
		let sim = DynamicsSimulator::new_with_config(&scene, &settings, Vec::new(), config).expect("sim");
		let runtime = sim.runtimes[0].as_ref().expect("runtime");
		assert_eq!(runtime.joints.len(), 2);
		assert!((runtime.joints[0].shape_preservation - 0.18).abs() < 1e-6);
		assert!((runtime.joints[1].shape_preservation - 0.18).abs() < 1e-6);
		assert!((runtime.joints[0].rest_response - 0.25).abs() < 1e-6);
		assert!((runtime.joints[1].rest_response - 0.25).abs() < 1e-6);
		assert!((runtime.joints[0].bounce_response - 0.15).abs() < 1e-6);
		assert!((runtime.joints[1].bounce_response - 0.20).abs() < 1e-6);
		assert_eq!(runtime.joints[0].damping_half_life_ms, Some(120.0));
		assert_eq!(runtime.joints[1].damping_half_life_ms, Some(120.0));
		assert!((runtime.joints[0].drag_force - 0.1).abs() < 1e-6);
		assert!((runtime.joints[1].drag_force - 0.1).abs() < 1e-6);
		assert!((runtime.joints[0].parent_motion_follow - 0.3).abs() < 1e-6);
		assert!((runtime.joints[1].parent_motion_follow - 0.3).abs() < 1e-6);
		let summaries = sim.response_category_summaries();
		assert_eq!(summaries.len(), 1);
		assert_eq!(summaries[0].category, "ears");
		assert_eq!(summaries[0].group_count, 1);
		assert_eq!(summaries[0].joint_count, 2);
		assert!((summaries[0].average_stiffness - 0.18).abs() < 1e-6);
		assert!((summaries[0].average_pull - 0.25).abs() < 1e-6);
		assert!((summaries[0].average_shape_preservation - 0.18).abs() < 1e-6);
		assert!((summaries[0].average_spring - 0.175).abs() < 1e-6);
		assert!((summaries[0].average_drag_force - 0.1).abs() < 1e-6);
		assert!((summaries[0].average_damping_half_life_ms.unwrap_or_default() - 120.0).abs() < 1e-6);
		assert!((summaries[0].average_parent_motion_follow - 0.3).abs() < 1e-6);
		assert!((summaries[0].average_orientation_follow - 0.054).abs() < 1e-6);
		let group_summaries = sim.response_group_summaries();
		assert_eq!(group_summaries.len(), 1);
		assert_eq!(group_summaries[0].source_id, "physbone:test-ears");
		assert_eq!(group_summaries[0].category, "ears");
		assert_eq!(group_summaries[0].joint_count, 2);
		assert_eq!(group_summaries[0].solver, DynamicsSolver::Verlet);
		assert!((group_summaries[0].average_stiffness - 0.18).abs() < 1e-6);
		assert!((group_summaries[0].average_pull - 0.25).abs() < 1e-6);
		assert!((group_summaries[0].average_shape_preservation - 0.18).abs() < 1e-6);
		assert!((group_summaries[0].average_damping_half_life_ms.unwrap_or_default() - 120.0).abs() < 1e-6);
		assert!((group_summaries[0].average_orientation_follow - 0.054).abs() < 1e-6);
	}

	#[test]
	fn translation_writeback_candidates_exclude_skinned_joints() {
		let scene = UnaSceneSnapshot {
			nodes: vec![
				node(0.0, Vec3::ZERO, vec![1]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![2]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![]),
			],
			roots: vec![0],
			skins: vec![UnaSkin {
				joint_nodes: vec![1],
				..Default::default()
			}],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				interaction_chain_start_index: 0,
				source_kind: Default::default(),
				enabled: true,
				source_id: String::new(),
				comment: String::new(),
				category: String::new(),
				stiffness: 1.0,
				pull: 1.0,
				spring: 0.0,
				integration_type: Default::default(),
				gravity_power: 0.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.4,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: UnaDynamicsWritebackMode::RotationTranslation,
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1, 2],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let sim = DynamicsSimulator::new(&scene, &settings).expect("sim");

		assert_eq!(sim.translation_writeback_candidate_count(), 1);
		assert_eq!(sim.translation_writeback_target_count(), 1);
		let runtime = sim.runtimes[0].as_ref().expect("runtime");
		assert_eq!(
			runtime.joints[0].translation_writeback_target,
			Some(TailTranslationWritebackTarget::NextChainNode { node: 2 })
		);
		assert_eq!(runtime.joints[1].translation_writeback_target, None);
	}

	#[test]
	fn translation_writeback_targets_do_not_duplicate_terminal_tail() {
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
				interaction_chain_start_index: 0,
				enabled: true,
				stiffness: 1.0,
				pull: 1.0,
				spring: 0.0,
				integration_type: Default::default(),
				gravity_power: 0.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.4,
				writeback_mode: UnaDynamicsWritebackMode::RotationTranslation,
				bone_node_indices: vec![0, 1, 2],
				..Default::default()
			}],
			..Default::default()
		};
		let sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
		let runtime = sim.runtimes[0].as_ref().expect("runtime");

		assert_eq!(sim.translation_writeback_candidate_count(), 2);
		assert_eq!(sim.translation_writeback_target_count(), 1);
		assert_eq!(
			runtime.joints[0].translation_writeback_target,
			Some(TailTranslationWritebackTarget::NextChainNode { node: 2 })
		);
		assert_eq!(runtime.joints[1].translation_writeback_target, None);
	}

	#[test]
	fn translation_writeback_targets_assign_safe_two_node_terminal_child() {
		let scene = UnaSceneSnapshot {
			nodes: vec![node(0.0, Vec3::ZERO, vec![1]), node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![])],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				interaction_chain_start_index: 0,
				enabled: true,
				stiffness: 1.0,
				pull: 1.0,
				spring: 0.0,
				integration_type: Default::default(),
				gravity_power: 0.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.4,
				writeback_mode: UnaDynamicsWritebackMode::RotationTranslation,
				bone_node_indices: vec![0, 1],
				..Default::default()
			}],
			..Default::default()
		};
		let sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
		let runtime = sim.runtimes[0].as_ref().expect("runtime");

		assert_eq!(sim.translation_writeback_candidate_count(), 1);
		assert_eq!(sim.translation_writeback_target_count(), 1);
		assert_eq!(
			runtime.joints[0].translation_writeback_target,
			Some(TailTranslationWritebackTarget::ChildNode)
		);
	}

	#[test]
	fn translation_writeback_targets_skip_skinned_two_node_terminal_child() {
		let scene = UnaSceneSnapshot {
			nodes: vec![node(0.0, Vec3::ZERO, vec![1]), node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![])],
			roots: vec![0],
			skins: vec![UnaSkin {
				joint_nodes: vec![1],
				..Default::default()
			}],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				interaction_chain_start_index: 0,
				enabled: true,
				stiffness: 1.0,
				pull: 1.0,
				spring: 0.0,
				integration_type: Default::default(),
				gravity_power: 0.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.4,
				writeback_mode: UnaDynamicsWritebackMode::RotationTranslation,
				bone_node_indices: vec![0, 1],
				..Default::default()
			}],
			..Default::default()
		};
		let sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
		let runtime = sim.runtimes[0].as_ref().expect("runtime");

		assert_eq!(sim.translation_writeback_candidate_count(), 0);
		assert_eq!(sim.translation_writeback_target_count(), 0);
		assert_eq!(runtime.joints[0].translation_writeback_target, None);
	}

	#[test]
	fn rotation_translation_writeback_stretches_next_chain_node_within_limit() {
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
				interaction_chain_start_index: 0,
				enabled: true,
				stiffness: 0.0,
				pull: 1.0,
				spring: 0.0,
				integration_type: Default::default(),
				gravity_power: 4.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [1.0, 0.0, 0.0],
				drag_force: 0.0,
				writeback_mode: UnaDynamicsWritebackMode::RotationTranslation,
				limit: Some(UnaDynamicsLimit {
					max_stretch: 0.5,
					..Default::default()
				}),
				bone_node_indices: vec![0, 1, 2],
				..Default::default()
			}],
			..Default::default()
		};
		let mut sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
		for _ in 0..120 {
			sim.step(&mut scene, &settings, 1.0 / 60.0);
		}

		let (_, _, tip_local_translation) = Mat4::from_cols_array(&scene.nodes[2].transform).to_scale_rotation_translation();
		let stretched_length = tip_local_translation.length();
		assert!(
			stretched_length > 1.01,
			"next chain node local translation should stretch beyond rest length; got {stretched_length}"
		);
		assert!(
			stretched_length <= 1.5 + 1e-4,
			"next chain node local translation should respect max_stretch; got {stretched_length}"
		);
	}

	#[test]
	fn rotation_translation_writeback_stretches_two_node_child_within_limit() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![node(0.0, Vec3::ZERO, vec![1]), node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![])],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				interaction_chain_start_index: 0,
				enabled: true,
				stiffness: 0.0,
				pull: 1.0,
				spring: 0.0,
				integration_type: Default::default(),
				gravity_power: 4.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [1.0, 0.0, 0.0],
				drag_force: 0.0,
				writeback_mode: UnaDynamicsWritebackMode::RotationTranslation,
				limit: Some(UnaDynamicsLimit {
					max_stretch: 0.5,
					..Default::default()
				}),
				bone_node_indices: vec![0, 1],
				..Default::default()
			}],
			..Default::default()
		};
		let mut sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
		for _ in 0..120 {
			sim.step(&mut scene, &settings, 1.0 / 60.0);
		}

		let (_, _, child_local_translation) = Mat4::from_cols_array(&scene.nodes[1].transform).to_scale_rotation_translation();
		let stretched_length = child_local_translation.length();
		assert!(
			stretched_length > 1.01,
			"terminal child local translation should stretch beyond rest length; got {stretched_length}"
		);
		assert!(
			stretched_length <= 1.5 + 1e-4,
			"terminal child local translation should respect max_stretch; got {stretched_length}"
		);
	}

	#[test]
	fn targetless_stretch_uses_simulation_length_without_skinned_translation_writeback() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![node(0.0, Vec3::ZERO, vec![1]), node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![])],
			roots: vec![0],
			skins: vec![UnaSkin {
				joint_nodes: vec![1],
				..Default::default()
			}],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				interaction_chain_start_index: 0,
				enabled: true,
				stiffness: 0.0,
				pull: 1.0,
				spring: 0.0,
				integration_type: Default::default(),
				gravity_power: 4.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [1.0, 0.0, 0.0],
				drag_force: 0.0,
				writeback_mode: UnaDynamicsWritebackMode::RotationTranslation,
				limit: Some(UnaDynamicsLimit {
					max_stretch: 0.5,
					..Default::default()
				}),
				bone_node_indices: vec![0, 1],
				..Default::default()
			}],
			..Default::default()
		};
		let mut sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
		assert_eq!(sim.translation_writeback_candidate_count(), 0);
		assert_eq!(sim.translation_writeback_target_count(), 0);
		for _ in 0..120 {
			sim.step(&mut scene, &settings, 1.0 / 60.0);
		}

		let (_, _, child_local_translation) = Mat4::from_cols_array(&scene.nodes[1].transform).to_scale_rotation_translation();
		assert!(
			(child_local_translation.length() - 1.0).abs() <= 1e-4,
			"skinned child local translation must stay at rest length; got {}",
			child_local_translation.length()
		);
		let runtime = sim.runtimes[0].as_ref().expect("runtime");
		let joint = &runtime.joints[0];
		let child_pos = Vec3::new(0.0, 1.0, 0.0);
		let simulated_length = (joint.curr_tail - child_pos).length();
		assert!(
			simulated_length > 1.01,
			"targetless stretch should affect simulated tail length; got {simulated_length}"
		);
		assert!(
			simulated_length <= 1.5 + 1e-4,
			"targetless simulated tail length should respect max_stretch; got {simulated_length}"
		);
	}

	#[test]
	fn max_squish_sets_tail_length_lower_bound() {
		let limit = UnaDynamicsLimit {
			max_stretch: 0.25,
			max_squish: 0.4,
			..Default::default()
		};
		let (min_length, max_length) = tail_length_range(2.0, Some(&limit));
		assert!((min_length - 1.2).abs() < 1e-5, "min_length={min_length}");
		assert!((max_length - 2.5).abs() < 1e-5, "max_length={max_length}");
	}

	#[test]
	fn stretch_motion_scales_tail_length_range_when_authored() {
		let limit = UnaDynamicsLimit {
			max_stretch: 0.5,
			max_squish: 0.5,
			stretch_motion: Some(0.25),
			..Default::default()
		};
		let (min_length, max_length) = tail_length_range(2.0, Some(&limit));
		assert!((min_length - 1.75).abs() < 1e-5, "min_length={min_length}");
		assert!((max_length - 2.25).abs() < 1e-5, "max_length={max_length}");
	}

	#[test]
	fn dynamics_stretch_limit_uses_per_joint_curve_samples() {
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				interaction_chain_start_index: 0,
				source_kind: UnaDynamicsSourceKind::VrcPhysBone,
				enabled: true,
				source_id: "stretch-samples".to_string(),
				comment: String::new(),
				category: String::new(),
				stiffness: 1.0,
				pull: 0.0,
				spring: 0.0,
				integration_type: Default::default(),
				gravity_power: 0.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.0,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: Some(UnaDynamicsLimit {
					limit_type: String::new(),
					limit_rotation: [0.0, 0.0, 0.0],
					max_angle_x: 0.0,
					max_angle_z: 0.0,
					max_stretch: 0.0,
					max_squish: 0.0,
					stretch_motion: None,
					max_stretch_samples: vec![0.1, 0.5],
					max_squish_samples: vec![0.2, 0.4],
					stretch_motion_samples: vec![0.25, 0.75],
				}),
				interaction: None,
				bone_node_indices: vec![0, 1, 2],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let dynamics = settings.runtime_dynamics();
		let group = dynamics.dynamics_group(0).expect("group");
		let root_limit = sampled_joint_limit(group, 0).expect("root limit");
		let tip_limit = sampled_joint_limit(group, 1).expect("tip limit");
		assert_eq!(root_limit.max_stretch, 0.1);
		assert_eq!(root_limit.max_squish, 0.2);
		assert_eq!(root_limit.stretch_motion, Some(0.25));
		assert_eq!(tip_limit.max_stretch, 0.5);
		assert_eq!(tip_limit.max_squish, 0.4);
		assert_eq!(tip_limit.stretch_motion, Some(0.75));
	}

	#[test]
	fn dynamics_stretch_override_scales_range_and_replaces_motion() {
		let mut limit = UnaDynamicsLimit {
			max_stretch: 0.25,
			max_squish: 0.1,
			stretch_motion: Some(0.0),
			max_stretch_samples: vec![0.2],
			max_squish_samples: vec![0.05],
			stretch_motion_samples: vec![0.0],
			..Default::default()
		};
		let params = ResolvedDynamicsPhysicsParams {
			solver: DynamicsSolver::Verlet,
			damping_half_life_ms: None,
			rest_response_override: None,
			shape_preservation_override: None,
			bounce_scale: 1.0,
			source_shape_preservation_scale: 1.0,
			source_rest_response_scale: 1.0,
			source_bounce_response_scale: 1.0,
			source_motion_coupling_scale: 1.0,
			shape_preservation: 0.0,
			rest_response: 0.0,
			bounce_response: 0.0,
			xpbd_compliance: 0.0,
			stretch_range_scale: 2.0,
			stretch_motion_override: Some(0.5),
			gravity_scale: 1.0,
			gravity_falloff: 0.0,
			immobile: 0.0,
			immobile_type: Default::default(),
			motion_coupling_override: None,
			drag_scale: 1.0,
			constraint_iterations: 1,
		};
		apply_dynamics_limit_overrides(&mut limit, &params);
		assert_eq!(limit.max_stretch, 0.5);
		assert_eq!(limit.max_squish, 0.2);
		assert_eq!(limit.max_stretch_samples, vec![0.4]);
		assert_eq!(limit.max_squish_samples, vec![0.1]);
		assert_eq!(limit.stretch_motion, Some(0.5));
		assert!(limit.stretch_motion_samples.is_empty());
	}

	#[test]
	fn rotation_translation_writeback_squishes_two_node_child_within_limit() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![node(0.0, Vec3::ZERO, vec![1]), node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![])],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				interaction_chain_start_index: 0,
				enabled: true,
				stiffness: 0.0,
				pull: 0.0,
				spring: 0.0,
				integration_type: Default::default(),
				gravity_power: 0.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 1.0,
				writeback_mode: UnaDynamicsWritebackMode::RotationTranslation,
				limit: Some(UnaDynamicsLimit {
					max_squish: 0.4,
					..Default::default()
				}),
				bone_node_indices: vec![0, 1],
				..Default::default()
			}],
			..Default::default()
		};
		let mut sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
		let runtime = sim.runtimes[0].as_mut().expect("runtime");
		runtime.joints[0].curr_tail = Vec3::new(0.0, 0.1, 0.0);
		runtime.joints[0].prev_tail = Vec3::new(0.0, 0.1, 0.0);
		sim.step(&mut scene, &settings, 1.0 / 60.0);

		let (_, _, child_local_translation) = Mat4::from_cols_array(&scene.nodes[1].transform).to_scale_rotation_translation();
		let squished_length = child_local_translation.length();
		assert!(
			(squished_length - 0.6).abs() <= 1e-4,
			"terminal child local translation should respect max_squish lower bound; got {squished_length}"
		);
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
				interaction_chain_start_index: 0,
				source_kind: Default::default(),
				enabled: false,
				source_id: String::new(),
				comment: String::new(),
				category: String::new(),
				stiffness: 1.0,
				pull: 1.0,
				spring: 0.0,
				integration_type: Default::default(),
				gravity_power: 1.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.4,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1],
			}],
			colliders: Vec::new(),
			..Default::default()
		};

		assert!(DynamicsSimulator::new(&scene, &settings).is_none());
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
				interaction_chain_start_index: 0,
				source_kind: Default::default(),
				enabled: true,
				source_id: String::new(),
				comment: String::new(),
				category: String::new(),
				stiffness: 1.0,
				pull: 1.0,
				spring: 0.0,
				integration_type: Default::default(),
				gravity_power: 0.5,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.4,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1, 2],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let mut sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
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
				interaction_chain_start_index: 0,
				source_kind: Default::default(),
				enabled: true,
				source_id: String::new(),
				comment: "ミミ spring".to_string(),
				category: String::new(),
				stiffness: 0.1,
				pull: 0.1,
				spring: 0.0,
				integration_type: Default::default(),
				gravity_power: 1.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.3,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let config = DynamicsPhysicsConfig {
			overrides: vec![DynamicsCategoryOverride {
				category: "ears".to_string(),
				params: DynamicsPhysicsParams {
					solver: Some(DynamicsSolver::Xpbd),
					damping_half_life_ms: Some(90.0),
					rest_response: Some(0.5),
					bounce_scale: None,
					xpbd_compliance: Some(0.025),
					gravity_scale: None,
					drag_scale: None,
					constraint_iterations: Some(8),
					..Default::default()
				},
			}],
			..Default::default()
		};
		let sim = DynamicsSimulator::new_with_config(&scene, &settings, Vec::new(), config).expect("sim");
		let rt = sim.runtimes[0].as_ref().expect("runtime");
		assert_eq!(rt.params.solver, DynamicsSolver::Xpbd);
		assert_eq!(rt.params.damping_half_life_ms, Some(90.0));
		assert_eq!(rt.params.rest_response, 0.5);
		assert_eq!(rt.params.xpbd_compliance, 0.025);
		assert_eq!(rt.params.constraint_iterations, 8);
	}

	#[test]
	fn unphysics_category_aliases_cover_common_outfit_cloth_roots() {
		let scene = UnaSceneSnapshot {
			nodes: vec![node(0.0, Vec3::ZERO, vec![1]), node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![])],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![
				UnaSpringBoneGroup {
					interaction_chain_start_index: 0,
					source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
					enabled: true,
					source_id: "physbone:ExampleOutfit/Armature/Hips/ShirtRoot".to_string(),
					comment: String::new(),
					category: String::new(),
					stiffness: 0.1,
					pull: 0.1,
					spring: 0.0,
					integration_type: Default::default(),
					gravity_power: 0.0,
					gravity_falloff: 0.0,
					immobile: 0.0,
					immobile_type: Default::default(),
					gravity_dir: [0.0, -1.0, 0.0],
					drag_force: 0.3,
					center_node: None,
					hit_radius: 0.0,
					hit_radius_samples: Vec::new(),
					stiffness_samples: Vec::new(),
					pull_samples: Vec::new(),
					spring_samples: Vec::new(),
					gravity_power_samples: Vec::new(),
					gravity_falloff_samples: Vec::new(),
					immobile_samples: Vec::new(),
					max_angle_x_samples: Vec::new(),
					max_angle_z_samples: Vec::new(),
					writeback_mode: Default::default(),
					limit: None,
					interaction: None,
					bone_node_indices: vec![0, 1],
				},
				UnaSpringBoneGroup {
					interaction_chain_start_index: 0,
					source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
					enabled: true,
					source_id: "physbone:ExampleOutfit/Armature/Hips/Spine/Chest/SweaterRoot".to_string(),
					comment: String::new(),
					category: String::new(),
					stiffness: 0.1,
					pull: 0.1,
					spring: 0.0,
					integration_type: Default::default(),
					gravity_power: 0.0,
					gravity_falloff: 0.0,
					immobile: 0.0,
					immobile_type: Default::default(),
					gravity_dir: [0.0, -1.0, 0.0],
					drag_force: 0.3,
					center_node: None,
					hit_radius: 0.0,
					hit_radius_samples: Vec::new(),
					stiffness_samples: Vec::new(),
					pull_samples: Vec::new(),
					spring_samples: Vec::new(),
					gravity_power_samples: Vec::new(),
					gravity_falloff_samples: Vec::new(),
					immobile_samples: Vec::new(),
					max_angle_x_samples: Vec::new(),
					max_angle_z_samples: Vec::new(),
					writeback_mode: Default::default(),
					limit: None,
					interaction: None,
					bone_node_indices: vec![0, 1],
				},
				UnaSpringBoneGroup {
					interaction_chain_start_index: 0,
					source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
					enabled: true,
					source_id: "physbone:Armature/Hips/Spine/Chest/Shoulder_L/Upperarm_L/Lowerarm_L/Left Hand/coat_hand_root_L".to_string(),
					comment: String::new(),
					category: String::new(),
					stiffness: 0.1,
					pull: 0.1,
					spring: 0.0,
					integration_type: Default::default(),
					gravity_power: 0.0,
					gravity_falloff: 0.0,
					immobile: 0.0,
					immobile_type: Default::default(),
					gravity_dir: [0.0, -1.0, 0.0],
					drag_force: 0.3,
					center_node: None,
					hit_radius: 0.0,
					hit_radius_samples: Vec::new(),
					stiffness_samples: Vec::new(),
					pull_samples: Vec::new(),
					spring_samples: Vec::new(),
					gravity_power_samples: Vec::new(),
					gravity_falloff_samples: Vec::new(),
					immobile_samples: Vec::new(),
					max_angle_x_samples: Vec::new(),
					max_angle_z_samples: Vec::new(),
					writeback_mode: Default::default(),
					limit: None,
					interaction: None,
					bone_node_indices: vec![0, 1],
				},
				UnaSpringBoneGroup {
					interaction_chain_start_index: 0,
					source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
					enabled: true,
					source_id: "physbone:WardrobeA/Armature/Hips/Spine/Chest/Leg_frills_Root_L".to_string(),
					comment: String::new(),
					category: String::new(),
					stiffness: 0.1,
					pull: 0.1,
					spring: 0.0,
					integration_type: Default::default(),
					gravity_power: 0.0,
					gravity_falloff: 0.0,
					immobile: 0.0,
					immobile_type: Default::default(),
					gravity_dir: [0.0, -1.0, 0.0],
					drag_force: 0.3,
					center_node: None,
					hit_radius: 0.0,
					hit_radius_samples: Vec::new(),
					stiffness_samples: Vec::new(),
					pull_samples: Vec::new(),
					spring_samples: Vec::new(),
					gravity_power_samples: Vec::new(),
					gravity_falloff_samples: Vec::new(),
					immobile_samples: Vec::new(),
					max_angle_x_samples: Vec::new(),
					max_angle_z_samples: Vec::new(),
					writeback_mode: Default::default(),
					limit: None,
					interaction: None,
					bone_node_indices: vec![0, 1],
				},
			],
			..Default::default()
		};

		let categories: Vec<_> = settings
			.runtime_dynamics()
			.dynamics_groups()
			.map(|group| classify_dynamics_group_category(&scene, group, &default_spring_bone_categories()))
			.collect();
		assert_eq!(categories, vec!["cloth", "cloth", "cloth", "cloth"]);
	}

	#[test]
	fn unphysics_category_aliases_cover_common_outfit_accessories() {
		let scene = UnaSceneSnapshot {
			nodes: vec![node(0.0, Vec3::ZERO, vec![1]), node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![])],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![
				UnaSpringBoneGroup {
					interaction_chain_start_index: 0,
					source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
					enabled: true,
					source_id: "physbone:ExampleAccessory/PB/Pocket Watch".to_string(),
					comment: String::new(),
					category: String::new(),
					stiffness: 0.1,
					pull: 0.1,
					spring: 0.0,
					integration_type: Default::default(),
					gravity_power: 0.0,
					gravity_falloff: 0.0,
					immobile: 0.0,
					immobile_type: Default::default(),
					gravity_dir: [0.0, -1.0, 0.0],
					drag_force: 0.3,
					center_node: None,
					hit_radius: 0.0,
					hit_radius_samples: Vec::new(),
					stiffness_samples: Vec::new(),
					pull_samples: Vec::new(),
					spring_samples: Vec::new(),
					gravity_power_samples: Vec::new(),
					gravity_falloff_samples: Vec::new(),
					immobile_samples: Vec::new(),
					max_angle_x_samples: Vec::new(),
					max_angle_z_samples: Vec::new(),
					writeback_mode: Default::default(),
					limit: None,
					interaction: None,
					bone_node_indices: vec![0, 1],
				},
				UnaSpringBoneGroup {
					interaction_chain_start_index: 0,
					source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
					enabled: true,
					source_id: "physbone:WardrobeAccessory/PB/BookBag_PB".to_string(),
					comment: String::new(),
					category: String::new(),
					stiffness: 0.1,
					pull: 0.1,
					spring: 0.0,
					integration_type: Default::default(),
					gravity_power: 0.0,
					gravity_falloff: 0.0,
					immobile: 0.0,
					immobile_type: Default::default(),
					gravity_dir: [0.0, -1.0, 0.0],
					drag_force: 0.3,
					center_node: None,
					hit_radius: 0.0,
					hit_radius_samples: Vec::new(),
					stiffness_samples: Vec::new(),
					pull_samples: Vec::new(),
					spring_samples: Vec::new(),
					gravity_power_samples: Vec::new(),
					gravity_falloff_samples: Vec::new(),
					immobile_samples: Vec::new(),
					max_angle_x_samples: Vec::new(),
					max_angle_z_samples: Vec::new(),
					writeback_mode: Default::default(),
					limit: None,
					interaction: None,
					bone_node_indices: vec![0, 1],
				},
				UnaSpringBoneGroup {
					interaction_chain_start_index: 0,
					source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
					enabled: true,
					source_id: "physbone:ExampleAccessory/Armature/Hips/Spine/Chest/Potion_02_PB".to_string(),
					comment: String::new(),
					category: String::new(),
					stiffness: 0.1,
					pull: 0.1,
					spring: 0.0,
					integration_type: Default::default(),
					gravity_power: 0.0,
					gravity_falloff: 0.0,
					immobile: 0.0,
					immobile_type: Default::default(),
					gravity_dir: [0.0, -1.0, 0.0],
					drag_force: 0.3,
					center_node: None,
					hit_radius: 0.0,
					hit_radius_samples: Vec::new(),
					stiffness_samples: Vec::new(),
					pull_samples: Vec::new(),
					spring_samples: Vec::new(),
					gravity_power_samples: Vec::new(),
					gravity_falloff_samples: Vec::new(),
					immobile_samples: Vec::new(),
					max_angle_x_samples: Vec::new(),
					max_angle_z_samples: Vec::new(),
					writeback_mode: Default::default(),
					limit: None,
					interaction: None,
					bone_node_indices: vec![0, 1],
				},
				UnaSpringBoneGroup {
					interaction_chain_start_index: 0,
					source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
					enabled: true,
					source_id: "physbone:ExampleAvatar/Armature/Hips/NerveCable".to_string(),
					comment: String::new(),
					category: String::new(),
					stiffness: 0.1,
					pull: 0.1,
					spring: 0.0,
					integration_type: Default::default(),
					gravity_power: 0.0,
					gravity_falloff: 0.0,
					immobile: 0.0,
					immobile_type: Default::default(),
					gravity_dir: [0.0, -1.0, 0.0],
					drag_force: 0.3,
					center_node: None,
					hit_radius: 0.0,
					hit_radius_samples: Vec::new(),
					stiffness_samples: Vec::new(),
					pull_samples: Vec::new(),
					spring_samples: Vec::new(),
					gravity_power_samples: Vec::new(),
					gravity_falloff_samples: Vec::new(),
					immobile_samples: Vec::new(),
					max_angle_x_samples: Vec::new(),
					max_angle_z_samples: Vec::new(),
					writeback_mode: Default::default(),
					limit: None,
					interaction: None,
					bone_node_indices: vec![0, 1],
				},
				UnaSpringBoneGroup {
					interaction_chain_start_index: 0,
					source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
					enabled: true,
					source_id: "physbone:ExampleOutfit/Armature/Hips/Spine/Chest/Neck/Head/EarringsRoot_L".to_string(),
					comment: String::new(),
					category: String::new(),
					stiffness: 0.1,
					pull: 0.1,
					spring: 0.0,
					integration_type: Default::default(),
					gravity_power: 0.0,
					gravity_falloff: 0.0,
					immobile: 0.0,
					immobile_type: Default::default(),
					gravity_dir: [0.0, -1.0, 0.0],
					drag_force: 0.3,
					center_node: None,
					hit_radius: 0.0,
					hit_radius_samples: Vec::new(),
					stiffness_samples: Vec::new(),
					pull_samples: Vec::new(),
					spring_samples: Vec::new(),
					gravity_power_samples: Vec::new(),
					gravity_falloff_samples: Vec::new(),
					immobile_samples: Vec::new(),
					max_angle_x_samples: Vec::new(),
					max_angle_z_samples: Vec::new(),
					writeback_mode: Default::default(),
					limit: None,
					interaction: None,
					bone_node_indices: vec![0, 1],
				},
				UnaSpringBoneGroup {
					interaction_chain_start_index: 0,
					source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
					enabled: true,
					source_id: "physbone:ExampleOutfit/Armature/Hips/Upperleg_L/Lowerleg_L/Foot_L/Cycr_MaryJaneShoesL_Root".to_string(),
					comment: String::new(),
					category: String::new(),
					stiffness: 0.1,
					pull: 0.1,
					spring: 0.0,
					integration_type: Default::default(),
					gravity_power: 0.0,
					gravity_falloff: 0.0,
					immobile: 0.0,
					immobile_type: Default::default(),
					gravity_dir: [0.0, -1.0, 0.0],
					drag_force: 0.3,
					center_node: None,
					hit_radius: 0.0,
					hit_radius_samples: Vec::new(),
					stiffness_samples: Vec::new(),
					pull_samples: Vec::new(),
					spring_samples: Vec::new(),
					gravity_power_samples: Vec::new(),
					gravity_falloff_samples: Vec::new(),
					immobile_samples: Vec::new(),
					max_angle_x_samples: Vec::new(),
					max_angle_z_samples: Vec::new(),
					writeback_mode: Default::default(),
					limit: None,
					interaction: None,
					bone_node_indices: vec![0, 1],
				},
			],
			..Default::default()
		};

		let categories: Vec<_> = settings
			.runtime_dynamics()
			.dynamics_groups()
			.map(|group| classify_dynamics_group_category(&scene, group, &default_spring_bone_categories()))
			.collect();
		assert_eq!(
			categories,
			vec!["accessory", "accessory", "accessory", "accessory", "accessory", "accessory"]
		);
	}

	#[test]
	fn unphysics_category_uses_component_leaf_before_parent_path_aliases() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![node(0.0, Vec3::ZERO, vec![1]), node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![])],
			roots: vec![0],
			..Default::default()
		};
		scene.nodes[0].name = Some("cycr_PocketWatch_Root".to_string());
		scene.nodes[1].name = Some("cycr_PocketWatch_1".to_string());
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				interaction_chain_start_index: 0,
				source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
				enabled: true,
				source_id: "physbone:ExampleOutfit/Armature/Hips/CYCRPleated_skirtRoot/CYCRPleated_skirt_BR_1/cycr_PocketWatch_Root"
					.to_string(),
				comment: String::new(),
				category: String::new(),
				stiffness: 0.1,
				pull: 0.1,
				spring: 0.0,
				integration_type: Default::default(),
				gravity_power: 0.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.3,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1],
			}],
			..Default::default()
		};

		let category = classify_dynamics_group_category(
			&scene,
			settings.runtime_dynamics().dynamics_groups().next().expect("group"),
			&default_spring_bone_categories(),
		);
		assert_eq!(category, "accessory");
	}

	#[test]
	fn unphysics_category_aliases_do_not_match_inside_unrelated_words() {
		let scene = UnaSceneSnapshot {
			nodes: vec![node(0.0, Vec3::ZERO, vec![1]), node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![])],
			roots: vec![0],
			..Default::default()
		};
		let make_group = |source_id: &str| UnaSpringBoneGroup {
			interaction_chain_start_index: 0,
			source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
			enabled: true,
			source_id: source_id.to_string(),
			comment: String::new(),
			category: String::new(),
			stiffness: 0.1,
			pull: 0.1,
			spring: 0.0,
			integration_type: Default::default(),
			gravity_power: 0.0,
			gravity_falloff: 0.0,
			immobile: 0.0,
			immobile_type: Default::default(),
			gravity_dir: [0.0, -1.0, 0.0],
			drag_force: 0.3,
			center_node: None,
			hit_radius: 0.0,
			hit_radius_samples: Vec::new(),
			stiffness_samples: Vec::new(),
			pull_samples: Vec::new(),
			spring_samples: Vec::new(),
			gravity_power_samples: Vec::new(),
			gravity_falloff_samples: Vec::new(),
			immobile_samples: Vec::new(),
			max_angle_x_samples: Vec::new(),
			max_angle_z_samples: Vec::new(),
			writeback_mode: Default::default(),
			limit: None,
			interaction: None,
			bone_node_indices: vec![0, 1],
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![
				make_group("physbone:Furniture/ChairRoot"),
				make_group("physbone:Mechanism/GearRoot"),
				make_group("physbone:Lighting/SearchLampRoot"),
				make_group("physbone:Head/J_Sec_Hair1_01"),
			],
			..Default::default()
		};

		let categories = settings
			.runtime_dynamics()
			.dynamics_groups()
			.map(|group| classify_dynamics_group_category(&scene, group, &default_spring_bone_categories()))
			.collect::<Vec<_>>();

		assert_eq!(categories, vec!["other", "other", "other", "hair"]);
	}

	#[test]
	fn unphysics_soft_body_category_keeps_body_jiggle_out_of_other() {
		let scene = UnaSceneSnapshot {
			nodes: vec![
				node(0.0, Vec3::ZERO, vec![1]),
				node(0.0, Vec3::new(0.0, 0.4, 0.0), vec![2]),
				node(0.0, Vec3::new(0.0, 0.4, 0.0), vec![3]),
				node(0.0, Vec3::new(0.0, 0.4, 0.0), vec![]),
			],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				interaction_chain_start_index: 0,
				source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
				enabled: true,
				source_id: "physbone:Armature/Hips/Spine/Chest/Breast_L".to_string(),
				comment: String::new(),
				category: String::new(),
				stiffness: 0.08,
				pull: 0.05,
				spring: 0.45,
				integration_type: Default::default(),
				gravity_power: 0.0,
				gravity_falloff: 0.0,
				immobile: 0.8,
				immobile_type: UnaDynamicsImmobileType::AllMotion,
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.4,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1, 2, 3],
			}],
			..Default::default()
		};

		let sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
		let runtime = sim.runtimes[0].as_ref().expect("runtime");
		assert_eq!(runtime.category, "soft_body");
		assert!(
			runtime.joints[0].parent_motion_follow < 0.70,
			"soft body should keep a general chain-based motion distribution: follow={}",
			runtime.joints[0].parent_motion_follow
		);
		assert!(
			runtime.joints.last().expect("tip").parent_motion_follow < runtime.joints[0].parent_motion_follow,
			"soft body should get root-to-tip motion distribution"
		);
		assert!(
			runtime.joints.last().expect("tip").bounce_response < 0.60,
			"soft body should keep bounce below a saturated inertia-retention response: bounce={}",
			runtime.joints.last().expect("tip").bounce_response
		);
	}

	#[test]
	fn chain_without_semantic_name_stays_other() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				node(0.0, Vec3::ZERO, vec![1]),
				node(0.0, Vec3::new(0.064, 0.187, 0.006), vec![2]),
				node(0.0, Vec3::new(0.019, 0.051, 0.0), vec![]),
			],
			roots: vec![0],
			..Default::default()
		};
		scene.nodes[1].name = Some("Bone".to_string());
		scene.nodes[2].name = Some("Bone.003".to_string());
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				interaction_chain_start_index: 0,
				source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
				enabled: true,
				source_id: "physbone:Armature/Hips/Spine/Chest/Neck/Head/J_Bip_C_Head/J_Bip_C_Head_2/Bone".to_string(),
				comment: "Bone".to_string(),
				category: String::new(),
				stiffness: 0.228,
				pull: 0.183,
				spring: 0.75,
				integration_type: UnaDynamicsIntegrationType::VrcAdvanced,
				gravity_power: 0.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.4,
				center_node: None,
				hit_radius: 0.02,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![1, 2],
			}],
			..Default::default()
		};
		let category = classify_dynamics_group_category(
			&scene,
			settings.runtime_dynamics().dynamics_groups().next().expect("group"),
			&default_spring_bone_categories(),
		);
		assert_eq!(category, "other");
	}

	#[test]
	fn unphysics_group_override_beats_category_override_by_source_id() {
		let scene = UnaSceneSnapshot {
			nodes: vec![
				node(0.0, Vec3::ZERO, vec![1]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![]),
				node(0.0, Vec3::new(2.0, 0.0, 0.0), vec![3]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![]),
			],
			roots: vec![0, 2],
			..Default::default()
		};
		let mut groups = Vec::new();
		for (source_id, chain) in [("physbone:left-ear", vec![0, 1]), ("physbone:right-ear", vec![2, 3])] {
			groups.push(UnaSpringBoneGroup {
				interaction_chain_start_index: 0,
				source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
				enabled: true,
				source_id: source_id.to_string(),
				comment: String::new(),
				category: "ears".to_string(),
				stiffness: 0.2,
				pull: 0.2,
				spring: 0.4,
				integration_type: UnaDynamicsIntegrationType::VrcAdvanced,
				gravity_power: 0.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.4,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: None,
				interaction: None,
				bone_node_indices: chain,
			});
		}
		let settings = UnaSpringBoneSettings {
			groups,
			..Default::default()
		};
		let config = DynamicsPhysicsConfig {
			overrides: vec![DynamicsCategoryOverride {
				category: "ears".to_string(),
				params: DynamicsPhysicsParams {
					rest_response: Some(0.45),
					bounce_scale: Some(0.8),
					..Default::default()
				},
			}],
			match_overrides: vec![
				DynamicsMatchOverride {
					name: "all ear test groups".to_string(),
					source_id_contains: vec!["ear".to_string()],
					params: DynamicsPhysicsParams {
						rest_response: Some(0.22),
						shape_preservation: Some(0.11),
						motion_coupling: Some(0.33),
						..Default::default()
					},
					..Default::default()
				},
				DynamicsMatchOverride {
					name: "left later override".to_string(),
					source_id_regex: vec![r"left-.+$".to_string()],
					params: DynamicsPhysicsParams {
						rest_response: Some(0.18),
						bounce_scale: Some(0.5),
						..Default::default()
					},
					..Default::default()
				},
				DynamicsMatchOverride {
					name: "right regex".to_string(),
					source_id_regex: vec![r"right-.+$".to_string()],
					params: DynamicsPhysicsParams {
						rest_response: Some(0.19),
						bounce_scale: Some(0.6),
						..Default::default()
					},
					..Default::default()
				},
				DynamicsMatchOverride {
					name: "broken regex".to_string(),
					source_id_regex: vec!["(".to_string()],
					params: DynamicsPhysicsParams {
						motion_coupling: Some(0.01),
						..Default::default()
					},
					..Default::default()
				},
				DynamicsMatchOverride {
					name: "missing cape".to_string(),
					source_id_contains: vec!["cape".to_string()],
					params: DynamicsPhysicsParams {
						rest_response: Some(0.01),
						..Default::default()
					},
					..Default::default()
				},
			],
			group_overrides: vec![
				DynamicsGroupOverride {
					source_id: "physbone:right-ear".to_string(),
					params: DynamicsPhysicsParams {
						solver: Some(DynamicsSolver::Xpbd),
						rest_response: Some(0.08),
						bounce_scale: Some(0.25),
						motion_coupling: Some(0.2),
						..Default::default()
					},
				},
				DynamicsGroupOverride {
					source_id: "physbone:missing".to_string(),
					params: DynamicsPhysicsParams {
						rest_response: Some(0.99),
						..Default::default()
					},
				},
			],
			..Default::default()
		};
		let sim = DynamicsSimulator::new_with_config(&scene, &settings, Vec::new(), config).expect("sim");
		let left = sim.runtimes[0].as_ref().expect("left");
		let right = sim.runtimes[1].as_ref().expect("right");
		assert_eq!(left.params.solver, DynamicsSolver::Verlet);
		assert_eq!(left.params.rest_response, 0.18);
		assert_eq!(left.params.shape_preservation, 0.11);
		assert!((left.params.bounce_response - 0.4 * 0.5).abs() < 1e-6);
		assert_eq!(left.joints[0].parent_motion_follow, 0.33);
		assert_eq!(right.params.solver, DynamicsSolver::Xpbd);
		assert_eq!(right.params.rest_response, 0.08);
		assert!((right.params.bounce_response - 0.4 * 0.25).abs() < 1e-6);
		assert_eq!(right.joints[0].parent_motion_follow, 0.2);
		let summaries = sim.response_group_summaries();
		let category_summaries = sim.response_category_summaries();
		let ears_category = category_summaries
			.iter()
			.find(|summary| summary.category == "ears")
			.expect("ears category summary");
		assert_eq!(ears_category.group_count, 2);
		assert_eq!(ears_category.matched_override_group_count, 2);
		assert_eq!(ears_category.group_override_group_count, 1);
		let left_summary = summaries
			.iter()
			.find(|summary| summary.source_id == "physbone:left-ear")
			.expect("left summary");
		assert_eq!(left_summary.matched_overrides, vec!["all ear test groups", "left later override"]);
		assert!(!left_summary.group_override_applied);
		assert!(left_summary
			.invalid_match_regexes
			.iter()
			.any(|message| message.contains("broken regex")));
		let right_summary = summaries
			.iter()
			.find(|summary| summary.source_id == "physbone:right-ear")
			.expect("right summary");
		assert_eq!(right_summary.matched_overrides, vec!["all ear test groups", "right regex"]);
		assert!(right_summary.group_override_applied);
		assert!(right_summary
			.invalid_match_regexes
			.iter()
			.any(|message| message.contains("broken regex")));
		let tuning_warnings = sim.tuning_warnings();
		assert!(tuning_warnings
			.iter()
			.any(|warning| warning.contains("dynamics match override did not match") && warning.contains("missing cape")));
		assert!(tuning_warnings
			.iter()
			.any(|warning| warning.contains("dynamics exact group override") && warning.contains("physbone:missing")));
	}

	#[test]
	fn unphysics_match_override_matches_comment_and_chain_names() {
		let scene = UnaSceneSnapshot {
			nodes: vec![
				node(0.0, Vec3::ZERO, vec![1]),
				UnaSceneNode {
					name: Some("SleeveRoot_L".to_string()),
					..node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![])
				},
				node(0.0, Vec3::new(2.0, 0.0, 0.0), vec![3]),
				UnaSceneNode {
					name: Some("GenericChild".to_string()),
					..node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![])
				},
			],
			roots: vec![0, 2],
			..Default::default()
		};
		let group = |source_id: &str, comment: &str, chain: Vec<usize>| UnaSpringBoneGroup {
			interaction_chain_start_index: 0,
			source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
			enabled: true,
			source_id: source_id.to_string(),
			comment: comment.to_string(),
			category: String::new(),
			stiffness: 0.2,
			pull: 0.2,
			spring: 0.0,
			integration_type: Default::default(),
			gravity_power: 0.0,
			gravity_falloff: 0.0,
			immobile: 0.0,
			immobile_type: Default::default(),
			gravity_dir: [0.0, -1.0, 0.0],
			drag_force: 0.2,
			center_node: None,
			hit_radius: 0.0,
			hit_radius_samples: Vec::new(),
			stiffness_samples: Vec::new(),
			pull_samples: Vec::new(),
			spring_samples: Vec::new(),
			gravity_power_samples: Vec::new(),
			gravity_falloff_samples: Vec::new(),
			immobile_samples: Vec::new(),
			max_angle_x_samples: Vec::new(),
			max_angle_z_samples: Vec::new(),
			writeback_mode: Default::default(),
			limit: None,
			interaction: None,
			bone_node_indices: chain,
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![
				group("physbone:generic-a", "", vec![0, 1]),
				group("physbone:generic-b", "Cloth panel", vec![2, 3]),
			],
			..Default::default()
		};
		let config = DynamicsPhysicsConfig {
			match_overrides: vec![
				DynamicsMatchOverride {
					name: "chain sleeve".to_string(),
					source_id_contains: vec!["sleeve".to_string()],
					params: DynamicsPhysicsParams {
						rest_response: Some(0.07),
						..Default::default()
					},
					..Default::default()
				},
				DynamicsMatchOverride {
					name: "comment cloth panel".to_string(),
					source_id_regex: vec!["cloth_panel".to_string()],
					params: DynamicsPhysicsParams {
						rest_response: Some(0.11),
						..Default::default()
					},
					..Default::default()
				},
			],
			..Default::default()
		};
		let sim = DynamicsSimulator::new_with_config(&scene, &settings, Vec::new(), config).expect("sim");
		let first = sim.runtimes[0].as_ref().expect("first");
		let second = sim.runtimes[1].as_ref().expect("second");
		assert_eq!(first.params.rest_response, 0.07);
		assert_eq!(second.params.rest_response, 0.11);
		let summaries = sim.response_group_summaries();
		assert_eq!(
			summaries
				.iter()
				.find(|summary| summary.source_id == "physbone:generic-a")
				.expect("generic-a")
				.matched_overrides,
			vec!["chain sleeve"]
		);
		assert_eq!(
			summaries
				.iter()
				.find(|summary| summary.source_id == "physbone:generic-b")
				.expect("generic-b")
				.matched_overrides,
			vec!["comment cloth panel"]
		);
	}

	#[test]
	fn unphysics_rest_response_and_shape_preservation_are_independent_terms() {
		let scene = UnaSceneSnapshot {
			nodes: vec![node(0.0, Vec3::ZERO, vec![1]), node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![])],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				interaction_chain_start_index: 0,
				source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
				enabled: true,
				source_id: String::new(),
				comment: "ears".to_string(),
				category: "ears".to_string(),
				stiffness: 0.228,
				pull: 0.183,
				spring: 0.75,
				integration_type: UnaDynamicsIntegrationType::VrcAdvanced,
				gravity_power: 0.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.4,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let config = DynamicsPhysicsConfig {
			overrides: vec![DynamicsCategoryOverride {
				category: "ears".to_string(),
				params: DynamicsPhysicsParams {
					solver: Some(DynamicsSolver::Verlet),
					damping_half_life_ms: None,
					stiffness_hz: Some(0.02),
					shape_preservation: Some(0.03),
					bounce_scale: Some(0.25),
					xpbd_compliance: None,
					gravity_scale: None,
					drag_scale: None,
					constraint_iterations: None,
					..Default::default()
				},
			}],
			..Default::default()
		};
		let sim = DynamicsSimulator::new_with_config(&scene, &settings, Vec::new(), config).expect("sim");
		let rt = sim.runtimes[0].as_ref().expect("runtime");
		assert!((rt.params.rest_response - 0.02).abs() < 1e-6);
		assert!((rt.params.shape_preservation - 0.03).abs() < 1e-6);
		assert!((rt.joints[0].shape_preservation - 0.03).abs() < 1e-6);
		assert!((rt.params.bounce_response - 0.1875).abs() < 1e-6);
	}

	#[test]
	fn unphysics_legacy_stiffness_hz_override_lowers_to_normalized_rest_response() {
		let scene = UnaSceneSnapshot {
			nodes: vec![node(0.0, Vec3::ZERO, vec![1]), node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![])],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				interaction_chain_start_index: 0,
				source_kind: Default::default(),
				enabled: true,
				source_id: String::new(),
				comment: "hair".to_string(),
				category: "hair".to_string(),
				stiffness: 0.05,
				pull: 0.05,
				spring: 0.0,
				integration_type: Default::default(),
				gravity_power: 0.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.2,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let config = DynamicsPhysicsConfig {
			overrides: vec![DynamicsCategoryOverride {
				category: "hair".to_string(),
				params: DynamicsPhysicsParams {
					stiffness_hz: Some(12.0),
					shape_preservation: Some(0.03),
					..Default::default()
				},
			}],
			..Default::default()
		};
		let sim = DynamicsSimulator::new_with_config(&scene, &settings, Vec::new(), config).expect("sim");
		let rt = sim.runtimes[0].as_ref().expect("runtime");

		assert!((rt.params.rest_response - 0.2).abs() < 1e-6);
		assert!((rt.params.shape_preservation - 0.03).abs() < 1e-6);
		assert!((rt.joints[0].rest_response - 0.2).abs() < 1e-6);
		assert!((rt.joints[0].shape_preservation - 0.03).abs() < 1e-6);
	}

	#[test]
	fn unphysics_xpbd_rest_response_override_updates_implicit_compliance() {
		fn resolved_compliance(rest_response: f32) -> f32 {
			let scene = UnaSceneSnapshot {
				nodes: vec![node(0.0, Vec3::ZERO, vec![1]), node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![])],
				roots: vec![0],
				..Default::default()
			};
			let settings = UnaSpringBoneSettings {
				groups: vec![UnaSpringBoneGroup {
					interaction_chain_start_index: 0,
					source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
					enabled: true,
					source_id: String::new(),
					comment: "ears".to_string(),
					category: "ears".to_string(),
					stiffness: 0.6,
					pull: 0.6,
					spring: 0.5,
					integration_type: UnaDynamicsIntegrationType::VrcAdvanced,
					gravity_power: 0.0,
					gravity_falloff: 0.0,
					immobile: 0.0,
					immobile_type: Default::default(),
					gravity_dir: [0.0, -1.0, 0.0],
					drag_force: 0.2,
					center_node: None,
					hit_radius: 0.0,
					hit_radius_samples: Vec::new(),
					stiffness_samples: Vec::new(),
					pull_samples: Vec::new(),
					spring_samples: Vec::new(),
					gravity_power_samples: Vec::new(),
					gravity_falloff_samples: Vec::new(),
					immobile_samples: Vec::new(),
					max_angle_x_samples: Vec::new(),
					max_angle_z_samples: Vec::new(),
					writeback_mode: Default::default(),
					limit: None,
					interaction: None,
					bone_node_indices: vec![0, 1],
				}],
				..Default::default()
			};
			let config = DynamicsPhysicsConfig {
				overrides: vec![DynamicsCategoryOverride {
					category: "ears".to_string(),
					params: DynamicsPhysicsParams {
						solver: Some(DynamicsSolver::Xpbd),
						rest_response: Some(rest_response),
						..Default::default()
					},
				}],
				..Default::default()
			};
			let sim = DynamicsSimulator::new_with_config(&scene, &settings, Vec::new(), config).expect("sim");
			sim.runtimes[0].as_ref().expect("runtime").params.xpbd_compliance
		}

		let soft = resolved_compliance(0.03);
		let firm = resolved_compliance(0.30);
		assert!(
			soft > firm * 10.0,
			"XPBD implicit compliance must follow UNPhysics rest_response override: soft={soft} firm={firm}"
		);
	}

	#[test]
	fn unphysics_xpbd_rest_response_sets_softness_floor_for_explicit_compliance() {
		let scene = UnaSceneSnapshot {
			nodes: vec![node(0.0, Vec3::ZERO, vec![1]), node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![])],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				interaction_chain_start_index: 0,
				source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
				enabled: true,
				source_id: String::new(),
				comment: "ears".to_string(),
				category: "ears".to_string(),
				stiffness: 0.6,
				pull: 0.6,
				spring: 0.5,
				integration_type: UnaDynamicsIntegrationType::VrcAdvanced,
				gravity_power: 0.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.2,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1],
			}],
			..Default::default()
		};
		let rest_response = 0.03;
		let config = DynamicsPhysicsConfig {
			overrides: vec![DynamicsCategoryOverride {
				category: "ears".to_string(),
				params: DynamicsPhysicsParams {
					solver: Some(DynamicsSolver::Xpbd),
					rest_response: Some(rest_response),
					xpbd_compliance: Some(0.0001),
					..Default::default()
				},
			}],
			..Default::default()
		};
		let sim = DynamicsSimulator::new_with_config(&scene, &settings, Vec::new(), config).expect("sim");
		let resolved = sim.runtimes[0].as_ref().expect("runtime").params.xpbd_compliance;
		let floor = convert_unphysics_rest_response_to_xpbd_compliance(rest_response);
		assert!(
			(resolved - floor).abs() < 1e-6,
			"XPBD explicit compliance must not harden past UNPhysics rest_response softness floor: resolved={resolved} floor={floor}"
		);
	}

	#[test]
	fn unphysics_xpbd_rest_response_floor_changes_solver_output_with_hard_explicit_compliance() {
		fn remaining_deflection_after_recovery(rest_response: f32) -> f32 {
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
					interaction_chain_start_index: 0,
					source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
					enabled: true,
					source_id: String::new(),
					comment: "profile-adjustable xpbd ears".to_string(),
					category: "ears".to_string(),
					stiffness: 0.6,
					pull: 0.6,
					spring: 0.55,
					integration_type: UnaDynamicsIntegrationType::VrcAdvanced,
					gravity_power: 0.0,
					gravity_falloff: 0.0,
					immobile: 0.0,
					immobile_type: Default::default(),
					gravity_dir: [0.0, -1.0, 0.0],
					drag_force: 0.25,
					center_node: None,
					hit_radius: 0.0,
					hit_radius_samples: Vec::new(),
					stiffness_samples: Vec::new(),
					pull_samples: Vec::new(),
					spring_samples: Vec::new(),
					gravity_power_samples: Vec::new(),
					gravity_falloff_samples: Vec::new(),
					immobile_samples: Vec::new(),
					max_angle_x_samples: Vec::new(),
					max_angle_z_samples: Vec::new(),
					writeback_mode: Default::default(),
					limit: None,
					interaction: None,
					bone_node_indices: vec![0, 1, 2],
				}],
				..Default::default()
			};
			let config = DynamicsPhysicsConfig {
				overrides: vec![DynamicsCategoryOverride {
					category: "ears".to_string(),
					params: DynamicsPhysicsParams {
						solver: Some(DynamicsSolver::Xpbd),
						rest_response: Some(rest_response),
						xpbd_compliance: Some(0.0001),
						bounce_scale: Some(0.4),
						motion_coupling: Some(0.35),
						constraint_iterations: Some(6),
						..Default::default()
					},
				}],
				..Default::default()
			};
			let mut sim = DynamicsSimulator::new_with_config(&scene, &settings, Vec::new(), config).expect("sim");
			scene.nodes[0].transform = Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2).to_cols_array();
			sim.step(&mut scene, &settings, 1.0 / 60.0);
			let rest_after_turn = Vec3::new(-2.0, 0.0, 0.0);
			for _ in 0..45 {
				sim.step(&mut scene, &settings, 1.0 / 60.0);
			}
			let tip = world_from_snapshot(&scene)[2].transform_point3(Vec3::ZERO);
			tip.distance(rest_after_turn)
		}

		let soft = remaining_deflection_after_recovery(0.03);
		let firm = remaining_deflection_after_recovery(0.30);
		assert!(
			soft > firm * 1.4,
			"XPBD solver output must remain profile-adjustable even with hard explicit compliance: soft={soft} firm={firm}"
		);
	}

	#[test]
	fn unphysics_bounce_scale_changes_solver_output() {
		fn residual_velocity_with_bounce_scale(bounce_scale: f32) -> f32 {
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
					interaction_chain_start_index: 0,
					source_kind: Default::default(),
					enabled: true,
					source_id: String::new(),
					comment: "ears".to_string(),
					category: "ears".to_string(),
					stiffness: 0.12,
					pull: 0.12,
					spring: 1.0,
					integration_type: Default::default(),
					gravity_power: 0.0,
					gravity_falloff: 0.0,
					immobile: 0.0,
					immobile_type: Default::default(),
					gravity_dir: [0.0, -1.0, 0.0],
					drag_force: 0.05,
					center_node: None,
					hit_radius: 0.0,
					hit_radius_samples: Vec::new(),
					stiffness_samples: Vec::new(),
					pull_samples: Vec::new(),
					spring_samples: Vec::new(),
					gravity_power_samples: Vec::new(),
					gravity_falloff_samples: Vec::new(),
					immobile_samples: Vec::new(),
					max_angle_x_samples: Vec::new(),
					max_angle_z_samples: Vec::new(),
					writeback_mode: Default::default(),
					limit: None,
					interaction: None,
					bone_node_indices: vec![0, 1, 2],
				}],
				..Default::default()
			};
			let config = DynamicsPhysicsConfig {
				overrides: vec![DynamicsCategoryOverride {
					category: "ears".to_string(),
					params: DynamicsPhysicsParams {
						solver: Some(DynamicsSolver::Verlet),
						stiffness_hz: Some(0.12),
						bounce_scale: Some(bounce_scale),
						..Default::default()
					},
				}],
				..Default::default()
			};
			let mut sim = DynamicsSimulator::new_with_config(&scene, &settings, Vec::new(), config).expect("sim");
			for frame in 1..=40 {
				let angle = std::f32::consts::FRAC_PI_2 * frame as f32 / 40.0;
				scene.nodes[0].transform = Mat4::from_rotation_z(angle).to_cols_array();
				sim.step(&mut scene, &settings, 1.0 / 60.0);
			}
			for _ in 0..12 {
				sim.step(&mut scene, &settings, 1.0 / 60.0);
			}
			sim.runtimes[0]
				.as_ref()
				.expect("runtime")
				.joints
				.last()
				.expect("joint")
				.prev_velocity
				.length()
		}

		let low_bounce = residual_velocity_with_bounce_scale(0.0);
		let high_bounce = residual_velocity_with_bounce_scale(1.0);
		assert!(
			high_bounce > low_bounce * 1.2,
			"UNPhysics bounce_scale should affect solver output: low={low_bounce} high={high_bounce}"
		);
	}

	#[test]
	fn unphysics_damping_half_life_changes_residual_motion() {
		fn residual_velocity_with_half_life(damping_half_life_ms: f32) -> f32 {
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
					interaction_chain_start_index: 0,
					source_kind: Default::default(),
					enabled: true,
					source_id: String::new(),
					comment: "cloth".to_string(),
					category: "cloth".to_string(),
					stiffness: 0.12,
					pull: 0.10,
					spring: 0.8,
					integration_type: Default::default(),
					gravity_power: 0.0,
					gravity_falloff: 0.0,
					immobile: 0.0,
					immobile_type: Default::default(),
					gravity_dir: [0.0, -1.0, 0.0],
					drag_force: 0.02,
					center_node: None,
					hit_radius: 0.0,
					hit_radius_samples: Vec::new(),
					stiffness_samples: Vec::new(),
					pull_samples: Vec::new(),
					spring_samples: Vec::new(),
					gravity_power_samples: Vec::new(),
					gravity_falloff_samples: Vec::new(),
					immobile_samples: Vec::new(),
					max_angle_x_samples: Vec::new(),
					max_angle_z_samples: Vec::new(),
					writeback_mode: Default::default(),
					limit: None,
					interaction: None,
					bone_node_indices: vec![0, 1, 2],
				}],
				..Default::default()
			};
			let config = DynamicsPhysicsConfig {
				overrides: vec![DynamicsCategoryOverride {
					category: "cloth".to_string(),
					params: DynamicsPhysicsParams {
						solver: Some(DynamicsSolver::Verlet),
						damping_half_life_ms: Some(damping_half_life_ms),
						rest_response: Some(0.10),
						shape_preservation: Some(0.03),
						bounce_scale: Some(1.0),
						motion_coupling: Some(0.25),
						..Default::default()
					},
				}],
				..Default::default()
			};
			let mut sim = DynamicsSimulator::new_with_config(&scene, &settings, Vec::new(), config).expect("sim");
			for frame in 1..=18 {
				let angle = std::f32::consts::FRAC_PI_2 * frame as f32 / 18.0;
				scene.nodes[0].transform = Mat4::from_rotation_z(angle).to_cols_array();
				sim.step(&mut scene, &settings, 1.0 / 60.0);
			}
			for _ in 0..45 {
				sim.step(&mut scene, &settings, 1.0 / 60.0);
			}
			sim.runtimes[0]
				.as_ref()
				.expect("runtime")
				.joints
				.iter()
				.map(|joint| joint.prev_velocity.length())
				.sum()
		}

		let strong_damping = residual_velocity_with_half_life(40.0);
		let weak_damping = residual_velocity_with_half_life(800.0);
		assert!(
			strong_damping < weak_damping * 0.65,
			"damping half-life should reduce residual motion: strong={strong_damping} weak={weak_damping}"
		);
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
				interaction_chain_start_index: 0,
				source_kind: Default::default(),
				enabled: true,
				source_id: String::new(),
				comment: String::new(),
				category: String::new(),
				stiffness: 0.1,
				pull: 0.1,
				spring: 0.0,
				integration_type: Default::default(),
				gravity_power: 1.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.3,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
		let rt = sim.runtimes[0].as_ref().expect("runtime");
		assert_eq!(sim.physics.simulation_hz, 60.0);
		assert_eq!(rt.params.damping_half_life_ms, None);
		assert!((rt.params.rest_response - 0.1).abs() < 1e-6);
		assert_eq!(rt.params.bounce_response, 0.0);
		assert!((rt.params.xpbd_compliance - convert_unphysics_response_to_xpbd_compliance(0.1)).abs() < 1e-6);
		assert_eq!(rt.params.gravity_scale, 1.0);
		assert_eq!(rt.params.gravity_falloff, 0.0);
		assert_eq!(rt.params.immobile, 0.0);
		assert_eq!(rt.params.drag_scale, 1.0);
	}

	#[test]
	fn default_config_preserves_normalized_undynamics_terms() {
		let scene = UnaSceneSnapshot {
			nodes: vec![node(0.0, Vec3::ZERO, vec![1]), node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![])],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				interaction_chain_start_index: 0,
				source_kind: Default::default(),
				enabled: true,
				source_id: String::new(),
				comment: String::new(),
				category: String::new(),
				stiffness: 0.8,
				pull: 0.25,
				spring: 0.15,
				integration_type: Default::default(),
				gravity_power: 1.0,
				gravity_falloff: 0.6,
				immobile: 0.35,
				immobile_type: Default::default(),
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.3,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
		let rt = sim.runtimes[0].as_ref().expect("runtime");
		assert!((rt.params.rest_response - 0.25).abs() < 1e-6);
		assert!((rt.params.bounce_response - 0.15).abs() < 1e-6);
		assert_eq!(rt.params.gravity_falloff, 0.6);
		assert_eq!(rt.params.immobile, 0.35);
	}

	#[test]
	fn legacy_compat_solver_aliases_deserialize_as_verlet() {
		assert_eq!(
			serde_json::from_str::<DynamicsSolver>("\"compat_univrm\"").expect("compat_univrm"),
			DynamicsSolver::Verlet
		);
		assert_eq!(
			serde_json::from_str::<DynamicsSolver>("\"compat_euler\"").expect("compat_euler"),
			DynamicsSolver::Verlet
		);
	}

	#[test]
	fn xpbd_compliance_changes_solver_response() {
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
				interaction_chain_start_index: 0,
				source_kind: Default::default(),
				enabled: true,
				source_id: String::new(),
				comment: String::new(),
				category: String::new(),
				stiffness: 0.8,
				pull: 0.8,
				spring: 0.0,
				integration_type: Default::default(),
				gravity_power: 1.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [1.0, -1.0, 0.0],
				drag_force: 0.2,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1, 2],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let xpbd_config = |xpbd_compliance| DynamicsPhysicsConfig {
			overrides: vec![DynamicsCategoryOverride {
				category: "other".to_string(),
				params: DynamicsPhysicsParams {
					solver: Some(DynamicsSolver::Xpbd),
					bounce_scale: None,
					xpbd_compliance: Some(xpbd_compliance),
					constraint_iterations: Some(6),
					..Default::default()
				},
			}],
			..Default::default()
		};
		let mut firm_scene = base_scene.clone();
		let mut soft_scene = base_scene;
		let mut firm_xpbd =
			DynamicsSimulator::new_with_config(&firm_scene, &settings, Vec::new(), xpbd_config(0.005)).expect("firm xpbd sim");
		let mut soft_xpbd =
			DynamicsSimulator::new_with_config(&soft_scene, &settings, Vec::new(), xpbd_config(0.2)).expect("soft xpbd sim");
		for _ in 0..60 {
			firm_xpbd.step(&mut firm_scene, &settings, 1.0 / 60.0);
			soft_xpbd.step(&mut soft_scene, &settings, 1.0 / 60.0);
		}
		let firm_tip = world_from_snapshot(&firm_scene)[2].transform_point3(Vec3::ZERO);
		let soft_tip = world_from_snapshot(&soft_scene)[2].transform_point3(Vec3::ZERO);
		let rest_tip = Vec3::new(0.0, 2.0, 0.0);
		assert!(firm_tip.is_finite() && soft_tip.is_finite());
		assert!(
			(firm_tip - soft_tip).length() > 1e-4,
			"XPBD compliance should affect the backend response: firm={firm_tip:?} soft={soft_tip:?} rest={rest_tip:?}"
		);
	}

	#[test]
	fn xpbd_solver_backend_differs_from_verlet_under_explicit_config() {
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
				interaction_chain_start_index: 0,
				source_kind: Default::default(),
				enabled: true,
				source_id: String::new(),
				comment: String::new(),
				category: String::new(),
				stiffness: 0.8,
				pull: 0.8,
				spring: 0.0,
				integration_type: Default::default(),
				gravity_power: 1.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [1.0, -1.0, 0.0],
				drag_force: 0.2,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1, 2],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let mut xpbd_scene = base_scene.clone();
		let mut verlet_scene = base_scene;
		let mut xpbd = DynamicsSimulator::new_with_config(
			&xpbd_scene,
			&settings,
			Vec::new(),
			DynamicsPhysicsConfig {
				overrides: vec![DynamicsCategoryOverride {
					category: "other".to_string(),
					params: DynamicsPhysicsParams {
						solver: Some(DynamicsSolver::Xpbd),
						bounce_scale: None,
						xpbd_compliance: Some(0.005),
						constraint_iterations: Some(6),
						..Default::default()
					},
				}],
				..Default::default()
			},
		)
		.expect("xpbd sim");
		let mut verlet =
			DynamicsSimulator::new_with_config(&verlet_scene, &settings, Vec::new(), DynamicsPhysicsConfig::default()).expect("verlet sim");
		for _ in 0..60 {
			xpbd.step(&mut xpbd_scene, &settings, 1.0 / 60.0);
			verlet.step(&mut verlet_scene, &settings, 1.0 / 60.0);
		}
		let xpbd_tip = world_from_snapshot(&xpbd_scene)[2].transform_point3(Vec3::ZERO);
		let verlet_tip = world_from_snapshot(&verlet_scene)[2].transform_point3(Vec3::ZERO);
		assert!(
			(xpbd_tip - verlet_tip).length() > 1e-4,
			"explicit XPBD config should select a distinct backend response: xpbd={xpbd_tip:?} verlet={verlet_tip:?}"
		);
	}

	#[test]
	fn parent_translation_moves_tail_when_immobile_zero() {
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
				interaction_chain_start_index: 0,
				source_kind: Default::default(),
				enabled: true,
				source_id: String::new(),
				comment: String::new(),
				category: String::new(),
				stiffness: 0.0,
				pull: 0.0,
				spring: 0.0,
				integration_type: Default::default(),
				gravity_power: 0.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.0,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1, 2],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let mut sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
		scene.nodes[0].transform = Mat4::from_translation(Vec3::new(1.0, 0.0, 0.0)).to_cols_array();
		sim.step(&mut scene, &settings, 1.0 / 60.0);
		let tip = world_from_snapshot(&scene)[2].transform_point3(Vec3::ZERO);
		assert!(
			(tip - Vec3::new(1.0, 2.0, 0.0)).length() > 0.25,
			"immobile=0 should keep parent translation as motion that can sway the chain: tip={tip:?}"
		);
	}

	#[test]
	fn parent_rotation_moves_tail_when_immobile_zero() {
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
				interaction_chain_start_index: 0,
				source_kind: Default::default(),
				enabled: true,
				source_id: String::new(),
				comment: String::new(),
				category: String::new(),
				stiffness: 0.0,
				pull: 0.0,
				spring: 0.0,
				integration_type: Default::default(),
				gravity_power: 0.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.0,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1, 2],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let mut sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
		scene.nodes[0].transform = Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2).to_cols_array();
		sim.step(&mut scene, &settings, 1.0 / 60.0);
		let tip = world_from_snapshot(&scene)[2].transform_point3(Vec3::ZERO);
		assert!(
			(tip - Vec3::new(-2.0, 0.0, 0.0)).length() > 0.25,
			"immobile=0 should keep parent rotation as motion that can sway the chain: tip={tip:?}"
		);
	}

	#[test]
	fn immobile_type_all_motion_damps_local_parent_motion() {
		fn rotated_tip_for(immobile_type: UnaDynamicsImmobileType) -> Vec3 {
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
					interaction_chain_start_index: 0,
					source_kind: Default::default(),
					enabled: true,
					source_id: String::new(),
					comment: String::new(),
					category: String::new(),
					stiffness: 0.0,
					pull: 0.0,
					spring: 0.0,
					integration_type: Default::default(),
					gravity_power: 0.0,
					gravity_falloff: 0.0,
					immobile: 1.0,
					immobile_type,
					gravity_dir: [0.0, -1.0, 0.0],
					drag_force: 0.0,
					center_node: None,
					hit_radius: 0.0,
					hit_radius_samples: Vec::new(),
					stiffness_samples: Vec::new(),
					pull_samples: Vec::new(),
					spring_samples: Vec::new(),
					gravity_power_samples: Vec::new(),
					gravity_falloff_samples: Vec::new(),
					immobile_samples: Vec::new(),
					max_angle_x_samples: Vec::new(),
					max_angle_z_samples: Vec::new(),
					writeback_mode: Default::default(),
					limit: None,
					interaction: None,
					bone_node_indices: vec![0, 1, 2],
				}],
				colliders: Vec::new(),
				..Default::default()
			};
			let mut sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
			scene.nodes[0].transform = Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2).to_cols_array();
			sim.step(&mut scene, &settings, 1.0 / 60.0);
			world_from_snapshot(&scene)[2].transform_point3(Vec3::ZERO)
		}

		let all_motion_tip = rotated_tip_for(UnaDynamicsImmobileType::AllMotion);
		let world_tip = rotated_tip_for(UnaDynamicsImmobileType::World);
		assert!(
			(all_motion_tip - Vec3::new(-2.0, 0.0, 0.0)).length() < 0.3,
			"All Motion immobile should strongly damp local parent/head motion: tip={all_motion_tip:?}"
		);
		assert!(
			(world_tip - Vec3::new(-2.0, 0.0, 0.0)).length() > 0.25,
			"World immobile should leave local parent/head motion available to sway the chain: tip={world_tip:?}"
		);
	}

	#[test]
	fn center_node_defines_motion_frame_for_parent_motion_follow() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				node(0.0, Vec3::ZERO, vec![]),
				node(0.0, Vec3::ZERO, vec![2]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![3]),
				node(0.0, Vec3::new(0.0, 1.0, 0.0), vec![]),
			],
			roots: vec![0, 1],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: vec![UnaSpringBoneGroup {
				interaction_chain_start_index: 0,
				source_kind: Default::default(),
				enabled: true,
				source_id: String::new(),
				comment: String::new(),
				category: String::new(),
				stiffness: 0.0,
				pull: 0.0,
				spring: 0.0,
				integration_type: Default::default(),
				gravity_power: 0.0,
				gravity_falloff: 0.0,
				immobile: 1.0,
				immobile_type: UnaDynamicsImmobileType::AllMotion,
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.0,
				center_node: Some(0),
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![1, 2, 3],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		let mut sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
		scene.nodes[1].transform = Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2).to_cols_array();
		sim.step(&mut scene, &settings, 1.0 / 60.0);
		let tip = world_from_snapshot(&scene)[3].transform_point3(Vec3::ZERO);
		assert!(
			(tip - Vec3::new(-2.0, 0.0, 0.0)).length() > 0.25,
			"center_node should make UNPhysics measure motion from the center frame, leaving local chain-root motion available to sway: tip={tip:?}"
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
				interaction_chain_start_index: 0,
				source_kind: Default::default(),
				enabled: true,
				source_id: String::new(),
				comment: String::new(),
				category: String::new(),
				stiffness: 0.1,
				pull: 0.1,
				spring: 0.0,
				integration_type: Default::default(),
				gravity_power: 1.0,
				gravity_falloff: 0.0,
				immobile: 0.0,
				immobile_type: Default::default(),
				gravity_dir: [0.0, -1.0, 0.0],
				drag_force: 0.3,
				center_node: None,
				hit_radius: 0.0,
				hit_radius_samples: Vec::new(),
				stiffness_samples: Vec::new(),
				pull_samples: Vec::new(),
				spring_samples: Vec::new(),
				gravity_power_samples: Vec::new(),
				gravity_falloff_samples: Vec::new(),
				immobile_samples: Vec::new(),
				max_angle_x_samples: Vec::new(),
				max_angle_z_samples: Vec::new(),
				writeback_mode: Default::default(),
				limit: None,
				interaction: None,
				bone_node_indices: vec![0, 1],
			}],
			colliders: Vec::new(),
			..Default::default()
		};
		// 2 ノードチェーン = 1 joint。tail を bone_axis に沿って初期化し動作することを確認。
		let mut sim = DynamicsSimulator::new(&scene, &settings).expect("sim");
		for _ in 0..120 {
			sim.step(&mut scene, &settings, 1.0 / 60.0);
		}
		// 1 joint chain は重力 (y=-1) で tail が下に流れる。
		let tip = world_from_snapshot(&scene)[1].transform_point3(Vec3::ZERO);
		assert!(tip.is_finite(), "tip should remain finite: {:?}", tip);
	}
}
