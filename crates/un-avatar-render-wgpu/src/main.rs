#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! 独立プロセスのネイティブ・アバターウィンドウ。

fn main() -> Result<(), un_avatar_render_wgpu::RunError> {
	un_avatar_render_wgpu::run_cli()
}
