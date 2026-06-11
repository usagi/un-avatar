//! UN Avatar — UNA 内部表現の中核（bootstrap）。
//!
//! 設計: `docs/crate-io-plugin-plan.md` §4.2

#![forbid(unsafe_code)]

use std::{
	borrow::Cow,
	collections::{BTreeMap, BTreeSet},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use un_avatar_types::{FormatId, HumanoidProfile};

/// 材質のシェーディング種別（MaterialPolicy v0・レンダラーが解釈）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnaShadingModel {
	/// 簡易ライト（Lambert）＋ベース色テクスチャ。
	#[default]
	LitLambert,
	/// `KHR_materials_unlit` 相当。
	Unlit,
	/// Legacy v1 avatar toon path for VRM/MToon inputs.
	MToonLike,
	/// v2 avatar toon path for `.unavatar` / lilToon-compatible inputs.
	LilToonLike,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UnaRuntimeToonModel {
	MToonLike,
	LilToonLike,
}

impl UnaShadingModel {
	/// WGSL `drawu.params.x` 用の判別子。
	pub fn as_draw_discriminant(self) -> f32 {
		match self {
			UnaShadingModel::LitLambert => 0.0,
			UnaShadingModel::Unlit => 1.0,
			UnaShadingModel::MToonLike => 2.0,
			UnaShadingModel::LilToonLike => 3.0,
		}
	}

	pub fn is_toon_like(self) -> bool {
		matches!(self, UnaShadingModel::MToonLike | UnaShadingModel::LilToonLike)
	}

	pub fn is_liltoon_like(self) -> bool {
		matches!(self, UnaShadingModel::LilToonLike)
	}

	pub fn is_mtoon_like(self) -> bool {
		matches!(self, UnaShadingModel::MToonLike)
	}
}

/// glTF `alphaMode` 相当（`MASK` は `alphaCutoff` による切り抜き）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnaAlphaMode {
	#[default]
	Opaque,
	Mask,
	Blend,
}

impl UnaAlphaMode {
	/// WGSL `drawu.params.y`: `OPAQUE` = 0、`MASK` = 1（discard + 不透明出し）、`BLEND` = 2（テクスチャ α をそのまま出す・ブレンドパス用）。
	pub fn as_shader_alpha_kind(self) -> f32 {
		match self {
			UnaAlphaMode::Opaque => 0.0,
			UnaAlphaMode::Mask => 1.0,
			UnaAlphaMode::Blend => 2.0,
		}
	}
}

/// Material face culling. glTF only exposes `doubleSided`, but `.unavatar`
/// keeps Unity/lilToon `Cull Off / Front / Back` as a first-class value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnaCullMode {
	Off,
	Front,
	#[default]
	Back,
}

/// 1 モーフターゲット分のデルタ（頂点数はベース `UnaMeshBuffers::positions` と一致）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaMorphTargetDeltas {
	pub position_deltas: Vec<[f32; 3]>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub normal_deltas: Option<Vec<[f32; 3]>>,
}

/// VRM の式プリセット 1 件（BlendShapeClip / Expression に対応する正規化形）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaExpressionPreset {
	pub name: String,
	#[serde(default)]
	pub binds: Vec<UnaMorphTargetBind>,
}

/// 式ウェイト 1.0 のときに効くモーフバインド（mesh / primitive / morph index）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaMorphTargetBind {
	pub mesh_index: usize,
	pub primitive_index: usize,
	pub morph_target_index: usize,
	#[serde(default = "one_f32")]
	pub weight_scale: f32,
}

/// VRM から構造化した式カタログ（`source` JSON からの投影。正本は引き続き `UnaVrmExtension::source`）。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UnaExpressionCatalog {
	#[serde(default)]
	pub presets: Vec<UnaExpressionPreset>,
}

/// ランタイムの式プリセットウェイト（0..=1 推奨）。キーは `UnaExpressionPreset::name`。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UnaExpressionWeights {
	#[serde(default)]
	pub preset_weights: BTreeMap<String, f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UnaRuntimeActionSet {
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub actions: Vec<UnaRuntimeAction>,
}

pub const UNA_RUNTIME_ACTION_PARAMETER_EPSILON: f32 = 0.005;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UnaRuntimeActionQuery<'a> {
	pub action_id: Option<&'a str>,
	pub supervisor_command: Option<&'a str>,
	pub expression_menu_path: Option<&'a str>,
	pub parameter_name: Option<&'a str>,
	pub parameter_value: Option<f32>,
}

impl UnaRuntimeActionSet {
	pub fn find_action(&self, query: UnaRuntimeActionQuery<'_>) -> Option<&UnaRuntimeAction> {
		self.actions.iter().find(|action| action.matches_query(query))
	}

	pub fn evaluation_target_write_collisions(&self) -> Vec<UnaEvaluationTargetWriteCollision> {
		let mut writes_by_target: BTreeMap<(UnaEvaluationTargetKind, String), Vec<UnaEvaluationRuntimeActionTargetWrite>> = BTreeMap::new();
		for action in &self.actions {
			for write in action.evaluation_target_writes() {
				writes_by_target
					.entry((write.target_kind.clone(), write.target_key.clone()))
					.or_default()
					.push(write);
			}
		}
		writes_by_target
			.into_iter()
			.filter_map(|((target_kind, target_key), writes)| {
				let owner_keys = writes
					.iter()
					.map(|write| write.owner_key.clone())
					.collect::<BTreeSet<_>>()
					.into_iter()
					.collect::<Vec<_>>();
				if owner_keys.len() < 2 {
					return None;
				}
				let action_ids = writes
					.iter()
					.map(|write| write.action_id.clone())
					.collect::<BTreeSet<_>>()
					.into_iter()
					.collect::<Vec<_>>();
				Some(UnaEvaluationTargetWriteCollision {
					target_kind,
					target_key,
					owner_keys,
					action_ids,
					writes,
				})
			})
			.collect()
	}
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UnaRuntimeAction {
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub id: String,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub label: String,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub triggers: Vec<UnaRuntimeActionTrigger>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub conditions: Vec<UnaRuntimeActionCondition>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub effects: Vec<UnaRuntimeActionEffect>,
}

impl UnaRuntimeAction {
	pub fn matches_query(&self, query: UnaRuntimeActionQuery<'_>) -> bool {
		query.action_id.is_some_and(|id| self.id == id) || self.triggers.iter().any(|trigger| trigger.matches_query(query))
	}

	pub fn parameter_assignments(&self) -> BTreeMap<String, f32> {
		self.triggers
			.iter()
			.filter_map(|trigger| match trigger {
				UnaRuntimeActionTrigger::ParameterValue { name, value } if !name.is_empty() => Some((name.clone(), *value)),
				_ => None,
			})
			.collect()
	}

	pub fn parameter_condition_state(&self, name: &str, value: f32) -> Option<bool> {
		self.parameter_condition_state_in_scene(None, name, value)
	}

	pub fn parameter_condition_state_in_scene(&self, scene: Option<&UnaSceneSnapshot>, name: &str, value: f32) -> Option<bool> {
		let mut saw_parameter_condition = false;
		for condition in &self.conditions {
			if let Some(active) = condition.parameter_condition_matches_in_scene(scene, name, value) {
				saw_parameter_condition = true;
				if !active {
					return Some(false);
				}
			}
		}
		saw_parameter_condition.then_some(true)
	}

	pub fn condition_parameter_names(&self) -> Vec<String> {
		let mut names = BTreeSet::new();
		for condition in &self.conditions {
			if let Some(name) = condition.parameter_name.as_deref().filter(|name| !name.is_empty()) {
				names.insert(name.to_string());
			}
		}
		names.into_iter().collect()
	}

	pub fn current_parameter_condition_state(
		&self,
		scene: Option<&UnaSceneSnapshot>,
		parameter_values: &BTreeMap<String, f32>,
	) -> Option<&'static str> {
		let mut saw_condition = false;
		let mut saw_runtime_value = false;
		let mut saw_inactive = false;
		for condition in &self.conditions {
			let Some(name) = condition.parameter_name.as_deref() else {
				continue;
			};
			saw_condition = true;
			let Some(value) = parameter_values.get(name).copied() else {
				continue;
			};
			saw_runtime_value = true;
			match self.parameter_condition_state_in_scene(scene, name, value) {
				Some(true) => return Some("active"),
				Some(false) => saw_inactive = true,
				None => {}
			}
		}
		if saw_inactive {
			Some("inactive")
		} else if saw_condition && !saw_runtime_value {
			Some("missing_parameter")
		} else {
			None
		}
	}

	pub fn evaluation_target_writes(&self) -> Vec<UnaEvaluationRuntimeActionTargetWrite> {
		self.effects.iter().map(|effect| effect.evaluation_target_write(&self.id)).collect()
	}
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnaEvaluationTargetKind {
	WardrobeSet,
	NodeVisibility,
	MaterialProperty,
	MaterialSlot,
	ExpressionWeight,
	DynamicsEnabled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnaEvaluationRuntimeActionTargetWrite {
	pub owner_key: String,
	pub action_id: String,
	pub effect_kind: String,
	pub target_kind: UnaEvaluationTargetKind,
	pub target_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnaEvaluationTargetWriteCollision {
	pub target_kind: UnaEvaluationTargetKind,
	pub target_key: String,
	pub owner_keys: Vec<String>,
	pub action_ids: Vec<String>,
	pub writes: Vec<UnaEvaluationRuntimeActionTargetWrite>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnaEvaluationRestoreReadiness {
	pub owner_key: String,
	pub action_id: String,
	pub effect_kind: String,
	pub target_kind: UnaEvaluationTargetKind,
	pub target_key: String,
	pub restore_target: bool,
	pub current_value_available: bool,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub current_value: Option<Value>,
	pub baseline_required: bool,
	pub ready: bool,
	pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaEvaluationRestoreBaselineCandidate {
	pub owner_key: String,
	pub action_id: String,
	pub effect_kind: String,
	pub target_kind: UnaEvaluationTargetKind,
	pub target_key: String,
	pub baseline_value: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaEvaluationRestoreBaselineEntry {
	pub owner_key: String,
	pub target_kind: UnaEvaluationTargetKind,
	pub target_key: String,
	pub baseline_value: Value,
	pub source_action_ids: Vec<String>,
	pub source_effect_kinds: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UnaRuntimeActionCondition {
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub source_component_id: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub source_node: Option<UnaRuntimeNodeTarget>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub parameter_name: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub parameter_value: Option<f32>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub sub_parameter_names: Vec<String>,
	#[serde(default, skip_serializing_if = "is_false")]
	pub inverted: bool,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub active_parent_nodes: Vec<UnaRuntimeNodeTarget>,
}

impl UnaRuntimeActionCondition {
	pub fn parameter_condition_matches(&self, name: &str, value: f32) -> Option<bool> {
		let condition_name = self.parameter_name.as_deref()?;
		let condition_value = self.parameter_value?;
		let active = condition_name == name && (value - condition_value).abs() <= UNA_RUNTIME_ACTION_PARAMETER_EPSILON;
		Some(active ^ self.inverted)
	}

	pub fn parameter_condition_matches_in_scene(&self, scene: Option<&UnaSceneSnapshot>, name: &str, value: f32) -> Option<bool> {
		let active = self.parameter_condition_matches(name, value)?;
		Some(active && self.active_parent_nodes_match(scene))
	}

	pub fn active_parent_nodes_match(&self, scene: Option<&UnaSceneSnapshot>) -> bool {
		if self.active_parent_nodes.is_empty() {
			return true;
		}
		let Some(scene) = scene else {
			return false;
		};
		self.active_parent_nodes.iter().all(|target| {
			resolve_runtime_node_target(scene, target)
				.and_then(|index| scene.nodes.get(index))
				.is_some_and(|node| node.visible)
		})
	}
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaModularAvatarVertexFilterGroup {
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub source_component_id: Option<String>,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub source_component_type: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub target: Option<UnaRuntimeNodeTarget>,
	pub combine: UnaVertexFilterCombineMode,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub filters: Vec<UnaVertexFilter>,
}

impl Default for UnaModularAvatarVertexFilterGroup {
	fn default() -> Self {
		Self {
			source_component_id: None,
			source_component_type: String::new(),
			target: None,
			combine: UnaVertexFilterCombineMode::Single,
			filters: Vec::new(),
		}
	}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaVertexFilterCombineMode {
	#[default]
	Single,
	Union,
	Intersection,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UnaVertexFilter {
	BlendShape {
		#[serde(default, skip_serializing_if = "Vec::is_empty")]
		shapes: Vec<String>,
		threshold: f32,
	},
	Bone {
		bone: UnaRuntimeNodeTarget,
		threshold: f32,
	},
	Axis {
		center: [f32; 3],
		axis: [f32; 3],
	},
	Mask {
		material_index: usize,
		#[serde(default, skip_serializing_if = "Option::is_none")]
		texture: Option<String>,
		mode: UnaVertexFilterMaskMode,
	},
	Unknown {
		#[serde(default, skip_serializing_if = "String::is_empty")]
		source_component_type: String,
	},
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaVertexFilterMaskMode {
	#[default]
	DeleteBlack,
	DeleteWhite,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UnaRuntimeState {
	/// Currently resolved wardrobe set. Source package wardrobe metadata remains in `UnaUnavatarExtension`;
	/// this is runtime state so hot-switch clients can observe the active resolver choice.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub active_wardrobe_set: Option<String>,
	/// Asset groups required by the currently resolved wardrobe set. Source declarations remain in `.unavatar`;
	/// this records the transient selection for renderer cache planning.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub active_asset_groups: Vec<String>,
	/// Last successfully activated runtime action. Action definitions remain in `UnaRuntimeActionSet`;
	/// this field only records transient runtime state for status/diagnostics.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub last_action_id: Option<String>,
	/// Runtime parameter values selected through action activation. Parameter definitions and menu metadata remain in
	/// `UnaRuntimeActionSet`; this map records only the current transient state.
	#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
	pub parameter_values: BTreeMap<String, f32>,
	/// Runtime dynamics enable overrides keyed by source id. Authored dynamics defaults remain on source groups; wardrobe
	/// and runtime actions write only this transient state.
	#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
	pub dynamics_enabled_overrides: BTreeMap<String, bool>,
}

pub const UNA_RUNTIME_RESOLVER_VERSION: u32 = 3;

pub fn modular_avatar_component_support_kind(short_type: &str) -> &'static str {
	match short_type {
		"ModularAvatarBoneProxy"
		| "ModularAvatarBlendshapeSync"
		| "ModularAvatarMergeArmature"
		| "ModularAvatarMeshCutter"
		| "ModularAvatarMeshSettings"
		| "ModularAvatarRemoveVertexColor"
		| "ModularAvatarReplaceObject"
		| "ModularAvatarShapeChanger" => "resolver",
		"ModularAvatarMaterialSetter" | "ModularAvatarMaterialSwap" | "ModularAvatarObjectToggle" => "runtime_action",
		"ModularAvatarConvertConstraints"
		| "ModularAvatarFloorAdjuster"
		| "ModularAvatarGlobalCollider"
		| "ModularAvatarMMDLayerControl"
		| "ModularAvatarMergeAnimator"
		| "ModularAvatarMergeBlendTree"
		| "ModularAvatarPBBlocker"
		| "ModularAvatarPlatformFilter"
		| "ModularAvatarRenameVRChatCollisionTags"
		| "ModularAvatarScaleAdjuster"
		| "ModularAvatarVRChatSettings"
		| "ModularAvatarWorldFixedObject"
		| "ModularAvatarWorldScaleObject"
		| "MAMoveIndependently" => "unsupported",
		"ModularAvatarMenuItem"
		| "ModularAvatarMenuGroup"
		| "ModularAvatarMenuInstaller"
		| "ModularAvatarMenuInstallTarget"
		| "ModularAvatarParameters"
		| "ModularAvatarSyncParameterSequence"
		| "ModularAvatarVisibleHeadAccessory"
		| "VertexFilterByAxisComponent"
		| "VertexFilterByBoneComponent"
		| "VertexFilterByMaskComponent"
		| "VertexFilterByShapeComponent" => "metadata",
		_ => "unsupported",
	}
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UnaRuntimeResolverCacheKey {
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub wardrobe_set: Option<String>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub active_asset_groups: Vec<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub modular_avatar_components_hash: Option<u64>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub material_source_hash: Option<u64>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub mesh_source_hash: Option<u64>,
	pub resolver_version: u32,
}

impl UnaRuntimeResolverCacheKey {
	pub fn from_document(document: &UnaDocument) -> Self {
		let state = &document.runtime_state;
		Self {
			wardrobe_set: state.active_wardrobe_set.clone(),
			active_asset_groups: state.active_asset_groups.clone(),
			modular_avatar_components_hash: document.unavatar.as_ref().and_then(unavatar_modular_avatar_components_hash),
			material_source_hash: document.scene.as_ref().and_then(scene_material_source_hash),
			mesh_source_hash: document.scene.as_ref().and_then(scene_mesh_source_hash),
			resolver_version: UNA_RUNTIME_RESOLVER_VERSION,
		}
	}
}

fn scene_mesh_source_hash(scene: &UnaSceneSnapshot) -> Option<u64> {
	if scene.meshes.is_empty() {
		return None;
	}
	let meshes = scene
		.meshes
		.iter()
		.map(|primitives| {
			Value::Array(
				primitives
					.iter()
					.map(|primitive| {
						let mut out = serde_json::Map::new();
						if let Some(name) = &primitive.name {
							out.insert("name".to_string(), Value::String(name.clone()));
						}
						out.insert("positions".to_string(), Value::from(primitive.positions.len() as u64));
						out.insert(
							"normals".to_string(),
							primitive
								.normals
								.as_ref()
								.map_or(Value::Null, |values| Value::from(values.len() as u64)),
						);
						out.insert(
							"tangents".to_string(),
							primitive
								.tangents
								.as_ref()
								.map_or(Value::Null, |values| Value::from(values.len() as u64)),
						);
						out.insert(
							"texCoords0".to_string(),
							primitive
								.tex_coords_0
								.as_ref()
								.map_or(Value::Null, |values| Value::from(values.len() as u64)),
						);
						out.insert(
							"texCoords1".to_string(),
							primitive
								.tex_coords_1
								.as_ref()
								.map_or(Value::Null, |values| Value::from(values.len() as u64)),
						);
						out.insert(
							"texCoords2".to_string(),
							primitive
								.tex_coords_2
								.as_ref()
								.map_or(Value::Null, |values| Value::from(values.len() as u64)),
						);
						out.insert(
							"texCoords3".to_string(),
							primitive
								.tex_coords_3
								.as_ref()
								.map_or(Value::Null, |values| Value::from(values.len() as u64)),
						);
						out.insert(
							"colors0".to_string(),
							primitive
								.colors_0
								.as_ref()
								.map_or(Value::Null, |values| Value::from(values.len() as u64)),
						);
						out.insert(
							"joints".to_string(),
							primitive
								.joints
								.as_ref()
								.map_or(Value::Null, |values| Value::from(values.len() as u64)),
						);
						out.insert(
							"weights".to_string(),
							primitive
								.weights
								.as_ref()
								.map_or(Value::Null, |values| Value::from(values.len() as u64)),
						);
						out.insert(
							"indices".to_string(),
							primitive
								.indices
								.as_ref()
								.map_or(Value::Null, |indices| Value::from(indices.len() as u64)),
						);
						out.insert(
							"material".to_string(),
							primitive.material_index.map_or(Value::Null, |index| Value::from(index as u64)),
						);
						out.insert("morphTargets".to_string(), Value::from(primitive.morph_targets.len() as u64));
						out.insert(
							"defaultMorphWeights".to_string(),
							Value::from(primitive.default_morph_weights.len() as u64),
						);
						out.insert(
							"morphTargetNames".to_string(),
							Value::Array(
								primitive
									.morph_target_names
									.iter()
									.map(|name| Value::String(name.clone()))
									.collect(),
							),
						);
						Value::Object(out)
					})
					.collect(),
			)
		})
		.collect();
	Some(stable_json_hash(&Value::Array(meshes)))
}

fn scene_material_source_hash(scene: &UnaSceneSnapshot) -> Option<u64> {
	if scene.materials.is_empty() {
		return None;
	}
	let materials = scene
		.materials
		.iter()
		.map(|material| {
			let mut out = serde_json::Map::new();
			if let Some(name) = &material.name {
				out.insert("name".to_string(), Value::String(name.clone()));
			}
			out.insert("shading".to_string(), serde_json::to_value(material.shading).unwrap_or(Value::Null));
			out.insert(
				"baseColorTexture".to_string(),
				material
					.base_color_texture_index
					.map_or(Value::Null, |index| Value::from(index as u64)),
			);
			out.insert(
				"normalTexture".to_string(),
				material.normal_texture_index.map_or(Value::Null, |index| Value::from(index as u64)),
			);
			out.insert(
				"emissiveTexture".to_string(),
				material
					.emissive_texture_index
					.map_or(Value::Null, |index| Value::from(index as u64)),
			);
			if let Some(unavatar_material) = &material.unavatar_material {
				out.insert("unavatarMaterial".to_string(), unavatar_material.clone());
			}
			if let Some(liltoon_like) = &material.liltoon_like {
				out.insert("liltoonLike".to_string(), serde_json::to_value(liltoon_like).unwrap_or(Value::Null));
			}
			if let Some(mtoon) = &material.mtoon {
				out.insert("mtoon".to_string(), serde_json::to_value(mtoon).unwrap_or(Value::Null));
			}
			Value::Object(out)
		})
		.collect();
	Some(stable_json_hash(&Value::Array(materials)))
}

fn unavatar_modular_avatar_components_hash(unavatar: &UnaUnavatarExtension) -> Option<u64> {
	let components = unavatar.source.get("modularAvatar")?.get("components")?;
	Some(stable_json_hash(components))
}

fn stable_json_hash(value: &Value) -> u64 {
	const FNV64_OFFSET: u64 = 0xcbf29ce484222325;
	const FNV64_PRIME: u64 = 0x100000001b3;

	fn update(mut hash: u64, bytes: &[u8]) -> u64 {
		for byte in bytes {
			hash ^= u64::from(*byte);
			hash = hash.wrapping_mul(FNV64_PRIME);
		}
		hash
	}

	fn visit(hash: u64, value: &Value) -> u64 {
		match value {
			Value::Null => update(hash, b"n"),
			Value::Bool(value) => update(update(hash, b"b"), if *value { b"1" } else { b"0" }),
			Value::Number(value) => update(update(hash, b"#"), value.to_string().as_bytes()),
			Value::String(value) => update(update(hash, b"s"), value.as_bytes()),
			Value::Array(values) => {
				let mut hash = update(hash, b"[");
				for value in values {
					hash = visit(hash, value);
					hash = update(hash, b",");
				}
				update(hash, b"]")
			}
			Value::Object(values) => {
				let mut hash = update(hash, b"{");
				let mut keys: Vec<_> = values.keys().collect();
				keys.sort();
				for key in keys {
					hash = update(update(hash, b"k"), key.as_bytes());
					hash = visit(hash, &values[key]);
					hash = update(hash, b",");
				}
				update(hash, b"}")
			}
		}
	}

	visit(FNV64_OFFSET, value)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UnaRuntimeActionTrigger {
	ExpressionMenu {
		#[serde(default, skip_serializing_if = "String::is_empty")]
		path: String,
	},
	KeyboardShortcut {
		#[serde(default, skip_serializing_if = "String::is_empty")]
		key: String,
	},
	SupervisorCommand {
		#[serde(default, skip_serializing_if = "String::is_empty")]
		command: String,
	},
	AnimationEvent {
		#[serde(default, skip_serializing_if = "String::is_empty")]
		name: String,
	},
	ParameterValue {
		#[serde(default, skip_serializing_if = "String::is_empty")]
		name: String,
		value: f32,
	},
}

impl UnaRuntimeActionTrigger {
	pub fn matches_query(&self, query: UnaRuntimeActionQuery<'_>) -> bool {
		match self {
			UnaRuntimeActionTrigger::ExpressionMenu { path } => query.expression_menu_path.is_some_and(|query_path| path == query_path),
			UnaRuntimeActionTrigger::KeyboardShortcut { .. } => false,
			UnaRuntimeActionTrigger::SupervisorCommand { command } => {
				query.supervisor_command.is_some_and(|query_command| command == query_command)
			}
			UnaRuntimeActionTrigger::AnimationEvent { .. } => false,
			UnaRuntimeActionTrigger::ParameterValue { name, value } => query.parameter_name.is_some_and(|query_name| {
				name == query_name
					&& query
						.parameter_value
						.is_some_and(|query_value| (query_value - *value).abs() <= UNA_RUNTIME_ACTION_PARAMETER_EPSILON)
			}),
		}
	}
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UnaRuntimeActionEffect {
	WardrobeSet {
		set_id: String,
	},
	NodeVisibility {
		target: UnaRuntimeNodeTarget,
		visible: bool,
	},
	ExpressionWeight {
		name: String,
		weight: f32,
	},
	MaterialColor {
		target: UnaRuntimeMaterialTarget,
		#[serde(default, skip_serializing_if = "String::is_empty")]
		parameter: String,
		color: [f32; 4],
	},
	MaterialScalar {
		target: UnaRuntimeMaterialTarget,
		#[serde(default, skip_serializing_if = "String::is_empty")]
		parameter: String,
		value: f32,
	},
	MaterialSlot {
		target: UnaRuntimeMaterialSlotTarget,
		material: Option<UnaRuntimeMaterialTarget>,
	},
	DynamicsEnabled {
		source_id: String,
		enabled: bool,
	},
}

impl UnaRuntimeActionEffect {
	pub fn evaluation_target_write(&self, action_id: &str) -> UnaEvaluationRuntimeActionTargetWrite {
		let owner_key = runtime_action_owner_key(action_id);
		let action_id = action_id.to_string();
		match self {
			UnaRuntimeActionEffect::WardrobeSet { set_id } => UnaEvaluationRuntimeActionTargetWrite {
				owner_key,
				action_id,
				effect_kind: "wardrobe_set".to_string(),
				target_kind: UnaEvaluationTargetKind::WardrobeSet,
				target_key: set_id.clone(),
			},
			UnaRuntimeActionEffect::NodeVisibility { target, .. } => UnaEvaluationRuntimeActionTargetWrite {
				owner_key,
				action_id,
				effect_kind: "node_visibility".to_string(),
				target_kind: UnaEvaluationTargetKind::NodeVisibility,
				target_key: runtime_node_target_key(target),
			},
			UnaRuntimeActionEffect::ExpressionWeight { name, .. } => UnaEvaluationRuntimeActionTargetWrite {
				owner_key,
				action_id,
				effect_kind: "expression_weight".to_string(),
				target_kind: UnaEvaluationTargetKind::ExpressionWeight,
				target_key: name.clone(),
			},
			UnaRuntimeActionEffect::MaterialColor { target, parameter, .. } => UnaEvaluationRuntimeActionTargetWrite {
				owner_key,
				action_id,
				effect_kind: "material_color".to_string(),
				target_kind: UnaEvaluationTargetKind::MaterialProperty,
				target_key: runtime_material_property_target_key(target, parameter),
			},
			UnaRuntimeActionEffect::MaterialScalar { target, parameter, .. } => UnaEvaluationRuntimeActionTargetWrite {
				owner_key,
				action_id,
				effect_kind: "material_scalar".to_string(),
				target_kind: UnaEvaluationTargetKind::MaterialProperty,
				target_key: runtime_material_property_target_key(target, parameter),
			},
			UnaRuntimeActionEffect::MaterialSlot { target, .. } => UnaEvaluationRuntimeActionTargetWrite {
				owner_key,
				action_id,
				effect_kind: "material_slot".to_string(),
				target_kind: UnaEvaluationTargetKind::MaterialSlot,
				target_key: runtime_material_slot_target_key(target),
			},
			UnaRuntimeActionEffect::DynamicsEnabled { source_id, .. } => UnaEvaluationRuntimeActionTargetWrite {
				owner_key,
				action_id,
				effect_kind: "dynamics_enabled".to_string(),
				target_kind: UnaEvaluationTargetKind::DynamicsEnabled,
				target_key: source_id.clone(),
			},
		}
	}
}

pub fn runtime_action_owner_key(action_id: &str) -> String {
	if action_id.is_empty() {
		"action:?".to_string()
	} else {
		format!("action:{action_id}")
	}
}

pub fn restore_baseline_capture_plan_from_candidates(
	candidates: Vec<UnaEvaluationRestoreBaselineCandidate>,
) -> Vec<UnaEvaluationRestoreBaselineEntry> {
	let mut entries: BTreeMap<(String, UnaEvaluationTargetKind, String, String), UnaEvaluationRestoreBaselineEntry> = BTreeMap::new();
	for candidate in candidates {
		let value_key = stable_json_key(&candidate.baseline_value);
		let key = (
			candidate.owner_key.clone(),
			candidate.target_kind.clone(),
			candidate.target_key.clone(),
			value_key,
		);
		let entry = entries.entry(key).or_insert_with(|| UnaEvaluationRestoreBaselineEntry {
			owner_key: candidate.owner_key.clone(),
			target_kind: candidate.target_kind.clone(),
			target_key: candidate.target_key.clone(),
			baseline_value: candidate.baseline_value.clone(),
			source_action_ids: Vec::new(),
			source_effect_kinds: Vec::new(),
		});
		if !entry.source_action_ids.contains(&candidate.action_id) {
			entry.source_action_ids.push(candidate.action_id);
		}
		if !entry.source_effect_kinds.contains(&candidate.effect_kind) {
			entry.source_effect_kinds.push(candidate.effect_kind);
		}
	}
	for entry in entries.values_mut() {
		entry.source_action_ids.sort();
		entry.source_effect_kinds.sort();
	}
	entries.into_values().collect()
}

fn stable_json_key(value: &Value) -> String {
	serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

fn runtime_node_target_key(target: &UnaRuntimeNodeTarget) -> String {
	target
		.resolved_node_id
		.as_deref()
		.or(target.source_node_id.as_deref())
		.or(target.path.as_deref())
		.map(str::to_string)
		.or_else(|| target.node_index.map(|index| format!("#{index}")))
		.unwrap_or_else(|| "?".to_string())
}

fn runtime_material_target_key(target: &UnaRuntimeMaterialTarget) -> String {
	target
		.name
		.as_deref()
		.map(str::to_string)
		.or_else(|| target.material_index.map(|index| format!("#{index}")))
		.unwrap_or_else(|| "?".to_string())
}

fn runtime_material_property_target_key(target: &UnaRuntimeMaterialTarget, parameter: &str) -> String {
	format!("{}:{parameter}", runtime_material_target_key(target))
}

fn runtime_material_slot_target_key(target: &UnaRuntimeMaterialSlotTarget) -> String {
	let primitive = target.primitive_index.map_or_else(|| "*".to_string(), |index| index.to_string());
	format!("{}[{primitive}]", runtime_node_target_key(&target.node))
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UnaRuntimeNodeTarget {
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub node_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub source_node_id: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub resolved_node_id: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub path: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UnaRuntimeMaterialTarget {
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub material_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub name: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UnaRuntimeMaterialSlotTarget {
	pub node: UnaRuntimeNodeTarget,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub primitive_index: Option<usize>,
}

/// VRM Secondary Animation / SpringBone の 1 チェーン（bootstrap）。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UnaSpringBoneGroup {
	#[serde(default, skip_serializing_if = "UnaDynamicsSourceKind::is_default")]
	pub source_kind: UnaDynamicsSourceKind,
	#[serde(default = "default_true")]
	pub enabled: bool,
	/// Source-side stable identifier, used by wardrobe / action state once dynamics toggles are resolved.
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub source_id: String,
	#[serde(default)]
	pub comment: String,
	/// UN Avatar 側で推定・編集する SpringBone 物理カテゴリ。
	///
	/// 永続 schema では固定 enum にしない。GUI の表示名変更やユーザー定義カテゴリ追加で
	/// 既存ファイルを壊さないため、文字列 ID を正本にする。
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub category: String,
	/// VRM 0 の `stiffiness` / 1 の関節剛性に相当（大きいほど理想姿勢に引く）。
	#[serde(default = "one_f32")]
	pub stiffness: f32,
	#[serde(default)]
	pub gravity_power: f32,
	#[serde(default = "default_spring_gravity_dir")]
	pub gravity_dir: [f32; 3],
	#[serde(default = "default_spring_drag")]
	pub drag_force: f32,
	/// VRM 0 `center` が非負のときのノードインデックス。
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub center_node: Option<usize>,
	#[serde(default)]
	pub hit_radius: f32,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub limit: Option<UnaDynamicsLimit>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub interaction: Option<UnaDynamicsInteraction>,
	/// glTF ノードインデックスのチェーン（親→子）。
	pub bone_node_indices: Vec<usize>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UnaDynamicsParameters {
	pub stiffness: f32,
	pub gravity_power: f32,
	pub gravity_dir: [f32; 3],
	pub drag_force: f32,
	pub center_node: Option<usize>,
	pub hit_radius: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UnaDynamicsChain<'a> {
	pub bone_node_indices: &'a [usize],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnaDynamicsGroup<'a> {
	pub source_kind: UnaDynamicsSourceKind,
	pub authored_enabled: bool,
	pub effective_enabled: bool,
	pub source_id: &'a str,
	pub comment: &'a str,
	pub category: &'a str,
	pub parameters: UnaDynamicsParameters,
	pub chain: UnaDynamicsChain<'a>,
	pub limit: Option<&'a UnaDynamicsLimit>,
	pub interaction: Option<&'a UnaDynamicsInteraction>,
}

impl<'a> UnaDynamicsGroup<'a> {
	fn from_spring_bone_group(group: &'a UnaSpringBoneGroup, effective_enabled: bool) -> Self {
		Self {
			source_kind: group.source_kind,
			authored_enabled: group.enabled,
			effective_enabled,
			source_id: &group.source_id,
			comment: &group.comment,
			category: &group.category,
			parameters: UnaDynamicsParameters {
				stiffness: group.stiffness,
				gravity_power: group.gravity_power,
				gravity_dir: group.gravity_dir,
				drag_force: group.drag_force,
				center_node: group.center_node,
				hit_radius: group.hit_radius,
			},
			chain: UnaDynamicsChain {
				bone_node_indices: &group.bone_node_indices,
			},
			limit: group.limit.as_ref(),
			interaction: group.interaction.as_ref(),
		}
	}
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UnaDynamicsLimit {
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub limit_type: String,
	#[serde(default)]
	pub max_angle_x: f32,
	#[serde(default)]
	pub max_angle_z: f32,
	#[serde(default)]
	pub max_stretch: f32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UnaDynamicsInteraction {
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub allow_grabbing: Option<bool>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub allow_posing: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnaDynamicsContactKind {
	Sender,
	Receiver,
	Unknown,
}

impl Default for UnaDynamicsContactKind {
	fn default() -> Self {
		Self::Unknown
	}
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnaDynamicsColliderShape {
	Sphere,
	Capsule,
	Unknown,
}

impl Default for UnaDynamicsColliderShape {
	fn default() -> Self {
		Self::Unknown
	}
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UnaDynamicsCollider {
	#[serde(default, skip_serializing_if = "UnaDynamicsSourceKind::is_default")]
	pub source_kind: UnaDynamicsSourceKind,
	pub node: usize,
	#[serde(default)]
	pub shape: UnaDynamicsColliderShape,
	#[serde(default)]
	pub radius: f32,
	#[serde(default)]
	pub height: f32,
	#[serde(default)]
	pub position: [f32; 3],
	#[serde(default = "identity_quat_array")]
	pub rotation: [f32; 4],
	#[serde(default)]
	pub inside_bounds: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UnaDynamicsContact {
	#[serde(default, skip_serializing_if = "UnaDynamicsSourceKind::is_default")]
	pub source_kind: UnaDynamicsSourceKind,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub source_id: String,
	pub node: usize,
	#[serde(default)]
	pub kind: UnaDynamicsContactKind,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub parameter: String,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub collision_tags: Vec<String>,
	#[serde(default)]
	pub shape: UnaDynamicsColliderShape,
	#[serde(default)]
	pub radius: f32,
	#[serde(default)]
	pub height: f32,
	#[serde(default)]
	pub position: [f32; 3],
	#[serde(default = "identity_quat_array")]
	pub rotation: [f32; 4],
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnaEvaluationContactParameterDeclaration {
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub owner_key: String,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub source_id: String,
	pub node: usize,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub parameter: String,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub collision_tags: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UnaEvaluationContactProbe {
	pub receiver_index: usize,
	pub sender_index: usize,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub receiver_source_id: String,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub sender_source_id: String,
	pub receiver_node: usize,
	pub sender_node: usize,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub parameter: String,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub matched_tags: Vec<String>,
	pub tag_match: bool,
	pub overlap: bool,
	pub would_emit: bool,
	pub distance: f32,
	pub threshold: f32,
	pub receiver_radius: f32,
	pub sender_radius: f32,
	pub receiver_shape: UnaDynamicsColliderShape,
	pub sender_shape: UnaDynamicsColliderShape,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub approximation: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UnaDynamicsConstraintRef {
	#[serde(default, skip_serializing_if = "UnaDynamicsSourceKind::is_default")]
	pub source_kind: UnaDynamicsSourceKind,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub source_id: String,
	pub target_node: usize,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub source_nodes: Vec<usize>,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub constraint_type: String,
	#[serde(default = "one_f32")]
	pub weight: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnaDynamicsSourceKind {
	#[default]
	VrmSpringBone,
	VrcPhysBone,
	Unknown,
}

impl UnaDynamicsSourceKind {
	pub fn is_default(self: &Self) -> bool {
		*self == Self::default()
	}
}

fn default_spring_gravity_dir() -> [f32; 3] {
	[0.0, -1.0, 0.0]
}

fn default_spring_drag() -> f32 {
	0.4
}

fn identity_quat_array() -> [f32; 4] {
	[0.0, 0.0, 0.0, 1.0]
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UnaSpringBoneSettings {
	#[serde(default)]
	pub groups: Vec<UnaSpringBoneGroup>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub colliders: Vec<UnaDynamicsCollider>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub contacts: Vec<UnaDynamicsContact>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub constraint_refs: Vec<UnaDynamicsConstraintRef>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct UnaRuntimeDynamics<'a> {
	spring_bones: Option<&'a UnaSpringBoneSettings>,
	runtime_state: Option<&'a UnaRuntimeState>,
}

#[derive(Debug, Default)]
pub struct UnaRuntimeDynamicsMut<'a> {
	spring_bones: Option<&'a mut UnaSpringBoneSettings>,
	runtime_state: Option<&'a mut UnaRuntimeState>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UnaRuntimeDynamicsCounts {
	pub groups: usize,
	pub enabled_groups: usize,
	pub source_enabled_groups: usize,
	pub runtime_enabled_overrides: usize,
	pub vrm_spring_bone_groups: usize,
	pub vrc_physbone_groups: usize,
	pub unknown_groups: usize,
	pub limit_groups: usize,
	pub angle_limit_groups: usize,
	pub stretch_limit_groups: usize,
	pub grabbing_enabled_groups: usize,
	pub posing_enabled_groups: usize,
	pub colliders: usize,
	pub vrm_spring_bone_colliders: usize,
	pub vrc_physbone_colliders: usize,
	pub unknown_colliders: usize,
	pub contacts: usize,
	pub vrc_contact_senders: usize,
	pub vrc_contact_receivers: usize,
	pub contact_parameter_declarations: usize,
	pub constraint_refs: usize,
	pub vrc_constraint_refs: usize,
}

impl<'a> UnaRuntimeDynamics<'a> {
	pub fn groups(self) -> &'a [UnaSpringBoneGroup] {
		self.spring_bones.map(|settings| settings.groups.as_slice()).unwrap_or(&[])
	}

	pub fn group(self, index: usize) -> Option<&'a UnaSpringBoneGroup> {
		self.groups().get(index)
	}

	pub fn dynamics_group(self, index: usize) -> Option<UnaDynamicsGroup<'a>> {
		self.group(index)
			.map(|group| UnaDynamicsGroup::from_spring_bone_group(group, self.group_enabled(group)))
	}

	pub fn dynamics_groups(self) -> impl Iterator<Item = UnaDynamicsGroup<'a>> + 'a {
		self.groups()
			.iter()
			.map(move |group| UnaDynamicsGroup::from_spring_bone_group(group, self.group_enabled(group)))
	}

	pub fn has_groups(self) -> bool {
		!self.groups().is_empty()
	}

	pub fn group_count(self) -> usize {
		self.groups().len()
	}

	pub fn enabled_group_count(self) -> usize {
		self.groups().iter().filter(|group| self.group_enabled(group)).count()
	}

	pub fn group_enabled(self, group: &UnaSpringBoneGroup) -> bool {
		if !group.source_id.is_empty() {
			if let Some(enabled) = self
				.runtime_state
				.and_then(|state| state.dynamics_enabled_overrides.get(&group.source_id))
				.copied()
			{
				return enabled;
			}
		}
		group.enabled
	}

	pub fn source_group_count(self, source_kind: UnaDynamicsSourceKind) -> usize {
		self.groups().iter().filter(|group| group.source_kind == source_kind).count()
	}

	pub fn collider_count(self) -> usize {
		self.colliders().count()
	}

	pub fn dynamic_bone_node_indices(self) -> impl Iterator<Item = usize> + 'a {
		self.groups().iter().flat_map(|group| group.bone_node_indices.iter().copied())
	}

	pub fn reset_node_indices(self) -> Vec<usize> {
		let mut nodes = BTreeSet::new();
		for node in self.dynamic_bone_node_indices() {
			nodes.insert(node);
		}
		for constraint_ref in self.constraint_refs() {
			nodes.insert(constraint_ref.target_node);
			nodes.extend(constraint_ref.source_nodes.iter().copied());
		}
		nodes.into_iter().collect()
	}

	pub fn colliders(self) -> impl Iterator<Item = &'a UnaDynamicsCollider> {
		self.spring_bones.into_iter().flat_map(|settings| settings.colliders.iter())
	}

	pub fn contacts(self) -> impl Iterator<Item = &'a UnaDynamicsContact> {
		self.spring_bones.into_iter().flat_map(|settings| settings.contacts.iter())
	}

	pub fn contact_parameter_declarations(self) -> Vec<UnaEvaluationContactParameterDeclaration> {
		self.contacts()
			.enumerate()
			.filter_map(|(index, contact)| {
				if contact.kind != UnaDynamicsContactKind::Receiver || contact.parameter.is_empty() {
					return None;
				}
				Some(UnaEvaluationContactParameterDeclaration {
					owner_key: contact_owner_key(&contact.source_id, index),
					source_id: contact.source_id.clone(),
					node: contact.node,
					parameter: contact.parameter.clone(),
					collision_tags: contact.collision_tags.clone(),
				})
			})
			.collect()
	}

	pub fn constraint_refs(self) -> impl Iterator<Item = &'a UnaDynamicsConstraintRef> {
		self.spring_bones.into_iter().flat_map(|settings| settings.constraint_refs.iter())
	}

	pub fn source_collider_count(self, source_kind: UnaDynamicsSourceKind) -> usize {
		self.colliders().filter(|collider| collider.source_kind == source_kind).count()
	}

	pub fn counts(self) -> UnaRuntimeDynamicsCounts {
		let mut counts = UnaRuntimeDynamicsCounts::default();
		counts.runtime_enabled_overrides = self
			.runtime_state
			.map(|state| state.dynamics_enabled_overrides.len())
			.unwrap_or_default();
		for group in self.groups() {
			counts.groups += 1;
			if group.enabled {
				counts.source_enabled_groups += 1;
			}
			if self.group_enabled(group) {
				counts.enabled_groups += 1;
			}
			if let Some(limit) = group.limit.as_ref() {
				counts.limit_groups += 1;
				if !limit.limit_type.is_empty() || limit.max_angle_x.abs() > 0.0 || limit.max_angle_z.abs() > 0.0 {
					counts.angle_limit_groups += 1;
				}
				if limit.max_stretch.abs() > 0.0 {
					counts.stretch_limit_groups += 1;
				}
			}
			if group.interaction.as_ref().and_then(|interaction| interaction.allow_grabbing) == Some(true) {
				counts.grabbing_enabled_groups += 1;
			}
			if group.interaction.as_ref().and_then(|interaction| interaction.allow_posing) == Some(true) {
				counts.posing_enabled_groups += 1;
			}
			match group.source_kind {
				UnaDynamicsSourceKind::VrmSpringBone => counts.vrm_spring_bone_groups += 1,
				UnaDynamicsSourceKind::VrcPhysBone => counts.vrc_physbone_groups += 1,
				UnaDynamicsSourceKind::Unknown => counts.unknown_groups += 1,
			}
		}
		for collider in self.colliders() {
			counts.colliders += 1;
			match collider.source_kind {
				UnaDynamicsSourceKind::VrmSpringBone => counts.vrm_spring_bone_colliders += 1,
				UnaDynamicsSourceKind::VrcPhysBone => counts.vrc_physbone_colliders += 1,
				UnaDynamicsSourceKind::Unknown => counts.unknown_colliders += 1,
			}
		}
		for contact in self.contacts() {
			counts.contacts += 1;
			if contact.kind == UnaDynamicsContactKind::Receiver && !contact.parameter.is_empty() {
				counts.contact_parameter_declarations += 1;
			}
			if contact.source_kind == UnaDynamicsSourceKind::VrcPhysBone {
				match contact.kind {
					UnaDynamicsContactKind::Sender => counts.vrc_contact_senders += 1,
					UnaDynamicsContactKind::Receiver => counts.vrc_contact_receivers += 1,
					UnaDynamicsContactKind::Unknown => {}
				}
			}
		}
		for constraint_ref in self.constraint_refs() {
			counts.constraint_refs += 1;
			if constraint_ref.source_kind == UnaDynamicsSourceKind::VrcPhysBone {
				counts.vrc_constraint_refs += 1;
			}
		}
		counts
	}
}

fn contact_owner_key(source_id: &str, fallback_index: usize) -> String {
	if source_id.is_empty() {
		format!("contact:receiver:{fallback_index}")
	} else if source_id.starts_with("contact:") {
		source_id.to_string()
	} else {
		format!("contact:{source_id}")
	}
}

#[derive(Clone, Copy, Debug, Default)]
struct ContactProbeSphere {
	center: [f32; 3],
	radius: f32,
}

fn contact_probe_sphere(contact: &UnaDynamicsContact, world: &[[f32; 16]]) -> Option<ContactProbeSphere> {
	let matrix = world.get(contact.node).copied()?;
	let radius = contact_probe_local_radius(contact) * mat4_max_scale(matrix);
	Some(ContactProbeSphere {
		center: mat4_transform_point3(matrix, contact.position),
		radius,
	})
}

fn contact_probe_local_radius(contact: &UnaDynamicsContact) -> f32 {
	let radius = contact.radius.max(0.0);
	match contact.shape {
		UnaDynamicsColliderShape::Capsule => radius + contact.height.max(0.0) * 0.5,
		UnaDynamicsColliderShape::Sphere | UnaDynamicsColliderShape::Unknown => radius,
	}
}

fn contact_probe_approximation(receiver: &UnaDynamicsContact, sender: &UnaDynamicsContact) -> String {
	if receiver.shape == UnaDynamicsColliderShape::Sphere && sender.shape == UnaDynamicsColliderShape::Sphere {
		"sphere".to_string()
	} else {
		"bounding_sphere".to_string()
	}
}

fn contact_matched_tags(receiver_tags: &[String], sender_tags: &[String]) -> Vec<String> {
	let sender = sender_tags.iter().collect::<BTreeSet<_>>();
	receiver_tags
		.iter()
		.filter(|tag| sender.contains(tag))
		.cloned()
		.collect::<BTreeSet<_>>()
		.into_iter()
		.collect()
}

fn scene_world_matrices(scene: &UnaSceneSnapshot) -> Vec<[f32; 16]> {
	fn visit(scene: &UnaSceneSnapshot, idx: usize, parent: [f32; 16], world: &mut [[f32; 16]], seen: &mut [bool]) {
		let Some(node) = scene.nodes.get(idx) else { return };
		if idx >= world.len() {
			return;
		}
		let current = mat4_mul(parent, node.transform);
		world[idx] = current;
		seen[idx] = true;
		for &child in &node.children {
			visit(scene, child, current, world, seen);
		}
	}

	let mut world = vec![identity_mat4(); scene.nodes.len().max(1)];
	let mut seen = vec![false; scene.nodes.len().max(1)];
	for &root in scene.resolved_roots().iter() {
		visit(scene, root, identity_mat4(), &mut world, &mut seen);
	}
	for index in 0..scene.nodes.len() {
		if !seen[index] {
			visit(scene, index, identity_mat4(), &mut world, &mut seen);
		}
	}
	world
}

fn identity_mat4() -> [f32; 16] {
	[
		1.0, 0.0, 0.0, 0.0, //
		0.0, 1.0, 0.0, 0.0, //
		0.0, 0.0, 1.0, 0.0, //
		0.0, 0.0, 0.0, 1.0,
	]
}

fn mat4_mul(a: [f32; 16], b: [f32; 16]) -> [f32; 16] {
	let mut out = [0.0; 16];
	for col in 0..4 {
		for row in 0..4 {
			out[col * 4 + row] =
				a[row] * b[col * 4] + a[4 + row] * b[col * 4 + 1] + a[8 + row] * b[col * 4 + 2] + a[12 + row] * b[col * 4 + 3];
		}
	}
	out
}

fn mat4_transform_point3(m: [f32; 16], p: [f32; 3]) -> [f32; 3] {
	[
		m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
		m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
		m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
	]
}

fn mat4_max_scale(m: [f32; 16]) -> f32 {
	let sx = vec3_len([m[0], m[1], m[2]]);
	let sy = vec3_len([m[4], m[5], m[6]]);
	let sz = vec3_len([m[8], m[9], m[10]]);
	sx.max(sy).max(sz).max(0.0)
}

fn vec3_distance(a: [f32; 3], b: [f32; 3]) -> f32 {
	vec3_len([a[0] - b[0], a[1] - b[1], a[2] - b[2]])
}

fn vec3_len(v: [f32; 3]) -> f32 {
	(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

impl<'a> UnaRuntimeDynamicsMut<'a> {
	pub fn as_readonly(&self) -> UnaRuntimeDynamics<'_> {
		UnaRuntimeDynamics {
			spring_bones: self.spring_bones.as_deref(),
			runtime_state: self.runtime_state.as_deref(),
		}
	}

	pub fn groups_mut(&mut self) -> &mut [UnaSpringBoneGroup] {
		self.spring_bones
			.as_deref_mut()
			.map(|settings| settings.groups.as_mut_slice())
			.unwrap_or(&mut [])
	}

	pub fn reset_enabled(&mut self) {
		if let Some(runtime_state) = self.runtime_state.as_deref_mut() {
			runtime_state.dynamics_enabled_overrides.clear();
		}
	}

	pub fn set_group_enabled_by_source_id(&mut self, source_id: &str, enabled: bool) -> bool {
		if source_id.is_empty() {
			return false;
		}
		let matches = self.groups_mut().iter().any(|group| group.source_id == source_id);
		if matches {
			if let Some(runtime_state) = self.runtime_state.as_deref_mut() {
				runtime_state.dynamics_enabled_overrides.insert(source_id.to_string(), enabled);
			}
		}
		matches
	}
}

impl UnaSpringBoneSettings {
	pub fn runtime_dynamics(&self) -> UnaRuntimeDynamics<'_> {
		UnaRuntimeDynamics {
			spring_bones: Some(self),
			runtime_state: None,
		}
	}
}

/// glTF メッシュ default + 表情プリセットから、当該 primitive のモーフウェイトを合成する。
pub fn morph_weights_for_primitive(
	mesh_bufs: &UnaMeshBuffers,
	catalog: Option<&UnaExpressionCatalog>,
	w_expr: Option<&UnaExpressionWeights>,
	mesh_index: usize,
	primitive_index: usize,
) -> Vec<f32> {
	let n = mesh_bufs.morph_targets.len();
	if n == 0 {
		return Vec::new();
	}
	let mut w = Vec::with_capacity(n);
	w.extend(mesh_bufs.default_morph_weights.iter().take(n).copied());
	w.resize(n, 0.0);
	let Some(cat) = catalog else { return w };
	let Some(ew) = w_expr else { return w };
	for preset in &cat.presets {
		let pw = ew.preset_weights.get(&preset.name).copied().unwrap_or(0.0);
		if pw == 0.0 {
			continue;
		}
		for b in &preset.binds {
			if b.mesh_index != mesh_index || b.primitive_index != primitive_index {
				continue;
			}
			let Some(slot) = w.get_mut(b.morph_target_index) else {
				continue;
			};
			*slot = (*slot + pw * b.weight_scale).clamp(0.0, 1.0);
		}
	}
	w
}

/// UNA ドキュメント。`scene` はインポート済みシーンのスナップショット（スキーマは段階的に拡張）。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UnaDocument {
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub scene: Option<UnaSceneSnapshot>,
	/// `.unavatar` / glTF `extensions.UN_avatar` の正本。v2 では wardrobe / VRC 由来 metadata の入口になる。
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub unavatar: Option<UnaUnavatarExtension>,
	/// VRM 0.x / VRM 1.0 拡張ブロック（インポート時のみ）。
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub vrm: Option<UnaVrmExtension>,
	/// Humanoid ボーン → glTF ノードインデックス（リターゲット用・VRM 等が設定）。
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub humanoid_profile: Option<HumanoidProfile>,
	/// VRM 式→モーフバインド（表示・VMC 先の参照用）。
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub expression_catalog: Option<UnaExpressionCatalog>,
	/// 式プリセットの現在ウェイト。
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub expression_weights: Option<UnaExpressionWeights>,
	/// VRC Expression Menu / shortcut / supervisor / animation 由来の軽量 runtime action model。
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub runtime_actions: Option<UnaRuntimeActionSet>,
	/// Hot switch / action evaluation after source import. This is not persisted source data.
	#[serde(default, skip_serializing_if = "UnaRuntimeState::is_default")]
	pub runtime_state: UnaRuntimeState,
	/// VRM SpringBone / secondaryAnimation から取り込んだ揺れもの用チェーン（ランタイムで更新）。
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub spring_bones: Option<UnaSpringBoneSettings>,
}

impl UnaRuntimeState {
	pub fn is_default(self: &Self) -> bool {
		self == &Self::default()
	}
}

impl UnaDocument {
	pub fn runtime_model(&self) -> UnaRuntimeModel<'_> {
		UnaRuntimeModel { document: self }
	}

	pub fn runtime_model_mut(&mut self) -> UnaRuntimeModelMut<'_> {
		UnaRuntimeModelMut { document: self }
	}

	pub fn runtime_scene_and_dynamics_mut(&mut self) -> Option<UnaRuntimeSceneDynamicsMut<'_>> {
		self.runtime_model_mut().scene_and_dynamics_mut()
	}

	pub fn scoped_asset_selection(&self) -> UnaSceneScopedAssetSelection {
		let active_asset_groups = self.runtime_model().active_asset_groups();
		if active_asset_groups.is_empty() {
			return UnaSceneScopedAssetSelection::default();
		}
		let Some(scene) = self.scene.as_ref() else {
			return UnaSceneScopedAssetSelection {
				missing_active_asset_groups: active_asset_groups.to_vec(),
				..Default::default()
			};
		};
		scene.scoped_asset_selection(active_asset_groups)
	}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnaRuntimeSourceKind {
	#[default]
	GltfLike,
	Vrm0,
	Vrm1,
	Unavatar,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnaHumanoidRuntimeBasis {
	#[default]
	Vrm0,
	Vrm1,
	UnavatarUnity,
	Native,
}

/// Runtime-facing normalized model view. This is intentionally a borrowed adapter
/// while the runtime model boundary is being introduced; format-specific import data
/// remains in [`UnaDocument`], but renderer/skeleton code can start depending on this
/// view instead of branching directly on source extensions.
#[derive(Clone, Copy, Debug)]
pub struct UnaRuntimeModel<'a> {
	document: &'a UnaDocument,
}

#[derive(Debug)]
pub struct UnaRuntimeModelMut<'a> {
	document: &'a mut UnaDocument,
}

#[derive(Clone, Copy, Debug)]
pub struct UnaRuntimeRetargetInputs<'a> {
	pub humanoid_basis: UnaHumanoidRuntimeBasis,
	pub profile: Option<&'a HumanoidProfile>,
	pub scene: Option<&'a UnaSceneSnapshot>,
	pub expression_catalog: Option<&'a UnaExpressionCatalog>,
}

#[derive(Clone, Copy, Debug)]
pub struct UnaRuntimeSceneDynamics<'a> {
	pub scene: &'a UnaSceneSnapshot,
	pub humanoid_profile: Option<&'a HumanoidProfile>,
	pub dynamics: UnaRuntimeDynamics<'a>,
}

impl<'a> UnaRuntimeSceneDynamics<'a> {
	pub fn contact_probes(self) -> Vec<UnaEvaluationContactProbe> {
		let world = scene_world_matrices(self.scene);
		let contacts = self.dynamics.contacts().collect::<Vec<_>>();
		let mut probes = Vec::new();
		for (receiver_index, receiver) in contacts.iter().enumerate() {
			if receiver.kind != UnaDynamicsContactKind::Receiver || receiver.parameter.is_empty() {
				continue;
			}
			let Some(receiver_sphere) = contact_probe_sphere(receiver, &world) else {
				continue;
			};
			for (sender_index, sender) in contacts.iter().enumerate() {
				if sender.kind != UnaDynamicsContactKind::Sender {
					continue;
				}
				let Some(sender_sphere) = contact_probe_sphere(sender, &world) else {
					continue;
				};
				let matched_tags = contact_matched_tags(&receiver.collision_tags, &sender.collision_tags);
				let tag_match = !matched_tags.is_empty();
				let distance = vec3_distance(receiver_sphere.center, sender_sphere.center);
				let threshold = receiver_sphere.radius + sender_sphere.radius;
				let overlap = distance <= threshold;
				probes.push(UnaEvaluationContactProbe {
					receiver_index,
					sender_index,
					receiver_source_id: receiver.source_id.clone(),
					sender_source_id: sender.source_id.clone(),
					receiver_node: receiver.node,
					sender_node: sender.node,
					parameter: receiver.parameter.clone(),
					matched_tags,
					tag_match,
					overlap,
					would_emit: tag_match && overlap,
					distance,
					threshold,
					receiver_radius: receiver_sphere.radius,
					sender_radius: sender_sphere.radius,
					receiver_shape: receiver.shape.clone(),
					sender_shape: sender.shape.clone(),
					approximation: contact_probe_approximation(receiver, sender),
				});
			}
		}
		probes
	}
}

#[derive(Clone, Copy, Debug)]
pub struct UnaRuntimeSceneExpressions<'a> {
	pub scene: &'a UnaSceneSnapshot,
	pub expression_catalog: Option<&'a UnaExpressionCatalog>,
}

#[derive(Debug)]
pub struct UnaRuntimeSceneDynamicsMut<'a> {
	pub scene: &'a mut UnaSceneSnapshot,
	pub dynamics: UnaRuntimeDynamicsMut<'a>,
}

impl<'a> UnaRuntimeModel<'a> {
	pub fn source_kind(self) -> UnaRuntimeSourceKind {
		if self.document.unavatar.is_some() {
			return UnaRuntimeSourceKind::Unavatar;
		}
		if let Some(vrm) = self.document.vrm.as_ref() {
			return if vrm.is_vrm0() {
				UnaRuntimeSourceKind::Vrm0
			} else {
				UnaRuntimeSourceKind::Vrm1
			};
		}
		UnaRuntimeSourceKind::GltfLike
	}

	pub fn humanoid_basis(self) -> UnaHumanoidRuntimeBasis {
		match self.source_kind() {
			// .unavatar is exported from Unity as glTF-space local TRS:
			// position = (-x, y, z), rotation = (x, -y, -z, w).
			UnaRuntimeSourceKind::Unavatar => UnaHumanoidRuntimeBasis::UnavatarUnity,
			UnaRuntimeSourceKind::Vrm1 => UnaHumanoidRuntimeBasis::Vrm1,
			UnaRuntimeSourceKind::Vrm0 | UnaRuntimeSourceKind::GltfLike => UnaHumanoidRuntimeBasis::Vrm0,
		}
	}

	pub fn scene(self) -> Option<&'a UnaSceneSnapshot> {
		self.document.scene.as_ref()
	}

	pub fn scene_nodes(self) -> Option<&'a [UnaSceneNode]> {
		self.scene().map(|scene| scene.nodes.as_slice())
	}

	pub fn humanoid_profile(self) -> Option<&'a HumanoidProfile> {
		self.document.humanoid_profile.as_ref()
	}

	pub fn has_humanoid_scene(self) -> bool {
		self.humanoid_profile().is_some() && self.scene().is_some()
	}

	pub fn scene_profile_dynamics(self) -> Option<UnaRuntimeSceneDynamics<'a>> {
		Some(UnaRuntimeSceneDynamics {
			scene: self.scene()?,
			humanoid_profile: self.humanoid_profile(),
			dynamics: self.dynamics(),
		})
	}

	pub fn expression_catalog(self) -> Option<&'a UnaExpressionCatalog> {
		self.document.expression_catalog.as_ref()
	}

	pub fn expression_weights(self) -> Option<&'a UnaExpressionWeights> {
		self.document.expression_weights.as_ref()
	}

	pub fn runtime_actions(self) -> Option<&'a UnaRuntimeActionSet> {
		self.document.runtime_actions.as_ref()
	}

	pub fn runtime_state(self) -> &'a UnaRuntimeState {
		&self.document.runtime_state
	}

	pub fn resolver_cache_key(self) -> UnaRuntimeResolverCacheKey {
		UnaRuntimeResolverCacheKey::from_document(self.document)
	}

	pub fn active_wardrobe_set(self) -> Option<&'a str> {
		self.runtime_state().active_wardrobe_set.as_deref()
	}

	pub fn active_asset_groups(self) -> &'a [String] {
		&self.runtime_state().active_asset_groups
	}

	pub fn last_action_id(self) -> Option<&'a str> {
		self.runtime_state().last_action_id.as_deref()
	}

	pub fn runtime_parameter_values(self) -> &'a BTreeMap<String, f32> {
		&self.runtime_state().parameter_values
	}

	pub fn node_visible(self, target: &UnaRuntimeNodeTarget) -> Option<bool> {
		let scene = self.document.scene.as_ref()?;
		let index = resolve_runtime_node_target(scene, target)?;
		scene.nodes.get(index).map(|node| node.visible)
	}

	pub fn material_color(self, target: &UnaRuntimeMaterialTarget, parameter: &str) -> Option<[f32; 4]> {
		let material = self.resolve_material(target)?;
		read_runtime_material_color(material, parameter)
	}

	pub fn material_scalar(self, target: &UnaRuntimeMaterialTarget, parameter: &str) -> Option<f32> {
		let material = self.resolve_material(target)?;
		read_runtime_material_scalar(material, parameter)
	}

	pub fn material_slot(self, target: &UnaRuntimeMaterialSlotTarget) -> Option<Option<usize>> {
		let scene = self.document.scene.as_ref()?;
		let node_index = resolve_runtime_node_target(scene, &target.node)?;
		let mesh_index = scene.nodes.get(node_index).and_then(|node| node.mesh)?;
		let primitive_index = target.primitive_index.unwrap_or(0);
		scene
			.meshes
			.get(mesh_index)
			.and_then(|mesh| mesh.get(primitive_index))
			.map(|primitive| primitive.material_index)
	}

	pub fn dynamics_enabled(self, source_id: &str) -> Option<bool> {
		self.dynamics()
			.dynamics_groups()
			.find(|group| group.source_id == source_id)
			.map(|group| group.effective_enabled)
	}

	pub fn runtime_action_restore_readiness(self, action: &UnaRuntimeAction) -> Vec<UnaEvaluationRestoreReadiness> {
		action
			.effects
			.iter()
			.map(|effect| self.runtime_action_effect_restore_readiness(&action.id, effect))
			.collect()
	}

	pub fn runtime_action_set_restore_readiness(self, actions: &UnaRuntimeActionSet) -> Vec<UnaEvaluationRestoreReadiness> {
		actions
			.actions
			.iter()
			.flat_map(|action| self.runtime_action_restore_readiness(action))
			.collect()
	}

	pub fn runtime_action_set_restore_baseline_candidates(
		self,
		actions: &UnaRuntimeActionSet,
	) -> Vec<UnaEvaluationRestoreBaselineCandidate> {
		self.runtime_action_set_restore_readiness(actions)
			.into_iter()
			.filter_map(|readiness| {
				if !readiness.restore_target {
					return None;
				}
				let baseline_value = readiness.current_value?;
				Some(UnaEvaluationRestoreBaselineCandidate {
					owner_key: readiness.owner_key,
					action_id: readiness.action_id,
					effect_kind: readiness.effect_kind,
					target_kind: readiness.target_kind,
					target_key: readiness.target_key,
					baseline_value,
				})
			})
			.collect()
	}

	pub fn runtime_action_set_restore_baseline_capture_plan(self, actions: &UnaRuntimeActionSet) -> Vec<UnaEvaluationRestoreBaselineEntry> {
		restore_baseline_capture_plan_from_candidates(self.runtime_action_set_restore_baseline_candidates(actions))
	}

	fn resolve_material(self, target: &UnaRuntimeMaterialTarget) -> Option<&'a UnaMaterialPbr> {
		let scene = self.document.scene.as_ref()?;
		if let Some(index) = resolve_runtime_material_index(scene, target) {
			return scene.materials.get(index);
		}
		None
	}

	fn runtime_action_effect_restore_readiness(self, action_id: &str, effect: &UnaRuntimeActionEffect) -> UnaEvaluationRestoreReadiness {
		let write = effect.evaluation_target_write(action_id);
		let (restore_target, current_value, baseline_required, reason) = match effect {
			UnaRuntimeActionEffect::WardrobeSet { .. } => (false, None, false, "not_restore_target"),
			UnaRuntimeActionEffect::ExpressionWeight { .. } => (false, None, false, "not_restore_target"),
			UnaRuntimeActionEffect::NodeVisibility { target, .. } => {
				let current_value = self.node_visible(target).map(Value::from);
				let available = current_value.is_some();
				(
					true,
					current_value,
					true,
					if available { "baseline_not_captured" } else { "target_unresolved" },
				)
			}
			UnaRuntimeActionEffect::MaterialColor { target, parameter, .. } => {
				let current_value = self
					.material_color(target, parameter)
					.map(|color| Value::Array(color.into_iter().map(Value::from).collect()));
				let available = current_value.is_some();
				(
					true,
					current_value,
					true,
					if available {
						"baseline_not_captured"
					} else {
						"target_unresolved_or_unsupported_parameter"
					},
				)
			}
			UnaRuntimeActionEffect::MaterialScalar { target, parameter, .. } => {
				let current_value = self.material_scalar(target, parameter).map(Value::from);
				let available = current_value.is_some();
				(
					true,
					current_value,
					true,
					if available {
						"baseline_not_captured"
					} else {
						"target_unresolved_or_unsupported_parameter"
					},
				)
			}
			UnaRuntimeActionEffect::MaterialSlot { target, .. } => {
				let current_value = self
					.material_slot(target)
					.map(|slot| slot.map_or(Value::Null, |index| Value::from(index as u64)));
				let available = current_value.is_some();
				(
					true,
					current_value,
					true,
					if available { "baseline_not_captured" } else { "target_unresolved" },
				)
			}
			UnaRuntimeActionEffect::DynamicsEnabled { source_id, .. } => {
				let current_value = self.dynamics_enabled(source_id).map(Value::from);
				let available = current_value.is_some();
				(
					true,
					current_value,
					true,
					if available { "baseline_not_captured" } else { "target_unresolved" },
				)
			}
		};
		let current_value_available = current_value.is_some();
		UnaEvaluationRestoreReadiness {
			owner_key: write.owner_key,
			action_id: write.action_id,
			effect_kind: write.effect_kind,
			target_kind: write.target_kind,
			target_key: write.target_key,
			restore_target,
			current_value_available,
			current_value,
			baseline_required,
			ready: false,
			reason: reason.to_string(),
		}
	}

	pub fn scene_expression_catalog(self) -> Option<UnaRuntimeSceneExpressions<'a>> {
		Some(UnaRuntimeSceneExpressions {
			scene: self.scene()?,
			expression_catalog: self.expression_catalog(),
		})
	}

	pub fn humanoid_retarget_inputs(self) -> UnaRuntimeRetargetInputs<'a> {
		UnaRuntimeRetargetInputs {
			humanoid_basis: self.humanoid_basis(),
			profile: self.humanoid_profile(),
			scene: self.scene(),
			expression_catalog: self.expression_catalog(),
		}
	}

	pub fn dynamics(self) -> UnaRuntimeDynamics<'a> {
		UnaRuntimeDynamics {
			spring_bones: self.document.spring_bones.as_ref(),
			runtime_state: Some(&self.document.runtime_state),
		}
	}
}

impl<'a> UnaRuntimeModelMut<'a> {
	pub fn scene_and_dynamics_mut(self) -> Option<UnaRuntimeSceneDynamicsMut<'a>> {
		let UnaDocument {
			scene,
			spring_bones,
			runtime_state,
			..
		} = self.document;
		Some(UnaRuntimeSceneDynamicsMut {
			scene: scene.as_mut()?,
			dynamics: UnaRuntimeDynamicsMut {
				spring_bones: spring_bones.as_mut(),
				runtime_state: Some(runtime_state),
			},
		})
	}

	pub fn humanoid_scene_mut(&mut self) -> Option<(&mut UnaSceneSnapshot, &HumanoidProfile)> {
		let UnaDocument {
			scene, humanoid_profile, ..
		} = self.document;
		Some((scene.as_mut()?, humanoid_profile.as_ref()?))
	}

	pub fn expression_weights_mut(&mut self) -> &mut UnaExpressionWeights {
		self.document.expression_weights.get_or_insert_with(Default::default)
	}

	pub fn runtime_state_mut(&mut self) -> &mut UnaRuntimeState {
		&mut self.document.runtime_state
	}

	pub fn set_active_wardrobe_set(&mut self, set_id: Option<String>) {
		self.runtime_state_mut().active_wardrobe_set = set_id;
	}

	pub fn set_active_asset_groups(&mut self, asset_groups: Vec<String>) {
		self.runtime_state_mut().active_asset_groups = asset_groups;
	}

	pub fn set_last_action_id(&mut self, action_id: Option<String>) {
		self.runtime_state_mut().last_action_id = action_id;
	}

	pub fn set_runtime_parameter_value(&mut self, name: impl Into<String>, value: f32) {
		self.runtime_state_mut().parameter_values.insert(name.into(), value);
	}

	pub fn set_runtime_parameter_values(&mut self, values: BTreeMap<String, f32>) {
		self.runtime_state_mut().parameter_values.extend(values);
	}

	pub fn set_node_visible(&mut self, target: &UnaRuntimeNodeTarget, visible: bool) -> bool {
		let Some(scene) = self.document.scene.as_mut() else {
			return false;
		};
		let Some(index) = resolve_runtime_node_target(scene, target) else {
			return false;
		};
		let Some(node) = scene.nodes.get_mut(index) else {
			return false;
		};
		node.visible = visible;
		true
	}

	pub fn set_material_color(&mut self, target: &UnaRuntimeMaterialTarget, parameter: &str, color: [f32; 4]) -> Result<(), String> {
		let Some(material) = self.resolve_material_mut(target) else {
			return Err(format!("runtime material target not found: {target:?}"));
		};
		apply_runtime_material_color(material, parameter, color)
	}

	pub fn set_material_scalar(&mut self, target: &UnaRuntimeMaterialTarget, parameter: &str, value: f32) -> Result<(), String> {
		let Some(material) = self.resolve_material_mut(target) else {
			return Err(format!("runtime material target not found: {target:?}"));
		};
		apply_runtime_material_scalar(material, parameter, value)
	}

	pub fn set_material_slot(
		&mut self,
		target: &UnaRuntimeMaterialSlotTarget,
		material: Option<&UnaRuntimeMaterialTarget>,
	) -> Result<(), String> {
		let Some(scene) = self.document.scene.as_mut() else {
			return Err("document has no scene".to_string());
		};
		let material_index = if let Some(material) = material {
			Some(
				resolve_runtime_material_index(scene, material)
					.ok_or_else(|| format!("runtime material target not found: {material:?}"))?,
			)
		} else {
			None
		};
		let Some(node_index) = resolve_runtime_node_target(scene, &target.node) else {
			return Err(format!("runtime node target not found: {:?}", target.node));
		};
		let Some(mesh_index) = scene.nodes.get(node_index).and_then(|node| node.mesh) else {
			return Err(format!("runtime node target has no mesh: {:?}", target.node));
		};
		let primitive_index = target.primitive_index.unwrap_or(0);
		let Some(primitive) = scene.meshes.get_mut(mesh_index).and_then(|mesh| mesh.get_mut(primitive_index)) else {
			return Err(format!(
				"runtime material slot target not found: node={:?}, primitive_index={primitive_index}",
				target.node
			));
		};
		primitive.material_index = material_index;
		Ok(())
	}

	fn resolve_material_mut(&mut self, target: &UnaRuntimeMaterialTarget) -> Option<&mut UnaMaterialPbr> {
		let scene = self.document.scene.as_mut()?;
		if let Some(index) = resolve_runtime_material_index(scene, target) {
			return scene.materials.get_mut(index);
		}
		None
	}
}

fn resolve_runtime_material_index(scene: &UnaSceneSnapshot, target: &UnaRuntimeMaterialTarget) -> Option<usize> {
	if let Some(index) = target.material_index.filter(|index| *index < scene.materials.len()) {
		return Some(index);
	}
	let name = target.name.as_deref().filter(|name| !name.is_empty())?;
	scene.materials.iter().position(|material| material.name.as_deref() == Some(name))
}

fn resolve_runtime_node_target(scene: &UnaSceneSnapshot, target: &UnaRuntimeNodeTarget) -> Option<usize> {
	if let Some(source_node_id) = target.source_node_id.as_deref().filter(|value| !value.is_empty()) {
		if let Some((index, _)) = scene
			.nodes
			.iter()
			.enumerate()
			.find(|(_, node)| node.source_node_id.as_deref() == Some(source_node_id))
		{
			return Some(index);
		}
	}
	if let Some(resolved_node_id) = target.resolved_node_id.as_deref().filter(|value| !value.is_empty()) {
		if let Some((index, _)) = scene
			.nodes
			.iter()
			.enumerate()
			.find(|(_, node)| node.resolved_node_id.as_deref() == Some(resolved_node_id))
		{
			return Some(index);
		}
	}
	if let Some(path) = target.path.as_deref().filter(|value| !value.is_empty()) {
		if let Some(index) = runtime_scene_node_paths(scene).get(path).copied() {
			return Some(index);
		}
	}
	if let Some(index) = target.node_index.filter(|index| *index < scene.nodes.len()) {
		return Some(index);
	}
	None
}

fn runtime_scene_node_paths(scene: &UnaSceneSnapshot) -> std::collections::BTreeMap<String, usize> {
	fn visit(scene: &UnaSceneSnapshot, index: usize, path: String, out: &mut std::collections::BTreeMap<String, usize>) {
		out.insert(path.clone(), index);
		let Some(node) = scene.nodes.get(index) else {
			return;
		};
		for &child in &node.children {
			let Some(child_node) = scene.nodes.get(child) else {
				continue;
			};
			let child_name = child_node.name.as_deref().unwrap_or("");
			let child_path = if path.is_empty() {
				child_name.to_string()
			} else {
				format!("{path}/{child_name}")
			};
			visit(scene, child, child_path, out);
		}
	}

	let mut out = std::collections::BTreeMap::new();
	for &root in &scene.roots {
		visit(scene, root, String::new(), &mut out);
		if let Some(name) = scene
			.nodes
			.get(root)
			.and_then(|node| node.name.as_deref())
			.filter(|name| !name.is_empty())
		{
			visit(scene, root, name.to_string(), &mut out);
		}
	}
	out
}

fn runtime_material_parameter_key(parameter: &str) -> String {
	parameter
		.trim()
		.trim_start_matches('_')
		.chars()
		.filter(|ch| *ch != '_' && *ch != '-' && !ch.is_whitespace())
		.flat_map(char::to_lowercase)
		.collect()
}

pub fn apply_runtime_material_color(material: &mut UnaMaterialPbr, parameter: &str, color: [f32; 4]) -> Result<(), String> {
	let key = runtime_material_parameter_key(parameter);
	match key.as_str() {
		"" | "color" | "basecolor" | "maincolor" => {
			material.base_color_factor = color.map(|value| value.clamp(0.0, 1.0));
			Ok(())
		}
		"emissive" | "emission" | "emissivecolor" | "emissioncolor" => {
			material.emissive_factor = [color[0].max(0.0), color[1].max(0.0), color[2].max(0.0)];
			Ok(())
		}
		_ => Err(format!("runtime material color parameter `{parameter}` is not supported")),
	}
}

pub fn read_runtime_material_color(material: &UnaMaterialPbr, parameter: &str) -> Option<[f32; 4]> {
	let key = runtime_material_parameter_key(parameter);
	match key.as_str() {
		"" | "color" | "basecolor" | "maincolor" => Some(material.base_color_factor),
		"emissive" | "emission" | "emissivecolor" | "emissioncolor" => Some([
			material.emissive_factor[0],
			material.emissive_factor[1],
			material.emissive_factor[2],
			1.0,
		]),
		_ => None,
	}
}

pub fn apply_runtime_material_scalar(material: &mut UnaMaterialPbr, parameter: &str, value: f32) -> Result<(), String> {
	if !value.is_finite() {
		return Err(format!("runtime material scalar parameter `{parameter}` received non-finite value"));
	}
	let key = runtime_material_parameter_key(parameter);
	match key.as_str() {
		"alpha" | "opacity" => {
			material.base_color_factor[3] = value.clamp(0.0, 1.0);
			Ok(())
		}
		"metallic" | "metallicfactor" => {
			material.metallic_factor = value.clamp(0.0, 1.0);
			Ok(())
		}
		"roughness" | "roughnessfactor" => {
			material.roughness_factor = value.clamp(0.0, 1.0);
			Ok(())
		}
		"smoothness" | "smoothnessfactor" => {
			material.roughness_factor = 1.0 - value.clamp(0.0, 1.0);
			Ok(())
		}
		"cutoff" | "alphacutoff" | "alphacutofffactor" => {
			material.alpha_cutoff = value.clamp(0.0, 1.0);
			Ok(())
		}
		_ => Err(format!("runtime material scalar parameter `{parameter}` is not supported")),
	}
}

pub fn read_runtime_material_scalar(material: &UnaMaterialPbr, parameter: &str) -> Option<f32> {
	let key = runtime_material_parameter_key(parameter);
	match key.as_str() {
		"alpha" | "opacity" => Some(material.base_color_factor[3]),
		"metallic" | "metallicfactor" => Some(material.metallic_factor),
		"roughness" | "roughnessfactor" => Some(material.roughness_factor),
		"smoothness" | "smoothnessfactor" => Some(1.0 - material.roughness_factor),
		"cutoff" | "alphacutoff" | "alphacutofffactor" => Some(material.alpha_cutoff),
		_ => None,
	}
}

/// `.unavatar` 固有 metadata。現段階では raw JSON を正本として保持し、runtime 対応が進むごとに構造化する。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaUnavatarExtension {
	pub spec_version: String,
	pub source: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnaNodeConstraintAxis {
	X,
	Y,
	Z,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnaNodeConstraintAimAxis {
	PositiveX,
	NegativeX,
	PositiveY,
	NegativeY,
	PositiveZ,
	NegativeZ,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnaNodeConstraintKind {
	Roll { axis: UnaNodeConstraintAxis },
	Aim { axis: UnaNodeConstraintAimAxis },
	Rotation,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaNodeConstraint {
	pub target_node: usize,
	pub source_node: usize,
	#[serde(default = "one_f32")]
	pub weight: f32,
	pub kind: UnaNodeConstraintKind,
}

/// VRM 固有メタ／Humanoid／MToon ヒントと、拡張 JSON 正本（ブレンドシェイプ・表情・SpringBone 等は `source` に完全保持）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaVrmExtension {
	pub spec_version: String,
	#[serde(default)]
	pub meta: Value,
	/// 正規化ボーン名（小文字）→ ノードインデックス。
	#[serde(default)]
	pub humanoid_bones: BTreeMap<String, usize>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub mtoon_materials_v0: Vec<UnaVrm0MtoonMaterialEntry>,
	/// glTF `materials` 配列インデックス（`VRMC_materials_mtoon` 付き）。
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub mtoon_material_indices_v1: Vec<usize>,
	pub source: Value,
}

impl UnaVrmExtension {
	pub fn is_vrm0(&self) -> bool {
		self.spec_version.starts_with('0')
	}
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaVrm0MtoonMaterialEntry {
	pub material_index: usize,
	pub shader_name: String,
	pub raw: Value,
}

/// シーンの読み取り専用スナップショット（glTF 等からの bootstrap 用）。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UnaSceneSnapshot {
	/// `nodes[*].mesh` が参照するメッシュ（1 要素 = glTF mesh、内側ベクトル = primitive）。
	pub meshes: Vec<Vec<UnaMeshBuffers>>,
	pub materials: Vec<UnaMaterialPbr>,
	pub images: Vec<UnaImageRgba>,
	/// Source package metadata for `images`, without duplicating source bytes in memory.
	/// The binary itself remains owned by the `.unavatar` / glTF package layer.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub image_sources: Vec<Option<UnaImageSourceMetadata>>,
	/// glTF `skins` と同順。joint の番号はこの配列内のインデックス（頂点 JOINTS は 0 始まり・スキン局所）。
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub skins: Vec<UnaSkin>,
	pub nodes: Vec<UnaSceneNode>,
	pub roots: Vec<usize>,
	/// VRM 1 `VRMC_node_constraint` 由来のノード拘束。target/source は `nodes` インデックス。
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub node_constraints: Vec<UnaNodeConstraint>,
	/// Source asset group ownership used by wardrobe lazy upload planning.
	/// Runtime hot switch state remains in [`UnaRuntimeState::active_asset_groups`].
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub asset_group_ownership: Vec<UnaSceneAssetGroupOwnership>,
}

impl UnaSceneSnapshot {
	/// Runtime roots after import normalization. Authored `roots` are borrowed as-is;
	/// legacy or partial imports without roots fall back to parentless nodes.
	pub fn resolved_roots(&self) -> Cow<'_, [usize]> {
		resolved_scene_roots(&self.nodes, &self.roots)
	}

	pub fn asset_group_ownership_counts(&self) -> UnaSceneAssetGroupOwnershipCounts {
		let mut counts = UnaSceneAssetGroupOwnershipCounts {
			groups: self.asset_group_ownership.len(),
			..Default::default()
		};
		for group in &self.asset_group_ownership {
			counts.mesh_primitives += group.mesh_primitives.len();
			counts.materials += group.materials.len();
			counts.images += group.images.len();
			counts.dynamics += group.dynamics_source_ids.len();
		}
		counts
	}

	pub fn scoped_asset_selection(&self, active_asset_groups: &[String]) -> UnaSceneScopedAssetSelection {
		if active_asset_groups.is_empty() {
			return UnaSceneScopedAssetSelection::default();
		}
		let mut remaining_active_groups = active_asset_groups.iter().cloned().collect::<BTreeSet<_>>();
		let mut owned_active_groups = BTreeSet::new();
		let mut mesh_primitives = BTreeSet::<(usize, usize)>::new();
		let mut materials = BTreeSet::new();
		let mut images = BTreeSet::new();
		let mut dynamics_source_ids = BTreeSet::new();
		for group in &self.asset_group_ownership {
			if !remaining_active_groups.contains(&group.group_id) {
				continue;
			}
			owned_active_groups.insert(group.group_id.clone());
			mesh_primitives.extend(
				group
					.mesh_primitives
					.iter()
					.map(|primitive| (primitive.mesh_index, primitive.primitive_index)),
			);
			materials.extend(group.materials.iter().copied());
			images.extend(group.images.iter().copied());
			dynamics_source_ids.extend(group.dynamics_source_ids.iter().cloned());
		}
		for group in &owned_active_groups {
			remaining_active_groups.remove(group);
		}
		UnaSceneScopedAssetSelection {
			owned_active_groups: owned_active_groups.into_iter().collect(),
			missing_active_asset_groups: remaining_active_groups.into_iter().collect(),
			mesh_primitives: mesh_primitives
				.into_iter()
				.map(|(mesh_index, primitive_index)| UnaMeshPrimitiveKey {
					mesh_index,
					primitive_index,
				})
				.collect(),
			materials: materials.into_iter().collect(),
			images: images.into_iter().collect(),
			dynamics_source_ids: dynamics_source_ids.into_iter().collect(),
		}
	}
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnaSceneAssetGroupOwnership {
	pub group_id: String,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub mesh_primitives: Vec<UnaMeshPrimitiveKey>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub materials: Vec<usize>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub images: Vec<usize>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub dynamics_source_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnaMeshPrimitiveKey {
	pub mesh_index: usize,
	pub primitive_index: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UnaSceneAssetGroupOwnershipCounts {
	pub groups: usize,
	pub mesh_primitives: usize,
	pub materials: usize,
	pub images: usize,
	pub dynamics: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UnaSceneScopedAssetSelection {
	pub owned_active_groups: Vec<String>,
	pub missing_active_asset_groups: Vec<String>,
	pub mesh_primitives: Vec<UnaMeshPrimitiveKey>,
	pub materials: Vec<usize>,
	pub images: Vec<usize>,
	pub dynamics_source_ids: Vec<String>,
}

pub fn resolved_scene_roots<'a>(nodes: &[UnaSceneNode], roots: &'a [usize]) -> Cow<'a, [usize]> {
	if !roots.is_empty() {
		return Cow::Borrowed(roots);
	}
	let mut has_parent = vec![false; nodes.len()];
	for node in nodes {
		for &child in &node.children {
			if let Some(slot) = has_parent.get_mut(child) {
				*slot = true;
			}
		}
	}
	Cow::Owned(
		has_parent
			.iter()
			.enumerate()
			.filter_map(|(idx, has_parent)| (!*has_parent).then_some(idx))
			.collect(),
	)
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UnaImageSourceMetadata {
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub name: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub mime_type: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub uri: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub source_pixel_format: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub channels: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub color_space: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub texture_type: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub texture_shape: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub source_layout: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub unity_generate_cubemap: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub srgb: Option<bool>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub sampler: Option<UnaTextureSampler>,
	pub byte_length: u64,
	pub source_hash: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnaTextureWrapMode {
	ClampToEdge,
	MirroredRepeat,
	#[default]
	Repeat,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnaTextureFilterMode {
	Nearest,
	#[default]
	Linear,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UnaTextureSampler {
	pub mag_filter: UnaTextureFilterMode,
	pub min_filter: UnaTextureFilterMode,
	pub wrap_s: UnaTextureWrapMode,
	pub wrap_t: UnaTextureWrapMode,
}

/// 1 スキン分のジョイントノード（シーン `nodes` のインデックス）と逆バインド行列（列主序 16 floats・`transform` と同じ並び）。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UnaSkin {
	pub joint_nodes: Vec<usize>,
	pub inverse_bind_matrices: Vec<[f32; 16]>,
	/// glTF `skin.skeleton` / Unity `SkinnedMeshRenderer.rootBone` に相当する root bone node。
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub skeleton_node: Option<usize>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UnaBounds {
	pub center: [f32; 3],
	pub extents: [f32; 3],
}

/// ノード局所変換（列主序 4×4・WGSL / glTF と同趣）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaSceneNode {
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub name: Option<String>,
	/// `.unavatar` exporter が付与する stable node id。wardrobe operations はこれを正本にし、
	/// path は表示と古いファイル向け fallback に使う。
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub source_node_id: Option<String>,
	/// Runtime resolver が作る派生 graph node id。source id は authoring target として保持し、
	/// resolved id は cache/debug のために別管理する。
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub resolved_node_id: Option<String>,
	/// Runtime visibility. `.unavatar` wardrobe base / set operations can turn whole subtrees off before upload/draw.
	#[serde(default = "default_true")]
	pub visible: bool,
	#[serde(with = "col16")]
	pub transform: [f32; 16],
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub children: Vec<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub mesh: Option<usize>,
	/// glTF `skin` インデックス（`UnaSceneSnapshot::skins`）。メッシュ付きノードのみ。
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub skin: Option<usize>,
	/// Unity `Renderer.probeAnchor` に相当する node。主に Modular Avatar Mesh Settings 由来。
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub probe_anchor_node: Option<usize>,
	/// Unity `SkinnedMeshRenderer.localBounds` に相当する rootBone local bounds。
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub local_bounds: Option<UnaBounds>,
}

fn default_true() -> bool {
	true
}

fn is_false(value: &bool) -> bool {
	!*value
}

mod col16 {
	use serde::{Deserialize, Deserializer, Serialize, Serializer};

	pub fn serialize<S>(v: &[f32; 16], s: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		v.as_slice().serialize(s)
	}

	pub fn deserialize<'de, D>(d: D) -> Result<[f32; 16], D::Error>
	where
		D: Deserializer<'de>,
	{
		<[f32; 16]>::deserialize(d)
	}
}

/// メッシュ 1 primitive 分。`joints` / `weights` が無い場合は剛体メッシュ（JOINTS_0 はパレット 0 のみ使用）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaMeshBuffers {
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub name: Option<String>,
	pub positions: Vec<[f32; 3]>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub normals: Option<Vec<[f32; 3]>>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub tangents: Option<Vec<[f32; 4]>>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub tex_coords_0: Option<Vec<[f32; 2]>>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub tex_coords_1: Option<Vec<[f32; 2]>>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub tex_coords_2: Option<Vec<[f32; 2]>>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub tex_coords_3: Option<Vec<[f32; 2]>>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub colors_0: Option<Vec<[f32; 4]>>,
	/// スキン内ジョイントインデックス（頂点ごと 4 本）。未使用スロットは 0。
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub joints: Option<Vec<[u16; 4]>>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub weights: Option<Vec<[f32; 4]>>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub indices: Option<Vec<u32>>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub material_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub morph_targets: Vec<UnaMorphTargetDeltas>,
	/// glTF `mesh.extras.targetNames` 由来のモーフターゲット名。`.unavatar` wardrobe の blendShapeWeight 解決に使う。
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub morph_target_names: Vec<String>,
	/// glTF `mesh.weights` のコピー（長さは `morph_targets` と一致する想定）。
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub default_morph_weights: Vec<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaMaterialPbr {
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub name: Option<String>,
	#[serde(default)]
	pub double_sided: bool,
	#[serde(default)]
	pub cull_mode: UnaCullMode,
	pub base_color_factor: [f32; 4],
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub base_color_texture_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub normal_texture_index: Option<usize>,
	#[serde(default = "one_f32")]
	pub normal_texture_scale: f32,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub occlusion_texture_index: Option<usize>,
	#[serde(default = "one_f32")]
	pub occlusion_texture_strength: f32,
	#[serde(default)]
	pub emissive_factor: [f32; 3],
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub emissive_texture_index: Option<usize>,
	#[serde(default = "one_f32")]
	pub metallic_factor: f32,
	#[serde(default = "one_f32")]
	pub roughness_factor: f32,
	#[serde(default)]
	pub shading: UnaShadingModel,
	#[serde(default)]
	pub alpha_mode: UnaAlphaMode,
	/// glTF `alphaCutoff`（`MASK` 時。未指定の既定は 0.5）。
	#[serde(default = "default_alpha_cutoff")]
	pub alpha_cutoff: f32,
	/// Base UV transform shared by the renderer for the primary material UV:
	/// `[offset_x, offset_y, scale_x, scale_y]`.
	#[serde(default = "default_uv_offset_scale")]
	pub uv_offset_scale: [f32; 4],
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub mtoon: Option<UnaMtoonMaterial>,
	/// lilToon-like v2 material model. This is the primary runtime material for v2.
	/// lilToon inputs are imported here; MToon/VRM may be mapped later, but is
	/// not the design base.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub liltoon_like: Option<UnaLilToonLikeMaterial>,
	/// `.unavatar` material extension payload as authored/exported. Runtime
	/// material importers may read this for lilToon-like compatibility without
	/// reparsing the source glTF JSON.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub unavatar_material: Option<Value>,
}

impl UnaMaterialPbr {
	pub fn runtime_toon_model(&self) -> Option<UnaRuntimeToonModel> {
		match self.shading {
			UnaShadingModel::MToonLike => Some(UnaRuntimeToonModel::MToonLike),
			UnaShadingModel::LilToonLike => Some(UnaRuntimeToonModel::LilToonLike),
			UnaShadingModel::LitLambert | UnaShadingModel::Unlit => None,
		}
	}

	pub fn liltoon_like_runtime(&self) -> Option<&UnaLilToonLikeMaterial> {
		self.shading.is_liltoon_like().then_some(self.liltoon_like.as_ref()).flatten()
	}

	pub fn liltoon_like_source_profile(&self) -> Option<&UnaLilToonLikeMaterial> {
		self.liltoon_like.as_ref()
	}

	pub fn mtoon_like_runtime(&self) -> Option<&UnaMtoonMaterial> {
		self.shading.is_mtoon_like().then_some(self.mtoon.as_ref()).flatten()
	}

	pub fn mtoon_source_profile(&self) -> Option<&UnaMtoonMaterial> {
		self.mtoon.as_ref()
	}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum UnaMtoonOutlineWidthMode {
	#[default]
	None,
	WorldCoordinates,
	ScreenCoordinates,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnaLilToonLikeSourceProfile {
	#[default]
	Unknown,
	Liltoon,
	LiltoonGem,
	LiltoonRefraction,
	MtoonConverted,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnaLilToonLikeBlendMode {
	Normal,
	#[default]
	Add,
	Screen,
	Multiply,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaLilToonLikeMainColor {
	#[serde(default = "default_liltoon_main_texture_hsvg")]
	pub main_texture_hsvg_factor: [f32; 4],
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub main_color_adjust_mask_texture_index: Option<usize>,
	#[serde(default)]
	pub gradation_enabled_factor: f32,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub gradation_texture_index: Option<usize>,
	#[serde(default)]
	pub gradation_strength_factor: f32,
	#[serde(default)]
	pub second_enabled_factor: f32,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub second_texture_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub second_blend_mask_texture_index: Option<usize>,
	#[serde(default = "one_vec4")]
	pub second_color_factor: [f32; 4],
	#[serde(default)]
	pub second_blend_mode: UnaLilToonLikeBlendMode,
	#[serde(default = "one_f32")]
	pub second_enable_lighting_factor: f32,
	#[serde(default)]
	pub second_alpha_mode_factor: f32,
	#[serde(default)]
	pub second_cull_factor: f32,
	#[serde(default = "default_liltoon_layer_distance_fade")]
	pub second_distance_fade_factor: [f32; 4],
	#[serde(default)]
	pub second_decal_flags_factor: [f32; 4],
	#[serde(default)]
	pub second_decal_transform_factor: [f32; 4],
	#[serde(default)]
	pub second_decal_animation_factor: [f32; 4],
	#[serde(default)]
	pub second_decal_sub_param_factor: [f32; 4],
	#[serde(default)]
	pub second_dissolve: UnaLilToonLikeDissolve,
	#[serde(default)]
	pub third_enabled_factor: f32,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub third_texture_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub third_blend_mask_texture_index: Option<usize>,
	#[serde(default = "one_vec4")]
	pub third_color_factor: [f32; 4],
	#[serde(default)]
	pub third_blend_mode: UnaLilToonLikeBlendMode,
	#[serde(default = "one_f32")]
	pub third_enable_lighting_factor: f32,
	#[serde(default)]
	pub third_alpha_mode_factor: f32,
	#[serde(default)]
	pub third_cull_factor: f32,
	#[serde(default = "default_liltoon_layer_distance_fade")]
	pub third_distance_fade_factor: [f32; 4],
	#[serde(default)]
	pub third_decal_flags_factor: [f32; 4],
	#[serde(default)]
	pub third_decal_transform_factor: [f32; 4],
	#[serde(default)]
	pub third_decal_animation_factor: [f32; 4],
	#[serde(default)]
	pub third_decal_sub_param_factor: [f32; 4],
	#[serde(default)]
	pub third_dissolve: UnaLilToonLikeDissolve,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaLilToonLikeShadow {
	#[serde(default = "one_f32")]
	pub enabled_factor: f32,
	#[serde(default)]
	pub color_factor: [f32; 3],
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub color_texture_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub strength_mask_texture_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub border_mask_texture_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub blur_mask_texture_index: Option<usize>,
	#[serde(default = "one_f32")]
	pub strength_factor: f32,
	#[serde(default = "default_liltoon_shadow_border")]
	pub border_factor: f32,
	#[serde(default = "default_liltoon_shadow_blur")]
	pub blur_factor: f32,
	#[serde(default)]
	pub border_range_factor: f32,
	#[serde(default = "default_liltoon_shadow_main_strength")]
	pub main_strength_factor: f32,
	#[serde(default = "default_liltoon_shadow_env_strength")]
	pub env_strength_factor: f32,
	#[serde(default = "default_liltoon_shadow_border_color")]
	pub border_color_factor: [f32; 3],
	#[serde(default)]
	pub post_ao_factor: f32,
	#[serde(default = "default_liltoon_shadow_ao_shift")]
	pub ao_shift_factor: [f32; 4],
	#[serde(default = "default_liltoon_shadow_ao_shift2")]
	pub ao_shift2_factor: [f32; 4],
	#[serde(default = "one_f32")]
	pub normal_strength_factor: f32,
	#[serde(default)]
	pub receive_factor: f32,
	#[serde(default)]
	pub second_color_factor: [f32; 4],
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub second_color_texture_index: Option<usize>,
	#[serde(default)]
	pub second_border_factor: f32,
	#[serde(default)]
	pub second_blur_factor: f32,
	#[serde(default = "one_f32")]
	pub second_normal_strength_factor: f32,
	#[serde(default)]
	pub second_receive_factor: f32,
	#[serde(default)]
	pub third_color_factor: [f32; 4],
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub third_color_texture_index: Option<usize>,
	#[serde(default)]
	pub third_border_factor: f32,
	#[serde(default)]
	pub third_blur_factor: f32,
	#[serde(default = "one_f32")]
	pub third_normal_strength_factor: f32,
	#[serde(default)]
	pub third_receive_factor: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaLilToonLikeNormal {
	#[serde(default)]
	pub second_enabled_factor: f32,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub second_texture_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub second_scale_mask_texture_index: Option<usize>,
	#[serde(default = "one_f32")]
	pub second_scale_factor: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaLilToonLikeMatcap {
	#[serde(default)]
	pub enabled_factor: f32,
	#[serde(default = "one_vec3")]
	pub color_factor: [f32; 3],
	#[serde(default = "one_f32")]
	pub color_alpha_factor: f32,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub texture_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub blend_mask_texture_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub bump_texture_index: Option<usize>,
	#[serde(default = "one_f32")]
	pub blend_factor: f32,
	#[serde(default)]
	pub main_strength_factor: f32,
	#[serde(default)]
	pub enable_lighting_factor: f32,
	#[serde(default)]
	pub blend_mode: UnaLilToonLikeBlendMode,
	#[serde(default = "one_f32")]
	pub normal_strength_factor: f32,
	#[serde(default)]
	pub custom_normal_factor: f32,
	#[serde(default = "one_f32")]
	pub bump_scale_factor: f32,
	#[serde(default)]
	pub shadow_mask_factor: f32,
	#[serde(default = "one_f32")]
	pub apply_transparency_factor: f32,
	#[serde(default)]
	pub lod_factor: f32,
	#[serde(default)]
	pub backface_mask_factor: f32,
	#[serde(default = "one_f32")]
	pub perspective_factor: f32,
	#[serde(default = "one_f32")]
	pub z_rotation_cancel_factor: f32,
	#[serde(default = "one_f32")]
	pub vr_parallax_strength_factor: f32,
	#[serde(default)]
	pub blend_uv1_factor: [f32; 2],
	#[serde(default)]
	pub second_enabled_factor: f32,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub second_texture_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub second_blend_mask_texture_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub second_bump_texture_index: Option<usize>,
	#[serde(default = "one_vec4")]
	pub second_color_factor: [f32; 4],
	#[serde(default)]
	pub second_main_strength_factor: f32,
	#[serde(default = "one_f32")]
	pub second_blend_factor: f32,
	#[serde(default = "one_f32")]
	pub second_enable_lighting_factor: f32,
	#[serde(default)]
	pub second_shadow_mask_factor: f32,
	#[serde(default = "one_f32")]
	pub second_apply_transparency_factor: f32,
	#[serde(default)]
	pub second_blend_mode: UnaLilToonLikeBlendMode,
	#[serde(default = "one_f32")]
	pub second_normal_strength_factor: f32,
	#[serde(default)]
	pub second_custom_normal_factor: f32,
	#[serde(default = "one_f32")]
	pub second_bump_scale_factor: f32,
	#[serde(default)]
	pub second_lod_factor: f32,
	#[serde(default)]
	pub second_backface_mask_factor: f32,
	#[serde(default = "one_f32")]
	pub second_perspective_factor: f32,
	#[serde(default = "one_f32")]
	pub second_z_rotation_cancel_factor: f32,
	#[serde(default = "one_f32")]
	pub second_vr_parallax_strength_factor: f32,
	#[serde(default)]
	pub second_blend_uv1_factor: [f32; 2],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaLilToonLikeReflection {
	#[serde(default)]
	pub enabled_factor: f32,
	#[serde(default = "one_vec4")]
	pub color_factor: [f32; 4],
	#[serde(default = "default_liltoon_smoothness")]
	pub smoothness_factor: f32,
	#[serde(default)]
	pub metallic_factor: f32,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub metallic_texture_index: Option<usize>,
	#[serde(default = "default_liltoon_reflectance")]
	pub reflectance_factor: f32,
	#[serde(default = "one_f32")]
	pub apply_specular_factor: f32,
	#[serde(default = "one_f32")]
	pub apply_specular_forward_add_factor: f32,
	#[serde(default = "one_f32")]
	pub apply_reflection_factor: f32,
	#[serde(default = "one_f32")]
	pub apply_transparency_factor: f32,
	#[serde(default = "one_f32")]
	pub specular_toon_factor: f32,
	#[serde(default = "default_liltoon_specular_border")]
	pub specular_border_factor: f32,
	#[serde(default)]
	pub specular_blur_factor: f32,
	#[serde(default = "one_f32")]
	pub specular_normal_strength_factor: f32,
	#[serde(default = "one_f32")]
	pub reflection_normal_strength_factor: f32,
	#[serde(default = "one_f32")]
	pub cube_enable_lighting_factor: f32,
	#[serde(default)]
	pub cube_color_factor: [f32; 4],
	#[serde(default)]
	pub cube_override_factor: f32,
	#[serde(default)]
	pub blend_mode: UnaLilToonLikeBlendMode,
	#[serde(default)]
	pub anisotropy_enabled_factor: f32,
	#[serde(default = "one_f32")]
	pub anisotropy_scale_factor: f32,
	#[serde(default)]
	pub anisotropy_shift_factor: f32,
	#[serde(default)]
	pub anisotropy_shift_noise_scale_factor: f32,
	#[serde(default = "one_f32")]
	pub anisotropy_specular_strength_factor: f32,
	#[serde(default = "one_f32")]
	pub anisotropy_tangent_width_factor: f32,
	#[serde(default = "one_f32")]
	pub anisotropy_bitangent_width_factor: f32,
	#[serde(default)]
	pub anisotropy_to_reflection_factor: f32,
	#[serde(default)]
	pub anisotropy_to_matcap_factor: f32,
	#[serde(default)]
	pub anisotropy_to_second_matcap_factor: f32,
	#[serde(default)]
	pub anisotropy_second_shift_factor: f32,
	#[serde(default)]
	pub anisotropy_second_shift_noise_scale_factor: f32,
	#[serde(default)]
	pub anisotropy_second_specular_strength_factor: f32,
	#[serde(default = "one_f32")]
	pub anisotropy_second_tangent_width_factor: f32,
	#[serde(default = "one_f32")]
	pub anisotropy_second_bitangent_width_factor: f32,
	#[serde(default = "one_vec4")]
	pub gem_env_color_factor: [f32; 4],
	#[serde(default = "one_f32")]
	pub gem_env_contrast_factor: f32,
	#[serde(default = "default_liltoon_refraction_fresnel_power")]
	pub gem_refraction_fresnel_power_factor: f32,
	#[serde(default = "default_liltoon_gem_refraction_strength")]
	pub gem_refraction_strength_factor: f32,
	#[serde(default = "one_vec4")]
	pub refraction_color_factor: [f32; 4],
	#[serde(default)]
	pub refraction_color_from_main_factor: f32,
	#[serde(default = "default_liltoon_gem_chromatic_aberration")]
	pub gem_chromatic_aberration_factor: f32,
	#[serde(default = "default_liltoon_gem_particle_loop")]
	pub gem_particle_loop_factor: f32,
	#[serde(default = "default_liltoon_gem_particle_color")]
	pub gem_particle_color_factor: [f32; 4],
	#[serde(default = "one_f32")]
	pub gem_vr_parallax_strength_factor: f32,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub cube_texture_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub color_texture_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub smoothness_texture_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub anisotropy_tangent_texture_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub anisotropy_scale_mask_texture_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub anisotropy_shift_noise_mask_texture_index: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaLilToonLikeRim {
	#[serde(default)]
	pub enabled_factor: f32,
	#[serde(default = "one_vec4")]
	pub color_factor: [f32; 4],
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub texture_index: Option<usize>,
	#[serde(default)]
	pub main_strength_factor: f32,
	#[serde(default = "default_liltoon_rim_border")]
	pub border_factor: f32,
	#[serde(default = "default_liltoon_rim_blur")]
	pub blur_factor: f32,
	#[serde(default = "default_liltoon_rim_fresnel_power")]
	pub fresnel_power_factor: f32,
	#[serde(default = "one_f32")]
	pub enable_lighting_factor: f32,
	#[serde(default)]
	pub blend_mode: UnaLilToonLikeBlendMode,
	#[serde(default)]
	pub shadow_mask_factor: f32,
	#[serde(default = "one_f32")]
	pub apply_transparency_factor: f32,
	#[serde(default = "one_f32")]
	pub normal_strength_factor: f32,
	#[serde(default = "one_f32")]
	pub backface_mask_factor: f32,
	#[serde(default)]
	pub directional_strength_factor: f32,
	#[serde(default)]
	pub directional_range_factor: f32,
	#[serde(default = "one_vec4")]
	pub indirect_color_factor: [f32; 4],
	#[serde(default)]
	pub indirect_range_factor: f32,
	#[serde(default = "default_liltoon_rim_border")]
	pub indirect_border_factor: f32,
	#[serde(default = "default_liltoon_rim_blur")]
	pub indirect_blur_factor: f32,
	#[serde(default)]
	pub shade_enabled_factor: f32,
	#[serde(default = "default_liltoon_rim_shade_color")]
	pub shade_color_factor: [f32; 4],
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub shade_mask_texture_index: Option<usize>,
	#[serde(default = "default_liltoon_rim_border")]
	pub shade_border_factor: f32,
	#[serde(default = "default_liltoon_rim_blur")]
	pub shade_blur_factor: f32,
	#[serde(default = "default_liltoon_rim_fresnel_power")]
	pub shade_fresnel_power_factor: f32,
	#[serde(default = "one_f32")]
	pub shade_normal_strength_factor: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaLilToonLikeEmission {
	#[serde(default)]
	pub enabled_factor: f32,
	#[serde(default)]
	pub color_factor: [f32; 4],
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub texture_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub blend_mask_texture_index: Option<usize>,
	#[serde(default)]
	pub main_strength_factor: f32,
	#[serde(default = "one_f32")]
	pub blend_factor: f32,
	#[serde(default)]
	pub blend_mode: UnaLilToonLikeBlendMode,
	#[serde(default = "default_liltoon_emission_blink")]
	pub blink_factor: [f32; 4],
	#[serde(default)]
	pub fluorescence_factor: f32,
	#[serde(default)]
	pub parallax_depth_factor: f32,
	#[serde(default)]
	pub uv_scroll_rotate_factor: [f32; 4],
	#[serde(default)]
	pub blend_mask_uv_scroll_rotate_factor: [f32; 4],
	#[serde(default)]
	pub gradation_enabled_factor: f32,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub gradation_texture_index: Option<usize>,
	#[serde(default = "one_f32")]
	pub gradation_speed_factor: f32,
	#[serde(default)]
	pub second_enabled_factor: f32,
	#[serde(default)]
	pub second_color_factor: [f32; 4],
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub second_texture_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub second_blend_mask_texture_index: Option<usize>,
	#[serde(default = "one_f32")]
	pub second_blend_factor: f32,
	#[serde(default)]
	pub second_blend_mode: UnaLilToonLikeBlendMode,
	#[serde(default = "default_liltoon_emission_blink")]
	pub second_blink_factor: [f32; 4],
	#[serde(default)]
	pub second_fluorescence_factor: f32,
	#[serde(default)]
	pub second_parallax_depth_factor: f32,
	#[serde(default)]
	pub second_uv_scroll_rotate_factor: [f32; 4],
	#[serde(default)]
	pub second_blend_mask_uv_scroll_rotate_factor: [f32; 4],
	#[serde(default)]
	pub second_main_strength_factor: f32,
	#[serde(default)]
	pub second_gradation_enabled_factor: f32,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub second_gradation_texture_index: Option<usize>,
	#[serde(default = "one_f32")]
	pub second_gradation_speed_factor: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaLilToonLikeAudioLink {
	#[serde(default)]
	pub enabled_factor: f32,
	#[serde(default = "default_liltoon_audio_link_default_value")]
	pub default_value_factor: [f32; 4],
	#[serde(default = "one_f32")]
	pub uv_mode_factor: f32,
	#[serde(default = "default_liltoon_audio_link_uv_params")]
	pub uv_params_factor: [f32; 4],
	#[serde(default)]
	pub start_factor: [f32; 4],
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub mask_texture_index: Option<usize>,
	#[serde(default)]
	pub mask_uv_scroll_rotate_factor: [f32; 4],
	#[serde(default)]
	pub mask_uv_mode_factor: f32,
	#[serde(default)]
	pub to_main_second_factor: f32,
	#[serde(default)]
	pub to_main_third_factor: f32,
	#[serde(default)]
	pub to_emission_factor: f32,
	#[serde(default)]
	pub to_emission_gradation_factor: f32,
	#[serde(default)]
	pub to_emission_second_factor: f32,
	#[serde(default)]
	pub to_emission_second_gradation_factor: f32,
	#[serde(default)]
	pub to_vertex_factor: f32,
	#[serde(default = "one_f32")]
	pub vertex_uv_mode_factor: f32,
	#[serde(default = "default_liltoon_audio_link_uv_params")]
	pub vertex_uv_params_factor: [f32; 4],
	#[serde(default)]
	pub vertex_start_factor: [f32; 4],
	#[serde(default = "default_liltoon_audio_link_vertex_strength")]
	pub vertex_strength_factor: [f32; 4],
	#[serde(default)]
	pub as_local_factor: f32,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub local_map_texture_index: Option<usize>,
	#[serde(default = "default_liltoon_audio_link_local_map_params")]
	pub local_map_params_factor: [f32; 4],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaLilToonLikeOutline {
	#[serde(default)]
	pub enabled_factor: f32,
	#[serde(default)]
	pub color_factor: [f32; 4],
	#[serde(default)]
	pub lit_color_factor: [f32; 4],
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub texture_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub width_mask_texture_index: Option<usize>,
	#[serde(default)]
	pub width_factor: f32,
	#[serde(default = "default_liltoon_outline_fix_width")]
	pub fix_width_factor: f32,
	#[serde(default = "one_f32")]
	pub enable_lighting_factor: f32,
	#[serde(default)]
	pub lit_scale_factor: f32,
	#[serde(default)]
	pub lit_offset_factor: f32,
	#[serde(default)]
	pub lit_apply_tex_factor: f32,
	#[serde(default)]
	pub lit_shadow_receive_factor: f32,
	#[serde(default)]
	pub z_bias_factor: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaLilToonLikeBacklight {
	#[serde(default)]
	pub enabled_factor: f32,
	#[serde(default)]
	pub color_factor: [f32; 4],
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub texture_index: Option<usize>,
	#[serde(default)]
	pub main_strength_factor: f32,
	#[serde(default = "one_f32")]
	pub normal_strength_factor: f32,
	#[serde(default = "default_liltoon_rim_border")]
	pub border_factor: f32,
	#[serde(default = "default_liltoon_rim_blur")]
	pub blur_factor: f32,
	#[serde(default)]
	pub directivity_factor: f32,
	#[serde(default = "one_f32")]
	pub view_strength_factor: f32,
	#[serde(default = "one_f32")]
	pub receive_shadow_factor: f32,
	#[serde(default = "one_f32")]
	pub backface_mask_factor: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaLilToonLikeGlitter {
	#[serde(default)]
	pub enabled_factor: f32,
	#[serde(default = "one_vec4")]
	pub color_factor: [f32; 4],
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub color_texture_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub shape_texture_index: Option<usize>,
	#[serde(default = "default_liltoon_glitter_params1")]
	pub params1_factor: [f32; 4],
	#[serde(default = "default_liltoon_glitter_params2")]
	pub params2_factor: [f32; 4],
	#[serde(default = "one_vec4")]
	pub atlas_factor: [f32; 4],
	#[serde(default)]
	pub main_strength_factor: f32,
	#[serde(default = "one_f32")]
	pub normal_strength_factor: f32,
	#[serde(default = "one_f32")]
	pub post_contrast_factor: f32,
	#[serde(default = "default_liltoon_glitter_sensitivity")]
	pub sensitivity_factor: f32,
	#[serde(default = "one_f32")]
	pub enable_lighting_factor: f32,
	#[serde(default)]
	pub shadow_mask_factor: f32,
	#[serde(default = "one_f32")]
	pub apply_transparency_factor: f32,
	#[serde(default)]
	pub backface_mask_factor: f32,
	#[serde(default)]
	pub scale_randomize_factor: f32,
	#[serde(default)]
	pub uv_mode_factor: f32,
	#[serde(default)]
	pub color_texture_uv_mode_factor: f32,
	#[serde(default)]
	pub apply_shape_factor: f32,
	#[serde(default)]
	pub angle_randomize_factor: f32,
	#[serde(default)]
	pub vr_parallax_strength_factor: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaLilToonLikeDissolve {
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub mask_texture_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub noise_mask_texture_index: Option<usize>,
	#[serde(default = "default_liltoon_dissolve_color")]
	pub color_factor: [f32; 4],
	#[serde(default = "default_liltoon_dissolve_params")]
	pub params_factor: [f32; 4],
	#[serde(default)]
	pub position_factor: [f32; 4],
	#[serde(default = "default_liltoon_dissolve_noise_strength")]
	pub noise_strength_factor: f32,
	#[serde(default)]
	pub noise_uv_scroll_rotate_factor: [f32; 4],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaLilToonLikeParallax {
	#[serde(default)]
	pub enabled_factor: f32,
	#[serde(default)]
	pub pom_enabled_factor: f32,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub texture_index: Option<usize>,
	#[serde(default = "default_liltoon_parallax_scale")]
	pub scale_factor: f32,
	#[serde(default = "default_liltoon_parallax_offset")]
	pub offset_factor: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaLilToonLikeIdMask {
	#[serde(default)]
	pub compile_factor: f32,
	#[serde(default = "default_liltoon_id_mask_from")]
	pub from_factor: f32,
	#[serde(default)]
	pub is_bitmap_factor: f32,
	#[serde(default)]
	pub controls_dissolve_factor: f32,
	#[serde(default)]
	pub flags_factor: [f32; 8],
	#[serde(default)]
	pub prior_flags_factor: [f32; 8],
	#[serde(default)]
	pub indices_factor: [i32; 8],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaLilToonLikeUdimDiscard {
	#[serde(default)]
	pub compile_factor: f32,
	#[serde(default)]
	pub mode_factor: f32,
	#[serde(default)]
	pub uv_factor: f32,
	#[serde(default)]
	pub row0_factor: [f32; 4],
	#[serde(default)]
	pub row1_factor: [f32; 4],
	#[serde(default)]
	pub row2_factor: [f32; 4],
	#[serde(default)]
	pub row3_factor: [f32; 4],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaLilToonLikeAlphaMask {
	#[serde(default)]
	pub mode_factor: f32,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub texture_index: Option<usize>,
	#[serde(default = "one_f32")]
	pub scale_factor: f32,
	#[serde(default)]
	pub value_factor: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaLilToonLikeFur {
	#[serde(default)]
	pub enabled_factor: f32,
	#[serde(default)]
	pub layer_count_factor: f32,
	#[serde(default)]
	pub vector_factor: [f32; 4],
	#[serde(default)]
	pub vertex_color_to_vector_factor: f32,
	#[serde(default = "one_f32")]
	pub vector_scale_factor: f32,
	#[serde(default)]
	pub gravity_factor: f32,
	#[serde(default)]
	pub shell_ao_factor: f32,
	#[serde(default)]
	pub root_offset_factor: f32,
	#[serde(default = "default_liltoon_fur_cutout_length")]
	pub cutout_length_factor: f32,
	#[serde(default)]
	pub randomize_factor: f32,
	#[serde(default = "one_f32")]
	pub noise_tiling_factor: f32,
	#[serde(default)]
	pub noise_offset_factor: f32,
	#[serde(default = "default_liltoon_fur_rim_color")]
	pub rim_color_factor: [f32; 4],
	#[serde(default = "default_liltoon_fur_rim_fresnel_power")]
	pub rim_fresnel_power_factor: f32,
	#[serde(default = "default_liltoon_fur_rim_anti_light")]
	pub rim_anti_light_factor: f32,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub vector_texture_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub length_mask_texture_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub noise_mask_texture_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub mask_texture_index: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaLilToonLikeBlendState {
	#[serde(default = "one_f32")]
	pub source_factor: f32,
	#[serde(default)]
	pub destination_factor: f32,
	#[serde(default)]
	pub operation_factor: f32,
	#[serde(default = "one_f32")]
	pub alpha_source_factor: f32,
	#[serde(default = "default_liltoon_alpha_destination_factor")]
	pub alpha_destination_factor: f32,
	#[serde(default)]
	pub alpha_operation_factor: f32,
	#[serde(default)]
	pub forward_add_alpha_source_factor: f32,
	#[serde(default = "one_f32")]
	pub forward_add_alpha_destination_factor: f32,
	#[serde(default = "default_liltoon_forward_add_alpha_operation_factor")]
	pub forward_add_alpha_operation_factor: f32,
	#[serde(default = "one_f32")]
	pub alpha_boost_factor: f32,
	#[serde(default = "default_liltoon_subpass_cutoff")]
	pub subpass_cutoff_factor: f32,
	#[serde(default = "default_liltoon_pre_cutoff")]
	pub pre_cutoff_factor: f32,
	#[serde(default = "one_f32")]
	pub pre_zwrite_factor: f32,
	#[serde(default)]
	pub alpha_to_mask_factor: f32,
	/// lilToon `_PreCull` for transparent z-write/subpass rendering: 0 Off, 1 Front, 2 Back.
	#[serde(default = "default_liltoon_precull_factor")]
	pub pre_cull_factor: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaLilToonLikeRendering {
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub render_queue_number: Option<i32>,
	#[serde(default)]
	pub backface_color_factor: [f32; 4],
	#[serde(default = "default_liltoon_distance_fade")]
	pub distance_fade_factor: [f32; 4],
	#[serde(default = "default_liltoon_distance_fade_color")]
	pub distance_fade_color_factor: [f32; 4],
	#[serde(default)]
	pub distance_fade_rim_color_factor: [f32; 4],
	#[serde(default = "default_liltoon_distance_fade_rim_fresnel_power")]
	pub distance_fade_rim_fresnel_power_factor: f32,
	#[serde(default)]
	pub distance_fade_mode_factor: f32,
	#[serde(default = "default_liltoon_light_min_limit")]
	pub light_min_limit_factor: f32,
	#[serde(default = "one_f32")]
	pub light_max_limit_factor: f32,
	#[serde(default)]
	pub monochrome_lighting_factor: f32,
	#[serde(default)]
	pub as_unlit_factor: f32,
	#[serde(default)]
	pub vertex_light_strength_factor: f32,
	#[serde(default = "one_f32")]
	pub aa_strength_factor: f32,
	#[serde(default)]
	pub gsaa_strength_factor: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaLilToonLikeMaterial {
	#[serde(default)]
	pub source_profile: UnaLilToonLikeSourceProfile,
	#[serde(default)]
	pub flip_backface_normal_factor: f32,
	#[serde(default)]
	pub main_color: UnaLilToonLikeMainColor,
	#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
	pub texture_uv_offset_scales: BTreeMap<String, [f32; 4]>,
	#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
	pub texture_uv_mode_factors: BTreeMap<String, f32>,
	#[serde(default)]
	pub rendering: UnaLilToonLikeRendering,
	#[serde(default)]
	pub normal: UnaLilToonLikeNormal,
	#[serde(default)]
	pub shadow: UnaLilToonLikeShadow,
	#[serde(default)]
	pub matcap: UnaLilToonLikeMatcap,
	#[serde(default)]
	pub reflection: UnaLilToonLikeReflection,
	#[serde(default)]
	pub rim: UnaLilToonLikeRim,
	#[serde(default)]
	pub emission: UnaLilToonLikeEmission,
	#[serde(default)]
	pub audio_link: UnaLilToonLikeAudioLink,
	#[serde(default)]
	pub outline: UnaLilToonLikeOutline,
	#[serde(default)]
	pub backlight: UnaLilToonLikeBacklight,
	#[serde(default)]
	pub glitter: UnaLilToonLikeGlitter,
	#[serde(default)]
	pub dissolve: UnaLilToonLikeDissolve,
	#[serde(default)]
	pub parallax: UnaLilToonLikeParallax,
	#[serde(default)]
	pub id_mask: UnaLilToonLikeIdMask,
	#[serde(default)]
	pub udim_discard: UnaLilToonLikeUdimDiscard,
	#[serde(default)]
	pub alpha_mask: UnaLilToonLikeAlphaMask,
	#[serde(default)]
	pub fur: UnaLilToonLikeFur,
	#[serde(default)]
	pub blend_state: UnaLilToonLikeBlendState,
}

impl UnaLilToonLikeMaterial {
	pub fn is_gem_profile(&self) -> bool {
		self.source_profile == UnaLilToonLikeSourceProfile::LiltoonGem
	}

	pub fn is_refraction_profile(&self) -> bool {
		self.source_profile == UnaLilToonLikeSourceProfile::LiltoonRefraction
	}

	pub fn needs_screen_refraction(&self) -> bool {
		self.is_gem_profile() || (self.is_refraction_profile() && self.reflection.gem_refraction_strength_factor.abs() > 0.00001)
	}

	pub fn uses_reflection_source_cube(&self) -> bool {
		self.reflection.cube_override_factor > 0.5 || self.is_gem_profile()
	}
}

impl Default for UnaLilToonLikeShadow {
	fn default() -> Self {
		Self {
			enabled_factor: 1.0,
			color_factor: [0.0, 0.0, 0.0],
			color_texture_index: None,
			strength_mask_texture_index: None,
			border_mask_texture_index: None,
			blur_mask_texture_index: None,
			strength_factor: 1.0,
			border_factor: default_liltoon_shadow_border(),
			blur_factor: default_liltoon_shadow_blur(),
			border_range_factor: 0.0,
			main_strength_factor: default_liltoon_shadow_main_strength(),
			env_strength_factor: default_liltoon_shadow_env_strength(),
			border_color_factor: default_liltoon_shadow_border_color(),
			post_ao_factor: 0.0,
			ao_shift_factor: default_liltoon_shadow_ao_shift(),
			ao_shift2_factor: default_liltoon_shadow_ao_shift2(),
			normal_strength_factor: 1.0,
			receive_factor: 0.0,
			second_color_factor: [0.0, 0.0, 0.0, 0.0],
			second_color_texture_index: None,
			second_border_factor: 0.0,
			second_blur_factor: 0.0,
			second_normal_strength_factor: 1.0,
			second_receive_factor: 0.0,
			third_color_factor: [0.0, 0.0, 0.0, 0.0],
			third_color_texture_index: None,
			third_border_factor: 0.0,
			third_blur_factor: 0.0,
			third_normal_strength_factor: 1.0,
			third_receive_factor: 0.0,
		}
	}
}

impl Default for UnaLilToonLikeMainColor {
	fn default() -> Self {
		Self {
			main_texture_hsvg_factor: default_liltoon_main_texture_hsvg(),
			main_color_adjust_mask_texture_index: None,
			gradation_enabled_factor: 0.0,
			gradation_texture_index: None,
			gradation_strength_factor: 0.0,
			second_enabled_factor: 0.0,
			second_texture_index: None,
			second_blend_mask_texture_index: None,
			second_color_factor: [1.0, 1.0, 1.0, 1.0],
			second_blend_mode: UnaLilToonLikeBlendMode::default(),
			second_enable_lighting_factor: 1.0,
			second_alpha_mode_factor: 0.0,
			second_cull_factor: 0.0,
			second_distance_fade_factor: default_liltoon_layer_distance_fade(),
			second_decal_flags_factor: [0.0, 0.0, 0.0, 0.0],
			second_decal_transform_factor: [0.0, 0.0, 0.0, 0.0],
			second_decal_animation_factor: [0.0, 0.0, 0.0, 0.0],
			second_decal_sub_param_factor: [0.0, 0.0, 0.0, 0.0],
			second_dissolve: UnaLilToonLikeDissolve::default(),
			third_enabled_factor: 0.0,
			third_texture_index: None,
			third_blend_mask_texture_index: None,
			third_color_factor: [1.0, 1.0, 1.0, 1.0],
			third_blend_mode: UnaLilToonLikeBlendMode::default(),
			third_enable_lighting_factor: 1.0,
			third_alpha_mode_factor: 0.0,
			third_cull_factor: 0.0,
			third_distance_fade_factor: default_liltoon_layer_distance_fade(),
			third_decal_flags_factor: [0.0, 0.0, 0.0, 0.0],
			third_decal_transform_factor: [0.0, 0.0, 0.0, 0.0],
			third_decal_animation_factor: [0.0, 0.0, 0.0, 0.0],
			third_decal_sub_param_factor: [0.0, 0.0, 0.0, 0.0],
			third_dissolve: UnaLilToonLikeDissolve::default(),
		}
	}
}

impl Default for UnaLilToonLikeNormal {
	fn default() -> Self {
		Self {
			second_enabled_factor: 0.0,
			second_texture_index: None,
			second_scale_mask_texture_index: None,
			second_scale_factor: 1.0,
		}
	}
}

impl Default for UnaLilToonLikeMatcap {
	fn default() -> Self {
		Self {
			enabled_factor: 0.0,
			color_factor: [1.0, 1.0, 1.0],
			color_alpha_factor: 1.0,
			texture_index: None,
			blend_mask_texture_index: None,
			bump_texture_index: None,
			blend_factor: 1.0,
			main_strength_factor: 0.0,
			enable_lighting_factor: 0.0,
			blend_mode: UnaLilToonLikeBlendMode::default(),
			normal_strength_factor: 1.0,
			custom_normal_factor: 0.0,
			bump_scale_factor: 1.0,
			shadow_mask_factor: 0.0,
			apply_transparency_factor: 1.0,
			lod_factor: 0.0,
			backface_mask_factor: 0.0,
			perspective_factor: 1.0,
			z_rotation_cancel_factor: 1.0,
			vr_parallax_strength_factor: 1.0,
			blend_uv1_factor: [0.0, 0.0],
			second_enabled_factor: 0.0,
			second_texture_index: None,
			second_blend_mask_texture_index: None,
			second_bump_texture_index: None,
			second_color_factor: [1.0, 1.0, 1.0, 1.0],
			second_main_strength_factor: 0.0,
			second_blend_factor: 1.0,
			second_enable_lighting_factor: 1.0,
			second_shadow_mask_factor: 0.0,
			second_apply_transparency_factor: 1.0,
			second_blend_mode: UnaLilToonLikeBlendMode::default(),
			second_normal_strength_factor: 1.0,
			second_custom_normal_factor: 0.0,
			second_bump_scale_factor: 1.0,
			second_lod_factor: 0.0,
			second_backface_mask_factor: 0.0,
			second_perspective_factor: 1.0,
			second_z_rotation_cancel_factor: 1.0,
			second_vr_parallax_strength_factor: 1.0,
			second_blend_uv1_factor: [0.0, 0.0],
		}
	}
}

impl Default for UnaLilToonLikeReflection {
	fn default() -> Self {
		Self {
			enabled_factor: 0.0,
			color_factor: [1.0, 1.0, 1.0, 1.0],
			smoothness_factor: default_liltoon_smoothness(),
			metallic_factor: 0.0,
			metallic_texture_index: None,
			reflectance_factor: default_liltoon_reflectance(),
			apply_specular_factor: 1.0,
			apply_specular_forward_add_factor: 1.0,
			apply_reflection_factor: 1.0,
			apply_transparency_factor: 1.0,
			specular_toon_factor: 1.0,
			specular_border_factor: default_liltoon_specular_border(),
			specular_blur_factor: 0.0,
			specular_normal_strength_factor: 1.0,
			reflection_normal_strength_factor: 1.0,
			cube_enable_lighting_factor: 1.0,
			cube_color_factor: [0.0, 0.0, 0.0, 1.0],
			cube_override_factor: 0.0,
			blend_mode: UnaLilToonLikeBlendMode::default(),
			anisotropy_enabled_factor: 0.0,
			anisotropy_scale_factor: 1.0,
			anisotropy_shift_factor: 0.0,
			anisotropy_shift_noise_scale_factor: 0.0,
			anisotropy_specular_strength_factor: 1.0,
			anisotropy_tangent_width_factor: 1.0,
			anisotropy_bitangent_width_factor: 1.0,
			anisotropy_to_reflection_factor: 0.0,
			anisotropy_to_matcap_factor: 0.0,
			anisotropy_to_second_matcap_factor: 0.0,
			anisotropy_second_shift_factor: 0.0,
			anisotropy_second_shift_noise_scale_factor: 0.0,
			anisotropy_second_specular_strength_factor: 0.0,
			anisotropy_second_tangent_width_factor: 1.0,
			anisotropy_second_bitangent_width_factor: 1.0,
			gem_env_color_factor: [1.0, 1.0, 1.0, 1.0],
			gem_env_contrast_factor: 1.0,
			gem_refraction_fresnel_power_factor: default_liltoon_refraction_fresnel_power(),
			gem_refraction_strength_factor: default_liltoon_gem_refraction_strength(),
			refraction_color_factor: [1.0, 1.0, 1.0, 1.0],
			refraction_color_from_main_factor: 0.0,
			gem_chromatic_aberration_factor: default_liltoon_gem_chromatic_aberration(),
			gem_particle_loop_factor: default_liltoon_gem_particle_loop(),
			gem_particle_color_factor: default_liltoon_gem_particle_color(),
			gem_vr_parallax_strength_factor: 1.0,
			cube_texture_index: None,
			color_texture_index: None,
			smoothness_texture_index: None,
			anisotropy_tangent_texture_index: None,
			anisotropy_scale_mask_texture_index: None,
			anisotropy_shift_noise_mask_texture_index: None,
		}
	}
}

impl Default for UnaLilToonLikeRim {
	fn default() -> Self {
		Self {
			enabled_factor: 0.0,
			color_factor: [1.0, 1.0, 1.0, 1.0],
			texture_index: None,
			main_strength_factor: 0.0,
			border_factor: default_liltoon_rim_border(),
			blur_factor: default_liltoon_rim_blur(),
			fresnel_power_factor: default_liltoon_rim_fresnel_power(),
			enable_lighting_factor: 1.0,
			blend_mode: UnaLilToonLikeBlendMode::default(),
			shadow_mask_factor: 0.0,
			apply_transparency_factor: 1.0,
			normal_strength_factor: 1.0,
			backface_mask_factor: 1.0,
			directional_strength_factor: 0.0,
			directional_range_factor: 0.0,
			indirect_color_factor: [1.0, 1.0, 1.0, 1.0],
			indirect_range_factor: 0.0,
			indirect_border_factor: default_liltoon_rim_border(),
			indirect_blur_factor: default_liltoon_rim_blur(),
			shade_enabled_factor: 0.0,
			shade_color_factor: default_liltoon_rim_shade_color(),
			shade_mask_texture_index: None,
			shade_border_factor: default_liltoon_rim_border(),
			shade_blur_factor: default_liltoon_rim_blur(),
			shade_fresnel_power_factor: default_liltoon_rim_fresnel_power(),
			shade_normal_strength_factor: 1.0,
		}
	}
}

impl Default for UnaLilToonLikeEmission {
	fn default() -> Self {
		Self {
			enabled_factor: 0.0,
			color_factor: [0.0, 0.0, 0.0, 1.0],
			texture_index: None,
			blend_mask_texture_index: None,
			main_strength_factor: 0.0,
			blend_factor: 1.0,
			blend_mode: UnaLilToonLikeBlendMode::default(),
			blink_factor: default_liltoon_emission_blink(),
			fluorescence_factor: 0.0,
			parallax_depth_factor: 0.0,
			uv_scroll_rotate_factor: [0.0, 0.0, 0.0, 0.0],
			blend_mask_uv_scroll_rotate_factor: [0.0, 0.0, 0.0, 0.0],
			gradation_enabled_factor: 0.0,
			gradation_texture_index: None,
			gradation_speed_factor: 1.0,
			second_enabled_factor: 0.0,
			second_color_factor: [0.0, 0.0, 0.0, 0.0],
			second_texture_index: None,
			second_blend_mask_texture_index: None,
			second_blend_factor: 1.0,
			second_blend_mode: UnaLilToonLikeBlendMode::default(),
			second_blink_factor: default_liltoon_emission_blink(),
			second_fluorescence_factor: 0.0,
			second_parallax_depth_factor: 0.0,
			second_uv_scroll_rotate_factor: [0.0, 0.0, 0.0, 0.0],
			second_blend_mask_uv_scroll_rotate_factor: [0.0, 0.0, 0.0, 0.0],
			second_main_strength_factor: 0.0,
			second_gradation_enabled_factor: 0.0,
			second_gradation_texture_index: None,
			second_gradation_speed_factor: 1.0,
		}
	}
}

impl Default for UnaLilToonLikeAudioLink {
	fn default() -> Self {
		Self {
			enabled_factor: 0.0,
			default_value_factor: default_liltoon_audio_link_default_value(),
			uv_mode_factor: 1.0,
			uv_params_factor: default_liltoon_audio_link_uv_params(),
			start_factor: [0.0, 0.0, 0.0, 0.0],
			mask_texture_index: None,
			mask_uv_scroll_rotate_factor: [0.0, 0.0, 0.0, 0.0],
			mask_uv_mode_factor: 0.0,
			to_main_second_factor: 0.0,
			to_main_third_factor: 0.0,
			to_emission_factor: 0.0,
			to_emission_gradation_factor: 0.0,
			to_emission_second_factor: 0.0,
			to_emission_second_gradation_factor: 0.0,
			to_vertex_factor: 0.0,
			vertex_uv_mode_factor: 1.0,
			vertex_uv_params_factor: default_liltoon_audio_link_uv_params(),
			vertex_start_factor: [0.0, 0.0, 0.0, 0.0],
			vertex_strength_factor: default_liltoon_audio_link_vertex_strength(),
			as_local_factor: 0.0,
			local_map_texture_index: None,
			local_map_params_factor: default_liltoon_audio_link_local_map_params(),
		}
	}
}

impl Default for UnaLilToonLikeOutline {
	fn default() -> Self {
		Self {
			enabled_factor: 0.0,
			color_factor: [0.6, 0.56, 0.73, 1.0],
			lit_color_factor: [1.0, 0.2, 0.0, 0.0],
			texture_index: None,
			width_mask_texture_index: None,
			width_factor: 0.0,
			fix_width_factor: default_liltoon_outline_fix_width(),
			enable_lighting_factor: 1.0,
			lit_scale_factor: 10.0,
			lit_offset_factor: -8.0,
			lit_apply_tex_factor: 0.0,
			lit_shadow_receive_factor: 0.0,
			z_bias_factor: 0.0,
		}
	}
}

impl Default for UnaLilToonLikeBacklight {
	fn default() -> Self {
		Self {
			enabled_factor: 0.0,
			color_factor: [0.85, 0.8, 0.7, 1.0],
			texture_index: None,
			main_strength_factor: 0.0,
			normal_strength_factor: 1.0,
			border_factor: default_liltoon_rim_border(),
			blur_factor: default_liltoon_rim_blur(),
			directivity_factor: 5.0,
			view_strength_factor: 1.0,
			receive_shadow_factor: 1.0,
			backface_mask_factor: 1.0,
		}
	}
}

impl Default for UnaLilToonLikeGlitter {
	fn default() -> Self {
		Self {
			enabled_factor: 0.0,
			color_factor: [1.0, 1.0, 1.0, 1.0],
			color_texture_index: None,
			shape_texture_index: None,
			params1_factor: default_liltoon_glitter_params1(),
			params2_factor: default_liltoon_glitter_params2(),
			atlas_factor: [1.0, 1.0, 0.0, 0.0],
			main_strength_factor: 0.0,
			normal_strength_factor: 1.0,
			post_contrast_factor: 1.0,
			sensitivity_factor: default_liltoon_glitter_sensitivity(),
			enable_lighting_factor: 1.0,
			shadow_mask_factor: 0.0,
			apply_transparency_factor: 1.0,
			backface_mask_factor: 0.0,
			scale_randomize_factor: 0.0,
			uv_mode_factor: 0.0,
			color_texture_uv_mode_factor: 0.0,
			apply_shape_factor: 0.0,
			angle_randomize_factor: 0.0,
			vr_parallax_strength_factor: 0.0,
		}
	}
}

impl Default for UnaLilToonLikeAlphaMask {
	fn default() -> Self {
		Self {
			mode_factor: 0.0,
			texture_index: None,
			scale_factor: 1.0,
			value_factor: 0.0,
		}
	}
}

impl Default for UnaLilToonLikeDissolve {
	fn default() -> Self {
		Self {
			mask_texture_index: None,
			noise_mask_texture_index: None,
			color_factor: default_liltoon_dissolve_color(),
			params_factor: default_liltoon_dissolve_params(),
			position_factor: [0.0, 0.0, 0.0, 0.0],
			noise_strength_factor: default_liltoon_dissolve_noise_strength(),
			noise_uv_scroll_rotate_factor: [0.0, 0.0, 0.0, 0.0],
		}
	}
}

impl Default for UnaLilToonLikeParallax {
	fn default() -> Self {
		Self {
			enabled_factor: 0.0,
			pom_enabled_factor: 0.0,
			texture_index: None,
			scale_factor: default_liltoon_parallax_scale(),
			offset_factor: default_liltoon_parallax_offset(),
		}
	}
}

impl Default for UnaLilToonLikeIdMask {
	fn default() -> Self {
		Self {
			compile_factor: 0.0,
			from_factor: default_liltoon_id_mask_from(),
			is_bitmap_factor: 0.0,
			controls_dissolve_factor: 0.0,
			flags_factor: [0.0; 8],
			prior_flags_factor: [0.0; 8],
			indices_factor: [0; 8],
		}
	}
}

impl Default for UnaLilToonLikeUdimDiscard {
	fn default() -> Self {
		Self {
			compile_factor: 0.0,
			mode_factor: 0.0,
			uv_factor: 0.0,
			row0_factor: [0.0; 4],
			row1_factor: [0.0; 4],
			row2_factor: [0.0; 4],
			row3_factor: [0.0; 4],
		}
	}
}

impl Default for UnaLilToonLikeFur {
	fn default() -> Self {
		Self {
			enabled_factor: 0.0,
			layer_count_factor: 0.0,
			vector_factor: [0.0, 0.0, 0.0, 0.0],
			vertex_color_to_vector_factor: 0.0,
			vector_scale_factor: 1.0,
			gravity_factor: 0.0,
			shell_ao_factor: 0.0,
			root_offset_factor: 0.0,
			cutout_length_factor: default_liltoon_fur_cutout_length(),
			randomize_factor: 0.0,
			noise_tiling_factor: 1.0,
			noise_offset_factor: 0.0,
			rim_color_factor: default_liltoon_fur_rim_color(),
			rim_fresnel_power_factor: default_liltoon_fur_rim_fresnel_power(),
			rim_anti_light_factor: default_liltoon_fur_rim_anti_light(),
			vector_texture_index: None,
			length_mask_texture_index: None,
			noise_mask_texture_index: None,
			mask_texture_index: None,
		}
	}
}

impl Default for UnaLilToonLikeBlendState {
	fn default() -> Self {
		Self {
			source_factor: 1.0,
			destination_factor: 0.0,
			operation_factor: 0.0,
			alpha_source_factor: 1.0,
			alpha_destination_factor: default_liltoon_alpha_destination_factor(),
			alpha_operation_factor: 0.0,
			forward_add_alpha_source_factor: 0.0,
			forward_add_alpha_destination_factor: 1.0,
			forward_add_alpha_operation_factor: default_liltoon_forward_add_alpha_operation_factor(),
			alpha_boost_factor: 1.0,
			subpass_cutoff_factor: default_liltoon_subpass_cutoff(),
			pre_cutoff_factor: default_liltoon_pre_cutoff(),
			pre_zwrite_factor: 1.0,
			alpha_to_mask_factor: 0.0,
			pre_cull_factor: default_liltoon_precull_factor(),
		}
	}
}

impl Default for UnaLilToonLikeRendering {
	fn default() -> Self {
		Self {
			render_queue_number: None,
			backface_color_factor: [0.0, 0.0, 0.0, 0.0],
			distance_fade_factor: default_liltoon_distance_fade(),
			distance_fade_color_factor: default_liltoon_distance_fade_color(),
			distance_fade_rim_color_factor: [0.0, 0.0, 0.0, 0.0],
			distance_fade_rim_fresnel_power_factor: default_liltoon_distance_fade_rim_fresnel_power(),
			distance_fade_mode_factor: 0.0,
			light_min_limit_factor: default_liltoon_light_min_limit(),
			light_max_limit_factor: 1.0,
			monochrome_lighting_factor: 0.0,
			as_unlit_factor: 0.0,
			vertex_light_strength_factor: 0.0,
			aa_strength_factor: 1.0,
			gsaa_strength_factor: 0.0,
		}
	}
}

impl Default for UnaLilToonLikeMaterial {
	fn default() -> Self {
		Self {
			source_profile: UnaLilToonLikeSourceProfile::Unknown,
			flip_backface_normal_factor: 0.0,
			main_color: UnaLilToonLikeMainColor::default(),
			texture_uv_offset_scales: BTreeMap::new(),
			texture_uv_mode_factors: BTreeMap::new(),
			rendering: UnaLilToonLikeRendering::default(),
			normal: UnaLilToonLikeNormal::default(),
			shadow: UnaLilToonLikeShadow::default(),
			matcap: UnaLilToonLikeMatcap::default(),
			reflection: UnaLilToonLikeReflection::default(),
			rim: UnaLilToonLikeRim::default(),
			emission: UnaLilToonLikeEmission::default(),
			audio_link: UnaLilToonLikeAudioLink::default(),
			outline: UnaLilToonLikeOutline::default(),
			backlight: UnaLilToonLikeBacklight::default(),
			glitter: UnaLilToonLikeGlitter::default(),
			dissolve: UnaLilToonLikeDissolve::default(),
			parallax: UnaLilToonLikeParallax::default(),
			id_mask: UnaLilToonLikeIdMask::default(),
			udim_discard: UnaLilToonLikeUdimDiscard::default(),
			alpha_mask: UnaLilToonLikeAlphaMask::default(),
			fur: UnaLilToonLikeFur::default(),
			blend_state: UnaLilToonLikeBlendState::default(),
		}
	}
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaMtoonMaterial {
	#[serde(default)]
	pub transparent_with_z_write: bool,
	#[serde(default)]
	pub render_queue_offset_number: i32,
	#[serde(default)]
	pub shade_color_factor: [f32; 3],
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub shade_multiply_texture_index: Option<usize>,
	#[serde(default)]
	pub shading_shift_factor: f32,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub shading_shift_texture_index: Option<usize>,
	#[serde(default = "one_f32")]
	pub shading_shift_texture_scale: f32,
	#[serde(default = "default_mtoon_shading_toony")]
	pub shading_toony_factor: f32,
	#[serde(default = "default_mtoon_gi_equalization")]
	pub gi_equalization_factor: f32,
	#[serde(default = "one_vec3")]
	pub matcap_factor: [f32; 3],
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub matcap_texture_index: Option<usize>,
	#[serde(default)]
	pub parametric_rim_color_factor: [f32; 3],
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub rim_multiply_texture_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub reflection_cube_texture_index: Option<usize>,
	#[serde(default = "one_f32")]
	pub rim_lighting_mix_factor: f32,
	#[serde(default = "default_mtoon_rim_fresnel_power")]
	pub parametric_rim_fresnel_power_factor: f32,
	#[serde(default)]
	pub parametric_rim_lift_factor: f32,
	#[serde(default)]
	pub outline_width_mode: UnaMtoonOutlineWidthMode,
	#[serde(default)]
	pub outline_width_factor: f32,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub outline_width_multiply_texture_index: Option<usize>,
	#[serde(default)]
	pub outline_color_factor: [f32; 3],
	#[serde(default = "one_f32")]
	pub outline_lighting_mix_factor: f32,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub uv_animation_mask_texture_index: Option<usize>,
	/// Base UV transform shared by the current toon compatibility paths:
	/// `[offset_x, offset_y, scale_x, scale_y]`.
	#[serde(default = "default_uv_offset_scale")]
	pub uv_offset_scale: [f32; 4],
	#[serde(default)]
	pub uv_animation_scroll_x_speed_factor: f32,
	#[serde(default)]
	pub uv_animation_scroll_y_speed_factor: f32,
	#[serde(default)]
	pub uv_animation_rotation_speed_factor: f32,
}

impl Default for UnaMtoonMaterial {
	fn default() -> Self {
		Self {
			transparent_with_z_write: false,
			render_queue_offset_number: 0,
			shade_color_factor: [0.0, 0.0, 0.0],
			shade_multiply_texture_index: None,
			shading_shift_factor: 0.0,
			shading_shift_texture_index: None,
			shading_shift_texture_scale: 1.0,
			shading_toony_factor: default_mtoon_shading_toony(),
			gi_equalization_factor: default_mtoon_gi_equalization(),
			matcap_factor: one_vec3(),
			matcap_texture_index: None,
			parametric_rim_color_factor: [0.0, 0.0, 0.0],
			rim_multiply_texture_index: None,
			reflection_cube_texture_index: None,
			rim_lighting_mix_factor: 1.0,
			parametric_rim_fresnel_power_factor: default_mtoon_rim_fresnel_power(),
			parametric_rim_lift_factor: 0.0,
			outline_width_mode: UnaMtoonOutlineWidthMode::None,
			outline_width_factor: 0.0,
			outline_width_multiply_texture_index: None,
			outline_color_factor: [0.0, 0.0, 0.0],
			outline_lighting_mix_factor: 1.0,
			uv_animation_mask_texture_index: None,
			uv_offset_scale: default_uv_offset_scale(),
			uv_animation_scroll_x_speed_factor: 0.0,
			uv_animation_scroll_y_speed_factor: 0.0,
			uv_animation_rotation_speed_factor: 0.0,
		}
	}
}

fn one_f32() -> f32 {
	1.0
}

fn one_vec3() -> [f32; 3] {
	[1.0, 1.0, 1.0]
}

fn one_vec4() -> [f32; 4] {
	[1.0, 1.0, 1.0, 1.0]
}

fn default_uv_offset_scale() -> [f32; 4] {
	[0.0, 0.0, 1.0, 1.0]
}

fn default_mtoon_shading_toony() -> f32 {
	0.9
}

fn default_mtoon_gi_equalization() -> f32 {
	0.9
}

fn default_liltoon_shadow_border() -> f32 {
	0.5
}

fn default_liltoon_shadow_blur() -> f32 {
	0.1
}

fn default_liltoon_shadow_main_strength() -> f32 {
	0.0
}

fn default_liltoon_shadow_env_strength() -> f32 {
	0.0
}

fn default_liltoon_shadow_border_color() -> [f32; 3] {
	[1.0, 0.1, 0.0]
}

fn default_liltoon_shadow_ao_shift() -> [f32; 4] {
	[1.0, 0.0, 1.0, 0.0]
}

fn default_liltoon_shadow_ao_shift2() -> [f32; 4] {
	[1.0, 0.0, 0.0, 0.0]
}

fn default_liltoon_glitter_params1() -> [f32; 4] {
	[256.0, 256.0, 0.16, 50.0]
}

fn default_liltoon_glitter_params2() -> [f32; 4] {
	[0.25, 0.0, 0.0, 0.0]
}

fn default_liltoon_glitter_sensitivity() -> f32 {
	0.25
}

fn default_liltoon_dissolve_color() -> [f32; 4] {
	[1.0, 1.0, 1.0, 1.0]
}

fn default_liltoon_dissolve_params() -> [f32; 4] {
	[0.0, 0.0, 0.5, 0.1]
}

fn default_liltoon_dissolve_noise_strength() -> f32 {
	0.1
}

fn default_liltoon_layer_distance_fade() -> [f32; 4] {
	[0.0, 0.0, 0.0, 0.0]
}

fn default_liltoon_parallax_scale() -> f32 {
	0.02
}

fn default_liltoon_parallax_offset() -> f32 {
	0.5
}

fn default_liltoon_id_mask_from() -> f32 {
	8.0
}

fn default_liltoon_smoothness() -> f32 {
	0.5
}

fn default_liltoon_reflectance() -> f32 {
	0.5
}

fn default_liltoon_refraction_fresnel_power() -> f32 {
	5.0
}

fn default_liltoon_gem_refraction_strength() -> f32 {
	0.5
}

fn default_liltoon_gem_chromatic_aberration() -> f32 {
	0.02
}

fn default_liltoon_gem_particle_loop() -> f32 {
	8.0
}

fn default_liltoon_gem_particle_color() -> [f32; 4] {
	[4.0, 4.0, 4.0, 1.0]
}

fn default_liltoon_specular_border() -> f32 {
	0.5
}

fn default_liltoon_emission_blink() -> [f32; 4] {
	[0.0, 0.0, std::f32::consts::PI, 0.0]
}

fn default_liltoon_audio_link_default_value() -> [f32; 4] {
	[0.0, 0.0, 2.0, 0.75]
}

fn default_liltoon_audio_link_uv_params() -> [f32; 4] {
	[0.25, 0.0, 0.0, 0.125]
}

fn default_liltoon_audio_link_vertex_strength() -> [f32; 4] {
	[0.0, 0.0, 0.0, 1.0]
}

fn default_liltoon_audio_link_local_map_params() -> [f32; 4] {
	[120.0, 1.0, 0.0, 0.0]
}

fn default_liltoon_rim_border() -> f32 {
	0.5
}

fn default_liltoon_rim_blur() -> f32 {
	0.65
}

fn default_liltoon_rim_fresnel_power() -> f32 {
	3.5
}

fn default_liltoon_rim_shade_color() -> [f32; 4] {
	[0.5, 0.5, 0.5, 1.0]
}

fn default_liltoon_outline_fix_width() -> f32 {
	0.5
}

fn default_liltoon_subpass_cutoff() -> f32 {
	0.5
}

fn default_liltoon_pre_cutoff() -> f32 {
	0.5
}

fn default_liltoon_precull_factor() -> f32 {
	2.0
}

fn default_liltoon_alpha_destination_factor() -> f32 {
	10.0
}

fn default_liltoon_forward_add_alpha_operation_factor() -> f32 {
	4.0
}

fn default_liltoon_fur_cutout_length() -> f32 {
	0.8
}

fn default_liltoon_fur_rim_color() -> [f32; 4] {
	[0.0, 0.0, 0.0, 1.0]
}

fn default_liltoon_fur_rim_fresnel_power() -> f32 {
	3.0
}

fn default_liltoon_fur_rim_anti_light() -> f32 {
	0.5
}

fn default_liltoon_main_texture_hsvg() -> [f32; 4] {
	[0.0, 1.0, 1.0, 1.0]
}

fn default_liltoon_light_min_limit() -> f32 {
	0.05
}

fn default_liltoon_distance_fade() -> [f32; 4] {
	[0.1, 0.01, 0.0, 0.0]
}

fn default_liltoon_distance_fade_color() -> [f32; 4] {
	[0.0, 0.0, 0.0, 1.0]
}

fn default_liltoon_distance_fade_rim_fresnel_power() -> f32 {
	5.0
}

fn default_mtoon_rim_fresnel_power() -> f32 {
	5.0
}

fn default_alpha_cutoff() -> f32 {
	0.5
}

impl Default for UnaMaterialPbr {
	fn default() -> Self {
		Self {
			name: None,
			double_sided: false,
			cull_mode: UnaCullMode::Back,
			base_color_factor: [1.0, 1.0, 1.0, 1.0],
			base_color_texture_index: None,
			normal_texture_index: None,
			normal_texture_scale: 1.0,
			occlusion_texture_index: None,
			occlusion_texture_strength: 1.0,
			emissive_factor: [0.0, 0.0, 0.0],
			emissive_texture_index: None,
			metallic_factor: 1.0,
			roughness_factor: 1.0,
			shading: UnaShadingModel::default(),
			alpha_mode: UnaAlphaMode::default(),
			alpha_cutoff: default_alpha_cutoff(),
			uv_offset_scale: default_uv_offset_scale(),
			mtoon: None,
			liltoon_like: None,
			unavatar_material: None,
		}
	}
}

/// Decoded image pixel format as imported before renderer upload normalization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnaImagePixelFormat {
	R8,
	R8G8,
	R8G8B8,
	R8G8B8A8,
	R16,
	R16G16,
	R16G16B16,
	R16G16B16A16,
	R16G16B16Float,
	R16G16B16A16Float,
	R32G32B32Float,
	R32G32B32A32Float,
}

impl UnaImagePixelFormat {
	pub fn bytes_per_pixel(self) -> usize {
		match self {
			UnaImagePixelFormat::R8 => 1,
			UnaImagePixelFormat::R8G8 => 2,
			UnaImagePixelFormat::R8G8B8 => 3,
			UnaImagePixelFormat::R8G8B8A8 => 4,
			UnaImagePixelFormat::R16 => 2,
			UnaImagePixelFormat::R16G16 => 4,
			UnaImagePixelFormat::R16G16B16 => 6,
			UnaImagePixelFormat::R16G16B16A16 => 8,
			UnaImagePixelFormat::R16G16B16Float => 6,
			UnaImagePixelFormat::R16G16B16A16Float => 8,
			UnaImagePixelFormat::R32G32B32Float => 12,
			UnaImagePixelFormat::R32G32B32A32Float => 16,
		}
	}

	pub fn is_rgba8_upload_native(self) -> bool {
		matches!(
			self,
			UnaImagePixelFormat::R8 | UnaImagePixelFormat::R8G8 | UnaImagePixelFormat::R8G8B8 | UnaImagePixelFormat::R8G8B8A8
		)
	}
}

fn default_image_pixel_format() -> UnaImagePixelFormat {
	UnaImagePixelFormat::R8G8B8A8
}

/// Decoded image pixels. `pixel_format` + `pixels` is the single stored representation;
/// RGBA8 compatibility buffers are generated only at processing boundaries.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaImageRgba {
	pub width: u32,
	pub height: u32,
	#[serde(default = "default_image_pixel_format")]
	pub pixel_format: UnaImagePixelFormat,
	#[serde(alias = "rgba")]
	pub pixels: Vec<u8>,
}

impl UnaImageRgba {
	pub fn rgba8_compat_pixels(&self) -> Cow<'_, [u8]> {
		match self.pixel_format {
			UnaImagePixelFormat::R8G8B8A8 => Cow::Borrowed(&self.pixels),
			UnaImagePixelFormat::R8 => {
				let mut rgba = Vec::with_capacity(rgba8_capacity(self.pixels.len(), 1));
				for &r in &self.pixels {
					rgba.extend_from_slice(&[r, r, r, 255]);
				}
				Cow::Owned(rgba)
			}
			UnaImagePixelFormat::R8G8 => {
				let mut rgba = Vec::with_capacity(rgba8_capacity(self.pixels.len(), 2));
				for chunk in self.pixels.chunks_exact(2) {
					rgba.extend_from_slice(&[chunk[0], chunk[0], chunk[0], chunk[1]]);
				}
				Cow::Owned(rgba)
			}
			UnaImagePixelFormat::R8G8B8 => {
				let mut rgba = Vec::with_capacity(rgba8_capacity(self.pixels.len(), 3));
				for chunk in self.pixels.chunks_exact(3) {
					rgba.extend_from_slice(chunk);
					rgba.push(255);
				}
				Cow::Owned(rgba)
			}
			UnaImagePixelFormat::R16 => Cow::Owned(rgba8_from_u16_channels(&self.pixels, 1)),
			UnaImagePixelFormat::R16G16 => Cow::Owned(rgba8_from_u16_channels(&self.pixels, 2)),
			UnaImagePixelFormat::R16G16B16 => Cow::Owned(rgba8_from_u16_channels(&self.pixels, 3)),
			UnaImagePixelFormat::R16G16B16A16 => Cow::Owned(rgba8_from_u16_channels(&self.pixels, 4)),
			UnaImagePixelFormat::R16G16B16Float => Cow::Owned(rgba8_from_f16_channels(&self.pixels, 3)),
			UnaImagePixelFormat::R16G16B16A16Float => Cow::Owned(rgba8_from_f16_channels(&self.pixels, 4)),
			UnaImagePixelFormat::R32G32B32Float => Cow::Owned(rgba8_from_f32_channels(&self.pixels, 3)),
			UnaImagePixelFormat::R32G32B32A32Float => Cow::Owned(rgba8_from_f32_channels(&self.pixels, 4)),
		}
	}
}

fn rgba8_capacity(pixel_bytes: usize, stride: usize) -> usize {
	pixel_bytes / stride.max(1) * 4
}

fn rgba8_from_f16_channels(pixels: &[u8], channels: usize) -> Vec<u8> {
	let stride = channels * 2;
	if stride == 0 {
		return Vec::new();
	}
	let mut rgba = Vec::with_capacity(rgba8_capacity(pixels.len(), stride));
	for chunk in pixels.chunks_exact(stride) {
		let channel = |index: usize| -> f32 {
			if index >= channels {
				return if index == 3 { 1.0 } else { 0.0 };
			}
			let offset = index * 2;
			half::f16::from_bits(u16::from_le_bytes([chunk[offset], chunk[offset + 1]])).to_f32()
		};
		let r = channel(0);
		let g = if channels == 1 { r } else { channel(1) };
		let b = if channels == 1 {
			r
		} else if channels == 2 {
			0.0
		} else {
			channel(2)
		};
		let a = if channels >= 4 { channel(3) } else { 1.0 };
		for value in [r, g, b, a] {
			rgba.push(float_to_u8(value));
		}
	}
	rgba
}

fn rgba8_from_u16_channels(pixels: &[u8], channels: usize) -> Vec<u8> {
	let stride = channels * 2;
	let mut rgba = Vec::with_capacity(rgba8_capacity(pixels.len(), stride));
	for pixel in pixels.chunks_exact(stride) {
		let channel = |index: usize| -> u8 {
			if index >= channels {
				return if index == 3 { 255 } else { 0 };
			}
			let offset = index * 2;
			(u16::from_ne_bytes([pixel[offset], pixel[offset + 1]]) >> 8) as u8
		};
		let r = channel(0);
		let g = if channels == 1 { r } else { channel(1) };
		let b = if channels == 1 {
			r
		} else if channels == 2 {
			0
		} else {
			channel(2)
		};
		let a = if channels >= 4 { channel(3) } else { 255 };
		rgba.extend_from_slice(&[r, g, b, a]);
	}
	rgba
}

fn float_to_u8(value: f32) -> u8 {
	(value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn rgba8_from_f32_channels(pixels: &[u8], channels: usize) -> Vec<u8> {
	let stride = channels * 4;
	let mut rgba = Vec::with_capacity(rgba8_capacity(pixels.len(), stride));
	for pixel in pixels.chunks_exact(stride) {
		let channel = |index: usize| -> u8 {
			if index >= channels {
				return if index == 3 { 255 } else { 0 };
			}
			let offset = index * 4;
			let value = f32::from_ne_bytes([pixel[offset], pixel[offset + 1], pixel[offset + 2], pixel[offset + 3]]);
			float_to_u8(value)
		};
		let r = channel(0);
		let g = channel(1);
		let b = if channels == 2 { 0 } else { channel(2) };
		let a = if channels >= 4 { channel(3) } else { 255 };
		rgba.extend_from_slice(&[r, g, b, a]);
	}
	rgba
}

/// レポート行の重大度（`crate-io-plugin-plan.md` §7.4 の分類）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportSeverity {
	Info,
	Warning,
	Error,
	Fatal,
}

/// 機械可読な 1 行（コード・重大度付き）。`messages` と併置し、段階的に移行する。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportMessage {
	pub severity: ReportSeverity,
	pub text: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub code: Option<String>,
}

impl ReportMessage {
	pub fn info(text: impl Into<String>) -> Self {
		Self {
			severity: ReportSeverity::Info,
			text: text.into(),
			code: None,
		}
	}

	pub fn warning(text: impl Into<String>) -> Self {
		Self {
			severity: ReportSeverity::Warning,
			text: text.into(),
			code: None,
		}
	}
}

/// import 全体の成否（`crate-io-plugin-plan.md` §7.4）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportStatus {
	#[default]
	Success,
	PartialSuccess,
	Failed,
}

/// 拡張ブロブとして保持した範囲（§7.4・フィールドは段階的に増やす）。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreservedExtension {
	pub id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub note: Option<String>,
}

/// 近似変換の記録（§7.4）。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Approximation {
	pub feature: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub detail: Option<String>,
}

/// 失われた機能の記録（§7.4）。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LostFeature {
	pub feature: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub detail: Option<String>,
}

/// import の集計レポート（§7.4。`messages` / `diagnostics` は運用上併用）。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImportReport {
	/// 人間向けフラットメッセージ（ログ・後方互換用）。
	pub messages: Vec<String>,
	/// 構造化診断（JSON レポート・ツール連携用）。
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub diagnostics: Vec<ReportMessage>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub source_format: Option<FormatId>,
	#[serde(default, skip_serializing_if = "is_default_report_status")]
	pub status: ReportStatus,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub preserved_extensions: Vec<PreservedExtension>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub approximations: Vec<Approximation>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub lost_features: Vec<LostFeature>,
}

fn is_default_report_status(s: &ReportStatus) -> bool {
	matches!(s, ReportStatus::Success)
}

impl ImportReport {
	pub fn push_info(&mut self, text: impl Into<String>) {
		let t = text.into();
		self.messages.push(t.clone());
		self.diagnostics.push(ReportMessage::info(t));
	}

	pub fn push_warning(&mut self, text: impl Into<String>) {
		let t = text.into();
		self.messages.push(t.clone());
		self.diagnostics.push(ReportMessage::warning(t));
	}
}

/// export の集計レポート。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportReport {
	pub messages: Vec<String>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub diagnostics: Vec<ReportMessage>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub target_format: Option<FormatId>,
	#[serde(default, skip_serializing_if = "is_default_report_status")]
	pub status: ReportStatus,
}

impl ExportReport {
	pub fn push_info(&mut self, text: impl Into<String>) {
		let t = text.into();
		self.messages.push(t.clone());
		self.diagnostics.push(ReportMessage::info(t));
	}
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
	use super::*;

	#[test]
	fn scene_asset_group_ownership_counts_grouped_assets() {
		let scene = UnaSceneSnapshot {
			asset_group_ownership: vec![
				UnaSceneAssetGroupOwnership {
					group_id: "avatar:base".to_string(),
					mesh_primitives: vec![UnaMeshPrimitiveKey {
						mesh_index: 0,
						primitive_index: 0,
					}],
					materials: vec![0],
					images: vec![0, 1],
					dynamics_source_ids: Vec::new(),
				},
				UnaSceneAssetGroupOwnership {
					group_id: "physics:hair".to_string(),
					dynamics_source_ids: vec!["physbone:hair".to_string()],
					..Default::default()
				},
			],
			..Default::default()
		};

		assert_eq!(
			scene.asset_group_ownership_counts(),
			UnaSceneAssetGroupOwnershipCounts {
				groups: 2,
				mesh_primitives: 1,
				materials: 1,
				images: 2,
				dynamics: 1,
			}
		);
	}

	#[test]
	fn scene_scoped_asset_selection_lists_active_group_assets() {
		let scene = UnaSceneSnapshot {
			asset_group_ownership: vec![
				UnaSceneAssetGroupOwnership {
					group_id: "outfit:coat".to_string(),
					mesh_primitives: vec![
						UnaMeshPrimitiveKey {
							mesh_index: 2,
							primitive_index: 1,
						},
						UnaMeshPrimitiveKey {
							mesh_index: 2,
							primitive_index: 1,
						},
					],
					materials: vec![5, 3, 3],
					images: vec![7, 4, 7],
					dynamics_source_ids: vec!["physbone:coat".to_string()],
				},
				UnaSceneAssetGroupOwnership {
					group_id: "avatar:base".to_string(),
					mesh_primitives: vec![UnaMeshPrimitiveKey {
						mesh_index: 0,
						primitive_index: 0,
					}],
					materials: vec![0],
					images: vec![0],
					dynamics_source_ids: vec!["spring:base".to_string()],
				},
			],
			..Default::default()
		};

		let selection = scene.scoped_asset_selection(&["outfit:coat".to_string(), "missing:hat".to_string()]);
		assert_eq!(selection.owned_active_groups, vec!["outfit:coat".to_string()]);
		assert_eq!(selection.missing_active_asset_groups, vec!["missing:hat".to_string()]);
		assert_eq!(
			selection.mesh_primitives,
			vec![UnaMeshPrimitiveKey {
				mesh_index: 2,
				primitive_index: 1,
			}]
		);
		assert_eq!(selection.materials, vec![3, 5]);
		assert_eq!(selection.images, vec![4, 7]);
		assert_eq!(selection.dynamics_source_ids, vec!["physbone:coat".to_string()]);
	}

	#[test]
	fn document_scoped_asset_selection_reports_missing_groups_without_scene() {
		let mut document = UnaDocument::default();
		document
			.runtime_model_mut()
			.set_active_asset_groups(vec!["outfit:coat".to_string(), "texture:red".to_string()]);

		let selection = document.scoped_asset_selection();
		assert!(selection.owned_active_groups.is_empty());
		assert_eq!(
			selection.missing_active_asset_groups,
			vec!["outfit:coat".to_string(), "texture:red".to_string()]
		);
		assert!(selection.mesh_primitives.is_empty());
		assert!(selection.materials.is_empty());
		assert!(selection.images.is_empty());
		assert!(selection.dynamics_source_ids.is_empty());
	}

	#[test]
	fn morph_weights_merge_expression_preset() {
		let mesh = UnaMeshBuffers {
			name: None,
			positions: vec![[0.0; 3]],
			normals: None,
			tangents: None,
			tex_coords_0: None,
			tex_coords_1: None,
			tex_coords_2: None,
			tex_coords_3: None,
			colors_0: None,
			joints: None,
			weights: None,
			indices: None,
			material_index: None,
			morph_targets: vec![UnaMorphTargetDeltas {
				position_deltas: vec![[0.0; 3]],
				normal_deltas: None,
			}],
			morph_target_names: vec![],
			default_morph_weights: vec![0.0],
		};
		let cat = UnaExpressionCatalog {
			presets: vec![UnaExpressionPreset {
				name: "smile".into(),
				binds: vec![UnaMorphTargetBind {
					mesh_index: 0,
					primitive_index: 0,
					morph_target_index: 0,
					weight_scale: 1.0,
				}],
			}],
		};
		let mut ew = UnaExpressionWeights::default();
		ew.preset_weights.insert("smile".into(), 0.5);
		let w = morph_weights_for_primitive(&mesh, Some(&cat), Some(&ew), 0, 0);
		assert_eq!(w.len(), 1);
		assert!((w[0] - 0.5).abs() < 1e-6);
	}

	#[test]
	fn morph_weights_clamp() {
		let mesh = UnaMeshBuffers {
			name: None,
			positions: vec![[0.0; 3]],
			normals: None,
			tangents: None,
			tex_coords_0: None,
			tex_coords_1: None,
			tex_coords_2: None,
			tex_coords_3: None,
			colors_0: None,
			joints: None,
			weights: None,
			indices: None,
			material_index: None,
			morph_targets: vec![UnaMorphTargetDeltas {
				position_deltas: vec![[0.0; 3]],
				normal_deltas: None,
			}],
			morph_target_names: vec![],
			default_morph_weights: vec![0.8],
		};
		let cat = UnaExpressionCatalog {
			presets: vec![UnaExpressionPreset {
				name: "x".into(),
				binds: vec![UnaMorphTargetBind {
					mesh_index: 0,
					primitive_index: 0,
					morph_target_index: 0,
					weight_scale: 1.0,
				}],
			}],
		};
		let mut ew = UnaExpressionWeights::default();
		ew.preset_weights.insert("x".into(), 0.5);
		let w = morph_weights_for_primitive(&mesh, Some(&cat), Some(&ew), 0, 0);
		assert!((w[0] - 1.0).abs() < 1e-6);
	}

	fn test_node(children: Vec<usize>) -> UnaSceneNode {
		UnaSceneNode {
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

	fn test_translation_node(x: f32, y: f32, z: f32) -> UnaSceneNode {
		let mut node = test_node(Vec::new());
		node.transform[12] = x;
		node.transform[13] = y;
		node.transform[14] = z;
		node
	}

	#[test]
	fn resolved_roots_borrows_explicit_roots() {
		let roots = vec![2usize];
		let scene = UnaSceneSnapshot {
			roots,
			..Default::default()
		};
		let resolved = scene.resolved_roots();

		assert!(matches!(resolved, Cow::Borrowed(_)));
		assert_eq!(&*resolved, &[2]);
	}

	#[test]
	fn resolved_roots_falls_back_to_parentless_nodes() {
		let scene = UnaSceneSnapshot {
			nodes: vec![test_node(vec![1]), test_node(Vec::new()), test_node(Vec::new())],
			roots: Vec::new(),
			..Default::default()
		};

		assert_eq!(&*scene.resolved_roots(), &[0, 2]);
	}

	#[test]
	fn runtime_model_reports_unavatar_source() {
		let document = UnaDocument {
			unavatar: Some(UnaUnavatarExtension {
				spec_version: "2.0".to_string(),
				source: serde_json::json!({}),
			}),
			..Default::default()
		};

		assert_eq!(document.runtime_model().source_kind(), UnaRuntimeSourceKind::Unavatar);
		assert_eq!(document.runtime_model().humanoid_basis(), UnaHumanoidRuntimeBasis::UnavatarUnity);
	}

	#[test]
	fn runtime_model_reports_vrm_source_version() {
		let mut document = UnaDocument {
			vrm: Some(UnaVrmExtension {
				spec_version: "0.0".to_string(),
				meta: serde_json::json!({}),
				humanoid_bones: BTreeMap::new(),
				mtoon_materials_v0: Vec::new(),
				mtoon_material_indices_v1: Vec::new(),
				source: serde_json::json!({}),
			}),
			..Default::default()
		};

		assert!(document.vrm.as_ref().unwrap().is_vrm0());
		assert_eq!(document.runtime_model().source_kind(), UnaRuntimeSourceKind::Vrm0);
		assert_eq!(document.runtime_model().humanoid_basis(), UnaHumanoidRuntimeBasis::Vrm0);
		document.vrm.as_mut().unwrap().spec_version = "1.0".to_string();
		assert!(!document.vrm.as_ref().unwrap().is_vrm0());
		assert_eq!(document.runtime_model().source_kind(), UnaRuntimeSourceKind::Vrm1);
		assert_eq!(document.runtime_model().humanoid_basis(), UnaHumanoidRuntimeBasis::Vrm1);
	}

	#[test]
	fn runtime_model_defaults_to_vrm0_humanoid_basis_for_gltf_like_sources() {
		let document = UnaDocument::default();

		assert_eq!(document.runtime_model().source_kind(), UnaRuntimeSourceKind::GltfLike);
		assert_eq!(document.runtime_model().humanoid_basis(), UnaHumanoidRuntimeBasis::Vrm0);
	}

	#[test]
	fn runtime_model_reports_humanoid_scene_only_when_both_exist() {
		let mut document = UnaDocument::default();
		assert!(document.runtime_model().scene_nodes().is_none());
		assert!(!document.runtime_model().has_humanoid_scene());
		assert!(document.runtime_model().humanoid_retarget_inputs().profile.is_none());
		assert!(document.runtime_model().humanoid_retarget_inputs().scene.is_none());

		document.scene = Some(UnaSceneSnapshot::default());
		assert!(document.runtime_model().scene_nodes().is_some());
		assert!(!document.runtime_model().has_humanoid_scene());
		assert!(document.runtime_model().humanoid_retarget_inputs().profile.is_none());
		assert!(document.runtime_model().humanoid_retarget_inputs().scene.is_some());

		document.humanoid_profile = Some(HumanoidProfile::default());
		document.expression_weights = Some(UnaExpressionWeights::default());
		assert!(document.runtime_model().has_humanoid_scene());
		assert!(document.runtime_model().expression_weights().is_some());
		let retarget_inputs = document.runtime_model().humanoid_retarget_inputs();
		assert_eq!(retarget_inputs.humanoid_basis, UnaHumanoidRuntimeBasis::Vrm0);
		assert!(retarget_inputs.profile.is_some());
		assert!(retarget_inputs.scene.is_some());
		let scene_expressions = document.runtime_model().scene_expression_catalog().unwrap();
		assert!(scene_expressions.expression_catalog.is_none());
		let scene_dynamics = document.runtime_model().scene_profile_dynamics().unwrap();
		assert!(scene_dynamics.humanoid_profile.is_some());
		assert_eq!(scene_dynamics.dynamics.group_count(), 0);
	}

	#[test]
	fn runtime_model_mut_exposes_mutable_runtime_scene_views() {
		let mut document = UnaDocument {
			scene: Some(UnaSceneSnapshot::default()),
			humanoid_profile: Some(HumanoidProfile::default()),
			spring_bones: Some(UnaSpringBoneSettings {
				groups: vec![UnaSpringBoneGroup::default()],
				colliders: Vec::new(),
				..Default::default()
			}),
			..Default::default()
		};

		{
			let mut runtime_model = document.runtime_model_mut();
			let (scene, profile) = runtime_model.humanoid_scene_mut().unwrap();
			scene.roots.push(7);
			assert!(profile.bone_node_indices.is_empty());
			runtime_model
				.expression_weights_mut()
				.preset_weights
				.insert("Blink".to_string(), 0.5);
		}
		{
			let runtime = document.runtime_scene_and_dynamics_mut().unwrap();
			assert_eq!(runtime.scene.roots, vec![7]);
			assert_eq!(runtime.dynamics.as_readonly().group_count(), 1);
		}
		assert_eq!(
			document
				.expression_weights
				.as_ref()
				.and_then(|weights| weights.preset_weights.get("Blink").copied()),
			Some(0.5)
		);
	}

	#[test]
	fn runtime_model_mut_sets_node_visibility_by_runtime_target() {
		let mut document = UnaDocument {
			scene: Some(UnaSceneSnapshot {
				nodes: vec![
					UnaSceneNode {
						name: Some("Root".to_string()),
						children: vec![1],
						..test_node(Vec::new())
					},
					UnaSceneNode {
						name: Some("Child".to_string()),
						source_node_id: Some("node_child".to_string()),
						resolved_node_id: Some("runtime:child".to_string()),
						..test_node(Vec::new())
					},
					UnaSceneNode {
						name: Some("Fallback".to_string()),
						source_node_id: Some("node_fallback".to_string()),
						resolved_node_id: None,
						..test_node(Vec::new())
					},
				],
				roots: vec![0, 2],
				..Default::default()
			}),
			..Default::default()
		};

		assert!(document.runtime_model_mut().set_node_visible(
			&UnaRuntimeNodeTarget {
				node_index: Some(2),
				source_node_id: Some("node_child".to_string()),
				resolved_node_id: None,
				path: None,
			},
			false,
		));
		assert!(!document.scene.as_ref().unwrap().nodes[1].visible);
		assert!(document.scene.as_ref().unwrap().nodes[2].visible);

		assert!(document.runtime_model_mut().set_node_visible(
			&UnaRuntimeNodeTarget {
				node_index: None,
				source_node_id: None,
				resolved_node_id: Some("runtime:child".to_string()),
				path: None,
			},
			false,
		));
		assert!(!document.scene.as_ref().unwrap().nodes[1].visible);

		assert!(document.runtime_model_mut().set_node_visible(
			&UnaRuntimeNodeTarget {
				node_index: None,
				source_node_id: None,
				resolved_node_id: None,
				path: Some("Root/Child".to_string()),
			},
			true,
		));
		assert!(document.scene.as_ref().unwrap().nodes[1].visible);

		assert!(document.runtime_model_mut().set_node_visible(
			&UnaRuntimeNodeTarget {
				node_index: Some(2),
				source_node_id: None,
				resolved_node_id: None,
				path: Some("missing".to_string()),
			},
			false,
		));
		assert!(!document.scene.as_ref().unwrap().nodes[2].visible);
		assert_eq!(
			document.runtime_model().node_visible(&UnaRuntimeNodeTarget {
				node_index: None,
				source_node_id: Some("node_fallback".to_string()),
				resolved_node_id: None,
				path: None,
			}),
			Some(false)
		);

		assert!(!document.runtime_model_mut().set_node_visible(
			&UnaRuntimeNodeTarget {
				node_index: Some(99),
				source_node_id: None,
				resolved_node_id: None,
				path: None,
			},
			true,
		));
	}

	#[test]
	fn runtime_model_mut_applies_basic_material_overrides() {
		let mut document = UnaDocument {
			scene: Some(UnaSceneSnapshot {
				materials: vec![
					UnaMaterialPbr {
						name: Some("Body".to_string()),
						..Default::default()
					},
					UnaMaterialPbr {
						name: Some("Accent".to_string()),
						..Default::default()
					},
				],
				..Default::default()
			}),
			..Default::default()
		};

		document
			.runtime_model_mut()
			.set_material_color(
				&UnaRuntimeMaterialTarget {
					material_index: Some(0),
					name: Some("Accent".to_string()),
				},
				"_Color",
				[1.2, 0.5, -0.1, 0.25],
			)
			.unwrap();
		assert_eq!(
			document.scene.as_ref().unwrap().materials[0].base_color_factor,
			[1.0, 0.5, 0.0, 0.25]
		);
		assert_eq!(
			document.scene.as_ref().unwrap().materials[1].base_color_factor,
			[1.0, 1.0, 1.0, 1.0]
		);

		document
			.runtime_model_mut()
			.set_material_color(
				&UnaRuntimeMaterialTarget {
					material_index: None,
					name: Some("Accent".to_string()),
				},
				"_EmissionColor",
				[2.0, 1.0, -1.0, 0.5],
			)
			.unwrap();
		assert_eq!(document.scene.as_ref().unwrap().materials[1].emissive_factor, [2.0, 1.0, 0.0]);
		assert_eq!(
			document.runtime_model().material_color(
				&UnaRuntimeMaterialTarget {
					material_index: None,
					name: Some("Accent".to_string()),
				},
				"_EmissionColor",
			),
			Some([2.0, 1.0, 0.0, 1.0])
		);

		document
			.runtime_model_mut()
			.set_material_scalar(
				&UnaRuntimeMaterialTarget {
					material_index: None,
					name: Some("Accent".to_string()),
				},
				"_Smoothness",
				0.75,
			)
			.unwrap();
		assert_eq!(document.scene.as_ref().unwrap().materials[1].roughness_factor, 0.25);
		assert_eq!(
			document.runtime_model().material_scalar(
				&UnaRuntimeMaterialTarget {
					material_index: None,
					name: Some("Accent".to_string()),
				},
				"_Smoothness",
			),
			Some(0.75)
		);

		let err = document
			.runtime_model_mut()
			.set_material_scalar(
				&UnaRuntimeMaterialTarget {
					material_index: None,
					name: Some("Accent".to_string()),
				},
				"_Unsupported",
				1.0,
			)
			.expect_err("unsupported material scalar should fail");
		assert!(err.contains("not supported"));
	}

	#[test]
	fn runtime_model_mut_replaces_material_slots() {
		let primitive = UnaMeshBuffers {
			name: None,
			positions: vec![[0.0; 3]],
			normals: None,
			tangents: None,
			tex_coords_0: None,
			tex_coords_1: None,
			tex_coords_2: None,
			tex_coords_3: None,
			colors_0: None,
			joints: None,
			weights: None,
			indices: None,
			material_index: Some(0),
			morph_targets: Vec::new(),
			morph_target_names: Vec::new(),
			default_morph_weights: Vec::new(),
		};
		let mut document = UnaDocument {
			scene: Some(UnaSceneSnapshot {
				meshes: vec![vec![primitive.clone(), primitive]],
				materials: vec![
					UnaMaterialPbr {
						name: Some("Base".to_string()),
						..Default::default()
					},
					UnaMaterialPbr {
						name: Some("Alt".to_string()),
						..Default::default()
					},
				],
				nodes: vec![
					UnaSceneNode {
						name: Some("Root".to_string()),
						source_node_id: None,
						resolved_node_id: None,
						visible: true,
						transform: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
						children: vec![1],
						mesh: None,
						skin: None,
						probe_anchor_node: None,
						local_bounds: None,
					},
					UnaSceneNode {
						name: Some("Renderer".to_string()),
						source_node_id: Some("node_renderer".to_string()),
						resolved_node_id: None,
						visible: true,
						transform: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
						children: Vec::new(),
						mesh: Some(0),
						skin: None,
						probe_anchor_node: None,
						local_bounds: None,
					},
				],
				roots: vec![0],
				..Default::default()
			}),
			..Default::default()
		};

		document
			.runtime_model_mut()
			.set_material_slot(
				&UnaRuntimeMaterialSlotTarget {
					node: UnaRuntimeNodeTarget {
						node_index: None,
						source_node_id: Some("node_renderer".to_string()),
						resolved_node_id: None,
						path: None,
					},
					primitive_index: Some(1),
				},
				Some(&UnaRuntimeMaterialTarget {
					material_index: None,
					name: Some("Alt".to_string()),
				}),
			)
			.unwrap();
		assert_eq!(document.scene.as_ref().unwrap().meshes[0][0].material_index, Some(0));
		assert_eq!(document.scene.as_ref().unwrap().meshes[0][1].material_index, Some(1));

		document
			.runtime_model_mut()
			.set_material_slot(
				&UnaRuntimeMaterialSlotTarget {
					node: UnaRuntimeNodeTarget {
						node_index: None,
						source_node_id: None,
						resolved_node_id: None,
						path: Some("Root/Renderer".to_string()),
					},
					primitive_index: None,
				},
				Some(&UnaRuntimeMaterialTarget {
					material_index: Some(0),
					name: None,
				}),
			)
			.unwrap();
		assert_eq!(document.scene.as_ref().unwrap().meshes[0][0].material_index, Some(0));
		assert_eq!(
			document.runtime_model().material_slot(&UnaRuntimeMaterialSlotTarget {
				node: UnaRuntimeNodeTarget {
					node_index: None,
					source_node_id: None,
					resolved_node_id: None,
					path: Some("Root/Renderer".to_string()),
				},
				primitive_index: None,
			}),
			Some(Some(0))
		);

		document
			.runtime_model_mut()
			.set_material_slot(
				&UnaRuntimeMaterialSlotTarget {
					node: UnaRuntimeNodeTarget {
						node_index: None,
						source_node_id: None,
						resolved_node_id: None,
						path: Some("Root/Renderer".to_string()),
					},
					primitive_index: Some(1),
				},
				None,
			)
			.unwrap();
		assert_eq!(document.scene.as_ref().unwrap().meshes[0][1].material_index, None);
	}

	#[test]
	fn runtime_dynamics_mut_sets_runtime_enabled_overrides_by_source_id() {
		let mut document = UnaDocument {
			scene: Some(UnaSceneSnapshot::default()),
			spring_bones: Some(UnaSpringBoneSettings {
				groups: vec![
					UnaSpringBoneGroup {
						source_id: "physbone:hair".to_string(),
						enabled: true,
						..Default::default()
					},
					UnaSpringBoneGroup {
						source_id: "physbone:hair".to_string(),
						enabled: true,
						..Default::default()
					},
					UnaSpringBoneGroup {
						source_id: "physbone:tail".to_string(),
						enabled: true,
						..Default::default()
					},
				],
				colliders: Vec::new(),
				..Default::default()
			}),
			..Default::default()
		};

		{
			let mut runtime = document.runtime_scene_and_dynamics_mut().unwrap();
			assert!(runtime.dynamics.set_group_enabled_by_source_id("physbone:hair", false));
			assert!(!runtime.dynamics.set_group_enabled_by_source_id("physbone:missing", false));
			let readonly = runtime.dynamics.as_readonly();
			let groups = readonly.groups();
			assert!(!readonly.group_enabled(&groups[0]));
			assert!(!readonly.group_enabled(&groups[1]));
			assert!(readonly.group_enabled(&groups[2]));
			assert!(groups.iter().all(|group| group.enabled));
			let counts = readonly.counts();
			assert_eq!(counts.enabled_groups, 1);
			assert_eq!(counts.source_enabled_groups, 3);
			assert_eq!(counts.runtime_enabled_overrides, 1);
		}
		assert_eq!(
			document.runtime_state.dynamics_enabled_overrides,
			BTreeMap::from([("physbone:hair".to_string(), false)])
		);
		assert_eq!(document.runtime_model().dynamics_enabled("physbone:hair"), Some(false));
		assert_eq!(document.runtime_model().dynamics_enabled("physbone:tail"), Some(true));
		assert_eq!(document.runtime_model().dynamics_enabled("physbone:missing"), None);
	}

	#[test]
	fn runtime_dynamics_reset_clears_runtime_enabled_overrides() {
		let mut document = UnaDocument {
			scene: Some(UnaSceneSnapshot::default()),
			spring_bones: Some(UnaSpringBoneSettings {
				groups: vec![
					UnaSpringBoneGroup {
						source_kind: UnaDynamicsSourceKind::VrcPhysBone,
						source_id: "physbone:hair".to_string(),
						enabled: false,
						..Default::default()
					},
					UnaSpringBoneGroup {
						source_kind: UnaDynamicsSourceKind::VrmSpringBone,
						source_id: "spring:tail".to_string(),
						enabled: true,
						..Default::default()
					},
				],
				colliders: Vec::new(),
				..Default::default()
			}),
			..Default::default()
		};

		{
			let mut runtime = document.runtime_scene_and_dynamics_mut().unwrap();
			assert!(runtime.dynamics.set_group_enabled_by_source_id("physbone:hair", true));
			assert!(runtime.dynamics.set_group_enabled_by_source_id("spring:tail", false));
			runtime.dynamics.reset_enabled();
		}

		let dynamics = document.runtime_model().dynamics();
		let groups = dynamics.groups();
		assert!(!dynamics.group_enabled(&groups[0]));
		assert!(dynamics.group_enabled(&groups[1]));
		assert_eq!(dynamics.counts().runtime_enabled_overrides, 0);
		assert!(document.runtime_state.dynamics_enabled_overrides.is_empty());
	}

	#[test]
	fn runtime_dynamics_exposes_source_neutral_group_views() {
		let mut document = UnaDocument {
			scene: Some(UnaSceneSnapshot::default()),
			spring_bones: Some(UnaSpringBoneSettings {
				groups: vec![UnaSpringBoneGroup {
					source_kind: UnaDynamicsSourceKind::VrcPhysBone,
					source_id: "physbone:hair".to_string(),
					comment: "Hair".to_string(),
					category: "hair".to_string(),
					enabled: false,
					stiffness: 0.7,
					gravity_power: 0.2,
					gravity_dir: [0.0, -1.0, 0.0],
					drag_force: 0.3,
					center_node: Some(10),
					hit_radius: 0.04,
					limit: Some(UnaDynamicsLimit {
						limit_type: "angle".to_string(),
						max_angle_x: 45.0,
						max_angle_z: 25.0,
						max_stretch: 0.1,
					}),
					interaction: Some(UnaDynamicsInteraction {
						allow_grabbing: Some(true),
						allow_posing: Some(false),
					}),
					bone_node_indices: vec![1, 2, 3],
				}],
				colliders: Vec::new(),
				..Default::default()
			}),
			..Default::default()
		};
		document
			.runtime_state
			.dynamics_enabled_overrides
			.insert("physbone:hair".to_string(), true);

		let dynamics = document.runtime_model().dynamics();
		let groups = dynamics.dynamics_groups().collect::<Vec<_>>();
		assert_eq!(groups.len(), 1);
		let group = groups[0];
		assert_eq!(group.source_kind, UnaDynamicsSourceKind::VrcPhysBone);
		assert!(!group.authored_enabled);
		assert!(group.effective_enabled);
		assert_eq!(group.source_id, "physbone:hair");
		assert_eq!(group.comment, "Hair");
		assert_eq!(group.category, "hair");
		assert_eq!(group.parameters.stiffness, 0.7);
		assert_eq!(group.parameters.gravity_power, 0.2);
		assert_eq!(group.parameters.drag_force, 0.3);
		assert_eq!(group.parameters.center_node, Some(10));
		assert_eq!(group.parameters.hit_radius, 0.04);
		assert_eq!(group.chain.bone_node_indices, &[1, 2, 3]);
		assert_eq!(group.limit.unwrap().limit_type, "angle");
		assert_eq!(group.interaction.unwrap().allow_grabbing, Some(true));
		let counts = dynamics.counts();
		assert_eq!(counts.limit_groups, 1);
		assert_eq!(counts.angle_limit_groups, 1);
		assert_eq!(counts.stretch_limit_groups, 1);
		assert_eq!(counts.grabbing_enabled_groups, 1);
		assert_eq!(counts.posing_enabled_groups, 0);
	}

	#[test]
	fn runtime_dynamics_exposes_contact_and_constraint_metadata() {
		let settings = UnaSpringBoneSettings {
			groups: Vec::new(),
			colliders: Vec::new(),
			contacts: vec![
				UnaDynamicsContact {
					source_kind: UnaDynamicsSourceKind::VrcPhysBone,
					source_id: "contact:hand".to_string(),
					node: 3,
					kind: UnaDynamicsContactKind::Receiver,
					parameter: "ContactHand".to_string(),
					collision_tags: vec!["Hand".to_string(), "Interact".to_string()],
					shape: UnaDynamicsColliderShape::Sphere,
					radius: 0.05,
					..Default::default()
				},
				UnaDynamicsContact {
					source_kind: UnaDynamicsSourceKind::VrcPhysBone,
					source_id: "contact:sender".to_string(),
					node: 4,
					kind: UnaDynamicsContactKind::Sender,
					collision_tags: vec!["Interact".to_string()],
					..Default::default()
				},
			],
			constraint_refs: vec![UnaDynamicsConstraintRef {
				source_kind: UnaDynamicsSourceKind::VrcPhysBone,
				source_id: "constraint:world-fixed".to_string(),
				target_node: 8,
				source_nodes: vec![1, 2],
				constraint_type: "parent".to_string(),
				weight: 0.75,
			}],
		};

		let dynamics = settings.runtime_dynamics();
		let contacts = dynamics.contacts().collect::<Vec<_>>();
		assert_eq!(contacts.len(), 2);
		assert_eq!(contacts[0].parameter, "ContactHand");
		assert_eq!(contacts[0].collision_tags, vec!["Hand", "Interact"]);
		let constraint_refs = dynamics.constraint_refs().collect::<Vec<_>>();
		assert_eq!(constraint_refs.len(), 1);
		assert_eq!(constraint_refs[0].constraint_type, "parent");
		assert_eq!(constraint_refs[0].source_nodes, vec![1, 2]);
		let counts = dynamics.counts();
		assert_eq!(counts.contacts, 2);
		assert_eq!(counts.vrc_contact_receivers, 1);
		assert_eq!(counts.vrc_contact_senders, 1);
		assert_eq!(counts.contact_parameter_declarations, 1);
		assert_eq!(counts.constraint_refs, 1);
		assert_eq!(counts.vrc_constraint_refs, 1);
		let contact_parameters = dynamics.contact_parameter_declarations();
		assert_eq!(contact_parameters.len(), 1);
		assert_eq!(contact_parameters[0].owner_key, "contact:hand");
		assert_eq!(contact_parameters[0].source_id, "contact:hand");
		assert_eq!(contact_parameters[0].node, 3);
		assert_eq!(contact_parameters[0].parameter, "ContactHand");
		assert_eq!(contact_parameters[0].collision_tags, vec!["Hand", "Interact"]);
	}

	#[test]
	fn runtime_scene_dynamics_reports_diagnostics_only_contact_probes() {
		let document = UnaDocument {
			scene: Some(UnaSceneSnapshot {
				nodes: vec![
					test_node(vec![1, 2]),
					test_translation_node(0.0, 0.0, 0.0),
					test_translation_node(0.07, 0.0, 0.0),
				],
				roots: vec![0],
				..Default::default()
			}),
			spring_bones: Some(UnaSpringBoneSettings {
				contacts: vec![
					UnaDynamicsContact {
						source_kind: UnaDynamicsSourceKind::VrcPhysBone,
						source_id: "contact:hand".to_string(),
						node: 1,
						kind: UnaDynamicsContactKind::Receiver,
						parameter: "ContactHand".to_string(),
						collision_tags: vec!["Hand".to_string(), "Interact".to_string()],
						shape: UnaDynamicsColliderShape::Sphere,
						radius: 0.05,
						..Default::default()
					},
					UnaDynamicsContact {
						source_kind: UnaDynamicsSourceKind::VrcPhysBone,
						source_id: "contact:sender".to_string(),
						node: 2,
						kind: UnaDynamicsContactKind::Sender,
						collision_tags: vec!["Hand".to_string()],
						shape: UnaDynamicsColliderShape::Sphere,
						radius: 0.04,
						..Default::default()
					},
				],
				..Default::default()
			}),
			..Default::default()
		};

		let probes = document.runtime_model().scene_profile_dynamics().unwrap().contact_probes();
		assert_eq!(probes.len(), 1);
		assert_eq!(probes[0].receiver_source_id, "contact:hand");
		assert_eq!(probes[0].sender_source_id, "contact:sender");
		assert_eq!(probes[0].parameter, "ContactHand");
		assert_eq!(probes[0].matched_tags, vec!["Hand"]);
		assert!(probes[0].tag_match);
		assert!(probes[0].overlap);
		assert!(probes[0].would_emit);
		assert_eq!(probes[0].approximation, "sphere");
	}

	#[test]
	fn runtime_action_model_roundtrips_optional_effects() {
		let document = UnaDocument {
			runtime_actions: Some(UnaRuntimeActionSet {
				actions: vec![UnaRuntimeAction {
					id: "field-drape".to_string(),
					label: "Field Drape".to_string(),
					triggers: vec![UnaRuntimeActionTrigger::SupervisorCommand {
						command: "field_drape".to_string(),
					}],
					conditions: Vec::new(),
					effects: vec![
						UnaRuntimeActionEffect::WardrobeSet {
							set_id: "field_drape".to_string(),
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

		let json = serde_json::to_string(&document).unwrap();
		let decoded: UnaDocument = serde_json::from_str(&json).unwrap();
		let actions = decoded.runtime_model().runtime_actions().unwrap();
		assert_eq!(actions.actions.len(), 1);
		assert_eq!(actions.actions[0].effects.len(), 2);
	}

	#[test]
	fn runtime_action_reports_evaluation_target_writes() {
		let action = UnaRuntimeAction {
			id: "variant:coat".to_string(),
			effects: vec![
				UnaRuntimeActionEffect::NodeVisibility {
					target: UnaRuntimeNodeTarget {
						resolved_node_id: Some("node:coat".to_string()),
						path: Some("Root/Coat".to_string()),
						..Default::default()
					},
					visible: true,
				},
				UnaRuntimeActionEffect::MaterialScalar {
					target: UnaRuntimeMaterialTarget {
						material_index: Some(2),
						name: Some("Coat".to_string()),
					},
					parameter: "_Cutoff".to_string(),
					value: 0.4,
				},
				UnaRuntimeActionEffect::MaterialSlot {
					target: UnaRuntimeMaterialSlotTarget {
						node: UnaRuntimeNodeTarget {
							path: Some("Root/Coat".to_string()),
							..Default::default()
						},
						primitive_index: Some(1),
					},
					material: None,
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
			..Default::default()
		};

		let writes = action.evaluation_target_writes();

		assert_eq!(writes.len(), 5);
		assert!(writes.iter().all(|write| write.owner_key == "action:variant:coat"));
		assert_eq!(writes[0].target_kind, UnaEvaluationTargetKind::NodeVisibility);
		assert_eq!(writes[0].target_key, "node:coat");
		assert_eq!(writes[1].target_kind, UnaEvaluationTargetKind::MaterialProperty);
		assert_eq!(writes[1].target_key, "Coat:_Cutoff");
		assert_eq!(writes[2].target_kind, UnaEvaluationTargetKind::MaterialSlot);
		assert_eq!(writes[2].target_key, "Root/Coat[1]");
		assert_eq!(writes[3].target_kind, UnaEvaluationTargetKind::ExpressionWeight);
		assert_eq!(writes[3].target_key, "Smile");
		assert_eq!(writes[4].target_kind, UnaEvaluationTargetKind::DynamicsEnabled);
		assert_eq!(writes[4].target_key, "physbone:hair");
	}

	#[test]
	fn runtime_action_set_reports_target_write_collisions_between_actions() {
		let actions = UnaRuntimeActionSet {
			actions: vec![
				UnaRuntimeAction {
					id: "hat:on".to_string(),
					effects: vec![UnaRuntimeActionEffect::NodeVisibility {
						target: UnaRuntimeNodeTarget {
							path: Some("Root/Hat".to_string()),
							..Default::default()
						},
						visible: true,
					}],
					..Default::default()
				},
				UnaRuntimeAction {
					id: "hat:off".to_string(),
					effects: vec![UnaRuntimeActionEffect::NodeVisibility {
						target: UnaRuntimeNodeTarget {
							path: Some("Root/Hat".to_string()),
							..Default::default()
						},
						visible: false,
					}],
					..Default::default()
				},
				UnaRuntimeAction {
					id: "coat:multi".to_string(),
					effects: vec![
						UnaRuntimeActionEffect::MaterialScalar {
							target: UnaRuntimeMaterialTarget {
								name: Some("Coat".to_string()),
								..Default::default()
							},
							parameter: "_Cutoff".to_string(),
							value: 0.4,
						},
						UnaRuntimeActionEffect::MaterialScalar {
							target: UnaRuntimeMaterialTarget {
								name: Some("Coat".to_string()),
								..Default::default()
							},
							parameter: "_Cutoff".to_string(),
							value: 0.6,
						},
					],
					..Default::default()
				},
			],
		};

		let collisions = actions.evaluation_target_write_collisions();

		assert_eq!(collisions.len(), 1);
		assert_eq!(collisions[0].target_kind, UnaEvaluationTargetKind::NodeVisibility);
		assert_eq!(collisions[0].target_key, "Root/Hat");
		assert_eq!(collisions[0].owner_keys, vec!["action:hat:off", "action:hat:on"]);
		assert_eq!(collisions[0].action_ids, vec!["hat:off", "hat:on"]);
		assert_eq!(collisions[0].writes.len(), 2);
	}

	#[test]
	fn runtime_action_restore_readiness_reports_baseline_requirements() {
		let primitive = UnaMeshBuffers {
			name: None,
			positions: vec![[0.0; 3]],
			normals: None,
			tangents: None,
			tex_coords_0: None,
			tex_coords_1: None,
			tex_coords_2: None,
			tex_coords_3: None,
			colors_0: None,
			joints: None,
			weights: None,
			indices: None,
			material_index: Some(0),
			morph_targets: Vec::new(),
			morph_target_names: Vec::new(),
			default_morph_weights: Vec::new(),
		};
		let document = UnaDocument {
			scene: Some(UnaSceneSnapshot {
				nodes: vec![UnaSceneNode {
					name: Some("Renderer".to_string()),
					source_node_id: Some("node_renderer".to_string()),
					visible: true,
					mesh: Some(0),
					..test_node(Vec::new())
				}],
				meshes: vec![vec![primitive]],
				materials: vec![UnaMaterialPbr {
					name: Some("Mat".to_string()),
					..Default::default()
				}],
				roots: vec![0],
				..Default::default()
			}),
			spring_bones: Some(UnaSpringBoneSettings {
				groups: vec![UnaSpringBoneGroup {
					source_id: "physbone:hair".to_string(),
					enabled: true,
					..Default::default()
				}],
				..Default::default()
			}),
			..Default::default()
		};
		let action = UnaRuntimeAction {
			id: "variant:coat".to_string(),
			effects: vec![
				UnaRuntimeActionEffect::NodeVisibility {
					target: UnaRuntimeNodeTarget {
						source_node_id: Some("node_renderer".to_string()),
						..Default::default()
					},
					visible: false,
				},
				UnaRuntimeActionEffect::MaterialScalar {
					target: UnaRuntimeMaterialTarget {
						name: Some("Mat".to_string()),
						..Default::default()
					},
					parameter: "_Smoothness".to_string(),
					value: 0.5,
				},
				UnaRuntimeActionEffect::MaterialSlot {
					target: UnaRuntimeMaterialSlotTarget {
						node: UnaRuntimeNodeTarget {
							source_node_id: Some("node_renderer".to_string()),
							..Default::default()
						},
						primitive_index: None,
					},
					material: None,
				},
				UnaRuntimeActionEffect::DynamicsEnabled {
					source_id: "physbone:hair".to_string(),
					enabled: false,
				},
				UnaRuntimeActionEffect::NodeVisibility {
					target: UnaRuntimeNodeTarget {
						source_node_id: Some("missing".to_string()),
						..Default::default()
					},
					visible: true,
				},
				UnaRuntimeActionEffect::ExpressionWeight {
					name: "Smile".to_string(),
					weight: 1.0,
				},
			],
			..Default::default()
		};

		let readiness = document.runtime_model().runtime_action_restore_readiness(&action);

		assert_eq!(readiness.len(), 6);
		assert!(readiness[0].restore_target);
		assert!(readiness[0].current_value_available);
		assert_eq!(readiness[0].current_value, Some(Value::from(true)));
		assert!(readiness[0].baseline_required);
		assert!(!readiness[0].ready);
		assert_eq!(readiness[0].reason, "baseline_not_captured");
		assert_eq!(readiness[1].reason, "baseline_not_captured");
		assert_eq!(readiness[1].current_value, Some(Value::from(0.0)));
		assert_eq!(readiness[2].reason, "baseline_not_captured");
		assert_eq!(readiness[2].current_value, Some(Value::from(0_u64)));
		assert_eq!(readiness[3].reason, "baseline_not_captured");
		assert_eq!(readiness[3].current_value, Some(Value::from(true)));
		assert!(readiness[4].restore_target);
		assert!(!readiness[4].current_value_available);
		assert_eq!(readiness[4].reason, "target_unresolved");
		assert!(!readiness[5].restore_target);
		assert_eq!(readiness[5].reason, "not_restore_target");

		let candidates = document
			.runtime_model()
			.runtime_action_set_restore_baseline_candidates(&UnaRuntimeActionSet { actions: vec![action] });
		assert_eq!(candidates.len(), 4);
		assert_eq!(candidates[0].target_key, "node_renderer");
		assert_eq!(candidates[0].baseline_value, Value::from(true));
		assert_eq!(candidates[1].baseline_value, Value::from(0.0));
		assert_eq!(candidates[2].baseline_value, Value::from(0_u64));
		assert_eq!(candidates[3].baseline_value, Value::from(true));

		let plan = restore_baseline_capture_plan_from_candidates(candidates);
		assert_eq!(plan.len(), 4);
		assert_eq!(plan[0].owner_key, "action:variant:coat");
		assert_eq!(plan[0].source_action_ids, vec!["variant:coat"]);
		assert_eq!(plan[0].source_effect_kinds, vec!["node_visibility"]);
	}

	#[test]
	fn runtime_action_model_defaults_to_absent_for_legacy_documents() {
		let decoded: UnaDocument = serde_json::from_str("{}").unwrap();
		assert!(decoded.runtime_model().runtime_actions().is_none());
	}

	#[test]
	fn runtime_action_query_matches_ids_and_triggers() {
		let actions = UnaRuntimeActionSet {
			actions: vec![UnaRuntimeAction {
				id: "wardrobe:field_drape".to_string(),
				label: "Field Drape".to_string(),
				triggers: vec![
					UnaRuntimeActionTrigger::ExpressionMenu {
						path: "Wardrobe/Field Drape".to_string(),
					},
					UnaRuntimeActionTrigger::ParameterValue {
						name: "Outfit".to_string(),
						value: 2.0,
					},
				],
				conditions: Vec::new(),
				effects: vec![UnaRuntimeActionEffect::WardrobeSet {
					set_id: "field_drape".to_string(),
				}],
			}],
		};

		assert!(actions
			.find_action(UnaRuntimeActionQuery {
				action_id: Some("wardrobe:field_drape"),
				..Default::default()
			})
			.is_some());
		assert!(actions
			.find_action(UnaRuntimeActionQuery {
				expression_menu_path: Some("Wardrobe/Field Drape"),
				..Default::default()
			})
			.is_some());
		assert!(actions
			.find_action(UnaRuntimeActionQuery {
				parameter_name: Some("Outfit"),
				parameter_value: Some(2.004),
				..Default::default()
			})
			.is_some());
		assert!(actions
			.find_action(UnaRuntimeActionQuery {
				parameter_name: Some("Outfit"),
				parameter_value: Some(2.006),
				..Default::default()
			})
			.is_none());
		assert_eq!(
			actions.actions[0].parameter_assignments(),
			BTreeMap::from([("Outfit".to_string(), 2.0)])
		);
	}

	#[test]
	fn runtime_action_condition_matches_parameter_with_inversion() {
		let condition = UnaRuntimeActionCondition {
			parameter_name: Some("Hat".to_string()),
			parameter_value: Some(1.0),
			..Default::default()
		};
		assert_eq!(condition.parameter_condition_matches("Hat", 1.004), Some(true));
		assert_eq!(condition.parameter_condition_matches("Hat", 1.006), Some(false));
		assert_eq!(condition.parameter_condition_matches("Other", 1.0), Some(false));

		let inverted = UnaRuntimeActionCondition {
			inverted: true,
			..condition
		};
		assert_eq!(inverted.parameter_condition_matches("Hat", 1.004), Some(false));
		assert_eq!(inverted.parameter_condition_matches("Hat", 1.006), Some(true));
		assert_eq!(UnaRuntimeActionCondition::default().parameter_condition_matches("Hat", 1.0), None);
	}

	#[test]
	fn runtime_action_reports_parameter_condition_state() {
		let action = UnaRuntimeAction {
			conditions: vec![UnaRuntimeActionCondition {
				parameter_name: Some("Hat".to_string()),
				parameter_value: Some(1.0),
				..Default::default()
			}],
			..Default::default()
		};
		assert_eq!(action.parameter_condition_state("Hat", 1.0), Some(true));
		assert_eq!(action.parameter_condition_state("Hat", 0.0), Some(false));
		assert_eq!(action.parameter_condition_state("Other", 1.0), Some(false));

		let inverted = UnaRuntimeAction {
			conditions: vec![UnaRuntimeActionCondition {
				parameter_name: Some("Hat".to_string()),
				parameter_value: Some(1.0),
				inverted: true,
				..Default::default()
			}],
			..Default::default()
		};
		assert_eq!(inverted.parameter_condition_state("Hat", 1.0), Some(false));
		assert_eq!(inverted.parameter_condition_state("Hat", 0.0), Some(true));
		assert_eq!(UnaRuntimeAction::default().parameter_condition_state("Hat", 1.0), None);
	}

	#[test]
	fn runtime_action_reports_current_parameter_condition_state() {
		let action = UnaRuntimeAction {
			conditions: vec![UnaRuntimeActionCondition {
				parameter_name: Some("Hat".to_string()),
				parameter_value: Some(1.0),
				..Default::default()
			}],
			..Default::default()
		};
		assert_eq!(action.condition_parameter_names(), vec!["Hat"]);
		assert_eq!(
			action.current_parameter_condition_state(None, &BTreeMap::from([("Hat".to_string(), 1.0)])),
			Some("active")
		);
		assert_eq!(
			action.current_parameter_condition_state(None, &BTreeMap::from([("Hat".to_string(), 0.0)])),
			Some("inactive")
		);
		assert_eq!(
			action.current_parameter_condition_state(None, &BTreeMap::new()),
			Some("missing_parameter")
		);
		assert_eq!(
			UnaRuntimeAction::default().current_parameter_condition_state(None, &BTreeMap::from([("Hat".to_string(), 1.0)])),
			None
		);
	}

	#[test]
	fn runtime_action_condition_checks_active_parent_nodes() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![test_node(vec![1]), test_node(Vec::new())],
			roots: vec![0],
			..Default::default()
		};
		let action = UnaRuntimeAction {
			conditions: vec![UnaRuntimeActionCondition {
				parameter_name: Some("Hat".to_string()),
				parameter_value: Some(1.0),
				active_parent_nodes: vec![UnaRuntimeNodeTarget {
					node_index: Some(0),
					..Default::default()
				}],
				..Default::default()
			}],
			..Default::default()
		};

		assert_eq!(action.parameter_condition_state_in_scene(Some(&scene), "Hat", 1.0), Some(true));
		scene.nodes[0].visible = false;
		assert_eq!(action.parameter_condition_state_in_scene(Some(&scene), "Hat", 1.0), Some(false));
		assert_eq!(action.parameter_condition_state_in_scene(None, "Hat", 1.0), Some(false));
	}

	#[test]
	fn runtime_state_tracks_active_wardrobe_set() {
		let mut document = UnaDocument::default();
		assert_eq!(document.runtime_model().active_wardrobe_set(), None);
		assert_eq!(document.runtime_model().last_action_id(), None);

		document
			.runtime_model_mut()
			.set_active_wardrobe_set(Some("field_drape".to_string()));
		document
			.runtime_model_mut()
			.set_active_asset_groups(vec!["outfit:field_drape".to_string(), "texture:red".to_string()]);
		document
			.runtime_model_mut()
			.set_last_action_id(Some("wardrobe:field_drape".to_string()));
		document.runtime_model_mut().set_runtime_parameter_value("Outfit", 2.0);
		assert_eq!(document.runtime_model().active_wardrobe_set(), Some("field_drape"));
		assert_eq!(
			document.runtime_model().active_asset_groups(),
			&["outfit:field_drape".to_string(), "texture:red".to_string()]
		);
		assert_eq!(
			document.runtime_model().resolver_cache_key(),
			UnaRuntimeResolverCacheKey {
				wardrobe_set: Some("field_drape".to_string()),
				active_asset_groups: vec!["outfit:field_drape".to_string(), "texture:red".to_string()],
				modular_avatar_components_hash: None,
				material_source_hash: None,
				mesh_source_hash: None,
				resolver_version: UNA_RUNTIME_RESOLVER_VERSION,
			}
		);
		assert_eq!(document.runtime_model().last_action_id(), Some("wardrobe:field_drape"));
		assert_eq!(document.runtime_model().runtime_parameter_values().get("Outfit"), Some(&2.0));

		document.runtime_model_mut().set_runtime_parameter_value("Outfit", 3.0);
		assert_eq!(
			document.runtime_model().resolver_cache_key(),
			UnaRuntimeResolverCacheKey {
				wardrobe_set: Some("field_drape".to_string()),
				active_asset_groups: vec!["outfit:field_drape".to_string(), "texture:red".to_string()],
				modular_avatar_components_hash: None,
				material_source_hash: None,
				mesh_source_hash: None,
				resolver_version: UNA_RUNTIME_RESOLVER_VERSION,
			}
		);

		let json = serde_json::to_string(&document).unwrap();
		let decoded: UnaDocument = serde_json::from_str(&json).unwrap();
		assert_eq!(decoded.runtime_model().active_wardrobe_set(), Some("field_drape"));
		assert_eq!(
			decoded.runtime_model().active_asset_groups(),
			&["outfit:field_drape".to_string(), "texture:red".to_string()]
		);
		assert_eq!(decoded.runtime_model().last_action_id(), Some("wardrobe:field_drape"));
		assert_eq!(decoded.runtime_model().runtime_parameter_values().get("Outfit"), Some(&3.0));
	}

	#[test]
	fn runtime_resolver_cache_key_hashes_modular_avatar_components() {
		let mut document = UnaDocument {
			unavatar: Some(UnaUnavatarExtension {
				spec_version: "0.1-preview".to_string(),
				source: serde_json::json!({
					"modularAvatar": {
						"components": [{
							"shortType": "ModularAvatarMaterialSetter",
							"id": "mat-setter",
							"fields": {
								"objects": [{"slot": 0, "material": "Red"}]
							}
						}]
					}
				}),
			}),
			..Default::default()
		};
		let first = document.runtime_model().resolver_cache_key();
		assert!(first.modular_avatar_components_hash.is_some());

		document.unavatar.as_mut().unwrap().source = serde_json::json!({
			"modularAvatar": {
				"components": [{
					"fields": {
						"objects": [{"material": "Red", "slot": 0}]
					},
					"id": "mat-setter",
					"shortType": "ModularAvatarMaterialSetter"
				}]
			}
		});
		assert_eq!(
			document.runtime_model().resolver_cache_key().modular_avatar_components_hash,
			first.modular_avatar_components_hash
		);

		document.unavatar.as_mut().unwrap().source["modularAvatar"]["components"][0]["id"] = Value::String("other".to_string());
		assert_ne!(
			document.runtime_model().resolver_cache_key().modular_avatar_components_hash,
			first.modular_avatar_components_hash
		);
	}

	#[test]
	fn runtime_resolver_cache_key_hashes_material_source_profile() {
		let mut document = UnaDocument {
			scene: Some(UnaSceneSnapshot {
				materials: vec![UnaMaterialPbr {
					name: Some("Jacket".to_string()),
					shading: UnaShadingModel::LilToonLike,
					base_color_texture_index: Some(3),
					unavatar_material: Some(serde_json::json!({
						"shader": "lilToon",
						"floatParams": {"_Cutoff": 0.5}
					})),
					..Default::default()
				}],
				..Default::default()
			}),
			..Default::default()
		};
		let first = document.runtime_model().resolver_cache_key();
		assert!(first.material_source_hash.is_some());

		apply_runtime_material_color(&mut document.scene.as_mut().unwrap().materials[0], "_Color", [0.2, 0.3, 0.4, 0.5]).unwrap();
		assert_eq!(
			document.runtime_model().resolver_cache_key().material_source_hash,
			first.material_source_hash
		);

		document.scene.as_mut().unwrap().materials[0].unavatar_material = Some(serde_json::json!({
			"shader": "lilToon",
			"floatParams": {"_Cutoff": 0.1}
		}));
		assert_ne!(
			document.runtime_model().resolver_cache_key().material_source_hash,
			first.material_source_hash
		);
	}

	#[test]
	fn runtime_resolver_cache_key_hashes_mesh_render_identity() {
		let mut document = UnaDocument {
			scene: Some(UnaSceneSnapshot {
				meshes: vec![vec![UnaMeshBuffers {
					name: Some("Body".to_string()),
					positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
					normals: None,
					tangents: None,
					tex_coords_0: None,
					tex_coords_1: None,
					tex_coords_2: None,
					tex_coords_3: None,
					colors_0: None,
					joints: None,
					weights: None,
					indices: Some(vec![0, 1]),
					material_index: Some(0),
					morph_targets: Vec::new(),
					morph_target_names: vec!["Smile".to_string()],
					default_morph_weights: Vec::new(),
				}]],
				..Default::default()
			}),
			..Default::default()
		};
		let first = document.runtime_model().resolver_cache_key();
		assert!(first.mesh_source_hash.is_some());

		document.scene.as_mut().unwrap().meshes[0][0].material_index = Some(1);
		assert_ne!(
			document.runtime_model().resolver_cache_key().mesh_source_hash,
			first.mesh_source_hash
		);

		document.scene.as_mut().unwrap().meshes[0][0].material_index = Some(0);
		document.scene.as_mut().unwrap().meshes[0][0]
			.morph_target_names
			.push("Blink".to_string());
		assert_ne!(
			document.runtime_model().resolver_cache_key().mesh_source_hash,
			first.mesh_source_hash
		);

		document.scene.as_mut().unwrap().meshes[0][0].morph_target_names = vec!["Smile".to_string()];
		document.scene.as_mut().unwrap().meshes[0][0].colors_0 = Some(vec![[1.0, 1.0, 1.0, 1.0]; 2]);
		assert_ne!(
			document.runtime_model().resolver_cache_key().mesh_source_hash,
			first.mesh_source_hash
		);
	}

	#[test]
	fn modular_avatar_component_support_kind_classifies_known_runtime_features() {
		assert_eq!(modular_avatar_component_support_kind("ModularAvatarBoneProxy"), "resolver");
		assert_eq!(modular_avatar_component_support_kind("ModularAvatarBlendshapeSync"), "resolver");
		assert_eq!(modular_avatar_component_support_kind("ModularAvatarRemoveVertexColor"), "resolver");
		assert_eq!(modular_avatar_component_support_kind("ModularAvatarMaterialSwap"), "runtime_action");
		assert_eq!(modular_avatar_component_support_kind("ModularAvatarObjectToggle"), "runtime_action");
		assert_eq!(modular_avatar_component_support_kind("ModularAvatarMenuItem"), "metadata");
		assert_eq!(modular_avatar_component_support_kind("ModularAvatarParameters"), "metadata");
		assert_eq!(
			modular_avatar_component_support_kind("ModularAvatarSyncParameterSequence"),
			"metadata"
		);
		assert_eq!(
			modular_avatar_component_support_kind("ModularAvatarVisibleHeadAccessory"),
			"metadata"
		);
		assert_eq!(modular_avatar_component_support_kind("ModularAvatarMeshCutter"), "resolver");
		assert_eq!(modular_avatar_component_support_kind("ModularAvatarShapeChanger"), "resolver");
		assert_eq!(
			modular_avatar_component_support_kind("ModularAvatarConvertConstraints"),
			"unsupported"
		);
		assert_eq!(modular_avatar_component_support_kind("ModularAvatarFloorAdjuster"), "unsupported");
		assert_eq!(modular_avatar_component_support_kind("ModularAvatarGlobalCollider"), "unsupported");
		assert_eq!(modular_avatar_component_support_kind("ModularAvatarMMDLayerControl"), "unsupported");
		assert_eq!(modular_avatar_component_support_kind("ModularAvatarMergeAnimator"), "unsupported");
		assert_eq!(modular_avatar_component_support_kind("ModularAvatarMergeBlendTree"), "unsupported");
		assert_eq!(modular_avatar_component_support_kind("ModularAvatarPBBlocker"), "unsupported");
		assert_eq!(modular_avatar_component_support_kind("ModularAvatarPlatformFilter"), "unsupported");
		assert_eq!(
			modular_avatar_component_support_kind("ModularAvatarRenameVRChatCollisionTags"),
			"unsupported"
		);
		assert_eq!(modular_avatar_component_support_kind("ModularAvatarScaleAdjuster"), "unsupported");
		assert_eq!(modular_avatar_component_support_kind("ModularAvatarVRChatSettings"), "unsupported");
		assert_eq!(
			modular_avatar_component_support_kind("ModularAvatarWorldFixedObject"),
			"unsupported"
		);
		assert_eq!(
			modular_avatar_component_support_kind("ModularAvatarWorldScaleObject"),
			"unsupported"
		);
		assert_eq!(modular_avatar_component_support_kind("MAMoveIndependently"), "unsupported");
		assert_eq!(modular_avatar_component_support_kind("VertexFilterByShapeComponent"), "metadata");
		assert_eq!(modular_avatar_component_support_kind("SomethingElse"), "unsupported");
	}

	#[test]
	fn modular_avatar_vertex_filter_group_serializes_common_filter_representation() {
		let group = UnaModularAvatarVertexFilterGroup {
			source_component_id: Some("mesh-cutter".to_string()),
			source_component_type: "ModularAvatarMeshCutter".to_string(),
			combine: UnaVertexFilterCombineMode::Intersection,
			filters: vec![UnaVertexFilter::BlendShape {
				shapes: vec!["Sleeve".to_string()],
				threshold: 0.001,
			}],
			..Default::default()
		};
		let value = serde_json::to_value(&group).unwrap();
		assert_eq!(value["source_component_id"], "mesh-cutter");
		assert_eq!(value["source_component_type"], "ModularAvatarMeshCutter");
		assert_eq!(value["combine"], "Intersection");
		assert_eq!(value["filters"][0]["kind"], "blend_shape");
		assert_eq!(value["filters"][0]["shapes"][0], "Sleeve");
	}

	#[test]
	fn material_runtime_toon_view_respects_shading_model() {
		let mut material = UnaMaterialPbr {
			liltoon_like: Some(UnaLilToonLikeMaterial::default()),
			mtoon: Some(UnaMtoonMaterial::default()),
			..Default::default()
		};
		assert_eq!(material.runtime_toon_model(), None);
		assert!(material.liltoon_like_runtime().is_none());
		assert!(material.mtoon_like_runtime().is_none());

		material.shading = UnaShadingModel::LilToonLike;
		assert_eq!(material.runtime_toon_model(), Some(UnaRuntimeToonModel::LilToonLike));
		assert!(material.liltoon_like_runtime().is_some());
		assert!(material.liltoon_like_source_profile().is_some());
		assert!(material.mtoon_like_runtime().is_none());

		material.shading = UnaShadingModel::MToonLike;
		assert_eq!(material.runtime_toon_model(), Some(UnaRuntimeToonModel::MToonLike));
		assert!(material.liltoon_like_runtime().is_none());
		assert!(material.mtoon_like_runtime().is_some());
		assert!(material.mtoon_source_profile().is_some());
	}

	#[test]
	fn liltoon_source_profile_helpers_capture_special_runtime_semantics() {
		let mut material = UnaLilToonLikeMaterial::default();
		assert!(!material.is_gem_profile());
		assert!(!material.is_refraction_profile());
		assert!(!material.needs_screen_refraction());
		assert!(!material.uses_reflection_source_cube());

		material.source_profile = UnaLilToonLikeSourceProfile::LiltoonGem;
		assert!(material.is_gem_profile());
		assert!(!material.is_refraction_profile());
		assert!(material.uses_reflection_source_cube());
		assert!(material.needs_screen_refraction());

		material.reflection.gem_refraction_strength_factor = 0.0;
		assert!(material.needs_screen_refraction());

		material.source_profile = UnaLilToonLikeSourceProfile::LiltoonRefraction;
		assert!(!material.is_gem_profile());
		assert!(material.is_refraction_profile());
		assert!(!material.needs_screen_refraction());
		material.reflection.gem_refraction_strength_factor = 0.25;
		assert!(material.needs_screen_refraction());
	}

	#[test]
	fn runtime_dynamics_reports_spring_bone_groups() {
		let document = UnaDocument {
			spring_bones: Some(UnaSpringBoneSettings {
				groups: vec![
					UnaSpringBoneGroup {
						enabled: true,
						bone_node_indices: vec![0, 1],
						..Default::default()
					},
					UnaSpringBoneGroup {
						enabled: true,
						source_kind: UnaDynamicsSourceKind::VrcPhysBone,
						bone_node_indices: vec![2, 3],
						..Default::default()
					},
				],
				colliders: vec![
					UnaDynamicsCollider {
						node: 4,
						source_kind: UnaDynamicsSourceKind::VrcPhysBone,
						..Default::default()
					},
					UnaDynamicsCollider {
						node: 5,
						..Default::default()
					},
				],
				constraint_refs: vec![UnaDynamicsConstraintRef {
					target_node: 6,
					source_nodes: vec![2, 7],
					..Default::default()
				}],
				..Default::default()
			}),
			..Default::default()
		};
		let dynamics = document.runtime_model().dynamics();

		assert!(dynamics.has_groups());
		assert_eq!(dynamics.group_count(), 2);
		assert_eq!(dynamics.enabled_group_count(), 2);
		assert_eq!(dynamics.source_group_count(UnaDynamicsSourceKind::VrmSpringBone), 1);
		assert_eq!(dynamics.source_group_count(UnaDynamicsSourceKind::VrcPhysBone), 1);

		let counts = dynamics.counts();
		assert_eq!(counts.groups, 2);
		assert_eq!(counts.enabled_groups, 2);
		assert_eq!(counts.vrm_spring_bone_groups, 1);
		assert_eq!(counts.vrc_physbone_groups, 1);
		assert_eq!(counts.unknown_groups, 0);
		assert_eq!(counts.colliders, 2);
		assert_eq!(counts.vrm_spring_bone_colliders, 1);
		assert_eq!(counts.vrc_physbone_colliders, 1);
		assert_eq!(dynamics.dynamic_bone_node_indices().collect::<Vec<_>>(), vec![0, 1, 2, 3]);
		assert_eq!(dynamics.reset_node_indices(), vec![0, 1, 2, 3, 6, 7]);
		assert_eq!(dynamics.colliders().map(|collider| collider.node).collect::<Vec<_>>(), vec![4, 5]);
	}

	#[test]
	fn spring_bone_group_source_kind_defaults_for_legacy_json() {
		let group: UnaSpringBoneGroup = serde_json::from_str(
			r#"{
				"comment": "legacy",
				"stiffness": 1.0,
				"gravity_power": 0.0,
				"drag_force": 0.4,
				"hit_radius": 0.02,
				"bone_node_indices": [0, 1]
			}"#,
		)
		.expect("legacy spring bone group");

		assert_eq!(group.source_kind, UnaDynamicsSourceKind::VrmSpringBone);
	}

	#[test]
	fn import_report_serializes_source_format_when_set() {
		let mut r = ImportReport::default();
		r.source_format = Some(FormatId::new("io.un-avatar.una"));
		r.status = ReportStatus::PartialSuccess;
		r.preserved_extensions.push(PreservedExtension {
			id: "ext".into(),
			note: Some("blob".into()),
		});
		let v = serde_json::to_value(&r).unwrap();
		assert_eq!(v["source_format"], "io.un-avatar.una");
		assert_eq!(v["status"], "partial_success");
		assert!(v["preserved_extensions"].is_array());
	}

	#[test]
	fn import_report_serializes_diagnostics() {
		let mut r = ImportReport::default();
		r.push_info("loaded");
		r.push_warning("partial");
		let v = serde_json::to_value(&r).unwrap();
		assert_eq!(v["messages"], serde_json::json!(["loaded", "partial"]));
		let d = v["diagnostics"].as_array().expect("diagnostics");
		assert_eq!(d.len(), 2);
		assert_eq!(d[0]["severity"], "info");
		assert_eq!(d[0]["text"], "loaded");
		assert_eq!(d[1]["severity"], "warning");
		assert_eq!(d[1]["text"], "partial");
	}
}
