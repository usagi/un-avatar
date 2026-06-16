struct StartupSplash {
	time: f32,
	progress: f32,
	aspect: f32,
	phase: f32,
}

@group(0) @binding(0) var<uniform> splash: StartupSplash;

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

fn line_mask(value: f32, width: f32) -> f32 {
	let aa = fwidth(value) + 0.001;
	return 1.0 - smoothstep(width - aa, width + aa, abs(value));
}

fn ring_mask(r: f32, radius: f32, width: f32) -> f32 {
	return line_mask(r - radius, width);
}

fn soft_disc(p: vec2<f32>, radius: f32) -> f32 {
	let d = length(p);
	let aa = fwidth(d) + 0.001;
	return 1.0 - smoothstep(radius - aa, radius + aa, d);
}

fn wardrobe_splash_color(ndc: vec2<f32>, aspect: f32, t: f32) -> vec4<f32> {
	let side = smoothstep(0.46, 0.76, abs(ndc.x));
	let edge_fade = 1.0 - smoothstep(0.96, 1.04, abs(ndc.x));
	let vertical_fade = 1.0 - smoothstep(0.94, 1.08, abs(ndc.y));
	let curtain = side * edge_fade * vertical_fade;

	let sweep = fract((ndc.y + ndc.x * 0.38) * 8.0 - t * 1.65);
	let stripe = curtain * (1.0 - smoothstep(0.018, 0.052, abs(sweep - 0.5)));
	let shard = curtain * line_mask(ndc.x * 0.72 + ndc.y * 0.34 + sin(t * 2.0) * 0.09, 0.012);

	var gate_x = -0.68;
	if ndc.x >= 0.0 {
		gate_x = 0.68;
	}
	let local = vec2((ndc.x - gate_x) * aspect, ndc.y * 1.18);
	let r = length(local);
	let gate = ring_mask(r, 0.21 + 0.018 * sin(t * 4.2), 0.010);
	let gate_inner = ring_mask(r, 0.12, 0.004);
	let core = soft_disc(local, 0.050 + 0.010 * sin(t * 5.4));

	let scan_y = ndc.y + 0.62 - fract(t * 0.42) * 1.24;
	let scan = curtain * line_mask(scan_y, 0.012);
	let haze = smoothstep(0.18, 0.90, curtain) * (0.42 + 0.18 * sin(t * 2.7 + ndc.y * 5.0));

	let base = vec3(0.015, 0.020, 0.030);
	let cyan = vec3(0.18, 0.95, 1.00);
	let magenta = vec3(1.00, 0.28, 0.72);
	let gold = vec3(1.00, 0.68, 0.22);
	let accent = mix(cyan, magenta, 0.42 + 0.28 * sin(t * 1.7));
	let hot = mix(accent, gold, scan * 0.55 + gate * 0.35);
	let energy = haze * 0.38 + stripe * 0.46 + shard * 0.65 + gate * 1.25 + gate_inner * 1.05 + core * 1.45 + scan * 0.85;
	let center_dim = 0.035 * (1.0 - side) * (1.0 - smoothstep(0.88, 1.04, abs(ndc.y)));
	let color = base * (curtain * 0.55 + center_dim) + hot * energy;
	let alpha = clamp(curtain * 0.18 + center_dim + energy * 0.58, 0.0, 0.78);
	return vec4(color, alpha);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
	if splash.phase > 4.5 && splash.phase < 5.5 {
		return wardrobe_splash_color(in.ndc, splash.aspect, splash.time);
	}

	let uv = vec2(in.ndc.x * splash.aspect, in.ndc.y);
	let r = length(uv);
	let angle = atan2(uv.y, uv.x);
	let tau = 6.28318530718;
	let t = splash.time;
	let pulse = 0.5 + 0.5 * sin(t * 2.2);
	let determinate = splash.progress >= 0.0;
	let progress = clamp(splash.progress, 0.0, 1.0);
	let phase_shift = splash.phase * 0.43;

	let vignette = smoothstep(1.35, 0.12, r);
	let core = soft_disc(uv, 0.12 + pulse * 0.012);
	let outer = ring_mask(r, 0.33 + pulse * 0.012, 0.010);
	let inner = ring_mask(r, 0.22, 0.004);
	let scan = ring_mask(r, 0.44 + 0.035 * sin(t * 1.7 + angle * 3.0), 0.003);

	var arc = 0.0;
	if determinate {
		let a = fract((angle + 3.14159265) / tau);
		let leading = smoothstep(progress + 0.015, progress, a);
		let trailing = smoothstep(0.0, 0.02, a);
		arc = outer * leading * trailing;
	} else {
		let sweep = fract((angle / tau) + t * 0.18 + phase_shift);
		arc = outer * smoothstep(0.0, 0.08, sweep) * smoothstep(0.34, 0.12, sweep);
	}

	let orbit_angle = t * 1.65 + phase_shift;
	let orbit_pos = vec2(cos(orbit_angle), sin(orbit_angle)) * 0.33;
	let dot = soft_disc(uv - orbit_pos, 0.023);
	let second_pos = vec2(cos(-orbit_angle * 0.73 + 1.9), sin(-orbit_angle * 0.73 + 1.9)) * 0.22;
	let dot2 = soft_disc(uv - second_pos, 0.014);

	let bar_y = -0.49;
	let bar_x = in.ndc.x;
	let bar_body = (1.0 - smoothstep(0.015, 0.022, abs(in.ndc.y - bar_y))) * smoothstep(-0.46, -0.42, bar_x) * smoothstep(0.46, 0.42, bar_x);
	var fill_width = -0.42 + 0.84 * fract(t * 0.19);
	if determinate {
		fill_width = mix(-0.42, 0.42, progress);
	}
	let fill = bar_body * smoothstep(fill_width + 0.035, fill_width, bar_x);

	let base = vec3(0.02, 0.045, 0.055);
	let mint = vec3(0.10, 1.00, 0.73);
	let gold = vec3(1.00, 0.76, 0.28);
	let pink = vec3(1.00, 0.38, 0.72);
	let failed = splash.phase >= 8.0;
	let healthy_accent = mix(mint, mix(gold, pink, fract(splash.phase * 0.37)), 0.26 + 0.16 * sin(t + splash.phase));
	let accent = select(healthy_accent, vec3(1.0, 0.16, 0.25), failed);
	let energy = core * 0.55 + inner * 0.75 + outer * 0.35 + arc * 1.65 + scan * 0.45 + dot * 1.6 + dot2 * 1.1 + fill * 1.35;
	let color = base * vignette + accent * energy;
	let alpha = clamp(0.18 * vignette + energy * 0.72, 0.0, 0.92);
	return vec4(color, alpha);
}
