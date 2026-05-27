struct Globals {
	view_proj: mat4x4<f32>,
	inv_view_proj: mat4x4<f32>,
	light_dir: vec4<f32>,
	camera_pos: vec4<f32>,
}

@group(0) @binding(0) var<uniform> g: Globals;

struct VsOut {
	@builtin(position) clip_pos: vec4<f32>,
	@location(0) ndc: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
	var positions = array<vec2<f32>, 3>(
		vec2(-1.0, -1.0),
		vec2(3.0, -1.0),
		vec2(-1.0, 3.0)
	);
	let ndc = positions[vi];
	var out: VsOut;
	out.clip_pos = vec4(ndc, 0.0, 1.0);
	out.ndc = ndc;
	return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
	let inv = g.inv_view_proj;
	let ndc = in.ndc;
	let near_h = inv * vec4(ndc, 0.0, 1.0);
	let far_h = inv * vec4(ndc, 1.0, 1.0);
	let near_w = near_h.xyz / near_h.w;
	let far_w = far_h.xyz / far_h.w;
	let d = normalize(far_w - near_w);

	let horizon = 0.5 * (d.y + 1.0);
	let sky_low = vec3(0.08, 0.09, 0.12);
	let sky_high = vec3(0.35, 0.55, 0.9);
	let sky = mix(sky_low, sky_high, pow(horizon, 0.9));
	let l = normalize(g.light_dir.xyz);
	let sun = pow(max(dot(d, l), 0.0), 48.0);
	let sun_color = vec3(1.0, 0.95, 0.8) * 0.35;
	return vec4(sky + sun * sun_color, 1.0);
}
