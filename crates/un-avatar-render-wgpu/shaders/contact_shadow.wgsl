struct Frame {
	view_proj: mat4x4<f32>,
	light_dir: vec4<f32>,
	camera_pos: vec4<f32>,
}

struct ContactShadow {
	params: vec4<f32>,
}

struct VsOut {
	@builtin(position) clip: vec4<f32>,
	@location(0) local: vec2<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
@group(1) @binding(0) var<uniform> shadow: ContactShadow;

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
	let corners = array<vec2<f32>, 6>(
		vec2<f32>(-1.0, -1.0),
		vec2<f32>( 1.0, -1.0),
		vec2<f32>(-1.0,  1.0),
		vec2<f32>(-1.0,  1.0),
		vec2<f32>( 1.0, -1.0),
		vec2<f32>( 1.0,  1.0),
	);
	let local = corners[vi];
	let radius = max(shadow.params.y, 0.0001);
	let world = vec4<f32>(local.x * radius, shadow.params.w, local.y * radius * 0.58, 1.0);
	var out: VsOut;
	out.clip = frame.view_proj * world;
	out.local = local;
	return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
	let strength = clamp(shadow.params.x, 0.0, 1.0);
	let softness = max(shadow.params.z, 0.1);
	let d2 = dot(in.local, in.local);
	let falloff = pow(clamp(1.0 - d2, 0.0, 1.0), softness);
	let alpha = strength * falloff;
	return vec4<f32>(0.0, 0.0, 0.0, alpha);
}
