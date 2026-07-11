struct Globals {
	view_proj: mat4x4<f32>,
	inv_view_proj: mat4x4<f32>,
	light_dir: vec4<f32>,
	camera_pos: vec4<f32>,
}

struct WardrobeBillboard {
	view_proj: mat4x4<f32>,
	camera_pos: vec4<f32>,
	center_size: vec4<f32>,
	time_params: vec4<f32>,
}

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var<uniform> billboard: WardrobeBillboard;

struct VsOut {
	@builtin(position) clip: vec4<f32>,
	@location(0) local: vec2<f32>,
}

fn segment_distance(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
	let ab = b - a;
	let h = clamp(dot(p - a, ab) / max(dot(ab, ab), 0.00001), 0.0, 1.0);
	return length(p - (a + ab * h));
}

fn line_mask(value: f32, width: f32) -> f32 {
	let aa = fwidth(value) + 0.001;
	return 1.0 - smoothstep(width - aa, width + aa, abs(value));
}

fn segment_mask(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>, width: f32) -> f32 {
	let d = segment_distance(p, a, b);
	let aa = fwidth(d) + 0.001;
	return 1.0 - smoothstep(width - aa, width + aa, d);
}

fn rounded_rect_distance(p: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
	let q = abs(p) - half_size + vec2(radius);
	return length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

fn rounded_rect_fill(p: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
	let d = rounded_rect_distance(p, half_size, radius);
	let aa = fwidth(d) + 0.001;
	return 1.0 - smoothstep(0.0 - aa, 0.0 + aa, d);
}

fn rounded_rect_line(p: vec2<f32>, half_size: vec2<f32>, radius: f32, width: f32) -> f32 {
	return line_mask(rounded_rect_distance(p, half_size, radius), width);
}

fn soft_disc(p: vec2<f32>, radius: f32) -> f32 {
	let d = length(p);
	let aa = fwidth(d) + 0.001;
	return 1.0 - smoothstep(radius - aa, radius + aa, d);
}

fn ring_mask(r: f32, radius: f32, width: f32) -> f32 {
	return line_mask(r - radius, width);
}

fn sparkle(p: vec2<f32>, size: f32) -> f32 {
	let d = abs(p.x) + abs(p.y);
	let aa = fwidth(d) + 0.001;
	let diamond = 1.0 - smoothstep(size - aa, size + aa, d);
	let cross = line_mask(p.x, size * 0.030) * (1.0 - smoothstep(size * 0.25, size, abs(p.y)))
		+ line_mask(p.y, size * 0.030) * (1.0 - smoothstep(size * 0.25, size, abs(p.x)));
	return clamp(diamond + cross * 0.45, 0.0, 1.0);
}

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
	let center = billboard.center_size.xyz;
	let size = max(billboard.center_size.w, 0.01);
	let to_camera = normalize(billboard.camera_pos.xyz - center);
	var right = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), to_camera));
	if dot(right, right) < 0.0001 {
		right = vec3<f32>(1.0, 0.0, 0.0);
	}
	let up = normalize(cross(to_camera, right));
	let world = center + right * local.x * size * 0.62 + up * local.y * size * 0.50;
	var out: VsOut;
	out.clip = billboard.view_proj * vec4<f32>(world, 1.0);
	out.local = local;
	return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
	let p = in.local;
	let t = billboard.time_params.x;
	let progress = clamp(billboard.time_params.y, 0.0, 1.0);
	let float_y = sin(t * 2.1) * 0.035;
	let q = p - vec2<f32>(0.0, float_y);
	let card = rounded_rect_fill(q, vec2<f32>(0.76, 0.62), 0.24);
	let edge = rounded_rect_line(q, vec2<f32>(0.76, 0.62), 0.24, 0.030);
	let inner = rounded_rect_line(q, vec2<f32>(0.57, 0.43), 0.18, 0.012);
	let aura = soft_disc(q * vec2<f32>(0.88, 1.16), 0.86);
	let stitch_a = sparkle(q - vec2<f32>(-0.58, 0.39 + sin(t * 1.4) * 0.025), 0.040);
	let stitch_b = sparkle(q - vec2<f32>(0.60, 0.38 + sin(t * 1.1 + 1.8) * 0.025), 0.038);
	let stitch_c = sparkle(q - vec2<f32>(-0.50, -0.42 + sin(t * 1.3 + 0.8) * 0.025), 0.032);
	let spin = cos(t * 3.4);
	let spin_width = mix(0.18, 1.0, abs(spin));
	let hp = vec2<f32>(q.x / spin_width, q.y);
	let hanger_hook = ring_mask(length((hp - vec2<f32>(0.0, 0.30)) * vec2<f32>(1.0, 1.42)), 0.105, 0.014)
		* smoothstep(-0.02, 0.18, hp.y - 0.30);
	let hanger_top = segment_mask(hp, vec2<f32>(0.0, 0.24), vec2<f32>(0.0, 0.12), 0.018);
	let hanger_l = segment_mask(hp, vec2<f32>(0.0, 0.12), vec2<f32>(-0.34, -0.05), 0.022);
	let hanger_r = segment_mask(hp, vec2<f32>(0.0, 0.12), vec2<f32>(0.34, -0.05), 0.022);
	let body = rounded_rect_fill(hp - vec2<f32>(0.0, -0.20), vec2<f32>(0.25, 0.26), 0.070)
		* (1.0 - rounded_rect_fill(hp - vec2<f32>(0.0, 0.02), vec2<f32>(0.095, 0.070), 0.030));
	let sleeve_l = segment_mask(hp, vec2<f32>(-0.20, -0.05), vec2<f32>(-0.39, -0.19), 0.070);
	let sleeve_r = segment_mask(hp, vec2<f32>(0.20, -0.05), vec2<f32>(0.39, -0.19), 0.070);
	let garment = clamp(hanger_hook + hanger_top + hanger_l + hanger_r + body + sleeve_l + sleeve_r, 0.0, 1.0) * card;
	let flip_glow = smoothstep(0.08, 0.40, 1.0 - abs(spin)) * card;
	let sweep = line_mask(q.y + q.x * 0.16 - 0.56 + fract(t * 0.42) * 1.12, 0.018) * card;
	let shine = line_mask(hp.x * 0.70 + hp.y * 0.18 - 0.40 + fract(t * 0.64) * 0.80, 0.035)
		* smoothstep(0.34, 0.05, abs(hp.x))
		* smoothstep(0.32, 0.02, abs(hp.y + 0.14))
		* card;
	let dot_wave = 0.5 + 0.5 * sin(t * 5.6);
	let dots = soft_disc(q - vec2<f32>(0.35, -0.43 + dot_wave * 0.038), 0.030)
		+ soft_disc(q - vec2<f32>(0.44, -0.43 + (0.5 + 0.5 * sin(t * 5.6 + 1.8)) * 0.038), 0.030)
		+ soft_disc(q - vec2<f32>(0.53, -0.43 + (0.5 + 0.5 * sin(t * 5.6 + 3.6)) * 0.038), 0.030);
	let cloth_shadow = rounded_rect_fill(hp - vec2<f32>(0.0, -0.30), vec2<f32>(0.34, 0.08), 0.08) * card;
	let progress_track = rounded_rect_fill(q - vec2<f32>(0.0, -0.53), vec2<f32>(0.48, 0.025), 0.025) * card;
	let progress_center = -0.48 + progress * 0.48;
	let progress_fill = rounded_rect_fill(q - vec2<f32>(progress_center, -0.53), vec2<f32>(max(progress * 0.48, 0.001), 0.025), 0.025) * card;
	let grid = line_mask(fract((q.x + q.y * 0.22 + t * 0.045) * 9.0) - 0.5, 0.010) * card * 0.16;
	let base = vec3<f32>(0.018, 0.026, 0.042) * card;
	let glass = vec3<f32>(0.070, 0.095, 0.155);
	let white = vec3<f32>(0.92, 1.00, 0.98);
	let cyan = vec3<f32>(0.28, 1.00, 0.96);
	let magenta = vec3<f32>(1.00, 0.34, 0.78);
	let violet = vec3<f32>(0.50, 0.40, 0.92);
	let glow = aura * 0.22 + edge * 0.95 + inner * 0.24 + garment * 1.30 + shine * 1.05 + flip_glow * 0.68 + sweep * 0.50 + dots * 0.95 + (stitch_a + stitch_b + stitch_c) * 0.82 + progress_fill * 0.70;
	let color = base
		+ glass * card * 0.92
		+ violet * grid
		+ white * (garment * 0.88 + cloth_shadow * 0.08)
		+ cyan * (edge * 0.62 + inner * 0.34 + shine * 0.76 + dots * 0.78 + sweep * 0.44 + progress_fill * 0.82)
		+ glass * progress_track * 0.36
		+ magenta * ((stitch_a + stitch_b + stitch_c) * 0.66 + flip_glow * 0.42)
		+ cyan * aura * 0.055;
	let alpha = clamp(aura * 0.08 + card * 0.76 + glow * 0.11 + progress_track * 0.10, 0.0, 0.92);
	return vec4<f32>(color, alpha);
}
