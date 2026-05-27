struct Globals {
	view_proj: mat4x4<f32>,
	inv_view_proj: mat4x4<f32>,
	light_dir: vec4<f32>,
	camera_pos: vec4<f32>,
}

@group(0) @binding(0) var<uniform> globals: Globals;

struct VsOut {
	@builtin(position) clip: vec4<f32>,
	@location(0) color: vec3<f32>,
}

fn axis_position(vertex_index: u32) -> vec3<f32> {
	switch vertex_index {
		case 1u: { return vec3<f32>(1.0, 0.0, 0.0); }
		case 3u: { return vec3<f32>(0.0, 1.0, 0.0); }
		case 5u: { return vec3<f32>(0.0, 0.0, 1.0); }
		default: { return vec3<f32>(0.0, 0.0, 0.0); }
	}
}

fn axis_color(vertex_index: u32) -> vec3<f32> {
	if vertex_index < 2u {
		return vec3<f32>(1.0, 0.05, 0.05);
	}
	if vertex_index < 4u {
		return vec3<f32>(0.05, 1.0, 0.05);
	}
	return vec3<f32>(0.1, 0.35, 1.0);
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VsOut {
	var out: VsOut;
	let pos = axis_position(vertex_index);
	out.clip = globals.view_proj * vec4<f32>(pos, 1.0);
	out.color = axis_color(vertex_index);
	return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
	return vec4<f32>(in.color, 1.0);
}