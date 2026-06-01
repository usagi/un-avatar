// メッシュ描画: 頂点は共通、フラグメントはシェーディング種別ごとに別エントリ＋別パイプライン（ランタイムのシェーディング分岐を避ける）。
//
// - fs_lit: LitLambert
// - fs_unlit: Unlit（ベース色のみ）
// - fs_toon: avatar toon shading. v2 `.unavatar` materials enter through the
//   lilToon-like parameter branch; VRM/MToon remains a legacy input path.

struct Frame {
	view_proj: mat4x4<f32>,
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
	reflection_color: vec4<f32>,
	reflection_control: vec4<f32>,
	reflection_params: vec4<f32>,
	reflection_ext_params: vec4<f32>,
	reflection_cube_color: vec4<f32>,
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
	emission_color: vec4<f32>,
	emission_params: vec4<f32>,
	outline_color: vec4<f32>,
	outline_params: vec4<f32>,
	outline_lit_color: vec4<f32>,
	outline_lit_params: vec4<f32>,
	alpha_mask_params: vec4<f32>,
	emissive_factor: vec4<f32>,
	uv_anim_params: vec4<f32>,
	uv_offset_scale: vec4<f32>,
}

struct MorphU {
	target_count: u32,
	vertex_count: u32,
	_pad: vec2<u32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
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
@group(1) @binding(13) var reflection_tex: texture_2d<f32>;
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
@group(2) @binding(0) var<storage, read> bones: array<mat4x4<f32>>;
@group(3) @binding(0) var<uniform> morphu: MorphU;
@group(3) @binding(1) var<storage, read> morph_weights: array<f32>;
@group(3) @binding(2) var<storage, read> morph_deltas: array<vec4<f32>>;

struct VsIn {
	@location(0) pos: vec3<f32>,
	@location(1) norm: vec3<f32>,
	@location(2) uv: vec2<f32>,
	@location(3) joints: vec4<u32>,
	@location(4) weights: vec4<f32>,
}

struct VsOut {
	@builtin(position) clip: vec4<f32>,
	@location(0) wn: vec3<f32>,
	@location(1) uv: vec2<f32>,
	@location(2) wp: vec3<f32>,
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
	let j0 = v.joints.x;
	let j1 = v.joints.y;
	let j2 = v.joints.z;
	let j3 = v.joints.w;
	let dbg = bitcast<u32>(drawu.params.w);
	if (dbg & DBG_BIND_POSE_RIGID) != 0u {
		let wp = drawt.model * vec4<f32>(pos, 1.0);
		let mn = mat3_upper(drawt.model) * norm;
		o.wn = normalize(mn);
		o.uv = v.uv;
		o.wp = wp.xyz;
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

	o.wn = wn;
	o.uv = v.uv;
	o.wp = wp.xyz;
	o.clip = frame.view_proj * wp;
	return o;
}

@vertex
fn vs_main(v: VsIn, @builtin(vertex_index) vertex_index: u32) -> VsOut {
	return skinned_position_normal(v, vertex_index);
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
	let width = select(drawu.outline_params.y * 0.03, drawu.outline_params.y, drawu.outline_params.x < 1.5) * mask;
	let wp = vec4<f32>(o.wp + n * width, 1.0);
	o.clip = frame.view_proj * wp;
	return o;
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

fn fragment_out_alpha(alpha_kind: f32, a: f32, base_color_a: f32) -> f32 {
	if alpha_kind > 1.5 {
		return a;
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
	let raw_mask = textureSample(alpha_mask_tex, alpha_mask_samp, uv).r;
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

fn premultiply_when_blending(rgb: vec3<f32>, out_a: f32, alpha_kind: f32) -> vec3<f32> {
	if alpha_kind > 1.5 {
		return rgb * out_a;
	}
	return rgb;
}

fn discard_invisible_transparent_zwrite(a: f32, alpha_kind: f32, transparent_zwrite: f32) {
	if alpha_kind > 1.5 && transparent_zwrite > 0.5 && a <= 0.001 {
		discard;
	}
}

fn discard_transparent_zprepass(a: f32, alpha_kind: f32, cutoff: f32, transparent_zwrite: f32) {
	// lilToon Transparent+ZWrite materials can author cutoff as 0.0.
	// Still avoid writing depth for fully transparent texels; the color pass
	// discards them and stale depth would incorrectly hide later surfaces.
	let z_cutoff = max(cutoff, 1.0 / 255.0);
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

fn face_normal(n: vec3<f32>, front_facing: bool, flags: u32) -> vec3<f32> {
	if front_facing || (flags & MAT_DOUBLE_SIDED) == 0u {
		return n;
	}
	return -n;
}

fn toon_matcap_uv(n: vec3<f32>, v: vec3<f32>) -> vec2<f32> {
	var world_view_x = normalize(vec3<f32>(v.z, 0.0, -v.x));
	if (length(world_view_x) < 0.0001) {
		world_view_x = vec3<f32>(1.0, 0.0, 0.0);
	}
	let world_view_y = cross(v, world_view_x);
	return vec2<f32>(dot(world_view_x, n), dot(world_view_y, n)) * 0.495 + vec2<f32>(0.5, 0.5);
}

fn toon_reflection_uv(n: vec3<f32>, v: vec3<f32>) -> vec2<f32> {
	let r = normalize(reflect(-v, n));
	let u = atan2(r.z, r.x) * 0.15915494309189535 + 0.5;
	let vv = acos(clamp(r.y, -1.0, 1.0)) * 0.3183098861837907;
	return vec2<f32>(u, vv);
}

fn linearstep(edge0: f32, edge1: f32, x: f32) -> f32 {
	return clamp((x - edge0) / max(edge1 - edge0, 0.00001), 0.0, 1.0);
}

fn lil_tooning_scale(value: f32, border: f32, blur: f32) -> f32 {
	if (blur <= 0.00001) {
		return select(0.0, 1.0, value >= border);
	}
	let border_min = clamp(border - blur * 0.5, 0.0, 1.0);
	let border_max = clamp(border + blur * 0.5, 0.0, 1.0);
	return linearstep(border_min, border_max, value);
}

fn lil_tooning_scale_range(value: f32, border: f32, blur: f32, border_range: f32) -> f32 {
	if (blur <= 0.00001 && border_range <= 0.00001) {
		return select(0.0, 1.0, value >= border);
	}
	let border_min = clamp(border - blur * 0.5 - border_range, 0.0, 1.0);
	let border_max = clamp(border + blur * 0.5, 0.0, 1.0);
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

fn normal_mapped(n_in: vec3<f32>, wp: vec3<f32>, uv: vec2<f32>, scale: f32) -> vec3<f32> {
	let n = normalize(n_in);
	if (abs(scale) < 0.000001) {
		return n;
	}
	let packed = textureSample(normal_tex, normal_samp, uv).xyz;
	var tn = packed * 2.0 - vec3<f32>(1.0, 1.0, 1.0);
	tn.x = tn.x * scale;
	tn.y = tn.y * scale;
	tn = normalize(tn);

	let dp1 = dpdx(wp);
	let dp2 = dpdy(wp);
	let duv1 = dpdx(uv);
	let duv2 = dpdy(uv);
	let det = duv1.x * duv2.y - duv1.y * duv2.x;
	if (abs(det) < 0.0000001) {
		return n;
	}
	let inv_det = 1.0 / det;
	var t = (dp1 * duv2.y - dp2 * duv1.y) * inv_det;
	var b = (-dp1 * duv2.x + dp2 * duv1.x) * inv_det;
	t = normalize(t - n * dot(n, t));
	b = normalize(b - n * dot(n, b) - t * dot(t, b));
	return normalize(t * tn.x + b * tn.y + n * tn.z);
}

fn authored_occlusion(uv: vec2<f32>, dbg: u32) -> f32 {
	if ((dbg & 1024u) == 0u) {
		return 1.0;
	}
	let strength = clamp(drawu.rim_color.w, 0.0, 2.0);
	let sample = textureSample(occlusion_tex, occlusion_samp, uv).r;
	return clamp(mix(1.0, sample, strength), 0.0, 1.0);
}

@fragment
fn fs_lit(i: VsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
	let dbg = bitcast<u32>(drawu.params.w);
	discard_by_cull_mode(front_facing, dbg);
	let uv = animated_uv(i.uv);
	let samp_tex = textureSample(tex, base_samp, uv);
	let alb = samp_tex.rgb;
	let tex_a = samp_tex.a;
	let a = apply_lil_alpha_mask(tex_a * drawu.base_color.a, uv);
	let alpha_kind = drawu.params.y;
	let cutoff = drawu.params.z;
	mask_discard_lit_unlit(alb, a, alpha_kind, cutoff);
	discard_invisible_transparent_zwrite(a, alpha_kind, drawu.outline_params.w);
	let out_a = fragment_out_alpha(alpha_kind, a, drawu.base_color.a);
	let base = select(alb * drawu.base_color.rgb, drawu.base_color.rgb, (dbg & DBG_SOLID_PRIM_COLOR) != 0u);
	let l = normalize(frame.light_dir.xyz);
	let normal_scale = select(drawu.shade_color.w, 0.0, (dbg & DBG_DISABLE_NORMAL_MAP) != 0u);
	let n = face_normal(normal_mapped(i.wn, i.wp, uv, normal_scale), front_facing, dbg);
	let ndl = max(dot(n, l), 0.0);
	let ambient = frame.ambient_color.rgb * (frame.ambient_color.w * 0.57);
	let direct = frame.light_color.rgb * (frame.light_color.w * 0.8 * ndl);
	let lit = base * (ambient + direct) * authored_occlusion(uv, dbg);
	return vec4<f32>(lit, out_a);
}

@fragment
fn fs_unlit(i: VsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
	let dbg = bitcast<u32>(drawu.params.w);
	discard_by_cull_mode(front_facing, dbg);
	let uv = animated_uv(i.uv);
	let samp_tex = textureSample(tex, base_samp, uv);
	let alb = samp_tex.rgb;
	let tex_a = samp_tex.a;
	let a = apply_lil_alpha_mask(tex_a * drawu.base_color.a, uv);
	let alpha_kind = drawu.params.y;
	let cutoff = drawu.params.z;
	mask_discard_lit_unlit(alb, a, alpha_kind, cutoff);
	discard_invisible_transparent_zwrite(a, alpha_kind, drawu.outline_params.w);
	let out_a = fragment_out_alpha(alpha_kind, a, drawu.base_color.a);
	let base = select(alb * drawu.base_color.rgb, drawu.base_color.rgb, (dbg & DBG_SOLID_PRIM_COLOR) != 0u);
	return vec4<f32>(base, out_a);
}

@fragment
fn fs_toon(i: VsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
	let dbg = bitcast<u32>(drawu.params.w);
	discard_by_cull_mode(front_facing, dbg);
	let uv = animated_uv(i.uv);
	let samp_tex = textureSample(tex, base_samp, uv);
	let alb = samp_tex.rgb;
	let tex_a = samp_tex.a;
	let a = apply_lil_alpha_mask(tex_a * drawu.base_color.a, uv);
	let alpha_kind = drawu.params.y;
	let cutoff = drawu.params.z;
	mask_discard_toon(alb, a, alpha_kind, cutoff);
	discard_invisible_transparent_zwrite(a, alpha_kind, drawu.outline_params.w);
	let out_a = fragment_out_alpha(alpha_kind, a, drawu.base_color.a);
	let base = select(alb * drawu.base_color.rgb, drawu.base_color.rgb, (dbg & DBG_SOLID_PRIM_COLOR) != 0u);
	if ((dbg & DBG_BASE_TEXTURE_ONLY) != 0u) {
		// 診断用: shading / GI / matcap / rim / emissive / shade_term を全てスキップして base のみ。
		// リングがまだ残るならテクスチャ自身（モデル制作者が描いた肌グラデ）かメッシュ重なり由来。
		return vec4<f32>(premultiply_when_blending(max(base, vec3<f32>(0.0, 0.0, 0.0)), out_a, alpha_kind), out_a);
	}
	let normal_scale = select(drawu.shade_color.w, 0.0, (dbg & DBG_DISABLE_NORMAL_MAP) != 0u);
	let geometry_n = face_normal(normalize(i.wn), front_facing, dbg);
	let n = face_normal(normal_mapped(i.wn, i.wp, uv, normal_scale), front_facing, dbg);
	let shadow_n = normalize(mix(geometry_n, n, clamp(drawu.shadow_ext_params.w, 0.0, 1.0)));
	let shadow2_n = normalize(mix(geometry_n, n, clamp(drawu.shadow2_params.z, 0.0, 1.0)));
	let shadow3_n = normalize(mix(geometry_n, n, clamp(drawu.shadow3_params.z, 0.0, 1.0)));
	let l = normalize(frame.light_dir.xyz);
	let v = normalize(frame.camera_pos.xyz - i.wp);

	let force_shift_zero = (dbg & DBG_FORCE_SHADING_SHIFT_ZERO) != 0u;
	var shading: f32;
	if (drawu.shadow_params.x > 0.5) {
		let shadow_border_mask = textureSample(shadow_border_mask_tex, shadow_border_mask_samp, uv).r;
		let shadow_blur_mask = textureSample(shadow_blur_mask_tex, shadow_blur_mask_samp, uv).r;
		let lil_shadow_value = (dot(shadow_n, l) * 0.5 + 0.5) * shadow_border_mask;
		let shadow_strength_mask = textureSample(shading_shift_tex, shading_shift_samp, uv).r;
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
	let shade_term_raw = drawu.shade_color.rgb * textureSample(shade_tex, shade_samp, uv).rgb;
	let shade_term = select(shade_term_raw, base, disable_shade_color);
	var lit: vec3<f32>;
	if (drawu.shadow_params.x > 0.5) {
		let light_color = frame.light_color.rgb * frame.light_color.w;
		let direct_col = base * light_color;
		var indirect_col = shade_term * light_color;
		let shadow2_value = dot(shadow2_n, l) * 0.5 + 0.5;
		let shadow2 = lil_tooning_scale_range(
			shadow2_value,
			clamp(drawu.shadow2_params.x, 0.0, 1.0),
			clamp(drawu.shadow2_params.y, 0.0, 1.0),
			clamp(drawu.shadow_ext_params.x, 0.0, 1.0),
		);
		let shadow2_strength = clamp((1.0 - shadow2) * drawu.shadow2_color.a, 0.0, 1.0);
		indirect_col = mix(indirect_col, drawu.shadow2_color.rgb * light_color, shadow2_strength);
		let shadow3_value = dot(shadow3_n, l) * 0.5 + 0.5;
		let shadow3 = lil_tooning_scale_range(
			shadow3_value,
			clamp(drawu.shadow3_params.x, 0.0, 1.0),
			clamp(drawu.shadow3_params.y, 0.0, 1.0),
			clamp(drawu.shadow_ext_params.x, 0.0, 1.0),
		);
		let shadow3_strength = clamp((1.0 - shadow3) * drawu.shadow3_color.a, 0.0, 1.0);
		indirect_col = mix(indirect_col, drawu.shadow3_color.rgb * light_color, shadow3_strength);
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

	let disable_matcap = (dbg & DBG_DISABLE_MATCAP) != 0u;
	let disable_rim = (dbg & DBG_DISABLE_RIM) != 0u;
	let matcap_n = normalize(mix(geometry_n, n, clamp(drawu.matcap_ext_params.x, 0.0, 1.0)));
	let matcap_uv = toon_matcap_uv(matcap_n, v);
	let matcap_tex_color = textureSampleLevel(matcap_tex, matcap_samp, matcap_uv, max(drawu.matcap_ext_params.z, 0.0));
	let matcap_raw = drawu.matcap_factor.rgb * matcap_tex_color.rgb;
	if (!disable_matcap) {
		if (drawu.shadow_params.x > 0.5) {
			let lit_matcap = mix(matcap_raw, matcap_raw * frame.light_color.rgb * frame.light_color.w, clamp(drawu.matcap_params.z, 0.0, 1.0));
			let albedo_matcap = mix(lit_matcap, lit_matcap * base, clamp(drawu.matcap_params.y, 0.0, 1.0));
			let matcap_blend_mask = textureSample(matcap_blend_mask_tex, matcap_blend_mask_samp, uv).r;
			let matcap_shadow = mix(1.0, shading, clamp(drawu.matcap_ext_params.y, 0.0, 1.0));
			let matcap_blend = clamp(drawu.matcap_params.x * matcap_tex_color.a * matcap_blend_mask * drawu.matcap_factor.w * matcap_shadow, 0.0, 1.0);
			lit = lil_blend_color(lit, albedo_matcap, matcap_blend, drawu.matcap_params.w);
		} else {
			lit = lit + matcap_raw * drawu.matcap_factor.w;
		}
		if (drawu.matcap2_params.x > 0.0) {
			let matcap2_n = normalize(mix(geometry_n, n, clamp(drawu.matcap2_ext_params.x, 0.0, 1.0)));
			let matcap2_uv = toon_matcap_uv(matcap2_n, v);
			let matcap2_tex_color = textureSampleLevel(matcap2_tex, matcap_samp, matcap2_uv, max(drawu.matcap2_ext_params.z, 0.0));
			let matcap2_lighting = mix(vec3<f32>(1.0, 1.0, 1.0), frame.light_color.rgb * frame.light_color.w, clamp(drawu.matcap2_params.z, 0.0, 1.0));
			let matcap2_raw = drawu.matcap2_factor.rgb * matcap2_tex_color.rgb * matcap2_lighting;
			let matcap2_albedo = mix(matcap2_raw, matcap2_raw * base, clamp(drawu.matcap2_params.y, 0.0, 1.0));
			let matcap2_blend_mask = textureSample(matcap2_blend_mask_tex, matcap_blend_mask_samp, uv).r;
			let matcap2_blend = clamp(drawu.matcap2_params.x * drawu.matcap2_factor.a * matcap2_tex_color.a * matcap2_blend_mask, 0.0, 1.0);
			lit = lil_blend_color(lit, matcap2_albedo, matcap2_blend, drawu.matcap2_params.w);
		}
	}
	var specular = vec3<f32>(0.0, 0.0, 0.0);
	var authored_reflection = vec3<f32>(0.0, 0.0, 0.0);
	let half_vec = normalize(l + v);
	let specular_n = normalize(mix(geometry_n, n, clamp(drawu.specular_toon_params.w, 0.0, 1.0)));
	let reflection_n = normalize(mix(geometry_n, n, clamp(drawu.reflection_params.w, 0.0, 1.0)));
	let reflection_uv = toon_reflection_uv(reflection_n, v);
	let reflection_fresnel = pow(clamp(1.0 - dot(reflection_n, v), 0.0, 1.0), 2.0);
	if (drawu.shadow_params.x > 0.5) {
		if (drawu.reflection_control.x > 0.5) {
			let reflection_color_texel = textureSample(reflection_color_tex, reflection_color_samp, uv);
			let smoothness = clamp(drawu.reflection_params.x * textureSample(smoothness_tex, smoothness_samp, uv).r, 0.0, 1.0);
			let metallic = clamp(drawu.reflection_params.y * textureSample(metallic_tex, metallic_samp, uv).r, 0.0, 1.0);
			let reflectance = clamp(drawu.reflection_params.z, 0.0, 1.0);
			let specular_color = mix(vec3<f32>(reflectance, reflectance, reflectance), base, metallic);
			let specular_power = mix(8.0, 128.0, smoothness);
			let nh = max(dot(specular_n, half_vec), 0.0);
			var specular_shape = pow(nh, specular_power);
			if (drawu.specular_toon_params.x > 0.5) {
				let perceptual_roughness = max(1.0 - smoothness, 0.02);
				let roughness = max(perceptual_roughness * perceptual_roughness, 0.0004);
				let toon_specular = pow(nh, 1.0 / roughness);
				specular_shape = lil_tooning_scale(
					toon_specular,
					clamp(drawu.specular_toon_params.y, 0.0, 1.0),
					clamp(drawu.specular_toon_params.z, 0.0, 1.0)
				);
			}
			specular = specular_color * frame.light_color.rgb * frame.light_color.w * specular_shape * clamp(drawu.reflection_control.y, 0.0, 1.0);
			let reflection_color = drawu.reflection_color * reflection_color_texel;
			let reflection_lighting = mix(
				vec3<f32>(1.0, 1.0, 1.0),
				frame.light_color.rgb * frame.light_color.w,
				clamp(drawu.reflection_ext_params.x, 0.0, 1.0),
			);
			let env = textureSample(reflection_tex, reflection_samp, reflection_uv).rgb * reflection_color.rgb * drawu.reflection_cube_color.rgb * reflection_lighting;
			authored_reflection = env * (0.18 + 0.32 * reflection_fresnel) * reflectance * clamp(drawu.reflection_control.z, 0.0, 1.0);
		}
	} else {
		let specular_intensity = clamp(drawu.uv_anim_params.w, 0.0, 2.0);
		let specular_shape = pow(max(dot(n, half_vec), 0.0), clamp(drawu.emissive_factor.w, 1.0, 128.0));
		specular = vec3<f32>(specular_shape * specular_intensity);
		authored_reflection = textureSample(reflection_tex, reflection_samp, reflection_uv).rgb * (0.18 + 0.32 * reflection_fresnel);
	}
	var rim = vec3<f32>(0.0, 0.0, 0.0);
	if (!disable_rim) {
		if (drawu.shadow_params.x > 0.5) {
			let rim_tex_color = textureSample(rim_tex, rim_samp, uv);
			var rim_color = drawu.rim_color.rgb * rim_tex_color.rgb;
			rim_color = mix(rim_color, rim_color * base, clamp(drawu.rim_control.y, 0.0, 1.0));
			let rim_n = normalize(mix(geometry_n, n, clamp(drawu.rim_ext_params.y, 0.0, 1.0)));
			let rim_raw = pow(clamp(1.0 - abs(dot(rim_n, v)), 0.0, 1.0), max(drawu.rim_params.z, 0.00001));
			let rim_factor = lil_tooning_scale(rim_raw, clamp(drawu.rim_params.x, 0.0, 1.0), clamp(drawu.rim_params.y, 0.0, 1.0));
			let lit_rim_color = mix(rim_color, rim_color * frame.light_color.rgb * frame.light_color.w, clamp(drawu.rim_control.z, 0.0, 1.0));
			let rim_alpha = clamp(drawu.rim_control.x * rim_tex_color.a, 0.0, 1.0);
			let rim_shadow = mix(1.0, shading, clamp(drawu.rim_ext_params.x, 0.0, 1.0));
			let rim_backface = select(clamp(drawu.rim_ext_params.z, 0.0, 1.0), 1.0, front_facing);
			rim = lit_rim_color * rim_factor * rim_alpha * rim_shadow * rim_backface;
			let rim_dir = pow(clamp(dot(rim_n, l) * 0.5 + 0.5, 0.0, 1.0), mix(1.0, 8.0, clamp(drawu.rim_indirect_params.y, 0.0, 1.0)));
			let rim_dir_factor = clamp(drawu.rim_indirect_params.x * rim_dir, 0.0, 1.0);
			rim = mix(rim, rim * rim_dir, rim_dir_factor);
			let indir_raw = pow(clamp(1.0 - abs(dot(rim_n, v)), 0.0, 1.0), max(drawu.rim_params.z, 0.00001));
			let indir_factor = lil_tooning_scale(
				indir_raw,
				clamp(drawu.rim_indirect_params.w, 0.0, 1.0),
				clamp(drawu.rim_indirect_ext_params.x, 0.0, 1.0)
			) * clamp(drawu.rim_indirect_params.z * drawu.rim_indirect_color.a, 0.0, 1.0);
			rim = rim + drawu.rim_indirect_color.rgb * indir_factor * rim_alpha * rim_shadow * rim_backface;
		} else {
			let rim_base = pow(clamp(1.0 - dot(n, v) + drawu.rim_params.z, 0.0, 1.0), max(drawu.rim_params.y, 0.00001));
			rim = rim_base * drawu.rim_color.rgb;
			rim = rim * mix(vec3<f32>(1.0, 1.0, 1.0), textureSample(rim_tex, rim_samp, uv).rgb, clamp(drawu.rim_params.w, 0.0, 1.0));
			let lighting_scalar = clamp(0.35 + 0.65 * max(dot(n, l), 0.0), 0.0, 1.0);
			rim = rim * mix(vec3<f32>(1.0, 1.0, 1.0), vec3<f32>(lighting_scalar, lighting_scalar, lighting_scalar), clamp(drawu.rim_params.x, 0.0, 1.0));
		}
	}
	if (drawu.shadow_params.x > 0.5) {
		let reflection_color_texel = textureSample(reflection_color_tex, reflection_color_samp, uv);
		let reflection_color_alpha = clamp(drawu.reflection_color.a * reflection_color_texel.a, 0.0, 1.0);
		lit = lil_blend_color(lit, specular * drawu.reflection_color.rgb * reflection_color_texel.rgb, clamp(reflection_color_alpha * drawu.reflection_control.y, 0.0, 1.0), drawu.reflection_control.w);
		lit = lil_blend_color(lit, authored_reflection, clamp(reflection_color_alpha * drawu.reflection_control.z, 0.0, 1.0), drawu.reflection_control.w);
		if (drawu.rim_shade_params.x > 0.5) {
			let rim_shade_n = normalize(mix(geometry_n, n, clamp(drawu.rim_ext_params.w, 0.0, 1.0)));
			let rim_shade_raw = pow(clamp(1.0 - abs(dot(rim_shade_n, v)), 0.0, 1.0), max(drawu.rim_shade_params.w, 0.00001));
			let rim_shade = lil_tooning_scale(
				rim_shade_raw,
				clamp(drawu.rim_shade_params.y, 0.0, 1.0),
				clamp(drawu.rim_shade_params.z, 0.0, 1.0)
			) * clamp(drawu.rim_shade_color.a, 0.0, 1.0);
			lit = mix(lit, lit * drawu.rim_shade_color.rgb, rim_shade);
		}
		lit = lil_blend_color(lit, rim, 1.0, drawu.rim_control.w);
	} else {
		lit = lit + specular + authored_reflection;
		lit = lit + rim;
	}

	let disable_emissive = (dbg & DBG_DISABLE_EMISSIVE) != 0u;
	let emission_tex_color = textureSample(emissive_tex, emissive_samp, uv);
	if (!disable_emissive) {
		if (drawu.shadow_params.x > 0.5) {
			var emission_color = drawu.emission_color.rgb * emission_tex_color.rgb;
			emission_color = mix(emission_color, emission_color * base, clamp(drawu.emission_params.y, 0.0, 1.0));
			let emission_blend = clamp(drawu.emission_params.x * drawu.emission_params.z * drawu.emission_color.a * emission_tex_color.a, 0.0, 1.0);
			lit = lil_blend_color(lit, emission_color, emission_blend, drawu.emission_params.w);
		} else {
			let emission_raw = drawu.emissive_factor.rgb * emission_tex_color.rgb;
			lit = lit + emission_raw;
		}
	}
	return vec4<f32>(premultiply_when_blending(max(lit, vec3<f32>(0.0, 0.0, 0.0)), out_a, alpha_kind), out_a);
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
	let n = normalize(i.wn);
	let l = normalize(frame.light_dir.xyz);
	let lighting_scalar = clamp(0.35 + 0.65 * max(dot(n, l), 0.0), 0.0, 1.0);
	let lit_base = drawu.outline_color.rgb * mix(vec3<f32>(1.0, 1.0, 1.0), vec3<f32>(lighting_scalar, lighting_scalar, lighting_scalar), clamp(drawu.outline_params.z, 0.0, 1.0));
	let outline_ndotl = dot(n, l) * 0.5 + 0.5;
	let lit_factor = clamp(outline_ndotl * drawu.outline_lit_params.x + drawu.outline_lit_params.y, 0.0, 1.0) * clamp(drawu.outline_lit_color.a, 0.0, 1.0);
	let lit_color = mix(drawu.outline_lit_color.rgb, samp_tex.rgb * drawu.outline_lit_color.rgb, clamp(drawu.outline_lit_params.z, 0.0, 1.0));
	let color = mix(lit_base, lit_color, lit_factor);
	return vec4<f32>(color, clamp(drawu.outline_color.a, 0.0, 1.0));
}

@fragment
fn fs_toon_zprepass(i: VsOut) -> @location(0) vec4<f32> {
	let uv = animated_uv(i.uv);
	let samp_tex = textureSample(tex, base_samp, uv);
	let a = apply_lil_alpha_mask(samp_tex.a * drawu.base_color.a, uv);
	discard_transparent_zprepass(a, drawu.params.y, drawu.params.z, drawu.outline_params.w);
	return vec4<f32>(0.0, 0.0, 0.0, 0.0);
}
