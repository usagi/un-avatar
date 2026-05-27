struct VsOut {
	@builtin(position) clip_pos: vec4<f32>,
	@location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
	var positions = array<vec2<f32>, 3>(
		vec2<f32>(-1.0, -1.0),
		vec2<f32>(3.0, -1.0),
		vec2<f32>(-1.0, 3.0)
	);
	var uvs = array<vec2<f32>, 3>(
		vec2<f32>(0.0, 1.0),
		vec2<f32>(2.0, 1.0),
		vec2<f32>(0.0, -1.0)
	);
	var o: VsOut;
	o.clip_pos = vec4<f32>(positions[vi], 0.0, 1.0);
	o.uv = uvs[vi];
	return o;
}

struct ColorAdjust {
	color: vec4<f32>,
	bloom: vec4<f32>,
	ssao: vec4<f32>,
	grading: vec4<f32>,
};

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> adjust: ColorAdjust;
@group(0) @binding(3) var depth_tex: texture_depth_2d;
@group(0) @binding(4) var bloom_tex: texture_2d<f32>;

fn apply_color_adjust(color: vec3<f32>) -> vec3<f32> {
	let exposure = adjust.color.x;
	let contrast = adjust.color.y;
	let saturation = adjust.color.z;
	var rgb = color * exp2(exposure);
	rgb = (rgb - vec3<f32>(0.5)) * contrast + vec3<f32>(0.5);
	let luma = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
	rgb = mix(vec3<f32>(luma), rgb, saturation);
	rgb = apply_white_balance(rgb);
	rgb = apply_grading_look(rgb);
	return max(rgb, vec3<f32>(0.0));
}

fn apply_white_balance(color: vec3<f32>) -> vec3<f32> {
	let temperature = clamp(adjust.grading.z, -1.0, 1.0);
	let tint = clamp(adjust.grading.w, -1.0, 1.0);
	let warmth = vec3<f32>(1.0 + temperature * 0.10, 1.0 + temperature * 0.025, 1.0 - temperature * 0.10);
	let tint_balance = vec3<f32>(1.0 + tint * 0.055, 1.0 - abs(tint) * 0.065, 1.0 - tint * 0.035);
	return max(color * warmth * tint_balance, vec3<f32>(0.0));
}

fn saturate_with_luma(color: vec3<f32>, amount: f32) -> vec3<f32> {
	let y = dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
	return mix(vec3<f32>(y), color, amount);
}

fn film_look(color: vec3<f32>) -> vec3<f32> {
	let y = dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
	let shadows = vec3<f32>(0.88, 1.02, 1.08);
	let highlights = vec3<f32>(1.08, 1.03, 0.94);
	let grade = mix(color * shadows, color * highlights, smoothstep(0.18, 0.82, y));
	return saturate_with_luma((grade - vec3<f32>(0.5)) * 1.08 + vec3<f32>(0.5), 0.94);
}

fn apply_grading_look(color: vec3<f32>) -> vec3<f32> {
	let look = i32(round(adjust.grading.x));
	let intensity = clamp(adjust.grading.y, 0.0, 1.0);
	if (intensity <= 0.0 || look == 0) {
		return color;
	}
	var graded = color;
	if (look == 1) {
		graded = saturate_with_luma(color * vec3<f32>(1.06, 1.02, 0.93) + vec3<f32>(0.012, 0.006, 0.0), 1.04);
	} else if (look == 2) {
		graded = saturate_with_luma(color * vec3<f32>(0.94, 1.01, 1.08) + vec3<f32>(0.0, 0.004, 0.012), 1.02);
	} else if (look == 3) {
		graded = film_look(color);
	} else if (look == 4) {
		graded = saturate_with_luma(1.0 - pow(max(vec3<f32>(0.0), 1.0 - color), vec3<f32>(1.08)), 0.92);
	} else if (look == 5) {
		graded = saturate_with_luma((color - vec3<f32>(0.5)) * 1.12 + vec3<f32>(0.5), 1.16);
	}
	return mix(color, max(graded, vec3<f32>(0.0)), intensity);
}

fn bright_part(color: vec3<f32>) -> vec3<f32> {
	let threshold = adjust.bloom.x;
	let y = dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
	let knee = max((1.0 - threshold) * 0.5, 0.08);
	let weight = smoothstep(threshold - knee, threshold + knee, y);
	return color * weight;
}

fn bloom_sample(uv: vec2<f32>) -> vec3<f32> {
	let strength = adjust.color.w;
	if (strength <= 0.0) {
		return vec3<f32>(0.0);
	}
	if (adjust.bloom.z > 0.5) {
		return textureSample(bloom_tex, samp, uv).rgb * strength;
	}
	let dims = max(vec2<f32>(textureDimensions(tex, 0)), vec2<f32>(1.0));
	let px = adjust.bloom.y / dims;
	let mid_px = px * 1.8;
	let far_px = px * 3.2;
	var acc = bright_part(textureSample(tex, samp, uv).rgb) * 0.12;
	acc += bright_part(textureSample(tex, samp, uv + vec2<f32>( px.x,  0.0)).rgb) * 0.09;
	acc += bright_part(textureSample(tex, samp, uv + vec2<f32>(-px.x,  0.0)).rgb) * 0.09;
	acc += bright_part(textureSample(tex, samp, uv + vec2<f32>( 0.0,  px.y)).rgb) * 0.09;
	acc += bright_part(textureSample(tex, samp, uv + vec2<f32>( 0.0, -px.y)).rgb) * 0.09;
	acc += bright_part(textureSample(tex, samp, uv + vec2<f32>( px.x,  px.y)).rgb) * 0.06;
	acc += bright_part(textureSample(tex, samp, uv + vec2<f32>(-px.x,  px.y)).rgb) * 0.06;
	acc += bright_part(textureSample(tex, samp, uv + vec2<f32>( px.x, -px.y)).rgb) * 0.06;
	acc += bright_part(textureSample(tex, samp, uv + vec2<f32>(-px.x, -px.y)).rgb) * 0.06;
	acc += bright_part(textureSample(tex, samp, uv + vec2<f32>( mid_px.x,  0.0)).rgb) * 0.04;
	acc += bright_part(textureSample(tex, samp, uv + vec2<f32>(-mid_px.x,  0.0)).rgb) * 0.04;
	acc += bright_part(textureSample(tex, samp, uv + vec2<f32>( 0.0,  mid_px.y)).rgb) * 0.04;
	acc += bright_part(textureSample(tex, samp, uv + vec2<f32>( 0.0, -mid_px.y)).rgb) * 0.04;
	acc += bright_part(textureSample(tex, samp, uv + vec2<f32>( mid_px.x,  mid_px.y)).rgb) * 0.025;
	acc += bright_part(textureSample(tex, samp, uv + vec2<f32>(-mid_px.x,  mid_px.y)).rgb) * 0.025;
	acc += bright_part(textureSample(tex, samp, uv + vec2<f32>( mid_px.x, -mid_px.y)).rgb) * 0.025;
	acc += bright_part(textureSample(tex, samp, uv + vec2<f32>(-mid_px.x, -mid_px.y)).rgb) * 0.025;
	acc += bright_part(textureSample(tex, samp, uv + vec2<f32>( far_px.x,  0.0)).rgb) * 0.012;
	acc += bright_part(textureSample(tex, samp, uv + vec2<f32>(-far_px.x,  0.0)).rgb) * 0.012;
	acc += bright_part(textureSample(tex, samp, uv + vec2<f32>( 0.0,  far_px.y)).rgb) * 0.012;
	acc += bright_part(textureSample(tex, samp, uv + vec2<f32>( 0.0, -far_px.y)).rgb) * 0.012;
	return acc * strength;
}

fn ssao_factor(uv: vec2<f32>, alpha: f32) -> f32 {
	let strength = clamp(adjust.ssao.x, 0.0, 1.0);
	if (strength <= 0.0 || alpha <= 0.02) {
		return 1.0;
	}
	let dims_i = vec2<i32>(textureDimensions(depth_tex));
	let dims = max(vec2<f32>(dims_i), vec2<f32>(1.0));
	let coord = clamp(vec2<i32>(floor(uv * dims)), vec2<i32>(0), dims_i - vec2<i32>(1));
	let center = textureLoad(depth_tex, coord, 0);
	if (center >= 0.9999) {
		return 1.0;
	}
	let radius = max(adjust.ssao.y, 1.0);
	let bias = adjust.ssao.z;
	let range = max(adjust.ssao.w, 0.001);
	var occ = 0.0;
	let offsets = array<vec2<f32>, 8>(
		vec2<f32>( 1.0,  0.0), vec2<f32>(-1.0,  0.0),
		vec2<f32>( 0.0,  1.0), vec2<f32>( 0.0, -1.0),
		vec2<f32>( 0.7,  0.7), vec2<f32>(-0.7,  0.7),
		vec2<f32>( 0.7, -0.7), vec2<f32>(-0.7, -0.7)
	);
	for (var idx = 0u; idx < 8u; idx = idx + 1u) {
		let sc = clamp(coord + vec2<i32>(round(offsets[idx] * radius)), vec2<i32>(0), dims_i - vec2<i32>(1));
		let d = textureLoad(depth_tex, sc, 0);
		let diff = center - d;
		if (diff > bias && diff < range) {
			occ += 1.0 - smoothstep(bias, range, diff);
		}
	}
	return clamp(1.0 - strength * (occ / 8.0), 0.0, 1.0);
}

@fragment
fn fs_main(i: VsOut) -> @location(0) vec4<f32> {
	let uv = clamp(i.uv, vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0));
	let color = textureSample(tex, samp, uv);
	let ao = ssao_factor(uv, color.a);
	return vec4<f32>(apply_color_adjust(color.rgb * ao + bloom_sample(uv)), color.a);
}
