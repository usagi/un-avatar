// メッシュ描画: 頂点は共通、フラグメントはシェーディング種別ごとに別エントリ＋別パイプライン（ランタイムのシェーディング分岐を避ける）。
//
// - fs_lit: LitLambert
// - fs_unlit: Unlit（ベース色のみ）
// - fs_toon: avatar toon shading. v2 `.unavatar` materials enter through the
//   lilToon-like parameter branch; VRM/MToon remains a legacy input path.

struct Frame {
	view_proj: mat4x4<f32>,
	view: mat4x4<f32>,
	light_dir: vec4<f32>,
	camera_pos: vec4<f32>,
	light_color: vec4<f32>,
	ambient_color: vec4<f32>,
	time_params: vec4<f32>,
}

struct DrawTransform {
	model: mat4x4<f32>,
}

struct DrawMaterial {
	base_color: vec4<f32>,
	params: vec4<f32>,
	shade_color: vec4<f32>,
	shading_params: vec4<f32>,
	shadow_params: vec4<f32>,
	shadow_ext_params: vec4<f32>,
	shadow_border_color: vec4<f32>,
	shadow2_color: vec4<f32>,
	shadow2_params: vec4<f32>,
	shadow3_color: vec4<f32>,
	shadow3_params: vec4<f32>,
	matcap_factor: vec4<f32>,
	matcap_params: vec4<f32>,
	matcap_ext_params: vec4<f32>,
	matcap2_factor: vec4<f32>,
	matcap2_params: vec4<f32>,
	matcap2_ext_params: vec4<f32>,
	matcap_uv_params: vec4<f32>,
	reflection_color: vec4<f32>,
	reflection_control: vec4<f32>,
	reflection_params: vec4<f32>,
	reflection_ext_params: vec4<f32>,
	reflection_cube_color: vec4<f32>,
	anisotropy_params: vec4<f32>,
	anisotropy_ext_params: vec4<f32>,
	anisotropy2_params: vec4<f32>,
	anisotropy_width_params: vec4<f32>,
	gem_env_color: vec4<f32>,
	gem_params: vec4<f32>,
	gem_particle_color: vec4<f32>,
	specular_toon_params: vec4<f32>,
	rim_color: vec4<f32>,
	rim_params: vec4<f32>,
	rim_control: vec4<f32>,
	rim_ext_params: vec4<f32>,
	rim_indirect_color: vec4<f32>,
	rim_indirect_params: vec4<f32>,
	rim_indirect_ext_params: vec4<f32>,
	rim_shade_color: vec4<f32>,
	rim_shade_params: vec4<f32>,
	backlight_color: vec4<f32>,
	backlight_params: vec4<f32>,
	backlight_ext_params: vec4<f32>,
	backlight_shadow_params: vec4<f32>,
	emission_color: vec4<f32>,
	emission_params: vec4<f32>,
	emission_blink_params: vec4<f32>,
	emission_grad_params: vec4<f32>,
	emission2nd_color: vec4<f32>,
	emission2nd_params: vec4<f32>,
	emission2nd_blink_params: vec4<f32>,
	emission2nd_grad_params: vec4<f32>,
	emission2nd_ext_params: vec4<f32>,
	emission2nd_uv_offset_scale: vec4<f32>,
	emission2nd_uv_anim_params: vec4<f32>,
	emission_blend_mask_uv_offset_scale: vec4<f32>,
	emission_blend_mask_uv_anim_params: vec4<f32>,
	emission2nd_blend_mask_uv_offset_scale: vec4<f32>,
	emission2nd_blend_mask_uv_anim_params: vec4<f32>,
	outline_color: vec4<f32>,
	outline_params: vec4<f32>,
	outline_lit_color: vec4<f32>,
	outline_lit_params: vec4<f32>,
	outline_ext_params: vec4<f32>,
	alpha_mask_params: vec4<f32>,
	fur_params: vec4<f32>,
	fur_vector_params: vec4<f32>,
	fur_noise_params: vec4<f32>,
	fur_ext_params: vec4<f32>,
	fur_rim_color: vec4<f32>,
	fur_rim_params: vec4<f32>,
	alpha_ext_params: vec4<f32>,
	lighting_ext_params: vec4<f32>,
	transparency_params: vec4<f32>,
	emissive_factor: vec4<f32>,
	uv_anim_params: vec4<f32>,
	uv_offset_scale: vec4<f32>,
	normal_uv_offset_scale: vec4<f32>,
	normal2nd_uv_offset_scale: vec4<f32>,
	normal2nd_params: vec4<f32>,
	shade_uv_offset_scale: vec4<f32>,
	rim_uv_offset_scale: vec4<f32>,
	emission_uv_offset_scale: vec4<f32>,
	emission_uv_anim_params: vec4<f32>,
	reflection_color_uv_offset_scale: vec4<f32>,
	smoothness_uv_offset_scale: vec4<f32>,
	metallic_uv_offset_scale: vec4<f32>,
	anisotropy_tangent_uv_offset_scale: vec4<f32>,
	anisotropy_scale_mask_uv_offset_scale: vec4<f32>,
	anisotropy_shift_noise_uv_offset_scale: vec4<f32>,
	shadow_strength_mask_uv_offset_scale: vec4<f32>,
	shadow_border_mask_uv_offset_scale: vec4<f32>,
	shadow_blur_mask_uv_offset_scale: vec4<f32>,
	matcap_blend_mask_uv_offset_scale: vec4<f32>,
	matcap2_blend_mask_uv_offset_scale: vec4<f32>,
	alpha_mask_uv_offset_scale: vec4<f32>,
	main_color_adjust_params: vec4<f32>,
	main_gradation_params: vec4<f32>,
	main2nd_color: vec4<f32>,
	main2nd_params: vec4<f32>,
	main2nd_uv_offset_scale: vec4<f32>,
	main2nd_blend_mask_uv_offset_scale: vec4<f32>,
	main3rd_color: vec4<f32>,
	main3rd_params: vec4<f32>,
	main3rd_uv_offset_scale: vec4<f32>,
	main3rd_blend_mask_uv_offset_scale: vec4<f32>,
}

struct MorphU {
	target_count: u32,
	vertex_count: u32,
	_pad: vec2<u32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var screen_tex: texture_2d<f32>;
@group(0) @binding(2) var screen_samp: sampler;
@group(1) @binding(0) var<uniform> drawt: DrawTransform;
@group(1) @binding(1) var tex: texture_2d<f32>;
@group(1) @binding(2) var base_samp: sampler;
@group(1) @binding(3) var shade_tex: texture_2d<f32>;
@group(1) @binding(4) var shading_shift_tex: texture_2d<f32>;
@group(1) @binding(5) var matcap_tex: texture_2d<f32>;
@group(1) @binding(6) var rim_tex: texture_2d<f32>;
@group(1) @binding(7) var emissive_tex: texture_2d<f32>;
@group(1) @binding(8) var outline_width_tex: texture_2d<f32>;
@group(1) @binding(9) var uv_anim_mask_tex: texture_2d<f32>;
@group(1) @binding(10) var<uniform> drawu: DrawMaterial;
@group(1) @binding(11) var normal_tex: texture_2d<f32>;
@group(1) @binding(12) var occlusion_tex: texture_2d<f32>;
@group(1) @binding(13) var reflection_tex: texture_cube<f32>;
@group(1) @binding(14) var shade_samp: sampler;
@group(1) @binding(15) var shading_shift_samp: sampler;
@group(1) @binding(16) var matcap_samp: sampler;
@group(1) @binding(17) var rim_samp: sampler;
@group(1) @binding(18) var emissive_samp: sampler;
@group(1) @binding(19) var outline_width_samp: sampler;
@group(1) @binding(20) var normal_samp: sampler;
@group(1) @binding(21) var occlusion_samp: sampler;
@group(1) @binding(22) var reflection_samp: sampler;
@group(1) @binding(23) var uv_anim_mask_samp: sampler;
@group(1) @binding(24) var shadow_border_mask_tex: texture_2d<f32>;
@group(1) @binding(25) var shadow_blur_mask_tex: texture_2d<f32>;
@group(1) @binding(26) var shadow_border_mask_samp: sampler;
@group(1) @binding(27) var shadow_blur_mask_samp: sampler;
@group(1) @binding(28) var reflection_color_tex: texture_2d<f32>;
@group(1) @binding(29) var smoothness_tex: texture_2d<f32>;
@group(1) @binding(30) var metallic_tex: texture_2d<f32>;
@group(1) @binding(31) var reflection_color_samp: sampler;
@group(1) @binding(32) var smoothness_samp: sampler;
@group(1) @binding(33) var metallic_samp: sampler;
@group(1) @binding(34) var matcap_blend_mask_tex: texture_2d<f32>;
@group(1) @binding(35) var matcap_blend_mask_samp: sampler;
@group(1) @binding(36) var alpha_mask_tex: texture_2d<f32>;
@group(1) @binding(37) var alpha_mask_samp: sampler;
@group(1) @binding(38) var matcap2_tex: texture_2d<f32>;
@group(1) @binding(39) var matcap2_blend_mask_tex: texture_2d<f32>;
@group(1) @binding(40) var outline_tex: texture_2d<f32>;
@group(1) @binding(41) var main2nd_tex: texture_2d<f32>;
@group(1) @binding(42) var main3rd_tex: texture_2d<f32>;
@group(1) @binding(43) var main2nd_blend_mask_tex: texture_2d<f32>;
@group(1) @binding(44) var main3rd_blend_mask_tex: texture_2d<f32>;
@group(1) @binding(45) var normal2nd_tex: texture_2d<f32>;
@group(1) @binding(46) var emission_gradation_tex: texture_2d<f32>;
@group(1) @binding(47) var main_gradation_tex: texture_2d<f32>;
@group(1) @binding(48) var emission2nd_tex: texture_2d<f32>;
@group(1) @binding(49) var emission2nd_blend_mask_tex: texture_2d<f32>;
@group(1) @binding(50) var emission2nd_gradation_tex: texture_2d<f32>;
@group(1) @binding(51) var anisotropy_tangent_tex: texture_2d<f32>;
@group(1) @binding(52) var anisotropy_scale_mask_tex: texture_2d<f32>;
@group(1) @binding(53) var anisotropy_shift_noise_tex: texture_2d<f32>;
@group(1) @binding(54) var emission_blend_mask_tex: texture_2d<f32>;
@group(1) @binding(55) var rim_shade_mask_tex: texture_2d<f32>;
@group(1) @binding(56) var backlight_color_tex: texture_2d<f32>;
@group(1) @binding(57) var shadow2_color_tex: texture_2d<f32>;
@group(1) @binding(58) var shadow3_color_tex: texture_2d<f32>;
@group(1) @binding(59) var fur_vector_tex: texture_2d<f32>;
@group(1) @binding(60) var fur_length_mask_tex: texture_2d<f32>;
@group(1) @binding(61) var fur_noise_mask_tex: texture_2d<f32>;
@group(1) @binding(62) var fur_mask_tex: texture_2d<f32>;
@group(2) @binding(0) var<storage, read> bones: array<mat4x4<f32>>;
@group(3) @binding(0) var<uniform> morphu: MorphU;
@group(3) @binding(1) var<storage, read> morph_weights: array<f32>;
@group(3) @binding(2) var<storage, read> morph_deltas: array<vec4<f32>>;

struct VsIn {
	@location(0) pos: vec3<f32>,
	@location(1) norm: vec3<f32>,
	@location(2) tangent: vec4<f32>,
	@location(3) uv: vec2<f32>,
	@location(4) joints: vec4<u32>,
	@location(5) weights: vec4<f32>,
	@location(6) color: vec4<f32>,
}

struct VsOut {
	@builtin(position) clip: vec4<f32>,
	@location(0) wn: vec3<f32>,
	@location(1) uv: vec2<f32>,
	@location(2) wp: vec3<f32>,
	@location(3) wt: vec4<f32>,
}

struct FurVsOut {
	@builtin(position) clip: vec4<f32>,
	@location(0) wn: vec3<f32>,
	@location(1) uv: vec2<f32>,
	@location(2) wp: vec3<f32>,
	@location(3) wt: vec4<f32>,
	@location(4) fur_shell: f32,
	@location(5) fur_alpha: f32,
	@location(6) fur_card_side: f32,
	@location(7) fur_uv0: vec2<f32>,
}

struct CsfcFurVsIn {
	@location(0) position_layer: vec4<f32>,
	@location(1) normal_side: vec4<f32>,
	@location(2) uv: vec2<f32>,
	@location(3) fur_alpha: f32,
	@location(4) root_position: vec4<f32>,
	@location(5) pre_position: vec4<f32>,
}

fn mat3_upper(m: mat4x4<f32>) -> mat3x3<f32> {
	return mat3x3<f32>(m[0].xyz, m[1].xyz, m[2].xyz);
}

const DBG_BIND_POSE_RIGID: u32 = 1u;

fn morphed_position_normal(pos_in: vec3<f32>, norm_in: vec3<f32>, vertex_index: u32) -> array<vec3<f32>, 2> {
	var pos = pos_in;
	var norm = norm_in;
	if (vertex_index < morphu.vertex_count) {
		for (var morph_target = 0u; morph_target < morphu.target_count; morph_target = morph_target + 1u) {
			let weight = morph_weights[morph_target];
			if (abs(weight) > 0.000001) {
				let base = (morph_target * morphu.vertex_count + vertex_index) * 2u;
				pos = pos + morph_deltas[base].xyz * weight;
				norm = norm + morph_deltas[base + 1u].xyz * weight;
			}
		}
	}
	return array<vec3<f32>, 2>(pos, normalize(norm));
}

fn skinned_position_normal(v: VsIn, vertex_index: u32) -> VsOut {
	var o: VsOut;
	let morphed = morphed_position_normal(v.pos, v.norm, vertex_index);
	let pos = morphed[0];
	let norm = morphed[1];
	let tangent = v.tangent.xyz;
	let tangent_sign = select(1.0, -1.0, v.tangent.w < 0.0);
	let j0 = v.joints.x;
	let j1 = v.joints.y;
	let j2 = v.joints.z;
	let j3 = v.joints.w;
	let dbg = bitcast<u32>(drawu.params.w);
	if (dbg & DBG_BIND_POSE_RIGID) != 0u {
		let wp = drawt.model * vec4<f32>(pos, 1.0);
		let mn = mat3_upper(drawt.model) * norm;
		let mt = mat3_upper(drawt.model) * tangent;
		o.wn = normalize(mn);
		o.uv = v.uv;
		o.wp = wp.xyz;
		o.wt = vec4<f32>(mt, tangent_sign);
		o.clip = frame.view_proj * wp;
		return o;
	}
	let m0 = bones[j0];
	let m1 = bones[j1];
	let m2 = bones[j2];
	let m3 = bones[j3];
	let p0 = m0 * vec4<f32>(pos, 1.0);
	let p1 = m1 * vec4<f32>(pos, 1.0);
	let p2 = m2 * vec4<f32>(pos, 1.0);
	let p3 = m3 * vec4<f32>(pos, 1.0);
	let local_p = v.weights.x * p0 + v.weights.y * p1 + v.weights.z * p2 + v.weights.w * p3;
	let wp = drawt.model * local_p;

	let n0 = mat3_upper(m0) * norm;
	let n1 = mat3_upper(m1) * norm;
	let n2 = mat3_upper(m2) * norm;
	let n3 = mat3_upper(m3) * norm;
	let local_n = normalize(v.weights.x * n0 + v.weights.y * n1 + v.weights.z * n2 + v.weights.w * n3);
	let wn = normalize(mat3_upper(drawt.model) * local_n);
	let t0 = mat3_upper(m0) * tangent;
	let t1 = mat3_upper(m1) * tangent;
	let t2 = mat3_upper(m2) * tangent;
	let t3 = mat3_upper(m3) * tangent;
	let local_t = v.weights.x * t0 + v.weights.y * t1 + v.weights.z * t2 + v.weights.w * t3;
	let wt = mat3_upper(drawt.model) * local_t;

	o.wn = wn;
	o.uv = v.uv;
	o.wp = wp.xyz;
	o.wt = vec4<f32>(wt, tangent_sign);
	o.clip = frame.view_proj * wp;
	return o;
}

fn fur_vs_out_from_base(o: VsOut) -> FurVsOut {
	var out: FurVsOut;
	out.clip = o.clip;
	out.wn = o.wn;
	out.uv = o.uv;
	out.wp = o.wp;
	out.wt = o.wt;
	out.fur_shell = 0.0;
	out.fur_alpha = 1.0;
	out.fur_card_side = 0.0;
	out.fur_uv0 = o.uv;
	return out;
}

fn normalize_or(v: vec3<f32>, fallback: vec3<f32>) -> vec3<f32> {
	let len2 = dot(v, v);
	if len2 <= 0.0000001 {
		return fallback;
	}
	return v * inverseSqrt(len2);
}

@vertex
fn vs_main(v: VsIn, @builtin(vertex_index) vertex_index: u32) -> VsOut {
	return skinned_position_normal(v, vertex_index);
}

fn unpack_fur_vector_map(texel: vec4<f32>, scale: f32) -> vec3<f32> {
	var n = vec3<f32>(texel.a * texel.r, texel.g, 1.0) * 2.0 - vec3<f32>(1.0);
	n.x = n.x * scale;
	n.y = n.y * scale;
	if (dot(n, n) < 0.000001) {
		return vec3<f32>(0.0, 0.0, 1.0);
	}
	n.z = sqrt(max(1.0 - min(dot(n.xy, n.xy), 1.0), 0.0));
	return n;
}

fn lil_blend_normal_ts(dst_normal: vec3<f32>, src_normal: vec3<f32>) -> vec3<f32> {
	return vec3<f32>(dst_normal.xy + src_normal.xy, dst_normal.z * src_normal.z);
}

@vertex
fn vs_outline(v: VsIn, @builtin(vertex_index) vertex_index: u32) -> VsOut {
	var o = skinned_position_normal(v, vertex_index);
	if (drawu.outline_params.x < 0.5 || drawu.outline_params.y <= 0.0) {
		return o;
	}
	let n = normalize(o.wn);
	let uv = animated_uv(o.uv);
	let mask = textureSampleLevel(outline_width_tex, outline_width_samp, uv, 0.0).r;
	let distance_fix = mix(1.0, clamp(length(frame.camera_pos.xyz - o.wp), 0.0, 1.0), clamp(drawu.outline_ext_params.x, 0.0, 1.0));
	let width = select(drawu.outline_params.y * 0.03, drawu.outline_params.y, drawu.outline_params.x < 1.5) * mask * distance_fix;
	let wp = vec4<f32>(o.wp + n * width, 1.0);
	o.clip = frame.view_proj * wp;
	o.clip.z = o.clip.z + drawu.outline_ext_params.y * o.clip.w;
	return o;
}

@vertex
fn vs_fur(v: VsIn, @builtin(vertex_index) vertex_index: u32, @builtin(instance_index) instance_index: u32) -> FurVsOut {
	var o = fur_vs_out_from_base(skinned_position_normal(v, vertex_index));
	let enabled = clamp(drawu.fur_params.x, 0.0, 1.0);
	let layer_count = max(drawu.fur_params.y, 1.0);
	let shell = (f32(instance_index) + 1.0) / layer_count;
	o.fur_shell = shell * enabled;
	if (enabled <= 0.000001 || drawu.fur_params.y <= 0.0) {
		return o;
	}
	let n = normalize(o.wn);
	let t = normalize(o.wt.xyz - n * dot(n, o.wt.xyz));
	let b = normalize(cross(n, t)) * o.wt.w;
	let fur_uv = main_uv_without_animation(o.uv);
	let length_mask = textureSampleLevel(fur_length_mask_tex, base_samp, fur_uv, 0.0).r;
	var fur_vector_ts = drawu.fur_vector_params.xyz + vec3<f32>(0.0, 0.0, 0.001);
	if (drawu.fur_rim_params.w > 0.5) {
		fur_vector_ts = lil_blend_normal_ts(fur_vector_ts, v.color.xyz);
	}
	if (drawu.fur_rim_params.z > 0.5) {
		let vector_tex = unpack_fur_vector_map(textureSampleLevel(fur_vector_tex, base_samp, fur_uv, 0.0), drawu.fur_ext_params.x);
		fur_vector_ts = lil_blend_normal_ts(fur_vector_ts, vector_tex);
	}
	var fur_vector_ws = normalize(t * fur_vector_ts.x + b * fur_vector_ts.y + n * fur_vector_ts.z) * drawu.fur_vector_params.w;
	let randomize = clamp(drawu.fur_params.w, 0.0, 1.0);
	if (randomize > 0.000001) {
		let seed = vec3<u32>(
			vertex_index * 3u,
			vertex_index * 5u + 1u,
			vertex_index * 7u + 2u,
		) * vec3<u32>(1597334677u, 3812015801u, 2912667907u);
		let random_dir = normalize(vec3<f32>(seed) * (2.0 / 4294967295.0) - vec3<f32>(1.0));
		fur_vector_ws = fur_vector_ws + random_dir * drawu.fur_vector_params.w * randomize;
	}
	let fur_length = length(fur_vector_ws);
	fur_vector_ws.y = fur_vector_ws.y - drawu.fur_params.z * fur_length;
	let wp = vec4<f32>(o.wp + fur_vector_ws * shell * enabled * length_mask, 1.0);
	o.wp = wp.xyz;
	o.fur_alpha = length_mask;
	o.fur_card_side = 0.0;
	o.fur_uv0 = v.uv;
	o.clip = frame.view_proj * wp;
	return o;
}

fn csfc_fur_vs(v: CsfcFurVsIn, cutout_pre: bool) -> FurVsOut {
	var o: FurVsOut;
	let local_position = select(v.position_layer.xyz, v.pre_position.xyz, cutout_pre);
	let center_wp = (drawt.model * vec4<f32>(local_position, 1.0)).xyz;
	let wn = normalize_or(mat3_upper(drawt.model) * v.normal_side.xyz, vec3<f32>(0.0, 1.0, 0.0));
	let view_dir = normalize_or(frame.camera_pos.xyz - center_wp, vec3<f32>(0.0, 0.0, 1.0));
	let side_dir = normalize_or(cross(view_dir, wn), normalize_or(cross(vec3<f32>(0.0, 1.0, 0.0), wn), vec3<f32>(1.0, 0.0, 0.0)));
	let side_width = v.normal_side.w;
	let world_pos = center_wp + side_dir * side_width;
	let wp = vec4<f32>(world_pos, 1.0);
	o.clip = frame.view_proj * wp;
	o.wn = wn;
	o.uv = v.uv;
	o.wp = world_pos;
	o.wt = vec4<f32>(1.0, 0.0, 0.0, 1.0);
	o.fur_shell = clamp(v.position_layer.w, 0.0, 1.0);
	o.fur_alpha = 1.0 + clamp(v.fur_alpha, 0.0, 1.0);
	o.fur_card_side = select(0.0, select(-1.0, 1.0, side_width >= 0.0), abs(side_width) > 0.0000001);
	o.fur_uv0 = v.uv;
	return o;
}

@vertex
fn vs_csfc_fur(v: CsfcFurVsIn) -> FurVsOut {
	return csfc_fur_vs(v, false);
}

@vertex
fn vs_csfc_fur_pre(v: CsfcFurVsIn) -> FurVsOut {
	return csfc_fur_vs(v, true);
}

const DBG_SOLID_PRIM_COLOR: u32 = 2u;
const DBG_DISABLE_RIM: u32 = 4u;
const DBG_FORCE_SHADING_SHIFT_ZERO: u32 = 8u;
const DBG_DISABLE_MATCAP: u32 = 16u;
const DBG_DISABLE_EMISSIVE: u32 = 32u;
const DBG_DISABLE_SHADE_COLOR: u32 = 64u;
const DBG_BASE_TEXTURE_ONLY: u32 = 128u;
const DBG_DISABLE_NORMAL_MAP: u32 = 256u;
const MAT_DOUBLE_SIDED: u32 = 512u;
const MAT_CULL_FRONT: u32 = 2048u;
const SRC_LILTOON: u32 = 4096u;
const SRC_LILTOON_GEM: u32 = 8192u;
const SRC_LILTOON_REFRACTION: u32 = 16384u;
const SRC_LILTOON_ADDITIVE_BLEND: u32 = 32768u;

/// MASK（Lit/Unlit）: ゲートはテクスチャ α のみ。
fn mask_discard_lit_unlit(alb: vec3<f32>, a: f32, alpha_kind: f32, cutoff: f32) {
	_ = alb;
	if alpha_kind > 0.5 && alpha_kind < 1.5 {
		if a < cutoff {
			discard;
		}
	}
}

/// Toon MASK: VRoid clothing often leaves RGB in fully transparent texels, so gate on alpha only.
fn mask_discard_toon(alb: vec3<f32>, a: f32, alpha_kind: f32, cutoff: f32) {
	_ = alb;
	if alpha_kind > 0.5 && alpha_kind < 1.5 {
		if a < cutoff {
			discard;
		}
	}
}

fn liltoon_cutout_alpha(a: f32, alpha_kind: f32, cutoff: f32, is_liltoon: bool) -> f32 {
	if is_liltoon && alpha_kind > 0.5 && alpha_kind < 1.5 {
		return clamp((a - cutoff) / max(fwidth(a), 0.0001) + 0.5, 0.0, 1.0);
	}
	return a;
}

fn liltoon_blend_discard(a: f32, alpha_kind: f32, cutoff: f32, is_liltoon: bool) {
	// lilToon's transparent fragment path still performs clip(alpha - _Cutoff)
	// before blending. Without this, atlas edge alpha becomes unintended
	// see-through cloth instead of discarded texels.
	if is_liltoon && alpha_kind > 1.5 {
		if a < cutoff {
			discard;
		}
	}
}

fn fragment_out_alpha(alpha_kind: f32, a: f32, base_color_a: f32) -> f32 {
	if alpha_kind > 1.5 {
		return clamp(a * max(drawu.alpha_mask_params.w, 0.0), 0.0, 1.0);
	}
	if alpha_kind > 0.5 && alpha_kind < 1.5 {
		return 1.0;
	}
	return base_color_a;
}

fn apply_lil_alpha_mask(a: f32, uv: vec2<f32>) -> f32 {
	let mode = i32(round(drawu.alpha_mask_params.x));
	if (mode <= 0) {
		return a;
	}
	let mask_uv = uv * drawu.alpha_mask_uv_offset_scale.zw + drawu.alpha_mask_uv_offset_scale.xy;
	let raw_mask = textureSample(alpha_mask_tex, alpha_mask_samp, mask_uv).r;
	let alpha_mask = clamp(raw_mask * drawu.alpha_mask_params.y + drawu.alpha_mask_params.z, 0.0, 1.0);
	if (mode == 1) {
		return alpha_mask;
	}
	if (mode == 2) {
		return a * alpha_mask;
	}
	if (mode == 3) {
		return clamp(a + alpha_mask, 0.0, 1.0);
	}
	if (mode == 4) {
		return clamp(a - alpha_mask, 0.0, 1.0);
	}
	return a;
}

fn fur_shell_alpha(uv: vec2<f32>, fur_uv0: vec2<f32>, shell: f32, length_mask: f32, card_side: f32, fur_cutout_pre: bool) -> f32 {
	if (length_mask > 1.0) {
		let csfc_alpha = clamp(length_mask - 1.0, 0.0, 1.0);
		let center_alpha = pow(1.0 - clamp(abs(card_side), 0.0, 1.0), 1.65);
		let fur_mask = textureSample(fur_mask_tex, base_samp, uv).r;
		let shell01 = clamp(shell, 0.0, 1.0);
		let noise_uv = fur_uv0 * max(drawu.fur_noise_params.xy, vec2<f32>(0.0001, 0.0001)) + drawu.fur_noise_params.zw;
		let fur_noise_mask = textureSample(fur_noise_mask_tex, base_samp, noise_uv).r;
		let root_offset = drawu.fur_ext_params.z;
		let fur_layer_shift = shell01 - shell01 * root_offset + root_offset;
		let fur_layer_abs = abs(fur_layer_shift);
		let layer_alpha = select(
			clamp(fur_noise_mask - fur_layer_shift * fur_layer_abs * fur_layer_abs, 0.0, 1.0),
			clamp(fur_noise_mask - fur_layer_shift * fur_layer_abs * fur_layer_abs * fur_layer_abs + 0.25, 0.0, 1.0),
			fur_cutout_pre,
		);
		return csfc_alpha * center_alpha * layer_alpha * fur_mask;
	}
	if (shell <= 0.0) {
		return 1.0;
	}
	let fur_mask = textureSample(fur_mask_tex, base_samp, uv).r;
	let noise_uv = fur_uv0 * max(drawu.fur_noise_params.xy, vec2<f32>(0.0001, 0.0001)) + drawu.fur_noise_params.zw;
	let fur_noise_mask = textureSample(fur_noise_mask_tex, base_samp, noise_uv).r;
	let root_offset = drawu.fur_ext_params.z;
	let fur_layer_shift = shell - shell * root_offset + root_offset;
	let fur_layer_abs = abs(fur_layer_shift);
	let fur_alpha = select(
		clamp(fur_noise_mask - fur_layer_shift * fur_layer_abs * fur_layer_abs, 0.0, 1.0),
		clamp(fur_noise_mask - fur_layer_shift * fur_layer_abs * fur_layer_abs * fur_layer_abs + 0.25, 0.0, 1.0),
		fur_cutout_pre,
	);
	return fur_alpha * fur_mask * clamp(length_mask, 0.0, 1.0);
}

fn fur_shell_ao(shell: f32, fur_uv0: vec2<f32>, fur_cutout_pre: bool) -> f32 {
	if (shell <= 0.0) {
		return 1.0;
	}
	let fur_ao = clamp(drawu.fur_ext_params.y, 0.0, 1.0);
	if (fur_cutout_pre) {
		let cutout_ao = fur_ao * clamp(1.0 - fwidth(shell), 0.0, 1.0);
		return shell * cutout_ao * 2.0 + 1.0 - cutout_ao;
	}
	let noise_uv = fur_uv0 * max(drawu.fur_noise_params.xy, vec2<f32>(0.0001, 0.0001)) + drawu.fur_noise_params.zw;
	let fur_noise_mask = textureSample(fur_noise_mask_tex, base_samp, noise_uv).r;
	return clamp(1.0 - fur_noise_mask + fur_noise_mask * shell, 0.0, 1.0) * fur_ao * 1.25 + 1.0 - fur_ao;
}

fn premultiply_when_blending(rgb: vec3<f32>, out_a: f32, alpha_kind: f32, premultiply: bool) -> vec3<f32> {
	if alpha_kind > 1.5 && premultiply {
		return rgb * out_a;
	}
	return rgb;
}

fn discard_invisible_transparent_zwrite(a: f32, alpha_kind: f32, transparent_zwrite: f32) {
	if alpha_kind > 1.5 && transparent_zwrite > 0.5 && a <= 0.001 {
		discard;
	}
}

fn discard_transparent_zprepass(a: f32, alpha_kind: f32, cutoff: f32, subpass_cutoff: f32, transparent_zwrite: f32) {
	// lilToon Transparent+ZWrite applies _Cutoff first, then _SubpassCutoff
	// for the transparent subpass. Keep depth consistent with the color pass.
	let pre_cutoff = drawu.alpha_ext_params.z;
	let z_cutoff = max(max(pre_cutoff, subpass_cutoff), 1.0 / 255.0);
	_ = cutoff;
	if alpha_kind > 1.5 && transparent_zwrite > 0.5 && a < z_cutoff {
		discard;
	}
}

fn discard_by_cull_mode(front_facing: bool, flags: u32) {
	if front_facing && (flags & MAT_CULL_FRONT) != 0u {
		discard;
	}
	if !front_facing && (flags & MAT_DOUBLE_SIDED) == 0u && (flags & MAT_CULL_FRONT) == 0u {
		discard;
	}
}

fn discard_by_liltoon_cull_factor(front_facing: bool, cull_factor: f32) {
	if cull_factor < 0.5 {
		return;
	}
	if cull_factor < 1.5 {
		if front_facing {
			discard;
		}
		return;
	}
	if !front_facing {
		discard;
	}
}

fn face_normal(n: vec3<f32>, front_facing: bool, flags: u32) -> vec3<f32> {
	if front_facing || (flags & MAT_DOUBLE_SIDED) == 0u {
		return n;
	}
	return -n;
}

fn toon_matcap_uv(n: vec3<f32>, v: vec3<f32>, perspective: f32) -> vec2<f32> {
	let camera_pos_len = length(frame.camera_pos.xyz);
	let camera_dir = select(vec3<f32>(0.0, 0.0, 1.0), normalize(frame.camera_pos.xyz), camera_pos_len >= 0.0001);
	let normal_vd = normalize(mix(camera_dir, v, clamp(perspective, 0.0, 1.0)));
	var world_view_x = normalize(vec3<f32>(normal_vd.z, 0.0, -normal_vd.x));
	if (length(world_view_x) < 0.0001) {
		world_view_x = vec3<f32>(1.0, 0.0, 0.0);
	}
	let world_view_y = cross(normal_vd, world_view_x);
	return vec2<f32>(dot(world_view_x, n), dot(world_view_y, n)) * 0.495 + vec2<f32>(0.5, 0.5);
}

fn toon_reflection_uv(n: vec3<f32>, v: vec3<f32>) -> vec2<f32> {
	let r = normalize(reflect(-v, n));
	let u = atan2(r.z, r.x) * 0.15915494309189535 + 0.5;
	let vv = acos(clamp(r.y, -1.0, 1.0)) * 0.3183098861837907;
	return vec2<f32>(u, vv);
}

fn screen_uv(clip: vec4<f32>) -> vec2<f32> {
	let ndc = clip.xy / max(clip.w, 0.000001);
	return vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
}

fn screen_normal_offset(world_pos: vec3<f32>, normal: vec3<f32>, base_uv: vec2<f32>) -> vec2<f32> {
	let shifted_clip = frame.view_proj * vec4<f32>(world_pos + normalize(normal) * 0.05, 1.0);
	let projected = screen_uv(shifted_clip) - base_uv;
	let fallback = vec2<f32>(normal.x, -normal.y) * 0.02;
	let use_fallback = length(projected) < 0.000001 || !all(projected == projected);
	return clamp(select(projected, fallback, use_fallback), vec2<f32>(-0.08), vec2<f32>(0.08));
}

fn liltoon_refraction_offset(normal: vec3<f32>) -> vec2<f32> {
	let view_normal = normalize((frame.view * vec4<f32>(normalize(normal), 0.0)).xyz);
	return vec2<f32>(view_normal.x, -view_normal.y);
}

fn linearstep(edge0: f32, edge1: f32, x: f32) -> f32 {
	return clamp((x - edge0) / max(edge1 - edge0, 0.00001), 0.0, 1.0);
}

fn lil_tooning_scale(value: f32, border: f32, blur: f32) -> f32 {
	let aa_blur = blur * max(drawu.alpha_ext_params.y, 0.0);
	if (aa_blur <= 0.00001) {
		return select(0.0, 1.0, value >= border);
	}
	let border_min = clamp(border - aa_blur * 0.5, 0.0, 1.0);
	let border_max = clamp(border + aa_blur * 0.5, 0.0, 1.0);
	return linearstep(border_min, border_max, value);
}

fn lil_tooning_scale_range(value: f32, border: f32, blur: f32, border_range: f32) -> f32 {
	let aa_blur = blur * max(drawu.alpha_ext_params.y, 0.0);
	if (aa_blur <= 0.00001 && border_range <= 0.00001) {
		return select(0.0, 1.0, value >= border);
	}
	let border_min = clamp(border - aa_blur * 0.5 - border_range, 0.0, 1.0);
	let border_max = clamp(border + aa_blur * 0.5, 0.0, 1.0);
	return linearstep(border_min, border_max, value);
}

fn lil_blend_color(dst: vec3<f32>, src: vec3<f32>, src_a: f32, blend_mode: f32) -> vec3<f32> {
	let add = dst + src;
	let mul = dst * src;
	var out_col = src;
	if (blend_mode >= 0.5 && blend_mode < 1.5) {
		out_col = add;
	} else if (blend_mode >= 1.5 && blend_mode < 2.5) {
		out_col = max(add - mul, dst);
	} else if (blend_mode >= 2.5) {
		out_col = mul;
	}
	return mix(dst, out_col, clamp(src_a, 0.0, 1.0));
}

fn lil_blend_color3(dst: vec3<f32>, src: vec3<f32>, src_a: vec3<f32>, blend_mode: f32) -> vec3<f32> {
	let add = dst + src;
	let mul = dst * src;
	var out_col = src;
	if (blend_mode >= 0.5 && blend_mode < 1.5) {
		out_col = add;
	} else if (blend_mode >= 1.5 && blend_mode < 2.5) {
		out_col = max(add - mul, dst);
	} else if (blend_mode >= 2.5) {
		out_col = mul;
	}
	return mix(dst, out_col, clamp(src_a, vec3<f32>(0.0), vec3<f32>(1.0)));
}

fn lil_blend_weighted_color(dst: vec3<f32>, weighted_src: vec3<f32>, src_a: f32, blend_mode: f32) -> vec3<f32> {
	let a = clamp(src_a, 0.0, 1.0);
	let src = select(vec3<f32>(0.0, 0.0, 0.0), weighted_src / max(a, 0.00001), a > 0.00001);
	return lil_blend_color(dst, src, a, blend_mode);
}

fn lil_backface_visibility(backface_mask: f32, front_facing: bool) -> f32 {
	let backface_visible = 1.0 - step(0.000001, clamp(backface_mask, 0.0, 1.0));
	return select(backface_visible, 1.0, front_facing);
}

fn fresnel_lerp(specular: vec3<f32>, grazing_term: f32, nv: f32) -> vec3<f32> {
	let f = pow(clamp(1.0 - nv, 0.0, 1.0), 5.0);
	return mix(specular, vec3<f32>(grazing_term), f);
}

fn fresnel_term(f0: vec3<f32>, cos_a: f32) -> vec3<f32> {
	let a = 1.0 - clamp(cos_a, 0.0, 1.0);
	return f0 + (vec3<f32>(1.0) - f0) * a * a * a * a * a;
}

fn rgb_to_hsv(c: vec3<f32>) -> vec3<f32> {
	let k = vec4<f32>(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);
	let p = mix(vec4<f32>(c.bg, k.wz), vec4<f32>(c.gb, k.xy), select(0.0, 1.0, c.b < c.g));
	let q = mix(vec4<f32>(p.xyw, c.r), vec4<f32>(c.r, p.yzx), select(0.0, 1.0, p.x < c.r));
	let d = q.x - min(q.w, q.y);
	let e = 0.0000001;
	return vec3<f32>(abs(q.z + (q.w - q.y) / (6.0 * d + e)), d / (q.x + e), q.x);
}

fn hsv_to_rgb(c: vec3<f32>) -> vec3<f32> {
	let p = abs(fract(c.xxx + vec3<f32>(0.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - vec3<f32>(3.0));
	return c.z * mix(vec3<f32>(1.0), clamp(p - vec3<f32>(1.0), vec3<f32>(0.0), vec3<f32>(1.0)), c.y);
}

fn apply_main_hsvg(color: vec3<f32>) -> vec3<f32> {
	let p = drawu.main_color_adjust_params;
	if (abs(p.x) + abs(p.y - 1.0) + abs(p.z - 1.0) + abs(p.w - 1.0) < 0.000001) {
		return color;
	}
	var hsv = rgb_to_hsv(max(color, vec3<f32>(0.0)));
	hsv.x = fract(hsv.x + p.x);
	hsv.y = clamp(hsv.y * p.y, 0.0, 1.0);
	hsv.z = max(hsv.z * p.z, 0.0);
	let rgb = hsv_to_rgb(hsv);
	return pow(max(rgb, vec3<f32>(0.0)), vec3<f32>(1.0 / max(p.w, 0.0001)));
}

fn apply_main_gradation(color: vec3<f32>) -> vec3<f32> {
	let strength = clamp(drawu.main_gradation_params.x * drawu.main_gradation_params.y, 0.0, 1.0);
	if (strength <= 0.000001) {
		return color;
	}
	let c = clamp(color, vec3<f32>(0.0), vec3<f32>(1.0));
	let mapped = vec3<f32>(
		textureSample(main_gradation_tex, base_samp, vec2<f32>(c.r, 0.5)).r,
		textureSample(main_gradation_tex, base_samp, vec2<f32>(c.g, 0.5)).g,
		textureSample(main_gradation_tex, base_samp, vec2<f32>(c.b, 0.5)).b
	);
	return mix(color, mapped, strength);
}

fn apply_lil_main_layer_alpha(base_a: f32, layer_a: f32, alpha_mode: f32) -> f32 {
	if (alpha_mode < 0.5) {
		return base_a;
	}
	if (alpha_mode < 1.5) {
		return layer_a;
	}
	if (alpha_mode < 2.5) {
		return base_a * layer_a;
	}
	if (alpha_mode < 3.5) {
		return clamp(base_a + layer_a, 0.0, 1.0);
	}
	return clamp(base_a - layer_a, 0.0, 1.0);
}

fn apply_lil_main_layers(base: vec4<f32>, uv: vec2<f32>) -> vec4<f32> {
	var out_col = base;
	if (drawu.main2nd_params.x > 0.5) {
		let layer_uv = uv * drawu.main2nd_uv_offset_scale.zw + drawu.main2nd_uv_offset_scale.xy;
		let mask_uv = uv * drawu.main2nd_blend_mask_uv_offset_scale.zw + drawu.main2nd_blend_mask_uv_offset_scale.xy;
		let layer = textureSample(main2nd_tex, base_samp, layer_uv) * drawu.main2nd_color;
		let layer_alpha = layer.a * textureSample(main2nd_blend_mask_tex, base_samp, mask_uv).r;
		let out_alpha = apply_lil_main_layer_alpha(out_col.a, layer_alpha, drawu.main2nd_params.z);
		let out_rgb = lil_blend_color(out_col.rgb, layer.rgb, layer_alpha * drawu.main2nd_params.y, drawu.main2nd_params.w);
		out_col = vec4<f32>(out_rgb, out_alpha);
	}
	if (drawu.main3rd_params.x > 0.5) {
		let layer_uv = uv * drawu.main3rd_uv_offset_scale.zw + drawu.main3rd_uv_offset_scale.xy;
		let mask_uv = uv * drawu.main3rd_blend_mask_uv_offset_scale.zw + drawu.main3rd_blend_mask_uv_offset_scale.xy;
		let layer = textureSample(main3rd_tex, base_samp, layer_uv) * drawu.main3rd_color;
		let layer_alpha = layer.a * textureSample(main3rd_blend_mask_tex, base_samp, mask_uv).r;
		let out_alpha = apply_lil_main_layer_alpha(out_col.a, layer_alpha, drawu.main3rd_params.z);
		let out_rgb = lil_blend_color(out_col.rgb, layer.rgb, layer_alpha * drawu.main3rd_params.y, drawu.main3rd_params.w);
		out_col = vec4<f32>(out_rgb, out_alpha);
	}
	return out_col;
}

fn animated_uv(uv: vec2<f32>) -> vec2<f32> {
	let base_uv = uv * drawu.uv_offset_scale.zw + drawu.uv_offset_scale.xy;
	let speed = drawu.uv_anim_params.xyz;
	if (abs(speed.x) + abs(speed.y) + abs(speed.z) < 0.000001) {
		return base_uv;
	}
	let mask = textureSampleLevel(uv_anim_mask_tex, uv_anim_mask_samp, base_uv, 0.0).r;
	let t = frame.time_params.x;
	var out_uv = base_uv + speed.xy * t;
	let angle = speed.z * t;
	let s = sin(angle);
	let c = cos(angle);
	let centered = out_uv - vec2<f32>(0.5, 0.5);
	out_uv = vec2<f32>(centered.x * c - centered.y * s, centered.x * s + centered.y * c) + vec2<f32>(0.5, 0.5);
	return mix(base_uv, out_uv, clamp(mask, 0.0, 1.0));
}

fn main_uv_without_animation(uv: vec2<f32>) -> vec2<f32> {
	return uv * drawu.uv_offset_scale.zw + drawu.uv_offset_scale.xy;
}

fn lil_calc_uv_scroll_rotate(uv: vec2<f32>, offset_scale: vec4<f32>, scroll_rotate: vec4<f32>) -> vec2<f32> {
	var out_uv = uv * offset_scale.zw + offset_scale.xy;
	let angle = scroll_rotate.z + scroll_rotate.w * frame.time_params.x;
	let s = sin(angle);
	let c = cos(angle);
	let centered = out_uv - vec2<f32>(0.5, 0.5);
	out_uv = vec2<f32>(centered.x * c - centered.y * s, centered.x * s + centered.y * c) + vec2<f32>(0.5, 0.5);
	return out_uv + fract(scroll_rotate.xy * frame.time_params.x);
}

fn lil_calc_blink(blink: vec4<f32>) -> f32 {
	var out_blink = sin(frame.time_params.x * blink.z + blink.w) * 0.5 + 0.5;
	if (blink.y > 0.5) {
		out_blink = round(out_blink);
	}
	return mix(1.0, out_blink, clamp(blink.x, 0.0, 1.0));
}

fn lil_parallax_offset(n: vec3<f32>, tangent_in: vec4<f32>, v: vec3<f32>) -> vec2<f32> {
	let tangent_ortho = tangent_in.xyz - n * dot(n, tangent_in.xyz);
	let t = normalize(tangent_ortho);
	let b = normalize(cross(n, t)) * tangent_in.w;
	let view_ts = vec3<f32>(dot(v, t), dot(v, b), max(dot(v, n), 0.0001));
	return view_ts.xy / (view_ts.z + 0.5);
}

fn normal_mapped(n_in: vec3<f32>, tangent_in: vec4<f32>, uv: vec2<f32>, scale: f32) -> vec3<f32> {
	let n = normalize(n_in);
	if (abs(scale) < 0.000001) {
		return n;
	}
	let normal_uv = uv * drawu.normal_uv_offset_scale.zw + drawu.normal_uv_offset_scale.xy;
	let packed = textureSample(normal_tex, normal_samp, normal_uv).xyz;
	var tn = packed * 2.0 - vec3<f32>(1.0, 1.0, 1.0);
	tn.x = tn.x * scale;
	tn.y = tn.y * scale;
	if (drawu.normal2nd_params.x > 0.5) {
		let normal2nd_uv = uv * drawu.normal2nd_uv_offset_scale.zw + drawu.normal2nd_uv_offset_scale.xy;
		let packed2 = textureSample(normal2nd_tex, normal_samp, normal2nd_uv).xyz;
		var tn2 = packed2 * 2.0 - vec3<f32>(1.0, 1.0, 1.0);
		tn2.x = tn2.x * drawu.normal2nd_params.y;
		tn2.y = tn2.y * drawu.normal2nd_params.y;
		tn = vec3<f32>(tn.xy + tn2.xy, tn.z * tn2.z);
	}
	tn = normalize(tn);

	let tangent_ortho = tangent_in.xyz - n * dot(n, tangent_in.xyz);
	let t = normalize(tangent_ortho);
	let b = normalize(cross(n, t)) * tangent_in.w;
	return normalize(t * tn.x + b * tn.y + n * tn.z);
}

struct AnisotropyBasis {
	normal: vec3<f32>,
	tangent: vec3<f32>,
	amount: f32,
	shift_noise: f32,
	enabled: f32,
}

fn lil_anisotropy_basis(n: vec3<f32>, tangent_in: vec4<f32>, uv: vec2<f32>, v: vec3<f32>) -> AnisotropyBasis {
	let enabled = clamp(drawu.anisotropy_params.x, 0.0, 1.0);
	if (enabled <= 0.000001) {
		return AnisotropyBasis(n, n, 0.0, 0.0, 0.0);
	}
	let base_tangent = normalize(tangent_in.xyz - n * dot(n, tangent_in.xyz));
	let base_bitangent = normalize(cross(n, base_tangent)) * tangent_in.w;
	let tangent_uv = uv * drawu.anisotropy_tangent_uv_offset_scale.zw + drawu.anisotropy_tangent_uv_offset_scale.xy;
	var tangent_sample = textureSample(anisotropy_tangent_tex, normal_samp, tangent_uv).xyz * 2.0 - vec3<f32>(1.0, 1.0, 1.0);
	if (dot(tangent_sample, tangent_sample) < 0.000001) {
		tangent_sample = vec3<f32>(1.0, 0.0, 0.0);
	}
	var aniso_t = normalize(base_tangent * tangent_sample.x + base_bitangent * tangent_sample.y + n * tangent_sample.z);
	aniso_t = normalize(aniso_t - n * dot(n, aniso_t));
	let aniso_b = normalize(cross(n, aniso_t)) * tangent_in.w;
	let scale_uv = uv * drawu.anisotropy_scale_mask_uv_offset_scale.zw + drawu.anisotropy_scale_mask_uv_offset_scale.xy;
	let scale_mask = textureSample(anisotropy_scale_mask_tex, base_samp, scale_uv).r;
	let anisotropy = drawu.anisotropy_params.y * scale_mask;
	let shift_axis = select(aniso_b, aniso_t, anisotropy >= 0.0);
	let aniso_n = normalize(n + shift_axis * clamp(abs(anisotropy), 0.0, 1.0) * max(0.15, 1.0 - abs(dot(n, v))));
	let noise_uv = uv * drawu.anisotropy_shift_noise_uv_offset_scale.zw + drawu.anisotropy_shift_noise_uv_offset_scale.xy;
	let shift_noise = textureSample(anisotropy_shift_noise_tex, base_samp, noise_uv).r - 0.5;
	return AnisotropyBasis(aniso_n, aniso_t, clamp(anisotropy, -1.0, 1.0), shift_noise, enabled);
}

fn lil_anisotropic_specular_shape(n: vec3<f32>, t: vec3<f32>, half_vec: vec3<f32>, tangent_width: f32, bitangent_width: f32, shift: f32, roughness: f32) -> f32 {
	let shifted_t = normalize(t + n * shift);
	let shifted_b = normalize(cross(n, shifted_t));
	let rough = max(roughness, 0.02);
	let tw = max(tangent_width * rough, 0.02);
	let bw = max(bitangent_width * rough, 0.02);
	let t_term = dot(shifted_t, half_vec) / tw;
	let b_term = dot(shifted_b, half_vec) / bw;
	let nh = max(dot(n, half_vec), 0.0);
	return exp(-clamp(t_term * t_term + b_term * b_term, 0.0, 80.0)) * nh;
}

fn authored_occlusion(uv: vec2<f32>, dbg: u32) -> f32 {
	if ((dbg & 1024u) == 0u) {
		return 1.0;
	}
	let strength = clamp(drawu.rim_color.w, 0.0, 2.0);
	let sample = textureSample(occlusion_tex, occlusion_samp, uv).r;
	return clamp(mix(1.0, sample, strength), 0.0, 1.0);
}

fn lil_direct_light_color() -> vec3<f32> {
	let raw = frame.light_color.rgb * frame.light_color.w;
	let luminance = dot(raw, vec3<f32>(0.2126, 0.7152, 0.0722));
	let monochrome = mix(raw, vec3<f32>(luminance, luminance, luminance), clamp(drawu.lighting_ext_params.z, 0.0, 1.0));
	let min_limit = max(drawu.lighting_ext_params.x, 0.0);
	let max_limit = max(drawu.lighting_ext_params.y, min_limit);
	return clamp(monochrome, vec3<f32>(min_limit, min_limit, min_limit), vec3<f32>(max_limit, max_limit, max_limit));
}

@fragment
fn fs_lit(i: VsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
	let dbg = bitcast<u32>(drawu.params.w);
	discard_by_cull_mode(front_facing, dbg);
	let uv = animated_uv(i.uv);
	let samp_tex = textureSample(tex, base_samp, uv);
	let main_rgb = apply_main_gradation(apply_main_hsvg(samp_tex.rgb));
	let main_col = apply_lil_main_layers(vec4<f32>(main_rgb * drawu.base_color.rgb, samp_tex.a * drawu.base_color.a), uv);
	let a = apply_lil_alpha_mask(main_col.a, uv);
	let alpha_kind = drawu.params.y;
	let cutoff = drawu.params.z;
	mask_discard_lit_unlit(main_col.rgb, a, alpha_kind, cutoff);
	discard_invisible_transparent_zwrite(a, alpha_kind, drawu.outline_params.w);
	let out_a = fragment_out_alpha(alpha_kind, a, drawu.base_color.a);
	let base = select(main_col.rgb, drawu.base_color.rgb, (dbg & DBG_SOLID_PRIM_COLOR) != 0u);
	let l = normalize(frame.light_dir.xyz);
	let normal_scale = select(drawu.shade_color.w, 0.0, (dbg & DBG_DISABLE_NORMAL_MAP) != 0u);
	let n = face_normal(normal_mapped(i.wn, i.wt, i.uv, normal_scale), front_facing, dbg);
	let ndl = max(dot(n, l), 0.0);
	let ambient = frame.ambient_color.rgb * (frame.ambient_color.w * 0.57);
	let direct = lil_direct_light_color() * (0.8 * ndl);
	let lit = base * (ambient + direct) * authored_occlusion(uv, dbg);
	return vec4<f32>(lit, out_a);
}

@fragment
fn fs_unlit(i: VsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
	let dbg = bitcast<u32>(drawu.params.w);
	discard_by_cull_mode(front_facing, dbg);
	let uv = animated_uv(i.uv);
	let samp_tex = textureSample(tex, base_samp, uv);
	let main_rgb = apply_main_gradation(apply_main_hsvg(samp_tex.rgb));
	let main_col = apply_lil_main_layers(vec4<f32>(main_rgb * drawu.base_color.rgb, samp_tex.a * drawu.base_color.a), uv);
	let a = apply_lil_alpha_mask(main_col.a, uv);
	let alpha_kind = drawu.params.y;
	let cutoff = drawu.params.z;
	mask_discard_lit_unlit(main_col.rgb, a, alpha_kind, cutoff);
	discard_invisible_transparent_zwrite(a, alpha_kind, drawu.outline_params.w);
	let out_a = fragment_out_alpha(alpha_kind, a, drawu.base_color.a);
	let base = select(main_col.rgb, drawu.base_color.rgb, (dbg & DBG_SOLID_PRIM_COLOR) != 0u);
	return vec4<f32>(base, out_a);
}

fn toon_fragment(i: VsOut, front_facing: bool, use_transparent_prepass: bool, fur_shell: f32, fur_alpha_in: f32, fur_card_side: f32, fur_cutout_pre: bool, fur_uv0: vec2<f32>) -> vec4<f32> {
	let dbg = bitcast<u32>(drawu.params.w);
	let is_liltoon = (dbg & SRC_LILTOON) != 0u;
	let is_liltoon_gem = (dbg & SRC_LILTOON_GEM) != 0u;
	let is_liltoon_refraction = (dbg & SRC_LILTOON_REFRACTION) != 0u;
	let is_liltoon_additive_blend = (dbg & SRC_LILTOON_ADDITIVE_BLEND) != 0u;
	if use_transparent_prepass {
		discard_by_liltoon_cull_factor(front_facing, drawu.alpha_ext_params.w);
	} else {
		discard_by_cull_mode(front_facing, dbg);
	}
	let uv = animated_uv(i.uv);
	let samp_tex = textureSample(tex, base_samp, uv);
	let main_rgb = apply_main_gradation(apply_main_hsvg(samp_tex.rgb));
	let main_col = apply_lil_main_layers(vec4<f32>(main_rgb * drawu.base_color.rgb, samp_tex.a * drawu.base_color.a), uv);
	let a = apply_lil_alpha_mask(main_col.a, uv);
	let fur_alpha = fur_shell_alpha(uv, fur_uv0, fur_shell, fur_alpha_in, fur_card_side, fur_cutout_pre);
	if (fur_shell > 0.0 && fur_alpha <= 0.015) {
		discard;
	}
	let alpha_kind = drawu.params.y;
	let cutoff = drawu.params.z;
	let is_fur_pass = fur_shell > 0.0;
	if (is_fur_pass && (alpha_kind > 0.5 && alpha_kind < 1.5 || fur_cutout_pre)) {
		if (a * fur_alpha <= 0.4) {
			discard;
		}
	} else if (is_liltoon && alpha_kind > 0.5 && alpha_kind < 1.5) {
		if (liltoon_cutout_alpha(a, alpha_kind, cutoff, is_liltoon) <= 0.0) {
			discard;
		}
	} else {
		mask_discard_toon(main_col.rgb, a, alpha_kind, cutoff);
	}
	if (!is_fur_pass) {
		liltoon_blend_discard(a, alpha_kind, cutoff, is_liltoon && !is_liltoon_gem);
	}
	if use_transparent_prepass {
		discard_transparent_zprepass(a, alpha_kind, cutoff, drawu.alpha_ext_params.x, drawu.outline_params.w);
	}
	discard_invisible_transparent_zwrite(a, alpha_kind, drawu.outline_params.w);
	var out_a = fragment_out_alpha(alpha_kind, a, drawu.base_color.a) * fur_alpha;
	if (is_fur_pass && (alpha_kind > 0.5 && alpha_kind < 1.5 || fur_cutout_pre)) {
		out_a = clamp(a * fur_alpha * 5.0 - 2.0, 0.0, 1.0);
	} else if (is_liltoon && alpha_kind > 0.5 && alpha_kind < 1.5) {
		out_a = liltoon_cutout_alpha(a, alpha_kind, cutoff, is_liltoon) * fur_alpha;
	}
	let compute_fur = fur_shell > 0.0 && fur_alpha_in > 1.0;
	if (fur_shell > 0.0) {
		if (fur_cutout_pre || alpha_kind > 0.5 && alpha_kind < 1.5) {
			if (out_a <= 0.0) {
				discard;
			}
		} else {
			if (out_a <= max(cutoff, 1.0 / 255.0)) {
				discard;
			}
		}
	}
	let base = select(main_col.rgb, drawu.base_color.rgb, (dbg & DBG_SOLID_PRIM_COLOR) != 0u) * fur_shell_ao(fur_shell, fur_uv0, fur_cutout_pre);
	if ((dbg & DBG_BASE_TEXTURE_ONLY) != 0u) {
		// 診断用: shading / GI / matcap / rim / emissive / shade_term を全てスキップして base のみ。
		// リングがまだ残るならテクスチャ自身（モデル制作者が描いた肌グラデ）かメッシュ重なり由来。
		return vec4<f32>(premultiply_when_blending(max(base, vec3<f32>(0.0, 0.0, 0.0)), out_a, alpha_kind, !compute_fur && !fur_cutout_pre && !is_liltoon_additive_blend), out_a);
	}
	let normal_scale = select(drawu.shade_color.w, 0.0, (dbg & DBG_DISABLE_NORMAL_MAP) != 0u || is_fur_pass);
	let geometry_n_faced = face_normal(normalize(i.wn), front_facing, dbg);
	let n_faced = face_normal(normal_mapped(i.wn, i.wt, i.uv, normal_scale), front_facing, dbg);
	let l = normalize(frame.light_dir.xyz);
	let v = normalize(frame.camera_pos.xyz - i.wp);
	let gem_backface_normal = is_liltoon_gem && !front_facing;
	let geometry_n = select(geometry_n_faced, normalize(geometry_n_faced - v * 0.2), gem_backface_normal);
	let n = select(n_faced, normalize(n_faced - v * 0.2), gem_backface_normal);
	let shadow_n = normalize(mix(geometry_n, n, clamp(drawu.shadow_ext_params.w, 0.0, 1.0)));
	let shadow2_n = normalize(mix(geometry_n, n, clamp(drawu.shadow2_params.z, 0.0, 1.0)));
	let shadow3_n = normalize(mix(geometry_n, n, clamp(drawu.shadow3_params.z, 0.0, 1.0)));
	let parallax_offset = lil_parallax_offset(n, i.wt, v);
	let gem_view = v;
	let anisotropy_basis = lil_anisotropy_basis(n, i.wt, uv, v);
	let anisotropy_n = anisotropy_basis.normal;

	let force_shift_zero = (dbg & DBG_FORCE_SHADING_SHIFT_ZERO) != 0u;
	var shading: f32;
	if (drawu.shadow_params.x > 0.5) {
		let shadow_strength_mask_uv = uv * drawu.shadow_strength_mask_uv_offset_scale.zw + drawu.shadow_strength_mask_uv_offset_scale.xy;
		let shadow_border_mask_uv = uv * drawu.shadow_border_mask_uv_offset_scale.zw + drawu.shadow_border_mask_uv_offset_scale.xy;
		let shadow_blur_mask_uv = uv * drawu.shadow_blur_mask_uv_offset_scale.zw + drawu.shadow_blur_mask_uv_offset_scale.xy;
		let shadow_border_mask = textureSample(shadow_border_mask_tex, shadow_border_mask_samp, shadow_border_mask_uv).r;
		let shadow_blur_mask = textureSample(shadow_blur_mask_tex, shadow_blur_mask_samp, shadow_blur_mask_uv).r;
		let lil_shadow_value = (dot(shadow_n, l) * 0.5 + 0.5) * shadow_border_mask;
		let shadow_strength_mask = textureSample(shading_shift_tex, shading_shift_samp, shadow_strength_mask_uv).r;
		let lil_shadow = lil_tooning_scale_range(
			lil_shadow_value,
			clamp(drawu.shadow_params.z, 0.0, 1.0),
			clamp(drawu.shadow_params.w * shadow_blur_mask, 0.0, 1.0),
			clamp(drawu.shadow_ext_params.x, 0.0, 1.0),
		);
		shading = mix(1.0, lil_shadow, clamp(drawu.shadow_params.y * shadow_strength_mask, 0.0, 1.0));
	} else {
		let raw_shift_tex_value = textureSample(shading_shift_tex, shading_shift_samp, uv).r * drawu.shading_params.z;
		let shift_tex_value = select(raw_shift_tex_value, 0.0, force_shift_zero);
		let shading_shift_factor = select(drawu.shading_params.x, 0.0, force_shift_zero);
		shading = dot(n, l) + shading_shift_factor + shift_tex_value;
		// VRMC_materials_mtoon-1.0 の shading transition は linearstep(-1 + toony, 1 - toony, shading)。
		// boundary は `1 - toony` の線形（旧実装の `1 - toony²` だと境界幅が約 2 倍広くなっていた）。
		let toony_st = clamp(drawu.shading_params.y, 0.0, 1.0);
		let toony_boundary = 1.0 - toony_st;
		shading = linearstep(-toony_boundary, toony_boundary, shading);
	}
	let disable_shade_color = (dbg & DBG_DISABLE_SHADE_COLOR) != 0u;
	let shade_uv = uv * drawu.shade_uv_offset_scale.zw + drawu.shade_uv_offset_scale.xy;
	let shade_texel = textureSample(shade_tex, shade_samp, shade_uv);
	let mtoon_shade_term_raw = drawu.shade_color.rgb * shade_texel.rgb;
	let lil_shadow_term_raw = mix(base, shade_texel.rgb, clamp(shade_texel.a, 0.0, 1.0)) * drawu.shade_color.rgb;
	let shade_term_raw = select(mtoon_shade_term_raw, lil_shadow_term_raw, is_liltoon);
	let shade_term = select(shade_term_raw, base, disable_shade_color);
	var lit: vec3<f32>;
	if (drawu.shadow_params.x > 0.5) {
		let light_color = lil_direct_light_color();
		let direct_col = base * light_color;
		var indirect_col = shade_term * light_color;
		let shadow2_value = dot(shadow2_n, l) * 0.5 + 0.5;
		let shadow2_color_texel = textureSample(shadow2_color_tex, shade_samp, shade_uv);
		let shadow2_color = mix(base, shadow2_color_texel.rgb, clamp(shadow2_color_texel.a, 0.0, 1.0)) * drawu.shadow2_color.rgb;
		let shadow2 = lil_tooning_scale_range(
			shadow2_value,
			clamp(drawu.shadow2_params.x, 0.0, 1.0),
			clamp(drawu.shadow2_params.y, 0.0, 1.0),
			clamp(drawu.shadow_ext_params.x, 0.0, 1.0),
		);
		let shadow2_strength = clamp((1.0 - shadow2) * drawu.shadow2_color.a, 0.0, 1.0);
		indirect_col = mix(indirect_col, shadow2_color * light_color, shadow2_strength);
		let shadow3_value = dot(shadow3_n, l) * 0.5 + 0.5;
		let shadow3_color_texel = textureSample(shadow3_color_tex, shade_samp, shade_uv);
		let shadow3_color = mix(base, shadow3_color_texel.rgb, clamp(shadow3_color_texel.a, 0.0, 1.0)) * drawu.shadow3_color.rgb;
		let shadow3 = lil_tooning_scale_range(
			shadow3_value,
			clamp(drawu.shadow3_params.x, 0.0, 1.0),
			clamp(drawu.shadow3_params.y, 0.0, 1.0),
			clamp(drawu.shadow_ext_params.x, 0.0, 1.0),
		);
		let shadow3_strength = clamp((1.0 - shadow3) * drawu.shadow3_color.a, 0.0, 1.0);
		indirect_col = mix(indirect_col, shadow3_color * light_color, shadow3_strength);
		indirect_col = mix(indirect_col, indirect_col * base, clamp(drawu.shadow_ext_params.y, 0.0, 1.0));
		indirect_col = mix(
			indirect_col,
			base,
			clamp(max(max(frame.ambient_color.r, frame.ambient_color.g), frame.ambient_color.b) * frame.ambient_color.w * drawu.shadow_ext_params.z, 0.0, 1.0),
		);
		indirect_col = min(indirect_col, direct_col);
		let border_mix = lil_tooning_scale_range(
			dot(shadow_n, l) * 0.5 + 0.5,
			clamp(drawu.shadow_params.z, 0.0, 1.0),
			clamp(drawu.shadow_params.w, 0.0, 1.0),
			clamp(drawu.shadow_ext_params.x, 0.0, 1.0),
		);
		indirect_col = mix(indirect_col, direct_col, border_mix * drawu.shadow_border_color.rgb);
		lit = mix(indirect_col, direct_col, shading) * authored_occlusion(uv, dbg);
	} else {
		let direct_color = mix(shade_term, base, shading) * frame.light_color.rgb * frame.light_color.w;

		// VRM0/UniVRM adds indirect light after the toon shade mix and clamps it by
		// the lit color. This keeps white clothes from falling into large faceted
		// gray patches while still preserving authored shade colors.
		let gi_equalization = clamp(drawu.shading_params.w, 0.0, 1.0);
		let indirect_light = mix(shade_term, base, gi_equalization) * frame.ambient_color.rgb * frame.ambient_color.w;
		lit = min(direct_color + indirect_light, base) * authored_occlusion(uv, dbg);
	}
	if (is_liltoon_gem) {
		lit = base * clamp(abs(dot(n, v)), 0.0, 1.0) * 0.75;
	}

	let disable_matcap = (dbg & DBG_DISABLE_MATCAP) != 0u;
	let disable_rim = (dbg & DBG_DISABLE_RIM) != 0u;
	if (is_fur_pass) {
		if (is_liltoon && drawu.rim_shade_params.x > 0.5) {
			let rim_shade_n = normalize(mix(geometry_n, n, clamp(drawu.rim_ext_params.w, 0.0, 1.0)));
			let rim_shade_raw = pow(clamp(1.0 - abs(dot(rim_shade_n, v)), 0.0, 1.0), max(drawu.rim_shade_params.w, 0.00001));
			let rim_shade_mask = textureSample(rim_shade_mask_tex, rim_samp, uv).r;
			let rim_shade = lil_tooning_scale(
				rim_shade_raw,
				clamp(drawu.rim_shade_params.y, 0.0, 1.0),
				clamp(drawu.rim_shade_params.z, 0.0, 1.0)
			) * rim_shade_mask * clamp(drawu.rim_shade_color.a, 0.0, 1.0);
			lit = mix(lit, lit * drawu.rim_shade_color.rgb, rim_shade);
		}
		if (!disable_rim) {
			let fur_rim_raw = pow(clamp(1.0 - abs(dot(normalize(n), v)), 0.0, 1.0), max(drawu.fur_rim_params.x, 0.00001));
			let inv_lighting = clamp(vec3<f32>(1.0) / max(lil_direct_light_color() + frame.ambient_color.rgb * frame.ambient_color.w, vec3<f32>(0.25)), vec3<f32>(1.0), vec3<f32>(4.0));
			let fur_rim_anti_light = mix(1.0, dot(inv_lighting, vec3<f32>(1.0 / 3.0)), clamp(drawu.fur_rim_params.y, 0.0, 1.0));
			lit = lit + clamp(fur_shell, 0.0, 1.0) * fur_rim_raw * fur_rim_anti_light * drawu.fur_rim_color.rgb * lil_direct_light_color();
		}
		return vec4<f32>(premultiply_when_blending(max(lit, vec3<f32>(0.0, 0.0, 0.0)), out_a, alpha_kind, !compute_fur && !fur_cutout_pre && !is_liltoon_additive_blend), out_a);
	}
	var specular = vec3<f32>(0.0, 0.0, 0.0);
	var specular_blend = vec3<f32>(0.0, 0.0, 0.0);
	var authored_reflection = vec3<f32>(0.0, 0.0, 0.0);
	var authored_reflection_blend = vec3<f32>(0.0, 0.0, 0.0);
	var authored_reflection_env = vec3<f32>(0.0, 0.0, 0.0);
	let half_vec = normalize(l + v);
	let specular_base_n = normalize(mix(geometry_n, n, clamp(drawu.specular_toon_params.w, 0.0, 1.0)));
	let reflection_base_n = normalize(mix(geometry_n, n, clamp(drawu.reflection_params.w, 0.0, 1.0)));
	let specular_n = normalize(mix(specular_base_n, anisotropy_n, clamp(drawu.anisotropy_params.z * anisotropy_basis.enabled, 0.0, 1.0)));
	let reflection_n = normalize(mix(reflection_base_n, anisotropy_n, clamp(drawu.anisotropy_params.z * anisotropy_basis.enabled, 0.0, 1.0)));
	let reflection_dir = normalize(reflect(-v, reflection_n));
	let reflection_fresnel = pow(clamp(1.0 - dot(reflection_n, v), 0.0, 1.0), 2.0);
	var reflection_metallic = 0.0;
	if (!is_liltoon_gem && drawu.reflection_control.x > 0.5) {
		let reflection_color_uv = uv * drawu.reflection_color_uv_offset_scale.zw + drawu.reflection_color_uv_offset_scale.xy;
		let smoothness_uv = uv * drawu.smoothness_uv_offset_scale.zw + drawu.smoothness_uv_offset_scale.xy;
		let metallic_uv = uv * drawu.metallic_uv_offset_scale.zw + drawu.metallic_uv_offset_scale.xy;
		let reflection_color_texel = textureSample(reflection_color_tex, reflection_color_samp, reflection_color_uv);
		let smoothness = clamp(drawu.reflection_params.x * textureSample(smoothness_tex, smoothness_samp, smoothness_uv).r, 0.0, 1.0);
		let metallic = clamp(drawu.reflection_params.y * textureSample(metallic_tex, metallic_samp, metallic_uv).r, 0.0, 1.0);
		reflection_metallic = metallic;
		let base_perceptual_roughness = max(1.0 - smoothness, 0.02);
		let aniso_perceptual_roughness = max(1.2 - abs(anisotropy_basis.amount), 0.02);
		let perceptual_roughness = mix(base_perceptual_roughness, aniso_perceptual_roughness, clamp(drawu.anisotropy_params.z * anisotropy_basis.enabled, 0.0, 1.0));
		let roughness = perceptual_roughness * perceptual_roughness;
		let reflectance = clamp(drawu.reflection_params.z, 0.0, 1.0);
		let specular_color = mix(vec3<f32>(reflectance, reflectance, reflectance), base, metallic);
		let nh = max(dot(specular_n, half_vec), 0.0);
		let nv_spec = max(dot(specular_n, v), 0.0);
		let nl_spec = max(dot(specular_n, l), 0.0);
		let lh = max(dot(l, half_vec), 0.0);
		var specular_reflect = vec3<f32>(0.0);
		if (drawu.specular_toon_params.x > 0.5) {
			let toon_specular = pow(nh, 1.0 / max(roughness, 0.0004));
			let specular_shape = lil_tooning_scale(
				toon_specular,
				clamp(drawu.specular_toon_params.y, 0.0, 1.0),
				clamp(drawu.specular_toon_params.z, 0.0, 1.0)
			);
			specular_reflect = vec3<f32>(specular_shape);
		} else if (anisotropy_basis.enabled > 0.5 && drawu.anisotropy_params.z > 0.5) {
			let shift1 = anisotropy_basis.shift_noise * drawu.anisotropy_ext_params.z + drawu.anisotropy_ext_params.y;
			let shift2 = anisotropy_basis.shift_noise * drawu.anisotropy2_params.y + drawu.anisotropy2_params.x;
			let aniso1 = lil_anisotropic_specular_shape(
				specular_n,
				anisotropy_basis.tangent,
				half_vec,
				drawu.anisotropy_width_params.x,
				drawu.anisotropy_width_params.y,
				shift1,
				roughness
			) * drawu.anisotropy_ext_params.w;
			let aniso2 = lil_anisotropic_specular_shape(
				specular_n,
				anisotropy_basis.tangent,
				half_vec,
				drawu.anisotropy_width_params.z,
				drawu.anisotropy_width_params.w,
				shift2,
				roughness
			) * drawu.anisotropy2_params.z;
			let specular_shape = max(aniso1, aniso2) * nl_spec;
			specular_reflect = specular_shape * fresnel_term(specular_color, lh);
		} else {
			let roughness2 = max(roughness, 0.002);
			let lambda_v = nl_spec * (nv_spec * (1.0 - roughness2) + roughness2);
			let lambda_l = nv_spec * (nl_spec * (1.0 - roughness2) + roughness2);
			let r2 = roughness2 * roughness2;
			let d = (nh * r2 - nh) * nh + 1.0;
			let ggx = r2 / (d * d + 0.0000001);
			let smith_joint_ggx = 0.5 / (lambda_v + lambda_l + 0.00001);
			let specular_term = smith_joint_ggx * ggx * nl_spec;
			specular_reflect = specular_term * fresnel_term(specular_color, lh);
		}
		specular = specular_reflect * frame.light_color.rgb * frame.light_color.w;
		specular_blend = specular_reflect;
		let reflection_lighting = mix(
			vec3<f32>(1.0, 1.0, 1.0),
			frame.light_color.rgb * frame.light_color.w,
			clamp(drawu.reflection_ext_params.x, 0.0, 1.0),
		);
		let cube_tint = mix(vec3<f32>(1.0, 1.0, 1.0), drawu.reflection_cube_color.rgb, clamp(drawu.reflection_cube_color.a, 0.0, 1.0));
		let reflection_lod = clamp(perceptual_roughness * 5.0, 0.0, 8.0);
		let env = textureSampleLevel(reflection_tex, reflection_samp, reflection_dir, reflection_lod).rgb * cube_tint * reflection_lighting;
		authored_reflection_env = env;
		let one_minus_reflectivity = 0.96 - metallic * 0.96;
		let grazing_term = clamp(smoothness + (1.0 - one_minus_reflectivity), 0.0, 1.0);
		let surface_reduction = 1.0 / (roughness * roughness + 1.0);
		authored_reflection = env * surface_reduction * fresnel_lerp(specular_color, grazing_term, max(dot(reflection_n, v), 0.0));
		authored_reflection_blend = authored_reflection * select(1.0, a, is_liltoon_refraction);
	} else if (is_liltoon_gem) {
		let smoothness_uv = uv * drawu.smoothness_uv_offset_scale.zw + drawu.smoothness_uv_offset_scale.xy;
		let smoothness = clamp(drawu.reflection_params.x * textureSample(smoothness_tex, smoothness_samp, smoothness_uv).r, 0.0, 1.0);
		let perceptual_roughness = clamp(1.0 - smoothness, 0.0, 1.0);
		let roughness = perceptual_roughness * perceptual_roughness;
		let cube_tint = drawu.reflection_cube_color.rgb;
		let gem_reflection_lighting = mix(
			vec3<f32>(1.0, 1.0, 1.0),
			frame.light_color.rgb * frame.light_color.w,
			clamp(drawu.reflection_ext_params.x, 0.0, 1.0),
		);
		let gem_reflection_dir = normalize(reflect(-v, n));
		let gem_env_lod = clamp(perceptual_roughness * 5.0, 0.0, 8.0);
		let nv_particle = clamp(abs(dot(n, gem_view)), 0.0, 1.0);
		let nv_view = clamp(abs(dot(n, v)), 0.0, 1.0);
		let inv_nv = 1.0 - nv_particle;
		let chroma = clamp(drawu.gem_params.y, 0.0, 1.0);
		let gem_n_g = normalize(n + v * inv_nv * chroma);
		let gem_n_b = normalize(n + v * inv_nv * chroma * 2.0);
		let env_base = textureSampleLevel(reflection_tex, reflection_samp, gem_reflection_dir, gem_env_lod).rgb;
		let env_r = env_base.r;
		let env_g = select(env_base.g, textureSampleLevel(reflection_tex, reflection_samp, normalize(reflect(-v, gem_n_g)), gem_env_lod).g, !front_facing);
		let env_b = select(env_base.b, textureSampleLevel(reflection_tex, reflection_samp, normalize(reflect(-v, gem_n_b)), gem_env_lod).b, !front_facing);
		var env = vec3<f32>(env_r, env_g, env_b) * cube_tint * gem_reflection_lighting;
		let contrast = max(drawu.reflection_ext_params.y, 0.0001);
		env = pow(clamp(env, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(contrast)) * contrast * drawu.gem_env_color.rgb;
		let env_luma = dot(env, vec3<f32>(1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0));
		env = mix(vec3<f32>(env_luma), env, clamp(1.0 / contrast, 0.0, 1.0));
		env = select(env * base * nv_view, env, front_facing);
		let one_minus_reflectivity = 0.96;
		let grazing_term = clamp(smoothness + (1.0 - one_minus_reflectivity), 0.0, 1.0);
		let surface_reduction = 1.0 / (roughness * roughness + 1.0);
		let reflectance = vec3<f32>(clamp(drawu.reflection_params.z, 0.0, 1.0));
		let particle_loop = drawu.gem_params.z;
		let particle_1 = step(0.5, fract(nv_particle * particle_loop));
		let particle_2 = step(0.5, fract(abs(dot(n, normalize(gem_view.yzx))) * particle_loop));
		let particle_3 = step(0.5, fract(abs(dot(n, normalize(gem_view.zxy))) * particle_loop));
		let particle = select(particle_1 * particle_2 * particle_3, 0.0, particle_loop <= 0.0);
		let particle_color = select(vec3<f32>(1.0) + particle * drawu.gem_particle_color.rgb, vec3<f32>(1.0), front_facing);
		authored_reflection = (surface_reduction * fresnel_lerp(reflectance, grazing_term, nv_view) + vec3<f32>(0.5)) * 0.5 * particle_color * env;
	} else if (!is_liltoon) {
		let specular_intensity = clamp(drawu.uv_anim_params.w, 0.0, 2.0);
		let specular_shape = pow(max(dot(n, half_vec), 0.0), clamp(drawu.emissive_factor.w, 1.0, 128.0));
		specular = vec3<f32>(specular_shape * specular_intensity);
		specular_blend = specular;
		authored_reflection = textureSample(reflection_tex, reflection_samp, reflection_dir).rgb * (0.18 + 0.32 * reflection_fresnel);
		authored_reflection_blend = authored_reflection;
	}
	var rim = vec3<f32>(0.0, 0.0, 0.0);
	var rim_blend = 0.0;
	if (!disable_rim) {
		let rim_uv = uv * drawu.rim_uv_offset_scale.zw + drawu.rim_uv_offset_scale.xy;
		if (is_liltoon && drawu.rim_control.x > 0.0) {
			let rim_tex_color = textureSample(rim_tex, rim_samp, rim_uv);
			var rim_color = drawu.rim_color.rgb * rim_tex_color.rgb;
			rim_color = mix(rim_color, rim_color * base, clamp(drawu.rim_control.y, 0.0, 1.0));
			let rim_n = normalize(mix(geometry_n, n, clamp(drawu.rim_ext_params.y, 0.0, 1.0)));
			let rim_raw = pow(clamp(1.0 - abs(dot(rim_n, v)), 0.0, 1.0), max(drawu.rim_params.z, 0.00001));
			let rim_factor = lil_tooning_scale(rim_raw, clamp(drawu.rim_params.x, 0.0, 1.0), clamp(drawu.rim_params.y, 0.0, 1.0));
			let lit_rim_color = mix(rim_color, rim_color * frame.light_color.rgb * frame.light_color.w, clamp(drawu.rim_control.z, 0.0, 1.0));
			let rim_alpha = clamp(drawu.rim_control.x * rim_tex_color.a, 0.0, 1.0);
			let rim_shadow = mix(1.0, shading, clamp(drawu.rim_ext_params.x, 0.0, 1.0));
			let rim_backface = lil_backface_visibility(drawu.rim_ext_params.z, front_facing);
			let rim_transparency = mix(1.0, a, select(clamp(drawu.transparency_params.z, 0.0, 1.0), 0.0, is_liltoon_refraction));
			let rim_direct_blend = clamp(rim_factor * rim_alpha * rim_shadow * rim_backface * rim_transparency, 0.0, 1.0);
			rim = lit_rim_color * rim_direct_blend;
			rim_blend = max(rim_blend, rim_direct_blend);
			let rim_dir = pow(clamp(dot(rim_n, l) * 0.5 + 0.5, 0.0, 1.0), mix(1.0, 8.0, clamp(drawu.rim_indirect_params.y, 0.0, 1.0)));
			let rim_dir_factor = clamp(drawu.rim_indirect_params.x * rim_dir, 0.0, 1.0);
			rim = mix(rim, rim * rim_dir, rim_dir_factor);
			let ln_raw = clamp(dot(rim_n, l) * 0.5 + 0.5, 0.0, 1.0);
			let indir_range = clamp(drawu.rim_indirect_params.z, 0.0, 1.0);
			let ln_indir = clamp((1.0 - ln_raw + indir_range) / (1.0 + indir_range), 0.0, 1.0);
			let indir_raw = pow(clamp(1.0 - abs(dot(rim_n, v)), 0.0, 1.0), max(drawu.rim_params.z, 0.00001)) * ln_indir * clamp(drawu.rim_indirect_params.x, 0.0, 1.0);
			let indir_factor = lil_tooning_scale(
				indir_raw,
				clamp(drawu.rim_indirect_params.w, 0.0, 1.0),
				clamp(drawu.rim_indirect_ext_params.x, 0.0, 1.0)
			) * clamp(drawu.rim_indirect_color.a, 0.0, 1.0);
			let rim_indirect_blend = clamp(indir_factor * rim_alpha * rim_shadow * rim_backface * rim_transparency, 0.0, 1.0);
			rim = rim + drawu.rim_indirect_color.rgb * rim_indirect_blend;
			rim_blend = max(rim_blend, rim_indirect_blend);
		} else if (!is_liltoon) {
			let rim_base = pow(clamp(1.0 - dot(n, v) + drawu.rim_params.z, 0.0, 1.0), max(drawu.rim_params.y, 0.00001));
			rim = rim_base * drawu.rim_color.rgb;
			rim = rim * mix(vec3<f32>(1.0, 1.0, 1.0), textureSample(rim_tex, rim_samp, rim_uv).rgb, clamp(drawu.rim_params.w, 0.0, 1.0));
			let lighting_scalar = clamp(0.35 + 0.65 * max(dot(n, l), 0.0), 0.0, 1.0);
			rim = rim * mix(vec3<f32>(1.0, 1.0, 1.0), vec3<f32>(lighting_scalar, lighting_scalar, lighting_scalar), clamp(drawu.rim_params.x, 0.0, 1.0));
		}
	}
	if (is_liltoon_refraction) {
		let refraction_strength = drawu.gem_params.x;
		if (abs(refraction_strength) > 0.00001) {
			let refraction_fresnel = pow(clamp(1.0 - max(dot(n, v), 0.0), 0.0, 1.0), max(drawu.reflection_ext_params.z, 0.0001));
			let base_screen_uv = screen_uv(i.clip);
			let screen_offset = liltoon_refraction_offset(n) * refraction_strength * refraction_fresnel;
			var refract_color = textureSample(screen_tex, screen_samp, clamp(base_screen_uv + screen_offset, vec2<f32>(0.0), vec2<f32>(1.0))).rgb;
			refract_color = refract_color * drawu.gem_particle_color.rgb;
			if (drawu.gem_params.y > 0.5) {
				refract_color = refract_color * base;
			}
			lit = mix(refract_color, lit, clamp(out_a, 0.0, 1.0));
			out_a = 1.0;
		}
	}
	if (is_liltoon) {
		let reflection_color_uv = uv * drawu.reflection_color_uv_offset_scale.zw + drawu.reflection_color_uv_offset_scale.xy;
		let reflection_color_texel = textureSample(reflection_color_tex, reflection_color_samp, reflection_color_uv);
		let reflection_transparency = mix(1.0, a, select(clamp(drawu.transparency_params.w, 0.0, 1.0), 0.0, is_liltoon_refraction));
		let reflection_color_alpha = clamp(drawu.reflection_color.a * reflection_color_texel.a * reflection_transparency, 0.0, 1.0);
		lit = lit - reflection_metallic * lit;
		let reflection_tint = drawu.reflection_color.rgb * reflection_color_texel.rgb;
		let reflection_light_tint = reflection_tint * frame.light_color.rgb * frame.light_color.w;
		lit = lil_blend_color3(lit, reflection_light_tint, specular_blend * reflection_color_alpha * drawu.reflection_control.y, drawu.reflection_control.w);
		if (is_liltoon_gem) {
			let refraction_strength = drawu.gem_params.x;
			if (abs(refraction_strength) > 0.00001) {
				let refraction_fresnel = pow(clamp(1.0 - abs(dot(n, v)), 0.0, 1.0), max(drawu.reflection_ext_params.z, 0.0001));
				let base_screen_uv = screen_uv(i.clip);
				let screen_offset = liltoon_refraction_offset(n) * refraction_fresnel;
				let chroma = clamp(drawu.gem_params.y, 0.0, 1.0);
				let refract_r = textureSample(screen_tex, screen_samp, clamp(base_screen_uv + screen_offset * refraction_strength, vec2<f32>(0.0), vec2<f32>(1.0))).r;
				let refract_g = textureSample(screen_tex, screen_samp, clamp(base_screen_uv + screen_offset * (refraction_strength + chroma), vec2<f32>(0.0), vec2<f32>(1.0))).g;
				let refract_b = textureSample(screen_tex, screen_samp, clamp(base_screen_uv + screen_offset * (refraction_strength + chroma * 2.0), vec2<f32>(0.0), vec2<f32>(1.0))).b;
				let contrast = max(drawu.reflection_ext_params.y, 0.0001);
				var refract_color = pow(clamp(vec3<f32>(refract_r, refract_g, refract_b), vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(contrast)) * contrast;
				let refract_luma = dot(refract_color, vec3<f32>(1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0));
				refract_color = mix(vec3<f32>(refract_luma), refract_color, clamp(1.0 / contrast, 0.0, 1.0));
				lit = lit * refract_color;
			}
			lit = lit + authored_reflection;
		} else {
			if (is_liltoon_refraction && drawu.reflection_control.x > 0.5 && drawu.reflection_control.z > 0.0) {
				let refraction_reflection_mix = clamp(a + (1.0 - a) * pow(clamp(abs(dot(reflection_n, v)), 0.0, 1.0), abs(drawu.gem_params.x) * 0.5 + 0.25), 0.0, 1.0);
				lit = mix(authored_reflection_env, lit, refraction_reflection_mix);
			}
			lit = lil_blend_color3(lit, reflection_tint, authored_reflection_blend * reflection_color_alpha * drawu.reflection_control.z, drawu.reflection_control.w);
		}
		if (!disable_matcap) {
			if (drawu.matcap_params.x > 0.0) {
				let matcap_base_n = normalize(mix(geometry_n, n, clamp(drawu.matcap_ext_params.x, 0.0, 1.0)));
				let matcap_n = normalize(mix(matcap_base_n, anisotropy_n, clamp(drawu.anisotropy_params.w * anisotropy_basis.enabled, 0.0, 1.0)));
				let matcap_uv = toon_matcap_uv(matcap_n, v, drawu.matcap_uv_params.x);
				let matcap_tex_color = textureSampleLevel(matcap_tex, matcap_samp, matcap_uv, max(drawu.matcap_ext_params.z, 0.0));
				let matcap_raw = drawu.matcap_factor.rgb * matcap_tex_color.rgb;
				let lit_matcap = mix(matcap_raw, matcap_raw * frame.light_color.rgb * frame.light_color.w, clamp(drawu.matcap_params.z, 0.0, 1.0));
				let albedo_matcap = mix(lit_matcap, lit_matcap * base, clamp(drawu.matcap_params.y, 0.0, 1.0));
				let matcap_blend_mask_uv = uv * drawu.matcap_blend_mask_uv_offset_scale.zw + drawu.matcap_blend_mask_uv_offset_scale.xy;
				let matcap_blend_mask = textureSample(matcap_blend_mask_tex, matcap_blend_mask_samp, matcap_blend_mask_uv).r;
				let matcap_shadow = mix(1.0, shading, clamp(drawu.matcap_ext_params.y, 0.0, 1.0));
				let matcap_backface = lil_backface_visibility(drawu.matcap_ext_params.w, front_facing);
				let matcap_transparency = mix(1.0, a, select(clamp(drawu.transparency_params.x, 0.0, 1.0), 0.0, is_liltoon_refraction));
				let matcap_blend = clamp(drawu.matcap_params.x * matcap_tex_color.a * matcap_blend_mask * drawu.matcap_factor.w * matcap_shadow * matcap_backface * matcap_transparency, 0.0, 1.0);
				lit = lil_blend_color(lit, albedo_matcap, matcap_blend, drawu.matcap_params.w);
			}
			if (drawu.matcap2_params.x > 0.0) {
				let matcap2_base_n = normalize(mix(geometry_n, n, clamp(drawu.matcap2_ext_params.x, 0.0, 1.0)));
				let matcap2_n = normalize(mix(matcap2_base_n, anisotropy_n, clamp(drawu.anisotropy_ext_params.x * anisotropy_basis.enabled, 0.0, 1.0)));
				let matcap2_uv = toon_matcap_uv(matcap2_n, v, drawu.matcap_uv_params.z);
				let matcap2_tex_color = textureSampleLevel(matcap2_tex, matcap_samp, matcap2_uv, max(drawu.matcap2_ext_params.z, 0.0));
				let matcap2_lighting = mix(vec3<f32>(1.0, 1.0, 1.0), frame.light_color.rgb * frame.light_color.w, clamp(drawu.matcap2_params.z, 0.0, 1.0));
				let matcap2_raw = drawu.matcap2_factor.rgb * matcap2_tex_color.rgb * matcap2_lighting;
				let matcap2_albedo = mix(matcap2_raw, matcap2_raw * base, clamp(drawu.matcap2_params.y, 0.0, 1.0));
				let matcap2_blend_mask_uv = uv * drawu.matcap2_blend_mask_uv_offset_scale.zw + drawu.matcap2_blend_mask_uv_offset_scale.xy;
				let matcap2_blend_mask = textureSample(matcap2_blend_mask_tex, matcap_blend_mask_samp, matcap2_blend_mask_uv).r;
				let matcap2_shadow = mix(1.0, shading, clamp(drawu.matcap2_ext_params.y, 0.0, 1.0));
				let matcap2_backface = lil_backface_visibility(drawu.matcap2_ext_params.w, front_facing);
				let matcap2_transparency = mix(1.0, a, select(clamp(drawu.transparency_params.y, 0.0, 1.0), 0.0, is_liltoon_refraction));
				let matcap2_blend = clamp(drawu.matcap2_params.x * drawu.matcap2_factor.a * matcap2_tex_color.a * matcap2_blend_mask * matcap2_shadow * matcap2_backface * matcap2_transparency, 0.0, 1.0);
				lit = lil_blend_color(lit, matcap2_albedo, matcap2_blend, drawu.matcap2_params.w);
			}
		}
		if (!is_liltoon_gem && drawu.rim_shade_params.x > 0.5) {
			let rim_shade_n = normalize(mix(geometry_n, n, clamp(drawu.rim_ext_params.w, 0.0, 1.0)));
			let rim_shade_raw = pow(clamp(1.0 - abs(dot(rim_shade_n, v)), 0.0, 1.0), max(drawu.rim_shade_params.w, 0.00001));
			let rim_shade_mask = textureSample(rim_shade_mask_tex, rim_samp, uv).r;
			let rim_shade = lil_tooning_scale(
				rim_shade_raw,
				clamp(drawu.rim_shade_params.y, 0.0, 1.0),
				clamp(drawu.rim_shade_params.z, 0.0, 1.0)
			) * rim_shade_mask * clamp(drawu.rim_shade_color.a, 0.0, 1.0);
			lit = mix(lit, lit * drawu.rim_shade_color.rgb, rim_shade);
		}
		if (!is_liltoon_gem && drawu.backlight_params.x > 0.5) {
			let backlight_n = normalize(mix(geometry_n, n, clamp(drawu.backlight_params.z, 0.0, 1.0)));
			let backlight_factor = pow(clamp(-dot(normalize(l + v), l) * 0.5 + 0.5, 0.0, 1.0), max(drawu.backlight_params.w, 0.00001));
			let backlight_ln_dir = normalize(-v * clamp(drawu.backlight_ext_params.z, 0.0, 1.0) + l);
			let backlight_receive_shadow = mix(1.0, shading, clamp(drawu.backlight_shadow_params.x, 0.0, 1.0));
			let backlight_ln_raw = (dot(backlight_ln_dir, backlight_n) * 0.5 + 0.5) * backlight_receive_shadow;
			let backlight_ln = lil_tooning_scale(
				backlight_ln_raw,
				clamp(drawu.backlight_ext_params.x, 0.0, 1.0),
				clamp(drawu.backlight_ext_params.y, 0.0, 1.0)
			);
			let backlight_backface = lil_backface_visibility(drawu.backlight_ext_params.w, front_facing);
			let backlight_color_sample = textureSample(backlight_color_tex, base_samp, uv);
			let authored_backlight_color = drawu.backlight_color * backlight_color_sample;
			let backlight = clamp(backlight_factor * backlight_ln, 0.0, 1.0) * backlight_backface * clamp(authored_backlight_color.a, 0.0, 1.0);
			let backlight_color = mix(authored_backlight_color.rgb, authored_backlight_color.rgb * base, clamp(drawu.backlight_params.y, 0.0, 1.0));
			lit = lit + backlight * backlight_color * frame.light_color.rgb * frame.light_color.w;
		}
		lit = lil_blend_weighted_color(lit, rim, rim_blend, drawu.rim_control.w);
	} else {
		if (!disable_matcap) {
			let matcap_base_n = normalize(mix(geometry_n, n, clamp(drawu.matcap_ext_params.x, 0.0, 1.0)));
			let matcap_n = normalize(mix(matcap_base_n, anisotropy_n, clamp(drawu.anisotropy_params.w * anisotropy_basis.enabled, 0.0, 1.0)));
			let matcap_uv = toon_matcap_uv(matcap_n, v, drawu.matcap_uv_params.x);
			let matcap_tex_color = textureSampleLevel(matcap_tex, matcap_samp, matcap_uv, max(drawu.matcap_ext_params.z, 0.0));
			let matcap_raw = drawu.matcap_factor.rgb * matcap_tex_color.rgb;
			if (drawu.matcap_params.x > 0.0) {
				let lit_matcap = mix(matcap_raw, matcap_raw * frame.light_color.rgb * frame.light_color.w, clamp(drawu.matcap_params.z, 0.0, 1.0));
				let albedo_matcap = mix(lit_matcap, lit_matcap * base, clamp(drawu.matcap_params.y, 0.0, 1.0));
				let matcap_blend_mask_uv = uv * drawu.matcap_blend_mask_uv_offset_scale.zw + drawu.matcap_blend_mask_uv_offset_scale.xy;
				let matcap_blend_mask = textureSample(matcap_blend_mask_tex, matcap_blend_mask_samp, matcap_blend_mask_uv).r;
				let matcap_shadow = mix(1.0, shading, clamp(drawu.matcap_ext_params.y, 0.0, 1.0));
				let matcap_backface = lil_backface_visibility(drawu.matcap_ext_params.w, front_facing);
				let matcap_transparency = mix(1.0, a, clamp(drawu.transparency_params.x, 0.0, 1.0));
				let matcap_blend = clamp(drawu.matcap_params.x * matcap_tex_color.a * matcap_blend_mask * drawu.matcap_factor.w * matcap_shadow * matcap_backface * matcap_transparency, 0.0, 1.0);
				lit = lil_blend_color(lit, albedo_matcap, matcap_blend, drawu.matcap_params.w);
			} else {
				lit = lit + matcap_raw * drawu.matcap_factor.w;
			}
			if (drawu.matcap2_params.x > 0.0) {
				let matcap2_base_n = normalize(mix(geometry_n, n, clamp(drawu.matcap2_ext_params.x, 0.0, 1.0)));
				let matcap2_n = normalize(mix(matcap2_base_n, anisotropy_n, clamp(drawu.anisotropy_ext_params.x * anisotropy_basis.enabled, 0.0, 1.0)));
				let matcap2_uv = toon_matcap_uv(matcap2_n, v, drawu.matcap_uv_params.z);
				let matcap2_tex_color = textureSampleLevel(matcap2_tex, matcap_samp, matcap2_uv, max(drawu.matcap2_ext_params.z, 0.0));
				let matcap2_lighting = mix(vec3<f32>(1.0, 1.0, 1.0), frame.light_color.rgb * frame.light_color.w, clamp(drawu.matcap2_params.z, 0.0, 1.0));
				let matcap2_raw = drawu.matcap2_factor.rgb * matcap2_tex_color.rgb * matcap2_lighting;
				let matcap2_albedo = mix(matcap2_raw, matcap2_raw * base, clamp(drawu.matcap2_params.y, 0.0, 1.0));
				let matcap2_blend_mask_uv = uv * drawu.matcap2_blend_mask_uv_offset_scale.zw + drawu.matcap2_blend_mask_uv_offset_scale.xy;
				let matcap2_blend_mask = textureSample(matcap2_blend_mask_tex, matcap_blend_mask_samp, matcap2_blend_mask_uv).r;
				let matcap2_shadow = mix(1.0, shading, clamp(drawu.matcap2_ext_params.y, 0.0, 1.0));
				let matcap2_backface = lil_backface_visibility(drawu.matcap2_ext_params.w, front_facing);
				let matcap2_transparency = mix(1.0, a, clamp(drawu.transparency_params.y, 0.0, 1.0));
				let matcap2_blend = clamp(drawu.matcap2_params.x * drawu.matcap2_factor.a * matcap2_tex_color.a * matcap2_blend_mask * matcap2_shadow * matcap2_backface * matcap2_transparency, 0.0, 1.0);
				lit = lil_blend_color(lit, matcap2_albedo, matcap2_blend, drawu.matcap2_params.w);
			}
		}
		lit = lit + specular + authored_reflection;
		lit = lit + rim;
	}
	if (fur_shell > 0.0 && !disable_rim) {
		let fur_rim_raw = pow(clamp(1.0 - abs(dot(normalize(n), v)), 0.0, 1.0), max(drawu.fur_rim_params.x, 0.00001));
		let inv_lighting = clamp(vec3<f32>(1.0) / max(lil_direct_light_color() + frame.ambient_color.rgb * frame.ambient_color.w, vec3<f32>(0.25)), vec3<f32>(1.0), vec3<f32>(4.0));
		let fur_rim_anti_light = mix(1.0, dot(inv_lighting, vec3<f32>(1.0 / 3.0)), clamp(drawu.fur_rim_params.y, 0.0, 1.0));
		lit = lit + clamp(fur_shell, 0.0, 1.0) * fur_rim_raw * fur_rim_anti_light * drawu.fur_rim_color.rgb * lil_direct_light_color();
	}

	let disable_emissive = (dbg & DBG_DISABLE_EMISSIVE) != 0u;
	let emission_uv = lil_calc_uv_scroll_rotate(uv, drawu.emission_uv_offset_scale, drawu.emission_uv_anim_params) + parallax_offset * drawu.emission_grad_params.w;
	let emission_tex_color = textureSample(emissive_tex, emissive_samp, emission_uv);
	if (!disable_emissive) {
		if (is_liltoon) {
			var emission_color = drawu.emission_color.rgb * emission_tex_color.rgb;
			let emission_mask_uv = lil_calc_uv_scroll_rotate(uv, drawu.emission_blend_mask_uv_offset_scale, drawu.emission_blend_mask_uv_anim_params);
			let emission_mask = textureSample(emission_blend_mask_tex, emissive_samp, emission_mask_uv).r;
			if (drawu.emission_grad_params.x > 0.5) {
				let grad_u = fract(drawu.emission_grad_params.y * frame.time_params.x);
				emission_color = emission_color * textureSample(emission_gradation_tex, emissive_samp, vec2<f32>(grad_u, 0.5)).rgb;
			}
			let inv_lighting = clamp(vec3<f32>(1.0) / max(lil_direct_light_color() + frame.ambient_color.rgb * frame.ambient_color.w, vec3<f32>(0.25)), vec3<f32>(1.0), vec3<f32>(4.0));
			emission_color = mix(emission_color, emission_color * inv_lighting, clamp(drawu.emission_grad_params.z, 0.0, 1.0));
			emission_color = mix(emission_color, emission_color * base, clamp(drawu.emission_params.y, 0.0, 1.0));
			let emission_blink = lil_calc_blink(drawu.emission_blink_params);
			let emission_blend = clamp(drawu.emission_params.x * drawu.emission_params.z * emission_blink * emission_mask * drawu.emission_color.a * emission_tex_color.a, 0.0, 1.0);
			lit = lil_blend_color(lit, emission_color, emission_blend, drawu.emission_params.w);
			if (drawu.emission2nd_params.x > 0.5) {
				let emission2nd_uv = lil_calc_uv_scroll_rotate(uv, drawu.emission2nd_uv_offset_scale, drawu.emission2nd_uv_anim_params) + parallax_offset * drawu.emission2nd_ext_params.x;
				let emission2nd_mask_uv = lil_calc_uv_scroll_rotate(uv, drawu.emission2nd_blend_mask_uv_offset_scale, drawu.emission2nd_blend_mask_uv_anim_params);
				let emission2nd_sample = textureSample(emission2nd_tex, emissive_samp, emission2nd_uv) * drawu.emission2nd_color * textureSample(emission2nd_blend_mask_tex, emissive_samp, emission2nd_mask_uv);
				var emission2nd_rgb_work = emission2nd_sample.rgb;
				if (drawu.emission2nd_grad_params.x > 0.5) {
					let grad_u = fract(drawu.emission2nd_grad_params.y * frame.time_params.x);
					emission2nd_rgb_work = emission2nd_rgb_work * textureSample(emission2nd_gradation_tex, emissive_samp, vec2<f32>(grad_u, 0.5)).rgb;
				}
				emission2nd_rgb_work = mix(emission2nd_rgb_work, emission2nd_rgb_work * inv_lighting, clamp(drawu.emission2nd_grad_params.z, 0.0, 1.0));
				let emission2nd_rgb = mix(emission2nd_rgb_work, emission2nd_rgb_work * base, clamp(drawu.emission2nd_params.y, 0.0, 1.0));
				let emission2nd_blink = lil_calc_blink(drawu.emission2nd_blink_params);
				let emission2nd_blend = clamp(drawu.emission2nd_params.x * drawu.emission2nd_params.z * emission2nd_blink * emission2nd_sample.a, 0.0, 1.0);
				lit = lil_blend_color(lit, emission2nd_rgb, emission2nd_blend, drawu.emission2nd_params.w);
			}
		} else {
			let emission_raw = drawu.emissive_factor.rgb * emission_tex_color.rgb;
			lit = lit + emission_raw;
		}
	}
	return vec4<f32>(premultiply_when_blending(max(lit, vec3<f32>(0.0, 0.0, 0.0)), out_a, alpha_kind, !compute_fur && !fur_cutout_pre && !is_liltoon_additive_blend), out_a);
}

@fragment
fn fs_toon(i: VsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
	return toon_fragment(i, front_facing, false, 0.0, 1.0, 0.0, false, i.uv);
}

@fragment
fn fs_toon_gem_pre(i: VsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
	let dbg = bitcast<u32>(drawu.params.w);
	discard_by_cull_mode(front_facing, dbg);
	return vec4<f32>(0.0, 0.0, 0.0, 0.0);
}

@fragment
fn fs_fur_toon(i: FurVsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
	var base: VsOut;
	base.clip = i.clip;
	base.wn = i.wn;
	base.uv = i.uv;
	base.wp = i.wp;
	base.wt = i.wt;
	return toon_fragment(base, front_facing, false, i.fur_shell, i.fur_alpha, i.fur_card_side, false, i.fur_uv0);
}

@fragment
fn fs_fur_toon_pre(i: FurVsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
	var base: VsOut;
	base.clip = i.clip;
	base.wn = i.wn;
	base.uv = i.uv;
	base.wp = i.wp;
	base.wt = i.wt;
	return toon_fragment(base, front_facing, false, i.fur_shell, i.fur_alpha, i.fur_card_side, true, i.fur_uv0);
}

@fragment
fn fs_outline(i: VsOut) -> @location(0) vec4<f32> {
	let uv = animated_uv(i.uv);
	let samp_tex = textureSample(tex, base_samp, uv);
	let a = apply_lil_alpha_mask(samp_tex.a * drawu.base_color.a, uv);
	mask_discard_toon(samp_tex.rgb, a, drawu.params.y, drawu.params.z);
	let mask = textureSample(outline_width_tex, outline_width_samp, uv).r;
	if (mask <= 0.001) {
		discard;
	}
	let outline_sample = textureSample(outline_tex, base_samp, uv);
	let n = normalize(i.wn);
	let l = normalize(frame.light_dir.xyz);
	let lighting_scalar = clamp(0.35 + 0.65 * max(dot(n, l), 0.0), 0.0, 1.0);
	let outline_base = drawu.outline_color.rgb * outline_sample.rgb;
	let lit_base = outline_base * mix(vec3<f32>(1.0, 1.0, 1.0), vec3<f32>(lighting_scalar, lighting_scalar, lighting_scalar), clamp(drawu.outline_params.z, 0.0, 1.0));
	let outline_ndotl = dot(n, l) * 0.5 + 0.5;
	let lit_factor = clamp(outline_ndotl * drawu.outline_lit_params.x + drawu.outline_lit_params.y, 0.0, 1.0) * clamp(drawu.outline_lit_color.a, 0.0, 1.0);
	let lit_color = mix(drawu.outline_lit_color.rgb, samp_tex.rgb * drawu.outline_lit_color.rgb, clamp(drawu.outline_lit_params.z, 0.0, 1.0));
	let color = mix(lit_base, lit_color, lit_factor);
	return vec4<f32>(color, clamp(drawu.outline_color.a * outline_sample.a, 0.0, 1.0));
}

@fragment
fn fs_toon_backpass(i: VsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
	return toon_fragment(i, front_facing, true, 0.0, 1.0, 0.0, false, i.uv);
}
