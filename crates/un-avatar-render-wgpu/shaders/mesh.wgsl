// メッシュ描画: 頂点は共通、フラグメントはシェーディング種別ごとに別エントリ＋別パイプライン（ランタイムのシェーディング分岐を避ける）。
//
// - fs_lit: LitLambert
// - fs_unlit: Unlit（ベース色のみ）
// - fs_mtoon: VRM MToon parameter-driven toon shading

struct Frame {
	view_proj: mat4x4<f32>,
	light_dir: vec4<f32>,
	camera_pos: vec4<f32>,
	light_color: vec4<f32>,
	ambient_color: vec4<f32>,
}

struct DrawTransform {
	model: mat4x4<f32>,
}

struct DrawMaterial {
	base_color: vec4<f32>,
	params: vec4<f32>,
	shade_color: vec4<f32>,
	shading_params: vec4<f32>,
	matcap_factor: vec4<f32>,
	rim_color: vec4<f32>,
	rim_params: vec4<f32>,
	outline_color: vec4<f32>,
	outline_params: vec4<f32>,
	emissive_factor: vec4<f32>,
	uv_anim_params: vec4<f32>,
}

struct MorphU {
	target_count: u32,
	vertex_count: u32,
	_pad: vec2<u32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
@group(1) @binding(0) var<uniform> drawt: DrawTransform;
@group(1) @binding(1) var tex: texture_2d<f32>;
@group(1) @binding(2) var samp: sampler;
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
	let mask = textureSampleLevel(outline_width_tex, samp, o.uv, 0.0).g;
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

/// MASK（Lit/Unlit）: ゲートはテクスチャ α のみ。
fn mask_discard_lit_unlit(alb: vec3<f32>, a: f32, alpha_kind: f32, cutoff: f32) {
	_ = alb;
	if alpha_kind > 0.5 && alpha_kind < 1.5 {
		if a < cutoff {
			discard;
		}
	}
}

/// MASK（MToon）: VRoid clothing often leaves RGB in fully transparent texels, so gate on alpha only.
fn mask_discard_mtoon(alb: vec3<f32>, a: f32, alpha_kind: f32, cutoff: f32) {
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

fn discard_backface_if_single_sided(front_facing: bool, flags: u32) {
	if !front_facing && (flags & MAT_DOUBLE_SIDED) == 0u {
		discard;
	}
}

fn face_normal(n: vec3<f32>, front_facing: bool, flags: u32) -> vec3<f32> {
	if front_facing || (flags & MAT_DOUBLE_SIDED) == 0u {
		return n;
	}
	return -n;
}

fn mtoon_matcap_uv(n: vec3<f32>, v: vec3<f32>) -> vec2<f32> {
	var world_view_x = normalize(vec3<f32>(v.z, 0.0, -v.x));
	if (length(world_view_x) < 0.0001) {
		world_view_x = vec3<f32>(1.0, 0.0, 0.0);
	}
	let world_view_y = cross(v, world_view_x);
	return vec2<f32>(dot(world_view_x, n), dot(world_view_y, n)) * 0.495 + vec2<f32>(0.5, 0.5);
}

fn mtoon_reflection_uv(n: vec3<f32>, v: vec3<f32>) -> vec2<f32> {
	let r = normalize(reflect(-v, n));
	let u = atan2(r.z, r.x) * 0.15915494309189535 + 0.5;
	let vv = acos(clamp(r.y, -1.0, 1.0)) * 0.3183098861837907;
	return vec2<f32>(u, vv);
}

fn linearstep(edge0: f32, edge1: f32, x: f32) -> f32 {
	return clamp((x - edge0) / (edge1 - edge0), 0.0, 1.0);
}

fn normal_mapped(n_in: vec3<f32>, wp: vec3<f32>, uv: vec2<f32>, scale: f32) -> vec3<f32> {
	let n = normalize(n_in);
	if (abs(scale) < 0.000001) {
		return n;
	}
	let packed = textureSample(normal_tex, samp, uv).xyz;
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
	let sample = textureSample(occlusion_tex, samp, uv).r;
	return clamp(mix(1.0, sample, strength), 0.0, 1.0);
}

@fragment
fn fs_lit(i: VsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
	let dbg = bitcast<u32>(drawu.params.w);
	discard_backface_if_single_sided(front_facing, dbg);
	let samp_tex = textureSample(tex, samp, i.uv);
	let alb = samp_tex.rgb;
	let tex_a = samp_tex.a;
	let a = tex_a * drawu.base_color.a;
	let alpha_kind = drawu.params.y;
	let cutoff = drawu.params.z;
	mask_discard_lit_unlit(alb, a, alpha_kind, cutoff);
	let out_a = fragment_out_alpha(alpha_kind, a, drawu.base_color.a);
	let base = select(alb * drawu.base_color.rgb, drawu.base_color.rgb, (dbg & DBG_SOLID_PRIM_COLOR) != 0u);
	let l = normalize(frame.light_dir.xyz);
	let normal_scale = select(drawu.shade_color.w, 0.0, (dbg & DBG_DISABLE_NORMAL_MAP) != 0u);
	let n = face_normal(normal_mapped(i.wn, i.wp, i.uv, normal_scale), front_facing, dbg);
	let ndl = max(dot(n, l), 0.0);
	let ambient = frame.ambient_color.rgb * (frame.ambient_color.w * 0.57);
	let direct = frame.light_color.rgb * (frame.light_color.w * 0.8 * ndl);
	let lit = base * (ambient + direct) * authored_occlusion(i.uv, dbg);
	return vec4<f32>(lit, out_a);
}

@fragment
fn fs_unlit(i: VsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
	let dbg = bitcast<u32>(drawu.params.w);
	discard_backface_if_single_sided(front_facing, dbg);
	let samp_tex = textureSample(tex, samp, i.uv);
	let alb = samp_tex.rgb;
	let tex_a = samp_tex.a;
	let a = tex_a * drawu.base_color.a;
	let alpha_kind = drawu.params.y;
	let cutoff = drawu.params.z;
	mask_discard_lit_unlit(alb, a, alpha_kind, cutoff);
	let out_a = fragment_out_alpha(alpha_kind, a, drawu.base_color.a);
	let base = select(alb * drawu.base_color.rgb, drawu.base_color.rgb, (dbg & DBG_SOLID_PRIM_COLOR) != 0u);
	return vec4<f32>(base, out_a);
}

@fragment
fn fs_mtoon(i: VsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
	let dbg = bitcast<u32>(drawu.params.w);
	discard_backface_if_single_sided(front_facing, dbg);
	let samp_tex = textureSample(tex, samp, i.uv);
	let alb = samp_tex.rgb;
	let tex_a = samp_tex.a;
	let a = tex_a * drawu.base_color.a;
	let alpha_kind = drawu.params.y;
	let cutoff = drawu.params.z;
	mask_discard_mtoon(alb, a, alpha_kind, cutoff);
	let out_a = fragment_out_alpha(alpha_kind, a, drawu.base_color.a);
	let base = select(alb * drawu.base_color.rgb, drawu.base_color.rgb, (dbg & DBG_SOLID_PRIM_COLOR) != 0u);
	if ((dbg & DBG_BASE_TEXTURE_ONLY) != 0u) {
		// 診断用: shading / GI / matcap / rim / emissive / shade_term を全てスキップして base のみ。
		// リングがまだ残るならテクスチャ自身（モデル制作者が描いた肌グラデ）かメッシュ重なり由来。
		return vec4<f32>(max(base, vec3<f32>(0.0, 0.0, 0.0)), out_a);
	}
	let normal_scale = select(drawu.shade_color.w, 0.0, (dbg & DBG_DISABLE_NORMAL_MAP) != 0u);
	let n = face_normal(normal_mapped(i.wn, i.wp, i.uv, normal_scale), front_facing, dbg);
	let l = normalize(frame.light_dir.xyz);
	let v = normalize(frame.camera_pos.xyz - i.wp);

	let force_shift_zero = (dbg & DBG_FORCE_SHADING_SHIFT_ZERO) != 0u;
	let raw_shift_tex_value = textureSample(shading_shift_tex, samp, i.uv).r * drawu.shading_params.z;
	let shift_tex_value = select(raw_shift_tex_value, 0.0, force_shift_zero);
	let shading_shift_factor = select(drawu.shading_params.x, 0.0, force_shift_zero);
	var shading = dot(n, l) + shading_shift_factor + shift_tex_value;
	// VRMC_materials_mtoon-1.0 の shading transition は linearstep(-1 + toony, 1 - toony, shading)。
	// boundary は `1 - toony` の線形（旧実装の `1 - toony²` だと境界幅が約 2 倍広くなっていた）。
	let toony_st = clamp(drawu.shading_params.y, 0.0, 1.0);
	let toony_boundary = 1.0 - toony_st;
	shading = linearstep(-toony_boundary, toony_boundary, shading);
	let disable_shade_color = (dbg & DBG_DISABLE_SHADE_COLOR) != 0u;
	let shade_term_raw = drawu.shade_color.rgb * textureSample(shade_tex, samp, i.uv).rgb;
	let shade_term = select(shade_term_raw, base, disable_shade_color);
	let direct_color = mix(shade_term, base, shading) * frame.light_color.rgb * frame.light_color.w;

	// VRM0/UniVRM adds indirect light after the toon shade mix and clamps it by
	// the lit color. This keeps white clothes from falling into large faceted
	// gray patches while still preserving authored shade colors.
	let gi_equalization = clamp(drawu.shading_params.w, 0.0, 1.0);
	let indirect_light = mix(shade_term, base, gi_equalization) * frame.ambient_color.rgb * frame.ambient_color.w;
	var lit = min(direct_color + indirect_light, base) * authored_occlusion(i.uv, dbg);

	let disable_matcap = (dbg & DBG_DISABLE_MATCAP) != 0u;
	let disable_rim = (dbg & DBG_DISABLE_RIM) != 0u;
	let matcap_uv = mtoon_matcap_uv(n, v);
	let matcap_raw = drawu.matcap_factor.rgb * textureSample(matcap_tex, samp, matcap_uv).rgb * drawu.matcap_factor.w;
	let matcap = select(matcap_raw, vec3<f32>(0.0, 0.0, 0.0), disable_matcap);
	let specular_intensity = clamp(drawu.uv_anim_params.w, 0.0, 2.0);
	let half_vec = normalize(l + v);
	let specular_shape = pow(max(dot(n, half_vec), 0.0), clamp(drawu.emissive_factor.w, 1.0, 128.0));
	let specular = vec3<f32>(specular_shape * specular_intensity);
	let reflection_uv = mtoon_reflection_uv(n, v);
	let reflection_fresnel = pow(clamp(1.0 - dot(n, v), 0.0, 1.0), 2.0);
	let authored_reflection = textureSample(reflection_tex, samp, reflection_uv).rgb * (0.18 + 0.32 * reflection_fresnel);
	let rim_base = pow(clamp(1.0 - dot(n, v) + drawu.rim_params.z, 0.0, 1.0), max(drawu.rim_params.y, 0.00001));
	var rim = select(rim_base * drawu.rim_color.rgb, vec3<f32>(0.0, 0.0, 0.0), disable_rim);
	rim = rim * mix(vec3<f32>(1.0, 1.0, 1.0), textureSample(rim_tex, samp, i.uv).rgb, clamp(drawu.rim_params.w, 0.0, 1.0));
	let lighting_scalar = clamp(0.35 + 0.65 * max(dot(n, l), 0.0), 0.0, 1.0);
	rim = rim * mix(vec3<f32>(1.0, 1.0, 1.0), vec3<f32>(lighting_scalar, lighting_scalar, lighting_scalar), clamp(drawu.rim_params.x, 0.0, 1.0));
	lit = lit + matcap + specular + authored_reflection + rim;

	let disable_emissive = (dbg & DBG_DISABLE_EMISSIVE) != 0u;
	let emission_raw = drawu.emissive_factor.rgb * textureSample(emissive_tex, samp, i.uv).rgb;
	let emission = select(emission_raw, vec3<f32>(0.0, 0.0, 0.0), disable_emissive);
	lit = lit + emission;
	return vec4<f32>(max(lit, vec3<f32>(0.0, 0.0, 0.0)), out_a);
}

@fragment
fn fs_outline(i: VsOut) -> @location(0) vec4<f32> {
	let samp_tex = textureSample(tex, samp, i.uv);
	let a = samp_tex.a * drawu.base_color.a;
	mask_discard_mtoon(samp_tex.rgb, a, drawu.params.y, drawu.params.z);
	let mask = textureSample(outline_width_tex, samp, i.uv).g;
	if (mask <= 0.001) {
		discard;
	}
	let n = normalize(i.wn);
	let l = normalize(frame.light_dir.xyz);
	let lighting_scalar = clamp(0.35 + 0.65 * max(dot(n, l), 0.0), 0.0, 1.0);
	let color = drawu.outline_color.rgb * mix(vec3<f32>(1.0, 1.0, 1.0), vec3<f32>(lighting_scalar, lighting_scalar, lighting_scalar), clamp(drawu.outline_params.z, 0.0, 1.0));
	return vec4<f32>(color, 1.0);
}
