//! glTF / [`UnaSceneSnapshot`] 由来のメッシュ描画（スキニング・モーフ・シェーディング種別）。

use std::{borrow::Cow, collections::BTreeMap};

use glam::{Mat4, Vec2, Vec3, Vec4};
use half::f16;
use serde::Serialize;
use un_avatar_core::{
	UnaAlphaMode, UnaBounds, UnaCullMode, UnaExpressionCatalog, UnaExpressionWeights, UnaImageRgba, UnaImageSourceMetadata, UnaMaterialPbr,
	UnaMeshBuffers, UnaMtoonMaterial, UnaMtoonOutlineWidthMode, UnaSceneSnapshot, UnaShadingModel, UnaTextureFilterMode, UnaTextureSampler,
	UnaTextureWrapMode,
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
	mip_level_count, read_compressed_texture_cache, source_texture_upload, texture_cache_key, texture_cache_key_from_source_metadata,
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
}

use wgpu::util::DeviceExt;

const SHADER_MESH: &str = include_str!("../shaders/mesh.wgsl");
const SHADER_COMPUTE_FUR_CARDS: &str = include_str!("../shaders/compute_fur_cards.wgsl");

fn mesh_shader_source_for_tier(variant_tier: MeshShaderVariantTier) -> Cow<'static, str> {
	match variant_tier {
		MeshShaderVariantTier::HighCapability => Cow::Borrowed(SHADER_MESH),
		MeshShaderVariantTier::BaselineFallback => Cow::Owned(baseline_fallback_mesh_shader_source()),
	}
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
		"\tif (drawu.normal2nd_params.x > 0.5) {\n\t\tlet normal2nd_base_uv = lil_select_uv(drawu.normal2nd_params.z, uv, uv1, uv2, uv3);\n\t\tlet normal2nd_uv = normal2nd_base_uv * drawu.normal2nd_uv_offset_scale.zw + drawu.normal2nd_uv_offset_scale.xy;\n\t\tlet normal2nd_scale_mask_uv = uv * drawu.normal2nd_scale_mask_uv_offset_scale.zw + drawu.normal2nd_scale_mask_uv_offset_scale.xy;\n\t\tlet scale_mask = textureSample(normal2nd_scale_mask_tex, base_samp, normal2nd_scale_mask_uv).r;\n\t\tlet tn2 = lil_unpack_normal_scale(textureSample(normal2nd_tex, normal_samp, normal2nd_uv), drawu.normal2nd_params.y * scale_mask);\n\t\ttn = vec3<f32>(tn.xy + tn2.xy, tn.z * tn2.z);\n\t}\n",
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

const _: () = assert!(std::mem::size_of::<Vertex>() == 112);

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct TextureUploadSummary {
	pub image_count: u32,
	pub resized_count: u32,
	pub cubemap_count: u32,
	pub cubemap_converted_count: u32,
	pub cubemap_fallback_count: u32,
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
	default_morph_weights: Vec<f32>,
}

#[derive(Clone, Debug)]
struct ExpressionBinding {
	preset_index: usize,
	morph_target_index: usize,
	weight_scale: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

fn append_ordered_draw_batch(batches: &mut Vec<DrawBatch>, pipeline: DrawPipelineKind, draw_index: usize, batch_capacity: usize) {
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

fn transparent_backpass_pipeline_for_draw(draw: &MeshDraw) -> DrawPipelineKind {
	let zwrite = draw
		.material
		.liltoon_like_source_profile()
		.is_none_or(|u| u.blend_state.pre_zwrite_factor > 0.5);
	if zwrite {
		DrawPipelineKind::TransparentToonBackpass
	} else {
		DrawPipelineKind::TransparentToonBackpassNoZWrite
	}
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
	raw: Vec<f32>,
	uploaded: Vec<f32>,
	uploaded_changed: bool,
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
	bind_outline_material: wgpu::BindGroup,
	skin_palette_index: usize,
	skin_palette_static_identity: bool,
	_morph_meta_buffer: wgpu::Buffer,
	morph_weight_buffer: wgpu::Buffer,
	_morph_delta_buffer: wgpu::Buffer,
	morph_bind_group: wgpu::BindGroup,
	_compute_fur_cards: Option<ComputeFurCardsDrawResources>,
	world_node_index: usize,
	active: bool,
	shading: UnaShadingModel,
	morph_pos: Vec<Vec<[f32; 3]>>,
	default_morph_weights: Vec<f32>,
	expression_bindings: Vec<ExpressionBinding>,
	morph_weights: Vec<f32>,
	morph_weight_scratch: Vec<f32>,
	alpha_mode: UnaAlphaMode,
	material: UnaMaterialPbr,
	mesh_index: usize,
	primitive_index: usize,
	probe_anchor_node: Option<usize>,
	local_bounds: Option<UnaBounds>,
	world_origin: Vec3,
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
	base_vertices: Vec<Vertex>,
	source_vertex_scratch: Vec<ComputeFurCardsSourceVertexGpu>,
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
}

impl SceneMeshRuntimeRequirements {
	fn include(&mut self, other: Self) {
		self.audio_link_texture |= other.audio_link_texture;
		self.screen_refraction |= other.screen_refraction;
		self.fur |= other.fur;
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
	needs_screen_refraction: bool,
	active_skin_palette_indices: Vec<usize>,
	runtime_requirements: SceneMeshRuntimeRequirements,
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
	if matches!(shading, UnaShadingModel::MToonLike | UnaShadingModel::LilToonLike) && liltoon_uses_additive_color_blend(&draw.material) {
		if zwrite {
			DrawPipelineKind::BlendToonAddZWrite
		} else {
			DrawPipelineKind::BlendToonAdd
		}
	} else if zwrite && matches!(shading, UnaShadingModel::MToonLike | UnaShadingModel::LilToonLike) {
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
	material.liltoon_like_runtime().is_some_and(|u| {
		(u.source_profile == un_avatar_core::UnaLilToonLikeSourceProfile::LiltoonGem
			|| u.source_profile == un_avatar_core::UnaLilToonLikeSourceProfile::LiltoonRefraction)
			&& u.reflection.gem_refraction_strength_factor.abs() > 0.00001
	})
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
	if shading != UnaShadingModel::LilToonLike {
		return false;
	}
	material.liltoon_like_runtime().is_some_and(|liltoon_like| {
		liltoon_like.audio_link.enabled_factor > 0.5 && liltoon_audio_link_has_active_target(&liltoon_like.audio_link)
	})
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
	}
}

fn draw_uses_screen_refraction_grab(draw: &MeshDraw) -> bool {
	material_needs_screen_refraction(&draw.material)
}

fn material_uses_liltoon_gem_prepass(material: &UnaMaterialPbr) -> bool {
	material
		.liltoon_like_runtime()
		.is_some_and(|u| u.source_profile == un_avatar_core::UnaLilToonLikeSourceProfile::LiltoonGem)
}

fn draw_uses_liltoon_gem_prepass(draw: &MeshDraw) -> bool {
	material_uses_liltoon_gem_prepass(&draw.material)
}

fn liltoon_reflection_texture_index(liltoon_like: &un_avatar_core::UnaLilToonLikeMaterial) -> Option<usize> {
	let use_source_cube = liltoon_like.reflection.cube_override_factor > 0.5
		|| liltoon_like.source_profile == un_avatar_core::UnaLilToonLikeSourceProfile::LiltoonGem;
	use_source_cube.then_some(liltoon_like.reflection.cube_texture_index).flatten()
}

fn transparent_backpass_enabled(
	alpha_mode: UnaAlphaMode,
	transparent_with_z_write: bool,
	shading: UnaShadingModel,
	liltoon_backpass_enabled: bool,
) -> bool {
	alpha_mode == UnaAlphaMode::Blend
		&& transparent_with_z_write
		&& liltoon_backpass_enabled
		&& matches!(shading, UnaShadingModel::MToonLike | UnaShadingModel::LilToonLike)
}

fn draw_uses_transparent_backpass(draw: &MeshDraw, shading: UnaShadingModel) -> bool {
	let liltoon_backpass_enabled = draw
		.material
		.liltoon_like_runtime()
		.is_none_or(|u| u.blend_state.pre_zwrite_factor > 0.5);
	transparent_backpass_enabled(
		draw.alpha_mode,
		draw.material
			.mtoon_source_profile()
			.is_some_and(|mtoon| mtoon.transparent_with_z_write),
		shading,
		liltoon_backpass_enabled,
	)
}

fn transparent_forward_zwrite_enabled(alpha_mode: UnaAlphaMode, transparent_with_z_write: bool, shading: UnaShadingModel) -> bool {
	alpha_mode == UnaAlphaMode::Blend
		&& transparent_with_z_write
		&& matches!(shading, UnaShadingModel::MToonLike | UnaShadingModel::LilToonLike)
}

fn build_draw_order(draws: &[MeshDraw], opts: &SceneMeshLoadOpts) -> SceneMeshDrawState {
	let mut state = SceneMeshDrawState {
		outline_draw_indices: Vec::with_capacity(draws.len()),
		fur_draw_indices: Vec::with_capacity(draws.len()),
		opaque_batches: Vec::new(),
		transparent_backpass_draw_indices: Vec::with_capacity(draws.len()),
		blended_batches: Vec::new(),
		active_draw_indices: Vec::with_capacity(draws.len()),
		needs_screen_refraction: false,
		active_skin_palette_indices: Vec::with_capacity(draws.len()),
		runtime_requirements: SceneMeshRuntimeRequirements::default(),
	};
	let batch_capacity = (draws.len() / 10).max(1);
	let mut opaque_batches = vec![
		draw_batch(DrawPipelineKind::OpaqueLit, batch_capacity),
		draw_batch(DrawPipelineKind::OpaqueUnlit, batch_capacity),
		draw_batch(DrawPipelineKind::OpaqueToon, batch_capacity),
		draw_batch(DrawPipelineKind::OpaqueToon, batch_capacity),
		draw_batch(DrawPipelineKind::OpaqueLit, batch_capacity),
		draw_batch(DrawPipelineKind::OpaqueUnlit, batch_capacity),
		draw_batch(DrawPipelineKind::OpaqueToon, batch_capacity),
		draw_batch(DrawPipelineKind::OpaqueToon, batch_capacity),
	];
	let mut blended_draws = Vec::with_capacity(draws.len());
	let mut blended_batches = Vec::with_capacity(batch_capacity);

	for (draw_index, draw) in draws.iter().enumerate() {
		if !draw.active {
			continue;
		}
		state.active_draw_indices.push(draw_index);
		if !draw.skin_palette_static_identity {
			state.active_skin_palette_indices.push(draw.skin_palette_index);
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

		let shading_index = match shading {
			UnaShadingModel::LitLambert => 0,
			UnaShadingModel::Unlit => 1,
			UnaShadingModel::MToonLike => 2,
			UnaShadingModel::LilToonLike => 3,
		};
		match draw.alpha_mode {
			UnaAlphaMode::Opaque | UnaAlphaMode::Mask
				if draw_uses_screen_refraction_grab(draw)
					|| draw_uses_late_non_blend_queue(draw.alpha_mode, draw_render_queue_number(draw)) =>
			{
				blended_draws.push((opaque_pipeline_for_shading(shading), draw_index));
			}
			UnaAlphaMode::Opaque => opaque_batches[shading_index].draw_indices.push(draw_index),
			UnaAlphaMode::Mask => opaque_batches[4 + shading_index].draw_indices.push(draw_index),
			UnaAlphaMode::Blend if draw_uses_transparent_backpass(draw, shading) => {
				blended_draws.push((transparent_backpass_pipeline_for_draw(draw), draw_index));
				if draw_uses_liltoon_gem_prepass(draw) {
					blended_draws.push((DrawPipelineKind::LilToonGemPre, draw_index));
				}
				blended_draws.push((blend_pipeline_for_draw(draw, shading, true), draw_index));
			}
			UnaAlphaMode::Blend => {
				if draw_uses_liltoon_gem_prepass(draw) {
					blended_draws.push((DrawPipelineKind::LilToonGemPre, draw_index));
				}
				blended_draws.push((
					blend_pipeline_for_draw(
						draw,
						shading,
						transparent_forward_zwrite_enabled(
							draw.alpha_mode,
							draw.material
								.mtoon_source_profile()
								.is_some_and(|mtoon| mtoon.transparent_with_z_write),
							shading,
						),
					),
					draw_index,
				));
			}
		}
	}
	blended_draws.sort_by_key(|&(pipeline, draw_index)| {
		let (render_queue, draw_index) = draw_render_order_key(draws, draw_index);
		(render_queue, draw_index, blended_pipeline_pass_order(pipeline))
	});
	for (pipeline, draw_index) in blended_draws {
		append_ordered_draw_batch(&mut blended_batches, pipeline, draw_index, batch_capacity);
	}

	group_draw_indices_by_skin_palette(draws, &mut state.outline_draw_indices);
	group_draw_indices_by_skin_palette(draws, &mut state.fur_draw_indices);
	group_draw_indices_by_skin_palette(draws, &mut state.transparent_backpass_draw_indices);
	for batch in &mut opaque_batches {
		group_draw_indices_by_skin_palette(draws, &mut batch.draw_indices);
	}

	opaque_batches.retain(|batch| !batch.draw_indices.is_empty());
	state.opaque_batches = opaque_batches;
	state.blended_batches = blended_batches;
	state.active_skin_palette_indices.sort_unstable();
	state.active_skin_palette_indices.dedup();
	state
}

pub(crate) struct SceneMeshes {
	pipeline_outline_toon: wgpu::RenderPipeline,
	_compute_fur_cards_compute_pipeline: ComputeFurCardsComputePipeline,
	pipeline_compute_fur_cards_pre_toon: wgpu::RenderPipeline,
	pipeline_compute_fur_cards_toon: wgpu::RenderPipeline,
	pipeline_opaque_lit: wgpu::RenderPipeline,
	pipeline_opaque_unlit: wgpu::RenderPipeline,
	pipeline_opaque_toon: wgpu::RenderPipeline,
	pipeline_transparent_toon_backpass: wgpu::RenderPipeline,
	pipeline_transparent_toon_backpass_no_zwrite: wgpu::RenderPipeline,
	pipeline_blend_lit: wgpu::RenderPipeline,
	pipeline_blend_unlit: wgpu::RenderPipeline,
	pipeline_blend_toon: wgpu::RenderPipeline,
	pipeline_blend_toon_zwrite: wgpu::RenderPipeline,
	pipeline_blend_toon_add: wgpu::RenderPipeline,
	pipeline_blend_toon_add_zwrite: wgpu::RenderPipeline,
	pipeline_liltoon_gem_pre_toon: wgpu::RenderPipeline,
	frame_buffer: wgpu::Buffer,
	frame_uploaded: Option<MeshFrameGpu>,
	frame_layout: wgpu::BindGroupLayout,
	frame_bind_group: wgpu::BindGroup,
	screen_grab_sampler: wgpu::Sampler,
	_screen_grab_fallback_texture: wgpu::Texture,
	_audio_link_texture: wgpu::Texture,
	audio_link_view: wgpu::TextureView,
	audio_link_uploaded_sequence: u64,
	audio_link_frame_params: [f32; 4],
	#[allow(dead_code)]
	_samplers: Vec<wgpu::Sampler>,
	#[allow(dead_code)]
	_textures: Vec<wgpu::Texture>,
	#[allow(dead_code)]
	_cube_textures: Vec<wgpu::Texture>,
	draws: Vec<MeshDraw>,
	skin_palettes: Vec<SkinPalette>,
	outline_draw_indices: Vec<usize>,
	fur_draw_indices: Vec<usize>,
	opaque_batches: Vec<DrawBatch>,
	transparent_backpass_draw_indices: Vec<usize>,
	blended_batches: Vec<DrawBatch>,
	active_draw_indices: Vec<usize>,
	needs_screen_refraction: bool,
	active_skin_palette_indices: Vec<usize>,
	texture_summary: TextureUploadSummary,
	runtime_requirements: SceneMeshRuntimeRequirements,
	visibility_scratch: Vec<bool>,
	expression_names: Vec<String>,
	expression_value_scratch: Vec<f32>,
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

fn expand_primitive(buf: &UnaMeshBuffers, bake_static_default_morphs: bool) -> Option<ExpandedPrimitive> {
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
		let mut pos = positions[pi];
		let mut n = normals.and_then(|nn| nn.get(pi)).copied().unwrap_or(default_n);
		if bake_static_default_morphs {
			for (target_index, target) in buf.morph_targets.iter().enumerate() {
				let weight = default_morph_weight_for(buf, target_index);
				if weight.abs() <= 0.000001 {
					continue;
				}
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
	fill_missing_tangents(&mut verts, &indices);

	Some(ExpandedPrimitive {
		verts,
		indices,
		morph_pos: morph_push,
		morph_nrm: morph_nrm_push,
		default_morph_weights: if bake_static_default_morphs {
			vec![0.0; num_morph]
		} else {
			buf.default_morph_weights.clone()
		},
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

fn scene_default_morph_weights_for_draw(
	scene: &UnaSceneSnapshot,
	mesh_index: usize,
	primitive_index: usize,
	target_count: usize,
) -> Vec<f32> {
	let mut out = vec![0.0; target_count];
	let Some(primitive) = scene.meshes.get(mesh_index).and_then(|mesh| mesh.get(primitive_index)) else {
		return out;
	};
	let copy_len = primitive.default_morph_weights.len().min(target_count);
	for (dst, src) in out
		.iter_mut()
		.take(copy_len)
		.zip(primitive.default_morph_weights.iter().take(copy_len))
	{
		*dst = src.clamp(0.0, 1.0);
	}
	out
}

fn refresh_morph_default_weights(
	default_morph_weights: &mut Vec<f32>,
	uploaded_morph_weights: &mut Vec<f32>,
	scene: &UnaSceneSnapshot,
	mesh_index: usize,
	primitive_index: usize,
	target_count: usize,
) -> bool {
	let next = scene_default_morph_weights_for_draw(scene, mesh_index, primitive_index, target_count);
	if *default_morph_weights == next {
		return false;
	}
	*default_morph_weights = next;
	uploaded_morph_weights.clear();
	true
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
		UnaTextureWrapMode::ClampToEdge => wgpu::AddressMode::ClampToEdge,
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

struct CubeUpload {
	face_size: u32,
	mips: Vec<CubeMipUpload>,
	layout: &'static str,
}

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
	let width = image.width.max(1);
	let height = image.height.max(1);
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

fn cube_upload_from_image(image: &UnaImageRgba, source: Option<&UnaImageSourceMetadata>) -> Option<CubeUpload> {
	let (layout, face_size) = cube_source_layout(image, source)?;
	let srgb = texture_source_is_srgb(source);
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
	Some(CubeUpload {
		face_size,
		mips: build_cube_mips_rgba16f(face_size, base_rgba),
		layout: layout.name(),
	})
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
			if d.shading == UnaShadingModel::LilToonLike {
				return d
					.material
					.liltoon_like_runtime()
					.is_some_and(|material| material.outline.enabled_factor > 0.5 && material.outline.width_factor > 0.0);
			}
			d.shading == UnaShadingModel::MToonLike
				&& d.material
					.mtoon_like_runtime()
					.is_some_and(|mtoon| effective_mtoon_outline(mtoon, opts).is_some())
		}
		AvatarOutlinePolicy::Off => false,
	}
}

fn material_fur_layer_count(material: &UnaMaterialPbr, shading: UnaShadingModel) -> u32 {
	if shading != UnaShadingModel::LilToonLike {
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
fn compute_fur_cards_source_vertex_from_vertex(vertex: Vertex) -> ComputeFurCardsSourceVertexGpu {
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
	verts.iter().copied().map(compute_fur_cards_source_vertex_from_vertex).collect()
}

fn compute_fur_cards_palette_matrix(raw: &[f32], joint_index: u16) -> Mat4 {
	let matrix_count = raw.len() / 16;
	if matrix_count == 0 {
		return Mat4::IDENTITY;
	}
	let joint_index = (joint_index as usize).min(matrix_count - 1);
	let base = joint_index * 16;
	Mat4::from_cols_array(&raw[base..base + 16].try_into().expect("palette matrix slice length"))
}

fn compute_fur_cards_skinned_source_vertex_from_vertex(vertex: Vertex, palette_raw: &[f32]) -> ComputeFurCardsSourceVertexGpu {
	let position = Vec3::from_array(vertex.pos);
	let normal = Vec3::from_array(vertex.norm);
	let tangent = Vec3::new(vertex.tangent[0], vertex.tangent[1], vertex.tangent[2]);
	let mut skinned_position = Vec3::ZERO;
	let mut skinned_normal = Vec3::ZERO;
	let mut skinned_tangent = Vec3::ZERO;
	for i in 0..4 {
		let weight = vertex.weights[i];
		if weight.abs() <= 0.000001 {
			continue;
		}
		let matrix = compute_fur_cards_palette_matrix(palette_raw, vertex.joints[i]);
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
	ComputeFurCardsSourceVertexGpu {
		position: [skinned_position.x, skinned_position.y, skinned_position.z, 1.0],
		normal: [
			skinned_normal.normalize_or_zero().x,
			skinned_normal.normalize_or_zero().y,
			skinned_normal.normalize_or_zero().z,
			0.0,
		],
		tangent: [
			skinned_tangent.normalize_or_zero().x,
			skinned_tangent.normalize_or_zero().y,
			skinned_tangent.normalize_or_zero().z,
			vertex.tangent[3],
		],
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

fn compute_fur_cards_skinned_source_vertices_from_mesh(
	verts: &[Vertex],
	palette_raw: &[f32],
	out: &mut Vec<ComputeFurCardsSourceVertexGpu>,
) {
	out.clear();
	out.reserve(verts.len());
	out.extend(
		verts
			.iter()
			.copied()
			.map(|vertex| compute_fur_cards_skinned_source_vertex_from_vertex(vertex, palette_raw)),
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

#[allow(dead_code)]
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
	let liltoon_fur = material.liltoon_like_source_profile().map(|liltoon_like| &liltoon_like.fur);
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
		cache: None,
	});
	ComputeFurCardsComputePipeline {
		_bind_group_layout: bind_group_layout.clone(),
		_pipeline: pipeline,
	}
}

#[allow(dead_code)]
fn compute_fur_cards_cards_per_triangle_for_material(material: &UnaMaterialPbr) -> u32 {
	material
		.liltoon_like_source_profile()
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
	let fur = material.liltoon_like_source_profile().map(|liltoon_like| &liltoon_like.fur);
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
	indices: &[u32],
	cpu_maps: ComputeFurCardsCpuFurMaps<'_>,
	fur_vector_view: &wgpu::TextureView,
	fur_length_mask_view: &wgpu::TextureView,
	fur_noise_mask_view: &wgpu::TextureView,
	fur_mask_view: &wgpu::TextureView,
	fur_sampler: &wgpu::Sampler,
) -> Option<ComputeFurCardsDrawResources> {
	let source_vertices = compute_fur_cards_source_vertices_from_mesh(verts);
	let source_triangles = compute_fur_cards_source_triangles_from_indices(indices, source_vertices.len());
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
		base_vertices: verts.to_vec(),
		source_vertex_scratch: Vec::with_capacity(verts.len()),
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
	match mat.cull_mode {
		UnaCullMode::Off => flags |= 512,
		UnaCullMode::Front => flags |= 2048,
		UnaCullMode::Back => {}
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
	let liltoon_like = mat.liltoon_like_source_profile();
	if liltoon_like.is_some() {
		flags |= 4096;
	}
	if liltoon_like
		.map(|u| u.source_profile == un_avatar_core::UnaLilToonLikeSourceProfile::LiltoonGem)
		.unwrap_or(false)
	{
		flags |= 8192;
	}
	if liltoon_like
		.map(|u| u.source_profile == un_avatar_core::UnaLilToonLikeSourceProfile::LiltoonRefraction)
		.unwrap_or(false)
	{
		flags |= 16384;
	}
	if liltoon_uses_additive_color_blend(mat) {
		flags |= 32768;
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
			if u.source_profile == un_avatar_core::UnaLilToonLikeSourceProfile::LiltoonRefraction {
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
			if u.source_profile == un_avatar_core::UnaLilToonLikeSourceProfile::LiltoonRefraction {
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
				u.parallax
					.enabled_factor
					.max(if u.parallax.texture_index.is_some() { 1.0 } else { 0.0 })
					.clamp(0.0, 1.0),
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
			let flag_sum = u.id_mask.flags_factor.iter().copied().sum::<f32>()
				+ u.id_mask.prior_flags_factor.iter().copied().sum::<f32>()
				+ u.id_mask.controls_dissolve_factor;
			[
				u.id_mask
					.compile_factor
					.max(if flag_sum > 0.0001 { 1.0 } else { 0.0 })
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
			let row_sum = u.udim_discard.row0_factor.iter().copied().sum::<f32>()
				+ u.udim_discard.row1_factor.iter().copied().sum::<f32>()
				+ u.udim_discard.row2_factor.iter().copied().sum::<f32>()
				+ u.udim_discard.row3_factor.iter().copied().sum::<f32>();
			[
				u.udim_discard
					.compile_factor
					.max(if row_sum > 0.0001 { 1.0 } else { 0.0 })
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
		matcap_factor: [
			matcap_color[0],
			matcap_color[1],
			matcap_color[2],
			opts.avatar_matcap.scale.clamp(0.0, 2.0),
		],
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
			(mat.occlusion_texture_strength * opts.avatar_ambient_occlusion.strength).clamp(0.0, 2.0),
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

fn mesh_draw_material_gpu_runtime(
	mat: &UnaMaterialPbr,
	default_mtoon: &UnaMtoonMaterial,
	opts: &SceneMeshLoadOpts,
	mesh_index: usize,
	prim_index: usize,
) -> MeshDrawMaterialGpu {
	let mtoon = mat.mtoon_source_profile().unwrap_or(default_mtoon);
	mesh_draw_material_gpu(mat, mtoon, opts, mesh_index, prim_index)
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
		let alpha_to_coverage_enabled = matches!(label, "mesh_opaque_toon" | "mesh_compute_fur_cards_pre_toon");
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
				alpha_to_coverage_enabled,
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
		shader_variant_tier: MeshShaderVariantTier,
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

		let full_material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
					binding: 9,
					visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
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
						view_dimension: wgpu::TextureViewDimension::Cube,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				sampler_bind_group_layout_entry(14, wgpu::ShaderStages::FRAGMENT),
				sampler_bind_group_layout_entry(15, wgpu::ShaderStages::FRAGMENT),
				sampler_bind_group_layout_entry(16, wgpu::ShaderStages::FRAGMENT),
				sampler_bind_group_layout_entry(17, wgpu::ShaderStages::FRAGMENT),
				sampler_bind_group_layout_entry(18, wgpu::ShaderStages::FRAGMENT),
				sampler_bind_group_layout_entry(19, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
				sampler_bind_group_layout_entry(20, wgpu::ShaderStages::FRAGMENT),
				sampler_bind_group_layout_entry(21, wgpu::ShaderStages::FRAGMENT),
				sampler_bind_group_layout_entry(22, wgpu::ShaderStages::FRAGMENT),
				sampler_bind_group_layout_entry(23, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT),
				wgpu::BindGroupLayoutEntry {
					binding: 24,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 25,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				sampler_bind_group_layout_entry(26, wgpu::ShaderStages::FRAGMENT),
				sampler_bind_group_layout_entry(27, wgpu::ShaderStages::FRAGMENT),
				wgpu::BindGroupLayoutEntry {
					binding: 28,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 29,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 30,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				sampler_bind_group_layout_entry(31, wgpu::ShaderStages::FRAGMENT),
				sampler_bind_group_layout_entry(32, wgpu::ShaderStages::FRAGMENT),
				sampler_bind_group_layout_entry(33, wgpu::ShaderStages::FRAGMENT),
				wgpu::BindGroupLayoutEntry {
					binding: 34,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				sampler_bind_group_layout_entry(35, wgpu::ShaderStages::FRAGMENT),
				wgpu::BindGroupLayoutEntry {
					binding: 36,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				sampler_bind_group_layout_entry(37, wgpu::ShaderStages::FRAGMENT),
				wgpu::BindGroupLayoutEntry {
					binding: 38,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 39,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 41,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 42,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 43,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 44,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 45,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 46,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 47,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 48,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 49,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 50,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 51,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 52,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 53,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 54,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 55,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 56,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 57,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 58,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 59,
					visibility: wgpu::ShaderStages::VERTEX,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 60,
					visibility: wgpu::ShaderStages::VERTEX,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 61,
					visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 62,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 63,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 64,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 65,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 66,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
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
			],
		});
		let baseline_material_entries = mesh_material_layout_entries(MeshShaderVariantTier::BaselineFallback);
		let baseline_material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("mesh_material_baseline_fallback"),
			entries: &baseline_material_entries,
		});
		let material_layout = match shader_variant_tier {
			MeshShaderVariantTier::HighCapability => &full_material_layout,
			MeshShaderVariantTier::BaselineFallback => &baseline_material_layout,
		};
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
		let compute_fur_cards_compute_pipeline = create_compute_fur_cards_compute_pipeline(device, &compute_fur_cards_bind_group_layout);

		let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("mesh"),
			bind_group_layouts: &[
				Some(&frame_layout),
				Some(material_layout),
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

		let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
			label: Some("mesh"),
			source: wgpu::ShaderSource::Wgsl(mesh_shader_source_for_tier(shader_variant_tier)),
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

		let pipeline_outline_toon = Self::create_mesh_pipeline(
			device,
			&outline_pipeline_layout,
			&shader,
			format,
			&vb_layout,
			"mesh_outline_toon",
			"vs_outline",
			"fs_outline",
			None,
			wgpu::ColorWrites::ALL,
			false,
			wgpu::CompareFunction::Less,
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
		let pipeline_opaque_toon = Self::create_mesh_pipeline(
			device,
			&pipeline_layout,
			&shader,
			format,
			&vb_layout,
			"mesh_opaque_toon",
			"vs_main",
			"fs_toon",
			None,
			wgpu::ColorWrites::ALL,
			true,
			wgpu::CompareFunction::LessEqual,
			None,
			sample_count,
		);
		let blend = Some(wgpu::BlendState::ALPHA_BLENDING);
		// lilToon Transparent uses SrcBlend=One, DstBlend=OneMinusSrcAlpha
		// with shader-side premultiply. The lilToon-like v2 path follows the same
		// premultiplied-alpha convention while it still shares this pipeline.
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
		let pipeline_blend_toon = Self::create_mesh_pipeline(
			device,
			&pipeline_layout,
			&shader,
			format,
			&vb_layout,
			"mesh_blend_toon",
			"vs_main",
			"fs_toon",
			premultiplied_blend,
			wgpu::ColorWrites::ALL,
			false,
			wgpu::CompareFunction::LessEqual,
			None,
			sample_count,
		);
		let pipeline_compute_fur_cards_pre_toon = Self::create_mesh_pipeline(
			device,
			&pipeline_layout,
			&shader,
			format,
			&compute_fur_cards_vb_layout,
			"mesh_compute_fur_cards_pre_toon",
			"vs_compute_fur_cards_pre",
			"fs_fur_toon_pre",
			None,
			wgpu::ColorWrites::ALL,
			true,
			wgpu::CompareFunction::LessEqual,
			None,
			sample_count,
		);
		let pipeline_compute_fur_cards_toon = Self::create_mesh_pipeline(
			device,
			&pipeline_layout,
			&shader,
			format,
			&compute_fur_cards_vb_layout,
			"mesh_compute_fur_cards_toon",
			"vs_compute_fur_cards",
			"fs_fur_toon",
			blend,
			wgpu::ColorWrites::ALL,
			false,
			wgpu::CompareFunction::LessEqual,
			None,
			sample_count,
		);
		let pipeline_transparent_toon_backpass = Self::create_mesh_pipeline(
			device,
			&pipeline_layout,
			&shader,
			format,
			&vb_layout,
			"mesh_transparent_toon_backpass",
			"vs_main",
			"fs_toon_backpass",
			premultiplied_blend,
			wgpu::ColorWrites::ALL,
			true,
			wgpu::CompareFunction::LessEqual,
			None,
			sample_count,
		);
		let pipeline_transparent_toon_backpass_no_zwrite = Self::create_mesh_pipeline(
			device,
			&pipeline_layout,
			&shader,
			format,
			&vb_layout,
			"mesh_transparent_toon_backpass_no_zwrite",
			"vs_main",
			"fs_toon_backpass",
			premultiplied_blend,
			wgpu::ColorWrites::ALL,
			false,
			wgpu::CompareFunction::LessEqual,
			None,
			sample_count,
		);
		let pipeline_blend_toon_zwrite = Self::create_mesh_pipeline(
			device,
			&pipeline_layout,
			&shader,
			format,
			&vb_layout,
			"mesh_blend_toon_zwrite",
			"vs_main",
			"fs_toon",
			premultiplied_blend,
			wgpu::ColorWrites::ALL,
			true,
			wgpu::CompareFunction::LessEqual,
			None,
			sample_count,
		);
		let pipeline_blend_toon_add = Self::create_mesh_pipeline(
			device,
			&pipeline_layout,
			&shader,
			format,
			&vb_layout,
			"mesh_blend_toon_add",
			"vs_main",
			"fs_toon",
			additive_toon_blend,
			wgpu::ColorWrites::ALL,
			false,
			wgpu::CompareFunction::LessEqual,
			None,
			sample_count,
		);
		let pipeline_blend_toon_add_zwrite = Self::create_mesh_pipeline(
			device,
			&pipeline_layout,
			&shader,
			format,
			&vb_layout,
			"mesh_blend_toon_add_zwrite",
			"vs_main",
			"fs_toon",
			additive_toon_blend,
			wgpu::ColorWrites::ALL,
			true,
			wgpu::CompareFunction::LessEqual,
			None,
			sample_count,
		);
		let pipeline_liltoon_gem_pre_toon = Self::create_mesh_pipeline(
			device,
			&pipeline_layout,
			&shader,
			format,
			&vb_layout,
			"mesh_liltoon_gem_pre_toon",
			"vs_main",
			"fs_toon_gem_pre",
			gem_pre_blend,
			wgpu::ColorWrites::ALL,
			false,
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
		let scene_texture_base = textures.len();
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

		let mut cube_image_views: Vec<Option<wgpu::TextureView>> = Vec::with_capacity(scene.images.len());
		for (image_index, im) in scene.images.iter().enumerate() {
			let src_w = im.width.max(1);
			let src_h = im.height.max(1);
			let role = texture_roles.get(image_index).copied().unwrap_or_default();
			let source_metadata = scene.image_sources.get(image_index).and_then(Option::as_ref);
			let skin_tone_override = skin_tone_matched_images.get(image_index).and_then(Option::as_deref);
			if texture_source_is_cube(source_metadata) {
				texture_summary.cubemap_count += 1;
			}
			if let Some(cube_upload) = cube_upload_from_image(im, source_metadata) {
				report(
					"gpu-upload",
					format!(
						"Uploading cubemap texture {}/{} face={} mips={} layout={} ({role:?})",
						image_index + 1,
						scene.images.len(),
						cube_upload.face_size,
						cube_upload.mips.len(),
						cube_upload.layout
					),
				);
				let tex = device.create_texture(&wgpu::TextureDescriptor {
					label: Some("gltf_image_cube"),
					size: wgpu::Extent3d {
						width: cube_upload.face_size,
						height: cube_upload.face_size,
						depth_or_array_layers: 6,
					},
					mip_level_count: cube_upload.mips.len() as u32,
					sample_count: 1,
					dimension: wgpu::TextureDimension::D2,
					format: wgpu::TextureFormat::Rgba16Float,
					usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
					view_formats: &[],
				});
				for (mip_level, mip) in cube_upload.mips.iter().enumerate() {
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
				texture_summary.cubemap_converted_count += 1;
				texture_summary.cubemap_uploaded_bytes += cube_upload.mips.iter().map(|mip| mip.data_rgba16f.len() as u64).sum::<u64>();
				let view = tex.create_view(&wgpu::TextureViewDescriptor {
					label: Some("gltf_image_cube_view"),
					dimension: Some(wgpu::TextureViewDimension::Cube),
					..Default::default()
				});
				cube_textures.push(tex);
				cube_image_views.push(Some(view));
			} else {
				if texture_source_is_cube(source_metadata) {
					texture_summary.cubemap_fallback_count += 1;
					report(
						"gpu-upload",
						format!(
							"Cubemap texture {}/{} has unsupported source layout {:?}; using black cube fallback",
							image_index + 1,
							scene.images.len(),
							source_metadata.and_then(|source| source.source_layout.as_deref())
						),
					);
				}
				cube_image_views.push(None);
			}
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
				TextureUploadKind::Rgba if rgba_upload_uses_linear_format(role, source_metadata) => wgpu::TextureFormat::Rgba8Unorm,
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
			.skip(scene_texture_base)
			.map(|t| t.create_view(&wgpu::TextureViewDescriptor::default()))
			.collect();
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
		let effective_visibility = scene_effective_visibility(scene);
		let mut draws = Vec::with_capacity(mesh_draw_capacity(scene));
		let mut skin_palettes = Vec::with_capacity(skin_palette_capacity(scene));
		let mut skin_palette_indices = BTreeMap::new();
		let mut empty_morph_resources: Option<MorphGpuResources> = None;
		let default_material = UnaMaterialPbr::default();
		let default_mtoon = UnaMtoonMaterial::default();
		for (ni, node) in scene.nodes.iter().enumerate() {
			let active = effective_visibility.get(ni).copied().unwrap_or(false);
			let Some(mesh_i) = node.mesh else { continue };
			let Some(mesh_prims) = scene.meshes.get(mesh_i) else { continue };
			for (prim_i, buf) in mesh_prims.iter().enumerate() {
				report("gpu-upload", format!("Preparing mesh {mesh_i} primitive {prim_i}"));
				let mi = buf.material_index.unwrap_or(0);
				let mat = scene.materials.get(mi).unwrap_or(&default_material);
				if material_is_fully_invisible_for_draw(mat, &opts) {
					report("gpu-upload", format!("Skipping fully transparent mesh {mesh_i} primitive {prim_i}"));
					continue;
				}
				let Some(exp) = expand_primitive(buf, !opts.debug_zero_morphs) else {
					continue;
				};
				let ExpandedPrimitive {
					mut verts,
					indices,
					morph_pos,
					morph_nrm,
					default_morph_weights,
				} = exp;
				let skin = node.skin.and_then(|skin_index| scene.skins.get(skin_index));
				normalize_skinning_vertices(&mut verts, buf.joints.is_some(), skin);
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

				let mtoon = mat.mtoon_source_profile().unwrap_or(&default_mtoon);
				let liltoon_like = mat.liltoon_like_source_profile();
				let tex_view = texture_view_or(&image_views, mat.base_color_texture_index, &white_view);
				let tex_sampler = texture_sampler_or(&samplers, &image_sampler_indices, mat.base_color_texture_index, 0);
				let shade_texture_index = liltoon_like
					.and_then(|liltoon_like| liltoon_like.shadow.color_texture_index)
					.or(mtoon.shade_multiply_texture_index);
				let shade_fallback_view = if liltoon_like.is_some() {
					&transparent_black_view
				} else {
					&white_view
				};
				let shade_view = texture_view_or(&image_views, shade_texture_index, shade_fallback_view);
				let shade_sampler = texture_sampler_or(&samplers, &image_sampler_indices, shade_texture_index, 0);
				let shadow2_color_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.shadow.second_color_texture_index);
				let shadow2_color_view = texture_view_or(&image_views, shadow2_color_texture_index, &transparent_black_view);
				let shadow3_color_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.shadow.third_color_texture_index);
				let shadow3_color_view = texture_view_or(&image_views, shadow3_color_texture_index, &transparent_black_view);
				let liltoon_strength_mask_texture_index =
					liltoon_like.and_then(|liltoon_like| liltoon_like.shadow.strength_mask_texture_index);
				let shading_shift_texture_index = liltoon_strength_mask_texture_index.or(mtoon.shading_shift_texture_index);
				let shift_fallback_view = if liltoon_like.is_some() { &white_view } else { &black_view };
				let shift_view = texture_view_or(&image_views, shading_shift_texture_index, shift_fallback_view);
				let shift_sampler = texture_sampler_or(&samplers, &image_sampler_indices, shading_shift_texture_index, 0);
				let shadow_border_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.shadow.border_mask_texture_index);
				let shadow_border_mask_view = texture_view_or(&image_views, shadow_border_mask_texture_index, &white_view);
				let shadow_border_mask_sampler = texture_sampler_or(&samplers, &image_sampler_indices, shadow_border_mask_texture_index, 0);
				let shadow_blur_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.shadow.blur_mask_texture_index);
				let shadow_blur_mask_view = texture_view_or(&image_views, shadow_blur_mask_texture_index, &white_view);
				let shadow_blur_mask_sampler = texture_sampler_or(&samplers, &image_sampler_indices, shadow_blur_mask_texture_index, 0);
				let matcap_texture_index = liltoon_like
					.and_then(|liltoon_like| liltoon_like.matcap.texture_index)
					.or(mtoon.matcap_texture_index);
				let matcap_fallback_view = if liltoon_like.is_some() { &white_view } else { &black_view };
				let matcap_view = texture_view_or(&image_views, matcap_texture_index, matcap_fallback_view);
				let matcap_sampler = texture_sampler_or(&samplers, &image_sampler_indices, matcap_texture_index, 0);
				let matcap_blend_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.matcap.blend_mask_texture_index);
				let matcap_blend_mask_view = texture_view_or(&image_views, matcap_blend_mask_texture_index, &white_view);
				let matcap_blend_mask_sampler = texture_sampler_or(&samplers, &image_sampler_indices, matcap_blend_mask_texture_index, 0);
				let matcap_bump_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.matcap.bump_texture_index);
				let matcap_bump_view = texture_view_or(&image_views, matcap_bump_texture_index, &neutral_normal_view);
				let matcap2_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.matcap.second_texture_index);
				let matcap2_view = texture_view_or(&image_views, matcap2_texture_index, &white_view);
				let matcap2_blend_mask_texture_index =
					liltoon_like.and_then(|liltoon_like| liltoon_like.matcap.second_blend_mask_texture_index);
				let matcap2_blend_mask_view = texture_view_or(&image_views, matcap2_blend_mask_texture_index, &white_view);
				let matcap2_bump_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.matcap.second_bump_texture_index);
				let matcap2_bump_view = texture_view_or(&image_views, matcap2_bump_texture_index, &neutral_normal_view);
				let main2nd_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.main_color.second_texture_index);
				let main2nd_view = texture_view_or(&image_views, main2nd_texture_index, &white_view);
				let main2nd_blend_mask_texture_index =
					liltoon_like.and_then(|liltoon_like| liltoon_like.main_color.second_blend_mask_texture_index);
				let main2nd_blend_mask_view = texture_view_or(&image_views, main2nd_blend_mask_texture_index, &white_view);
				let main2nd_dissolve_mask_texture_index =
					liltoon_like.and_then(|liltoon_like| liltoon_like.main_color.second_dissolve.mask_texture_index);
				let main2nd_dissolve_mask_view = texture_view_or(&image_views, main2nd_dissolve_mask_texture_index, &white_view);
				let main2nd_dissolve_noise_mask_texture_index =
					liltoon_like.and_then(|liltoon_like| liltoon_like.main_color.second_dissolve.noise_mask_texture_index);
				let main2nd_dissolve_noise_mask_view =
					texture_view_or(&image_views, main2nd_dissolve_noise_mask_texture_index, &white_view);
				let main3rd_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.main_color.third_texture_index);
				let main3rd_view = texture_view_or(&image_views, main3rd_texture_index, &white_view);
				let main3rd_blend_mask_texture_index =
					liltoon_like.and_then(|liltoon_like| liltoon_like.main_color.third_blend_mask_texture_index);
				let main3rd_blend_mask_view = texture_view_or(&image_views, main3rd_blend_mask_texture_index, &white_view);
				let main3rd_dissolve_mask_texture_index =
					liltoon_like.and_then(|liltoon_like| liltoon_like.main_color.third_dissolve.mask_texture_index);
				let main3rd_dissolve_mask_view = texture_view_or(&image_views, main3rd_dissolve_mask_texture_index, &white_view);
				let main3rd_dissolve_noise_mask_texture_index =
					liltoon_like.and_then(|liltoon_like| liltoon_like.main_color.third_dissolve.noise_mask_texture_index);
				let main3rd_dissolve_noise_mask_view =
					texture_view_or(&image_views, main3rd_dissolve_noise_mask_texture_index, &white_view);
				let main_gradation_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.main_color.gradation_texture_index);
				let main_gradation_view = texture_view_or(&image_views, main_gradation_texture_index, &white_view);
				let main_color_adjust_mask_texture_index =
					liltoon_like.and_then(|liltoon_like| liltoon_like.main_color.main_color_adjust_mask_texture_index);
				let main_color_adjust_mask_view = texture_view_or(&image_views, main_color_adjust_mask_texture_index, &white_view);
				let alpha_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.alpha_mask.texture_index);
				let alpha_mask_view = texture_view_or(&image_views, alpha_mask_texture_index, &white_view);
				let alpha_mask_sampler = texture_sampler_or(&samplers, &image_sampler_indices, alpha_mask_texture_index, 0);
				let rim_texture_index = liltoon_like
					.and_then(|liltoon_like| liltoon_like.rim.texture_index)
					.or(mtoon.rim_multiply_texture_index);
				let rim_view = texture_view_or(&image_views, rim_texture_index, &white_view);
				let rim_sampler = texture_sampler_or(&samplers, &image_sampler_indices, rim_texture_index, 0);
				let rim_shade_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.rim.shade_mask_texture_index);
				let rim_shade_mask_view = texture_view_or(&image_views, rim_shade_mask_texture_index, &white_view);
				let backlight_color_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.backlight.texture_index);
				let glitter_color_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.glitter.color_texture_index);
				let glitter_shape_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.glitter.shape_texture_index);
				let dissolve_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.dissolve.mask_texture_index);
				let dissolve_noise_mask_texture_index =
					liltoon_like.and_then(|liltoon_like| liltoon_like.dissolve.noise_mask_texture_index);
				let parallax_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.parallax.texture_index);
				let backlight_color_view = texture_view_or(&image_views, backlight_color_texture_index, &white_view);
				let glitter_color_view = texture_view_or(&image_views, glitter_color_texture_index, &white_view);
				let glitter_shape_view = texture_view_or(&image_views, glitter_shape_texture_index, &white_view);
				let dissolve_mask_view = texture_view_or(&image_views, dissolve_mask_texture_index, &white_view);
				let dissolve_noise_mask_view = texture_view_or(&image_views, dissolve_noise_mask_texture_index, &white_view);
				let parallax_view = texture_view_or(&image_views, parallax_texture_index, &white_view);
				let reflection_texture_index = if let Some(liltoon_like) = liltoon_like {
					liltoon_reflection_texture_index(liltoon_like)
				} else {
					mtoon.reflection_cube_texture_index
				};
				let reflection_view = reflection_texture_index
					.and_then(|index| cube_image_views.get(index).and_then(Option::as_ref))
					.unwrap_or(&black_cube_view);
				let reflection_sampler = &reflection_cube_sampler;
				let reflection_color_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.reflection.color_texture_index);
				let reflection_color_view = texture_view_or(&image_views, reflection_color_texture_index, &white_view);
				let reflection_color_sampler = texture_sampler_or(&samplers, &image_sampler_indices, reflection_color_texture_index, 0);
				let smoothness_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.reflection.smoothness_texture_index);
				let smoothness_view = texture_view_or(&image_views, smoothness_texture_index, &white_view);
				let smoothness_sampler = texture_sampler_or(&samplers, &image_sampler_indices, smoothness_texture_index, 0);
				let metallic_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.reflection.metallic_texture_index);
				let metallic_view = texture_view_or(&image_views, metallic_texture_index, &white_view);
				let metallic_sampler = texture_sampler_or(&samplers, &image_sampler_indices, metallic_texture_index, 0);
				let emissive_texture_index = liltoon_like
					.and_then(|liltoon_like| liltoon_like.emission.texture_index)
					.or(mat.emissive_texture_index);
				let emissive_fallback_view = if liltoon_like.is_some() { &white_view } else { &black_view };
				let emissive_view = texture_view_or(&image_views, emissive_texture_index, emissive_fallback_view);
				let emissive_sampler = texture_sampler_or(&samplers, &image_sampler_indices, emissive_texture_index, 0);
				let emission_blend_mask_texture_index =
					liltoon_like.and_then(|liltoon_like| liltoon_like.emission.blend_mask_texture_index);
				let emission_blend_mask_view = texture_view_or(&image_views, emission_blend_mask_texture_index, &white_view);
				let emission_gradation_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.emission.gradation_texture_index);
				let emission_gradation_view = texture_view_or(&image_views, emission_gradation_texture_index, &white_view);
				let emission2nd_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.emission.second_texture_index);
				let emission2nd_view = texture_view_or(&image_views, emission2nd_texture_index, &white_view);
				let emission2nd_blend_mask_texture_index =
					liltoon_like.and_then(|liltoon_like| liltoon_like.emission.second_blend_mask_texture_index);
				let emission2nd_blend_mask_view = texture_view_or(&image_views, emission2nd_blend_mask_texture_index, &white_view);
				let emission2nd_gradation_texture_index =
					liltoon_like.and_then(|liltoon_like| liltoon_like.emission.second_gradation_texture_index);
				let emission2nd_gradation_view = texture_view_or(&image_views, emission2nd_gradation_texture_index, &white_view);
				let occlusion_view = texture_view_or(&image_views, mat.occlusion_texture_index, &white_view);
				let occlusion_sampler = texture_sampler_or(&samplers, &image_sampler_indices, mat.occlusion_texture_index, 0);
				let outline_width_mask_texture_index = liltoon_like
					.and_then(|liltoon_like| liltoon_like.outline.width_mask_texture_index)
					.or(mtoon.outline_width_multiply_texture_index);
				let outline_view = texture_view_or(&image_views, outline_width_mask_texture_index, &white_view);
				let outline_sampler = texture_sampler_or(&samplers, &image_sampler_indices, outline_width_mask_texture_index, 0);
				let outline_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.outline.texture_index);
				let outline_color_view = texture_view_or(&image_views, outline_texture_index, &white_view);
				let uv_mask_view = texture_view_or(&image_views, mtoon.uv_animation_mask_texture_index, &white_view);
				let uv_mask_sampler = texture_sampler_or(&samplers, &image_sampler_indices, mtoon.uv_animation_mask_texture_index, 0);
				let normal_view = texture_view_or(&image_views, mat.normal_texture_index, &neutral_normal_view);
				let normal_sampler = texture_sampler_or(&samplers, &image_sampler_indices, mat.normal_texture_index, 0);
				let normal2nd_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.normal.second_texture_index);
				let normal2nd_view = texture_view_or(&image_views, normal2nd_texture_index, &neutral_normal_view);
				let normal2nd_scale_mask_texture_index =
					liltoon_like.and_then(|liltoon_like| liltoon_like.normal.second_scale_mask_texture_index);
				let normal2nd_scale_mask_view = texture_view_or(&image_views, normal2nd_scale_mask_texture_index, &white_view);
				let anisotropy_tangent_texture_index =
					liltoon_like.and_then(|liltoon_like| liltoon_like.reflection.anisotropy_tangent_texture_index);
				let anisotropy_tangent_view = texture_view_or(&image_views, anisotropy_tangent_texture_index, &neutral_normal_view);
				let anisotropy_scale_mask_texture_index =
					liltoon_like.and_then(|liltoon_like| liltoon_like.reflection.anisotropy_scale_mask_texture_index);
				let anisotropy_scale_mask_view = texture_view_or(&image_views, anisotropy_scale_mask_texture_index, &white_view);
				let anisotropy_shift_noise_texture_index =
					liltoon_like.and_then(|liltoon_like| liltoon_like.reflection.anisotropy_shift_noise_mask_texture_index);
				let anisotropy_shift_noise_view = texture_view_or(&image_views, anisotropy_shift_noise_texture_index, &white_view);
				let fur_vector_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.fur.vector_texture_index);
				let fur_vector_view = texture_view_or(&image_views, fur_vector_texture_index, &neutral_vector_view);
				let fur_length_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.fur.length_mask_texture_index);
				let fur_length_mask_view = texture_view_or(&image_views, fur_length_mask_texture_index, &white_view);
				let fur_noise_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.fur.noise_mask_texture_index);
				let fur_noise_mask_view = texture_view_or(&image_views, fur_noise_mask_texture_index, &white_view);
				let fur_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.fur.mask_texture_index);
				let fur_mask_view = texture_view_or(&image_views, fur_mask_texture_index, &white_view);
				let audio_link_mask_texture_index = liltoon_like.and_then(|liltoon_like| liltoon_like.audio_link.mask_texture_index);
				let audio_link_mask_view = texture_view_or(&image_views, audio_link_mask_texture_index, &blue_view);
				let audio_link_local_map_texture_index =
					liltoon_like.and_then(|liltoon_like| liltoon_like.audio_link.local_map_texture_index);
				let audio_link_local_map_view = texture_view_or(&image_views, audio_link_local_map_texture_index, &black_view);
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

				let mut bind_material_entries = vec![
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
					layout: &outline_material_layout,
					entries: &[
						wgpu::BindGroupEntry {
							binding: 0,
							resource: draw_transform.as_entire_binding(),
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
							resource: draw_material_buffer.as_entire_binding(),
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

				let morph_target_count = morph_pos.len();
				let has_morph_targets = morph_target_count > 0;
				let morph_resources = if has_morph_targets {
					let morph_deltas = morph_delta_data(&morph_pos, morph_nrm.as_deref(), verts.len());
					create_morph_resources(
						device,
						queue,
						&morph_bind_group_layout,
						morph_target_count as u32,
						verts.len() as u32,
						&morph_deltas,
					)
				} else {
					let empty_morph_resources = empty_morph_resources
						.get_or_insert_with(|| create_morph_resources(device, queue, &morph_bind_group_layout, 0, 0, &[]));
					MorphGpuResources {
						meta_buffer: empty_morph_resources.meta_buffer.clone(),
						weight_buffer: empty_morph_resources.weight_buffer.clone(),
						delta_buffer: empty_morph_resources.delta_buffer.clone(),
						bind_group: empty_morph_resources.bind_group.clone(),
					}
				};
				let compute_fur_cards = if material_has_fur(mat, mat.shading, &opts) {
					create_compute_fur_cards_draw_resources(
						device,
						&compute_fur_cards_bind_group_layout,
						mat,
						&verts,
						&indices,
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
				draws.push(MeshDraw {
					vertex_buffer: vbuf,
					index_buffer: ibuf,
					index_format,
					index_count: indices.len() as u32,
					draw_transform,
					draw_transform_uploaded: None,
					draw_material: draw_material_buffer,
					bind_material,
					bind_outline_material,
					skin_palette_index,
					skin_palette_static_identity: skin_palette_key.skin_index.is_none(),
					_morph_meta_buffer: morph_resources.meta_buffer,
					morph_weight_buffer: morph_resources.weight_buffer,
					_morph_delta_buffer: morph_resources.delta_buffer,
					morph_bind_group: morph_resources.bind_group,
					_compute_fur_cards: compute_fur_cards,
					world_node_index: ni,
					active,
					shading: mat.shading,
					morph_pos,
					expression_bindings: if has_morph_targets {
						expression_bindings.get(&(mesh_i, prim_i)).cloned().unwrap_or_default()
					} else {
						Vec::new()
					},
					default_morph_weights,
					morph_weights: Vec::with_capacity(morph_target_count),
					morph_weight_scratch: Vec::with_capacity(morph_target_count),
					alpha_mode: mat.alpha_mode,
					material: mat.clone(),
					mesh_index: mesh_i,
					primitive_index: prim_i,
					probe_anchor_node: node.probe_anchor_node,
					local_bounds: node.local_bounds,
					world_origin: Vec3::ZERO,
				});
			}
		}

		let draw_state = build_draw_order(&draws, &opts);
		let has_morph_draws = draws.iter().any(|draw| !draw.morph_pos.is_empty());
		let expression_value_capacity = expression_names.len();

		Ok(Self {
			pipeline_outline_toon,
			_compute_fur_cards_compute_pipeline: compute_fur_cards_compute_pipeline,
			pipeline_compute_fur_cards_pre_toon,
			pipeline_compute_fur_cards_toon,
			pipeline_opaque_lit,
			pipeline_opaque_unlit,
			pipeline_opaque_toon,
			pipeline_transparent_toon_backpass,
			pipeline_transparent_toon_backpass_no_zwrite,
			pipeline_blend_lit,
			pipeline_blend_unlit,
			pipeline_blend_toon,
			pipeline_blend_toon_zwrite,
			pipeline_blend_toon_add,
			pipeline_blend_toon_add_zwrite,
			pipeline_liltoon_gem_pre_toon,
			frame_buffer,
			frame_uploaded: None,
			frame_layout,
			frame_bind_group,
			screen_grab_sampler,
			_screen_grab_fallback_texture: screen_grab_fallback_texture,
			_audio_link_texture: audio_link_texture,
			audio_link_view,
			audio_link_uploaded_sequence: 0,
			audio_link_frame_params: [0.0; 4],
			_samplers: samplers,
			_textures: textures,
			_cube_textures: cube_textures,
			draws,
			skin_palettes,
			outline_draw_indices: draw_state.outline_draw_indices,
			fur_draw_indices: draw_state.fur_draw_indices,
			opaque_batches: draw_state.opaque_batches,
			transparent_backpass_draw_indices: draw_state.transparent_backpass_draw_indices,
			blended_batches: draw_state.blended_batches,
			active_draw_indices: draw_state.active_draw_indices,
			needs_screen_refraction: draw_state.needs_screen_refraction,
			active_skin_palette_indices: draw_state.active_skin_palette_indices,
			texture_summary,
			runtime_requirements: draw_state.runtime_requirements,
			visibility_scratch: Vec::new(),
			expression_names,
			expression_value_scratch: Vec::with_capacity(expression_value_capacity),
			has_morph_draws,
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
		palette.uploaded_changed = false;
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
		skin_palettes.push(SkinPalette {
			key,
			buffer: bone_buffer,
			bind_group: bone_bind_group,
			matrix_capacity,
			static_identity,
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
		pass.set_bind_group(3, &d.morph_bind_group, &[]);
		pass.set_vertex_buffer(0, d.vertex_buffer.slice(..));
		pass.set_index_buffer(d.index_buffer.slice(..), d.index_format);
		pass.draw_indexed(0..d.index_count, 0, 0..instance_count);
	}

	fn draw_inner(&self, pass: &mut wgpu::RenderPass<'_>, state: &mut DrawBindState, draw_index: usize) {
		let bind_material = &self.draws[draw_index].bind_material;
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
		pass.set_bind_group(1, &d.bind_material, &[]);
		let palette = &self.skin_palettes[d.skin_palette_index];
		if state.skin_palette_index != Some(d.skin_palette_index) {
			pass.set_bind_group(2, &palette.bind_group, &[]);
			state.skin_palette_index = Some(d.skin_palette_index);
		}
		pass.set_bind_group(3, &d.morph_bind_group, &[]);
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
		pass.set_pipeline(&self.pipeline_outline_toon);
		let mut state = DrawBindState::default();
		for &draw_index in &self.outline_draw_indices {
			let bind_material = &self.draws[draw_index].bind_outline_material;
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
		pass.set_pipeline(&self._compute_fur_cards_compute_pipeline._pipeline);
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
			compute_fur_cards_skinned_source_vertices_from_mesh(
				&compute_fur_cards.base_vertices,
				&palette.uploaded,
				&mut compute_fur_cards.source_vertex_scratch,
			);
			if compute_fur_cards.source_vertex_scratch.len() != compute_fur_cards.base_vertices.len() {
				continue;
			}
			queue.write_buffer(
				&compute_fur_cards.source_vertex_buffer,
				0,
				bytemuck::cast_slice(&compute_fur_cards.source_vertex_scratch),
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
		self.outline_draw_indices = draw_state.outline_draw_indices;
		self.fur_draw_indices = draw_state.fur_draw_indices;
		self.opaque_batches = draw_state.opaque_batches;
		self.transparent_backpass_draw_indices = draw_state.transparent_backpass_draw_indices;
		self.blended_batches = draw_state.blended_batches;
		self.active_draw_indices = draw_state.active_draw_indices;
		self.needs_screen_refraction = draw_state.needs_screen_refraction;
		self.active_skin_palette_indices = draw_state.active_skin_palette_indices;
		self.runtime_requirements = draw_state.runtime_requirements;
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
		let default_mtoon = UnaMtoonMaterial::default();
		for draw in &self.draws {
			let material =
				mesh_draw_material_gpu_runtime(&draw.material, &default_mtoon, &self.opts, draw.mesh_index, draw.primitive_index);
			queue.write_buffer(&draw.draw_material, 0, bytemuck::bytes_of(&material));
		}
	}

	#[inline]
	fn pipeline_for_kind(&self, kind: DrawPipelineKind) -> &wgpu::RenderPipeline {
		match kind {
			DrawPipelineKind::OpaqueLit => &self.pipeline_opaque_lit,
			DrawPipelineKind::OpaqueUnlit => &self.pipeline_opaque_unlit,
			DrawPipelineKind::OpaqueToon => &self.pipeline_opaque_toon,
			DrawPipelineKind::BlendLit => &self.pipeline_blend_lit,
			DrawPipelineKind::BlendUnlit => &self.pipeline_blend_unlit,
			DrawPipelineKind::BlendToon => &self.pipeline_blend_toon,
			DrawPipelineKind::BlendToonZWrite => &self.pipeline_blend_toon_zwrite,
			DrawPipelineKind::BlendToonAdd => &self.pipeline_blend_toon_add,
			DrawPipelineKind::BlendToonAddZWrite => &self.pipeline_blend_toon_add_zwrite,
			DrawPipelineKind::TransparentToonBackpass => &self.pipeline_transparent_toon_backpass,
			DrawPipelineKind::TransparentToonBackpassNoZWrite => &self.pipeline_transparent_toon_backpass_no_zwrite,
			DrawPipelineKind::LilToonGemPre => &self.pipeline_liltoon_gem_pre_toon,
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
			let mut backpass_zwrite = None;
			for &draw_index in &self.transparent_backpass_draw_indices {
				let zwrite = self.draws[draw_index]
					.material
					.liltoon_like_source_profile()
					.is_none_or(|u| u.blend_state.pre_zwrite_factor > 0.5);
				if backpass_zwrite != Some(zwrite) {
					pass.set_pipeline(if zwrite {
						&self.pipeline_transparent_toon_backpass
					} else {
						&self.pipeline_transparent_toon_backpass_no_zwrite
					});
					backpass_zwrite = Some(zwrite);
					*state = DrawBindState::default();
				}
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
			pass.set_pipeline(self.pipeline_for_kind(batch.pipeline));
			let len = if batch_index == end_batch {
				end_pos
			} else {
				batch.draw_indices.len()
			};
			for &draw_index in batch.draw_indices.iter().take(len) {
				self.draw_inner(pass, state, draw_index);
			}
		}
	}

	fn draw_blended_batches_from(&self, pass: &mut wgpu::RenderPass<'_>, state: &mut DrawBindState, start: Option<(usize, usize)>) {
		let (start_batch, start_pos) = start.unwrap_or((0, 0));
		for (batch_index, batch) in self.blended_batches.iter().enumerate().skip(start_batch) {
			pass.set_pipeline(self.pipeline_for_kind(batch.pipeline));
			let skip = if batch_index == start_batch { start_pos } else { 0 };
			for &draw_index in batch.draw_indices.iter().skip(skip) {
				self.draw_inner(pass, state, draw_index);
			}
		}
	}

	fn draw_fur_blended(&self, pass: &mut wgpu::RenderPass<'_>, state: &mut DrawBindState) {
		if !self.fur_draw_indices.is_empty() {
			*state = DrawBindState::default();
			pass.set_pipeline(&self.pipeline_compute_fur_cards_pre_toon);
			for &draw_index in &self.fur_draw_indices {
				let _ = self.draw_compute_fur_cards_inner(pass, state, draw_index);
			}
			*state = DrawBindState::default();
			pass.set_pipeline(&self.pipeline_compute_fur_cards_toon);
			for &draw_index in &self.fur_draw_indices {
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
		refresh_scene_morph_defaults: bool,
	) {
		let debug_skin_legacy_no_inv_mesh = self.opts.debug_skin_legacy_no_inv_mesh;
		let debug_zero_morphs = self.opts.debug_zero_morphs;
		if refresh_scene_morph_defaults {
			self.refresh_morph_defaults_from_scene(scene);
			self.refresh_draw_visibility_from_scene(scene);
		}
		self.expression_value_scratch.clear();
		if self.has_morph_draws && (expr_weights.is_some() || expression_overrides.is_some()) {
			self.expression_value_scratch.resize(self.expression_names.len(), 0.0);
			for (index, name) in self.expression_names.iter().enumerate() {
				let value = expression_overrides
					.and_then(|overrides| overrides.get(name).copied())
					.or_else(|| expr_weights.and_then(|weights| weights.preset_weights.get(name).copied()))
					.unwrap_or(0.0);
				self.expression_value_scratch[index] = value;
			}
		}
		if !self.active_skin_palette_indices.is_empty() {
			for &palette_index in &self.active_skin_palette_indices {
				let Some(palette) = self.skin_palettes.get_mut(palette_index) else {
					continue;
				};
				let skin = palette.key.skin_index.and_then(|si| scene.skins.get(si));
				Self::write_skin_palette(queue, palette, skin, world, debug_skin_legacy_no_inv_mesh);
			}
			if !self.fur_draw_indices.is_empty() {
				self.update_compute_fur_cards_source_vertices(queue);
			}
		}
		let expression_values = (!self.expression_value_scratch.is_empty()).then_some(self.expression_value_scratch.as_slice());

		for &draw_index in &self.active_draw_indices {
			let Some(d) = self.draws.get_mut(draw_index) else {
				continue;
			};
			let mesh_world = world.get(d.world_node_index).copied().unwrap_or(Mat4::IDENTITY);
			d.world_origin = if let Some(bounds) = d.local_bounds {
				let reference_world = d.probe_anchor_node.and_then(|node| world.get(node)).copied().unwrap_or(mesh_world);
				reference_world.transform_point3(Vec3::from(bounds.center))
			} else {
				mesh_world.transform_point3(Vec3::ZERO)
			};

			if !d.morph_pos.is_empty() {
				let draw_has_active_expression = expression_bindings_have_active_weight(&d.expression_bindings, expression_values);
				let skip_static_default_morph = !draw_has_active_expression
					&& !debug_zero_morphs
					&& morph_weights_match_default(&d.morph_weights, &d.default_morph_weights, d.morph_pos.len());
				if !skip_static_default_morph {
					d.morph_weight_scratch.clear();
					if debug_zero_morphs {
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
			if let Some(compute_fur_cards) = d._compute_fur_cards.as_mut() {
				let model = mesh_world.to_cols_array_2d();
				let inv_model = mesh_world.inverse().to_cols_array_2d();
				if compute_fur_cards.params.model != model || compute_fur_cards.params.inv_model != inv_model {
					compute_fur_cards.params.model = model;
					compute_fur_cards.params.inv_model = inv_model;
					queue.write_buffer(&compute_fur_cards.params_buffer, 0, bytemuck::bytes_of(&compute_fur_cards.params));
				}
			}
		}
	}

	pub fn refresh_morph_defaults_from_scene(&mut self, scene: &UnaSceneSnapshot) -> usize {
		if !self.has_morph_draws {
			return 0;
		}
		let mut changed = 0;
		for draw in &mut self.draws {
			let target_count = draw.morph_pos.len();
			if target_count == 0 {
				continue;
			}
			if refresh_morph_default_weights(
				&mut draw.default_morph_weights,
				&mut draw.morph_weights,
				scene,
				draw.mesh_index,
				draw.primitive_index,
				target_count,
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
			if draw.active != next {
				draw.active = next;
				changed += 1;
			}
		}
		if changed > 0 {
			self.rebuild_draw_order();
		}
		changed
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
			byte_length: 0,
			source_hash: 0,
		}
	}

	#[test]
	fn mesh_sampler_metadata_maps_to_wgpu_modes() {
		assert_eq!(wgpu_address_mode(UnaTextureWrapMode::ClampToEdge), wgpu::AddressMode::ClampToEdge);
		assert_eq!(
			wgpu_address_mode(UnaTextureWrapMode::MirroredRepeat),
			wgpu::AddressMode::MirrorRepeat
		);
		assert_eq!(wgpu_address_mode(UnaTextureWrapMode::Repeat), wgpu::AddressMode::Repeat);
		assert_eq!(wgpu_filter_mode(UnaTextureFilterMode::Nearest), wgpu::FilterMode::Nearest);
		assert_eq!(wgpu_filter_mode(UnaTextureFilterMode::Linear), wgpu::FilterMode::Linear);
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
		let _pipeline = create_compute_fur_cards_compute_pipeline(&device, &bind_group_layout);
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
		let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
			label: Some("mesh_shader_test"),
			source: wgpu::ShaderSource::Wgsl(mesh_shader_source_for_tier(shader_variant_tier)),
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

		let _outline_toon = SceneMeshes::create_mesh_pipeline(
			&device,
			&outline_pipeline_layout,
			&shader,
			wgpu::TextureFormat::Rgba8Unorm,
			&vb_layout,
			"mesh_outline_toon",
			"vs_outline",
			"fs_outline",
			None,
			wgpu::ColorWrites::ALL,
			false,
			wgpu::CompareFunction::Less,
			Some(wgpu::Face::Front),
			1,
		);
		let _opaque_toon = SceneMeshes::create_mesh_pipeline(
			&device,
			&pipeline_layout,
			&shader,
			wgpu::TextureFormat::Rgba8Unorm,
			&vb_layout,
			"mesh_opaque_toon",
			"vs_main",
			"fs_toon",
			None,
			wgpu::ColorWrites::ALL,
			true,
			wgpu::CompareFunction::LessEqual,
			None,
			1,
		);
		let _compute_fur_cards_pre_toon = SceneMeshes::create_mesh_pipeline(
			&device,
			&pipeline_layout,
			&shader,
			wgpu::TextureFormat::Rgba8Unorm,
			&compute_fur_cards_vb_layout,
			"mesh_compute_fur_cards_pre_toon",
			"vs_compute_fur_cards_pre",
			"fs_fur_toon_pre",
			None,
			wgpu::ColorWrites::ALL,
			true,
			wgpu::CompareFunction::LessEqual,
			None,
			1,
		);
		let _compute_fur_cards_toon = SceneMeshes::create_mesh_pipeline(
			&device,
			&pipeline_layout,
			&shader,
			wgpu::TextureFormat::Rgba8Unorm,
			&compute_fur_cards_vb_layout,
			"mesh_compute_fur_cards_toon",
			"vs_compute_fur_cards",
			"fs_fur_toon",
			Some(wgpu::BlendState::ALPHA_BLENDING),
			wgpu::ColorWrites::ALL,
			false,
			wgpu::CompareFunction::LessEqual,
			None,
			1,
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
		assert_eq!(scene_default_morph_weights_for_draw(&scene, 0, 0, 2), vec![1.0, 0.0]);
		assert_eq!(scene_default_morph_weights_for_draw(&scene, 9, 0, 2), vec![0.0, 0.0]);
	}

	#[test]
	fn refresh_morph_default_weights_invalidates_uploaded_weights_only_on_change() {
		let scene = UnaSceneSnapshot {
			meshes: vec![vec![UnaMeshBuffers {
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
				morph_target_names: vec!["A".to_string()],
				default_morph_weights: vec![0.75],
			}]],
			..Default::default()
		};
		let mut defaults = vec![0.25];
		let mut uploaded = vec![0.25];
		assert!(refresh_morph_default_weights(&mut defaults, &mut uploaded, &scene, 0, 0, 1));
		assert_eq!(defaults, vec![0.75]);
		assert!(uploaded.is_empty());

		uploaded.push(0.75);
		assert!(!refresh_morph_default_weights(&mut defaults, &mut uploaded, &scene, 0, 0, 1));
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
		fill_morph_weights_for_draw(&[0.2, 0.25], 2, &bindings, Some(&[0.5]), &mut out);
		assert_eq!(out, vec![0.2, 0.5]);
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
	fn expand_primitive_bakes_static_default_morphs() {
		let buf = UnaMeshBuffers {
			name: None,
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

		let baked = expand_primitive(&buf, true).expect("expanded primitive");
		assert_eq!(baked.verts[0].pos, [2.0, 2.0, 2.5]);
		assert_eq!(baked.default_morph_weights, vec![0.0]);

		let dynamic = expand_primitive(&buf, false).expect("expanded primitive");
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
		let mut batches = Vec::new();
		append_ordered_draw_batch(&mut batches, DrawPipelineKind::BlendToon, 0, 1);
		append_ordered_draw_batch(&mut batches, DrawPipelineKind::BlendToonZWrite, 1, 1);
		append_ordered_draw_batch(&mut batches, DrawPipelineKind::BlendLit, 2, 1);
		append_ordered_draw_batch(&mut batches, DrawPipelineKind::BlendToon, 3, 1);
		append_ordered_draw_batch(&mut batches, DrawPipelineKind::BlendToon, 4, 1);

		assert_eq!(batches.len(), 4);
		assert_eq!(batches[0].pipeline, DrawPipelineKind::BlendToon);
		assert_eq!(batches[0].draw_indices, vec![0]);
		assert_eq!(batches[1].pipeline, DrawPipelineKind::BlendToonZWrite);
		assert_eq!(batches[1].draw_indices, vec![1]);
		assert_eq!(batches[2].pipeline, DrawPipelineKind::BlendLit);
		assert_eq!(batches[2].draw_indices, vec![2]);
		assert_eq!(batches[3].pipeline, DrawPipelineKind::BlendToon);
		assert_eq!(batches[3].draw_indices, vec![3, 4]);
	}

	#[test]
	fn ordered_draw_batches_keep_gem_prepass_adjacent_to_forward() {
		let mut batches = Vec::new();
		append_ordered_draw_batch(&mut batches, DrawPipelineKind::BlendToon, 0, 4);
		append_ordered_draw_batch(&mut batches, DrawPipelineKind::LilToonGemPre, 1, 4);
		append_ordered_draw_batch(&mut batches, DrawPipelineKind::BlendToonAdd, 1, 4);
		append_ordered_draw_batch(&mut batches, DrawPipelineKind::BlendToon, 2, 4);

		assert_eq!(
			batches.iter().map(|batch| batch.pipeline).collect::<Vec<_>>(),
			vec![
				DrawPipelineKind::BlendToon,
				DrawPipelineKind::LilToonGemPre,
				DrawPipelineKind::BlendToonAdd,
				DrawPipelineKind::BlendToon
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
	fn liltoon_source_flag_reaches_draw_uniform() {
		let mat = UnaMaterialPbr {
			liltoon_like: Some(un_avatar_core::UnaLilToonLikeMaterial::default()),
			..Default::default()
		};

		let draw = mesh_draw_material_gpu(&mat, &UnaMtoonMaterial::default(), &SceneMeshLoadOpts::default(), 0, 0);
		let flags = draw.params[3].to_bits();

		assert_ne!(flags & 4096, 0);
		assert_eq!(flags & 32768, 0);
	}

	#[test]
	fn liltoon_gem_source_flag_reaches_draw_uniform() {
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

		assert_ne!(flags & 4096, 0);
		assert_ne!(flags & 8192, 0);
		assert_ne!(flags & 32768, 0);
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
		assert_eq!(liltoon_reflection_texture_index(&normal), Some(42));
	}

	#[test]
	fn liltoon_refraction_source_flag_reaches_draw_uniform() {
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

		assert_ne!(flags & 4096, 0);
		assert_ne!(flags & 16384, 0);
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
		compute_fur_cards_skinned_source_vertices_from_mesh(&verts, &palette, &mut source_vertices);

		assert_eq!(source_vertices.len(), 1);
		assert!((source_vertices[0].position[0] - 4.0).abs() < 0.00001);
		assert!((source_vertices[0].position[1] - 2.0).abs() < 0.00001);
		assert!((source_vertices[0].position[2] - 3.0).abs() < 0.00001);
		assert_eq!(source_vertices[0].normal, [0.0, 1.0, 0.0, 0.0]);
		assert_eq!(source_vertices[0].tangent, [1.0, 0.0, 0.0, -1.0]);
		assert_eq!(source_vertices[0].color, [0.25, 0.5, 0.75, 1.0]);
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
