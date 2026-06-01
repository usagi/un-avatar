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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnaSpringBoneGroup {
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
	/// glTF ノードインデックスのチェーン（親→子）。
	pub bone_node_indices: Vec<usize>,
}

fn default_spring_gravity_dir() -> [f32; 3] {
	[0.0, -1.0, 0.0]
}

fn default_spring_drag() -> f32 {
	0.4
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UnaSpringBoneSettings {
	#[serde(default)]
	pub groups: Vec<UnaSpringBoneGroup>,
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
	let mut w = if mesh_bufs.default_morph_weights.len() == n {
		mesh_bufs.default_morph_weights.clone()
	} else {
		let mut v = mesh_bufs.default_morph_weights.clone();
		v.resize(n, 0.0);
		v.truncate(n);
		v
	};
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
	pub tex_coords_0: Option<Vec<[f32; 2]>>,
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
	#[serde(default = "one_f32")]
	pub normal_strength_factor: f32,
	#[serde(default)]
	pub receive_factor: f32,
	#[serde(default)]
	pub second_color_factor: [f32; 4],
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
pub struct UnaLilToonLikeMatcap {
	#[serde(default)]
	pub enabled_factor: f32,
	#[serde(default = "one_vec3")]
	pub color_factor: [f32; 3],
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub texture_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub blend_mask_texture_index: Option<usize>,
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
	pub shadow_mask_factor: f32,
	#[serde(default)]
	pub lod_factor: f32,
	#[serde(default)]
	pub second_enabled_factor: f32,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub second_texture_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub second_blend_mask_texture_index: Option<usize>,
	#[serde(default = "one_vec4")]
	pub second_color_factor: [f32; 4],
	#[serde(default)]
	pub second_main_strength_factor: f32,
	#[serde(default = "one_f32")]
	pub second_blend_factor: f32,
	#[serde(default = "one_f32")]
	pub second_enable_lighting_factor: f32,
	#[serde(default)]
	pub second_blend_mode: UnaLilToonLikeBlendMode,
	#[serde(default = "one_f32")]
	pub second_normal_strength_factor: f32,
	#[serde(default)]
	pub second_lod_factor: f32,
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
	pub apply_reflection_factor: f32,
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
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub cube_texture_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub color_texture_index: Option<usize>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub smoothness_texture_index: Option<usize>,
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
	#[serde(default)]
	pub main_strength_factor: f32,
	#[serde(default = "one_f32")]
	pub blend_factor: f32,
	#[serde(default)]
	pub blend_mode: UnaLilToonLikeBlendMode,
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
pub struct UnaLilToonLikeMaterial {
	#[serde(default)]
	pub source_profile: UnaLilToonLikeSourceProfile,
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
	pub outline: UnaLilToonLikeOutline,
	#[serde(default)]
	pub backlight: UnaLilToonLikeBacklight,
	#[serde(default)]
	pub alpha_mask: UnaLilToonLikeAlphaMask,
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
			normal_strength_factor: 1.0,
			receive_factor: 0.0,
			second_color_factor: [0.0, 0.0, 0.0, 0.0],
			second_border_factor: 0.0,
			second_blur_factor: 0.0,
			second_normal_strength_factor: 1.0,
			second_receive_factor: 0.0,
			third_color_factor: [0.0, 0.0, 0.0, 0.0],
			third_border_factor: 0.0,
			third_blur_factor: 0.0,
			third_normal_strength_factor: 1.0,
			third_receive_factor: 0.0,
		}
	}
}

impl Default for UnaLilToonLikeMatcap {
	fn default() -> Self {
		Self {
			enabled_factor: 0.0,
			color_factor: [1.0, 1.0, 1.0],
			texture_index: None,
			blend_mask_texture_index: None,
			blend_factor: 1.0,
			main_strength_factor: 0.0,
			enable_lighting_factor: 0.0,
			blend_mode: UnaLilToonLikeBlendMode::default(),
			normal_strength_factor: 1.0,
			shadow_mask_factor: 0.0,
			lod_factor: 0.0,
			second_enabled_factor: 0.0,
			second_texture_index: None,
			second_blend_mask_texture_index: None,
			second_color_factor: [1.0, 1.0, 1.0, 1.0],
			second_main_strength_factor: 0.0,
			second_blend_factor: 1.0,
			second_enable_lighting_factor: 1.0,
			second_blend_mode: UnaLilToonLikeBlendMode::default(),
			second_normal_strength_factor: 1.0,
			second_lod_factor: 0.0,
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
			apply_reflection_factor: 1.0,
			specular_toon_factor: 1.0,
			specular_border_factor: default_liltoon_specular_border(),
			specular_blur_factor: 0.0,
			specular_normal_strength_factor: 1.0,
			reflection_normal_strength_factor: 1.0,
			cube_enable_lighting_factor: 1.0,
			cube_color_factor: [0.0, 0.0, 0.0, 1.0],
			cube_override_factor: 0.0,
			blend_mode: UnaLilToonLikeBlendMode::default(),
			cube_texture_index: None,
			color_texture_index: None,
			smoothness_texture_index: None,
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
			main_strength_factor: 0.0,
			blend_factor: 1.0,
			blend_mode: UnaLilToonLikeBlendMode::default(),
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

impl Default for UnaLilToonLikeMaterial {
	fn default() -> Self {
		Self {
			source_profile: UnaLilToonLikeSourceProfile::Unknown,
			shadow: UnaLilToonLikeShadow::default(),
			matcap: UnaLilToonLikeMatcap::default(),
			reflection: UnaLilToonLikeReflection::default(),
			rim: UnaLilToonLikeRim::default(),
			emission: UnaLilToonLikeEmission::default(),
			outline: UnaLilToonLikeOutline::default(),
			backlight: UnaLilToonLikeBacklight::default(),
			alpha_mask: UnaLilToonLikeAlphaMask::default(),
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

fn default_liltoon_smoothness() -> f32 {
	0.5
}

fn default_liltoon_reflectance() -> f32 {
	0.5
}

fn default_liltoon_specular_border() -> f32 {
	0.5
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
				let mut rgba = Vec::with_capacity(self.pixels.len() * 4);
				for &r in &self.pixels {
					rgba.extend_from_slice(&[r, r, r, 255]);
				}
				Cow::Owned(rgba)
			}
			UnaImagePixelFormat::R8G8 => {
				let mut rgba = Vec::with_capacity(self.pixels.len() / 2 * 4);
				for chunk in self.pixels.chunks_exact(2) {
					rgba.extend_from_slice(&[chunk[0], chunk[0], chunk[0], chunk[1]]);
				}
				Cow::Owned(rgba)
			}
			UnaImagePixelFormat::R8G8B8 => {
				let mut rgba = Vec::with_capacity(self.pixels.len() / 3 * 4);
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

fn rgba8_from_f16_channels(pixels: &[u8], channels: usize) -> Vec<u8> {
	let stride = channels * 2;
	if stride == 0 {
		return Vec::new();
	}
	let mut rgba = Vec::with_capacity(pixels.len() / stride * 4);
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
	let mut rgba = Vec::with_capacity(pixels.len() / stride.max(1) * 4);
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
	let mut rgba = Vec::with_capacity(pixels.len() / stride.max(1) * 4);
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
			tex_coords_0: None,
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
			tex_coords_0: None,
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
