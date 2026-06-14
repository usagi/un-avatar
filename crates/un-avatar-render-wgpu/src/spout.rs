//! Spout2 送出（Windows）。標準配布は `cargo xtask package` が Spout2 を取得・ビルドして同梱します。開発手動ビルドで実リンクする場合は feature `spout-sdk`、`SPOUT2_SDK_DIR`（SpoutSender.h）、`SPOUT2_LIB_DIR`（Spout.lib）、プロセス起動前の `Spout.dll` への PATH が必要です。未指定時はスタブでビルドのみ通ります。

#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub(crate) struct SpoutFrameStats {
	pub frames_attempted: u64,
	pub frames_sent: u64,
	pub frame_failures: u64,
	pub consecutive_failures: u32,
	pub last_send_ok: Option<bool>,
	pub last_readback_ms: Option<f32>,
	pub last_send_ms: Option<f32>,
	pub last_total_ms: Option<f32>,
	pub sender_initialized: Option<bool>,
	pub sender_width: Option<u32>,
	pub sender_height: Option<u32>,
}

/// ウィンドウとは別解像度で Spout 送出する場合の指定。
#[allow(dead_code)] // スタブビルドでは未読（`spout-sdk` 有効時に Spout 実装が使用）
#[derive(Clone, Debug, Default)]
pub(crate) struct SpoutLaunchConfig {
	pub name: String,
	pub width: Option<u32>,
	pub height: Option<u32>,
}

#[cfg(all(windows, feature = "spout-sdk"))]
include!("spout_sdk.inc.rs");

#[cfg(all(windows, feature = "spout-sdk"))]
pub(crate) fn backend_available() -> bool {
	true
}

#[cfg(all(windows, not(feature = "spout-sdk")))]
pub(crate) struct SpoutCapture;

#[cfg(any(not(windows), all(windows, not(feature = "spout-sdk"))))]
pub(crate) fn backend_available() -> bool {
	false
}

#[cfg(all(windows, not(feature = "spout-sdk")))]
impl SpoutCapture {
	pub fn try_new(
		_device: &wgpu::Device,
		_surface_format: wgpu::TextureFormat,
		_window_w: u32,
		_window_h: u32,
		_cfg: SpoutLaunchConfig,
	) -> Option<Self> {
		None
	}

	pub fn dimensions(&self) -> (u32, u32) {
		unreachable!("Spout スタブ")
	}

	pub fn resize_to(
		&mut self,
		_device: &wgpu::Device,
		_window_w: u32,
		_window_h: u32,
		_cfg: &SpoutLaunchConfig,
		_surface_format: wgpu::TextureFormat,
	) {
		unreachable!("Spout スタブ")
	}

	pub fn color_view(&self) -> &wgpu::TextureView {
		unreachable!("Spout スタブ")
	}

	pub fn depth_view(&self) -> &wgpu::TextureView {
		unreachable!("Spout スタブ")
	}

	pub fn copy_to_staging(&mut self, _encoder: &mut wgpu::CommandEncoder) -> Option<usize> {
		unreachable!("Spout スタブ")
	}

	pub fn after_submit_request_map(&mut self, _idx: usize) {
		unreachable!("Spout スタブ")
	}

	pub fn send_mapped_rgba(&mut self, _device: &wgpu::Device) {
		unreachable!("Spout スタブ")
	}

	pub fn stats(&self) -> SpoutFrameStats {
		unreachable!("Spout スタブ")
	}

	pub fn encode_blit(
		&self,
		_encoder: &mut wgpu::CommandEncoder,
		_swap_view: &wgpu::TextureView,
		_target_width: u32,
		_target_height: u32,
		_clear: wgpu::Color,
	) {
		unreachable!("Spout スタブ")
	}
}
