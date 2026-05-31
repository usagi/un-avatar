//! 肌色合わせ用の CPU 画像解析と補正。

use std::collections::BTreeMap;

use glam::{Mat4, Vec3};
use serde::Serialize;
use un_avatar_core::{UnaMaterialPbr, UnaSceneSnapshot};

use crate::debug_dump::eye_area_material_name;
use crate::texture_pipeline::normalized_rgba_base;

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct SkinToneMatchingDebug {
	pub enabled: bool,
	pub face_seam_uv_count: usize,
	pub body_seam_uv_count: usize,
	pub face_sample: Option<[f32; 3]>,
	pub body_sample: Option<[f32; 3]>,
	pub target: Option<[f32; 3]>,
	pub records: Vec<SkinToneMatchingRecord>,
	pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct SkinToneMatchingRecord {
	pub image_index: usize,
	pub kind: &'static str,
	pub width: u32,
	pub height: u32,
	pub source_lab: [f32; 3],
	#[serde(skip_serializing_if = "Option::is_none")]
	pub adjusted_lab: Option<[f32; 3]>,
	pub adjusted_pixels: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SkinToneTextureKind {
	Face,
	Body,
	Mixed,
}

#[derive(Clone, Copy, Debug)]
struct Lab {
	l: f32,
	a: f32,
	b: f32,
}

#[derive(Clone, Copy, Debug)]
struct SkinToneUvCandidate {
	image_index: usize,
	pos: Vec3,
	uv: [f32; 2],
}

fn merge_skin_tone_kind(current: Option<SkinToneTextureKind>, next: SkinToneTextureKind) -> Option<SkinToneTextureKind> {
	match current {
		None => Some(next),
		Some(kind) if kind == next => Some(kind),
		Some(_) => Some(SkinToneTextureKind::Mixed),
	}
}

pub(crate) fn material_skin_tone_kind(mat: &UnaMaterialPbr) -> Option<SkinToneTextureKind> {
	let name = mat.name.as_deref()?;
	let name = name.to_ascii_lowercase();
	if eye_area_material_name(Some(name.as_str()))
		|| name.contains("eye")
		|| name.contains("iris")
		|| name.contains("pupil")
		|| name.contains("hair")
		|| name.contains("mouth")
		|| name.contains("lip")
		|| name.contains("tooth")
		|| name.contains("teeth")
		|| name.contains("tongue")
		|| name.contains("gum")
		|| name.contains("目")
		|| name.contains("瞳")
		|| name.contains("髪")
		|| name.contains("口")
		|| name.contains("唇")
		|| name.contains("歯")
		|| name.contains("舌")
	{
		return None;
	}
	if name.contains("face")
		|| name.contains("head")
		|| name.contains("顔")
		|| name.contains("頭")
		|| name.contains("脸")
		|| name.contains("臉")
		|| name.contains("面部")
	{
		return Some(SkinToneTextureKind::Face);
	}
	if name.contains("body")
		|| name.contains("skin")
		|| name.contains("neck")
		|| name.contains("torso")
		|| name.contains("chest")
		|| name.contains("arm")
		|| name.contains("leg")
		|| name.contains("hand")
		|| name.contains("foot")
		|| name.contains("肌")
		|| name.contains("皮膚")
		|| name.contains("体")
		|| name.contains("身体")
		|| name.contains("首")
		|| name.contains("胴")
		|| name.contains("胸")
		|| name.contains("腕")
		|| name.contains("脚")
		|| name.contains("手")
		|| name.contains("足")
	{
		return Some(SkinToneTextureKind::Body);
	}
	None
}

pub(crate) fn skin_tone_texture_kinds_for_scene(scene: &UnaSceneSnapshot) -> Vec<Option<SkinToneTextureKind>> {
	let mut kinds = vec![None; scene.images.len()];
	for mat in &scene.materials {
		let Some(texture_index) = mat.base_color_texture_index else {
			continue;
		};
		let Some(kind) = material_skin_tone_kind(mat) else { continue };
		let Some(slot) = kinds.get_mut(texture_index) else { continue };
		*slot = merge_skin_tone_kind(*slot, kind);
	}
	kinds
}

fn srgb8_to_linear(v: u8) -> f32 {
	let v = f32::from(v) / 255.0;
	if v <= 0.04045 {
		v / 12.92
	} else {
		((v + 0.055) / 1.055).powf(2.4)
	}
}

fn linear_to_srgb8(v: f32) -> u8 {
	let v = v.clamp(0.0, 1.0);
	let srgb = if v <= 0.0031308 {
		v * 12.92
	} else {
		1.055 * v.powf(1.0 / 2.4) - 0.055
	};
	(srgb.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

fn xyz_to_lab_f(t: f32) -> f32 {
	const DELTA: f32 = 6.0 / 29.0;
	if t > DELTA * DELTA * DELTA {
		t.cbrt()
	} else {
		t / (3.0 * DELTA * DELTA) + 4.0 / 29.0
	}
}

fn lab_to_xyz_f(t: f32) -> f32 {
	const DELTA: f32 = 6.0 / 29.0;
	if t > DELTA {
		t * t * t
	} else {
		3.0 * DELTA * DELTA * (t - 4.0 / 29.0)
	}
}

fn rgb8_to_lab(r: u8, g: u8, b: u8) -> Lab {
	let r = srgb8_to_linear(r);
	let g = srgb8_to_linear(g);
	let b = srgb8_to_linear(b);
	let x = 0.4124564 * r + 0.3575761 * g + 0.1804375 * b;
	let y = 0.2126729 * r + 0.7151522 * g + 0.0721750 * b;
	let z = 0.0193339 * r + 0.119_192 * g + 0.9503041 * b;
	let fx = xyz_to_lab_f(x / 0.95047);
	let fy = xyz_to_lab_f(y);
	let fz = xyz_to_lab_f(z / 1.08883);
	Lab {
		l: 116.0 * fy - 16.0,
		a: 500.0 * (fx - fy),
		b: 200.0 * (fy - fz),
	}
}

fn lab_to_rgb8(lab: Lab) -> [u8; 3] {
	let fy = (lab.l + 16.0) / 116.0;
	let fx = fy + lab.a / 500.0;
	let fz = fy - lab.b / 200.0;
	let x = 0.95047 * lab_to_xyz_f(fx);
	let y = lab_to_xyz_f(fy);
	let z = 1.08883 * lab_to_xyz_f(fz);
	let r = 3.2404542 * x - 1.5371385 * y - 0.4985314 * z;
	let g = -0.969_266 * x + 1.8760108 * y + 0.0415560 * z;
	let b = 0.0556434 * x - 0.2040259 * y + 1.0572252 * z;
	[linear_to_srgb8(r), linear_to_srgb8(g), linear_to_srgb8(b)]
}

fn lab_distance(a: Lab, b: Lab) -> f32 {
	let dl = a.l - b.l;
	let da = a.a - b.a;
	let db = a.b - b.b;
	(dl * dl + da * da + db * db).sqrt()
}

fn lab_to_array(lab: Lab) -> [f32; 3] {
	[lab.l, lab.a, lab.b]
}

fn likely_skin_lab(lab: Lab) -> bool {
	(35.0..=96.0).contains(&lab.l) && (-8.0..=34.0).contains(&lab.a) && (-2.0..=45.0).contains(&lab.b)
}

fn median_channel(values: &mut [f32]) -> f32 {
	values.sort_by(|a, b| a.total_cmp(b));
	values[values.len() / 2]
}

fn estimate_skin_lab(rgba: &[u8], width: u32, height: u32) -> Option<Lab> {
	let pixel_count = (width.max(1) as usize).saturating_mul(height.max(1) as usize);
	let step = (pixel_count / 4096).max(1);
	let sample_capacity = pixel_count.div_ceil(step).min(4096);
	let mut ls = Vec::with_capacity(sample_capacity);
	let mut as_ = Vec::with_capacity(sample_capacity);
	let mut bs = Vec::with_capacity(sample_capacity);
	for px in rgba.chunks_exact(4).step_by(step) {
		if px[3] < 192 {
			continue;
		}
		let lab = rgb8_to_lab(px[0], px[1], px[2]);
		if likely_skin_lab(lab) {
			ls.push(lab.l);
			as_.push(lab.a);
			bs.push(lab.b);
		}
	}
	if ls.len() < 16 {
		return None;
	}
	Some(Lab {
		l: median_channel(&mut ls),
		a: median_channel(&mut as_),
		b: median_channel(&mut bs),
	})
}

fn uv_to_pixel(uv: [f32; 2], width: u32, height: u32) -> (i32, i32) {
	let w = width.max(1) as f32;
	let h = height.max(1) as f32;
	let u = uv[0].rem_euclid(1.0);
	let v = uv[1].rem_euclid(1.0);
	let x = (u * (w - 1.0)).round() as i32;
	let y = ((1.0 - v) * (h - 1.0)).round() as i32;
	(x, y)
}

fn sample_skin_lab_near_uv(rgba: &[u8], width: u32, height: u32, uvs: &[[f32; 2]]) -> Option<Lab> {
	if uvs.is_empty() {
		return None;
	}
	let width = width.max(1);
	let height = height.max(1);
	let sample_capacity = uvs.len().min(128).saturating_mul(29);
	let mut ls = Vec::with_capacity(sample_capacity);
	let mut as_ = Vec::with_capacity(sample_capacity);
	let mut bs = Vec::with_capacity(sample_capacity);
	for &uv in uvs.iter().take(128) {
		let (cx, cy) = uv_to_pixel(uv, width, height);
		for dy in -3..=3 {
			for dx in -3..=3 {
				if dx * dx + dy * dy > 10 {
					continue;
				}
				let x = (cx + dx).clamp(0, width as i32 - 1) as u32;
				let y = (cy + dy).clamp(0, height as i32 - 1) as u32;
				let i = ((y * width + x) as usize) * 4;
				let Some(px) = rgba.get(i..i + 4) else { continue };
				if px[3] < 192 {
					continue;
				}
				let lab = rgb8_to_lab(px[0], px[1], px[2]);
				if likely_skin_lab(lab) {
					ls.push(lab.l);
					as_.push(lab.a);
					bs.push(lab.b);
				}
			}
		}
	}
	if ls.len() < 8 {
		return None;
	}
	Some(Lab {
		l: median_channel(&mut ls),
		a: median_channel(&mut as_),
		b: median_channel(&mut bs),
	})
}

fn central_span(candidates: &[SkinToneUvCandidate]) -> (f32, f32, f32, f32, f32, f32) {
	let mut min_x = f32::INFINITY;
	let mut max_x = f32::NEG_INFINITY;
	let mut min_y = f32::INFINITY;
	let mut max_y = f32::NEG_INFINITY;
	let mut min_z = f32::INFINITY;
	let mut max_z = f32::NEG_INFINITY;
	for candidate in candidates {
		min_x = min_x.min(candidate.pos.x);
		max_x = max_x.max(candidate.pos.x);
		min_y = min_y.min(candidate.pos.y);
		max_y = max_y.max(candidate.pos.y);
		min_z = min_z.min(candidate.pos.z);
		max_z = max_z.max(candidate.pos.z);
	}
	(min_x, max_x, min_y, max_y, min_z, max_z)
}

fn select_face_lower_uvs(candidates: &[SkinToneUvCandidate]) -> Vec<(usize, [f32; 2])> {
	if candidates.is_empty() {
		return Vec::new();
	}
	let (min_x, max_x, min_y, max_y, min_z, max_z) = central_span(candidates);
	let x_center = (min_x + max_x) * 0.5;
	let z_center = (min_z + max_z) * 0.5;
	let x_limit = ((max_x - min_x) * 0.32).max(0.03);
	let z_limit = ((max_z - min_z) * 0.60).max(0.03);
	let y_limit = min_y + (max_y - min_y).max(0.001) * 0.24;
	let mut selected: Vec<_> = candidates
		.iter()
		.filter(|candidate| {
			candidate.pos.y <= y_limit && (candidate.pos.x - x_center).abs() <= x_limit && (candidate.pos.z - z_center).abs() <= z_limit
		})
		.map(|candidate| (candidate.image_index, candidate.uv))
		.collect();
	if selected.is_empty() {
		selected = candidates
			.iter()
			.filter(|candidate| candidate.pos.y <= y_limit)
			.map(|candidate| (candidate.image_index, candidate.uv))
			.collect();
	}
	selected
}

fn select_body_upper_uvs(candidates: &[SkinToneUvCandidate]) -> Vec<(usize, [f32; 2])> {
	if candidates.is_empty() {
		return Vec::new();
	}
	let (min_x, max_x, min_y, max_y, min_z, max_z) = central_span(candidates);
	let x_center = (min_x + max_x) * 0.5;
	let z_center = (min_z + max_z) * 0.5;
	let x_limit = ((max_x - min_x) * 0.26).max(0.035);
	let z_limit = ((max_z - min_z) * 0.70).max(0.035);
	let y_limit = max_y - (max_y - min_y).max(0.001) * 0.18;
	let mut selected: Vec<_> = candidates
		.iter()
		.filter(|candidate| {
			candidate.pos.y >= y_limit && (candidate.pos.x - x_center).abs() <= x_limit && (candidate.pos.z - z_center).abs() <= z_limit
		})
		.map(|candidate| (candidate.image_index, candidate.uv))
		.collect();
	if selected.is_empty() {
		selected = candidates
			.iter()
			.filter(|candidate| candidate.pos.y >= y_limit)
			.map(|candidate| (candidate.image_index, candidate.uv))
			.collect();
	}
	selected
}

type SkinToneUvSamplesByImage = BTreeMap<usize, Vec<[f32; 2]>>;

fn collect_skin_tone_seam_uvs(scene: &UnaSceneSnapshot, world: &[Mat4]) -> (SkinToneUvSamplesByImage, SkinToneUvSamplesByImage) {
	let mut face_candidates = Vec::new();
	let mut body_candidates = Vec::new();
	for (node_index, node) in scene.nodes.iter().enumerate() {
		let Some(mesh_index) = node.mesh else { continue };
		let Some(primitives) = scene.meshes.get(mesh_index) else { continue };
		let node_world = world.get(node_index).copied().unwrap_or(Mat4::IDENTITY);
		for primitive in primitives {
			let Some(uvs) = primitive.tex_coords_0.as_deref() else { continue };
			let Some(material_index) = primitive.material_index else { continue };
			let Some(material) = scene.materials.get(material_index) else {
				continue;
			};
			let Some(texture_index) = material.base_color_texture_index else {
				continue;
			};
			let Some(kind) = material_skin_tone_kind(material) else { continue };
			for (i, pos) in primitive.positions.iter().enumerate() {
				let Some(uv) = uvs.get(i).copied() else { continue };
				let candidate = SkinToneUvCandidate {
					image_index: texture_index,
					pos: node_world.transform_point3(Vec3::from_array(*pos)),
					uv,
				};
				match kind {
					SkinToneTextureKind::Face => face_candidates.push(candidate),
					SkinToneTextureKind::Body => body_candidates.push(candidate),
					SkinToneTextureKind::Mixed => {}
				}
			}
		}
	}
	let mut face_uvs: BTreeMap<usize, Vec<[f32; 2]>> = BTreeMap::new();
	for (image_index, uv) in select_face_lower_uvs(&face_candidates) {
		face_uvs.entry(image_index).or_default().push(uv);
	}
	let mut body_uvs: BTreeMap<usize, Vec<[f32; 2]>> = BTreeMap::new();
	for (image_index, uv) in select_body_upper_uvs(&body_candidates) {
		body_uvs.entry(image_index).or_default().push(uv);
	}
	(face_uvs, body_uvs)
}

fn apply_skin_tone_shift(rgba: &[u8], width: u32, height: u32, source: Lab, target: Lab, strength: Lab) -> (Vec<u8>, u32) {
	let mut out = normalized_rgba_base(rgba, width.max(1), height.max(1));
	let mut adjusted_pixels = 0u32;
	let delta = Lab {
		l: (target.l - source.l) * strength.l,
		a: (target.a - source.a) * strength.a,
		b: (target.b - source.b) * strength.b,
	};
	for px in out.chunks_exact_mut(4) {
		if px[3] < 192 {
			continue;
		}
		let lab = rgb8_to_lab(px[0], px[1], px[2]);
		if !likely_skin_lab(lab) {
			continue;
		}
		let distance = lab_distance(lab, source);
		let weight = (1.0 - distance / 56.0).clamp(0.28, 1.0);
		if weight <= 0.0 {
			continue;
		}
		let adjusted = Lab {
			l: (lab.l + delta.l * weight).clamp(0.0, 100.0),
			a: lab.a + delta.a * weight,
			b: lab.b + delta.b * weight,
		};
		let rgb = lab_to_rgb8(adjusted);
		px[0] = rgb[0];
		px[1] = rgb[1];
		px[2] = rgb[2];
		adjusted_pixels = adjusted_pixels.saturating_add(1);
	}
	(out, adjusted_pixels)
}

pub(crate) fn build_skin_tone_matched_images(
	scene: &UnaSceneSnapshot,
	world: &[Mat4],
	kinds: &[Option<SkinToneTextureKind>],
) -> (Vec<Option<Vec<u8>>>, SkinToneMatchingDebug) {
	let (face_seam_uvs, body_seam_uvs) = collect_skin_tone_seam_uvs(scene, world);
	let face_seam_uv_count = face_seam_uvs.values().map(Vec::len).sum();
	let body_seam_uv_count = body_seam_uvs.values().map(Vec::len).sum();
	let mut debug = SkinToneMatchingDebug {
		enabled: true,
		face_seam_uv_count,
		body_seam_uv_count,
		..Default::default()
	};
	let mut face_samples = Vec::new();
	let mut body_samples = Vec::new();
	for (index, im) in scene.images.iter().enumerate() {
		let rgba = im.rgba8_compat_pixels();
		match kinds.get(index).copied().flatten() {
			Some(SkinToneTextureKind::Face) => {
				let sample = face_seam_uvs
					.get(&index)
					.and_then(|uvs| sample_skin_lab_near_uv(rgba.as_ref(), im.width, im.height, uvs))
					.or_else(|| estimate_skin_lab(rgba.as_ref(), im.width, im.height));
				if let Some(sample) = sample {
					face_samples.push((index, sample));
				}
			}
			Some(SkinToneTextureKind::Body) => {
				let sample = body_seam_uvs
					.get(&index)
					.and_then(|uvs| sample_skin_lab_near_uv(rgba.as_ref(), im.width, im.height, uvs))
					.or_else(|| estimate_skin_lab(rgba.as_ref(), im.width, im.height));
				if let Some(sample) = sample {
					body_samples.push((index, sample));
				}
			}
			Some(SkinToneTextureKind::Mixed) | None => {}
		}
	}
	let Some(face) = face_samples.first().map(|(_, sample)| *sample) else {
		debug.note = Some("face sample not found".to_string());
		return (vec![None; scene.images.len()], debug);
	};
	let Some(body) = body_samples.first().map(|(_, sample)| *sample) else {
		debug.face_sample = Some(lab_to_array(face));
		debug.note = Some("body sample not found".to_string());
		return (vec![None; scene.images.len()], debug);
	};
	let target = Lab {
		l: face.l * 0.78 + body.l * 0.22,
		a: face.a * 0.78 + body.a * 0.22,
		b: face.b * 0.78 + body.b * 0.22,
	};
	debug.face_sample = Some(lab_to_array(face));
	debug.body_sample = Some(lab_to_array(body));
	debug.target = Some(lab_to_array(target));
	let mut adjusted = vec![None; scene.images.len()];
	for (index, source) in face_samples {
		let im = &scene.images[index];
		let source_rgba = im.rgba8_compat_pixels();
		let (rgba, adjusted_pixels) = apply_skin_tone_shift(
			source_rgba.as_ref(),
			im.width,
			im.height,
			source,
			target,
			Lab { l: 0.18, a: 0.35, b: 0.35 },
		);
		let adjusted_lab = face_seam_uvs
			.get(&index)
			.and_then(|uvs| sample_skin_lab_near_uv(&rgba, im.width, im.height, uvs))
			.or_else(|| estimate_skin_lab(&rgba, im.width, im.height))
			.map(lab_to_array);
		debug.records.push(SkinToneMatchingRecord {
			image_index: index,
			kind: "face",
			width: im.width,
			height: im.height,
			source_lab: lab_to_array(source),
			adjusted_lab,
			adjusted_pixels,
		});
		adjusted[index] = Some(rgba);
	}
	for (index, source) in body_samples {
		let im = &scene.images[index];
		let source_rgba = im.rgba8_compat_pixels();
		let (rgba, adjusted_pixels) = apply_skin_tone_shift(
			source_rgba.as_ref(),
			im.width,
			im.height,
			source,
			target,
			Lab { l: 0.8, a: 1.0, b: 1.0 },
		);
		let adjusted_lab = body_seam_uvs
			.get(&index)
			.and_then(|uvs| sample_skin_lab_near_uv(&rgba, im.width, im.height, uvs))
			.or_else(|| estimate_skin_lab(&rgba, im.width, im.height))
			.map(lab_to_array);
		debug.records.push(SkinToneMatchingRecord {
			image_index: index,
			kind: "body",
			width: im.width,
			height: im.height,
			source_lab: lab_to_array(source),
			adjusted_lab,
			adjusted_pixels,
		});
		adjusted[index] = Some(rgba);
	}
	(adjusted, debug)
}

pub(crate) fn skin_tone_matching_debug_for_scene_with_world(scene: &UnaSceneSnapshot, world: &[Mat4]) -> SkinToneMatchingDebug {
	let kinds = skin_tone_texture_kinds_for_scene(scene);
	let (_, debug) = build_skin_tone_matched_images(scene, world, &kinds);
	debug
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn skin_tone_matching_moves_skin_colors_closer_in_lab() {
		let face = [248, 232, 220, 255].repeat(32);
		let body = [235, 190, 176, 255].repeat(32);
		let face_lab = estimate_skin_lab(&face, 8, 4).expect("face skin sample");
		let body_lab = estimate_skin_lab(&body, 8, 4).expect("body skin sample");
		let target = Lab {
			l: (face_lab.l + body_lab.l) * 0.5,
			a: (face_lab.a + body_lab.a) * 0.5,
			b: (face_lab.b + body_lab.b) * 0.5,
		};

		let (face_adjusted, face_pixels) = apply_skin_tone_shift(&face, 8, 4, face_lab, target, Lab { l: 0.18, a: 0.35, b: 0.35 });
		let (body_adjusted, body_pixels) = apply_skin_tone_shift(&body, 8, 4, body_lab, target, Lab { l: 0.8, a: 1.0, b: 1.0 });
		let adjusted_face_lab = estimate_skin_lab(&face_adjusted, 8, 4).expect("adjusted face skin sample");
		let adjusted_body_lab = estimate_skin_lab(&body_adjusted, 8, 4).expect("adjusted body skin sample");

		assert!(face_pixels > 0);
		assert!(body_pixels > 0);
		assert!(lab_distance(adjusted_face_lab, adjusted_body_lab) < lab_distance(face_lab, body_lab));
	}

	#[test]
	fn skin_tone_material_kind_detects_chinese_face_name() {
		let mat = UnaMaterialPbr {
			name: Some("脸".to_string()),
			..Default::default()
		};
		assert_eq!(material_skin_tone_kind(&mat), Some(SkinToneTextureKind::Face));
	}
}
