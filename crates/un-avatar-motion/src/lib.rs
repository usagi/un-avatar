//! UN Avatar — モーション受信・バッファ（bootstrap）。
//!
//! **UNMotionFrame** およびトランスポート非依存の共有スキーマは crates.io の
//! [**un-motion-frame**](https://crates.io/crates/un-motion-frame)（UNMotion から切り出し）を正とする。
//!
//! 設計: `docs/crate-io-plugin-plan.md` §4.5

#![forbid(unsafe_code)]

/// UNMotion / UN Avatar 共通フレーム型。実体は `un-motion-frame` クレート。
pub use un_motion_frame;

pub use un_avatar_vmc;

/// 将来: MotionBuffer など un-avatar 固有の合成・バッファはここに置く。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MotionStub;

#[cfg(test)]
mod tests {
	use un_motion_frame::UNMotionFrame;

	#[test]
	fn un_motion_frame_type_is_public_api() {
		let _sz = std::mem::size_of::<UNMotionFrame>();
		assert!(_sz > 0);
	}
}
