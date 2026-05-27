//! UN Avatar — 材質抽象（bootstrap）。
//!
//! 設計: `docs/crate-io-plugin-plan.md` §4.8

#![forbid(unsafe_code)]

/// 製品ロードマップ MaterialPolicy v0 のクレート側フック（分岐の正本は [`un_avatar_core::UnaMaterialPbr::shading`]）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MaterialPolicyV0 {
	/// 入力データ（MToon ヒント・Unlit 等）に従う。
	#[default]
	Auto,
}

/// 材質プレースホルダ。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MaterialStub;
