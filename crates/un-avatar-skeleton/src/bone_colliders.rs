use std::collections::BTreeMap;

use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use un_avatar_core::{UnaDynamicsColliderShape, UnaRuntimeDynamics, UnaSceneSnapshot};
use un_avatar_types::HumanoidProfile;

const OFF_EPSILON: f32 = 0.001;
const COLLISION_CONTACT_SLOP: f32 = 0.0001;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoneColliderSource {
	#[default]
	Off,
	Auto,
	AutoAndVrm,
	Vrm,
}

impl BoneColliderSource {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Off => "off",
			Self::Auto => "auto",
			Self::AutoAndVrm => "auto+vrm",
			Self::Vrm => "vrm",
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BoneColliderPartRadiiMm {
	pub head: f32,
	pub neck_chest: f32,
	pub torso: f32,
	pub upper_arms: f32,
	pub lower_arms: f32,
	pub hands: f32,
}

impl Default for BoneColliderPartRadiiMm {
	fn default() -> Self {
		Self {
			head: 120.0,
			neck_chest: 80.0,
			torso: 140.0,
			upper_arms: 55.0,
			lower_arms: 45.0,
			hands: 50.0,
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BoneColliderConfig {
	pub enabled: bool,
	pub radius_mm: BoneColliderPartRadiiMm,
}

impl Default for BoneColliderConfig {
	fn default() -> Self {
		Self {
			enabled: false,
			radius_mm: BoneColliderPartRadiiMm::default(),
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BoneColliderPrimitive {
	Sphere {
		node: usize,
		radius: f32,
	},
	Capsule {
		start_node: usize,
		end_node: usize,
		radius: f32,
	},
	LocalSphere {
		node: usize,
		center: [f32; 3],
		radius: f32,
		inside_bounds: bool,
	},
	LocalCapsule {
		node: usize,
		center: [f32; 3],
		axis: [f32; 3],
		half_length: f32,
		radius: f32,
		inside_bounds: bool,
	},
	LocalPlane {
		node: usize,
		center: [f32; 3],
		normal: [f32; 3],
		inside_bounds: bool,
	},
}

#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeBoneColliderPrimitive {
	pub primitive: BoneColliderPrimitive,
	pub source_id: String,
	pub collider_path: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum WorldBoneColliderPrimitive {
	Sphere {
		center: Vec3,
		radius: f32,
		inside_bounds: bool,
	},
	Capsule {
		a: Vec3,
		b: Vec3,
		radius: f32,
		inside_bounds: bool,
	},
	Plane {
		point: Vec3,
		normal: Vec3,
		inside_bounds: bool,
	},
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoneColliderStats {
	pub count: u32,
	pub source: BoneColliderSource,
}

pub fn build_bone_colliders(
	scene: &UnaSceneSnapshot,
	profile: Option<&HumanoidProfile>,
	config: BoneColliderConfig,
) -> Vec<BoneColliderPrimitive> {
	if !config.enabled {
		return Vec::new();
	}
	let Some(profile) = profile else {
		return Vec::new();
	};
	let lookup = HumanoidLookup::new(profile);
	let world = scene_world(scene);
	let height = estimate_humanoid_height(&lookup, &world).unwrap_or(1.6).clamp(0.8, 2.4);
	let mut out = Vec::new();
	let radius_mm = config.radius_mm;

	if part_enabled(radius_mm.head) {
		if let Some(head) = lookup.node("head") {
			push_sphere(&mut out, head, mm_to_m(radius_mm.head), height * 0.18);
		}
	}
	if part_enabled(radius_mm.neck_chest) {
		let neck = lookup.node("neck");
		let chest = lookup
			.node("upperchest")
			.or_else(|| lookup.node("chest"))
			.or_else(|| lookup.node("spine"));
		if let (Some(a), Some(b)) = (neck, chest) {
			push_capsule(&mut out, a, b, mm_to_m(radius_mm.neck_chest), height * 0.16);
		}
	}
	if part_enabled(radius_mm.torso) {
		let top = lookup
			.node("chest")
			.or_else(|| lookup.node("upperchest"))
			.or_else(|| lookup.node("spine"));
		let bottom = lookup.node("hips");
		if let (Some(a), Some(b)) = (top, bottom) {
			push_capsule(&mut out, a, b, mm_to_m(radius_mm.torso), height * 0.26);
		}
	}
	if part_enabled(radius_mm.upper_arms) {
		for side in ["left", "right"] {
			if let (Some(a), Some(b)) = (lookup.node(&format!("{side}upperarm")), lookup.node(&format!("{side}lowerarm"))) {
				push_capsule(&mut out, a, b, mm_to_m(radius_mm.upper_arms), height * 0.10);
			}
		}
	}
	if part_enabled(radius_mm.lower_arms) {
		for side in ["left", "right"] {
			if let (Some(a), Some(b)) = (lookup.node(&format!("{side}lowerarm")), lookup.node(&format!("{side}hand"))) {
				push_capsule(&mut out, a, b, mm_to_m(radius_mm.lower_arms), height * 0.09);
			}
		}
	}
	if part_enabled(radius_mm.hands) {
		for side in ["left", "right"] {
			if let Some(hand) = lookup.node(&format!("{side}hand")) {
				push_sphere(&mut out, hand, mm_to_m(radius_mm.hands), height * 0.11);
			}
		}
	}

	out
}

pub fn build_dynamics_bone_colliders(
	scene: &UnaSceneSnapshot,
	profile: Option<&HumanoidProfile>,
	config: BoneColliderConfig,
	dynamics: UnaRuntimeDynamics<'_>,
) -> Vec<BoneColliderPrimitive> {
	build_dynamics_bone_colliders_with_sources(scene, profile, config, dynamics)
		.into_iter()
		.map(|collider| collider.primitive)
		.collect()
}

pub fn build_dynamics_bone_colliders_with_sources(
	scene: &UnaSceneSnapshot,
	profile: Option<&HumanoidProfile>,
	config: BoneColliderConfig,
	dynamics: UnaRuntimeDynamics<'_>,
) -> Vec<RuntimeBoneColliderPrimitive> {
	let mut out = build_bone_colliders(scene, profile, config)
		.into_iter()
		.map(|primitive| RuntimeBoneColliderPrimitive {
			primitive,
			source_id: String::new(),
			collider_path: String::new(),
		})
		.collect::<Vec<_>>();
	for collider in dynamics.colliders() {
		if collider.node >= scene.nodes.len() {
			continue;
		}
		let radius = collider.radius.max(0.0);
		if collider.shape != UnaDynamicsColliderShape::Plane && (!radius.is_finite() || radius <= OFF_EPSILON) {
			continue;
		}
		let center = collider.position;
		let primitive = match collider.shape {
			UnaDynamicsColliderShape::Sphere => Some(BoneColliderPrimitive::LocalSphere {
				node: collider.node,
				center,
				radius,
				inside_bounds: collider.inside_bounds,
			}),
			UnaDynamicsColliderShape::Capsule => {
				let axis = Quat::from_xyzw(
					collider.rotation[0],
					collider.rotation[1],
					collider.rotation[2],
					collider.rotation[3],
				) * Vec3::Y;
				let axis = axis.try_normalize().unwrap_or(Vec3::Y).to_array();
				let half_length = (collider.height.max(0.0) * 0.5 - radius).max(0.0);
				Some(BoneColliderPrimitive::LocalCapsule {
					node: collider.node,
					center,
					axis,
					half_length,
					radius,
					inside_bounds: collider.inside_bounds,
				})
			}
			UnaDynamicsColliderShape::Plane => {
				let normal = Quat::from_xyzw(
					collider.rotation[0],
					collider.rotation[1],
					collider.rotation[2],
					collider.rotation[3],
				) * Vec3::Y;
				let normal = normal.try_normalize().unwrap_or(Vec3::Y).to_array();
				Some(BoneColliderPrimitive::LocalPlane {
					node: collider.node,
					center,
					normal,
					inside_bounds: collider.inside_bounds,
				})
			}
			UnaDynamicsColliderShape::Unknown => None,
		};
		if let Some(primitive) = primitive {
			out.push(RuntimeBoneColliderPrimitive {
				primitive,
				source_id: collider.source_id.clone(),
				collider_path: collider.collider_path.clone(),
			});
		}
	}
	out
}

pub fn build_runtime_bone_colliders(
	scene: &UnaSceneSnapshot,
	profile: Option<&HumanoidProfile>,
	config: BoneColliderConfig,
	dynamics: UnaRuntimeDynamics<'_>,
) -> Vec<BoneColliderPrimitive> {
	build_dynamics_bone_colliders(scene, profile, config, dynamics)
}

pub fn collider_stats(colliders: &[BoneColliderPrimitive]) -> BoneColliderStats {
	BoneColliderStats {
		count: colliders.len() as u32,
		source: if colliders.is_empty() {
			BoneColliderSource::Off
		} else {
			BoneColliderSource::Auto
		},
	}
}

pub(crate) fn scene_world(scene: &UnaSceneSnapshot) -> Vec<Mat4> {
	let mut world = vec![Mat4::IDENTITY; scene.nodes.len().max(1)];
	for &root in scene.resolved_roots().iter() {
		propagate_world(scene, &mut world, root, Mat4::IDENTITY);
	}
	world
}

#[cfg(test)]
pub(crate) fn push_out_of_colliders(point: Vec3, world: &[Mat4], colliders: &[BoneColliderPrimitive], extra_radius: f32) -> Vec3 {
	let mut resolved = Vec::with_capacity(colliders.len());
	resolve_world_colliders(world, colliders, &mut resolved);
	push_out_of_world_colliders(point, &resolved, extra_radius)
}

pub(crate) fn resolve_world_colliders(world: &[Mat4], colliders: &[BoneColliderPrimitive], out: &mut Vec<WorldBoneColliderPrimitive>) {
	out.clear();
	out.reserve(colliders.len());
	for collider in colliders {
		match *collider {
			BoneColliderPrimitive::Sphere { node, radius } => {
				if let Some(center) = node_position(world, node) {
					out.push(WorldBoneColliderPrimitive::Sphere {
						center,
						radius,
						inside_bounds: false,
					});
				}
			}
			BoneColliderPrimitive::Capsule {
				start_node,
				end_node,
				radius,
			} => {
				if let (Some(a), Some(b)) = (node_position(world, start_node), node_position(world, end_node)) {
					out.push(WorldBoneColliderPrimitive::Capsule {
						a,
						b,
						radius,
						inside_bounds: false,
					});
				}
			}
			BoneColliderPrimitive::LocalSphere {
				node,
				center,
				radius,
				inside_bounds,
			} => {
				if let Some((center, radius)) = local_sphere_world(world, node, center, radius) {
					out.push(WorldBoneColliderPrimitive::Sphere {
						center,
						radius,
						inside_bounds,
					});
				}
			}
			BoneColliderPrimitive::LocalCapsule {
				node,
				center,
				axis,
				half_length,
				radius,
				inside_bounds,
			} => {
				if let Some((a, b, radius)) = local_capsule_world(world, node, center, axis, half_length, radius) {
					out.push(WorldBoneColliderPrimitive::Capsule {
						a,
						b,
						radius,
						inside_bounds,
					});
				}
			}
			BoneColliderPrimitive::LocalPlane {
				node,
				center,
				normal,
				inside_bounds,
			} => {
				if let Some((point, normal)) = local_plane_world(world, node, center, normal) {
					out.push(WorldBoneColliderPrimitive::Plane {
						point,
						normal,
						inside_bounds,
					});
				}
			}
		}
	}
}

#[cfg(test)]
pub(crate) fn push_out_of_world_colliders(point: Vec3, colliders: &[WorldBoneColliderPrimitive], extra_radius: f32) -> Vec3 {
	let mut p = point;
	let extra = extra_radius.max(0.0);
	for collider in colliders {
		p = push_out_of_world_collider(p, *collider, extra);
	}
	p
}

pub(crate) fn push_out_of_world_collider(point: Vec3, collider: WorldBoneColliderPrimitive, extra_radius: f32) -> Vec3 {
	match collider {
		WorldBoneColliderPrimitive::Sphere {
			center,
			radius,
			inside_bounds,
		} => {
			if inside_bounds {
				push_into_sphere(point, center, radius - extra_radius)
			} else {
				push_out_sphere(point, center, radius + extra_radius)
			}
		}
		WorldBoneColliderPrimitive::Capsule {
			a,
			b,
			radius,
			inside_bounds,
		} => {
			let center = closest_on_segment(point, a, b);
			if inside_bounds {
				push_into_sphere(point, center, radius - extra_radius)
			} else {
				push_out_sphere(point, center, radius + extra_radius)
			}
		}
		WorldBoneColliderPrimitive::Plane {
			point: plane_point,
			normal,
			inside_bounds,
		} => push_plane_half_space(point, plane_point, normal, extra_radius, inside_bounds),
	}
}

#[cfg(test)]
pub(crate) fn push_segment_tail_out_of_world_colliders(
	anchor: Vec3,
	tail: Vec3,
	colliders: &[WorldBoneColliderPrimitive],
	extra_radius: f32,
) -> Vec3 {
	let mut tail = push_out_of_world_colliders(tail, colliders, extra_radius);
	let extra = extra_radius.max(0.0);
	for _ in 0..3 {
		let before = tail;
		for collider in colliders {
			match *collider {
				WorldBoneColliderPrimitive::Sphere {
					center,
					radius,
					inside_bounds: false,
				} => {
					tail = push_segment_tail_out_of_sphere(anchor, tail, center, radius + extra);
				}
				WorldBoneColliderPrimitive::Capsule {
					a,
					b,
					radius,
					inside_bounds: false,
				} => {
					tail = push_segment_tail_out_of_capsule(anchor, tail, a, b, radius + extra);
				}
				_ => {}
			}
		}
		if (tail - before).length_squared() <= 1e-12 {
			break;
		}
	}
	tail
}

fn part_enabled(value: f32) -> bool {
	value.is_finite() && value >= OFF_EPSILON
}

fn mm_to_m(value: f32) -> f32 {
	if value.is_finite() {
		(value.max(0.0) * 0.001).min(1.0)
	} else {
		0.0
	}
}

fn push_sphere(out: &mut Vec<BoneColliderPrimitive>, node: usize, radius: f32, max: f32) {
	let r = radius.min(max);
	if r.is_finite() && r > OFF_EPSILON {
		out.push(BoneColliderPrimitive::Sphere { node, radius: r });
	}
}

fn push_capsule(out: &mut Vec<BoneColliderPrimitive>, start_node: usize, end_node: usize, radius: f32, max: f32) {
	let r = radius.min(max);
	if r.is_finite() && r > OFF_EPSILON && start_node != end_node {
		out.push(BoneColliderPrimitive::Capsule {
			start_node,
			end_node,
			radius: r,
		});
	}
}

fn propagate_world(scene: &UnaSceneSnapshot, world: &mut [Mat4], node: usize, parent: Mat4) {
	if node >= scene.nodes.len() || node >= world.len() {
		return;
	}
	let w = parent * Mat4::from_cols_array(&scene.nodes[node].transform);
	world[node] = w;
	for &child in &scene.nodes[node].children {
		propagate_world(scene, world, child, w);
	}
}

fn node_position(world: &[Mat4], node: usize) -> Option<Vec3> {
	world.get(node).map(|m| m.transform_point3(Vec3::ZERO))
}

pub fn local_sphere_world(world: &[Mat4], node: usize, center: [f32; 3], radius: f32) -> Option<(Vec3, f32)> {
	let m = world.get(node)?;
	Some((m.transform_point3(Vec3::from(center)), radius * conservative_world_scale(*m)))
}

pub fn local_capsule_world(
	world: &[Mat4],
	node: usize,
	center: [f32; 3],
	axis: [f32; 3],
	half_length: f32,
	radius: f32,
) -> Option<(Vec3, Vec3, f32)> {
	let m = world.get(node)?;
	let center = Vec3::from(center);
	let axis = Vec3::from(axis).try_normalize().unwrap_or(Vec3::Y);
	let half = half_length.max(0.0);
	let a = m.transform_point3(center - axis * half);
	let b = m.transform_point3(center + axis * half);
	Some((a, b, radius * conservative_world_scale(*m)))
}

pub fn local_plane_world(world: &[Mat4], node: usize, center: [f32; 3], normal: [f32; 3]) -> Option<(Vec3, Vec3)> {
	let m = world.get(node)?;
	let point = m.transform_point3(Vec3::from(center));
	let normal = m.transform_vector3(Vec3::from(normal)).try_normalize().unwrap_or(Vec3::Y);
	Some((point, normal))
}

pub fn distance_point_segment(point: Vec3, a: Vec3, b: Vec3) -> f32 {
	let ab = b - a;
	let denom = ab.length_squared();
	if denom <= 1.0e-12 {
		return point.distance(a);
	}
	let t = ((point - a).dot(ab) / denom).clamp(0.0, 1.0);
	point.distance(a + ab * t)
}

fn conservative_world_scale(m: Mat4) -> f32 {
	let sx = m.x_axis.truncate().length();
	let sy = m.y_axis.truncate().length();
	let sz = m.z_axis.truncate().length();
	let scale = sx.max(sy).max(sz);
	if scale.is_finite() && scale > OFF_EPSILON {
		scale
	} else {
		1.0
	}
}

fn estimate_humanoid_height(lookup: &HumanoidLookup, world: &[Mat4]) -> Option<f32> {
	let head = lookup.node("head").and_then(|n| node_position(world, n));
	let hips = lookup.node("hips").and_then(|n| node_position(world, n));
	match (head, hips) {
		(Some(h), Some(p)) => Some((h.y - p.y).abs() * 2.3),
		_ => None,
	}
}

fn closest_on_segment(p: Vec3, a: Vec3, b: Vec3) -> Vec3 {
	let ab = b - a;
	let denom = ab.length_squared();
	if denom <= 1e-12 {
		return a;
	}
	let t = ((p - a).dot(ab) / denom).clamp(0.0, 1.0);
	a + ab * t
}

#[cfg(test)]
fn closest_on_segment_with_t(p: Vec3, a: Vec3, b: Vec3) -> (Vec3, f32) {
	let ab = b - a;
	let denom = ab.length_squared();
	if denom <= 1e-12 {
		return (a, 0.0);
	}
	let t = ((p - a).dot(ab) / denom).clamp(0.0, 1.0);
	(a + ab * t, t)
}

#[cfg(test)]
fn closest_segment_segment(a0: Vec3, a1: Vec3, b0: Vec3, b1: Vec3) -> (Vec3, Vec3, f32, f32) {
	let d1 = a1 - a0;
	let d2 = b1 - b0;
	let r = a0 - b0;
	let a = d1.length_squared();
	let e = d2.length_squared();
	let f = d2.dot(r);
	let (mut s, t);
	if a <= 1e-12 && e <= 1e-12 {
		return (a0, b0, 0.0, 0.0);
	}
	if a <= 1e-12 {
		s = 0.0;
		t = (f / e).clamp(0.0, 1.0);
	} else {
		let c = d1.dot(r);
		if e <= 1e-12 {
			t = 0.0;
			s = (-c / a).clamp(0.0, 1.0);
		} else {
			let b = d1.dot(d2);
			let denom = a * e - b * b;
			s = if denom.abs() > 1e-12 {
				((b * f - c * e) / denom).clamp(0.0, 1.0)
			} else {
				0.0
			};
			let t_nom = b * s + f;
			if t_nom < 0.0 {
				t = 0.0;
				s = (-c / a).clamp(0.0, 1.0);
			} else if t_nom > e {
				t = 1.0;
				s = ((b - c) / a).clamp(0.0, 1.0);
			} else {
				t = t_nom / e;
			}
		}
	}
	(a0 + d1 * s, b0 + d2 * t, s, t)
}

#[cfg(test)]
fn segment_push_direction(anchor: Vec3, tail: Vec3, delta: Vec3) -> Vec3 {
	if delta.length_squared() > 1e-12 {
		return delta.normalize();
	}
	let axis = (tail - anchor).normalize_or_zero();
	let cross_y = axis.cross(Vec3::Y);
	if cross_y.length_squared() > 1e-12 {
		cross_y.normalize()
	} else {
		axis.cross(Vec3::X).normalize_or_zero()
	}
}

#[cfg(test)]
fn push_segment_tail_by_closest_delta(tail: Vec3, closest: Vec3, target: Vec3, segment_t: f32) -> Vec3 {
	if !(0.0001..=1.0).contains(&segment_t) {
		return tail;
	}
	let delta = target - closest;
	if delta.length_squared() <= 1e-12 {
		return tail;
	}
	tail + delta / segment_t
}

#[cfg(test)]
fn push_segment_tail_out_of_sphere(anchor: Vec3, tail: Vec3, center: Vec3, radius: f32) -> Vec3 {
	if !radius.is_finite() || radius <= OFF_EPSILON {
		return tail;
	}
	let (closest, segment_t) = closest_on_segment_with_t(center, anchor, tail);
	let delta = closest - center;
	let dist = delta.length();
	if dist >= radius || radius - dist <= COLLISION_CONTACT_SLOP || !dist.is_finite() {
		return tail;
	}
	let dir = segment_push_direction(anchor, tail, delta);
	let target = center + dir * radius;
	push_segment_tail_by_closest_delta(tail, closest, target, segment_t)
}

#[cfg(test)]
fn push_segment_tail_out_of_capsule(anchor: Vec3, tail: Vec3, a: Vec3, b: Vec3, radius: f32) -> Vec3 {
	if !radius.is_finite() || radius <= OFF_EPSILON {
		return tail;
	}
	let (closest_bone, closest_collider, segment_t, _) = closest_segment_segment(anchor, tail, a, b);
	let delta = closest_bone - closest_collider;
	let dist = delta.length();
	if dist >= radius || radius - dist <= COLLISION_CONTACT_SLOP || !dist.is_finite() {
		return tail;
	}
	let dir = segment_push_direction(anchor, tail, delta);
	let target = closest_collider + dir * radius;
	push_segment_tail_by_closest_delta(tail, closest_bone, target, segment_t)
}

fn push_out_sphere(point: Vec3, center: Vec3, radius: f32) -> Vec3 {
	if !radius.is_finite() || radius <= OFF_EPSILON {
		return point;
	}
	let delta = point - center;
	let dist_sq = delta.length_squared();
	let radius_sq = radius * radius;
	if dist_sq >= radius_sq || !dist_sq.is_finite() {
		return point;
	}
	let dist = dist_sq.sqrt();
	if radius - dist <= COLLISION_CONTACT_SLOP {
		return point;
	}
	if dist <= 1e-6 {
		center + Vec3::Y * radius
	} else {
		center + delta / dist * radius
	}
}

fn push_into_sphere(point: Vec3, center: Vec3, radius: f32) -> Vec3 {
	if !radius.is_finite() {
		return point;
	}
	let radius = radius.max(0.0);
	let delta = point - center;
	let dist_sq = delta.length_squared();
	let radius_sq = radius * radius;
	if dist_sq <= radius_sq || !dist_sq.is_finite() {
		return point;
	}
	let dist = dist_sq.sqrt();
	if dist <= 1e-6 || radius <= OFF_EPSILON {
		center
	} else {
		center + delta / dist * radius
	}
}

fn push_plane_half_space(point: Vec3, plane_point: Vec3, normal: Vec3, extra_radius: f32, inside_bounds: bool) -> Vec3 {
	let normal = normal.try_normalize().unwrap_or(Vec3::Y);
	let signed = (point - plane_point).dot(normal);
	let margin = extra_radius.max(0.0);
	if inside_bounds {
		if signed > -margin {
			point + normal * (-margin - signed)
		} else {
			point
		}
	} else if signed < margin {
		point + normal * (margin - signed)
	} else {
		point
	}
}

struct HumanoidLookup {
	map: BTreeMap<String, usize>,
}

impl HumanoidLookup {
	fn new(profile: &HumanoidProfile) -> Self {
		let mut map = BTreeMap::new();
		for (key, value) in &profile.bone_node_indices {
			map.insert(normalize_key(key), *value);
		}
		Self { map }
	}

	fn node(&self, key: &str) -> Option<usize> {
		self.map.get(&normalize_key(key)).copied()
	}
}

fn normalize_key(key: &str) -> String {
	key.chars()
		.filter(|ch| ch.is_ascii_alphanumeric())
		.map(|ch| ch.to_ascii_lowercase())
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;
	use un_avatar_core::{
		UnaDynamicsCollider, UnaDynamicsColliderShape, UnaDynamicsSourceKind, UnaSceneNode, UnaSceneSnapshot, UnaSpringBoneSettings,
	};

	fn node(name: &str, translation: Vec3, children: Vec<usize>) -> UnaSceneNode {
		node_with_transform(name, Mat4::from_translation(translation), children)
	}

	fn node_with_transform(name: &str, transform: Mat4, children: Vec<usize>) -> UnaSceneNode {
		UnaSceneNode {
			source_node_id: None,
			resolved_node_id: None,
			name: Some(name.to_string()),
			visible: true,
			transform: transform.to_cols_array(),
			children,
			mesh: None,
			skin: None,
			probe_anchor_node: None,
			local_bounds: None,
		}
	}

	#[test]
	fn bone_collider_config_defaults_to_disabled() {
		assert!(!BoneColliderConfig::default().enabled);
	}

	#[test]
	fn default_config_generates_no_auto_bone_colliders() {
		let scene = UnaSceneSnapshot {
			nodes: vec![node("Head", Vec3::ZERO, vec![])],
			roots: vec![0],
			..Default::default()
		};
		let mut profile = HumanoidProfile::default();
		profile.bone_node_indices.insert("head".to_string(), 0);
		let colliders = build_bone_colliders(&scene, Some(&profile), BoneColliderConfig::default());
		assert!(colliders.is_empty(), "colliders={colliders:?}");
	}

	#[test]
	fn generates_basic_humanoid_colliders() {
		let scene = UnaSceneSnapshot {
			nodes: vec![
				node("Hips", Vec3::ZERO, vec![1, 4, 7]),
				node("Chest", Vec3::new(0.0, 0.6, 0.0), vec![2]),
				node("Neck", Vec3::new(0.0, 0.2, 0.0), vec![3]),
				node("Head", Vec3::new(0.0, 0.18, 0.0), vec![]),
				node("LeftUpperArm", Vec3::new(-0.25, 0.55, 0.0), vec![5]),
				node("LeftLowerArm", Vec3::new(-0.35, 0.0, 0.0), vec![6]),
				node("LeftHand", Vec3::new(-0.30, 0.0, 0.0), vec![]),
				node("RightUpperArm", Vec3::new(0.25, 0.55, 0.0), vec![8]),
				node("RightLowerArm", Vec3::new(0.35, 0.0, 0.0), vec![9]),
				node("RightHand", Vec3::new(0.30, 0.0, 0.0), vec![]),
			],
			roots: vec![0],
			..Default::default()
		};
		let mut profile = HumanoidProfile::default();
		for (key, index) in [
			("hips", 0),
			("chest", 1),
			("neck", 2),
			("head", 3),
			("leftUpperArm", 4),
			("leftLowerArm", 5),
			("leftHand", 6),
			("rightUpperArm", 7),
			("rightLowerArm", 8),
			("rightHand", 9),
		] {
			profile.bone_node_indices.insert(key.to_string(), index);
		}
		let colliders = build_bone_colliders(
			&scene,
			Some(&profile),
			BoneColliderConfig {
				enabled: true,
				..BoneColliderConfig::default()
			},
		);
		assert!(colliders.len() >= 8, "colliders={colliders:?}");
	}

	#[test]
	fn runtime_colliders_include_unavatar_source_colliders() {
		let scene = UnaSceneSnapshot {
			nodes: vec![node("Root", Vec3::ZERO, Vec::new())],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: Vec::new(),
			colliders: vec![UnaDynamicsCollider {
				source_kind: UnaDynamicsSourceKind::VrcPhysBone,
				source_id: "physbone:hair".to_string(),
				node: 0,
				shape: UnaDynamicsColliderShape::Sphere,
				radius: 0.1,
				position: [0.2, 0.0, 0.0],
				..Default::default()
			}],
			..Default::default()
		};
		let colliders = build_dynamics_bone_colliders(
			&scene,
			None,
			BoneColliderConfig {
				enabled: false,
				..Default::default()
			},
			settings.runtime_dynamics(),
		);
		let tagged = build_dynamics_bone_colliders_with_sources(
			&scene,
			None,
			BoneColliderConfig {
				enabled: false,
				..Default::default()
			},
			settings.runtime_dynamics(),
		);
		assert_eq!(tagged[0].source_id, "physbone:hair");
		assert_eq!(
			colliders,
			vec![BoneColliderPrimitive::LocalSphere {
				node: 0,
				center: [0.2, 0.0, 0.0],
				radius: 0.1,
				inside_bounds: false,
			}]
		);

		let world = scene_world(&scene);
		let pushed = push_out_of_colliders(Vec3::new(0.21, 0.0, 0.0), &world, &colliders, 0.0);
		assert!((pushed.x - 0.3).abs() < 1e-5, "pushed={pushed:?}");
	}

	#[test]
	fn local_source_sphere_collider_uses_node_scale_at_runtime() {
		let scene = UnaSceneSnapshot {
			nodes: vec![node_with_transform(
				"Root",
				Mat4::from_scale_rotation_translation(Vec3::new(2.0, 1.0, 1.0), Quat::IDENTITY, Vec3::ZERO),
				Vec::new(),
			)],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: Vec::new(),
			colliders: vec![UnaDynamicsCollider {
				source_kind: UnaDynamicsSourceKind::VrcPhysBone,
				node: 0,
				shape: UnaDynamicsColliderShape::Sphere,
				radius: 0.1,
				position: [0.2, 0.0, 0.0],
				..Default::default()
			}],
			..Default::default()
		};
		let colliders = build_dynamics_bone_colliders(
			&scene,
			None,
			BoneColliderConfig {
				enabled: false,
				..Default::default()
			},
			settings.runtime_dynamics(),
		);
		assert_eq!(
			colliders,
			vec![BoneColliderPrimitive::LocalSphere {
				node: 0,
				center: [0.2, 0.0, 0.0],
				radius: 0.1,
				inside_bounds: false,
			}]
		);

		let world = scene_world(&scene);
		let Some((center, radius)) = local_sphere_world(&world, 0, [0.2, 0.0, 0.0], 0.1) else {
			panic!("scaled local sphere should resolve")
		};
		assert!((center.x - 0.4).abs() < 1e-5, "center={center:?}");
		assert!((radius - 0.2).abs() < 1e-5, "radius={radius}");
		let pushed = push_out_of_colliders(Vec3::new(0.41, 0.0, 0.0), &world, &colliders, 0.0);
		assert!((pushed.x - 0.6).abs() < 1e-5, "pushed={pushed:?}");
	}

	#[test]
	fn inside_bounds_source_collider_keeps_points_inside() {
		let scene = UnaSceneSnapshot {
			nodes: vec![node("Root", Vec3::ZERO, Vec::new())],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: Vec::new(),
			colliders: vec![UnaDynamicsCollider {
				source_kind: UnaDynamicsSourceKind::VrcPhysBone,
				node: 0,
				shape: UnaDynamicsColliderShape::Sphere,
				radius: 0.2,
				inside_bounds: true,
				..Default::default()
			}],
			..Default::default()
		};
		let colliders = build_dynamics_bone_colliders(
			&scene,
			None,
			BoneColliderConfig {
				enabled: false,
				..Default::default()
			},
			settings.runtime_dynamics(),
		);
		let world = scene_world(&scene);
		let pushed = push_out_of_colliders(Vec3::new(0.5, 0.0, 0.0), &world, &colliders, 0.05);
		assert!((pushed.x - 0.15).abs() < 1e-5, "pushed={pushed:?}");
	}

	#[test]
	fn capsule_source_collider_pushes_points_from_segment() {
		let scene = UnaSceneSnapshot {
			nodes: vec![node("Root", Vec3::ZERO, Vec::new())],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: Vec::new(),
			colliders: vec![UnaDynamicsCollider {
				source_kind: UnaDynamicsSourceKind::VrcPhysBone,
				node: 0,
				shape: UnaDynamicsColliderShape::Capsule,
				radius: 0.1,
				height: 0.6,
				rotation: Quat::IDENTITY.to_array(),
				..Default::default()
			}],
			..Default::default()
		};
		let colliders = build_dynamics_bone_colliders(
			&scene,
			None,
			BoneColliderConfig {
				enabled: false,
				..Default::default()
			},
			settings.runtime_dynamics(),
		);
		assert_eq!(colliders.len(), 1);
		let BoneColliderPrimitive::LocalCapsule {
			node,
			center,
			axis,
			half_length,
			radius,
			inside_bounds,
		} = colliders[0]
		else {
			panic!("unexpected collider={:?}", colliders[0]);
		};
		assert_eq!(node, 0);
		assert_eq!(center, [0.0, 0.0, 0.0]);
		assert_eq!(axis, [0.0, 1.0, 0.0]);
		assert!((half_length - 0.2).abs() < 1e-5, "half_length={half_length}");
		assert!((radius - 0.1).abs() < 1e-5, "radius={radius}");
		assert!(!inside_bounds);

		let world = scene_world(&scene);
		let pushed = push_out_of_colliders(Vec3::new(0.03, 0.0, 0.0), &world, &colliders, 0.0);
		assert!((pushed.x - 0.1).abs() < 1e-5, "pushed={pushed:?}");
	}

	#[test]
	fn segment_tail_collision_pushes_when_endpoint_is_outside() {
		let colliders = vec![WorldBoneColliderPrimitive::Sphere {
			center: Vec3::ZERO,
			radius: 0.25,
			inside_bounds: false,
		}];
		let anchor = Vec3::new(-1.0, 0.0, 0.0);
		let tail = Vec3::new(1.0, 0.0, 0.0);
		assert_eq!(push_out_of_world_colliders(tail, &colliders, 0.0), tail);
		let pushed = push_segment_tail_out_of_world_colliders(anchor, tail, &colliders, 0.0);
		let (closest, _) = closest_on_segment_with_t(Vec3::ZERO, anchor, pushed);
		assert!(closest.length() >= 0.25 - 1e-5, "pushed={pushed:?} closest={closest:?}");
	}

	#[test]
	fn collision_ignores_submillimeter_contact_slop() {
		let point = Vec3::new(0.24995, 0.0, 0.0);
		let pushed = push_out_sphere(point, Vec3::ZERO, 0.25);
		assert_eq!(pushed, point);

		let anchor = Vec3::new(-1.0, 0.0, 0.0);
		let tail = Vec3::new(1.0, 0.0, 0.0);
		let collider = WorldBoneColliderPrimitive::Sphere {
			center: Vec3::new(0.0, 0.24995, 0.0),
			radius: 0.25,
			inside_bounds: false,
		};
		let pushed = push_segment_tail_out_of_world_colliders(anchor, tail, &[collider], 0.0);
		assert_eq!(pushed, tail);
	}

	#[test]
	fn local_source_capsule_collider_uses_node_scale_at_runtime() {
		let scene = UnaSceneSnapshot {
			nodes: vec![node_with_transform(
				"Root",
				Mat4::from_scale_rotation_translation(Vec3::new(1.0, 2.0, 1.0), Quat::IDENTITY, Vec3::ZERO),
				Vec::new(),
			)],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: Vec::new(),
			colliders: vec![UnaDynamicsCollider {
				source_kind: UnaDynamicsSourceKind::VrcPhysBone,
				node: 0,
				shape: UnaDynamicsColliderShape::Capsule,
				radius: 0.1,
				height: 0.6,
				rotation: Quat::IDENTITY.to_array(),
				..Default::default()
			}],
			..Default::default()
		};
		let colliders = build_dynamics_bone_colliders(
			&scene,
			None,
			BoneColliderConfig {
				enabled: false,
				..Default::default()
			},
			settings.runtime_dynamics(),
		);
		let world = scene_world(&scene);
		let Some((a, b, radius)) = local_capsule_world(&world, 0, [0.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0.2, 0.1) else {
			panic!("scaled local capsule should resolve")
		};
		assert!((a.y + 0.4).abs() < 1e-5, "a={a:?}");
		assert!((b.y - 0.4).abs() < 1e-5, "b={b:?}");
		assert!((radius - 0.2).abs() < 1e-5, "radius={radius}");
		let pushed = push_out_of_colliders(Vec3::new(0.03, 0.0, 0.0), &world, &colliders, 0.0);
		assert!((pushed.x - 0.2).abs() < 1e-5, "pushed={pushed:?}");
	}

	#[test]
	fn inside_bounds_capsule_source_collider_keeps_points_inside() {
		let scene = UnaSceneSnapshot {
			nodes: vec![node("Root", Vec3::ZERO, Vec::new())],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: Vec::new(),
			colliders: vec![UnaDynamicsCollider {
				source_kind: UnaDynamicsSourceKind::VrcPhysBone,
				node: 0,
				shape: UnaDynamicsColliderShape::Capsule,
				radius: 0.1,
				height: 0.6,
				rotation: Quat::IDENTITY.to_array(),
				inside_bounds: true,
				..Default::default()
			}],
			..Default::default()
		};
		let colliders = build_dynamics_bone_colliders(
			&scene,
			None,
			BoneColliderConfig {
				enabled: false,
				..Default::default()
			},
			settings.runtime_dynamics(),
		);
		let world = scene_world(&scene);
		let pushed = push_out_of_colliders(Vec3::new(0.5, 0.0, 0.0), &world, &colliders, 0.02);
		assert!((pushed.x - 0.08).abs() < 1e-5, "pushed={pushed:?}");
	}

	#[test]
	fn plane_source_collider_pushes_points_to_positive_half_space() {
		let scene = UnaSceneSnapshot {
			nodes: vec![node("Floor", Vec3::ZERO, Vec::new())],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: Vec::new(),
			colliders: vec![UnaDynamicsCollider {
				source_kind: UnaDynamicsSourceKind::VrcPhysBone,
				node: 0,
				shape: UnaDynamicsColliderShape::Plane,
				position: [0.0, 0.0, 0.0],
				rotation: Quat::IDENTITY.to_array(),
				..Default::default()
			}],
			..Default::default()
		};
		let colliders = build_dynamics_bone_colliders(
			&scene,
			None,
			BoneColliderConfig {
				enabled: false,
				..Default::default()
			},
			settings.runtime_dynamics(),
		);
		assert_eq!(
			colliders,
			vec![BoneColliderPrimitive::LocalPlane {
				node: 0,
				center: [0.0, 0.0, 0.0],
				normal: [0.0, 1.0, 0.0],
				inside_bounds: false,
			}]
		);
		let world = scene_world(&scene);
		let pushed = push_out_of_colliders(Vec3::new(0.0, -0.2, 0.0), &world, &colliders, 0.05);
		assert!((pushed.y - 0.05).abs() < 1e-5, "pushed={pushed:?}");
	}

	#[test]
	fn inside_bounds_plane_source_collider_keeps_points_on_negative_side() {
		let scene = UnaSceneSnapshot {
			nodes: vec![node("Ceiling", Vec3::ZERO, Vec::new())],
			roots: vec![0],
			..Default::default()
		};
		let settings = UnaSpringBoneSettings {
			groups: Vec::new(),
			colliders: vec![UnaDynamicsCollider {
				source_kind: UnaDynamicsSourceKind::VrcPhysBone,
				node: 0,
				shape: UnaDynamicsColliderShape::Plane,
				inside_bounds: true,
				rotation: Quat::IDENTITY.to_array(),
				..Default::default()
			}],
			..Default::default()
		};
		let colliders = build_dynamics_bone_colliders(
			&scene,
			None,
			BoneColliderConfig {
				enabled: false,
				..Default::default()
			},
			settings.runtime_dynamics(),
		);
		let world = scene_world(&scene);
		let pushed = push_out_of_colliders(Vec3::new(0.0, -0.2, 0.0), &world, &colliders, 0.05);
		assert!((pushed.y + 0.2).abs() < 1e-5, "pushed={pushed:?}");
		let clamped = push_out_of_colliders(Vec3::new(0.0, 0.2, 0.0), &world, &colliders, 0.05);
		assert!((clamped.y + 0.05).abs() < 1e-5, "clamped={clamped:?}");
	}
}
