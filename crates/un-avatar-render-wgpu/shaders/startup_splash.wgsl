struct StartupSplash {
	time: f32,
	progress: f32,
	aspect: f32,
	phase: f32,
	rect_center: vec2<f32>,
	rect_half_size: vec2<f32>,
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

fn sparkle(p: vec2<f32>, size: f32) -> f32 {
	let d = abs(p.x) + abs(p.y);
	let aa = fwidth(d) + 0.001;
	let diamond = 1.0 - smoothstep(size - aa, size + aa, d);
	let cross = line_mask(p.x, size * 0.030) * (1.0 - smoothstep(size * 0.25, size, abs(p.y)))
		+ line_mask(p.y, size * 0.030) * (1.0 - smoothstep(size * 0.25, size, abs(p.x)));
	return clamp(diamond + cross * 0.45, 0.0, 1.0);
}

fn segment_distance(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
	let ab = b - a;
	let h = clamp(dot(p - a, ab) / max(dot(ab, ab), 0.00001), 0.0, 1.0);
	return length(p - (a + ab * h));
}

fn segment_mask(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>, width: f32) -> f32 {
	let d = segment_distance(p, a, b);
	let aa = fwidth(d) + 0.001;
	return 1.0 - smoothstep(width - aa, width + aa, d);
}

fn glyph_stroke(p: vec2<f32>, glyph: f32) -> f32 {
	let top = segment_mask(p, vec2(-0.36, 0.42), vec2(0.36, 0.42), 0.045);
	let mid = segment_mask(p, vec2(-0.28, 0.00), vec2(0.30, 0.00), 0.045);
	let bottom = segment_mask(p, vec2(-0.36, -0.42), vec2(0.36, -0.42), 0.045);
	let left = segment_mask(p, vec2(-0.36, -0.38), vec2(-0.36, 0.38), 0.045);
	let right = segment_mask(p, vec2(0.36, -0.38), vec2(0.36, 0.38), 0.045);
	let center = segment_mask(p, vec2(0.0, -0.40), vec2(0.0, 0.40), 0.045);
	let diag = segment_mask(p, vec2(-0.32, 0.38), vec2(0.32, -0.38), 0.040);
	let lower_right = segment_mask(p, vec2(0.36, -0.38), vec2(0.36, -0.02), 0.045);
	let spur = segment_mask(p, vec2(0.02, -0.02), vec2(0.36, -0.02), 0.045);
	if glyph < 0.5 {
		return clamp(top + bottom + left, 0.0, 1.0);
	}
	if glyph < 1.5 {
		return clamp(left + right + mid, 0.0, 1.0);
	}
	if glyph < 2.5 {
		return clamp(top + left + right + mid, 0.0, 1.0);
	}
	if glyph < 3.5 {
		return clamp(left + right + diag, 0.0, 1.0);
	}
	if glyph < 4.5 {
		return clamp(top + bottom + left + lower_right + spur, 0.0, 1.0);
	}
	return clamp(top + bottom + center, 0.0, 1.0);
}

fn changing_word(local_ndc: vec2<f32>) -> f32 {
	let scale = vec2(0.105, 0.210);
	let base_y = -0.48;
	var m = 0.0;
	for (var i = 0; i < 8; i = i + 1) {
		let x = (f32(i) - 3.5) * 0.150;
		let p = (local_ndc - vec2(x, base_y)) / scale;
		var glyph = 0.0;
		if i == 1 {
			glyph = 1.0;
		} else if i == 2 {
			glyph = 2.0;
		} else if i == 3 || i == 5 {
			glyph = 3.0;
		} else if i == 4 || i == 7 {
			glyph = 4.0;
		} else if i == 6 {
			glyph = 5.0;
		}
		m = max(m, glyph_stroke(p, glyph));
	}
	return m;
}

fn ping_pong01(value: f32) -> f32 {
	let f = fract(value);
	return 1.0 - abs(f * 2.0 - 1.0);
}

fn wardrobe_splash_color(ndc: vec2<f32>, aspect: f32, t: f32, rect_center: vec2<f32>, rect_half_size: vec2<f32>) -> vec4<f32> {
	let rect_half = max(rect_half_size, vec2(0.18, 0.24));
	let local_ndc = (ndc - rect_center) / rect_half;
	let p = vec2(local_ndc.x * aspect * rect_half.x, local_ndc.y * rect_half.y);
	let rect_p = vec2((ndc.x - rect_center.x) * aspect, ndc.y - rect_center.y);
	let rect_half_p = vec2(rect_half.x * aspect, rect_half.y);
	let rect_dist = rounded_rect_distance(rect_p, rect_half_p, 0.14);
	let rect_mask = 1.0 - smoothstep(0.0, 0.055, rect_dist);
	let feather = 1.0 - smoothstep(0.06, 0.22, rect_dist);
	let y01 = clamp(local_ndc.y * 0.5 + 0.5, 0.0, 1.0);
	let vignette = 1.0 - smoothstep(0.35, 1.25, length(p));

	let ink = vec3(0.025, 0.035, 0.060);
	let twilight = vec3(0.105, 0.065, 0.165);
	let base = mix(ink, twilight, y01);
	let peach = vec3(1.00, 0.54, 0.70);
	let mint = vec3(0.38, 1.00, 0.83);
	let sky = vec3(0.36, 0.78, 1.00);
	let cream = vec3(1.00, 0.86, 0.58);

	let booth = rounded_rect_fill(rect_p, rect_half_p * 0.96, 0.16) * rect_mask;
	let booth_edge = rounded_rect_line(rect_p, rect_half_p * 0.98, 0.17, 0.014);
	let inner = rounded_rect_line(rect_p, rect_half_p * 0.74, 0.13, 0.004) * rect_mask;
	let stage = soft_disc(vec2(local_ndc.x * 0.58, local_ndc.y * 1.25 + 0.42), 0.28);
	let orbit_p = vec2(local_ndc.x * 0.88, local_ndc.y * 1.12 - 0.02);
	let orbit_r = length(orbit_p);
	let orbit_angle = atan2(orbit_p.y, orbit_p.x);
	let tau = 6.28318530718;
	let orbit_sweep = fract(orbit_angle / tau + t * 0.42);
	let orbit_arc = ring_mask(orbit_r, 0.56 + 0.018 * sin(t * 2.6), 0.012)
		* smoothstep(0.00, 0.10, orbit_sweep)
		* smoothstep(0.36, 0.12, orbit_sweep)
		* rect_mask;

	let hanger_hook = ring_mask(length((local_ndc - vec2(0.0, 0.40)) * vec2(1.0, 1.45)), 0.105, 0.012)
		* smoothstep(-0.02, 0.18, local_ndc.y - 0.40);
	let hanger_top = segment_mask(local_ndc, vec2(0.0, 0.34), vec2(0.0, 0.22), 0.014);
	let hanger_l = segment_mask(local_ndc, vec2(0.0, 0.22), vec2(-0.34, 0.06), 0.018);
	let hanger_r = segment_mask(local_ndc, vec2(0.0, 0.22), vec2(0.34, 0.06), 0.018);
	let shirt_body = rounded_rect_fill(local_ndc - vec2(0.0, -0.07), vec2(0.25, 0.25), 0.055)
		* (1.0 - rounded_rect_fill(local_ndc - vec2(0.0, 0.18), vec2(0.095, 0.065), 0.030));
	let sleeve_l = segment_mask(local_ndc, vec2(-0.24, 0.10), vec2(-0.42, -0.04), 0.070);
	let sleeve_r = segment_mask(local_ndc, vec2(0.24, 0.10), vec2(0.42, -0.04), 0.070);
	let garment = clamp(hanger_hook + hanger_top + hanger_l + hanger_r + shirt_body + sleeve_l + sleeve_r, 0.0, 1.0) * rect_mask;

	let word = changing_word(local_ndc) * rect_mask * (0.78 + 0.22 * sin(t * 5.0));
	let dot1_bounce = ping_pong01(t * 1.9 + 0.00);
	let dot2_bounce = ping_pong01(t * 1.9 + 0.18);
	let dot3_bounce = ping_pong01(t * 1.9 + 0.36);
	let dot1 = soft_disc(local_ndc - vec2(0.43, -0.50 + dot1_bounce * 0.045), 0.025) * (0.35 + dot1_bounce * 0.90);
	let dot2 = soft_disc(local_ndc - vec2(0.50, -0.50 + dot2_bounce * 0.045), 0.025) * (0.35 + dot2_bounce * 0.90);
	let dot3 = soft_disc(local_ndc - vec2(0.57, -0.50 + dot3_bounce * 0.045), 0.025) * (0.35 + dot3_bounce * 0.90);
	let dots = (dot1 + dot2 + dot3) * rect_mask;

	let sweep = line_mask(local_ndc.y + local_ndc.x * 0.16 - 0.72 + fract(t * 0.36) * 1.44, 0.020) * rect_mask;
	let garment_shine = line_mask(local_ndc.x * 0.72 + local_ndc.y * 0.18 - 0.48 + fract(t * 0.58) * 0.96, 0.035)
		* smoothstep(0.40, 0.05, abs(local_ndc.x))
		* smoothstep(0.35, 0.02, abs(local_ndc.y + 0.04))
		* rect_mask;
	let sparkle_a = sparkle(local_ndc - vec2(-0.58, 0.42 + sin(t * 1.7) * 0.035), 0.055);
	let sparkle_b = sparkle(local_ndc - vec2(0.62, 0.30 + sin(t * 1.3 + 1.2) * 0.035), 0.047);
	let sparkle_c = sparkle(local_ndc - vec2(-0.48, -0.30 + sin(t * 1.9 + 2.1) * 0.030), 0.040);
	let sparkle_d = sparkle(local_ndc - vec2(0.50, -0.26 + sin(t * 1.5 + 0.8) * 0.030), 0.045);
	let sparkle_e = sparkle(local_ndc - vec2(0.00, 0.58 + sin(t * 1.1) * 0.025), 0.033);
	let sparkles = sparkle_a + sparkle_b + sparkle_c + sparkle_d + sparkle_e;

	let pulse = 0.5 + 0.5 * sin(t * 3.0);
	let halo = soft_disc(vec2(local_ndc.x * 0.76, local_ndc.y * 1.10 - 0.04), 0.34 + pulse * 0.028) * rect_mask;
	let lace = line_mask(fract((local_ndc.x * 0.42 - local_ndc.y * 0.30) * 8.0 + t * 0.24) - 0.5, 0.018) * booth * 0.12;

	let glow = stage * 0.18 + halo * 0.25 + orbit_arc * 1.15 + booth_edge * 0.85 + inner * 0.20 + garment * 1.20 + garment_shine * 1.40 + word * 1.35 + dots * 1.15 + sweep * 0.42 + sparkles * 0.80 + lace * 0.18;
	let pastel = mint * (booth_edge * 0.55 + garment * 0.70 + garment_shine * 0.90 + sparkle_b * 0.80 + dots * 0.95)
		+ peach * (word * 0.78 + sparkle_a * 0.75 + sparkle_c * 0.70)
		+ sky * (hanger_hook * 0.80 + inner * 0.35 + sweep * 0.45 + orbit_arc * 0.85)
		+ cream * (sparkle_e * 0.95 + stage * 0.10);
	let color = base * (0.54 + vignette * 0.18) * rect_mask + vec3(0.040, 0.020, 0.060) * booth + pastel + vec3(0.06, 0.07, 0.12) * glow;
	let alpha = clamp((0.88 * rect_mask + glow * 0.12) * feather, 0.0, 1.0);
	return vec4(color, alpha);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
	if splash.phase > 4.5 && splash.phase < 5.5 {
		return wardrobe_splash_color(in.ndc, splash.aspect, splash.time, splash.rect_center, splash.rect_half_size);
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
