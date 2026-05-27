//! UN Avatar — 共有低レベル型（bootstrap）。
//!
//! 設計の正本: `docs/crate-io-plugin-plan.md`

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// IO 形式を表す論理 ID（例: `io.vrm1`）。§6.4
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FormatId(pub String);

impl FormatId {
	pub fn new(s: impl Into<String>) -> Self {
		Self(s.into())
	}
}

/// VRM / glTF Humanoid 等から得た骨名 → glTF ノードインデックス（ボーン名は小文字に正規化推奨）。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanoidProfile {
	#[serde(default)]
	pub bone_node_indices: BTreeMap<String, usize>,
}

/// プレースホルダ。スキーマ確定後に置き換える。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TypeStub;

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn stub_is_debug() {
		let _ = format!("{:?}", TypeStub);
	}
}
