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
	let world = center + right * local.x * size * 0.92 + up * local.y * size * 0.54;
	var out: VsOut;
	out.clip = billboard.view_proj * vec4<f32>(world, 1.0);
	out.local = local;
	return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
	let p = in.local;
	let t = billboard.time_params.x;
	let card = rounded_rect_fill(p, vec2<f32>(0.96, 0.82), 0.20);
	let edge = rounded_rect_line(p, vec2<f32>(0.96, 0.82), 0.20, 0.030);
	let inner = rounded_rect_line(p, vec2<f32>(0.76, 0.56), 0.16, 0.010);
	let spin = cos(t * 3.1);
	let spin_width = mix(0.22, 1.0, abs(spin));
	let hp = vec2<f32>(p.x / spin_width, p.y);
	let hanger_hook = ring_mask(length((hp - vec2<f32>(0.0, 0.34)) * vec2<f32>(1.0, 1.45)), 0.12, 0.014)
		* smoothstep(-0.02, 0.18, hp.y - 0.34);
	let hanger_top = segment_mask(hp, vec2<f32>(0.0, 0.28), vec2<f32>(0.0, 0.16), 0.018);
	let hanger_l = segment_mask(hp, vec2<f32>(0.0, 0.16), vec2<f32>(-0.40, 0.00), 0.024);
	let hanger_r = segment_mask(hp, vec2<f32>(0.0, 0.16), vec2<f32>(0.40, 0.00), 0.024);
	let body = rounded_rect_fill(hp - vec2<f32>(0.0, -0.14), vec2<f32>(0.30, 0.27), 0.065)
		* (1.0 - rounded_rect_fill(hp - vec2<f32>(0.0, 0.09), vec2<f32>(0.11, 0.070), 0.030));
	let sleeve_l = segment_mask(hp, vec2<f32>(-0.25, 0.04), vec2<f32>(-0.46, -0.12), 0.080);
	let sleeve_r = segment_mask(hp, vec2<f32>(0.25, 0.04), vec2<f32>(0.46, -0.12), 0.080);
	let garment = clamp(hanger_hook + hanger_top + hanger_l + hanger_r + body + sleeve_l + sleeve_r, 0.0, 1.0) * card;
	let shine = line_mask(hp.x * 0.72 + hp.y * 0.18 - 0.48 + fract(t * 0.58) * 0.96, 0.040)
		* smoothstep(0.42, 0.05, abs(hp.x))
		* smoothstep(0.38, 0.02, abs(hp.y + 0.09))
		* card;
	let flip_glow = smoothstep(0.10, 0.42, 1.0 - abs(spin)) * card;
	let sweep = line_mask(p.y + p.x * 0.18 - 0.72 + fract(t * 0.36) * 1.44, 0.020) * card;
	let dot_wave = 0.5 + 0.5 * sin(t * 5.4);
	let dots = soft_disc(p - vec2<f32>(0.46, -0.58 + dot_wave * 0.045), 0.033)
		+ soft_disc(p - vec2<f32>(0.56, -0.58 + (0.5 + 0.5 * sin(t * 5.4 + 1.7)) * 0.045), 0.033)
		+ soft_disc(p - vec2<f32>(0.66, -0.58 + (0.5 + 0.5 * sin(t * 5.4 + 3.4)) * 0.045), 0.033);
	let sparkles = sparkle(p - vec2<f32>(-0.66, 0.42 + sin(t * 1.7) * 0.040), 0.060)
		+ sparkle(p - vec2<f32>(0.72, 0.36 + sin(t * 1.3 + 1.2) * 0.040), 0.052);
	let base = vec3<f32>(0.055, 0.080, 0.110) * card;
	let mint = vec3<f32>(0.45, 1.00, 0.90);
	let peach = vec3<f32>(1.00, 0.56, 0.78);
	let sky = vec3<f32>(0.46, 0.86, 1.00);
	let glow = edge * 1.0 + inner * 0.22 + garment * 1.30 + shine * 1.45 + flip_glow * 0.95 + sweep * 0.45 + dots * 1.10 + sparkles * 0.8;
	let color = base + mint * (edge * 0.60 + garment * 0.75 + shine * 0.90 + dots * 0.90)
		+ peach * (sparkles * 0.85 + flip_glow * 0.45)
		+ sky * (inner * 0.45 + sweep * 0.65);
	let alpha = clamp(card * 0.86 + glow * 0.14, 0.0, 1.0);
	return vec4<f32>(color, alpha);
}
