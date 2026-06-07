use std::borrow::Cow;

use un_avatar_core::UnaSceneNode;

pub(crate) fn scene_roots_or_parentless<'a>(nodes: &[UnaSceneNode], roots: &'a [usize]) -> Cow<'a, [usize]> {
	if !roots.is_empty() {
		return Cow::Borrowed(roots);
	}
	let mut has_parent = vec![false; nodes.len()];
	for node in nodes {
		for &child in &node.children {
			if let Some(slot) = has_parent.get_mut(child) {
				*slot = true;
			}
		}
	}
	Cow::Owned(
		has_parent
			.iter()
			.enumerate()
			.filter_map(|(idx, has_parent)| (!*has_parent).then_some(idx))
			.collect(),
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn node(children: Vec<usize>) -> UnaSceneNode {
		UnaSceneNode {
			source_node_id: None,
			name: None,
			visible: true,
			transform: glam::Mat4::IDENTITY.to_cols_array(),
			children,
			mesh: None,
			skin: None,
			probe_anchor_node: None,
			local_bounds: None,
		}
	}

	#[test]
	fn uses_explicit_roots_without_allocating() {
		let roots = [2usize];
		let resolved = scene_roots_or_parentless(&[], &roots);

		assert!(matches!(resolved, Cow::Borrowed(_)));
		assert_eq!(&*resolved, &[2]);
	}

	#[test]
	fn falls_back_to_parentless_nodes() {
		let nodes = vec![node(vec![1]), node(Vec::new()), node(Vec::new())];
		let resolved = scene_roots_or_parentless(&nodes, &[]);

		assert_eq!(&*resolved, &[0, 2]);
	}
}
