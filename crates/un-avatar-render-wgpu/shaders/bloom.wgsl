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

struct PostUniform {
	color: vec4<f32>,
	bloom: vec4<f32>,
	ssao: vec4<f32>,
	grading: vec4<f32>,
}

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> adjust: PostUniform;
@group(0) @binding(3) var depth_tex: texture_depth_2d;
@group(0) @binding(4) var bloom_tex: texture_2d<f32>;

fn bright_part(color: vec3<f32>) -> vec3<f32> {
	let threshold = adjust.bloom.x;
	let y = dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
	let knee = max((1.0 - threshold) * 0.5, 0.08);
	let weight = smoothstep(threshold - knee, threshold + knee, y);
	return color * weight;
}

@fragment
fn fs_extract(i: VsOut) -> @location(0) vec4<f32> {
	let uv = clamp(i.uv, vec2<f32>(0.0), vec2<f32>(1.0));
	let dims = max(vec2<f32>(textureDimensions(tex, 0)), vec2<f32>(1.0));
	let px = 1.0 / dims;
	var acc = bright_part(textureSample(tex, samp, uv).rgb) * 0.40;
	acc += bright_part(textureSample(tex, samp, uv + vec2<f32>( px.x, 0.0)).rgb) * 0.15;
	acc += bright_part(textureSample(tex, samp, uv + vec2<f32>(-px.x, 0.0)).rgb) * 0.15;
	acc += bright_part(textureSample(tex, samp, uv + vec2<f32>(0.0,  px.y)).rgb) * 0.15;
	acc += bright_part(textureSample(tex, samp, uv + vec2<f32>(0.0, -px.y)).rgb) * 0.15;
	return vec4<f32>(acc, 1.0);
}

fn blur(uv: vec2<f32>, axis: vec2<f32>) -> vec3<f32> {
	let dims = max(vec2<f32>(textureDimensions(tex, 0)), vec2<f32>(1.0));
	let px = axis * max(adjust.bloom.y * 0.16, 1.0) / dims;
	var acc = textureSample(tex, samp, uv).rgb * 0.227027;
	acc += textureSample(tex, samp, uv + px * 1.384615).rgb * 0.316216;
	acc += textureSample(tex, samp, uv - px * 1.384615).rgb * 0.316216;
	acc += textureSample(tex, samp, uv + px * 3.230769).rgb * 0.070270;
	acc += textureSample(tex, samp, uv - px * 3.230769).rgb * 0.070270;
	return acc;
}

@fragment
fn fs_blur_h(i: VsOut) -> @location(0) vec4<f32> {
	let uv = clamp(i.uv, vec2<f32>(0.0), vec2<f32>(1.0));
	return vec4<f32>(blur(uv, vec2<f32>(1.0, 0.0)), 1.0);
}

@fragment
fn fs_blur_v(i: VsOut) -> @location(0) vec4<f32> {
	let uv = clamp(i.uv, vec2<f32>(0.0), vec2<f32>(1.0));
	return vec4<f32>(blur(uv, vec2<f32>(0.0, 1.0)), 1.0);
}
