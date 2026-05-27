//! Scene node transform helpers shared by mesh preparation and runtime updates.

use glam::Mat4;
use un_avatar_core::UnaSceneSnapshot;

pub(crate) fn scene_world_matrices(scene: &UnaSceneSnapshot) -> Vec<Mat4> {
	let mut world = Vec::new();
	write_world_from_nodes(scene, &mut world);
	world
}

pub(crate) fn write_world_from_nodes(scene: &UnaSceneSnapshot, world: &mut Vec<Mat4>) {
	let n = scene.nodes.len().max(1);
	world.clear();
	world.resize(n, Mat4::IDENTITY);
	fn visit(nodes: &[un_avatar_core::UnaSceneNode], idx: usize, parent: Mat4, world: &mut [Mat4]) {
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
	for &r in &scene.roots {
		if r < scene.nodes.len() {
			visit(&scene.nodes, r, Mat4::IDENTITY, world);
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
