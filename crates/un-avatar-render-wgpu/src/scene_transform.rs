//! Scene node transform helpers shared by mesh preparation and runtime updates.

use glam::Mat4;
use un_avatar_core::{UnaSceneNode, UnaSceneSnapshot};

pub(crate) fn scene_world_matrices(scene: &UnaSceneSnapshot) -> Vec<Mat4> {
	let mut world = Vec::with_capacity(scene.nodes.len().max(1));
	write_world_from_nodes(scene, &mut world);
	world
}

pub(crate) fn write_world_from_nodes(scene: &UnaSceneSnapshot, world: &mut Vec<Mat4>) {
	let n = scene.nodes.len().max(1);
	world.clear();
	world.resize(n, Mat4::IDENTITY);
	fn visit(nodes: &[UnaSceneNode], idx: usize, parent: Mat4, world: &mut [Mat4]) {
		if idx >= nodes.len() {
			return;
		}
		let local = Mat4::from_cols_array(&nodes[idx].transform);
		let w = parent * local;
		world[idx] = w;
		for &c in &nodes[idx].children {
			if c < nodes.len() {
				visit(nodes, c, w, world);
			}
		}
	}
	if scene.roots.is_empty() {
		for &root in scene.resolved_roots().iter() {
			visit(&scene.nodes, root, Mat4::IDENTITY, world);
		}
	} else {
		for &r in &scene.roots {
			if r < scene.nodes.len() {
				visit(&scene.nodes, r, Mat4::IDENTITY, world);
			}
		}
	}
}

pub(crate) fn safe_inverse_mesh_world(m: Mat4) -> Mat4 {
	let inv = m.inverse();
	if inv.to_cols_array().iter().all(|x| x.is_finite()) {
		inv
	} else {
		Mat4::IDENTITY
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use glam::Vec3;
	use un_avatar_core::UnaSceneNode;

	fn node(transform: Mat4, children: Vec<usize>) -> UnaSceneNode {
		UnaSceneNode {
			source_node_id: None,
			resolved_node_id: None,
			name: None,
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
	fn world_matrices_follow_explicit_roots() {
		let scene = UnaSceneSnapshot {
			nodes: vec![
				node(Mat4::from_translation(Vec3::new(1.0, 0.0, 0.0)), vec![1]),
				node(Mat4::from_translation(Vec3::new(0.0, 2.0, 0.0)), Vec::new()),
			],
			roots: vec![0],
			..Default::default()
		};

		let world = scene_world_matrices(&scene);

		assert_eq!(world[1].transform_point3(Vec3::ZERO), Vec3::new(1.0, 2.0, 0.0));
	}

	#[test]
	fn world_matrices_fall_back_to_parentless_roots() {
		let scene = UnaSceneSnapshot {
			nodes: vec![
				node(Mat4::from_translation(Vec3::new(1.0, 0.0, 0.0)), vec![1]),
				node(Mat4::from_translation(Vec3::new(0.0, 2.0, 0.0)), Vec::new()),
			],
			roots: Vec::new(),
			..Default::default()
		};

		let world = scene_world_matrices(&scene);

		assert_eq!(world[1].transform_point3(Vec3::ZERO), Vec3::new(1.0, 2.0, 0.0));
	}
}
