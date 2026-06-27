//! glTF / [`UnaSceneSnapshot`] 由来のメッシュ描画（スキニング・モーフ・シェーディング種別）。

use std::{
	borrow::Cow,
	collections::BTreeMap,
	fs,
	io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
	path::{Path, PathBuf},
	time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use glam::{Mat4, Vec2, Vec3, Vec4};
use half::f16;
use serde::Serialize;
use un_avatar_core::{
	UnaAlphaMode, UnaCullMode, UnaExpressionCatalog, UnaExpressionWeights, UnaImageRgba, UnaImageSourceMetadata, UnaMaterialPbr,
	UnaMeshBuffers, UnaMtoonMaterial, UnaMtoonOutlineWidthMode, UnaSceneSnapshot, UnaShadingModel, UnaTextureFilterMode, UnaTextureSampler,
	UnaTextureWrapMode,
};
use un_avatar_skeleton::{
	apply_dynamics_mesh_cloth_assist_to_vertices, dynamics_mesh_cloth_assist_joint_roles,
	dynamics_mesh_cloth_assist_mesh_matches_with_categories as mesh_cloth_assist_mesh_matches_with_categories,
	dynamics_token_filter_matches, DynamicsCategoryDefinition, DynamicsMeshClothAssistConfig, DynamicsMeshClothAssistJointRole,
	DynamicsMeshClothAssistVertex,
};

use crate::avatar_material::{effective_mtoon_outline, effective_mtoon_rim, texture_roles_for_scene};
use crate::debug_dump::{debug_primitive_color_rgba, iris_like_material_name};
use crate::liltoon_features;
use crate::scene_transform::{safe_inverse_mesh_world, scene_world_matrices};
use crate::skin_tone::{
	build_skin_tone_matched_images, material_skin_tone_kind, skin_tone_matching_debug_for_scene_with_world,
	skin_tone_texture_kinds_for_scene, SkinToneMatchingDebug,
};
use crate::texture_pipeline::{
	compressed_cache_lookup_from_source, compressed_cache_lookup_from_source_metadata, compressed_upload_kind_for_texture,
	compression_preference_for_role, create_vulkan_gpu_texture_compression_context, estimated_processed_mip_count,
	load_or_build_processed_texture, load_or_build_processed_texture_with_rgba, mip_level_count, read_compressed_texture_cache,
	source_texture_upload, texture_cache_key, texture_cache_key_from_source_metadata, texture_upload_payload, GpuTextureCompressionContext,
	SourceTextureUpload, TextureCacheEvent, TextureRole, TextureUploadKind, TextureUploadPayload,
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
	/// lilToon reflection / specular / gem reflection 寄与を 0 にする診断 toggle。
	pub debug_disable_reflection: bool,
	/// toon fragment path の出力を `base = alb × base_color.rgb` のみで早期 return する診断 toggle。
	/// shading / GI / rim / matcap / emissive / shade_term を全てスキップ。
	/// これでリングが残るならテクスチャ自身またはメッシュ重なり由来。
	pub debug_base_texture_only: bool,
	/// lilToon Fur 描画を完全にスキップする診断 toggle。
	/// Compute Fur 実装の副作用が通常描画へ波及しているかを切り分ける。
	pub disable_fur: bool,
	/// アバター用途の outline override。既定は VRM / MToon authored outline を尊重する。
	pub avatar_outline: AvatarOutlineOptions,
	/// 顔と体で別テクスチャの肌色差が首境界に出るモデル向けの実験的なロード時補正。
	pub skin_tone_matching: bool,
	/// Body bone dominated cloth vertices can borrow more influence from already-authored cloth dynamic joints.
	pub mesh_cloth_assist: DynamicsMeshClothAssistConfig,
	/// Normalized dynamics category definitions used when mesh cloth assist has no explicit mesh filter.
	pub mesh_cloth_assist_categories: Vec<DynamicsCategoryDefinition>,
	/// Scene node indices that are actual dynamics deformation targets.
	/// When this is non-empty, mesh cloth assist uses it instead of name-only cloth joint classification.
	pub dynamic_deforming_node_indices: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MeshShaderVariantTier {
	HighCapability,
	BaselineFallback,
}

impl MeshShaderVariantTier {
	pub(crate) fn is_high_capability(self) -> bool {
		matches!(self, Self::HighCapability)
	}

	pub(crate) fn is_baseline_fallback(self) -> bool {
		matches!(self, Self::BaselineFallback)
	}

	fn material_layout_label(self) -> &'static str {
		if self.is_high_capability() {
			"mesh_material"
		} else {
			"mesh_material_baseline_fallback"
		}
	}
}

#[derive(Clone, Copy)]
enum MeshPipelineAlphaCoverage {
	Off,
	On,
}

impl MeshPipelineAlphaCoverage {
	fn enabled(self) -> bool {
		matches!(self, Self::On)
	}
}

#[derive(Clone, Copy)]
struct MeshPipelineRenderState {
	color_blend: Option<wgpu::BlendState>,
	color_write_mask: wgpu::ColorWrites,
	depth_write: bool,
	depth_compare: wgpu::CompareFunction,
	stencil: MaterialStencilState,
	cull_mode: Option<wgpu::Face>,
	alpha_coverage: MeshPipelineAlphaCoverage,
	sample_count: u32,
}

impl MeshPipelineRenderState {
	fn mesh_main(color_blend: Option<wgpu::BlendState>, depth_write: bool, sample_count: u32) -> Self {
		Self {
			color_blend,
			color_write_mask: wgpu::ColorWrites::ALL,
			depth_write,
			depth_compare: wgpu::CompareFunction::LessEqual,
			stencil: MaterialStencilState::default(),
			cull_mode: None,
			alpha_coverage: MeshPipelineAlphaCoverage::Off,
			sample_count,
		}
	}

	fn outline(sample_count: u32) -> Self {
		Self {
			color_blend: None,
			color_write_mask: wgpu::ColorWrites::ALL,
			depth_write: false,
			depth_compare: wgpu::CompareFunction::Less,
			stencil: MaterialStencilState::default(),
			cull_mode: Some(wgpu::Face::Front),
			alpha_coverage: MeshPipelineAlphaCoverage::Off,
			sample_count,
		}
	}

	fn with_alpha_coverage(mut self, alpha_coverage: MeshPipelineAlphaCoverage) -> Self {
		self.alpha_coverage = alpha_coverage;
		self
	}

	fn with_material_render_state(mut self, key: DrawPipelineKey) -> Self {
		self.stencil = key.stencil;
		self.color_write_mask = color_writes_from_unity_mask(key.color_mask);
		self
	}

	fn with_material_render_state_key(mut self, key: MaterialRenderStateKey) -> Self {
		self.stencil = key.stencil;
		self.color_write_mask = color_writes_from_unity_mask(key.color_mask);
		self
	}
}

use wgpu::util::DeviceExt;

const SHADER_MESH: &str = include_str!("../shaders/mesh.wgsl");
const SHADER_COMPUTE_FUR_CARDS: &str = include_str!("../shaders/compute_fur_cards.wgsl");
const MAT_UNTOON_GEM_PROFILE: u32 = 8192;
const MAT_UNTOON_REFRACTION_PROFILE: u32 = 16384;
const MAT_UNTOON_ADDITIVE_BLEND: u32 = 32768;

fn mesh_shader_source_for_tier(variant_tier: MeshShaderVariantTier) -> Cow<'static, str> {
	match variant_tier {
		MeshShaderVariantTier::HighCapability => Cow::Borrowed(SHADER_MESH),
		MeshShaderVariantTier::BaselineFallback => Cow::Owned(baseline_fallback_mesh_shader_source()),
	}
}

fn mesh_shader_source_for_features(variant_tier: MeshShaderVariantTier, features: UntoonShaderFeatures) -> Cow<'static, str> {
	let mut shader = mesh_shader_source_for_tier(variant_tier).into_owned();
	for (name, enabled) in features.shader_feature_values() {
		let value = if enabled { "1.0" } else { "0.0" };
		shader = shader.replace(&format!("override {name}: f32 = 1.0;"), &format!("const {name}: f32 = {value};"));
	}
	Cow::Owned(shader)
}

fn create_mesh_shader_module_for_features(
	device: &wgpu::Device,
	variant_tier: MeshShaderVariantTier,
	features: UntoonShaderFeatures,
	label: &'static str,
) -> wgpu::ShaderModule {
	device.create_shader_module(wgpu::ShaderModuleDescriptor {
		label: Some(label),
		source: wgpu::ShaderSource::Wgsl(mesh_shader_source_for_features(variant_tier, features)),
	})
}

fn baseline_fallback_mesh_shader_source() -> String {
	let mut shader = SHADER_MESH.to_string();
	for snippet in [
		"@group(1) @binding(24) var shadow_border_mask_tex: texture_2d<f32>;\n",
		"@group(1) @binding(25) var shadow_blur_mask_tex: texture_2d<f32>;\n",
		"@group(1) @binding(26) var shadow_border_mask_samp: sampler;\n",
		"@group(1) @binding(27) var shadow_blur_mask_samp: sampler;\n",
		"@group(1) @binding(38) var matcap2_tex: texture_2d<f32>;\n",
		"@group(1) @binding(39) var matcap2_blend_mask_tex: texture_2d<f32>;\n",
		"@group(1) @binding(41) var main2nd_tex: texture_2d<f32>;\n",
		"@group(1) @binding(42) var main3rd_tex: texture_2d<f32>;\n",
		"@group(1) @binding(43) var main2nd_blend_mask_tex: texture_2d<f32>;\n",
		"@group(1) @binding(44) var main3rd_blend_mask_tex: texture_2d<f32>;\n",
		"@group(1) @binding(45) var normal2nd_tex: texture_2d<f32>;\n",
		"@group(1) @binding(46) var emission_gradation_tex: texture_2d<f32>;\n",
		"@group(1) @binding(47) var main_gradation_tex: texture_2d<f32>;\n",
		"@group(1) @binding(48) var emission2nd_tex: texture_2d<f32>;\n",
		"@group(1) @binding(49) var emission2nd_blend_mask_tex: texture_2d<f32>;\n",
		"@group(1) @binding(50) var emission2nd_gradation_tex: texture_2d<f32>;\n",
		"@group(1) @binding(51) var anisotropy_tangent_tex: texture_2d<f32>;\n",
		"@group(1) @binding(52) var anisotropy_scale_mask_tex: texture_2d<f32>;\n",
		"@group(1) @binding(53) var anisotropy_shift_noise_tex: texture_2d<f32>;\n",
		"@group(1) @binding(54) var emission_blend_mask_tex: texture_2d<f32>;\n",
		"@group(1) @binding(55) var rim_shade_mask_tex: texture_2d<f32>;\n",
		"@group(1) @binding(56) var backlight_color_tex: texture_2d<f32>;\n",
		"@group(1) @binding(57) var shadow2_color_tex: texture_2d<f32>;\n",
		"@group(1) @binding(58) var shadow3_color_tex: texture_2d<f32>;\n",
		"@group(1) @binding(59) var fur_vector_tex: texture_2d<f32>;\n",
		"@group(1) @binding(60) var fur_length_mask_tex: texture_2d<f32>;\n",
		"@group(1) @binding(61) var fur_noise_mask_tex: texture_2d<f32>;\n",
		"@group(1) @binding(62) var fur_mask_tex: texture_2d<f32>;\n",
		"@group(1) @binding(63) var glitter_color_tex: texture_2d<f32>;\n",
		"@group(1) @binding(64) var glitter_shape_tex: texture_2d<f32>;\n",
		"@group(1) @binding(65) var dissolve_mask_tex: texture_2d<f32>;\n",
		"@group(1) @binding(66) var dissolve_noise_mask_tex: texture_2d<f32>;\n",
		"@group(1) @binding(67) var parallax_tex: texture_2d<f32>;\n",
		"@group(1) @binding(68) var main2nd_dissolve_mask_tex: texture_2d<f32>;\n",
		"@group(1) @binding(69) var main2nd_dissolve_noise_mask_tex: texture_2d<f32>;\n",
		"@group(1) @binding(70) var main3rd_dissolve_mask_tex: texture_2d<f32>;\n",
		"@group(1) @binding(71) var main3rd_dissolve_noise_mask_tex: texture_2d<f32>;\n",
		"@group(1) @binding(72) var normal2nd_scale_mask_tex: texture_2d<f32>;\n",
		"@group(1) @binding(73) var matcap_bump_tex: texture_2d<f32>;\n",
		"@group(1) @binding(74) var matcap2_bump_tex: texture_2d<f32>;\n",
		"@group(1) @binding(75) var main_color_adjust_mask_tex: texture_2d<f32>;\n",
	] {
		shader = shader.replace(snippet, "");
	}
	shader = shader.replace(
		"let main_color_adjust_mask = textureSample(main_color_adjust_mask_tex, base_samp, uv).r;",
		"let main_color_adjust_mask = 1.0;",
	);
	shader = shader.replace(
		"let glitter_color_texel = textureSample(glitter_color_tex, base_samp, glitter_color_uv);",
		"let glitter_color_texel = vec4<f32>(1.0, 1.0, 1.0, 1.0);",
	);
	shader = shader.replace(
		"textureSample(matcap_bump_tex, normal_samp, map_uv)",
		"vec4<f32>(0.5, 0.5, 1.0, 1.0)",
	);
	shader = shader.replace(
		"textureSample(matcap2_bump_tex, normal_samp, map_uv)",
		"vec4<f32>(0.5, 0.5, 1.0, 1.0)",
	);
	shader = shader.replace(
		"let shape_tex = textureSampleGrad(glitter_shape_tex, base_samp, mask_uv, abs(dpdx(pos)) * mipfactor.x, abs(dpdy(pos)) * mipfactor.y);",
		"let shape_tex = vec4<f32>(1.0, 1.0, 1.0, 1.0);",
	);
	shader = shader.replace(
		"let dissolve_mask_val = textureSample(dissolve_mask_tex, base_samp, dissolve_mask_uv).r;",
		"let dissolve_mask_val = 1.0;",
	);
	shader = shader.replace(
		"let dissolve_noise = (textureSample(dissolve_noise_mask_tex, base_samp, dissolve_noise_uv).r - 0.5) * drawu.dissolve_ext.x * drawu.dissolve_ext.z;",
		"let dissolve_noise = 0.0;",
	);
	for (from, to) in [
		(
			"let layer = textureSample(main2nd_tex, base_samp, layer_uv.sample_uv) * drawu.main2nd_color;",
			"let layer = drawu.main2nd_color;",
		),
		("textureSample(main2nd_blend_mask_tex, base_samp, uv).r", "1.0"),
		("textureSample(main2nd_dissolve_mask_tex, base_samp, dissolve_mask_uv).r", "1.0"),
		(
			"textureSample(main2nd_dissolve_noise_mask_tex, base_samp, dissolve_noise_uv).r",
			"0.5",
		),
		(
			"let layer = textureSample(main3rd_tex, base_samp, layer_uv.sample_uv) * drawu.main3rd_color;",
			"let layer = drawu.main3rd_color;",
		),
		("textureSample(main3rd_blend_mask_tex, base_samp, uv).r", "1.0"),
		("textureSample(main3rd_dissolve_mask_tex, base_samp, dissolve_mask_uv).r", "1.0"),
		(
			"textureSample(main3rd_dissolve_noise_mask_tex, base_samp, dissolve_noise_uv).r",
			"0.5",
		),
	] {
		shader = shader.replace(from, to);
	}
	shader = shader.replace("textureSampleLevel(parallax_tex, base_samp, parallax_map_uv, 0.0).r", "0.5");
	shader = shader.replace("textureSampleLevel(parallax_tex, base_samp, ray_pos.xy, 0.0).r", "0.5");
	shader = shader.replace(
		"let fur_mask = textureSample(fur_mask_tex, base_samp, uv).r;",
		"let fur_mask = 1.0;",
	);
	shader = shader.replace(
		"let fur_noise_mask = textureSample(fur_noise_mask_tex, base_samp, noise_uv).r;",
		"let fur_noise_mask = 1.0;",
	);
	shader = shader.replace(
		"let fur_noise_mask = textureSample(fur_noise_mask_tex, base_samp, noise_uv).r;",
		"let fur_noise_mask = 1.0;",
	);
	shader = shader.replace(
		"\tlet fur_mask = textureSampleLevel(fur_mask_tex, base_samp, fur_uv, 0.0).r;\n\tlet noise_uv = fur_uv * max(drawu.fur_noise_params.xy, vec2<f32>(0.0001, 0.0001)) + drawu.fur_noise_params.zw;\n\tlet noise = textureSampleLevel(fur_noise_mask_tex, base_samp, noise_uv, 0.0).r;\n",
		"\tlet fur_mask = 1.0;\n\tlet noise = 1.0;\n",
	);
	shader = shader.replace(
		"\tlet length_mask = textureSampleLevel(fur_length_mask_tex, base_samp, fur_uv, 0.0).r;\n\tlet vector_tex = textureSampleLevel(fur_vector_tex, base_samp, fur_uv, 0.0).xyz * 2.0 - vec3<f32>(1.0);\n\tlet fur_vector_ts = drawu.fur_vector_params.xyz + vector_tex + vec3<f32>(0.0, 0.0, 0.001);\n",
		"\tlet length_mask = 1.0;\n\tlet fur_vector_ts = drawu.fur_vector_params.xyz + vec3<f32>(0.0, 0.0, 0.001);\n",
	);
	shader = shader.replace(
		"let length_mask = textureSampleLevel(fur_length_mask_tex, base_samp, fur_uv, 0.0).r;",
		"let length_mask = 1.0;",
	);
	shader = shader.replace(
		"let vector_tex = textureSampleLevel(fur_vector_tex, base_samp, fur_uv, 0.0).xyz * 2.0 - vec3<f32>(1.0);",
		"let vector_tex = vec3<f32>(0.0);",
	);
	shader = shader.replace(
		"let vector_tex = unpack_fur_vector_map(textureSampleLevel(fur_vector_tex, base_samp, fur_uv, 0.0), drawu.fur_ext_params.x);",
		"let vector_tex = vec3<f32>(0.0, 0.0, 1.0);",
	);
	shader = shader.replace(
		"fn lil_anisotropy_basis(n: vec3<f32>, tangent_in: vec4<f32>, uv: vec2<f32>, v: vec3<f32>) -> AnisotropyBasis {\n\tlet enabled = clamp(drawu.anisotropy_params.x, 0.0, 1.0);\n\tif (enabled <= 0.000001) {\n\t\treturn AnisotropyBasis(n, n, 0.0, 0.0, 0.0);\n\t}\n\tlet base_tangent = normalize(tangent_in.xyz - n * dot(n, tangent_in.xyz));\n\tlet base_bitangent = normalize(cross(n, base_tangent)) * tangent_in.w;\n\tlet tangent_uv = uv * drawu.anisotropy_tangent_uv_offset_scale.zw + drawu.anisotropy_tangent_uv_offset_scale.xy;\n\tvar tangent_sample = textureSample(anisotropy_tangent_tex, normal_samp, tangent_uv).xyz * 2.0 - vec3<f32>(1.0, 1.0, 1.0);\n\tif (dot(tangent_sample, tangent_sample) < 0.000001) {\n\t\ttangent_sample = vec3<f32>(1.0, 0.0, 0.0);\n\t}\n\tvar aniso_t = normalize(base_tangent * tangent_sample.x + base_bitangent * tangent_sample.y + n * tangent_sample.z);\n\taniso_t = normalize(aniso_t - n * dot(n, aniso_t));\n\tlet aniso_b = normalize(cross(n, aniso_t)) * tangent_in.w;\n\tlet scale_uv = uv * drawu.anisotropy_scale_mask_uv_offset_scale.zw + drawu.anisotropy_scale_mask_uv_offset_scale.xy;\n\tlet scale_mask = textureSample(anisotropy_scale_mask_tex, base_samp, scale_uv).r;\n\tlet anisotropy = drawu.anisotropy_params.y * scale_mask;\n\tlet shift_axis = select(aniso_b, aniso_t, anisotropy >= 0.0);\n\tlet aniso_n = normalize(n + shift_axis * clamp(abs(anisotropy), 0.0, 1.0) * max(0.15, 1.0 - abs(dot(n, v))));\n\tlet noise_uv = uv * drawu.anisotropy_shift_noise_uv_offset_scale.zw + drawu.anisotropy_shift_noise_uv_offset_scale.xy;\n\tlet shift_noise = textureSample(anisotropy_shift_noise_tex, base_samp, noise_uv).r - 0.5;\n\treturn AnisotropyBasis(aniso_n, aniso_t, clamp(anisotropy, -1.0, 1.0), shift_noise, enabled);\n}\n",
		"fn lil_anisotropy_basis(n: vec3<f32>, tangent_in: vec4<f32>, uv: vec2<f32>, v: vec3<f32>) -> AnisotropyBasis {\n\treturn AnisotropyBasis(n, n, n, 0.0, 0.0, 0.0);\n}\n",
	);
	shader = shader.replace(
		"var tangent_sample = textureSample(anisotropy_tangent_tex, normal_samp, tangent_uv).xyz * 2.0 - vec3<f32>(1.0, 1.0, 1.0);",
		"var tangent_sample = vec3<f32>(1.0, 0.0, 0.0);",
	);
	shader = shader.replace(
		"let scale_mask = textureSample(anisotropy_scale_mask_tex, base_samp, scale_uv).r;",
		"let scale_mask = 1.0;",
	);
	shader = shader.replace(
		"let shift_noise = textureSample(anisotropy_shift_noise_tex, base_samp, noise_uv).r - 0.5;",
		"let shift_noise = 0.0;",
	);
	shader = shader.replace(
		"\t\t\tlet emission_mask_uv = lil_calc_uv_scroll_rotate(uv, drawu.emission_blend_mask_uv_offset_scale, drawu.emission_blend_mask_uv_anim_params);\n\t\t\tlet emission_mask = textureSample(emission_blend_mask_tex, emissive_samp, emission_mask_uv).r;\n",
		"\t\t\tlet emission_mask = 1.0;\n",
	);
	shader = shader.replace(
		"\t\t\tlet rim_shade_mask = textureSample(rim_shade_mask_tex, rim_samp, uv).r;\n",
		"\t\t\tlet rim_shade_mask = 1.0;\n",
	);
	shader = shader.replace(
		"\tlet rim_shade_mask = textureSample(rim_shade_mask_tex, rim_samp, uv).r;\n",
		"\tlet rim_shade_mask = 1.0;\n",
	);
	shader = shader.replace(
		"\t\t\tlet backlight_color_uv = uv * drawu.backlight_color_uv_offset_scale.zw + drawu.backlight_color_uv_offset_scale.xy;\n\t\t\tlet backlight_color_sample = textureSample(backlight_color_tex, base_samp, backlight_color_uv);\n\t\t\tlet authored_backlight_color = drawu.backlight_color * backlight_color_sample;\n",
		"\t\t\tlet authored_backlight_color = drawu.backlight_color;\n",
	);
	shader = shader.replace(
		"\t\tlet backlight_color_uv = uv * drawu.backlight_color_uv_offset_scale.zw + drawu.backlight_color_uv_offset_scale.xy;\n\t\tlet backlight_color_sample = textureSample(backlight_color_tex, base_samp, backlight_color_uv);\n\t\tlet authored_backlight_color = drawu.backlight_color * backlight_color_sample;\n",
		"\t\tlet authored_backlight_color = drawu.backlight_color;\n",
	);
	shader = shader.replace(
		"\t\tlet shadow2_color_texel = textureSample(shadow2_color_tex, shade_samp, uv);\n\t\tlet shadow2_color = mix(base, shadow2_color_texel.rgb, clamp(shadow2_color_texel.a, 0.0, 1.0)) * drawu.shadow2_color.rgb;\n",
		"\t\tlet shadow2_color = base * drawu.shadow2_color.rgb;\n",
	);
	shader = shader.replace(
		"\t\tlet shadow3_color_texel = textureSample(shadow3_color_tex, shade_samp, uv);\n\t\tlet shadow3_color = mix(base, shadow3_color_texel.rgb, clamp(shadow3_color_texel.a, 0.0, 1.0)) * drawu.shadow3_color.rgb;\n",
		"\t\tlet shadow3_color = base * drawu.shadow3_color.rgb;\n",
	);
	shader = shader.replace(
		"\t\t\tif (drawu.emission2nd_params.x > 0.5) {\n\t\t\t\tlet emission2nd_uv = uv * drawu.emission2nd_uv_offset_scale.zw + drawu.emission2nd_uv_offset_scale.xy;\n\t\t\t\tlet emission2nd_mask_uv = uv * drawu.emission2nd_blend_mask_uv_offset_scale.zw + drawu.emission2nd_blend_mask_uv_offset_scale.xy;\n\t\t\t\tvar emission2nd_sample = textureSample(emission2nd_tex, emissive_samp, emission2nd_uv) * drawu.emission2nd_color;\n\t\t\t\temission2nd_sample = emission2nd_sample * textureSample(emission2nd_blend_mask_tex, emissive_samp, emission2nd_mask_uv);\n\t\t\t\tif (drawu.emission2nd_grad_params.x > 0.5) {\n\t\t\t\t\tlet grad_u = fract(drawu.emission2nd_grad_params.y * frame.time_params.x);\n\t\t\t\t\temission2nd_sample.rgb = emission2nd_sample.rgb * textureSample(emission2nd_gradation_tex, emissive_samp, vec2<f32>(grad_u, 0.5)).rgb;\n\t\t\t\t}\n\t\t\t\tlet emission2nd_rgb = mix(emission2nd_sample.rgb, emission2nd_sample.rgb * base, clamp(drawu.emission2nd_params.y, 0.0, 1.0));\n\t\t\t\tlet emission2nd_blend = clamp(drawu.emission2nd_params.x * drawu.emission2nd_params.z * emission2nd_sample.a, 0.0, 1.0);\n\t\t\t\tlit = lil_blend_color(lit, emission2nd_rgb, emission2nd_blend, drawu.emission2nd_params.w);\n\t\t\t}\n",
		"",
	);
	shader = shader.replace(
		"textureSample(emission2nd_tex, emissive_samp, emission2nd_uv)",
		"vec4<f32>(1.0, 1.0, 1.0, 1.0)",
	);
	shader = shader.replace(
		"textureSample(emission2nd_blend_mask_tex, emissive_samp, emission2nd_mask_uv)",
		"vec4<f32>(1.0, 1.0, 1.0, 1.0)",
	);
	shader = shader.replace(
		"textureSample(emission2nd_gradation_tex, emissive_samp, vec2<f32>(grad_u, 0.5)).rgb",
		"vec3<f32>(1.0, 1.0, 1.0)",
	);
	shader = shader.replace(
		"\t\t\tif (drawu.emission2nd_params.x > 0.5) {\n\t\t\t\tlet emission2nd_uv = lil_calc_uv_scroll_rotate(uv, drawu.emission2nd_uv_offset_scale, drawu.emission2nd_uv_anim_params);\n\t\t\t\tlet emission2nd_mask_uv = lil_calc_uv_scroll_rotate(uv, drawu.emission2nd_blend_mask_uv_offset_scale, drawu.emission2nd_blend_mask_uv_anim_params);\n\t\t\t\tvar emission2nd_sample = textureSample(emission2nd_tex, emissive_samp, emission2nd_uv) * drawu.emission2nd_color;\n\t\t\t\temission2nd_sample = emission2nd_sample * textureSample(emission2nd_blend_mask_tex, emissive_samp, emission2nd_mask_uv);\n\t\t\t\tif (drawu.emission2nd_grad_params.x > 0.5) {\n\t\t\t\t\tlet grad_u = fract(drawu.emission2nd_grad_params.y * frame.time_params.x);\n\t\t\t\t\temission2nd_sample.rgb = emission2nd_sample.rgb * textureSample(emission2nd_gradation_tex, emissive_samp, vec2<f32>(grad_u, 0.5)).rgb;\n\t\t\t\t}\n\t\t\t\temission2nd_sample.rgb = mix(emission2nd_sample.rgb, emission2nd_sample.rgb * inv_lighting, clamp(drawu.emission2nd_grad_params.z, 0.0, 1.0));\n\t\t\t\tlet emission2nd_rgb = mix(emission2nd_sample.rgb, emission2nd_sample.rgb * base, clamp(drawu.emission2nd_params.y, 0.0, 1.0));\n\t\t\t\tlet emission2nd_blink = lil_calc_blink(drawu.emission2nd_blink_params);\n\t\t\t\tlet emission2nd_blend = clamp(drawu.emission2nd_params.x * drawu.emission2nd_params.z * emission2nd_blink * emission2nd_sample.a, 0.0, 1.0);\n\t\t\t\tlit = lil_blend_color(lit, emission2nd_rgb, emission2nd_blend, drawu.emission2nd_params.w);\n\t\t\t}\n",
		"",
	);
	shader = shader.replace(
		"\t\t\tif (drawu.emission2nd_params.x > 0.5) {\n\t\t\t\tlet emission2nd_uv_base = lil_select_emission_uv(drawu.emission2nd_uv_anim_params.w, uv, i.uv1, i.uv2, i.uv3, uv_rim);\n\t\t\t\tlet emission2nd_uv = lil_calc_uv_scroll_rotate(emission2nd_uv_base, drawu.emission2nd_uv_offset_scale, drawu.emission2nd_uv_anim_params) + parallax_offset * drawu.emission2nd_ext_params.x;\n\t\t\t\tlet emission2nd_mask_uv = lil_calc_uv_scroll_rotate(uv, drawu.emission2nd_blend_mask_uv_offset_scale, drawu.emission2nd_blend_mask_uv_anim_params);\n\t\t\t\tlet emission2nd_sample = textureSample(emission2nd_tex, emissive_samp, emission2nd_uv) * drawu.emission2nd_color * textureSample(emission2nd_blend_mask_tex, emissive_samp, emission2nd_mask_uv);\n\t\t\t\tvar emission2nd_rgb_work = emission2nd_sample.rgb;\n\t\t\t\tif (drawu.emission2nd_grad_params.x > 0.5) {\n\t\t\t\t\tlet grad_u = fract(drawu.emission2nd_grad_params.y * frame.time_params.x);\n\t\t\t\t\temission2nd_rgb_work = emission2nd_rgb_work * textureSample(emission2nd_gradation_tex, emissive_samp, vec2<f32>(grad_u, 0.5)).rgb;\n\t\t\t\t}\n\t\t\t\temission2nd_rgb_work = mix(emission2nd_rgb_work, emission2nd_rgb_work * inv_lighting, clamp(drawu.emission2nd_grad_params.z, 0.0, 1.0));\n\t\t\t\tlet emission2nd_rgb = mix(emission2nd_rgb_work, emission2nd_rgb_work * base, clamp(drawu.emission2nd_params.y, 0.0, 1.0));\n\t\t\t\tlet emission2nd_blink = lil_calc_blink(drawu.emission2nd_blink_params);\n\t\t\t\tlet emission2nd_blend = clamp(drawu.emission2nd_params.x * drawu.emission2nd_params.z * emission2nd_blink * emission2nd_sample.a, 0.0, 1.0);\n\t\t\t\tlit = lil_blend_color(lit, emission2nd_rgb, emission2nd_blend, drawu.emission2nd_params.w);\n\t\t\t}\n",
		"",
	);
	shader = shader.replace(
		"\t\t\tif (drawu.emission2nd_params.x > 0.5) {\n\t\t\t\tlet emission2nd_uv_base = lil_select_emission_uv(drawu.emission2nd_uv_anim_params.w, uv, i.uv1, i.uv2, i.uv3, uv_rim);\n\t\t\t\tlet emission2nd_uv = lil_calc_uv_scroll_rotate(emission2nd_uv_base, drawu.emission2nd_uv_offset_scale, drawu.emission2nd_uv_anim_params) + parallax_offset * drawu.emission2nd_ext_params.x;\n\t\t\t\tlet emission2nd_mask_uv = lil_calc_uv_scroll_rotate(uv, drawu.emission2nd_blend_mask_uv_offset_scale, drawu.emission2nd_blend_mask_uv_anim_params);\n\t\t\t\tlet emission2nd_sample = textureSample(emission2nd_tex, emissive_samp, emission2nd_uv) * drawu.emission2nd_color * textureSample(emission2nd_blend_mask_tex, emissive_samp, emission2nd_mask_uv);\n\t\t\t\tvar emission2nd_rgb_work = emission2nd_sample.rgb;\n\t\t\t\tif (drawu.emission2nd_grad_params.x > 0.5) {\n\t\t\t\t\tlet grad_u = fract(drawu.emission2nd_grad_params.y * frame.time_params.x);\n\t\t\t\t\temission2nd_rgb_work = emission2nd_rgb_work * textureSample(emission2nd_gradation_tex, emissive_samp, vec2<f32>(grad_u, 0.5)).rgb;\n\t\t\t\t}\n\t\t\t\temission2nd_rgb_work = mix(emission2nd_rgb_work, emission2nd_rgb_work * inv_lighting, clamp(drawu.emission2nd_grad_params.z, 0.0, 1.0));\n\t\t\t\tlet emission2nd_rgb = mix(emission2nd_rgb_work, emission2nd_rgb_work * base, clamp(drawu.emission2nd_params.y, 0.0, 1.0));\n\t\t\t\tlet emission2nd_blink = lil_calc_blink(drawu.emission2nd_blink_params);\n\t\t\t\tlet emission2nd_blend = clamp(drawu.emission2nd_params.x * drawu.emission2nd_params.z * emission2nd_blink * emission2nd_sample.a * emission_transparency, 0.0, 1.0);\n\t\t\t\tlit = lil_blend_color(lit, emission2nd_rgb, emission2nd_blend, drawu.emission2nd_params.w);\n\t\t\t}\n",
		"",
	);
	shader = shader.replace(
		"fn apply_main_gradation(color: vec3<f32>) -> vec3<f32> {\n\tlet strength = clamp(drawu.main_gradation_params.x * drawu.main_gradation_params.y, 0.0, 1.0);\n\tif (strength <= 0.000001) {\n\t\treturn color;\n\t}\n\tlet c = linear_to_srgb(clamp(color, vec3<f32>(0.0), vec3<f32>(1.0)));\n\tlet mapped_srgb = vec3<f32>(\n\t\ttextureSample(main_gradation_tex, base_samp, vec2<f32>(c.r, 0.5)).r,\n\t\ttextureSample(main_gradation_tex, base_samp, vec2<f32>(c.g, 0.5)).g,\n\t\ttextureSample(main_gradation_tex, base_samp, vec2<f32>(c.b, 0.5)).b\n\t);\n\tlet mapped = srgb_to_linear(mapped_srgb);\n\treturn mix(color, mapped, strength);\n}\n",
		"fn apply_main_gradation(color: vec3<f32>) -> vec3<f32> {\n\treturn color;\n}\n",
	);
	shader = shader.replace(
		"\t\t\tif (drawu.emission_grad_params.x > 0.5) {\n\t\t\t\tlet grad_u = fract(drawu.emission_grad_params.y * frame.time_params.x);\n\t\t\t\temission_color = emission_color * textureSample(emission_gradation_tex, emissive_samp, vec2<f32>(grad_u, 0.5)).rgb;\n\t\t\t}\n",
		"",
	);
	shader = shader.replace(
		"\t\t\tif (drawu.emission_grad_params.x > 0.5) {\n\t\t\t\tlet grad_u = fract(drawu.emission_grad_params.y * frame.time_params.x + audio_link_value * drawu.audio_link_params.w);\n\t\t\t\temission_color = emission_color * textureSample(emission_gradation_tex, emissive_samp, vec2<f32>(grad_u, 0.5)).rgb;\n\t\t\t}\n",
		"",
	);
	shader = shader.replace(
		"\t\t\tif (drawu.emission2nd_params.x > 0.5) {\n\t\t\t\tlet emission2nd_uv_base = lil_select_emission_uv(drawu.emission2nd_uv_anim_params.w, uv, i.uv1, i.uv2, i.uv3, uv_rim);\n\t\t\t\tlet emission2nd_uv = lil_calc_uv_scroll_rotate(emission2nd_uv_base, drawu.emission2nd_uv_offset_scale, drawu.emission2nd_uv_anim_params) + parallax_offset * drawu.emission2nd_ext_params.x;\n\t\t\t\tlet emission2nd_mask_uv = lil_calc_uv_scroll_rotate(uv, drawu.emission2nd_blend_mask_uv_offset_scale, drawu.emission2nd_blend_mask_uv_anim_params);\n\t\t\t\tlet emission2nd_sample = textureSample(emission2nd_tex, emissive_samp, emission2nd_uv) * drawu.emission2nd_color * textureSample(emission2nd_blend_mask_tex, emissive_samp, emission2nd_mask_uv);\n\t\t\t\tvar emission2nd_rgb_work = emission2nd_sample.rgb;\n\t\t\t\tif (drawu.emission2nd_grad_params.x > 0.5) {\n\t\t\t\t\tlet grad_u = fract(drawu.emission2nd_grad_params.y * frame.time_params.x + audio_link_value * drawu.audio_link_ext.y);\n\t\t\t\t\temission2nd_rgb_work = emission2nd_rgb_work * textureSample(emission2nd_gradation_tex, emissive_samp, vec2<f32>(grad_u, 0.5)).rgb;\n\t\t\t\t}\n\t\t\t\temission2nd_rgb_work = mix(emission2nd_rgb_work, emission2nd_rgb_work * inv_lighting, clamp(drawu.emission2nd_grad_params.z, 0.0, 1.0));\n\t\t\t\tlet emission2nd_rgb = mix(emission2nd_rgb_work, emission2nd_rgb_work * base, clamp(drawu.emission2nd_params.y, 0.0, 1.0));\n\t\t\t\tlet emission2nd_blink = lil_calc_blink(drawu.emission2nd_blink_params);\n\t\t\t\tlet emission2nd_audio = mix(1.0, audio_link_value, clamp(drawu.audio_link_ext.x, 0.0, 1.0));\n\t\t\t\tlet emission2nd_blend = clamp(drawu.emission2nd_params.x * drawu.emission2nd_params.z * emission2nd_blink * emission2nd_sample.a * emission2nd_audio * emission_transparency, 0.0, 1.0);\n\t\t\t\tlit = lil_blend_color(lit, emission2nd_rgb, emission2nd_blend, drawu.emission2nd_params.w);\n\t\t\t}\n",
		"",
	);
	shader = shader.replace(
		"\tif (UNTOON_FEATURE_NORMAL_SECOND > 0.5 && drawu.normal2nd_params.x > 0.5) {\n\t\tlet normal2nd_base_uv = lil_select_uv(drawu.normal2nd_params.z, uv, uv1, uv2, uv3);\n\t\tlet normal2nd_uv = normal2nd_base_uv * drawu.normal2nd_uv_offset_scale.zw + drawu.normal2nd_uv_offset_scale.xy;\n\t\tlet normal2nd_scale_mask_uv = uv * drawu.normal2nd_scale_mask_uv_offset_scale.zw + drawu.normal2nd_scale_mask_uv_offset_scale.xy;\n\t\tlet scale_mask = textureSample(normal2nd_scale_mask_tex, base_samp, normal2nd_scale_mask_uv).r;\n\t\tlet tn2 = lil_unpack_normal_scale(textureSample(normal2nd_tex, normal_samp, normal2nd_uv), drawu.normal2nd_params.y * scale_mask);\n\t\ttn = vec3<f32>(tn.xy + tn2.xy, tn.z * tn2.z);\n\t}\n",
		"",
	);
	shader = shader.replace(
		"let shadow_border_mask = lil_shadow_border_ao_mask(textureSample(shadow_border_mask_tex, shadow_border_mask_samp, uv).rgb);\n\t\tlet shadow_blur_mask = textureSample(shadow_blur_mask_tex, shadow_blur_mask_samp, uv).rgb;",
		"let shadow_border_mask = lil_shadow_border_ao_mask(vec3<f32>(1.0, 1.0, 1.0));\n\t\tlet shadow_blur_mask = vec3<f32>(1.0, 1.0, 1.0);",
	);
	shader = shader.replace(
		"let matcap2_tex_color = textureSampleLevel(matcap2_tex, matcap_samp, matcap2_uv, max(drawu.matcap2_ext_params.z, 0.0));",
		"let matcap2_tex_color = vec4<f32>(1.0, 1.0, 1.0, 1.0);",
	);
	shader = shader.replace(
		"let matcap2_blend_mask = textureSample(matcap2_blend_mask_tex, matcap_blend_mask_samp, matcap2_blend_mask_uv).rgb;",
		"let matcap2_blend_mask = vec3<f32>(1.0, 1.0, 1.0);",
	);
	shader = shader.replace(
		"\t\tif (drawu.matcap2_params.x > 0.0) {\n\t\t\tlet matcap2_base_n = normalize(mix(geometry_n, n, clamp(drawu.matcap2_ext_params.x, 0.0, 1.0)));\n\t\t\tlet matcap2_n = normalize(mix(matcap2_base_n, anisotropy_n, clamp(drawu.anisotropy_ext_params.x * anisotropy_basis.enabled, 0.0, 1.0)));\n\t\t\tlet matcap2_uv = toon_matcap_uv(i.uv1, matcap2_n, v, drawu.matcap2_tex_uv_offset_scale, drawu.matcap_uv_ext_params.zw, drawu.matcap_uv_params.z, drawu.matcap_uv_params.w);\n\t\t\tlet matcap2_tex_color = textureSampleLevel(matcap2_tex, matcap_samp, matcap2_uv, max(drawu.matcap2_ext_params.z, 0.0));\n\t\t\tlet matcap2_lighting = mix(vec3<f32>(1.0, 1.0, 1.0), frame.light_color.rgb * frame.light_color.w, clamp(drawu.matcap2_params.z, 0.0, 1.0));\n\t\t\tlet matcap2_raw = drawu.matcap2_factor.rgb * matcap2_tex_color.rgb * matcap2_lighting;\n\t\t\tlet matcap2_albedo = mix(matcap2_raw, matcap2_raw * base, clamp(drawu.matcap2_params.y, 0.0, 1.0));\n\t\t\tlet matcap2_blend_mask_uv = uv * drawu.matcap2_blend_mask_uv_offset_scale.zw + drawu.matcap2_blend_mask_uv_offset_scale.xy;\n\t\t\tlet matcap2_blend_mask = textureSample(matcap2_blend_mask_tex, matcap_blend_mask_samp, matcap2_blend_mask_uv).rgb;\n\t\t\tlet matcap2_shadow = mix(1.0, lil_effect_shadowmix, clamp(drawu.matcap2_ext_params.y, 0.0, 1.0));\n\t\t\tlet matcap2_backface = lil_backface_visibility(drawu.matcap2_ext_params.w, front_facing);\n\t\t\tlet matcap2_transparency = mix(1.0, a, clamp(drawu.transparency_params.y, 0.0, 1.0));\n\t\t\tlet matcap2_blend = clamp(drawu.matcap2_params.x * drawu.matcap2_factor.a * matcap2_tex_color.a * matcap2_blend_mask * matcap2_shadow * matcap2_backface * matcap2_transparency, vec3<f32>(0.0), vec3<f32>(1.0));\n\t\t\tlit = lil_blend_color3(lit, matcap2_albedo, matcap2_blend, drawu.matcap2_params.w);\n\t\t}\n",
		"",
	);
	shader
}

/// シェーダとボーンバッファの上限（io-gltf のスキン joint 上限と同値に保つ）。
pub(crate) const MAX_BONES: usize = 512;

const BONE_MATRIX_SIZE: u64 = (16 * std::mem::size_of::<f32>()) as u64;
const STATIC_IDENTITY_SKIN_PALETTE_NODE: usize = usize::MAX;
const MORPH_WEIGHT_BUFFER_MIN_SIZE: u64 = 16;
const MORPH_DELTA_BUFFER_MIN_SIZE: u64 = 16;

#[repr(C)]
#[derive(Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct MeshFrameGpu {
	view_proj: [[f32; 4]; 4],
	view: [[f32; 4]; 4],
	light_dir: [f32; 4],
	camera_pos: [f32; 4],
	light_color: [f32; 4],
	ambient_color: [f32; 4],
	time_params: [f32; 4],
	audio_link_params: [f32; 4],
	_pad: [[f32; 4]; 2],
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
/// bit5=emissive OFF (debug), bit6=shade_term を base 置換 (debug), bit7=toon path を base のみで早期 return (debug),
/// bit8=normalTexture OFF (debug), bit9=double-sided material, bit10=occlusion texture available, bit11=cull front,
/// bit12=lilToon-like source material, bit13=lilToon Gem source material, bit14=lilToon Refraction source material,
/// bit15=lilToon color blend is additive (`SrcBlend=One`, `DstBlend=One`)。
#[repr(C)]
#[derive(Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct MeshDrawMaterialGpu {
	base_color: [f32; 4],
	backface_color: [f32; 4],
	params: [f32; 4],
	shade_color: [f32; 4],
	shading_params: [f32; 4],
	shadow_params: [f32; 4],
	shadow_ext_params: [f32; 4],
	shadow_ao_params: [f32; 4],
	shadow_ao_shift: [f32; 4],
	shadow_ao_shift2: [f32; 4],
	shadow_border_color: [f32; 4],
	shadow2_color: [f32; 4],
	shadow2_params: [f32; 4],
	shadow3_color: [f32; 4],
	shadow3_params: [f32; 4],
	matcap_factor: [f32; 4],
	matcap_params: [f32; 4],
	matcap_ext_params: [f32; 4],
	matcap_bump_params: [f32; 4],
	matcap2_factor: [f32; 4],
	matcap2_params: [f32; 4],
	matcap2_ext_params: [f32; 4],
	matcap2_bump_params: [f32; 4],
	matcap_uv_params: [f32; 4],
	matcap_uv_ext_params: [f32; 4],
	reflection_color: [f32; 4],
	reflection_control: [f32; 4],
	reflection_params: [f32; 4],
	reflection_ext_params: [f32; 4],
	reflection_cube_color: [f32; 4],
	anisotropy_params: [f32; 4],
	anisotropy_ext_params: [f32; 4],
	anisotropy2_params: [f32; 4],
	anisotropy_width_params: [f32; 4],
	gem_env_color: [f32; 4],
	gem_params: [f32; 4],
	gem_particle_color: [f32; 4],
	specular_toon_params: [f32; 4],
	rim_color: [f32; 4],
	rim_params: [f32; 4],
	rim_control: [f32; 4],
	rim_ext_params: [f32; 4],
	rim_indirect_color: [f32; 4],
	rim_indirect_params: [f32; 4],
	rim_indirect_ext_params: [f32; 4],
	rim_shade_color: [f32; 4],
	rim_shade_params: [f32; 4],
	backlight_color: [f32; 4],
	backlight_params: [f32; 4],
	backlight_ext_params: [f32; 4],
	backlight_shadow_params: [f32; 4],
	backlight_color_uv_offset_scale: [f32; 4],
	glitter_color: [f32; 4],
	glitter_params1: [f32; 4],
	glitter_params2: [f32; 4],
	glitter_control: [f32; 4],
	glitter_ext: [f32; 4],
	glitter_ext2: [f32; 4],
	glitter_ext3: [f32; 4],
	glitter_color_uv_offset_scale: [f32; 4],
	glitter_shape_uv_offset_scale: [f32; 4],
	glitter_atlas: [f32; 4],
	distance_fade: [f32; 4],
	distance_fade_color: [f32; 4],
	distance_fade_rim_color: [f32; 4],
	distance_fade_params: [f32; 4],
	dissolve_color: [f32; 4],
	dissolve_params: [f32; 4],
	dissolve_pos: [f32; 4],
	dissolve_ext: [f32; 4],
	dissolve_mask_uv_offset_scale: [f32; 4],
	dissolve_noise_uv_offset_scale: [f32; 4],
	dissolve_noise_uv_anim_params: [f32; 4],
	parallax_params: [f32; 4],
	parallax_uv_offset_scale: [f32; 4],
	id_mask_params: [f32; 4],
	id_mask_flags0: [f32; 4],
	id_mask_flags1: [f32; 4],
	id_mask_prior_flags0: [f32; 4],
	id_mask_prior_flags1: [f32; 4],
	id_mask_indices0: [f32; 4],
	id_mask_indices1: [f32; 4],
	udim_discard_params: [f32; 4],
	udim_discard_row0: [f32; 4],
	udim_discard_row1: [f32; 4],
	udim_discard_row2: [f32; 4],
	udim_discard_row3: [f32; 4],
	emission_color: [f32; 4],
	emission_params: [f32; 4],
	emission_blink_params: [f32; 4],
	emission_grad_params: [f32; 4],
	emission2nd_color: [f32; 4],
	emission2nd_params: [f32; 4],
	emission2nd_blink_params: [f32; 4],
	emission2nd_grad_params: [f32; 4],
	emission2nd_ext_params: [f32; 4],
	emission2nd_uv_offset_scale: [f32; 4],
	emission2nd_uv_anim_params: [f32; 4],
	emission_blend_mask_uv_offset_scale: [f32; 4],
	emission_blend_mask_uv_anim_params: [f32; 4],
	emission2nd_blend_mask_uv_offset_scale: [f32; 4],
	emission2nd_blend_mask_uv_anim_params: [f32; 4],
	audio_link_params: [f32; 4],
	audio_link_default: [f32; 4],
	audio_link_uv_params: [f32; 4],
	audio_link_start: [f32; 4],
	audio_link_ext: [f32; 4],
	audio_link_vertex_params: [f32; 4],
	audio_link_vertex_uv_params: [f32; 4],
	audio_link_vertex_start: [f32; 4],
	audio_link_vertex_strength: [f32; 4],
	audio_link_mask_params: [f32; 4],
	audio_link_mask_uv_offset_scale: [f32; 4],
	audio_link_mask_uv_anim_params: [f32; 4],
	audio_link_local_map_params: [f32; 4],
	outline_color: [f32; 4],
	outline_params: [f32; 4],
	outline_lit_color: [f32; 4],
	outline_lit_params: [f32; 4],
	outline_ext_params: [f32; 4],
	alpha_mask_params: [f32; 4],
	fur_params: [f32; 4],
	fur_vector_params: [f32; 4],
	fur_noise_params: [f32; 4],
	fur_ext_params: [f32; 4],
	fur_rim_color: [f32; 4],
	fur_rim_params: [f32; 4],
	alpha_ext_params: [f32; 4],
	lighting_ext_params: [f32; 4],
	rendering_ext_params: [f32; 4],
	transparency_params: [f32; 4],
	material_ext_params: [f32; 4],
	emissive_factor: [f32; 4],
	uv_anim_params: [f32; 4],
	uv_offset_scale: [f32; 4],
	normal_uv_offset_scale: [f32; 4],
	normal2nd_uv_offset_scale: [f32; 4],
	normal2nd_scale_mask_uv_offset_scale: [f32; 4],
	normal2nd_params: [f32; 4],
	shade_uv_offset_scale: [f32; 4],
	rim_uv_offset_scale: [f32; 4],
	emission_uv_offset_scale: [f32; 4],
	emission_uv_anim_params: [f32; 4],
	reflection_color_uv_offset_scale: [f32; 4],
	smoothness_uv_offset_scale: [f32; 4],
	metallic_uv_offset_scale: [f32; 4],
	anisotropy_tangent_uv_offset_scale: [f32; 4],
	anisotropy_scale_mask_uv_offset_scale: [f32; 4],
	anisotropy_shift_noise_uv_offset_scale: [f32; 4],
	shadow_strength_mask_uv_offset_scale: [f32; 4],
	shadow_border_mask_uv_offset_scale: [f32; 4],
	shadow_blur_mask_uv_offset_scale: [f32; 4],
	matcap_blend_mask_uv_offset_scale: [f32; 4],
	matcap_tex_uv_offset_scale: [f32; 4],
	matcap_bump_uv_offset_scale: [f32; 4],
	matcap2_blend_mask_uv_offset_scale: [f32; 4],
	matcap2_tex_uv_offset_scale: [f32; 4],
	matcap2_bump_uv_offset_scale: [f32; 4],
	alpha_mask_uv_offset_scale: [f32; 4],
	main_color_adjust_params: [f32; 4],
	main_gradation_params: [f32; 4],
	main2nd_color: [f32; 4],
	main2nd_params: [f32; 4],
	main2nd_ext: [f32; 4],
	main2nd_distance_fade: [f32; 4],
	main2nd_decal_flags: [f32; 4],
	main2nd_decal_transform: [f32; 4],
	main2nd_decal_animation: [f32; 4],
	main2nd_decal_sub_param: [f32; 4],
	main2nd_uv_offset_scale: [f32; 4],
	main2nd_blend_mask_uv_offset_scale: [f32; 4],
	main2nd_dissolve_color: [f32; 4],
	main2nd_dissolve_params: [f32; 4],
	main2nd_dissolve_pos: [f32; 4],
	main2nd_dissolve_ext: [f32; 4],
	main2nd_dissolve_mask_uv_offset_scale: [f32; 4],
	main2nd_dissolve_noise_uv_offset_scale: [f32; 4],
	main2nd_dissolve_noise_uv_anim_params: [f32; 4],
	main3rd_color: [f32; 4],
	main3rd_params: [f32; 4],
	main3rd_ext: [f32; 4],
	main3rd_distance_fade: [f32; 4],
	main3rd_decal_flags: [f32; 4],
	main3rd_decal_transform: [f32; 4],
	main3rd_decal_animation: [f32; 4],
	main3rd_decal_sub_param: [f32; 4],
	main3rd_uv_offset_scale: [f32; 4],
	main3rd_blend_mask_uv_offset_scale: [f32; 4],
	main3rd_dissolve_color: [f32; 4],
	main3rd_dissolve_params: [f32; 4],
	main3rd_dissolve_pos: [f32; 4],
	main3rd_dissolve_ext: [f32; 4],
	main3rd_dissolve_mask_uv_offset_scale: [f32; 4],
	main3rd_dissolve_noise_uv_offset_scale: [f32; 4],
	main3rd_dissolve_noise_uv_anim_params: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct MorphMetaGpu {
	target_count: u32,
	vertex_count: u32,
	_pad: [u32; 2],
}

struct MorphGpuResources {
	meta_buffer: wgpu::Buffer,
	weight_buffer: wgpu::Buffer,
	delta_buffer: wgpu::Buffer,
	bind_group: wgpu::BindGroup,
}

#[derive(Clone)]
struct SharedMorphDeltaResources {
	meta_buffer: wgpu::Buffer,
	delta_buffer: wgpu::Buffer,
	target_count: u32,
}

const _: () = assert!(std::mem::size_of::<MeshFrameGpu>() == 256);
const _: () = assert!(std::mem::size_of::<MeshDrawTransformGpu>() == 64);
const _: () = assert!(std::mem::size_of::<MeshDrawMaterialGpu>() == 3120);
const _: () = assert!(std::mem::size_of::<MorphMetaGpu>() == 16);

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
	pos: [f32; 3],
	norm: [f32; 3],
	tangent: [f32; 4],
	uv: [f32; 2],
	uv1: [f32; 2],
	uv2: [f32; 2],
	uv3: [f32; 2],
	joints: [u16; 4],
	weights: [f32; 4],
	color: [f32; 4],
}

impl DynamicsMeshClothAssistVertex for Vertex {
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

const _: () = assert!(std::mem::size_of::<Vertex>() == 112);

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct TextureUploadSummary {
	pub image_count: u32,
	pub deferred_image_upload_count: u32,
	pub deferred_image_mip_bytes: u64,
	pub resized_count: u32,
	pub cubemap_count: u32,
	pub deferred_cubemap_upload_count: u32,
	pub cubemap_converted_count: u32,
	pub cubemap_fallback_count: u32,
	pub cubemap_cache_hits: u32,
	pub cubemap_cache_misses: u32,
	pub cubemap_cache_writes: u32,
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
	pub cubemap_uploaded_bytes: u64,
	pub deferred_cubemap_mip_bytes: u64,
	pub max_source_dimension: u32,
	pub max_uploaded_dimension: u32,
	pub limit_max_dimension: Option<u32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub skin_tone_matching_debug: Option<SkinToneMatchingDebug>,
}

impl TextureUploadSummary {
	fn record_image(
		&mut self,
		source_width: u32,
		source_height: u32,
		uploaded_width: u32,
		uploaded_height: u32,
		uploaded_mip_bytes: u64,
		uploaded: bool,
	) {
		self.image_count += 1;
		self.source_bytes += (source_width as u64) * (source_height as u64) * 4;
		if uploaded {
			self.uploaded_mip_bytes += uploaded_mip_bytes;
		} else {
			self.deferred_image_upload_count += 1;
			self.deferred_image_mip_bytes += uploaded_mip_bytes;
		}
		self.max_source_dimension = self.max_source_dimension.max(source_width.max(source_height));
		self.max_uploaded_dimension = self.max_uploaded_dimension.max(uploaded_width.max(uploaded_height));
		if source_width != uploaded_width || source_height != uploaded_height {
			self.resized_count += 1;
		}
	}
}

#[derive(Default)]
struct TexturePrepareSummary {
	images: u32,
	resident_images: u32,
	deferred_images: u32,
	resident_elapsed: Duration,
	deferred_elapsed: Duration,
	cube_elapsed: Duration,
	source_elapsed: Duration,
	rgba_elapsed: Duration,
	cache_lookup_elapsed: Duration,
	cache_read_elapsed: Duration,
	processed_elapsed: Duration,
	payload_elapsed: Duration,
	upload_elapsed: Duration,
	cache_hits: u32,
	cache_misses: u32,
	cache_writes: u32,
	cache_read_bytes: u64,
	compressed_cache_hits: u32,
	compressed_cache_misses: u32,
	compressed_cache_writes: u32,
	roles: [TexturePrepareRoleSummary; TEXTURE_PREPARE_ROLE_COUNT],
}

const TEXTURE_PREPARE_ROLE_COUNT: usize = 8;

#[derive(Clone, Copy, Default)]
struct TexturePrepareRoleSummary {
	images: u32,
	resident_images: u32,
	cache_read_elapsed: Duration,
	upload_elapsed: Duration,
	cache_read_bytes: u64,
	cache_hits: u32,
	compressed_cache_hits: u32,
}

fn texture_prepare_role_index(role: TextureRole) -> usize {
	match role {
		TextureRole::Face => 0,
		TextureRole::Eyes => 1,
		TextureRole::Clothing => 2,
		TextureRole::Normal => 3,
		TextureRole::Occlusion => 4,
		TextureRole::Emissive => 5,
		TextureRole::GenericColor => 6,
		TextureRole::Data => 7,
	}
}

fn texture_prepare_role_label(index: usize) -> &'static str {
	match index {
		0 => "Face",
		1 => "Eyes",
		2 => "Clothing",
		3 => "Normal",
		4 => "Occlusion",
		5 => "Emissive",
		6 => "GenericColor",
		7 => "Data",
		_ => "Unknown",
	}
}

impl TexturePrepareSummary {
	fn record(
		&mut self,
		image_index: usize,
		image_name: Option<&str>,
		mime_type: Option<&str>,
		role: TextureRole,
		resident: bool,
		elapsed: Duration,
		timings: TextureImagePrepareTimings,
		cache_event: TextureCacheEvent,
		compressed_cache_event: TextureCacheEvent,
	) {
		self.images += 1;
		if resident {
			self.resident_images += 1;
			self.resident_elapsed += elapsed;
		} else {
			self.deferred_images += 1;
			self.deferred_elapsed += elapsed;
		}
		self.cube_elapsed += timings.cube;
		self.source_elapsed += timings.source;
		self.rgba_elapsed += timings.rgba;
		self.cache_lookup_elapsed += timings.cache_lookup;
		let processed_cache_read_elapsed = cache_event.read_elapsed;
		self.cache_read_elapsed += timings.cache_read + processed_cache_read_elapsed;
		self.processed_elapsed += timings.processed.saturating_sub(processed_cache_read_elapsed);
		self.payload_elapsed += timings.payload;
		self.upload_elapsed += timings.upload;
		let role_summary = &mut self.roles[texture_prepare_role_index(role)];
		role_summary.images += 1;
		if resident {
			role_summary.resident_images += 1;
		}
		role_summary.cache_read_elapsed += timings.cache_read + processed_cache_read_elapsed;
		role_summary.upload_elapsed += timings.upload;
		role_summary.cache_read_bytes = role_summary.cache_read_bytes.saturating_add(cache_event.read_bytes);
		if cache_event.hit {
			self.cache_hits += 1;
			role_summary.cache_hits += 1;
		}
		if cache_event.miss {
			self.cache_misses += 1;
		}
		if cache_event.write {
			self.cache_writes += 1;
		}
		self.cache_read_bytes = self.cache_read_bytes.saturating_add(cache_event.read_bytes);
		if compressed_cache_event.hit {
			self.compressed_cache_hits += 1;
			role_summary.compressed_cache_hits += 1;
		}
		if compressed_cache_event.miss {
			self.compressed_cache_misses += 1;
		}
		if compressed_cache_event.write {
			self.compressed_cache_writes += 1;
		}
		let elapsed_ms = elapsed.as_secs_f64() * 1000.0;
		let read_mb = cache_event.read_bytes as f64 / (1024.0 * 1024.0);
		if elapsed_ms >= 50.0 || read_mb >= 64.0 {
			let stage_ms = |elapsed: Duration| elapsed.as_secs_f64() * 1000.0;
			let image_name = image_name.unwrap_or("");
			let mime_type = mime_type.unwrap_or("");
			eprintln!(
				"un-avatar-renderer: gpu scene texture image={} name={image_name:?} mime={mime_type:?} resident={} role={role:?}: {elapsed_ms:.1}ms cube={:.1}ms source={:.1}ms rgba={:.1}ms cache_lookup={:.1}ms cache_read={:.1}ms processed={:.1}ms payload={:.1}ms upload={:.1}ms read_mb={read_mb:.1}",
				image_index,
				resident,
				stage_ms(timings.cube),
				stage_ms(timings.source),
				stage_ms(timings.rgba),
				stage_ms(timings.cache_lookup),
				stage_ms(timings.cache_read + processed_cache_read_elapsed),
				stage_ms(timings.processed.saturating_sub(processed_cache_read_elapsed)),
				stage_ms(timings.payload),
				stage_ms(timings.upload),
			);
		}
	}

	fn log(&self, total: Duration) {
		let total_ms = total.as_secs_f64() * 1000.0;
		if total_ms < 50.0 && self.images == 0 {
			return;
		}
		eprintln!(
			"un-avatar-renderer: gpu scene texture prepare summary: total={total_ms:.1}ms images={} resident={} deferred={} resident_elapsed={:.1}ms deferred_elapsed={:.1}ms cube={:.1}ms source={:.1}ms rgba={:.1}ms cache_lookup={:.1}ms cache_read={:.1}ms processed={:.1}ms payload={:.1}ms upload={:.1}ms processed_cache={}/{}/{} processed_cache_read_mb={:.1} compressed_cache={}/{}/{}",
			self.images,
			self.resident_images,
			self.deferred_images,
			self.resident_elapsed.as_secs_f64() * 1000.0,
			self.deferred_elapsed.as_secs_f64() * 1000.0,
			self.cube_elapsed.as_secs_f64() * 1000.0,
			self.source_elapsed.as_secs_f64() * 1000.0,
			self.rgba_elapsed.as_secs_f64() * 1000.0,
			self.cache_lookup_elapsed.as_secs_f64() * 1000.0,
			self.cache_read_elapsed.as_secs_f64() * 1000.0,
			self.processed_elapsed.as_secs_f64() * 1000.0,
			self.payload_elapsed.as_secs_f64() * 1000.0,
			self.upload_elapsed.as_secs_f64() * 1000.0,
			self.cache_hits,
			self.cache_misses,
			self.cache_writes,
			self.cache_read_bytes as f64 / (1024.0 * 1024.0),
			self.compressed_cache_hits,
			self.compressed_cache_misses,
			self.compressed_cache_writes,
		);
		let role_parts: Vec<String> = self
			.roles
			.iter()
			.enumerate()
			.filter(|(_, role)| role.images > 0)
			.map(|(index, role)| {
				format!(
					"{}={}/{} read={:.1}MB/{:.1}ms upload={:.1}ms cache_hits={} compressed_hits={}",
					texture_prepare_role_label(index),
					role.resident_images,
					role.images,
					role.cache_read_bytes as f64 / (1024.0 * 1024.0),
					role.cache_read_elapsed.as_secs_f64() * 1000.0,
					role.upload_elapsed.as_secs_f64() * 1000.0,
					role.cache_hits,
					role.compressed_cache_hits,
				)
			})
			.collect();
		if !role_parts.is_empty() {
			eprintln!("un-avatar-renderer: gpu scene texture prepare roles: {}", role_parts.join(" "));
		}
	}
}

#[derive(Clone, Copy, Default)]
struct TextureImagePrepareTimings {
	cube: Duration,
	source: Duration,
	rgba: Duration,
	cache_lookup: Duration,
	cache_read: Duration,
	processed: Duration,
	payload: Duration,
	upload: Duration,
}

#[derive(Clone, Debug)]
pub(crate) struct SceneMeshBuildProgress {
	pub phase: &'static str,
	pub current: u32,
	pub total: u32,
	pub message: String,
}

fn log_slow_gpu_scene_step(label: impl std::fmt::Display, elapsed: std::time::Duration) {
	let elapsed_ms = elapsed.as_secs_f64() * 1000.0;
	if elapsed_ms >= 50.0 {
		eprintln!("un-avatar-renderer: gpu scene {label}: {elapsed_ms:.1}ms");
	}
}

fn take_gpu_scene_step_elapsed(step_start: &mut Instant) -> Duration {
	let elapsed = step_start.elapsed();
	*step_start = Instant::now();
	elapsed
}

fn log_slow_gpu_scene_primitive(
	mesh_index: usize,
	primitive_index: usize,
	vertex_count: usize,
	index_count: usize,
	morph_target_count: usize,
	asset_resident: bool,
	total: Duration,
	timings: &[(&str, Duration)],
) {
	let total_ms = total.as_secs_f64() * 1000.0;
	if total_ms < 50.0 {
		return;
	}
	let details = timings
		.iter()
		.map(|(label, elapsed)| format!("{label}={:.1}ms", elapsed.as_secs_f64() * 1000.0))
		.collect::<Vec<_>>()
		.join(" ");
	eprintln!(
		"un-avatar-renderer: gpu scene primitive mesh={mesh_index} primitive={primitive_index} vertices={vertex_count} indices={index_count} morphs={morph_target_count} resident={asset_resident} total={total_ms:.1}ms {details}"
	);
}

#[derive(Default)]
struct MeshPrepareSummary {
	prepared_primitives: u32,
	resident_primitives: u32,
	deferred_primitives: u32,
	skipped_invisible_primitives: u32,
	skipped_empty_primitives: u32,
	expanded_cache_hits: u32,
	expanded_cache_misses: u32,
	expanded_uncacheable: u32,
	vertices: u64,
	indices: u64,
	resident_vertex_bytes: u64,
	resident_index_bytes: u64,
	deferred_vertex_bytes: u64,
	deferred_index_bytes: u64,
	mesh_cloth_assist_vertices: u64,
	material_elapsed: Duration,
	dynamic_morph_elapsed: Duration,
	expand_elapsed: Duration,
	skinning_elapsed: Duration,
	skin_palette_elapsed: Duration,
	buffer_upload_elapsed: Duration,
	material_bind_elapsed: Duration,
	morph_resource_elapsed: Duration,
	fur_resource_elapsed: Duration,
	draw_push_elapsed: Duration,
}

impl MeshPrepareSummary {
	fn record_timings(&mut self, timings: MeshPrepareTimings) {
		self.material_elapsed += timings.material;
		self.dynamic_morph_elapsed += timings.dynamic_morphs;
		self.expand_elapsed += timings.expand;
		self.skinning_elapsed += timings.skinning;
		self.skin_palette_elapsed += timings.skin_palette;
		self.buffer_upload_elapsed += timings.buffer_upload;
		self.material_bind_elapsed += timings.material_bind;
		self.morph_resource_elapsed += timings.morph_resources;
		self.fur_resource_elapsed += timings.fur_resources;
		self.draw_push_elapsed += timings.draw_push;
	}

	fn log(&self, total: Duration) {
		let total_ms = total.as_secs_f64() * 1000.0;
		if total_ms < 50.0 && self.prepared_primitives == 0 {
			return;
		}
		eprintln!(
			"un-avatar-renderer: gpu scene mesh prepare summary: total={total_ms:.1}ms prepared={} resident={} deferred={} skipped_invisible={} skipped_empty={} vertices={} indices={} resident_bytes={} deferred_bytes={} mesh_cloth_assist_vertices={} cache_hits={} cache_misses={} uncacheable={} material={:.1}ms dynamic_morphs={:.1}ms expand={:.1}ms skinning={:.1}ms skin_palette={:.1}ms buffers={:.1}ms material_bind={:.1}ms morph_resources={:.1}ms fur_resources={:.1}ms draw_push={:.1}ms",
			self.prepared_primitives,
			self.resident_primitives,
			self.deferred_primitives,
			self.skipped_invisible_primitives,
			self.skipped_empty_primitives,
			self.vertices,
			self.indices,
			self.resident_vertex_bytes + self.resident_index_bytes,
			self.deferred_vertex_bytes + self.deferred_index_bytes,
			self.mesh_cloth_assist_vertices,
			self.expanded_cache_hits,
			self.expanded_cache_misses,
			self.expanded_uncacheable,
			self.material_elapsed.as_secs_f64() * 1000.0,
			self.dynamic_morph_elapsed.as_secs_f64() * 1000.0,
			self.expand_elapsed.as_secs_f64() * 1000.0,
			self.skinning_elapsed.as_secs_f64() * 1000.0,
			self.skin_palette_elapsed.as_secs_f64() * 1000.0,
			self.buffer_upload_elapsed.as_secs_f64() * 1000.0,
			self.material_bind_elapsed.as_secs_f64() * 1000.0,
			self.morph_resource_elapsed.as_secs_f64() * 1000.0,
			self.fur_resource_elapsed.as_secs_f64() * 1000.0,
			self.draw_push_elapsed.as_secs_f64() * 1000.0,
		);
	}
}

struct MeshPrepareTimings {
	material: Duration,
	dynamic_morphs: Duration,
	expand: Duration,
	skinning: Duration,
	skin_palette: Duration,
	buffer_upload: Duration,
	material_bind: Duration,
	morph_resources: Duration,
	fur_resources: Duration,
	draw_push: Duration,
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

fn scene_texture_upload_step_count(
	scene: &UnaSceneSnapshot,
	texture_roles: &[TextureRole],
	texture_max_dimension: Option<u32>,
	active_texture_indices: Option<&[usize]>,
) -> u32 {
	scene
		.images
		.iter()
		.enumerate()
		.map(|(image_index, im)| {
			if active_texture_indices.is_some_and(|indices| indices.binary_search(&image_index).is_err()) {
				return 1;
			}
			let role = texture_roles.get(image_index).copied().unwrap_or_default();
			estimated_processed_mip_count(im.width, im.height, texture_max_dimension, role)
		})
		.sum()
}

#[derive(Clone, Debug, Default)]
struct SceneAssetResidencySets {
	all_resident: bool,
	owned_mesh_primitives: Vec<(usize, usize)>,
	resident_mesh_primitives: Vec<(usize, usize)>,
	owned_materials: Vec<usize>,
	resident_materials: Vec<usize>,
	owned_images: Vec<usize>,
	resident_images: Vec<usize>,
}

impl SceneAssetResidencySets {
	fn for_scene(scene: &UnaSceneSnapshot, active_asset_groups: &[String]) -> Self {
		if scene.asset_group_ownership.is_empty() || active_asset_groups.is_empty() {
			return Self {
				all_resident: true,
				..Default::default()
			};
		}
		let mut sets = Self::default();
		for group in &scene.asset_group_ownership {
			sets.owned_mesh_primitives.extend(
				group
					.mesh_primitives
					.iter()
					.map(|primitive| (primitive.mesh_index, primitive.primitive_index)),
			);
			sets.owned_materials.extend(group.materials.iter().copied());
			sets.owned_images.extend(group.images.iter().copied());
		}
		let selection = scene.scoped_asset_selection(active_asset_groups);
		sets.resident_mesh_primitives.extend(
			selection
				.mesh_primitives
				.iter()
				.map(|primitive| (primitive.mesh_index, primitive.primitive_index)),
		);
		sets.resident_materials.extend(selection.materials);
		sets.resident_images.extend(selection.images);
		sets.owned_mesh_primitives = sorted_unique(sets.owned_mesh_primitives);
		sets.resident_mesh_primitives = sorted_unique(sets.resident_mesh_primitives);
		sets.owned_materials = sorted_unique_indices(sets.owned_materials);
		sets.resident_materials = sorted_unique_indices(sets.resident_materials);
		sets.owned_images = sorted_unique_indices(sets.owned_images);
		sets.resident_images = sorted_unique_indices(sets.resident_images);
		sets
	}

	fn mesh_primitive_resident(&self, mesh_index: usize, primitive_index: usize) -> bool {
		self.all_resident
			|| self.owned_mesh_primitives.binary_search(&(mesh_index, primitive_index)).is_err()
			|| self.resident_mesh_primitives.binary_search(&(mesh_index, primitive_index)).is_ok()
	}

	fn material_resident(&self, material_index: usize) -> bool {
		self.all_resident
			|| self.owned_materials.binary_search(&material_index).is_err()
			|| self.resident_materials.binary_search(&material_index).is_ok()
	}

	fn image_resident(&self, image_index: usize) -> bool {
		self.all_resident
			|| self.owned_images.binary_search(&image_index).is_err()
			|| self.resident_images.binary_search(&image_index).is_ok()
	}
}

#[derive(Clone)]
struct ExpandedPrimitive {
	verts: Vec<Vertex>,
	indices: Vec<u32>,
	morph_pos: Vec<Vec<[f32; 3]>>,
	morph_nrm: Option<Vec<Vec<[f32; 3]>>>,
	morph_source_indices: Vec<usize>,
	default_morph_weights: Vec<f32>,
}

#[derive(Clone)]
struct ExpandedMorphPayload {
	morph_pos: Box<[Vec<[f32; 3]>]>,
	morph_nrm: Option<Box<[Vec<[f32; 3]>]>>,
	morph_source_indices: Box<[usize]>,
	default_morph_weights: Box<[f32]>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ExpandedPrimitiveCacheKey {
	vertex_payload_id: u64,
	dynamic_morph_targets: Box<[usize]>,
}

#[derive(Clone, Debug)]
struct ExpressionBinding {
	preset_index: usize,
	morph_target_index: usize,
	weight_scale: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum DrawPipelineKind {
	OpaqueLit,
	OpaqueUnlit,
	OpaqueToon,
	BlendLit,
	BlendUnlit,
	BlendToon,
	BlendToonZWrite,
	BlendToonAdd,
	BlendToonAddZWrite,
	TransparentToonBackpass,
	TransparentToonBackpassNoZWrite,
	LilToonGemPre,
}

impl DrawPipelineKind {
	fn label(self) -> &'static str {
		match self {
			DrawPipelineKind::OpaqueLit => "mesh_opaque_lit",
			DrawPipelineKind::OpaqueUnlit => "mesh_opaque_unlit",
			DrawPipelineKind::OpaqueToon => "mesh_opaque_toon",
			DrawPipelineKind::BlendLit => "mesh_blend_lit",
			DrawPipelineKind::BlendUnlit => "mesh_blend_unlit",
			DrawPipelineKind::BlendToon => "mesh_blend_toon",
			DrawPipelineKind::BlendToonZWrite => "mesh_blend_toon_zwrite",
			DrawPipelineKind::BlendToonAdd => "mesh_blend_toon_add",
			DrawPipelineKind::BlendToonAddZWrite => "mesh_blend_toon_add_zwrite",
			DrawPipelineKind::TransparentToonBackpass => "mesh_transparent_toon_backpass",
			DrawPipelineKind::TransparentToonBackpassNoZWrite => "mesh_transparent_toon_backpass_no_zwrite",
			DrawPipelineKind::LilToonGemPre => "mesh_liltoon_gem_pre_toon",
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct DrawPipelineKey {
	kind: DrawPipelineKind,
	stencil: MaterialStencilState,
	color_mask: u8,
}

impl DrawPipelineKey {
	fn new(kind: DrawPipelineKind, draw: &MeshDraw, opts: &SceneMeshLoadOpts) -> Self {
		Self {
			kind,
			stencil: draw.stencil_state,
			color_mask: if opts.force_simple_basecolor || opts.debug_primitive_colors {
				15
			} else {
				draw.color_mask
			},
		}
	}

	fn from_parts(kind: DrawPipelineKind, stencil: MaterialStencilState, color_mask: u8) -> Self {
		Self {
			kind,
			stencil,
			color_mask: color_mask & 0x0f,
		}
	}

	fn label(self) -> &'static str {
		self.kind.label()
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct MaterialRenderStateKey {
	stencil: MaterialStencilState,
	color_mask: u8,
}

impl MaterialRenderStateKey {
	fn new(stencil: MaterialStencilState, color_mask: u8) -> Self {
		Self {
			stencil,
			color_mask: color_mask & 0x0f,
		}
	}

	fn from_draw_outline(draw: &MeshDraw, opts: &SceneMeshLoadOpts) -> Self {
		Self::new(
			draw.outline_stencil_state,
			if opts.force_simple_basecolor || opts.debug_primitive_colors {
				15
			} else {
				draw.outline_color_mask
			},
		)
	}

	fn from_draw_fur(draw: &MeshDraw, opts: &SceneMeshLoadOpts) -> Self {
		Self::new(
			draw.fur_stencil_state,
			if opts.force_simple_basecolor || opts.debug_primitive_colors {
				15
			} else {
				draw.fur_color_mask
			},
		)
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct MaterialStencilState {
	reference: u8,
	read_mask: u8,
	write_mask: u8,
	compare: u8,
	pass_op: u8,
	fail_op: u8,
	depth_fail_op: u8,
}

impl Default for MaterialStencilState {
	fn default() -> Self {
		Self {
			reference: 0,
			read_mask: 255,
			write_mask: 255,
			compare: 8,
			pass_op: 0,
			fail_op: 0,
			depth_fail_op: 0,
		}
	}
}

impl MaterialStencilState {
	fn to_wgpu(self) -> wgpu::StencilState {
		let face = wgpu::StencilFaceState {
			compare: unity_compare_function(self.compare),
			fail_op: unity_stencil_operation(self.fail_op),
			depth_fail_op: unity_stencil_operation(self.depth_fail_op),
			pass_op: unity_stencil_operation(self.pass_op),
		};
		wgpu::StencilState {
			front: face,
			back: face,
			read_mask: self.read_mask as u32,
			write_mask: self.write_mask as u32,
		}
	}
}

fn unity_compare_function(value: u8) -> wgpu::CompareFunction {
	match value {
		1 => wgpu::CompareFunction::Never,
		2 => wgpu::CompareFunction::Less,
		3 => wgpu::CompareFunction::Equal,
		4 => wgpu::CompareFunction::LessEqual,
		5 => wgpu::CompareFunction::Greater,
		6 => wgpu::CompareFunction::NotEqual,
		7 => wgpu::CompareFunction::GreaterEqual,
		_ => wgpu::CompareFunction::Always,
	}
}

fn unity_stencil_operation(value: u8) -> wgpu::StencilOperation {
	match value {
		1 => wgpu::StencilOperation::Zero,
		2 => wgpu::StencilOperation::Replace,
		3 => wgpu::StencilOperation::IncrementClamp,
		4 => wgpu::StencilOperation::DecrementClamp,
		5 => wgpu::StencilOperation::Invert,
		6 => wgpu::StencilOperation::IncrementWrap,
		7 => wgpu::StencilOperation::DecrementWrap,
		_ => wgpu::StencilOperation::Keep,
	}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct UntoonShaderFeatures {
	profile_extensions: bool,
	main_layers: bool,
	alpha_mask: bool,
	dissolve: bool,
	parallax: bool,
	id_mask: bool,
	udim_discard: bool,
	audio_link: bool,
	shadow_layers: bool,
	matcap: bool,
	matcap_second: bool,
	matcap_custom_normal: bool,
	reflection: bool,
	reflection_cube: bool,
	anisotropy: bool,
	rim: bool,
	rim_shade: bool,
	backlight: bool,
	glitter: bool,
	emission: bool,
	emission_second: bool,
	distance_fade: bool,
	fur: bool,
	gem: bool,
	refraction: bool,
	normal_second: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum UntoonSourceProfile {
	#[default]
	Plain,
	MToon,
	LilToon,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct UntoonFeaturePlan {
	source_profile: UntoonSourceProfile,
	shader_features: UntoonShaderFeatures,
}

impl UntoonFeaturePlan {
	fn none() -> Self {
		Self::default()
	}
}

impl UntoonShaderFeatures {
	fn include(&mut self, other: Self) {
		self.profile_extensions |= other.profile_extensions;
		self.main_layers |= other.main_layers;
		self.alpha_mask |= other.alpha_mask;
		self.dissolve |= other.dissolve;
		self.parallax |= other.parallax;
		self.id_mask |= other.id_mask;
		self.udim_discard |= other.udim_discard;
		self.audio_link |= other.audio_link;
		self.shadow_layers |= other.shadow_layers;
		self.matcap |= other.matcap;
		self.matcap_second |= other.matcap_second;
		self.matcap_custom_normal |= other.matcap_custom_normal;
		self.reflection |= other.reflection;
		self.reflection_cube |= other.reflection_cube;
		self.anisotropy |= other.anisotropy;
		self.rim |= other.rim;
		self.rim_shade |= other.rim_shade;
		self.backlight |= other.backlight;
		self.glitter |= other.glitter;
		self.emission |= other.emission;
		self.emission_second |= other.emission_second;
		self.distance_fade |= other.distance_fade;
		self.fur |= other.fur;
		self.gem |= other.gem;
		self.refraction |= other.refraction;
		self.normal_second |= other.normal_second;
	}

	fn shader_feature_values(self) -> [(&'static str, bool); 26] {
		[
			("UNTOON_FEATURE_PROFILE_EXTENSIONS", self.profile_extensions),
			("UNTOON_FEATURE_MAIN_LAYERS", self.main_layers),
			("UNTOON_FEATURE_ALPHA_MASK", self.alpha_mask),
			("UNTOON_FEATURE_DISSOLVE", self.dissolve),
			("UNTOON_FEATURE_PARALLAX", self.parallax),
			("UNTOON_FEATURE_ID_MASK", self.id_mask),
			("UNTOON_FEATURE_UDIM_DISCARD", self.udim_discard),
			("UNTOON_FEATURE_AUDIO_LINK", self.audio_link),
			("UNTOON_FEATURE_SHADOW_LAYERS", self.shadow_layers),
			("UNTOON_FEATURE_MATCAP", self.matcap),
			("UNTOON_FEATURE_MATCAP_SECOND", self.matcap_second),
			("UNTOON_FEATURE_MATCAP_CUSTOM_NORMAL", self.matcap_custom_normal),
			("UNTOON_FEATURE_REFLECTION", self.reflection),
			("UNTOON_FEATURE_REFLECTION_CUBE", self.reflection_cube),
			("UNTOON_FEATURE_ANISOTROPY", self.anisotropy),
			("UNTOON_FEATURE_RIM", self.rim),
			("UNTOON_FEATURE_RIM_SHADE", self.rim_shade),
			("UNTOON_FEATURE_BACKLIGHT", self.backlight),
			("UNTOON_FEATURE_GLITTER", self.glitter),
			("UNTOON_FEATURE_EMISSION", self.emission),
			("UNTOON_FEATURE_EMISSION_SECOND", self.emission_second),
			("UNTOON_FEATURE_DISTANCE_FADE", self.distance_fade),
			("UNTOON_FEATURE_FUR", self.fur),
			("UNTOON_FEATURE_GEM", self.gem),
			("UNTOON_FEATURE_REFRACTION", self.refraction),
			("UNTOON_FEATURE_NORMAL_SECOND", self.normal_second),
		]
	}
}

fn full_liltoon_prewarm_features() -> UntoonShaderFeatures {
	UntoonShaderFeatures {
		profile_extensions: true,
		main_layers: true,
		alpha_mask: true,
		dissolve: true,
		parallax: true,
		id_mask: true,
		udim_discard: true,
		audio_link: true,
		shadow_layers: true,
		matcap: true,
		matcap_second: true,
		matcap_custom_normal: true,
		reflection: true,
		reflection_cube: true,
		anisotropy: true,
		rim: true,
		rim_shade: true,
		backlight: true,
		glitter: true,
		emission: true,
		emission_second: true,
		distance_fade: true,
		fur: true,
		gem: true,
		refraction: true,
		normal_second: true,
	}
}

fn mtoon_prewarm_features() -> UntoonShaderFeatures {
	UntoonShaderFeatures {
		shadow_layers: true,
		matcap: true,
		reflection: true,
		reflection_cube: true,
		rim: true,
		emission: true,
		..Default::default()
	}
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MeshPipelinePrewarmSummary {
	pub shader_modules: usize,
	pub render_pipelines: usize,
	pub compute_pipelines: usize,
}

#[derive(Clone, Debug)]
struct DrawBatch {
	pipeline: DrawPipelineKey,
	draw_indices: Vec<usize>,
}

fn draw_batch(pipeline: DrawPipelineKey, capacity: usize) -> DrawBatch {
	DrawBatch {
		pipeline,
		draw_indices: Vec::with_capacity(capacity),
	}
}

fn append_ordered_draw_batch(batches: &mut Vec<DrawBatch>, pipeline: DrawPipelineKey, draw_index: usize, batch_capacity: usize) {
	if let Some(last) = batches.last_mut() {
		if last.pipeline == pipeline {
			last.draw_indices.push(draw_index);
			return;
		}
	}
	let mut batch = draw_batch(pipeline, batch_capacity);
	batch.draw_indices.push(draw_index);
	batches.push(batch);
}

fn finalize_draw_batches(batches: &mut Vec<DrawBatch>) {
	batches.retain(|batch| !batch.draw_indices.is_empty());
	for batch in batches.iter_mut() {
		batch.draw_indices.shrink_to_fit();
	}
	batches.shrink_to_fit();
}

fn transparent_backpass_pipeline_for_draw(draw: &MeshDraw) -> DrawPipelineKind {
	let zwrite = draw
		.material
		.liltoon_like_runtime()
		.is_none_or(|u| u.blend_state.pre_zwrite_factor > 0.5);
	if zwrite {
		DrawPipelineKind::TransparentToonBackpass
	} else {
		DrawPipelineKind::TransparentToonBackpassNoZWrite
	}
}

fn json_number_f32(value: &serde_json::Value) -> Option<f32> {
	value
		.as_f64()
		.map(|value| value as f32)
		.or_else(|| value.as_i64().map(|value| value as f32))
}

fn material_source_float_param(material: &UnaMaterialPbr, name: &str) -> Option<f32> {
	let source = material.unavatar_material.as_ref()?;
	source
		.get("floatParams")
		.and_then(|params| params.get(name))
		.and_then(json_number_f32)
		.or_else(|| {
			source
				.get("floatProperties")
				.and_then(|params| params.get(name))
				.and_then(json_number_f32)
		})
}

fn material_source_u8_param(material: &UnaMaterialPbr, name: &str, default_value: u8) -> u8 {
	material_source_float_param(material, name)
		.map(|value| value.round().clamp(0.0, 255.0) as u8)
		.unwrap_or(default_value)
}

fn prefixed_material_name(prefix: &str, suffix: &str) -> String {
	if prefix.is_empty() {
		format!("_{suffix}")
	} else {
		format!("_{prefix}{suffix}")
	}
}

fn material_source_u8_param_prefixed(material: &UnaMaterialPbr, prefix: &str, suffix: &str, default_value: u8) -> u8 {
	material_source_u8_param(material, &prefixed_material_name(prefix, suffix), default_value)
}

fn material_stencil_state_prefixed(material: &UnaMaterialPbr, prefix: &str) -> MaterialStencilState {
	MaterialStencilState {
		reference: material_source_u8_param_prefixed(material, prefix, "StencilRef", 0),
		read_mask: material_source_u8_param_prefixed(material, prefix, "StencilReadMask", 255),
		write_mask: material_source_u8_param_prefixed(material, prefix, "StencilWriteMask", 255),
		compare: material_source_u8_param_prefixed(material, prefix, "StencilComp", 8),
		pass_op: material_source_u8_param_prefixed(material, prefix, "StencilPass", 0),
		fail_op: material_source_u8_param_prefixed(material, prefix, "StencilFail", 0),
		depth_fail_op: material_source_u8_param_prefixed(material, prefix, "StencilZFail", 0),
	}
}

fn material_stencil_state(material: &UnaMaterialPbr) -> MaterialStencilState {
	material_stencil_state_prefixed(material, "")
}

fn material_outline_stencil_state(material: &UnaMaterialPbr) -> MaterialStencilState {
	material_stencil_state_prefixed(material, "Outline")
}

fn material_fur_stencil_state(material: &UnaMaterialPbr) -> MaterialStencilState {
	material_stencil_state_prefixed(material, "Fur")
}

fn material_color_mask(material: &UnaMaterialPbr) -> u8 {
	material_source_u8_param(material, "_ColorMask", 15) & 0x0f
}

fn material_color_mask_prefixed(material: &UnaMaterialPbr, prefix: &str) -> u8 {
	material_source_u8_param_prefixed(material, prefix, "ColorMask", 15) & 0x0f
}

fn material_outline_color_mask(material: &UnaMaterialPbr) -> u8 {
	material_color_mask_prefixed(material, "Outline")
}

fn material_fur_color_mask(material: &UnaMaterialPbr) -> u8 {
	material_color_mask_prefixed(material, "Fur")
}

fn color_writes_from_unity_mask(mask: u8) -> wgpu::ColorWrites {
	let mut writes = wgpu::ColorWrites::empty();
	if mask & 0x1 != 0 {
		writes |= wgpu::ColorWrites::ALPHA;
	}
	if mask & 0x2 != 0 {
		writes |= wgpu::ColorWrites::BLUE;
	}
	if mask & 0x4 != 0 {
		writes |= wgpu::ColorWrites::GREEN;
	}
	if mask & 0x8 != 0 {
		writes |= wgpu::ColorWrites::RED;
	}
	writes
}

fn material_source_shader_name(material: &UnaMaterialPbr) -> Option<&str> {
	material
		.unavatar_material
		.as_ref()
		.and_then(|source| {
			source
				.get("sourceShader")
				.or_else(|| source.get("shaderName"))
				.or_else(|| source.get("shader"))
		})
		.and_then(|value| value.as_str())
}

fn material_transparent_with_zwrite(material: &UnaMaterialPbr) -> bool {
	if material.liltoon_like_runtime().is_some() {
		if let Some(value) =
			material_source_float_param(material, "_ZWrite").or_else(|| material_source_float_param(material, "_ZWriteMode"))
		{
			return value > 0.5;
		}
		return material_source_shader_name(material).is_some_and(|shader| shader.to_ascii_lowercase().contains("twopass"));
	}
	material.mtoon_like_runtime().is_some_and(|mtoon| mtoon.transparent_with_z_write)
}

fn push_unique_index(indices: &mut Vec<usize>, index: usize) {
	if !indices.contains(&index) {
		indices.push(index);
	}
}

fn push_texture_index(indices: &mut Vec<usize>, index: Option<usize>) {
	if let Some(index) = index {
		push_unique_index(indices, index);
	}
}

fn sorted_unique<T: Ord>(mut values: Vec<T>) -> Vec<T> {
	values.sort_unstable();
	values.dedup();
	values
}

fn sorted_unique_indices(indices: Vec<usize>) -> Vec<usize> {
	sorted_unique(indices)
}

fn lil_enabled(value: f32) -> bool {
	liltoon_features::enabled(value)
}

fn material_texture_indices(material: &UnaMaterialPbr) -> Vec<usize> {
	let mut indices = Vec::new();
	push_texture_index(&mut indices, material.base_color_texture_index);
	push_texture_index(&mut indices, material.normal_texture_index);
	push_texture_index(&mut indices, material.occlusion_texture_index);
	push_texture_index(&mut indices, material.emissive_texture_index);
	if let Some(mtoon) = material.mtoon_like_runtime() {
		push_texture_index(&mut indices, mtoon.shade_multiply_texture_index);
		push_texture_index(&mut indices, mtoon.shading_shift_texture_index);
		push_texture_index(&mut indices, mtoon.matcap_texture_index);
		push_texture_index(&mut indices, mtoon.rim_multiply_texture_index);
		push_texture_index(&mut indices, mtoon.outline_width_multiply_texture_index);
		push_texture_index(&mut indices, mtoon.uv_animation_mask_texture_index);
	}
	if let Some(liltoon) = material.liltoon_like_runtime() {
		if liltoon_features::uses_main_color_adjustment(&liltoon.main_color) {
			push_texture_index(&mut indices, liltoon.main_color.main_color_adjust_mask_texture_index);
		}
		if lil_enabled(liltoon.main_color.gradation_enabled_factor) {
			push_texture_index(&mut indices, liltoon.main_color.gradation_texture_index);
		}
		if lil_enabled(liltoon.main_color.second_enabled_factor) {
			push_texture_index(&mut indices, liltoon.main_color.second_texture_index);
			push_texture_index(&mut indices, liltoon.main_color.second_blend_mask_texture_index);
			push_texture_index(&mut indices, liltoon.main_color.second_dissolve.mask_texture_index);
			push_texture_index(&mut indices, liltoon.main_color.second_dissolve.noise_mask_texture_index);
		}
		if lil_enabled(liltoon.main_color.third_enabled_factor) {
			push_texture_index(&mut indices, liltoon.main_color.third_texture_index);
			push_texture_index(&mut indices, liltoon.main_color.third_blend_mask_texture_index);
			push_texture_index(&mut indices, liltoon.main_color.third_dissolve.mask_texture_index);
			push_texture_index(&mut indices, liltoon.main_color.third_dissolve.noise_mask_texture_index);
		}
		if lil_enabled(liltoon.shadow.enabled_factor) {
			push_texture_index(&mut indices, liltoon.shadow.color_texture_index);
			push_texture_index(&mut indices, liltoon.shadow.strength_mask_texture_index);
			push_texture_index(&mut indices, liltoon.shadow.border_mask_texture_index);
			push_texture_index(&mut indices, liltoon.shadow.blur_mask_texture_index);
			push_texture_index(&mut indices, liltoon.shadow.second_color_texture_index);
			push_texture_index(&mut indices, liltoon.shadow.third_color_texture_index);
		}
		if lil_enabled(liltoon.normal.second_enabled_factor) {
			push_texture_index(&mut indices, liltoon.normal.second_texture_index);
			push_texture_index(&mut indices, liltoon.normal.second_scale_mask_texture_index);
		}
		if lil_enabled(liltoon.matcap.enabled_factor) {
			push_texture_index(&mut indices, liltoon.matcap.texture_index);
			push_texture_index(&mut indices, liltoon.matcap.blend_mask_texture_index);
			push_texture_index(&mut indices, liltoon.matcap.bump_texture_index);
		}
		if lil_enabled(liltoon.matcap.second_enabled_factor) {
			push_texture_index(&mut indices, liltoon.matcap.second_texture_index);
			push_texture_index(&mut indices, liltoon.matcap.second_blend_mask_texture_index);
			push_texture_index(&mut indices, liltoon.matcap.second_bump_texture_index);
		}
		if lil_enabled(liltoon.reflection.enabled_factor) {
			push_texture_index(&mut indices, liltoon.reflection.metallic_texture_index);
			push_texture_index(&mut indices, liltoon.reflection.color_texture_index);
			push_texture_index(&mut indices, liltoon.reflection.smoothness_texture_index);
		}
		if lil_enabled(liltoon.reflection.anisotropy_enabled_factor) {
			push_texture_index(&mut indices, liltoon.reflection.anisotropy_tangent_texture_index);
			push_texture_index(&mut indices, liltoon.reflection.anisotropy_scale_mask_texture_index);
			push_texture_index(&mut indices, liltoon.reflection.anisotropy_shift_noise_mask_texture_index);
		}
		if lil_enabled(liltoon.rim.enabled_factor) {
			push_texture_index(&mut indices, liltoon.rim.texture_index);
		}
		if lil_enabled(liltoon.rim.shade_enabled_factor) {
			push_texture_index(&mut indices, liltoon.rim.shade_mask_texture_index);
		}
		if lil_enabled(liltoon.emission.enabled_factor) {
			push_texture_index(&mut indices, liltoon.emission.texture_index);
			push_texture_index(&mut indices, liltoon.emission.blend_mask_texture_index);
			if lil_enabled(liltoon.emission.gradation_enabled_factor) {
				push_texture_index(&mut indices, liltoon.emission.gradation_texture_index);
			}
		}
		if lil_enabled(liltoon.emission.second_enabled_factor) {
			push_texture_index(&mut indices, liltoon.emission.second_texture_index);
			push_texture_index(&mut indices, liltoon.emission.second_blend_mask_texture_index);
			if lil_enabled(liltoon.emission.second_gradation_enabled_factor) {
				push_texture_index(&mut indices, liltoon.emission.second_gradation_texture_index);
			}
		}
		if liltoon.alpha_mask.mode_factor > 0.5 {
			push_texture_index(&mut indices, liltoon.alpha_mask.texture_index);
		}
		if lil_enabled(liltoon.audio_link.enabled_factor) {
			push_texture_index(&mut indices, liltoon.audio_link.mask_texture_index);
			push_texture_index(&mut indices, liltoon.audio_link.local_map_texture_index);
		}
		if lil_enabled(liltoon.outline.enabled_factor) {
			push_texture_index(&mut indices, liltoon.outline.texture_index);
			push_texture_index(&mut indices, liltoon.outline.width_mask_texture_index);
		}
		if lil_enabled(liltoon.backlight.enabled_factor) {
			push_texture_index(&mut indices, liltoon.backlight.texture_index);
		}
		if lil_enabled(liltoon.glitter.enabled_factor) {
			push_texture_index(&mut indices, liltoon.glitter.color_texture_index);
			push_texture_index(&mut indices, liltoon.glitter.shape_texture_index);
		}
		if liltoon.dissolve.params_factor[0] > 0.5 {
			push_texture_index(&mut indices, liltoon.dissolve.mask_texture_index);
			push_texture_index(&mut indices, liltoon.dissolve.noise_mask_texture_index);
		}
		if lil_enabled(liltoon.parallax.enabled_factor) {
			push_texture_index(&mut indices, liltoon.parallax.texture_index);
		}
		if lil_enabled(liltoon.fur.enabled_factor) {
			push_texture_index(&mut indices, liltoon.fur.vector_texture_index);
			push_texture_index(&mut indices, liltoon.fur.length_mask_texture_index);
			push_texture_index(&mut indices, liltoon.fur.noise_mask_texture_index);
			push_texture_index(&mut indices, liltoon.fur.mask_texture_index);
		}
	}
	sorted_unique_indices(indices)
}

fn material_cube_texture_indices(material: &UnaMaterialPbr) -> Vec<usize> {
	let mut indices = Vec::new();
	if let Some(mtoon) = material.mtoon_like_runtime() {
		push_texture_index(&mut indices, mtoon.reflection_cube_texture_index);
	}
	if let Some(liltoon) = material.liltoon_like_runtime() {
		push_texture_index(&mut indices, liltoon_reflection_texture_index(liltoon));
	}
	sorted_unique_indices(indices)
}

fn material_resident_texture_indices(material: &UnaMaterialPbr) -> Vec<usize> {
	let mut indices = material_texture_indices(material);
	indices.extend(material_cube_texture_indices(material));
	sorted_unique_indices(indices)
}

fn initial_active_texture_indices_for_scene(
	scene: &UnaSceneSnapshot,
	effective_visibility: &[bool],
	asset_residency: &SceneAssetResidencySets,
	opts: &SceneMeshLoadOpts,
) -> Vec<usize> {
	let default_material = UnaMaterialPbr::default();
	let mut indices = Vec::new();
	for (node_index, node) in scene.nodes.iter().enumerate() {
		if !effective_visibility.get(node_index).copied().unwrap_or(false) {
			continue;
		}
		let Some(mesh_index) = node.mesh else { continue };
		let Some(mesh_primitives) = scene.meshes.get(mesh_index) else {
			continue;
		};
		for (primitive_index, primitive) in mesh_primitives.iter().enumerate() {
			if !asset_residency.mesh_primitive_resident(mesh_index, primitive_index) {
				continue;
			}
			let material = primitive
				.material_index
				.and_then(|material_index| scene.materials.get(material_index))
				.unwrap_or(&default_material);
			if material_is_fully_invisible_for_draw(material, opts) {
				continue;
			}
			indices.extend(material_resident_texture_indices(material));
		}
	}
	sorted_unique_indices(indices)
}

fn initial_active_2d_texture_indices_for_scene(
	scene: &UnaSceneSnapshot,
	effective_visibility: &[bool],
	asset_residency: &SceneAssetResidencySets,
	opts: &SceneMeshLoadOpts,
) -> Vec<usize> {
	let default_material = UnaMaterialPbr::default();
	let mut indices = Vec::new();
	for (node_index, node) in scene.nodes.iter().enumerate() {
		if !effective_visibility.get(node_index).copied().unwrap_or(false) {
			continue;
		}
		let Some(mesh_index) = node.mesh else { continue };
		let Some(mesh_primitives) = scene.meshes.get(mesh_index) else {
			continue;
		};
		for (primitive_index, primitive) in mesh_primitives.iter().enumerate() {
			if !asset_residency.mesh_primitive_resident(mesh_index, primitive_index) {
				continue;
			}
			let material = primitive
				.material_index
				.and_then(|material_index| scene.materials.get(material_index))
				.unwrap_or(&default_material);
			if material_is_fully_invisible_for_draw(material, opts) {
				continue;
			}
			indices.extend(material_texture_indices(material));
		}
	}
	sorted_unique_indices(indices)
}

fn blended_pipeline_pass_order(pipeline: DrawPipelineKind) -> u8 {
	match pipeline {
		DrawPipelineKind::TransparentToonBackpass | DrawPipelineKind::TransparentToonBackpassNoZWrite => 0,
		DrawPipelineKind::LilToonGemPre => 1,
		_ => 2,
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
	inverse_bind_matrices: Box<[Mat4]>,
	raw: Vec<f32>,
	uploaded: Vec<f32>,
	uploaded_changed: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct DrawTransformUpdateTimings {
	pub skin_palette_ms: f32,
	pub skin_palette_write_ms: f32,
	pub fur_source_vertices_ms: f32,
	pub expression_values_ms: f32,
	pub morph_weights_ms: f32,
	pub draw_transform_ms: f32,
}

enum SceneMeshIndexUpload {
	U16(Box<[u16]>),
	U32(Box<[u32]>),
}

impl SceneMeshIndexUpload {
	fn from_indices(index_format: wgpu::IndexFormat, indices: Vec<u32>) -> Self {
		match index_format {
			wgpu::IndexFormat::Uint16 => Self::U16(indices.into_iter().map(|index| index as u16).collect::<Vec<_>>().into_boxed_slice()),
			wgpu::IndexFormat::Uint32 => Self::U32(indices.into_boxed_slice()),
		}
	}

	fn len(&self) -> usize {
		match self {
			Self::U16(indices) => indices.len(),
			Self::U32(indices) => indices.len(),
		}
	}

	fn buffer_bytes(&self) -> u64 {
		match self {
			Self::U16(indices) => (indices.len() * std::mem::size_of::<u16>()) as u64,
			Self::U32(indices) => (indices.len() * std::mem::size_of::<u32>()) as u64,
		}
	}

	fn create_buffer(&self, device: &wgpu::Device) -> wgpu::Buffer {
		match self {
			Self::U16(indices) => device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
				label: Some("mesh_i_u16"),
				contents: bytemuck::cast_slice(indices),
				usage: wgpu::BufferUsages::INDEX,
			}),
			Self::U32(indices) => device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
				label: Some("mesh_i_u32"),
				contents: bytemuck::cast_slice(indices),
				usage: wgpu::BufferUsages::INDEX,
			}),
		}
	}

	fn source_triangles(&self, vertex_count: usize) -> Vec<ComputeFurCardsSourceTriangleGpu> {
		match self {
			Self::U16(indices) => compute_fur_cards_source_triangles_from_indices_u16(indices, vertex_count),
			Self::U32(indices) => compute_fur_cards_source_triangles_from_indices(indices, vertex_count),
		}
	}
}

struct SceneMeshBufferUpload {
	vertices: Box<[Vertex]>,
	indices: SceneMeshIndexUpload,
}

impl SceneMeshBufferUpload {
	fn vertex_buffer_bytes(&self) -> u64 {
		(self.vertices.len() * std::mem::size_of::<Vertex>()) as u64
	}

	fn index_buffer_bytes(&self) -> u64 {
		self.indices.buffer_bytes()
	}

	fn create_buffers(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> (wgpu::Buffer, wgpu::Buffer) {
		let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("mesh_v"),
			size: self.vertex_buffer_bytes(),
			usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		queue.write_buffer(&vertex_buffer, 0, bytemuck::cast_slice(&self.vertices));
		let index_buffer = self.indices.create_buffer(device);
		(vertex_buffer, index_buffer)
	}
}

struct MeshDraw {
	vertex_buffer: Option<wgpu::Buffer>,
	index_buffer: Option<wgpu::Buffer>,
	vertex_buffer_bytes: u64,
	index_buffer_bytes: u64,
	buffer_upload: SceneMeshBufferUpload,
	index_format: wgpu::IndexFormat,
	index_count: u32,
	draw_transform: wgpu::Buffer,
	draw_transform_uploaded: Option<MeshDrawTransformGpu>,
	draw_material: wgpu::Buffer,
	bind_material: Option<wgpu::BindGroup>,
	bind_outline_material: Option<wgpu::BindGroup>,
	skin_palette_index: usize,
	skin_palette_static_identity: bool,
	morph_resources: Option<MorphGpuResources>,
	_compute_fur_cards: Option<ComputeFurCardsDrawResources>,
	world_node_index: usize,
	visible: bool,
	asset_resident: bool,
	shading: UnaShadingModel,
	morph_target_count: usize,
	morph_source_indices: Box<[usize]>,
	morph_target_names: Box<[String]>,
	morph_target_override_keys: Box<[String]>,
	morph_target_override_suffix_keys: Box<[Option<String>]>,
	morph_pos: Vec<Vec<[f32; 3]>>,
	morph_nrm: Option<Vec<Vec<[f32; 3]>>>,
	default_morph_weights: Vec<f32>,
	expression_bindings: Box<[ExpressionBinding]>,
	morph_weights: Vec<f32>,
	morph_weight_scratch: Vec<f32>,
	alpha_mode: UnaAlphaMode,
	material_slot_index: Option<usize>,
	material: UnaMaterialPbr,
	stencil_state: MaterialStencilState,
	color_mask: u8,
	outline_stencil_state: MaterialStencilState,
	outline_color_mask: u8,
	fur_stencil_state: MaterialStencilState,
	fur_color_mask: u8,
	texture_indices: Box<[usize]>,
	cube_texture_indices: Box<[usize]>,
	mesh_index: usize,
	primitive_index: usize,
}

struct SceneTextureViews {
	white: wgpu::TextureView,
	black: wgpu::TextureView,
	neutral_normal: wgpu::TextureView,
	transparent_black: wgpu::TextureView,
	blue: wgpu::TextureView,
	neutral_vector: wgpu::TextureView,
	black_cube: wgpu::TextureView,
	images: Vec<wgpu::TextureView>,
	cubes: Vec<Option<wgpu::TextureView>>,
}

struct MeshMaterialBindingSource<'a> {
	material: &'a UnaMaterialPbr,
	draw_transform: &'a wgpu::Buffer,
	draw_material: &'a wgpu::Buffer,
}

impl MeshDraw {
	fn active(&self) -> bool {
		self.visible && self.asset_resident
	}

	fn mesh_buffers_resident(&self) -> bool {
		self.vertex_buffer.is_some() && self.index_buffer.is_some()
	}

	fn ensure_mesh_buffers(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) -> bool {
		if self.mesh_buffers_resident() {
			return false;
		}
		let (vertex_buffer, index_buffer) = self.buffer_upload.create_buffers(device, queue);
		self.vertex_buffer = Some(vertex_buffer);
		self.index_buffer = Some(index_buffer);
		true
	}

	fn drop_mesh_buffers(&mut self) -> bool {
		let had_buffers = self.vertex_buffer.take().is_some() || self.index_buffer.take().is_some();
		had_buffers
	}
}

#[allow(dead_code)]
struct ComputeFurCardsDrawResources {
	params: ComputeFurCardsGenerateParamsGpu,
	params_buffer: wgpu::Buffer,
	source_vertex_buffer: wgpu::Buffer,
	card_source_buffer: wgpu::Buffer,
	generated_vertex_buffer: wgpu::Buffer,
	generated_index_buffer: wgpu::Buffer,
	bind_group: wgpu::BindGroup,
	triangle_count: u32,
	card_count: u32,
	generated_index_count: u32,
	dispatch_workgroups: u32,
}

#[derive(Default)]
struct DrawBindState {
	frame_bound: bool,
	skin_palette_index: Option<usize>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SceneMeshRuntimeRequirements {
	pub(crate) audio_link_texture: bool,
	pub(crate) screen_refraction: bool,
	pub(crate) fur: bool,
	toon_shader_features: UntoonShaderFeatures,
}

impl SceneMeshRuntimeRequirements {
	fn include(&mut self, other: Self) {
		self.audio_link_texture |= other.audio_link_texture;
		self.screen_refraction |= other.screen_refraction;
		self.fur |= other.fur;
		self.toon_shader_features.include(other.toon_shader_features);
	}
}

#[derive(Default)]
struct SceneMeshDrawState {
	outline_draw_indices: Vec<usize>,
	fur_draw_indices: Vec<usize>,
	opaque_batches: Vec<DrawBatch>,
	transparent_backpass_draw_indices: Vec<usize>,
	blended_batches: Vec<DrawBatch>,
	active_draw_indices: Vec<usize>,
	active_morph_draw_indices: Vec<usize>,
	needs_screen_refraction: bool,
	active_skin_palette_indices: Vec<usize>,
	runtime_requirements: SceneMeshRuntimeRequirements,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SceneMeshAssetResidencyCounts {
	pub(crate) total_draw_mesh_primitive_count: usize,
	pub(crate) resident_draw_mesh_primitive_count: usize,
	pub(crate) inactive_draw_mesh_primitive_count: usize,
	pub(crate) total_draw_mesh_buffer_bytes: u64,
	pub(crate) resident_draw_mesh_buffer_bytes: u64,
	pub(crate) inactive_draw_mesh_buffer_bytes: u64,
	pub(crate) total_image_texture_count: usize,
	pub(crate) resident_image_texture_count: usize,
	pub(crate) inactive_image_texture_count: usize,
	pub(crate) draws_using_inactive_image_texture_count: usize,
	pub(crate) active_draws_using_inactive_image_texture_count: usize,
	pub(crate) inactive_image_textures_used_by_active_draw_count: usize,
	pub(crate) inactive_image_textures_used_by_active_draw: Vec<usize>,
	pub(crate) active_draws_using_inactive_cube_texture_count: usize,
	pub(crate) inactive_cube_textures_used_by_active_draw_count: usize,
	pub(crate) inactive_cube_textures_used_by_active_draw: Vec<usize>,
	pub(crate) total_material_slot_count: usize,
	pub(crate) resident_material_slot_count: usize,
	pub(crate) inactive_material_slot_count: usize,
	pub(crate) active_draws_using_inactive_material_slot_count: usize,
	pub(crate) inactive_material_slots_used_by_active_draw_count: usize,
	pub(crate) inactive_material_slots_used_by_active_draw: Vec<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SceneMeshActiveResidencyGaps {
	pub(crate) inactive_image_texture_indices: Vec<usize>,
	pub(crate) inactive_cube_texture_indices: Vec<usize>,
	pub(crate) inactive_material_slot_indices: Vec<usize>,
	pub(crate) active_draws_using_inactive_image_texture_count: usize,
	pub(crate) active_draws_using_inactive_cube_texture_count: usize,
	pub(crate) active_draws_using_inactive_material_slot_count: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SceneMeshAssetResidencyRefresh {
	pub(crate) active_draw_state_changed_count: usize,
	pub(crate) mesh_buffer_load_indices: Vec<usize>,
	pub(crate) mesh_buffer_unload_indices: Vec<usize>,
	pub(crate) image_texture_load_indices: Vec<usize>,
	pub(crate) image_texture_unload_indices: Vec<usize>,
	pub(crate) cube_texture_load_indices: Vec<usize>,
	pub(crate) cube_texture_unload_indices: Vec<usize>,
	pub(crate) material_slot_load_indices: Vec<usize>,
	pub(crate) material_slot_unload_indices: Vec<usize>,
}

impl SceneMeshAssetResidencyRefresh {
	pub(crate) fn has_scoped_resource_changes(&self) -> bool {
		!self.mesh_buffer_load_indices.is_empty()
			|| !self.mesh_buffer_unload_indices.is_empty()
			|| !self.image_texture_load_indices.is_empty()
			|| !self.image_texture_unload_indices.is_empty()
			|| !self.cube_texture_load_indices.is_empty()
			|| !self.cube_texture_unload_indices.is_empty()
			|| !self.material_slot_load_indices.is_empty()
			|| !self.material_slot_unload_indices.is_empty()
	}
}

fn active_residency_gaps_from_draws<'a>(
	draws: impl IntoIterator<Item = (bool, &'a [usize], &'a [usize], Option<usize>)>,
	image_texture_residency: &[bool],
	cube_texture_residency: &[bool],
	material_slot_residency: &[bool],
) -> SceneMeshActiveResidencyGaps {
	let mut inactive_image_texture_indices = Vec::new();
	let mut inactive_cube_texture_indices = Vec::new();
	let mut inactive_material_slot_indices = Vec::new();
	let mut active_draws_using_inactive_image_texture_count = 0;
	let mut active_draws_using_inactive_cube_texture_count = 0;
	let mut active_draws_using_inactive_material_slot_count = 0;
	for (active, texture_indices, cube_texture_indices, material_slot_index) in draws {
		if !active {
			continue;
		}
		let mut draw_uses_inactive_image_texture = false;
		let mut draw_uses_inactive_cube_texture = false;
		for texture_index in texture_indices {
			if image_texture_residency.get(*texture_index).is_some_and(|resident| !resident) {
				push_unique_index(&mut inactive_image_texture_indices, *texture_index);
				draw_uses_inactive_image_texture = true;
			}
		}
		for texture_index in cube_texture_indices {
			if cube_texture_residency.get(*texture_index).is_some_and(|resident| !resident) {
				push_unique_index(&mut inactive_cube_texture_indices, *texture_index);
				draw_uses_inactive_cube_texture = true;
			}
		}
		if draw_uses_inactive_image_texture {
			active_draws_using_inactive_image_texture_count += 1;
		}
		if draw_uses_inactive_cube_texture {
			active_draws_using_inactive_cube_texture_count += 1;
		}
		if let Some(material_slot_index) = material_slot_index {
			if material_slot_residency.get(material_slot_index).is_some_and(|resident| !resident) {
				push_unique_index(&mut inactive_material_slot_indices, material_slot_index);
				active_draws_using_inactive_material_slot_count += 1;
			}
		}
	}
	SceneMeshActiveResidencyGaps {
		inactive_image_texture_indices: sorted_unique_indices(inactive_image_texture_indices),
		inactive_cube_texture_indices: sorted_unique_indices(inactive_cube_texture_indices),
		inactive_material_slot_indices: sorted_unique_indices(inactive_material_slot_indices),
		active_draws_using_inactive_image_texture_count,
		active_draws_using_inactive_cube_texture_count,
		active_draws_using_inactive_material_slot_count,
	}
}

fn residency_load_indices(old: &[bool], next: &[bool]) -> Vec<usize> {
	residency_transition_indices(old, next, false, true)
}

fn residency_unload_indices(old: &[bool], next: &[bool]) -> Vec<usize> {
	residency_transition_indices(old, next, true, false)
}

fn texture_residency_for_active_draws<'a>(
	scene: &UnaSceneSnapshot,
	asset_residency: &SceneAssetResidencySets,
	active_draw_texture_indices: impl IntoIterator<Item = (&'a [usize], &'a [usize])>,
) -> (Vec<bool>, Vec<bool>) {
	let mut image_residency = vec![false; scene.images.len()];
	let mut cube_residency = vec![false; scene.images.len()];
	for (image_indices, cube_indices) in active_draw_texture_indices {
		for &image_index in image_indices {
			if asset_residency.image_resident(image_index) {
				if let Some(resident) = image_residency.get_mut(image_index) {
					*resident = true;
				}
			}
		}
		for &image_index in cube_indices {
			if asset_residency.image_resident(image_index)
				&& texture_source_is_cube(scene.image_sources.get(image_index).and_then(Option::as_ref))
			{
				if let Some(resident) = cube_residency.get_mut(image_index) {
					*resident = true;
				}
			}
		}
	}
	(image_residency, cube_residency)
}

fn residency_transition_indices(old: &[bool], next: &[bool], from: bool, to: bool) -> Vec<usize> {
	let mut indices = Vec::new();
	for index in 0..old.len().max(next.len()) {
		let old_value = old.get(index).copied().unwrap_or(false);
		let next_value = next.get(index).copied().unwrap_or(false);
		if old_value == from && next_value == to {
			indices.push(index);
		}
	}
	indices
}

fn promote_residency_indices(residency: &mut [bool], indices: &[usize]) -> usize {
	let mut changed = 0;
	for index in indices {
		let Some(resident) = residency.get_mut(*index) else {
			continue;
		};
		if !*resident {
			*resident = true;
			changed += 1;
		}
	}
	changed
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

fn blend_pipeline_for_shading(shading: UnaShadingModel) -> DrawPipelineKind {
	match shading {
		UnaShadingModel::LitLambert => DrawPipelineKind::BlendLit,
		UnaShadingModel::Unlit => DrawPipelineKind::BlendUnlit,
		UnaShadingModel::MToonLike | UnaShadingModel::LilToonLike => DrawPipelineKind::BlendToon,
	}
}

fn opaque_pipeline_for_shading(shading: UnaShadingModel) -> DrawPipelineKind {
	match shading {
		UnaShadingModel::LitLambert => DrawPipelineKind::OpaqueLit,
		UnaShadingModel::Unlit => DrawPipelineKind::OpaqueUnlit,
		UnaShadingModel::MToonLike | UnaShadingModel::LilToonLike => DrawPipelineKind::OpaqueToon,
	}
}

fn liltoon_uses_additive_color_blend(material: &UnaMaterialPbr) -> bool {
	let Some(liltoon_like) = material.liltoon_like_runtime() else {
		return false;
	};
	(liltoon_like.blend_state.source_factor - 1.0).abs() < 0.001
		&& (liltoon_like.blend_state.destination_factor - 1.0).abs() < 0.001
		&& liltoon_like.blend_state.operation_factor.abs() < 0.001
}

fn blend_pipeline_for_draw(draw: &MeshDraw, shading: UnaShadingModel, zwrite: bool) -> DrawPipelineKind {
	if shading.is_toon_like() && liltoon_uses_additive_color_blend(&draw.material) {
		if zwrite {
			DrawPipelineKind::BlendToonAddZWrite
		} else {
			DrawPipelineKind::BlendToonAdd
		}
	} else if zwrite && shading.is_toon_like() {
		DrawPipelineKind::BlendToonZWrite
	} else {
		blend_pipeline_for_shading(shading)
	}
}

fn material_render_queue_number(material: &UnaMaterialPbr, alpha_mode: UnaAlphaMode) -> i32 {
	if let Some(render_queue) = material
		.liltoon_like_runtime()
		.and_then(|liltoon_like| liltoon_like.rendering.render_queue_number)
	{
		return render_queue;
	}
	match alpha_mode {
		UnaAlphaMode::Opaque => 2000,
		UnaAlphaMode::Mask => 2450,
		UnaAlphaMode::Blend => 3000,
	}
}

fn draw_render_queue_number(draw: &MeshDraw) -> i32 {
	material_render_queue_number(&draw.material, draw.alpha_mode)
}

fn draw_render_order_key(draws: &[MeshDraw], draw_index: usize) -> (i32, usize) {
	(draw_render_queue_number(&draws[draw_index]), draw_index)
}

fn draw_uses_late_non_blend_queue(alpha_mode: UnaAlphaMode, render_queue: i32) -> bool {
	!matches!(alpha_mode, UnaAlphaMode::Blend) && render_queue >= 3000
}

fn material_needs_screen_refraction(material: &UnaMaterialPbr) -> bool {
	material
		.liltoon_like_runtime()
		.is_some_and(un_avatar_core::UnaLilToonLikeMaterial::needs_screen_refraction)
}

fn liltoon_audio_link_has_active_target(audio_link: &un_avatar_core::UnaLilToonLikeAudioLink) -> bool {
	[
		audio_link.to_main_second_factor,
		audio_link.to_main_third_factor,
		audio_link.to_emission_factor,
		audio_link.to_emission_gradation_factor,
		audio_link.to_emission_second_factor,
		audio_link.to_emission_second_gradation_factor,
		audio_link.to_vertex_factor,
	]
	.into_iter()
	.any(|value| value > 0.5)
}

fn material_needs_audio_link_texture(material: &UnaMaterialPbr, shading: UnaShadingModel) -> bool {
	if !shading.is_liltoon_like() {
		return false;
	}
	material.liltoon_like_runtime().is_some_and(|liltoon_like| {
		liltoon_like.audio_link.enabled_factor > 0.5 && liltoon_audio_link_has_active_target(&liltoon_like.audio_link)
	})
}

fn material_untoon_feature_plan(material: &UnaMaterialPbr, shading: UnaShadingModel, opts: &SceneMeshLoadOpts) -> UntoonFeaturePlan {
	if opts.force_simple_basecolor || !shading.is_toon_like() {
		return UntoonFeaturePlan::none();
	}
	if let Some(liltoon_like) = material.liltoon_like_runtime() {
		let has_main_layer_dissolve = liltoon_like.main_color.second_dissolve.mask_texture_index.is_some()
			|| liltoon_like.main_color.second_dissolve.noise_mask_texture_index.is_some()
			|| liltoon_like.main_color.third_dissolve.mask_texture_index.is_some()
			|| liltoon_like.main_color.third_dissolve.noise_mask_texture_index.is_some();
		let has_dissolve = liltoon_like.dissolve.mask_texture_index.is_some()
			|| liltoon_like.dissolve.noise_mask_texture_index.is_some()
			|| liltoon_like.dissolve.params_factor[0].abs() > 0.00001
			|| ((lil_enabled(liltoon_like.main_color.second_enabled_factor) || lil_enabled(liltoon_like.main_color.third_enabled_factor))
				&& has_main_layer_dissolve);
		let main_layers = lil_enabled(liltoon_like.main_color.second_enabled_factor)
			|| lil_enabled(liltoon_like.main_color.third_enabled_factor)
			|| liltoon_features::uses_main_color_adjustment(&liltoon_like.main_color);
		let matcap = lil_enabled(liltoon_like.matcap.enabled_factor);
		let matcap_second = lil_enabled(liltoon_like.matcap.second_enabled_factor);
		let reflection = lil_enabled(liltoon_like.reflection.enabled_factor);
		let reflection_cube = liltoon_uses_reflection_cube_texture(liltoon_like);
		let anisotropy = lil_enabled(liltoon_like.reflection.anisotropy_enabled_factor);
		let rim = lil_enabled(liltoon_like.rim.enabled_factor);
		let emission = lil_enabled(liltoon_like.emission.enabled_factor);
		let emission_second = lil_enabled(liltoon_like.emission.second_enabled_factor);
		UntoonFeaturePlan {
			source_profile: UntoonSourceProfile::LilToon,
			shader_features: UntoonShaderFeatures {
				profile_extensions: true,
				main_layers,
				alpha_mask: liltoon_like.alpha_mask.texture_index.is_some() || liltoon_like.alpha_mask.mode_factor.abs() > 0.00001,
				dissolve: has_dissolve,
				parallax: lil_enabled(liltoon_like.parallax.enabled_factor),
				id_mask: liltoon_features::uses_id_mask(&liltoon_like.id_mask),
				udim_discard: liltoon_features::uses_udim_discard(&liltoon_like.udim_discard),
				audio_link: material_needs_audio_link_texture(material, shading),
				shadow_layers: lil_enabled(liltoon_like.shadow.enabled_factor),
				matcap,
				matcap_second,
				matcap_custom_normal: liltoon_like.matcap.custom_normal_factor > 0.5
					|| liltoon_like.matcap.second_custom_normal_factor > 0.5,
				reflection,
				reflection_cube,
				anisotropy,
				rim,
				rim_shade: lil_enabled(liltoon_like.rim.shade_enabled_factor),
				backlight: lil_enabled(liltoon_like.backlight.enabled_factor),
				glitter: lil_enabled(liltoon_like.glitter.enabled_factor),
				emission,
				emission_second,
				distance_fade: liltoon_like.rendering.distance_fade_color_factor[3] > 0.00001
					|| liltoon_like.rendering.distance_fade_rim_color_factor[3] > 0.00001
					|| liltoon_like.rendering.distance_fade_mode_factor > 0.5,
				fur: material_has_fur(material, shading, opts),
				gem: liltoon_like.is_gem_profile(),
				refraction: liltoon_like.needs_screen_refraction(),
				normal_second: lil_enabled(liltoon_like.normal.second_enabled_factor),
			},
		}
	} else {
		let mtoon = material.mtoon_like_runtime();
		UntoonFeaturePlan {
			source_profile: UntoonSourceProfile::MToon,
			shader_features: UntoonShaderFeatures {
				profile_extensions: false,
				shadow_layers: true,
				matcap: mtoon.is_some_and(|mtoon| mtoon.matcap_texture_index.is_some()),
				reflection: mtoon.is_some_and(|mtoon| mtoon.reflection_cube_texture_index.is_some()),
				reflection_cube: mtoon.is_some_and(|mtoon| mtoon.reflection_cube_texture_index.is_some()),
				rim: mtoon.is_some_and(|mtoon| {
					mtoon.rim_multiply_texture_index.is_some()
						|| mtoon.parametric_rim_color_factor.iter().any(|value| value.abs() > 0.00001)
				}),
				emission: material.emissive_texture_index.is_some() || material.emissive_factor.iter().any(|value| value.abs() > 0.00001),
				..Default::default()
			},
		}
	}
}

fn material_untoon_shader_features(material: &UnaMaterialPbr, shading: UnaShadingModel, opts: &SceneMeshLoadOpts) -> UntoonShaderFeatures {
	material_untoon_feature_plan(material, shading, opts).shader_features
}

fn material_runtime_requirements(
	material: &UnaMaterialPbr,
	shading: UnaShadingModel,
	opts: &SceneMeshLoadOpts,
) -> SceneMeshRuntimeRequirements {
	SceneMeshRuntimeRequirements {
		audio_link_texture: material_needs_audio_link_texture(material, shading),
		screen_refraction: material_needs_screen_refraction(material),
		fur: material_has_fur(material, shading, opts),
		toon_shader_features: material_untoon_shader_features(material, shading, opts),
	}
}

fn draw_uses_screen_refraction_grab(draw: &MeshDraw) -> bool {
	material_needs_screen_refraction(&draw.material)
}

fn material_uses_liltoon_gem_prepass(material: &UnaMaterialPbr) -> bool {
	material
		.liltoon_like_runtime()
		.is_some_and(un_avatar_core::UnaLilToonLikeMaterial::is_gem_profile)
}

fn draw_uses_liltoon_gem_prepass(draw: &MeshDraw) -> bool {
	material_uses_liltoon_gem_prepass(&draw.material)
}

fn liltoon_reflection_texture_index(liltoon_like: &un_avatar_core::UnaLilToonLikeMaterial) -> Option<usize> {
	liltoon_uses_reflection_cube_texture(liltoon_like)
		.then_some(liltoon_like.reflection.cube_texture_index)
		.flatten()
}

fn liltoon_uses_reflection_cube_texture(liltoon_like: &un_avatar_core::UnaLilToonLikeMaterial) -> bool {
	liltoon_like.reflection.cube_texture_index.is_some()
		&& liltoon_like.uses_reflection_source_cube()
		&& (liltoon_like.is_gem_profile()
			|| (lil_enabled(liltoon_like.reflection.enabled_factor) && lil_enabled(liltoon_like.reflection.apply_reflection_factor)))
}

fn transparent_backpass_enabled(
	alpha_mode: UnaAlphaMode,
	transparent_with_z_write: bool,
	shading: UnaShadingModel,
	liltoon_backpass_enabled: bool,
) -> bool {
	alpha_mode == UnaAlphaMode::Blend && transparent_with_z_write && liltoon_backpass_enabled && shading.is_toon_like()
}

fn draw_uses_transparent_backpass(draw: &MeshDraw, shading: UnaShadingModel) -> bool {
	let liltoon_backpass_enabled = draw
		.material
		.liltoon_like_runtime()
		.is_none_or(|u| u.blend_state.pre_zwrite_factor > 0.5);
	transparent_backpass_enabled(
		draw.alpha_mode,
		material_transparent_with_zwrite(&draw.material),
		shading,
		liltoon_backpass_enabled,
	)
}

fn transparent_forward_zwrite_enabled(alpha_mode: UnaAlphaMode, transparent_with_z_write: bool, shading: UnaShadingModel) -> bool {
	alpha_mode == UnaAlphaMode::Blend && transparent_with_z_write && shading.is_toon_like()
}

fn build_draw_order(draws: &[MeshDraw], opts: &SceneMeshLoadOpts) -> SceneMeshDrawState {
	build_draw_order_for_scope(draws, opts, false)
}

fn build_potential_draw_order(draws: &[MeshDraw], opts: &SceneMeshLoadOpts) -> SceneMeshDrawState {
	build_draw_order_for_scope(draws, opts, true)
}

fn build_draw_order_for_scope(draws: &[MeshDraw], opts: &SceneMeshLoadOpts, include_inactive: bool) -> SceneMeshDrawState {
	let mut state = SceneMeshDrawState {
		outline_draw_indices: Vec::with_capacity(draws.len()),
		fur_draw_indices: Vec::with_capacity(draws.len()),
		opaque_batches: Vec::new(),
		transparent_backpass_draw_indices: Vec::with_capacity(draws.len()),
		blended_batches: Vec::new(),
		active_draw_indices: Vec::with_capacity(draws.len()),
		active_morph_draw_indices: Vec::new(),
		needs_screen_refraction: false,
		active_skin_palette_indices: Vec::with_capacity(draws.len()),
		runtime_requirements: SceneMeshRuntimeRequirements::default(),
	};
	let batch_capacity = (draws.len() / 10).max(1);
	let mut opaque_draws = Vec::with_capacity(draws.len());
	let mut blended_draws = Vec::with_capacity(draws.len());
	let mut blended_batches = Vec::with_capacity(batch_capacity);

	for (draw_index, draw) in draws.iter().enumerate() {
		if !include_inactive && !draw.active() {
			continue;
		}
		if draw.active() {
			state.active_draw_indices.push(draw_index);
			if draw.morph_target_count > 0 {
				state.active_morph_draw_indices.push(draw_index);
			}
			if !draw.skin_palette_static_identity {
				state.active_skin_palette_indices.push(draw.skin_palette_index);
			}
		}
		let requirements = material_runtime_requirements(&draw.material, draw.shading, opts);
		state.runtime_requirements.include(requirements);
		if requirements.screen_refraction {
			state.needs_screen_refraction = true;
		}
		let shading = effective_mesh_shading(draw, opts);
		if !opts.disable_mtoon_outlines
			&& draw_has_outline(draw, opts)
			&& matches!(draw.alpha_mode, UnaAlphaMode::Opaque | UnaAlphaMode::Mask)
		{
			state.outline_draw_indices.push(draw_index);
		}
		let has_fur = requirements.fur;
		if has_fur {
			state.fur_draw_indices.push(draw_index);
		}

		match draw.alpha_mode {
			UnaAlphaMode::Opaque | UnaAlphaMode::Mask
				if draw_uses_screen_refraction_grab(draw)
					|| draw_uses_late_non_blend_queue(draw.alpha_mode, draw_render_queue_number(draw)) =>
			{
				blended_draws.push((DrawPipelineKey::new(opaque_pipeline_for_shading(shading), draw, opts), draw_index));
			}
			UnaAlphaMode::Opaque => {
				opaque_draws.push((DrawPipelineKey::new(opaque_pipeline_for_shading(shading), draw, opts), draw_index));
			}
			UnaAlphaMode::Mask => {
				opaque_draws.push((DrawPipelineKey::new(opaque_pipeline_for_shading(shading), draw, opts), draw_index));
			}
			UnaAlphaMode::Blend if draw_uses_transparent_backpass(draw, shading) => {
				blended_draws.push((
					DrawPipelineKey::new(transparent_backpass_pipeline_for_draw(draw), draw, opts),
					draw_index,
				));
				if draw_uses_liltoon_gem_prepass(draw) {
					blended_draws.push((DrawPipelineKey::new(DrawPipelineKind::LilToonGemPre, draw, opts), draw_index));
				}
				blended_draws.push((
					DrawPipelineKey::new(blend_pipeline_for_draw(draw, shading, true), draw, opts),
					draw_index,
				));
			}
			UnaAlphaMode::Blend => {
				if draw_uses_liltoon_gem_prepass(draw) {
					blended_draws.push((DrawPipelineKey::new(DrawPipelineKind::LilToonGemPre, draw, opts), draw_index));
				}
				blended_draws.push((
					DrawPipelineKey::new(
						blend_pipeline_for_draw(
							draw,
							shading,
							transparent_forward_zwrite_enabled(draw.alpha_mode, material_transparent_with_zwrite(&draw.material), shading),
						),
						draw,
						opts,
					),
					draw_index,
				));
			}
		}
	}
	blended_draws.sort_by_key(|&(pipeline, draw_index)| {
		let (render_queue, draw_index) = draw_render_order_key(draws, draw_index);
		(render_queue, draw_index, blended_pipeline_pass_order(pipeline.kind))
	});
	for (pipeline, draw_index) in blended_draws {
		append_ordered_draw_batch(&mut blended_batches, pipeline, draw_index, batch_capacity);
	}

	let mut opaque_batches = Vec::with_capacity(opaque_draws.len());
	opaque_draws.sort_by_key(|&(_, draw_index)| draw_render_order_key(draws, draw_index));
	for (pipeline, draw_index) in opaque_draws {
		append_ordered_draw_batch(&mut opaque_batches, pipeline, draw_index, batch_capacity);
	}

	group_draw_indices_by_skin_palette(draws, &mut state.outline_draw_indices);
	group_draw_indices_by_skin_palette(draws, &mut state.fur_draw_indices);
	group_draw_indices_by_skin_palette(draws, &mut state.transparent_backpass_draw_indices);

	finalize_draw_batches(&mut opaque_batches);
	finalize_draw_batches(&mut blended_batches);
	state.opaque_batches = opaque_batches;
	state.blended_batches = blended_batches;
	state.active_skin_palette_indices.sort_unstable();
	state.active_skin_palette_indices.dedup();
	state
}

fn draw_untoon_shader_features(draw: &MeshDraw, opts: &SceneMeshLoadOpts) -> UntoonShaderFeatures {
	material_untoon_shader_features(&draw.material, effective_mesh_shading(draw, opts), opts)
}

fn include_draw_features_for_pipeline(
	pipeline_features: &mut BTreeMap<DrawPipelineKey, UntoonShaderFeatures>,
	pipeline: DrawPipelineKey,
	draw: &MeshDraw,
	opts: &SceneMeshLoadOpts,
) {
	pipeline_features
		.entry(pipeline)
		.or_default()
		.include(draw_untoon_shader_features(draw, opts));
}

fn draw_pipeline_shader_features(
	draws: &[MeshDraw],
	draw_state: &SceneMeshDrawState,
	opts: &SceneMeshLoadOpts,
) -> BTreeMap<DrawPipelineKey, UntoonShaderFeatures> {
	let mut pipeline_features = BTreeMap::new();
	for batch in draw_state.opaque_batches.iter().chain(draw_state.blended_batches.iter()) {
		for &draw_index in &batch.draw_indices {
			if let Some(draw) = draws.get(draw_index) {
				include_draw_features_for_pipeline(&mut pipeline_features, batch.pipeline, draw, opts);
			}
		}
	}
	for &draw_index in &draw_state.transparent_backpass_draw_indices {
		let Some(draw) = draws.get(draw_index) else {
			continue;
		};
		let zwrite = draw
			.material
			.liltoon_like_runtime()
			.is_none_or(|u| u.blend_state.pre_zwrite_factor > 0.5);
		include_draw_features_for_pipeline(
			&mut pipeline_features,
			DrawPipelineKey::new(
				if zwrite {
					DrawPipelineKind::TransparentToonBackpass
				} else {
					DrawPipelineKind::TransparentToonBackpassNoZWrite
				},
				draw,
				opts,
			),
			draw,
			opts,
		);
	}
	pipeline_features
}

enum SceneImageTextureUpload {
	Source(SourceTextureUpload),
	Payload {
		payload: TextureUploadPayload,
		format: wgpu::TextureFormat,
		width: u32,
		height: u32,
	},
	Lazy(SceneImageTextureLazyUpload),
}

struct SceneImageTextureLazyUpload {
	image_index: usize,
	role: TextureRole,
	mipmap_filter: TextureMipmapFilter,
	texture_max_dimension: Option<u32>,
	texture_compression: TextureCompressionMode,
	block_compression_encoder: BlockCompressionEncoder,
	block_compression_cpu_threads: usize,
	processed_texture_cache: bool,
	texture_compression_advanced: TextureCompressionAdvancedOptions,
	texture_compression_bc_supported: bool,
	gpu_texture_compression_enabled: bool,
}

struct SceneImageTextureSlot {
	upload: SceneImageTextureUpload,
	texture: Option<wgpu::Texture>,
	view: Option<wgpu::TextureView>,
}

impl SceneImageTextureSlot {
	fn new(upload: SceneImageTextureUpload) -> Self {
		Self {
			upload,
			texture: None,
			view: None,
		}
	}

	fn ensure_uploaded(
		&mut self,
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		scene: Option<&UnaSceneSnapshot>,
		gpu_texture_compression: &mut Option<GpuTextureCompressionContext>,
	) -> Option<wgpu::TextureView> {
		if let Some(view) = &self.view {
			return Some(view.clone());
		}
		if let SceneImageTextureUpload::Lazy(lazy) = &self.upload {
			let scene = scene?;
			let upload = build_lazy_scene_image_texture_upload(scene, lazy, gpu_texture_compression)?;
			self.upload = upload;
		}
		let texture = match &self.upload {
			SceneImageTextureUpload::Source(upload) => create_source_image_texture(device, queue, upload),
			SceneImageTextureUpload::Payload {
				payload,
				format,
				width,
				height,
			} => create_payload_image_texture(device, queue, payload, *format, *width, *height),
			SceneImageTextureUpload::Lazy(_) => return None,
		};
		let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
		self.texture = Some(texture);
		self.view = Some(view.clone());
		Some(view)
	}

	fn unload(&mut self) -> bool {
		let had_resource = self.texture.is_some() || self.view.is_some();
		self.view = None;
		self.texture = None;
		had_resource
	}
}

fn build_lazy_scene_image_texture_upload(
	scene: &UnaSceneSnapshot,
	lazy: &SceneImageTextureLazyUpload,
	gpu_texture_compression: &mut Option<GpuTextureCompressionContext>,
) -> Option<SceneImageTextureUpload> {
	let image = scene.images.get(lazy.image_index)?;
	let source_metadata = scene.image_sources.get(lazy.image_index).and_then(Option::as_ref);
	let decoded_source;
	let upload_image = if let Some(decoded) = source_metadata.and_then(decode_encoded_source_image) {
		decoded_source = decoded;
		&decoded_source
	} else {
		image
	};
	build_scene_image_texture_upload(upload_image, source_metadata, lazy, gpu_texture_compression)
}

fn build_lazy_scene_cube_texture_upload(
	scene: &UnaSceneSnapshot,
	lazy: &SceneCubeTextureLazyUpload,
) -> Option<(CubeUpload, CubeUploadCacheEvent)> {
	let image = scene.images.get(lazy.image_index)?;
	let source_metadata = scene.image_sources.get(lazy.image_index).and_then(Option::as_ref);
	let decoded_source;
	let upload_image = if let Some(decoded) = source_metadata.and_then(decode_encoded_source_image) {
		decoded_source = decoded;
		&decoded_source
	} else {
		image
	};
	cube_upload_from_image(upload_image, source_metadata, lazy.processed_texture_cache)
}

fn decode_encoded_source_image(source: &UnaImageSourceMetadata) -> Option<UnaImageRgba> {
	let file_bytes;
	let bytes = if let Some(bytes) = source.encoded_bytes.as_deref() {
		bytes
	} else {
		let path = source.source_file_path.as_ref()?;
		let offset = source.byte_offset?;
		let len = usize::try_from(source.byte_length).ok()?;
		let mut file = fs::File::open(path).ok()?;
		file.seek(SeekFrom::Start(offset)).ok()?;
		file_bytes = {
			let mut bytes = vec![0u8; len];
			file.read_exact(&mut bytes).ok()?;
			bytes
		};
		file_bytes.as_slice()
	};
	let decoded = image::load_from_memory(bytes).ok()?.to_rgba8();
	let (width, height) = decoded.dimensions();
	Some(UnaImageRgba {
		width,
		height,
		pixel_format: un_avatar_core::UnaImagePixelFormat::R8G8B8A8,
		pixels: decoded.into_raw(),
	})
}

fn source_has_lazy_encoded_bytes(source: &UnaImageSourceMetadata) -> bool {
	source.encoded_bytes.is_some() || (source.source_file_path.is_some() && source.byte_offset.is_some() && source.byte_length > 0)
}

fn is_deferred_scene_image_placeholder(image: &UnaImageRgba) -> bool {
	image.width == 0 && image.height == 0 && image.pixels.is_empty()
}

fn scene_image_source_dimensions(image: &UnaImageRgba, source: Option<&UnaImageSourceMetadata>) -> (u32, u32) {
	if image.width > 0 && image.height > 0 {
		return (image.width, image.height);
	}
	source
		.and_then(|source| Some((source.width?, source.height?)))
		.unwrap_or((image.width.max(1), image.height.max(1)))
}

struct SceneCubeTextureLazyUpload {
	image_index: usize,
	processed_texture_cache: bool,
}

enum SceneCubeTextureUpload {
	Source(CubeUpload),
	Lazy(SceneCubeTextureLazyUpload),
}

struct SceneCubeTextureSlot {
	upload: SceneCubeTextureUpload,
	texture: Option<wgpu::Texture>,
	view: Option<wgpu::TextureView>,
}

impl SceneCubeTextureSlot {
	fn new(upload: CubeUpload) -> Self {
		Self {
			upload: SceneCubeTextureUpload::Source(upload),
			texture: None,
			view: None,
		}
	}

	fn new_lazy(image_index: usize, processed_texture_cache: bool) -> Self {
		Self {
			upload: SceneCubeTextureUpload::Lazy(SceneCubeTextureLazyUpload {
				image_index,
				processed_texture_cache,
			}),
			texture: None,
			view: None,
		}
	}

	fn ensure_uploaded(
		&mut self,
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		scene: Option<&UnaSceneSnapshot>,
	) -> Option<wgpu::TextureView> {
		if let Some(view) = &self.view {
			return Some(view.clone());
		}
		if let SceneCubeTextureUpload::Lazy(lazy) = &self.upload {
			let (upload, _) = build_lazy_scene_cube_texture_upload(scene?, lazy)?;
			self.upload = SceneCubeTextureUpload::Source(upload);
		}
		let SceneCubeTextureUpload::Source(upload) = &self.upload else {
			return None;
		};
		let texture = create_cube_texture_from_upload(device, queue, upload);
		let view = texture.create_view(&wgpu::TextureViewDescriptor {
			label: Some("gltf_image_cube_view"),
			dimension: Some(wgpu::TextureViewDimension::Cube),
			..Default::default()
		});
		self.texture = Some(texture);
		self.view = Some(view.clone());
		Some(view)
	}

	fn unload(&mut self) -> bool {
		let had_resource = self.texture.is_some() || self.view.is_some();
		self.view = None;
		self.texture = None;
		had_resource
	}
}

pub(crate) struct SceneMeshes {
	pipelines: BTreeMap<DrawPipelineKey, wgpu::RenderPipeline>,
	pipelines_outline_toon: BTreeMap<MaterialRenderStateKey, wgpu::RenderPipeline>,
	compute_fur_cards_bind_group_layout: wgpu::BindGroupLayout,
	compute_fur_cards_compute_pipeline: Option<ComputeFurCardsComputePipeline>,
	pipelines_compute_fur_cards_pre_toon: BTreeMap<MaterialRenderStateKey, wgpu::RenderPipeline>,
	pipelines_compute_fur_cards_toon: BTreeMap<MaterialRenderStateKey, wgpu::RenderPipeline>,
	frame_buffer: wgpu::Buffer,
	frame_uploaded: Option<MeshFrameGpu>,
	frame_layout: wgpu::BindGroupLayout,
	frame_bind_group: wgpu::BindGroup,
	material_layout: wgpu::BindGroupLayout,
	outline_material_layout: wgpu::BindGroupLayout,
	morph_bind_group_layout: wgpu::BindGroupLayout,
	shader_variant_tier: MeshShaderVariantTier,
	screen_grab_sampler: wgpu::Sampler,
	reflection_cube_sampler: wgpu::Sampler,
	_screen_grab_fallback_texture: wgpu::Texture,
	_audio_link_texture: wgpu::Texture,
	audio_link_view: wgpu::TextureView,
	audio_link_uploaded_sequence: u64,
	audio_link_frame_params: [f32; 4],
	texture_views: SceneTextureViews,
	image_texture_slots: Vec<SceneImageTextureSlot>,
	cube_texture_slots: Vec<Option<SceneCubeTextureSlot>>,
	#[allow(dead_code)]
	_samplers: Box<[wgpu::Sampler]>,
	image_sampler_indices: Box<[usize]>,
	#[allow(dead_code)]
	_textures: Vec<wgpu::Texture>,
	#[allow(dead_code)]
	_cube_textures: Vec<wgpu::Texture>,
	draws: Vec<MeshDraw>,
	skin_palettes: Vec<SkinPalette>,
	outline_draw_indices: Box<[usize]>,
	fur_draw_indices: Box<[usize]>,
	opaque_batches: Vec<DrawBatch>,
	transparent_backpass_draw_indices: Box<[usize]>,
	blended_batches: Vec<DrawBatch>,
	active_draw_indices: Box<[usize]>,
	active_morph_draw_indices: Box<[usize]>,
	needs_screen_refraction: bool,
	active_skin_palette_indices: Box<[usize]>,
	image_texture_residency: Vec<bool>,
	cube_texture_residency: Vec<bool>,
	material_slot_residency: Vec<bool>,
	lazy_gpu_texture_compression: Option<GpuTextureCompressionContext>,
	texture_summary: TextureUploadSummary,
	runtime_requirements: SceneMeshRuntimeRequirements,
	visibility_scratch: Vec<bool>,
	expression_names: Box<[String]>,
	expression_value_scratch: Vec<f32>,
	fur_source_vertex_scratch: Vec<ComputeFurCardsSourceVertexGpu>,
	fur_palette_matrix_scratch: Vec<Mat4>,
	has_morph_draws: bool,
	opts: SceneMeshLoadOpts,
}

fn default_morph_weight_for(buf: &UnaMeshBuffers, target_index: usize) -> f32 {
	buf.default_morph_weights.get(target_index).copied().unwrap_or(0.0).clamp(0.0, 1.0)
}

fn tangent_is_missing(tangent: [f32; 4]) -> bool {
	let t = Vec3::new(tangent[0], tangent[1], tangent[2]);
	t.length_squared() <= 0.0000001
}

fn fallback_tangent_for_normal(normal: Vec3) -> Vec3 {
	let axis = if normal.y.abs() < 0.999 { Vec3::Y } else { Vec3::X };
	axis.cross(normal).try_normalize().unwrap_or(Vec3::X)
}

fn fill_missing_tangents(verts: &mut [Vertex], indices: &[u32]) {
	if !verts.iter().any(|v| tangent_is_missing(v.tangent)) {
		return;
	}
	let mut tan1 = vec![Vec3::ZERO; verts.len()];
	let mut tan2 = vec![Vec3::ZERO; verts.len()];
	for tri in indices.chunks_exact(3) {
		let i0 = tri[0] as usize;
		let i1 = tri[1] as usize;
		let i2 = tri[2] as usize;
		if i0 >= verts.len() || i1 >= verts.len() || i2 >= verts.len() {
			continue;
		}
		let p0 = Vec3::from_array(verts[i0].pos);
		let p1 = Vec3::from_array(verts[i1].pos);
		let p2 = Vec3::from_array(verts[i2].pos);
		let uv0 = verts[i0].uv;
		let uv1 = verts[i1].uv;
		let uv2 = verts[i2].uv;
		let e1 = p1 - p0;
		let e2 = p2 - p0;
		let duv1 = [uv1[0] - uv0[0], uv1[1] - uv0[1]];
		let duv2 = [uv2[0] - uv0[0], uv2[1] - uv0[1]];
		let det = duv1[0] * duv2[1] - duv1[1] * duv2[0];
		if det.abs() <= 0.0000001 {
			continue;
		}
		let inv_det = 1.0 / det;
		let sdir = (e1 * duv2[1] - e2 * duv1[1]) * inv_det;
		let tdir = (e2 * duv1[0] - e1 * duv2[0]) * inv_det;
		tan1[i0] += sdir;
		tan1[i1] += sdir;
		tan1[i2] += sdir;
		tan2[i0] += tdir;
		tan2[i1] += tdir;
		tan2[i2] += tdir;
	}
	for (i, vert) in verts.iter_mut().enumerate() {
		if !tangent_is_missing(vert.tangent) {
			continue;
		}
		let n = Vec3::from_array(vert.norm).try_normalize().unwrap_or(Vec3::Y);
		let authored_tangent = tan1[i];
		let tangent_ortho = authored_tangent - n * n.dot(authored_tangent);
		let tangent = tangent_ortho.try_normalize().unwrap_or_else(|| fallback_tangent_for_normal(n));
		let sign = if tan2[i].length_squared() > 0.0000001 && n.cross(tangent).dot(tan2[i]) < 0.0 {
			-1.0
		} else {
			1.0
		};
		vert.tangent = [tangent.x, tangent.y, tangent.z, sign];
	}
}

fn primitive_indices(buf: &UnaMeshBuffers) -> Vec<u32> {
	let vertex_count = buf.positions.len();
	match &buf.indices {
		Some(idx) => {
			let mut out_idx = Vec::with_capacity(idx.len());
			for &pi in idx {
				if (pi as usize) < vertex_count {
					out_idx.push(pi);
				}
			}
			out_idx
		}
		None => (0..vertex_count as u32).collect(),
	}
}

fn primitive_expand_cache_safe(buf: &UnaMeshBuffers) -> bool {
	buf.tangents.as_ref().is_some_and(|tangents| {
		tangents.len() >= buf.positions.len() && tangents.iter().copied().all(|tangent| !tangent_is_missing(tangent))
	})
}

#[cfg(test)]
fn expand_primitive(buf: &UnaMeshBuffers, dynamic_morph_targets: Option<&[usize]>) -> Option<ExpandedPrimitive> {
	expand_primitive_with_cached_morph(buf, dynamic_morph_targets, None)
}

fn expand_primitive_with_cached_morph(
	buf: &UnaMeshBuffers,
	dynamic_morph_targets: Option<&[usize]>,
	cached_morph_payload: Option<&ExpandedMorphPayload>,
) -> Option<ExpandedPrimitive> {
	let default_n = [0.0_f32, 1.0, 0.0];
	let positions = &buf.positions;
	if positions.is_empty() {
		return None;
	}
	let normals = buf.normals.as_deref();
	let tangents = buf.tangents.as_deref();
	let uvs = buf.tex_coords_0.as_deref();
	let uvs1 = buf.tex_coords_1.as_deref();
	let uvs2 = buf.tex_coords_2.as_deref();
	let uvs3 = buf.tex_coords_3.as_deref();
	let colors = buf.colors_0.as_deref();
	let joints_buf = buf.joints.as_deref();
	let weights_buf = buf.weights.as_deref();
	let j_default = [0u16; 4];
	let w_default = [1.0_f32, 0.0, 0.0, 0.0];

	let num_morph = buf.morph_targets.len();
	let cached_morph_payload = cached_morph_payload.filter(|payload| payload.morph_source_indices.iter().all(|&index| index < num_morph));
	let morph_source_indices: Vec<usize> = cached_morph_payload.map_or_else(
		|| {
			dynamic_morph_targets
				.map(|indices| indices.iter().copied().filter(|&index| index < num_morph).collect())
				.unwrap_or_else(|| (0..num_morph).collect())
		},
		|payload| payload.morph_source_indices.to_vec(),
	);
	let vertex_capacity = positions.len();
	let mut morph_push: Option<Vec<Vec<[f32; 3]>>> = if cached_morph_payload.is_none() {
		Some(morph_source_indices.iter().map(|_| Vec::with_capacity(vertex_capacity)).collect())
	} else {
		None
	};
	let has_morph_normals = morph_source_indices.iter().any(|&target_index| {
		buf.morph_targets
			.get(target_index)
			.is_some_and(|target| target.normal_deltas.is_some())
	});
	let mut morph_nrm_push: Option<Vec<Vec<[f32; 3]>>> = if cached_morph_payload.is_none() && has_morph_normals {
		Some(morph_source_indices.iter().map(|_| Vec::with_capacity(vertex_capacity)).collect())
	} else {
		None
	};
	let static_default_morphs = dynamic_morph_targets
		.map(|dynamic_morph_targets| {
			buf.morph_targets
				.iter()
				.enumerate()
				.filter_map(|(target_index, target)| {
					if dynamic_morph_targets.binary_search(&target_index).is_ok() {
						return None;
					}
					let weight = default_morph_weight_for(buf, target_index);
					(weight.abs() > 0.000001).then_some((target, weight))
				})
				.collect::<Vec<_>>()
		})
		.unwrap_or_default();

	let mut verts = Vec::with_capacity(positions.len());
	for pi in 0..positions.len() {
		let mut pos = positions[pi];
		let mut n = normals.and_then(|nn| nn.get(pi)).copied().unwrap_or(default_n);
		if !static_default_morphs.is_empty() {
			for &(target, weight) in &static_default_morphs {
				if let Some(delta) = target.position_deltas.get(pi) {
					pos[0] += delta[0] * weight;
					pos[1] += delta[1] * weight;
					pos[2] += delta[2] * weight;
				}
				if let Some(delta) = target.normal_deltas.as_ref().and_then(|deltas| deltas.get(pi)) {
					n[0] += delta[0] * weight;
					n[1] += delta[1] * weight;
					n[2] += delta[2] * weight;
				}
			}
		}
		let uv = uvs.and_then(|uu| uu.get(pi)).copied().unwrap_or([0.0, 0.0]);
		let uv1 = uvs1.and_then(|uu| uu.get(pi)).copied().unwrap_or(uv);
		let uv2 = uvs2.and_then(|uu| uu.get(pi)).copied().unwrap_or(uv);
		let uv3 = uvs3.and_then(|uu| uu.get(pi)).copied().unwrap_or(uv);
		let tangent = tangents.and_then(|tt| tt.get(pi)).copied().unwrap_or([0.0, 0.0, 0.0, 1.0]);
		let color = colors.and_then(|cc| cc.get(pi)).copied().unwrap_or([1.0, 1.0, 1.0, 1.0]);
		let jo = joints_buf.and_then(|jj| jj.get(pi)).copied().unwrap_or(j_default);
		let we = weights_buf.and_then(|ww| ww.get(pi)).copied().unwrap_or(w_default);
		let n = Vec3::from_array(n)
			.try_normalize()
			.unwrap_or(Vec3::from_array(default_n))
			.to_array();
		verts.push(Vertex {
			pos,
			norm: n,
			tangent,
			uv,
			uv1,
			uv2,
			uv3,
			joints: jo,
			weights: we,
			color,
		});
		if let Some(morph_push) = morph_push.as_mut() {
			for (&target_index, bucket) in morph_source_indices.iter().zip(morph_push.iter_mut()) {
				let d = buf
					.morph_targets
					.get(target_index)
					.and_then(|target| target.position_deltas.get(pi))
					.copied()
					.unwrap_or([0.0, 0.0, 0.0]);
				bucket.push(d);
			}
		}
		if let Some(ref mut normal_buckets) = morph_nrm_push {
			for (&target_index, bucket) in morph_source_indices.iter().zip(normal_buckets.iter_mut()) {
				let nd = buf
					.morph_targets
					.get(target_index)
					.and_then(|target| target.normal_deltas.as_ref().and_then(|n| n.get(pi)))
					.copied()
					.unwrap_or([0.0, 0.0, 0.0]);
				bucket.push(nd);
			}
		}
	}

	let indices = primitive_indices(buf);

	if verts.is_empty() || indices.is_empty() {
		return None;
	}
	fill_missing_tangents(&mut verts, &indices);

	let morph_payload = cached_morph_payload.cloned().unwrap_or_else(|| ExpandedMorphPayload {
		morph_pos: morph_push.unwrap_or_default().into_boxed_slice(),
		morph_nrm: morph_nrm_push.map(Vec::into_boxed_slice),
		default_morph_weights: morph_source_indices
			.iter()
			.map(|&target_index| default_morph_weight_for(buf, target_index))
			.collect::<Vec<_>>()
			.into_boxed_slice(),
		morph_source_indices: morph_source_indices.into_boxed_slice(),
	});

	Some(ExpandedPrimitive {
		verts,
		indices,
		morph_pos: morph_payload.morph_pos.into_vec(),
		morph_nrm: morph_payload.morph_nrm.map(|morph_nrm| morph_nrm.into_vec()),
		default_morph_weights: morph_payload.default_morph_weights.into_vec(),
		morph_source_indices: morph_payload.morph_source_indices.into_vec(),
	})
}

fn expanded_morph_payload_from_primitive(exp: &ExpandedPrimitive) -> ExpandedMorphPayload {
	ExpandedMorphPayload {
		morph_pos: exp.morph_pos.clone().into_boxed_slice(),
		morph_nrm: exp.morph_nrm.clone().map(Vec::into_boxed_slice),
		morph_source_indices: exp.morph_source_indices.clone().into_boxed_slice(),
		default_morph_weights: exp.default_morph_weights.clone().into_boxed_slice(),
	}
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

fn dynamic_morph_target_indices(
	buf: &UnaMeshBuffers,
	bindings: &[ExpressionBinding],
	dynamic_morph_target_names: &[String],
	include_all: bool,
) -> Vec<usize> {
	if buf.morph_targets.is_empty() {
		return Vec::new();
	}
	if include_all {
		return (0..buf.morph_targets.len()).collect();
	}
	let mut indices = Vec::with_capacity(buf.default_morph_weights.len().min(buf.morph_targets.len()) + bindings.len());
	for (index, &weight) in buf.default_morph_weights.iter().enumerate() {
		if index < buf.morph_targets.len() && weight.abs() > 0.000001 {
			push_unique_index(&mut indices, index);
		}
	}
	for binding in bindings {
		if binding.morph_target_index < buf.morph_targets.len() {
			push_unique_index(&mut indices, binding.morph_target_index);
		}
	}
	if !dynamic_morph_target_names.is_empty() {
		for (index, name) in buf.morph_target_names.iter().enumerate() {
			if index < buf.morph_targets.len()
				&& dynamic_morph_target_names
					.binary_search_by(|candidate| candidate.as_str().cmp(name.as_str()))
					.is_ok()
			{
				push_unique_index(&mut indices, index);
			}
		}
	}
	sorted_unique_indices(indices)
}

fn remap_expression_bindings(bindings: &[ExpressionBinding], morph_source_indices: &[usize]) -> Vec<ExpressionBinding> {
	if bindings.is_empty() || morph_source_indices.is_empty() {
		return Vec::new();
	}
	bindings
		.iter()
		.filter_map(|binding| {
			let morph_target_index = morph_source_indices.binary_search(&binding.morph_target_index).ok()?;
			Some(ExpressionBinding {
				preset_index: binding.preset_index,
				morph_target_index,
				weight_scale: binding.weight_scale,
			})
		})
		.collect()
}

fn scene_has_morph_targets(scene: &UnaSceneSnapshot) -> bool {
	scene.meshes.iter().flatten().any(|primitive| !primitive.morph_targets.is_empty())
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
	let mut out = Vec::new();
	write_scene_effective_visibility(scene, &mut out);
	out
}

fn write_scene_effective_visibility(scene: &UnaSceneSnapshot, out: &mut Vec<bool>) {
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

	out.clear();
	out.resize(scene.nodes.len(), false);
	if scene.roots.is_empty() {
		for &root in scene.resolved_roots().iter() {
			visit(scene, root, true, out);
		}
	} else {
		for &root in &scene.roots {
			visit(scene, root, true, out);
		}
	}
}

fn skin_palette_capacity(scene: &UnaSceneSnapshot) -> usize {
	scene.nodes.iter().filter(|node| node.mesh.is_some()).count()
}

fn normalize_skinning_vertices(verts: &mut [Vertex], primitive_has_joints: bool, skin: Option<&un_avatar_core::UnaSkin>) {
	if !primitive_has_joints {
		return;
	}
	let Some(skin) = skin else {
		for v in verts {
			v.joints = [0, 0, 0, 0];
			v.weights = [1.0, 0.0, 0.0, 0.0];
		}
		return;
	};
	let joint_count = skin.joint_nodes.len().min(skin.inverse_bind_matrices.len()).min(MAX_BONES);
	if joint_count == 0 {
		for v in verts {
			v.joints = [0, 0, 0, 0];
			v.weights = [1.0, 0.0, 0.0, 0.0];
		}
		return;
	}
	let cap = (joint_count - 1).min(u16::MAX as usize) as u16;
	for v in verts {
		for joint in &mut v.joints {
			if *joint as usize >= joint_count {
				*joint = cap;
			}
		}
	}
}

fn apply_mesh_cloth_assist_to_vertices(
	verts: &mut [Vertex],
	indices: &[u32],
	skin: Option<&un_avatar_core::UnaSkin>,
	node_paths: &[String],
	mesh_path: &str,
	config: &DynamicsMeshClothAssistConfig,
	categories: &[DynamicsCategoryDefinition],
	dynamic_deforming_node_indices: &[usize],
) -> usize {
	if !config.enabled || config.max_assist_weight <= 0.0 || verts.is_empty() || indices.is_empty() {
		return 0;
	}
	let Some(skin) = skin else {
		return 0;
	};
	if !mesh_cloth_assist_mesh_matches_with_categories(mesh_path, &config.mesh_path_contains, categories) {
		return 0;
	}
	let joint_count = skin.joint_nodes.len().min(skin.inverse_bind_matrices.len()).min(MAX_BONES);
	if joint_count == 0 {
		return 0;
	}
	let dynamic_nodes = (!dynamic_deforming_node_indices.is_empty()).then_some(dynamic_deforming_node_indices);
	let joint_roles = dynamics_mesh_cloth_assist_joint_roles(skin, joint_count, dynamic_nodes, |joint_index| {
		mesh_cloth_assist_joint_leaf(skin, node_paths, joint_index)
	});
	apply_dynamics_mesh_cloth_assist_to_vertices(verts, indices, joint_count, config, |joint_index| {
		joint_roles
			.get(joint_index)
			.copied()
			.unwrap_or(DynamicsMeshClothAssistJointRole::Other)
	})
}

fn debug_dump_mesh_vertex_weights_if_requested(
	mesh_i: usize,
	prim_i: usize,
	node_path: &str,
	material_slot_index: Option<usize>,
	material_name: Option<&str>,
	verts: &[Vertex],
	indices: &[u32],
	skin: Option<&un_avatar_core::UnaSkin>,
	node_paths: &[String],
	dynamic_deforming_node_indices: &[usize],
	mesh_cloth_assist_vertices: usize,
) {
	let Some(path) = std::env::var_os("UN_AVATAR_DEBUG_MESH_WEIGHTS_PATH") else {
		return;
	};
	let filter = std::env::var("UN_AVATAR_DEBUG_MESH_WEIGHTS_FILTER").unwrap_or_default();
	if !filter.is_empty() && !dynamics_token_filter_matches(node_path, &filter) {
		return;
	}
	let joint_count = skin
		.map(|skin| skin.joint_nodes.len().min(skin.inverse_bind_matrices.len()).min(MAX_BONES))
		.unwrap_or(0);
	let vertices = verts
		.iter()
		.enumerate()
		.map(|(vertex_index, vert)| {
			let influences = vert
				.joints
				.iter()
				.zip(vert.weights.iter())
				.map(|(&joint, &weight)| {
					let joint_index = joint as usize;
					let node_index = skin.and_then(|skin| skin.joint_nodes.get(joint_index)).copied();
					let inverse_bind_matrix = skin.and_then(|skin| skin.inverse_bind_matrices.get(joint_index)).copied();
					let node_path = node_index
						.and_then(|node_index| node_paths.get(node_index))
						.cloned()
						.unwrap_or_default();
					serde_json::json!({
						"joint_index": joint_index,
						"weight": weight,
						"node_index": node_index,
						"path": node_path,
						"dynamic_deforming": node_index.is_some_and(|node_index| dynamic_deforming_node_indices.binary_search(&node_index).is_ok()),
						"inverse_bind_matrix": inverse_bind_matrix,
					})
				})
				.collect::<Vec<_>>();
			serde_json::json!({
				"vertex": vertex_index,
				"pos": vert.pos,
				"joints": vert.joints,
				"weights": vert.weights,
				"influences": influences,
			})
		})
		.collect::<Vec<_>>();
	let value = serde_json::json!({
		"mesh_index": mesh_i,
		"primitive_index": prim_i,
		"node_path": node_path,
		"material_slot_index": material_slot_index,
		"material_name": material_name,
		"vertex_count": verts.len(),
		"joint_count": joint_count,
		"mesh_cloth_assist_vertices": mesh_cloth_assist_vertices,
		"indices": indices,
		"vertices": vertices,
	});
	let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) else {
		return;
	};
	let _ = serde_json::to_writer(&mut file, &value);
	let _ = writeln!(file);
}

fn mesh_cloth_assist_joint_leaf<'a>(skin: &un_avatar_core::UnaSkin, node_paths: &'a [String], joint_index: usize) -> &'a str {
	let Some(&node_index) = skin.joint_nodes.get(joint_index) else {
		return "";
	};
	let path = node_paths.get(node_index).map(String::as_str).unwrap_or("");
	path.rsplit('/').next().unwrap_or(path)
}

fn skin_palette_matrix_capacity(skin: Option<&un_avatar_core::UnaSkin>) -> usize {
	skin.map(|skin| skin.joint_nodes.len().min(skin.inverse_bind_matrices.len()).min(MAX_BONES))
		.unwrap_or(1)
		.max(1)
}

fn skin_palette_key_for_node(world_node_index: usize, skin_index: Option<usize>) -> SkinPaletteKey {
	SkinPaletteKey {
		world_node_index: skin_index.map_or(STATIC_IDENTITY_SKIN_PALETTE_NODE, |_| world_node_index),
		skin_index,
	}
}

fn expression_names(catalog: Option<&UnaExpressionCatalog>) -> Vec<String> {
	let Some(catalog) = catalog else {
		return Vec::new();
	};
	let mut names = Vec::with_capacity(catalog.presets.len());
	names.extend(catalog.presets.iter().map(|preset| preset.name.clone()));
	names
}

fn scene_node_paths(scene: &UnaSceneSnapshot) -> Vec<String> {
	let mut parents = vec![None; scene.nodes.len()];
	for (parent, node) in scene.nodes.iter().enumerate() {
		for &child in &node.children {
			if child < parents.len() {
				parents[child] = Some(parent);
			}
		}
	}
	let mut out = vec![String::new(); scene.nodes.len()];
	for index in 0..scene.nodes.len() {
		let mut chain = Vec::new();
		let mut cursor = Some(index);
		while let Some(node_index) = cursor {
			if let Some(node) = scene.nodes.get(node_index) {
				if let Some(name) = node.name.as_deref().filter(|name| !name.is_empty()) {
					chain.push(name.to_string());
				}
			}
			cursor = parents.get(node_index).copied().flatten();
		}
		chain.reverse();
		out[index] = chain.join("/");
	}
	out
}

fn morph_override_key(node_path: &str, morph_name: &str) -> String {
	if node_path.is_empty() {
		morph_name.to_string()
	} else {
		format!("{node_path}\0{morph_name}")
	}
}

fn morph_override_path_suffix_key(key: &str) -> Option<String> {
	let (path, morph_name) = key.split_once('\0')?;
	let (_, root_relative_path) = path.split_once('/')?;
	Some(format!("{root_relative_path}\0{morph_name}"))
}

fn morph_override_value(overrides: &BTreeMap<String, f32>, key: Option<&String>, suffix_key: Option<&String>, name: &str) -> Option<f32> {
	if let Some(key) = key {
		if let Some(value) = overrides.get(key).copied() {
			return Some(value);
		}
	}
	if let Some(suffix_key) = suffix_key {
		if let Some(value) = overrides.get(suffix_key).copied() {
			return Some(value);
		}
	}
	overrides.get(name).copied()
}

fn fill_morph_weights_for_draw(
	default_morph_weights: &[f32],
	target_count: usize,
	bindings: &[ExpressionBinding],
	expression_values: Option<&[f32]>,
	morph_target_names: &[String],
	morph_target_override_keys: &[String],
	morph_target_override_suffix_keys: &[Option<String>],
	morph_name_overrides: Option<&BTreeMap<String, f32>>,
	out: &mut Vec<f32>,
) {
	out.clear();
	if target_count == 0 {
		return;
	}
	let copy_len = default_morph_weights.len().min(target_count);
	out.extend_from_slice(&default_morph_weights[..copy_len]);
	out.resize(target_count, 0.0);
	if let Some(expression_values) = expression_values {
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
	if let Some(overrides) = morph_name_overrides {
		for (index, name) in morph_target_names.iter().enumerate().take(target_count) {
			let Some(value) = morph_override_value(
				overrides,
				morph_target_override_keys.get(index),
				morph_target_override_suffix_keys.get(index).and_then(Option::as_ref),
				name,
			) else {
				continue;
			};
			if let Some(slot) = out.get_mut(index) {
				*slot = value.clamp(0.0, 1.0);
			}
		}
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

fn morph_names_have_active_override(
	morph_target_names: &[String],
	morph_target_override_keys: &[String],
	morph_target_override_suffix_keys: &[Option<String>],
	morph_name_overrides: Option<&BTreeMap<String, f32>>,
) -> bool {
	let Some(overrides) = morph_name_overrides else {
		return false;
	};
	morph_target_names.iter().enumerate().any(|(index, name)| {
		morph_override_value(
			overrides,
			morph_target_override_keys.get(index),
			morph_target_override_suffix_keys.get(index).and_then(Option::as_ref),
			name,
		)
		.unwrap_or(0.0)
			!= 0.0
	})
}

fn morph_weights_match_default(uploaded: &[f32], default_morph_weights: &[f32], target_count: usize) -> bool {
	if uploaded.len() != target_count {
		return false;
	}
	let copy_len = default_morph_weights.len().min(target_count);
	uploaded[..copy_len] == default_morph_weights[..copy_len] && uploaded[copy_len..].iter().all(|&weight| weight == 0.0)
}

fn scene_default_morph_weights_for_draw(
	scene: &UnaSceneSnapshot,
	mesh_index: usize,
	primitive_index: usize,
	morph_source_indices: &[usize],
) -> Vec<f32> {
	let mut out = vec![0.0; morph_source_indices.len()];
	let Some(primitive) = scene.meshes.get(mesh_index).and_then(|mesh| mesh.get(primitive_index)) else {
		return out;
	};
	for (dst, &source_index) in out.iter_mut().zip(morph_source_indices) {
		*dst = primitive
			.default_morph_weights
			.get(source_index)
			.copied()
			.unwrap_or(0.0)
			.clamp(0.0, 1.0);
	}
	out
}

fn refresh_morph_default_weights(
	default_morph_weights: &mut Vec<f32>,
	uploaded_morph_weights: &mut Vec<f32>,
	scene: &UnaSceneSnapshot,
	mesh_index: usize,
	primitive_index: usize,
	morph_source_indices: &[usize],
) -> bool {
	let next = scene_default_morph_weights_for_draw(scene, mesh_index, primitive_index, morph_source_indices);
	if *default_morph_weights == next {
		return false;
	}
	*default_morph_weights = next;
	uploaded_morph_weights.clear();
	true
}

fn morph_delta_data(morph_pos: &[Vec<[f32; 3]>], morph_nrm: Option<&[Vec<[f32; 3]>]>, vertex_count: usize) -> Vec<[f32; 4]> {
	let mut out = Vec::with_capacity(morph_pos.len().saturating_mul(vertex_count).saturating_mul(2).max(1));
	fill_morph_delta_data(morph_pos, morph_nrm, vertex_count, &mut out);
	out
}

fn fill_morph_delta_data(morph_pos: &[Vec<[f32; 3]>], morph_nrm: Option<&[Vec<[f32; 3]>]>, vertex_count: usize, out: &mut Vec<[f32; 4]>) {
	out.clear();
	out.reserve(morph_pos.len().saturating_mul(vertex_count).saturating_mul(2));
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
}

fn create_morph_resources(
	device: &wgpu::Device,
	queue: &wgpu::Queue,
	layout: &wgpu::BindGroupLayout,
	target_count: u32,
	vertex_count: u32,
	morph_deltas: &[[f32; 4]],
) -> MorphGpuResources {
	let morph_meta = MorphMetaGpu {
		target_count,
		vertex_count,
		_pad: [0; 2],
	};
	let meta_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
		label: Some("mesh_morph_meta"),
		contents: bytemuck::bytes_of(&morph_meta),
		usage: wgpu::BufferUsages::UNIFORM,
	});
	let weight_size = ((target_count as u64) * std::mem::size_of::<f32>() as u64).max(MORPH_WEIGHT_BUFFER_MIN_SIZE);
	let weight_buffer = device.create_buffer(&wgpu::BufferDescriptor {
		label: Some("mesh_morph_weights"),
		size: weight_size,
		usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
		mapped_at_creation: false,
	});
	let delta_size = ((morph_deltas.len() * std::mem::size_of::<[f32; 4]>()) as u64).max(MORPH_DELTA_BUFFER_MIN_SIZE);
	let delta_buffer = device.create_buffer(&wgpu::BufferDescriptor {
		label: Some("mesh_morph_deltas"),
		size: delta_size,
		usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
		mapped_at_creation: false,
	});
	if !morph_deltas.is_empty() {
		queue.write_buffer(&delta_buffer, 0, bytemuck::cast_slice(morph_deltas));
	}
	let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
		label: Some("mesh_morph_bg"),
		layout,
		entries: &[
			wgpu::BindGroupEntry {
				binding: 0,
				resource: meta_buffer.as_entire_binding(),
			},
			wgpu::BindGroupEntry {
				binding: 1,
				resource: weight_buffer.as_entire_binding(),
			},
			wgpu::BindGroupEntry {
				binding: 2,
				resource: delta_buffer.as_entire_binding(),
			},
		],
	});
	MorphGpuResources {
		meta_buffer,
		weight_buffer,
		delta_buffer,
		bind_group,
	}
}

fn create_shared_morph_delta_resources(
	device: &wgpu::Device,
	queue: &wgpu::Queue,
	target_count: u32,
	vertex_count: u32,
	morph_deltas: &[[f32; 4]],
) -> SharedMorphDeltaResources {
	let morph_meta = MorphMetaGpu {
		target_count,
		vertex_count,
		_pad: [0; 2],
	};
	let meta_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
		label: Some("mesh_morph_meta_shared"),
		contents: bytemuck::bytes_of(&morph_meta),
		usage: wgpu::BufferUsages::UNIFORM,
	});
	let delta_size = ((morph_deltas.len() * std::mem::size_of::<[f32; 4]>()) as u64).max(MORPH_DELTA_BUFFER_MIN_SIZE);
	let delta_buffer = device.create_buffer(&wgpu::BufferDescriptor {
		label: Some("mesh_morph_deltas_shared"),
		size: delta_size,
		usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
		mapped_at_creation: false,
	});
	if !morph_deltas.is_empty() {
		queue.write_buffer(&delta_buffer, 0, bytemuck::cast_slice(morph_deltas));
	}
	SharedMorphDeltaResources {
		meta_buffer,
		delta_buffer,
		target_count,
	}
}

fn create_morph_resources_with_shared_deltas(
	device: &wgpu::Device,
	layout: &wgpu::BindGroupLayout,
	shared: &SharedMorphDeltaResources,
) -> MorphGpuResources {
	let weight_size = ((shared.target_count as u64) * std::mem::size_of::<f32>() as u64).max(MORPH_WEIGHT_BUFFER_MIN_SIZE);
	let weight_buffer = device.create_buffer(&wgpu::BufferDescriptor {
		label: Some("mesh_morph_weights"),
		size: weight_size,
		usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
		mapped_at_creation: false,
	});
	let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
		label: Some("mesh_morph_bg"),
		layout,
		entries: &[
			wgpu::BindGroupEntry {
				binding: 0,
				resource: shared.meta_buffer.as_entire_binding(),
			},
			wgpu::BindGroupEntry {
				binding: 1,
				resource: weight_buffer.as_entire_binding(),
			},
			wgpu::BindGroupEntry {
				binding: 2,
				resource: shared.delta_buffer.as_entire_binding(),
			},
		],
	});
	MorphGpuResources {
		meta_buffer: shared.meta_buffer.clone(),
		weight_buffer,
		delta_buffer: shared.delta_buffer.clone(),
		bind_group,
	}
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

fn write_matrix_to_raw_slot(raw: &mut [f32], matrix_index: usize, matrix: Mat4) {
	let start = matrix_index.saturating_mul(16);
	let end = start.saturating_add(16);
	if let Some(slot) = raw.get_mut(start..end) {
		slot.copy_from_slice(&matrix.to_cols_array());
	}
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

fn texture_sampler_or<'a>(
	samplers: &'a [wgpu::Sampler],
	image_sampler_indices: &[usize],
	index: Option<usize>,
	fallback_index: usize,
) -> &'a wgpu::Sampler {
	let sampler_index = index
		.and_then(|image_index| image_sampler_indices.get(image_index).copied())
		.unwrap_or(fallback_index);
	&samplers[sampler_index]
}

fn create_source_image_texture(device: &wgpu::Device, queue: &wgpu::Queue, upload: &SourceTextureUpload) -> wgpu::Texture {
	let tex = device.create_texture(&wgpu::TextureDescriptor {
		label: Some("gltf_image_source"),
		size: wgpu::Extent3d {
			width: upload.width,
			height: upload.height,
			depth_or_array_layers: 1,
		},
		mip_level_count: 1,
		sample_count: 1,
		dimension: wgpu::TextureDimension::D2,
		format: upload.format,
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
		&upload.data,
		wgpu::TexelCopyBufferLayout {
			offset: 0,
			bytes_per_row: Some(upload.bytes_per_row),
			rows_per_image: Some(upload.height),
		},
		wgpu::Extent3d {
			width: upload.width,
			height: upload.height,
			depth_or_array_layers: 1,
		},
	);
	tex
}

fn create_payload_image_texture(
	device: &wgpu::Device,
	queue: &wgpu::Queue,
	payload: &TextureUploadPayload,
	format: wgpu::TextureFormat,
	width: u32,
	height: u32,
) -> wgpu::Texture {
	let tex = device.create_texture(&wgpu::TextureDescriptor {
		label: Some("gltf_image"),
		size: wgpu::Extent3d {
			width,
			height,
			depth_or_array_layers: 1,
		},
		mip_level_count: payload.mips.len() as u32,
		sample_count: 1,
		dimension: wgpu::TextureDimension::D2,
		format,
		usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
		view_formats: &[],
	});
	for (mip_level, mip) in payload.mips.iter().enumerate() {
		let (bytes_per_row, rows_per_image) = texture_mip_copy_layout(payload.kind, mip.width, mip.height);
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
	tex
}

fn texture_mip_copy_layout(kind: TextureUploadKind, width: u32, height: u32) -> (u32, u32) {
	match kind {
		TextureUploadKind::Rgba => (4 * width, height),
		TextureUploadKind::Bc1Srgb => (width.div_ceil(4) * 8, height.div_ceil(4)),
		TextureUploadKind::Bc5Unorm | TextureUploadKind::Bc7Unorm | TextureUploadKind::Bc7Srgb => {
			(width.div_ceil(4) * 16, height.div_ceil(4))
		}
	}
}

fn payload_texture_format(
	kind: TextureUploadKind,
	role: TextureRole,
	source_metadata: Option<&UnaImageSourceMetadata>,
) -> wgpu::TextureFormat {
	match kind {
		TextureUploadKind::Rgba if rgba_upload_uses_linear_format(role, source_metadata) => wgpu::TextureFormat::Rgba8Unorm,
		TextureUploadKind::Rgba => wgpu::TextureFormat::Rgba8UnormSrgb,
		TextureUploadKind::Bc1Srgb => wgpu::TextureFormat::Bc1RgbaUnormSrgb,
		TextureUploadKind::Bc5Unorm => wgpu::TextureFormat::Bc5RgUnorm,
		TextureUploadKind::Bc7Unorm => wgpu::TextureFormat::Bc7RgbaUnorm,
		TextureUploadKind::Bc7Srgb => wgpu::TextureFormat::Bc7RgbaUnormSrgb,
	}
}

fn payload_top_mip_dimensions(payload: &TextureUploadPayload, fallback_width: u32, fallback_height: u32) -> (u32, u32) {
	payload
		.mips
		.first()
		.map_or((fallback_width, fallback_height), |mip| (mip.width, mip.height))
}

fn upload_payload_texture_slot(
	device: &wgpu::Device,
	queue: &wgpu::Queue,
	transparent_black_view: &wgpu::TextureView,
	gpu_texture_compression: &mut Option<GpuTextureCompressionContext>,
	image_views: &mut Vec<wgpu::TextureView>,
	payload: TextureUploadPayload,
	format: wgpu::TextureFormat,
	width: u32,
	height: u32,
) -> (SceneImageTextureSlot, Duration) {
	let mut slot = SceneImageTextureSlot::new(SceneImageTextureUpload::Payload {
		payload,
		format,
		width,
		height,
	});
	let upload_start = Instant::now();
	if let Some(view) = slot.ensure_uploaded(device, queue, None, gpu_texture_compression) {
		image_views.push(view);
	} else {
		image_views.push(transparent_black_view.clone());
	}
	(slot, upload_start.elapsed())
}

fn build_scene_image_texture_upload(
	im: &UnaImageRgba,
	source_metadata: Option<&UnaImageSourceMetadata>,
	lazy: &SceneImageTextureLazyUpload,
	gpu_texture_compression: &mut Option<GpuTextureCompressionContext>,
) -> Option<SceneImageTextureUpload> {
	if lazy.texture_max_dimension.is_none() && lazy.texture_compression != TextureCompressionMode::Compat {
		if let Some(source_upload) = source_texture_upload(im, source_metadata, lazy.role) {
			return Some(SceneImageTextureUpload::Source(source_upload));
		}
	}
	let src_w = im.width.max(1);
	let src_h = im.height.max(1);
	let rgba_compat = im.rgba8_compat_pixels();
	let rgba = rgba_compat.as_ref();
	let source_key = source_metadata.map_or_else(
		|| texture_cache_key(src_w, src_h, lazy.texture_max_dimension, lazy.role, lazy.mipmap_filter, rgba),
		|source| texture_cache_key_from_source_metadata(src_w, src_h, lazy.texture_max_dimension, lazy.role, lazy.mipmap_filter, source),
	);
	let compressed_cache_lookup = lazy.processed_texture_cache.then(|| {
		compressed_cache_lookup_from_source(
			rgba,
			src_w,
			src_h,
			lazy.texture_max_dimension,
			lazy.role,
			lazy.texture_compression,
			&lazy.texture_compression_advanced,
			lazy.texture_compression_bc_supported,
			source_key,
		)
	});
	let compressed_cache_lookup = compressed_cache_lookup.flatten();
	let compressed_cache_hit = compressed_cache_lookup.as_ref().and_then(|lookup| {
		read_compressed_texture_cache(&lookup.path, lookup.key, lookup.kind)
			.map(|payload| (payload, lookup.processed_width, lookup.processed_height))
	});
	let (payload, processed_w, processed_h) = if let Some((payload, processed_w, processed_h)) = compressed_cache_hit {
		(payload, processed_w, processed_h)
	} else {
		let (processed, _) = load_or_build_processed_texture(
			rgba,
			src_w,
			src_h,
			lazy.texture_max_dimension,
			lazy.role,
			lazy.mipmap_filter,
			lazy.processed_texture_cache,
			source_key,
		);
		let processed_w = processed.width;
		let processed_h = processed.height;
		if lazy.gpu_texture_compression_enabled
			&& gpu_texture_compression.is_none()
			&& compressed_upload_kind_for_texture(
				&processed,
				lazy.texture_compression,
				&lazy.texture_compression_advanced,
				lazy.role,
				lazy.texture_compression_bc_supported,
			)
			.is_some()
		{
			*gpu_texture_compression = create_vulkan_gpu_texture_compression_context().ok();
		}
		let (payload, _) = texture_upload_payload(
			processed,
			lazy.texture_compression,
			&lazy.texture_compression_advanced,
			lazy.role,
			lazy.texture_compression_bc_supported,
			lazy.block_compression_encoder,
			lazy.block_compression_cpu_threads,
			gpu_texture_compression.as_mut(),
			lazy.processed_texture_cache,
			compressed_cache_lookup.as_ref(),
			compressed_cache_lookup.is_some(),
		);
		(payload, processed_w, processed_h)
	};
	let (w, h) = payload_top_mip_dimensions(&payload, processed_w, processed_h);
	let format = payload_texture_format(payload.kind, lazy.role, source_metadata);
	Some(SceneImageTextureUpload::Payload {
		payload,
		format,
		width: w,
		height: h,
	})
}

#[allow(clippy::too_many_arguments)]
fn create_mesh_draw_bind_groups(
	device: &wgpu::Device,
	material_layout: &wgpu::BindGroupLayout,
	outline_material_layout: &wgpu::BindGroupLayout,
	shader_variant_tier: MeshShaderVariantTier,
	texture_views: &SceneTextureViews,
	samplers: &[wgpu::Sampler],
	image_sampler_indices: &[usize],
	reflection_cube_sampler: &wgpu::Sampler,
	material: &UnaMaterialPbr,
	draw_transform: &wgpu::Buffer,
	draw_material: &wgpu::Buffer,
) -> (wgpu::BindGroup, wgpu::BindGroup) {
	create_mesh_material_bind_groups(
		device,
		material_layout,
		outline_material_layout,
		shader_variant_tier,
		texture_views,
		samplers,
		image_sampler_indices,
		reflection_cube_sampler,
		MeshMaterialBindingSource {
			material,
			draw_transform,
			draw_material,
		},
	)
}

fn create_mesh_draw_morph_resources(
	device: &wgpu::Device,
	queue: &wgpu::Queue,
	layout: &wgpu::BindGroupLayout,
	draw: &MeshDraw,
) -> MorphGpuResources {
	if draw.morph_target_count > 0 {
		let morph_deltas = morph_delta_data(&draw.morph_pos, draw.morph_nrm.as_deref(), draw.buffer_upload.vertices.len());
		create_morph_resources(
			device,
			queue,
			layout,
			draw.morph_target_count as u32,
			draw.buffer_upload.vertices.len() as u32,
			&morph_deltas,
		)
	} else {
		create_morph_resources(device, queue, layout, 0, 0, &[])
	}
}

#[allow(clippy::too_many_arguments)]
fn create_mesh_material_bind_groups(
	device: &wgpu::Device,
	material_layout: &wgpu::BindGroupLayout,
	outline_material_layout: &wgpu::BindGroupLayout,
	shader_variant_tier: MeshShaderVariantTier,
	texture_views: &SceneTextureViews,
	samplers: &[wgpu::Sampler],
	image_sampler_indices: &[usize],
	reflection_cube_sampler: &wgpu::Sampler,
	source: MeshMaterialBindingSource<'_>,
) -> (wgpu::BindGroup, wgpu::BindGroup) {
	let mat = source.material;
	let default_mtoon = UnaMtoonMaterial::default();
	let mtoon = mat.mtoon_like_runtime().unwrap_or(&default_mtoon);
	let liltoon_like = mat.liltoon_like_runtime();
	let tex_view = texture_view_or(&texture_views.images, mat.base_color_texture_index, &texture_views.white);
	let tex_sampler = texture_sampler_or(samplers, image_sampler_indices, mat.base_color_texture_index, 0);
	let shade_texture_index = liltoon_like
		.and_then(|liltoon_like| liltoon_like.shadow.color_texture_index)
		.or(mtoon.shade_multiply_texture_index);
	let shade_fallback_view = if liltoon_like.is_some() {
		&texture_views.transparent_black
	} else {
		&texture_views.white
	};
	let shade_view = texture_view_or(&texture_views.images, shade_texture_index, shade_fallback_view);
	let shade_sampler = texture_sampler_or(samplers, image_sampler_indices, shade_texture_index, 0);
	let shadow2_color_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.shadow.second_color_texture_index);
	let shadow2_color_view = texture_view_or(&texture_views.images, shadow2_color_texture_index, &texture_views.transparent_black);
	let shadow3_color_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.shadow.third_color_texture_index);
	let shadow3_color_view = texture_view_or(&texture_views.images, shadow3_color_texture_index, &texture_views.transparent_black);
	let liltoon_strength_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.shadow.strength_mask_texture_index);
	let shading_shift_texture_index = liltoon_strength_mask_texture_index.or(mtoon.shading_shift_texture_index);
	let shift_fallback_view = if liltoon_like.is_some() {
		&texture_views.white
	} else {
		&texture_views.black
	};
	let shift_view = texture_view_or(&texture_views.images, shading_shift_texture_index, shift_fallback_view);
	let shift_sampler = texture_sampler_or(samplers, image_sampler_indices, shading_shift_texture_index, 0);
	let shadow_border_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.shadow.border_mask_texture_index);
	let shadow_border_mask_view = texture_view_or(&texture_views.images, shadow_border_mask_texture_index, &texture_views.white);
	let shadow_border_mask_sampler = texture_sampler_or(samplers, image_sampler_indices, shadow_border_mask_texture_index, 0);
	let shadow_blur_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.shadow.blur_mask_texture_index);
	let shadow_blur_mask_view = texture_view_or(&texture_views.images, shadow_blur_mask_texture_index, &texture_views.white);
	let shadow_blur_mask_sampler = texture_sampler_or(samplers, image_sampler_indices, shadow_blur_mask_texture_index, 0);
	let matcap_texture_index = liltoon_like
		.and_then(|liltoon_like| liltoon_like.matcap.texture_index)
		.or(mtoon.matcap_texture_index);
	let matcap_fallback_view = if liltoon_like.is_some() {
		&texture_views.white
	} else {
		&texture_views.black
	};
	let matcap_view = texture_view_or(&texture_views.images, matcap_texture_index, matcap_fallback_view);
	let matcap_sampler = texture_sampler_or(samplers, image_sampler_indices, matcap_texture_index, 0);
	let matcap_blend_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.matcap.blend_mask_texture_index);
	let matcap_blend_mask_view = texture_view_or(&texture_views.images, matcap_blend_mask_texture_index, &texture_views.white);
	let matcap_blend_mask_sampler = texture_sampler_or(samplers, image_sampler_indices, matcap_blend_mask_texture_index, 0);
	let matcap_bump_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.matcap.bump_texture_index);
	let matcap_bump_view = texture_view_or(&texture_views.images, matcap_bump_texture_index, &texture_views.neutral_normal);
	let matcap2_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.matcap.second_texture_index);
	let matcap2_view = texture_view_or(&texture_views.images, matcap2_texture_index, &texture_views.white);
	let matcap2_blend_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.matcap.second_blend_mask_texture_index);
	let matcap2_blend_mask_view = texture_view_or(&texture_views.images, matcap2_blend_mask_texture_index, &texture_views.white);
	let matcap2_bump_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.matcap.second_bump_texture_index);
	let matcap2_bump_view = texture_view_or(&texture_views.images, matcap2_bump_texture_index, &texture_views.neutral_normal);
	let main2nd_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.main_color.second_texture_index);
	let main2nd_view = texture_view_or(&texture_views.images, main2nd_texture_index, &texture_views.white);
	let main2nd_blend_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.main_color.second_blend_mask_texture_index);
	let main2nd_blend_mask_view = texture_view_or(&texture_views.images, main2nd_blend_mask_texture_index, &texture_views.white);
	let main2nd_dissolve_mask_texture_index =
		liltoon_like.and_then(|liltoon_like| liltoon_like.main_color.second_dissolve.mask_texture_index);
	let main2nd_dissolve_mask_view = texture_view_or(&texture_views.images, main2nd_dissolve_mask_texture_index, &texture_views.white);
	let main2nd_dissolve_noise_mask_texture_index =
		liltoon_like.and_then(|liltoon_like| liltoon_like.main_color.second_dissolve.noise_mask_texture_index);
	let main2nd_dissolve_noise_mask_view = texture_view_or(
		&texture_views.images,
		main2nd_dissolve_noise_mask_texture_index,
		&texture_views.white,
	);
	let main3rd_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.main_color.third_texture_index);
	let main3rd_view = texture_view_or(&texture_views.images, main3rd_texture_index, &texture_views.white);
	let main3rd_blend_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.main_color.third_blend_mask_texture_index);
	let main3rd_blend_mask_view = texture_view_or(&texture_views.images, main3rd_blend_mask_texture_index, &texture_views.white);
	let main3rd_dissolve_mask_texture_index =
		liltoon_like.and_then(|liltoon_like| liltoon_like.main_color.third_dissolve.mask_texture_index);
	let main3rd_dissolve_mask_view = texture_view_or(&texture_views.images, main3rd_dissolve_mask_texture_index, &texture_views.white);
	let main3rd_dissolve_noise_mask_texture_index =
		liltoon_like.and_then(|liltoon_like| liltoon_like.main_color.third_dissolve.noise_mask_texture_index);
	let main3rd_dissolve_noise_mask_view = texture_view_or(
		&texture_views.images,
		main3rd_dissolve_noise_mask_texture_index,
		&texture_views.white,
	);
	let main_gradation_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.main_color.gradation_texture_index);
	let main_gradation_view = texture_view_or(&texture_views.images, main_gradation_texture_index, &texture_views.white);
	let main_color_adjust_mask_texture_index =
		liltoon_like.and_then(|liltoon_like| liltoon_like.main_color.main_color_adjust_mask_texture_index);
	let main_color_adjust_mask_view = texture_view_or(&texture_views.images, main_color_adjust_mask_texture_index, &texture_views.white);
	let alpha_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.alpha_mask.texture_index);
	let alpha_mask_view = texture_view_or(&texture_views.images, alpha_mask_texture_index, &texture_views.white);
	let alpha_mask_sampler = texture_sampler_or(samplers, image_sampler_indices, alpha_mask_texture_index, 0);
	let rim_texture_index = liltoon_like
		.and_then(|liltoon_like| liltoon_like.rim.texture_index)
		.or(mtoon.rim_multiply_texture_index);
	let rim_view = texture_view_or(&texture_views.images, rim_texture_index, &texture_views.white);
	let rim_sampler = texture_sampler_or(samplers, image_sampler_indices, rim_texture_index, 0);
	let rim_shade_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.rim.shade_mask_texture_index);
	let rim_shade_mask_view = texture_view_or(&texture_views.images, rim_shade_mask_texture_index, &texture_views.white);
	let backlight_color_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.backlight.texture_index);
	let glitter_color_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.glitter.color_texture_index);
	let glitter_shape_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.glitter.shape_texture_index);
	let dissolve_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.dissolve.mask_texture_index);
	let dissolve_noise_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.dissolve.noise_mask_texture_index);
	let parallax_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.parallax.texture_index);
	let backlight_color_view = texture_view_or(&texture_views.images, backlight_color_texture_index, &texture_views.white);
	let glitter_color_view = texture_view_or(&texture_views.images, glitter_color_texture_index, &texture_views.white);
	let glitter_shape_view = texture_view_or(&texture_views.images, glitter_shape_texture_index, &texture_views.white);
	let dissolve_mask_view = texture_view_or(&texture_views.images, dissolve_mask_texture_index, &texture_views.white);
	let dissolve_noise_mask_view = texture_view_or(&texture_views.images, dissolve_noise_mask_texture_index, &texture_views.white);
	let parallax_view = texture_view_or(&texture_views.images, parallax_texture_index, &texture_views.white);
	let reflection_texture_index = if let Some(liltoon_like) = liltoon_like {
		liltoon_reflection_texture_index(liltoon_like)
	} else {
		mtoon.reflection_cube_texture_index
	};
	let reflection_view = reflection_texture_index
		.and_then(|index| texture_views.cubes.get(index).and_then(Option::as_ref))
		.unwrap_or(&texture_views.black_cube);
	let reflection_sampler = reflection_cube_sampler;
	let reflection_color_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.reflection.color_texture_index);
	let reflection_color_view = texture_view_or(&texture_views.images, reflection_color_texture_index, &texture_views.white);
	let reflection_color_sampler = texture_sampler_or(samplers, image_sampler_indices, reflection_color_texture_index, 0);
	let smoothness_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.reflection.smoothness_texture_index);
	let smoothness_view = texture_view_or(&texture_views.images, smoothness_texture_index, &texture_views.white);
	let smoothness_sampler = texture_sampler_or(samplers, image_sampler_indices, smoothness_texture_index, 0);
	let metallic_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.reflection.metallic_texture_index);
	let metallic_view = texture_view_or(&texture_views.images, metallic_texture_index, &texture_views.white);
	let metallic_sampler = texture_sampler_or(samplers, image_sampler_indices, metallic_texture_index, 0);
	let emissive_texture_index = liltoon_like
		.and_then(|liltoon_like| liltoon_like.emission.texture_index)
		.or(mat.emissive_texture_index);
	let emissive_fallback_view = if liltoon_like.is_some() {
		&texture_views.white
	} else {
		&texture_views.black
	};
	let emissive_view = texture_view_or(&texture_views.images, emissive_texture_index, emissive_fallback_view);
	let emissive_sampler = texture_sampler_or(samplers, image_sampler_indices, emissive_texture_index, 0);
	let emission_blend_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.emission.blend_mask_texture_index);
	let emission_blend_mask_view = texture_view_or(&texture_views.images, emission_blend_mask_texture_index, &texture_views.white);
	let emission_gradation_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.emission.gradation_texture_index);
	let emission_gradation_view = texture_view_or(&texture_views.images, emission_gradation_texture_index, &texture_views.white);
	let emission2nd_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.emission.second_texture_index);
	let emission2nd_view = texture_view_or(&texture_views.images, emission2nd_texture_index, &texture_views.white);
	let emission2nd_blend_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.emission.second_blend_mask_texture_index);
	let emission2nd_blend_mask_view = texture_view_or(&texture_views.images, emission2nd_blend_mask_texture_index, &texture_views.white);
	let emission2nd_gradation_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.emission.second_gradation_texture_index);
	let emission2nd_gradation_view = texture_view_or(&texture_views.images, emission2nd_gradation_texture_index, &texture_views.white);
	let occlusion_view = texture_view_or(&texture_views.images, mat.occlusion_texture_index, &texture_views.white);
	let occlusion_sampler = texture_sampler_or(samplers, image_sampler_indices, mat.occlusion_texture_index, 0);
	let outline_width_mask_texture_index = liltoon_like
		.and_then(|liltoon_like| liltoon_like.outline.width_mask_texture_index)
		.or(mtoon.outline_width_multiply_texture_index);
	let outline_view = texture_view_or(&texture_views.images, outline_width_mask_texture_index, &texture_views.white);
	let outline_sampler = texture_sampler_or(samplers, image_sampler_indices, outline_width_mask_texture_index, 0);
	let outline_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.outline.texture_index);
	let outline_color_view = texture_view_or(&texture_views.images, outline_texture_index, &texture_views.white);
	let uv_mask_view = texture_view_or(&texture_views.images, mtoon.uv_animation_mask_texture_index, &texture_views.white);
	let uv_mask_sampler = texture_sampler_or(samplers, image_sampler_indices, mtoon.uv_animation_mask_texture_index, 0);
	let normal_view = texture_view_or(&texture_views.images, mat.normal_texture_index, &texture_views.neutral_normal);
	let normal_sampler = texture_sampler_or(samplers, image_sampler_indices, mat.normal_texture_index, 0);
	let normal2nd_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.normal.second_texture_index);
	let normal2nd_view = texture_view_or(&texture_views.images, normal2nd_texture_index, &texture_views.neutral_normal);
	let normal2nd_scale_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.normal.second_scale_mask_texture_index);
	let normal2nd_scale_mask_view = texture_view_or(&texture_views.images, normal2nd_scale_mask_texture_index, &texture_views.white);
	let anisotropy_tangent_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.reflection.anisotropy_tangent_texture_index);
	let anisotropy_tangent_view = texture_view_or(
		&texture_views.images,
		anisotropy_tangent_texture_index,
		&texture_views.neutral_normal,
	);
	let anisotropy_scale_mask_texture_index =
		liltoon_like.and_then(|liltoon_like| liltoon_like.reflection.anisotropy_scale_mask_texture_index);
	let anisotropy_scale_mask_view = texture_view_or(&texture_views.images, anisotropy_scale_mask_texture_index, &texture_views.white);
	let anisotropy_shift_noise_texture_index =
		liltoon_like.and_then(|liltoon_like| liltoon_like.reflection.anisotropy_shift_noise_mask_texture_index);
	let anisotropy_shift_noise_view = texture_view_or(&texture_views.images, anisotropy_shift_noise_texture_index, &texture_views.white);
	let fur_vector_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.fur.vector_texture_index);
	let fur_vector_view = texture_view_or(&texture_views.images, fur_vector_texture_index, &texture_views.neutral_vector);
	let fur_length_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.fur.length_mask_texture_index);
	let fur_length_mask_view = texture_view_or(&texture_views.images, fur_length_mask_texture_index, &texture_views.white);
	let fur_noise_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.fur.noise_mask_texture_index);
	let fur_noise_mask_view = texture_view_or(&texture_views.images, fur_noise_mask_texture_index, &texture_views.white);
	let fur_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.fur.mask_texture_index);
	let fur_mask_view = texture_view_or(&texture_views.images, fur_mask_texture_index, &texture_views.white);
	let audio_link_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.audio_link.mask_texture_index);
	let audio_link_mask_view = texture_view_or(&texture_views.images, audio_link_mask_texture_index, &texture_views.blue);
	let audio_link_local_map_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.audio_link.local_map_texture_index);
	let audio_link_local_map_view = texture_view_or(&texture_views.images, audio_link_local_map_texture_index, &texture_views.black);

	let mut bind_material_entries = vec![
		wgpu::BindGroupEntry {
			binding: 0,
			resource: source.draw_transform.as_entire_binding(),
		},
		wgpu::BindGroupEntry {
			binding: 10,
			resource: source.draw_material.as_entire_binding(),
		},
		wgpu::BindGroupEntry {
			binding: 1,
			resource: wgpu::BindingResource::TextureView(tex_view),
		},
		wgpu::BindGroupEntry {
			binding: 2,
			resource: wgpu::BindingResource::Sampler(tex_sampler),
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
		wgpu::BindGroupEntry {
			binding: 14,
			resource: wgpu::BindingResource::Sampler(shade_sampler),
		},
		wgpu::BindGroupEntry {
			binding: 15,
			resource: wgpu::BindingResource::Sampler(shift_sampler),
		},
		wgpu::BindGroupEntry {
			binding: 16,
			resource: wgpu::BindingResource::Sampler(matcap_sampler),
		},
		wgpu::BindGroupEntry {
			binding: 17,
			resource: wgpu::BindingResource::Sampler(rim_sampler),
		},
		wgpu::BindGroupEntry {
			binding: 18,
			resource: wgpu::BindingResource::Sampler(emissive_sampler),
		},
		wgpu::BindGroupEntry {
			binding: 20,
			resource: wgpu::BindingResource::Sampler(normal_sampler),
		},
		wgpu::BindGroupEntry {
			binding: 21,
			resource: wgpu::BindingResource::Sampler(occlusion_sampler),
		},
		wgpu::BindGroupEntry {
			binding: 22,
			resource: wgpu::BindingResource::Sampler(reflection_sampler),
		},
		wgpu::BindGroupEntry {
			binding: 23,
			resource: wgpu::BindingResource::Sampler(uv_mask_sampler),
		},
		wgpu::BindGroupEntry {
			binding: 28,
			resource: wgpu::BindingResource::TextureView(reflection_color_view),
		},
		wgpu::BindGroupEntry {
			binding: 29,
			resource: wgpu::BindingResource::TextureView(smoothness_view),
		},
		wgpu::BindGroupEntry {
			binding: 30,
			resource: wgpu::BindingResource::TextureView(metallic_view),
		},
		wgpu::BindGroupEntry {
			binding: 31,
			resource: wgpu::BindingResource::Sampler(reflection_color_sampler),
		},
		wgpu::BindGroupEntry {
			binding: 32,
			resource: wgpu::BindingResource::Sampler(smoothness_sampler),
		},
		wgpu::BindGroupEntry {
			binding: 33,
			resource: wgpu::BindingResource::Sampler(metallic_sampler),
		},
		wgpu::BindGroupEntry {
			binding: 34,
			resource: wgpu::BindingResource::TextureView(matcap_blend_mask_view),
		},
		wgpu::BindGroupEntry {
			binding: 35,
			resource: wgpu::BindingResource::Sampler(matcap_blend_mask_sampler),
		},
		wgpu::BindGroupEntry {
			binding: 36,
			resource: wgpu::BindingResource::TextureView(alpha_mask_view),
		},
		wgpu::BindGroupEntry {
			binding: 37,
			resource: wgpu::BindingResource::Sampler(alpha_mask_sampler),
		},
	];
	if shader_variant_tier.is_high_capability() {
		bind_material_entries.extend([
			wgpu::BindGroupEntry {
				binding: 19,
				resource: wgpu::BindingResource::Sampler(outline_sampler),
			},
			wgpu::BindGroupEntry {
				binding: 24,
				resource: wgpu::BindingResource::TextureView(shadow_border_mask_view),
			},
			wgpu::BindGroupEntry {
				binding: 25,
				resource: wgpu::BindingResource::TextureView(shadow_blur_mask_view),
			},
			wgpu::BindGroupEntry {
				binding: 26,
				resource: wgpu::BindingResource::Sampler(shadow_border_mask_sampler),
			},
			wgpu::BindGroupEntry {
				binding: 27,
				resource: wgpu::BindingResource::Sampler(shadow_blur_mask_sampler),
			},
			wgpu::BindGroupEntry {
				binding: 38,
				resource: wgpu::BindingResource::TextureView(matcap2_view),
			},
			wgpu::BindGroupEntry {
				binding: 39,
				resource: wgpu::BindingResource::TextureView(matcap2_blend_mask_view),
			},
			wgpu::BindGroupEntry {
				binding: 41,
				resource: wgpu::BindingResource::TextureView(main2nd_view),
			},
			wgpu::BindGroupEntry {
				binding: 42,
				resource: wgpu::BindingResource::TextureView(main3rd_view),
			},
			wgpu::BindGroupEntry {
				binding: 43,
				resource: wgpu::BindingResource::TextureView(main2nd_blend_mask_view),
			},
			wgpu::BindGroupEntry {
				binding: 44,
				resource: wgpu::BindingResource::TextureView(main3rd_blend_mask_view),
			},
			wgpu::BindGroupEntry {
				binding: 45,
				resource: wgpu::BindingResource::TextureView(normal2nd_view),
			},
			wgpu::BindGroupEntry {
				binding: 46,
				resource: wgpu::BindingResource::TextureView(emission_gradation_view),
			},
			wgpu::BindGroupEntry {
				binding: 47,
				resource: wgpu::BindingResource::TextureView(main_gradation_view),
			},
			wgpu::BindGroupEntry {
				binding: 48,
				resource: wgpu::BindingResource::TextureView(emission2nd_view),
			},
			wgpu::BindGroupEntry {
				binding: 49,
				resource: wgpu::BindingResource::TextureView(emission2nd_blend_mask_view),
			},
			wgpu::BindGroupEntry {
				binding: 50,
				resource: wgpu::BindingResource::TextureView(emission2nd_gradation_view),
			},
			wgpu::BindGroupEntry {
				binding: 51,
				resource: wgpu::BindingResource::TextureView(anisotropy_tangent_view),
			},
			wgpu::BindGroupEntry {
				binding: 52,
				resource: wgpu::BindingResource::TextureView(anisotropy_scale_mask_view),
			},
			wgpu::BindGroupEntry {
				binding: 53,
				resource: wgpu::BindingResource::TextureView(anisotropy_shift_noise_view),
			},
			wgpu::BindGroupEntry {
				binding: 54,
				resource: wgpu::BindingResource::TextureView(emission_blend_mask_view),
			},
			wgpu::BindGroupEntry {
				binding: 55,
				resource: wgpu::BindingResource::TextureView(rim_shade_mask_view),
			},
			wgpu::BindGroupEntry {
				binding: 56,
				resource: wgpu::BindingResource::TextureView(backlight_color_view),
			},
			wgpu::BindGroupEntry {
				binding: 57,
				resource: wgpu::BindingResource::TextureView(shadow2_color_view),
			},
			wgpu::BindGroupEntry {
				binding: 58,
				resource: wgpu::BindingResource::TextureView(shadow3_color_view),
			},
			wgpu::BindGroupEntry {
				binding: 59,
				resource: wgpu::BindingResource::TextureView(fur_vector_view),
			},
			wgpu::BindGroupEntry {
				binding: 60,
				resource: wgpu::BindingResource::TextureView(fur_length_mask_view),
			},
			wgpu::BindGroupEntry {
				binding: 61,
				resource: wgpu::BindingResource::TextureView(fur_noise_mask_view),
			},
			wgpu::BindGroupEntry {
				binding: 62,
				resource: wgpu::BindingResource::TextureView(fur_mask_view),
			},
			wgpu::BindGroupEntry {
				binding: 63,
				resource: wgpu::BindingResource::TextureView(glitter_color_view),
			},
			wgpu::BindGroupEntry {
				binding: 64,
				resource: wgpu::BindingResource::TextureView(glitter_shape_view),
			},
			wgpu::BindGroupEntry {
				binding: 65,
				resource: wgpu::BindingResource::TextureView(dissolve_mask_view),
			},
			wgpu::BindGroupEntry {
				binding: 66,
				resource: wgpu::BindingResource::TextureView(dissolve_noise_mask_view),
			},
			wgpu::BindGroupEntry {
				binding: 67,
				resource: wgpu::BindingResource::TextureView(parallax_view),
			},
			wgpu::BindGroupEntry {
				binding: 68,
				resource: wgpu::BindingResource::TextureView(main2nd_dissolve_mask_view),
			},
			wgpu::BindGroupEntry {
				binding: 69,
				resource: wgpu::BindingResource::TextureView(main2nd_dissolve_noise_mask_view),
			},
			wgpu::BindGroupEntry {
				binding: 70,
				resource: wgpu::BindingResource::TextureView(main3rd_dissolve_mask_view),
			},
			wgpu::BindGroupEntry {
				binding: 71,
				resource: wgpu::BindingResource::TextureView(main3rd_dissolve_noise_mask_view),
			},
			wgpu::BindGroupEntry {
				binding: 72,
				resource: wgpu::BindingResource::TextureView(normal2nd_scale_mask_view),
			},
			wgpu::BindGroupEntry {
				binding: 73,
				resource: wgpu::BindingResource::TextureView(matcap_bump_view),
			},
			wgpu::BindGroupEntry {
				binding: 74,
				resource: wgpu::BindingResource::TextureView(matcap2_bump_view),
			},
			wgpu::BindGroupEntry {
				binding: 75,
				resource: wgpu::BindingResource::TextureView(main_color_adjust_mask_view),
			},
			wgpu::BindGroupEntry {
				binding: 76,
				resource: wgpu::BindingResource::TextureView(audio_link_mask_view),
			},
			wgpu::BindGroupEntry {
				binding: 77,
				resource: wgpu::BindingResource::TextureView(audio_link_local_map_view),
			},
		]);
	}
	let bind_material = device.create_bind_group(&wgpu::BindGroupDescriptor {
		label: Some("mesh_mat"),
		layout: material_layout,
		entries: &bind_material_entries,
	});
	let bind_outline_material = device.create_bind_group(&wgpu::BindGroupDescriptor {
		label: Some("mesh_outline_mat"),
		layout: outline_material_layout,
		entries: &[
			wgpu::BindGroupEntry {
				binding: 0,
				resource: source.draw_transform.as_entire_binding(),
			},
			wgpu::BindGroupEntry {
				binding: 1,
				resource: wgpu::BindingResource::TextureView(tex_view),
			},
			wgpu::BindGroupEntry {
				binding: 2,
				resource: wgpu::BindingResource::Sampler(tex_sampler),
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
				binding: 10,
				resource: source.draw_material.as_entire_binding(),
			},
			wgpu::BindGroupEntry {
				binding: 19,
				resource: wgpu::BindingResource::Sampler(outline_sampler),
			},
			wgpu::BindGroupEntry {
				binding: 23,
				resource: wgpu::BindingResource::Sampler(uv_mask_sampler),
			},
			wgpu::BindGroupEntry {
				binding: 36,
				resource: wgpu::BindingResource::TextureView(alpha_mask_view),
			},
			wgpu::BindGroupEntry {
				binding: 37,
				resource: wgpu::BindingResource::Sampler(alpha_mask_sampler),
			},
			wgpu::BindGroupEntry {
				binding: 40,
				resource: wgpu::BindingResource::TextureView(outline_color_view),
			},
			wgpu::BindGroupEntry {
				binding: 76,
				resource: wgpu::BindingResource::TextureView(audio_link_mask_view),
			},
			wgpu::BindGroupEntry {
				binding: 77,
				resource: wgpu::BindingResource::TextureView(audio_link_local_map_view),
			},
		],
	});
	(bind_material, bind_outline_material)
}

fn sampler_bind_group_layout_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
	wgpu::BindGroupLayoutEntry {
		binding,
		visibility,
		ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
		count: None,
	}
}

fn texture_bind_group_layout_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
	texture_bind_group_layout_entry_with_dimension(binding, visibility, wgpu::TextureViewDimension::D2)
}

fn texture_bind_group_layout_entry_with_dimension(
	binding: u32,
	visibility: wgpu::ShaderStages,
	view_dimension: wgpu::TextureViewDimension,
) -> wgpu::BindGroupLayoutEntry {
	wgpu::BindGroupLayoutEntry {
		binding,
		visibility,
		ty: wgpu::BindingType::Texture {
			multisampled: false,
			view_dimension,
			sample_type: wgpu::TextureSampleType::Float { filterable: true },
		},
		count: None,
	}
}

fn uniform_bind_group_layout_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
	wgpu::BindGroupLayoutEntry {
		binding,
		visibility,
		ty: wgpu::BindingType::Buffer {
			ty: wgpu::BufferBindingType::Uniform,
			has_dynamic_offset: false,
			min_binding_size: None,
		},
		count: None,
	}
}

fn storage_bind_group_layout_entry(binding: u32, visibility: wgpu::ShaderStages, read_only: bool) -> wgpu::BindGroupLayoutEntry {
	wgpu::BindGroupLayoutEntry {
		binding,
		visibility,
		ty: wgpu::BindingType::Buffer {
			ty: wgpu::BufferBindingType::Storage { read_only },
			has_dynamic_offset: false,
			min_binding_size: None,
		},
		count: None,
	}
}

fn mesh_material_layout_entries(variant_tier: MeshShaderVariantTier) -> Vec<wgpu::BindGroupLayoutEntry> {
	let mut entries = vec![
		uniform_bind_group_layout_entry(0, wgpu::ShaderStages::VERTEX),
		texture_bind_group_layout_entry(1, wgpu::ShaderStages::FRAGMENT),
		sampler_bind_group_layout_entry(2, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
		texture_bind_group_layout_entry(3, wgpu::ShaderStages::FRAGMENT),
		texture_bind_group_layout_entry(4, wgpu::ShaderStages::FRAGMENT),
		texture_bind_group_layout_entry(5, wgpu::ShaderStages::FRAGMENT),
		texture_bind_group_layout_entry(6, wgpu::ShaderStages::FRAGMENT),
		texture_bind_group_layout_entry(7, wgpu::ShaderStages::FRAGMENT),
		texture_bind_group_layout_entry(9, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
		uniform_bind_group_layout_entry(10, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
		texture_bind_group_layout_entry(11, wgpu::ShaderStages::FRAGMENT),
		texture_bind_group_layout_entry(12, wgpu::ShaderStages::FRAGMENT),
		texture_bind_group_layout_entry_with_dimension(13, wgpu::ShaderStages::FRAGMENT, wgpu::TextureViewDimension::Cube),
		sampler_bind_group_layout_entry(14, wgpu::ShaderStages::FRAGMENT),
		sampler_bind_group_layout_entry(15, wgpu::ShaderStages::FRAGMENT),
		sampler_bind_group_layout_entry(16, wgpu::ShaderStages::FRAGMENT),
		sampler_bind_group_layout_entry(17, wgpu::ShaderStages::FRAGMENT),
		sampler_bind_group_layout_entry(18, wgpu::ShaderStages::FRAGMENT),
		sampler_bind_group_layout_entry(20, wgpu::ShaderStages::FRAGMENT),
		sampler_bind_group_layout_entry(21, wgpu::ShaderStages::FRAGMENT),
		sampler_bind_group_layout_entry(22, wgpu::ShaderStages::FRAGMENT),
		sampler_bind_group_layout_entry(23, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
		texture_bind_group_layout_entry(28, wgpu::ShaderStages::FRAGMENT),
		texture_bind_group_layout_entry(29, wgpu::ShaderStages::FRAGMENT),
		texture_bind_group_layout_entry(30, wgpu::ShaderStages::FRAGMENT),
		sampler_bind_group_layout_entry(31, wgpu::ShaderStages::FRAGMENT),
		sampler_bind_group_layout_entry(32, wgpu::ShaderStages::FRAGMENT),
		sampler_bind_group_layout_entry(33, wgpu::ShaderStages::FRAGMENT),
		texture_bind_group_layout_entry(34, wgpu::ShaderStages::FRAGMENT),
		sampler_bind_group_layout_entry(35, wgpu::ShaderStages::FRAGMENT),
		texture_bind_group_layout_entry(36, wgpu::ShaderStages::FRAGMENT),
		sampler_bind_group_layout_entry(37, wgpu::ShaderStages::FRAGMENT),
	];
	if variant_tier.is_high_capability() {
		entries.extend([
			texture_bind_group_layout_entry(24, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(25, wgpu::ShaderStages::FRAGMENT),
			sampler_bind_group_layout_entry(19, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
			sampler_bind_group_layout_entry(26, wgpu::ShaderStages::FRAGMENT),
			sampler_bind_group_layout_entry(27, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(38, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(39, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(41, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(42, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(43, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(44, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(45, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(46, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(47, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(48, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(49, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(50, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(51, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(52, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(53, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(54, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(55, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(56, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(57, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(58, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(59, wgpu::ShaderStages::VERTEX),
			texture_bind_group_layout_entry(60, wgpu::ShaderStages::VERTEX),
			texture_bind_group_layout_entry(61, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(62, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(63, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(64, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(65, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(66, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(67, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(68, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(69, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(70, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(71, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(72, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(73, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(74, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(75, wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(76, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
			texture_bind_group_layout_entry(77, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
		]);
	}
	entries
}

fn mesh_outline_material_layout_entries() -> Vec<wgpu::BindGroupLayoutEntry> {
	vec![
		uniform_bind_group_layout_entry(0, wgpu::ShaderStages::VERTEX),
		texture_bind_group_layout_entry(1, wgpu::ShaderStages::FRAGMENT),
		sampler_bind_group_layout_entry(2, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
		texture_bind_group_layout_entry(8, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
		texture_bind_group_layout_entry(9, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
		uniform_bind_group_layout_entry(10, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
		sampler_bind_group_layout_entry(19, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
		sampler_bind_group_layout_entry(23, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
		texture_bind_group_layout_entry(36, wgpu::ShaderStages::FRAGMENT),
		sampler_bind_group_layout_entry(37, wgpu::ShaderStages::FRAGMENT),
		texture_bind_group_layout_entry(40, wgpu::ShaderStages::FRAGMENT),
		texture_bind_group_layout_entry(76, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
		texture_bind_group_layout_entry(77, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
	]
}

fn create_mesh_sampler(device: &wgpu::Device, label: &'static str, sampler: &UnaTextureSampler) -> wgpu::Sampler {
	let mag_filter = wgpu_filter_mode(sampler.mag_filter);
	let min_filter = wgpu_filter_mode(sampler.min_filter);
	device.create_sampler(&wgpu::SamplerDescriptor {
		label: Some(label),
		address_mode_u: wgpu_address_mode(sampler.wrap_s),
		address_mode_v: wgpu_address_mode(sampler.wrap_t),
		address_mode_w: wgpu::AddressMode::ClampToEdge,
		mag_filter,
		min_filter,
		mipmap_filter: if sampler.min_filter == UnaTextureFilterMode::Nearest {
			wgpu::MipmapFilterMode::Nearest
		} else {
			wgpu::MipmapFilterMode::Linear
		},
		anisotropy_clamp: if mag_filter == wgpu::FilterMode::Linear && min_filter == wgpu::FilterMode::Linear {
			4
		} else {
			1
		},
		..Default::default()
	})
}

fn liltoon_reflection_cube_sampler_descriptor(label: &'static str) -> wgpu::SamplerDescriptor<'static> {
	wgpu::SamplerDescriptor {
		label: Some(label),
		address_mode_u: wgpu::AddressMode::Repeat,
		address_mode_v: wgpu::AddressMode::Repeat,
		address_mode_w: wgpu::AddressMode::Repeat,
		mag_filter: wgpu::FilterMode::Linear,
		min_filter: wgpu::FilterMode::Linear,
		mipmap_filter: wgpu::MipmapFilterMode::Linear,
		anisotropy_clamp: 4,
		..Default::default()
	}
}

fn wgpu_address_mode(mode: UnaTextureWrapMode) -> wgpu::AddressMode {
	match mode {
		UnaTextureWrapMode::ClampToEdge | UnaTextureWrapMode::MirrorOnce => wgpu::AddressMode::ClampToEdge,
		UnaTextureWrapMode::MirroredRepeat => wgpu::AddressMode::MirrorRepeat,
		UnaTextureWrapMode::Repeat => wgpu::AddressMode::Repeat,
	}
}

fn wgpu_filter_mode(mode: UnaTextureFilterMode) -> wgpu::FilterMode {
	match mode {
		UnaTextureFilterMode::Nearest => wgpu::FilterMode::Nearest,
		UnaTextureFilterMode::Linear => wgpu::FilterMode::Linear,
	}
}

fn rgba_upload_uses_linear_format(role: TextureRole, source: Option<&UnaImageSourceMetadata>) -> bool {
	if matches!(role, TextureRole::Normal | TextureRole::Occlusion) {
		return true;
	}
	if matches!(role, TextureRole::Data) && source.is_none() {
		return true;
	}
	if let Some(source) = source {
		if source.srgb == Some(false) {
			return true;
		}
		if source
			.color_space
			.as_deref()
			.is_some_and(|value| value.eq_ignore_ascii_case("linear") || value.eq_ignore_ascii_case("data"))
		{
			return true;
		}
	}
	false
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

fn push_solid_texture_1x1_view(
	textures: &mut Vec<wgpu::Texture>,
	device: &wgpu::Device,
	queue: &wgpu::Queue,
	label: &'static str,
	format: wgpu::TextureFormat,
	rgba: [u8; 4],
) -> wgpu::TextureView {
	let texture = create_solid_texture_1x1(device, queue, label, format, rgba);
	let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
	textures.push(texture);
	view
}

fn create_solid_cube_texture_1x1(
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
			depth_or_array_layers: 6,
		},
		mip_level_count: 1,
		sample_count: 1,
		dimension: wgpu::TextureDimension::D2,
		format,
		usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
		view_formats: &[],
	});
	let mut data = Vec::with_capacity(6 * 4);
	for _ in 0..6 {
		data.extend_from_slice(&rgba);
	}
	queue.write_texture(
		texture.as_image_copy(),
		&data,
		wgpu::TexelCopyBufferLayout {
			offset: 0,
			bytes_per_row: Some(4),
			rows_per_image: Some(1),
		},
		wgpu::Extent3d {
			width: 1,
			height: 1,
			depth_or_array_layers: 6,
		},
	);
	texture
}

fn create_cube_texture_from_upload(device: &wgpu::Device, queue: &wgpu::Queue, upload: &CubeUpload) -> wgpu::Texture {
	let tex = device.create_texture(&wgpu::TextureDescriptor {
		label: Some("gltf_image_cube"),
		size: wgpu::Extent3d {
			width: upload.face_size,
			height: upload.face_size,
			depth_or_array_layers: 6,
		},
		mip_level_count: upload.mips.len() as u32,
		sample_count: 1,
		dimension: wgpu::TextureDimension::D2,
		format: wgpu::TextureFormat::Rgba16Float,
		usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
		view_formats: &[],
	});
	for (mip_level, mip) in upload.mips.iter().enumerate() {
		queue.write_texture(
			wgpu::TexelCopyTextureInfo {
				texture: &tex,
				mip_level: mip_level as u32,
				origin: wgpu::Origin3d::ZERO,
				aspect: wgpu::TextureAspect::All,
			},
			&mip.data_rgba16f,
			wgpu::TexelCopyBufferLayout {
				offset: 0,
				bytes_per_row: Some(mip.face_size * 8),
				rows_per_image: Some(mip.face_size),
			},
			wgpu::Extent3d {
				width: mip.face_size,
				height: mip.face_size,
				depth_or_array_layers: 6,
			},
		);
	}
	tex
}

#[derive(Clone)]
struct CubeUpload {
	face_size: u32,
	mips: Vec<CubeMipUpload>,
	layout: &'static str,
}

#[derive(Clone, Copy, Debug, Default)]
struct CubeUploadCacheEvent {
	hit: bool,
	miss: bool,
	write: bool,
}

#[derive(Clone)]
struct CubeMipUpload {
	face_size: u32,
	data_rgba16f: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CubeSourceLayout {
	Latlong,
	SphereMap,
	HorizontalStrip,
	VerticalStrip,
	HorizontalCross,
	VerticalCross,
}

impl CubeSourceLayout {
	fn name(self) -> &'static str {
		match self {
			Self::Latlong => "latlong",
			Self::SphereMap => "sphere_map",
			Self::HorizontalStrip => "horizontal_strip",
			Self::VerticalStrip => "vertical_strip",
			Self::HorizontalCross => "horizontal_cross",
			Self::VerticalCross => "vertical_cross",
		}
	}
}

impl CubeSourceLayout {
	fn cache_tag(self) -> u8 {
		match self {
			Self::Latlong => 1,
			Self::SphereMap => 2,
			Self::HorizontalStrip => 3,
			Self::VerticalStrip => 4,
			Self::HorizontalCross => 5,
			Self::VerticalCross => 6,
		}
	}
}

const CUBE_TEXTURE_CACHE_MAGIC: &[u8; 8] = b"UNACUB1\0";
const CUBE_TEXTURE_CACHE_VERSION: u64 = 1;
const CUBE_CACHE_FNV64_OFFSET: u64 = 0xcbf29ce484222325;
const CUBE_CACHE_FNV64_PRIME: u64 = 0x100000001b3;

fn cube_cache_hash_update(mut hash: u64, bytes: &[u8]) -> u64 {
	for byte in bytes {
		hash ^= u64::from(*byte);
		hash = hash.wrapping_mul(CUBE_CACHE_FNV64_PRIME);
	}
	hash
}

fn cube_texture_cache_key(
	image: &UnaImageRgba,
	source: &UnaImageSourceMetadata,
	layout: CubeSourceLayout,
	face_size: u32,
	srgb: bool,
) -> u64 {
	let mut hash = CUBE_CACHE_FNV64_OFFSET;
	hash = cube_cache_hash_update(hash, b"un-avatar-cube-texture-cache");
	hash = cube_cache_hash_update(hash, &CUBE_TEXTURE_CACHE_VERSION.to_le_bytes());
	hash = cube_cache_hash_update(hash, &image.width.to_le_bytes());
	hash = cube_cache_hash_update(hash, &image.height.to_le_bytes());
	hash = cube_cache_hash_update(hash, &face_size.to_le_bytes());
	hash = cube_cache_hash_update(hash, &[layout.cache_tag(), u8::from(srgb)]);
	hash = cube_cache_hash_update(hash, &source.byte_length.to_le_bytes());
	hash = cube_cache_hash_update(hash, &source.source_hash.to_le_bytes());
	for value in [
		source.mime_type.as_deref(),
		source.uri.as_deref(),
		source.texture_shape.as_deref(),
		source.source_layout.as_deref(),
		source.unity_generate_cubemap.as_deref(),
	]
	.into_iter()
	.flatten()
	{
		hash = cube_cache_hash_update(hash, value.as_bytes());
		hash = cube_cache_hash_update(hash, &[0]);
	}
	hash
}

fn cube_texture_cache_dir() -> Option<PathBuf> {
	if let Some(path) = std::env::var_os("UN_AVATAR_TEXTURE_CACHE_DIR") {
		return Some(PathBuf::from(path));
	}
	#[cfg(windows)]
	{
		std::env::var_os("LOCALAPPDATA")
			.map(PathBuf::from)
			.map(|p| p.join("UN Avatar").join("texture-cache").join("v1"))
	}
	#[cfg(not(windows))]
	{
		std::env::var_os("XDG_CACHE_HOME")
			.map(PathBuf::from)
			.or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
			.map(|p| p.join("un-avatar").join("texture-cache").join("v1"))
	}
}

fn cube_texture_cache_path(key: u64) -> Option<PathBuf> {
	cube_texture_cache_dir().map(|dir| dir.join(format!("{key:016x}.ucube")))
}

fn cube_cache_temp_path(path: &Path) -> PathBuf {
	let stamp = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|duration| duration.as_nanos())
		.unwrap_or(0);
	let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("cache");
	path.with_file_name(format!("{file_name}.{}.{}.tmp", std::process::id(), stamp))
}

fn cube_read_exact_array<const N: usize>(reader: &mut impl Read) -> Option<[u8; N]> {
	let mut bytes = [0u8; N];
	reader.read_exact(&mut bytes).ok()?;
	Some(bytes)
}

fn cube_read_u32_le(reader: &mut impl Read) -> Option<u32> {
	Some(u32::from_le_bytes(cube_read_exact_array(reader)?))
}

fn cube_read_u64_le(reader: &mut impl Read) -> Option<u64> {
	Some(u64::from_le_bytes(cube_read_exact_array(reader)?))
}

fn cube_write_u32_le(writer: &mut impl Write, value: u32) -> std::io::Result<()> {
	writer.write_all(&value.to_le_bytes())
}

fn cube_write_u64_le(writer: &mut impl Write, value: u64) -> std::io::Result<()> {
	writer.write_all(&value.to_le_bytes())
}

fn cube_write_cache_file(path: &Path, write_contents: impl FnOnce(&mut BufWriter<fs::File>) -> std::io::Result<()>) -> bool {
	let Some(parent) = path.parent() else { return false };
	if fs::create_dir_all(parent).is_err() {
		return false;
	}
	let temp_path = cube_cache_temp_path(path);
	let write_result = (|| -> std::io::Result<()> {
		let mut writer = BufWriter::new(fs::File::create(&temp_path)?);
		write_contents(&mut writer)?;
		writer.flush()
	})();
	if write_result.is_err() {
		let _ = fs::remove_file(&temp_path);
		return false;
	}
	if fs::rename(&temp_path, path).is_ok() {
		return true;
	}
	let _ = fs::remove_file(path);
	let renamed = fs::rename(&temp_path, path).is_ok();
	if !renamed {
		let _ = fs::remove_file(&temp_path);
	}
	renamed
}

fn read_cube_texture_cache(path: &Path, key: u64) -> Option<CubeUpload> {
	let mut file = BufReader::new(fs::File::open(path).ok()?);
	if &cube_read_exact_array::<8>(&mut file)? != CUBE_TEXTURE_CACHE_MAGIC {
		return None;
	}
	if cube_read_u64_le(&mut file)? != key {
		return None;
	}
	let face_size = cube_read_u32_le(&mut file)?.max(1);
	let layout_tag = cube_read_exact_array::<1>(&mut file)?[0];
	let layout = match layout_tag {
		1 => CubeSourceLayout::Latlong,
		2 => CubeSourceLayout::SphereMap,
		3 => CubeSourceLayout::HorizontalStrip,
		4 => CubeSourceLayout::VerticalStrip,
		5 => CubeSourceLayout::HorizontalCross,
		6 => CubeSourceLayout::VerticalCross,
		_ => return None,
	};
	let mip_count = cube_read_u32_le(&mut file)? as usize;
	if mip_count == 0 || mip_count > 32 {
		return None;
	}
	let mut mips = Vec::with_capacity(mip_count);
	for _ in 0..mip_count {
		let mip_face_size = cube_read_u32_le(&mut file)?.max(1);
		let len = cube_read_u64_le(&mut file)? as usize;
		let expected = (mip_face_size as usize)
			.checked_mul(mip_face_size as usize)?
			.checked_mul(6)?
			.checked_mul(8)?;
		if len != expected {
			return None;
		}
		let mut data_rgba16f = vec![0u8; len];
		file.read_exact(&mut data_rgba16f).ok()?;
		mips.push(CubeMipUpload {
			face_size: mip_face_size,
			data_rgba16f,
		});
	}
	Some(CubeUpload {
		face_size,
		mips,
		layout: layout.name(),
	})
}

fn write_cube_texture_cache(path: &Path, key: u64, layout: CubeSourceLayout, upload: &CubeUpload) -> bool {
	cube_write_cache_file(path, |writer| {
		writer.write_all(CUBE_TEXTURE_CACHE_MAGIC)?;
		cube_write_u64_le(writer, key)?;
		cube_write_u32_le(writer, upload.face_size)?;
		writer.write_all(&[layout.cache_tag()])?;
		cube_write_u32_le(writer, upload.mips.len() as u32)?;
		for mip in &upload.mips {
			cube_write_u32_le(writer, mip.face_size)?;
			cube_write_u64_le(writer, mip.data_rgba16f.len() as u64)?;
			writer.write_all(&mip.data_rgba16f)?;
		}
		Ok(())
	})
}

fn texture_source_is_cube(source: Option<&UnaImageSourceMetadata>) -> bool {
	source
		.and_then(|source| source.texture_shape.as_deref())
		.is_some_and(|shape| shape.eq_ignore_ascii_case("TextureCube") || shape.eq_ignore_ascii_case("Cube"))
}

fn texture_source_is_srgb(source: Option<&UnaImageSourceMetadata>) -> bool {
	source.is_some_and(|source| match source.color_space.as_deref() {
		Some(color_space) => color_space.eq_ignore_ascii_case("srgb"),
		None => source.srgb == Some(true),
	})
}

fn cube_source_layout(image: &UnaImageRgba, source: Option<&UnaImageSourceMetadata>) -> Option<(CubeSourceLayout, u32)> {
	if !texture_source_is_cube(source) {
		return None;
	}
	let (width, height) = scene_image_source_dimensions(image, source);
	let width = width.max(1);
	let height = height.max(1);
	let layout_hint = source
		.and_then(|source| source.source_layout.as_deref())
		.unwrap_or("")
		.to_ascii_lowercase();
	if layout_hint.contains("latlong") || layout_hint.contains("cylindrical") || width == height.saturating_mul(2) {
		return Some((CubeSourceLayout::Latlong, (width / 4).min(height / 2).max(1)));
	}
	if layout_hint.contains("sphere") || width == height {
		return Some((CubeSourceLayout::SphereMap, width.min(height).max(1)));
	}
	if layout_hint.contains("horizontal_strip") || width == height.saturating_mul(6) {
		return Some((CubeSourceLayout::HorizontalStrip, height.max(1)));
	}
	if layout_hint.contains("vertical_strip") || height == width.saturating_mul(6) {
		return Some((CubeSourceLayout::VerticalStrip, width.max(1)));
	}
	if layout_hint.contains("horizontal_cross") || width.saturating_mul(3) == height.saturating_mul(4) {
		return Some((CubeSourceLayout::HorizontalCross, (width / 4).min(height / 3).max(1)));
	}
	if layout_hint.contains("vertical_cross") || width.saturating_mul(4) == height.saturating_mul(3) {
		return Some((CubeSourceLayout::VerticalCross, (width / 3).min(height / 4).max(1)));
	}
	if layout_hint.contains("unity_auto") {
		return Some((CubeSourceLayout::SphereMap, width.min(height).max(1)));
	}
	None
}

fn estimated_cube_upload_mip_bytes(face_size: u32) -> u64 {
	let mut total = 0u64;
	let mut size = face_size.max(1);
	loop {
		total = total.saturating_add(size as u64 * size as u64 * 6 * 8);
		if size <= 1 {
			break;
		}
		size = (size / 2).max(1);
	}
	total
}

fn cube_upload_from_image(
	image: &UnaImageRgba,
	source: Option<&UnaImageSourceMetadata>,
	cache_enabled: bool,
) -> Option<(CubeUpload, CubeUploadCacheEvent)> {
	let (layout, face_size) = cube_source_layout(image, source)?;
	let srgb = texture_source_is_srgb(source);
	let cache_lookup = cache_enabled
		.then(|| {
			let source = source?;
			let key = cube_texture_cache_key(image, source, layout, face_size, srgb);
			let path = cube_texture_cache_path(key)?;
			Some((key, path))
		})
		.flatten();
	if let Some((key, path)) = cache_lookup.as_ref() {
		if let Some(upload) = read_cube_texture_cache(path, *key) {
			return Some((
				upload,
				CubeUploadCacheEvent {
					hit: true,
					miss: false,
					write: false,
				},
			));
		}
	}
	let mut base_rgba = Vec::with_capacity(face_size as usize * face_size as usize * 6);
	for face in 0..6 {
		for y in 0..face_size {
			for x in 0..face_size {
				let u = (((x as f32 + 0.5) / face_size as f32) * 2.0) - 1.0;
				let v = (((y as f32 + 0.5) / face_size as f32) * 2.0) - 1.0;
				let dir = cube_face_direction(face, u, v);
				let rgba = match layout {
					CubeSourceLayout::Latlong => sample_latlong(image, dir, srgb),
					CubeSourceLayout::SphereMap => sample_sphere_map(image, dir, srgb),
					CubeSourceLayout::HorizontalStrip
					| CubeSourceLayout::VerticalStrip
					| CubeSourceLayout::HorizontalCross
					| CubeSourceLayout::VerticalCross => sample_packed_cube_face(image, layout, face, u, v, srgb),
				};
				base_rgba.push(rgba);
			}
		}
	}
	let upload = CubeUpload {
		face_size,
		mips: build_cube_mips_rgba16f(face_size, base_rgba),
		layout: layout.name(),
	};
	let write = cache_lookup
		.as_ref()
		.is_some_and(|(key, path)| write_cube_texture_cache(path, *key, layout, &upload));
	Some((
		upload,
		CubeUploadCacheEvent {
			hit: false,
			miss: cache_lookup.is_some(),
			write,
		},
	))
}

fn build_cube_mips_rgba16f(face_size: u32, base_rgba: Vec<[f32; 4]>) -> Vec<CubeMipUpload> {
	let mut mips = Vec::with_capacity(mip_level_count(face_size, face_size) as usize);
	let mut current_size = face_size.max(1);
	let mut current = base_rgba;
	loop {
		mips.push(CubeMipUpload {
			face_size: current_size,
			data_rgba16f: cube_rgba_f32_to_rgba16f_bytes(&current),
		});
		if current_size <= 1 {
			break;
		}
		let next_size = (current_size / 2).max(1);
		current = downsample_cube_faces(&current, current_size, next_size);
		current_size = next_size;
	}
	mips
}

fn cube_rgba_f32_to_rgba16f_bytes(pixels: &[[f32; 4]]) -> Vec<u8> {
	let mut data = Vec::with_capacity(pixels.len() * 8);
	for rgba in pixels {
		for value in rgba {
			data.extend_from_slice(&f16::from_f32(*value).to_bits().to_le_bytes());
		}
	}
	data
}

fn downsample_cube_faces(source: &[[f32; 4]], source_size: u32, next_size: u32) -> Vec<[f32; 4]> {
	let source_size = source_size.max(1) as usize;
	let next_size = next_size.max(1) as usize;
	let mut next = vec![[0.0; 4]; 6 * next_size * next_size];
	for face in 0..6usize {
		for y in 0..next_size {
			for x in 0..next_size {
				let sx = (x * 2).min(source_size - 1);
				let sy = (y * 2).min(source_size - 1);
				let mut sum = [0.0; 4];
				let mut count = 0.0;
				for oy in 0..2usize {
					for ox in 0..2usize {
						let px = (sx + ox).min(source_size - 1);
						let py = (sy + oy).min(source_size - 1);
						let sample = source[face * source_size * source_size + py * source_size + px];
						for channel in 0..4 {
							sum[channel] += sample[channel];
						}
						count += 1.0;
					}
				}
				for channel in 0..4 {
					sum[channel] /= count;
				}
				next[face * next_size * next_size + y * next_size + x] = sum;
			}
		}
	}
	next
}

fn cube_face_direction(face: u32, u: f32, v: f32) -> Vec3 {
	let dir = match face {
		0 => Vec3::new(1.0, -v, -u),
		1 => Vec3::new(-1.0, -v, u),
		2 => Vec3::new(u, 1.0, v),
		3 => Vec3::new(u, -1.0, -v),
		4 => Vec3::new(u, -v, 1.0),
		_ => Vec3::new(-u, -v, -1.0),
	};
	dir.normalize_or_zero()
}

fn sample_latlong(image: &UnaImageRgba, dir: Vec3, srgb: bool) -> [f32; 4] {
	let theta = dir.z.atan2(dir.x);
	let u = theta / std::f32::consts::TAU + 0.5;
	let v = dir.y.clamp(-1.0, 1.0).acos() / std::f32::consts::PI;
	sample_image_bilinear(image, u, v, true, srgb)
}

fn sample_sphere_map(image: &UnaImageRgba, dir: Vec3, srgb: bool) -> [f32; 4] {
	let denom = 2.0 * (dir.x * dir.x + dir.y * dir.y + (dir.z + 1.0) * (dir.z + 1.0)).sqrt();
	if denom <= 0.000001 {
		let samples = [
			sample_image_bilinear(image, 1.0, 0.5, false, srgb),
			sample_image_bilinear(image, 0.0, 0.5, false, srgb),
			sample_image_bilinear(image, 0.5, 0.0, false, srgb),
			sample_image_bilinear(image, 0.5, 1.0, false, srgb),
		];
		let mut avg = [0.0; 4];
		for sample in samples {
			for channel in 0..4 {
				avg[channel] += sample[channel] * 0.25;
			}
		}
		return avg;
	}
	let u = dir.x / denom + 0.5;
	let v = -dir.y / denom + 0.5;
	sample_image_bilinear(image, u, v, false, srgb)
}

fn sample_packed_cube_face(image: &UnaImageRgba, layout: CubeSourceLayout, face: u32, u: f32, v: f32, srgb: bool) -> [f32; 4] {
	let face_u = (u * 0.5 + 0.5).clamp(0.0, 1.0);
	let face_v = (v * 0.5 + 0.5).clamp(0.0, 1.0);
	let (cell_x, cell_y, columns, rows) = match layout {
		CubeSourceLayout::HorizontalStrip => (face, 0, 6, 1),
		CubeSourceLayout::VerticalStrip => (0, face, 1, 6),
		CubeSourceLayout::HorizontalCross => match face {
			0 => (2, 1, 4, 3),
			1 => (0, 1, 4, 3),
			2 => (1, 0, 4, 3),
			3 => (1, 2, 4, 3),
			4 => (1, 1, 4, 3),
			_ => (3, 1, 4, 3),
		},
		CubeSourceLayout::VerticalCross => match face {
			0 => (2, 1, 3, 4),
			1 => (0, 1, 3, 4),
			2 => (1, 0, 3, 4),
			3 => (1, 2, 3, 4),
			4 => (1, 1, 3, 4),
			_ => (1, 3, 3, 4),
		},
		CubeSourceLayout::Latlong | CubeSourceLayout::SphereMap => return [0.0, 0.0, 0.0, 1.0],
	};
	let u = (cell_x as f32 + face_u) / columns as f32;
	let v = (cell_y as f32 + face_v) / rows as f32;
	sample_image_bilinear(image, u, v, false, srgb)
}

fn sample_image_bilinear(image: &UnaImageRgba, u: f32, v: f32, wrap_u: bool, srgb: bool) -> [f32; 4] {
	let width = image.width.max(1);
	let height = image.height.max(1);
	let u = if wrap_u { u.rem_euclid(1.0) } else { u.clamp(0.0, 1.0) };
	let v = v.clamp(0.0, 1.0);
	let x = u * (width - 1) as f32;
	let y = v * (height - 1) as f32;
	let x0 = x.floor() as u32;
	let y0 = y.floor() as u32;
	let x1 = (x0 + 1).min(width - 1);
	let y1 = (y0 + 1).min(height - 1);
	let tx = x - x0 as f32;
	let ty = y - y0 as f32;
	let c00 = sample_image_pixel(image, x0, y0, srgb);
	let c10 = sample_image_pixel(image, x1, y0, srgb);
	let c01 = sample_image_pixel(image, x0, y1, srgb);
	let c11 = sample_image_pixel(image, x1, y1, srgb);
	let mix = |a: f32, b: f32, t: f32| a + (b - a) * t;
	let mut out = [0.0; 4];
	for i in 0..4 {
		out[i] = mix(mix(c00[i], c10[i], tx), mix(c01[i], c11[i], tx), ty);
	}
	out
}

fn sample_image_pixel(image: &UnaImageRgba, x: u32, y: u32, srgb: bool) -> [f32; 4] {
	let pixel_index = y as usize * image.width.max(1) as usize + x as usize;
	let srgb_to_linear = |value: f32| -> f32 {
		if !srgb {
			return value;
		}
		if value <= 0.04045 {
			value / 12.92
		} else {
			((value + 0.055) / 1.055).powf(2.4)
		}
	};
	match image.pixel_format {
		un_avatar_core::UnaImagePixelFormat::R8 => {
			let r = image.pixels.get(pixel_index).copied().unwrap_or(0) as f32 / 255.0;
			let r = srgb_to_linear(r);
			[r, r, r, 1.0]
		}
		un_avatar_core::UnaImagePixelFormat::R8G8 => {
			let offset = pixel_index * 2;
			let r = image.pixels.get(offset).copied().unwrap_or(0) as f32 / 255.0;
			let a = image.pixels.get(offset + 1).copied().unwrap_or(255) as f32 / 255.0;
			let r = srgb_to_linear(r);
			[r, r, r, a]
		}
		un_avatar_core::UnaImagePixelFormat::R8G8B8 => {
			let offset = pixel_index * 3;
			[
				srgb_to_linear(image.pixels.get(offset).copied().unwrap_or(0) as f32 / 255.0),
				srgb_to_linear(image.pixels.get(offset + 1).copied().unwrap_or(0) as f32 / 255.0),
				srgb_to_linear(image.pixels.get(offset + 2).copied().unwrap_or(0) as f32 / 255.0),
				1.0,
			]
		}
		un_avatar_core::UnaImagePixelFormat::R8G8B8A8 => {
			let offset = pixel_index * 4;
			[
				srgb_to_linear(image.pixels.get(offset).copied().unwrap_or(0) as f32 / 255.0),
				srgb_to_linear(image.pixels.get(offset + 1).copied().unwrap_or(0) as f32 / 255.0),
				srgb_to_linear(image.pixels.get(offset + 2).copied().unwrap_or(0) as f32 / 255.0),
				image.pixels.get(offset + 3).copied().unwrap_or(255) as f32 / 255.0,
			]
		}
		un_avatar_core::UnaImagePixelFormat::R16G16B16Float => sample_f16_pixel(&image.pixels, pixel_index, 3),
		un_avatar_core::UnaImagePixelFormat::R16G16B16A16Float => sample_f16_pixel(&image.pixels, pixel_index, 4),
		un_avatar_core::UnaImagePixelFormat::R32G32B32Float => sample_f32_pixel(&image.pixels, pixel_index, 3),
		un_avatar_core::UnaImagePixelFormat::R32G32B32A32Float => sample_f32_pixel(&image.pixels, pixel_index, 4),
		_ => {
			let rgba = image.rgba8_compat_pixels();
			let offset = pixel_index * 4;
			[
				rgba.get(offset).copied().unwrap_or(0) as f32 / 255.0,
				rgba.get(offset + 1).copied().unwrap_or(0) as f32 / 255.0,
				rgba.get(offset + 2).copied().unwrap_or(0) as f32 / 255.0,
				rgba.get(offset + 3).copied().unwrap_or(255) as f32 / 255.0,
			]
		}
	}
}

fn sample_f16_pixel(pixels: &[u8], pixel_index: usize, channels: usize) -> [f32; 4] {
	let offset = pixel_index * channels * 2;
	let channel = |index: usize| -> f32 {
		if index >= channels {
			return if index == 3 { 1.0 } else { 0.0 };
		}
		let offset = offset + index * 2;
		let bytes = [
			pixels.get(offset).copied().unwrap_or(0),
			pixels.get(offset + 1).copied().unwrap_or(0),
		];
		f16::from_bits(u16::from_le_bytes(bytes)).to_f32()
	};
	[channel(0), channel(1), channel(2), channel(3)]
}

fn sample_f32_pixel(pixels: &[u8], pixel_index: usize, channels: usize) -> [f32; 4] {
	let offset = pixel_index * channels * 4;
	let channel = |index: usize| -> f32 {
		if index >= channels {
			return if index == 3 { 1.0 } else { 0.0 };
		}
		let offset = offset + index * 4;
		let bytes = [
			pixels.get(offset).copied().unwrap_or(0),
			pixels.get(offset + 1).copied().unwrap_or(0),
			pixels.get(offset + 2).copied().unwrap_or(0),
			pixels.get(offset + 3).copied().unwrap_or(0),
		];
		f32::from_le_bytes(bytes)
	};
	[channel(0), channel(1), channel(2), channel(3)]
}

fn draw_has_outline(d: &MeshDraw, opts: &SceneMeshLoadOpts) -> bool {
	match opts.avatar_outline.policy {
		AvatarOutlinePolicy::Override => false,
		AvatarOutlinePolicy::Authored => {
			if !matches!(d.alpha_mode, UnaAlphaMode::Opaque | UnaAlphaMode::Mask) {
				return false;
			}
			if d.shading.is_liltoon_like() {
				return d
					.material
					.liltoon_like_runtime()
					.is_some_and(|material| material.outline.enabled_factor > 0.5 && material.outline.width_factor > 0.0);
			}
			d.shading.is_mtoon_like()
				&& d.material
					.mtoon_like_runtime()
					.is_some_and(|mtoon| effective_mtoon_outline(mtoon, opts).is_some())
		}
		AvatarOutlinePolicy::Off => false,
	}
}

fn material_fur_layer_count(material: &UnaMaterialPbr, shading: UnaShadingModel) -> u32 {
	if !shading.is_liltoon_like() {
		return 0;
	}
	let Some(liltoon_like) = material.liltoon_like_runtime() else {
		return 0;
	};
	if liltoon_like.fur.enabled_factor <= 0.5 {
		return 0;
	}
	liltoon_fur_sample_count_for_layer_num(liltoon_like.fur.layer_count_factor)
}

fn liltoon_fur_layer_num(layer_num: f32) -> u32 {
	if layer_num <= 1.5 {
		1
	} else if layer_num < 2.5 {
		2
	} else {
		3
	}
}

fn liltoon_fur_sample_count_for_layer_num(layer_num: f32) -> u32 {
	match liltoon_fur_layer_num(layer_num) {
		1 => 4,
		2 => 7,
		_ => 13,
	}
}

fn liltoon_fur_segment_count(layer_num: f32) -> u32 {
	liltoon_fur_sample_count_for_layer_num(layer_num).saturating_sub(1)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
enum ComputeFurCardsExpressionMode {
	LilToonCompatible,
	UnaStandard,
	UnaHighQuality,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code)]
struct ComputeFurCardsTriangleMetrics {
	world_area: f32,
	uv_area: f32,
	average_fur_mask: f32,
	average_length_mask: f32,
	fur_length: f32,
	projected_area_factor: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code)]
struct ComputeFurCardsAllocationParams {
	target_world_area: f32,
	target_uv_area: f32,
	target_fur_length: f32,
	min_cards_per_visible_triangle: u32,
	max_cards_per_triangle: u32,
	global_quality_scale: f32,
}

impl Default for ComputeFurCardsAllocationParams {
	fn default() -> Self {
		Self {
			target_world_area: 0.00018,
			target_uv_area: 0.00008,
			target_fur_length: 0.02,
			min_cards_per_visible_triangle: 1,
			max_cards_per_triangle: 96,
			global_quality_scale: 1.0,
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code)]
struct ComputeFurCardsBarycentricSample {
	barycentric: [f32; 3],
	seed: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
struct ComputeFurCardsBufferRequirements {
	card_count: u32,
	vertex_count: u32,
	index_count: u32,
	vertex_bytes: u64,
	index_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
struct ComputeFurCardsSourceBufferRequirements {
	vertex_count: u32,
	triangle_count: u32,
	vertex_bytes: u64,
	triangle_bytes: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct ComputeFurCardsSourceVertexGpu {
	position: [f32; 4],
	normal: [f32; 4],
	tangent: [f32; 4],
	uv: [f32; 4],
	color: [f32; 4],
	joints: [u32; 4],
	weights: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
struct ComputeFurCardsSourceTriangleGpu {
	indices: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
struct ComputeFurCardsCardSourceGpu {
	indices: [u32; 4],
	sample_index: u32,
	_pad: [u32; 3],
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
struct ComputeFurCardsCpuFurMaps<'a> {
	length_mask: Option<&'a UnaImageRgba>,
	fur_mask: Option<&'a UnaImageRgba>,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ComputeFurCardsGenerateParamsGpu {
	source_triangle_count: u32,
	card_count: u32,
	max_generated_vertices: u32,
	max_generated_indices: u32,
	cards_per_triangle: u32,
	_seed: u32,
	randomize: f32,
	feature_flags: u32,
	fur_length: f32,
	card_width: f32,
	root_offset: f32,
	gravity: f32,
	cutout_length: f32,
	_pad2: [u32; 3],
	direction: [f32; 4],
	main_uv: [f32; 4],
	model: [[f32; 4]; 4],
	inv_model: [[f32; 4]; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ComputeFurCardsGeneratedVertexGpu {
	root_or_tip_position: [f32; 3],
	fur_layer: f32,
	normal: [f32; 3],
	card_side: f32,
	uv: [f32; 2],
	fur_alpha: f32,
	_seed: u32,
	root_position: [f32; 4],
	pre_position: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<ComputeFurCardsSourceVertexGpu>() == 112);
const _: () = assert!(std::mem::size_of::<ComputeFurCardsSourceTriangleGpu>() == 16);
const _: () = assert!(std::mem::size_of::<ComputeFurCardsCardSourceGpu>() == 32);
const _: () = assert!(std::mem::size_of::<ComputeFurCardsGenerateParamsGpu>() == 224);
const _: () = assert!(std::mem::size_of::<ComputeFurCardsGeneratedVertexGpu>() == 80);

const COMPUTE_FUR_CARDS_FEATURE_FUR_VECTOR_TEX: u32 = 1;
const COMPUTE_FUR_CARDS_FEATURE_VERTEX_COLOR_FUR_VECTOR: u32 = 2;

#[allow(dead_code)]
fn compute_fur_cards_mode_density(layer_num: f32, mode: ComputeFurCardsExpressionMode) -> f32 {
	let compatible = liltoon_fur_sample_count_for_layer_num(layer_num) as f32;
	match mode {
		ComputeFurCardsExpressionMode::LilToonCompatible => compatible,
		ComputeFurCardsExpressionMode::UnaStandard => compatible * 1.25,
		ComputeFurCardsExpressionMode::UnaHighQuality => compatible * 2.0,
	}
}

#[allow(dead_code)]
fn compute_fur_cards_safe_ratio(value: f32, target: f32) -> f32 {
	if !value.is_finite() || !target.is_finite() || target <= 0.0 {
		return 1.0;
	}
	(value.max(0.0) / target).sqrt().clamp(0.0, 8.0)
}

#[allow(dead_code)]
fn compute_fur_cards_triangle_card_count(
	layer_num: f32,
	mode: ComputeFurCardsExpressionMode,
	metrics: ComputeFurCardsTriangleMetrics,
	params: ComputeFurCardsAllocationParams,
) -> u32 {
	let mask = metrics.average_fur_mask.clamp(0.0, 1.0);
	let length_mask = metrics.average_length_mask.clamp(0.0, 1.0);
	let fur_length = metrics.fur_length.max(0.0);
	if mask <= 0.0001 || length_mask <= 0.0001 || fur_length <= 0.000001 || params.max_cards_per_triangle == 0 {
		return 0;
	}

	let base = compute_fur_cards_mode_density(layer_num, mode);
	let area_weight = compute_fur_cards_safe_ratio(metrics.world_area, params.target_world_area);
	let uv_weight = compute_fur_cards_safe_ratio(metrics.uv_area, params.target_uv_area);
	let length_weight = compute_fur_cards_safe_ratio(length_mask * fur_length, params.target_fur_length);
	let screen_weight = metrics.projected_area_factor.max(0.0).sqrt().clamp(0.25, 4.0);
	let quality = params.global_quality_scale.max(0.0);

	let raw = base * area_weight.max(0.2) * uv_weight.max(0.2) * mask * length_weight.max(0.2) * screen_weight * quality;
	let rounded = raw.round() as u32;
	rounded
		.max(params.min_cards_per_visible_triangle.min(params.max_cards_per_triangle))
		.min(params.max_cards_per_triangle)
}

#[allow(dead_code)]
fn compute_fur_cards_hash_u32(mut x: u32) -> u32 {
	x ^= x >> 16;
	x = x.wrapping_mul(0x7feb_352d);
	x ^= x >> 15;
	x = x.wrapping_mul(0x846c_a68b);
	x ^ (x >> 16)
}

#[allow(dead_code)]
fn compute_fur_cards_unit_from_hash(seed: u32) -> f32 {
	let value = compute_fur_cards_hash_u32(seed) >> 8;
	((value as f32) + 0.5) * (1.0 / 16_777_216.0)
}

#[allow(dead_code)]
fn compute_fur_cards_radical_inverse_vdc(mut bits: u32) -> f32 {
	bits = bits.rotate_right(16);
	bits = ((bits & 0x5555_5555) << 1) | ((bits & 0xaaaa_aaaa) >> 1);
	bits = ((bits & 0x3333_3333) << 2) | ((bits & 0xcccc_cccc) >> 2);
	bits = ((bits & 0x0f0f_0f0f) << 4) | ((bits & 0xf0f0_f0f0) >> 4);
	bits = ((bits & 0x00ff_00ff) << 8) | ((bits & 0xff00_ff00) >> 8);
	(bits as f32) * 2.328_306_4e-10
}

#[allow(dead_code)]
fn compute_fur_cards_barycentric_sample(triangle_seed: u32, sample_index: u32) -> ComputeFurCardsBarycentricSample {
	let seed = compute_fur_cards_hash_u32(triangle_seed ^ sample_index.wrapping_mul(0x9e37_79b9));
	let jitter = compute_fur_cards_unit_from_hash(seed);
	let u = ((sample_index as f32 + jitter) * 0.618_034).fract();
	let v = compute_fur_cards_radical_inverse_vdc(sample_index ^ seed);
	let su = u.sqrt();
	let barycentric = [1.0 - su, su * (1.0 - v), su * v];
	ComputeFurCardsBarycentricSample { barycentric, seed }
}

#[allow(dead_code)]
fn compute_fur_cards_interpolate_vec2(values: [Vec2; 3], barycentric: [f32; 3]) -> Vec2 {
	values[0] * barycentric[0] + values[1] * barycentric[1] + values[2] * barycentric[2]
}

#[allow(dead_code)]
fn compute_fur_cards_interpolate_vec3(values: [Vec3; 3], barycentric: [f32; 3]) -> Vec3 {
	values[0] * barycentric[0] + values[1] * barycentric[1] + values[2] * barycentric[2]
}

#[allow(dead_code)]
fn compute_fur_cards_buffer_requirements(card_count: u32) -> Option<ComputeFurCardsBufferRequirements> {
	let vertex_count = card_count.checked_mul(4)?;
	let index_count = card_count.checked_mul(6)?;
	let vertex_bytes = (vertex_count as u64).checked_mul(std::mem::size_of::<ComputeFurCardsGeneratedVertexGpu>() as u64)?;
	let index_bytes = (index_count as u64).checked_mul(std::mem::size_of::<u32>() as u64)?;
	Some(ComputeFurCardsBufferRequirements {
		card_count,
		vertex_count,
		index_count,
		vertex_bytes,
		index_bytes,
	})
}

#[allow(dead_code)]
fn compute_fur_cards_source_vertex_from_vertex(vertex: &Vertex) -> ComputeFurCardsSourceVertexGpu {
	ComputeFurCardsSourceVertexGpu {
		position: [vertex.pos[0], vertex.pos[1], vertex.pos[2], 1.0],
		normal: [vertex.norm[0], vertex.norm[1], vertex.norm[2], 0.0],
		tangent: vertex.tangent,
		uv: [vertex.uv[0], vertex.uv[1], 0.0, 0.0],
		color: vertex.color,
		joints: [
			vertex.joints[0] as u32,
			vertex.joints[1] as u32,
			vertex.joints[2] as u32,
			vertex.joints[3] as u32,
		],
		weights: vertex.weights,
	}
}

#[allow(dead_code)]
fn compute_fur_cards_source_vertices_from_mesh(verts: &[Vertex]) -> Vec<ComputeFurCardsSourceVertexGpu> {
	verts.iter().map(compute_fur_cards_source_vertex_from_vertex).collect()
}

fn compute_fur_cards_palette_matrices(raw: &[f32], out: &mut Vec<Mat4>) {
	out.clear();
	out.reserve(raw.len() / 16);
	for matrix in raw.chunks_exact(16) {
		out.push(Mat4::from_cols_array(matrix.try_into().expect("palette matrix slice length")));
	}
}

fn compute_fur_cards_skinned_source_vertex_from_vertex(vertex: &Vertex, palette_matrices: &[Mat4]) -> ComputeFurCardsSourceVertexGpu {
	let position = Vec3::from_array(vertex.pos);
	let normal = Vec3::from_array(vertex.norm);
	let tangent = Vec3::new(vertex.tangent[0], vertex.tangent[1], vertex.tangent[2]);
	if vertex.weights == [1.0, 0.0, 0.0, 0.0] {
		let palette_matrix_count = palette_matrices.len();
		let matrix = if palette_matrix_count == 0 {
			Mat4::IDENTITY
		} else {
			palette_matrices[(vertex.joints[0] as usize).min(palette_matrix_count - 1)]
		};
		let mut skinned_normal = matrix.transform_vector3(normal);
		let mut skinned_tangent = matrix.transform_vector3(tangent);
		if skinned_normal.length_squared() <= 0.0000001 {
			skinned_normal = normal;
		}
		if skinned_tangent.length_squared() <= 0.0000001 {
			skinned_tangent = tangent;
		}
		let skinned_normal = skinned_normal.normalize_or_zero();
		let skinned_tangent = skinned_tangent.normalize_or_zero();
		return ComputeFurCardsSourceVertexGpu {
			position: matrix.transform_point3(position).extend(1.0).to_array(),
			normal: [skinned_normal.x, skinned_normal.y, skinned_normal.z, 0.0],
			tangent: [skinned_tangent.x, skinned_tangent.y, skinned_tangent.z, vertex.tangent[3]],
			uv: [vertex.uv[0], vertex.uv[1], 0.0, 0.0],
			color: vertex.color,
			joints: [
				vertex.joints[0] as u32,
				vertex.joints[1] as u32,
				vertex.joints[2] as u32,
				vertex.joints[3] as u32,
			],
			weights: vertex.weights,
		};
	}
	let mut skinned_position = Vec3::ZERO;
	let mut skinned_normal = Vec3::ZERO;
	let mut skinned_tangent = Vec3::ZERO;
	let palette_matrix_count = palette_matrices.len();
	for i in 0..4 {
		let weight = vertex.weights[i];
		if weight.abs() <= 0.000001 {
			continue;
		}
		let matrix = if palette_matrix_count == 0 {
			Mat4::IDENTITY
		} else {
			palette_matrices[(vertex.joints[i] as usize).min(palette_matrix_count - 1)]
		};
		skinned_position += matrix.transform_point3(position) * weight;
		skinned_normal += matrix.transform_vector3(normal) * weight;
		skinned_tangent += matrix.transform_vector3(tangent) * weight;
	}
	if skinned_normal.length_squared() <= 0.0000001 {
		skinned_normal = normal;
	}
	if skinned_tangent.length_squared() <= 0.0000001 {
		skinned_tangent = tangent;
	}
	let skinned_normal = skinned_normal.normalize_or_zero();
	let skinned_tangent = skinned_tangent.normalize_or_zero();
	ComputeFurCardsSourceVertexGpu {
		position: [skinned_position.x, skinned_position.y, skinned_position.z, 1.0],
		normal: [skinned_normal.x, skinned_normal.y, skinned_normal.z, 0.0],
		tangent: [skinned_tangent.x, skinned_tangent.y, skinned_tangent.z, vertex.tangent[3]],
		uv: [vertex.uv[0], vertex.uv[1], 0.0, 0.0],
		color: vertex.color,
		joints: [
			vertex.joints[0] as u32,
			vertex.joints[1] as u32,
			vertex.joints[2] as u32,
			vertex.joints[3] as u32,
		],
		weights: vertex.weights,
	}
}

fn compute_fur_cards_skinned_source_vertices_from_matrices(
	verts: &[Vertex],
	palette_matrices: &[Mat4],
	out: &mut Vec<ComputeFurCardsSourceVertexGpu>,
) {
	out.clear();
	out.reserve(verts.len());
	out.extend(
		verts
			.iter()
			.map(|vertex| compute_fur_cards_skinned_source_vertex_from_vertex(vertex, palette_matrices)),
	);
}

#[allow(dead_code)]
fn compute_fur_cards_source_triangles_from_indices(indices: &[u32], vertex_count: usize) -> Vec<ComputeFurCardsSourceTriangleGpu> {
	indices
		.chunks_exact(3)
		.filter_map(|tri| {
			let i0 = tri[0] as usize;
			let i1 = tri[1] as usize;
			let i2 = tri[2] as usize;
			if i0 < vertex_count && i1 < vertex_count && i2 < vertex_count {
				Some(ComputeFurCardsSourceTriangleGpu {
					indices: [tri[0], tri[1], tri[2], 0],
				})
			} else {
				None
			}
		})
		.collect()
}

fn compute_fur_cards_source_triangles_from_indices_u16(indices: &[u16], vertex_count: usize) -> Vec<ComputeFurCardsSourceTriangleGpu> {
	indices
		.chunks_exact(3)
		.filter_map(|tri| {
			let i0 = tri[0] as usize;
			let i1 = tri[1] as usize;
			let i2 = tri[2] as usize;
			(i0 < vertex_count && i1 < vertex_count && i2 < vertex_count).then_some(ComputeFurCardsSourceTriangleGpu {
				indices: [i0 as u32, i1 as u32, i2 as u32, 0],
			})
		})
		.collect()
}

#[allow(dead_code)]
fn compute_fur_cards_cpu_map_red_at(map: Option<&UnaImageRgba>, uv: Vec2, fallback: f32) -> f32 {
	map.map(|image| sample_image_bilinear(image, uv.x, uv.y, true, false)[0])
		.unwrap_or(fallback)
		.clamp(0.0, 1.0)
}

#[allow(dead_code)]
fn compute_fur_cards_average_cpu_map_red(map: Option<&UnaImageRgba>, uvs: [Vec2; 3], fallback: f32) -> f32 {
	let centroid = (uvs[0] + uvs[1] + uvs[2]) * (1.0 / 3.0);
	[
		compute_fur_cards_cpu_map_red_at(map, uvs[0], fallback),
		compute_fur_cards_cpu_map_red_at(map, uvs[1], fallback),
		compute_fur_cards_cpu_map_red_at(map, uvs[2], fallback),
		compute_fur_cards_cpu_map_red_at(map, centroid, fallback),
	]
	.into_iter()
	.sum::<f32>()
		* 0.25
}

#[allow(dead_code)]
fn compute_fur_cards_triangle_metrics_from_source(
	verts: &[Vertex],
	triangle: ComputeFurCardsSourceTriangleGpu,
	fur_length: f32,
	cpu_maps: ComputeFurCardsCpuFurMaps<'_>,
) -> Option<ComputeFurCardsTriangleMetrics> {
	let i0 = triangle.indices[0] as usize;
	let i1 = triangle.indices[1] as usize;
	let i2 = triangle.indices[2] as usize;
	let v0 = verts.get(i0)?;
	let v1 = verts.get(i1)?;
	let v2 = verts.get(i2)?;
	let p0 = Vec3::from_array(v0.pos);
	let p1 = Vec3::from_array(v1.pos);
	let p2 = Vec3::from_array(v2.pos);
	let uv0 = Vec2::from_array(v0.uv);
	let uv1 = Vec2::from_array(v1.uv);
	let uv2 = Vec2::from_array(v2.uv);
	let world_area = (p1 - p0).cross(p2 - p0).length() * 0.5;
	let uv_e1 = uv1 - uv0;
	let uv_e2 = uv2 - uv0;
	let uv_area = (uv_e1.x * uv_e2.y - uv_e1.y * uv_e2.x).abs() * 0.5;
	let uvs = [uv0, uv1, uv2];
	Some(ComputeFurCardsTriangleMetrics {
		world_area,
		uv_area,
		average_fur_mask: compute_fur_cards_average_cpu_map_red(cpu_maps.fur_mask, uvs, 1.0),
		average_length_mask: compute_fur_cards_average_cpu_map_red(cpu_maps.length_mask, uvs, 1.0),
		fur_length,
		projected_area_factor: 1.0,
	})
}

#[allow(dead_code)]
fn compute_fur_cards_card_sources_from_triangles(
	material: &UnaMaterialPbr,
	verts: &[Vertex],
	triangles: &[ComputeFurCardsSourceTriangleGpu],
	cpu_maps: ComputeFurCardsCpuFurMaps<'_>,
) -> Option<Vec<ComputeFurCardsCardSourceGpu>> {
	let liltoon_fur = material.liltoon_like_runtime().map(|liltoon_like| &liltoon_like.fur);
	let layer_num = liltoon_fur.map(|fur| fur.layer_count_factor).unwrap_or(1.0);
	let fur_length = liltoon_fur.map(|fur| fur.vector_factor[3].max(0.0)).unwrap_or(0.0);
	let segment_count = liltoon_fur_segment_count(layer_num);
	if segment_count == 0 || fur_length <= 0.000001 {
		return None;
	}
	let mut card_sources = Vec::with_capacity(triangles.len().saturating_mul(segment_count as usize));
	for &triangle in triangles {
		let metrics = compute_fur_cards_triangle_metrics_from_source(verts, triangle, fur_length, cpu_maps)?;
		if metrics.average_fur_mask <= 0.0001 || metrics.average_length_mask <= 0.0001 {
			continue;
		}
		for sample_index in 0..segment_count {
			card_sources.push(ComputeFurCardsCardSourceGpu {
				indices: triangle.indices,
				sample_index,
				_pad: [0; 3],
			});
		}
	}
	(!card_sources.is_empty()).then_some(card_sources)
}

#[allow(dead_code)]
fn compute_fur_cards_source_buffer_requirements(vertex_count: u32, triangle_count: u32) -> Option<ComputeFurCardsSourceBufferRequirements> {
	let vertex_bytes = (vertex_count as u64).checked_mul(std::mem::size_of::<ComputeFurCardsSourceVertexGpu>() as u64)?;
	let triangle_bytes = (triangle_count as u64).checked_mul(std::mem::size_of::<ComputeFurCardsSourceTriangleGpu>() as u64)?;
	Some(ComputeFurCardsSourceBufferRequirements {
		vertex_count,
		triangle_count,
		vertex_bytes,
		triangle_bytes,
	})
}

#[allow(dead_code)]
fn compute_fur_cards_dispatch_workgroups(triangle_count: u32) -> u32 {
	triangle_count.saturating_add(63) / 64
}

#[allow(dead_code)]
struct ComputeFurCardsComputePipeline {
	_bind_group_layout: wgpu::BindGroupLayout,
	_pipeline: wgpu::ComputePipeline,
}

#[allow(dead_code)]
fn create_compute_fur_cards_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
	device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
		label: Some("compute_fur_cards"),
		entries: &[
			uniform_bind_group_layout_entry(0, wgpu::ShaderStages::COMPUTE),
			storage_bind_group_layout_entry(1, wgpu::ShaderStages::COMPUTE, true),
			storage_bind_group_layout_entry(2, wgpu::ShaderStages::COMPUTE, true),
			storage_bind_group_layout_entry(3, wgpu::ShaderStages::COMPUTE, false),
			storage_bind_group_layout_entry(4, wgpu::ShaderStages::COMPUTE, false),
			texture_bind_group_layout_entry(5, wgpu::ShaderStages::COMPUTE),
			texture_bind_group_layout_entry(6, wgpu::ShaderStages::COMPUTE),
			texture_bind_group_layout_entry(7, wgpu::ShaderStages::COMPUTE),
			texture_bind_group_layout_entry(8, wgpu::ShaderStages::COMPUTE),
			sampler_bind_group_layout_entry(9, wgpu::ShaderStages::COMPUTE),
		],
	})
}

#[allow(dead_code)]
fn create_compute_fur_cards_compute_pipeline(
	device: &wgpu::Device,
	bind_group_layout: &wgpu::BindGroupLayout,
	pipeline_cache: Option<&wgpu::PipelineCache>,
) -> ComputeFurCardsComputePipeline {
	let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
		label: Some("compute_fur_cards"),
		bind_group_layouts: &[Some(&bind_group_layout)],
		immediate_size: 0,
	});
	let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
		label: Some("compute_fur_cards"),
		source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER_COMPUTE_FUR_CARDS)),
	});
	let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
		label: Some("compute_fur_cards"),
		layout: Some(&pipeline_layout),
		module: &shader,
		entry_point: Some("compute_fur_cards_generate"),
		compilation_options: wgpu::PipelineCompilationOptions::default(),
		cache: pipeline_cache,
	});
	ComputeFurCardsComputePipeline {
		_bind_group_layout: bind_group_layout.clone(),
		_pipeline: pipeline,
	}
}

#[allow(dead_code)]
fn compute_fur_cards_cards_per_triangle_for_material(material: &UnaMaterialPbr) -> u32 {
	material
		.liltoon_like_runtime()
		.map(|liltoon_like| liltoon_fur_sample_count_for_layer_num(liltoon_like.fur.layer_count_factor))
		.unwrap_or(1)
		.max(1)
}

#[allow(dead_code)]
fn compute_fur_cards_generate_params_from_material(
	material: &UnaMaterialPbr,
	source_triangle_count: u32,
	_cards_per_triangle: u32,
	generated: ComputeFurCardsBufferRequirements,
) -> ComputeFurCardsGenerateParamsGpu {
	let fur = material.liltoon_like_runtime().map(|liltoon_like| &liltoon_like.fur);
	let vector = fur.map(|f| f.vector_factor).unwrap_or([0.0, 0.0, 1.0, 0.0]);
	let fur_length = vector[3].max(0.0);
	let cards_per_triangle = fur.map(|f| liltoon_fur_segment_count(f.layer_count_factor)).unwrap_or(0);
	ComputeFurCardsGenerateParamsGpu {
		source_triangle_count,
		card_count: generated.card_count,
		max_generated_vertices: generated.vertex_count,
		max_generated_indices: generated.index_count,
		cards_per_triangle,
		_seed: 0,
		randomize: fur.map(|f| f.randomize_factor.clamp(0.0, 1.0)).unwrap_or(0.0),
		feature_flags: fur
			.map(|f| {
				let mut flags = 0;
				if f.vector_texture_index.is_some() {
					flags |= COMPUTE_FUR_CARDS_FEATURE_FUR_VECTOR_TEX;
				}
				if f.vertex_color_to_vector_factor > 0.5 {
					flags |= COMPUTE_FUR_CARDS_FEATURE_VERTEX_COLOR_FUR_VECTOR;
				}
				flags
			})
			.unwrap_or(0),
		fur_length,
		card_width: (fur_length * 0.14).max(0.0012),
		root_offset: fur
			.map(|f| (-f.root_offset_factor.clamp(-1.0, 0.0) * fur_length).max(fur_length * 0.05))
			.unwrap_or(fur_length * 0.05),
		gravity: fur.map(|f| f.gravity_factor).unwrap_or(0.0),
		cutout_length: fur.map(|f| f.cutout_length_factor.max(0.0)).unwrap_or(0.8),
		_pad2: [0; 3],
		direction: [vector[0], vector[1], vector[2], fur.map(|f| f.vector_scale_factor).unwrap_or(1.0)],
		main_uv: [
			material.uv_offset_scale[2],
			material.uv_offset_scale[3],
			material.uv_offset_scale[0],
			material.uv_offset_scale[1],
		],
		model: Mat4::IDENTITY.to_cols_array_2d(),
		inv_model: Mat4::IDENTITY.to_cols_array_2d(),
	}
}

#[allow(dead_code)]
fn create_compute_fur_cards_draw_resources(
	device: &wgpu::Device,
	compute_fur_cards_bind_group_layout: &wgpu::BindGroupLayout,
	material: &UnaMaterialPbr,
	verts: &[Vertex],
	indices: &SceneMeshIndexUpload,
	cpu_maps: ComputeFurCardsCpuFurMaps<'_>,
	fur_vector_view: &wgpu::TextureView,
	fur_length_mask_view: &wgpu::TextureView,
	fur_noise_mask_view: &wgpu::TextureView,
	fur_mask_view: &wgpu::TextureView,
	fur_sampler: &wgpu::Sampler,
) -> Option<ComputeFurCardsDrawResources> {
	let source_vertices = compute_fur_cards_source_vertices_from_mesh(verts);
	let source_triangles = indices.source_triangles(source_vertices.len());
	let triangle_count = u32::try_from(source_triangles.len()).ok()?;
	if triangle_count == 0 {
		return None;
	}
	let vertex_count = u32::try_from(source_vertices.len()).ok()?;
	let _source_requirements = compute_fur_cards_source_buffer_requirements(vertex_count, triangle_count)?;
	let card_sources = compute_fur_cards_card_sources_from_triangles(material, verts, &source_triangles, cpu_maps)?;
	let card_count = u32::try_from(card_sources.len()).ok()?;
	let generated_requirements = compute_fur_cards_buffer_requirements(card_count)?;
	let params = compute_fur_cards_generate_params_from_material(material, triangle_count, 0, generated_requirements);

	let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
		label: Some("compute_fur_cards_params"),
		contents: bytemuck::bytes_of(&params),
		usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
	});
	let source_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
		label: Some("compute_fur_cards_source_vertices"),
		contents: bytemuck::cast_slice(&source_vertices),
		usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
	});
	let card_source_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
		label: Some("compute_fur_cards_card_sources"),
		contents: bytemuck::cast_slice(&card_sources),
		usage: wgpu::BufferUsages::STORAGE,
	});
	let generated_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
		label: Some("compute_fur_cards_generated_vertices"),
		size: generated_requirements.vertex_bytes,
		usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_SRC,
		mapped_at_creation: false,
	});
	let generated_index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
		label: Some("compute_fur_cards_generated_indices"),
		size: generated_requirements.index_bytes,
		usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_SRC,
		mapped_at_creation: false,
	});
	let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
		label: Some("compute_fur_cards_bg"),
		layout: compute_fur_cards_bind_group_layout,
		entries: &[
			wgpu::BindGroupEntry {
				binding: 0,
				resource: params_buffer.as_entire_binding(),
			},
			wgpu::BindGroupEntry {
				binding: 1,
				resource: source_vertex_buffer.as_entire_binding(),
			},
			wgpu::BindGroupEntry {
				binding: 2,
				resource: card_source_buffer.as_entire_binding(),
			},
			wgpu::BindGroupEntry {
				binding: 3,
				resource: generated_vertex_buffer.as_entire_binding(),
			},
			wgpu::BindGroupEntry {
				binding: 4,
				resource: generated_index_buffer.as_entire_binding(),
			},
			wgpu::BindGroupEntry {
				binding: 5,
				resource: wgpu::BindingResource::TextureView(fur_vector_view),
			},
			wgpu::BindGroupEntry {
				binding: 6,
				resource: wgpu::BindingResource::TextureView(fur_length_mask_view),
			},
			wgpu::BindGroupEntry {
				binding: 7,
				resource: wgpu::BindingResource::TextureView(fur_noise_mask_view),
			},
			wgpu::BindGroupEntry {
				binding: 8,
				resource: wgpu::BindingResource::TextureView(fur_mask_view),
			},
			wgpu::BindGroupEntry {
				binding: 9,
				resource: wgpu::BindingResource::Sampler(fur_sampler),
			},
		],
	});

	Some(ComputeFurCardsDrawResources {
		params,
		params_buffer,
		source_vertex_buffer,
		card_source_buffer,
		generated_vertex_buffer,
		generated_index_buffer,
		bind_group,
		triangle_count,
		card_count: generated_requirements.card_count,
		generated_index_count: generated_requirements.index_count,
		dispatch_workgroups: compute_fur_cards_dispatch_workgroups(card_count),
	})
}

fn material_has_fur(material: &UnaMaterialPbr, shading: UnaShadingModel, opts: &SceneMeshLoadOpts) -> bool {
	!opts.force_simple_basecolor
		&& !opts.debug_bind_pose
		&& !opts.debug_primitive_colors
		&& !opts.disable_fur
		&& material_fur_layer_count(material, shading) > 0
}

fn material_is_fully_invisible_for_draw(mat: &UnaMaterialPbr, opts: &SceneMeshLoadOpts) -> bool {
	if opts.force_simple_basecolor || opts.debug_primitive_colors {
		return false;
	}
	if opts.relax_iris_alpha && iris_like_material_name(mat.name.as_deref()) && mat.base_color_factor[3] <= 0.001 {
		return false;
	}
	mat.base_color_factor[3] <= 0.001 && matches!(mat.alpha_mode, UnaAlphaMode::Mask | UnaAlphaMode::Blend)
}

fn mesh_draw_material_gpu_with_profiles(
	mat: &UnaMaterialPbr,
	mtoon: &UnaMtoonMaterial,
	liltoon_like: Option<&un_avatar_core::UnaLilToonLikeMaterial>,
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
	if opts.debug_disable_rim_lighting {
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
	match mat.cull_mode {
		UnaCullMode::Off => flags |= 512,
		UnaCullMode::Front => flags |= 2048,
		UnaCullMode::Back => {}
	}
	if mat.occlusion_texture_index.is_some() {
		flags |= 1024;
	}
	let (rim_color, rim_lighting_mix, rim_power, rim_lift) = effective_mtoon_rim(mat, mtoon, opts);
	let rim_texture_mix = 1.0;
	if liltoon_like.is_some_and(un_avatar_core::UnaLilToonLikeMaterial::is_gem_profile) {
		flags |= MAT_UNTOON_GEM_PROFILE;
	}
	if liltoon_like.is_some_and(un_avatar_core::UnaLilToonLikeMaterial::is_refraction_profile) {
		flags |= MAT_UNTOON_REFRACTION_PROFILE;
	}
	if liltoon_uses_additive_color_blend(mat) {
		flags |= MAT_UNTOON_ADDITIVE_BLEND;
	}
	let normal_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_BumpMap", "_NormalMap", "_BumpTex"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let normal2nd_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_Bump2ndMap", "_BumpMap2nd", "_NormalMap2nd"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let normal2nd_scale_mask_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_Bump2ndScaleMask"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let normal2nd_params = liltoon_like
		.map(|u| {
			[
				u.normal.second_enabled_factor.clamp(0.0, 1.0),
				u.normal.second_scale_factor,
				u.texture_uv_mode_factors
					.get("_Bump2ndMap")
					.or_else(|| u.texture_uv_mode_factors.get("_BumpMap2nd"))
					.or_else(|| u.texture_uv_mode_factors.get("_NormalMap2nd"))
					.copied()
					.unwrap_or(0.0)
					.clamp(0.0, 3.0),
				0.0,
			]
		})
		.unwrap_or([0.0, 1.0, 0.0, 0.0]);
	let shade_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_ShadowColorTex", "_ShadeTex", "_1st_ShadeMap"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let rim_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_RimColorTex"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let emission_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_EmissionMap"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let emission_blend_mask_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_EmissionBlendMask"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let emission2nd_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_Emission2ndMap"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let emission2nd_blend_mask_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_Emission2ndBlendMask"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let reflection_color_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_ReflectionColorTex"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let smoothness_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_SmoothnessTex"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let metallic_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_MetallicGlossMap"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let anisotropy_tangent_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_AnisotropyTangentMap"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let anisotropy_scale_mask_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_AnisotropyScaleMask"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let anisotropy_shift_noise_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_AnisotropyShiftNoiseMask"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let shadow_strength_mask_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_ShadowStrengthMask"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let shadow_border_mask_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_ShadowBorderMask"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let shadow_blur_mask_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_ShadowBlurMask"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let matcap_blend_mask_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_MatCapBlendMask"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let matcap_tex_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_MatCapTex", "_MatcapTex"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let matcap_bump_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_MatCapBumpMap"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let matcap2_blend_mask_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_MatCap2ndBlendMask"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let matcap2_tex_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_MatCap2ndTex"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let matcap2_bump_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_MatCap2ndBumpMap"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let alpha_mask_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_AlphaMask"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let main_color_adjust_params = liltoon_like
		.map(|u| u.main_color.main_texture_hsvg_factor)
		.unwrap_or([0.0, 1.0, 1.0, 1.0]);
	let main_gradation_params = liltoon_like
		.map(|u| {
			[
				u.main_color.gradation_enabled_factor.clamp(0.0, 1.0),
				u.main_color.gradation_strength_factor.clamp(0.0, 1.0),
				0.0,
				0.0,
			]
		})
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let main2nd_color = liltoon_like
		.map(|u| u.main_color.second_color_factor)
		.unwrap_or([1.0, 1.0, 1.0, 1.0]);
	let main2nd_params = liltoon_like
		.map(|u| {
			[
				u.main_color.second_enabled_factor.clamp(0.0, 1.0),
				u.main_color.second_enable_lighting_factor.clamp(0.0, 1.0),
				u.main_color.second_alpha_mode_factor.clamp(0.0, 4.0),
				liltoon_blend_mode_gpu(u.main_color.second_blend_mode),
			]
		})
		.unwrap_or([0.0, 1.0, 0.0, 1.0]);
	let main2nd_ext = liltoon_like
		.map(|u| {
			[
				u.texture_uv_mode_factors.get("_Main2ndTex").copied().unwrap_or(0.0).clamp(0.0, 4.0),
				u.main_color.second_cull_factor.clamp(0.0, 2.0),
				0.0,
				0.0,
			]
		})
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let main2nd_distance_fade = liltoon_like
		.map(|u| u.main_color.second_distance_fade_factor)
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let main2nd_decal_flags = liltoon_like
		.map(|u| u.main_color.second_decal_flags_factor)
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let main2nd_decal_transform = liltoon_like
		.map(|u| u.main_color.second_decal_transform_factor)
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let main2nd_decal_animation = liltoon_like
		.map(|u| u.main_color.second_decal_animation_factor)
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let main2nd_decal_sub_param = liltoon_like
		.map(|u| u.main_color.second_decal_sub_param_factor)
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let main2nd_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_Main2ndTex"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let main2nd_blend_mask_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_Main2ndBlendMask"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let main2nd_dissolve_color = liltoon_like
		.map(|u| u.main_color.second_dissolve.color_factor)
		.unwrap_or([1.0, 1.0, 1.0, 1.0]);
	let main2nd_dissolve_params = liltoon_like
		.map(|u| u.main_color.second_dissolve.params_factor)
		.unwrap_or([0.0, 0.0, 0.5, 0.1]);
	let main2nd_dissolve_pos = liltoon_like
		.map(|u| u.main_color.second_dissolve.position_factor)
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let main2nd_dissolve_ext = liltoon_like
		.map(|u| {
			[
				u.main_color.second_dissolve.noise_strength_factor,
				if u.main_color.second_dissolve.mask_texture_index.is_some() {
					1.0
				} else {
					0.0
				},
				if u.main_color.second_dissolve.noise_mask_texture_index.is_some() {
					1.0
				} else {
					0.0
				},
				0.0,
			]
		})
		.unwrap_or([0.1, 0.0, 0.0, 0.0]);
	let main2nd_dissolve_mask_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_Main2ndDissolveMask"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let main2nd_dissolve_noise_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_Main2ndDissolveNoiseMask"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let main2nd_dissolve_noise_uv_anim_params = liltoon_like
		.map(|u| u.main_color.second_dissolve.noise_uv_scroll_rotate_factor)
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let main3rd_color = liltoon_like
		.map(|u| u.main_color.third_color_factor)
		.unwrap_or([1.0, 1.0, 1.0, 1.0]);
	let main3rd_params = liltoon_like
		.map(|u| {
			[
				u.main_color.third_enabled_factor.clamp(0.0, 1.0),
				u.main_color.third_enable_lighting_factor.clamp(0.0, 1.0),
				u.main_color.third_alpha_mode_factor.clamp(0.0, 4.0),
				liltoon_blend_mode_gpu(u.main_color.third_blend_mode),
			]
		})
		.unwrap_or([0.0, 1.0, 0.0, 1.0]);
	let main3rd_ext = liltoon_like
		.map(|u| {
			[
				u.texture_uv_mode_factors.get("_Main3rdTex").copied().unwrap_or(0.0).clamp(0.0, 4.0),
				u.main_color.third_cull_factor.clamp(0.0, 2.0),
				0.0,
				0.0,
			]
		})
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let main3rd_distance_fade = liltoon_like
		.map(|u| u.main_color.third_distance_fade_factor)
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let main3rd_decal_flags = liltoon_like
		.map(|u| u.main_color.third_decal_flags_factor)
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let main3rd_decal_transform = liltoon_like
		.map(|u| u.main_color.third_decal_transform_factor)
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let main3rd_decal_animation = liltoon_like
		.map(|u| u.main_color.third_decal_animation_factor)
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let main3rd_decal_sub_param = liltoon_like
		.map(|u| u.main_color.third_decal_sub_param_factor)
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let main3rd_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_Main3rdTex"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let main3rd_blend_mask_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_Main3rdBlendMask"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let main3rd_dissolve_color = liltoon_like
		.map(|u| u.main_color.third_dissolve.color_factor)
		.unwrap_or([1.0, 1.0, 1.0, 1.0]);
	let main3rd_dissolve_params = liltoon_like
		.map(|u| u.main_color.third_dissolve.params_factor)
		.unwrap_or([0.0, 0.0, 0.5, 0.1]);
	let main3rd_dissolve_pos = liltoon_like
		.map(|u| u.main_color.third_dissolve.position_factor)
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let main3rd_dissolve_ext = liltoon_like
		.map(|u| {
			[
				u.main_color.third_dissolve.noise_strength_factor,
				if u.main_color.third_dissolve.mask_texture_index.is_some() {
					1.0
				} else {
					0.0
				},
				if u.main_color.third_dissolve.noise_mask_texture_index.is_some() {
					1.0
				} else {
					0.0
				},
				0.0,
			]
		})
		.unwrap_or([0.1, 0.0, 0.0, 0.0]);
	let main3rd_dissolve_mask_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_Main3rdDissolveMask"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let main3rd_dissolve_noise_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_Main3rdDissolveNoiseMask"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let main3rd_dissolve_noise_uv_anim_params = liltoon_like
		.map(|u| u.main_color.third_dissolve.noise_uv_scroll_rotate_factor)
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let outline = effective_mtoon_outline(mtoon, opts);
	let (outline_mode, outline_width, outline_color, outline_lighting_mix, outline_lit_color, outline_lit_params) =
		if let Some(liltoon_like) = liltoon_like {
			if liltoon_like.outline.enabled_factor > 0.5 && liltoon_like.outline.width_factor > 0.0 {
				(
					UnaMtoonOutlineWidthMode::WorldCoordinates,
					liltoon_like.outline.width_factor,
					liltoon_like.outline.color_factor,
					liltoon_like.outline.enable_lighting_factor,
					liltoon_like.outline.lit_color_factor,
					[
						liltoon_like.outline.lit_scale_factor,
						liltoon_like.outline.lit_offset_factor,
						liltoon_like.outline.lit_apply_tex_factor,
						liltoon_like.outline.lit_shadow_receive_factor,
					],
				)
			} else {
				(
					UnaMtoonOutlineWidthMode::None,
					0.0,
					[0.0, 0.0, 0.0, 0.0],
					0.0,
					[0.0, 0.0, 0.0, 0.0],
					[10.0, -8.0, 0.0, 0.0],
				)
			}
		} else {
			outline
				.map(|o| {
					(
						o.mode,
						o.width,
						[o.color[0], o.color[1], o.color[2], 1.0],
						o.lighting_mix,
						[0.0, 0.0, 0.0, 0.0],
						[10.0, -8.0, 0.0, 0.0],
					)
				})
				.unwrap_or((
					UnaMtoonOutlineWidthMode::None,
					0.0,
					[0.0, 0.0, 0.0, 0.0],
					0.0,
					[0.0, 0.0, 0.0, 0.0],
					[10.0, -8.0, 0.0, 0.0],
				))
		};
	let shade_color = liltoon_like.map(|u| u.shadow.color_factor).unwrap_or(mtoon.shade_color_factor);
	let shadow_params = liltoon_like
		.map(|u| {
			[
				u.shadow.enabled_factor.clamp(0.0, 1.0),
				(u.shadow.enabled_factor * u.shadow.strength_factor).clamp(0.0, 1.0),
				u.shadow.border_factor.clamp(0.0, 1.0),
				u.shadow.blur_factor.clamp(0.0, 1.0),
			]
		})
		.unwrap_or([0.0, 1.0, 0.5, 0.1]);
	let shadow_ext_params = liltoon_like
		.map(|u| {
			[
				u.shadow.border_range_factor.clamp(0.0, 1.0),
				u.shadow.main_strength_factor.clamp(0.0, 1.0),
				u.shadow.env_strength_factor.clamp(0.0, 1.0),
				u.shadow.normal_strength_factor.clamp(0.0, 1.0),
			]
		})
		.unwrap_or([0.0, 0.0, 0.0, 1.0]);
	let shadow_ao_params = liltoon_like
		.map(|u| [u.shadow.post_ao_factor.clamp(0.0, 1.0), 0.0, 0.0, 0.0])
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let shadow_ao_shift = liltoon_like.map(|u| u.shadow.ao_shift_factor).unwrap_or([1.0, 0.0, 1.0, 0.0]);
	let shadow_ao_shift2 = liltoon_like.map(|u| u.shadow.ao_shift2_factor).unwrap_or([1.0, 0.0, 0.0, 0.0]);
	let shadow_border_color = liltoon_like
		.map(|u| {
			[
				u.shadow.border_color_factor[0],
				u.shadow.border_color_factor[1],
				u.shadow.border_color_factor[2],
				1.0,
			]
		})
		.unwrap_or([1.0, 0.1, 0.0, 1.0]);
	let shadow2_color = liltoon_like.map(|u| u.shadow.second_color_factor).unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let shadow2_params = liltoon_like
		.map(|u| {
			[
				u.shadow.second_border_factor.clamp(0.0, 1.0),
				u.shadow.second_blur_factor.clamp(0.0, 1.0),
				u.shadow.second_normal_strength_factor.clamp(0.0, 1.0),
				u.shadow.second_receive_factor.clamp(0.0, 1.0),
			]
		})
		.unwrap_or([0.0, 0.0, 1.0, 0.0]);
	let shadow3_color = liltoon_like.map(|u| u.shadow.third_color_factor).unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let shadow3_params = liltoon_like
		.map(|u| {
			[
				u.shadow.third_border_factor.clamp(0.0, 1.0),
				u.shadow.third_blur_factor.clamp(0.0, 1.0),
				u.shadow.third_normal_strength_factor.clamp(0.0, 1.0),
				u.shadow.third_receive_factor.clamp(0.0, 1.0),
			]
		})
		.unwrap_or([0.0, 0.0, 1.0, 0.0]);
	let matcap_color = liltoon_like.map(|u| u.matcap.color_factor).unwrap_or(mtoon.matcap_factor);
	let matcap_params = liltoon_like
		.map(|u| {
			[
				(u.matcap.enabled_factor * u.matcap.blend_factor * u.matcap.color_alpha_factor).clamp(0.0, 1.0),
				u.matcap.main_strength_factor.clamp(0.0, 1.0),
				u.matcap.enable_lighting_factor.clamp(0.0, 1.0),
				match u.matcap.blend_mode {
					un_avatar_core::UnaLilToonLikeBlendMode::Normal => 0.0,
					un_avatar_core::UnaLilToonLikeBlendMode::Add => 1.0,
					un_avatar_core::UnaLilToonLikeBlendMode::Screen => 2.0,
					un_avatar_core::UnaLilToonLikeBlendMode::Multiply => 3.0,
				},
			]
		})
		.unwrap_or([1.0, 0.0, 0.0, 1.0]);
	let matcap_ext_params = liltoon_like
		.map(|u| {
			[
				u.matcap.normal_strength_factor.clamp(0.0, 1.0),
				u.matcap.shadow_mask_factor.clamp(0.0, 1.0),
				u.matcap.lod_factor.max(0.0),
				u.matcap.backface_mask_factor.clamp(0.0, 1.0),
			]
		})
		.unwrap_or([1.0, 0.0, 0.0, 0.0]);
	let matcap_bump_params = liltoon_like
		.map(|u| {
			[
				if u.matcap.bump_texture_index.is_some() {
					u.matcap.custom_normal_factor.clamp(0.0, 1.0)
				} else {
					0.0
				},
				u.matcap.bump_scale_factor,
				0.0,
				0.0,
			]
		})
		.unwrap_or([0.0, 1.0, 0.0, 0.0]);
	let matcap2_factor = liltoon_like.map(|u| u.matcap.second_color_factor).unwrap_or([1.0, 1.0, 1.0, 0.0]);
	let matcap2_params = liltoon_like
		.map(|u| {
			[
				(u.matcap.second_enabled_factor * u.matcap.second_blend_factor).clamp(0.0, 1.0),
				u.matcap.second_main_strength_factor.clamp(0.0, 1.0),
				u.matcap.second_enable_lighting_factor.clamp(0.0, 1.0),
				match u.matcap.second_blend_mode {
					un_avatar_core::UnaLilToonLikeBlendMode::Normal => 0.0,
					un_avatar_core::UnaLilToonLikeBlendMode::Add => 1.0,
					un_avatar_core::UnaLilToonLikeBlendMode::Screen => 2.0,
					un_avatar_core::UnaLilToonLikeBlendMode::Multiply => 3.0,
				},
			]
		})
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let matcap2_ext_params = liltoon_like
		.map(|u| {
			[
				u.matcap.second_normal_strength_factor.clamp(0.0, 1.0),
				u.matcap.second_shadow_mask_factor.clamp(0.0, 1.0),
				u.matcap.second_lod_factor.max(0.0),
				u.matcap.second_backface_mask_factor.clamp(0.0, 1.0),
			]
		})
		.unwrap_or([1.0, 0.0, 0.0, 0.0]);
	let matcap2_bump_params = liltoon_like
		.map(|u| {
			[
				if u.matcap.second_bump_texture_index.is_some() {
					u.matcap.second_custom_normal_factor.clamp(0.0, 1.0)
				} else {
					0.0
				},
				u.matcap.second_bump_scale_factor,
				0.0,
				0.0,
			]
		})
		.unwrap_or([0.0, 1.0, 0.0, 0.0]);
	let matcap_uv_params = liltoon_like
		.map(|u| {
			[
				u.matcap.perspective_factor.clamp(0.0, 1.0),
				u.matcap.z_rotation_cancel_factor.clamp(0.0, 1.0),
				u.matcap.second_perspective_factor.clamp(0.0, 1.0),
				u.matcap.second_z_rotation_cancel_factor.clamp(0.0, 1.0),
			]
		})
		.unwrap_or([1.0, 1.0, 1.0, 1.0]);
	let matcap_uv_ext_params = liltoon_like
		.map(|u| {
			[
				u.matcap.blend_uv1_factor[0].clamp(0.0, 1.0),
				u.matcap.blend_uv1_factor[1].clamp(0.0, 1.0),
				u.matcap.second_blend_uv1_factor[0].clamp(0.0, 1.0),
				u.matcap.second_blend_uv1_factor[1].clamp(0.0, 1.0),
			]
		})
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let reflection_color = liltoon_like.map(|u| u.reflection.color_factor).unwrap_or([1.0, 1.0, 1.0, 1.0]);
	let mut reflection_control = liltoon_like
		.map(|u| {
			[
				u.reflection.enabled_factor.clamp(0.0, 1.0),
				(u.reflection.enabled_factor * u.reflection.apply_specular_factor).clamp(0.0, 1.0),
				(u.reflection.enabled_factor * u.reflection.apply_reflection_factor).clamp(0.0, 1.0),
				match u.reflection.blend_mode {
					un_avatar_core::UnaLilToonLikeBlendMode::Normal => 0.0,
					un_avatar_core::UnaLilToonLikeBlendMode::Add => 1.0,
					un_avatar_core::UnaLilToonLikeBlendMode::Screen => 2.0,
					un_avatar_core::UnaLilToonLikeBlendMode::Multiply => 3.0,
				},
			]
		})
		.unwrap_or([0.0, 0.0, 0.0, 1.0]);
	if opts.debug_disable_reflection {
		reflection_control[0] = 0.0;
		reflection_control[1] = 0.0;
		reflection_control[2] = 0.0;
	}
	let reflection_params = liltoon_like
		.map(|u| {
			[
				u.reflection.smoothness_factor.clamp(0.0, 1.0),
				u.reflection.metallic_factor.clamp(0.0, 1.0),
				u.reflection.reflectance_factor.clamp(0.0, 1.0),
				u.reflection.reflection_normal_strength_factor.clamp(0.0, 1.0),
			]
		})
		.unwrap_or([0.0, 0.0, 0.0, 1.0]);
	let reflection_ext_params = liltoon_like
		.map(|u| {
			[
				u.reflection.cube_enable_lighting_factor.clamp(0.0, 1.0),
				u.reflection.gem_env_contrast_factor.max(0.0001),
				u.reflection.gem_refraction_fresnel_power_factor.max(0.0001),
				u.reflection.gem_env_color_factor[3].clamp(0.0, 1.0),
			]
		})
		.unwrap_or([1.0, 0.0, 0.0, 0.0]);
	let reflection_cube_color = liltoon_like
		.map(|u| {
			[
				u.reflection.cube_color_factor[0],
				u.reflection.cube_color_factor[1],
				u.reflection.cube_color_factor[2],
				u.reflection.cube_override_factor.clamp(0.0, 1.0),
			]
		})
		.unwrap_or([1.0, 1.0, 1.0, 0.0]);
	let anisotropy_params = liltoon_like
		.map(|u| {
			[
				u.reflection.anisotropy_enabled_factor.clamp(0.0, 1.0),
				u.reflection.anisotropy_scale_factor,
				u.reflection.anisotropy_to_reflection_factor.clamp(0.0, 1.0),
				u.reflection.anisotropy_to_matcap_factor.clamp(0.0, 1.0),
			]
		})
		.unwrap_or([0.0, 1.0, 0.0, 0.0]);
	let anisotropy_ext_params = liltoon_like
		.map(|u| {
			[
				u.reflection.anisotropy_to_second_matcap_factor.clamp(0.0, 1.0),
				u.reflection.anisotropy_shift_factor,
				u.reflection.anisotropy_shift_noise_scale_factor,
				u.reflection.anisotropy_specular_strength_factor.max(0.0),
			]
		})
		.unwrap_or([0.0, 0.0, 0.0, 1.0]);
	let anisotropy2_params = liltoon_like
		.map(|u| {
			[
				u.reflection.anisotropy_second_shift_factor,
				u.reflection.anisotropy_second_shift_noise_scale_factor,
				u.reflection.anisotropy_second_specular_strength_factor.max(0.0),
				0.0,
			]
		})
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let anisotropy_width_params = liltoon_like
		.map(|u| {
			[
				u.reflection.anisotropy_tangent_width_factor.max(0.0001),
				u.reflection.anisotropy_bitangent_width_factor.max(0.0001),
				u.reflection.anisotropy_second_tangent_width_factor.max(0.0001),
				u.reflection.anisotropy_second_bitangent_width_factor.max(0.0001),
			]
		})
		.unwrap_or([1.0, 1.0, 1.0, 1.0]);
	let gem_env_color = liltoon_like
		.map(|u| u.reflection.gem_env_color_factor)
		.unwrap_or([1.0, 1.0, 1.0, 1.0]);
	let gem_params = liltoon_like
		.map(|u| {
			if u.is_refraction_profile() {
				[
					u.reflection.gem_refraction_strength_factor,
					u.reflection.refraction_color_from_main_factor.clamp(0.0, 1.0),
					0.0,
					1.0,
				]
			} else {
				[
					u.reflection.gem_refraction_strength_factor,
					u.reflection.gem_chromatic_aberration_factor.max(0.0),
					u.reflection.gem_particle_loop_factor.max(0.0),
					u.reflection.gem_vr_parallax_strength_factor,
				]
			}
		})
		.unwrap_or([0.5, 0.02, 8.0, 1.0]);
	let gem_particle_color = liltoon_like
		.map(|u| {
			if u.is_refraction_profile() {
				u.reflection.refraction_color_factor
			} else {
				u.reflection.gem_particle_color_factor
			}
		})
		.unwrap_or([4.0, 4.0, 4.0, 1.0]);
	let specular_toon_params = liltoon_like
		.map(|u| {
			[
				u.reflection.specular_toon_factor.clamp(0.0, 1.0),
				u.reflection.specular_border_factor.clamp(0.0, 1.0),
				u.reflection.specular_blur_factor.clamp(0.0, 1.0),
				u.reflection.specular_normal_strength_factor.clamp(0.0, 1.0),
			]
		})
		.unwrap_or([0.0, 0.5, 0.0, 1.0]);
	let alpha_mask_params = liltoon_like
		.map(|u| {
			[
				u.alpha_mask.mode_factor.clamp(0.0, 4.0),
				u.alpha_mask.scale_factor,
				u.alpha_mask.value_factor,
				1.0,
			]
		})
		.unwrap_or([0.0, 1.0, 0.0, 1.0]);
	let fur_params = liltoon_like
		.map(|u| {
			[
				u.fur.enabled_factor.clamp(0.0, 1.0),
				liltoon_fur_sample_count_for_layer_num(u.fur.layer_count_factor) as f32,
				u.fur.gravity_factor,
				u.fur.randomize_factor.clamp(0.0, 1.0),
			]
		})
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let fur_vector_params = liltoon_like
		.map(|u| {
			[
				u.fur.vector_factor[0],
				u.fur.vector_factor[1],
				u.fur.vector_factor[2],
				u.fur.vector_factor[3],
			]
		})
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let fur_noise_params = liltoon_like
		.map(|u| {
			if let Some(st) = texture_slot_uv_offset_scale(u, &["_FurNoiseMask"]) {
				[st[2].max(0.0), st[3].max(0.0), st[0], st[1]]
			} else {
				[
					u.fur.noise_tiling_factor.max(0.0),
					u.fur.noise_tiling_factor.max(0.0),
					u.fur.noise_offset_factor,
					u.fur.noise_offset_factor,
				]
			}
		})
		.unwrap_or([1.0, 1.0, 0.0, 0.0]);
	let fur_ext_params = liltoon_like
		.map(|u| {
			[
				u.fur.vector_scale_factor,
				u.fur.shell_ao_factor.clamp(0.0, 1.0),
				u.fur.root_offset_factor.clamp(-1.0, 0.0),
				u.fur.cutout_length_factor.max(0.0),
			]
		})
		.unwrap_or([1.0, 0.0, 0.0, 0.8]);
	let fur_rim_color = liltoon_like.map(|u| u.fur.rim_color_factor).unwrap_or([0.0, 0.0, 0.0, 1.0]);
	let fur_rim_params = liltoon_like
		.map(|u| {
			[
				u.fur.rim_fresnel_power_factor.clamp(0.01, 50.0),
				u.fur.rim_anti_light_factor.clamp(0.0, 1.0),
				if u.fur.vector_texture_index.is_some() { 1.0 } else { 0.0 },
				u.fur.vertex_color_to_vector_factor.clamp(0.0, 1.0),
			]
		})
		.unwrap_or([3.0, 0.5, 0.0, 0.0]);
	let alpha_ext_params = liltoon_like
		.map(|u| {
			[
				u.blend_state.subpass_cutoff_factor.clamp(0.0, 1.0),
				u.rendering.aa_strength_factor.max(0.0),
				u.blend_state.pre_cutoff_factor.clamp(0.0, 1.0),
				u.blend_state.pre_cull_factor.clamp(0.0, 2.0),
			]
		})
		.unwrap_or([0.5, 1.0, 0.0, 2.0]);
	let lighting_ext_params = liltoon_like
		.map(|u| {
			[
				u.rendering.light_min_limit_factor.max(0.0),
				u.rendering.light_max_limit_factor.max(u.rendering.light_min_limit_factor).max(0.0),
				u.rendering.monochrome_lighting_factor.clamp(0.0, 1.0),
				u.rendering.vertex_light_strength_factor.clamp(0.0, 1.0),
			]
		})
		.unwrap_or([0.0, 1.0, 0.0, 0.0]);
	let rendering_ext_params = liltoon_like
		.map(|u| {
			[
				u.rendering.gsaa_strength_factor.max(0.0),
				if liltoon_reflection_texture_index(u).is_some() { 1.0 } else { 0.0 },
				u.rendering.as_unlit_factor.clamp(0.0, 1.0),
				0.0,
			]
		})
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let transparency_params = liltoon_like
		.map(|u| {
			[
				u.matcap.apply_transparency_factor.clamp(0.0, 1.0),
				u.matcap.second_apply_transparency_factor.clamp(0.0, 1.0),
				u.rim.apply_transparency_factor.clamp(0.0, 1.0),
				u.reflection.apply_transparency_factor.clamp(0.0, 1.0),
			]
		})
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let material_ext_params = liltoon_like
		.map(|u| [u.flip_backface_normal_factor.clamp(0.0, 1.0), 0.0, 0.0, 0.0])
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let outline_ext_params = liltoon_like
		.map(|u| [u.outline.fix_width_factor.clamp(0.0, 1.0), u.outline.z_bias_factor, 0.0, 0.0])
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let rim_color_factor = liltoon_like.map(|u| u.rim.color_factor);
	let rim_color_gpu = rim_color_factor.unwrap_or([rim_color[0], rim_color[1], rim_color[2], 1.0]);
	let rim_params = liltoon_like
		.map(|u| {
			[
				u.rim.border_factor.clamp(0.0, 1.0),
				u.rim.blur_factor.clamp(0.0, 1.0),
				u.rim.fresnel_power_factor.clamp(0.01, 50.0),
				u.rim.enabled_factor.clamp(0.0, 1.0),
			]
		})
		.unwrap_or([rim_lighting_mix, rim_power, rim_lift, rim_texture_mix]);
	let rim_control = liltoon_like
		.map(|u| {
			[
				(u.rim.enabled_factor * u.rim.color_factor[3]).clamp(0.0, 1.0),
				u.rim.main_strength_factor.clamp(0.0, 1.0),
				u.rim.enable_lighting_factor.clamp(0.0, 1.0),
				match u.rim.blend_mode {
					un_avatar_core::UnaLilToonLikeBlendMode::Normal => 0.0,
					un_avatar_core::UnaLilToonLikeBlendMode::Add => 1.0,
					un_avatar_core::UnaLilToonLikeBlendMode::Screen => 2.0,
					un_avatar_core::UnaLilToonLikeBlendMode::Multiply => 3.0,
				},
			]
		})
		.unwrap_or([1.0, 0.0, 0.0, 1.0]);
	let rim_ext_params = liltoon_like
		.map(|u| {
			[
				u.rim.shadow_mask_factor.clamp(0.0, 1.0),
				u.rim.normal_strength_factor.clamp(0.0, 1.0),
				u.rim.backface_mask_factor.clamp(0.0, 1.0),
				u.rim.shade_normal_strength_factor.clamp(0.0, 1.0),
			]
		})
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let rim_indirect_color = liltoon_like.map(|u| u.rim.indirect_color_factor).unwrap_or([1.0, 1.0, 1.0, 1.0]);
	let rim_indirect_params = liltoon_like
		.map(|u| {
			[
				u.rim.directional_strength_factor.clamp(0.0, 1.0),
				u.rim.directional_range_factor.clamp(-1.0, 1.0),
				u.rim.indirect_range_factor.clamp(-1.0, 1.0),
				u.rim.indirect_border_factor.clamp(0.0, 1.0),
			]
		})
		.unwrap_or([0.0, 0.0, 0.0, 0.5]);
	let rim_indirect_ext_params = liltoon_like
		.map(|u| [u.rim.indirect_blur_factor.clamp(0.0, 1.0), 0.0, 0.0, 0.0])
		.unwrap_or([0.1, 0.0, 0.0, 0.0]);
	let rim_shade_color = liltoon_like.map(|u| u.rim.shade_color_factor).unwrap_or([0.5, 0.5, 0.5, 1.0]);
	let rim_shade_params = liltoon_like
		.map(|u| {
			[
				u.rim.shade_enabled_factor.clamp(0.0, 1.0),
				u.rim.shade_border_factor.clamp(0.0, 1.0),
				u.rim.shade_blur_factor.clamp(0.0, 1.0),
				u.rim.shade_fresnel_power_factor.clamp(0.01, 50.0),
			]
		})
		.unwrap_or([0.0, 0.5, 0.65, 3.5]);
	let backlight_color = liltoon_like.map(|u| u.backlight.color_factor).unwrap_or([0.85, 0.8, 0.7, 1.0]);
	let backlight_params = liltoon_like
		.map(|u| {
			[
				u.backlight.enabled_factor.clamp(0.0, 1.0),
				u.backlight.main_strength_factor.clamp(0.0, 1.0),
				u.backlight.normal_strength_factor.clamp(0.0, 1.0),
				u.backlight.directivity_factor.max(0.0),
			]
		})
		.unwrap_or([0.0, 0.0, 1.0, 5.0]);
	let backlight_ext_params = liltoon_like
		.map(|u| {
			[
				u.backlight.border_factor.clamp(0.0, 1.0),
				u.backlight.blur_factor.clamp(0.0, 1.0),
				u.backlight.view_strength_factor.clamp(0.0, 1.0),
				u.backlight.backface_mask_factor.clamp(0.0, 1.0),
			]
		})
		.unwrap_or([0.35, 0.05, 1.0, 1.0]);
	let backlight_shadow_params = liltoon_like
		.map(|u| [u.backlight.receive_shadow_factor.clamp(0.0, 1.0), 0.0, 0.0, 0.0])
		.unwrap_or([1.0, 0.0, 0.0, 0.0]);
	let backlight_color_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_BacklightColorTex"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let glitter_color = liltoon_like.map(|u| u.glitter.color_factor).unwrap_or([1.0, 1.0, 1.0, 1.0]);
	let glitter_params1 = liltoon_like.map(|u| u.glitter.params1_factor).unwrap_or([256.0, 256.0, 0.16, 50.0]);
	let glitter_params2 = liltoon_like.map(|u| u.glitter.params2_factor).unwrap_or([0.25, 0.0, 0.0, 0.0]);
	let glitter_control = liltoon_like
		.map(|u| {
			[
				u.glitter.enabled_factor.clamp(0.0, 1.0),
				u.glitter.main_strength_factor.clamp(0.0, 1.0),
				u.glitter.normal_strength_factor.clamp(0.0, 1.0),
				u.glitter.post_contrast_factor.max(0.0),
			]
		})
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let glitter_ext = liltoon_like
		.map(|u| {
			[
				u.glitter.sensitivity_factor.max(0.0),
				u.glitter.enable_lighting_factor.clamp(0.0, 1.0),
				u.glitter.shadow_mask_factor.clamp(0.0, 1.0),
				u.glitter.apply_transparency_factor.clamp(0.0, 1.0),
			]
		})
		.unwrap_or([0.25, 1.0, 0.0, 1.0]);
	let glitter_ext2 = liltoon_like
		.map(|u| {
			[
				u.glitter.backface_mask_factor.clamp(0.0, 1.0),
				u.glitter.scale_randomize_factor.clamp(0.0, 1.0),
				u.glitter.uv_mode_factor.clamp(0.0, 1.0),
				u.glitter.color_texture_uv_mode_factor.clamp(0.0, 3.0),
			]
		})
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let glitter_ext3 = liltoon_like
		.map(|u| {
			[
				u.glitter.vr_parallax_strength_factor.clamp(0.0, 1.0),
				u.glitter.apply_shape_factor.clamp(0.0, 1.0),
				u.glitter.angle_randomize_factor.clamp(0.0, 1.0),
				0.0,
			]
		})
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let glitter_color_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_GlitterColorTex"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let glitter_shape_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_GlitterShapeTex"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let glitter_atlas = liltoon_like.map(|u| u.glitter.atlas_factor).unwrap_or([1.0, 1.0, 0.0, 0.0]);
	let distance_fade = liltoon_like
		.map(|u| u.rendering.distance_fade_factor)
		.unwrap_or([0.1, 0.01, 0.0, 0.0]);
	let backface_color = liltoon_like
		.map(|u| u.rendering.backface_color_factor)
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let distance_fade_color = liltoon_like
		.map(|u| u.rendering.distance_fade_color_factor)
		.unwrap_or([0.0, 0.0, 0.0, 1.0]);
	let distance_fade_rim_color = liltoon_like
		.map(|u| u.rendering.distance_fade_rim_color_factor)
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let distance_fade_params = liltoon_like
		.map(|u| {
			[
				u.rendering.distance_fade_mode_factor.clamp(0.0, 1.0),
				u.rendering.distance_fade_rim_fresnel_power_factor.max(0.00001),
				0.0,
				0.0,
			]
		})
		.unwrap_or([0.0, 5.0, 0.0, 0.0]);
	let dissolve_color = liltoon_like.map(|u| u.dissolve.color_factor).unwrap_or([1.0, 1.0, 1.0, 1.0]);
	let dissolve_params = liltoon_like.map(|u| u.dissolve.params_factor).unwrap_or([0.0, 0.0, 0.5, 0.1]);
	let dissolve_pos = liltoon_like.map(|u| u.dissolve.position_factor).unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let dissolve_ext = liltoon_like
		.map(|u| {
			[
				u.dissolve.noise_strength_factor,
				if u.dissolve.mask_texture_index.is_some() { 1.0 } else { 0.0 },
				if u.dissolve.noise_mask_texture_index.is_some() { 1.0 } else { 0.0 },
				0.0,
			]
		})
		.unwrap_or([0.1, 0.0, 0.0, 0.0]);
	let dissolve_mask_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_DissolveMask"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let dissolve_noise_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_DissolveNoiseMask"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let dissolve_noise_uv_anim_params = liltoon_like
		.map(|u| u.dissolve.noise_uv_scroll_rotate_factor)
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let parallax_params = liltoon_like
		.map(|u| {
			[
				u.parallax.enabled_factor.clamp(0.0, 1.0),
				u.parallax.pom_enabled_factor.clamp(0.0, 1.0),
				u.parallax.scale_factor,
				u.parallax.offset_factor,
			]
		})
		.unwrap_or([0.0, 0.0, 0.02, 0.5]);
	let parallax_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_ParallaxMap"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let id_mask_params = liltoon_like
		.map(|u| {
			[
				u.id_mask
					.compile_factor
					.max(if liltoon_features::id_mask_has_runtime_controls(&u.id_mask) {
						1.0
					} else {
						0.0
					})
					.clamp(0.0, 1.0),
				u.id_mask.from_factor.clamp(0.0, 8.0),
				u.id_mask.is_bitmap_factor.clamp(0.0, 1.0),
				u.id_mask.controls_dissolve_factor.clamp(0.0, 1.0),
			]
		})
		.unwrap_or([0.0, 8.0, 0.0, 0.0]);
	let id_mask_flags0 = liltoon_like
		.map(|u| {
			[
				u.id_mask.flags_factor[0],
				u.id_mask.flags_factor[1],
				u.id_mask.flags_factor[2],
				u.id_mask.flags_factor[3],
			]
		})
		.unwrap_or([0.0; 4]);
	let id_mask_flags1 = liltoon_like
		.map(|u| {
			[
				u.id_mask.flags_factor[4],
				u.id_mask.flags_factor[5],
				u.id_mask.flags_factor[6],
				u.id_mask.flags_factor[7],
			]
		})
		.unwrap_or([0.0; 4]);
	let id_mask_prior_flags0 = liltoon_like
		.map(|u| {
			[
				u.id_mask.prior_flags_factor[0],
				u.id_mask.prior_flags_factor[1],
				u.id_mask.prior_flags_factor[2],
				u.id_mask.prior_flags_factor[3],
			]
		})
		.unwrap_or([0.0; 4]);
	let id_mask_prior_flags1 = liltoon_like
		.map(|u| {
			[
				u.id_mask.prior_flags_factor[4],
				u.id_mask.prior_flags_factor[5],
				u.id_mask.prior_flags_factor[6],
				u.id_mask.prior_flags_factor[7],
			]
		})
		.unwrap_or([0.0; 4]);
	let id_mask_indices0 = liltoon_like
		.map(|u| {
			[
				u.id_mask.indices_factor[0] as f32,
				u.id_mask.indices_factor[1] as f32,
				u.id_mask.indices_factor[2] as f32,
				u.id_mask.indices_factor[3] as f32,
			]
		})
		.unwrap_or([0.0; 4]);
	let id_mask_indices1 = liltoon_like
		.map(|u| {
			[
				u.id_mask.indices_factor[4] as f32,
				u.id_mask.indices_factor[5] as f32,
				u.id_mask.indices_factor[6] as f32,
				u.id_mask.indices_factor[7] as f32,
			]
		})
		.unwrap_or([0.0; 4]);
	let udim_discard_params = liltoon_like
		.map(|u| {
			[
				u.udim_discard
					.compile_factor
					.max(if liltoon_features::udim_discard_has_runtime_rows(&u.udim_discard) {
						1.0
					} else {
						0.0
					})
					.clamp(0.0, 1.0),
				u.udim_discard.mode_factor.clamp(0.0, 1.0),
				u.udim_discard.uv_factor.clamp(0.0, 3.0),
				0.0,
			]
		})
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let udim_discard_row0 = liltoon_like.map(|u| u.udim_discard.row0_factor).unwrap_or([0.0; 4]);
	let udim_discard_row1 = liltoon_like.map(|u| u.udim_discard.row1_factor).unwrap_or([0.0; 4]);
	let udim_discard_row2 = liltoon_like.map(|u| u.udim_discard.row2_factor).unwrap_or([0.0; 4]);
	let udim_discard_row3 = liltoon_like.map(|u| u.udim_discard.row3_factor).unwrap_or([0.0; 4]);
	let emission_color = liltoon_like.map(|u| u.emission.color_factor).unwrap_or([
		mat.emissive_factor[0],
		mat.emissive_factor[1],
		mat.emissive_factor[2],
		1.0,
	]);
	let emission_params = liltoon_like
		.map(|u| {
			[
				u.emission.enabled_factor.clamp(0.0, 1.0),
				u.emission.main_strength_factor.clamp(0.0, 1.0),
				u.emission.blend_factor.clamp(0.0, 1.0),
				match u.emission.blend_mode {
					un_avatar_core::UnaLilToonLikeBlendMode::Normal => 0.0,
					un_avatar_core::UnaLilToonLikeBlendMode::Add => 1.0,
					un_avatar_core::UnaLilToonLikeBlendMode::Screen => 2.0,
					un_avatar_core::UnaLilToonLikeBlendMode::Multiply => 3.0,
				},
			]
		})
		.unwrap_or([1.0, 0.0, 1.0, 1.0]);
	let emission_blink_params = liltoon_like
		.map(|u| u.emission.blink_factor)
		.unwrap_or([0.0, 0.0, std::f32::consts::PI, 0.0]);
	let emission_uv_anim_params = liltoon_like
		.map(|u| {
			let mut params = u.emission.uv_scroll_rotate_factor;
			params[3] = u
				.texture_uv_mode_factors
				.get("_EmissionMap")
				.copied()
				.unwrap_or(0.0)
				.clamp(0.0, 4.0);
			params
		})
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let emission_grad_params = liltoon_like
		.map(|u| {
			[
				u.emission.gradation_enabled_factor.clamp(0.0, 1.0),
				u.emission.gradation_speed_factor,
				u.emission.fluorescence_factor.clamp(0.0, 1.0),
				u.emission.parallax_depth_factor,
			]
		})
		.unwrap_or([0.0, 1.0, 0.0, 0.0]);
	let emission_blend_mask_uv_anim_params = liltoon_like
		.map(|u| u.emission.blend_mask_uv_scroll_rotate_factor)
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let emission2nd_color = liltoon_like.map(|u| u.emission.second_color_factor).unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let emission2nd_params = liltoon_like
		.map(|u| {
			[
				u.emission.second_enabled_factor.clamp(0.0, 1.0),
				u.emission.second_main_strength_factor.clamp(0.0, 1.0),
				u.emission.second_blend_factor.clamp(0.0, 1.0),
				liltoon_blend_mode_gpu(u.emission.second_blend_mode),
			]
		})
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let emission2nd_blink_params = liltoon_like
		.map(|u| u.emission.second_blink_factor)
		.unwrap_or([0.0, 0.0, std::f32::consts::PI, 0.0]);
	let emission2nd_grad_params = liltoon_like
		.map(|u| {
			[
				u.emission.second_gradation_enabled_factor.clamp(0.0, 1.0),
				u.emission.second_gradation_speed_factor,
				u.emission.second_fluorescence_factor.clamp(0.0, 1.0),
				0.0,
			]
		})
		.unwrap_or([0.0, 1.0, 0.0, 0.0]);
	let emission2nd_ext_params = liltoon_like
		.map(|u| [u.emission.second_parallax_depth_factor, 0.0, 0.0, 0.0])
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let emission2nd_uv_anim_params = liltoon_like
		.map(|u| {
			let mut params = u.emission.second_uv_scroll_rotate_factor;
			params[3] = u
				.texture_uv_mode_factors
				.get("_Emission2ndMap")
				.copied()
				.unwrap_or(0.0)
				.clamp(0.0, 4.0);
			params
		})
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let emission2nd_blend_mask_uv_anim_params = liltoon_like
		.map(|u| u.emission.second_blend_mask_uv_scroll_rotate_factor)
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let audio_link_params = liltoon_like
		.map(|u| {
			[
				u.audio_link.enabled_factor.clamp(0.0, 1.0),
				u.audio_link.uv_mode_factor.clamp(0.0, 5.0),
				u.audio_link.to_emission_factor.clamp(0.0, 1.0),
				u.audio_link.to_emission_gradation_factor.clamp(0.0, 1.0),
			]
		})
		.unwrap_or([0.0, 1.0, 0.0, 0.0]);
	let audio_link_default = liltoon_like
		.map(|u| u.audio_link.default_value_factor)
		.unwrap_or([0.0, 0.0, 2.0, 0.75]);
	let audio_link_uv_params = liltoon_like
		.map(|u| u.audio_link.uv_params_factor)
		.unwrap_or([0.25, 0.0, 0.0, 0.125]);
	let audio_link_start = liltoon_like.map(|u| u.audio_link.start_factor).unwrap_or([0.0; 4]);
	let audio_link_ext = liltoon_like
		.map(|u| {
			[
				u.audio_link.to_emission_second_factor.clamp(0.0, 1.0),
				u.audio_link.to_emission_second_gradation_factor.clamp(0.0, 1.0),
				u.audio_link.to_main_second_factor.clamp(0.0, 1.0),
				u.audio_link.to_main_third_factor.clamp(0.0, 1.0),
			]
		})
		.unwrap_or([0.0, 0.0, 0.0, 0.0]);
	let audio_link_vertex_params = liltoon_like
		.map(|u| {
			[
				u.audio_link.to_vertex_factor.clamp(0.0, 1.0),
				u.audio_link.vertex_uv_mode_factor.clamp(0.0, 3.0),
				u.audio_link.as_local_factor.clamp(0.0, 1.0),
				0.0,
			]
		})
		.unwrap_or([0.0, 1.0, 0.0, 0.0]);
	let audio_link_vertex_uv_params = liltoon_like
		.map(|u| u.audio_link.vertex_uv_params_factor)
		.unwrap_or([0.25, 0.0, 0.0, 0.125]);
	let audio_link_vertex_start = liltoon_like.map(|u| u.audio_link.vertex_start_factor).unwrap_or([0.0; 4]);
	let audio_link_vertex_strength = liltoon_like
		.map(|u| u.audio_link.vertex_strength_factor)
		.unwrap_or([0.0, 0.0, 0.0, 1.0]);
	let audio_link_mask_texture_present = liltoon_like
		.map(|u| if u.audio_link.mask_texture_index.is_some() { 1.0 } else { 0.0 })
		.unwrap_or(0.0);
	let audio_link_local_map_texture_present = liltoon_like
		.map(|u| if u.audio_link.local_map_texture_index.is_some() { 1.0 } else { 0.0 })
		.unwrap_or(0.0);
	let audio_link_mask_params = liltoon_like
		.map(|u| {
			[
				u.audio_link.mask_uv_mode_factor.clamp(0.0, 3.0),
				audio_link_mask_texture_present,
				audio_link_local_map_texture_present,
				0.0,
			]
		})
		.unwrap_or([0.0; 4]);
	let audio_link_mask_uv_offset_scale = liltoon_like
		.and_then(|u| texture_slot_uv_offset_scale(u, &["_AudioLinkMask"]))
		.unwrap_or([0.0, 0.0, 1.0, 1.0]);
	let audio_link_mask_uv_anim_params = liltoon_like.map(|u| u.audio_link.mask_uv_scroll_rotate_factor).unwrap_or([0.0; 4]);
	let audio_link_local_map_params = liltoon_like
		.map(|u| u.audio_link.local_map_params_factor)
		.unwrap_or([120.0, 1.0, 0.0, 0.0]);
	MeshDrawMaterialGpu {
		base_color,
		backface_color,
		params: [0.0, eff_alpha.as_shader_alpha_kind(), mat.alpha_cutoff, f32::from_bits(flags)],
		shade_color: [
			shade_color[0],
			shade_color[1],
			shade_color[2],
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
		shadow_params,
		shadow_ext_params,
		shadow_ao_params,
		shadow_ao_shift,
		shadow_ao_shift2,
		shadow_border_color,
		shadow2_color,
		shadow2_params,
		shadow3_color,
		shadow3_params,
		matcap_factor: [matcap_color[0], matcap_color[1], matcap_color[2], 1.0],
		matcap_params,
		matcap_ext_params,
		matcap_bump_params,
		matcap2_factor,
		matcap2_params,
		matcap2_ext_params,
		matcap2_bump_params,
		matcap_uv_params,
		matcap_uv_ext_params,
		reflection_color,
		reflection_control,
		reflection_params,
		reflection_ext_params,
		reflection_cube_color,
		anisotropy_params,
		anisotropy_ext_params,
		anisotropy2_params,
		anisotropy_width_params,
		gem_env_color,
		gem_params,
		gem_particle_color,
		specular_toon_params,
		rim_color: [
			rim_color_gpu[0],
			rim_color_gpu[1],
			rim_color_gpu[2],
			mat.occlusion_texture_strength.clamp(0.0, 2.0),
		],
		rim_params,
		rim_control,
		rim_ext_params,
		rim_indirect_color,
		rim_indirect_params,
		rim_indirect_ext_params,
		rim_shade_color,
		rim_shade_params,
		backlight_color,
		backlight_params,
		backlight_ext_params,
		backlight_shadow_params,
		backlight_color_uv_offset_scale,
		glitter_color,
		glitter_params1,
		glitter_params2,
		glitter_control,
		glitter_ext,
		glitter_ext2,
		glitter_ext3,
		glitter_color_uv_offset_scale,
		glitter_shape_uv_offset_scale,
		glitter_atlas,
		distance_fade,
		distance_fade_color,
		distance_fade_rim_color,
		distance_fade_params,
		dissolve_color,
		dissolve_params,
		dissolve_pos,
		dissolve_ext,
		dissolve_mask_uv_offset_scale,
		dissolve_noise_uv_offset_scale,
		dissolve_noise_uv_anim_params,
		parallax_params,
		parallax_uv_offset_scale,
		id_mask_params,
		id_mask_flags0,
		id_mask_flags1,
		id_mask_prior_flags0,
		id_mask_prior_flags1,
		id_mask_indices0,
		id_mask_indices1,
		udim_discard_params,
		udim_discard_row0,
		udim_discard_row1,
		udim_discard_row2,
		udim_discard_row3,
		emission_color,
		emission_params,
		emission_blink_params,
		emission_grad_params,
		emission2nd_color,
		emission2nd_params,
		emission2nd_blink_params,
		emission2nd_grad_params,
		emission2nd_ext_params,
		emission2nd_uv_offset_scale,
		emission2nd_uv_anim_params,
		emission_blend_mask_uv_offset_scale,
		emission_blend_mask_uv_anim_params,
		emission2nd_blend_mask_uv_offset_scale,
		emission2nd_blend_mask_uv_anim_params,
		audio_link_params,
		audio_link_default,
		audio_link_uv_params,
		audio_link_start,
		audio_link_ext,
		audio_link_vertex_params,
		audio_link_vertex_uv_params,
		audio_link_vertex_start,
		audio_link_vertex_strength,
		audio_link_mask_params,
		audio_link_mask_uv_offset_scale,
		audio_link_mask_uv_anim_params,
		audio_link_local_map_params,
		outline_color,
		outline_params: [
			outline_mode_gpu(outline_mode),
			outline_width,
			outline_lighting_mix,
			if mtoon.transparent_with_z_write { 1.0 } else { 0.0 },
		],
		outline_lit_color,
		outline_lit_params,
		outline_ext_params,
		alpha_mask_params,
		fur_params,
		fur_vector_params,
		fur_noise_params,
		fur_ext_params,
		fur_rim_color,
		fur_rim_params,
		alpha_ext_params,
		lighting_ext_params,
		rendering_ext_params,
		transparency_params,
		material_ext_params,
		emissive_factor: [mat.emissive_factor[0], mat.emissive_factor[1], mat.emissive_factor[2], 24.0],
		uv_anim_params: [
			mtoon.uv_animation_scroll_x_speed_factor,
			mtoon.uv_animation_scroll_y_speed_factor,
			mtoon.uv_animation_rotation_speed_factor,
			0.0,
		],
		uv_offset_scale: mat.uv_offset_scale,
		normal_uv_offset_scale,
		normal2nd_uv_offset_scale,
		normal2nd_scale_mask_uv_offset_scale,
		normal2nd_params,
		shade_uv_offset_scale,
		rim_uv_offset_scale,
		emission_uv_offset_scale,
		emission_uv_anim_params,
		reflection_color_uv_offset_scale,
		smoothness_uv_offset_scale,
		metallic_uv_offset_scale,
		anisotropy_tangent_uv_offset_scale,
		anisotropy_scale_mask_uv_offset_scale,
		anisotropy_shift_noise_uv_offset_scale,
		shadow_strength_mask_uv_offset_scale,
		shadow_border_mask_uv_offset_scale,
		shadow_blur_mask_uv_offset_scale,
		matcap_blend_mask_uv_offset_scale,
		matcap_tex_uv_offset_scale,
		matcap_bump_uv_offset_scale,
		matcap2_blend_mask_uv_offset_scale,
		matcap2_tex_uv_offset_scale,
		matcap2_bump_uv_offset_scale,
		alpha_mask_uv_offset_scale,
		main_color_adjust_params,
		main_gradation_params,
		main2nd_color,
		main2nd_params,
		main2nd_ext,
		main2nd_distance_fade,
		main2nd_decal_flags,
		main2nd_decal_transform,
		main2nd_decal_animation,
		main2nd_decal_sub_param,
		main2nd_uv_offset_scale,
		main2nd_blend_mask_uv_offset_scale,
		main2nd_dissolve_color,
		main2nd_dissolve_params,
		main2nd_dissolve_pos,
		main2nd_dissolve_ext,
		main2nd_dissolve_mask_uv_offset_scale,
		main2nd_dissolve_noise_uv_offset_scale,
		main2nd_dissolve_noise_uv_anim_params,
		main3rd_color,
		main3rd_params,
		main3rd_ext,
		main3rd_distance_fade,
		main3rd_decal_flags,
		main3rd_decal_transform,
		main3rd_decal_animation,
		main3rd_decal_sub_param,
		main3rd_uv_offset_scale,
		main3rd_blend_mask_uv_offset_scale,
		main3rd_dissolve_color,
		main3rd_dissolve_params,
		main3rd_dissolve_pos,
		main3rd_dissolve_ext,
		main3rd_dissolve_mask_uv_offset_scale,
		main3rd_dissolve_noise_uv_offset_scale,
		main3rd_dissolve_noise_uv_anim_params,
	}
}

#[cfg(test)]
fn mesh_draw_material_gpu(
	mat: &UnaMaterialPbr,
	mtoon: &UnaMtoonMaterial,
	opts: &SceneMeshLoadOpts,
	mesh_index: usize,
	prim_index: usize,
) -> MeshDrawMaterialGpu {
	mesh_draw_material_gpu_with_profiles(mat, mtoon, mat.liltoon_like_source_profile(), opts, mesh_index, prim_index)
}

fn mesh_draw_material_gpu_runtime(
	mat: &UnaMaterialPbr,
	default_mtoon: &UnaMtoonMaterial,
	opts: &SceneMeshLoadOpts,
	mesh_index: usize,
	prim_index: usize,
) -> MeshDrawMaterialGpu {
	let mtoon = mat.mtoon_like_runtime().unwrap_or(default_mtoon);
	mesh_draw_material_gpu_with_profiles(mat, mtoon, mat.liltoon_like_runtime(), opts, mesh_index, prim_index)
}

fn liltoon_blend_mode_gpu(mode: un_avatar_core::UnaLilToonLikeBlendMode) -> f32 {
	match mode {
		un_avatar_core::UnaLilToonLikeBlendMode::Normal => 0.0,
		un_avatar_core::UnaLilToonLikeBlendMode::Add => 1.0,
		un_avatar_core::UnaLilToonLikeBlendMode::Screen => 2.0,
		un_avatar_core::UnaLilToonLikeBlendMode::Multiply => 3.0,
	}
}

fn texture_slot_uv_offset_scale(liltoon_like: &un_avatar_core::UnaLilToonLikeMaterial, keys: &[&str]) -> Option<[f32; 4]> {
	keys.iter().find_map(|key| liltoon_like.texture_uv_offset_scales.get(*key).copied())
}

impl SceneMeshes {
	pub(crate) fn diagnostic_morph_state(&self, scene: &UnaSceneSnapshot, filter: Option<&str>, max_draws: usize) -> serde_json::Value {
		let mut paths: Vec<Option<String>> = vec![None; scene.nodes.len()];
		fn walk(scene: &UnaSceneSnapshot, node: usize, prefix: String, paths: &mut [Option<String>]) {
			let Some(scene_node) = scene.nodes.get(node) else {
				return;
			};
			let name = scene_node.name.as_deref().unwrap_or("<unnamed>");
			let path = if prefix.is_empty() {
				name.to_string()
			} else {
				format!("{prefix}/{name}")
			};
			paths[node] = Some(path.clone());
			for child in &scene_node.children {
				walk(scene, *child, path.clone(), paths);
			}
		}
		let child_nodes = sorted_unique_indices(scene.nodes.iter().flat_map(|node| node.children.iter().copied()).collect());
		for index in 0..scene.nodes.len() {
			if child_nodes.binary_search(&index).is_err() {
				walk(scene, index, String::new(), &mut paths);
			}
		}
		let mut matched_draw_count = 0usize;
		let mut draws = Vec::new();
		for (draw_index, draw) in self.draws.iter().enumerate() {
			if draw.morph_target_count == 0 {
				continue;
			}
			let node_path = paths.get(draw.world_node_index).cloned().flatten().unwrap_or_default();
			if let Some(filter) = filter.as_deref() {
				if !dynamics_token_filter_matches(&node_path, filter) {
					continue;
				}
			}
			matched_draw_count += 1;
			if draws.len() >= max_draws {
				continue;
			}
			let morphs: Vec<_> = draw
				.morph_target_names
				.iter()
				.enumerate()
				.map(|(index, name)| {
					serde_json::json!({
						"index": index,
						"name": name,
						"default_weight": draw.default_morph_weights.get(index).copied().unwrap_or(0.0),
						"uploaded_weight": draw.morph_weights.get(index).copied().unwrap_or(0.0),
					})
				})
				.collect();
			draws.push(serde_json::json!({
					"draw_index": draw_index,
					"node_index": draw.world_node_index,
					"node_path": node_path,
					"visible": draw.visible,
					"asset_resident": draw.asset_resident,
					"morph_target_count": draw.morph_target_count,
					"morphs": morphs,
			}));
		}
		serde_json::json!({
			"matched_draw_count": matched_draw_count,
			"sample_limit": max_draws,
			"truncated": matched_draw_count > draws.len(),
			"draws": draws
		})
	}

	#[allow(clippy::too_many_arguments)]
	fn create_mesh_pipeline(
		device: &wgpu::Device,
		pipeline_layout: &wgpu::PipelineLayout,
		shader: &wgpu::ShaderModule,
		format: wgpu::TextureFormat,
		vb_layout: &wgpu::VertexBufferLayout<'_>,
		pipeline_cache: Option<&wgpu::PipelineCache>,
		label: &'static str,
		vertex_entry: &'static str,
		fragment_entry: &'static str,
		render_state: MeshPipelineRenderState,
	) -> wgpu::RenderPipeline {
		let alpha_to_coverage_enabled = render_state.alpha_coverage.enabled();
		device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some(label),
			layout: Some(pipeline_layout),
			cache: pipeline_cache,
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
					blend: render_state.color_blend,
					write_mask: render_state.color_write_mask,
				})],
			}),
			primitive: wgpu::PrimitiveState {
				topology: wgpu::PrimitiveTopology::TriangleList,
				cull_mode: render_state.cull_mode,
				..Default::default()
			},
			depth_stencil: Some(wgpu::DepthStencilState {
				format: wgpu::TextureFormat::Depth24PlusStencil8,
				depth_write_enabled: Some(render_state.depth_write),
				depth_compare: Some(render_state.depth_compare),
				stencil: render_state.stencil.to_wgpu(),
				bias: wgpu::DepthBiasState::default(),
			}),
			multisample: wgpu::MultisampleState {
				count: render_state.sample_count,
				alpha_to_coverage_enabled,
				..Default::default()
			},
			multiview_mask: None,
		})
	}

	fn create_draw_pipeline(
		device: &wgpu::Device,
		pipeline_layout: &wgpu::PipelineLayout,
		shader: &wgpu::ShaderModule,
		format: wgpu::TextureFormat,
		vb_layout: &wgpu::VertexBufferLayout<'_>,
		pipeline_cache: Option<&wgpu::PipelineCache>,
		key: DrawPipelineKey,
		sample_count: u32,
	) -> wgpu::RenderPipeline {
		let blend = Some(wgpu::BlendState::ALPHA_BLENDING);
		let premultiplied_blend = Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING);
		let additive_toon_blend = Some(wgpu::BlendState {
			color: wgpu::BlendComponent {
				src_factor: wgpu::BlendFactor::One,
				dst_factor: wgpu::BlendFactor::One,
				operation: wgpu::BlendOperation::Add,
			},
			alpha: wgpu::BlendComponent {
				src_factor: wgpu::BlendFactor::One,
				dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
				operation: wgpu::BlendOperation::Add,
			},
		});
		let gem_pre_blend = Some(wgpu::BlendState {
			color: wgpu::BlendComponent {
				src_factor: wgpu::BlendFactor::One,
				dst_factor: wgpu::BlendFactor::Zero,
				operation: wgpu::BlendOperation::Add,
			},
			alpha: wgpu::BlendComponent {
				src_factor: wgpu::BlendFactor::Zero,
				dst_factor: wgpu::BlendFactor::One,
				operation: wgpu::BlendOperation::Add,
			},
		});
		let (label, vertex_entry, fragment_entry, render_state) = match key.kind {
			DrawPipelineKind::OpaqueLit => (
				"mesh_opaque_lit",
				"vs_main",
				"fs_lit",
				MeshPipelineRenderState::mesh_main(None, true, sample_count),
			),
			DrawPipelineKind::OpaqueUnlit => (
				"mesh_opaque_unlit",
				"vs_main",
				"fs_unlit",
				MeshPipelineRenderState::mesh_main(None, true, sample_count),
			),
			DrawPipelineKind::OpaqueToon => (
				"mesh_opaque_toon",
				"vs_main",
				"fs_toon",
				MeshPipelineRenderState::mesh_main(None, true, sample_count).with_alpha_coverage(MeshPipelineAlphaCoverage::On),
			),
			DrawPipelineKind::BlendLit => (
				"mesh_blend_lit",
				"vs_main",
				"fs_lit",
				MeshPipelineRenderState::mesh_main(blend, false, sample_count),
			),
			DrawPipelineKind::BlendUnlit => (
				"mesh_blend_unlit",
				"vs_main",
				"fs_unlit",
				MeshPipelineRenderState::mesh_main(blend, false, sample_count),
			),
			DrawPipelineKind::BlendToon => (
				"mesh_blend_toon",
				"vs_main",
				"fs_toon",
				MeshPipelineRenderState::mesh_main(premultiplied_blend, false, sample_count),
			),
			DrawPipelineKind::BlendToonZWrite => (
				"mesh_blend_toon_zwrite",
				"vs_main",
				"fs_toon",
				MeshPipelineRenderState::mesh_main(premultiplied_blend, true, sample_count),
			),
			DrawPipelineKind::BlendToonAdd => (
				"mesh_blend_toon_add",
				"vs_main",
				"fs_toon",
				MeshPipelineRenderState::mesh_main(additive_toon_blend, false, sample_count),
			),
			DrawPipelineKind::BlendToonAddZWrite => (
				"mesh_blend_toon_add_zwrite",
				"vs_main",
				"fs_toon",
				MeshPipelineRenderState::mesh_main(additive_toon_blend, true, sample_count),
			),
			DrawPipelineKind::TransparentToonBackpass => (
				"mesh_transparent_toon_backpass",
				"vs_main",
				"fs_toon_backpass",
				MeshPipelineRenderState::mesh_main(premultiplied_blend, true, sample_count),
			),
			DrawPipelineKind::TransparentToonBackpassNoZWrite => (
				"mesh_transparent_toon_backpass_no_zwrite",
				"vs_main",
				"fs_toon_backpass",
				MeshPipelineRenderState::mesh_main(premultiplied_blend, false, sample_count),
			),
			DrawPipelineKind::LilToonGemPre => (
				"mesh_liltoon_gem_pre_toon",
				"vs_main",
				"fs_toon_gem_pre",
				MeshPipelineRenderState::mesh_main(gem_pre_blend, false, sample_count),
			),
		};
		Self::create_mesh_pipeline(
			device,
			pipeline_layout,
			shader,
			format,
			vb_layout,
			pipeline_cache,
			label,
			vertex_entry,
			fragment_entry,
			render_state.with_material_render_state(key),
		)
	}

	pub(crate) fn prewarm_standard_pipelines(
		device: &wgpu::Device,
		format: wgpu::TextureFormat,
		sample_count: u32,
		shader_variant_tier: MeshShaderVariantTier,
		pipeline_cache: Option<&wgpu::PipelineCache>,
		mut progress: impl FnMut(&'static str),
	) -> MeshPipelinePrewarmSummary {
		let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("mesh_prewarm_frame"),
			entries: &[
				wgpu::BindGroupLayoutEntry {
					binding: 0,
					visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Buffer {
						ty: wgpu::BufferBindingType::Uniform,
						has_dynamic_offset: false,
						min_binding_size: None,
					},
					count: None,
				},
				texture_bind_group_layout_entry(1, wgpu::ShaderStages::FRAGMENT),
				sampler_bind_group_layout_entry(2, wgpu::ShaderStages::FRAGMENT),
				texture_bind_group_layout_entry(3, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
			],
		});
		let material_entries = mesh_material_layout_entries(shader_variant_tier);
		let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("mesh_prewarm_material"),
			entries: &material_entries,
		});
		let outline_material_entries = mesh_outline_material_layout_entries();
		let outline_material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("mesh_prewarm_outline_material"),
			entries: &outline_material_entries,
		});
		let skin_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("mesh_prewarm_skin"),
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
		let morph_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("mesh_prewarm_morph"),
			entries: &[
				uniform_bind_group_layout_entry(0, wgpu::ShaderStages::VERTEX),
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
		let compute_fur_cards_layout = create_compute_fur_cards_bind_group_layout(device);
		let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("mesh_prewarm"),
			bind_group_layouts: &[Some(&frame_layout), Some(&material_layout), Some(&skin_layout), Some(&morph_layout)],
			immediate_size: 0,
		});
		let outline_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("mesh_prewarm_outline"),
			bind_group_layouts: &[
				Some(&frame_layout),
				Some(&outline_material_layout),
				Some(&skin_layout),
				Some(&morph_layout),
			],
			immediate_size: 0,
		});
		const MESH_VTX_ATTRS: [wgpu::VertexAttribute; 10] = [
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
				format: wgpu::VertexFormat::Float32x4,
			},
			wgpu::VertexAttribute {
				offset: 40,
				shader_location: 3,
				format: wgpu::VertexFormat::Float32x2,
			},
			wgpu::VertexAttribute {
				offset: 72,
				shader_location: 4,
				format: wgpu::VertexFormat::Uint16x4,
			},
			wgpu::VertexAttribute {
				offset: 80,
				shader_location: 5,
				format: wgpu::VertexFormat::Float32x4,
			},
			wgpu::VertexAttribute {
				offset: 96,
				shader_location: 6,
				format: wgpu::VertexFormat::Float32x4,
			},
			wgpu::VertexAttribute {
				offset: 48,
				shader_location: 7,
				format: wgpu::VertexFormat::Float32x2,
			},
			wgpu::VertexAttribute {
				offset: 56,
				shader_location: 8,
				format: wgpu::VertexFormat::Float32x2,
			},
			wgpu::VertexAttribute {
				offset: 64,
				shader_location: 9,
				format: wgpu::VertexFormat::Float32x2,
			},
		];
		let vb_layout = wgpu::VertexBufferLayout {
			array_stride: std::mem::size_of::<Vertex>() as u64,
			step_mode: wgpu::VertexStepMode::Vertex,
			attributes: &MESH_VTX_ATTRS,
		};
		const COMPUTE_FUR_CARDS_VTX_ATTRS: [wgpu::VertexAttribute; 6] = [
			wgpu::VertexAttribute {
				offset: 0,
				shader_location: 0,
				format: wgpu::VertexFormat::Float32x4,
			},
			wgpu::VertexAttribute {
				offset: 16,
				shader_location: 1,
				format: wgpu::VertexFormat::Float32x4,
			},
			wgpu::VertexAttribute {
				offset: 32,
				shader_location: 2,
				format: wgpu::VertexFormat::Float32x2,
			},
			wgpu::VertexAttribute {
				offset: 40,
				shader_location: 3,
				format: wgpu::VertexFormat::Float32,
			},
			wgpu::VertexAttribute {
				offset: 48,
				shader_location: 4,
				format: wgpu::VertexFormat::Float32x4,
			},
			wgpu::VertexAttribute {
				offset: 64,
				shader_location: 5,
				format: wgpu::VertexFormat::Float32x4,
			},
		];
		let compute_fur_cards_vb_layout = wgpu::VertexBufferLayout {
			array_stride: std::mem::size_of::<ComputeFurCardsGeneratedVertexGpu>() as u64,
			step_mode: wgpu::VertexStepMode::Vertex,
			attributes: &COMPUTE_FUR_CARDS_VTX_ATTRS,
		};

		let feature_sets = [
			("mesh_prewarm_liltoon_full", full_liltoon_prewarm_features()),
			("mesh_prewarm_mtoon", mtoon_prewarm_features()),
		];
		let pipeline_kinds = [
			DrawPipelineKind::OpaqueToon,
			DrawPipelineKind::BlendToon,
			DrawPipelineKind::BlendToonZWrite,
			DrawPipelineKind::BlendToonAdd,
			DrawPipelineKind::BlendToonAddZWrite,
			DrawPipelineKind::TransparentToonBackpass,
			DrawPipelineKind::TransparentToonBackpassNoZWrite,
			DrawPipelineKind::LilToonGemPre,
		];
		let mut summary = MeshPipelinePrewarmSummary::default();
		for (shader_label, features) in feature_sets {
			progress(shader_label);
			let shader = create_mesh_shader_module_for_features(device, shader_variant_tier, features, shader_label);
			summary.shader_modules += 1;
			for kind in pipeline_kinds {
				progress(kind.label());
				let key = DrawPipelineKey::from_parts(kind, MaterialStencilState::default(), 15);
				let _pipeline = Self::create_draw_pipeline(
					device,
					&pipeline_layout,
					&shader,
					format,
					&vb_layout,
					pipeline_cache,
					key,
					sample_count,
				);
				summary.render_pipelines += 1;
			}
			progress("mesh_outline_toon");
			let _outline_pipeline = Self::create_mesh_pipeline(
				device,
				&outline_pipeline_layout,
				&shader,
				format,
				&vb_layout,
				pipeline_cache,
				"mesh_outline_toon",
				"vs_outline",
				"fs_outline",
				MeshPipelineRenderState::outline(sample_count),
			);
			summary.render_pipelines += 1;
			progress("mesh_compute_fur_cards_pre_toon");
			let _fur_pre = Self::create_mesh_pipeline(
				device,
				&pipeline_layout,
				&shader,
				format,
				&compute_fur_cards_vb_layout,
				pipeline_cache,
				"mesh_compute_fur_cards_pre_toon",
				"vs_compute_fur_cards_pre",
				"fs_fur_toon_pre",
				MeshPipelineRenderState::mesh_main(None, true, sample_count).with_alpha_coverage(MeshPipelineAlphaCoverage::On),
			);
			summary.render_pipelines += 1;
			progress("mesh_compute_fur_cards_toon");
			let _fur_toon = Self::create_mesh_pipeline(
				device,
				&pipeline_layout,
				&shader,
				format,
				&compute_fur_cards_vb_layout,
				pipeline_cache,
				"mesh_compute_fur_cards_toon",
				"vs_compute_fur_cards",
				"fs_fur_toon",
				MeshPipelineRenderState::mesh_main(Some(wgpu::BlendState::ALPHA_BLENDING), false, sample_count),
			);
			summary.render_pipelines += 1;
		}
		progress("compute_fur_cards");
		let _compute = create_compute_fur_cards_compute_pipeline(device, &compute_fur_cards_layout, pipeline_cache);
		summary.compute_pipelines += 1;
		summary
	}

	#[allow(clippy::too_many_arguments)]
	pub fn new(
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		format: wgpu::TextureFormat,
		sample_count: u32,
		shader_variant_tier: MeshShaderVariantTier,
		pipeline_cache: Option<&wgpu::PipelineCache>,
		scene: &UnaSceneSnapshot,
		catalog: Option<&UnaExpressionCatalog>,
		dynamic_morph_target_names: &[String],
		active_asset_groups: &[String],
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
		gpu_texture_compression_enabled: bool,
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
		let asset_residency = SceneAssetResidencySets::for_scene(scene, active_asset_groups);
		let effective_visibility = scene_effective_visibility(scene);
		let initial_active_texture_indices =
			initial_active_texture_indices_for_scene(scene, &effective_visibility, &asset_residency, &opts);
		let initial_active_2d_texture_indices =
			initial_active_2d_texture_indices_for_scene(scene, &effective_visibility, &asset_residency, &opts);
		let mut total_steps = 4u32
			.saturating_add(scene_texture_upload_step_count(
				scene,
				&texture_roles,
				texture_max_dimension,
				Some(initial_active_2d_texture_indices.as_slice()),
			))
			.saturating_add(scene_primitive_count(scene))
			.max(1);
		let mut current_step = 0u32;
		let mut report = |phase: &'static str, total: u32, message: String| {
			current_step = current_step.saturating_add(1).min(total);
			progress(SceneMeshBuildProgress {
				phase,
				current: current_step,
				total,
				message,
			});
		};
		report("gpu-upload", total_steps, "Preparing GPU scene layouts".to_string());
		let scene_layout_start = Instant::now();
		let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("mesh_frame"),
			entries: &[
				wgpu::BindGroupLayoutEntry {
					binding: 0,
					visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
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
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 3,
					visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
			],
		});

		let material_entries = mesh_material_layout_entries(shader_variant_tier);
		let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some(shader_variant_tier.material_layout_label()),
			entries: &material_entries,
		});
		let outline_material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("mesh_outline_material"),
			entries: &mesh_outline_material_layout_entries(),
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
		let compute_fur_cards_bind_group_layout = create_compute_fur_cards_bind_group_layout(device);

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
		let outline_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("mesh_outline"),
			bind_group_layouts: &[
				Some(&frame_layout),
				Some(&outline_material_layout),
				Some(&skin_bind_group_layout),
				Some(&morph_bind_group_layout),
			],
			immediate_size: 0,
		});

		const MESH_VTX_ATTRS: [wgpu::VertexAttribute; 10] = [
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
				format: wgpu::VertexFormat::Float32x4,
			},
			wgpu::VertexAttribute {
				offset: 40,
				shader_location: 3,
				format: wgpu::VertexFormat::Float32x2,
			},
			wgpu::VertexAttribute {
				offset: 72,
				shader_location: 4,
				format: wgpu::VertexFormat::Uint16x4,
			},
			wgpu::VertexAttribute {
				offset: 80,
				shader_location: 5,
				format: wgpu::VertexFormat::Float32x4,
			},
			wgpu::VertexAttribute {
				offset: 96,
				shader_location: 6,
				format: wgpu::VertexFormat::Float32x4,
			},
			wgpu::VertexAttribute {
				offset: 48,
				shader_location: 7,
				format: wgpu::VertexFormat::Float32x2,
			},
			wgpu::VertexAttribute {
				offset: 56,
				shader_location: 8,
				format: wgpu::VertexFormat::Float32x2,
			},
			wgpu::VertexAttribute {
				offset: 64,
				shader_location: 9,
				format: wgpu::VertexFormat::Float32x2,
			},
		];
		let vb_layout = wgpu::VertexBufferLayout {
			array_stride: std::mem::size_of::<Vertex>() as u64,
			step_mode: wgpu::VertexStepMode::Vertex,
			attributes: &MESH_VTX_ATTRS,
		};
		const COMPUTE_FUR_CARDS_VTX_ATTRS: [wgpu::VertexAttribute; 6] = [
			wgpu::VertexAttribute {
				offset: 0,
				shader_location: 0,
				format: wgpu::VertexFormat::Float32x4,
			},
			wgpu::VertexAttribute {
				offset: 16,
				shader_location: 1,
				format: wgpu::VertexFormat::Float32x4,
			},
			wgpu::VertexAttribute {
				offset: 32,
				shader_location: 2,
				format: wgpu::VertexFormat::Float32x2,
			},
			wgpu::VertexAttribute {
				offset: 40,
				shader_location: 3,
				format: wgpu::VertexFormat::Float32,
			},
			wgpu::VertexAttribute {
				offset: 48,
				shader_location: 4,
				format: wgpu::VertexFormat::Float32x4,
			},
			wgpu::VertexAttribute {
				offset: 64,
				shader_location: 5,
				format: wgpu::VertexFormat::Float32x4,
			},
		];
		let compute_fur_cards_vb_layout = wgpu::VertexBufferLayout {
			array_stride: std::mem::size_of::<ComputeFurCardsGeneratedVertexGpu>() as u64,
			step_mode: wgpu::VertexStepMode::Vertex,
			attributes: &COMPUTE_FUR_CARDS_VTX_ATTRS,
		};
		log_slow_gpu_scene_step("layout/shader module setup", scene_layout_start.elapsed());

		report("gpu-upload", total_steps, "Preparing mesh frame buffers".to_string());
		let frame_buffer = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("mesh_frame"),
			size: std::mem::size_of::<MeshFrameGpu>() as u64,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		let screen_grab_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
			label: Some("mesh_screen_grab_sampler"),
			address_mode_u: wgpu::AddressMode::ClampToEdge,
			address_mode_v: wgpu::AddressMode::ClampToEdge,
			address_mode_w: wgpu::AddressMode::ClampToEdge,
			mag_filter: wgpu::FilterMode::Linear,
			min_filter: wgpu::FilterMode::Linear,
			mipmap_filter: wgpu::MipmapFilterMode::Nearest,
			..Default::default()
		});
		let screen_grab_fallback_texture = create_solid_texture_1x1(
			device,
			queue,
			"screen-grab-fallback",
			wgpu::TextureFormat::Rgba8Unorm,
			[0, 0, 0, 255],
		);
		let screen_grab_fallback_view = screen_grab_fallback_texture.create_view(&wgpu::TextureViewDescriptor::default());
		let audio_link_texture = device.create_texture(&wgpu::TextureDescriptor {
			label: Some("audio-link-texture"),
			size: wgpu::Extent3d {
				width: crate::audio_link::AUDIO_LINK_TEXTURE_WIDTH,
				height: crate::audio_link::AUDIO_LINK_TEXTURE_HEIGHT,
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: wgpu::TextureFormat::Rgba8Unorm,
			usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
			view_formats: &[],
		});
		let audio_link_view = audio_link_texture.create_view(&wgpu::TextureViewDescriptor::default());

		let frame_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("mesh_frame"),
			layout: &frame_layout,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: frame_buffer.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&screen_grab_fallback_view),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: wgpu::BindingResource::Sampler(&screen_grab_sampler),
				},
				wgpu::BindGroupEntry {
					binding: 3,
					resource: wgpu::BindingResource::TextureView(&audio_link_view),
				},
			],
		});

		let mut samplers = vec![create_mesh_sampler(device, "mesh_sampler_default", &UnaTextureSampler::default())];
		let reflection_cube_sampler =
			device.create_sampler(&liltoon_reflection_cube_sampler_descriptor("mesh_liltoon_reflection_cube_sampler"));
		let mut image_sampler_indices = Vec::with_capacity(scene.image_sources.len());
		for source in &scene.image_sources {
			let Some(sampler) = source.as_ref().and_then(|source| source.sampler.as_ref()) else {
				image_sampler_indices.push(0);
				continue;
			};
			let sampler_index = samplers.len();
			samplers.push(create_mesh_sampler(device, "mesh_sampler_image", sampler));
			image_sampler_indices.push(sampler_index);
		}

		let mut textures: Vec<wgpu::Texture> = Vec::with_capacity(scene.images.len() + 6);

		let white_view = push_solid_texture_1x1_view(
			&mut textures,
			device,
			queue,
			"white1x1",
			wgpu::TextureFormat::Rgba8UnormSrgb,
			[255, 255, 255, 255],
		);
		let black_view = push_solid_texture_1x1_view(
			&mut textures,
			device,
			queue,
			"black1x1",
			wgpu::TextureFormat::Rgba8UnormSrgb,
			[0, 0, 0, 255],
		);
		let cube_texture_capacity = 1 + scene
			.image_sources
			.iter()
			.filter(|source| texture_source_is_cube(source.as_ref()))
			.count();
		let mut cube_textures: Vec<wgpu::Texture> = Vec::with_capacity(cube_texture_capacity);
		let black_cube_texture =
			create_solid_cube_texture_1x1(device, queue, "black_cube1x1", wgpu::TextureFormat::Rgba8UnormSrgb, [0, 0, 0, 255]);
		let black_cube_view = black_cube_texture.create_view(&wgpu::TextureViewDescriptor {
			label: Some("black_cube1x1_view"),
			dimension: Some(wgpu::TextureViewDimension::Cube),
			..Default::default()
		});
		cube_textures.push(black_cube_texture);
		let neutral_normal_view = push_solid_texture_1x1_view(
			&mut textures,
			device,
			queue,
			"neutral_normal1x1",
			wgpu::TextureFormat::Rgba8Unorm,
			[128, 128, 255, 255],
		);
		let transparent_black_view = push_solid_texture_1x1_view(
			&mut textures,
			device,
			queue,
			"transparent_black1x1",
			wgpu::TextureFormat::Rgba8UnormSrgb,
			[0, 0, 0, 0],
		);
		let blue_view = push_solid_texture_1x1_view(
			&mut textures,
			device,
			queue,
			"blue1x1",
			wgpu::TextureFormat::Rgba8UnormSrgb,
			[0, 0, 255, 255],
		);
		report("gpu-upload", total_steps, "Uploading fallback textures".to_string());
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

		let mut cube_image_views: Vec<Option<wgpu::TextureView>> = Vec::with_capacity(scene.images.len());
		let mut cube_texture_slots: Vec<Option<SceneCubeTextureSlot>> = Vec::with_capacity(scene.images.len());
		let mut image_views: Vec<wgpu::TextureView> = Vec::with_capacity(scene.images.len());
		let mut image_texture_slots: Vec<SceneImageTextureSlot> = Vec::with_capacity(scene.images.len());
		let mut image_texture_residency = Vec::with_capacity(scene.images.len());
		let mut cube_texture_residency = Vec::with_capacity(scene.images.len());
		let mut gpu_texture_compression = None;
		let material_slot_residency = scene
			.materials
			.iter()
			.enumerate()
			.map(|(material_index, _)| asset_residency.material_resident(material_index))
			.collect::<Vec<_>>();
		let texture_prepare_start = Instant::now();
		let mut texture_prepare_summary = TexturePrepareSummary::default();
		for (image_index, im) in scene.images.iter().enumerate() {
			let image_prepare_start = Instant::now();
			let mut image_prepare_timings = TextureImagePrepareTimings::default();
			let role = texture_roles.get(image_index).copied().unwrap_or_default();
			let image_resident =
				asset_residency.image_resident(image_index) && initial_active_2d_texture_indices.binary_search(&image_index).is_ok();
			let cube_resident =
				asset_residency.image_resident(image_index) && initial_active_texture_indices.binary_search(&image_index).is_ok();
			image_texture_residency.push(image_resident);
			cube_texture_residency
				.push(cube_resident && texture_source_is_cube(scene.image_sources.get(image_index).and_then(Option::as_ref)));
			let source_metadata = scene.image_sources.get(image_index).and_then(Option::as_ref);
			let (src_w, src_h) = scene_image_source_dimensions(im, source_metadata);
			let skin_tone_override = skin_tone_matched_images.get(image_index).and_then(Option::as_deref);
			if texture_source_is_cube(source_metadata) {
				texture_summary.cubemap_count += 1;
			}
			let cube_prepare_start = Instant::now();
			if let Some((_layout, face_size)) = cube_source_layout(im, source_metadata) {
				if cube_resident {
					if let Some((cube_upload, cube_cache_event)) = build_lazy_scene_cube_texture_upload(
						scene,
						&SceneCubeTextureLazyUpload {
							image_index,
							processed_texture_cache,
						},
					) {
						image_prepare_timings.cube += cube_prepare_start.elapsed();
						texture_summary.cubemap_converted_count += 1;
						if cube_cache_event.hit {
							texture_summary.cubemap_cache_hits += 1;
						}
						if cube_cache_event.miss {
							texture_summary.cubemap_cache_misses += 1;
						}
						if cube_cache_event.write {
							texture_summary.cubemap_cache_writes += 1;
						}
						let cube_bytes = cube_upload.mips.iter().map(|mip| mip.data_rgba16f.len() as u64).sum::<u64>();
						report(
							"gpu-upload",
							total_steps,
							format!(
								"Uploading cubemap texture {}/{} face={} mips={} layout={} ({role:?})",
								image_index + 1,
								scene.images.len(),
								cube_upload.face_size,
								cube_upload.mips.len(),
								cube_upload.layout
							),
						);
						texture_summary.cubemap_uploaded_bytes += cube_bytes;
						let upload_start = Instant::now();
						let mut slot = SceneCubeTextureSlot::new(cube_upload);
						cube_image_views.push(slot.ensure_uploaded(device, queue, Some(scene)));
						image_prepare_timings.upload += upload_start.elapsed();
						cube_texture_slots.push(Some(slot));
					} else {
						image_prepare_timings.cube += cube_prepare_start.elapsed();
						texture_summary.cubemap_fallback_count += 1;
						cube_image_views.push(None);
						cube_texture_slots.push(None);
					}
				} else {
					image_prepare_timings.cube += cube_prepare_start.elapsed();
					let cube_bytes = estimated_cube_upload_mip_bytes(face_size);
					texture_summary.deferred_cubemap_upload_count += 1;
					texture_summary.deferred_cubemap_mip_bytes += cube_bytes;
					cube_image_views.push(None);
					cube_texture_slots.push(Some(SceneCubeTextureSlot::new_lazy(image_index, processed_texture_cache)));
				}
			} else {
				image_prepare_timings.cube += cube_prepare_start.elapsed();
				if texture_source_is_cube(source_metadata) {
					texture_summary.cubemap_fallback_count += 1;
					report(
						"gpu-upload",
						total_steps,
						format!(
							"Cubemap texture {}/{} has unsupported source layout {:?}; using black cube fallback",
							image_index + 1,
							scene.images.len(),
							source_metadata.and_then(|source| source.source_layout.as_deref())
						),
					);
				}
				cube_image_views.push(None);
				cube_texture_slots.push(None);
			}
			if image_resident
				&& texture_max_dimension.is_none()
				&& skin_tone_override.is_none()
				&& texture_compression != TextureCompressionMode::Compat
			{
				let source_upload_start = Instant::now();
				if let Some(source_upload) = source_texture_upload(im, source_metadata, role) {
					image_prepare_timings.source += source_upload_start.elapsed();
					if image_resident {
						report(
							"gpu-upload",
							total_steps,
							format!(
								"Uploading precision-preserving source texture {}/{} {}x{} {:?} ({role:?})",
								image_index + 1,
								scene.images.len(),
								source_upload.width,
								source_upload.height,
								source_upload.format
							),
						);
					}
					texture_summary.record_image(
						src_w,
						src_h,
						source_upload.width,
						source_upload.height,
						source_upload.data.len() as u64,
						image_resident,
					);
					let mut slot = SceneImageTextureSlot::new(SceneImageTextureUpload::Source(source_upload));
					if image_resident {
						let upload_start = Instant::now();
						if let Some(view) = slot.ensure_uploaded(device, queue, None, &mut gpu_texture_compression) {
							image_views.push(view);
						} else {
							image_views.push(transparent_black_view.clone());
						}
						image_prepare_timings.upload += upload_start.elapsed();
					} else {
						report(
							"gpu-upload",
							total_steps,
							format!(
								"Deferring precision-preserving source texture {}/{} ({role:?})",
								image_index + 1,
								scene.images.len()
							),
						);
						image_views.push(transparent_black_view.clone());
					}
					image_texture_slots.push(slot);
					texture_prepare_summary.record(
						image_index,
						source_metadata.and_then(|source| source.name.as_deref()),
						source_metadata.and_then(|source| source.mime_type.as_deref()),
						role,
						image_resident,
						image_prepare_start.elapsed(),
						image_prepare_timings,
						TextureCacheEvent::DISABLED,
						TextureCacheEvent::DISABLED,
					);
					continue;
				}
				image_prepare_timings.source += source_upload_start.elapsed();
			}
			if image_resident
				&& skin_tone_override.is_none()
				&& is_deferred_scene_image_placeholder(im)
				&& !texture_source_is_cube(source_metadata)
			{
				if let Some(source) = source_metadata.filter(|source| source_has_lazy_encoded_bytes(source)) {
					let compression_preference = compression_preference_for_role(texture_compression, texture_compression_advanced, role);
					if compression_preference == TextureCompressionPreference::Source {
						let source_key =
							texture_cache_key_from_source_metadata(src_w, src_h, texture_max_dimension, role, mipmap_filter, source);
						let processed_start = Instant::now();
						let mut deferred_rgba_elapsed = Duration::ZERO;
						let (processed, cache_event) = load_or_build_processed_texture_with_rgba(
							src_w,
							src_h,
							texture_max_dimension,
							role,
							mipmap_filter,
							processed_texture_cache,
							source_key,
							|| {
								let rgba_start = Instant::now();
								let rgba = decode_encoded_source_image(source)
									.map(|image| image.rgba8_compat_pixels().into_owned())
									.unwrap_or_else(|| vec![0, 0, 0, 0]);
								deferred_rgba_elapsed += rgba_start.elapsed();
								Cow::Owned(rgba)
							},
						);
						image_prepare_timings.rgba += deferred_rgba_elapsed;
						image_prepare_timings.processed += processed_start.elapsed().saturating_sub(deferred_rgba_elapsed);
						let processed_w = processed.width;
						let processed_h = processed.height;
						let payload_start = Instant::now();
						let (payload, compressed_cache_event) = texture_upload_payload(
							processed,
							texture_compression,
							texture_compression_advanced,
							role,
							texture_compression_bc_supported,
							block_compression_encoder,
							block_compression_cpu_threads,
							gpu_texture_compression.as_mut(),
							processed_texture_cache,
							None,
							false,
						);
						image_prepare_timings.payload += payload_start.elapsed();
						report(
							"gpu-upload",
							total_steps,
							format!(
								"Uploading cached source texture {}/{} {}x{} -> {}x{} mips={} ({role:?})",
								image_index + 1,
								scene.images.len(),
								src_w,
								src_h,
								processed_w,
								processed_h,
								payload.mips.len()
							),
						);
						if cache_event.hit {
							texture_summary.cache_hits += 1;
						}
						if cache_event.miss {
							texture_summary.cache_misses += 1;
						}
						if cache_event.write {
							texture_summary.cache_writes += 1;
						}
						texture_summary.record_image(src_w, src_h, processed_w, processed_h, payload.byte_len(), true);
						let texture_format = payload_texture_format(payload.kind, role, source_metadata);
						let (slot, upload_elapsed) = upload_payload_texture_slot(
							device,
							queue,
							&transparent_black_view,
							&mut gpu_texture_compression,
							&mut image_views,
							payload,
							texture_format,
							processed_w,
							processed_h,
						);
						image_prepare_timings.upload += upload_elapsed;
						image_texture_slots.push(slot);
						texture_prepare_summary.record(
							image_index,
							source_metadata.and_then(|source| source.name.as_deref()),
							source_metadata.and_then(|source| source.mime_type.as_deref()),
							role,
							image_resident,
							image_prepare_start.elapsed(),
							image_prepare_timings,
							cache_event,
							compressed_cache_event,
						);
						continue;
					}
					let source_key =
						texture_cache_key_from_source_metadata(src_w, src_h, texture_max_dimension, role, mipmap_filter, source);
					let cache_lookup_start = Instant::now();
					let compressed_cache_lookup = processed_texture_cache.then(|| {
						compressed_cache_lookup_from_source_metadata(
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
					image_prepare_timings.cache_lookup += cache_lookup_start.elapsed();
					let cache_read_start = Instant::now();
					let compressed_cache_hit = compressed_cache_lookup.as_ref().and_then(|lookup| {
						read_compressed_texture_cache(&lookup.path, lookup.key, lookup.kind).map(|payload| {
							(
								payload,
								TextureCacheEvent {
									hit: true,
									miss: false,
									write: false,
									read_elapsed: Duration::ZERO,
									read_bytes: 0,
								},
								lookup.processed_width,
								lookup.processed_height,
							)
						})
					});
					image_prepare_timings.cache_read += cache_read_start.elapsed();
					if let Some((payload, compressed_cache_event, processed_w, processed_h)) = compressed_cache_hit {
						let (w, h) = payload_top_mip_dimensions(&payload, processed_w, processed_h);
						texture_summary.compressed_cache_hits += 1;
						texture_summary.compressed_count += 1;
						texture_summary.compressed_mip_bytes += payload.byte_len();
						texture_summary.record_image(src_w, src_h, w, h, payload.byte_len(), true);
						let texture_format = payload_texture_format(payload.kind, role, source_metadata);
						report(
							"gpu-upload",
							total_steps,
							format!(
								"Uploading cached compressed texture {}/{} {}x{} -> {}x{} mips={} ({role:?})",
								image_index + 1,
								scene.images.len(),
								src_w,
								src_h,
								w,
								h,
								payload.mips.len()
							),
						);
						let (slot, upload_elapsed) = upload_payload_texture_slot(
							device,
							queue,
							&transparent_black_view,
							&mut gpu_texture_compression,
							&mut image_views,
							payload,
							texture_format,
							w,
							h,
						);
						image_prepare_timings.upload += upload_elapsed;
						image_texture_slots.push(slot);
						texture_prepare_summary.record(
							image_index,
							source_metadata.and_then(|source| source.name.as_deref()),
							source_metadata.and_then(|source| source.mime_type.as_deref()),
							role,
							image_resident,
							image_prepare_start.elapsed(),
							image_prepare_timings,
							TextureCacheEvent::DISABLED,
							compressed_cache_event,
						);
						continue;
					}
					let processed_start = Instant::now();
					let mut deferred_rgba_elapsed = Duration::ZERO;
					let (processed, cache_event) = load_or_build_processed_texture_with_rgba(
						src_w,
						src_h,
						texture_max_dimension,
						role,
						mipmap_filter,
						processed_texture_cache,
						source_key,
						|| {
							let rgba_start = Instant::now();
							let rgba = decode_encoded_source_image(source)
								.map(|image| image.rgba8_compat_pixels().into_owned())
								.unwrap_or_else(|| vec![0, 0, 0, 0]);
							deferred_rgba_elapsed += rgba_start.elapsed();
							Cow::Owned(rgba)
						},
					);
					image_prepare_timings.rgba += deferred_rgba_elapsed;
					image_prepare_timings.processed += processed_start.elapsed().saturating_sub(deferred_rgba_elapsed);
					let processed_w = processed.width;
					let processed_h = processed.height;
					if gpu_texture_compression_enabled
						&& gpu_texture_compression.is_none()
						&& compressed_upload_kind_for_texture(
							&processed,
							texture_compression,
							texture_compression_advanced,
							role,
							texture_compression_bc_supported,
						)
						.is_some()
					{
						report("gpu-upload", total_steps, "Preparing GPU texture compression".to_string());
						gpu_texture_compression = Some(create_vulkan_gpu_texture_compression_context()?);
					}
					let payload_start = Instant::now();
					let (payload, compressed_cache_event) = texture_upload_payload(
						processed,
						texture_compression,
						texture_compression_advanced,
						role,
						texture_compression_bc_supported,
						block_compression_encoder,
						block_compression_cpu_threads,
						gpu_texture_compression.as_mut(),
						processed_texture_cache,
						compressed_cache_lookup.as_ref(),
						compressed_cache_lookup.is_some(),
					);
					image_prepare_timings.payload += payload_start.elapsed();
					let (w, h) = payload_top_mip_dimensions(&payload, processed_w, processed_h);
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
					}
					texture_summary.record_image(src_w, src_h, w, h, payload.byte_len(), true);
					let texture_format = payload_texture_format(payload.kind, role, source_metadata);
					report(
						"gpu-upload",
						total_steps,
						format!(
							"Uploading cached source texture {}/{} {}x{} -> {}x{} mips={} ({role:?})",
							image_index + 1,
							scene.images.len(),
							src_w,
							src_h,
							w,
							h,
							payload.mips.len()
						),
					);
					let (slot, upload_elapsed) = upload_payload_texture_slot(
						device,
						queue,
						&transparent_black_view,
						&mut gpu_texture_compression,
						&mut image_views,
						payload,
						texture_format,
						w,
						h,
					);
					image_prepare_timings.upload += upload_elapsed;
					image_texture_slots.push(slot);
					texture_prepare_summary.record(
						image_index,
						source_metadata.and_then(|source| source.name.as_deref()),
						source_metadata.and_then(|source| source.mime_type.as_deref()),
						role,
						image_resident,
						image_prepare_start.elapsed(),
						image_prepare_timings,
						cache_event,
						compressed_cache_event,
					);
					continue;
				}
			}
			if !image_resident && skin_tone_override.is_none() && texture_source_is_cube(source_metadata) {
				let estimated_mip_bytes = (src_w as u64)
					.saturating_mul(src_h as u64)
					.saturating_mul(4)
					.saturating_mul(estimated_processed_mip_count(src_w, src_h, texture_max_dimension, role) as u64);
				texture_summary.record_image(src_w, src_h, src_w, src_h, estimated_mip_bytes, false);
				report(
					"gpu-upload",
					total_steps,
					format!(
						"Deferring 2D upload for cubemap texture {}/{} ({role:?})",
						image_index + 1,
						scene.images.len()
					),
				);
				image_views.push(transparent_black_view.clone());
				image_texture_slots.push(SceneImageTextureSlot::new(SceneImageTextureUpload::Lazy(
					SceneImageTextureLazyUpload {
						image_index,
						role,
						mipmap_filter,
						texture_max_dimension,
						texture_compression,
						block_compression_encoder,
						block_compression_cpu_threads,
						processed_texture_cache,
						texture_compression_advanced: texture_compression_advanced.clone(),
						texture_compression_bc_supported,
						gpu_texture_compression_enabled,
					},
				)));
				texture_prepare_summary.record(
					image_index,
					source_metadata.and_then(|source| source.name.as_deref()),
					source_metadata.and_then(|source| source.mime_type.as_deref()),
					role,
					image_resident,
					image_prepare_start.elapsed(),
					image_prepare_timings,
					TextureCacheEvent::DISABLED,
					TextureCacheEvent::DISABLED,
				);
				continue;
			}
			if !image_resident && skin_tone_override.is_none() && !texture_source_is_cube(source_metadata) {
				let processed_w = texture_max_dimension.map_or(src_w, |max_dimension| src_w.min(max_dimension));
				let processed_h = texture_max_dimension.map_or(src_h, |max_dimension| src_h.min(max_dimension));
				let estimated_mip_bytes = (src_w as u64)
					.saturating_mul(src_h as u64)
					.saturating_mul(4)
					.saturating_mul(estimated_processed_mip_count(src_w, src_h, texture_max_dimension, role) as u64);
				texture_summary.record_image(src_w, src_h, processed_w, processed_h, estimated_mip_bytes, false);
				report(
					"gpu-upload",
					total_steps,
					format!("Deferring lazy texture {}/{} ({role:?})", image_index + 1, scene.images.len()),
				);
				image_views.push(transparent_black_view.clone());
				image_texture_slots.push(SceneImageTextureSlot::new(SceneImageTextureUpload::Lazy(
					SceneImageTextureLazyUpload {
						image_index,
						role,
						mipmap_filter,
						texture_max_dimension,
						texture_compression,
						block_compression_encoder,
						block_compression_cpu_threads,
						processed_texture_cache,
						texture_compression_advanced: texture_compression_advanced.clone(),
						texture_compression_bc_supported,
						gpu_texture_compression_enabled,
					},
				)));
				texture_prepare_summary.record(
					image_index,
					source_metadata.and_then(|source| source.name.as_deref()),
					source_metadata.and_then(|source| source.mime_type.as_deref()),
					role,
					image_resident,
					image_prepare_start.elapsed(),
					image_prepare_timings,
					TextureCacheEvent::DISABLED,
					TextureCacheEvent::DISABLED,
				);
				continue;
			}
			let source_metadata_key = skin_tone_override
				.is_none()
				.then(|| {
					source_metadata.map(|source| {
						texture_cache_key_from_source_metadata(src_w, src_h, texture_max_dimension, role, mipmap_filter, source)
					})
				})
				.flatten();
			let compression_preference = compression_preference_for_role(texture_compression, texture_compression_advanced, role);
			let can_defer_rgba_for_processed_cache =
				source_metadata_key.is_some() && compression_preference == TextureCompressionPreference::Source;
			let (payload, cache_event, compressed_cache_event, processed_w, processed_h) = if can_defer_rgba_for_processed_cache {
				let source_key = source_metadata_key.expect("checked above");
				let processed_start = Instant::now();
				let mut deferred_rgba_elapsed = Duration::ZERO;
				let (processed, cache_event) = load_or_build_processed_texture_with_rgba(
					src_w,
					src_h,
					texture_max_dimension,
					role,
					mipmap_filter,
					processed_texture_cache,
					source_key,
					|| {
						let rgba_start = Instant::now();
						let rgba = im.rgba8_compat_pixels();
						deferred_rgba_elapsed += rgba_start.elapsed();
						rgba
					},
				);
				image_prepare_timings.rgba += deferred_rgba_elapsed;
				image_prepare_timings.processed += processed_start.elapsed().saturating_sub(deferred_rgba_elapsed);
				let processed_w = processed.width;
				let processed_h = processed.height;
				let payload_start = Instant::now();
				let (payload, compressed_cache_event) = texture_upload_payload(
					processed,
					texture_compression,
					texture_compression_advanced,
					role,
					texture_compression_bc_supported,
					block_compression_encoder,
					block_compression_cpu_threads,
					gpu_texture_compression.as_mut(),
					processed_texture_cache,
					None,
					false,
				);
				image_prepare_timings.payload += payload_start.elapsed();
				(payload, cache_event, compressed_cache_event, processed_w, processed_h)
			} else {
				let rgba_start = Instant::now();
				let decoded_placeholder_image = if skin_tone_override.is_none() && is_deferred_scene_image_placeholder(im) {
					source_metadata.and_then(decode_encoded_source_image)
				} else {
					None
				};
				let texture_image = decoded_placeholder_image.as_ref().unwrap_or(im);
				let rgba_compat = texture_image.rgba8_compat_pixels();
				let rgba = skin_tone_override.unwrap_or(rgba_compat.as_ref());
				image_prepare_timings.rgba += rgba_start.elapsed();
				let source_key = source_metadata_key
					.unwrap_or_else(|| texture_cache_key(src_w, src_h, texture_max_dimension, role, mipmap_filter, rgba));
				let cache_lookup_start = Instant::now();
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
				image_prepare_timings.cache_lookup += cache_lookup_start.elapsed();
				let cache_read_start = Instant::now();
				let compressed_cache_hit = compressed_cache_lookup.as_ref().and_then(|lookup| {
					read_compressed_texture_cache(&lookup.path, lookup.key, lookup.kind).map(|payload| {
						(
							payload,
							TextureCacheEvent {
								hit: true,
								miss: false,
								write: false,
								read_elapsed: Duration::ZERO,
								read_bytes: 0,
							},
							lookup.processed_width,
							lookup.processed_height,
						)
					})
				});
				image_prepare_timings.cache_read += cache_read_start.elapsed();
				if let Some((payload, compressed_cache_event, processed_w, processed_h)) = compressed_cache_hit {
					(
						payload,
						TextureCacheEvent::DISABLED,
						compressed_cache_event,
						processed_w,
						processed_h,
					)
				} else {
					let processed_start = Instant::now();
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
					image_prepare_timings.processed += processed_start.elapsed();
					let processed_w = processed.width;
					let processed_h = processed.height;
					if gpu_texture_compression_enabled
						&& gpu_texture_compression.is_none()
						&& compressed_upload_kind_for_texture(
							&processed,
							texture_compression,
							texture_compression_advanced,
							role,
							texture_compression_bc_supported,
						)
						.is_some()
					{
						report("gpu-upload", total_steps, "Preparing GPU texture compression".to_string());
						gpu_texture_compression = Some(create_vulkan_gpu_texture_compression_context()?);
					}
					let payload_start = Instant::now();
					let (payload, compressed_cache_event) = texture_upload_payload(
						processed,
						texture_compression,
						texture_compression_advanced,
						role,
						texture_compression_bc_supported,
						block_compression_encoder,
						block_compression_cpu_threads,
						gpu_texture_compression.as_mut(),
						processed_texture_cache,
						compressed_cache_lookup.as_ref(),
						compressed_cache_lookup.is_some(),
					);
					image_prepare_timings.payload += payload_start.elapsed();
					(payload, cache_event, compressed_cache_event, processed_w, processed_h)
				}
			};
			// 圧縮テクスチャは block 整列 (4 の倍数) に padding されているため、テクスチャ次元・サンプリング寸法も
			// payload の最上位 mip サイズに揃える。非4倍数寸法を元の論理寸法へ戻すと BCn upload が停止する。
			// 非圧縮 (Rgba) は元の processed 寸法と一致する。
			let (w, h) = payload_top_mip_dimensions(&payload, processed_w, processed_h);
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
			texture_summary.record_image(src_w, src_h, w, h, payload.byte_len(), image_resident);
			let texture_format = payload_texture_format(payload.kind, role, source_metadata);
			if image_resident {
				for (mip_level, mip) in payload.mips.iter().enumerate() {
					report(
						"gpu-upload",
						total_steps,
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
				}
			} else {
				report(
					"gpu-upload",
					total_steps,
					format!(
						"Deferring texture {}/{} mips={} ({role:?})",
						image_index + 1,
						scene.images.len(),
						payload.mips.len()
					),
				);
			}
			if image_resident {
				let (slot, upload_elapsed) = upload_payload_texture_slot(
					device,
					queue,
					&transparent_black_view,
					&mut gpu_texture_compression,
					&mut image_views,
					payload,
					texture_format,
					w,
					h,
				);
				image_prepare_timings.upload += upload_elapsed;
				image_texture_slots.push(slot);
			} else {
				image_views.push(transparent_black_view.clone());
				image_texture_slots.push(SceneImageTextureSlot::new(SceneImageTextureUpload::Payload {
					payload,
					format: texture_format,
					width: w,
					height: h,
				}));
			}
			texture_prepare_summary.record(
				image_index,
				source_metadata.and_then(|source| source.name.as_deref()),
				source_metadata.and_then(|source| source.mime_type.as_deref()),
				role,
				image_resident,
				image_prepare_start.elapsed(),
				image_prepare_timings,
				cache_event,
				compressed_cache_event,
			);
		}
		texture_prepare_summary.log(texture_prepare_start.elapsed());

		assert_eq!(
			image_views.len(),
			scene.images.len(),
			"internal fallback texture count changed scene image view indexing"
		);
		let neutral_vector_texture = create_solid_texture_1x1(
			device,
			queue,
			"neutral_vector1x1",
			wgpu::TextureFormat::Rgba8Unorm,
			[128, 128, 255, 255],
		);
		textures.push(neutral_vector_texture);
		let neutral_vector_view = textures
			.last()
			.expect("neutral vector texture was just pushed")
			.create_view(&wgpu::TextureViewDescriptor::default());
		let texture_views = SceneTextureViews {
			white: white_view.clone(),
			black: black_view.clone(),
			neutral_normal: neutral_normal_view.clone(),
			transparent_black: transparent_black_view.clone(),
			blue: blue_view.clone(),
			neutral_vector: neutral_vector_view.clone(),
			black_cube: black_cube_view.clone(),
			images: image_views,
			cubes: cube_image_views,
		};

		let scene_has_morph_targets = scene_has_morph_targets(scene);
		let expression_names = if scene_has_morph_targets {
			expression_names(catalog)
		} else {
			Vec::new()
		};
		let expression_bindings = if scene_has_morph_targets {
			expression_binding_index(catalog)
		} else {
			BTreeMap::new()
		};
		let node_paths = scene_node_paths(scene);
		let mut draws = Vec::with_capacity(mesh_draw_capacity(scene));
		let mut skin_palettes = Vec::with_capacity(skin_palette_capacity(scene));
		let mut skin_palette_indices = BTreeMap::new();
		let mut empty_morph_resources: Option<MorphGpuResources> = None;
		let mut expanded_primitive_cache: BTreeMap<ExpandedPrimitiveCacheKey, ExpandedPrimitive> = BTreeMap::new();
		let mut expanded_morph_payload_cache: BTreeMap<ExpandedPrimitiveCacheKey, ExpandedMorphPayload> = BTreeMap::new();
		let mut shared_morph_delta_cache: BTreeMap<ExpandedPrimitiveCacheKey, SharedMorphDeltaResources> = BTreeMap::new();
		let mut morph_delta_scratch: Vec<[f32; 4]> = Vec::new();
		let default_material = UnaMaterialPbr::default();
		let default_mtoon = UnaMtoonMaterial::default();
		let mesh_prepare_start = Instant::now();
		let mut mesh_prepare_summary = MeshPrepareSummary::default();
		for (ni, node) in scene.nodes.iter().enumerate() {
			let active = effective_visibility.get(ni).copied().unwrap_or(false);
			let Some(mesh_i) = node.mesh else { continue };
			let Some(mesh_prims) = scene.meshes.get(mesh_i) else { continue };
			for (prim_i, buf) in mesh_prims.iter().enumerate() {
				report("gpu-upload", total_steps, format!("Preparing mesh {mesh_i} primitive {prim_i}"));
				let primitive_start = Instant::now();
				let mut step_start = Instant::now();
				let material_slot_index = buf
					.material_index
					.filter(|material_index| scene.materials.get(*material_index).is_some());
				let mat = material_slot_index
					.and_then(|mi| scene.materials.get(mi))
					.unwrap_or(&default_material);
				if material_is_fully_invisible_for_draw(mat, &opts) {
					log_slow_gpu_scene_step(
						format!("primitive mesh={mesh_i} primitive={prim_i} skipped invisible material"),
						primitive_start.elapsed(),
					);
					mesh_prepare_summary.skipped_invisible_primitives += 1;
					report(
						"gpu-upload",
						total_steps,
						format!("Skipping fully transparent mesh {mesh_i} primitive {prim_i}"),
					);
					continue;
				}
				let material_elapsed = take_gpu_scene_step_elapsed(&mut step_start);
				let original_expression_bindings = expression_bindings.get(&(mesh_i, prim_i)).map(Vec::as_slice).unwrap_or(&[]);
				let dynamic_morph_targets = dynamic_morph_target_indices(
					buf,
					original_expression_bindings,
					dynamic_morph_target_names,
					opts.debug_zero_morphs,
				);
				let dynamic_morph_elapsed = take_gpu_scene_step_elapsed(&mut step_start);
				let dynamic_morph_target_list = dynamic_morph_targets.clone().into_boxed_slice();
				let expanded_cache_key = buf
					.vertex_payload_id
					.filter(|_| primitive_expand_cache_safe(buf))
					.map(|vertex_payload_id| ExpandedPrimitiveCacheKey {
						vertex_payload_id,
						dynamic_morph_targets: dynamic_morph_target_list.clone(),
					});
				let morph_delta_cache_key = buf.vertex_payload_id.map(|vertex_payload_id| ExpandedPrimitiveCacheKey {
					vertex_payload_id,
					dynamic_morph_targets: dynamic_morph_target_list,
				});
				let exp = if let Some(cache_key) = expanded_cache_key.as_ref() {
					if let Some(exp) = expanded_primitive_cache.get(cache_key).cloned() {
						mesh_prepare_summary.expanded_cache_hits += 1;
						let mut exp = exp;
						exp.indices = primitive_indices(buf);
						Some(exp)
					} else {
						mesh_prepare_summary.expanded_cache_misses += 1;
						let exp = expand_primitive_with_cached_morph(
							buf,
							Some(&dynamic_morph_targets),
							morph_delta_cache_key
								.as_ref()
								.and_then(|cache_key| expanded_morph_payload_cache.get(cache_key)),
						);
						if let Some(exp) = &exp {
							expanded_primitive_cache.insert(cache_key.clone(), exp.clone());
							if let Some(morph_cache_key) = morph_delta_cache_key.as_ref() {
								expanded_morph_payload_cache
									.entry(morph_cache_key.clone())
									.or_insert_with(|| expanded_morph_payload_from_primitive(exp));
							}
						}
						exp
					}
				} else {
					mesh_prepare_summary.expanded_uncacheable += 1;
					let exp = expand_primitive_with_cached_morph(
						buf,
						Some(&dynamic_morph_targets),
						morph_delta_cache_key
							.as_ref()
							.and_then(|cache_key| expanded_morph_payload_cache.get(cache_key)),
					);
					if let (Some(cache_key), Some(exp)) = (morph_delta_cache_key.as_ref(), exp.as_ref()) {
						expanded_morph_payload_cache
							.entry(cache_key.clone())
							.or_insert_with(|| expanded_morph_payload_from_primitive(exp));
					}
					exp
				};
				let Some(exp) = exp else {
					log_slow_gpu_scene_step(
						format!("primitive mesh={mesh_i} primitive={prim_i} expand skipped"),
						primitive_start.elapsed(),
					);
					mesh_prepare_summary.skipped_empty_primitives += 1;
					continue;
				};
				let expand_elapsed = take_gpu_scene_step_elapsed(&mut step_start);
				let ExpandedPrimitive {
					mut verts,
					indices,
					morph_pos,
					morph_nrm,
					morph_source_indices,
					default_morph_weights,
				} = exp;
				let compact_expression_bindings = remap_expression_bindings(original_expression_bindings, &morph_source_indices);
				let morph_target_names = morph_source_indices
					.iter()
					.map(|&source_index| buf.morph_target_names.get(source_index).cloned().unwrap_or_default())
					.collect::<Vec<_>>();
				let node_path = node_paths.get(ni).map(String::as_str).unwrap_or("");
				let morph_target_override_keys = morph_target_names
					.iter()
					.map(|name| morph_override_key(node_path, name))
					.collect::<Vec<_>>();
				let morph_target_override_suffix_keys = morph_target_override_keys
					.iter()
					.map(|key| morph_override_path_suffix_key(key))
					.collect::<Vec<_>>();
				let skin = node.skin.and_then(|skin_index| scene.skins.get(skin_index));
				let mesh_cloth_assist_vertices = apply_mesh_cloth_assist_to_vertices(
					&mut verts,
					&indices,
					skin,
					&node_paths,
					node_path,
					&opts.mesh_cloth_assist,
					&opts.mesh_cloth_assist_categories,
					&opts.dynamic_deforming_node_indices,
				);
				mesh_prepare_summary.mesh_cloth_assist_vertices += mesh_cloth_assist_vertices as u64;
				if mesh_cloth_assist_vertices > 0 {
					log_slow_gpu_scene_step(
						format!("primitive mesh={mesh_i} primitive={prim_i} mesh cloth assist vertices={mesh_cloth_assist_vertices}"),
						primitive_start.elapsed(),
					);
				}
				normalize_skinning_vertices(&mut verts, buf.joints.is_some(), skin);
				debug_dump_mesh_vertex_weights_if_requested(
					mesh_i,
					prim_i,
					node_path,
					material_slot_index,
					mat.name.as_deref(),
					&verts,
					&indices,
					skin,
					&node_paths,
					&opts.dynamic_deforming_node_indices,
					mesh_cloth_assist_vertices,
				);
				let skinning_elapsed = take_gpu_scene_step_elapsed(&mut step_start);
				let skin_palette_key = skin_palette_key_for_node(ni, node.skin);
				let skin_palette_index = Self::skin_palette_index(
					device,
					queue,
					&skin_bind_group_layout,
					&mut skin_palettes,
					&mut skin_palette_indices,
					skin_palette_key,
					skin,
				);
				let skin_palette_elapsed = take_gpu_scene_step_elapsed(&mut step_start);
				let index_format = compact_index_format(&indices);
				let buffer_upload = SceneMeshBufferUpload {
					vertices: verts.into_boxed_slice(),
					indices: SceneMeshIndexUpload::from_indices(index_format, indices),
				};
				let vertex_buffer_bytes = buffer_upload.vertex_buffer_bytes();
				let index_buffer_bytes = buffer_upload.index_buffer_bytes();
				let index_count = buffer_upload.indices.len() as u32;
				let asset_resident = asset_residency.mesh_primitive_resident(mesh_i, prim_i);
				mesh_prepare_summary.prepared_primitives += 1;
				mesh_prepare_summary.vertices += buffer_upload.vertices.len() as u64;
				mesh_prepare_summary.indices += buffer_upload.indices.len() as u64;
				if asset_resident {
					mesh_prepare_summary.resident_primitives += 1;
					mesh_prepare_summary.resident_vertex_bytes += vertex_buffer_bytes;
					mesh_prepare_summary.resident_index_bytes += index_buffer_bytes;
				} else {
					mesh_prepare_summary.deferred_primitives += 1;
					mesh_prepare_summary.deferred_vertex_bytes += vertex_buffer_bytes;
					mesh_prepare_summary.deferred_index_bytes += index_buffer_bytes;
				}
				let (vertex_buffer, index_buffer) = if asset_resident {
					let (vertex_buffer, index_buffer) = buffer_upload.create_buffers(device, queue);
					(Some(vertex_buffer), Some(index_buffer))
				} else {
					(None, None)
				};
				let buffer_upload_elapsed = take_gpu_scene_step_elapsed(&mut step_start);

				let liltoon_like = mat.liltoon_like_runtime();
				let tex_sampler = texture_sampler_or(&samplers, &image_sampler_indices, mat.base_color_texture_index, 0);
				let fur_vector_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.fur.vector_texture_index);
				let fur_vector_view = texture_view_or(&texture_views.images, fur_vector_texture_index, &texture_views.neutral_vector);
				let fur_length_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.fur.length_mask_texture_index);
				let fur_length_mask_view = texture_view_or(&texture_views.images, fur_length_mask_texture_index, &texture_views.white);
				let fur_noise_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.fur.noise_mask_texture_index);
				let fur_noise_mask_view = texture_view_or(&texture_views.images, fur_noise_mask_texture_index, &texture_views.white);
				let fur_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.fur.mask_texture_index);
				let fur_mask_view = texture_view_or(&texture_views.images, fur_mask_texture_index, &texture_views.white);
				let compute_fur_cards_cpu_fur_maps = ComputeFurCardsCpuFurMaps {
					length_mask: fur_length_mask_texture_index.and_then(|index| scene.images.get(index)),
					fur_mask: fur_mask_texture_index.and_then(|index| scene.images.get(index)),
				};

				let draw_transform = device.create_buffer(&wgpu::BufferDescriptor {
					label: Some("mesh_draw_transform"),
					size: std::mem::size_of::<MeshDrawTransformGpu>() as u64,
					usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
					mapped_at_creation: false,
				});
				let draw_material = mesh_draw_material_gpu_runtime(mat, &default_mtoon, &opts, mesh_i, prim_i);
				let draw_material_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
					label: Some("mesh_draw_material"),
					contents: bytemuck::bytes_of(&draw_material),
					usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
				});

				let (bind_material, bind_outline_material) = if asset_resident {
					let (bind_material, bind_outline_material) = create_mesh_draw_bind_groups(
						device,
						&material_layout,
						&outline_material_layout,
						shader_variant_tier,
						&texture_views,
						&samplers,
						&image_sampler_indices,
						&reflection_cube_sampler,
						mat,
						&draw_transform,
						&draw_material_buffer,
					);
					(Some(bind_material), Some(bind_outline_material))
				} else {
					(None, None)
				};
				let material_bind_elapsed = take_gpu_scene_step_elapsed(&mut step_start);

				let morph_target_count = morph_pos.len();
				let has_morph_targets = morph_target_count > 0;
				let morph_resources = if asset_resident {
					Some(if has_morph_targets {
						if let Some(cache_key) = morph_delta_cache_key.as_ref() {
							if let Some(shared) = shared_morph_delta_cache.get(cache_key) {
								create_morph_resources_with_shared_deltas(device, &morph_bind_group_layout, shared)
							} else {
								fill_morph_delta_data(
									&morph_pos,
									morph_nrm.as_deref(),
									buffer_upload.vertices.len(),
									&mut morph_delta_scratch,
								);
								let shared = create_shared_morph_delta_resources(
									device,
									queue,
									morph_target_count as u32,
									buffer_upload.vertices.len() as u32,
									&morph_delta_scratch,
								);
								let resources = create_morph_resources_with_shared_deltas(device, &morph_bind_group_layout, &shared);
								shared_morph_delta_cache.insert(cache_key.clone(), shared);
								resources
							}
						} else {
							fill_morph_delta_data(
								&morph_pos,
								morph_nrm.as_deref(),
								buffer_upload.vertices.len(),
								&mut morph_delta_scratch,
							);
							create_morph_resources(
								device,
								queue,
								&morph_bind_group_layout,
								morph_target_count as u32,
								buffer_upload.vertices.len() as u32,
								&morph_delta_scratch,
							)
						}
					} else {
						let empty_morph_resources = empty_morph_resources
							.get_or_insert_with(|| create_morph_resources(device, queue, &morph_bind_group_layout, 0, 0, &[]));
						MorphGpuResources {
							meta_buffer: empty_morph_resources.meta_buffer.clone(),
							weight_buffer: empty_morph_resources.weight_buffer.clone(),
							delta_buffer: empty_morph_resources.delta_buffer.clone(),
							bind_group: empty_morph_resources.bind_group.clone(),
						}
					})
				} else {
					None
				};
				let morph_resource_elapsed = take_gpu_scene_step_elapsed(&mut step_start);
				let compute_fur_cards = if asset_resident && material_has_fur(mat, mat.shading, &opts) {
					create_compute_fur_cards_draw_resources(
						device,
						&compute_fur_cards_bind_group_layout,
						mat,
						&buffer_upload.vertices,
						&buffer_upload.indices,
						compute_fur_cards_cpu_fur_maps,
						fur_vector_view,
						fur_length_mask_view,
						fur_noise_mask_view,
						fur_mask_view,
						tex_sampler,
					)
				} else {
					None
				};
				let fur_resource_elapsed = take_gpu_scene_step_elapsed(&mut step_start);
				let vertex_count = buffer_upload.vertices.len();
				draws.push(MeshDraw {
					vertex_buffer,
					index_buffer,
					vertex_buffer_bytes,
					index_buffer_bytes,
					buffer_upload,
					index_format,
					index_count,
					draw_transform,
					draw_transform_uploaded: None,
					draw_material: draw_material_buffer,
					bind_material,
					bind_outline_material,
					skin_palette_index,
					skin_palette_static_identity: skin_palette_key.skin_index.is_none(),
					morph_resources,
					_compute_fur_cards: compute_fur_cards,
					world_node_index: ni,
					visible: active,
					asset_resident,
					shading: mat.shading,
					morph_target_count,
					morph_source_indices: morph_source_indices.into_boxed_slice(),
					morph_target_names: morph_target_names.into_boxed_slice(),
					morph_target_override_keys: morph_target_override_keys.into_boxed_slice(),
					morph_target_override_suffix_keys: morph_target_override_suffix_keys.into_boxed_slice(),
					morph_pos,
					morph_nrm,
					expression_bindings: compact_expression_bindings.into_boxed_slice(),
					default_morph_weights,
					morph_weights: Vec::with_capacity(morph_target_count),
					morph_weight_scratch: Vec::with_capacity(morph_target_count),
					alpha_mode: mat.alpha_mode,
					material_slot_index,
					material: mat.clone(),
					stencil_state: material_stencil_state(mat),
					color_mask: material_color_mask(mat),
					outline_stencil_state: material_outline_stencil_state(mat),
					outline_color_mask: material_outline_color_mask(mat),
					fur_stencil_state: material_fur_stencil_state(mat),
					fur_color_mask: material_fur_color_mask(mat),
					texture_indices: material_texture_indices(mat).into_boxed_slice(),
					cube_texture_indices: material_cube_texture_indices(mat).into_boxed_slice(),
					mesh_index: mesh_i,
					primitive_index: prim_i,
				});
				let draw_push_elapsed = take_gpu_scene_step_elapsed(&mut step_start);
				mesh_prepare_summary.record_timings(MeshPrepareTimings {
					material: material_elapsed,
					dynamic_morphs: dynamic_morph_elapsed,
					expand: expand_elapsed,
					skinning: skinning_elapsed,
					skin_palette: skin_palette_elapsed,
					buffer_upload: buffer_upload_elapsed,
					material_bind: material_bind_elapsed,
					morph_resources: morph_resource_elapsed,
					fur_resources: fur_resource_elapsed,
					draw_push: draw_push_elapsed,
				});
				log_slow_gpu_scene_primitive(
					mesh_i,
					prim_i,
					vertex_count,
					index_count as usize,
					morph_target_count,
					asset_resident,
					primitive_start.elapsed(),
					&[
						("material", material_elapsed),
						("dynamic_morphs", dynamic_morph_elapsed),
						("expand", expand_elapsed),
						("skinning", skinning_elapsed),
						("skin_palette", skin_palette_elapsed),
						("buffers", buffer_upload_elapsed),
						("material_bind", material_bind_elapsed),
						("morph_resources", morph_resource_elapsed),
						("fur_resources", fur_resource_elapsed),
						("draw_push", draw_push_elapsed),
					],
				);
			}
		}
		mesh_prepare_summary.log(mesh_prepare_start.elapsed());

		let draw_state = build_draw_order(&draws, &opts);
		let pipeline_draw_state = build_potential_draw_order(&draws, &opts);
		let mut required_pipeline_keys = Vec::new();
		for batch in pipeline_draw_state
			.opaque_batches
			.iter()
			.chain(pipeline_draw_state.blended_batches.iter())
		{
			required_pipeline_keys.push(batch.pipeline);
		}
		for &draw_index in &pipeline_draw_state.transparent_backpass_draw_indices {
			if let Some(draw) = draws.get(draw_index) {
				let zwrite = draw
					.material
					.liltoon_like_runtime()
					.is_none_or(|u| u.blend_state.pre_zwrite_factor > 0.5);
				required_pipeline_keys.push(DrawPipelineKey::new(
					if zwrite {
						DrawPipelineKind::TransparentToonBackpass
					} else {
						DrawPipelineKind::TransparentToonBackpassNoZWrite
					},
					draw,
					&opts,
				));
			}
		}
		required_pipeline_keys.sort_unstable();
		required_pipeline_keys.dedup();
		let needs_outline_pipeline = !pipeline_draw_state.outline_draw_indices.is_empty()
			&& !opts.force_simple_basecolor
			&& !opts.debug_bind_pose
			&& !opts.debug_primitive_colors;
		let needs_fur_pipelines = !pipeline_draw_state.fur_draw_indices.is_empty();
		let outline_pipeline_keys = if needs_outline_pipeline {
			let mut keys = pipeline_draw_state
				.outline_draw_indices
				.iter()
				.filter_map(|&draw_index| {
					draws
						.get(draw_index)
						.map(|draw| MaterialRenderStateKey::from_draw_outline(draw, &opts))
				})
				.collect::<Vec<_>>();
			keys.sort_unstable();
			keys.dedup();
			keys
		} else {
			Vec::new()
		};
		let fur_pipeline_keys = if needs_fur_pipelines {
			let mut keys = pipeline_draw_state
				.fur_draw_indices
				.iter()
				.filter_map(|&draw_index| draws.get(draw_index).map(|draw| MaterialRenderStateKey::from_draw_fur(draw, &opts)))
				.collect::<Vec<_>>();
			keys.sort_unstable();
			keys.dedup();
			keys
		} else {
			Vec::new()
		};
		let pipeline_shader_features = draw_pipeline_shader_features(&draws, &pipeline_draw_state, &opts);
		let mut outline_shader_features = UntoonShaderFeatures::default();
		for &draw_index in &pipeline_draw_state.outline_draw_indices {
			if let Some(draw) = draws.get(draw_index) {
				outline_shader_features.include(draw_untoon_shader_features(draw, &opts));
			}
		}
		let mut fur_shader_features = pipeline_draw_state.runtime_requirements.toon_shader_features;
		fur_shader_features.fur = needs_fur_pipelines;
		let pipeline_count = required_pipeline_keys
			.len()
			.saturating_add(outline_pipeline_keys.len())
			.saturating_add(usize::from(needs_fur_pipelines))
			.saturating_add(fur_pipeline_keys.len().saturating_mul(2));
		total_steps = total_steps.saturating_add(pipeline_count as u32).saturating_add(4);
		report("gpu-upload", total_steps, format!("Creating {pipeline_count} mesh pipeline(s)"));
		let shader_module_start = Instant::now();
		let outline_shader_module = needs_outline_pipeline
			.then(|| create_mesh_shader_module_for_features(device, shader_variant_tier, outline_shader_features, "mesh_outline_shader"));
		let fur_shader_module = needs_fur_pipelines
			.then(|| create_mesh_shader_module_for_features(device, shader_variant_tier, fur_shader_features, "mesh_fur_shader"));
		let mut pipeline_shader_features_by_key = BTreeMap::new();
		let mut draw_shader_modules = BTreeMap::new();
		for key in &required_pipeline_keys {
			let shader_features = pipeline_shader_features.get(key).copied().unwrap_or_default();
			pipeline_shader_features_by_key.insert(*key, shader_features);
			draw_shader_modules.entry(shader_features).or_insert_with(|| {
				create_mesh_shader_module_for_features(device, shader_variant_tier, shader_features, "mesh_draw_shader")
			});
		}
		log_slow_gpu_scene_step(
			format!(
				"mesh shader module creation draw_variants={} outline={} fur={}",
				draw_shader_modules.len(),
				needs_outline_pipeline,
				needs_fur_pipelines
			),
			shader_module_start.elapsed(),
		);
		let pipeline_start = Instant::now();
		let render_pipeline_start = Instant::now();
		let (
			pipelines_outline_toon,
			compute_fur_cards_compute_pipeline,
			pipelines_compute_fur_cards_pre_toon,
			pipelines_compute_fur_cards_toon,
			pipelines,
		) = std::thread::scope(|scope| {
			let mut pipeline_outline_toon_handles = Vec::new();
			if needs_outline_pipeline {
				let label = "mesh_outline_toon";
				for key in outline_pipeline_keys {
					report("gpu-upload", total_steps, format!("Creating mesh pipeline {label}"));
					let outline_pipeline_layout = outline_pipeline_layout.clone();
					let vb_layout = vb_layout.clone();
					let shader = outline_shader_module.as_ref().expect("outline shader module missing");
					pipeline_outline_toon_handles.push((
						key,
						scope.spawn(move || {
							let start = Instant::now();
							let pipeline = Self::create_mesh_pipeline(
								device,
								&outline_pipeline_layout,
								&shader,
								format,
								&vb_layout,
								pipeline_cache,
								label,
								"vs_outline",
								"fs_outline",
								MeshPipelineRenderState::outline(sample_count).with_material_render_state_key(key),
							);
							log_slow_gpu_scene_step(format!("render pipeline {label}"), start.elapsed());
							pipeline
						}),
					));
				}
			}
			let compute_fur_cards_compute_pipeline_handle = needs_fur_pipelines.then(|| {
				let label = "compute_fur_cards";
				report("gpu-upload", total_steps, format!("Creating mesh pipeline {label}"));
				let compute_fur_cards_bind_group_layout = compute_fur_cards_bind_group_layout.clone();
				scope.spawn(move || {
					let start = Instant::now();
					let pipeline = create_compute_fur_cards_compute_pipeline(device, &compute_fur_cards_bind_group_layout, pipeline_cache);
					log_slow_gpu_scene_step(format!("compute pipeline {label}"), start.elapsed());
					pipeline
				})
			});
			let mut pipeline_compute_fur_cards_pre_toon_handles = Vec::new();
			let mut pipeline_compute_fur_cards_toon_handles = Vec::new();
			if needs_fur_pipelines {
				for key in fur_pipeline_keys {
					let label = "mesh_compute_fur_cards_pre_toon";
					report("gpu-upload", total_steps, format!("Creating mesh pipeline {label}"));
					let pre_pipeline_layout = pipeline_layout.clone();
					let pre_compute_fur_cards_vb_layout = compute_fur_cards_vb_layout.clone();
					let shader = fur_shader_module.as_ref().expect("fur shader module missing");
					pipeline_compute_fur_cards_pre_toon_handles.push((
						key,
						scope.spawn(move || {
							let start = Instant::now();
							let pipeline = Self::create_mesh_pipeline(
								device,
								&pre_pipeline_layout,
								&shader,
								format,
								&pre_compute_fur_cards_vb_layout,
								pipeline_cache,
								label,
								"vs_compute_fur_cards_pre",
								"fs_fur_toon_pre",
								MeshPipelineRenderState::mesh_main(None, true, sample_count)
									.with_alpha_coverage(MeshPipelineAlphaCoverage::On)
									.with_material_render_state_key(key),
							);
							log_slow_gpu_scene_step(format!("render pipeline {label}"), start.elapsed());
							pipeline
						}),
					));
					let label = "mesh_compute_fur_cards_toon";
					report("gpu-upload", total_steps, format!("Creating mesh pipeline {label}"));
					let toon_pipeline_layout = pipeline_layout.clone();
					let toon_compute_fur_cards_vb_layout = compute_fur_cards_vb_layout.clone();
					let shader = fur_shader_module.as_ref().expect("fur shader module missing");
					pipeline_compute_fur_cards_toon_handles.push((
						key,
						scope.spawn(move || {
							let start = Instant::now();
							let pipeline = Self::create_mesh_pipeline(
								device,
								&toon_pipeline_layout,
								&shader,
								format,
								&toon_compute_fur_cards_vb_layout,
								pipeline_cache,
								label,
								"vs_compute_fur_cards",
								"fs_fur_toon",
								MeshPipelineRenderState::mesh_main(Some(wgpu::BlendState::ALPHA_BLENDING), false, sample_count)
									.with_material_render_state_key(key),
							);
							log_slow_gpu_scene_step(format!("render pipeline {label}"), start.elapsed());
							pipeline
						}),
					));
				}
			}
			let mut pipeline_handles = Vec::new();
			for key in required_pipeline_keys {
				let label = key.label();
				report("gpu-upload", total_steps, format!("Creating mesh pipeline {label}"));
				let shader_features = pipeline_shader_features_by_key.get(&key).copied().unwrap_or_default();
				let shader = draw_shader_modules.get(&shader_features).expect("draw shader module missing");
				let pipeline_layout = pipeline_layout.clone();
				let vb_layout = vb_layout.clone();
				pipeline_handles.push((
					key,
					scope.spawn(move || {
						let start = Instant::now();
						let pipeline = Self::create_draw_pipeline(
							device,
							&pipeline_layout,
							&shader,
							format,
							&vb_layout,
							pipeline_cache,
							key,
							sample_count,
						);
						log_slow_gpu_scene_step(format!("render pipeline {label}"), start.elapsed());
						pipeline
					}),
				));
			}
			let mut pipelines_outline_toon = BTreeMap::new();
			for (key, handle) in pipeline_outline_toon_handles {
				pipelines_outline_toon.insert(key, handle.join().expect("mesh outline pipeline worker panicked"));
			}
			let compute_fur_cards_compute_pipeline =
				compute_fur_cards_compute_pipeline_handle.map(|handle| handle.join().expect("compute fur pipeline worker panicked"));
			let mut pipelines_compute_fur_cards_pre_toon = BTreeMap::new();
			for (key, handle) in pipeline_compute_fur_cards_pre_toon_handles {
				pipelines_compute_fur_cards_pre_toon.insert(key, handle.join().expect("compute fur pre pipeline worker panicked"));
			}
			let mut pipelines_compute_fur_cards_toon = BTreeMap::new();
			for (key, handle) in pipeline_compute_fur_cards_toon_handles {
				pipelines_compute_fur_cards_toon.insert(key, handle.join().expect("compute fur draw pipeline worker panicked"));
			}
			let mut pipelines = BTreeMap::new();
			for (kind, handle) in pipeline_handles {
				pipelines.insert(kind, handle.join().expect("mesh draw pipeline worker panicked"));
			}
			(
				pipelines_outline_toon,
				compute_fur_cards_compute_pipeline,
				pipelines_compute_fur_cards_pre_toon,
				pipelines_compute_fur_cards_toon,
				pipelines,
			)
		});
		log_slow_gpu_scene_step(
			format!("render pipeline creation count={pipeline_count}"),
			render_pipeline_start.elapsed(),
		);
		log_slow_gpu_scene_step(
			format!("pipeline creation count={pipeline_count} outline={needs_outline_pipeline} fur={needs_fur_pipelines}"),
			pipeline_start.elapsed(),
		);
		let has_morph_draws = draws.iter().any(|draw| draw.morph_target_count > 0);
		let expression_value_capacity = expression_names.len();

		let mut scene_meshes = Self {
			pipelines,
			pipelines_outline_toon,
			compute_fur_cards_bind_group_layout,
			compute_fur_cards_compute_pipeline,
			pipelines_compute_fur_cards_pre_toon,
			pipelines_compute_fur_cards_toon,
			frame_buffer,
			frame_uploaded: None,
			frame_layout,
			frame_bind_group,
			material_layout,
			outline_material_layout,
			morph_bind_group_layout,
			shader_variant_tier,
			screen_grab_sampler,
			reflection_cube_sampler,
			_screen_grab_fallback_texture: screen_grab_fallback_texture,
			_audio_link_texture: audio_link_texture,
			audio_link_view,
			audio_link_uploaded_sequence: 0,
			audio_link_frame_params: [0.0; 4],
			texture_views,
			image_texture_slots,
			cube_texture_slots,
			_samplers: samplers.into_boxed_slice(),
			image_sampler_indices: image_sampler_indices.into_boxed_slice(),
			_textures: textures,
			_cube_textures: cube_textures,
			draws,
			skin_palettes,
			outline_draw_indices: draw_state.outline_draw_indices.into_boxed_slice(),
			fur_draw_indices: draw_state.fur_draw_indices.into_boxed_slice(),
			opaque_batches: draw_state.opaque_batches,
			transparent_backpass_draw_indices: draw_state.transparent_backpass_draw_indices.into_boxed_slice(),
			blended_batches: draw_state.blended_batches,
			active_draw_indices: draw_state.active_draw_indices.into_boxed_slice(),
			active_morph_draw_indices: draw_state.active_morph_draw_indices.into_boxed_slice(),
			needs_screen_refraction: draw_state.needs_screen_refraction,
			active_skin_palette_indices: draw_state.active_skin_palette_indices.into_boxed_slice(),
			image_texture_residency,
			cube_texture_residency,
			material_slot_residency,
			lazy_gpu_texture_compression: gpu_texture_compression,
			texture_summary,
			runtime_requirements: draw_state.runtime_requirements,
			visibility_scratch: Vec::new(),
			expression_names: expression_names.into_boxed_slice(),
			expression_value_scratch: Vec::with_capacity(expression_value_capacity),
			fur_source_vertex_scratch: Vec::new(),
			fur_palette_matrix_scratch: Vec::new(),
			has_morph_draws,
			opts,
		};
		let active_gaps = scene_meshes.active_residency_gaps();
		report("gpu-upload", total_steps, "Resolving active asset residency".to_string());
		if !active_gaps.inactive_image_texture_indices.is_empty()
			|| !active_gaps.inactive_cube_texture_indices.is_empty()
			|| !active_gaps.inactive_material_slot_indices.is_empty()
		{
			let active_residency_start = Instant::now();
			scene_meshes.promote_image_texture_residency(&active_gaps.inactive_image_texture_indices);
			scene_meshes.promote_cube_texture_residency(&active_gaps.inactive_cube_texture_indices);
			report(
				"gpu-upload",
				total_steps,
				format!(
					"Uploading active deferred textures image={} cube={} material={}",
					active_gaps.inactive_image_texture_indices.len(),
					active_gaps.inactive_cube_texture_indices.len(),
					active_gaps.inactive_material_slot_indices.len()
				),
			);
			let texture_residency_start = Instant::now();
			scene_meshes.apply_image_texture_view_residency(
				device,
				queue,
				scene,
				&active_gaps.inactive_image_texture_indices,
				&[],
				&active_gaps.inactive_cube_texture_indices,
				&[],
			);
			log_slow_gpu_scene_step("active image texture residency upload", texture_residency_start.elapsed());
			report(
				"gpu-upload",
				total_steps,
				format!(
					"Rebuilding active material bindings material={}",
					active_gaps.inactive_material_slot_indices.len()
				),
			);
			let material_residency_start = Instant::now();
			scene_meshes.promote_material_slot_residency(&active_gaps.inactive_material_slot_indices);
			scene_meshes.rebuild_material_bind_groups(device);
			log_slow_gpu_scene_step("active material residency rebuild", material_residency_start.elapsed());
			log_slow_gpu_scene_step("active asset residency completion", active_residency_start.elapsed());
		} else {
			report("gpu-upload", total_steps, "Active asset residency ready".to_string());
			report("gpu-upload", total_steps, "Active material bindings ready".to_string());
		}
		report("gpu-upload", total_steps, "GPU scene meshes ready".to_string());
		Ok(scene_meshes)
	}

	fn write_skin_palette(
		queue: &wgpu::Queue,
		palette: &mut SkinPalette,
		skin: Option<&un_avatar_core::UnaSkin>,
		world: &[Mat4],
		legacy_no_inv_mesh: bool,
	) {
		palette.uploaded_changed = false;
		if palette.static_identity {
			return;
		}
		if let Some(skin) = skin {
			let mesh_world = world.get(palette.key.world_node_index).copied().unwrap_or(Mat4::IDENTITY);
			let inv_mesh = safe_inverse_mesh_world(mesh_world);
			let joint_count = skin.joint_nodes.len().min(palette.matrix_capacity).min(MAX_BONES);
			if joint_count == 0 {
				palette.raw.resize(matrix_raw_capacity(1), 0.0);
				write_matrix_to_raw_slot(&mut palette.raw, 0, Mat4::IDENTITY);
			} else {
				palette.raw.resize(matrix_raw_capacity(joint_count), 0.0);
			}
			for (j, &n) in skin.joint_nodes.iter().take(joint_count).enumerate() {
				let wj = world.get(n).copied().unwrap_or(Mat4::IDENTITY);
				let ibm = palette.inverse_bind_matrices.get(j).copied().unwrap_or(Mat4::IDENTITY);
				let matrix = if legacy_no_inv_mesh { wj * ibm } else { inv_mesh * wj * ibm };
				write_matrix_to_raw_slot(&mut palette.raw, j, matrix);
			}
		} else {
			palette.raw.resize(matrix_raw_capacity(1), 0.0);
			write_matrix_to_raw_slot(&mut palette.raw, 0, Mat4::IDENTITY);
		}
		if palette.uploaded != palette.raw {
			queue.write_buffer(&palette.buffer, 0, bytemuck::cast_slice(&palette.raw));
			std::mem::swap(&mut palette.uploaded, &mut palette.raw);
			palette.uploaded_changed = true;
		}
	}

	fn skin_palette_index(
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		skin_bind_group_layout: &wgpu::BindGroupLayout,
		skin_palettes: &mut Vec<SkinPalette>,
		skin_palette_indices: &mut BTreeMap<SkinPaletteKey, usize>,
		key: SkinPaletteKey,
		skin: Option<&un_avatar_core::UnaSkin>,
	) -> usize {
		if let Some(&index) = skin_palette_indices.get(&key) {
			return index;
		}
		let matrix_capacity = skin_palette_matrix_capacity(skin);
		let bone_buffer = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("mesh_bones"),
			size: matrix_capacity as u64 * BONE_MATRIX_SIZE,
			usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		let bone_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("mesh_bone_bg"),
			layout: skin_bind_group_layout,
			entries: &[wgpu::BindGroupEntry {
				binding: 0,
				resource: bone_buffer.as_entire_binding(),
			}],
		});
		let index = skin_palettes.len();
		let static_identity = key.skin_index.is_none();
		let (raw, uploaded) = if static_identity {
			let raw = identity_matrix_raw();
			queue.write_buffer(&bone_buffer, 0, bytemuck::cast_slice(&raw));
			(Vec::new(), Vec::new())
		} else {
			let raw_capacity = matrix_raw_capacity(matrix_capacity);
			(Vec::with_capacity(raw_capacity), Vec::with_capacity(raw_capacity))
		};
		let inverse_bind_matrices = skin
			.map(|skin| {
				skin.inverse_bind_matrices
					.iter()
					.take(matrix_capacity)
					.map(Mat4::from_cols_array)
					.collect::<Vec<_>>()
					.into_boxed_slice()
			})
			.unwrap_or_default();
		skin_palettes.push(SkinPalette {
			key,
			buffer: bone_buffer,
			bind_group: bone_bind_group,
			matrix_capacity,
			static_identity,
			inverse_bind_matrices,
			raw,
			uploaded,
			uploaded_changed: false,
		});
		skin_palette_indices.insert(key, index);
		index
	}

	pub fn prepare_frame(
		&mut self,
		queue: &wgpu::Queue,
		view_proj: Mat4,
		view: Mat4,
		light_dir: Vec4,
		camera_pos: Vec4,
		light_color: Vec4,
		ambient_color: Vec4,
		time_secs: f32,
		audio_link_params: [f32; 4],
	) {
		let f = MeshFrameGpu {
			view_proj: view_proj.to_cols_array_2d(),
			view: view.to_cols_array_2d(),
			light_dir: light_dir.to_array(),
			camera_pos: camera_pos.to_array(),
			light_color: light_color.to_array(),
			ambient_color: ambient_color.to_array(),
			time_params: [time_secs, 0.0, 0.0, 0.0],
			audio_link_params,
			_pad: [[0.0; 4]; 2],
		};
		if self.frame_uploaded != Some(f) {
			queue.write_buffer(&self.frame_buffer, 0, bytemuck::bytes_of(&f));
			self.frame_uploaded = Some(f);
		}
	}

	fn draw_inner_with_material(
		&self,
		pass: &mut wgpu::RenderPass<'_>,
		state: &mut DrawBindState,
		draw_index: usize,
		bind_material: &wgpu::BindGroup,
	) {
		self.draw_inner_with_material_instances(pass, state, draw_index, bind_material, 1);
	}

	fn draw_inner_with_material_instances(
		&self,
		pass: &mut wgpu::RenderPass<'_>,
		state: &mut DrawBindState,
		draw_index: usize,
		bind_material: &wgpu::BindGroup,
		instance_count: u32,
	) {
		if instance_count == 0 {
			return;
		}
		let d = &self.draws[draw_index];
		let palette = &self.skin_palettes[d.skin_palette_index];
		if !state.frame_bound {
			pass.set_bind_group(0, &self.frame_bind_group, &[]);
			state.frame_bound = true;
		}
		pass.set_bind_group(1, bind_material, &[]);
		if state.skin_palette_index != Some(d.skin_palette_index) {
			pass.set_bind_group(2, &palette.bind_group, &[]);
			state.skin_palette_index = Some(d.skin_palette_index);
		}
		let (Some(vertex_buffer), Some(index_buffer)) = (&d.vertex_buffer, &d.index_buffer) else {
			return;
		};
		let Some(morph_resources) = d.morph_resources.as_ref() else {
			return;
		};
		pass.set_bind_group(3, &morph_resources.bind_group, &[]);
		pass.set_vertex_buffer(0, vertex_buffer.slice(..));
		pass.set_index_buffer(index_buffer.slice(..), d.index_format);
		pass.draw_indexed(0..d.index_count, 0, 0..instance_count);
	}

	fn draw_inner(&self, pass: &mut wgpu::RenderPass<'_>, state: &mut DrawBindState, draw_index: usize) {
		let Some(bind_material) = self.draws[draw_index].bind_material.as_ref() else {
			return;
		};
		self.draw_inner_with_material(pass, state, draw_index, bind_material);
	}

	fn draw_compute_fur_cards_inner(&self, pass: &mut wgpu::RenderPass<'_>, state: &mut DrawBindState, draw_index: usize) -> bool {
		let d = &self.draws[draw_index];
		let Some(compute_fur_cards) = d._compute_fur_cards.as_ref() else {
			return false;
		};
		if compute_fur_cards.generated_index_count == 0 {
			return false;
		}
		if !state.frame_bound {
			pass.set_bind_group(0, &self.frame_bind_group, &[]);
			state.frame_bound = true;
		}
		let Some(bind_material) = d.bind_material.as_ref() else {
			return false;
		};
		let Some(morph_resources) = d.morph_resources.as_ref() else {
			return false;
		};
		pass.set_bind_group(1, bind_material, &[]);
		let palette = &self.skin_palettes[d.skin_palette_index];
		if state.skin_palette_index != Some(d.skin_palette_index) {
			pass.set_bind_group(2, &palette.bind_group, &[]);
			state.skin_palette_index = Some(d.skin_palette_index);
		}
		pass.set_bind_group(3, &morph_resources.bind_group, &[]);
		pass.set_vertex_buffer(0, compute_fur_cards.generated_vertex_buffer.slice(..));
		pass.set_index_buffer(compute_fur_cards.generated_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
		pass.draw_indexed(0..compute_fur_cards.generated_index_count, 0, 0..1);
		true
	}

	pub fn draw_toon_outlines(&self, pass: &mut wgpu::RenderPass<'_>) {
		if self.outline_draw_indices.is_empty()
			|| self.opts.force_simple_basecolor
			|| self.opts.debug_bind_pose
			|| self.opts.debug_primitive_colors
		{
			return;
		}
		let mut state = DrawBindState::default();
		let mut current_key = None;
		for &draw_index in &self.outline_draw_indices {
			let key = MaterialRenderStateKey::from_draw_outline(&self.draws[draw_index], &self.opts);
			if current_key != Some(key) {
				let Some(pipeline_outline_toon) = self.pipelines_outline_toon.get(&key) else {
					continue;
				};
				pass.set_pipeline(pipeline_outline_toon);
				current_key = Some(key);
				state = DrawBindState::default();
			}
			let Some(bind_material) = self.draws[draw_index].bind_outline_material.as_ref() else {
				continue;
			};
			pass.set_stencil_reference(self.draws[draw_index].outline_stencil_state.reference as u32);
			self.draw_inner_with_material(pass, &mut state, draw_index, bind_material);
		}
	}

	pub fn encode_compute_fur_cards(&self, encoder: &mut wgpu::CommandEncoder) {
		if self.fur_draw_indices.is_empty() {
			return;
		}
		let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
			label: Some("compute_fur_cards"),
			timestamp_writes: None,
		});
		let Some(compute_pipeline) = self.compute_fur_cards_compute_pipeline.as_ref() else {
			return;
		};
		pass.set_pipeline(&compute_pipeline._pipeline);
		for &draw_index in &self.fur_draw_indices {
			let Some(compute_fur_cards) = self.draws[draw_index]._compute_fur_cards.as_ref() else {
				continue;
			};
			if compute_fur_cards.dispatch_workgroups == 0 {
				continue;
			}
			pass.set_bind_group(0, &compute_fur_cards.bind_group, &[]);
			pass.dispatch_workgroups(compute_fur_cards.dispatch_workgroups, 1, 1);
		}
	}

	fn update_compute_fur_cards_source_vertices(&mut self, queue: &wgpu::Queue) {
		let draws = &mut self.draws;
		let skin_palettes = &self.skin_palettes;
		let source_vertex_scratch = &mut self.fur_source_vertex_scratch;
		let palette_matrix_scratch = &mut self.fur_palette_matrix_scratch;
		let mut palette_matrix_scratch_index = None;
		for &draw_index in &self.fur_draw_indices {
			let Some(draw) = draws.get_mut(draw_index) else {
				continue;
			};
			let Some(compute_fur_cards) = draw._compute_fur_cards.as_mut() else {
				continue;
			};
			let Some(palette) = skin_palettes.get(draw.skin_palette_index) else {
				continue;
			};
			if palette.static_identity {
				continue;
			}
			if !palette.uploaded_changed {
				continue;
			}
			if palette_matrix_scratch_index != Some(draw.skin_palette_index) {
				compute_fur_cards_palette_matrices(&palette.uploaded, palette_matrix_scratch);
				palette_matrix_scratch_index = Some(draw.skin_palette_index);
			}
			let base_vertices = &draw.buffer_upload.vertices;
			compute_fur_cards_skinned_source_vertices_from_matrices(base_vertices, palette_matrix_scratch, source_vertex_scratch);
			if source_vertex_scratch.len() != base_vertices.len() {
				continue;
			}
			queue.write_buffer(
				&compute_fur_cards.source_vertex_buffer,
				0,
				bytemuck::cast_slice(source_vertex_scratch),
			);
		}
	}

	pub fn set_avatar_outline(&mut self, queue: &wgpu::Queue, outline: AvatarOutlineOptions) {
		if self.opts.avatar_outline == outline {
			return;
		}
		self.opts.avatar_outline = outline;
		self.rebuild_draw_order();
		self.rewrite_avatar_materials(queue);
	}

	fn rebuild_draw_order(&mut self) {
		let draw_state = build_draw_order(&self.draws, &self.opts);
		self.outline_draw_indices = draw_state.outline_draw_indices.into_boxed_slice();
		self.fur_draw_indices = draw_state.fur_draw_indices.into_boxed_slice();
		self.opaque_batches = draw_state.opaque_batches;
		self.transparent_backpass_draw_indices = draw_state.transparent_backpass_draw_indices.into_boxed_slice();
		self.blended_batches = draw_state.blended_batches;
		self.active_draw_indices = draw_state.active_draw_indices.into_boxed_slice();
		self.active_morph_draw_indices = draw_state.active_morph_draw_indices.into_boxed_slice();
		self.needs_screen_refraction = draw_state.needs_screen_refraction;
		self.active_skin_palette_indices = draw_state.active_skin_palette_indices.into_boxed_slice();
		self.runtime_requirements = draw_state.runtime_requirements;
	}

	fn rewrite_avatar_materials(&self, queue: &wgpu::Queue) {
		let default_mtoon = UnaMtoonMaterial::default();
		for draw in &self.draws {
			let material =
				mesh_draw_material_gpu_runtime(&draw.material, &default_mtoon, &self.opts, draw.mesh_index, draw.primitive_index);
			queue.write_buffer(&draw.draw_material, 0, bytemuck::bytes_of(&material));
		}
	}

	fn rebuild_draw_material_bind_groups(&mut self, device: &wgpu::Device, draw_index: usize) {
		let (bind_material, bind_outline_material) = create_mesh_draw_bind_groups(
			device,
			&self.material_layout,
			&self.outline_material_layout,
			self.shader_variant_tier,
			&self.texture_views,
			&self._samplers,
			&self.image_sampler_indices,
			&self.reflection_cube_sampler,
			&self.draws[draw_index].material,
			&self.draws[draw_index].draw_transform,
			&self.draws[draw_index].draw_material,
		);
		let draw = &mut self.draws[draw_index];
		draw.bind_material = Some(bind_material);
		draw.bind_outline_material = Some(bind_outline_material);
	}

	pub(crate) fn rebuild_material_bind_groups(&mut self, device: &wgpu::Device) -> usize {
		let mut rebuilt = 0;
		for draw_index in 0..self.draws.len() {
			if !self.draws[draw_index].active() {
				continue;
			}
			self.rebuild_draw_material_bind_groups(device, draw_index);
			rebuilt += 1;
		}
		rebuilt
	}

	pub fn refresh_draw_materials_from_scene(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, scene: &UnaSceneSnapshot) -> usize {
		let default_material = UnaMaterialPbr::default();
		let default_mtoon = UnaMtoonMaterial::default();
		let mut changed = 0;
		for draw_index in 0..self.draws.len() {
			let draw_mesh_index = self.draws[draw_index].mesh_index;
			let draw_primitive_index = self.draws[draw_index].primitive_index;
			let Some(primitive) = scene.meshes.get(draw_mesh_index).and_then(|mesh| mesh.get(draw_primitive_index)) else {
				continue;
			};
			let material_slot_index = primitive
				.material_index
				.filter(|material_index| scene.materials.get(*material_index).is_some());
			let material = material_slot_index
				.and_then(|material_index| scene.materials.get(material_index))
				.unwrap_or(&default_material);
			if self.draws[draw_index].material == *material && self.draws[draw_index].material_slot_index == material_slot_index {
				continue;
			}
			{
				let draw = &mut self.draws[draw_index];
				draw.material_slot_index = material_slot_index;
				draw.material = material.clone();
				draw.stencil_state = material_stencil_state(&draw.material);
				draw.color_mask = material_color_mask(&draw.material);
				draw.outline_stencil_state = material_outline_stencil_state(&draw.material);
				draw.outline_color_mask = material_outline_color_mask(&draw.material);
				draw.fur_stencil_state = material_fur_stencil_state(&draw.material);
				draw.fur_color_mask = material_fur_color_mask(&draw.material);
				draw.texture_indices = material_texture_indices(&draw.material).into_boxed_slice();
				draw.cube_texture_indices = material_cube_texture_indices(&draw.material).into_boxed_slice();
				draw.shading = material.shading;
				draw.alpha_mode = material.alpha_mode;
				draw._compute_fur_cards = None;
				let material_gpu =
					mesh_draw_material_gpu_runtime(&draw.material, &default_mtoon, &self.opts, draw.mesh_index, draw.primitive_index);
				queue.write_buffer(&draw.draw_material, 0, bytemuck::bytes_of(&material_gpu));
			}
			if self.draws[draw_index].active() {
				self.rebuild_draw_material_bind_groups(device, draw_index);
			} else {
				self.draws[draw_index].bind_material = None;
				self.draws[draw_index].bind_outline_material = None;
			}
			changed += 1;
		}
		if changed > 0 {
			self.rebuild_draw_order();
		}
		changed
	}

	pub(crate) fn ensure_active_draw_gpu_resources(
		&mut self,
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		scene: &UnaSceneSnapshot,
	) -> usize {
		let mut ensured = 0;
		for active_index in 0..self.active_draw_indices.len() {
			let draw_index = self.active_draw_indices[active_index];
			if self.ensure_draw_gpu_resources(device, queue, scene, draw_index) {
				ensured += 1;
			}
		}
		ensured
	}

	#[inline]
	fn pipeline_for_key(&self, key: DrawPipelineKey) -> &wgpu::RenderPipeline {
		self.pipelines.get(&key).expect("draw pipeline was requested but not created")
	}

	fn set_draw_stencil_reference(pass: &mut wgpu::RenderPass<'_>, draw: &MeshDraw) {
		pass.set_stencil_reference(draw.stencil_state.reference as u32);
	}

	fn set_fur_stencil_reference(pass: &mut wgpu::RenderPass<'_>, draw: &MeshDraw) {
		pass.set_stencil_reference(draw.fur_stencil_state.reference as u32);
	}

	pub fn draw_opaque(&self, pass: &mut wgpu::RenderPass<'_>) {
		if self.opaque_batches.is_empty() {
			return;
		}
		let mut state = DrawBindState::default();
		for batch in &self.opaque_batches {
			pass.set_pipeline(self.pipeline_for_key(batch.pipeline));
			for &draw_index in &batch.draw_indices {
				Self::set_draw_stencil_reference(pass, &self.draws[draw_index]);
				self.draw_inner(pass, &mut state, draw_index);
			}
		}
	}

	/// `alphaMode: BLEND`（および VRM0 MToon Transparent）。
	/// lilToon transparent z-write は `_PreCull` / `_SubpassCutoff` による FORWARD_BACK 相当 pass を先に描き、
	/// `_ZWrite` が有効な Forward color pass も本家同様に depth write ありで描く。
	pub fn draw_blended(&self, pass: &mut wgpu::RenderPass<'_>) {
		if self.fur_draw_indices.is_empty() && self.blended_batches.is_empty() && self.transparent_backpass_draw_indices.is_empty() {
			return;
		}
		let mut state = DrawBindState::default();
		self.draw_transparent_backpass(pass, &mut state);
		self.draw_blended_batches_from(pass, &mut state, None);
		self.draw_fur_blended(pass, &mut state);
	}

	fn draw_transparent_backpass(&self, pass: &mut wgpu::RenderPass<'_>, state: &mut DrawBindState) {
		if !self.transparent_backpass_draw_indices.is_empty() {
			let mut backpass_key = None;
			for &draw_index in &self.transparent_backpass_draw_indices {
				let zwrite = self.draws[draw_index]
					.material
					.liltoon_like_runtime()
					.is_none_or(|u| u.blend_state.pre_zwrite_factor > 0.5);
				let key = DrawPipelineKey::new(
					if zwrite {
						DrawPipelineKind::TransparentToonBackpass
					} else {
						DrawPipelineKind::TransparentToonBackpassNoZWrite
					},
					&self.draws[draw_index],
					&self.opts,
				);
				if backpass_key != Some(key) {
					pass.set_pipeline(self.pipeline_for_key(key));
					backpass_key = Some(key);
					*state = DrawBindState::default();
				}
				Self::set_draw_stencil_reference(pass, &self.draws[draw_index]);
				self.draw_inner(pass, state, draw_index);
			}
		}
	}

	fn first_screen_refraction_blended_draw(&self) -> Option<(usize, usize)> {
		for (batch_index, batch) in self.blended_batches.iter().enumerate() {
			for (draw_pos, &draw_index) in batch.draw_indices.iter().enumerate() {
				if draw_uses_screen_refraction_grab(&self.draws[draw_index]) {
					return Some((batch_index, draw_pos));
				}
			}
		}
		None
	}

	fn draw_blended_batches_until(&self, pass: &mut wgpu::RenderPass<'_>, state: &mut DrawBindState, end: Option<(usize, usize)>) {
		let Some((end_batch, end_pos)) = end else {
			self.draw_blended_batches_from(pass, state, None);
			return;
		};
		for (batch_index, batch) in self.blended_batches.iter().enumerate() {
			if batch_index > end_batch {
				break;
			}
			pass.set_pipeline(self.pipeline_for_key(batch.pipeline));
			let len = if batch_index == end_batch {
				end_pos
			} else {
				batch.draw_indices.len()
			};
			for &draw_index in batch.draw_indices.iter().take(len) {
				Self::set_draw_stencil_reference(pass, &self.draws[draw_index]);
				self.draw_inner(pass, state, draw_index);
			}
		}
	}

	fn draw_blended_batches_from(&self, pass: &mut wgpu::RenderPass<'_>, state: &mut DrawBindState, start: Option<(usize, usize)>) {
		let (start_batch, start_pos) = start.unwrap_or((0, 0));
		for (batch_index, batch) in self.blended_batches.iter().enumerate().skip(start_batch) {
			pass.set_pipeline(self.pipeline_for_key(batch.pipeline));
			let skip = if batch_index == start_batch { start_pos } else { 0 };
			for &draw_index in batch.draw_indices.iter().skip(skip) {
				Self::set_draw_stencil_reference(pass, &self.draws[draw_index]);
				self.draw_inner(pass, state, draw_index);
			}
		}
	}

	fn draw_fur_blended(&self, pass: &mut wgpu::RenderPass<'_>, state: &mut DrawBindState) {
		if !self.fur_draw_indices.is_empty() {
			*state = DrawBindState::default();
			let mut current_key = None;
			for &draw_index in &self.fur_draw_indices {
				let key = MaterialRenderStateKey::from_draw_fur(&self.draws[draw_index], &self.opts);
				if current_key != Some(key) {
					let Some(pre_toon) = self.pipelines_compute_fur_cards_pre_toon.get(&key) else {
						continue;
					};
					pass.set_pipeline(pre_toon);
					current_key = Some(key);
					*state = DrawBindState::default();
				}
				Self::set_fur_stencil_reference(pass, &self.draws[draw_index]);
				let _ = self.draw_compute_fur_cards_inner(pass, state, draw_index);
			}
			*state = DrawBindState::default();
			let mut current_key = None;
			for &draw_index in &self.fur_draw_indices {
				let key = MaterialRenderStateKey::from_draw_fur(&self.draws[draw_index], &self.opts);
				if current_key != Some(key) {
					let Some(toon) = self.pipelines_compute_fur_cards_toon.get(&key) else {
						continue;
					};
					pass.set_pipeline(toon);
					current_key = Some(key);
					*state = DrawBindState::default();
				}
				Self::set_fur_stencil_reference(pass, &self.draws[draw_index]);
				if self.draw_compute_fur_cards_inner(pass, state, draw_index) {
					continue;
				}
			}
		}
	}

	pub fn draw_blended_before_screen_refraction(&self, pass: &mut wgpu::RenderPass<'_>) {
		let mut state = DrawBindState::default();
		self.draw_transparent_backpass(pass, &mut state);
		self.draw_blended_batches_until(pass, &mut state, self.first_screen_refraction_blended_draw());
	}

	pub fn draw_blended_after_screen_refraction(&self, pass: &mut wgpu::RenderPass<'_>) {
		let mut state = DrawBindState::default();
		self.draw_blended_batches_from(pass, &mut state, self.first_screen_refraction_blended_draw());
		self.draw_fur_blended(pass, &mut state);
	}

	pub fn update_draw_transforms(
		&mut self,
		queue: &wgpu::Queue,
		scene: &UnaSceneSnapshot,
		world: &[Mat4],
		expr_weights: Option<&UnaExpressionWeights>,
		expression_overrides: Option<&BTreeMap<String, f32>>,
		morph_name_overrides: Option<&BTreeMap<String, f32>>,
		refresh_scene_morph_defaults: bool,
	) -> DrawTransformUpdateTimings {
		let mut timings = DrawTransformUpdateTimings::default();
		let debug_skin_legacy_no_inv_mesh = self.opts.debug_skin_legacy_no_inv_mesh;
		let debug_zero_morphs = self.opts.debug_zero_morphs;
		if refresh_scene_morph_defaults {
			self.refresh_morph_defaults_from_scene(scene);
			self.refresh_draw_visibility_from_scene(scene);
		}
		let t_expression0 = Instant::now();
		self.expression_value_scratch.clear();
		if !self.active_morph_draw_indices.is_empty() && (expr_weights.is_some() || expression_overrides.is_some()) {
			self.expression_value_scratch.extend(self.expression_names.iter().map(|name| {
				expression_overrides
					.and_then(|overrides| overrides.get(name).copied())
					.or_else(|| expr_weights.and_then(|weights| weights.preset_weights.get(name).copied()))
					.unwrap_or(0.0)
			}));
		}
		timings.expression_values_ms = t_expression0.elapsed().as_secs_f32() * 1000.0;
		let t_skin0 = Instant::now();
		let mut skin_palette_write_ms = 0.0;
		if !self.active_skin_palette_indices.is_empty() {
			for &palette_index in &self.active_skin_palette_indices {
				let Some(palette) = self.skin_palettes.get_mut(palette_index) else {
					continue;
				};
				let skin = palette.key.skin_index.and_then(|si| scene.skins.get(si));
				let t_write0 = Instant::now();
				Self::write_skin_palette(queue, palette, skin, world, debug_skin_legacy_no_inv_mesh);
				if palette.uploaded_changed {
					skin_palette_write_ms += t_write0.elapsed().as_secs_f32() * 1000.0;
				}
			}
			timings.skin_palette_ms = t_skin0.elapsed().as_secs_f32() * 1000.0;
			timings.skin_palette_write_ms = skin_palette_write_ms;
			if !self.fur_draw_indices.is_empty() {
				let t_fur0 = Instant::now();
				self.update_compute_fur_cards_source_vertices(queue);
				timings.fur_source_vertices_ms = t_fur0.elapsed().as_secs_f32() * 1000.0;
			}
		}
		let expression_values = (!self.expression_value_scratch.is_empty()).then_some(self.expression_value_scratch.as_slice());

		let t_draw0 = Instant::now();
		let mut morph_weights_ms = 0.0;
		if !self.active_morph_draw_indices.is_empty() {
			let t_morph0 = Instant::now();
			for &draw_index in &self.active_morph_draw_indices {
				let Some(d) = self.draws.get_mut(draw_index) else {
					continue;
				};
				let Some(morph_resources) = d.morph_resources.as_ref() else {
					continue;
				};
				let draw_has_active_expression = expression_bindings_have_active_weight(&d.expression_bindings, expression_values);
				let draw_has_active_morph_name_override = morph_names_have_active_override(
					&d.morph_target_names,
					&d.morph_target_override_keys,
					&d.morph_target_override_suffix_keys,
					morph_name_overrides,
				);
				let skip_static_default_morph = !draw_has_active_expression
					&& !draw_has_active_morph_name_override
					&& !debug_zero_morphs
					&& morph_weights_match_default(&d.morph_weights, &d.default_morph_weights, d.morph_target_count);
				if !skip_static_default_morph {
					d.morph_weight_scratch.clear();
					if debug_zero_morphs {
						d.morph_weight_scratch.resize(d.morph_target_count, 0.0);
					} else {
						fill_morph_weights_for_draw(
							&d.default_morph_weights,
							d.morph_target_count,
							&d.expression_bindings,
							expression_values,
							&d.morph_target_names,
							&d.morph_target_override_keys,
							&d.morph_target_override_suffix_keys,
							morph_name_overrides,
							&mut d.morph_weight_scratch,
						);
					}

					if d.morph_weight_scratch.len() == d.morph_target_count {
						if d.morph_weights != d.morph_weight_scratch {
							queue.write_buffer(&morph_resources.weight_buffer, 0, bytemuck::cast_slice(&d.morph_weight_scratch));
							d.morph_weights.clear();
							d.morph_weights.extend_from_slice(&d.morph_weight_scratch);
						}
					} else if !d.morph_weights.is_empty() {
						d.morph_weight_scratch.clear();
						d.morph_weight_scratch.resize(d.morph_target_count, 0.0);
						queue.write_buffer(&morph_resources.weight_buffer, 0, bytemuck::cast_slice(&d.morph_weight_scratch));
						d.morph_weights.clear();
					}
				}
			}
			morph_weights_ms = t_morph0.elapsed().as_secs_f32() * 1000.0;
		}

		for &draw_index in &self.active_draw_indices {
			let Some(d) = self.draws.get_mut(draw_index) else {
				continue;
			};
			let mesh_world = world.get(d.world_node_index).copied().unwrap_or(Mat4::IDENTITY);

			let model = mesh_world.to_cols_array_2d();
			let transform = MeshDrawTransformGpu { model };
			let transform_changed = d.draw_transform_uploaded != Some(transform);
			if transform_changed {
				queue.write_buffer(&d.draw_transform, 0, bytemuck::bytes_of(&transform));
				d.draw_transform_uploaded = Some(transform);
			}
			if transform_changed {
				if let Some(compute_fur_cards) = d._compute_fur_cards.as_mut() {
					let inv_model = mesh_world.inverse().to_cols_array_2d();
					if compute_fur_cards.params.model != model || compute_fur_cards.params.inv_model != inv_model {
						compute_fur_cards.params.model = model;
						compute_fur_cards.params.inv_model = inv_model;
						queue.write_buffer(&compute_fur_cards.params_buffer, 0, bytemuck::bytes_of(&compute_fur_cards.params));
					}
				}
			}
		}
		timings.draw_transform_ms = t_draw0.elapsed().as_secs_f32() * 1000.0;
		timings.morph_weights_ms = morph_weights_ms;
		timings
	}

	pub fn refresh_morph_defaults_from_scene(&mut self, scene: &UnaSceneSnapshot) -> usize {
		if !self.has_morph_draws {
			return 0;
		}
		let mut changed = 0;
		for draw in &mut self.draws {
			let target_count = draw.morph_target_count;
			if target_count == 0 {
				continue;
			}
			if refresh_morph_default_weights(
				&mut draw.default_morph_weights,
				&mut draw.morph_weights,
				scene,
				draw.mesh_index,
				draw.primitive_index,
				&draw.morph_source_indices,
			) {
				changed += 1;
			}
		}
		changed
	}

	pub fn refresh_draw_visibility_from_scene(&mut self, scene: &UnaSceneSnapshot) -> usize {
		write_scene_effective_visibility(scene, &mut self.visibility_scratch);
		let mut changed = 0;
		for draw in &mut self.draws {
			let next = self.visibility_scratch.get(draw.world_node_index).copied().unwrap_or(false);
			let was_active = draw.active();
			if draw.visible != next {
				draw.visible = next;
			}
			if draw.active() != was_active {
				changed += 1;
			}
		}
		if changed > 0 {
			self.rebuild_draw_order();
		}
		changed
	}

	pub(crate) fn promote_visible_draw_residency(&mut self) -> Vec<usize> {
		let mut promoted = Vec::new();
		for (draw_index, draw) in self.draws.iter_mut().enumerate() {
			if draw.visible && !draw.asset_resident {
				draw.asset_resident = true;
				promoted.push(draw_index);
			}
		}
		if !promoted.is_empty() {
			self.rebuild_draw_order();
		}
		promoted
	}

	pub fn refresh_asset_group_residency(&mut self, scene: &UnaSceneSnapshot, active_asset_groups: &[String]) -> usize {
		self.refresh_asset_group_residency_with_changes(scene, active_asset_groups)
			.active_draw_state_changed_count
	}

	pub(crate) fn refresh_asset_group_residency_with_changes(
		&mut self,
		scene: &UnaSceneSnapshot,
		active_asset_groups: &[String],
	) -> SceneMeshAssetResidencyRefresh {
		let mut refresh = SceneMeshAssetResidencyRefresh::default();
		let asset_residency = SceneAssetResidencySets::for_scene(scene, active_asset_groups);
		for (draw_index, draw) in self.draws.iter_mut().enumerate() {
			let next = asset_residency.mesh_primitive_resident(draw.mesh_index, draw.primitive_index);
			let was_active = draw.active();
			if draw.asset_resident != next {
				if next {
					refresh.mesh_buffer_load_indices.push(draw_index);
				} else {
					refresh.mesh_buffer_unload_indices.push(draw_index);
				}
				draw.asset_resident = next;
			}
			if draw.active() != was_active {
				refresh.active_draw_state_changed_count += 1;
			}
		}
		let active_draw_texture_indices = self.active_draw_indices.iter().filter_map(|&draw_index| {
			self.draws
				.get(draw_index)
				.map(|draw| (draw.texture_indices.as_ref(), draw.cube_texture_indices.as_ref()))
		});
		let (next_image_texture_residency, next_cube_texture_residency) =
			texture_residency_for_active_draws(scene, &asset_residency, active_draw_texture_indices);
		refresh.image_texture_load_indices = residency_load_indices(&self.image_texture_residency, &next_image_texture_residency);
		refresh.image_texture_unload_indices = residency_unload_indices(&self.image_texture_residency, &next_image_texture_residency);
		refresh.cube_texture_load_indices = residency_load_indices(&self.cube_texture_residency, &next_cube_texture_residency);
		refresh.cube_texture_unload_indices = residency_unload_indices(&self.cube_texture_residency, &next_cube_texture_residency);
		self.image_texture_residency = next_image_texture_residency;
		self.cube_texture_residency = next_cube_texture_residency;
		let next_material_slot_residency: Vec<bool> = scene
			.materials
			.iter()
			.enumerate()
			.map(|(material_index, _)| asset_residency.material_resident(material_index))
			.collect();
		refresh.material_slot_load_indices = residency_load_indices(&self.material_slot_residency, &next_material_slot_residency);
		refresh.material_slot_unload_indices = residency_unload_indices(&self.material_slot_residency, &next_material_slot_residency);
		self.material_slot_residency = next_material_slot_residency;
		if refresh.active_draw_state_changed_count > 0 {
			self.rebuild_draw_order();
		}
		refresh
	}

	pub(crate) fn apply_mesh_buffer_residency(
		&mut self,
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		scene: &UnaSceneSnapshot,
		load_indices: &[usize],
		unload_indices: &[usize],
	) -> (usize, usize) {
		let mut load_count = 0;
		for draw_index in load_indices.iter().copied() {
			let loaded = self
				.draws
				.get_mut(draw_index)
				.is_some_and(|draw| draw.ensure_mesh_buffers(device, queue));
			if loaded {
				load_count += 1;
			}
			self.ensure_draw_gpu_resources(device, queue, scene, draw_index);
		}
		let mut unload_count = 0;
		for draw_index in unload_indices.iter().copied() {
			let Some(draw) = self.draws.get_mut(draw_index) else {
				continue;
			};
			if !draw.asset_resident && draw.drop_mesh_buffers() {
				unload_count += 1;
			}
		}
		(load_count, unload_count)
	}

	fn ensure_draw_gpu_resources(
		&mut self,
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		scene: &UnaSceneSnapshot,
		draw_index: usize,
	) -> bool {
		let Some(draw) = self.draws.get(draw_index) else {
			return false;
		};
		if !draw.asset_resident {
			return false;
		}
		let needs_bind = draw.bind_material.is_none() || draw.bind_outline_material.is_none();
		let needs_morph = draw.morph_resources.is_none();
		let needs_fur = draw._compute_fur_cards.is_none() && material_has_fur(&draw.material, draw.shading, &self.opts);
		if !needs_bind && !needs_morph && !needs_fur {
			return false;
		}
		if needs_bind {
			self.rebuild_draw_material_bind_groups(device, draw_index);
		}
		if needs_morph {
			let resources = create_mesh_draw_morph_resources(device, queue, &self.morph_bind_group_layout, &self.draws[draw_index]);
			self.draws[draw_index].morph_resources = Some(resources);
		}
		if needs_fur {
			let draw = &self.draws[draw_index];
			let Some(liltoon_like) = draw.material.liltoon_like_runtime() else {
				return true;
			};
			let tex_sampler = texture_sampler_or(
				&self._samplers,
				&self.image_sampler_indices,
				draw.material.base_color_texture_index,
				0,
			);
			let fur_vector_texture_index = liltoon_like.fur.vector_texture_index;
			let fur_vector_view = texture_view_or(
				&self.texture_views.images,
				fur_vector_texture_index,
				&self.texture_views.neutral_vector,
			);
			let fur_length_mask_texture_index = liltoon_like.fur.length_mask_texture_index;
			let fur_length_mask_view =
				texture_view_or(&self.texture_views.images, fur_length_mask_texture_index, &self.texture_views.white);
			let fur_noise_mask_texture_index = liltoon_like.fur.noise_mask_texture_index;
			let fur_noise_mask_view = texture_view_or(&self.texture_views.images, fur_noise_mask_texture_index, &self.texture_views.white);
			let fur_mask_texture_index = liltoon_like.fur.mask_texture_index;
			let fur_mask_view = texture_view_or(&self.texture_views.images, fur_mask_texture_index, &self.texture_views.white);
			let compute_fur_cards_cpu_fur_maps = ComputeFurCardsCpuFurMaps {
				length_mask: fur_length_mask_texture_index.and_then(|index| scene.images.get(index)),
				fur_mask: fur_mask_texture_index.and_then(|index| scene.images.get(index)),
			};
			let compute_fur_cards = create_compute_fur_cards_draw_resources(
				device,
				&self.compute_fur_cards_bind_group_layout,
				&draw.material,
				&draw.buffer_upload.vertices,
				&draw.buffer_upload.indices,
				compute_fur_cards_cpu_fur_maps,
				fur_vector_view,
				fur_length_mask_view,
				fur_noise_mask_view,
				fur_mask_view,
				tex_sampler,
			);
			self.draws[draw_index]._compute_fur_cards = compute_fur_cards;
		}
		true
	}

	pub(crate) fn asset_residency_counts(&self) -> SceneMeshAssetResidencyCounts {
		let total_draw_mesh_primitive_count = self.draws.len();
		let resident_draw_mesh_primitive_count = self.draws.iter().filter(|draw| draw.asset_resident).count();
		let total_draw_mesh_buffer_bytes = self
			.draws
			.iter()
			.map(|draw| draw.vertex_buffer_bytes + draw.index_buffer_bytes)
			.sum::<u64>();
		let resident_draw_mesh_buffer_bytes = self
			.draws
			.iter()
			.filter(|draw| draw.asset_resident)
			.map(|draw| draw.vertex_buffer_bytes + draw.index_buffer_bytes)
			.sum::<u64>();
		let total_image_texture_count = self.image_texture_residency.len();
		let resident_image_texture_count = self.image_texture_residency.iter().filter(|resident| **resident).count();
		let draws_using_inactive_image_texture_count = self
			.draws
			.iter()
			.filter(|draw| {
				draw.texture_indices
					.iter()
					.any(|index| self.image_texture_residency.get(*index).is_some_and(|resident| !resident))
			})
			.count();
		let total_material_slot_count = self.material_slot_residency.len();
		let resident_material_slot_count = self.material_slot_residency.iter().filter(|resident| **resident).count();
		let active_gaps = self.active_residency_gaps();
		SceneMeshAssetResidencyCounts {
			total_draw_mesh_primitive_count,
			resident_draw_mesh_primitive_count,
			inactive_draw_mesh_primitive_count: total_draw_mesh_primitive_count.saturating_sub(resident_draw_mesh_primitive_count),
			total_draw_mesh_buffer_bytes,
			resident_draw_mesh_buffer_bytes,
			inactive_draw_mesh_buffer_bytes: total_draw_mesh_buffer_bytes.saturating_sub(resident_draw_mesh_buffer_bytes),
			total_image_texture_count,
			resident_image_texture_count,
			inactive_image_texture_count: total_image_texture_count.saturating_sub(resident_image_texture_count),
			draws_using_inactive_image_texture_count,
			active_draws_using_inactive_image_texture_count: active_gaps.active_draws_using_inactive_image_texture_count,
			inactive_image_textures_used_by_active_draw_count: active_gaps.inactive_image_texture_indices.len(),
			inactive_image_textures_used_by_active_draw: active_gaps.inactive_image_texture_indices,
			active_draws_using_inactive_cube_texture_count: active_gaps.active_draws_using_inactive_cube_texture_count,
			inactive_cube_textures_used_by_active_draw_count: active_gaps.inactive_cube_texture_indices.len(),
			inactive_cube_textures_used_by_active_draw: active_gaps.inactive_cube_texture_indices,
			total_material_slot_count,
			resident_material_slot_count,
			inactive_material_slot_count: total_material_slot_count.saturating_sub(resident_material_slot_count),
			active_draws_using_inactive_material_slot_count: active_gaps.active_draws_using_inactive_material_slot_count,
			inactive_material_slots_used_by_active_draw_count: active_gaps.inactive_material_slot_indices.len(),
			inactive_material_slots_used_by_active_draw: active_gaps.inactive_material_slot_indices,
		}
	}

	pub(crate) fn active_residency_gaps(&self) -> SceneMeshActiveResidencyGaps {
		active_residency_gaps_from_draws(
			self.draws.iter().map(|draw| {
				(
					draw.active(),
					&*draw.texture_indices,
					&*draw.cube_texture_indices,
					draw.material_slot_index,
				)
			}),
			&self.image_texture_residency,
			&self.cube_texture_residency,
			&self.material_slot_residency,
		)
	}

	pub(crate) fn promote_material_slot_residency(&mut self, material_slot_indices: &[usize]) -> usize {
		promote_residency_indices(&mut self.material_slot_residency, material_slot_indices)
	}

	pub(crate) fn promote_image_texture_residency(&mut self, image_texture_indices: &[usize]) -> usize {
		promote_residency_indices(&mut self.image_texture_residency, image_texture_indices)
	}

	pub(crate) fn promote_cube_texture_residency(&mut self, cube_texture_indices: &[usize]) -> usize {
		promote_residency_indices(&mut self.cube_texture_residency, cube_texture_indices)
	}

	pub(crate) fn apply_image_texture_view_residency(
		&mut self,
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		scene: &UnaSceneSnapshot,
		load_indices: &[usize],
		unload_indices: &[usize],
		cube_load_indices: &[usize],
		cube_unload_indices: &[usize],
	) -> (usize, usize, usize, usize) {
		let mut loaded = 0;
		for index in load_indices {
			let Some(slot) = self.image_texture_slots.get_mut(*index) else {
				continue;
			};
			let Some(source_view) = slot.ensure_uploaded(device, queue, Some(scene), &mut self.lazy_gpu_texture_compression) else {
				continue;
			};
			let Some(current_view) = self.texture_views.images.get_mut(*index) else {
				continue;
			};
			*current_view = source_view;
			loaded += 1;
		}
		let mut unloaded = 0;
		for index in unload_indices {
			let Some(slot) = self.image_texture_slots.get_mut(*index) else {
				continue;
			};
			let Some(current_view) = self.texture_views.images.get_mut(*index) else {
				continue;
			};
			*current_view = self.texture_views.transparent_black.clone();
			if slot.unload() {
				unloaded += 1;
			}
		}
		let mut cube_loaded = 0;
		for index in cube_load_indices {
			if let Some(Some(cube_slot)) = self.cube_texture_slots.get_mut(*index) {
				self.texture_views.cubes[*index] = cube_slot.ensure_uploaded(device, queue, Some(scene));
				cube_loaded += usize::from(self.texture_views.cubes[*index].is_some());
			}
		}
		let mut cube_unloaded = 0;
		for index in cube_unload_indices {
			if let Some(Some(cube_slot)) = self.cube_texture_slots.get_mut(*index) {
				if let Some(current_cube_view) = self.texture_views.cubes.get_mut(*index) {
					*current_cube_view = None;
				}
				if cube_slot.unload() {
					cube_unloaded += 1;
				}
			}
		}
		(loaded, unloaded, cube_loaded, cube_unloaded)
	}

	pub fn is_empty(&self) -> bool {
		self.active_draw_indices.is_empty()
	}

	pub fn needs_screen_refraction(&self) -> bool {
		self.needs_screen_refraction
	}

	pub(crate) fn runtime_requirements(&self) -> SceneMeshRuntimeRequirements {
		self.runtime_requirements
	}

	pub fn set_screen_grab_view(&mut self, device: &wgpu::Device, view: &wgpu::TextureView) {
		self.frame_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("mesh_frame"),
			layout: &self.frame_layout,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: self.frame_buffer.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(view),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: wgpu::BindingResource::Sampler(&self.screen_grab_sampler),
				},
				wgpu::BindGroupEntry {
					binding: 3,
					resource: wgpu::BindingResource::TextureView(&self.audio_link_view),
				},
			],
		});
	}

	pub fn upload_audio_link_texture(&mut self, queue: &wgpu::Queue, frame: &crate::audio_link::AudioLinkTextureFrame) {
		if frame.sequence == self.audio_link_uploaded_sequence || frame.pixels.is_empty() {
			return;
		}
		queue.write_texture(
			wgpu::TexelCopyTextureInfo {
				texture: &self._audio_link_texture,
				mip_level: 0,
				origin: wgpu::Origin3d::ZERO,
				aspect: wgpu::TextureAspect::All,
			},
			&frame.pixels,
			wgpu::TexelCopyBufferLayout {
				offset: 0,
				bytes_per_row: Some(crate::audio_link::AUDIO_LINK_TEXTURE_WIDTH * 4),
				rows_per_image: Some(crate::audio_link::AUDIO_LINK_TEXTURE_HEIGHT),
			},
			wgpu::Extent3d {
				width: crate::audio_link::AUDIO_LINK_TEXTURE_WIDTH,
				height: crate::audio_link::AUDIO_LINK_TEXTURE_HEIGHT,
				depth_or_array_layers: 1,
			},
		);
		self.audio_link_uploaded_sequence = frame.sequence;
		self.audio_link_frame_params = [1.0, frame.rms, frame.peak, frame.sequence as f32];
	}

	pub fn set_audio_link_external_enabled(&mut self, enabled: bool) {
		let next = if enabled {
			[
				1.0,
				self.audio_link_frame_params[1],
				self.audio_link_frame_params[2],
				self.audio_link_frame_params[3],
			]
		} else {
			[0.0; 4]
		};
		if self.audio_link_frame_params != next {
			self.audio_link_frame_params = next;
		}
	}

	pub fn audio_link_frame_params(&self) -> [f32; 4] {
		self.audio_link_frame_params
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
	use un_avatar_core::{UnaMorphTargetDeltas, UnaSceneNode};

	fn empty_source_metadata() -> UnaImageSourceMetadata {
		UnaImageSourceMetadata {
			name: None,
			mime_type: None,
			uri: None,
			source_pixel_format: None,
			channels: None,
			color_space: None,
			texture_type: None,
			texture_shape: None,
			source_layout: None,
			unity_generate_cubemap: None,
			srgb: None,
			sampler: None,
			width: None,
			height: None,
			byte_offset: None,
			byte_length: 0,
			source_hash: 0,
			source_file_path: None,
			encoded_bytes: None,
		}
	}

	#[test]
	fn mesh_sampler_metadata_maps_to_wgpu_modes() {
		assert_eq!(wgpu_address_mode(UnaTextureWrapMode::ClampToEdge), wgpu::AddressMode::ClampToEdge);
		assert_eq!(wgpu_address_mode(UnaTextureWrapMode::MirrorOnce), wgpu::AddressMode::ClampToEdge);
		assert_eq!(
			wgpu_address_mode(UnaTextureWrapMode::MirroredRepeat),
			wgpu::AddressMode::MirrorRepeat
		);
		assert_eq!(wgpu_address_mode(UnaTextureWrapMode::Repeat), wgpu::AddressMode::Repeat);
		assert_eq!(wgpu_filter_mode(UnaTextureFilterMode::Nearest), wgpu::FilterMode::Nearest);
		assert_eq!(wgpu_filter_mode(UnaTextureFilterMode::Linear), wgpu::FilterMode::Linear);
	}

	#[test]
	fn liltoon_stencil_writer_maps_from_exported_float_params() {
		let material = UnaMaterialPbr {
			unavatar_material: Some(serde_json::json!({
				"floatParams": {
					"_StencilComp": 8,
					"_StencilPass": 2,
					"_StencilReadMask": 255,
					"_StencilRef": 12,
					"_StencilWriteMask": 255,
					"_StencilFail": 0,
					"_StencilZFail": 0,
					"_ColorMask": 0
				}
			})),
			..Default::default()
		};

		let stencil = material_stencil_state(&material);

		assert_eq!(stencil.reference, 12);
		assert_eq!(unity_compare_function(stencil.compare), wgpu::CompareFunction::Always);
		assert_eq!(unity_stencil_operation(stencil.pass_op), wgpu::StencilOperation::Replace);
		assert_eq!(material_color_mask(&material), 0);
		assert_eq!(
			color_writes_from_unity_mask(material_color_mask(&material)),
			wgpu::ColorWrites::empty()
		);
	}

	#[test]
	fn liltoon_stencil_consumer_maps_not_equal_test() {
		let material = UnaMaterialPbr {
			unavatar_material: Some(serde_json::json!({
				"floatParams": {
					"_StencilComp": 6,
					"_StencilPass": 0,
					"_StencilReadMask": 255,
					"_StencilRef": 12,
					"_StencilWriteMask": 255,
					"_StencilFail": 0,
					"_StencilZFail": 0,
					"_ColorMask": 15
				}
			})),
			..Default::default()
		};

		let stencil = material_stencil_state(&material);
		let wgpu_stencil = stencil.to_wgpu();

		assert_eq!(stencil.reference, 12);
		assert_eq!(wgpu_stencil.front.compare, wgpu::CompareFunction::NotEqual);
		assert_eq!(wgpu_stencil.front.pass_op, wgpu::StencilOperation::Keep);
		assert_eq!(wgpu_stencil.read_mask, 255);
		assert_eq!(wgpu_stencil.write_mask, 255);
		assert_eq!(color_writes_from_unity_mask(material_color_mask(&material)), wgpu::ColorWrites::ALL);
	}

	#[test]
	fn liltoon_outline_and_fur_stencil_use_dedicated_prefixes() {
		let material = UnaMaterialPbr {
			unavatar_material: Some(serde_json::json!({
				"floatParams": {
					"_OutlineStencilComp": 6,
					"_OutlineStencilRef": 1,
					"_OutlineStencilPass": 0,
					"_OutlineColorMask": 15,
					"_FurStencilComp": 3,
					"_FurStencilRef": 7,
					"_FurStencilPass": 2,
					"_FurColorMask": 8
				}
			})),
			..Default::default()
		};

		let outline = material_outline_stencil_state(&material);
		let fur = material_fur_stencil_state(&material);

		assert_eq!(outline.reference, 1);
		assert_eq!(unity_compare_function(outline.compare), wgpu::CompareFunction::NotEqual);
		assert_eq!(material_outline_color_mask(&material), 15);
		assert_eq!(fur.reference, 7);
		assert_eq!(unity_compare_function(fur.compare), wgpu::CompareFunction::Equal);
		assert_eq!(unity_stencil_operation(fur.pass_op), wgpu::StencilOperation::Replace);
		assert_eq!(
			color_writes_from_unity_mask(material_fur_color_mask(&material)),
			wgpu::ColorWrites::RED
		);
	}

	#[test]
	fn mesh_primitive_asset_residency_keeps_active_and_unowned_primitives() {
		let scene = UnaSceneSnapshot {
			asset_group_ownership: vec![
				un_avatar_core::UnaSceneAssetGroupOwnership {
					group_id: "outfit:coat".to_string(),
					mesh_primitives: vec![un_avatar_core::UnaMeshPrimitiveKey {
						mesh_index: 1,
						primitive_index: 0,
					}],
					materials: vec![3],
					images: vec![5],
					..Default::default()
				},
				un_avatar_core::UnaSceneAssetGroupOwnership {
					group_id: "outfit:hat".to_string(),
					mesh_primitives: vec![un_avatar_core::UnaMeshPrimitiveKey {
						mesh_index: 2,
						primitive_index: 0,
					}],
					materials: vec![4],
					images: vec![6],
					..Default::default()
				},
			],
			..Default::default()
		};
		let active_groups = vec!["outfit:coat".to_string()];
		let residency = SceneAssetResidencySets::for_scene(&scene, &active_groups);

		assert!(residency.mesh_primitive_resident(0, 0));
		assert!(residency.mesh_primitive_resident(1, 0));
		assert!(!residency.mesh_primitive_resident(2, 0));
		assert!(residency.material_resident(0));
		assert!(residency.material_resident(3));
		assert!(!residency.material_resident(4));
		assert!(residency.image_resident(0));
		assert!(residency.image_resident(5));
		assert!(!residency.image_resident(6));
		let no_active_groups = SceneAssetResidencySets::for_scene(&scene, &[]);
		assert!(no_active_groups.mesh_primitive_resident(2, 0));
	}

	#[test]
	fn asset_residency_treats_empty_base_group_as_selected_group() {
		let scene = UnaSceneSnapshot {
			asset_group_ownership: vec![
				un_avatar_core::UnaSceneAssetGroupOwnership {
					group_id: String::new(),
					mesh_primitives: vec![un_avatar_core::UnaMeshPrimitiveKey {
						mesh_index: 0,
						primitive_index: 0,
					}],
					materials: vec![0],
					images: vec![0],
					..Default::default()
				},
				un_avatar_core::UnaSceneAssetGroupOwnership {
					group_id: "outfit:coat".to_string(),
					mesh_primitives: vec![un_avatar_core::UnaMeshPrimitiveKey {
						mesh_index: 1,
						primitive_index: 0,
					}],
					materials: vec![1],
					images: vec![1],
					..Default::default()
				},
			],
			..Default::default()
		};
		let base_group = vec![String::new()];
		let residency = SceneAssetResidencySets::for_scene(&scene, &base_group);

		assert!(residency.mesh_primitive_resident(0, 0));
		assert!(!residency.mesh_primitive_resident(1, 0));
		assert!(residency.material_resident(0));
		assert!(!residency.material_resident(1));
		assert!(residency.image_resident(0));
		assert!(!residency.image_resident(1));
	}

	#[test]
	fn residency_transition_indices_report_loads_and_unloads() {
		assert_eq!(
			residency_load_indices(&[true, false, false, true], &[false, true, false, true]),
			vec![1]
		);
		assert_eq!(
			residency_unload_indices(&[true, false, false, true], &[false, true, false, true]),
			vec![0]
		);
		assert_eq!(residency_load_indices(&[], &[true, false, true]), vec![0, 2]);
		assert_eq!(residency_unload_indices(&[true, false, true], &[]), vec![0, 2]);
	}

	#[test]
	fn promote_residency_indices_counts_newly_resident_slots() {
		let mut residency = vec![true, false, false, true];

		assert_eq!(promote_residency_indices(&mut residency, &[1, 3, 4, 1]), 1);
		assert_eq!(residency, vec![true, true, false, true]);
		assert_eq!(promote_residency_indices(&mut residency, &[2]), 1);
		assert_eq!(residency, vec![true, true, true, true]);
	}

	#[test]
	fn active_residency_gaps_include_cube_textures() {
		let draw_a_textures = [1usize];
		let draw_a_cubes = [3usize];
		let draw_b_textures = [2usize];
		let draw_b_cubes = [4usize];
		let draws = [
			(true, draw_a_textures.as_slice(), draw_a_cubes.as_slice(), Some(1usize)),
			(false, draw_b_textures.as_slice(), draw_b_cubes.as_slice(), Some(2usize)),
		];

		let gaps = active_residency_gaps_from_draws(
			draws,
			&[true, false, true],
			&[true, true, true, false, false],
			&[true, false, false],
		);

		assert_eq!(gaps.inactive_image_texture_indices, vec![1]);
		assert_eq!(gaps.inactive_cube_texture_indices, vec![3]);
		assert_eq!(gaps.inactive_material_slot_indices, vec![1]);
		assert_eq!(gaps.active_draws_using_inactive_image_texture_count, 1);
		assert_eq!(gaps.active_draws_using_inactive_cube_texture_count, 1);
		assert_eq!(gaps.active_draws_using_inactive_material_slot_count, 1);
	}

	#[test]
	fn asset_residency_refresh_reports_scoped_resource_changes() {
		let refresh = SceneMeshAssetResidencyRefresh {
			mesh_buffer_load_indices: vec![1],
			image_texture_load_indices: vec![2],
			material_slot_unload_indices: vec![4],
			..Default::default()
		};

		assert!(refresh.has_scoped_resource_changes());
		assert!(!SceneMeshAssetResidencyRefresh::default().has_scoped_resource_changes());
	}

	#[test]
	fn texture_mip_copy_layout_matches_upload_formats() {
		assert_eq!(texture_mip_copy_layout(TextureUploadKind::Rgba, 5, 3), (20, 3));
		assert_eq!(texture_mip_copy_layout(TextureUploadKind::Bc1Srgb, 5, 3), (16, 1));
		assert_eq!(texture_mip_copy_layout(TextureUploadKind::Bc5Unorm, 5, 7), (32, 2));
		assert_eq!(texture_mip_copy_layout(TextureUploadKind::Bc7Unorm, 8, 9), (32, 3));
		assert_eq!(texture_mip_copy_layout(TextureUploadKind::Bc7Srgb, 8, 9), (32, 3));
	}

	#[test]
	fn texture_upload_summary_separates_deferred_mip_bytes() {
		let mut summary = TextureUploadSummary::default();

		summary.record_image(4, 4, 4, 4, 64, true);
		summary.record_image(8, 4, 4, 2, 32, false);

		assert_eq!(summary.image_count, 2);
		assert_eq!(summary.uploaded_mip_bytes, 64);
		assert_eq!(summary.deferred_image_upload_count, 1);
		assert_eq!(summary.deferred_image_mip_bytes, 32);
		assert_eq!(summary.source_bytes, 192);
		assert_eq!(summary.resized_count, 1);
	}

	#[test]
	fn material_texture_indices_collects_unique_pbr_slots() {
		let mat = UnaMaterialPbr {
			base_color_texture_index: Some(3),
			normal_texture_index: Some(4),
			occlusion_texture_index: Some(3),
			emissive_texture_index: Some(7),
			..Default::default()
		};

		assert_eq!(material_texture_indices(&mat), vec![3, 4, 7]);
	}

	#[test]
	fn material_texture_indices_keep_reflection_cube_separate() {
		let mat = UnaMaterialPbr {
			shading: UnaShadingModel::MToonLike,
			base_color_texture_index: Some(3),
			mtoon: Some(UnaMtoonMaterial {
				reflection_cube_texture_index: Some(9),
				..Default::default()
			}),
			..Default::default()
		};

		assert_eq!(material_texture_indices(&mat), vec![3]);
		assert_eq!(material_cube_texture_indices(&mat), vec![9]);
		assert_eq!(material_resident_texture_indices(&mat), vec![3, 9]);
	}

	#[test]
	fn material_texture_indices_skip_disabled_liltoon_feature_slots() {
		let mut liltoon = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon.main_color.second_texture_index = Some(10);
		liltoon.matcap.texture_index = Some(11);
		liltoon.emission.texture_index = Some(12);
		liltoon.fur.mask_texture_index = Some(13);
		liltoon.reflection.cube_texture_index = Some(14);
		liltoon.reflection.cube_override_factor = 1.0;
		liltoon.parallax.texture_index = Some(15);
		liltoon.main_color.main_color_adjust_mask_texture_index = Some(16);
		let mut mat = UnaMaterialPbr {
			shading: UnaShadingModel::LilToonLike,
			base_color_texture_index: Some(3),
			liltoon_like: Some(liltoon),
			..Default::default()
		};

		assert_eq!(material_texture_indices(&mat), vec![3]);

		let liltoon = mat.liltoon_like.as_mut().unwrap();
		liltoon.main_color.second_enabled_factor = 1.0;
		liltoon.matcap.enabled_factor = 1.0;
		liltoon.emission.enabled_factor = 1.0;
		liltoon.fur.enabled_factor = 1.0;
		liltoon.reflection.enabled_factor = 1.0;
		liltoon.parallax.enabled_factor = 1.0;
		liltoon.main_color.main_texture_hsvg_factor = [0.1, 1.0, 1.0, 1.0];

		assert_eq!(material_texture_indices(&mat), vec![3, 10, 11, 12, 13, 15, 16]);
		assert_eq!(material_cube_texture_indices(&mat), vec![14]);
	}

	#[test]
	fn untoon_shader_features_skip_disabled_liltoon_feature_slots() {
		let mut liltoon = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon.shadow.enabled_factor = 0.0;
		liltoon.shadow.color_texture_index = Some(10);
		liltoon.matcap.texture_index = Some(11);
		liltoon.reflection.color_texture_index = Some(12);
		liltoon.reflection.anisotropy_tangent_texture_index = Some(13);
		liltoon.rim.texture_index = Some(14);
		liltoon.emission.texture_index = Some(15);
		liltoon.backlight.texture_index = Some(16);
		liltoon.glitter.color_texture_index = Some(17);
		liltoon.normal.second_texture_index = Some(18);
		liltoon.parallax.texture_index = Some(19);
		liltoon.main_color.main_color_adjust_mask_texture_index = Some(20);
		let mut mat = UnaMaterialPbr {
			shading: UnaShadingModel::LilToonLike,
			liltoon_like: Some(liltoon),
			..Default::default()
		};

		let features = material_untoon_shader_features(&mat, UnaShadingModel::LilToonLike, &SceneMeshLoadOpts::default());
		assert!(!features.main_layers);
		assert!(!features.shadow_layers);
		assert!(!features.matcap);
		assert!(!features.reflection);
		assert!(!features.anisotropy);
		assert!(!features.rim);
		assert!(!features.emission);
		assert!(!features.backlight);
		assert!(!features.glitter);
		assert!(!features.normal_second);
		assert!(!features.parallax);

		let liltoon = mat.liltoon_like.as_mut().unwrap();
		liltoon.shadow.enabled_factor = 1.0;
		liltoon.matcap.enabled_factor = 1.0;
		liltoon.reflection.enabled_factor = 1.0;
		liltoon.reflection.anisotropy_enabled_factor = 1.0;
		liltoon.rim.enabled_factor = 1.0;
		liltoon.emission.enabled_factor = 1.0;
		liltoon.backlight.enabled_factor = 1.0;
		liltoon.glitter.enabled_factor = 1.0;
		liltoon.normal.second_enabled_factor = 1.0;
		liltoon.parallax.enabled_factor = 1.0;
		liltoon.main_color.main_texture_hsvg_factor = [0.1, 1.0, 1.0, 1.0];

		let features = material_untoon_shader_features(&mat, UnaShadingModel::LilToonLike, &SceneMeshLoadOpts::default());
		assert!(features.main_layers);
		assert!(features.shadow_layers);
		assert!(features.matcap);
		assert!(features.reflection);
		assert!(features.anisotropy);
		assert!(features.rim);
		assert!(features.emission);
		assert!(features.backlight);
		assert!(features.glitter);
		assert!(features.normal_second);
		assert!(features.parallax);
	}

	#[test]
	fn untoon_shader_features_enable_id_mask_from_runtime_flags() {
		let mut liltoon = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon.id_mask.flags_factor[0] = 1.0;
		let mat = UnaMaterialPbr {
			shading: UnaShadingModel::LilToonLike,
			liltoon_like: Some(liltoon),
			..Default::default()
		};

		let features = material_untoon_shader_features(&mat, UnaShadingModel::LilToonLike, &SceneMeshLoadOpts::default());
		assert!(features.id_mask);
	}

	#[test]
	fn untoon_shader_features_enable_udim_discard_from_runtime_rows() {
		let mut liltoon = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon.udim_discard.row0_factor[1] = 1.0;
		let mat = UnaMaterialPbr {
			shading: UnaShadingModel::LilToonLike,
			liltoon_like: Some(liltoon),
			..Default::default()
		};

		let features = material_untoon_shader_features(&mat, UnaShadingModel::LilToonLike, &SceneMeshLoadOpts::default());
		assert!(features.udim_discard);
	}

	#[test]
	fn initial_active_textures_follow_visible_resident_primitives() {
		let identity = Mat4::IDENTITY.to_cols_array();
		let primitive = |material_index| UnaMeshBuffers {
			name: None,
			vertex_payload_id: None,
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
			material_index: Some(material_index),
			morph_targets: Vec::new(),
			morph_target_names: Vec::new(),
			default_morph_weights: Vec::new(),
		};
		let image = || UnaImageRgba {
			width: 1,
			height: 1,
			pixel_format: un_avatar_core::UnaImagePixelFormat::R8G8B8A8,
			pixels: vec![255, 255, 255, 255],
		};
		let scene = UnaSceneSnapshot {
			meshes: vec![vec![primitive(0)], vec![primitive(1)], vec![primitive(2)]],
			materials: vec![
				UnaMaterialPbr {
					base_color_texture_index: Some(0),
					..Default::default()
				},
				UnaMaterialPbr {
					base_color_texture_index: Some(1),
					..Default::default()
				},
				UnaMaterialPbr {
					base_color_texture_index: Some(2),
					..Default::default()
				},
			],
			images: vec![image(), image(), image()],
			nodes: vec![
				UnaSceneNode {
					name: None,
					source_node_id: None,
					resolved_node_id: None,
					visible: true,
					transform: identity,
					children: Vec::new(),
					mesh: Some(0),
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
				UnaSceneNode {
					name: None,
					source_node_id: None,
					resolved_node_id: None,
					visible: false,
					transform: identity,
					children: Vec::new(),
					mesh: Some(1),
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
				UnaSceneNode {
					name: None,
					source_node_id: None,
					resolved_node_id: None,
					visible: true,
					transform: identity,
					children: Vec::new(),
					mesh: Some(2),
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
			],
			asset_group_ownership: vec![un_avatar_core::UnaSceneAssetGroupOwnership {
				group_id: "outfit:hidden".to_string(),
				mesh_primitives: vec![un_avatar_core::UnaMeshPrimitiveKey {
					mesh_index: 2,
					primitive_index: 0,
				}],
				materials: vec![2],
				images: vec![2],
				..Default::default()
			}],
			..Default::default()
		};
		let active_asset_groups = vec!["outfit:base".to_string()];
		let residency = SceneAssetResidencySets::for_scene(&scene, &active_asset_groups);
		let effective_visibility = scene_effective_visibility(&scene);

		assert_eq!(
			initial_active_texture_indices_for_scene(&scene, &effective_visibility, &residency, &SceneMeshLoadOpts::default()),
			vec![0]
		);
	}

	#[test]
	fn hot_switch_texture_residency_tracks_active_draw_texture_slots() {
		let image = || UnaImageRgba {
			width: 1,
			height: 1,
			pixel_format: un_avatar_core::UnaImagePixelFormat::R8G8B8A8,
			pixels: vec![255, 255, 255, 255],
		};
		let mut cube_source = empty_source_metadata();
		cube_source.texture_shape = Some("cube".to_string());
		let scene = UnaSceneSnapshot {
			images: vec![image(), image(), image()],
			image_sources: vec![None, None, Some(cube_source)],
			asset_group_ownership: vec![un_avatar_core::UnaSceneAssetGroupOwnership {
				group_id: "outfit:coat".to_string(),
				images: vec![0, 1, 2],
				..Default::default()
			}],
			..Default::default()
		};
		let asset_residency = SceneAssetResidencySets::for_scene(&scene, &["outfit:coat".to_string()]);
		let active_image_texture_indices = [0usize];
		let active_cube_texture_indices = [2usize];

		let (image_residency, cube_residency) = texture_residency_for_active_draws(
			&scene,
			&asset_residency,
			std::iter::once((active_image_texture_indices.as_slice(), active_cube_texture_indices.as_slice())),
		);

		assert_eq!(image_residency, vec![true, false, false]);
		assert_eq!(cube_residency, vec![false, false, true]);
		assert_eq!(residency_load_indices(&[false, false, false], &image_residency), vec![0]);
	}

	#[test]
	fn liltoon_reflection_cube_sampler_matches_linear_repeat() {
		let desc = liltoon_reflection_cube_sampler_descriptor("test");

		assert_eq!(desc.address_mode_u, wgpu::AddressMode::Repeat);
		assert_eq!(desc.address_mode_v, wgpu::AddressMode::Repeat);
		assert_eq!(desc.address_mode_w, wgpu::AddressMode::Repeat);
		assert_eq!(desc.mag_filter, wgpu::FilterMode::Linear);
		assert_eq!(desc.min_filter, wgpu::FilterMode::Linear);
		assert_eq!(desc.mipmap_filter, wgpu::MipmapFilterMode::Linear);
	}

	#[test]
	fn full_mesh_shader_validates() {
		let module = naga::front::wgsl::parse_str(SHADER_MESH).expect("full mesh shader parses");
		let mut validator = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all());
		validator.validate(&module).expect("full mesh shader validates");
	}

	#[test]
	fn compute_fur_cards_shader_validates() {
		let module = naga::front::wgsl::parse_str(SHADER_COMPUTE_FUR_CARDS).expect("Compute Fur Cards shader parses");
		let mut validator = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all());
		validator.validate(&module).expect("Compute Fur Cards shader validates");
	}

	#[test]
	fn compute_fur_cards_compute_pipeline_interfaces_match() {
		let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
		let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
			power_preference: wgpu::PowerPreference::LowPower,
			compatible_surface: None,
			force_fallback_adapter: false,
		})) else {
			eprintln!("skipping Compute Fur Cards pipeline interface test: no wgpu adapter");
			return;
		};

		let limits = wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits());
		let Ok((device, _queue)) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
			label: Some("compute_fur_cards-compute-pipeline-interface-test"),
			required_features: wgpu::Features::empty(),
			required_limits: limits,
			memory_hints: Default::default(),
			..Default::default()
		})) else {
			eprintln!("skipping Compute Fur Cards pipeline interface test: request_device failed");
			return;
		};

		let bind_group_layout = create_compute_fur_cards_bind_group_layout(&device);
		let _pipeline = create_compute_fur_cards_compute_pipeline(&device, &bind_group_layout, None);
	}

	#[test]
	fn baseline_fallback_mesh_shader_strips_high_capability_bindings_and_validates() {
		let source = baseline_fallback_mesh_shader_source();
		for binding in [
			"@group(1) @binding(24)",
			"@group(1) @binding(25)",
			"@group(1) @binding(26)",
			"@group(1) @binding(27)",
			"@group(1) @binding(38)",
			"@group(1) @binding(39)",
			"@group(1) @binding(41)",
			"@group(1) @binding(42)",
			"@group(1) @binding(43)",
			"@group(1) @binding(44)",
			"@group(1) @binding(45)",
			"@group(1) @binding(46)",
			"@group(1) @binding(47)",
			"@group(1) @binding(48)",
			"@group(1) @binding(49)",
			"@group(1) @binding(50)",
			"@group(1) @binding(51)",
			"@group(1) @binding(52)",
			"@group(1) @binding(53)",
			"@group(1) @binding(54)",
			"@group(1) @binding(55)",
			"@group(1) @binding(56)",
			"@group(1) @binding(57)",
			"@group(1) @binding(58)",
			"@group(1) @binding(59)",
			"@group(1) @binding(60)",
			"@group(1) @binding(61)",
			"@group(1) @binding(62)",
			"@group(1) @binding(63)",
			"@group(1) @binding(64)",
			"@group(1) @binding(65)",
			"@group(1) @binding(66)",
			"@group(1) @binding(67)",
			"@group(1) @binding(68)",
			"@group(1) @binding(69)",
			"@group(1) @binding(70)",
			"@group(1) @binding(71)",
			"@group(1) @binding(72)",
			"@group(1) @binding(73)",
			"@group(1) @binding(74)",
			"@group(1) @binding(75)",
		] {
			assert!(!source.contains(binding), "baseline fallback shader still contains {binding}");
		}
		let module = naga::front::wgsl::parse_str(&source).expect("baseline fallback mesh shader parses");
		let mut validator = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all());
		validator.validate(&module).expect("baseline fallback mesh shader validates");
	}

	#[test]
	fn mesh_toon_pipeline_interfaces_match() {
		let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
		let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
			power_preference: wgpu::PowerPreference::LowPower,
			compatible_surface: None,
			force_fallback_adapter: false,
		})) else {
			eprintln!("skipping mesh pipeline interface test: no wgpu adapter");
			return;
		};

		let adapter_limits = adapter.limits();
		let shader_plan = crate::gpu::mesh_shader_resource_plan_for_adapter(&adapter_limits);
		let shader_variant_tier = shader_plan.tier;
		let limits = shader_plan.required_limits;

		let Ok((device, _queue)) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
			label: Some("mesh-pipeline-interface-test"),
			required_features: wgpu::Features::empty(),
			required_limits: limits,
			memory_hints: Default::default(),
			..Default::default()
		})) else {
			eprintln!("skipping mesh pipeline interface test: request_device failed");
			return;
		};

		let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("mesh_frame_test"),
			entries: &[
				uniform_bind_group_layout_entry(0, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
				texture_bind_group_layout_entry(1, wgpu::ShaderStages::FRAGMENT),
				sampler_bind_group_layout_entry(2, wgpu::ShaderStages::FRAGMENT),
				texture_bind_group_layout_entry(3, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
			],
		});
		let material_entries = mesh_material_layout_entries(shader_variant_tier);
		let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("mesh_material_test"),
			entries: &material_entries,
		});
		let outline_material_entries = mesh_outline_material_layout_entries();
		let outline_material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("mesh_outline_material_test"),
			entries: &outline_material_entries,
		});
		let bone_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("mesh_bones_test"),
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
		let morph_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("mesh_morph_test"),
			entries: &[
				uniform_bind_group_layout_entry(0, wgpu::ShaderStages::VERTEX),
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
			label: Some("mesh_pipeline_test"),
			bind_group_layouts: &[Some(&frame_layout), Some(&material_layout), Some(&bone_layout), Some(&morph_layout)],
			immediate_size: 0,
		});
		let outline_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("mesh_outline_pipeline_test"),
			bind_group_layouts: &[
				Some(&frame_layout),
				Some(&outline_material_layout),
				Some(&bone_layout),
				Some(&morph_layout),
			],
			immediate_size: 0,
		});
		let attrs = [
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
				format: wgpu::VertexFormat::Float32x4,
			},
			wgpu::VertexAttribute {
				offset: 40,
				shader_location: 3,
				format: wgpu::VertexFormat::Float32x2,
			},
			wgpu::VertexAttribute {
				offset: 72,
				shader_location: 4,
				format: wgpu::VertexFormat::Uint16x4,
			},
			wgpu::VertexAttribute {
				offset: 80,
				shader_location: 5,
				format: wgpu::VertexFormat::Float32x4,
			},
			wgpu::VertexAttribute {
				offset: 96,
				shader_location: 6,
				format: wgpu::VertexFormat::Float32x4,
			},
			wgpu::VertexAttribute {
				offset: 48,
				shader_location: 7,
				format: wgpu::VertexFormat::Float32x2,
			},
			wgpu::VertexAttribute {
				offset: 56,
				shader_location: 8,
				format: wgpu::VertexFormat::Float32x2,
			},
			wgpu::VertexAttribute {
				offset: 64,
				shader_location: 9,
				format: wgpu::VertexFormat::Float32x2,
			},
		];
		let vb_layout = wgpu::VertexBufferLayout {
			array_stride: std::mem::size_of::<Vertex>() as u64,
			step_mode: wgpu::VertexStepMode::Vertex,
			attributes: &attrs,
		};
		let compute_fur_cards_attrs = [
			wgpu::VertexAttribute {
				offset: 0,
				shader_location: 0,
				format: wgpu::VertexFormat::Float32x4,
			},
			wgpu::VertexAttribute {
				offset: 16,
				shader_location: 1,
				format: wgpu::VertexFormat::Float32x4,
			},
			wgpu::VertexAttribute {
				offset: 32,
				shader_location: 2,
				format: wgpu::VertexFormat::Float32x2,
			},
			wgpu::VertexAttribute {
				offset: 40,
				shader_location: 3,
				format: wgpu::VertexFormat::Float32,
			},
			wgpu::VertexAttribute {
				offset: 48,
				shader_location: 4,
				format: wgpu::VertexFormat::Float32x4,
			},
			wgpu::VertexAttribute {
				offset: 64,
				shader_location: 5,
				format: wgpu::VertexFormat::Float32x4,
			},
		];
		let compute_fur_cards_vb_layout = wgpu::VertexBufferLayout {
			array_stride: std::mem::size_of::<ComputeFurCardsGeneratedVertexGpu>() as u64,
			step_mode: wgpu::VertexStepMode::Vertex,
			attributes: &compute_fur_cards_attrs,
		};

		let outline_features = UntoonShaderFeatures::default();
		let outline_shader =
			create_mesh_shader_module_for_features(&device, shader_variant_tier, outline_features, "mesh_outline_shader_test");
		let _outline_toon = SceneMeshes::create_mesh_pipeline(
			&device,
			&outline_pipeline_layout,
			&outline_shader,
			wgpu::TextureFormat::Rgba8Unorm,
			&vb_layout,
			None,
			"mesh_outline_toon",
			"vs_outline",
			"fs_outline",
			MeshPipelineRenderState::outline(1),
		);
		let mut toon_features = UntoonShaderFeatures::default();
		toon_features.profile_extensions = true;
		toon_features.shadow_layers = true;
		toon_features.fur = true;
		let toon_shader = create_mesh_shader_module_for_features(&device, shader_variant_tier, toon_features, "mesh_toon_shader_test");
		let _opaque_toon = SceneMeshes::create_mesh_pipeline(
			&device,
			&pipeline_layout,
			&toon_shader,
			wgpu::TextureFormat::Rgba8Unorm,
			&vb_layout,
			None,
			"mesh_opaque_toon",
			"vs_main",
			"fs_toon",
			MeshPipelineRenderState::mesh_main(None, true, 1).with_alpha_coverage(MeshPipelineAlphaCoverage::On),
		);
		let _compute_fur_cards_pre_toon = SceneMeshes::create_mesh_pipeline(
			&device,
			&pipeline_layout,
			&toon_shader,
			wgpu::TextureFormat::Rgba8Unorm,
			&compute_fur_cards_vb_layout,
			None,
			"mesh_compute_fur_cards_pre_toon",
			"vs_compute_fur_cards_pre",
			"fs_fur_toon_pre",
			MeshPipelineRenderState::mesh_main(None, true, 1).with_alpha_coverage(MeshPipelineAlphaCoverage::On),
		);
		let _compute_fur_cards_toon = SceneMeshes::create_mesh_pipeline(
			&device,
			&pipeline_layout,
			&toon_shader,
			wgpu::TextureFormat::Rgba8Unorm,
			&compute_fur_cards_vb_layout,
			None,
			"mesh_compute_fur_cards_toon",
			"vs_compute_fur_cards",
			"fs_fur_toon",
			MeshPipelineRenderState::mesh_main(Some(wgpu::BlendState::ALPHA_BLENDING), false, 1),
		);
	}

	#[test]
	fn linear_source_metadata_uses_linear_rgba_upload_format() {
		let source = UnaImageSourceMetadata {
			color_space: Some("linear".to_string()),
			srgb: Some(false),
			byte_length: 1,
			source_hash: 1,
			..empty_source_metadata()
		};
		assert!(rgba_upload_uses_linear_format(TextureRole::GenericColor, Some(&source)));
		assert!(rgba_upload_uses_linear_format(TextureRole::Data, None));
		assert!(!rgba_upload_uses_linear_format(TextureRole::GenericColor, None));
	}

	#[test]
	fn data_role_respects_source_srgb_upload_format() {
		let source = UnaImageSourceMetadata {
			color_space: Some("srgb".to_string()),
			srgb: Some(true),
			byte_length: 1,
			source_hash: 1,
			..empty_source_metadata()
		};
		assert!(!rgba_upload_uses_linear_format(TextureRole::Data, Some(&source)));
		assert!(rgba_upload_uses_linear_format(TextureRole::Normal, Some(&source)));
	}

	#[test]
	fn cubemap_source_color_space_overrides_importer_srgb_flag() {
		let linear_source = UnaImageSourceMetadata {
			color_space: Some("linear".to_string()),
			srgb: Some(true),
			..empty_source_metadata()
		};
		let srgb_source = UnaImageSourceMetadata {
			color_space: Some("srgb".to_string()),
			srgb: Some(false),
			..empty_source_metadata()
		};
		let legacy_srgb_source = UnaImageSourceMetadata {
			srgb: Some(true),
			..empty_source_metadata()
		};
		assert!(!texture_source_is_srgb(Some(&linear_source)));
		assert!(texture_source_is_srgb(Some(&srgb_source)));
		assert!(texture_source_is_srgb(Some(&legacy_srgb_source)));
	}

	#[test]
	fn cubemap_source_layout_detects_common_unity_layouts() {
		let cube_source = UnaImageSourceMetadata {
			texture_shape: Some("TextureCube".to_string()),
			..empty_source_metadata()
		};
		let mut image = UnaImageRgba {
			width: 1024,
			height: 512,
			pixel_format: un_avatar_core::UnaImagePixelFormat::R8G8B8A8,
			pixels: vec![0; 1024 * 512 * 4],
		};
		assert_eq!(
			cube_source_layout(&image, Some(&cube_source)),
			Some((CubeSourceLayout::Latlong, 256))
		);
		image.width = 1536;
		image.height = 256;
		assert_eq!(
			cube_source_layout(&image, Some(&cube_source)),
			Some((CubeSourceLayout::HorizontalStrip, 256))
		);
		image.width = 256;
		image.height = 1536;
		assert_eq!(
			cube_source_layout(&image, Some(&cube_source)),
			Some((CubeSourceLayout::VerticalStrip, 256))
		);
		image.width = 1024;
		image.height = 768;
		assert_eq!(
			cube_source_layout(&image, Some(&cube_source)),
			Some((CubeSourceLayout::HorizontalCross, 256))
		);
		image.width = 768;
		image.height = 1024;
		assert_eq!(
			cube_source_layout(&image, Some(&cube_source)),
			Some((CubeSourceLayout::VerticalCross, 256))
		);
	}

	#[test]
	fn cubemap_source_layout_uses_metadata_dimensions_for_deferred_placeholder() {
		let cube_source = UnaImageSourceMetadata {
			texture_shape: Some("TextureCube".to_string()),
			width: Some(1024),
			height: Some(512),
			..empty_source_metadata()
		};
		let image = UnaImageRgba {
			width: 0,
			height: 0,
			pixel_format: un_avatar_core::UnaImagePixelFormat::R8G8B8A8,
			pixels: Vec::new(),
		};

		assert_eq!(
			cube_source_layout(&image, Some(&cube_source)),
			Some((CubeSourceLayout::Latlong, 256))
		);
	}

	#[test]
	fn packed_cubemap_sampler_reads_cross_face_cells() {
		let face_size = 4usize;
		let width = face_size * 4;
		let height = face_size * 3;
		let mut pixels = vec![0u8; width * height * 4];
		let mut put_cell = |cell_x: usize, cell_y: usize, rgba: [u8; 4]| {
			for y in 0..face_size {
				for x in 0..face_size {
					let offset = ((cell_y * face_size + y) * width + cell_x * face_size + x) * 4;
					pixels[offset..offset + 4].copy_from_slice(&rgba);
				}
			}
		};
		put_cell(2, 1, [255, 0, 0, 255]);
		put_cell(0, 1, [0, 255, 0, 255]);
		put_cell(1, 0, [0, 0, 255, 255]);
		put_cell(1, 2, [255, 255, 0, 255]);
		put_cell(1, 1, [0, 255, 255, 255]);
		put_cell(3, 1, [255, 0, 255, 255]);
		let image = UnaImageRgba {
			width: width as u32,
			height: height as u32,
			pixel_format: un_avatar_core::UnaImagePixelFormat::R8G8B8A8,
			pixels,
		};
		assert_eq!(
			sample_packed_cube_face(&image, CubeSourceLayout::HorizontalCross, 0, 0.0, 0.0, false),
			[1.0, 0.0, 0.0, 1.0]
		);
		assert_eq!(
			sample_packed_cube_face(&image, CubeSourceLayout::HorizontalCross, 5, 0.0, 0.0, false),
			[1.0, 0.0, 1.0, 1.0]
		);
	}

	#[test]
	fn sphere_map_back_direction_uses_edge_average_instead_of_black() {
		let width = 4usize;
		let height = 4usize;
		let mut pixels = vec![0u8; width * height * 4];
		for pixel in pixels.chunks_exact_mut(4) {
			pixel[3] = 255;
		}
		let mut put_pixel = |x: usize, y: usize, rgba: [u8; 4]| {
			let offset = (y * width + x) * 4;
			pixels[offset..offset + 4].copy_from_slice(&rgba);
		};
		put_pixel(width - 1, height / 2, [255, 0, 0, 255]);
		put_pixel(0, height / 2, [0, 255, 0, 255]);
		put_pixel(width / 2, 0, [0, 0, 255, 255]);
		put_pixel(width / 2, height - 1, [255, 255, 0, 255]);
		let image = UnaImageRgba {
			width: width as u32,
			height: height as u32,
			pixel_format: un_avatar_core::UnaImagePixelFormat::R8G8B8A8,
			pixels,
		};

		let sampled = sample_sphere_map(&image, Vec3::new(0.0, 0.0, -1.0), false);

		assert!(sampled[0] > 0.0);
		assert!(sampled[1] > 0.0);
		assert!(sampled[2] > 0.0);
		assert_eq!(sampled[3], 1.0);
	}

	#[test]
	fn cubemap_upload_builds_rgba16f_mip_chain() {
		let source = vec![[1.0, 0.5, 0.25, 1.0]; 6 * 2 * 2];
		let mips = build_cube_mips_rgba16f(2, source);
		assert_eq!(mips.len(), 2);
		assert_eq!(mips[0].face_size, 2);
		assert_eq!(mips[0].data_rgba16f.len(), 6 * 2 * 2 * 8);
		assert_eq!(mips[1].face_size, 1);
		assert_eq!(mips[1].data_rgba16f.len(), 6 * 8);
	}

	#[test]
	fn cubemap_cache_roundtrips_upload_mips() {
		let key = 0x1234_5678_9abc_def0;
		let path = std::env::temp_dir().join(format!("un-avatar-cubemap-cache-test-{}-{key:016x}.ucube", std::process::id()));
		let upload = CubeUpload {
			face_size: 2,
			mips: build_cube_mips_rgba16f(2, vec![[1.0, 0.5, 0.25, 1.0]; 6 * 2 * 2]),
			layout: CubeSourceLayout::HorizontalStrip.name(),
		};

		assert!(write_cube_texture_cache(&path, key, CubeSourceLayout::HorizontalStrip, &upload));
		let loaded = read_cube_texture_cache(&path, key).expect("cubemap cache should load");
		let _ = std::fs::remove_file(&path);

		assert_eq!(loaded.face_size, upload.face_size);
		assert_eq!(loaded.layout, upload.layout);
		assert_eq!(loaded.mips.len(), upload.mips.len());
		assert_eq!(loaded.mips[0].data_rgba16f, upload.mips[0].data_rgba16f);
	}

	#[test]
	fn cubemap_mips_use_plain_downsample_without_extra_roughness_blur() {
		let mut source = vec![[0.0, 0.0, 0.0, 1.0]; 6 * 4 * 4];
		source[1] = [1.0, 1.0, 1.0, 1.0];
		let mips = build_cube_mips_rgba16f(4, source);
		let mip1 = &mips[1].data_rgba16f;
		let read = |pixel: usize, channel: usize| {
			let offset = (pixel * 4 + channel) * 2;
			f16::from_bits(u16::from_le_bytes([mip1[offset], mip1[offset + 1]])).to_f32()
		};

		assert!((read(0, 0) - 0.25).abs() < 0.001);
		assert_eq!(read(1, 0), 0.0);
		assert_eq!(read(0, 3), 1.0);
	}

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
	fn scene_default_morph_weights_for_draw_clamps_and_pads_scene_values() {
		let scene = UnaSceneSnapshot {
			meshes: vec![vec![UnaMeshBuffers {
				name: None,
				vertex_payload_id: None,
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
				morph_targets: vec![
					UnaMorphTargetDeltas {
						position_deltas: vec![[0.0; 3]],
						normal_deltas: None,
					},
					UnaMorphTargetDeltas {
						position_deltas: vec![[0.0; 3]],
						normal_deltas: None,
					},
				],
				morph_target_names: vec!["A".to_string(), "B".to_string()],
				default_morph_weights: vec![1.2],
			}]],
			..Default::default()
		};
		assert_eq!(scene_default_morph_weights_for_draw(&scene, 0, 0, &[0, 1]), vec![1.0, 0.0]);
		assert_eq!(scene_default_morph_weights_for_draw(&scene, 9, 0, &[0, 1]), vec![0.0, 0.0]);
	}

	#[test]
	fn refresh_morph_default_weights_invalidates_uploaded_weights_only_on_change() {
		let scene = UnaSceneSnapshot {
			meshes: vec![vec![UnaMeshBuffers {
				name: None,
				vertex_payload_id: None,
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
				morph_target_names: vec!["A".to_string()],
				default_morph_weights: vec![0.75],
			}]],
			..Default::default()
		};
		let mut defaults = vec![0.25];
		let mut uploaded = vec![0.25];
		assert!(refresh_morph_default_weights(&mut defaults, &mut uploaded, &scene, 0, 0, &[0]));
		assert_eq!(defaults, vec![0.75]);
		assert!(uploaded.is_empty());

		uploaded.push(0.75);
		assert!(!refresh_morph_default_weights(&mut defaults, &mut uploaded, &scene, 0, 0, &[0]));
		assert_eq!(uploaded, vec![0.75]);
	}

	#[test]
	fn fill_morph_weights_for_draw_merges_scene_defaults_and_expression_bindings() {
		let bindings = [ExpressionBinding {
			preset_index: 0,
			morph_target_index: 1,
			weight_scale: 0.5,
		}];
		let mut out = Vec::new();
		fill_morph_weights_for_draw(&[0.2, 0.25], 2, &bindings, Some(&[0.5]), &[], &[], &[], None, &mut out);
		assert_eq!(out, vec![0.2, 0.5]);
	}

	#[test]
	fn fill_morph_weights_for_draw_zero_fills_missing_defaults() {
		let mut out = Vec::new();
		fill_morph_weights_for_draw(&[0.2], 3, &[], None, &[], &[], &[], None, &mut out);
		assert_eq!(out, vec![0.2, 0.0, 0.0]);
	}

	#[test]
	fn fill_morph_weights_for_draw_applies_morph_name_overrides() {
		let mut out = Vec::new();
		let overrides = BTreeMap::from([("(Do not Modify)ArmPit_Fix_L".to_string(), 0.75)]);
		fill_morph_weights_for_draw(
			&[0.0, 0.2],
			2,
			&[],
			None,
			&["(Do not Modify)ArmPit_Fix_L".to_string(), "Other".to_string()],
			&[],
			&[],
			Some(&overrides),
			&mut out,
		);
		assert_eq!(out, vec![0.75, 0.2]);
	}

	#[test]
	fn fill_morph_weights_for_draw_prefers_target_specific_morph_override_key() {
		let mut out = Vec::new();
		let overrides = BTreeMap::from([
			("(Do not Modify)ArmPit_Fix_L".to_string(), 0.25),
			(
				morph_override_key("AvatarRoot/Cloth_Panel_Mesh", "(Do not Modify)ArmPit_Fix_L"),
				0.8,
			),
		]);
		fill_morph_weights_for_draw(
			&[0.0],
			1,
			&[],
			None,
			&["(Do not Modify)ArmPit_Fix_L".to_string()],
			&[morph_override_key("AvatarRoot/Cloth_Panel_Mesh", "(Do not Modify)ArmPit_Fix_L")],
			&[],
			Some(&overrides),
			&mut out,
		);
		assert_eq!(out, vec![0.8]);
	}

	#[test]
	fn fill_morph_weights_for_draw_matches_avatar_root_relative_override_key() {
		let mut out = Vec::new();
		let overrides = BTreeMap::from([(
			morph_override_key("AvatarRoot/Cloth_Panel_Mesh", "(Do not Modify)ArmPit_Fix_L"),
			0.9,
		)]);
		let draw_key = morph_override_key(
			"GenericAvatar (UNAvatar Export)/AvatarRoot/Cloth_Panel_Mesh",
			"(Do not Modify)ArmPit_Fix_L",
		);
		let draw_suffix_key = morph_override_path_suffix_key(&draw_key);
		fill_morph_weights_for_draw(
			&[0.0],
			1,
			&[],
			None,
			&["(Do not Modify)ArmPit_Fix_L".to_string()],
			&[draw_key],
			&[draw_suffix_key],
			Some(&overrides),
			&mut out,
		);
		assert_eq!(out, vec![0.9]);
	}

	#[test]
	fn dynamic_morph_targets_use_normalized_binds_and_nonzero_defaults() {
		let buf = UnaMeshBuffers {
			name: None,
			vertex_payload_id: None,
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
			morph_targets: vec![
				UnaMorphTargetDeltas {
					position_deltas: vec![[0.0; 3]],
					normal_deltas: None,
				},
				UnaMorphTargetDeltas {
					position_deltas: vec![[0.0; 3]],
					normal_deltas: None,
				},
				UnaMorphTargetDeltas {
					position_deltas: vec![[0.0; 3]],
					normal_deltas: None,
				},
			],
			morph_target_names: vec![
				"eyeBlinkLeft".to_string(),
				"Chest1_____胸_首元".to_string(),
				"WardrobeOnlyZero".to_string(),
			],
			default_morph_weights: vec![0.0, 1.0, 0.0],
		};
		let bindings = [
			ExpressionBinding {
				preset_index: 0,
				morph_target_index: 0,
				weight_scale: 1.0,
			},
			ExpressionBinding {
				preset_index: 1,
				morph_target_index: 2,
				weight_scale: 1.0,
			},
		];

		let indices = dynamic_morph_target_indices(&buf, &bindings, &[], false);

		assert_eq!(indices, vec![0, 1, 2]);
	}

	#[test]
	fn dynamic_morph_targets_include_animator_morph_names() {
		let buf = UnaMeshBuffers {
			name: None,
			vertex_payload_id: None,
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
			morph_targets: vec![
				UnaMorphTargetDeltas {
					position_deltas: vec![[0.0; 3]],
					normal_deltas: None,
				},
				UnaMorphTargetDeltas {
					position_deltas: vec![[0.0; 3]],
					normal_deltas: None,
				},
			],
			morph_target_names: vec!["Unused".to_string(), "(Do not Modify)ArmPit_Fix_L".to_string()],
			default_morph_weights: vec![0.0, 0.0],
		};
		let names = vec!["(Do not Modify)ArmPit_Fix_L".to_string()];

		let indices = dynamic_morph_target_indices(&buf, &[], &names, false);

		assert_eq!(indices, vec![1]);
	}

	#[test]
	fn dynamic_morph_targets_return_empty_without_morph_payload() {
		let buf = UnaMeshBuffers {
			name: None,
			vertex_payload_id: None,
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
			morph_targets: Vec::new(),
			morph_target_names: vec!["Ignored".to_string()],
			default_morph_weights: vec![1.0],
		};
		let bindings = [ExpressionBinding {
			preset_index: 0,
			morph_target_index: 0,
			weight_scale: 1.0,
		}];
		let names = vec!["Ignored".to_string()];

		let indices = dynamic_morph_target_indices(&buf, &bindings, &names, true);

		assert!(indices.is_empty());
	}

	#[test]
	fn remap_expression_bindings_uses_compact_morph_indices() {
		let bindings = [
			ExpressionBinding {
				preset_index: 0,
				morph_target_index: 4,
				weight_scale: 0.25,
			},
			ExpressionBinding {
				preset_index: 1,
				morph_target_index: 99,
				weight_scale: 1.0,
			},
			ExpressionBinding {
				preset_index: 2,
				morph_target_index: 42,
				weight_scale: 0.5,
			},
		];

		let remapped = remap_expression_bindings(&bindings, &[4, 42]);

		assert_eq!(remapped.len(), 2);
		assert_eq!(remapped[0].preset_index, 0);
		assert_eq!(remapped[0].morph_target_index, 0);
		assert_eq!(remapped[0].weight_scale, 0.25);
		assert_eq!(remapped[1].preset_index, 2);
		assert_eq!(remapped[1].morph_target_index, 1);
		assert_eq!(remapped[1].weight_scale, 0.5);
	}

	fn test_vertex(joints: [u16; 4], weights: [f32; 4]) -> Vertex {
		Vertex {
			pos: [0.0; 3],
			norm: [0.0; 3],
			tangent: [0.0; 4],
			uv: [0.0; 2],
			uv1: [0.0; 2],
			uv2: [0.0; 2],
			uv3: [0.0; 2],
			joints,
			weights,
			color: [0.0; 4],
		}
	}

	#[test]
	fn normalize_skinning_vertices_resets_joint_attributes_without_skin() {
		let mut verts = vec![test_vertex([3, 2, 1, 0], [0.1, 0.2, 0.3, 0.4])];
		normalize_skinning_vertices(&mut verts, true, None);
		assert_eq!(verts[0].joints, [0, 0, 0, 0]);
		assert_eq!(verts[0].weights, [1.0, 0.0, 0.0, 0.0]);
	}

	#[test]
	fn normalize_skinning_vertices_clamps_to_valid_palette_range() {
		let skin = un_avatar_core::UnaSkin {
			joint_nodes: vec![0, 1],
			inverse_bind_matrices: vec![Mat4::IDENTITY.to_cols_array(), Mat4::IDENTITY.to_cols_array()],
			skeleton_node: None,
		};
		let mut verts = vec![test_vertex([0, 1, 2, 9], [0.25; 4])];
		normalize_skinning_vertices(&mut verts, true, Some(&skin));
		assert_eq!(verts[0].joints, [0, 1, 1, 1]);
		assert_eq!(skin_palette_matrix_capacity(Some(&skin)), 2);
	}

	#[test]
	fn skin_palette_capacity_uses_inverse_bind_count_as_bound() {
		let skin = un_avatar_core::UnaSkin {
			joint_nodes: vec![0, 1, 2],
			inverse_bind_matrices: vec![Mat4::IDENTITY.to_cols_array()],
			skeleton_node: None,
		};
		assert_eq!(skin_palette_matrix_capacity(Some(&skin)), 1);
	}

	#[test]
	fn skinless_palette_key_is_shared_identity() {
		assert_eq!(skin_palette_key_for_node(1, None), skin_palette_key_for_node(42, None));
		assert_ne!(skin_palette_key_for_node(1, Some(0)), skin_palette_key_for_node(42, Some(0)));
	}

	#[test]
	fn mesh_cloth_assist_empty_filter_matches_profile_cloth_category() {
		let filters = Vec::new();
		let categories = un_avatar_skeleton::DynamicsPhysicsConfig::default().normalized().categories;
		assert!(mesh_cloth_assist_mesh_matches_with_categories(
			"Avatar/LongCoat",
			&filters,
			&categories
		));
		assert!(mesh_cloth_assist_mesh_matches_with_categories(
			"Avatar/DressHem",
			&filters,
			&categories
		));
		assert!(mesh_cloth_assist_mesh_matches_with_categories(
			"Avatar/SleeveFrill",
			&filters,
			&categories
		));
		assert!(!mesh_cloth_assist_mesh_matches_with_categories(
			"Avatar/Body",
			&filters,
			&categories
		));
	}

	#[test]
	fn mesh_cloth_assist_runtime_membership_overrides_cloth_alias_fallback() {
		let skin = un_avatar_core::UnaSkin {
			joint_nodes: vec![0, 1, 2],
			inverse_bind_matrices: vec![
				Mat4::IDENTITY.to_cols_array(),
				Mat4::IDENTITY.to_cols_array(),
				Mat4::IDENTITY.to_cols_array(),
			],
			skeleton_node: None,
		};
		let node_paths = vec![
			"Root/Chest".to_string(),
			"Root/Cloth_Alias_Not_Runtime".to_string(),
			"Root/Actual_Dynamic".to_string(),
		];
		let mut verts = vec![
			test_vertex([0, 1, 0, 0], [0.78, 0.22, 0.0, 0.0]),
			test_vertex([0, 1, 0, 0], [0.78, 0.22, 0.0, 0.0]),
			test_vertex([0, 2, 0, 0], [0.58, 0.42, 0.0, 0.0]),
		];
		let config = un_avatar_skeleton::DynamicsMeshClothAssistConfig {
			enabled: true,
			body_dominance_threshold: 0.6,
			min_existing_dynamic_weight: 0.04,
			seed_missing_dynamic_influence: true,
			max_assist_weight: 0.3,
			mesh_path_contains: vec!["cloth".to_string()],
		};

		let changed = apply_mesh_cloth_assist_to_vertices(
			&mut verts,
			&[0, 1, 2],
			Some(&skin),
			&node_paths,
			"Avatar/Cloth_Panel_Mesh",
			&config,
			&[],
			&[2usize],
		);

		assert_eq!(changed, 2);
		assert!(verts[0].joints.contains(&2));
		let static_alias_weight = verts[0]
			.joints
			.iter()
			.zip(verts[0].weights.iter())
			.filter_map(|(&joint, &weight)| (joint == 1).then_some(weight))
			.sum::<f32>();
		assert!((static_alias_weight - 0.22).abs() < 0.0001);
	}

	#[test]
	fn mesh_cloth_assist_transfers_to_existing_dynamic_lane() {
		let skin = un_avatar_core::UnaSkin {
			joint_nodes: vec![0, 1],
			inverse_bind_matrices: vec![Mat4::IDENTITY.to_cols_array(), Mat4::IDENTITY.to_cols_array()],
			skeleton_node: None,
		};
		let node_paths = vec!["Root/Chest".to_string(), "Root/Cloth_Dyn_L".to_string()];
		let mut verts = vec![
			test_vertex([0, 1, 0, 0], [0.8, 0.2, 0.0, 0.0]),
			test_vertex([0, 1, 0, 0], [0.5, 0.5, 0.0, 0.0]),
			test_vertex([0, 1, 0, 0], [0.5, 0.5, 0.0, 0.0]),
		];
		let config = un_avatar_skeleton::DynamicsMeshClothAssistConfig {
			enabled: true,
			body_dominance_threshold: 0.55,
			min_existing_dynamic_weight: 0.05,
			seed_missing_dynamic_influence: true,
			max_assist_weight: 0.25,
			mesh_path_contains: vec!["cloth".to_string()],
		};

		let changed = apply_mesh_cloth_assist_to_vertices(
			&mut verts,
			&[0, 1, 2],
			Some(&skin),
			&node_paths,
			"Avatar/Cloth_Panel_Mesh",
			&config,
			&[],
			&[],
		);

		assert_eq!(changed, 1);
		assert!(verts[0].weights[0] < 0.8);
		assert!(verts[0].weights[1] > 0.2);
	}

	#[test]
	fn mesh_cloth_assist_repairs_connected_dynamic_weight_gap() {
		let skin = un_avatar_core::UnaSkin {
			joint_nodes: vec![0, 1],
			inverse_bind_matrices: vec![Mat4::IDENTITY.to_cols_array(), Mat4::IDENTITY.to_cols_array()],
			skeleton_node: None,
		};
		let node_paths = vec!["Root/Chest".to_string(), "Root/Cloth_Dyn_L".to_string()];
		let mut verts = vec![
			test_vertex([0, 1, 0, 0], [0.78, 0.22, 0.0, 0.0]),
			test_vertex([0, 1, 0, 0], [0.56, 0.44, 0.0, 0.0]),
			test_vertex([0, 1, 0, 0], [0.56, 0.44, 0.0, 0.0]),
		];
		let config = un_avatar_skeleton::DynamicsMeshClothAssistConfig {
			enabled: true,
			body_dominance_threshold: 0.6,
			min_existing_dynamic_weight: 0.04,
			seed_missing_dynamic_influence: true,
			max_assist_weight: 0.5,
			mesh_path_contains: vec!["cloth".to_string()],
		};

		let changed = apply_mesh_cloth_assist_to_vertices(
			&mut verts,
			&[0, 1, 2],
			Some(&skin),
			&node_paths,
			"Avatar/Cloth_Panel_Mesh",
			&config,
			&[],
			&[],
		);

		assert_eq!(changed, 1);
		assert!((verts[0].weights[0] - 0.56).abs() < 0.001);
		assert!((verts[0].weights[1] - 0.44).abs() < 0.001);
	}

	#[test]
	fn mesh_cloth_assist_relaxes_connected_dynamic_weight_chain() {
		let skin = un_avatar_core::UnaSkin {
			joint_nodes: vec![0, 1],
			inverse_bind_matrices: vec![Mat4::IDENTITY.to_cols_array(), Mat4::IDENTITY.to_cols_array()],
			skeleton_node: None,
		};
		let node_paths = vec!["Root/Chest".to_string(), "Root/Cloth_Dyn_L".to_string()];
		let mut verts = vec![
			test_vertex([0, 1, 0, 0], [0.9, 0.1, 0.0, 0.0]),
			test_vertex([0, 1, 0, 0], [0.78, 0.22, 0.0, 0.0]),
			test_vertex([0, 1, 0, 0], [0.32, 0.68, 0.0, 0.0]),
			test_vertex([0, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]),
		];
		let config = un_avatar_skeleton::DynamicsMeshClothAssistConfig {
			enabled: true,
			body_dominance_threshold: 0.6,
			min_existing_dynamic_weight: 0.04,
			seed_missing_dynamic_influence: true,
			max_assist_weight: 0.3,
			mesh_path_contains: vec!["cloth".to_string()],
		};

		let changed = apply_mesh_cloth_assist_to_vertices(
			&mut verts,
			&[0, 1, 3, 1, 2, 3],
			Some(&skin),
			&node_paths,
			"Avatar/Cloth_Panel_Mesh",
			&config,
			&[],
			&[],
		);

		assert_eq!(changed, 2);
		assert!(
			verts[0].weights[1] > 0.39,
			"first row should inherit dynamic weight across the connected cloth strip"
		);
		assert!(
			verts[1].weights[1] > 0.51,
			"middle row should first inherit from the stronger dynamic neighbor"
		);
		assert_eq!(
			verts[3].weights,
			[1.0, 0.0, 0.0, 0.0],
			"vertices without an authored dynamic lane are not seeded"
		);
	}

	#[test]
	fn mesh_cloth_assist_does_not_seed_missing_dynamic_lane() {
		let left_bind = Mat4::from_translation(Vec3::new(-1.0, 0.0, 0.0)).inverse().to_cols_array();
		let right_bind = Mat4::from_translation(Vec3::new(1.0, 0.0, 0.0)).inverse().to_cols_array();
		let skin = un_avatar_core::UnaSkin {
			joint_nodes: vec![0, 1, 2],
			inverse_bind_matrices: vec![Mat4::IDENTITY.to_cols_array(), left_bind, right_bind],
			skeleton_node: None,
		};
		let node_paths = vec![
			"Root/Chest".to_string(),
			"Root/Cloth_Dyn_L".to_string(),
			"Root/Cloth_Dyn_R".to_string(),
		];
		let mut verts = vec![
			Vertex {
				pos: [-1.0, 0.0, 0.0],
				..test_vertex([0, 0, 0, 0], [1.0, 0.0, 0.0, 0.0])
			},
			Vertex {
				pos: [1.0, 0.0, 0.0],
				..test_vertex([0, 0, 0, 0], [1.0, 0.0, 0.0, 0.0])
			},
		];
		let config = un_avatar_skeleton::DynamicsMeshClothAssistConfig {
			enabled: true,
			body_dominance_threshold: 0.55,
			min_existing_dynamic_weight: 0.05,
			seed_missing_dynamic_influence: true,
			max_assist_weight: 0.25,
			mesh_path_contains: vec!["cloth".to_string()],
		};

		let changed = apply_mesh_cloth_assist_to_vertices(
			&mut verts,
			&[0, 1, 2],
			Some(&skin),
			&node_paths,
			"Avatar/Cloth_Panel_Mesh",
			&config,
			&[],
			&[],
		);

		assert_eq!(changed, 0);
		assert_eq!(verts[0].joints, [0, 0, 0, 0]);
		assert_eq!(verts[1].joints, [0, 0, 0, 0]);
	}

	#[test]
	fn mesh_cloth_assist_seeds_missing_dynamic_lane_from_connected_static_cloth_anchor() {
		let skin = un_avatar_core::UnaSkin {
			joint_nodes: vec![0, 1, 2],
			inverse_bind_matrices: vec![
				Mat4::IDENTITY.to_cols_array(),
				Mat4::IDENTITY.to_cols_array(),
				Mat4::IDENTITY.to_cols_array(),
			],
			skeleton_node: None,
		};
		let node_paths = vec![
			"Root/Chest".to_string(),
			"Root/Cloth_Static_L".to_string(),
			"Root/Cloth_Dyn_L".to_string(),
		];
		let mut verts = vec![
			test_vertex([0, 1, 0, 0], [0.78, 0.22, 0.0, 0.0]),
			test_vertex([2, 1, 0, 0], [0.62, 0.38, 0.0, 0.0]),
			test_vertex([2, 1, 0, 0], [0.62, 0.38, 0.0, 0.0]),
		];
		let config = un_avatar_skeleton::DynamicsMeshClothAssistConfig {
			enabled: true,
			body_dominance_threshold: 0.6,
			min_existing_dynamic_weight: 0.04,
			seed_missing_dynamic_influence: true,
			max_assist_weight: 0.3,
			mesh_path_contains: vec!["cloth".to_string()],
		};
		let dynamic_nodes = vec![2usize];

		let changed = apply_mesh_cloth_assist_to_vertices(
			&mut verts,
			&[0, 1, 2],
			Some(&skin),
			&node_paths,
			"Avatar/Cloth_Panel_Mesh",
			&config,
			&[],
			&dynamic_nodes,
		);

		assert_eq!(changed, 1);
		assert!(verts[0].joints.contains(&2));
		let seeded_lane = verts[0].joints.iter().position(|&joint| joint == 2).expect("seeded lane");
		assert!(verts[0].weights[seeded_lane] > 0.25);
		assert!(verts[0].weights[0] < 0.78);
	}

	#[test]
	fn mesh_cloth_assist_propagates_tiny_dynamic_bridge_across_static_cloth_strip() {
		let skin = un_avatar_core::UnaSkin {
			joint_nodes: vec![0, 1, 2],
			inverse_bind_matrices: vec![
				Mat4::IDENTITY.to_cols_array(),
				Mat4::IDENTITY.to_cols_array(),
				Mat4::IDENTITY.to_cols_array(),
			],
			skeleton_node: None,
		};
		let node_paths = vec![
			"Root/Chest".to_string(),
			"Root/Cloth_Static_L".to_string(),
			"Root/Cloth_Dyn_L".to_string(),
		];
		let mut verts = vec![
			test_vertex([0, 1, 0, 0], [0.78, 0.22, 0.0, 0.0]),
			test_vertex([0, 1, 2, 0], [0.58, 0.418, 0.002, 0.0]),
			test_vertex([0, 1, 2, 0], [0.30, 0.684, 0.016, 0.0]),
			test_vertex([0, 1, 2, 0], [0.16, 0.768, 0.072, 0.0]),
		];
		let config = un_avatar_skeleton::DynamicsMeshClothAssistConfig {
			enabled: true,
			body_dominance_threshold: 0.55,
			min_existing_dynamic_weight: 0.04,
			seed_missing_dynamic_influence: true,
			max_assist_weight: 0.3,
			mesh_path_contains: vec!["cloth".to_string()],
		};
		let dynamic_nodes = vec![2usize];

		let changed = apply_mesh_cloth_assist_to_vertices(
			&mut verts,
			&[0, 1, 2, 1, 2, 3],
			Some(&skin),
			&node_paths,
			"Avatar/Cloth_Panel_Mesh",
			&config,
			&[],
			&dynamic_nodes,
		);

		assert!(changed >= 2);
		let head_dynamic = verts[0]
			.joints
			.iter()
			.zip(verts[0].weights.iter())
			.filter_map(|(&joint, &weight)| (joint == 2).then_some(weight))
			.sum::<f32>();
		assert!(
			head_dynamic >= 0.04,
			"tiny authored dynamic weights should propagate across the connected static cloth strip, got {head_dynamic}"
		);
	}

	#[test]
	fn mesh_cloth_assist_does_not_seed_when_disabled_by_config() {
		let skin = un_avatar_core::UnaSkin {
			joint_nodes: vec![0, 1],
			inverse_bind_matrices: vec![Mat4::IDENTITY.to_cols_array(), Mat4::IDENTITY.to_cols_array()],
			skeleton_node: None,
		};
		let node_paths = vec!["Root/Chest".to_string(), "Root/Cloth_Dyn_L".to_string()];
		let mut verts = vec![test_vertex([0, 0, 0, 0], [1.0, 0.0, 0.0, 0.0])];
		let config = un_avatar_skeleton::DynamicsMeshClothAssistConfig {
			enabled: true,
			body_dominance_threshold: 0.55,
			min_existing_dynamic_weight: 0.05,
			seed_missing_dynamic_influence: false,
			max_assist_weight: 0.25,
			mesh_path_contains: vec!["cloth".to_string()],
		};

		let changed = apply_mesh_cloth_assist_to_vertices(
			&mut verts,
			&[0, 1, 2],
			Some(&skin),
			&node_paths,
			"Avatar/Cloth_Panel_Mesh",
			&config,
			&[],
			&[],
		);

		assert_eq!(changed, 0);
		assert_eq!(verts[0].joints, [0, 0, 0, 0]);
		assert_eq!(verts[0].weights, [1.0, 0.0, 0.0, 0.0]);
	}

	#[test]
	fn expand_primitive_bakes_static_default_morphs() {
		let buf = UnaMeshBuffers {
			name: None,
			vertex_payload_id: None,
			positions: vec![[1.0, 2.0, 3.0]],
			normals: Some(vec![[0.0, 1.0, 0.0]]),
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
				position_deltas: vec![[2.0, 0.0, -1.0]],
				normal_deltas: Some(vec![[0.0, 1.0, 0.0]]),
			}],
			morph_target_names: Vec::new(),
			default_morph_weights: vec![0.5],
		};

		let static_morph_targets = Vec::new();
		let baked = expand_primitive(&buf, Some(&static_morph_targets)).expect("expanded primitive");
		assert_eq!(baked.verts[0].pos, [2.0, 2.0, 2.5]);
		assert!(baked.default_morph_weights.is_empty());
		assert!(baked.morph_pos.is_empty());

		let dynamic = expand_primitive(&buf, None).expect("expanded primitive");
		assert_eq!(dynamic.verts[0].pos, [1.0, 2.0, 3.0]);
		assert_eq!(dynamic.default_morph_weights, vec![0.5]);
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
					source_node_id: None,
					resolved_node_id: None,
					name: Some("root".to_string()),
					visible: false,
					transform: identity,
					children: vec![1],
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
				UnaSceneNode {
					source_node_id: None,
					resolved_node_id: None,
					name: Some("child".to_string()),
					visible: true,
					transform: identity,
					children: Vec::new(),
					mesh: Some(0),
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
			],
			roots: vec![0],
			..Default::default()
		};

		assert_eq!(scene_effective_visibility(&scene), vec![false, false]);
	}

	#[test]
	fn effective_visibility_falls_back_to_parentless_roots_when_scene_roots_are_missing() {
		let identity = Mat4::IDENTITY.to_cols_array();
		let scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					source_node_id: None,
					resolved_node_id: None,
					name: Some("root".to_string()),
					visible: true,
					transform: identity,
					children: vec![1],
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
				UnaSceneNode {
					source_node_id: None,
					resolved_node_id: None,
					name: Some("child".to_string()),
					visible: true,
					transform: identity,
					children: Vec::new(),
					mesh: Some(0),
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
				UnaSceneNode {
					source_node_id: None,
					resolved_node_id: None,
					name: Some("hidden_parentless".to_string()),
					visible: false,
					transform: identity,
					children: Vec::new(),
					mesh: Some(1),
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
			],
			roots: vec![],
			..Default::default()
		};

		assert_eq!(scene_effective_visibility(&scene), vec![true, true, false]);
	}

	#[test]
	fn ordered_draw_batches_preserve_transparent_sequence() {
		fn key(kind: DrawPipelineKind) -> DrawPipelineKey {
			DrawPipelineKey::from_parts(kind, MaterialStencilState::default(), 15)
		}

		let mut batches = Vec::new();
		append_ordered_draw_batch(&mut batches, key(DrawPipelineKind::BlendToon), 0, 1);
		append_ordered_draw_batch(&mut batches, key(DrawPipelineKind::BlendToonZWrite), 1, 1);
		append_ordered_draw_batch(&mut batches, key(DrawPipelineKind::BlendLit), 2, 1);
		append_ordered_draw_batch(&mut batches, key(DrawPipelineKind::BlendToon), 3, 1);
		append_ordered_draw_batch(&mut batches, key(DrawPipelineKind::BlendToon), 4, 1);

		assert_eq!(batches.len(), 4);
		assert_eq!(batches[0].pipeline, key(DrawPipelineKind::BlendToon));
		assert_eq!(batches[0].draw_indices, vec![0]);
		assert_eq!(batches[1].pipeline, key(DrawPipelineKind::BlendToonZWrite));
		assert_eq!(batches[1].draw_indices, vec![1]);
		assert_eq!(batches[2].pipeline, key(DrawPipelineKind::BlendLit));
		assert_eq!(batches[2].draw_indices, vec![2]);
		assert_eq!(batches[3].pipeline, key(DrawPipelineKind::BlendToon));
		assert_eq!(batches[3].draw_indices, vec![3, 4]);
	}

	#[test]
	fn ordered_draw_batches_keep_gem_prepass_adjacent_to_forward() {
		fn key(kind: DrawPipelineKind) -> DrawPipelineKey {
			DrawPipelineKey::from_parts(kind, MaterialStencilState::default(), 15)
		}

		let mut batches = Vec::new();
		append_ordered_draw_batch(&mut batches, key(DrawPipelineKind::BlendToon), 0, 4);
		append_ordered_draw_batch(&mut batches, key(DrawPipelineKind::LilToonGemPre), 1, 4);
		append_ordered_draw_batch(&mut batches, key(DrawPipelineKind::BlendToonAdd), 1, 4);
		append_ordered_draw_batch(&mut batches, key(DrawPipelineKind::BlendToon), 2, 4);

		assert_eq!(
			batches.iter().map(|batch| batch.pipeline).collect::<Vec<_>>(),
			vec![
				key(DrawPipelineKind::BlendToon),
				key(DrawPipelineKind::LilToonGemPre),
				key(DrawPipelineKind::BlendToonAdd),
				key(DrawPipelineKind::BlendToon)
			]
		);
		assert_eq!(batches[1].draw_indices, vec![1]);
		assert_eq!(batches[2].draw_indices, vec![1]);
	}

	#[test]
	fn transparent_backpass_orders_with_its_source_draw() {
		let mut draws = vec![
			(DrawPipelineKind::LilToonGemPre, 0usize),
			(DrawPipelineKind::BlendToonAdd, 0usize),
			(DrawPipelineKind::TransparentToonBackpass, 1usize),
			(DrawPipelineKind::BlendToonZWrite, 1usize),
		];

		draws.sort_by_key(|&(pipeline, draw_index)| (3000, draw_index, blended_pipeline_pass_order(pipeline)));

		assert_eq!(
			draws,
			vec![
				(DrawPipelineKind::LilToonGemPre, 0),
				(DrawPipelineKind::BlendToonAdd, 0),
				(DrawPipelineKind::TransparentToonBackpass, 1),
				(DrawPipelineKind::BlendToonZWrite, 1),
			]
		);
		assert!(
			blended_pipeline_pass_order(DrawPipelineKind::TransparentToonBackpass)
				< blended_pipeline_pass_order(DrawPipelineKind::BlendToonZWrite)
		);
	}

	#[test]
	fn transparent_zwrite_toon_uses_backpass_before_forward_pass() {
		assert!(transparent_backpass_enabled(
			UnaAlphaMode::Blend,
			true,
			UnaShadingModel::LilToonLike,
			true
		));
		assert!(transparent_backpass_enabled(
			UnaAlphaMode::Blend,
			true,
			UnaShadingModel::MToonLike,
			true
		));
		assert!(!transparent_backpass_enabled(
			UnaAlphaMode::Blend,
			true,
			UnaShadingModel::LitLambert,
			true
		));
		assert!(!transparent_backpass_enabled(
			UnaAlphaMode::Opaque,
			true,
			UnaShadingModel::LilToonLike,
			true
		));
		assert!(!transparent_backpass_enabled(
			UnaAlphaMode::Blend,
			false,
			UnaShadingModel::LilToonLike,
			true
		));
		assert!(!transparent_backpass_enabled(
			UnaAlphaMode::Blend,
			true,
			UnaShadingModel::LilToonLike,
			false
		));
		assert!(transparent_forward_zwrite_enabled(
			UnaAlphaMode::Blend,
			true,
			UnaShadingModel::LilToonLike
		));
		assert!(transparent_forward_zwrite_enabled(
			UnaAlphaMode::Blend,
			true,
			UnaShadingModel::MToonLike
		));
		assert!(!transparent_forward_zwrite_enabled(
			UnaAlphaMode::Blend,
			true,
			UnaShadingModel::LitLambert
		));
		assert!(!transparent_forward_zwrite_enabled(
			UnaAlphaMode::Blend,
			false,
			UnaShadingModel::LilToonLike
		));
	}

	#[test]
	fn liltoon_source_zwrite_controls_transparent_zwrite() {
		let base_liltoon = un_avatar_core::UnaLilToonLikeMaterial::default();
		let enabled = UnaMaterialPbr {
			shading: UnaShadingModel::LilToonLike,
			alpha_mode: UnaAlphaMode::Blend,
			liltoon_like: Some(base_liltoon.clone()),
			unavatar_material: Some(serde_json::json!({
				"sourceShader": "Hidden/lilToonTransparent",
				"floatParams": { "_ZWrite": 1.0 }
			})),
			..Default::default()
		};
		let disabled = UnaMaterialPbr {
			shading: UnaShadingModel::LilToonLike,
			alpha_mode: UnaAlphaMode::Blend,
			liltoon_like: Some(base_liltoon),
			unavatar_material: Some(serde_json::json!({
				"sourceShader": "Hidden/lilToonTransparent",
				"floatParams": { "_ZWrite": 0.0 }
			})),
			..Default::default()
		};

		assert!(material_transparent_with_zwrite(&enabled));
		assert!(!material_transparent_with_zwrite(&disabled));
	}

	#[test]
	fn high_render_queue_cutout_uses_late_non_blend_pass() {
		assert!(draw_uses_late_non_blend_queue(UnaAlphaMode::Mask, 3001));
		assert!(draw_uses_late_non_blend_queue(UnaAlphaMode::Opaque, 3000));
		assert!(!draw_uses_late_non_blend_queue(UnaAlphaMode::Mask, 2450));
		assert!(!draw_uses_late_non_blend_queue(UnaAlphaMode::Blend, 3000));
		assert_eq!(
			opaque_pipeline_for_shading(UnaShadingModel::LilToonLike),
			DrawPipelineKind::OpaqueToon
		);
	}

	#[test]
	fn liltoon_refraction_material_needs_screen_refraction_grab_before_late_pass() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.source_profile = un_avatar_core::UnaLilToonLikeSourceProfile::LiltoonRefraction;
		liltoon_like.reflection.gem_refraction_strength_factor = 0.1;
		liltoon_like.rendering.render_queue_number = Some(2900);
		let mat = UnaMaterialPbr {
			shading: UnaShadingModel::LilToonLike,
			alpha_mode: UnaAlphaMode::Opaque,
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		assert!(material_needs_screen_refraction(&mat));
		assert!(!draw_uses_late_non_blend_queue(
			mat.alpha_mode,
			material_render_queue_number(&mat, mat.alpha_mode)
		));
	}

	#[test]
	fn material_runtime_requirements_collects_toon_feature_bits() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.source_profile = un_avatar_core::UnaLilToonLikeSourceProfile::LiltoonRefraction;
		liltoon_like.reflection.gem_refraction_strength_factor = 0.1;
		liltoon_like.audio_link.enabled_factor = 1.0;
		liltoon_like.audio_link.to_emission_factor = 1.0;
		liltoon_like.fur.enabled_factor = 1.0;
		liltoon_like.fur.layer_count_factor = 1.0;
		let mat = UnaMaterialPbr {
			shading: UnaShadingModel::LilToonLike,
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let requirements = material_runtime_requirements(&mat, UnaShadingModel::LilToonLike, &SceneMeshLoadOpts::default());
		assert!(requirements.audio_link_texture);
		assert!(requirements.screen_refraction);
		assert!(requirements.fur);
		let liltoon_plan = material_untoon_feature_plan(&mat, UnaShadingModel::LilToonLike, &SceneMeshLoadOpts::default());
		assert_eq!(liltoon_plan.source_profile, UntoonSourceProfile::LilToon);
		assert!(liltoon_plan.shader_features.audio_link);
		assert!(liltoon_plan.shader_features.refraction);

		let disabled_fur = material_runtime_requirements(
			&mat,
			UnaShadingModel::LilToonLike,
			&SceneMeshLoadOpts {
				disable_fur: true,
				..Default::default()
			},
		);
		assert!(disabled_fur.audio_link_texture);
		assert!(disabled_fur.screen_refraction);
		assert!(!disabled_fur.fur);

		let mtoon_requirements = material_runtime_requirements(&mat, UnaShadingModel::MToonLike, &SceneMeshLoadOpts::default());
		assert!(!mtoon_requirements.audio_link_texture);
		assert!(mtoon_requirements.screen_refraction);
		assert!(!mtoon_requirements.fur);

		let mut merged = mtoon_requirements;
		merged.include(requirements);
		assert!(merged.audio_link_texture);
		assert!(merged.screen_refraction);
		assert!(merged.fur);
	}

	#[test]
	fn mtoon_compatibility_maps_to_untoon_features_without_profile_extensions() {
		let mat = UnaMaterialPbr {
			shading: UnaShadingModel::MToonLike,
			emissive_texture_index: Some(4),
			mtoon: Some(UnaMtoonMaterial {
				matcap_texture_index: Some(1),
				reflection_cube_texture_index: Some(2),
				rim_multiply_texture_index: Some(3),
				..Default::default()
			}),
			..Default::default()
		};

		let plan = material_untoon_feature_plan(&mat, UnaShadingModel::MToonLike, &SceneMeshLoadOpts::default());
		let features = plan.shader_features;

		assert_eq!(plan.source_profile, UntoonSourceProfile::MToon);
		assert!(!features.profile_extensions);
		assert!(features.shadow_layers);
		assert!(features.matcap);
		assert!(features.reflection);
		assert!(features.reflection_cube);
		assert!(features.rim);
		assert!(features.emission);
		assert!(!features.main_layers);
		assert!(!features.audio_link);
	}

	#[test]
	fn material_render_queue_prefers_liltoon_source_queue() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.rendering.render_queue_number = Some(2461);
		let mat = UnaMaterialPbr {
			shading: UnaShadingModel::LilToonLike,
			alpha_mode: UnaAlphaMode::Blend,
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		assert_eq!(material_render_queue_number(&mat, mat.alpha_mode), 2461);
		assert_eq!(material_render_queue_number(&UnaMaterialPbr::default(), UnaAlphaMode::Opaque), 2000);
		assert_eq!(material_render_queue_number(&UnaMaterialPbr::default(), UnaAlphaMode::Mask), 2450);
		assert_eq!(material_render_queue_number(&UnaMaterialPbr::default(), UnaAlphaMode::Blend), 3000);
	}

	#[test]
	fn normal_map_uv_transform_prefers_liltoon_bump_slot() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like
			.texture_uv_offset_scales
			.insert("_BumpMap".to_string(), [0.1, 0.2, 0.75, 1.5]);
		liltoon_like
			.texture_uv_offset_scales
			.insert("_Bump2ndMap".to_string(), [0.3, 0.4, 1.25, 1.75]);
		liltoon_like
			.texture_uv_offset_scales
			.insert("_Bump2ndScaleMask".to_string(), [0.5, 0.6, 1.5, 1.6]);
		liltoon_like.texture_uv_mode_factors.insert("_Bump2ndMap".to_string(), 2.0);
		liltoon_like.normal.second_enabled_factor = 1.0;
		liltoon_like.normal.second_scale_factor = 0.6;
		let mat = UnaMaterialPbr {
			normal_texture_index: Some(0),
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);

		assert_eq!(draw.normal_uv_offset_scale, [0.1, 0.2, 0.75, 1.5]);
		assert_eq!(draw.normal2nd_uv_offset_scale, [0.3, 0.4, 1.25, 1.75]);
		assert_eq!(draw.normal2nd_scale_mask_uv_offset_scale, [0.5, 0.6, 1.5, 1.6]);
		assert_eq!(draw.normal2nd_params, [1.0, 0.6, 2.0, 0.0]);
	}

	#[test]
	fn main_texture_hsvg_reaches_draw_uniform() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.main_color.main_texture_hsvg_factor = [0.25, 0.8, 1.2, 0.9];
		let mat = UnaMaterialPbr {
			shading: UnaShadingModel::LilToonLike,
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);

		assert_eq!(draw.main_color_adjust_params, [0.25, 0.8, 1.2, 0.9]);
	}

	#[test]
	fn second_matcap_shadow_mask_reaches_draw_uniform() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.matcap.second_shadow_mask_factor = 0.42;
		let mat = UnaMaterialPbr {
			shading: UnaShadingModel::LilToonLike,
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);

		assert_eq!(draw.matcap2_ext_params[1], 0.42);
	}

	#[test]
	fn liltoon_rim_signed_direction_ranges_reach_draw_uniform() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.rim.enabled_factor = 0.75;
		liltoon_like.rim.directional_strength_factor = 0.5;
		liltoon_like.rim.directional_range_factor = -0.75;
		liltoon_like.rim.indirect_range_factor = -0.25;
		let mat = UnaMaterialPbr {
			shading: UnaShadingModel::LilToonLike,
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);

		assert_eq!(draw.rim_indirect_params[0], 0.5);
		assert_eq!(draw.rim_indirect_params[1], -0.75);
		assert_eq!(draw.rim_indirect_params[2], -0.25);
		assert_eq!(draw.rim_params[3], 0.75);
	}

	#[test]
	fn liltoon_matcap_color_alpha_reaches_blend_uniform() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.matcap.enabled_factor = 1.0;
		liltoon_like.matcap.blend_factor = 0.5;
		liltoon_like.matcap.color_alpha_factor = 0.4;
		let mat = UnaMaterialPbr {
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);

		assert_eq!(draw.matcap_params[0], 0.2);
	}

	#[test]
	fn plain_profile_emits_no_untoon_semantic_flags() {
		let mat = UnaMaterialPbr {
			liltoon_like: Some(un_avatar_core::UnaLilToonLikeMaterial::default()),
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);
		let flags = draw.params[3].to_bits();

		assert_eq!(flags & MAT_UNTOON_GEM_PROFILE, 0);
		assert_eq!(flags & MAT_UNTOON_ADDITIVE_BLEND, 0);
	}

	#[test]
	fn gem_profile_flag_reaches_draw_uniform() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.source_profile = un_avatar_core::UnaLilToonLikeSourceProfile::LiltoonGem;
		liltoon_like.reflection.gem_refraction_strength_factor = 0.45;
		liltoon_like.reflection.gem_chromatic_aberration_factor = 0.03;
		liltoon_like.reflection.gem_particle_loop_factor = 6.0;
		liltoon_like.reflection.gem_vr_parallax_strength_factor = 0.8;
		liltoon_like.reflection.gem_particle_color_factor = [2.0, 3.0, 4.0, 0.5];
		liltoon_like.blend_state.destination_factor = 1.0;
		let mat = UnaMaterialPbr {
			shading: UnaShadingModel::LilToonLike,
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);
		let flags = draw.params[3].to_bits();

		assert_ne!(flags & MAT_UNTOON_GEM_PROFILE, 0);
		assert_ne!(flags & MAT_UNTOON_ADDITIVE_BLEND, 0);
		assert_eq!(draw.gem_params, [0.45, 0.03, 6.0, 0.8]);
		assert_eq!(draw.gem_particle_color, [2.0, 3.0, 4.0, 0.5]);
	}

	#[test]
	fn liltoon_gem_prepass_requires_gem_source() {
		let mut gem = un_avatar_core::UnaLilToonLikeMaterial {
			source_profile: un_avatar_core::UnaLilToonLikeSourceProfile::LiltoonGem,
			..Default::default()
		};
		let gem_material = UnaMaterialPbr {
			shading: UnaShadingModel::LilToonLike,
			liltoon_like: Some(gem.clone()),
			..Default::default()
		};
		assert!(material_uses_liltoon_gem_prepass(&gem_material));

		gem.blend_state.destination_factor = 1.0;
		let additive = UnaMaterialPbr {
			shading: UnaShadingModel::LilToonLike,
			liltoon_like: Some(gem),
			..Default::default()
		};
		assert!(material_uses_liltoon_gem_prepass(&additive));

		let mut transparent = un_avatar_core::UnaLilToonLikeMaterial::default();
		transparent.blend_state.destination_factor = 1.0;
		let non_gem = UnaMaterialPbr {
			shading: UnaShadingModel::LilToonLike,
			liltoon_like: Some(transparent),
			..Default::default()
		};
		assert!(!material_uses_liltoon_gem_prepass(&non_gem));
	}

	#[test]
	fn liltoon_gem_uses_exported_cube_without_override() {
		let mut gem = un_avatar_core::UnaLilToonLikeMaterial {
			source_profile: un_avatar_core::UnaLilToonLikeSourceProfile::LiltoonGem,
			..Default::default()
		};
		gem.reflection.cube_texture_index = Some(42);
		gem.reflection.cube_override_factor = 0.0;
		assert_eq!(liltoon_reflection_texture_index(&gem), Some(42));

		let mut normal = gem.clone();
		normal.source_profile = un_avatar_core::UnaLilToonLikeSourceProfile::Liltoon;
		assert_eq!(liltoon_reflection_texture_index(&normal), None);

		normal.reflection.cube_override_factor = 1.0;
		assert_eq!(liltoon_reflection_texture_index(&normal), None);

		normal.reflection.enabled_factor = 1.0;
		assert_eq!(liltoon_reflection_texture_index(&normal), Some(42));
	}

	#[test]
	fn refraction_profile_flag_reaches_draw_uniform() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.source_profile = un_avatar_core::UnaLilToonLikeSourceProfile::LiltoonRefraction;
		liltoon_like.reflection.gem_refraction_strength_factor = -0.25;
		liltoon_like.reflection.refraction_color_from_main_factor = 1.0;
		liltoon_like.reflection.refraction_color_factor = [0.8, 0.9, 1.0, 0.6];
		let mat = UnaMaterialPbr {
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);
		let flags = draw.params[3].to_bits();

		assert_ne!(flags & MAT_UNTOON_REFRACTION_PROFILE, 0);
		assert_eq!(draw.gem_params, [-0.25, 1.0, 0.0, 1.0]);
		assert_eq!(draw.gem_particle_color, [0.8, 0.9, 1.0, 0.6]);
	}

	#[test]
	fn disable_reflection_diagnostic_zeros_liltoon_reflection_controls() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.reflection.enabled_factor = 1.0;
		liltoon_like.reflection.apply_specular_factor = 0.8;
		liltoon_like.reflection.apply_reflection_factor = 0.6;
		let mat = UnaMaterialPbr {
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let normal = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);
		assert_eq!(normal.reflection_control[0], 1.0);
		assert_eq!(normal.reflection_control[1], 0.8);
		assert_eq!(normal.reflection_control[2], 0.6);

		let disabled = mesh_draw_material_gpu(
			&mat,
			&UnaMtoonMaterial::default(),
			&SceneMeshLoadOpts {
				debug_disable_reflection: true,
				..Default::default()
			},
			0,
			0,
		);
		assert_eq!(disabled.reflection_control[0], 0.0);
		assert_eq!(disabled.reflection_control[1], 0.0);
		assert_eq!(disabled.reflection_control[2], 0.0);
		assert_eq!(disabled.reflection_control[3], normal.reflection_control[3]);
	}

	#[test]
	fn liltoon_fur_layer_num_maps_to_geometry_sample_count() {
		assert_eq!(liltoon_fur_sample_count_for_layer_num(1.0), 4);
		assert_eq!(liltoon_fur_sample_count_for_layer_num(2.0), 7);
		assert_eq!(liltoon_fur_sample_count_for_layer_num(3.0), 13);
		assert_eq!(liltoon_fur_sample_count_for_layer_num(12.0), 13);
	}

	#[test]
	fn compute_fur_cards_mode_density_uses_liltoon_layer_num_as_compatibility_hint() {
		assert_eq!(
			compute_fur_cards_mode_density(1.0, ComputeFurCardsExpressionMode::LilToonCompatible),
			4.0
		);
		assert_eq!(
			compute_fur_cards_mode_density(2.0, ComputeFurCardsExpressionMode::LilToonCompatible),
			7.0
		);
		assert_eq!(
			compute_fur_cards_mode_density(3.0, ComputeFurCardsExpressionMode::LilToonCompatible),
			13.0
		);
		assert!(
			compute_fur_cards_mode_density(3.0, ComputeFurCardsExpressionMode::UnaStandard)
				> compute_fur_cards_mode_density(3.0, ComputeFurCardsExpressionMode::LilToonCompatible)
		);
		assert!(
			compute_fur_cards_mode_density(3.0, ComputeFurCardsExpressionMode::UnaHighQuality)
				> compute_fur_cards_mode_density(3.0, ComputeFurCardsExpressionMode::UnaStandard)
		);
	}

	#[test]
	fn compute_fur_cards_triangle_card_count_is_monotonic_for_area_mask_length_and_quality() {
		let base_metrics = ComputeFurCardsTriangleMetrics {
			world_area: 0.0004,
			uv_area: 0.0002,
			average_fur_mask: 0.5,
			average_length_mask: 0.5,
			fur_length: 0.02,
			projected_area_factor: 1.0,
		};
		let params = ComputeFurCardsAllocationParams {
			min_cards_per_visible_triangle: 1,
			max_cards_per_triangle: 128,
			..Default::default()
		};

		let base = compute_fur_cards_triangle_card_count(3.0, ComputeFurCardsExpressionMode::UnaStandard, base_metrics, params);
		let larger_area = compute_fur_cards_triangle_card_count(
			3.0,
			ComputeFurCardsExpressionMode::UnaStandard,
			ComputeFurCardsTriangleMetrics {
				world_area: base_metrics.world_area * 4.0,
				..base_metrics
			},
			params,
		);
		let stronger_mask = compute_fur_cards_triangle_card_count(
			3.0,
			ComputeFurCardsExpressionMode::UnaStandard,
			ComputeFurCardsTriangleMetrics {
				average_fur_mask: 1.0,
				..base_metrics
			},
			params,
		);
		let longer_fur = compute_fur_cards_triangle_card_count(
			3.0,
			ComputeFurCardsExpressionMode::UnaStandard,
			ComputeFurCardsTriangleMetrics {
				average_length_mask: 1.0,
				fur_length: 0.04,
				..base_metrics
			},
			params,
		);
		let higher_quality = compute_fur_cards_triangle_card_count(
			3.0,
			ComputeFurCardsExpressionMode::UnaStandard,
			base_metrics,
			ComputeFurCardsAllocationParams {
				global_quality_scale: 2.0,
				..params
			},
		);

		assert!(larger_area >= base);
		assert!(stronger_mask >= base);
		assert!(longer_fur >= base);
		assert!(higher_quality >= base);
		assert_eq!(
			compute_fur_cards_triangle_card_count(
				3.0,
				ComputeFurCardsExpressionMode::UnaStandard,
				ComputeFurCardsTriangleMetrics {
					average_fur_mask: 0.0,
					..base_metrics
				},
				params,
			),
			0
		);
	}

	#[test]
	fn compute_fur_cards_barycentric_samples_are_deterministic_and_normalized() {
		let a = compute_fur_cards_barycentric_sample(12345, 7);
		let b = compute_fur_cards_barycentric_sample(12345, 7);
		let c = compute_fur_cards_barycentric_sample(12345, 8);
		assert_eq!(a, b);
		assert_ne!(a, c);
		let sum = a.barycentric[0] + a.barycentric[1] + a.barycentric[2];
		assert!((sum - 1.0).abs() < 0.00001);
		assert!(a.barycentric.iter().all(|&v| (0.0..=1.0).contains(&v)));

		let p = compute_fur_cards_interpolate_vec3(
			[Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)],
			a.barycentric,
		);
		assert!((p.x + p.y + p.z - 1.0).abs() < 0.00001);
		let uv = compute_fur_cards_interpolate_vec2([Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)], a.barycentric);
		assert!(uv.x >= 0.0 && uv.y >= 0.0 && uv.x + uv.y <= 1.00001);
	}

	#[test]
	fn compute_fur_cards_buffer_requirements_use_quad_cards_and_reject_overflow() {
		let req = compute_fur_cards_buffer_requirements(10).expect("buffer requirements");
		assert_eq!(req.vertex_count, 40);
		assert_eq!(req.index_count, 60);
		assert_eq!(
			req.vertex_bytes,
			40 * std::mem::size_of::<ComputeFurCardsGeneratedVertexGpu>() as u64
		);
		assert_eq!(req.index_bytes, 60 * std::mem::size_of::<u32>() as u64);
		assert!(compute_fur_cards_buffer_requirements(u32::MAX).is_none());
	}

	#[test]
	fn compute_fur_cards_source_buffers_pack_mesh_vertices_and_valid_triangles() {
		let verts = vec![
			Vertex {
				pos: [1.0, 2.0, 3.0],
				norm: [0.0, 1.0, 0.0],
				tangent: [1.0, 0.0, 0.0, 1.0],
				uv: [0.25, 0.5],
				uv1: [0.25, 0.5],
				uv2: [0.25, 0.5],
				uv3: [0.25, 0.5],
				joints: [1, 2, 3, 4],
				weights: [0.1, 0.2, 0.3, 0.4],
				color: [0.2, 0.4, 0.6, 1.0],
			},
			Vertex {
				pos: [4.0, 5.0, 6.0],
				norm: [0.0, 0.0, 1.0],
				tangent: [0.0, 1.0, 0.0, -1.0],
				uv: [0.75, 0.125],
				uv1: [0.75, 0.125],
				uv2: [0.75, 0.125],
				uv3: [0.75, 0.125],
				joints: [5, 6, 7, 8],
				weights: [0.4, 0.3, 0.2, 0.1],
				color: [1.0, 1.0, 1.0, 1.0],
			},
			Vertex {
				pos: [7.0, 8.0, 9.0],
				norm: [1.0, 0.0, 0.0],
				tangent: [0.0, 0.0, 1.0, 1.0],
				uv: [1.0, 0.0],
				uv1: [1.0, 0.0],
				uv2: [1.0, 0.0],
				uv3: [1.0, 0.0],
				joints: [9, 10, 11, 12],
				weights: [1.0, 0.0, 0.0, 0.0],
				color: [1.0, 1.0, 1.0, 1.0],
			},
		];

		let source_vertices = compute_fur_cards_source_vertices_from_mesh(&verts);
		assert_eq!(source_vertices.len(), 3);
		assert_eq!(source_vertices[0].position, [1.0, 2.0, 3.0, 1.0]);
		assert_eq!(source_vertices[0].uv, [0.25, 0.5, 0.0, 0.0]);
		assert_eq!(source_vertices[0].color, [0.2, 0.4, 0.6, 1.0]);
		assert_eq!(source_vertices[0].joints, [1, 2, 3, 4]);

		let source_triangles = compute_fur_cards_source_triangles_from_indices(&[0, 1, 2, 0, 2, 9, 2, 1, 0], verts.len());
		assert_eq!(
			source_triangles,
			vec![
				ComputeFurCardsSourceTriangleGpu { indices: [0, 1, 2, 0] },
				ComputeFurCardsSourceTriangleGpu { indices: [2, 1, 0, 0] },
			]
		);
		let source_triangles_u16 = compute_fur_cards_source_triangles_from_indices_u16(&[0, 1, 2, 0, 2, 9, 2, 1, 0], verts.len());
		assert_eq!(source_triangles_u16, source_triangles);

		let source_req = compute_fur_cards_source_buffer_requirements(source_vertices.len() as u32, source_triangles.len() as u32)
			.expect("source requirements");
		assert_eq!(
			source_req.vertex_bytes,
			3 * std::mem::size_of::<ComputeFurCardsSourceVertexGpu>() as u64
		);
		assert_eq!(
			source_req.triangle_bytes,
			2 * std::mem::size_of::<ComputeFurCardsSourceTriangleGpu>() as u64
		);
	}

	#[test]
	fn compute_fur_cards_source_vertices_can_follow_skin_palette() {
		let verts = vec![Vertex {
			pos: [1.0, 2.0, 3.0],
			norm: [0.0, 1.0, 0.0],
			tangent: [1.0, 0.0, 0.0, -1.0],
			uv: [0.25, 0.5],
			uv1: [0.25, 0.5],
			uv2: [0.25, 0.5],
			uv3: [0.25, 0.5],
			joints: [0, 1, 0, 0],
			weights: [0.25, 0.75, 0.0, 0.0],
			color: [0.25, 0.5, 0.75, 1.0],
		}];
		let mut palette = Vec::new();
		write_matrix_to_raw(&mut palette, Mat4::IDENTITY);
		write_matrix_to_raw(&mut palette, Mat4::from_translation(Vec3::new(4.0, 0.0, 0.0)));
		let mut source_vertices = Vec::new();
		let mut palette_matrices = Vec::new();
		compute_fur_cards_palette_matrices(&palette, &mut palette_matrices);
		compute_fur_cards_skinned_source_vertices_from_matrices(&verts, &palette_matrices, &mut source_vertices);

		assert_eq!(source_vertices.len(), 1);
		assert!((source_vertices[0].position[0] - 4.0).abs() < 0.00001);
		assert!((source_vertices[0].position[1] - 2.0).abs() < 0.00001);
		assert!((source_vertices[0].position[2] - 3.0).abs() < 0.00001);
		assert_eq!(source_vertices[0].normal, [0.0, 1.0, 0.0, 0.0]);
		assert_eq!(source_vertices[0].tangent, [1.0, 0.0, 0.0, -1.0]);
		assert_eq!(source_vertices[0].color, [0.25, 0.5, 0.75, 1.0]);
	}

	#[test]
	fn compute_fur_cards_single_weight_skinning_uses_same_palette_clamp() {
		let verts = vec![Vertex {
			pos: [1.0, 2.0, 3.0],
			norm: [0.0, 1.0, 0.0],
			tangent: [1.0, 0.0, 0.0, 1.0],
			uv: [0.25, 0.5],
			uv1: [0.25, 0.5],
			uv2: [0.25, 0.5],
			uv3: [0.25, 0.5],
			joints: [9, 0, 0, 0],
			weights: [1.0, 0.0, 0.0, 0.0],
			color: [1.0, 1.0, 1.0, 1.0],
		}];
		let mut palette = Vec::new();
		write_matrix_to_raw(&mut palette, Mat4::IDENTITY);
		write_matrix_to_raw(&mut palette, Mat4::from_translation(Vec3::new(4.0, 0.0, 0.0)));
		let mut source_vertices = Vec::new();
		let mut palette_matrices = Vec::new();
		compute_fur_cards_palette_matrices(&palette, &mut palette_matrices);
		compute_fur_cards_skinned_source_vertices_from_matrices(&verts, &palette_matrices, &mut source_vertices);

		assert_eq!(source_vertices.len(), 1);
		assert_eq!(source_vertices[0].position, [5.0, 2.0, 3.0, 1.0]);
		assert_eq!(source_vertices[0].normal, [0.0, 1.0, 0.0, 0.0]);
		assert_eq!(source_vertices[0].tangent, [1.0, 0.0, 0.0, 1.0]);
		assert_eq!(source_vertices[0].joints, [9, 0, 0, 0]);
		assert_eq!(source_vertices[0].weights, [1.0, 0.0, 0.0, 0.0]);
	}

	#[test]
	fn compute_fur_cards_card_sources_emit_liltoon_compatible_segments_per_triangle() {
		let verts = vec![
			Vertex {
				pos: [0.0, 0.0, 0.0],
				norm: [0.0, 1.0, 0.0],
				tangent: [1.0, 0.0, 0.0, 1.0],
				uv: [0.0, 0.0],
				uv1: [0.0, 0.0],
				uv2: [0.0, 0.0],
				uv3: [0.0, 0.0],
				joints: [0; 4],
				weights: [1.0, 0.0, 0.0, 0.0],
				color: [1.0, 1.0, 1.0, 1.0],
			},
			Vertex {
				pos: [0.05, 0.0, 0.0],
				norm: [0.0, 1.0, 0.0],
				tangent: [1.0, 0.0, 0.0, 1.0],
				uv: [1.0, 0.0],
				uv1: [1.0, 0.0],
				uv2: [1.0, 0.0],
				uv3: [1.0, 0.0],
				joints: [0; 4],
				weights: [1.0, 0.0, 0.0, 0.0],
				color: [1.0, 1.0, 1.0, 1.0],
			},
			Vertex {
				pos: [0.0, 0.05, 0.0],
				norm: [0.0, 1.0, 0.0],
				tangent: [1.0, 0.0, 0.0, 1.0],
				uv: [0.0, 1.0],
				uv1: [0.0, 1.0],
				uv2: [0.0, 1.0],
				uv3: [0.0, 1.0],
				joints: [0; 4],
				weights: [1.0, 0.0, 0.0, 0.0],
				color: [1.0, 1.0, 1.0, 1.0],
			},
			Vertex {
				pos: [0.2, 0.0, 0.0],
				norm: [0.0, 1.0, 0.0],
				tangent: [1.0, 0.0, 0.0, 1.0],
				uv: [4.0, 0.0],
				uv1: [4.0, 0.0],
				uv2: [4.0, 0.0],
				uv3: [4.0, 0.0],
				joints: [0; 4],
				weights: [1.0, 0.0, 0.0, 0.0],
				color: [1.0, 1.0, 1.0, 1.0],
			},
			Vertex {
				pos: [0.0, 0.2, 0.0],
				norm: [0.0, 1.0, 0.0],
				tangent: [1.0, 0.0, 0.0, 1.0],
				uv: [0.0, 4.0],
				uv1: [0.0, 4.0],
				uv2: [0.0, 4.0],
				uv3: [0.0, 4.0],
				joints: [0; 4],
				weights: [1.0, 0.0, 0.0, 0.0],
				color: [1.0, 1.0, 1.0, 1.0],
			},
		];
		let triangles = compute_fur_cards_source_triangles_from_indices(&[0, 1, 2, 0, 3, 4], verts.len());
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.fur.layer_count_factor = 3.0;
		liltoon_like.fur.vector_factor = [0.0, 0.0, 1.0, 0.04];
		let mat = UnaMaterialPbr {
			shading: UnaShadingModel::LilToonLike,
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let card_sources = compute_fur_cards_card_sources_from_triangles(
			&mat,
			&verts,
			&triangles,
			ComputeFurCardsCpuFurMaps {
				length_mask: None,
				fur_mask: None,
			},
		)
		.expect("card sources");
		assert_eq!(card_sources.len(), triangles.len() * liltoon_fur_segment_count(3.0) as usize);
		assert_eq!(card_sources[0].sample_index, 0);
		assert_eq!(card_sources[11].sample_index, 11);
		assert!(card_sources.iter().any(|source| source.indices == [0, 3, 4, 0]));
		assert!(card_sources
			.iter()
			.all(|source| source.sample_index < liltoon_fur_segment_count(3.0)));
	}

	#[test]
	fn compute_fur_cards_card_sources_use_cpu_fur_masks_for_allocation_budget() {
		let verts = vec![
			Vertex {
				pos: [0.0, 0.0, 0.0],
				norm: [0.0, 1.0, 0.0],
				tangent: [1.0, 0.0, 0.0, 1.0],
				uv: [0.0, 0.0],
				uv1: [0.0, 0.0],
				uv2: [0.0, 0.0],
				uv3: [0.0, 0.0],
				joints: [0; 4],
				weights: [1.0, 0.0, 0.0, 0.0],
				color: [1.0, 1.0, 1.0, 1.0],
			},
			Vertex {
				pos: [0.1, 0.0, 0.0],
				norm: [0.0, 1.0, 0.0],
				tangent: [1.0, 0.0, 0.0, 1.0],
				uv: [1.0, 0.0],
				uv1: [1.0, 0.0],
				uv2: [1.0, 0.0],
				uv3: [1.0, 0.0],
				joints: [0; 4],
				weights: [1.0, 0.0, 0.0, 0.0],
				color: [1.0, 1.0, 1.0, 1.0],
			},
			Vertex {
				pos: [0.0, 0.1, 0.0],
				norm: [0.0, 1.0, 0.0],
				tangent: [1.0, 0.0, 0.0, 1.0],
				uv: [0.0, 1.0],
				uv1: [0.0, 1.0],
				uv2: [0.0, 1.0],
				uv3: [0.0, 1.0],
				joints: [0; 4],
				weights: [1.0, 0.0, 0.0, 0.0],
				color: [1.0, 1.0, 1.0, 1.0],
			},
		];
		let triangles = compute_fur_cards_source_triangles_from_indices(&[0, 1, 2], verts.len());
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.fur.layer_count_factor = 3.0;
		liltoon_like.fur.vector_factor = [0.0, 0.0, 1.0, 0.04];
		let mat = UnaMaterialPbr {
			shading: UnaShadingModel::LilToonLike,
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};
		let white_mask = UnaImageRgba {
			width: 2,
			height: 2,
			pixel_format: un_avatar_core::UnaImagePixelFormat::R8G8B8A8,
			pixels: vec![255; 2 * 2 * 4],
		};
		let black_mask = UnaImageRgba {
			width: 2,
			height: 2,
			pixel_format: un_avatar_core::UnaImagePixelFormat::R8G8B8A8,
			pixels: vec![0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255],
		};

		let full = compute_fur_cards_card_sources_from_triangles(
			&mat,
			&verts,
			&triangles,
			ComputeFurCardsCpuFurMaps {
				length_mask: Some(&white_mask),
				fur_mask: Some(&white_mask),
			},
		)
		.expect("full mask cards");
		let masked = compute_fur_cards_card_sources_from_triangles(
			&mat,
			&verts,
			&triangles,
			ComputeFurCardsCpuFurMaps {
				length_mask: Some(&white_mask),
				fur_mask: Some(&black_mask),
			},
		);

		assert!(!full.is_empty());
		assert!(masked.is_none());
	}

	#[test]
	fn compute_fur_cards_params_convert_liltoon_root_offset_to_outward_bias() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.fur.vector_factor = [0.0, 0.0, 1.0, 0.1];
		liltoon_like.fur.root_offset_factor = -0.4;
		liltoon_like.fur.vector_scale_factor = 1.5;
		liltoon_like.fur.vector_texture_index = Some(17);
		liltoon_like.fur.vertex_color_to_vector_factor = 1.0;
		liltoon_like.fur.cutout_length_factor = 0.4;
		let mat = UnaMaterialPbr {
			shading: UnaShadingModel::LilToonLike,
			uv_offset_scale: [0.25, -0.5, 2.0, 3.0],
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};
		let generated = compute_fur_cards_buffer_requirements(2).expect("requirements");

		let params = compute_fur_cards_generate_params_from_material(&mat, 1, 2, generated);

		assert!((params.root_offset - 0.04).abs() < 0.00001);
		assert!((params.card_width - 0.014).abs() < 0.00001);
		assert_eq!(params.direction[3], 1.5);
		assert_eq!(
			params.feature_flags & COMPUTE_FUR_CARDS_FEATURE_FUR_VECTOR_TEX,
			COMPUTE_FUR_CARDS_FEATURE_FUR_VECTOR_TEX
		);
		assert_eq!(
			params.feature_flags & COMPUTE_FUR_CARDS_FEATURE_VERTEX_COLOR_FUR_VECTOR,
			COMPUTE_FUR_CARDS_FEATURE_VERTEX_COLOR_FUR_VECTOR
		);
		assert_eq!(params.main_uv, [2.0, 3.0, 0.25, -0.5]);
		assert_eq!(params.model, Mat4::IDENTITY.to_cols_array_2d());
		assert_eq!(params.inv_model, Mat4::IDENTITY.to_cols_array_2d());
		assert_eq!(params.cutout_length, 0.4);
	}

	#[test]
	fn compute_fur_cards_dispatch_workgroups_cover_all_source_triangles() {
		assert_eq!(compute_fur_cards_dispatch_workgroups(0), 0);
		assert_eq!(compute_fur_cards_dispatch_workgroups(1), 1);
		assert_eq!(compute_fur_cards_dispatch_workgroups(64), 1);
		assert_eq!(compute_fur_cards_dispatch_workgroups(65), 2);
	}

	#[test]
	fn liltoon_fur_params_reach_draw_uniform() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.fur.enabled_factor = 1.0;
		liltoon_like.fur.layer_count_factor = 3.0;
		liltoon_like.fur.vector_factor = [0.1, 0.2, 0.3, 0.4];
		liltoon_like.fur.vector_scale_factor = 1.75;
		liltoon_like.fur.gravity_factor = 0.35;
		liltoon_like.fur.shell_ao_factor = 0.6;
		liltoon_like.fur.root_offset_factor = -0.35;
		liltoon_like.fur.cutout_length_factor = 0.9;
		liltoon_like.fur.randomize_factor = 0.45;
		liltoon_like.fur.noise_tiling_factor = 2.0;
		liltoon_like.fur.noise_offset_factor = 0.25;
		liltoon_like.fur.rim_color_factor = [0.2, 0.3, 0.4, 0.5];
		liltoon_like.fur.rim_fresnel_power_factor = 4.5;
		liltoon_like.fur.rim_anti_light_factor = 0.75;
		liltoon_like.fur.vector_texture_index = Some(9);
		liltoon_like.fur.vertex_color_to_vector_factor = 1.0;
		let mat = UnaMaterialPbr {
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);

		assert_eq!(draw.fur_params, [1.0, 13.0, 0.35, 0.45]);
		assert_eq!(draw.fur_vector_params, [0.1, 0.2, 0.3, 0.4]);
		assert_eq!(draw.fur_noise_params, [2.0, 2.0, 0.25, 0.25]);
		assert_eq!(draw.fur_ext_params, [1.75, 0.6, -0.35, 0.9]);
		assert_eq!(draw.fur_rim_color, [0.2, 0.3, 0.4, 0.5]);
		assert_eq!(draw.fur_rim_params, [4.5, 0.75, 1.0, 1.0]);
	}

	#[test]
	fn liltoon_fur_noise_mask_st_reaches_draw_uniform() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like
			.texture_uv_offset_scales
			.insert("_FurNoiseMask".to_string(), [0.0, -49.0, 50.0, 50.0]);
		liltoon_like.fur.noise_tiling_factor = 2.0;
		liltoon_like.fur.noise_offset_factor = 0.25;
		let mat = UnaMaterialPbr {
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);

		assert_eq!(draw.fur_noise_params, [50.0, 50.0, 0.0, -49.0]);
	}

	#[test]
	fn disable_fur_diagnostic_suppresses_fur_draws() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.fur.enabled_factor = 1.0;
		liltoon_like.fur.layer_count_factor = 2.0;
		let mat = UnaMaterialPbr {
			shading: UnaShadingModel::LilToonLike,
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		assert_eq!(material_fur_layer_count(&mat, UnaShadingModel::LilToonLike), 7);
		assert!(material_has_fur(&mat, UnaShadingModel::LilToonLike, &SceneMeshLoadOpts::default()));
		assert!(!material_has_fur(
			&mat,
			UnaShadingModel::LilToonLike,
			&SceneMeshLoadOpts {
				disable_fur: true,
				..Default::default()
			}
		));
	}

	#[test]
	fn liltoon_backlight_reaches_draw_uniform() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.backlight.enabled_factor = 1.0;
		liltoon_like.backlight.color_factor = [0.2, 0.3, 0.4, 0.5];
		liltoon_like.backlight.main_strength_factor = 0.6;
		liltoon_like.backlight.normal_strength_factor = 0.7;
		liltoon_like.backlight.directivity_factor = 8.0;
		liltoon_like.backlight.border_factor = 0.1;
		liltoon_like.backlight.blur_factor = 0.2;
		liltoon_like.backlight.view_strength_factor = 0.3;
		liltoon_like.backlight.backface_mask_factor = 0.4;
		liltoon_like.backlight.receive_shadow_factor = 0.5;
		liltoon_like
			.texture_uv_offset_scales
			.insert("_BacklightColorTex".to_string(), [0.6, 0.7, 1.6, 1.7]);
		let mat = UnaMaterialPbr {
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);

		assert_eq!(draw.backlight_color, [0.2, 0.3, 0.4, 0.5]);
		assert_eq!(draw.backlight_params, [1.0, 0.6, 0.7, 8.0]);
		assert_eq!(draw.backlight_ext_params, [0.1, 0.2, 0.3, 0.4]);
		assert_eq!(draw.backlight_shadow_params, [0.5, 0.0, 0.0, 0.0]);
		assert_eq!(draw.backlight_color_uv_offset_scale, [0.6, 0.7, 1.6, 1.7]);
	}

	#[test]
	fn liltoon_distance_fade_reaches_draw_uniform() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.rendering.backface_color_factor = [0.9, 0.8, 0.7, 0.6];
		liltoon_like.rendering.distance_fade_factor = [0.2, 5.0, 0.75, 1.0];
		liltoon_like.rendering.distance_fade_color_factor = [0.3, 0.4, 0.5, 0.6];
		liltoon_like.rendering.distance_fade_rim_color_factor = [0.7, 0.8, 0.9, 0.25];
		liltoon_like.rendering.distance_fade_rim_fresnel_power_factor = 6.5;
		liltoon_like.rendering.distance_fade_mode_factor = 1.0;
		let mat = UnaMaterialPbr {
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);

		assert_eq!(draw.backface_color, [0.9, 0.8, 0.7, 0.6]);
		assert_eq!(draw.distance_fade, [0.2, 5.0, 0.75, 1.0]);
		assert_eq!(draw.distance_fade_color, [0.3, 0.4, 0.5, 0.6]);
		assert_eq!(draw.distance_fade_rim_color, [0.7, 0.8, 0.9, 0.25]);
		assert_eq!(draw.distance_fade_params, [1.0, 6.5, 0.0, 0.0]);
	}

	#[test]
	fn liltoon_dissolve_reaches_draw_uniform() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.dissolve.mask_texture_index = Some(3);
		liltoon_like.dissolve.noise_mask_texture_index = Some(4);
		liltoon_like.dissolve.color_factor = [1.2, 1.1, 1.0, 0.9];
		liltoon_like.dissolve.params_factor = [1.0, 0.0, 0.45, 0.12];
		liltoon_like.dissolve.position_factor = [0.25, 0.75, 0.0, 0.5];
		liltoon_like.dissolve.noise_strength_factor = 0.25;
		liltoon_like.dissolve.noise_uv_scroll_rotate_factor = [0.01, 0.02, 0.03, 0.04];
		liltoon_like
			.texture_uv_offset_scales
			.insert("_DissolveMask".to_string(), [0.1, 0.2, 0.3, 0.4]);
		liltoon_like
			.texture_uv_offset_scales
			.insert("_DissolveNoiseMask".to_string(), [0.5, 0.6, 0.7, 0.8]);
		let mat = UnaMaterialPbr {
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);

		assert_eq!(draw.dissolve_color, [1.2, 1.1, 1.0, 0.9]);
		assert_eq!(draw.dissolve_params, [1.0, 0.0, 0.45, 0.12]);
		assert_eq!(draw.dissolve_pos, [0.25, 0.75, 0.0, 0.5]);
		assert_eq!(draw.dissolve_ext, [0.25, 1.0, 1.0, 0.0]);
		assert_eq!(draw.dissolve_mask_uv_offset_scale, [0.1, 0.2, 0.3, 0.4]);
		assert_eq!(draw.dissolve_noise_uv_offset_scale, [0.5, 0.6, 0.7, 0.8]);
		assert_eq!(draw.dissolve_noise_uv_anim_params, [0.01, 0.02, 0.03, 0.04]);
	}

	#[test]
	fn liltoon_parallax_reaches_draw_uniform() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.parallax.texture_index = Some(5);
		liltoon_like.parallax.enabled_factor = 1.0;
		liltoon_like.parallax.pom_enabled_factor = 1.0;
		liltoon_like.parallax.scale_factor = 0.07;
		liltoon_like.parallax.offset_factor = 0.35;
		liltoon_like
			.texture_uv_offset_scales
			.insert("_ParallaxMap".to_string(), [0.12, 0.23, 1.2, 1.3]);
		let mat = UnaMaterialPbr {
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);

		assert_eq!(draw.parallax_params, [1.0, 1.0, 0.07, 0.35]);
		assert_eq!(draw.parallax_uv_offset_scale, [0.12, 0.23, 1.2, 1.3]);
	}

	#[test]
	fn liltoon_main_layer_dissolve_reaches_draw_uniform() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.texture_uv_mode_factors.insert("_Main2ndTex".to_string(), 2.0);
		liltoon_like.texture_uv_mode_factors.insert("_Main3rdTex".to_string(), 4.0);
		liltoon_like.main_color.second_cull_factor = 1.0;
		liltoon_like.main_color.second_distance_fade_factor = [1.0, 6.0, 0.4, 0.0];
		liltoon_like.main_color.second_decal_flags_factor = [1.0, 1.0, 0.0, 1.0];
		liltoon_like.main_color.second_decal_transform_factor = [0.25, 1.0, 0.0, 0.0];
		liltoon_like.main_color.second_decal_animation_factor = [4.0, 2.0, 0.0, 0.0];
		liltoon_like.main_color.second_decal_sub_param_factor = [1.0, 1.0, 0.5, 0.0];
		liltoon_like.main_color.third_cull_factor = 2.0;
		liltoon_like.main_color.third_distance_fade_factor = [2.0, 7.0, 0.5, 0.0];
		liltoon_like.main_color.third_decal_flags_factor = [1.0, 0.0, 1.0, 0.0];
		liltoon_like.main_color.third_decal_transform_factor = [0.5, 0.0, 1.0, 0.0];
		liltoon_like.main_color.third_decal_animation_factor = [3.0, 3.0, 0.0, 0.0];
		liltoon_like.main_color.third_decal_sub_param_factor = [0.5, 0.5, 0.25, 0.0];
		liltoon_like.main_color.second_dissolve.mask_texture_index = Some(5);
		liltoon_like.main_color.second_dissolve.noise_mask_texture_index = Some(6);
		liltoon_like.main_color.second_dissolve.color_factor = [0.2, 0.3, 0.4, 0.5];
		liltoon_like.main_color.second_dissolve.params_factor = [1.0, 0.0, 0.25, 0.05];
		liltoon_like.main_color.second_dissolve.position_factor = [0.11, 0.22, 0.33, 0.44];
		liltoon_like.main_color.second_dissolve.noise_strength_factor = 0.26;
		liltoon_like.main_color.second_dissolve.noise_uv_scroll_rotate_factor = [0.05, 0.06, 0.07, 0.08];
		liltoon_like.main_color.third_dissolve.mask_texture_index = Some(7);
		liltoon_like.main_color.third_dissolve.noise_mask_texture_index = Some(8);
		liltoon_like.main_color.third_dissolve.color_factor = [0.6, 0.7, 0.8, 0.9];
		liltoon_like.main_color.third_dissolve.params_factor = [2.0, 1.0, 0.35, 0.06];
		liltoon_like.main_color.third_dissolve.position_factor = [0.55, 0.66, 0.77, 0.88];
		liltoon_like.main_color.third_dissolve.noise_strength_factor = 0.36;
		liltoon_like.main_color.third_dissolve.noise_uv_scroll_rotate_factor = [0.09, 0.10, 0.11, 0.12];
		liltoon_like
			.texture_uv_offset_scales
			.insert("_Main2ndDissolveMask".to_string(), [0.1, 0.2, 1.1, 1.2]);
		liltoon_like
			.texture_uv_offset_scales
			.insert("_Main2ndDissolveNoiseMask".to_string(), [0.3, 0.4, 1.3, 1.4]);
		liltoon_like
			.texture_uv_offset_scales
			.insert("_Main3rdDissolveMask".to_string(), [0.5, 0.6, 1.5, 1.6]);
		liltoon_like
			.texture_uv_offset_scales
			.insert("_Main3rdDissolveNoiseMask".to_string(), [0.7, 0.8, 1.7, 1.8]);
		let mat = UnaMaterialPbr {
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);

		assert_eq!(draw.main2nd_ext, [2.0, 1.0, 0.0, 0.0]);
		assert_eq!(draw.main2nd_distance_fade, [1.0, 6.0, 0.4, 0.0]);
		assert_eq!(draw.main2nd_decal_flags, [1.0, 1.0, 0.0, 1.0]);
		assert_eq!(draw.main2nd_decal_transform, [0.25, 1.0, 0.0, 0.0]);
		assert_eq!(draw.main2nd_decal_animation, [4.0, 2.0, 0.0, 0.0]);
		assert_eq!(draw.main2nd_decal_sub_param, [1.0, 1.0, 0.5, 0.0]);
		assert_eq!(draw.main2nd_dissolve_color, [0.2, 0.3, 0.4, 0.5]);
		assert_eq!(draw.main2nd_dissolve_params, [1.0, 0.0, 0.25, 0.05]);
		assert_eq!(draw.main2nd_dissolve_pos, [0.11, 0.22, 0.33, 0.44]);
		assert_eq!(draw.main2nd_dissolve_ext, [0.26, 1.0, 1.0, 0.0]);
		assert_eq!(draw.main2nd_dissolve_mask_uv_offset_scale, [0.1, 0.2, 1.1, 1.2]);
		assert_eq!(draw.main2nd_dissolve_noise_uv_offset_scale, [0.3, 0.4, 1.3, 1.4]);
		assert_eq!(draw.main2nd_dissolve_noise_uv_anim_params, [0.05, 0.06, 0.07, 0.08]);
		assert_eq!(draw.main3rd_ext, [4.0, 2.0, 0.0, 0.0]);
		assert_eq!(draw.main3rd_distance_fade, [2.0, 7.0, 0.5, 0.0]);
		assert_eq!(draw.main3rd_decal_flags, [1.0, 0.0, 1.0, 0.0]);
		assert_eq!(draw.main3rd_decal_transform, [0.5, 0.0, 1.0, 0.0]);
		assert_eq!(draw.main3rd_decal_animation, [3.0, 3.0, 0.0, 0.0]);
		assert_eq!(draw.main3rd_decal_sub_param, [0.5, 0.5, 0.25, 0.0]);
		assert_eq!(draw.main3rd_dissolve_color, [0.6, 0.7, 0.8, 0.9]);
		assert_eq!(draw.main3rd_dissolve_params, [2.0, 1.0, 0.35, 0.06]);
		assert_eq!(draw.main3rd_dissolve_pos, [0.55, 0.66, 0.77, 0.88]);
		assert_eq!(draw.main3rd_dissolve_ext, [0.36, 1.0, 1.0, 0.0]);
		assert_eq!(draw.main3rd_dissolve_mask_uv_offset_scale, [0.5, 0.6, 1.5, 1.6]);
		assert_eq!(draw.main3rd_dissolve_noise_uv_offset_scale, [0.7, 0.8, 1.7, 1.8]);
		assert_eq!(draw.main3rd_dissolve_noise_uv_anim_params, [0.09, 0.10, 0.11, 0.12]);
	}

	#[test]
	fn liltoon_id_mask_and_udim_discard_reach_draw_uniform() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.id_mask.compile_factor = 1.0;
		liltoon_like.id_mask.from_factor = 8.0;
		liltoon_like.id_mask.is_bitmap_factor = 1.0;
		liltoon_like.id_mask.controls_dissolve_factor = 1.0;
		liltoon_like.id_mask.flags_factor = [1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0];
		liltoon_like.id_mask.prior_flags_factor = [0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0];
		liltoon_like.id_mask.indices_factor = [10, 20, 30, 40, 50, 60, 70, 80];
		liltoon_like.udim_discard.compile_factor = 1.0;
		liltoon_like.udim_discard.mode_factor = 1.0;
		liltoon_like.udim_discard.uv_factor = 2.0;
		liltoon_like.udim_discard.row0_factor = [0.0, 1.0, 0.0, 0.0];
		liltoon_like.udim_discard.row2_factor = [0.0, 0.0, 0.0, 1.0];
		let mat = UnaMaterialPbr {
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);

		assert_eq!(draw.id_mask_params, [1.0, 8.0, 1.0, 1.0]);
		assert_eq!(draw.id_mask_flags0, [1.0, 0.0, 1.0, 0.0]);
		assert_eq!(draw.id_mask_flags1, [1.0, 0.0, 1.0, 0.0]);
		assert_eq!(draw.id_mask_prior_flags0, [0.0, 1.0, 0.0, 1.0]);
		assert_eq!(draw.id_mask_prior_flags1, [0.0, 1.0, 0.0, 1.0]);
		assert_eq!(draw.id_mask_indices0, [10.0, 20.0, 30.0, 40.0]);
		assert_eq!(draw.id_mask_indices1, [50.0, 60.0, 70.0, 80.0]);
		assert_eq!(draw.udim_discard_params, [1.0, 1.0, 2.0, 0.0]);
		assert_eq!(draw.udim_discard_row0, [0.0, 1.0, 0.0, 0.0]);
		assert_eq!(draw.udim_discard_row2, [0.0, 0.0, 0.0, 1.0]);
	}

	#[test]
	fn liltoon_glitter_reaches_draw_uniform() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.glitter.enabled_factor = 1.0;
		liltoon_like.glitter.color_factor = [0.2, 0.3, 0.4, 0.5];
		liltoon_like.glitter.params1_factor = [512.0, 513.0, 0.08, 2.0];
		liltoon_like.glitter.params2_factor = [0.6, 0.7, 0.8, 0.9];
		liltoon_like.glitter.atlas_factor = [3.0, 4.0, 0.0, 0.0];
		liltoon_like.glitter.main_strength_factor = 0.1;
		liltoon_like.glitter.normal_strength_factor = 0.2;
		liltoon_like.glitter.post_contrast_factor = 1.3;
		liltoon_like.glitter.sensitivity_factor = 0.4;
		liltoon_like.glitter.enable_lighting_factor = 0.5;
		liltoon_like.glitter.shadow_mask_factor = 0.6;
		liltoon_like.glitter.apply_transparency_factor = 0.7;
		liltoon_like.glitter.backface_mask_factor = 0.8;
		liltoon_like.glitter.scale_randomize_factor = 0.9;
		liltoon_like.glitter.uv_mode_factor = 1.0;
		liltoon_like.glitter.color_texture_uv_mode_factor = 2.0;
		liltoon_like.glitter.apply_shape_factor = 1.0;
		liltoon_like.glitter.angle_randomize_factor = 1.0;
		liltoon_like.glitter.vr_parallax_strength_factor = 0.4;
		liltoon_like
			.texture_uv_offset_scales
			.insert("_GlitterColorTex".to_string(), [0.1, 0.2, 0.3, 0.4]);
		liltoon_like
			.texture_uv_offset_scales
			.insert("_GlitterShapeTex".to_string(), [0.5, 0.6, 0.7, 0.8]);
		let mat = UnaMaterialPbr {
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);

		assert_eq!(draw.glitter_color, [0.2, 0.3, 0.4, 0.5]);
		assert_eq!(draw.glitter_params1, [512.0, 513.0, 0.08, 2.0]);
		assert_eq!(draw.glitter_params2, [0.6, 0.7, 0.8, 0.9]);
		assert_eq!(draw.glitter_control, [1.0, 0.1, 0.2, 1.3]);
		assert_eq!(draw.glitter_ext, [0.4, 0.5, 0.6, 0.7]);
		assert_eq!(draw.glitter_ext2, [0.8, 0.9, 1.0, 2.0]);
		assert_eq!(draw.glitter_ext3, [0.4, 1.0, 1.0, 0.0]);
		assert_eq!(draw.glitter_color_uv_offset_scale, [0.1, 0.2, 0.3, 0.4]);
		assert_eq!(draw.glitter_shape_uv_offset_scale, [0.5, 0.6, 0.7, 0.8]);
		assert_eq!(draw.glitter_atlas, [3.0, 4.0, 0.0, 0.0]);
	}

	#[test]
	fn liltoon_matcap_uv_flags_reach_draw_uniform() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.matcap.perspective_factor = 0.1;
		liltoon_like.matcap.z_rotation_cancel_factor = 0.2;
		liltoon_like.matcap.second_perspective_factor = 0.3;
		liltoon_like.matcap.second_z_rotation_cancel_factor = 0.4;
		liltoon_like.matcap.blend_uv1_factor = [0.5, 0.6];
		liltoon_like.matcap.second_blend_uv1_factor = [0.7, 0.8];
		liltoon_like
			.texture_uv_offset_scales
			.insert("_MatCapTex".to_string(), [0.11, 0.12, 1.11, 1.12]);
		liltoon_like
			.texture_uv_offset_scales
			.insert("_MatCap2ndTex".to_string(), [0.21, 0.22, 1.21, 1.22]);
		let mat = UnaMaterialPbr {
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);

		assert_eq!(draw.matcap_uv_params, [0.1, 0.2, 0.3, 0.4]);
		assert_eq!(draw.matcap_uv_ext_params, [0.5, 0.6, 0.7, 0.8]);
		assert_eq!(draw.matcap_tex_uv_offset_scale, [0.11, 0.12, 1.11, 1.12]);
		assert_eq!(draw.matcap2_tex_uv_offset_scale, [0.21, 0.22, 1.21, 1.22]);
	}

	#[test]
	fn liltoon_matcap_custom_normal_reaches_draw_uniform() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.matcap.bump_texture_index = Some(3);
		liltoon_like.matcap.custom_normal_factor = 1.0;
		liltoon_like.matcap.bump_scale_factor = 0.7;
		liltoon_like.matcap.second_bump_texture_index = Some(4);
		liltoon_like.matcap.second_custom_normal_factor = 1.0;
		liltoon_like.matcap.second_bump_scale_factor = 0.8;
		liltoon_like
			.texture_uv_offset_scales
			.insert("_MatCapBumpMap".to_string(), [0.1, 0.2, 1.1, 1.2]);
		liltoon_like
			.texture_uv_offset_scales
			.insert("_MatCap2ndBumpMap".to_string(), [0.3, 0.4, 1.3, 1.4]);
		let mat = UnaMaterialPbr {
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);

		assert_eq!(draw.matcap_bump_params, [1.0, 0.7, 0.0, 0.0]);
		assert_eq!(draw.matcap2_bump_params, [1.0, 0.8, 0.0, 0.0]);
		assert_eq!(draw.matcap_bump_uv_offset_scale, [0.1, 0.2, 1.1, 1.2]);
		assert_eq!(draw.matcap2_bump_uv_offset_scale, [0.3, 0.4, 1.3, 1.4]);
	}

	#[test]
	fn liltoon_matcap_custom_normal_requires_texture() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.matcap.custom_normal_factor = 1.0;
		liltoon_like.matcap.second_custom_normal_factor = 1.0;
		let mat = UnaMaterialPbr {
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);

		assert_eq!(draw.matcap_bump_params[0], 0.0);
		assert_eq!(draw.matcap2_bump_params[0], 0.0);
	}

	#[test]
	fn liltoon_apply_transparency_factors_reach_draw_uniform() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.matcap.apply_transparency_factor = 0.1;
		liltoon_like.matcap.second_apply_transparency_factor = 0.2;
		liltoon_like.rim.apply_transparency_factor = 0.3;
		liltoon_like.reflection.apply_transparency_factor = 0.4;
		let mat = UnaMaterialPbr {
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);

		assert_eq!(draw.transparency_params, [0.1, 0.2, 0.3, 0.4]);
	}

	#[test]
	fn liltoon_gsaa_strength_reaches_draw_uniform() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.rendering.gsaa_strength_factor = 0.7;
		let mat = UnaMaterialPbr {
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);

		assert_eq!(draw.rendering_ext_params, [0.7, 0.0, 0.0, 0.0]);
	}

	#[test]
	fn liltoon_audio_link_reaches_draw_uniform() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.audio_link.enabled_factor = 1.0;
		liltoon_like.audio_link.uv_mode_factor = 2.0;
		liltoon_like.audio_link.to_emission_factor = 1.0;
		liltoon_like.audio_link.to_emission_gradation_factor = 0.75;
		liltoon_like.audio_link.default_value_factor = [0.4, 0.5, 3.0, 0.2];
		liltoon_like.audio_link.uv_params_factor = [0.6, 0.1, 0.25, 0.75];
		liltoon_like.audio_link.start_factor = [1.0, 2.0, 3.0, 0.0];
		liltoon_like.audio_link.to_emission_second_factor = 0.5;
		liltoon_like.audio_link.to_emission_second_gradation_factor = 0.25;
		liltoon_like.audio_link.to_main_second_factor = 1.0;
		liltoon_like.audio_link.to_main_third_factor = 1.0;
		liltoon_like.audio_link.to_vertex_factor = 1.0;
		liltoon_like.audio_link.vertex_uv_mode_factor = 3.0;
		liltoon_like.audio_link.vertex_strength_factor = [0.2, 0.0, 0.0, 0.0];
		liltoon_like.audio_link.as_local_factor = 1.0;
		liltoon_like.audio_link.vertex_uv_params_factor = [0.7, 0.2, 0.3, 0.4];
		liltoon_like.audio_link.vertex_start_factor = [4.0, 5.0, 6.0, 0.0];
		liltoon_like.audio_link.mask_texture_index = Some(2);
		liltoon_like.audio_link.mask_uv_mode_factor = 2.0;
		liltoon_like.audio_link.mask_uv_scroll_rotate_factor = [0.01, 0.02, 0.03, 0.04];
		liltoon_like.audio_link.local_map_texture_index = Some(3);
		liltoon_like.audio_link.local_map_params_factor = [128.0, 2.0, 0.5, 0.0];
		liltoon_like
			.texture_uv_offset_scales
			.insert("_AudioLinkMask".to_string(), [0.1, 0.2, 0.3, 0.4]);
		let mat = UnaMaterialPbr {
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);

		assert_eq!(draw.audio_link_params, [1.0, 2.0, 1.0, 0.75]);
		assert_eq!(draw.audio_link_default, [0.4, 0.5, 3.0, 0.2]);
		assert_eq!(draw.audio_link_uv_params, [0.6, 0.1, 0.25, 0.75]);
		assert_eq!(draw.audio_link_start, [1.0, 2.0, 3.0, 0.0]);
		assert_eq!(draw.audio_link_ext, [0.5, 0.25, 1.0, 1.0]);
		assert_eq!(draw.audio_link_vertex_params, [1.0, 3.0, 1.0, 0.0]);
		assert_eq!(draw.audio_link_vertex_uv_params, [0.7, 0.2, 0.3, 0.4]);
		assert_eq!(draw.audio_link_vertex_start, [4.0, 5.0, 6.0, 0.0]);
		assert_eq!(draw.audio_link_vertex_strength, [0.2, 0.0, 0.0, 0.0]);
		assert_eq!(draw.audio_link_mask_params, [2.0, 1.0, 1.0, 0.0]);
		assert_eq!(draw.audio_link_mask_uv_offset_scale, [0.1, 0.2, 0.3, 0.4]);
		assert_eq!(draw.audio_link_mask_uv_anim_params, [0.01, 0.02, 0.03, 0.04]);
		assert_eq!(draw.audio_link_local_map_params, [128.0, 2.0, 0.5, 0.0]);
	}

	#[test]
	fn liltoon_audio_link_texture_need_requires_enabled_liltoon_target() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.audio_link.enabled_factor = 1.0;
		liltoon_like.audio_link.to_emission_factor = 1.0;
		let mat = UnaMaterialPbr {
			shading: UnaShadingModel::LilToonLike,
			liltoon_like: Some(liltoon_like.clone()),
			..Default::default()
		};
		assert!(material_needs_audio_link_texture(&mat, UnaShadingModel::LilToonLike));
		assert!(!material_needs_audio_link_texture(&mat, UnaShadingModel::MToonLike));

		liltoon_like.audio_link.enabled_factor = 0.0;
		let disabled = UnaMaterialPbr {
			shading: UnaShadingModel::LilToonLike,
			liltoon_like: Some(liltoon_like.clone()),
			..Default::default()
		};
		assert!(!material_needs_audio_link_texture(&disabled, UnaShadingModel::LilToonLike));

		liltoon_like.audio_link.enabled_factor = 1.0;
		liltoon_like.audio_link.to_emission_factor = 0.0;
		liltoon_like.audio_link.to_main_second_factor = 0.0;
		liltoon_like.audio_link.to_main_third_factor = 0.0;
		liltoon_like.audio_link.to_emission_gradation_factor = 0.0;
		liltoon_like.audio_link.to_emission_second_factor = 0.0;
		liltoon_like.audio_link.to_emission_second_gradation_factor = 0.0;
		liltoon_like.audio_link.to_vertex_factor = 0.0;
		let no_target = UnaMaterialPbr {
			shading: UnaShadingModel::LilToonLike,
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};
		assert!(!material_needs_audio_link_texture(&no_target, UnaShadingModel::LilToonLike));
	}

	#[test]
	fn liltoon_as_unlit_reaches_draw_uniform() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.rendering.as_unlit_factor = 0.4;
		let mat = UnaMaterialPbr {
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);

		assert_eq!(draw.rendering_ext_params[2], 0.4);
	}

	#[test]
	fn liltoon_source_reflection_cube_flag_reaches_draw_uniform() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.reflection.enabled_factor = 1.0;
		liltoon_like.reflection.cube_texture_index = Some(7);
		let mat = UnaMaterialPbr {
			liltoon_like: Some(liltoon_like.clone()),
			..Default::default()
		};
		let without_override = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);
		assert_eq!(without_override.rendering_ext_params[1], 0.0);

		liltoon_like.reflection.cube_override_factor = 1.0;
		let mat = UnaMaterialPbr {
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};
		let with_override = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);
		assert_eq!(with_override.rendering_ext_params[1], 1.0);
	}

	#[test]
	fn liltoon_flip_backface_normal_reaches_draw_uniform() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.flip_backface_normal_factor = 1.0;
		let mat = UnaMaterialPbr {
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);

		assert_eq!(draw.material_ext_params, [1.0, 0.0, 0.0, 0.0]);
	}

	#[test]
	fn liltoon_shadow_post_ao_reaches_draw_uniform() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.shadow.enabled_factor = 0.0;
		liltoon_like.shadow.strength_factor = 1.0;
		liltoon_like.shadow.post_ao_factor = 1.0;
		liltoon_like.shadow.ao_shift_factor = [3.0, 0.1, 2.0, 0.2];
		liltoon_like.shadow.ao_shift2_factor = [1.5, 0.3, 0.0, 0.0];
		let mat = UnaMaterialPbr {
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);

		assert_eq!(draw.shadow_params[0], 0.0);
		assert_eq!(draw.shadow_params[1], 0.0);
		assert_eq!(draw.shadow_ao_params, [1.0, 0.0, 0.0, 0.0]);
		assert_eq!(draw.shadow_ao_shift, [3.0, 0.1, 2.0, 0.2]);
		assert_eq!(draw.shadow_ao_shift2, [1.5, 0.3, 0.0, 0.0]);
	}

	#[test]
	fn liltoon_transparent_prepass_params_reach_draw_uniform() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.blend_state.subpass_cutoff_factor = 0.4;
		liltoon_like.blend_state.pre_cutoff_factor = 0.3;
		liltoon_like.blend_state.pre_cull_factor = 1.0;
		let mat = UnaMaterialPbr {
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);

		assert_eq!(draw.alpha_ext_params, [0.4, 1.0, 0.3, 1.0]);
	}

	#[test]
	fn liltoon_mask_uv_transforms_reach_draw_uniform() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like
			.texture_uv_offset_scales
			.insert("_ShadowColorTex".to_string(), [0.01, 0.02, 1.01, 1.02]);
		liltoon_like
			.texture_uv_offset_scales
			.insert("_RimColorTex".to_string(), [0.03, 0.04, 1.03, 1.04]);
		liltoon_like
			.texture_uv_offset_scales
			.insert("_EmissionMap".to_string(), [0.05, 0.06, 1.05, 1.06]);
		liltoon_like.texture_uv_mode_factors.insert("_EmissionMap".to_string(), 4.0);
		liltoon_like
			.texture_uv_offset_scales
			.insert("_Emission2ndMap".to_string(), [0.15, 0.16, 1.15, 1.16]);
		liltoon_like.texture_uv_mode_factors.insert("_Emission2ndMap".to_string(), 3.0);
		liltoon_like
			.texture_uv_offset_scales
			.insert("_ReflectionColorTex".to_string(), [0.07, 0.08, 1.07, 1.08]);
		liltoon_like
			.texture_uv_offset_scales
			.insert("_SmoothnessTex".to_string(), [0.09, 0.10, 1.09, 1.10]);
		liltoon_like
			.texture_uv_offset_scales
			.insert("_MetallicGlossMap".to_string(), [0.11, 0.12, 1.11, 1.12]);
		liltoon_like
			.texture_uv_offset_scales
			.insert("_ShadowStrengthMask".to_string(), [0.1, 0.2, 1.1, 1.2]);
		liltoon_like
			.texture_uv_offset_scales
			.insert("_ShadowBorderMask".to_string(), [0.3, 0.4, 1.3, 1.4]);
		liltoon_like
			.texture_uv_offset_scales
			.insert("_ShadowBlurMask".to_string(), [0.5, 0.6, 1.5, 1.6]);
		liltoon_like
			.texture_uv_offset_scales
			.insert("_MatCapBlendMask".to_string(), [0.7, 0.8, 1.7, 1.8]);
		liltoon_like
			.texture_uv_offset_scales
			.insert("_MatCap2ndBlendMask".to_string(), [0.9, 1.0, 1.9, 2.0]);
		liltoon_like
			.texture_uv_offset_scales
			.insert("_AlphaMask".to_string(), [1.1, 1.2, 2.1, 2.2]);
		let mat = UnaMaterialPbr {
			liltoon_like: Some(liltoon_like),
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);

		assert_eq!(draw.shade_uv_offset_scale, [0.01, 0.02, 1.01, 1.02]);
		assert_eq!(draw.rim_uv_offset_scale, [0.03, 0.04, 1.03, 1.04]);
		assert_eq!(draw.emission_uv_offset_scale, [0.05, 0.06, 1.05, 1.06]);
		assert_eq!(draw.emission_uv_anim_params[3], 4.0);
		assert_eq!(draw.emission2nd_uv_offset_scale, [0.15, 0.16, 1.15, 1.16]);
		assert_eq!(draw.emission2nd_uv_anim_params[3], 3.0);
		assert_eq!(draw.reflection_color_uv_offset_scale, [0.07, 0.08, 1.07, 1.08]);
		assert_eq!(draw.smoothness_uv_offset_scale, [0.09, 0.10, 1.09, 1.10]);
		assert_eq!(draw.metallic_uv_offset_scale, [0.11, 0.12, 1.11, 1.12]);
		assert_eq!(draw.shadow_strength_mask_uv_offset_scale, [0.1, 0.2, 1.1, 1.2]);
		assert_eq!(draw.shadow_border_mask_uv_offset_scale, [0.3, 0.4, 1.3, 1.4]);
		assert_eq!(draw.shadow_blur_mask_uv_offset_scale, [0.5, 0.6, 1.5, 1.6]);
		assert_eq!(draw.matcap_blend_mask_uv_offset_scale, [0.7, 0.8, 1.7, 1.8]);
		assert_eq!(draw.matcap2_blend_mask_uv_offset_scale, [0.9, 1.0, 1.9, 2.0]);
		assert_eq!(draw.alpha_mask_uv_offset_scale, [1.1, 1.2, 2.1, 2.2]);
	}

	#[test]
	fn liltoon_alpha_mask_replace_without_texture_uses_white_fallback() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.alpha_mask.mode_factor = 1.0;
		liltoon_like.alpha_mask.scale_factor = 1.0;
		liltoon_like.alpha_mask.value_factor = 0.13;
		let mat = UnaMaterialPbr {
			liltoon_like: Some(liltoon_like),
			alpha_mode: UnaAlphaMode::Blend,
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);

		assert_eq!(draw.alpha_mask_params, [1.0, 1.0, 0.13, 1.0]);
	}

	#[test]
	fn liltoon_alpha_mask_multiply_without_texture_uses_white_fallback() {
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.alpha_mask.mode_factor = 2.0;
		liltoon_like.alpha_mask.scale_factor = 1.0;
		liltoon_like.alpha_mask.value_factor = -1.0;
		let mat = UnaMaterialPbr {
			liltoon_like: Some(liltoon_like),
			alpha_mode: UnaAlphaMode::Blend,
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);

		assert_eq!(draw.alpha_mask_params[0], 2.0);
	}

	#[test]
	fn fully_transparent_blend_material_is_skipped_unless_debug_overrides_it() {
		let mat = UnaMaterialPbr {
			name: Some("toumei".to_string()),
			alpha_mode: UnaAlphaMode::Blend,
			base_color_factor: [1.0, 1.0, 1.0, 0.0],
			..Default::default()
		};
		assert!(material_is_fully_invisible_for_draw(&mat, &SceneMeshLoadOpts::default()));
		assert!(!material_is_fully_invisible_for_draw(
			&mat,
			&SceneMeshLoadOpts {
				debug_primitive_colors: true,
				..Default::default()
			}
		));
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

	#[test]
	fn cull_mode_sets_shader_flags() {
		let opts = SceneMeshLoadOpts::default();
		let mtoon = UnaMtoonMaterial::default();

		let off = mesh_draw_material_gpu(
			&UnaMaterialPbr {
				cull_mode: UnaCullMode::Off,
				..Default::default()
			},
			&mtoon,
			&opts,
			0,
			0,
		);
		assert_ne!(off.params[3].to_bits() & 512, 0);
		assert_eq!(off.params[3].to_bits() & 2048, 0);

		let front = mesh_draw_material_gpu(
			&UnaMaterialPbr {
				cull_mode: UnaCullMode::Front,
				..Default::default()
			},
			&mtoon,
			&opts,
			0,
			0,
		);
		assert_eq!(front.params[3].to_bits() & 512, 0);
		assert_ne!(front.params[3].to_bits() & 2048, 0);

		let back = mesh_draw_material_gpu(
			&UnaMaterialPbr {
				cull_mode: UnaCullMode::Back,
				..Default::default()
			},
			&mtoon,
			&opts,
			0,
			0,
		);
		assert_eq!(back.params[3].to_bits() & 512, 0);
		assert_eq!(back.params[3].to_bits() & 2048, 0);
	}
}
