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
	shadow_ao_params: vec4<f32>,
	shadow_ao_shift: vec4<f32>,
	shadow_ao_shift2: vec4<f32>,
	shadow_border_color: vec4<f32>,
	shadow2_color: vec4<f32>,
	shadow2_params: vec4<f32>,
	shadow3_color: vec4<f32>,
	shadow3_params: vec4<f32>,
	matcap_factor: vec4<f32>,
	matcap_params: vec4<f32>,
	matcap_ext_params: vec4<f32>,
	matcap_bump_params: vec4<f32>,
	matcap2_factor: vec4<f32>,
	matcap2_params: vec4<f32>,
	matcap2_ext_params: vec4<f32>,
	matcap2_bump_params: vec4<f32>,
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
	backlight_color_uv_offset_scale: vec4<f32>,
	glitter_color: vec4<f32>,
	glitter_params1: vec4<f32>,
	glitter_params2: vec4<f32>,
	glitter_control: vec4<f32>,
	glitter_ext: vec4<f32>,
	glitter_ext2: vec4<f32>,
	glitter_ext3: vec4<f32>,
	glitter_color_uv_offset_scale: vec4<f32>,
	glitter_shape_uv_offset_scale: vec4<f32>,
	glitter_atlas: vec4<f32>,
	distance_fade: vec4<f32>,
	distance_fade_color: vec4<f32>,
	distance_fade_rim_color: vec4<f32>,
	distance_fade_params: vec4<f32>,
	dissolve_color: vec4<f32>,
	dissolve_params: vec4<f32>,
	dissolve_pos: vec4<f32>,
	dissolve_ext: vec4<f32>,
	dissolve_mask_uv_offset_scale: vec4<f32>,
	dissolve_noise_uv_offset_scale: vec4<f32>,
	dissolve_noise_uv_anim_params: vec4<f32>,
	parallax_params: vec4<f32>,
	parallax_uv_offset_scale: vec4<f32>,
	id_mask_params: vec4<f32>,
	id_mask_flags0: vec4<f32>,
	id_mask_flags1: vec4<f32>,
	id_mask_prior_flags0: vec4<f32>,
	id_mask_prior_flags1: vec4<f32>,
	id_mask_indices0: vec4<f32>,
	id_mask_indices1: vec4<f32>,
	udim_discard_params: vec4<f32>,
	udim_discard_row0: vec4<f32>,
	udim_discard_row1: vec4<f32>,
	udim_discard_row2: vec4<f32>,
	udim_discard_row3: vec4<f32>,
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
	material_ext_params: vec4<f32>,
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
	matcap_bump_uv_offset_scale: vec4<f32>,
	matcap2_blend_mask_uv_offset_scale: vec4<f32>,
	matcap2_bump_uv_offset_scale: vec4<f32>,
	alpha_mask_uv_offset_scale: vec4<f32>,
	main_color_adjust_params: vec4<f32>,
	main_gradation_params: vec4<f32>,
	main2nd_color: vec4<f32>,
	main2nd_params: vec4<f32>,
	main2nd_ext: vec4<f32>,
	main2nd_distance_fade: vec4<f32>,
	main2nd_decal_flags: vec4<f32>,
	main2nd_decal_transform: vec4<f32>,
	main2nd_decal_animation: vec4<f32>,
	main2nd_decal_sub_param: vec4<f32>,
	main2nd_uv_offset_scale: vec4<f32>,
	main2nd_blend_mask_uv_offset_scale: vec4<f32>,
	main2nd_dissolve_color: vec4<f32>,
	main2nd_dissolve_params: vec4<f32>,
	main2nd_dissolve_pos: vec4<f32>,
	main2nd_dissolve_ext: vec4<f32>,
	main2nd_dissolve_mask_uv_offset_scale: vec4<f32>,
	main2nd_dissolve_noise_uv_offset_scale: vec4<f32>,
	main2nd_dissolve_noise_uv_anim_params: vec4<f32>,
	main3rd_color: vec4<f32>,
	main3rd_params: vec4<f32>,
	main3rd_ext: vec4<f32>,
	main3rd_distance_fade: vec4<f32>,
	main3rd_decal_flags: vec4<f32>,
	main3rd_decal_transform: vec4<f32>,
	main3rd_decal_animation: vec4<f32>,
	main3rd_decal_sub_param: vec4<f32>,
	main3rd_uv_offset_scale: vec4<f32>,
	main3rd_blend_mask_uv_offset_scale: vec4<f32>,
	main3rd_dissolve_color: vec4<f32>,
	main3rd_dissolve_params: vec4<f32>,
	main3rd_dissolve_pos: vec4<f32>,
	main3rd_dissolve_ext: vec4<f32>,
	main3rd_dissolve_mask_uv_offset_scale: vec4<f32>,
	main3rd_dissolve_noise_uv_offset_scale: vec4<f32>,
	main3rd_dissolve_noise_uv_anim_params: vec4<f32>,
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
@group(1) @binding(63) var glitter_color_tex: texture_2d<f32>;
@group(1) @binding(64) var glitter_shape_tex: texture_2d<f32>;
@group(1) @binding(65) var dissolve_mask_tex: texture_2d<f32>;
@group(1) @binding(66) var dissolve_noise_mask_tex: texture_2d<f32>;
@group(1) @binding(67) var parallax_tex: texture_2d<f32>;
@group(1) @binding(68) var main2nd_dissolve_mask_tex: texture_2d<f32>;
@group(1) @binding(69) var main2nd_dissolve_noise_mask_tex: texture_2d<f32>;
@group(1) @binding(70) var main3rd_dissolve_mask_tex: texture_2d<f32>;
@group(1) @binding(71) var main3rd_dissolve_noise_mask_tex: texture_2d<f32>;
@group(1) @binding(72) var normal2nd_scale_mask_tex: texture_2d<f32>;
@group(1) @binding(73) var matcap_bump_tex: texture_2d<f32>;
@group(1) @binding(74) var matcap2_bump_tex: texture_2d<f32>;
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
	@location(7) uv1: vec2<f32>,
	@location(8) uv2: vec2<f32>,
	@location(9) uv3: vec2<f32>,
}

struct VsOut {
	@builtin(position) clip: vec4<f32>,
	@location(0) wn: vec3<f32>,
	@location(1) uv: vec2<f32>,
	@location(2) wp: vec3<f32>,
	@location(3) wt: vec4<f32>,
	@location(4) uv1: vec2<f32>,
	@location(5) uv2: vec2<f32>,
	@location(6) uv3: vec2<f32>,
	@location(7) @interpolate(flat) id_mask: vec4<f32>,
}

struct FurVsOut {
	@builtin(position) clip: vec4<f32>,
	@location(0) wn: vec3<f32>,
	@location(1) uv: vec2<f32>,
	@location(2) wp: vec3<f32>,
	@location(3) wt: vec4<f32>,
	@location(4) uv1: vec2<f32>,
	@location(5) uv2: vec2<f32>,
	@location(6) uv3: vec2<f32>,
	@location(7) @interpolate(flat) id_mask: vec4<f32>,
	@location(8) fur_layer: f32,
	@location(9) fur_alpha: f32,
	@location(10) fur_card_side: f32,
	@location(11) fur_uv0: vec2<f32>,
}

struct ComputeFurCardsVsIn {
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
		o.uv1 = v.uv1;
		o.uv2 = v.uv2;
		o.uv3 = v.uv3;
		o.id_mask = lil_id_mask_vertex_state(v, vertex_index);
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
	o.uv1 = v.uv1;
	o.uv2 = v.uv2;
	o.uv3 = v.uv3;
	o.id_mask = lil_id_mask_vertex_state(v, vertex_index);
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
	out.uv1 = o.uv1;
	out.uv2 = o.uv2;
	out.uv3 = o.uv3;
	out.id_mask = o.id_mask;
	out.wp = o.wp;
	out.wt = o.wt;
	out.fur_layer = 0.0;
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

fn normalize_or2(v: vec2<f32>, fallback: vec2<f32>) -> vec2<f32> {
	let len2 = dot(v, v);
	if len2 <= 0.0000001 {
		return fallback;
	}
	return v * inverseSqrt(len2);
}

fn lil_rotate_uv(uv: vec2<f32>, angle: f32) -> vec2<f32> {
	let s = sin(angle);
	let c = cos(angle);
	let centered = uv - vec2<f32>(0.5, 0.5);
	return vec2<f32>(
		centered.x * c - centered.y * s,
		centered.x * s + centered.y * c,
	) + vec2<f32>(0.5, 0.5);
}

fn lil_id_mask_range(mask_input: u32, flags0: vec4<f32>, flags1: vec4<f32>) -> bool {
	let vertex = f32(mask_input);
	let indices0 = round(drawu.id_mask_indices0);
	let indices1 = round(drawu.id_mask_indices1);
	let a0 = vertex - indices0.x;
	let a1 = vertex - indices0.y;
	let a2 = vertex - indices0.z;
	let a3 = vertex - indices0.w;
	let a4 = vertex - indices1.x;
	let a5 = vertex - indices1.y;
	let a6 = vertex - indices1.z;
	let a7 = vertex - indices1.w;
	let b0 = vec4<f32>(clamp(a0 + 1.0, 0.0, 1.0), clamp(a1 + 1.0, 0.0, 1.0), clamp(a2 + 1.0, 0.0, 1.0), clamp(a3 + 1.0, 0.0, 1.0)) *
		vec4<f32>(clamp(-a1, 0.0, 1.0), clamp(-a2, 0.0, 1.0), clamp(-a3, 0.0, 1.0), clamp(-a4, 0.0, 1.0));
	let b1 = vec4<f32>(clamp(a4 + 1.0, 0.0, 1.0), clamp(a5 + 1.0, 0.0, 1.0), clamp(a6 + 1.0, 0.0, 1.0), clamp(a7 + 1.0, 0.0, 1.0)) *
		vec4<f32>(clamp(-a5, 0.0, 1.0), clamp(-a6, 0.0, 1.0), clamp(-a7, 0.0, 1.0), 1.0);
	return dot(b0, flags0) + dot(b1, flags1) > 0.5;
}

fn lil_id_mask_bit(mask_input: u32, flags0: vec4<f32>, flags1: vec4<f32>) -> bool {
	let enable0 = u32(round(flags0.x)) + u32(round(flags0.y)) * 2u + u32(round(flags0.z)) * 4u + u32(round(flags0.w)) * 8u;
	let enable1 = u32(round(flags1.x)) * 16u + u32(round(flags1.y)) * 32u + u32(round(flags1.z)) * 64u + u32(round(flags1.w)) * 128u;
	let enable_mask = enable0 + enable1;
	return mask_input != 0u && (enable_mask & mask_input) == mask_input;
}

fn lil_id_mask_matches(mask_input: u32, flags0: vec4<f32>, flags1: vec4<f32>) -> bool {
	if drawu.id_mask_params.z > 0.5 {
		return lil_id_mask_bit(mask_input, flags0, flags1);
	}
	return lil_id_mask_range(mask_input, flags0, flags1);
}

fn lil_id_mask_vertex_state(v: VsIn, vertex_index: u32) -> vec4<f32> {
	if drawu.id_mask_params.x <= 0.5 {
		return vec4<f32>(1.0, 1.0, 0.0, 0.0);
	}
	var mask_input = vertex_index;
	if drawu.id_mask_params.y < 0.5 {
		mask_input = u32(max(round(v.uv.x), 0.0));
	} else if drawu.id_mask_params.y < 1.5 {
		mask_input = u32(max(round(v.uv1.x), 0.0));
	} else if drawu.id_mask_params.y < 2.5 {
		mask_input = u32(max(round(v.uv2.x), 0.0));
	} else if drawu.id_mask_params.y < 3.5 {
		mask_input = u32(max(round(v.uv3.x), 0.0));
	}
	let masked = lil_id_mask_matches(mask_input, drawu.id_mask_flags0, drawu.id_mask_flags1);
	if drawu.id_mask_params.w > 0.5 {
		let prior_masked = lil_id_mask_matches(mask_input, drawu.id_mask_prior_flags0, drawu.id_mask_prior_flags1);
		return vec4<f32>(select(1.0, 0.0, masked && prior_masked), select(0.0, 1.0, masked != prior_masked), select(0.0, 1.0, prior_masked), 0.0);
	}
	return vec4<f32>(select(1.0, 0.0, masked), 1.0, 0.0, 0.0);
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

fn compute_fur_cards_vs(v: ComputeFurCardsVsIn, cutout_pre: bool) -> FurVsOut {
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
	o.uv1 = v.uv;
	o.uv2 = v.uv;
	o.uv3 = v.uv;
	o.id_mask = vec4<f32>(1.0, 1.0, 0.0, 0.0);
	o.wp = world_pos;
	o.wt = vec4<f32>(1.0, 0.0, 0.0, 1.0);
	o.fur_layer = clamp(v.position_layer.w, 0.0, 1.0);
	o.fur_alpha = 1.0 + clamp(v.fur_alpha, 0.0, 1.0);
	o.fur_card_side = select(0.0, select(-1.0, 1.0, side_width >= 0.0), abs(side_width) > 0.0000001);
	o.fur_uv0 = v.uv;
	return o;
}

@vertex
fn vs_compute_fur_cards(v: ComputeFurCardsVsIn) -> FurVsOut {
	return compute_fur_cards_vs(v, false);
}

@vertex
fn vs_compute_fur_cards_pre(v: ComputeFurCardsVsIn) -> FurVsOut {
	return compute_fur_cards_vs(v, true);
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

fn fur_layer_alpha(uv: vec2<f32>, fur_uv0: vec2<f32>, layer: f32, length_mask: f32, card_side: f32, fur_cutout_pre: bool) -> f32 {
	if (length_mask > 1.0) {
		let compute_fur_cards_alpha = clamp(length_mask - 1.0, 0.0, 1.0);
		let center_alpha = pow(1.0 - clamp(abs(card_side), 0.0, 1.0), 1.65);
		let fur_mask = textureSample(fur_mask_tex, base_samp, uv).r;
		let layer01 = clamp(layer, 0.0, 1.0);
		let noise_uv = fur_uv0 * max(drawu.fur_noise_params.xy, vec2<f32>(0.0001, 0.0001)) + drawu.fur_noise_params.zw;
		let fur_noise_mask = textureSample(fur_noise_mask_tex, base_samp, noise_uv).r;
		let root_offset = drawu.fur_ext_params.z;
		let fur_layer_shift = layer01 - layer01 * root_offset + root_offset;
		let fur_layer_abs = abs(fur_layer_shift);
		let layer_alpha = select(
			clamp(fur_noise_mask - fur_layer_shift * fur_layer_abs * fur_layer_abs, 0.0, 1.0),
			clamp(fur_noise_mask - fur_layer_shift * fur_layer_abs * fur_layer_abs * fur_layer_abs + 0.25, 0.0, 1.0),
			fur_cutout_pre,
		);
		return compute_fur_cards_alpha * center_alpha * layer_alpha * fur_mask;
	}
	if (layer <= 0.0) {
		return 1.0;
	}
	let fur_mask = textureSample(fur_mask_tex, base_samp, uv).r;
	let noise_uv = fur_uv0 * max(drawu.fur_noise_params.xy, vec2<f32>(0.0001, 0.0001)) + drawu.fur_noise_params.zw;
	let fur_noise_mask = textureSample(fur_noise_mask_tex, base_samp, noise_uv).r;
	let root_offset = drawu.fur_ext_params.z;
	let fur_layer_shift = layer - layer * root_offset + root_offset;
	let fur_layer_abs = abs(fur_layer_shift);
	let fur_alpha = select(
		clamp(fur_noise_mask - fur_layer_shift * fur_layer_abs * fur_layer_abs, 0.0, 1.0),
		clamp(fur_noise_mask - fur_layer_shift * fur_layer_abs * fur_layer_abs * fur_layer_abs + 0.25, 0.0, 1.0),
		fur_cutout_pre,
	);
	return fur_alpha * fur_mask * clamp(length_mask, 0.0, 1.0);
}

fn fur_layer_ao(layer: f32, fur_uv0: vec2<f32>, fur_cutout_pre: bool) -> f32 {
	if (layer <= 0.0) {
		return 1.0;
	}
	let fur_ao = clamp(drawu.fur_ext_params.y, 0.0, 1.0);
	if (fur_cutout_pre) {
		let cutout_ao = fur_ao * clamp(1.0 - fwidth(layer), 0.0, 1.0);
		return layer * cutout_ao * 2.0 + 1.0 - cutout_ao;
	}
	let noise_uv = fur_uv0 * max(drawu.fur_noise_params.xy, vec2<f32>(0.0001, 0.0001)) + drawu.fur_noise_params.zw;
	let fur_noise_mask = textureSample(fur_noise_mask_tex, base_samp, noise_uv).r;
	return clamp(1.0 - fur_noise_mask + fur_noise_mask * layer, 0.0, 1.0) * fur_ao * 1.25 + 1.0 - fur_ao;
}

fn premultiply_when_blending(rgb: vec3<f32>, out_a: f32, alpha_kind: f32, premultiply: bool) -> vec3<f32> {
	if alpha_kind > 1.5 && premultiply {
		return rgb * out_a;
	}
	return rgb;
}

fn lil_apply_distance_fade(rgb: vec3<f32>, out_a: f32, wp: vec3<f32>, n: vec3<f32>, v: vec3<f32>, front_facing: bool) -> vec4<f32> {
	let fade_params = drawu.distance_fade;
	if abs(fade_params.z) <= 0.000001 {
		return vec4<f32>(rgb, out_a);
	}
	let denom = select(0.000001, fade_params.y - fade_params.x, abs(fade_params.y - fade_params.x) > 0.000001);
	let depth = length(frame.camera_pos.xyz - wp);
	var dist_fade = clamp((depth - fade_params.x) / denom, 0.0, 1.0);
	if abs(fade_params.w) <= 0.000001 {
		dist_fade = dist_fade * fade_params.z;
	} else {
		let facing = select(0.0, 1.0, front_facing);
		dist_fade = select(dist_fade * fade_params.z, fade_params.z, facing < (fade_params.w - 1.0));
	}
	var fade_color = drawu.distance_fade_color.rgb;
	if drawu.distance_fade_rim_color.a > 0.0 {
		let fade_rim = pow(clamp(1.0 - abs(dot(normalize(n), normalize(v))), 0.0, 1.0), max(drawu.distance_fade_params.y, 0.00001));
		fade_color = mix(fade_color, drawu.distance_fade_rim_color.rgb * rgb, fade_rim * drawu.distance_fade_rim_color.a);
	}
	if drawu.distance_fade_color.a < 0.0 {
		return vec4<f32>(mix(rgb, fade_color, dist_fade), out_a - dist_fade);
	}
	return vec4<f32>(
		mix(rgb, fade_color * drawu.distance_fade_color.a, dist_fade),
		mix(out_a, out_a * drawu.distance_fade_color.a, dist_fade),
	);
}

fn lil_apply_dissolve(alpha: f32, uv: vec2<f32>, wp: vec3<f32>, dissolve_active: f32, invert: f32) -> vec2<f32> {
	let params = vec4<f32>(round(drawu.dissolve_params.x), round(drawu.dissolve_params.y), drawu.dissolve_params.z, max(drawu.dissolve_params.w, 0.000001));
	if params.x <= 0.0 || dissolve_active <= 0.5 {
		return vec2<f32>(alpha, 0.0);
	}
	let dissolve_mask_uv = uv * drawu.dissolve_mask_uv_offset_scale.zw + drawu.dissolve_mask_uv_offset_scale.xy;
	let dissolve_mask_val = textureSample(dissolve_mask_tex, base_samp, dissolve_mask_uv).r;
	let dissolve_noise_uv = lil_calc_uv_scroll_rotate(uv, drawu.dissolve_noise_uv_offset_scale, drawu.dissolve_noise_uv_anim_params);
	let dissolve_noise = (textureSample(dissolve_noise_mask_tex, base_samp, dissolve_noise_uv).r - 0.5) * drawu.dissolve_ext.x * drawu.dissolve_ext.z;
	let has_noise = drawu.dissolve_ext.z > 0.5;
	var dissolve_mask = select(1.0, dissolve_mask_val, params.x == 1.0 && drawu.dissolve_ext.y > 0.5);
	var dissolve_alpha = 0.0;
	if params.x == 1.0 {
		let value = dissolve_mask + dissolve_noise;
		dissolve_alpha = 1.0 - clamp(abs(value - params.z) / params.w, 0.0, 1.0);
		dissolve_mask = select(0.0, 1.0, value > params.z);
	} else if params.x == 2.0 {
		let directional = select(lil_rotate_uv(uv, drawu.dissolve_pos.w).x, dot(uv, normalize_or2(drawu.dissolve_pos.xy, vec2<f32>(1.0, 0.0))) + dissolve_noise, has_noise);
		let shape_value = select(distance(uv, drawu.dissolve_pos.xy) + dissolve_noise, directional, params.y == 1.0);
		dissolve_mask = dissolve_mask * select(0.0, 1.0, shape_value > params.z);
		dissolve_alpha = 1.0 - clamp(abs(shape_value - params.z) / params.w, 0.0, 1.0);
	} else if params.x == 3.0 {
		let shape_value = select(distance(wp, drawu.dissolve_pos.xyz), dot(wp, normalize_or(drawu.dissolve_pos.xyz, vec3<f32>(1.0, 0.0, 0.0))), params.y == 1.0) + dissolve_noise;
		dissolve_mask = dissolve_mask * select(0.0, 1.0, shape_value > params.z);
		dissolve_alpha = 1.0 - clamp(abs(shape_value - params.z) / params.w, 0.0, 1.0);
	}
	let dissolve_alpha_mask = select(dissolve_mask, 1.0 - dissolve_mask, invert > 0.5);
	let edge_alpha = select(dissolve_alpha, 1.0 - dissolve_alpha, invert > 0.5);
	return vec2<f32>(alpha * dissolve_alpha_mask, edge_alpha);
}

fn lil_udim_discard(i: VsOut) -> bool {
	if drawu.udim_discard_params.x <= 0.5 {
		return false;
	}
	let udim = lil_select_uv(drawu.udim_discard_params.z, i.uv, i.uv1, i.uv2, i.uv3);
	let xmask = vec4<f32>(
		select(0.0, 1.0, udim.x >= 0.0 && udim.x < 1.0),
		select(0.0, 1.0, udim.x >= 1.0 && udim.x < 2.0),
		select(0.0, 1.0, udim.x >= 2.0 && udim.x < 3.0),
		select(0.0, 1.0, udim.x >= 3.0 && udim.x < 4.0),
	);
	var discarded = 0.0;
	discarded = discarded + select(0.0, dot(drawu.udim_discard_row0, xmask), udim.y >= 0.0 && udim.y < 1.0);
	discarded = discarded + select(0.0, dot(drawu.udim_discard_row1, xmask), udim.y >= 1.0 && udim.y < 2.0);
	discarded = discarded + select(0.0, dot(drawu.udim_discard_row2, xmask), udim.y >= 2.0 && udim.y < 3.0);
	discarded = discarded + select(0.0, dot(drawu.udim_discard_row3, xmask), udim.y >= 3.0 && udim.y < 4.0);
	let in_grid = udim.y >= 0.0 && udim.y < 4.0 && udim.x >= 0.0 && udim.x < 4.0;
	return in_grid && discarded > 0.001;
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

fn lil_ortho_normalize(tangent: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
	let projected = tangent - normal * dot(normal, tangent);
	let len = length(projected);
	return select(vec3<f32>(0.0, 1.0, 0.0), projected / len, len > 0.0001);
}

fn toon_matcap_uv(n: vec3<f32>, v: vec3<f32>, perspective: f32, z_rot_cancel: f32) -> vec2<f32> {
	let camera_pos_len = length(frame.camera_pos.xyz);
	let camera_dir = select(vec3<f32>(0.0, 0.0, 1.0), normalize(frame.camera_pos.xyz), camera_pos_len >= 0.0001);
	let normal_vd = normalize(mix(camera_dir, v, clamp(perspective, 0.0, 1.0)));
	let camera_up = vec3<f32>(0.0, 1.0, 0.0);
	let old_tangent_raw = vec3<f32>(normal_vd.z, 0.0, -normal_vd.x);
	let old_tangent_len = length(old_tangent_raw);
	let old_tangent = select(vec3<f32>(1.0, 0.0, 0.0), old_tangent_raw / old_tangent_len, old_tangent_len > 0.0001);
	let old_bitangent = normalize(cross(normal_vd, old_tangent));
	let bitangent = lil_ortho_normalize(mix(old_bitangent, camera_up, step(0.5, z_rot_cancel)), normal_vd);
	let tangent = cross(normal_vd, bitangent);
	return vec2<f32>(dot(tangent, n), dot(bitangent, n)) * 0.5 + vec2<f32>(0.5, 0.5);
}

fn toon_reflection_uv(n: vec3<f32>, v: vec3<f32>) -> vec2<f32> {
	let r = normalize(reflect(-v, n));
	let u = atan2(r.z, r.x) * 0.15915494309189535 + 0.5;
	let vv = acos(clamp(r.y, -1.0, 1.0)) * 0.3183098861837907;
	return vec2<f32>(u, vv);
}

fn clip_to_screen_uv(clip: vec4<f32>) -> vec2<f32> {
	let ndc = clip.xy / max(clip.w, 0.000001);
	return vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
}

fn screen_uv(fragment_position: vec4<f32>) -> vec2<f32> {
	let dims = max(vec2<f32>(textureDimensions(screen_tex, 0)), vec2<f32>(1.0));
	return clamp(fragment_position.xy / dims, vec2<f32>(0.0), vec2<f32>(1.0));
}

fn screen_normal_offset(world_pos: vec3<f32>, normal: vec3<f32>, base_uv: vec2<f32>) -> vec2<f32> {
	let shifted_clip = frame.view_proj * vec4<f32>(world_pos + normalize(normal) * 0.05, 1.0);
	let projected = clip_to_screen_uv(shifted_clip) - base_uv;
	let fallback = vec2<f32>(normal.x, -normal.y) * 0.02;
	let use_fallback = length(projected) < 0.000001 || !all(projected == projected);
	return clamp(select(projected, fallback, use_fallback), vec2<f32>(-0.08), vec2<f32>(0.08));
}

fn liltoon_refraction_offset(normal: vec3<f32>) -> vec2<f32> {
	// lilToon Gem uses mul((float3x3)LIL_MATRIX_V, fd.N).xy for screen refraction.
	let view_normal = normalize((frame.view * vec4<f32>(normalize(normal), 0.0)).xyz);
	return view_normal.xy;
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

fn lil_flip_backface_normal(n: vec3<f32>, front_facing: bool, flip_backface_normal: f32) -> vec3<f32> {
	return select(n, -n, !front_facing && flip_backface_normal > 0.5);
}

fn lil_shadow_border_ao_mask(mask: vec3<f32>) -> vec3<f32> {
	return clamp(
		vec3<f32>(
			mask.r * drawu.shadow_ao_shift.x + drawu.shadow_ao_shift.y,
			mask.g * drawu.shadow_ao_shift.z + drawu.shadow_ao_shift.w,
			mask.b * drawu.shadow_ao_shift2.x + drawu.shadow_ao_shift2.y,
		),
		vec3<f32>(0.0),
		vec3<f32>(1.0),
	);
}

fn lil_shadow_apply_pre_ao(value: f32, mask: f32, post_ao: bool) -> f32 {
	return select(value * mask, value, post_ao);
}

fn lil_shadow_apply_post_ao(value: f32, mask: f32, post_ao: bool) -> f32 {
	return select(value, value * mask, post_ao);
}

fn lil_apply_rim_shade(lit: vec3<f32>, geometry_n: vec3<f32>, n: vec3<f32>, v: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
	if (drawu.rim_shade_params.x <= 0.5) {
		return lit;
	}
	let rim_shade_n = normalize(mix(geometry_n, n, clamp(drawu.rim_ext_params.w, 0.0, 1.0)));
	let rim_shade_raw = pow(clamp(1.0 - abs(dot(rim_shade_n, v)), 0.0, 1.0), max(drawu.rim_shade_params.w, 0.00001));
	let rim_shade_mask = textureSample(rim_shade_mask_tex, rim_samp, uv).r;
	let rim_shade = lil_tooning_scale(
		rim_shade_raw,
		clamp(drawu.rim_shade_params.y, 0.0, 1.0),
		clamp(drawu.rim_shade_params.z, 0.0, 1.0)
	) * rim_shade_mask * clamp(drawu.rim_shade_color.a, 0.0, 1.0);
	return mix(lit, lit * drawu.rim_shade_color.rgb, rim_shade);
}

struct GlitterVoronoi {
	near: vec4<f32>,
	nearoffset: vec2<f32>,
}

fn lil_glitter_hash(cell: vec2<f32>) -> vec3<f32> {
	let h = dot(cell, vec2<f32>(12.9898, 78.233));
	return fract(sin(vec3<f32>(h, h, h)) * vec3<f32>(46203.4357, 21091.5327, 35771.1966));
}

fn lil_nsq_distance(a: vec2<f32>, b: vec2<f32>) -> f32 {
	let d = a - b;
	return dot(d, d);
}

fn lil_glitter_voronoi(pos: vec2<f32>, scale_randomize: f32) -> GlitterVoronoi {
	let q = floor(pos);
	let noise0 = lil_glitter_hash(q);
	let noise1 = lil_glitter_hash(q + vec2<f32>(1.0, 0.0));
	let noise2 = lil_glitter_hash(q + vec2<f32>(0.0, 1.0));
	let noise3 = lil_glitter_hash(q + vec2<f32>(1.0, 1.0));
	let fracpos = fract(pos).xyxy + vec4<f32>(0.5, 0.5, -0.5, -0.5);
	var dist4 = vec4<f32>(
		lil_nsq_distance(fracpos.xy, noise0.xy),
		lil_nsq_distance(fracpos.zy, noise1.xy),
		lil_nsq_distance(fracpos.xw, noise2.xy),
		lil_nsq_distance(fracpos.zw, noise3.xy),
	);
	dist4 = mix(dist4, dist4 / max(vec4<f32>(noise0.z, noise1.z, noise2.z, noise3.z), vec4<f32>(0.001)), clamp(scale_randomize, 0.0, 1.0));

	let nearoffset0 = select(vec3<f32>(1.0, 0.0, dist4.y), vec3<f32>(0.0, 0.0, dist4.x), dist4.x < dist4.y);
	let nearoffset1 = select(vec3<f32>(1.0, 1.0, dist4.w), vec3<f32>(0.0, 1.0, dist4.z), dist4.z < dist4.w);
	let nearoffset = select(nearoffset1.xy, nearoffset0.xy, nearoffset0.z < nearoffset1.z);
	let near0 = select(vec4<f32>(noise1, dist4.y), vec4<f32>(noise0, dist4.x), dist4.x < dist4.y);
	let near1 = select(vec4<f32>(noise3, dist4.w), vec4<f32>(noise2, dist4.z), dist4.z < dist4.w);
	return GlitterVoronoi(select(near1, near0, near0.w < near1.w), nearoffset);
}

fn lil_select_uv(mode: f32, uv0: vec2<f32>, uv1: vec2<f32>, uv2: vec2<f32>, uv3: vec2<f32>) -> vec2<f32> {
	if (mode < 0.5) {
		return uv0;
	}
	if (mode < 1.5) {
		return uv1;
	}
	if (mode < 2.5) {
		return uv2;
	}
	return uv3;
}

fn lil_select_layer_uv(mode: f32, uv0: vec2<f32>, uv1: vec2<f32>, uv2: vec2<f32>, uv3: vec2<f32>, uv_mat: vec2<f32>) -> vec2<f32> {
	if (mode < 3.5) {
		return lil_select_uv(mode, uv0, uv1, uv2, uv3);
	}
	return uv_mat;
}

fn lil_select_emission_uv(mode: f32, uv0: vec2<f32>, uv1: vec2<f32>, uv2: vec2<f32>, uv3: vec2<f32>, uv_rim: vec2<f32>) -> vec2<f32> {
	if (mode < 3.5) {
		return lil_select_uv(mode, uv0, uv1, uv2, uv3);
	}
	return uv_rim;
}

fn lil_is_in_0_to_1_scalar(value_in: f32, nv: f32) -> f32 {
	let value = 0.5 - abs(value_in - 0.5);
	return clamp(value / clamp(fwidth(value), 0.0001, max(nv, 0.0001)), 0.0, 1.0);
}

fn lil_is_in_0_to_1(uv: vec2<f32>, nv: f32) -> f32 {
	return lil_is_in_0_to_1_scalar(uv.x, nv) * lil_is_in_0_to_1_scalar(uv.y, nv);
}

fn lil_calc_atlas_animation_uv(uv: vec2<f32>, decal_animation: vec4<f32>, decal_sub_param: vec4<f32>) -> vec2<f32> {
	if decal_animation.x <= 0.0 || decal_animation.y <= 0.0 || decal_animation.z <= 0.0 {
		return uv;
	}
	var out_uv = mix(vec2<f32>(uv.x, 1.0 - uv.y), vec2<f32>(0.5, 0.5), clamp(decal_sub_param.z, 0.0, 1.0));
	let anim_time = select(decal_animation.z, floor(frame.time_params.x * decal_animation.w) % decal_animation.z, decal_animation.w != 0.0);
	let offset = vec2<f32>(anim_time % decal_animation.x, floor(anim_time / decal_animation.x));
	out_uv = (out_uv + offset) * decal_sub_param.xy / max(decal_animation.xy, vec2<f32>(1.0, 1.0));
	out_uv.y = 1.0 - out_uv.y;
	return out_uv;
}

fn lil_calc_decal_uv(uv: vec2<f32>, uv_st: vec4<f32>, angle: f32, flags: vec4<f32>, transform: vec4<f32>, is_right_hand: bool) -> vec2<f32> {
	var out_uv = uv;
	if flags.w > 0.5 {
		out_uv.x = abs(out_uv.x - 0.5) + 0.5;
	}
	out_uv = out_uv * uv_st.zw + uv_st.xy;
	if transform.z > 0.5 && uv.x < 0.5 {
		out_uv.x = 1.0 - out_uv.x;
	}
	if transform.y > 0.5 && is_right_hand {
		out_uv.x = 1.0 - out_uv.x;
	}
	if flags.y > 0.5 && is_right_hand {
		out_uv.x = -1.0;
	}
	if flags.z > 0.5 && !is_right_hand {
		out_uv.x = -1.0;
	}
	out_uv = (out_uv - uv_st.xy) / max(abs(uv_st.zw), vec2<f32>(0.0001, 0.0001));
	out_uv = lil_rotate_uv(out_uv, angle);
	return out_uv * uv_st.zw + uv_st.xy;
}

struct LayerSubTexUv {
	sample_uv: vec2<f32>,
	alpha_mask: f32,
}

fn lil_layer_sub_tex_uv(uv: vec2<f32>, uv_st: vec4<f32>, flags: vec4<f32>, transform: vec4<f32>, decal_animation: vec4<f32>, decal_sub_param: vec4<f32>, nv: f32, is_right_hand: bool) -> LayerSubTexUv {
	if flags.x <= 0.5 {
		return LayerSubTexUv(uv * uv_st.zw + uv_st.xy, 1.0);
	}
	let decal_uv = lil_calc_decal_uv(uv, uv_st, transform.x, flags, transform, is_right_hand);
	let sample_uv = lil_calc_atlas_animation_uv(decal_uv, decal_animation, decal_sub_param);
	return LayerSubTexUv(sample_uv, lil_is_in_0_to_1(decal_uv, clamp(nv - 0.05, 0.0, 1.0)));
}

fn lil_calc_glitter(uv: vec2<f32>, normal: vec3<f32>, view_dir: vec3<f32>, camera_dir: vec3<f32>, light_dir: vec3<f32>) -> vec3<f32> {
	let scale = max(abs(drawu.glitter_params1.xy), vec2<f32>(0.0001, 0.0001));
	let pos_raw = uv * scale;
	let dd = fwidth(pos_raw);
	let random_cell = floor(pos_raw / max(floor(dd + vec2<f32>(3.0, 3.0)), vec2<f32>(1.0, 1.0)));
	let factor = fract(sin(dot(random_cell, vec2<f32>(12.9898, 78.233))) * 46203.4357) + 0.5;
	let factor2 = floor(dd + vec2<f32>(factor * 0.5, factor * 0.5));
	let pos = pos_raw / max(vec2<f32>(1.0, 1.0), factor2) + scale * factor2;
	let voronoi = lil_glitter_voronoi(pos, drawu.glitter_ext2.y);
	let nearest = voronoi.near;
	let unity_time_x = frame.time_params.x * 0.05;
	let time_seed = unity_time_x * drawu.glitter_params2.x;
	var glitter_normal = abs(fract(nearest.xyz * 14.274 + vec3<f32>(time_seed)) * 2.0 - vec3<f32>(1.0));
	glitter_normal = normalize_or(glitter_normal * 2.0 - vec3<f32>(1.0), normal);
	let sensitivity = max(drawu.glitter_ext.x, 0.0001);
	let contrast = max(drawu.glitter_params1.w, 0.0);
	var glitter = dot(glitter_normal, normalize_or(camera_dir, view_dir));
	glitter = abs(fract(glitter * sensitivity + sensitivity) - 0.5) * 4.0 - 1.0;
	glitter = clamp(1.0 - (glitter * contrast + contrast), 0.0, 1.0);
	glitter = pow(glitter, max(drawu.glitter_control.w, 0.0001));
	let size = max(drawu.glitter_params1.z, 0.0);
	glitter = glitter * clamp((size - nearest.w) / max(fwidth(nearest.w), 0.0001), 0.0, 1.0);
	let half_dir = normalize_or(view_dir + light_dir * drawu.glitter_params2.z, normal);
	let nh = clamp(dot(normal, half_dir), 0.0, 1.0);
	glitter = glitter * clamp(nh * drawu.glitter_params2.y + 1.0 - drawu.glitter_params2.y, 0.0, 1.0);
	var glitter_color = glitter - glitter * fract(nearest.xyz * 278.436) * clamp(drawu.glitter_params2.w, 0.0, 1.0);
	if (drawu.glitter_ext3.y > 0.5) {
		var mask_uv = pos - floor(pos) - voronoi.nearoffset + vec2<f32>(0.5, 0.5) - nearest.xy;
		mask_uv = mask_uv / max(drawu.glitter_params1.z, 0.0001) * drawu.glitter_shape_uv_offset_scale.zw + drawu.glitter_shape_uv_offset_scale.xy;
		if (drawu.glitter_ext3.z > 0.5) {
			let angle = nearest.z * 785.238;
			let si = sin(angle);
			let co = cos(angle);
			mask_uv = vec2<f32>(mask_uv.x * co - mask_uv.y * si, mask_uv.x * si + mask_uv.y * co);
		}
		let random_scale = mix(1.0, inverseSqrt(max(nearest.z, 0.001)), clamp(drawu.glitter_ext2.y, 0.0, 1.0));
		mask_uv = mask_uv * random_scale + vec2<f32>(0.5, 0.5);
		let in_bounds = mask_uv.x == clamp(mask_uv.x, 0.0, 1.0) && mask_uv.y == clamp(mask_uv.y, 0.0, 1.0);
		let atlas = max(drawu.glitter_atlas.xy, vec2<f32>(1.0, 1.0));
		mask_uv = (mask_uv + floor(nearest.xy * atlas)) / atlas;
		let mipfactor = 0.125 / max(drawu.glitter_params1.z, 0.0001) * atlas * drawu.glitter_shape_uv_offset_scale.zw * random_scale;
		let shape_tex = textureSampleGrad(glitter_shape_tex, base_samp, mask_uv, abs(dpdx(pos)) * mipfactor.x, abs(dpdy(pos)) * mipfactor.y);
		glitter_color = glitter_color * shape_tex.rgb * select(0.0, shape_tex.a, in_bounds);
	}
	return glitter_color;
}

fn fresnel_lerp(specular: vec3<f32>, grazing_term: f32, nv: f32) -> vec3<f32> {
	let f = pow(clamp(1.0 - nv, 0.0, 1.0), 5.0);
	return mix(specular, vec3<f32>(grazing_term), f);
}

fn lil_reflection_mip(perceptual_roughness: f32) -> f32 {
	let p = clamp(perceptual_roughness, 0.0, 1.0);
	return p * (10.2 - 4.2 * p);
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

fn apply_lil_layer_distance_fade(layer_a: f32, fade: vec4<f32>, depth: f32) -> f32 {
	if abs(fade.z) <= 0.000001 {
		return layer_a;
	}
	let denom = select(0.000001, fade.y - fade.x, abs(fade.y - fade.x) > 0.000001);
	let fade_alpha = clamp((depth - fade.x) / denom, 0.0, 1.0);
	return mix(layer_a, layer_a * fade_alpha, clamp(fade.z, 0.0, 1.0));
}

fn apply_lil_layer_cull(layer_a: f32, cull: f32, front_facing: bool) -> f32 {
	if (cull > 0.5 && cull < 1.5 && front_facing) || (cull > 1.5 && !front_facing) {
		return 0.0;
	}
	return layer_a;
}

struct MainLayerResult {
	col: vec4<f32>,
	dissolve_emission: vec3<f32>,
	second_unlit: vec4<f32>,
	third_unlit: vec4<f32>,
}

fn lil_apply_layer_dissolve(alpha: f32, uv: vec2<f32>, wp: vec3<f32>, params_in: vec4<f32>, pos: vec4<f32>, mask_val_in: f32, mask_enabled: f32, noise: f32, has_noise: bool) -> vec2<f32> {
	let params = vec4<f32>(round(params_in.x), round(params_in.y), params_in.z, max(params_in.w, 0.000001));
	if params.x <= 0.0 {
		return vec2<f32>(alpha, 0.0);
	}
	var dissolve_mask = select(1.0, mask_val_in, params.x == 1.0 && mask_enabled > 0.5);
	var dissolve_alpha = 0.0;
	if params.x == 1.0 {
		let value = dissolve_mask + noise;
		dissolve_alpha = 1.0 - clamp(abs(value - params.z) / params.w, 0.0, 1.0);
		dissolve_mask = select(0.0, 1.0, value > params.z);
	} else if params.x == 2.0 {
		let directional = select(lil_rotate_uv(uv, pos.w).x, dot(uv, normalize_or2(pos.xy, vec2<f32>(1.0, 0.0))) + noise, has_noise);
		let shape_value = select(distance(uv, pos.xy) + noise, directional, params.y == 1.0);
		dissolve_mask = dissolve_mask * select(0.0, 1.0, shape_value > params.z);
		dissolve_alpha = 1.0 - clamp(abs(shape_value - params.z) / params.w, 0.0, 1.0);
	} else if params.x == 3.0 {
		let shape_value = select(distance(wp, pos.xyz), dot(wp, normalize_or(pos.xyz, vec3<f32>(1.0, 0.0, 0.0))), params.y == 1.0) + noise;
		dissolve_mask = dissolve_mask * select(0.0, 1.0, shape_value > params.z);
		dissolve_alpha = 1.0 - clamp(abs(shape_value - params.z) / params.w, 0.0, 1.0);
	}
	return vec2<f32>(alpha * dissolve_mask, dissolve_alpha);
}

fn apply_lil_main_layers(base: vec4<f32>, uv: vec2<f32>, uv1: vec2<f32>, uv2: vec2<f32>, uv3: vec2<f32>, uv_mat: vec2<f32>, wp: vec3<f32>, nv: f32, is_right_hand: bool, front_facing: bool, is_liltoon: bool) -> MainLayerResult {
	var out_col = base;
	var dissolve_emission = vec3<f32>(0.0);
	var second_unlit = vec4<f32>(0.0);
	var third_unlit = vec4<f32>(0.0);
	let depth = length(frame.camera_pos.xyz - wp);
	if (drawu.main2nd_params.x > 0.5) {
		let layer_uv_raw = lil_select_layer_uv(drawu.main2nd_ext.x, uv, uv1, uv2, uv3, uv_mat);
		let layer_uv = lil_layer_sub_tex_uv(layer_uv_raw, drawu.main2nd_uv_offset_scale, drawu.main2nd_decal_flags, drawu.main2nd_decal_transform, drawu.main2nd_decal_animation, drawu.main2nd_decal_sub_param, nv, is_right_hand);
		let mask_uv = uv * drawu.main2nd_blend_mask_uv_offset_scale.zw + drawu.main2nd_blend_mask_uv_offset_scale.xy;
		let layer = textureSample(main2nd_tex, base_samp, layer_uv.sample_uv) * drawu.main2nd_color;
		var layer_alpha = layer.a * layer_uv.alpha_mask * textureSample(main2nd_blend_mask_tex, base_samp, mask_uv).r;
		if (is_liltoon) {
			let dissolve_mask_uv = uv * drawu.main2nd_dissolve_mask_uv_offset_scale.zw + drawu.main2nd_dissolve_mask_uv_offset_scale.xy;
			let dissolve_noise_uv = lil_calc_uv_scroll_rotate(uv, drawu.main2nd_dissolve_noise_uv_offset_scale, drawu.main2nd_dissolve_noise_uv_anim_params);
			let dissolve_noise = (textureSample(main2nd_dissolve_noise_mask_tex, base_samp, dissolve_noise_uv).r - 0.5) * drawu.main2nd_dissolve_ext.x * drawu.main2nd_dissolve_ext.z;
			let dissolve_result = lil_apply_layer_dissolve(
				layer_alpha,
				uv,
				wp,
				drawu.main2nd_dissolve_params,
				drawu.main2nd_dissolve_pos,
				textureSample(main2nd_dissolve_mask_tex, base_samp, dissolve_mask_uv).r,
				drawu.main2nd_dissolve_ext.y,
				dissolve_noise,
				drawu.main2nd_dissolve_ext.z > 0.5
			);
			layer_alpha = dissolve_result.x;
			dissolve_emission = dissolve_emission + drawu.main2nd_dissolve_color.rgb * dissolve_result.y;
		}
		layer_alpha = apply_lil_layer_distance_fade(layer_alpha, drawu.main2nd_distance_fade, depth);
		layer_alpha = apply_lil_layer_cull(layer_alpha, drawu.main2nd_ext.y, front_facing);
		second_unlit = vec4<f32>(layer.rgb, layer_alpha * (1.0 - clamp(drawu.main2nd_params.y, 0.0, 1.0)));
		let out_alpha = apply_lil_main_layer_alpha(out_col.a, layer_alpha, drawu.main2nd_params.z);
		let out_rgb = lil_blend_color(out_col.rgb, layer.rgb, layer_alpha * drawu.main2nd_params.y, drawu.main2nd_params.w);
		out_col = vec4<f32>(out_rgb, out_alpha);
	}
	if (drawu.main3rd_params.x > 0.5) {
		let layer_uv_raw = lil_select_layer_uv(drawu.main3rd_ext.x, uv, uv1, uv2, uv3, uv_mat);
		let layer_uv = lil_layer_sub_tex_uv(layer_uv_raw, drawu.main3rd_uv_offset_scale, drawu.main3rd_decal_flags, drawu.main3rd_decal_transform, drawu.main3rd_decal_animation, drawu.main3rd_decal_sub_param, nv, is_right_hand);
		let mask_uv = uv * drawu.main3rd_blend_mask_uv_offset_scale.zw + drawu.main3rd_blend_mask_uv_offset_scale.xy;
		let layer = textureSample(main3rd_tex, base_samp, layer_uv.sample_uv) * drawu.main3rd_color;
		var layer_alpha = layer.a * layer_uv.alpha_mask * textureSample(main3rd_blend_mask_tex, base_samp, mask_uv).r;
		if (is_liltoon) {
			let dissolve_mask_uv = uv * drawu.main3rd_dissolve_mask_uv_offset_scale.zw + drawu.main3rd_dissolve_mask_uv_offset_scale.xy;
			let dissolve_noise_uv = lil_calc_uv_scroll_rotate(uv, drawu.main3rd_dissolve_noise_uv_offset_scale, drawu.main3rd_dissolve_noise_uv_anim_params);
			let dissolve_noise = (textureSample(main3rd_dissolve_noise_mask_tex, base_samp, dissolve_noise_uv).r - 0.5) * drawu.main3rd_dissolve_ext.x * drawu.main3rd_dissolve_ext.z;
			let dissolve_result = lil_apply_layer_dissolve(
				layer_alpha,
				uv,
				wp,
				drawu.main3rd_dissolve_params,
				drawu.main3rd_dissolve_pos,
				textureSample(main3rd_dissolve_mask_tex, base_samp, dissolve_mask_uv).r,
				drawu.main3rd_dissolve_ext.y,
				dissolve_noise,
				drawu.main3rd_dissolve_ext.z > 0.5
			);
			layer_alpha = dissolve_result.x;
			dissolve_emission = dissolve_emission + drawu.main3rd_dissolve_color.rgb * dissolve_result.y;
		}
		layer_alpha = apply_lil_layer_distance_fade(layer_alpha, drawu.main3rd_distance_fade, depth);
		layer_alpha = apply_lil_layer_cull(layer_alpha, drawu.main3rd_ext.y, front_facing);
		third_unlit = vec4<f32>(layer.rgb, layer_alpha * (1.0 - clamp(drawu.main3rd_params.y, 0.0, 1.0)));
		let out_alpha = apply_lil_main_layer_alpha(out_col.a, layer_alpha, drawu.main3rd_params.z);
		let out_rgb = lil_blend_color(out_col.rgb, layer.rgb, layer_alpha * drawu.main3rd_params.y, drawu.main3rd_params.w);
		out_col = vec4<f32>(out_rgb, out_alpha);
	}
	return MainLayerResult(out_col, dissolve_emission, second_unlit, third_unlit);
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

fn lil_parallax_view_ts(n: vec3<f32>, tangent_in: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
	let tangent_ortho = tangent_in.xyz - n * dot(n, tangent_in.xyz);
	let t = normalize(tangent_ortho);
	let b = normalize(cross(n, t)) * tangent_in.w;
	return vec3<f32>(dot(v, t), dot(v, b), max(dot(v, n), 0.0001));
}

fn lil_apply_parallax(uv: vec2<f32>, n: vec3<f32>, tangent_in: vec4<f32>, v: vec3<f32>, is_liltoon: bool) -> vec2<f32> {
	if (!is_liltoon || drawu.parallax_params.x <= 0.5) {
		return uv;
	}
	let scale = drawu.parallax_params.z;
	if (abs(scale) <= 0.000001) {
		return uv;
	}
	let view_ts = lil_parallax_view_ts(n, tangent_in, v);
	let parallax_offset = view_ts.xy / (view_ts.z + 0.5);
	let parallax_map_uv = uv * drawu.parallax_uv_offset_scale.zw + drawu.parallax_uv_offset_scale.xy;
	if (drawu.parallax_params.y <= 0.5) {
		let height = (textureSampleLevel(parallax_tex, base_samp, parallax_map_uv, 0.0).r - drawu.parallax_params.w) * scale;
		return uv + height * parallax_offset;
	}

	var ray_pos = vec3<f32>(parallax_map_uv, 1.0) + (1.0 - drawu.parallax_params.w) * scale * view_ts;
	let ray_step_unscaled = -view_ts;
	var ray_step = vec3<f32>(ray_step_unscaled.xy * drawu.parallax_uv_offset_scale.zw, ray_step_unscaled.z);
	let step_count = min(max(u32(abs(scale) * 400.0), 1u), 64u);
	ray_step = ray_step / vec3<f32>(f32(step_count), f32(step_count), max(abs(scale) * f32(step_count), 0.0001));
	var prev_height = 0.0;
	var height = 0.0;
	for (var step = 0u; step < step_count; step = step + 1u) {
		prev_height = height;
		ray_pos = ray_pos + ray_step;
		height = textureSampleLevel(parallax_tex, base_samp, ray_pos.xy, 0.0).r;
		if (height >= ray_pos.z) {
			break;
		}
	}
	let prev_pos = ray_pos.xy - ray_step.xy;
	let next_delta = height - ray_pos.z;
	let prev_delta = prev_height - ray_pos.z + ray_step.z;
	let denom = next_delta - prev_delta;
	let weight = select(clamp(next_delta / denom, 0.0, 1.0), 0.0, abs(denom) <= 0.000001);
	let pom_uv = mix(ray_pos.xy, prev_pos, weight);
	return uv + (pom_uv - parallax_map_uv);
}

fn normal_mapped(n_in: vec3<f32>, tangent_in: vec4<f32>, uv: vec2<f32>, uv1: vec2<f32>, uv2: vec2<f32>, uv3: vec2<f32>, scale: f32) -> vec3<f32> {
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
		let normal2nd_base_uv = lil_select_uv(drawu.normal2nd_params.z, uv, uv1, uv2, uv3);
		let normal2nd_uv = normal2nd_base_uv * drawu.normal2nd_uv_offset_scale.zw + drawu.normal2nd_uv_offset_scale.xy;
		let packed2 = textureSample(normal2nd_tex, normal_samp, normal2nd_uv).xyz;
		let scale_mask = textureSample(normal2nd_scale_mask_tex, base_samp, uv).r;
		var tn2 = packed2 * 2.0 - vec3<f32>(1.0, 1.0, 1.0);
		tn2.x = tn2.x * drawu.normal2nd_params.y * scale_mask;
		tn2.y = tn2.y * drawu.normal2nd_params.y * scale_mask;
		tn = vec3<f32>(tn.xy + tn2.xy, tn.z * tn2.z);
	}
	tn = normalize(tn);

	let tangent_ortho = tangent_in.xyz - n * dot(n, tangent_in.xyz);
	let t = normalize(tangent_ortho);
	let b = normalize(cross(n, t)) * tangent_in.w;
	return normalize(t * tn.x + b * tn.y + n * tn.z);
}

fn custom_normal_mapped(
	n_in: vec3<f32>,
	tangent_in: vec4<f32>,
	uv: vec2<f32>,
	uv_offset_scale: vec4<f32>,
	scale: f32,
	which: f32,
) -> vec3<f32> {
	let n = normalize(n_in);
	let map_uv = uv * uv_offset_scale.zw + uv_offset_scale.xy;
	let packed = select(
		textureSample(matcap_bump_tex, normal_samp, map_uv).xyz,
		textureSample(matcap2_bump_tex, normal_samp, map_uv).xyz,
		which > 0.5,
	);
	var tn = packed * 2.0 - vec3<f32>(1.0, 1.0, 1.0);
	tn.x = tn.x * scale;
	tn.y = tn.y * scale;
	tn = normalize(tn);
	let tangent_ortho = tangent_in.xyz - n * dot(n, tangent_in.xyz);
	let t = normalize(tangent_ortho);
	let b = normalize(cross(n, t)) * tangent_in.w;
	return normalize(t * tn.x + b * tn.y + n * tn.z);
}

fn liltoon_custom_matcap_normal(
	n_in: vec3<f32>,
	tangent_in: vec4<f32>,
	uv: vec2<f32>,
	uv_offset_scale: vec4<f32>,
	scale: f32,
	which: f32,
	front_facing: bool,
	flags: u32,
	flip_backface_normal: f32,
	gem_backface_normal: bool,
	v: vec3<f32>,
) -> vec3<f32> {
	let mapped = face_normal(custom_normal_mapped(n_in, tangent_in, uv, uv_offset_scale, scale, which), front_facing, flags);
	let flipped = lil_flip_backface_normal(mapped, front_facing, flip_backface_normal);
	return select(flipped, normalize(flipped - v * 0.2), gem_backface_normal);
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

fn lil_correct_light_color(raw: vec3<f32>) -> vec3<f32> {
	let luminance = dot(raw, vec3<f32>(0.2126, 0.7152, 0.0722));
	let monochrome = mix(raw, vec3<f32>(luminance, luminance, luminance), clamp(drawu.lighting_ext_params.z, 0.0, 1.0));
	let min_limit = max(drawu.lighting_ext_params.x, 0.0);
	let max_limit = max(drawu.lighting_ext_params.y, min_limit);
	return clamp(monochrome, vec3<f32>(min_limit, min_limit, min_limit), vec3<f32>(max_limit, max_limit, max_limit));
}

fn lil_direct_light_color() -> vec3<f32> {
	return lil_correct_light_color(frame.light_color.rgb * frame.light_color.w);
}

fn liltoon_light_color() -> vec3<f32> {
	let main_light = frame.light_color.rgb * frame.light_color.w;
	let sh_proxy = frame.ambient_color.rgb * frame.ambient_color.w;
	return lil_correct_light_color(main_light + sh_proxy);
}

@fragment
fn fs_lit(i: VsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
	let dbg = bitcast<u32>(drawu.params.w);
	discard_by_cull_mode(front_facing, dbg);
	let uv = animated_uv(i.uv);
	let samp_tex = textureSample(tex, base_samp, uv);
	let main_rgb = apply_main_gradation(apply_main_hsvg(samp_tex.rgb));
	let main_layers = apply_lil_main_layers(vec4<f32>(main_rgb * drawu.base_color.rgb, samp_tex.a * drawu.base_color.a), uv, i.uv1, i.uv2, i.uv3, uv, i.wp, 1.0, false, front_facing, false);
	let main_col = main_layers.col;
	let a = apply_lil_alpha_mask(main_col.a, uv);
	let alpha_kind = drawu.params.y;
	let cutoff = drawu.params.z;
	mask_discard_lit_unlit(main_col.rgb, a, alpha_kind, cutoff);
	discard_invisible_transparent_zwrite(a, alpha_kind, drawu.outline_params.w);
	let out_a = fragment_out_alpha(alpha_kind, a, drawu.base_color.a);
	let base = select(main_col.rgb, drawu.base_color.rgb, (dbg & DBG_SOLID_PRIM_COLOR) != 0u);
	let l = normalize(frame.light_dir.xyz);
	let normal_scale = select(drawu.shade_color.w, 0.0, (dbg & DBG_DISABLE_NORMAL_MAP) != 0u);
	let n = face_normal(normal_mapped(i.wn, i.wt, i.uv, i.uv1, i.uv2, i.uv3, normal_scale), front_facing, dbg);
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
	let main_layers = apply_lil_main_layers(vec4<f32>(main_rgb * drawu.base_color.rgb, samp_tex.a * drawu.base_color.a), uv, i.uv1, i.uv2, i.uv3, uv, i.wp, 1.0, false, front_facing, false);
	let main_col = main_layers.col;
	let a = apply_lil_alpha_mask(main_col.a, uv);
	let alpha_kind = drawu.params.y;
	let cutoff = drawu.params.z;
	mask_discard_lit_unlit(main_col.rgb, a, alpha_kind, cutoff);
	discard_invisible_transparent_zwrite(a, alpha_kind, drawu.outline_params.w);
	let out_a = fragment_out_alpha(alpha_kind, a, drawu.base_color.a);
	let base = select(main_col.rgb, drawu.base_color.rgb, (dbg & DBG_SOLID_PRIM_COLOR) != 0u);
	return vec4<f32>(base, out_a);
}

fn toon_fragment(i: VsOut, front_facing: bool, use_transparent_prepass: bool, fur_layer: f32, fur_alpha_in: f32, fur_card_side: f32, fur_cutout_pre: bool, fur_uv0: vec2<f32>) -> vec4<f32> {
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
	if is_liltoon && lil_udim_discard(i) {
		discard;
	}
	let v = normalize(frame.camera_pos.xyz - i.wp);
	let geometry_n_faced_pre = face_normal(normalize(i.wn), front_facing, dbg);
	let uv = lil_apply_parallax(animated_uv(i.uv), geometry_n_faced_pre, i.wt, v, is_liltoon);
	let layer_uv_mat = toon_matcap_uv(geometry_n_faced_pre, v, drawu.matcap_uv_params.x, drawu.matcap_uv_params.y);
	let layer_nv = clamp(dot(geometry_n_faced_pre, v), 0.0, 1.0);
	let samp_tex = textureSample(tex, base_samp, uv);
	let main_rgb = apply_main_gradation(apply_main_hsvg(samp_tex.rgb));
	let main_layers = apply_lil_main_layers(vec4<f32>(main_rgb * drawu.base_color.rgb, samp_tex.a * drawu.base_color.a), uv, i.uv1, i.uv2, i.uv3, layer_uv_mat, i.wp, layer_nv, i.wt.w > 0.0, front_facing, is_liltoon);
	let main_col = main_layers.col;
	var a = apply_lil_alpha_mask(main_col.a, uv);
	if is_liltoon {
		a = a * i.id_mask.x;
	}
	let dissolve_result = lil_apply_dissolve(a, uv, i.wp, i.id_mask.y, i.id_mask.z);
	if is_liltoon {
		a = dissolve_result.x;
	}
	let fur_alpha = fur_layer_alpha(uv, fur_uv0, fur_layer, fur_alpha_in, fur_card_side, fur_cutout_pre);
	if (fur_layer > 0.0 && fur_alpha <= 0.015) {
		discard;
	}
	let alpha_kind = drawu.params.y;
	let cutoff = drawu.params.z;
	let is_fur_pass = fur_layer > 0.0;
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
	let compute_fur = fur_layer > 0.0 && fur_alpha_in > 1.0;
	if (fur_layer > 0.0) {
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
	let base = select(main_col.rgb, drawu.base_color.rgb, (dbg & DBG_SOLID_PRIM_COLOR) != 0u) * fur_layer_ao(fur_layer, fur_uv0, fur_cutout_pre);
	if ((dbg & DBG_BASE_TEXTURE_ONLY) != 0u) {
		// 診断用: shading / GI / matcap / rim / emissive / shade_term を全てスキップして base のみ。
		// リングがまだ残るならテクスチャ自身（モデル制作者が描いた肌グラデ）かメッシュ重なり由来。
		return vec4<f32>(premultiply_when_blending(max(base, vec3<f32>(0.0, 0.0, 0.0)), out_a, alpha_kind, !compute_fur && !fur_cutout_pre && !is_liltoon_additive_blend), out_a);
	}
	let normal_scale = select(drawu.shade_color.w, 0.0, (dbg & DBG_DISABLE_NORMAL_MAP) != 0u || is_fur_pass);
	let geometry_n_faced = face_normal(normalize(i.wn), front_facing, dbg);
	let n_faced = face_normal(normal_mapped(i.wn, i.wt, uv, i.uv1, i.uv2, i.uv3, normal_scale), front_facing, dbg);
	let l = normalize(frame.light_dir.xyz);
	let gem_backface_normal = is_liltoon_gem && !front_facing;
	let lil_geometry_n_faced = lil_flip_backface_normal(geometry_n_faced, front_facing, select(0.0, drawu.material_ext_params.x, is_liltoon));
	let lil_n_faced = lil_flip_backface_normal(n_faced, front_facing, select(0.0, drawu.material_ext_params.x, is_liltoon));
	let geometry_n = select(lil_geometry_n_faced, normalize(lil_geometry_n_faced - v * 0.2), gem_backface_normal);
	let n = select(lil_n_faced, normalize(lil_n_faced - v * 0.2), gem_backface_normal);
	let raw_light_color = frame.light_color.rgb * frame.light_color.w;
	let lil_light_color = liltoon_light_color();
	let effect_light_color = select(raw_light_color, lil_light_color, is_liltoon);
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
		let shadow_border_mask = lil_shadow_border_ao_mask(textureSample(shadow_border_mask_tex, shadow_border_mask_samp, shadow_border_mask_uv).rgb);
		let shadow_blur_mask = textureSample(shadow_blur_mask_tex, shadow_blur_mask_samp, shadow_blur_mask_uv).rgb;
		let shadow_post_ao = drawu.shadow_ao_params.x > 0.5;
		let lil_shadow_value = lil_shadow_apply_pre_ao(dot(shadow_n, l) * 0.5 + 0.5, shadow_border_mask.r, shadow_post_ao);
		let shadow_strength_mask = textureSample(shading_shift_tex, shading_shift_samp, shadow_strength_mask_uv);
		let lil_shadow_raw = lil_tooning_scale_range(
			lil_shadow_value,
			clamp(drawu.shadow_params.z, 0.0, 1.0),
			clamp(drawu.shadow_params.w * shadow_blur_mask.r, 0.0, 1.0),
			clamp(drawu.shadow_ext_params.x, 0.0, 1.0),
		);
		let lil_shadow = lil_shadow_apply_post_ao(lil_shadow_raw, shadow_border_mask.r, shadow_post_ao);
		shading = mix(1.0, lil_shadow, clamp(drawu.shadow_params.y * shadow_strength_mask.r, 0.0, 1.0));
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
		let light_color = select(lil_direct_light_color(), lil_light_color, is_liltoon);
		let direct_col = base * light_color;
		var indirect_col = shade_term * light_color;
		let shadow_border_mask_uv = uv * drawu.shadow_border_mask_uv_offset_scale.zw + drawu.shadow_border_mask_uv_offset_scale.xy;
		let shadow_blur_mask_uv = uv * drawu.shadow_blur_mask_uv_offset_scale.zw + drawu.shadow_blur_mask_uv_offset_scale.xy;
		let shadow_border_mask = lil_shadow_border_ao_mask(textureSample(shadow_border_mask_tex, shadow_border_mask_samp, shadow_border_mask_uv).rgb);
		let shadow_blur_mask = textureSample(shadow_blur_mask_tex, shadow_blur_mask_samp, shadow_blur_mask_uv).rgb;
		let shadow_post_ao = drawu.shadow_ao_params.x > 0.5;
		let shadow2_value = lil_shadow_apply_pre_ao(dot(shadow2_n, l) * 0.5 + 0.5, shadow_border_mask.g, shadow_post_ao);
		let shadow2_color_texel = textureSample(shadow2_color_tex, shade_samp, shade_uv);
		let shadow2_color = mix(base, shadow2_color_texel.rgb, clamp(shadow2_color_texel.a, 0.0, 1.0)) * drawu.shadow2_color.rgb;
		let shadow2_raw = lil_tooning_scale_range(
			shadow2_value,
			clamp(drawu.shadow2_params.x, 0.0, 1.0),
			clamp(drawu.shadow2_params.y * shadow_blur_mask.g, 0.0, 1.0),
			clamp(drawu.shadow_ext_params.x, 0.0, 1.0),
		);
		let shadow2 = lil_shadow_apply_post_ao(shadow2_raw, shadow_border_mask.g, shadow_post_ao);
		let shadow2_strength = clamp((1.0 - shadow2) * drawu.shadow2_color.a, 0.0, 1.0);
		indirect_col = mix(indirect_col, shadow2_color * light_color, shadow2_strength);
		let shadow3_value = lil_shadow_apply_pre_ao(dot(shadow3_n, l) * 0.5 + 0.5, shadow_border_mask.b, shadow_post_ao);
		let shadow3_color_texel = textureSample(shadow3_color_tex, shade_samp, shade_uv);
		let shadow3_color = mix(base, shadow3_color_texel.rgb, clamp(shadow3_color_texel.a, 0.0, 1.0)) * drawu.shadow3_color.rgb;
		let shadow3_raw = lil_tooning_scale_range(
			shadow3_value,
			clamp(drawu.shadow3_params.x, 0.0, 1.0),
			clamp(drawu.shadow3_params.y * shadow_blur_mask.b, 0.0, 1.0),
			clamp(drawu.shadow_ext_params.x, 0.0, 1.0),
		);
		let shadow3 = lil_shadow_apply_post_ao(shadow3_raw, shadow_border_mask.b, shadow_post_ao);
		let shadow3_strength = clamp((1.0 - shadow3) * drawu.shadow3_color.a, 0.0, 1.0);
		indirect_col = mix(indirect_col, shadow3_color * light_color, shadow3_strength);
		indirect_col = mix(indirect_col, indirect_col * base, clamp(drawu.shadow_ext_params.y, 0.0, 1.0));
		indirect_col = mix(
			indirect_col,
			base,
			clamp(max(max(frame.ambient_color.r, frame.ambient_color.g), frame.ambient_color.b) * frame.ambient_color.w * drawu.shadow_ext_params.z, 0.0, 1.0),
		);
		indirect_col = min(indirect_col, direct_col);
		let border_mix_raw = lil_tooning_scale_range(
			lil_shadow_apply_pre_ao(dot(shadow_n, l) * 0.5 + 0.5, shadow_border_mask.r, shadow_post_ao),
			clamp(drawu.shadow_params.z, 0.0, 1.0),
			clamp(drawu.shadow_params.w * shadow_blur_mask.r, 0.0, 1.0),
			clamp(drawu.shadow_ext_params.x, 0.0, 1.0),
		);
		let border_mix = lil_shadow_apply_post_ao(border_mix_raw, shadow_border_mask.r, shadow_post_ao);
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
	if (is_liltoon && !is_liltoon_gem) {
		if drawu.main2nd_params.x > 0.5 {
			lit = lil_blend_color(lit, main_layers.second_unlit.rgb, main_layers.second_unlit.a, drawu.main2nd_params.w);
		}
		if drawu.main3rd_params.x > 0.5 {
			lit = lil_blend_color(lit, main_layers.third_unlit.rgb, main_layers.third_unlit.a, drawu.main3rd_params.w);
		}
	}
	if (is_liltoon_gem) {
		lit = base * clamp(abs(dot(n, v)), 0.0, 1.0) * 0.75;
	}
	lit = select(lit, lil_apply_rim_shade(lit, geometry_n, n, v, uv), is_liltoon && !is_liltoon_gem && !is_fur_pass);

	let disable_matcap = (dbg & DBG_DISABLE_MATCAP) != 0u;
	let disable_rim = (dbg & DBG_DISABLE_RIM) != 0u;
	if (is_fur_pass) {
		lit = select(lit, lil_apply_rim_shade(lit, geometry_n, n, v, uv), is_liltoon);
		if (!disable_rim) {
			let fur_rim_raw = pow(clamp(1.0 - abs(dot(normalize(n), v)), 0.0, 1.0), max(drawu.fur_rim_params.x, 0.00001));
			let inv_lighting = clamp(vec3<f32>(1.0) / max(lil_direct_light_color() + frame.ambient_color.rgb * frame.ambient_color.w, vec3<f32>(0.25)), vec3<f32>(1.0), vec3<f32>(4.0));
			let fur_rim_anti_light = mix(1.0, dot(inv_lighting, vec3<f32>(1.0 / 3.0)), clamp(drawu.fur_rim_params.y, 0.0, 1.0));
			lit = lit + clamp(fur_layer, 0.0, 1.0) * fur_rim_raw * fur_rim_anti_light * drawu.fur_rim_color.rgb * lil_direct_light_color();
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
		specular = specular_reflect * effect_light_color;
		specular_blend = specular_reflect;
		let reflection_lighting = mix(
			vec3<f32>(1.0, 1.0, 1.0),
			effect_light_color,
			clamp(drawu.reflection_ext_params.x, 0.0, 1.0),
		);
		let cube_tint = mix(vec3<f32>(1.0, 1.0, 1.0), drawu.reflection_cube_color.rgb, clamp(drawu.reflection_cube_color.a, 0.0, 1.0));
		let reflection_lod = lil_reflection_mip(perceptual_roughness);
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
			effect_light_color,
			clamp(drawu.reflection_ext_params.x, 0.0, 1.0),
		);
		let gem_reflection_dir = normalize(reflect(-v, n));
		let gem_env_lod = lil_reflection_mip(perceptual_roughness);
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
	let lil_effect_shadowmix = select(shading, clamp(dot(n, l), 0.0, 1.0), is_liltoon_gem);
	if (!disable_rim) {
		let rim_uv = uv * drawu.rim_uv_offset_scale.zw + drawu.rim_uv_offset_scale.xy;
		if (is_liltoon && drawu.rim_control.x > 0.0) {
			let rim_tex_color = textureSample(rim_tex, rim_samp, rim_uv);
			var rim_color = drawu.rim_color.rgb * rim_tex_color.rgb;
			rim_color = mix(rim_color, rim_color * base, clamp(drawu.rim_control.y, 0.0, 1.0));
			let rim_n = normalize(mix(geometry_n, n, clamp(drawu.rim_ext_params.y, 0.0, 1.0)));
			let rim_raw = pow(clamp(1.0 - abs(dot(rim_n, v)), 0.0, 1.0), max(drawu.rim_params.z, 0.00001));
			let ln_raw = clamp(dot(rim_n, l) * 0.5 + 0.5, 0.0, 1.0);
			let rim_dir_strength = clamp(drawu.rim_indirect_params.x, 0.0, 1.0);
			let dir_range = clamp(drawu.rim_indirect_params.y, -1.0, 1.0);
			let ln_dir = clamp((ln_raw + dir_range) / max(1.0 + dir_range, 0.00001), 0.0, 1.0);
			let rim_dir_raw = mix(rim_raw, rim_raw * ln_dir, rim_dir_strength);
			let rim_factor = lil_tooning_scale(rim_dir_raw, clamp(drawu.rim_params.x, 0.0, 1.0), clamp(drawu.rim_params.y, 0.0, 1.0));
			let lit_rim_color = mix(rim_color, rim_color * effect_light_color, clamp(drawu.rim_control.z, 0.0, 1.0));
			let rim_alpha = clamp(drawu.rim_control.x * rim_tex_color.a, 0.0, 1.0);
			let rim_shadow = mix(1.0, lil_effect_shadowmix, clamp(drawu.rim_ext_params.x, 0.0, 1.0));
			let rim_backface = lil_backface_visibility(drawu.rim_ext_params.z, front_facing);
			let rim_transparency = mix(1.0, a, select(clamp(drawu.transparency_params.z, 0.0, 1.0), 0.0, is_liltoon_refraction));
			let rim_direct_blend = clamp(rim_factor * rim_alpha * rim_shadow * rim_backface * rim_transparency, 0.0, 1.0);
			rim = lit_rim_color * rim_direct_blend;
			rim_blend = max(rim_blend, rim_direct_blend);
			let indir_range = clamp(drawu.rim_indirect_params.z, -1.0, 1.0);
			let ln_indir = clamp((1.0 - ln_raw + indir_range) / max(1.0 + indir_range, 0.00001), 0.0, 1.0);
			let indir_raw = rim_raw * ln_indir * rim_dir_strength;
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
		let reflection_light_tint = reflection_tint * effect_light_color;
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
				var matcap_base_n = normalize(mix(geometry_n, n, clamp(drawu.matcap_ext_params.x, 0.0, 1.0)));
				if (drawu.matcap_bump_params.x > 0.5) {
					matcap_base_n = liltoon_custom_matcap_normal(i.wn, i.wt, uv, drawu.matcap_bump_uv_offset_scale, drawu.matcap_bump_params.y, 0.0, front_facing, dbg, drawu.material_ext_params.x, gem_backface_normal, v);
				}
				let matcap_n = normalize(mix(matcap_base_n, anisotropy_n, clamp(drawu.anisotropy_params.w * anisotropy_basis.enabled, 0.0, 1.0)));
				let matcap_uv = toon_matcap_uv(matcap_n, v, drawu.matcap_uv_params.x, drawu.matcap_uv_params.y);
				let matcap_tex_color = textureSampleLevel(matcap_tex, matcap_samp, matcap_uv, max(drawu.matcap_ext_params.z, 0.0));
				let matcap_raw = drawu.matcap_factor.rgb * matcap_tex_color.rgb;
				let lit_matcap = mix(matcap_raw, matcap_raw * effect_light_color, clamp(drawu.matcap_params.z, 0.0, 1.0));
				let albedo_matcap = mix(lit_matcap, lit_matcap * base, clamp(drawu.matcap_params.y, 0.0, 1.0));
				let matcap_blend_mask_uv = uv * drawu.matcap_blend_mask_uv_offset_scale.zw + drawu.matcap_blend_mask_uv_offset_scale.xy;
				let matcap_blend_mask = textureSample(matcap_blend_mask_tex, matcap_blend_mask_samp, matcap_blend_mask_uv).rgb;
				let matcap_shadow = mix(1.0, lil_effect_shadowmix, clamp(drawu.matcap_ext_params.y, 0.0, 1.0));
				let matcap_backface = lil_backface_visibility(drawu.matcap_ext_params.w, front_facing);
				let matcap_transparency = mix(1.0, a, select(clamp(drawu.transparency_params.x, 0.0, 1.0), 0.0, is_liltoon_refraction));
				let matcap_blend = clamp(drawu.matcap_params.x * matcap_tex_color.a * matcap_blend_mask * drawu.matcap_factor.w * matcap_shadow * matcap_backface * matcap_transparency, vec3<f32>(0.0), vec3<f32>(1.0));
				lit = lil_blend_color3(lit, albedo_matcap, matcap_blend, drawu.matcap_params.w);
			}
			if (drawu.matcap2_params.x > 0.0) {
				var matcap2_base_n = normalize(mix(geometry_n, n, clamp(drawu.matcap2_ext_params.x, 0.0, 1.0)));
				if (drawu.matcap2_bump_params.x > 0.5) {
					matcap2_base_n = liltoon_custom_matcap_normal(i.wn, i.wt, uv, drawu.matcap2_bump_uv_offset_scale, drawu.matcap2_bump_params.y, 1.0, front_facing, dbg, drawu.material_ext_params.x, gem_backface_normal, v);
				}
				let matcap2_n = normalize(mix(matcap2_base_n, anisotropy_n, clamp(drawu.anisotropy_ext_params.x * anisotropy_basis.enabled, 0.0, 1.0)));
				let matcap2_uv = toon_matcap_uv(matcap2_n, v, drawu.matcap_uv_params.z, drawu.matcap_uv_params.w);
				let matcap2_tex_color = textureSampleLevel(matcap2_tex, matcap_samp, matcap2_uv, max(drawu.matcap2_ext_params.z, 0.0));
				let matcap2_lighting = mix(vec3<f32>(1.0, 1.0, 1.0), effect_light_color, clamp(drawu.matcap2_params.z, 0.0, 1.0));
				let matcap2_raw = drawu.matcap2_factor.rgb * matcap2_tex_color.rgb * matcap2_lighting;
				let matcap2_albedo = mix(matcap2_raw, matcap2_raw * base, clamp(drawu.matcap2_params.y, 0.0, 1.0));
				let matcap2_blend_mask_uv = uv * drawu.matcap2_blend_mask_uv_offset_scale.zw + drawu.matcap2_blend_mask_uv_offset_scale.xy;
				let matcap2_blend_mask = textureSample(matcap2_blend_mask_tex, matcap_blend_mask_samp, matcap2_blend_mask_uv).rgb;
				let matcap2_shadow = mix(1.0, lil_effect_shadowmix, clamp(drawu.matcap2_ext_params.y, 0.0, 1.0));
				let matcap2_backface = lil_backface_visibility(drawu.matcap2_ext_params.w, front_facing);
				let matcap2_transparency = mix(1.0, a, select(clamp(drawu.transparency_params.y, 0.0, 1.0), 0.0, is_liltoon_refraction));
				let matcap2_blend = clamp(drawu.matcap2_params.x * drawu.matcap2_factor.a * matcap2_tex_color.a * matcap2_blend_mask * matcap2_shadow * matcap2_backface * matcap2_transparency, vec3<f32>(0.0), vec3<f32>(1.0));
				lit = lil_blend_color3(lit, matcap2_albedo, matcap2_blend, drawu.matcap2_params.w);
			}
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
			let backlight_color_uv = uv * drawu.backlight_color_uv_offset_scale.zw + drawu.backlight_color_uv_offset_scale.xy;
			let backlight_color_sample = textureSample(backlight_color_tex, base_samp, backlight_color_uv);
			let authored_backlight_color = drawu.backlight_color * backlight_color_sample;
			let backlight = clamp(backlight_factor * backlight_ln, 0.0, 1.0) * backlight_backface * clamp(authored_backlight_color.a, 0.0, 1.0);
			let backlight_color = mix(authored_backlight_color.rgb, authored_backlight_color.rgb * base, clamp(drawu.backlight_params.y, 0.0, 1.0));
			lit = lit + backlight * backlight_color * effect_light_color;
		}
		lit = lil_blend_weighted_color(lit, rim, rim_blend, drawu.rim_control.w);
	} else {
		if (!disable_matcap) {
			var matcap_base_n = normalize(mix(geometry_n, n, clamp(drawu.matcap_ext_params.x, 0.0, 1.0)));
			if (drawu.matcap_bump_params.x > 0.5) {
				matcap_base_n = liltoon_custom_matcap_normal(i.wn, i.wt, uv, drawu.matcap_bump_uv_offset_scale, drawu.matcap_bump_params.y, 0.0, front_facing, dbg, drawu.material_ext_params.x, gem_backface_normal, v);
			}
			let matcap_n = normalize(mix(matcap_base_n, anisotropy_n, clamp(drawu.anisotropy_params.w * anisotropy_basis.enabled, 0.0, 1.0)));
			let matcap_uv = toon_matcap_uv(matcap_n, v, drawu.matcap_uv_params.x, drawu.matcap_uv_params.y);
			let matcap_tex_color = textureSampleLevel(matcap_tex, matcap_samp, matcap_uv, max(drawu.matcap_ext_params.z, 0.0));
			let matcap_raw = drawu.matcap_factor.rgb * matcap_tex_color.rgb;
			if (drawu.matcap_params.x > 0.0) {
				let lit_matcap = mix(matcap_raw, matcap_raw * effect_light_color, clamp(drawu.matcap_params.z, 0.0, 1.0));
				let albedo_matcap = mix(lit_matcap, lit_matcap * base, clamp(drawu.matcap_params.y, 0.0, 1.0));
				let matcap_blend_mask_uv = uv * drawu.matcap_blend_mask_uv_offset_scale.zw + drawu.matcap_blend_mask_uv_offset_scale.xy;
				let matcap_blend_mask = textureSample(matcap_blend_mask_tex, matcap_blend_mask_samp, matcap_blend_mask_uv).rgb;
				let matcap_shadow = mix(1.0, shading, clamp(drawu.matcap_ext_params.y, 0.0, 1.0));
				let matcap_backface = lil_backface_visibility(drawu.matcap_ext_params.w, front_facing);
				let matcap_transparency = mix(1.0, a, clamp(drawu.transparency_params.x, 0.0, 1.0));
				let matcap_blend = clamp(drawu.matcap_params.x * matcap_tex_color.a * matcap_blend_mask * drawu.matcap_factor.w * matcap_shadow * matcap_backface * matcap_transparency, vec3<f32>(0.0), vec3<f32>(1.0));
				lit = lil_blend_color3(lit, albedo_matcap, matcap_blend, drawu.matcap_params.w);
			} else {
				lit = lit + matcap_raw * drawu.matcap_factor.w;
			}
			if (drawu.matcap2_params.x > 0.0) {
				var matcap2_base_n = normalize(mix(geometry_n, n, clamp(drawu.matcap2_ext_params.x, 0.0, 1.0)));
				if (drawu.matcap2_bump_params.x > 0.5) {
					matcap2_base_n = liltoon_custom_matcap_normal(i.wn, i.wt, uv, drawu.matcap2_bump_uv_offset_scale, drawu.matcap2_bump_params.y, 1.0, front_facing, dbg, drawu.material_ext_params.x, gem_backface_normal, v);
				}
				let matcap2_n = normalize(mix(matcap2_base_n, anisotropy_n, clamp(drawu.anisotropy_ext_params.x * anisotropy_basis.enabled, 0.0, 1.0)));
				let matcap2_uv = toon_matcap_uv(matcap2_n, v, drawu.matcap_uv_params.z, drawu.matcap_uv_params.w);
				let matcap2_tex_color = textureSampleLevel(matcap2_tex, matcap_samp, matcap2_uv, max(drawu.matcap2_ext_params.z, 0.0));
				let matcap2_lighting = mix(vec3<f32>(1.0, 1.0, 1.0), effect_light_color, clamp(drawu.matcap2_params.z, 0.0, 1.0));
				let matcap2_raw = drawu.matcap2_factor.rgb * matcap2_tex_color.rgb * matcap2_lighting;
				let matcap2_albedo = mix(matcap2_raw, matcap2_raw * base, clamp(drawu.matcap2_params.y, 0.0, 1.0));
				let matcap2_blend_mask_uv = uv * drawu.matcap2_blend_mask_uv_offset_scale.zw + drawu.matcap2_blend_mask_uv_offset_scale.xy;
				let matcap2_blend_mask = textureSample(matcap2_blend_mask_tex, matcap_blend_mask_samp, matcap2_blend_mask_uv).rgb;
				let matcap2_shadow = mix(1.0, shading, clamp(drawu.matcap2_ext_params.y, 0.0, 1.0));
				let matcap2_backface = lil_backface_visibility(drawu.matcap2_ext_params.w, front_facing);
				let matcap2_transparency = mix(1.0, a, clamp(drawu.transparency_params.y, 0.0, 1.0));
				let matcap2_blend = clamp(drawu.matcap2_params.x * drawu.matcap2_factor.a * matcap2_tex_color.a * matcap2_blend_mask * matcap2_shadow * matcap2_backface * matcap2_transparency, vec3<f32>(0.0), vec3<f32>(1.0));
				lit = lil_blend_color3(lit, matcap2_albedo, matcap2_blend, drawu.matcap2_params.w);
			}
		}
		lit = lit + specular + authored_reflection;
		lit = lit + rim;
	}
	if (fur_layer > 0.0 && !disable_rim) {
		let fur_rim_raw = pow(clamp(1.0 - abs(dot(normalize(n), v)), 0.0, 1.0), max(drawu.fur_rim_params.x, 0.00001));
		let inv_lighting = clamp(vec3<f32>(1.0) / max(lil_direct_light_color() + frame.ambient_color.rgb * frame.ambient_color.w, vec3<f32>(0.25)), vec3<f32>(1.0), vec3<f32>(4.0));
		let fur_rim_anti_light = mix(1.0, dot(inv_lighting, vec3<f32>(1.0 / 3.0)), clamp(drawu.fur_rim_params.y, 0.0, 1.0));
		lit = lit + clamp(fur_layer, 0.0, 1.0) * fur_rim_raw * fur_rim_anti_light * drawu.fur_rim_color.rgb * lil_direct_light_color();
	}

	if (is_liltoon && drawu.glitter_control.x > 0.5) {
		let glitter_n = normalize(mix(geometry_n, n, clamp(drawu.glitter_control.z, 0.0, 1.0)));
		let glitter_uv = lil_select_uv(drawu.glitter_ext2.z, uv, i.uv1, uv, uv);
		let glitter_color_uv_raw = lil_select_uv(drawu.glitter_ext2.w, uv, i.uv1, i.uv2, i.uv3);
		let glitter_color_uv = glitter_color_uv_raw * drawu.glitter_color_uv_offset_scale.zw + drawu.glitter_color_uv_offset_scale.xy;
		let glitter_camera_front = normalize_or(frame.camera_pos.xyz, v);
		let glitter_view = normalize_or(mix(glitter_camera_front, v, clamp(drawu.glitter_ext3.x, 0.0, 1.0)), v);
		let glitter_camera = normalize_or(mix(glitter_camera_front, v, clamp(drawu.glitter_ext3.x, 0.0, 1.0)), v);
		let glitter_proc = lil_calc_glitter(glitter_uv, glitter_n, glitter_view, glitter_camera, l);
		let glitter_color_texel = textureSample(glitter_color_tex, base_samp, glitter_color_uv);
		var glitter_color = drawu.glitter_color.rgb * glitter_color_texel.rgb * glitter_proc;
		glitter_color = mix(glitter_color, glitter_color * base, clamp(drawu.glitter_control.y, 0.0, 1.0));
		var glitter_alpha = clamp(drawu.glitter_color.a * glitter_color_texel.a, 0.0, 1.0);
		glitter_alpha = mix(glitter_alpha, glitter_alpha * out_a, clamp(drawu.glitter_ext.w, 0.0, 1.0));
		glitter_alpha = glitter_alpha * lil_backface_visibility(drawu.glitter_ext2.x, front_facing);
		glitter_alpha = mix(glitter_alpha, glitter_alpha * lil_effect_shadowmix, clamp(drawu.glitter_ext.z, 0.0, 1.0));
		let glitter_lit = mix(glitter_color, glitter_color * effect_light_color, clamp(drawu.glitter_ext.y, 0.0, 1.0));
		lit = lit + glitter_lit * glitter_alpha;
	}

	let disable_emissive = (dbg & DBG_DISABLE_EMISSIVE) != 0u;
	let uv_rim = vec2<f32>(abs(dot(n, v)));
	let emission_uv_base = lil_select_emission_uv(drawu.emission_uv_anim_params.w, uv, i.uv1, i.uv2, i.uv3, uv_rim);
	let emission_uv = lil_calc_uv_scroll_rotate(emission_uv_base, drawu.emission_uv_offset_scale, drawu.emission_uv_anim_params) + parallax_offset * drawu.emission_grad_params.w;
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
				let emission2nd_uv_base = lil_select_emission_uv(drawu.emission2nd_uv_anim_params.w, uv, i.uv1, i.uv2, i.uv3, uv_rim);
				let emission2nd_uv = lil_calc_uv_scroll_rotate(emission2nd_uv_base, drawu.emission2nd_uv_offset_scale, drawu.emission2nd_uv_anim_params) + parallax_offset * drawu.emission2nd_ext_params.x;
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
	if is_liltoon {
		lit = lit + drawu.dissolve_color.rgb * dissolve_result.y + main_layers.dissolve_emission;
	}
	let distance_faded = select(vec4<f32>(lit, out_a), lil_apply_distance_fade(lit, out_a, i.wp, n, v, front_facing), is_liltoon);
	let final_a = clamp(distance_faded.a, 0.0, 1.0);
	return vec4<f32>(
		premultiply_when_blending(max(distance_faded.rgb, vec3<f32>(0.0, 0.0, 0.0)), final_a, alpha_kind, !compute_fur && !fur_cutout_pre && !is_liltoon_additive_blend),
		final_a,
	);
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
	base.uv1 = i.uv1;
	base.uv2 = i.uv2;
	base.uv3 = i.uv3;
	base.id_mask = i.id_mask;
	base.wp = i.wp;
	base.wt = i.wt;
	return toon_fragment(base, front_facing, false, i.fur_layer, i.fur_alpha, i.fur_card_side, false, i.fur_uv0);
}

@fragment
fn fs_fur_toon_pre(i: FurVsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
	var base: VsOut;
	base.clip = i.clip;
	base.wn = i.wn;
	base.uv = i.uv;
	base.uv1 = i.uv1;
	base.uv2 = i.uv2;
	base.uv3 = i.uv3;
	base.id_mask = i.id_mask;
	base.wp = i.wp;
	base.wt = i.wt;
	return toon_fragment(base, front_facing, false, i.fur_layer, i.fur_alpha, i.fur_card_side, true, i.fur_uv0);
}

@fragment
fn fs_outline(i: VsOut) -> @location(0) vec4<f32> {
	let dbg = bitcast<u32>(drawu.params.w);
	let is_liltoon = (dbg & SRC_LILTOON) != 0u;
	if is_liltoon && (i.id_mask.x <= 0.0 || lil_udim_discard(i)) {
		discard;
	}
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
