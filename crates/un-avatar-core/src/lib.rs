//! UN Avatar — UNA 内部表現の中核（bootstrap）。
//!
//! 設計: `docs/crate-io-plugin-plan.md` §4.2

#![forbid(unsafe_code)]

use std::{borrow::Cow, collections::BTreeMap};

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
}

#[derive(Clone, Copy, Debug, Default)]
pub struct UnaRuntimeDynamics<'a> {
	spring_bones: Option<&'a UnaSpringBoneSettings>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UnaRuntimeDynamicsCounts {
	pub groups: usize,
	pub enabled_groups: usize,
	pub vrm_spring_bone_groups: usize,
	pub vrc_physbone_groups: usize,
	pub unknown_groups: usize,
	pub colliders: usize,
	pub vrm_spring_bone_colliders: usize,
	pub vrc_physbone_colliders: usize,
	pub unknown_colliders: usize,
}

impl<'a> UnaRuntimeDynamics<'a> {
	pub fn spring_bones(self) -> Option<&'a UnaSpringBoneSettings> {
		self.spring_bones
	}

	pub fn has_groups(self) -> bool {
		self.spring_bones.is_some_and(|settings| !settings.groups.is_empty())
	}

	pub fn group_count(self) -> usize {
		self.spring_bones.map(|settings| settings.groups.len()).unwrap_or(0)
	}

	pub fn enabled_group_count(self) -> usize {
		self.spring_bones
			.map(|settings| settings.groups.iter().filter(|group| group.enabled).count())
			.unwrap_or(0)
	}

	pub fn source_group_count(self, source_kind: UnaDynamicsSourceKind) -> usize {
		self.spring_bones
			.map(|settings| settings.groups.iter().filter(|group| group.source_kind == source_kind).count())
			.unwrap_or(0)
	}

	pub fn collider_count(self) -> usize {
		self.spring_bones.map(|settings| settings.colliders.len()).unwrap_or(0)
	}

	pub fn dynamic_bone_node_indices(self) -> impl Iterator<Item = usize> + 'a {
		self.spring_bones
			.into_iter()
			.flat_map(|settings| settings.groups.iter())
			.flat_map(|group| group.bone_node_indices.iter().copied())
	}

	pub fn colliders(self) -> impl Iterator<Item = &'a UnaDynamicsCollider> {
		self.spring_bones.into_iter().flat_map(|settings| settings.colliders.iter())
	}

	pub fn source_collider_count(self, source_kind: UnaDynamicsSourceKind) -> usize {
		self.spring_bones
			.map(|settings| {
				settings
					.colliders
					.iter()
					.filter(|collider| collider.source_kind == source_kind)
					.count()
			})
			.unwrap_or(0)
	}

	pub fn counts(self) -> UnaRuntimeDynamicsCounts {
		UnaRuntimeDynamicsCounts {
			groups: self.group_count(),
			enabled_groups: self.enabled_group_count(),
			vrm_spring_bone_groups: self.source_group_count(UnaDynamicsSourceKind::VrmSpringBone),
			vrc_physbone_groups: self.source_group_count(UnaDynamicsSourceKind::VrcPhysBone),
			unknown_groups: self.source_group_count(UnaDynamicsSourceKind::Unknown),
			colliders: self.collider_count(),
			vrm_spring_bone_colliders: self.source_collider_count(UnaDynamicsSourceKind::VrmSpringBone),
			vrc_physbone_colliders: self.source_collider_count(UnaDynamicsSourceKind::VrcPhysBone),
			unknown_colliders: self.source_collider_count(UnaDynamicsSourceKind::Unknown),
		}
	}
}

impl UnaSpringBoneSettings {
	pub fn runtime_dynamics(&self) -> UnaRuntimeDynamics<'_> {
		UnaRuntimeDynamics { spring_bones: Some(self) }
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
	/// VRM SpringBone / secondaryAnimation から取り込んだ揺れもの用チェーン（ランタイムで更新）。
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub spring_bones: Option<UnaSpringBoneSettings>,
}

impl UnaDocument {
	pub fn runtime_model(&self) -> UnaRuntimeModel<'_> {
		UnaRuntimeModel { document: self }
	}

	pub fn runtime_scene_and_dynamics_mut(&mut self) -> Option<(&mut UnaSceneSnapshot, UnaRuntimeDynamics<'_>)> {
		let UnaDocument { scene, spring_bones, .. } = self;
		let scene = scene.as_mut()?;
		Some((
			scene,
			UnaRuntimeDynamics {
				spring_bones: spring_bones.as_ref(),
			},
		))
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

#[derive(Clone, Copy, Debug)]
pub struct UnaRuntimeRetargetInputs<'a> {
	pub humanoid_basis: UnaHumanoidRuntimeBasis,
	pub profile: Option<&'a HumanoidProfile>,
	pub scene: Option<&'a UnaSceneSnapshot>,
	pub expression_catalog: Option<&'a UnaExpressionCatalog>,
}

impl<'a> UnaRuntimeModel<'a> {
	pub fn document(self) -> &'a UnaDocument {
		self.document
	}

	pub fn source_kind(self) -> UnaRuntimeSourceKind {
		if self.document.unavatar.is_some() {
			return UnaRuntimeSourceKind::Unavatar;
		}
		if let Some(vrm) = self.document.vrm.as_ref() {
			return if vrm.spec_version.starts_with('0') {
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

	pub fn humanoid_scene(self) -> Option<(&'a HumanoidProfile, &'a UnaSceneSnapshot)> {
		Some((self.humanoid_profile()?, self.scene()?))
	}

	pub fn has_humanoid_scene(self) -> bool {
		self.humanoid_scene().is_some()
	}

	pub fn scene_profile_dynamics(self) -> Option<(&'a UnaSceneSnapshot, Option<&'a HumanoidProfile>, UnaRuntimeDynamics<'a>)> {
		Some((self.scene()?, self.humanoid_profile(), self.dynamics()))
	}

	pub fn expression_catalog(self) -> Option<&'a UnaExpressionCatalog> {
		self.document.expression_catalog.as_ref()
	}

	pub fn scene_expression_catalog(self) -> Option<(&'a UnaSceneSnapshot, Option<&'a UnaExpressionCatalog>)> {
		Some((self.scene()?, self.expression_catalog()))
	}

	pub fn humanoid_retarget_inputs(self) -> UnaRuntimeRetargetInputs<'a> {
		UnaRuntimeRetargetInputs {
			humanoid_basis: self.humanoid_basis(),
			profile: self.humanoid_profile(),
			scene: self.scene(),
			expression_catalog: self.expression_catalog(),
		}
	}

	pub fn spring_bones(self) -> Option<&'a UnaSpringBoneSettings> {
		self.document.spring_bones.as_ref()
	}

	pub fn dynamics(self) -> UnaRuntimeDynamics<'a> {
		UnaRuntimeDynamics {
			spring_bones: self.document.spring_bones.as_ref(),
		}
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
}

impl UnaSceneSnapshot {
	/// Runtime roots after import normalization. Authored `roots` are borrowed as-is;
	/// legacy or partial imports without roots fall back to parentless nodes.
	pub fn resolved_roots(&self) -> Cow<'_, [usize]> {
		resolved_scene_roots(&self.nodes, &self.roots)
	}
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
		(self.is_gem_profile() || self.is_refraction_profile()) && self.reflection.gem_refraction_strength_factor.abs() > 0.00001
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

		assert_eq!(document.runtime_model().source_kind(), UnaRuntimeSourceKind::Vrm0);
		assert_eq!(document.runtime_model().humanoid_basis(), UnaHumanoidRuntimeBasis::Vrm0);
		document.vrm.as_mut().unwrap().spec_version = "1.0".to_string();
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
		assert!(document.runtime_model().humanoid_scene().is_none());
		assert!(!document.runtime_model().has_humanoid_scene());

		document.scene = Some(UnaSceneSnapshot::default());
		assert!(document.runtime_model().scene_nodes().is_some());
		assert!(document.runtime_model().humanoid_scene().is_none());
		assert!(!document.runtime_model().has_humanoid_scene());

		document.humanoid_profile = Some(HumanoidProfile::default());
		assert!(document.runtime_model().humanoid_scene().is_some());
		assert!(document.runtime_model().has_humanoid_scene());
		let retarget_inputs = document.runtime_model().humanoid_retarget_inputs();
		assert_eq!(retarget_inputs.humanoid_basis, UnaHumanoidRuntimeBasis::Vrm0);
		assert!(retarget_inputs.profile.is_some());
		assert!(retarget_inputs.scene.is_some());
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
		assert!(!material.needs_screen_refraction());
		material.reflection.gem_refraction_strength_factor = 0.25;

		material.source_profile = UnaLilToonLikeSourceProfile::LiltoonRefraction;
		assert!(!material.is_gem_profile());
		assert!(material.is_refraction_profile());
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
		let v = serde_json::to_value(&r).unwrap();
		assert_eq!(v["messages"], serde_json::json!(["loaded"]));
		let d = v["diagnostics"].as_array().expect("diagnostics");
		assert_eq!(d.len(), 1);
		assert_eq!(d[0]["severity"], "info");
		assert_eq!(d[0]["text"], "loaded");
	}
}
