struct VsOut {
	@builtin(position) clip_pos: vec4<f32>,
	@location(0) uv: vec2<f32>,
}

struct OutlineParams {
	color: vec4<f32>,
	// x: outline width in pixels, y: lighting mix (reserved), z: roundness, w: jump step in pixels.
	params: vec4<f32>,
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

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> outline: OutlineParams;

fn dims_i() -> vec2<i32> {
	return vec2<i32>(textureDimensions(tex, 0));
}

fn dims_f() -> vec2<f32> {
	return max(vec2<f32>(textureDimensions(tex, 0)), vec2<f32>(1.0));
}

fn coord_from_uv(uv: vec2<f32>) -> vec2<i32> {
	let d = dims_i();
	return clamp(vec2<i32>(floor(clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)) * dims_f())), vec2<i32>(0), d - vec2<i32>(1));
}

fn load_tex(coord: vec2<i32>) -> vec4<f32> {
	let d = dims_i();
	return textureLoad(tex, clamp(coord, vec2<i32>(0), d - vec2<i32>(1)), 0);
}

fn seed_valid(seed: vec2<f32>) -> bool {
	return seed.x >= 0.0 && seed.y >= 0.0;
}

fn smooth_source_alpha(coord: vec2<i32>, roundness: f32) -> f32 {
	let center = load_tex(coord).a;
	if (roundness <= 0.001) {
		return center;
	}
	let cute = sqrt(clamp(roundness, 0.0, 1.0));
	let r = max(1, i32(round(mix(1.0, 8.0, cute))));
	var sum = center * 0.12;
	var weight = 0.12;
	for (var y = -8; y <= 8; y = y + 1) {
		for (var x = -8; x <= 8; x = x + 1) {
			if (x == 0 && y == 0) {
				continue;
			}
			let o = vec2<i32>(x, y);
			let dist = length(vec2<f32>(o));
			if (dist <= f32(r) + 0.5) {
				let t = dist / max(f32(r), 1.0);
				let w = exp(-t * t * mix(2.4, 1.15, cute));
				sum = sum + load_tex(coord + o).a * w;
				weight = weight + w;
			}
		}
	}
	return sum / weight;
}

@fragment
fn fs_seed(i: VsOut) -> @location(0) vec4<f32> {
	let coord = coord_from_uv(i.uv);
	let roundness = clamp(outline.params.z, 0.0, 1.0);
	let raw = load_tex(coord).a;
	let cute = sqrt(roundness);
	let a = mix(raw, smooth_source_alpha(coord, roundness), cute);
	let mask = smoothstep(mix(0.02, 0.28, cute), mix(0.08, 0.58, cute), a);
	if (mask > 0.5) {
		let uv = (vec2<f32>(coord) + vec2<f32>(0.5)) / dims_f();
		return vec4<f32>(uv, raw, 1.0);
	}
	return vec4<f32>(-1.0, -1.0, raw, 1.0);
}

fn choose_seed(current_coord: vec2<i32>, current: vec4<f32>, candidate: vec4<f32>) -> vec4<f32> {
	if (!seed_valid(candidate.xy)) {
		return current;
	}
	if (!seed_valid(current.xy)) {
		return candidate;
	}
	let p = (vec2<f32>(current_coord) + vec2<f32>(0.5)) / dims_f();
	let current_dist = distance(p, current.xy);
	let candidate_dist = distance(p, candidate.xy);
	if (candidate_dist < current_dist) {
		return candidate;
	}
	return current;
}

@fragment
fn fs_jump(i: VsOut) -> @location(0) vec4<f32> {
	let coord = coord_from_uv(i.uv);
	let center = load_tex(coord);
	var best = center;
	let step_px = max(1, i32(round(outline.params.w)));
	for (var y = -1; y <= 1; y = y + 1) {
		for (var x = -1; x <= 1; x = x + 1) {
			if (x == 0 && y == 0) {
				continue;
			}
			let candidate = load_tex(coord + vec2<i32>(x, y) * step_px);
			best = choose_seed(coord, best, candidate);
		}
	}
	// Preserve the original center alpha in z. xy is the nearest avatar-mask seed.
	return vec4<f32>(best.xy, center.z, 1.0);
}

@fragment
fn fs_main(i: VsOut) -> @location(0) vec4<f32> {
	let coord = coord_from_uv(i.uv);
	let data = load_tex(coord);
	if (data.z > 0.02 || !seed_valid(data.xy)) {
		discard;
	}
	let dist_px = distance(vec2<f32>(coord) + vec2<f32>(0.5), data.xy * dims_f());
	let width_px = clamp(outline.params.x, 0.0, 96.0);
	if (dist_px > width_px) {
		discard;
	}
	let roundness = clamp(outline.params.z, 0.0, 1.0);
	let edge = mix(0.75, 1.35, roundness);
	let alpha = 1.0 - smoothstep(width_px - edge, width_px + edge, dist_px);
	return vec4<f32>(outline.color.rgb, alpha);
}
