//! glTF / [`UnaSceneSnapshot`] 由来のメッシュ描画（スキニング・モーフ・シェーディング種別）。

use std::{borrow::Cow, collections::BTreeMap};

use glam::{Mat4, Vec4};
use serde::Serialize;
use un_avatar_core::{
	UnaAlphaMode, UnaExpressionCatalog, UnaExpressionWeights, UnaMaterialPbr, UnaMeshBuffers, UnaMtoonMaterial, UnaMtoonOutlineWidthMode,
	UnaSceneSnapshot, UnaShadingModel,
};

use crate::avatar_material::{effective_mtoon_outline, effective_mtoon_rim, texture_roles_for_scene};
use crate::debug_dump::{debug_primitive_color_rgba, iris_like_material_name};
use crate::scene_transform::{safe_inverse_mesh_world, scene_world_matrices};
use crate::skin_tone::{
	build_skin_tone_matched_images, material_skin_tone_kind, skin_tone_matching_debug_for_scene_with_world,
	skin_tone_texture_kinds_for_scene, SkinToneMatchingDebug,
};
use crate::texture_pipeline::{
	compressed_cache_lookup_from_source, compression_preference_for_role, estimated_processed_mip_count, load_or_build_processed_texture,
	read_compressed_texture_cache, source_texture_upload, texture_cache_key, texture_cache_key_from_source_metadata,
	texture_upload_payload, GpuTextureCompressionContext, TextureCacheEvent, TextureRole, TextureUploadKind,
};
use crate::{
	BlockCompressionEncoder, TextureCompressionAdvancedOptions, TextureCompressionMode, TextureCompressionPreference, TextureMipmapFilter,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AvatarOutlinePolicy {
	Authored,
	Off,
	Override,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AvatarOutlineKind {
	Mtoon,
	Ink,
	Brush,
	Double,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AvatarOutlineOptions {
	pub policy: AvatarOutlinePolicy,
	pub kind: AvatarOutlineKind,
	pub width: Option<f32>,
	pub color: Option<[f32; 3]>,
	pub lighting_mix: Option<f32>,
	pub roundness: Option<f32>,
}

impl Default for AvatarOutlineOptions {
	fn default() -> Self {
		Self {
			policy: AvatarOutlinePolicy::Authored,
			kind: AvatarOutlineKind::Mtoon,
			width: None,
			color: None,
			lighting_mix: None,
			roundness: None,
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AvatarRimPolicy {
	Authored,
	Off,
	Override,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AvatarRimOptions {
	pub policy: AvatarRimPolicy,
	pub color: Option<[f32; 3]>,
	pub intensity: Option<f32>,
	pub lighting_mix: Option<f32>,
	pub fresnel_power: Option<f32>,
	pub lift: Option<f32>,
}

impl Default for AvatarRimOptions {
	fn default() -> Self {
		Self {
			policy: AvatarRimPolicy::Authored,
			color: None,
			intensity: None,
			lighting_mix: None,
			fresnel_power: None,
			lift: None,
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AvatarMatcapOptions {
	pub scale: f32,
}

impl Default for AvatarMatcapOptions {
	fn default() -> Self {
		Self { scale: 1.0 }
	}
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AvatarSpecularOptions {
	pub enabled: bool,
	pub intensity: f32,
	pub power: f32,
}

impl Default for AvatarSpecularOptions {
	fn default() -> Self {
		Self {
			enabled: false,
			intensity: 0.25,
			power: 24.0,
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AvatarAmbientOcclusionOptions {
	pub strength: f32,
}

impl Default for AvatarAmbientOcclusionOptions {
	fn default() -> Self {
		Self { strength: 1.0 }
	}
}

/// CPU・シェーダ共通のメッシュ表示オプション（切り分け用フラグ含む）。
#[derive(Clone, Debug, Default)]
pub struct SceneMeshLoadOpts {
	pub force_simple_basecolor: bool,
	/// スキニングせず `model * 頂点` のみ（メッシュ有無の確認用）。
	pub debug_bind_pose: bool,
	/// ベーステクスチャを無視しプリミティブごとに単色。
	pub debug_primitive_colors: bool,
	/// 式・default モーフを無視しモーフウェイトを全 0。
	pub debug_zero_morphs: bool,
	/// material alpha が 0 に近い古い虹彩マテリアルだけ強制 Opaque（discard 切り分け）。
	pub relax_iris_alpha: bool,
	/// spec 前の `joint * IBM` のみ（`inv(meshWorld)` なし。エクスポータ差の確認用）。
	pub debug_skin_legacy_no_inv_mesh: bool,
	/// MToon outline 描画を完全にスキップする診断 toggle。
	/// 一部 VRM モデルで目周辺に肌色寄りの太い outline が出る現象の切り分け用。
	pub disable_mtoon_outlines: bool,
	/// MToon の parametric Rim Lighting 寄与を 0 にする診断 toggle。
	/// 目周辺の肌色リング現象が rim light 由来か切り分けるための debug 用。
	pub debug_disable_rim_lighting: bool,
	/// `shading_shift_factor` と `shadingShiftTexture` の寄与をともに 0 に固定する診断 toggle。
	/// shadeColor への falloff 位置を素直に `dot(n, l)` だけにして影色テクスチャの寄与を切り分ける。
	pub debug_force_shading_shift_zero: bool,
	/// matcap (sphere add) の寄与を 0 にする診断 toggle。
	/// matcap で目周辺に擬似的なハイライト/シャドウが乗っているケースを切り分ける。
	pub debug_disable_matcap: bool,
	/// emissive (`emissive_factor × emissive_tex`) 寄与を 0 にする診断 toggle。
	/// 眉間/目周辺に肌色寄りの emissive が焼き込まれているケースを切り分ける。
	pub debug_disable_emissive: bool,
	/// `shade_color × shade_tex` の代わりに base を使う診断 toggle。
	/// shade_term そのものが肌色リングの原因か（=shade_tex の特定領域が肌色寄り）切り分ける。
	pub debug_disable_shade_color: bool,
	/// normalTexture の寄与を 0 にし、頂点法線のみで shading / rim を計算する診断 toggle。
	pub debug_disable_normal_map: bool,
	/// fs_mtoon の出力を `base = alb × base_color.rgb` のみで早期 return する診断 toggle。
	/// shading / GI / rim / matcap / emissive / shade_term を全てスキップ。
	/// これでリングが残るならテクスチャ自身またはメッシュ重なり由来。
	pub debug_base_texture_only: bool,
	/// アバター用途の outline override。既定は VRM / MToon authored outline を尊重する。
	pub avatar_outline: AvatarOutlineOptions,
	/// アバター用途の rim light override。既定は VRM / MToon authored rim を尊重する。
	pub avatar_rim: AvatarRimOptions,
	/// アバター用途の matcap 強度倍率。既定 1.0 は authored 値そのまま。
	pub avatar_matcap: AvatarMatcapOptions,
	/// アバター用途の合成 specular accent。既定 OFF。
	pub avatar_specular: AvatarSpecularOptions,
	/// authored occlusion texture の効き。既定 1.0 は authored 値そのまま。
	pub avatar_ambient_occlusion: AvatarAmbientOcclusionOptions,
	/// 顔と体で別テクスチャの肌色差が首境界に出るモデル向けの実験的なロード時補正。
	pub skin_tone_matching: bool,
}

use wgpu::util::DeviceExt;

const SHADER_MESH: &str = include_str!("../shaders/mesh.wgsl");

/// シェーダとボーンバッファの上限（io-gltf のスキン joint 上限と同値に保つ）。
pub(crate) const MAX_BONES: usize = 512;

const BONE_MATRIX_SIZE: u64 = (16 * std::mem::size_of::<f32>()) as u64;
const MORPH_WEIGHT_BUFFER_MIN_SIZE: u64 = 16;
const MORPH_DELTA_BUFFER_MIN_SIZE: u64 = 16;

#[repr(C)]
#[derive(Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct MeshFrameGpu {
	view_proj: [[f32; 4]; 4],
	light_dir: [f32; 4],
	camera_pos: [f32; 4],
	light_color: [f32; 4],
	ambient_color: [f32; 4],
	_pad: [[f32; 4]; 8],
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct MeshDrawTransformGpu {
	model: [[f32; 4]; 4],
}

/// `params.x` = 未使用（シェーディングはフラグメントエントリ別パイプラインで選択。0 固定）。
/// `params.y` = [`UnaAlphaMode::as_shader_alpha_kind`]（0 OPAQUE / 1 MASK / 2 BLEND）。
/// `params.z` = `alpha_cutoff`（MASK 時）。
/// `params.w` はビットパック `u32` を `f32` で渡す（`bitcast`）。
/// bit0=bind pose rigid, bit1=単色プリミティブ, bit2=Rim Lighting OFF (debug),
/// bit3=shading_shift_factor/shadingShiftTexture を 0 固定 (debug), bit4=matcap OFF (debug),
/// bit5=emissive OFF (debug), bit6=shade_term を base 置換 (debug), bit7=fs_mtoon を base のみで早期 return (debug),
/// bit8=normalTexture OFF (debug), bit9=double-sided material, bit10=occlusion texture available。
#[repr(C)]
#[derive(Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct MeshDrawMaterialGpu {
	base_color: [f32; 4],
	params: [f32; 4],
	shade_color: [f32; 4],
	shading_params: [f32; 4],
	matcap_factor: [f32; 4],
	rim_color: [f32; 4],
	rim_params: [f32; 4],
	outline_color: [f32; 4],
	outline_params: [f32; 4],
	emissive_factor: [f32; 4],
	uv_anim_params: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct MorphMetaGpu {
	target_count: u32,
	vertex_count: u32,
	_pad: [u32; 2],
}

const _: () = assert!(std::mem::size_of::<MeshFrameGpu>() == 256);
const _: () = assert!(std::mem::size_of::<MeshDrawTransformGpu>() == 64);
const _: () = assert!(std::mem::size_of::<MeshDrawMaterialGpu>() == 176);
const _: () = assert!(std::mem::size_of::<MorphMetaGpu>() == 16);

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
	pos: [f32; 3],
	norm: [f32; 3],
	uv: [f32; 2],
	joints: [u16; 4],
	weights: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<Vertex>() == 56);

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct TextureUploadSummary {
	pub image_count: u32,
	pub resized_count: u32,
	pub compression_mode: TextureCompressionMode,
	pub compression_bc_supported: bool,
	pub compression_astc_supported: bool,
	pub compression_etc2_supported: bool,
	pub compressed_count: u32,
	pub compression_fallback_count: u32,
	pub compressed_mip_bytes: u64,
	pub cache_enabled: bool,
	pub cache_hits: u32,
	pub cache_misses: u32,
	pub cache_writes: u32,
	pub compressed_cache_hits: u32,
	pub compressed_cache_misses: u32,
	pub compressed_cache_writes: u32,
	pub source_bytes: u64,
	pub uploaded_mip_bytes: u64,
	pub max_source_dimension: u32,
	pub max_uploaded_dimension: u32,
	pub limit_max_dimension: Option<u32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub skin_tone_matching_debug: Option<SkinToneMatchingDebug>,
}

impl TextureUploadSummary {
	fn record_image(&mut self, source_width: u32, source_height: u32, uploaded_width: u32, uploaded_height: u32, uploaded_mip_bytes: u64) {
		self.image_count += 1;
		self.source_bytes += (source_width as u64) * (source_height as u64) * 4;
		self.uploaded_mip_bytes += uploaded_mip_bytes;
		self.max_source_dimension = self.max_source_dimension.max(source_width.max(source_height));
		self.max_uploaded_dimension = self.max_uploaded_dimension.max(uploaded_width.max(uploaded_height));
		if source_width != uploaded_width || source_height != uploaded_height {
			self.resized_count += 1;
		}
	}
}

#[derive(Clone, Debug)]
pub(crate) struct SceneMeshBuildProgress {
	pub phase: &'static str,
	pub current: u32,
	pub total: u32,
	pub message: String,
}

fn scene_primitive_count(scene: &UnaSceneSnapshot) -> u32 {
	let mut count = 0u32;
	for node in &scene.nodes {
		let Some(mesh_i) = node.mesh else { continue };
		let Some(mesh_prims) = scene.meshes.get(mesh_i) else { continue };
		count = count.saturating_add(mesh_prims.len() as u32);
	}
	count
}

fn scene_texture_upload_step_count(scene: &UnaSceneSnapshot, texture_roles: &[TextureRole], texture_max_dimension: Option<u32>) -> u32 {
	scene
		.images
		.iter()
		.enumerate()
		.map(|(image_index, im)| {
			let role = texture_roles.get(image_index).copied().unwrap_or_default();
			estimated_processed_mip_count(im.width, im.height, texture_max_dimension, role)
		})
		.sum()
}

struct ExpandedPrimitive {
	verts: Vec<Vertex>,
	indices: Vec<u32>,
	morph_pos: Vec<Vec<[f32; 3]>>,
	morph_nrm: Option<Vec<Vec<[f32; 3]>>>,
}

#[derive(Clone, Debug)]
struct ExpressionBinding {
	preset_index: usize,
	morph_target_index: usize,
	weight_scale: f32,
}

#[derive(Clone, Copy, Debug)]
enum DrawPipelineKind {
	OpaqueLit,
	OpaqueUnlit,
	OpaqueMtoon,
	BlendLit,
	BlendUnlit,
	BlendMtoon,
}

#[derive(Clone, Debug)]
struct DrawBatch {
	pipeline: DrawPipelineKind,
	draw_indices: Vec<usize>,
}

fn draw_batch(pipeline: DrawPipelineKind, capacity: usize) -> DrawBatch {
	DrawBatch {
		pipeline,
		draw_indices: Vec::with_capacity(capacity),
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SkinPaletteKey {
	world_node_index: usize,
	skin_index: Option<usize>,
}

struct SkinPalette {
	key: SkinPaletteKey,
	buffer: wgpu::Buffer,
	bind_group: wgpu::BindGroup,
	matrix_capacity: usize,
	static_identity: bool,
	raw: Vec<f32>,
	uploaded: Vec<f32>,
}

struct MeshDraw {
	vertex_buffer: wgpu::Buffer,
	index_buffer: wgpu::Buffer,
	index_format: wgpu::IndexFormat,
	index_count: u32,
	draw_transform: wgpu::Buffer,
	draw_transform_uploaded: Option<MeshDrawTransformGpu>,
	draw_material: wgpu::Buffer,
	bind_material: wgpu::BindGroup,
	skin_palette_index: usize,
	_morph_meta_buffer: wgpu::Buffer,
	morph_weight_buffer: wgpu::Buffer,
	_morph_delta_buffer: wgpu::Buffer,
	morph_bind_group: wgpu::BindGroup,
	world_node_index: usize,
	shading: UnaShadingModel,
	morph_pos: Vec<Vec<[f32; 3]>>,
	default_morph_weights: Vec<f32>,
	expression_bindings: Vec<ExpressionBinding>,
	morph_weights: Vec<f32>,
	morph_weight_scratch: Vec<f32>,
	alpha_mode: UnaAlphaMode,
	material: UnaMaterialPbr,
	mtoon: UnaMtoonMaterial,
	mesh_index: usize,
	primitive_index: usize,
}

#[derive(Default)]
struct DrawBindState {
	frame_bound: bool,
	skin_palette_index: Option<usize>,
}

#[inline]
fn effective_mesh_shading(d: &MeshDraw, opts: &SceneMeshLoadOpts) -> UnaShadingModel {
	if opts.force_simple_basecolor {
		UnaShadingModel::LitLambert
	} else {
		d.shading
	}
}

fn group_draw_indices_by_skin_palette(draws: &[MeshDraw], draw_indices: &mut [usize]) {
	draw_indices.sort_by_key(|&draw_index| draws[draw_index].skin_palette_index);
}

fn build_draw_order(draws: &[MeshDraw], opts: &SceneMeshLoadOpts) -> (Vec<usize>, Vec<DrawBatch>, Vec<usize>, Vec<DrawBatch>) {
	let mut outline_draw_indices = Vec::with_capacity(draws.len());
	let mut transparent_zwrite_draw_indices = Vec::new();
	let batch_capacity = (draws.len() / 10).max(1);
	let mut opaque_batches = vec![
		draw_batch(DrawPipelineKind::OpaqueLit, batch_capacity),
		draw_batch(DrawPipelineKind::OpaqueUnlit, batch_capacity),
		draw_batch(DrawPipelineKind::OpaqueMtoon, batch_capacity),
		draw_batch(DrawPipelineKind::OpaqueLit, batch_capacity),
		draw_batch(DrawPipelineKind::OpaqueUnlit, batch_capacity),
		draw_batch(DrawPipelineKind::OpaqueMtoon, batch_capacity),
	];
	let mut blended_batches = vec![
		draw_batch(DrawPipelineKind::BlendLit, batch_capacity),
		draw_batch(DrawPipelineKind::BlendUnlit, batch_capacity),
		draw_batch(DrawPipelineKind::BlendMtoon, batch_capacity),
	];

	for (draw_index, draw) in draws.iter().enumerate() {
		let shading = effective_mesh_shading(draw, opts);
		if !opts.disable_mtoon_outlines
			&& draw_has_outline(draw, opts)
			&& matches!(draw.alpha_mode, UnaAlphaMode::Opaque | UnaAlphaMode::Mask)
		{
			outline_draw_indices.push(draw_index);
		}

		let shading_index = match shading {
			UnaShadingModel::LitLambert => 0,
			UnaShadingModel::Unlit => 1,
			UnaShadingModel::MToonLike => 2,
		};
		match draw.alpha_mode {
			UnaAlphaMode::Opaque => opaque_batches[shading_index].draw_indices.push(draw_index),
			UnaAlphaMode::Mask => opaque_batches[3 + shading_index].draw_indices.push(draw_index),
			UnaAlphaMode::Blend if draw.mtoon.transparent_with_z_write && shading == UnaShadingModel::MToonLike => {
				transparent_zwrite_draw_indices.push(draw_index);
				blended_batches[shading_index].draw_indices.push(draw_index);
			}
			UnaAlphaMode::Blend => blended_batches[shading_index].draw_indices.push(draw_index),
		}
	}

	group_draw_indices_by_skin_palette(draws, &mut outline_draw_indices);
	group_draw_indices_by_skin_palette(draws, &mut transparent_zwrite_draw_indices);
	for batch in &mut opaque_batches {
		group_draw_indices_by_skin_palette(draws, &mut batch.draw_indices);
	}

	opaque_batches.retain(|batch| !batch.draw_indices.is_empty());
	blended_batches.retain(|batch| !batch.draw_indices.is_empty());
	(outline_draw_indices, opaque_batches, transparent_zwrite_draw_indices, blended_batches)
}

pub(crate) struct SceneMeshes {
	pipeline_outline_mtoon: wgpu::RenderPipeline,
	pipeline_opaque_lit: wgpu::RenderPipeline,
	pipeline_opaque_unlit: wgpu::RenderPipeline,
	pipeline_opaque_mtoon: wgpu::RenderPipeline,
	pipeline_blend_lit: wgpu::RenderPipeline,
	pipeline_blend_unlit: wgpu::RenderPipeline,
	pipeline_blend_mtoon: wgpu::RenderPipeline,
	pipeline_transparent_zprepass_mtoon: wgpu::RenderPipeline,
	frame_buffer: wgpu::Buffer,
	frame_uploaded: Option<MeshFrameGpu>,
	frame_bind_group: wgpu::BindGroup,
	#[allow(dead_code)]
	sampler: wgpu::Sampler,
	#[allow(dead_code)]
	_textures: Vec<wgpu::Texture>,
	draws: Vec<MeshDraw>,
	skin_palettes: Vec<SkinPalette>,
	outline_draw_indices: Vec<usize>,
	opaque_batches: Vec<DrawBatch>,
	transparent_zwrite_draw_indices: Vec<usize>,
	blended_batches: Vec<DrawBatch>,
	texture_summary: TextureUploadSummary,
	expression_names: Vec<String>,
	expression_value_scratch: Vec<f32>,
	opts: SceneMeshLoadOpts,
}

fn expand_primitive(buf: &UnaMeshBuffers) -> Option<ExpandedPrimitive> {
	let default_n = [0.0_f32, 1.0, 0.0];
	let positions = &buf.positions;
	if positions.is_empty() {
		return None;
	}
	let normals = buf.normals.as_deref();
	let uvs = buf.tex_coords_0.as_deref();
	let joints_buf = buf.joints.as_deref();
	let weights_buf = buf.weights.as_deref();
	let j_default = [0u16; 4];
	let w_default = [1.0_f32, 0.0, 0.0, 0.0];

	let num_morph = buf.morph_targets.len();
	let vertex_capacity = positions.len();
	let mut morph_push: Vec<Vec<[f32; 3]>> = (0..num_morph).map(|_| Vec::with_capacity(vertex_capacity)).collect();
	let has_morph_normals = buf.morph_targets.iter().any(|target| target.normal_deltas.is_some());
	let mut morph_nrm_push: Option<Vec<Vec<[f32; 3]>>> = if has_morph_normals {
		Some((0..num_morph).map(|_| Vec::with_capacity(vertex_capacity)).collect())
	} else {
		None
	};

	let mut verts = Vec::with_capacity(positions.len());
	for pi in 0..positions.len() {
		let n = normals.and_then(|nn| nn.get(pi)).copied().unwrap_or(default_n);
		let uv = uvs.and_then(|uu| uu.get(pi)).copied().unwrap_or([0.0, 0.0]);
		let jo = joints_buf.and_then(|jj| jj.get(pi)).copied().unwrap_or(j_default);
		let we = weights_buf.and_then(|ww| ww.get(pi)).copied().unwrap_or(w_default);
		verts.push(Vertex {
			pos: positions[pi],
			norm: n,
			uv,
			joints: jo,
			weights: we,
		});
		for (target, bucket) in buf.morph_targets.iter().zip(morph_push.iter_mut()) {
			let d = target.position_deltas.get(pi).copied().unwrap_or([0.0, 0.0, 0.0]);
			bucket.push(d);
		}
		if let Some(ref mut normal_buckets) = morph_nrm_push {
			for (target, bucket) in buf.morph_targets.iter().zip(normal_buckets.iter_mut()) {
				let nd = target
					.normal_deltas
					.as_ref()
					.and_then(|n| n.get(pi))
					.copied()
					.unwrap_or([0.0, 0.0, 0.0]);
				bucket.push(nd);
			}
		}
	}

	let indices = match &buf.indices {
		Some(idx) => {
			let mut out_idx = Vec::with_capacity(idx.len());
			for &pi in idx {
				if (pi as usize) < positions.len() {
					out_idx.push(pi);
				}
			}
			out_idx
		}
		None => (0..positions.len() as u32).collect(),
	};

	if verts.is_empty() || indices.is_empty() {
		return None;
	}

	Some(ExpandedPrimitive {
		verts,
		indices,
		morph_pos: morph_push,
		morph_nrm: morph_nrm_push,
	})
}

fn expression_binding_index(catalog: Option<&UnaExpressionCatalog>) -> BTreeMap<(usize, usize), Vec<ExpressionBinding>> {
	let Some(catalog) = catalog else {
		return BTreeMap::new();
	};
	let mut index = BTreeMap::new();
	for (preset_index, preset) in catalog.presets.iter().enumerate() {
		for bind in &preset.binds {
			index
				.entry((bind.mesh_index, bind.primitive_index))
				.or_insert_with(|| Vec::with_capacity(1))
				.push(ExpressionBinding {
					preset_index,
					morph_target_index: bind.morph_target_index,
					weight_scale: bind.weight_scale,
				});
		}
	}
	index
}

fn mesh_draw_capacity(scene: &UnaSceneSnapshot) -> usize {
	scene
		.nodes
		.iter()
		.filter_map(|node| node.mesh)
		.filter_map(|mesh_index| scene.meshes.get(mesh_index))
		.map(Vec::len)
		.sum()
}

fn scene_effective_visibility(scene: &UnaSceneSnapshot) -> Vec<bool> {
	fn visit(scene: &UnaSceneSnapshot, idx: usize, parent_visible: bool, out: &mut [bool]) {
		let Some(node) = scene.nodes.get(idx) else { return };
		let visible = parent_visible && node.visible;
		if let Some(slot) = out.get_mut(idx) {
			*slot = visible;
		}
		for &child in &node.children {
			visit(scene, child, visible, out);
		}
	}

	let mut out = vec![false; scene.nodes.len()];
	for &root in &scene.roots {
		visit(scene, root, true, &mut out);
	}
	out
}

fn skin_palette_capacity(scene: &UnaSceneSnapshot) -> usize {
	scene.nodes.iter().filter(|node| node.mesh.is_some()).count()
}

fn expression_names(catalog: Option<&UnaExpressionCatalog>) -> Vec<String> {
	catalog
		.map(|catalog| catalog.presets.iter().map(|preset| preset.name.clone()).collect())
		.unwrap_or_default()
}

fn fill_morph_weights_for_draw(
	default_morph_weights: &[f32],
	target_count: usize,
	bindings: &[ExpressionBinding],
	expression_values: Option<&[f32]>,
	out: &mut Vec<f32>,
) {
	out.clear();
	out.resize(target_count, 0.0);
	if target_count == 0 {
		return;
	}
	let copy_len = default_morph_weights.len().min(target_count);
	out[..copy_len].copy_from_slice(&default_morph_weights[..copy_len]);
	let Some(expression_values) = expression_values else { return };
	for binding in bindings {
		let pw = expression_values.get(binding.preset_index).copied().unwrap_or(0.0);
		if pw == 0.0 {
			continue;
		}
		let Some(slot) = out.get_mut(binding.morph_target_index) else {
			continue;
		};
		*slot = (*slot + pw * binding.weight_scale).clamp(0.0, 1.0);
	}
}

fn expression_bindings_have_active_weight(bindings: &[ExpressionBinding], expression_values: Option<&[f32]>) -> bool {
	let Some(expression_values) = expression_values else {
		return false;
	};
	bindings
		.iter()
		.any(|binding| expression_values.get(binding.preset_index).copied().unwrap_or(0.0) != 0.0)
}

fn morph_weights_match_default(uploaded: &[f32], default_morph_weights: &[f32], target_count: usize) -> bool {
	if uploaded.len() != target_count {
		return false;
	}
	let copy_len = default_morph_weights.len().min(target_count);
	uploaded[..copy_len] == default_morph_weights[..copy_len] && uploaded[copy_len..].iter().all(|&weight| weight == 0.0)
}

fn morph_delta_data(morph_pos: &[Vec<[f32; 3]>], morph_nrm: Option<&[Vec<[f32; 3]>]>, vertex_count: usize) -> Vec<[f32; 4]> {
	let mut out = Vec::with_capacity(morph_pos.len().saturating_mul(vertex_count).saturating_mul(2).max(1));
	for (target_index, target_pos) in morph_pos.iter().enumerate() {
		let target_nrm = morph_nrm.and_then(|all| all.get(target_index));
		for vertex_index in 0..vertex_count {
			let pos = target_pos.get(vertex_index).copied().unwrap_or([0.0; 3]);
			let nrm = target_nrm.and_then(|target| target.get(vertex_index)).copied().unwrap_or([0.0; 3]);
			out.push([pos[0], pos[1], pos[2], 0.0]);
			out.push([nrm[0], nrm[1], nrm[2], 0.0]);
		}
	}
	if out.is_empty() {
		out.push([0.0; 4]);
	}
	out
}

fn compact_index_format(indices: &[u32]) -> wgpu::IndexFormat {
	if indices.iter().all(|&index| index <= u16::MAX as u32) {
		wgpu::IndexFormat::Uint16
	} else {
		wgpu::IndexFormat::Uint32
	}
}

fn write_matrix_to_raw(raw: &mut Vec<f32>, matrix: Mat4) {
	raw.extend_from_slice(&matrix.to_cols_array());
}

fn identity_matrix_raw() -> Vec<f32> {
	let mut raw = Vec::with_capacity(matrix_raw_capacity(1));
	write_matrix_to_raw(&mut raw, Mat4::IDENTITY);
	raw
}

fn matrix_raw_capacity(matrix_count: usize) -> usize {
	matrix_count.saturating_mul(16).max(16)
}

fn outline_mode_gpu(mode: UnaMtoonOutlineWidthMode) -> f32 {
	match mode {
		UnaMtoonOutlineWidthMode::None => 0.0,
		UnaMtoonOutlineWidthMode::WorldCoordinates => 1.0,
		UnaMtoonOutlineWidthMode::ScreenCoordinates => 2.0,
	}
}

fn texture_view_or<'a>(views: &'a [wgpu::TextureView], index: Option<usize>, fallback: &'a wgpu::TextureView) -> &'a wgpu::TextureView {
	match index {
		Some(ti) if ti < views.len() => &views[ti],
		_ => fallback,
	}
}

fn create_solid_texture_1x1(
	device: &wgpu::Device,
	queue: &wgpu::Queue,
	label: &'static str,
	format: wgpu::TextureFormat,
	rgba: [u8; 4],
) -> wgpu::Texture {
	let texture = device.create_texture(&wgpu::TextureDescriptor {
		label: Some(label),
		size: wgpu::Extent3d {
			width: 1,
			height: 1,
			depth_or_array_layers: 1,
		},
		mip_level_count: 1,
		sample_count: 1,
		dimension: wgpu::TextureDimension::D2,
		format,
		usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
		view_formats: &[],
	});
	queue.write_texture(
		texture.as_image_copy(),
		&rgba,
		wgpu::TexelCopyBufferLayout {
			offset: 0,
			bytes_per_row: Some(4),
			rows_per_image: None,
		},
		wgpu::Extent3d {
			width: 1,
			height: 1,
			depth_or_array_layers: 1,
		},
	);
	texture
}

fn draw_has_outline(d: &MeshDraw, opts: &SceneMeshLoadOpts) -> bool {
	match opts.avatar_outline.policy {
		AvatarOutlinePolicy::Override => false,
		AvatarOutlinePolicy::Authored => d.shading == UnaShadingModel::MToonLike && effective_mtoon_outline(&d.mtoon, opts).is_some(),
		AvatarOutlinePolicy::Off => false,
	}
}

fn mesh_draw_material_gpu(
	mat: &UnaMaterialPbr,
	mtoon: &UnaMtoonMaterial,
	opts: &SceneMeshLoadOpts,
	mesh_index: usize,
	prim_index: usize,
) -> MeshDrawMaterialGpu {
	let iris_relax = opts.relax_iris_alpha && iris_like_material_name(mat.name.as_deref()) && mat.base_color_factor[3] <= 0.001;
	let mut eff_alpha = mat.alpha_mode;
	let mut base_color = mat.base_color_factor;
	if opts.force_simple_basecolor || iris_relax {
		eff_alpha = UnaAlphaMode::Opaque;
		base_color[3] = 1.0;
	}
	if opts.debug_primitive_colors {
		base_color = debug_primitive_color_rgba(mesh_index, prim_index);
	}
	let mut flags: u32 = 0;
	if opts.debug_bind_pose {
		flags |= 1;
	}
	if opts.debug_primitive_colors {
		flags |= 2;
	}
	if opts.debug_disable_rim_lighting && opts.avatar_rim.policy != AvatarRimPolicy::Override {
		flags |= 4;
	}
	if opts.debug_force_shading_shift_zero {
		flags |= 8;
	}
	if opts.debug_disable_matcap {
		flags |= 16;
	}
	if opts.debug_disable_emissive {
		flags |= 32;
	}
	if opts.debug_disable_shade_color {
		flags |= 64;
	}
	if opts.skin_tone_matching && material_skin_tone_kind(mat).is_some() {
		flags |= 64;
	}
	if opts.debug_base_texture_only {
		flags |= 128;
	}
	if opts.debug_disable_normal_map {
		flags |= 256;
	}
	if mat.double_sided {
		flags |= 512;
	}
	if mat.occlusion_texture_index.is_some() {
		flags |= 1024;
	}
	let (rim_color, rim_lighting_mix, rim_power, rim_lift) = effective_mtoon_rim(mat, mtoon, opts);
	let rim_texture_mix = if opts.avatar_rim.policy == AvatarRimPolicy::Override {
		0.0
	} else {
		1.0
	};
	let outline = effective_mtoon_outline(mtoon, opts);
	let (outline_mode, outline_width, outline_color, outline_lighting_mix) = outline
		.map(|o| (o.mode, o.width, o.color, o.lighting_mix))
		.unwrap_or((UnaMtoonOutlineWidthMode::None, 0.0, [0.0, 0.0, 0.0], 0.0));
	MeshDrawMaterialGpu {
		base_color,
		params: [0.0, eff_alpha.as_shader_alpha_kind(), mat.alpha_cutoff, f32::from_bits(flags)],
		shade_color: [
			mtoon.shade_color_factor[0],
			mtoon.shade_color_factor[1],
			mtoon.shade_color_factor[2],
			if mat.normal_texture_index.is_some() {
				mat.normal_texture_scale
			} else {
				0.0
			},
		],
		shading_params: [
			mtoon.shading_shift_factor,
			mtoon.shading_toony_factor,
			mtoon.shading_shift_texture_scale,
			mtoon.gi_equalization_factor,
		],
		matcap_factor: [
			mtoon.matcap_factor[0],
			mtoon.matcap_factor[1],
			mtoon.matcap_factor[2],
			opts.avatar_matcap.scale.clamp(0.0, 2.0),
		],
		rim_color: [
			rim_color[0],
			rim_color[1],
			rim_color[2],
			(mat.occlusion_texture_strength * opts.avatar_ambient_occlusion.strength).clamp(0.0, 2.0),
		],
		rim_params: [rim_lighting_mix, rim_power, rim_lift, rim_texture_mix],
		outline_color: [outline_color[0], outline_color[1], outline_color[2], 0.0],
		outline_params: [
			outline_mode_gpu(outline_mode),
			outline_width,
			outline_lighting_mix,
			if mtoon.transparent_with_z_write { 1.0 } else { 0.0 },
		],
		emissive_factor: [
			mat.emissive_factor[0],
			mat.emissive_factor[1],
			mat.emissive_factor[2],
			if opts.avatar_specular.enabled {
				opts.avatar_specular.power.clamp(1.0, 128.0)
			} else {
				24.0
			},
		],
		uv_anim_params: [
			mtoon.uv_animation_scroll_x_speed_factor,
			mtoon.uv_animation_scroll_y_speed_factor,
			mtoon.uv_animation_rotation_speed_factor,
			if opts.avatar_specular.enabled {
				opts.avatar_specular.intensity.clamp(0.0, 2.0)
			} else {
				0.0
			},
		],
	}
}

impl SceneMeshes {
	#[allow(clippy::too_many_arguments)]
	fn create_mesh_pipeline(
		device: &wgpu::Device,
		pipeline_layout: &wgpu::PipelineLayout,
		shader: &wgpu::ShaderModule,
		format: wgpu::TextureFormat,
		vb_layout: &wgpu::VertexBufferLayout<'_>,
		label: &'static str,
		vertex_entry: &'static str,
		fragment_entry: &'static str,
		color_blend: Option<wgpu::BlendState>,
		color_write_mask: wgpu::ColorWrites,
		depth_write: bool,
		depth_compare: wgpu::CompareFunction,
		cull_mode: Option<wgpu::Face>,
		sample_count: u32,
	) -> wgpu::RenderPipeline {
		device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some(label),
			layout: Some(pipeline_layout),
			cache: None,
			vertex: wgpu::VertexState {
				module: shader,
				entry_point: Some(vertex_entry),
				compilation_options: Default::default(),
				buffers: std::slice::from_ref(vb_layout),
			},
			fragment: Some(wgpu::FragmentState {
				module: shader,
				entry_point: Some(fragment_entry),
				compilation_options: Default::default(),
				targets: &[Some(wgpu::ColorTargetState {
					format,
					blend: color_blend,
					write_mask: color_write_mask,
				})],
			}),
			primitive: wgpu::PrimitiveState {
				topology: wgpu::PrimitiveTopology::TriangleList,
				cull_mode,
				..Default::default()
			},
			depth_stencil: Some(wgpu::DepthStencilState {
				format: wgpu::TextureFormat::Depth24Plus,
				depth_write_enabled: Some(depth_write),
				depth_compare: Some(depth_compare),
				stencil: wgpu::StencilState::default(),
				bias: wgpu::DepthBiasState::default(),
			}),
			multisample: wgpu::MultisampleState {
				count: sample_count,
				..Default::default()
			},
			multiview_mask: None,
		})
	}

	#[allow(clippy::too_many_arguments)]
	pub fn new(
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		format: wgpu::TextureFormat,
		sample_count: u32,
		scene: &UnaSceneSnapshot,
		catalog: Option<&UnaExpressionCatalog>,
		opts: SceneMeshLoadOpts,
		texture_max_dimension: Option<u32>,
		texture_compression: TextureCompressionMode,
		block_compression_encoder: BlockCompressionEncoder,
		block_compression_cpu_threads: usize,
		mipmap_filter: TextureMipmapFilter,
		texture_compression_advanced: &TextureCompressionAdvancedOptions,
		texture_compression_bc_supported: bool,
		texture_compression_astc_supported: bool,
		texture_compression_etc2_supported: bool,
		processed_texture_cache: bool,
		mut gpu_texture_compression: Option<&mut GpuTextureCompressionContext>,
		mut progress: impl FnMut(SceneMeshBuildProgress),
	) -> Result<Self, String> {
		let texture_roles = texture_roles_for_scene(scene);
		let skin_tone_kinds = if opts.skin_tone_matching {
			skin_tone_texture_kinds_for_scene(scene)
		} else {
			Vec::new()
		};
		let (skin_tone_matched_images, skin_tone_matching_debug) = if opts.skin_tone_matching {
			let scene_world = scene_world_matrices(scene);
			let (images, debug) = build_skin_tone_matched_images(scene, &scene_world, &skin_tone_kinds);
			(images, Some(debug))
		} else {
			(
				Vec::new(),
				Some(SkinToneMatchingDebug {
					enabled: false,
					..Default::default()
				}),
			)
		};
		let total_steps = 3u32
			.saturating_add(scene_texture_upload_step_count(scene, &texture_roles, texture_max_dimension))
			.saturating_add(scene_primitive_count(scene))
			.max(1);
		let mut current_step = 0u32;
		let mut report = |phase: &'static str, message: String| {
			current_step = current_step.saturating_add(1).min(total_steps);
			progress(SceneMeshBuildProgress {
				phase,
				current: current_step,
				total: total_steps,
				message,
			});
		};
		report("gpu-upload", "Preparing GPU scene layouts".to_string());
		let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("mesh_frame"),
			entries: &[wgpu::BindGroupLayoutEntry {
				binding: 0,
				visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
				ty: wgpu::BindingType::Buffer {
					ty: wgpu::BufferBindingType::Uniform,
					has_dynamic_offset: false,
					min_binding_size: None,
				},
				count: None,
			}],
		});

		let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("mesh_material"),
			entries: &[
				wgpu::BindGroupLayoutEntry {
					binding: 0,
					visibility: wgpu::ShaderStages::VERTEX,
					ty: wgpu::BindingType::Buffer {
						ty: wgpu::BufferBindingType::Uniform,
						has_dynamic_offset: false,
						min_binding_size: None,
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 1,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 2,
					visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 3,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 4,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 5,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 6,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 7,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 8,
					visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 9,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 10,
					visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Buffer {
						ty: wgpu::BufferBindingType::Uniform,
						has_dynamic_offset: false,
						min_binding_size: None,
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 11,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 12,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 13,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
			],
		});

		let skin_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("mesh_skin"),
			entries: &[wgpu::BindGroupLayoutEntry {
				binding: 0,
				visibility: wgpu::ShaderStages::VERTEX,
				ty: wgpu::BindingType::Buffer {
					ty: wgpu::BufferBindingType::Storage { read_only: true },
					has_dynamic_offset: false,
					min_binding_size: None,
				},
				count: None,
			}],
		});

		let morph_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("mesh_morph"),
			entries: &[
				wgpu::BindGroupLayoutEntry {
					binding: 0,
					visibility: wgpu::ShaderStages::VERTEX,
					ty: wgpu::BindingType::Buffer {
						ty: wgpu::BufferBindingType::Uniform,
						has_dynamic_offset: false,
						min_binding_size: None,
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 1,
					visibility: wgpu::ShaderStages::VERTEX,
					ty: wgpu::BindingType::Buffer {
						ty: wgpu::BufferBindingType::Storage { read_only: true },
						has_dynamic_offset: false,
						min_binding_size: None,
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 2,
					visibility: wgpu::ShaderStages::VERTEX,
					ty: wgpu::BindingType::Buffer {
						ty: wgpu::BufferBindingType::Storage { read_only: true },
						has_dynamic_offset: false,
						min_binding_size: None,
					},
					count: None,
				},
			],
		});

		let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("mesh"),
			bind_group_layouts: &[
				Some(&frame_layout),
				Some(&material_layout),
				Some(&skin_bind_group_layout),
				Some(&morph_bind_group_layout),
			],
			immediate_size: 0,
		});

		let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
			label: Some("mesh"),
			source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER_MESH)),
		});

		const MESH_VTX_ATTRS: [wgpu::VertexAttribute; 5] = [
			wgpu::VertexAttribute {
				offset: 0,
				shader_location: 0,
				format: wgpu::VertexFormat::Float32x3,
			},
			wgpu::VertexAttribute {
				offset: 12,
				shader_location: 1,
				format: wgpu::VertexFormat::Float32x3,
			},
			wgpu::VertexAttribute {
				offset: 24,
				shader_location: 2,
				format: wgpu::VertexFormat::Float32x2,
			},
			wgpu::VertexAttribute {
				offset: 32,
				shader_location: 3,
				format: wgpu::VertexFormat::Uint16x4,
			},
			wgpu::VertexAttribute {
				offset: 40,
				shader_location: 4,
				format: wgpu::VertexFormat::Float32x4,
			},
		];
		let vb_layout = wgpu::VertexBufferLayout {
			array_stride: std::mem::size_of::<Vertex>() as u64,
			step_mode: wgpu::VertexStepMode::Vertex,
			attributes: &MESH_VTX_ATTRS,
		};

		let pipeline_outline_mtoon = Self::create_mesh_pipeline(
			device,
			&pipeline_layout,
			&shader,
			format,
			&vb_layout,
			"mesh_outline_mtoon",
			"vs_outline",
			"fs_outline",
			None,
			wgpu::ColorWrites::ALL,
			false,
			wgpu::CompareFunction::LessEqual,
			Some(wgpu::Face::Front),
			sample_count,
		);
		let pipeline_opaque_lit = Self::create_mesh_pipeline(
			device,
			&pipeline_layout,
			&shader,
			format,
			&vb_layout,
			"mesh_opaque_lit",
			"vs_main",
			"fs_lit",
			None,
			wgpu::ColorWrites::ALL,
			true,
			wgpu::CompareFunction::LessEqual,
			None,
			sample_count,
		);
		let pipeline_opaque_unlit = Self::create_mesh_pipeline(
			device,
			&pipeline_layout,
			&shader,
			format,
			&vb_layout,
			"mesh_opaque_unlit",
			"vs_main",
			"fs_unlit",
			None,
			wgpu::ColorWrites::ALL,
			true,
			wgpu::CompareFunction::LessEqual,
			None,
			sample_count,
		);
		let pipeline_opaque_mtoon = Self::create_mesh_pipeline(
			device,
			&pipeline_layout,
			&shader,
			format,
			&vb_layout,
			"mesh_opaque_mtoon",
			"vs_main",
			"fs_mtoon",
			None,
			wgpu::ColorWrites::ALL,
			true,
			wgpu::CompareFunction::LessEqual,
			None,
			sample_count,
		);
		let blend = Some(wgpu::BlendState::ALPHA_BLENDING);
		let pipeline_blend_lit = Self::create_mesh_pipeline(
			device,
			&pipeline_layout,
			&shader,
			format,
			&vb_layout,
			"mesh_blend_lit",
			"vs_main",
			"fs_lit",
			blend,
			wgpu::ColorWrites::ALL,
			false,
			wgpu::CompareFunction::LessEqual,
			None,
			sample_count,
		);
		let pipeline_blend_unlit = Self::create_mesh_pipeline(
			device,
			&pipeline_layout,
			&shader,
			format,
			&vb_layout,
			"mesh_blend_unlit",
			"vs_main",
			"fs_unlit",
			blend,
			wgpu::ColorWrites::ALL,
			false,
			wgpu::CompareFunction::LessEqual,
			None,
			sample_count,
		);
		let pipeline_blend_mtoon = Self::create_mesh_pipeline(
			device,
			&pipeline_layout,
			&shader,
			format,
			&vb_layout,
			"mesh_blend_mtoon",
			"vs_main",
			"fs_mtoon",
			blend,
			wgpu::ColorWrites::ALL,
			false,
			wgpu::CompareFunction::LessEqual,
			None,
			sample_count,
		);
		let pipeline_transparent_zprepass_mtoon = Self::create_mesh_pipeline(
			device,
			&pipeline_layout,
			&shader,
			format,
			&vb_layout,
			"mesh_transparent_zprepass_mtoon",
			"vs_main",
			"fs_mtoon",
			None,
			wgpu::ColorWrites::empty(),
			true,
			wgpu::CompareFunction::LessEqual,
			None,
			sample_count,
		);
		report("gpu-upload", "Preparing mesh frame buffers".to_string());
		let frame_buffer = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("mesh_frame"),
			size: std::mem::size_of::<MeshFrameGpu>() as u64,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});

		let frame_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("mesh_frame"),
			layout: &frame_layout,
			entries: &[wgpu::BindGroupEntry {
				binding: 0,
				resource: frame_buffer.as_entire_binding(),
			}],
		});

		let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
			label: Some("mesh"),
			address_mode_u: wgpu::AddressMode::ClampToEdge,
			address_mode_v: wgpu::AddressMode::ClampToEdge,
			mag_filter: wgpu::FilterMode::Linear,
			min_filter: wgpu::FilterMode::Linear,
			mipmap_filter: wgpu::MipmapFilterMode::Linear,
			anisotropy_clamp: 4,
			..Default::default()
		});

		let mut textures: Vec<wgpu::Texture> = Vec::with_capacity(scene.images.len() + 3);

		let white_texture = create_solid_texture_1x1(device, queue, "white1x1", wgpu::TextureFormat::Rgba8UnormSrgb, [255, 255, 255, 255]);
		textures.push(white_texture);
		let white_view = textures[0].create_view(&wgpu::TextureViewDescriptor::default());
		let black_texture = create_solid_texture_1x1(device, queue, "black1x1", wgpu::TextureFormat::Rgba8UnormSrgb, [0, 0, 0, 255]);
		textures.push(black_texture);
		let black_view = textures[1].create_view(&wgpu::TextureViewDescriptor::default());
		let neutral_normal_texture = create_solid_texture_1x1(
			device,
			queue,
			"neutral_normal1x1",
			wgpu::TextureFormat::Rgba8Unorm,
			[128, 128, 255, 255],
		);
		textures.push(neutral_normal_texture);
		let neutral_normal_view = textures[2].create_view(&wgpu::TextureViewDescriptor::default());
		report("gpu-upload", "Uploading fallback textures".to_string());
		let mut texture_summary = TextureUploadSummary {
			limit_max_dimension: texture_max_dimension,
			compression_mode: texture_compression,
			compression_bc_supported: texture_compression_bc_supported,
			compression_astc_supported: texture_compression_astc_supported,
			compression_etc2_supported: texture_compression_etc2_supported,
			cache_enabled: processed_texture_cache,
			skin_tone_matching_debug,
			..Default::default()
		};

		for (image_index, im) in scene.images.iter().enumerate() {
			let src_w = im.width.max(1);
			let src_h = im.height.max(1);
			let role = texture_roles.get(image_index).copied().unwrap_or_default();
			let skin_tone_override = skin_tone_matched_images.get(image_index).and_then(Option::as_deref);
			if texture_max_dimension.is_none() && skin_tone_override.is_none() && texture_compression != TextureCompressionMode::Compat {
				if let Some(source_upload) = source_texture_upload(im) {
					report(
						"gpu-upload",
						format!(
							"Uploading precision-preserving source texture {}/{} {}x{} {:?} ({role:?})",
							image_index + 1,
							scene.images.len(),
							source_upload.width,
							source_upload.height,
							source_upload.format
						),
					);
					texture_summary.record_image(
						src_w,
						src_h,
						source_upload.width,
						source_upload.height,
						source_upload.data.len() as u64,
					);
					let tex = device.create_texture(&wgpu::TextureDescriptor {
						label: Some("gltf_image_source"),
						size: wgpu::Extent3d {
							width: source_upload.width,
							height: source_upload.height,
							depth_or_array_layers: 1,
						},
						mip_level_count: 1,
						sample_count: 1,
						dimension: wgpu::TextureDimension::D2,
						format: source_upload.format,
						usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
						view_formats: &[],
					});
					queue.write_texture(
						wgpu::TexelCopyTextureInfo {
							texture: &tex,
							mip_level: 0,
							origin: wgpu::Origin3d::ZERO,
							aspect: wgpu::TextureAspect::All,
						},
						&source_upload.data,
						wgpu::TexelCopyBufferLayout {
							offset: 0,
							bytes_per_row: Some(source_upload.bytes_per_row),
							rows_per_image: Some(source_upload.height),
						},
						wgpu::Extent3d {
							width: source_upload.width,
							height: source_upload.height,
							depth_or_array_layers: 1,
						},
					);
					textures.push(tex);
					continue;
				}
			}
			let rgba_compat = im.rgba8_compat_pixels();
			let rgba = skin_tone_override.unwrap_or(rgba_compat.as_ref());
			let source_key = if skin_tone_override.is_none() {
				scene.image_sources.get(image_index).and_then(Option::as_ref).map_or_else(
					|| texture_cache_key(src_w, src_h, texture_max_dimension, role, mipmap_filter, rgba),
					|source| texture_cache_key_from_source_metadata(src_w, src_h, texture_max_dimension, role, mipmap_filter, source),
				)
			} else {
				texture_cache_key(src_w, src_h, texture_max_dimension, role, mipmap_filter, rgba)
			};
			let compressed_cache_lookup = processed_texture_cache.then(|| {
				compressed_cache_lookup_from_source(
					rgba,
					src_w,
					src_h,
					texture_max_dimension,
					role,
					texture_compression,
					texture_compression_advanced,
					texture_compression_bc_supported,
					source_key,
				)
			});
			let compressed_cache_lookup = compressed_cache_lookup.flatten();
			let compressed_cache_hit = compressed_cache_lookup.as_ref().and_then(|lookup| {
				read_compressed_texture_cache(&lookup.path, lookup.key, lookup.kind).map(|payload| {
					(
						payload,
						TextureCacheEvent {
							hit: true,
							miss: false,
							write: false,
						},
						lookup.processed_width,
						lookup.processed_height,
					)
				})
			});
			let (payload, cache_event, compressed_cache_event, processed_w, processed_h) =
				if let Some((payload, compressed_cache_event, processed_w, processed_h)) = compressed_cache_hit {
					(
						payload,
						TextureCacheEvent::DISABLED,
						compressed_cache_event,
						processed_w,
						processed_h,
					)
				} else {
					let (processed, cache_event) = load_or_build_processed_texture(
						rgba,
						src_w,
						src_h,
						texture_max_dimension,
						role,
						mipmap_filter,
						processed_texture_cache,
						source_key,
					);
					let processed_w = processed.width;
					let processed_h = processed.height;
					let (payload, compressed_cache_event) = texture_upload_payload(
						processed,
						texture_compression,
						texture_compression_advanced,
						role,
						texture_compression_bc_supported,
						block_compression_encoder,
						block_compression_cpu_threads,
						gpu_texture_compression.as_deref_mut(),
						processed_texture_cache,
						compressed_cache_lookup.as_ref(),
						compressed_cache_lookup.is_some(),
					);
					(payload, cache_event, compressed_cache_event, processed_w, processed_h)
				};
			// 圧縮テクスチャは block 整列 (4 の倍数) に padding されているため、テクスチャ次元・サンプリング寸法も
			// payload の最上位 mip サイズに揃える。非4倍数寸法を元の論理寸法へ戻すと BCn upload が停止する。
			// 非圧縮 (Rgba) は元の processed 寸法と一致する。
			let (w, h) = payload
				.mips
				.first()
				.map_or((processed_w, processed_h), |mip| (mip.width, mip.height));
			if compressed_cache_event.hit {
				texture_summary.compressed_cache_hits += 1;
			}
			if compressed_cache_event.miss {
				texture_summary.compressed_cache_misses += 1;
			}
			if compressed_cache_event.write {
				texture_summary.compressed_cache_writes += 1;
			}
			if cache_event.hit {
				texture_summary.cache_hits += 1;
			}
			if cache_event.miss {
				texture_summary.cache_misses += 1;
			}
			if cache_event.write {
				texture_summary.cache_writes += 1;
			}
			if payload.kind.is_compressed() {
				texture_summary.compressed_count += 1;
				texture_summary.compressed_mip_bytes += payload.byte_len();
			} else if compression_preference_for_role(texture_compression, texture_compression_advanced, role)
				!= TextureCompressionPreference::Source
			{
				texture_summary.compression_fallback_count += 1;
			}
			texture_summary.record_image(src_w, src_h, w, h, payload.byte_len());
			let texture_format = match payload.kind {
				TextureUploadKind::Rgba if role == TextureRole::Normal => wgpu::TextureFormat::Rgba8Unorm,
				TextureUploadKind::Rgba => wgpu::TextureFormat::Rgba8UnormSrgb,
				TextureUploadKind::Bc1Srgb => wgpu::TextureFormat::Bc1RgbaUnormSrgb,
				TextureUploadKind::Bc5Unorm => wgpu::TextureFormat::Bc5RgUnorm,
				TextureUploadKind::Bc7Srgb => wgpu::TextureFormat::Bc7RgbaUnormSrgb,
			};
			let tex = device.create_texture(&wgpu::TextureDescriptor {
				label: Some("gltf_image"),
				size: wgpu::Extent3d {
					width: w,
					height: h,
					depth_or_array_layers: 1,
				},
				mip_level_count: payload.mips.len() as u32,
				sample_count: 1,
				dimension: wgpu::TextureDimension::D2,
				format: texture_format,
				usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
				view_formats: &[],
			});
			for (mip_level, mip) in payload.mips.iter().enumerate() {
				report(
					"gpu-upload",
					format!(
						"Uploading texture {}/{} mip {}/{} {}x{} ({role:?})",
						image_index + 1,
						scene.images.len(),
						mip_level + 1,
						payload.mips.len(),
						mip.width,
						mip.height
					),
				);
				let (bytes_per_row, rows_per_image) = match payload.kind {
					TextureUploadKind::Rgba => (4 * mip.width, mip.height),
					TextureUploadKind::Bc1Srgb => (mip.width.div_ceil(4) * 8, mip.height.div_ceil(4)),
					TextureUploadKind::Bc5Unorm | TextureUploadKind::Bc7Srgb => (mip.width.div_ceil(4) * 16, mip.height.div_ceil(4)),
				};
				queue.write_texture(
					wgpu::TexelCopyTextureInfo {
						texture: &tex,
						mip_level: mip_level as u32,
						origin: wgpu::Origin3d::ZERO,
						aspect: wgpu::TextureAspect::All,
					},
					&mip.data,
					wgpu::TexelCopyBufferLayout {
						offset: 0,
						bytes_per_row: Some(bytes_per_row),
						rows_per_image: Some(rows_per_image),
					},
					wgpu::Extent3d {
						width: mip.width,
						height: mip.height,
						depth_or_array_layers: 1,
					},
				);
			}
			textures.push(tex);
		}

		let image_views: Vec<wgpu::TextureView> = textures
			.iter()
			.skip(3)
			.map(|t| t.create_view(&wgpu::TextureViewDescriptor::default()))
			.collect();

		let expression_names = expression_names(catalog);
		let expression_bindings = expression_binding_index(catalog);
		let effective_visibility = scene_effective_visibility(scene);
		let mut draws = Vec::with_capacity(mesh_draw_capacity(scene));
		let mut skin_palettes = Vec::with_capacity(skin_palette_capacity(scene));
		let mut skin_palette_indices = BTreeMap::new();
		for (ni, node) in scene.nodes.iter().enumerate() {
			if !effective_visibility.get(ni).copied().unwrap_or(false) {
				continue;
			}
			let Some(mesh_i) = node.mesh else { continue };
			let Some(mesh_prims) = scene.meshes.get(mesh_i) else { continue };
			for (prim_i, buf) in mesh_prims.iter().enumerate() {
				report("gpu-upload", format!("Preparing mesh {mesh_i} primitive {prim_i}"));
				let Some(exp) = expand_primitive(buf) else { continue };
				let ExpandedPrimitive {
					mut verts,
					indices,
					morph_pos,
					morph_nrm,
				} = exp;
				// スキニング: ノードに skin が無いのに JOINTS があると、bone[0] 以外を参照して頂点が吹き飛ぶ（前腕欠落など）。
				if node.skin.is_none() && buf.joints.is_some() {
					for v in &mut verts {
						v.joints = [0, 0, 0, 0];
						v.weights = [1.0, 0.0, 0.0, 0.0];
					}
				} else if let Some(si) = node.skin {
					if let Some(skin) = scene.skins.get(si) {
						let jc = skin.joint_nodes.len();
						if jc > 0 {
							let cap = (jc - 1).min(u16::MAX as usize) as u16;
							for v in &mut verts {
								for k in 0..4 {
									if v.joints[k] as usize >= jc {
										v.joints[k] = cap;
									}
								}
							}
						}
					}
				}
				let skin_palette_key = SkinPaletteKey {
					world_node_index: ni,
					skin_index: node.skin,
				};
				let skin_palette_index = if let Some(&index) = skin_palette_indices.get(&skin_palette_key) {
					index
				} else {
					let matrix_capacity = node
						.skin
						.and_then(|skin_index| scene.skins.get(skin_index))
						.map(|skin| skin.joint_nodes.len().min(MAX_BONES))
						.unwrap_or(1)
						.max(1);
					let bone_buffer = device.create_buffer(&wgpu::BufferDescriptor {
						label: Some("mesh_bones"),
						size: matrix_capacity as u64 * BONE_MATRIX_SIZE,
						usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
						mapped_at_creation: false,
					});
					let bone_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
						label: Some("mesh_bone_bg"),
						layout: &skin_bind_group_layout,
						entries: &[wgpu::BindGroupEntry {
							binding: 0,
							resource: bone_buffer.as_entire_binding(),
						}],
					});
					let index = skin_palettes.len();
					let raw_capacity = matrix_raw_capacity(matrix_capacity);
					let static_identity = skin_palette_key.skin_index.is_none();
					let (raw, uploaded) = if static_identity {
						let raw = identity_matrix_raw();
						queue.write_buffer(&bone_buffer, 0, bytemuck::cast_slice(&raw));
						(raw.clone(), raw)
					} else {
						(Vec::with_capacity(raw_capacity), Vec::with_capacity(raw_capacity))
					};
					skin_palettes.push(SkinPalette {
						key: skin_palette_key,
						buffer: bone_buffer,
						bind_group: bone_bind_group,
						matrix_capacity,
						static_identity,
						raw,
						uploaded,
					});
					skin_palette_indices.insert(skin_palette_key, index);
					index
				};
				let vbuf = device.create_buffer(&wgpu::BufferDescriptor {
					label: Some("mesh_v"),
					size: (verts.len() * std::mem::size_of::<Vertex>()) as u64,
					usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
					mapped_at_creation: false,
				});
				queue.write_buffer(&vbuf, 0, bytemuck::cast_slice(&verts));

				let index_format = compact_index_format(&indices);
				let ibuf = match index_format {
					wgpu::IndexFormat::Uint16 => {
						let indices16: Vec<u16> = indices.iter().map(|&index| index as u16).collect();
						device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
							label: Some("mesh_i_u16"),
							contents: bytemuck::cast_slice(&indices16),
							usage: wgpu::BufferUsages::INDEX,
						})
					}
					wgpu::IndexFormat::Uint32 => device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
						label: Some("mesh_i_u32"),
						contents: bytemuck::cast_slice(&indices),
						usage: wgpu::BufferUsages::INDEX,
					}),
				};

				let mi = buf.material_index.unwrap_or(0);
				let mat = scene.materials.get(mi).cloned().unwrap_or_default();
				let mtoon = mat.mtoon.clone().unwrap_or_default();
				let tex_view = texture_view_or(&image_views, mat.base_color_texture_index, &white_view);
				let shade_view = texture_view_or(&image_views, mtoon.shade_multiply_texture_index, &white_view);
				let shift_view = texture_view_or(&image_views, mtoon.shading_shift_texture_index, &black_view);
				let matcap_view = texture_view_or(&image_views, mtoon.matcap_texture_index, &black_view);
				let rim_view = texture_view_or(&image_views, mtoon.rim_multiply_texture_index, &white_view);
				let reflection_view = texture_view_or(&image_views, mtoon.reflection_cube_texture_index, &black_view);
				let emissive_view = texture_view_or(&image_views, mat.emissive_texture_index, &black_view);
				let occlusion_view = texture_view_or(&image_views, mat.occlusion_texture_index, &white_view);
				let outline_view = texture_view_or(&image_views, mtoon.outline_width_multiply_texture_index, &white_view);
				let uv_mask_view = texture_view_or(&image_views, mtoon.uv_animation_mask_texture_index, &white_view);
				let normal_view = texture_view_or(&image_views, mat.normal_texture_index, &neutral_normal_view);

				let draw_transform = device.create_buffer(&wgpu::BufferDescriptor {
					label: Some("mesh_draw_transform"),
					size: std::mem::size_of::<MeshDrawTransformGpu>() as u64,
					usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
					mapped_at_creation: false,
				});
				let draw_material = mesh_draw_material_gpu(&mat, &mtoon, &opts, mesh_i, prim_i);
				let draw_material_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
					label: Some("mesh_draw_material"),
					contents: bytemuck::bytes_of(&draw_material),
					usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
				});

				let bind_material = device.create_bind_group(&wgpu::BindGroupDescriptor {
					label: Some("mesh_mat"),
					layout: &material_layout,
					entries: &[
						wgpu::BindGroupEntry {
							binding: 0,
							resource: draw_transform.as_entire_binding(),
						},
						wgpu::BindGroupEntry {
							binding: 10,
							resource: draw_material_buffer.as_entire_binding(),
						},
						wgpu::BindGroupEntry {
							binding: 1,
							resource: wgpu::BindingResource::TextureView(tex_view),
						},
						wgpu::BindGroupEntry {
							binding: 2,
							resource: wgpu::BindingResource::Sampler(&sampler),
						},
						wgpu::BindGroupEntry {
							binding: 3,
							resource: wgpu::BindingResource::TextureView(shade_view),
						},
						wgpu::BindGroupEntry {
							binding: 4,
							resource: wgpu::BindingResource::TextureView(shift_view),
						},
						wgpu::BindGroupEntry {
							binding: 5,
							resource: wgpu::BindingResource::TextureView(matcap_view),
						},
						wgpu::BindGroupEntry {
							binding: 6,
							resource: wgpu::BindingResource::TextureView(rim_view),
						},
						wgpu::BindGroupEntry {
							binding: 7,
							resource: wgpu::BindingResource::TextureView(emissive_view),
						},
						wgpu::BindGroupEntry {
							binding: 8,
							resource: wgpu::BindingResource::TextureView(outline_view),
						},
						wgpu::BindGroupEntry {
							binding: 9,
							resource: wgpu::BindingResource::TextureView(uv_mask_view),
						},
						wgpu::BindGroupEntry {
							binding: 11,
							resource: wgpu::BindingResource::TextureView(normal_view),
						},
						wgpu::BindGroupEntry {
							binding: 12,
							resource: wgpu::BindingResource::TextureView(occlusion_view),
						},
						wgpu::BindGroupEntry {
							binding: 13,
							resource: wgpu::BindingResource::TextureView(reflection_view),
						},
					],
				});

				let morph_meta = MorphMetaGpu {
					target_count: morph_pos.len() as u32,
					vertex_count: verts.len() as u32,
					_pad: [0; 2],
				};
				let morph_meta_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
					label: Some("mesh_morph_meta"),
					contents: bytemuck::bytes_of(&morph_meta),
					usage: wgpu::BufferUsages::UNIFORM,
				});
				let morph_weight_size = ((morph_pos.len() * std::mem::size_of::<f32>()) as u64).max(MORPH_WEIGHT_BUFFER_MIN_SIZE);
				let morph_weight_buffer = device.create_buffer(&wgpu::BufferDescriptor {
					label: Some("mesh_morph_weights"),
					size: morph_weight_size,
					usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
					mapped_at_creation: false,
				});
				let morph_deltas = morph_delta_data(&morph_pos, morph_nrm.as_deref(), verts.len());
				let morph_delta_size = ((morph_deltas.len() * std::mem::size_of::<[f32; 4]>()) as u64).max(MORPH_DELTA_BUFFER_MIN_SIZE);
				let morph_delta_buffer = device.create_buffer(&wgpu::BufferDescriptor {
					label: Some("mesh_morph_deltas"),
					size: morph_delta_size,
					usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
					mapped_at_creation: false,
				});
				queue.write_buffer(&morph_delta_buffer, 0, bytemuck::cast_slice(&morph_deltas));
				let morph_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
					label: Some("mesh_morph_bg"),
					layout: &morph_bind_group_layout,
					entries: &[
						wgpu::BindGroupEntry {
							binding: 0,
							resource: morph_meta_buffer.as_entire_binding(),
						},
						wgpu::BindGroupEntry {
							binding: 1,
							resource: morph_weight_buffer.as_entire_binding(),
						},
						wgpu::BindGroupEntry {
							binding: 2,
							resource: morph_delta_buffer.as_entire_binding(),
						},
					],
				});

				draws.push(MeshDraw {
					vertex_buffer: vbuf,
					index_buffer: ibuf,
					index_format,
					index_count: indices.len() as u32,
					draw_transform,
					draw_transform_uploaded: None,
					draw_material: draw_material_buffer,
					bind_material,
					skin_palette_index,
					_morph_meta_buffer: morph_meta_buffer,
					morph_weight_buffer,
					_morph_delta_buffer: morph_delta_buffer,
					morph_bind_group,
					world_node_index: ni,
					shading: mat.shading,
					morph_pos,
					expression_bindings: expression_bindings.get(&(mesh_i, prim_i)).cloned().unwrap_or_default(),
					default_morph_weights: buf.default_morph_weights.clone(),
					morph_weights: Vec::with_capacity(morph_meta.target_count as usize),
					morph_weight_scratch: Vec::with_capacity(morph_meta.target_count as usize),
					alpha_mode: mat.alpha_mode,
					material: mat,
					mtoon,
					mesh_index: mesh_i,
					primitive_index: prim_i,
				});
			}
		}

		let (outline_draw_indices, opaque_batches, transparent_zwrite_draw_indices, blended_batches) = build_draw_order(&draws, &opts);

		Ok(Self {
			pipeline_outline_mtoon,
			pipeline_opaque_lit,
			pipeline_opaque_unlit,
			pipeline_opaque_mtoon,
			pipeline_blend_lit,
			pipeline_blend_unlit,
			pipeline_blend_mtoon,
			pipeline_transparent_zprepass_mtoon,
			frame_buffer,
			frame_uploaded: None,
			frame_bind_group,
			sampler,
			_textures: textures,
			draws,
			skin_palettes,
			outline_draw_indices,
			opaque_batches,
			transparent_zwrite_draw_indices,
			blended_batches,
			texture_summary,
			expression_names,
			expression_value_scratch: Vec::with_capacity(catalog.map_or(0, |catalog| catalog.presets.len())),
			opts,
		})
	}

	fn write_skin_palette(
		queue: &wgpu::Queue,
		palette: &mut SkinPalette,
		skin: Option<&un_avatar_core::UnaSkin>,
		world: &[Mat4],
		legacy_no_inv_mesh: bool,
	) {
		if palette.static_identity {
			return;
		}
		palette.raw.clear();
		if let Some(skin) = skin {
			let mesh_world = world.get(palette.key.world_node_index).copied().unwrap_or(Mat4::IDENTITY);
			let inv_mesh = safe_inverse_mesh_world(mesh_world);
			let joint_count = skin.joint_nodes.len().min(palette.matrix_capacity).min(MAX_BONES);
			palette.raw.reserve(joint_count * 16);
			for (j, &n) in skin.joint_nodes.iter().take(joint_count).enumerate() {
				let wj = world.get(n).copied().unwrap_or(Mat4::IDENTITY);
				let ibm = Mat4::from_cols_array(&skin.inverse_bind_matrices[j]);
				let matrix = if legacy_no_inv_mesh { wj * ibm } else { inv_mesh * wj * ibm };
				write_matrix_to_raw(&mut palette.raw, matrix);
			}
		} else {
			write_matrix_to_raw(&mut palette.raw, Mat4::IDENTITY);
		}
		if palette.raw.is_empty() {
			write_matrix_to_raw(&mut palette.raw, Mat4::IDENTITY);
		}
		if palette.uploaded != palette.raw {
			queue.write_buffer(&palette.buffer, 0, bytemuck::cast_slice(&palette.raw));
			palette.uploaded.clear();
			palette.uploaded.extend_from_slice(&palette.raw);
		}
	}

	pub fn prepare_frame(
		&mut self,
		queue: &wgpu::Queue,
		view_proj: Mat4,
		light_dir: Vec4,
		camera_pos: Vec4,
		light_color: Vec4,
		ambient_color: Vec4,
	) {
		let f = MeshFrameGpu {
			view_proj: view_proj.to_cols_array_2d(),
			light_dir: light_dir.to_array(),
			camera_pos: camera_pos.to_array(),
			light_color: light_color.to_array(),
			ambient_color: ambient_color.to_array(),
			_pad: [[0.0; 4]; 8],
		};
		if self.frame_uploaded != Some(f) {
			queue.write_buffer(&self.frame_buffer, 0, bytemuck::bytes_of(&f));
			self.frame_uploaded = Some(f);
		}
	}

	fn draw_inner(&self, pass: &mut wgpu::RenderPass<'_>, state: &mut DrawBindState, draw_index: usize) {
		let d = &self.draws[draw_index];
		let palette = &self.skin_palettes[d.skin_palette_index];
		if !state.frame_bound {
			pass.set_bind_group(0, &self.frame_bind_group, &[]);
			state.frame_bound = true;
		}
		pass.set_bind_group(1, &d.bind_material, &[]);
		if state.skin_palette_index != Some(d.skin_palette_index) {
			pass.set_bind_group(2, &palette.bind_group, &[]);
			state.skin_palette_index = Some(d.skin_palette_index);
		}
		pass.set_bind_group(3, &d.morph_bind_group, &[]);
		pass.set_vertex_buffer(0, d.vertex_buffer.slice(..));
		pass.set_index_buffer(d.index_buffer.slice(..), d.index_format);
		pass.draw_indexed(0..d.index_count, 0, 0..1);
	}

	pub fn draw_mtoon_outlines(&self, pass: &mut wgpu::RenderPass<'_>) {
		if self.outline_draw_indices.is_empty()
			|| self.opts.force_simple_basecolor
			|| self.opts.debug_bind_pose
			|| self.opts.debug_primitive_colors
		{
			return;
		}
		pass.set_pipeline(&self.pipeline_outline_mtoon);
		let mut state = DrawBindState::default();
		for &draw_index in &self.outline_draw_indices {
			self.draw_inner(pass, &mut state, draw_index);
		}
	}

	pub fn set_avatar_outline(&mut self, queue: &wgpu::Queue, outline: AvatarOutlineOptions) {
		if self.opts.avatar_outline == outline {
			return;
		}
		self.opts.avatar_outline = outline;
		let (outline_draw_indices, opaque_batches, transparent_zwrite_draw_indices, blended_batches) = build_draw_order(&self.draws, &self.opts);
		self.outline_draw_indices = outline_draw_indices;
		self.opaque_batches = opaque_batches;
		self.transparent_zwrite_draw_indices = transparent_zwrite_draw_indices;
		self.blended_batches = blended_batches;
		self.rewrite_avatar_materials(queue);
	}

	pub fn set_avatar_rim(&mut self, queue: &wgpu::Queue, rim: AvatarRimOptions) {
		if self.opts.avatar_rim == rim {
			return;
		}
		self.opts.avatar_rim = rim;
		self.rewrite_avatar_materials(queue);
	}

	pub fn set_avatar_matcap(&mut self, queue: &wgpu::Queue, matcap: AvatarMatcapOptions) {
		if self.opts.avatar_matcap == matcap {
			return;
		}
		self.opts.avatar_matcap = matcap;
		self.rewrite_avatar_materials(queue);
	}

	pub fn set_avatar_specular(&mut self, queue: &wgpu::Queue, specular: AvatarSpecularOptions) {
		if self.opts.avatar_specular == specular {
			return;
		}
		self.opts.avatar_specular = specular;
		self.rewrite_avatar_materials(queue);
	}

	pub fn set_avatar_ambient_occlusion(&mut self, queue: &wgpu::Queue, ambient_occlusion: AvatarAmbientOcclusionOptions) {
		if self.opts.avatar_ambient_occlusion == ambient_occlusion {
			return;
		}
		self.opts.avatar_ambient_occlusion = ambient_occlusion;
		self.rewrite_avatar_materials(queue);
	}

	fn rewrite_avatar_materials(&self, queue: &wgpu::Queue) {
		for draw in &self.draws {
			let material = mesh_draw_material_gpu(&draw.material, &draw.mtoon, &self.opts, draw.mesh_index, draw.primitive_index);
			queue.write_buffer(&draw.draw_material, 0, bytemuck::bytes_of(&material));
		}
	}

	#[inline]
	fn pipeline_for_kind(&self, kind: DrawPipelineKind) -> &wgpu::RenderPipeline {
		match kind {
			DrawPipelineKind::OpaqueLit => &self.pipeline_opaque_lit,
			DrawPipelineKind::OpaqueUnlit => &self.pipeline_opaque_unlit,
			DrawPipelineKind::OpaqueMtoon => &self.pipeline_opaque_mtoon,
			DrawPipelineKind::BlendLit => &self.pipeline_blend_lit,
			DrawPipelineKind::BlendUnlit => &self.pipeline_blend_unlit,
			DrawPipelineKind::BlendMtoon => &self.pipeline_blend_mtoon,
		}
	}

	pub fn draw_opaque(&self, pass: &mut wgpu::RenderPass<'_>) {
		if self.opaque_batches.is_empty() {
			return;
		}
		let mut state = DrawBindState::default();
		for batch in &self.opaque_batches {
			pass.set_pipeline(self.pipeline_for_kind(batch.pipeline));
			for &draw_index in &batch.draw_indices {
				self.draw_inner(pass, &mut state, draw_index);
			}
		}
	}

	/// `alphaMode: BLEND`（および VRM0 MToon Transparent）。transparent z-write は
	/// color write なしの alpha-tested depth prepass 後に、通常の SrcAlpha 合成で描く。
	pub fn draw_blended(&self, pass: &mut wgpu::RenderPass<'_>) {
		if self.blended_batches.is_empty() {
			return;
		}
		let mut state = DrawBindState::default();
		if !self.transparent_zwrite_draw_indices.is_empty() {
			pass.set_pipeline(&self.pipeline_transparent_zprepass_mtoon);
			for &draw_index in &self.transparent_zwrite_draw_indices {
				self.draw_inner(pass, &mut state, draw_index);
			}
		}
		for batch in &self.blended_batches {
			pass.set_pipeline(self.pipeline_for_kind(batch.pipeline));
			for &draw_index in &batch.draw_indices {
				self.draw_inner(pass, &mut state, draw_index);
			}
		}
	}

	pub fn update_draw_transforms(
		&mut self,
		queue: &wgpu::Queue,
		scene: &UnaSceneSnapshot,
		world: &[Mat4],
		expr_weights: Option<&UnaExpressionWeights>,
		expression_overrides: Option<&BTreeMap<String, f32>>,
	) {
		let opts = &self.opts;
		self.expression_value_scratch.clear();
		if expr_weights.is_some() || expression_overrides.is_some() {
			self.expression_value_scratch.reserve(self.expression_names.len());
			for name in &self.expression_names {
				let value = expression_overrides
					.and_then(|overrides| overrides.get(name).copied())
					.or_else(|| expr_weights.and_then(|weights| weights.preset_weights.get(name).copied()))
					.unwrap_or(0.0);
				self.expression_value_scratch.push(value);
			}
		}
		let expression_values = (!self.expression_value_scratch.is_empty()).then_some(self.expression_value_scratch.as_slice());

		for palette in &mut self.skin_palettes {
			let skin = palette.key.skin_index.and_then(|si| scene.skins.get(si));
			Self::write_skin_palette(queue, palette, skin, world, opts.debug_skin_legacy_no_inv_mesh);
		}

		for d in &mut self.draws {
			let mesh_world = world.get(d.world_node_index).copied().unwrap_or(Mat4::IDENTITY);

			if !d.morph_pos.is_empty() {
				let draw_has_active_expression = expression_bindings_have_active_weight(&d.expression_bindings, expression_values);
				let skip_static_default_morph = !draw_has_active_expression
					&& !opts.debug_zero_morphs
					&& morph_weights_match_default(&d.morph_weights, &d.default_morph_weights, d.morph_pos.len());
				if !skip_static_default_morph {
					d.morph_weight_scratch.clear();
					if opts.debug_zero_morphs {
						d.morph_weight_scratch.resize(d.morph_pos.len(), 0.0);
					} else {
						fill_morph_weights_for_draw(
							&d.default_morph_weights,
							d.morph_pos.len(),
							&d.expression_bindings,
							expression_values,
							&mut d.morph_weight_scratch,
						);
					}

					if d.morph_weight_scratch.len() == d.morph_pos.len() {
						if d.morph_weights != d.morph_weight_scratch {
							queue.write_buffer(&d.morph_weight_buffer, 0, bytemuck::cast_slice(&d.morph_weight_scratch));
							d.morph_weights.clear();
							d.morph_weights.extend_from_slice(&d.morph_weight_scratch);
						}
					} else if !d.morph_weights.is_empty() {
						d.morph_weight_scratch.clear();
						d.morph_weight_scratch.resize(d.morph_pos.len(), 0.0);
						queue.write_buffer(&d.morph_weight_buffer, 0, bytemuck::cast_slice(&d.morph_weight_scratch));
						d.morph_weights.clear();
					}
				}
			}

			let transform = MeshDrawTransformGpu {
				model: mesh_world.to_cols_array_2d(),
			};
			if d.draw_transform_uploaded != Some(transform) {
				queue.write_buffer(&d.draw_transform, 0, bytemuck::bytes_of(&transform));
				d.draw_transform_uploaded = Some(transform);
			}
		}
	}

	pub fn is_empty(&self) -> bool {
		self.draws.is_empty()
	}

	pub(crate) fn texture_summary(&self) -> TextureUploadSummary {
		self.texture_summary.clone()
	}
}

pub(crate) fn skin_tone_matching_debug_for_scene(scene: &UnaSceneSnapshot) -> SkinToneMatchingDebug {
	let world = scene_world_matrices(scene);
	skin_tone_matching_debug_for_scene_with_world(scene, &world)
}

#[cfg(test)]
mod tests {
	use super::*;
	use un_avatar_core::UnaSceneNode;

	#[test]
	fn skin_tone_matching_disables_mtoon_shade_color_on_skin_materials() {
		let mat = UnaMaterialPbr {
			name: Some("N00_000_00_Body_00_SKIN".to_string()),
			..Default::default()
		};
		let opts = SceneMeshLoadOpts {
			skin_tone_matching: true,
			..Default::default()
		};
		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &opts, 0, 0);
		assert_eq!(draw.params[3].to_bits() & 64, 64);
	}

	#[test]
	fn morph_weights_match_short_default_with_zero_tail() {
		assert!(morph_weights_match_default(&[0.25, 0.0, 0.0], &[0.25], 3));
		assert!(!morph_weights_match_default(&[0.25, 0.1, 0.0], &[0.25], 3));
		assert!(!morph_weights_match_default(&[0.25, 0.0], &[0.25], 3));
	}

	#[test]
	fn expression_binding_activity_tracks_only_bound_presets() {
		let bindings = [ExpressionBinding {
			preset_index: 1,
			morph_target_index: 0,
			weight_scale: 1.0,
		}];

		assert!(!expression_bindings_have_active_weight(&bindings, None));
		assert!(!expression_bindings_have_active_weight(&bindings, Some(&[0.8, 0.0])));
		assert!(expression_bindings_have_active_weight(&bindings, Some(&[0.0, 0.8])));
	}

	#[test]
	fn effective_visibility_inherits_hidden_parent_state() {
		let identity = Mat4::IDENTITY.to_cols_array();
		let scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("root".to_string()),
					visible: false,
					transform: identity,
					children: vec![1],
					mesh: None,
					skin: None,
				},
				UnaSceneNode {
					name: Some("child".to_string()),
					visible: true,
					transform: identity,
					children: Vec::new(),
					mesh: Some(0),
					skin: None,
				},
			],
			roots: vec![0],
			..Default::default()
		};

		assert_eq!(scene_effective_visibility(&scene), vec![false, false]);
	}

	#[test]
	fn relax_iris_alpha_preserves_visible_masked_eye_materials() {
		let mat = UnaMaterialPbr {
			name: Some("眼睛".to_string()),
			alpha_mode: UnaAlphaMode::Mask,
			base_color_factor: [1.0, 1.0, 1.0, 1.0],
			..Default::default()
		};
		let opts = SceneMeshLoadOpts {
			relax_iris_alpha: true,
			..Default::default()
		};
		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &opts, 0, 0);

		assert_eq!(draw.params[1], UnaAlphaMode::Mask.as_shader_alpha_kind());
		assert_eq!(draw.base_color[3], 1.0);
	}

	#[test]
	fn relax_iris_alpha_still_rescues_zero_alpha_eye_materials() {
		let mat = UnaMaterialPbr {
			name: Some("眼睛".to_string()),
			alpha_mode: UnaAlphaMode::Mask,
			base_color_factor: [1.0, 1.0, 1.0, 0.0],
			..Default::default()
		};
		let opts = SceneMeshLoadOpts {
			relax_iris_alpha: true,
			..Default::default()
		};
		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &opts, 0, 0);

		assert_eq!(draw.params[1], UnaAlphaMode::Opaque.as_shader_alpha_kind());
		assert_eq!(draw.base_color[3], 1.0);
	}
}
