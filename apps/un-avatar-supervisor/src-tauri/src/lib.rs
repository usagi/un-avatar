use std::{
	collections::{BTreeMap, BTreeSet},
	env, fs,
	io::{BufRead, BufReader},
	net::SocketAddr,
	path::{Path, PathBuf},
	process::{Child, ChildStderr, Command, Stdio},
	sync::{
		atomic::{AtomicBool, Ordering},
		Arc, Mutex, OnceLock,
	},
	time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use tauri::{
	image::Image,
	menu::{Menu, MenuItem, Submenu},
	plugin::PermissionState,
	tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
	Manager, Runtime, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_notification::NotificationExt;

mod i18n;

// rust-i18n の compile-time セットアップ。`locales/` を権威ソースに、`backend` は
// `i18n::UN_I18N_STORE` の clone（共有 crate `un-i18n` の `UnI18nStore`）。
// 詳細は `i18n.rs` と <https://github.com/usagi/un-common>。
rust_i18n::i18n!("locales", fallback = "ja-JP", backend = (*crate::i18n::UN_I18N_STORE).clone());

use rust_i18n::t;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const MAIN_WINDOW_LABEL: &str = "main";
const APP_TITLE: &str = "UN Avatar";
const PROFILE_ICON_THUMBNAIL_MAX_DIMENSION: u32 = 256;
/// メインウィンドウのタイトルバー文字列。`CARGO_PKG_VERSION` を取り込んで
/// `Cargo.toml` 更新だけで自動反映 (ハードコード忘れによる事故防止)。表示形式は
/// `U.N. Avatar - 1.0.0` (UN Motion Supervisor と統一)。
fn app_title_with_version() -> String {
	format!("U.N. Avatar - {}", env!("CARGO_PKG_VERSION"))
}
const MAX_RENDERER_LOG_LINES: usize = 120;
const MAX_STOPPED_RENDERER_HISTORY: usize = 20;
const MAX_DIAGNOSTICS_PREVIEW_BYTES: u64 = 1024 * 1024;
const RENDERER_STOP_GRACE_NORMAL: Duration = Duration::from_millis(900);
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

type SpringBoneAuthoredParamsByCategory = BTreeMap<String, SpringBoneCategoryAuthoredParams>;
type SpringBoneAuthoredParamsCache = BTreeMap<String, SpringBoneAuthoredParamsByCategory>;

static SPRING_BONE_AUTHORED_PARAMS_CACHE: OnceLock<Mutex<SpringBoneAuthoredParamsCache>> = OnceLock::new();
static RUNTIME_SESSION_ID: OnceLock<String> = OnceLock::new();
static RUNTIME_CONTROL_SESSION: OnceLock<Mutex<Option<zenoh::Session>>> = OnceLock::new();

#[derive(Default)]
struct SupervisorState {
	next_id: u32,
	next_notification_id: u32,
	renderers: BTreeMap<u32, ManagedRenderer>,
	notifications: Vec<AppNotification>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
struct AppRuntimeSettings {
	system_tray_enabled: bool,
	minimize_to_tray: bool,
	close_to_tray_while_running: bool,
	start_minimized_to_tray: bool,
	crash_notifications: bool,
	stop_all_on_console_exit: bool,
	renderer_close_hotkey: String,
	quit_behavior: QuitBehavior,
	theme_mode: String,
	/// Avatar Settings の Quick Run ボタンで renderer を起動したあと Renderers タブに
	/// 自動で遷移するかどうか。デフォルトは false (元の編集画面のまま) で、
	/// 動作確認のためにすぐ起動後の renderer を見たいユーザーは ON にできる。
	#[serde(default)]
	jump_to_renderers_on_quick_run: bool,
	/// 起動時に Renderers の launch target として選択中の profile/group を自動起動する。
	#[serde(default)]
	auto_launch_selected_on_startup: bool,
	/// Profile editor の開発・診断用 control を表示するか。
	#[serde(default)]
	show_developer_controls: bool,
	/// 直前に選択していたアバター設定 ID。Renderers/Avatar Settings 画面の Launch 対象と編集対象。
	/// 起動時に存在するなら復元し、ユーザーが選び直したら都度書き戻す。
	last_selected_setting_id: Option<String>,
	/// Avatar model picker の直近ディレクトリ。未選択ならユーザーの Documents を初期位置にする。
	last_avatar_model_dir: Option<String>,
	/// 終了時の Supervisor Console ウィンドウの outer 位置（px）。None なら OS 既定位置で起動。
	console_window_x: Option<i32>,
	console_window_y: Option<i32>,
	/// 終了時の Supervisor Console ウィンドウの inner サイズ（px）。
	console_window_width: Option<u32>,
	console_window_height: Option<u32>,
	/// UI 表示言語 (BCP-47 完全形, 例: `ja-JP` / `en-US`)。空文字なら `i18n::resolve_default_locale`
	/// (OS locale → サポート言語 → `ja-JP` 最終フォールバック) で起動時に解決する。
	#[serde(default)]
	locale: String,
}

impl Default for AppRuntimeSettings {
	fn default() -> Self {
		Self {
			system_tray_enabled: false,
			minimize_to_tray: true,
			close_to_tray_while_running: true,
			start_minimized_to_tray: false,
			crash_notifications: true,
			stop_all_on_console_exit: false,
			renderer_close_hotkey: "Escape".to_string(),
			quit_behavior: QuitBehavior::Ask,
			theme_mode: "system".to_string(),
			jump_to_renderers_on_quick_run: false,
			auto_launch_selected_on_startup: false,
			show_developer_controls: false,
			last_selected_setting_id: None,
			last_avatar_model_dir: None,
			console_window_x: None,
			console_window_y: None,
			console_window_width: None,
			console_window_height: None,
			locale: String::new(),
		}
	}
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum QuitBehavior {
	Ask,
	StopRenderers,
	LeaveRenderers,
}

struct ManagedRenderer {
	info: RendererInstance,
	child: Child,
	started_at: Instant,
	runtime_bus_key: String,
	runtime_status_cache: Arc<Mutex<RendererRuntimeTelemetryCache>>,
	runtime_status_stream_stop: Arc<AtomicBool>,
	stderr_tail: Arc<Mutex<Vec<String>>>,
	crash_notified: bool,
}

#[derive(Default)]
struct RendererRuntimeTelemetryCache {
	telemetry: Option<RendererRuntimeTelemetry>,
	updated_at: Option<Instant>,
	last_error: Option<String>,
}

fn is_false(value: &bool) -> bool {
	!*value
}

#[derive(Clone, Serialize)]
struct AppNotification {
	id: u32,
	level: NotificationLevel,
	title: String,
	body: String,
	created_at_secs: u64,
}

#[derive(Serialize)]
struct NativeNotificationStatus {
	permission_state: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum NotificationLevel {
	#[allow(dead_code)]
	Info,
	#[allow(dead_code)]
	Warning,
	Error,
}

#[derive(Clone, Serialize)]
struct RendererInstance {
	id: u32,
	name: String,
	state: RendererState,
	pid: Option<u32>,
	uptime_secs: u64,
	avatar_path: Option<String>,
	manifest_path: Option<String>,
	vmc_address: Option<String>,
	vmc_port: Option<u16>,
	motion_vmc_enabled: bool,
	motion_unmotion_enabled: bool,
	unmotion_zenoh_key: Option<String>,
	/// VMC と UNMF/Z の同時受信時にどちらをアバターに反映するか。manifest `[motion] primary_source`
	/// に対応。`"vmc"` または `"unmotion_zenoh"`。
	primary_motion_source: String,
	spout_enabled: bool,
	spout_name: Option<String>,
	spout_width: Option<u32>,
	spout_height: Option<u32>,
	transparent: bool,
	input_passthrough: bool,
	decorations: bool,
	always_on_top: bool,
	window_width: u32,
	window_height: u32,
	last_stderr: Option<String>,
	stderr_tail: Vec<String>,
	exit_code: Option<i32>,
}

#[derive(Clone, Serialize)]
struct RendererRuntimeStatus {
	id: u32,
	state: RendererState,
	pid: Option<u32>,
	connected: bool,
	protocol: Option<String>,
	control_capabilities: Vec<String>,
	#[serde(default)]
	scene_state: String,
	uptime_secs: u64,
	fps: Option<f32>,
	cpu_ms: Option<f32>,
	frame_cpu_total_ms: Option<f32>,
	frame_motion_apply_ms: Option<f32>,
	frame_dynamics_step_ms: Option<f32>,
	frame_globals_ms: Option<f32>,
	frame_surface_acquire_ms: Option<f32>,
	frame_target_prepare_ms: Option<f32>,
	frame_draw_state_refresh_ms: Option<f32>,
	frame_scene_world_ms: Option<f32>,
	frame_draw_skin_palette_ms: Option<f32>,
	frame_draw_skin_palette_write_ms: Option<f32>,
	frame_draw_fur_source_vertices_ms: Option<f32>,
	frame_draw_expression_values_ms: Option<f32>,
	frame_draw_morph_weights_ms: Option<f32>,
	frame_draw_transform_loop_ms: Option<f32>,
	frame_bone_collider_debug_ms: Option<f32>,
	frame_command_encode_ms: Option<f32>,
	frame_submit_present_ms: Option<f32>,
	frame_spout_cpu_ms: Option<f32>,
	frame_contact_eval_ms: Option<f32>,
	frame_runtime_action_eval_ms: Option<f32>,
	gpu_ms: Option<f32>,
	ram_mb: Option<u64>,
	surface_width: Option<u32>,
	surface_height: Option<u32>,
	aa: Option<String>,
	texture_resolution_limit: Option<String>,
	texture_compression: Option<String>,
	mipmap_filter: Option<String>,
	processed_texture_cache: Option<bool>,
	texture_summary: Option<TextureRuntimeSummary>,
	spout_available: bool,
	spout_enabled: bool,
	spout_name: Option<String>,
	spout_width: Option<u32>,
	spout_height: Option<u32>,
	spout_frames_attempted: u64,
	spout_frames_sent: u64,
	spout_frame_failures: u64,
	spout_consecutive_failures: u32,
	spout_last_send_ok: Option<bool>,
	spout_last_readback_ms: Option<f32>,
	spout_last_send_ms: Option<f32>,
	spout_last_total_ms: Option<f32>,
	spout_sender_initialized: Option<bool>,
	spout_sender_width: Option<u32>,
	spout_sender_height: Option<u32>,
	#[serde(default)]
	expression_presets: Vec<String>,
	#[serde(default)]
	look_at_enabled: bool,
	#[serde(default)]
	look_at_clamp_deg: Option<f32>,
	/// renderer の `[motion] apply_vmc_root_translation` 現在値。Waidayo 等で意図せず Root.translation が
	/// 非ゼロで送られ、アバターが前後にズレる問題（model1.vrm "Root" を最初の scene root に持つケース）
	/// を回避するため既定 OFF。フルボディトラッカー利用時に Avatar Settings → Motion から ON。
	#[serde(default)]
	apply_vmc_root_translation: bool,
	/// renderer の `[motion.unmotion_zenoh] enabled` の生値（manifest/CLI）と、subscriber thread の
	/// 起動成功状況の双方を AND した値が renderer 側 telemetry の `unmotion_zenoh_enabled`。
	/// ここでは `true` のときに `un-motion-frame-zenoh` 経由で実際に Sub セッションが動いている。
	#[serde(default)]
	unmotion_zenoh_enabled: bool,
	/// renderer の `[motion.unmotion_zenoh] key`。subscribe key は `<key>/v1` に展開される。
	#[serde(default)]
	unmotion_zenoh_key: String,
	#[serde(default)]
	unmotion_zenoh_received_frames: u64,
	#[serde(default)]
	motion_applied_frames: u64,
	#[serde(default)]
	audio_link_texture_needed: bool,
	/// `"vmc"` / `"unmotion_zenoh"`。VMC と UNMotion 同時受信時の primary 選択値。
	#[serde(default)]
	primary_motion_source: String,
	#[serde(default)]
	show_axes: bool,
	#[serde(default)]
	show_bone_colliders: bool,
	#[serde(default)]
	bone_collider_count: u32,
	#[serde(default)]
	bone_collider_source: String,
	#[serde(default)]
	dynamics_group_count: u32,
	#[serde(default)]
	dynamics_enabled_group_count: u32,
	#[serde(default)]
	dynamics_source_enabled_group_count: u32,
	#[serde(default)]
	dynamics_enabled_override_count: u32,
	#[serde(default)]
	dynamics_vrm_spring_bone_group_count: u32,
	#[serde(default)]
	dynamics_vrc_physbone_group_count: u32,
	#[serde(default)]
	dynamics_unknown_group_count: u32,
	#[serde(default)]
	dynamics_limit_group_count: u32,
	#[serde(default)]
	dynamics_angle_limit_group_count: u32,
	#[serde(default)]
	dynamics_stretch_limit_group_count: u32,
	#[serde(default)]
	dynamics_rotation_translation_writeback_group_count: u32,
	#[serde(default)]
	dynamics_translation_writeback_candidate_count: u32,
	#[serde(default)]
	dynamics_translation_writeback_target_count: u32,
	#[serde(default)]
	dynamics_stretch_translation_writeback_group_count: u32,
	#[serde(default)]
	dynamics_stretch_translation_writeback_target_group_count: u32,
	#[serde(default)]
	dynamics_grabbing_enabled_group_count: u32,
	#[serde(default)]
	dynamics_posing_enabled_group_count: u32,
	#[serde(default)]
	dynamics_collider_count: u32,
	#[serde(default)]
	dynamics_vrm_spring_bone_collider_count: u32,
	#[serde(default)]
	dynamics_vrc_physbone_collider_count: u32,
	#[serde(default)]
	dynamics_unknown_collider_count: u32,
	#[serde(default)]
	dynamics_contact_count: u32,
	#[serde(default)]
	dynamics_vrc_contact_sender_count: u32,
	#[serde(default)]
	dynamics_vrc_contact_receiver_count: u32,
	#[serde(default)]
	dynamics_contact_parameter_declaration_count: u32,
	#[serde(default)]
	dynamics_contact_probe_count: u32,
	#[serde(default)]
	dynamics_contact_probe_would_emit_count: u32,
	#[serde(default)]
	dynamics_contact_parameter_emission_count: u32,
	#[serde(default)]
	dynamics_contact_parameter_emitted_count: u32,
	#[serde(default)]
	dynamics_contact_parameter_reset_to_zero_count: u32,
	#[serde(default)]
	dynamics_constraint_ref_count: u32,
	#[serde(default)]
	dynamics_vrc_constraint_ref_count: u32,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	runtime_parameter_definitions: Vec<serde_json::Value>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	runtime_parameter_conflicts: Vec<serde_json::Value>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	runtime_actions: Vec<serde_json::Value>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	runtime_action_target_write_collisions: Vec<serde_json::Value>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	runtime_action_restore_readiness: Vec<serde_json::Value>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	runtime_action_restore_baseline_candidates: Vec<serde_json::Value>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	runtime_action_restore_baseline_capture_plan: Vec<serde_json::Value>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	runtime_action_restore_apply_plan: Vec<serde_json::Value>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	menu_action_candidates: Vec<serde_json::Value>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	menu_wardrobe_candidates: Vec<serde_json::Value>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	contact_parameter_declarations: Vec<serde_json::Value>,
	#[serde(default, skip_serializing_if = "is_false")]
	contact_parameter_emission_enabled: bool,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	contact_parameter_emissions: Vec<serde_json::Value>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	contact_probes: Vec<serde_json::Value>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	dynamics_groups: Vec<serde_json::Value>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	dynamics_interaction_hooks: Vec<serde_json::Value>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	dynamics_colliders: Vec<serde_json::Value>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	dynamics_constraint_refs: Vec<serde_json::Value>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	dynamics_warnings: Vec<String>,
	#[serde(default)]
	camera_locked: bool,
	#[serde(default)]
	window_focused: bool,
	#[serde(default)]
	window_activation_seq: u64,
	#[serde(default)]
	minimized: bool,
	#[serde(default)]
	camera: Option<RendererCameraSnapshot>,
	/// 現在の背景クリアカラー（[r, g, b, a]）。`Alpha 0` / `Dark` / `Transparent` ボタンの active 判定に使う。
	#[serde(default)]
	clear_color: [f64; 4],
	/// renderer プロセスが透明ウィンドウ属性で起動したか（profile の `transparent`）。
	/// Windows + winit ではランタイム切替が効かないので、ボタン active 表示の根拠としてこの値を見る。
	#[serde(default)]
	transparent_window: bool,
	#[serde(default)]
	input_passthrough: bool,
	#[serde(default)]
	startup_phase: Option<String>,
	#[serde(default)]
	startup_progress: Option<[u32; 2]>,
	#[serde(default)]
	startup_message: Option<String>,
	note: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct VrmMetadataField {
	label: String,
	value: String,
}

#[derive(Clone, Debug, Serialize)]
struct VrmMetadataInfo {
	path: String,
	file_name: String,
	vrm_format: String,
	spec_version: String,
	title: Option<String>,
	version: Option<String>,
	authors: Vec<String>,
	contact_information: Option<String>,
	references: Vec<String>,
	copyright_information: Option<String>,
	third_party_licenses: Option<String>,
	license_name: Option<String>,
	other_license_url: Option<String>,
	other_permission_url: Option<String>,
	thumbnail_data_url: Option<String>,
	technical_stats: Vec<VrmMetadataField>,
	permissions: Vec<VrmMetadataField>,
}

#[derive(Clone, Serialize)]
struct UnavatarWardrobeOptions {
	available: bool,
	base_label: String,
	sets: Vec<UnavatarWardrobeSetOption>,
	#[serde(skip_serializing_if = "Option::is_none")]
	error: Option<String>,
}

#[derive(Clone, Serialize)]
struct UnavatarWardrobeSetOption {
	id: String,
	name: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
struct RendererCameraSnapshot {
	target: [f32; 3],
	longitude_deg: f32,
	latitude_deg: f32,
	radius: f32,
	diagonal_fov_deg: f32,
}

#[derive(Clone, Default, Deserialize, Serialize)]
struct TextureRuntimeSummary {
	#[serde(default)]
	image_count: u32,
	#[serde(default)]
	resized_count: u32,
	#[serde(default)]
	cubemap_count: u32,
	#[serde(default)]
	cubemap_converted_count: u32,
	#[serde(default)]
	cubemap_fallback_count: u32,
	#[serde(default)]
	compression_mode: Option<String>,
	#[serde(default)]
	compression_bc_supported: bool,
	#[serde(default)]
	compression_astc_supported: bool,
	#[serde(default)]
	compression_etc2_supported: bool,
	#[serde(default)]
	compressed_count: u32,
	#[serde(default)]
	compression_fallback_count: u32,
	#[serde(default)]
	compressed_mip_bytes: u64,
	#[serde(default)]
	cache_enabled: bool,
	#[serde(default)]
	cache_hits: u32,
	#[serde(default)]
	cache_misses: u32,
	#[serde(default)]
	cache_writes: u32,
	#[serde(default)]
	compressed_cache_hits: u32,
	#[serde(default)]
	compressed_cache_misses: u32,
	#[serde(default)]
	compressed_cache_writes: u32,
	#[serde(default)]
	source_bytes: u64,
	#[serde(default)]
	uploaded_mip_bytes: u64,
	#[serde(default)]
	cubemap_uploaded_bytes: u64,
	#[serde(default)]
	max_source_dimension: u32,
	#[serde(default)]
	max_uploaded_dimension: u32,
	#[serde(default)]
	limit_max_dimension: Option<u32>,
}

#[derive(Clone, Deserialize)]
struct RendererRuntimeTelemetry {
	connected: bool,
	#[serde(default)]
	protocol: Option<String>,
	#[serde(default)]
	control_capabilities: Vec<String>,
	#[serde(default)]
	scene_state: String,
	uptime_secs: u64,
	fps: Option<f32>,
	cpu_ms: Option<f32>,
	#[serde(default)]
	frame_cpu_total_ms: Option<f32>,
	#[serde(default)]
	frame_motion_apply_ms: Option<f32>,
	#[serde(default)]
	frame_dynamics_step_ms: Option<f32>,
	#[serde(default)]
	frame_globals_ms: Option<f32>,
	#[serde(default)]
	frame_surface_acquire_ms: Option<f32>,
	#[serde(default)]
	frame_target_prepare_ms: Option<f32>,
	#[serde(default)]
	frame_draw_state_refresh_ms: Option<f32>,
	#[serde(default)]
	frame_scene_world_ms: Option<f32>,
	#[serde(default)]
	frame_draw_skin_palette_ms: Option<f32>,
	#[serde(default)]
	frame_draw_skin_palette_write_ms: Option<f32>,
	#[serde(default)]
	frame_draw_fur_source_vertices_ms: Option<f32>,
	#[serde(default)]
	frame_draw_expression_values_ms: Option<f32>,
	#[serde(default)]
	frame_draw_morph_weights_ms: Option<f32>,
	#[serde(default)]
	frame_draw_transform_loop_ms: Option<f32>,
	#[serde(default)]
	frame_bone_collider_debug_ms: Option<f32>,
	#[serde(default)]
	frame_command_encode_ms: Option<f32>,
	#[serde(default)]
	frame_submit_present_ms: Option<f32>,
	#[serde(default)]
	frame_spout_cpu_ms: Option<f32>,
	#[serde(default)]
	frame_contact_eval_ms: Option<f32>,
	#[serde(default)]
	frame_runtime_action_eval_ms: Option<f32>,
	gpu_ms: Option<f32>,
	ram_mb: Option<u64>,
	surface_width: Option<u32>,
	surface_height: Option<u32>,
	/// 現在のウィンドウ outer 位置（px）。Save Window State でプロファイル `[window] x/y` に書き戻す。
	#[serde(default)]
	window_position: Option<[i32; 2]>,
	/// 現在のウィンドウ inner サイズ（px）。Save Window State でプロファイル `[window] width/height` に書き戻す。
	#[serde(default)]
	window_inner_size: Option<[u32; 2]>,
	#[serde(default)]
	aa: Option<String>,
	#[serde(default)]
	texture_resolution_limit: Option<String>,
	#[serde(default)]
	texture_compression: Option<String>,
	#[serde(default)]
	mipmap_filter: Option<String>,
	#[serde(default)]
	processed_texture_cache: Option<bool>,
	#[serde(default)]
	texture_summary: Option<TextureRuntimeSummary>,
	#[serde(default)]
	spout_available: bool,
	spout_enabled: bool,
	spout_name: Option<String>,
	spout_width: Option<u32>,
	spout_height: Option<u32>,
	#[serde(default)]
	spout_frames_attempted: u64,
	#[serde(default)]
	spout_frames_sent: u64,
	#[serde(default)]
	spout_frame_failures: u64,
	#[serde(default)]
	spout_consecutive_failures: u32,
	#[serde(default)]
	spout_last_send_ok: Option<bool>,
	#[serde(default)]
	spout_last_readback_ms: Option<f32>,
	#[serde(default)]
	spout_last_send_ms: Option<f32>,
	#[serde(default)]
	spout_last_total_ms: Option<f32>,
	#[serde(default)]
	spout_sender_initialized: Option<bool>,
	#[serde(default)]
	spout_sender_width: Option<u32>,
	#[serde(default)]
	spout_sender_height: Option<u32>,
	#[serde(default)]
	expression_presets: Vec<String>,
	#[serde(default)]
	look_at_enabled: bool,
	#[serde(default)]
	look_at_clamp_deg: Option<f32>,
	#[serde(default)]
	apply_vmc_root_translation: bool,
	#[serde(default)]
	unmotion_zenoh_enabled: bool,
	#[serde(default)]
	unmotion_zenoh_key: String,
	#[serde(default)]
	unmotion_zenoh_received_frames: u64,
	#[serde(default)]
	motion_applied_frames: u64,
	#[serde(default)]
	audio_link_texture_needed: bool,
	#[serde(default)]
	primary_motion_source: String,
	#[serde(default)]
	show_axes: bool,
	#[serde(default)]
	show_bone_colliders: bool,
	#[serde(default)]
	bone_collider_count: u32,
	#[serde(default)]
	bone_collider_source: String,
	#[serde(default)]
	dynamics_group_count: u32,
	#[serde(default)]
	dynamics_enabled_group_count: u32,
	#[serde(default)]
	dynamics_source_enabled_group_count: u32,
	#[serde(default)]
	dynamics_enabled_override_count: u32,
	#[serde(default)]
	dynamics_vrm_spring_bone_group_count: u32,
	#[serde(default)]
	dynamics_vrc_physbone_group_count: u32,
	#[serde(default)]
	dynamics_unknown_group_count: u32,
	#[serde(default)]
	dynamics_limit_group_count: u32,
	#[serde(default)]
	dynamics_angle_limit_group_count: u32,
	#[serde(default)]
	dynamics_stretch_limit_group_count: u32,
	#[serde(default)]
	dynamics_rotation_translation_writeback_group_count: u32,
	#[serde(default)]
	dynamics_translation_writeback_candidate_count: u32,
	#[serde(default)]
	dynamics_translation_writeback_target_count: u32,
	#[serde(default)]
	dynamics_stretch_translation_writeback_group_count: u32,
	#[serde(default)]
	dynamics_stretch_translation_writeback_target_group_count: u32,
	#[serde(default)]
	dynamics_grabbing_enabled_group_count: u32,
	#[serde(default)]
	dynamics_posing_enabled_group_count: u32,
	#[serde(default)]
	dynamics_collider_count: u32,
	#[serde(default)]
	dynamics_vrm_spring_bone_collider_count: u32,
	#[serde(default)]
	dynamics_vrc_physbone_collider_count: u32,
	#[serde(default)]
	dynamics_unknown_collider_count: u32,
	#[serde(default)]
	dynamics_contact_count: u32,
	#[serde(default)]
	dynamics_vrc_contact_sender_count: u32,
	#[serde(default)]
	dynamics_vrc_contact_receiver_count: u32,
	#[serde(default)]
	dynamics_contact_parameter_declaration_count: u32,
	#[serde(default)]
	dynamics_contact_probe_count: u32,
	#[serde(default)]
	dynamics_contact_probe_would_emit_count: u32,
	#[serde(default)]
	dynamics_contact_parameter_emission_count: u32,
	#[serde(default)]
	dynamics_contact_parameter_emitted_count: u32,
	#[serde(default)]
	dynamics_contact_parameter_reset_to_zero_count: u32,
	#[serde(default)]
	dynamics_constraint_ref_count: u32,
	#[serde(default)]
	dynamics_vrc_constraint_ref_count: u32,
	#[serde(default)]
	runtime_parameter_definitions: Vec<serde_json::Value>,
	#[serde(default)]
	runtime_parameter_conflicts: Vec<serde_json::Value>,
	#[serde(default)]
	runtime_actions: Vec<serde_json::Value>,
	#[serde(default)]
	runtime_action_target_write_collisions: Vec<serde_json::Value>,
	#[serde(default)]
	runtime_action_restore_readiness: Vec<serde_json::Value>,
	#[serde(default)]
	runtime_action_restore_baseline_candidates: Vec<serde_json::Value>,
	#[serde(default)]
	runtime_action_restore_baseline_capture_plan: Vec<serde_json::Value>,
	#[serde(default)]
	runtime_action_restore_apply_plan: Vec<serde_json::Value>,
	#[serde(default)]
	menu_action_candidates: Vec<serde_json::Value>,
	#[serde(default)]
	menu_wardrobe_candidates: Vec<serde_json::Value>,
	#[serde(default)]
	contact_parameter_declarations: Vec<serde_json::Value>,
	#[serde(default)]
	contact_parameter_emission_enabled: bool,
	#[serde(default)]
	contact_parameter_emissions: Vec<serde_json::Value>,
	#[serde(default)]
	contact_probes: Vec<serde_json::Value>,
	#[serde(default)]
	dynamics_groups: Vec<serde_json::Value>,
	#[serde(default)]
	dynamics_interaction_hooks: Vec<serde_json::Value>,
	#[serde(default)]
	dynamics_colliders: Vec<serde_json::Value>,
	#[serde(default)]
	dynamics_constraint_refs: Vec<serde_json::Value>,
	#[serde(default)]
	dynamics_warnings: Vec<String>,
	#[serde(default)]
	camera_locked: bool,
	#[serde(default)]
	window_focused: bool,
	#[serde(default)]
	window_activation_seq: u64,
	#[serde(default)]
	minimized: bool,
	#[serde(default)]
	camera: Option<RendererCameraSnapshot>,
	#[serde(default)]
	clear_color: [f64; 4],
	#[serde(default)]
	transparent_window: bool,
	#[serde(default)]
	input_passthrough: bool,
	#[serde(default)]
	startup_phase: Option<String>,
	#[serde(default)]
	startup_progress: Option<[u32; 2]>,
	#[serde(default)]
	startup_message: Option<String>,
	note: Option<String>,
}

#[derive(Serialize)]
struct SupervisorDiagnosticsBundle {
	version: &'static str,
	generated_at_secs: u64,
	repo_root: String,
	build: SupervisorBuildInfo,
	app_settings: AppRuntimeSettings,
	native_notifications: NativeNotificationStatus,
	profiles: SupervisorProfileDiagnostics,
	renderers: Vec<RendererDiagnostics>,
	notifications: Vec<AppNotification>,
}

#[derive(Serialize)]
struct SupervisorProfileDiagnostics {
	seed_dir: String,
	user_dir: String,
	settings: Vec<AvatarSetting>,
	tray_launch_settings: Vec<AvatarSetting>,
	error: Option<String>,
}

#[derive(Serialize)]
struct RendererDiagnostics {
	info: RendererInstance,
	runtime_bus_key: String,
	runtime_status: RendererRuntimeStatus,
}

#[derive(Clone, Serialize)]
struct DiagnosticsExportEntry {
	path: String,
	archive_path: Option<String>,
	generated_at_secs: Option<u64>,
	modified_at_secs: Option<u64>,
	size_bytes: u64,
	archive_size_bytes: Option<u64>,
}

#[derive(Serialize)]
struct SupervisorBuildInfo {
	supervisor_version: &'static str,
	frontend_version: Option<String>,
	git_head: Option<String>,
	current_exe: Option<String>,
	renderer_exe: String,
}

#[derive(Clone, Copy, Serialize)]
struct RendererBoneColliderConfig {
	enabled: bool,
	radius_mm: RendererBoneColliderRadiiMm,
}

#[derive(Clone, Copy, Serialize)]
struct RendererBoneColliderRadiiMm {
	head: f32,
	neck_chest: f32,
	torso: f32,
	upper_arms: f32,
	lower_arms: f32,
	hands: f32,
}

#[derive(Clone, Serialize)]
struct RendererSpringBonePhysicsConfig {
	time_mode: String,
	simulation_hz: f32,
	substeps: u32,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	overrides: Vec<RendererSpringBoneCategoryOverride>,
}

#[derive(Clone, Serialize)]
struct RendererSpringBoneCategoryOverride {
	category: String,
	#[serde(flatten)]
	params: RendererSpringBonePhysicsParams,
}

#[derive(Clone, Serialize)]
struct RendererSpringBonePhysicsParams {
	#[serde(skip_serializing_if = "Option::is_none")]
	solver: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	damping_half_life_ms: Option<f32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	stiffness_hz: Option<f32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	xpbd_compliance: Option<f32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	gravity_scale: Option<f32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	drag_scale: Option<f32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	constraint_iterations: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
struct RendererSpringBoneSetting {
	spring_bones: bool,
	spring_bone_physics_configured: bool,
	spring_bone_simulation_hz: f32,
	spring_bone_substeps: u32,
	spring_bone_category_overrides: Vec<SpringBoneCategoryOverrideSetting>,
	bone_colliders_enabled: bool,
	bone_collider_head: f32,
	bone_collider_neck_chest: f32,
	bone_collider_torso: f32,
	bone_collider_upper_arms: f32,
	bone_collider_lower_arms: f32,
	bone_collider_hands: f32,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
struct RendererAllDynamicsSetting {
	dynamics_enable_all_on_launch: bool,
}

#[derive(Serialize, Deserialize)]
struct RendererCameraTransition {
	#[serde(alias = "durationMs")]
	duration_ms: u32,
	easing: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	mode: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AvatarSettingValueUpdate {
	field: String,
	value: serde_json::Value,
}

#[derive(Serialize)]
#[serde(tag = "command", rename_all = "snake_case")]
enum RendererControlCommand {
	Shutdown,
	ResetCamera,
	SetClearColor {
		r: f64,
		g: f64,
		b: f64,
		a: f64,
	},
	SetSpoutOutput {
		enabled: bool,
		name: Option<String>,
		width: Option<u32>,
		height: Option<u32>,
	},
	SetWindow {
		decorations: Option<bool>,
		transparent: Option<bool>,
		input_passthrough: Option<bool>,
		always_on_top: Option<bool>,
		#[serde(skip_serializing_if = "Option::is_none")]
		minimized: Option<bool>,
		width: Option<u32>,
		height: Option<u32>,
	},
	Screenshot {
		path: String,
	},
	SetExpressionOverride {
		name: String,
		weight: f32,
	},
	ActivateAction {
		#[serde(skip_serializing_if = "Option::is_none")]
		action_id: Option<String>,
		#[serde(skip_serializing_if = "Option::is_none")]
		menu_path: Option<String>,
		#[serde(skip_serializing_if = "Option::is_none")]
		wardrobe_set_id: Option<String>,
	},
	ClearExpressionOverrides,
	SetLookAt {
		enabled: bool,
		clamp_deg: Option<f32>,
	},
	Activate,
	SetShowAxes {
		enabled: bool,
	},
	SetShowBoneColliders {
		enabled: bool,
	},
	SetCameraLock {
		locked: bool,
	},
	SetCameraFov {
		diagonal_deg: f32,
	},
	SetCameraState {
		#[serde(skip_serializing_if = "Option::is_none")]
		target: Option<[f32; 3]>,
		#[serde(skip_serializing_if = "Option::is_none")]
		longitude_deg: Option<f32>,
		#[serde(skip_serializing_if = "Option::is_none")]
		latitude_deg: Option<f32>,
		#[serde(skip_serializing_if = "Option::is_none")]
		radius: Option<f32>,
		#[serde(skip_serializing_if = "Option::is_none")]
		diagonal_fov_deg: Option<f32>,
		#[serde(skip_serializing_if = "Option::is_none")]
		transition: Option<RendererCameraTransition>,
	},
	/// プロファイル `[window] x/y` を実行中レンダラーへ反映する用。`SetWindow` の x/y 拡張版。
	SetWindowPosition {
		#[serde(skip_serializing_if = "Option::is_none")]
		x: Option<i32>,
		#[serde(skip_serializing_if = "Option::is_none")]
		y: Option<i32>,
	},
	/// VMC `Root.translation` を scene root に加算するかの切替。
	SetApplyVmcRootTranslation {
		enabled: bool,
	},
	/// VMC と UNMF/Z 両方を受信できる構成での primary 選択。
	/// `source` は `"vmc"` か `"unmotion_zenoh"`（`crate::options::PrimaryMotionSource` と同じ）。
	SetPrimaryMotionSource {
		source: String,
	},
	SetMotionReceivers {
		#[serde(skip_serializing_if = "Option::is_none")]
		vmc_address: Option<String>,
		unmotion_zenoh_enabled: bool,
		unmotion_zenoh_key: String,
	},
	SetSpringBones {
		enabled: bool,
		bone_colliders: RendererBoneColliderConfig,
		#[serde(skip_serializing_if = "Option::is_none")]
		physics_config: Option<RendererSpringBonePhysicsConfig>,
	},
	SetDynamicsEnabled {
		source_id: String,
		enabled: bool,
	},
	SetAllDynamicsEnabled {
		enabled: bool,
	},
	SetAvatarOutline {
		policy: Option<String>,
		#[serde(skip_serializing_if = "Option::is_none")]
		r#type: Option<String>,
		width: Option<f32>,
		color: Option<[f32; 3]>,
		lighting_mix: Option<f32>,
		roundness: Option<f32>,
	},
	SetAvatarRim {
		policy: Option<String>,
		color: Option<[f32; 3]>,
		intensity: Option<f32>,
		lighting_mix: Option<f32>,
		fresnel_power: Option<f32>,
		lift: Option<f32>,
	},
	SetAvatarMatcap {
		scale: Option<f32>,
	},
	SetAvatarSpecular {
		enabled: Option<bool>,
		intensity: Option<f32>,
		power: Option<f32>,
	},
	SetAvatarAmbientOcclusion {
		strength: Option<f32>,
	},
	SetLighting {
		environment_enabled: Option<bool>,
		environment_color: Option<[f32; 3]>,
		environment_intensity: Option<f32>,
		directional_enabled: Option<bool>,
		directional_color: Option<[f32; 3]>,
		directional_intensity: Option<f32>,
		directional_azimuth_deg: Option<f32>,
		directional_elevation_deg: Option<f32>,
		directional_follow_camera_yaw: Option<bool>,
		directional_follow_camera_pitch: Option<bool>,
	},
	SetEnvironmentColor {
		exposure: Option<f32>,
		contrast: Option<f32>,
		saturation: Option<f32>,
		look: Option<String>,
		intensity: Option<f32>,
		temperature: Option<f32>,
		tint: Option<f32>,
	},
	SetBloom {
		enabled: Option<bool>,
		strength: Option<f32>,
		threshold: Option<f32>,
		radius: Option<f32>,
		quality: Option<String>,
	},
	SetSsao {
		enabled: Option<bool>,
		strength: Option<f32>,
		radius: Option<f32>,
		bias: Option<f32>,
		range: Option<f32>,
	},
	SetContactShadow {
		enabled: Option<bool>,
		strength: Option<f32>,
		radius: Option<f32>,
		softness: Option<f32>,
		height: Option<f32>,
	},
}

#[derive(Clone, Serialize)]
enum RendererState {
	#[allow(dead_code)]
	Starting,
	Running,
	Stopping,
	Exited,
	Crashed,
	Degraded,
}

#[derive(Clone, Serialize)]
struct AvatarSetting {
	id: String,
	name: String,
	created_at: String,
	sort_order: u32,
	storage: ProfileStorage,
	manifest_path: String,
	avatar_path: Option<String>,
	wardrobe_set: Option<String>,
	vmc_address: Option<String>,
	vmc_port: Option<u16>,
	motion_vmc_enabled: bool,
	motion_unmotion_enabled: bool,
	unmotion_zenoh_key: Option<String>,
	audio_link_source: String,
	audio_link_input_device_id: Option<String>,
	audio_link_input_device_name_hint: Option<String>,
	look_at_enabled: bool,
	look_at_clamp_deg: Option<f32>,
	/// VMC と UNMF/Z の両方を受信可能な構成で primary 側として実際にアバターに反映する
	/// motion source の選択。manifest `[motion] primary_source` に対応。
	/// 値は `"vmc"` / `"unmotion_zenoh"`。未指定時は VMC（Phase 1 からの既存挙動）。
	primary_motion_source: String,
	/// UNPhysics / UNDynamics の runtime solver を有効化するか。
	/// manifest `[physics.dynamics] enabled` に対応。
	spring_bones: bool,
	/// 起動後に authored default OFF を含む dynamics group を明示的に全 ON へ上書きするか。
	/// manifest `[physics.dynamics] enable_all_on_launch` に対応。既定 false。
	dynamics_enable_all_on_launch: bool,
	/// VRC Contact Receiver の runtime parameter emission を有効化するか。
	/// manifest `[physics.contacts] parameter_emission` に対応。既定 false。
	contact_parameter_emission: bool,
	spring_bone_physics_configured: bool,
	spring_bone_simulation_hz: f32,
	spring_bone_substeps: u32,
	spring_bone_category_overrides: Vec<SpringBoneCategoryOverrideSetting>,
	/// VMC `/VMC/Ext/Root/Pos` の translation を scene root へ加算するか。
	/// manifest `[motion] apply_vmc_root_translation` に対応。既定 false（Waidayo 等の calibration 都合で
	/// 意図せず非ゼロな translation が送られアバターが前後にズレる問題を防ぐため）。フルボディトラッカー
	/// で位置移動も載せたい時のみ true にする。
	apply_vmc_root_translation: bool,
	spout_enabled: bool,
	spout_name: Option<String>,
	spout_width: Option<u32>,
	spout_height: Option<u32>,
	aa: String,
	texture_resolution_limit: String,
	texture_compression: String,
	mipmap_filter: String,
	render_backend: String,
	block_compression_encoder: String,
	block_compression_cpu_threads: usize,
	/// Advanced モードで参照する、テクスチャ用途別の圧縮 preference。
	/// 開発者向けの用途別 override。8 役割 × 5 preference (source/auto/high_quality/small/gpu_native)。
	texture_compression_advanced: TextureCompressionAdvancedSetting,
	processed_texture_cache: bool,
	skin_tone_matching: bool,
	background_color: [f32; 3],
	transparent: bool,
	input_passthrough: bool,
	decorations: bool,
	always_on_top: bool,
	/// 起動直後にウィンドウを最小化するか。`[window] minimized` に対応。
	minimized: bool,
	/// XYZ デバッグ軸の初期表示。`[debug] show_axes` に対応。
	show_axes: bool,
	/// ボーンベースコライダーの debug 表示初期値。`[debug] show_bone_colliders` に対応。
	show_bone_colliders: bool,
	bone_colliders_enabled: bool,
	bone_collider_head: f32,
	bone_collider_neck_chest: f32,
	bone_collider_torso: f32,
	bone_collider_upper_arms: f32,
	bone_collider_lower_arms: f32,
	bone_collider_hands: f32,
	/// MToon outline 描画を無効化する診断 toggle。`[debug] disable_mtoon_outlines` に対応。
	/// 一部 VRM モデルで目周辺に肌色寄りの太い outline が出る現象の切り分け用。
	debug_disable_mtoon_outlines: bool,
	/// MToon の parametric Rim Lighting 寄与を 0 にする診断 toggle。`[debug] disable_rim_lighting` に対応。
	debug_disable_rim_lighting: bool,
	/// `shading_shift_factor` と `shadingShiftTexture` の寄与を 0 固定にする診断 toggle。
	/// `[debug] force_shading_shift_zero` に対応。
	debug_force_shading_shift_zero: bool,
	/// matcap (sphere add) 寄与を 0 にする診断 toggle。`[debug] disable_matcap` に対応。
	debug_disable_matcap: bool,
	/// emissive 寄与を 0 にする診断 toggle。`[debug] disable_emissive` に対応。
	debug_disable_emissive: bool,
	/// MToon `shade_color × shade_tex` の代わりに base を使う診断 toggle。`[debug] disable_shade_color` に対応。
	debug_disable_shade_color: bool,
	/// normalTexture を使わず頂点法線のみで shading / rim を計算する診断 toggle。`[debug] disable_normal_map` に対応。
	debug_disable_normal_map: bool,
	/// fs_mtoon を base のみで早期 return する診断 toggle。`[debug] base_texture_only` に対応。
	debug_base_texture_only: bool,
	/// アバター outline の扱い。`[effects.avatar.outline] policy` に対応。
	outline_policy: String,
	/// アバター outline の種類。v1 は `mtoon` のみ描画差分あり。`ink` / `brush` / `double` は予約値。
	outline_type: String,
	/// アバター outline の幅（メートル）。`None` は authored 値。
	outline_width: Option<f32>,
	/// アバター outline の色（linear RGB 0..1）。`None` は authored 値。
	outline_color: Option<[f32; 3]>,
	/// アバター outline にライティングを混ぜる量。0 は完全な指定色、1 は authored lighting mix 相当。
	outline_lighting_mix: Option<f32>,
	/// UNAvatar screen-space outline の角の丸み。0 は角張る、1 は丸い。
	outline_roundness: Option<f32>,
	rim_policy: String,
	rim_color: Option<[f32; 3]>,
	rim_intensity: Option<f32>,
	rim_lighting_mix: Option<f32>,
	rim_fresnel_power: Option<f32>,
	rim_lift: Option<f32>,
	matcap_scale: f32,
	specular_enabled: bool,
	specular_intensity: f32,
	specular_power: f32,
	ambient_occlusion_strength: f32,
	lighting_environment_enabled: bool,
	lighting_environment_color: [f32; 3],
	lighting_environment_intensity: f32,
	lighting_directional_enabled: bool,
	lighting_directional_color: [f32; 3],
	lighting_directional_intensity: f32,
	lighting_directional_azimuth_deg: f32,
	lighting_directional_elevation_deg: f32,
	lighting_directional_follow_camera_yaw: bool,
	lighting_directional_follow_camera_pitch: bool,
	color_exposure: f32,
	color_contrast: f32,
	color_saturation: f32,
	color_look: String,
	color_look_intensity: f32,
	color_temperature: f32,
	color_tint: f32,
	bloom_enabled: bool,
	bloom_strength: f32,
	bloom_threshold: f32,
	bloom_radius: f32,
	bloom_quality: String,
	ssao_enabled: bool,
	ssao_strength: f32,
	ssao_radius: f32,
	ssao_bias: f32,
	ssao_range: f32,
	contact_shadow_enabled: bool,
	contact_shadow_strength: f32,
	contact_shadow_radius: f32,
	contact_shadow_softness: f32,
	contact_shadow_height: f32,
	/// カメラ操作ロックの初期状態。`[camera] locked` に対応。
	camera_locked: bool,
	/// レンダラーウィンドウの起動時 outer 位置（px）。manifest `[window] x`/`[window] y` に対応。
	/// 両方が `Some` のときだけ renderer 側に適用される。
	window_x: Option<i32>,
	window_y: Option<i32>,
	/// カメラ注視点の初期座標。manifest `[camera] target = [x, y, z]` に対応。
	/// `None` のとき renderer 既定値（モデル中心）を使用。
	camera_target: Option<[f32; 3]>,
	/// カメラの経度（degree）。manifest `[camera] longitude_deg` に対応。
	camera_longitude_deg: Option<f32>,
	/// カメラの緯度（degree）。manifest `[camera] latitude_deg` に対応。
	camera_latitude_deg: Option<f32>,
	/// 注視点からカメラまでの距離（メートル）。manifest `[camera] radius` に対応。
	camera_radius: Option<f32>,
	/// 対角視野角（degree）。manifest `[camera] diagonal_fov_deg` に対応。35mm 換算 ≒ 35。
	camera_diagonal_fov_deg: Option<f32>,
	window_width: u32,
	window_height: u32,
	icon_path: Option<String>,
	allow_multiple_renderers: bool,
	notes: Option<String>,
	group: String,
}

struct PostEffectSettings {
	bloom_enabled: bool,
	bloom_strength: f32,
	bloom_threshold: f32,
	bloom_radius: f32,
	bloom_quality: String,
	ssao_enabled: bool,
	ssao_strength: f32,
	ssao_radius: f32,
	ssao_bias: f32,
	ssao_range: f32,
}

struct AvatarEffectSettings {
	outline_policy: String,
	outline_type: String,
	outline_width: Option<f32>,
	outline_color: Option<[f32; 3]>,
	outline_lighting_mix: Option<f32>,
	outline_roundness: Option<f32>,
	rim_policy: String,
	rim_color: Option<[f32; 3]>,
	rim_intensity: Option<f32>,
	rim_lighting_mix: Option<f32>,
	rim_fresnel_power: Option<f32>,
	rim_lift: Option<f32>,
	matcap_scale: f32,
	specular_enabled: bool,
	specular_intensity: f32,
	specular_power: f32,
	ambient_occlusion_strength: f32,
	contact_shadow_enabled: bool,
	contact_shadow_strength: f32,
	contact_shadow_radius: f32,
	contact_shadow_softness: f32,
	contact_shadow_height: f32,
}

struct ColorAdjustmentSettings {
	color_exposure: f32,
	color_contrast: f32,
	color_saturation: f32,
	color_look: String,
	color_look_intensity: f32,
	color_temperature: f32,
	color_tint: f32,
}

struct LightingSettings {
	lighting_environment_enabled: bool,
	lighting_environment_color: [f32; 3],
	lighting_environment_intensity: f32,
	lighting_directional_enabled: bool,
	lighting_directional_color: [f32; 3],
	lighting_directional_intensity: f32,
	lighting_directional_azimuth_deg: f32,
	lighting_directional_elevation_deg: f32,
	lighting_directional_follow_camera_yaw: bool,
	lighting_directional_follow_camera_pitch: bool,
}

struct DebugSettings {
	show_axes: bool,
	show_bone_colliders: bool,
	disable_mtoon_outlines: bool,
	disable_rim_lighting: bool,
	force_shading_shift_zero: bool,
	disable_matcap: bool,
	disable_emissive: bool,
	disable_shade_color: bool,
	disable_normal_map: bool,
	base_texture_only: bool,
}

struct PhysicsSettings {
	dynamics_enabled: Option<bool>,
	dynamics_enable_all_on_launch: bool,
	contact_parameter_emission: bool,
	spring_bone_physics_configured: bool,
	spring_bone_simulation_hz: f32,
	spring_bone_substeps: u32,
	spring_bone_category_overrides: Vec<SpringBoneCategoryOverrideSetting>,
	bone_colliders_enabled: bool,
	bone_collider_head: f32,
	bone_collider_neck_chest: f32,
	bone_collider_torso: f32,
	bone_collider_upper_arms: f32,
	bone_collider_lower_arms: f32,
	bone_collider_hands: f32,
}

struct RenderQualitySettings {
	aa: String,
	texture_resolution_limit: String,
	texture_compression: String,
	mipmap_filter: String,
	render_backend: String,
	block_compression_encoder: String,
	block_compression_cpu_threads: usize,
	texture_compression_advanced: TextureCompressionAdvancedSetting,
	processed_texture_cache: bool,
	skin_tone_matching: bool,
}

struct MotionSettings {
	vmc_address: Option<String>,
	vmc_port: Option<u16>,
	motion_vmc_enabled: bool,
	motion_unmotion_enabled: bool,
	unmotion_zenoh_key: Option<String>,
	look_at_enabled: bool,
	look_at_clamp_deg: Option<f32>,
	apply_vmc_root_translation: bool,
	primary_motion_source: String,
}

struct AudioLinkSettings {
	source: String,
	input_device_id: Option<String>,
	input_device_name_hint: Option<String>,
}

struct WindowSettings {
	icon_path: Option<String>,
	transparent: bool,
	input_passthrough: bool,
	decorations: bool,
	always_on_top: bool,
	minimized: bool,
	x: Option<i32>,
	y: Option<i32>,
	width: u32,
	height: u32,
}

struct CameraSettings {
	locked: bool,
	target: Option<[f32; 3]>,
	longitude_deg: Option<f32>,
	latitude_deg: Option<f32>,
	radius: Option<f32>,
	diagonal_fov_deg: Option<f32>,
}

struct OutputSettings {
	spout_enabled: bool,
	spout_name: Option<String>,
	spout_width: Option<u32>,
	spout_height: Option<u32>,
}

#[derive(Clone, Serialize, Deserialize)]
struct SpringBoneCategoryOverrideSetting {
	category: String,
	name: String,
	mode: String,
	spring_bone_count: usize,
	solver: String,
	damping_configured: bool,
	damping_half_life_ms: f32,
	stiffness_configured: bool,
	stiffness_hz: f32,
	xpbd_compliance_configured: bool,
	xpbd_compliance: f32,
	constraint_iterations_configured: bool,
	constraint_iterations: u32,
	authored_stiffness_hz: f32,
	authored_xpbd_compliance: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum ProfileStorage {
	Seed,
	User,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "snake_case")]
struct TextureCompressionAdvancedSetting {
	face: String,
	eyes: String,
	clothing: String,
	normal: String,
	occlusion: String,
	emissive: String,
	generic_color: String,
	data: String,
}

impl TextureCompressionAdvancedSetting {
	/// renderer 側の `TextureCompressionAdvancedOptions::default()` と一致させる値で初期化する。
	/// face/eyes/data は source、normal/occlusion は gpu_native、emissive は high_quality、その他は auto。
	fn renderer_defaults() -> Self {
		Self {
			face: "source".to_string(),
			eyes: "source".to_string(),
			clothing: "auto".to_string(),
			normal: "gpu_native".to_string(),
			occlusion: "gpu_native".to_string(),
			emissive: "high_quality".to_string(),
			generic_color: "auto".to_string(),
			data: "source".to_string(),
		}
	}

	fn from_manifest(advanced: Option<ManifestTextureCompressionAdvanced>) -> Self {
		let defaults = Self::renderer_defaults();
		let Some(a) = advanced else { return defaults };
		Self {
			face: a.face.unwrap_or(defaults.face),
			eyes: a.eyes.unwrap_or(defaults.eyes),
			clothing: a.clothing.unwrap_or(defaults.clothing),
			normal: a.normal.unwrap_or(defaults.normal),
			occlusion: a.occlusion.unwrap_or(defaults.occlusion),
			emissive: a.emissive.unwrap_or(defaults.emissive),
			generic_color: a.generic_color.unwrap_or(defaults.generic_color),
			data: a.data.unwrap_or(defaults.data),
		}
	}
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestProfile {
	id: Option<String>,
	display_name: Option<String>,
	created_at: Option<String>,
	sort_order: Option<u32>,
	allow_multiple_renderers: Option<bool>,
	notes: Option<String>,
	group: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestSpout {
	enabled: Option<bool>,
	name: Option<String>,
	width: Option<u32>,
	height: Option<u32>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestMotion {
	vmc_udp: Option<ManifestVmcUdp>,
	unmotion_zenoh: Option<ManifestUnmotionZenoh>,
	look_at: Option<ManifestLookAt>,
	/// VMC `/VMC/Ext/Root/Pos` の translation を scene root に加算するか。
	/// 既定 false。Waidayo 等で意図せず非ゼロな Root.translation が送られアバターが前後にズレる
	/// 問題を回避するため、フルボディトラッカー利用時のみ true にする。
	apply_vmc_root_translation: Option<bool>,
	/// VMC と UNMF/Z 両方を受信可能なときに、どちらをアバターに反映させるか。
	/// 旧 manifest 互換の primary source。現在の renderer は姿勢入力を key 単位で後着優先マージする。
	primary_source: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestAudioLink {
	source: Option<String>,
	input_device_id: Option<String>,
	input_device_name_hint: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestLookAt {
	enabled: Option<bool>,
	clamp_deg: Option<f32>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestVmcUdp {
	enabled: Option<bool>,
	address: Option<String>,
	port: Option<u16>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestUnmotionZenoh {
	enabled: Option<bool>,
	key: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestOutput {
	spout2: Option<ManifestSpout>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestRenderQuality {
	aa: Option<String>,
	texture_resolution_limit: Option<String>,
	texture_compression: Option<String>,
	mipmap_filter: Option<String>,
	render_backend: Option<String>,
	block_compression_encoder: Option<String>,
	block_compression_cpu_threads: Option<usize>,
	processed_texture_cache: Option<bool>,
	skin_tone_matching: Option<bool>,
	texture_compression_advanced: Option<ManifestTextureCompressionAdvanced>,
}

fn default_background_color() -> [f32; 3] {
	[0.12, 0.14, 0.18]
}

/// `Advanced` モードで使われる、テクスチャ用途別の圧縮 preference。
/// 値は `source` / `auto` / `high_quality` / `small` / `gpu_native`。
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
struct ManifestTextureCompressionAdvanced {
	face: Option<String>,
	eyes: Option<String>,
	clothing: Option<String>,
	normal: Option<String>,
	occlusion: Option<String>,
	emissive: Option<String>,
	generic_color: Option<String>,
	data: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestWindow {
	icon_path: Option<PathBuf>,
	decorations: Option<bool>,
	transparent: Option<bool>,
	input_passthrough: Option<bool>,
	always_on_top: Option<bool>,
	width: Option<u32>,
	height: Option<u32>,
	/// Outer ウィンドウ位置（px）。x/y は両方揃って指定されたときだけ renderer 側に適用される。
	x: Option<i32>,
	y: Option<i32>,
	minimized: Option<bool>,
}

fn manifest_background_color(manifest: &AvatarManifestSummary) -> [f32; 3] {
	if let Some(color) = manifest.background_color {
		return clamp_rgb(color);
	}
	if let Some([r, g, b, _]) = manifest.clear_color {
		return clamp_rgb([r as f32, g as f32, b as f32]);
	}
	default_background_color()
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestDebug {
	show_axes: Option<bool>,
	show_bone_colliders: Option<bool>,
	disable_mtoon_outlines: Option<bool>,
	disable_rim_lighting: Option<bool>,
	force_shading_shift_zero: Option<bool>,
	disable_matcap: Option<bool>,
	disable_emissive: Option<bool>,
	disable_shade_color: Option<bool>,
	disable_normal_map: Option<bool>,
	base_texture_only: Option<bool>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestPhysics {
	bone_colliders: Option<ManifestBoneColliders>,
	contacts: Option<ManifestContactsPhysics>,
	dynamics: Option<ManifestDynamicsPhysics>,
	spring_bone: Option<ManifestSpringBonePhysics>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestContactsPhysics {
	parameter_emission: Option<bool>,
	parameter_emission_enabled: Option<bool>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestDynamicsPhysics {
	enabled: Option<bool>,
	enable_all_on_launch: Option<bool>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestSpringBonePhysics {
	simulation_hz: Option<f32>,
	substeps: Option<u32>,
	overrides: Option<Vec<ManifestSpringBoneCategoryOverride>>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestSpringBoneCategoryOverride {
	category: String,
	solver: Option<String>,
	damping_half_life_ms: Option<f32>,
	stiffness_hz: Option<f32>,
	xpbd_compliance: Option<f32>,
	constraint_iterations: Option<u32>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestBoneColliders {
	enabled: Option<bool>,
	radius_mm: Option<ManifestBoneColliderRadiiMm>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestBoneColliderRadiiMm {
	head: Option<f32>,
	neck_chest: Option<f32>,
	torso: Option<f32>,
	upper_arms: Option<f32>,
	lower_arms: Option<f32>,
	hands: Option<f32>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestCameraSetting {
	locked: Option<bool>,
	/// 注視点（world 座標 [x, y, z]）。renderer 側 `OrbitCamera::target` の初期値。
	target: Option<[f32; 3]>,
	/// 経度（degree）。アバター正面（+Z 側）を 0 として時計回り。
	longitude_deg: Option<f32>,
	/// 緯度（degree）。水平を 0、見下ろし方向が +。
	latitude_deg: Option<f32>,
	/// 注視点からカメラまでの距離（メートル）。
	radius: Option<f32>,
	/// 対角視野角（degree）。35mm 換算 ≒ 35deg。
	diagonal_fov_deg: Option<f32>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestEnvironment {
	color: Option<ManifestEnvironmentColor>,
	lighting: Option<ManifestLighting>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestEnvironmentColor {
	exposure: Option<f32>,
	contrast: Option<f32>,
	saturation: Option<f32>,
	look: Option<String>,
	intensity: Option<f32>,
	temperature: Option<f32>,
	tint: Option<f32>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestLighting {
	environment: Option<ManifestEnvironmentLight>,
	directional: Option<ManifestDirectionalLight>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestEnvironmentLight {
	enabled: Option<bool>,
	color: Option<[f32; 3]>,
	intensity: Option<f32>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestDirectionalLight {
	enabled: Option<bool>,
	color: Option<[f32; 3]>,
	intensity: Option<f32>,
	#[serde(alias = "longitude_deg")]
	azimuth_deg: Option<f32>,
	#[serde(alias = "latitude_deg")]
	elevation_deg: Option<f32>,
	follow_camera_yaw: Option<bool>,
	#[serde(rename = "reference")]
	/// Deprecated manifest spelling. Kept only to read existing profiles.
	legacy_reference: Option<String>,
	follow_camera_pitch: Option<bool>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestEffects {
	avatar: Option<ManifestAvatarEffects>,
	post: Option<ManifestPostEffects>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestPostEffects {
	bloom: Option<ManifestBloom>,
	ssao: Option<ManifestSsao>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestBloom {
	enabled: Option<bool>,
	strength: Option<f32>,
	threshold: Option<f32>,
	radius: Option<f32>,
	quality: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestSsao {
	enabled: Option<bool>,
	strength: Option<f32>,
	radius: Option<f32>,
	bias: Option<f32>,
	range: Option<f32>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestAvatarEffects {
	outline_policy: Option<String>,
	#[serde(alias = "outline_kind")]
	outline_type: Option<String>,
	outline_width: Option<f32>,
	outline_color: Option<[f32; 3]>,
	outline_lighting_mix: Option<f32>,
	outline_roundness: Option<f32>,
	outline: Option<ManifestAvatarOutline>,
	rim: Option<ManifestAvatarRim>,
	matcap: Option<ManifestAvatarMatcap>,
	specular: Option<ManifestAvatarSpecular>,
	ambient_occlusion: Option<ManifestAvatarAmbientOcclusion>,
	contact_shadow: Option<ManifestContactShadow>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestAvatarOutline {
	policy: Option<String>,
	#[serde(alias = "kind")]
	r#type: Option<String>,
	width: Option<f32>,
	color: Option<[f32; 3]>,
	lighting_mix: Option<f32>,
	roundness: Option<f32>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestAvatarRim {
	policy: Option<String>,
	color: Option<[f32; 3]>,
	intensity: Option<f32>,
	lighting_mix: Option<f32>,
	fresnel_power: Option<f32>,
	lift: Option<f32>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestAvatarMatcap {
	scale: Option<f32>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestAvatarSpecular {
	enabled: Option<bool>,
	intensity: Option<f32>,
	power: Option<f32>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestAvatarAmbientOcclusion {
	strength: Option<f32>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestContactShadow {
	enabled: Option<bool>,
	strength: Option<f32>,
	radius: Option<f32>,
	softness: Option<f32>,
	height: Option<f32>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct AvatarManifestSummary {
	title: Option<String>,
	avatar_path: Option<PathBuf>,
	wardrobe_set: Option<String>,
	icon_path: Option<PathBuf>,
	vmc_address: Option<String>,
	vmc_port: Option<u16>,
	motion: Option<ManifestMotion>,
	audio_link: Option<ManifestAudioLink>,
	physics: Option<ManifestPhysics>,
	output: Option<ManifestOutput>,
	aa: Option<String>,
	render_quality: Option<ManifestRenderQuality>,
	transparent: Option<bool>,
	input_passthrough: Option<bool>,
	decorations: Option<bool>,
	background_color: Option<[f32; 3]>,
	clear_color: Option<[f64; 4]>,
	profile: Option<ManifestProfile>,
	spout: Option<ManifestSpout>,
	window: Option<ManifestWindow>,
	debug: Option<ManifestDebug>,
	camera: Option<ManifestCameraSetting>,
	environment: Option<ManifestEnvironment>,
	effects: Option<ManifestEffects>,
}

pub fn run() {
	// Phase E settings policy (decision 1+2): user dir が空のとき限定で
	// bundled テンプレートをコピーする。app builder 構築前 (Tauri 依存無し)
	// に実行することで、setup callback 内のどの順序で何が走るかに依存しない。
	ensure_user_profiles_seeded();
	let mut initial_settings = load_app_settings();
	// AppRuntimeSettings.locale が未設定 / 未サポートなら OS → ja-JP の順で解決し、
	// rust-i18n のグローバル locale を反映する (tray menu / native notification 用)。
	// 解決済の値は initial_settings 内に書き戻し、Mutex 化されたあともクライアントに
	// 一貫した locale が返るようにする。永続化は次回 sync_app_settings 経由。
	if initial_settings.locale.is_empty() || !crate::i18n::UN_I18N_STORE.has_locale(&initial_settings.locale) {
		let resolved = crate::i18n::resolve_default_locale(&crate::i18n::UN_I18N_STORE);
		tracing::info!(locale = %resolved, "i18n: resolving locale from OS / fallback");
		initial_settings.locale = resolved;
	}
	crate::i18n::apply_locale(&initial_settings.locale);
	tauri::Builder::default()
		.plugin(tauri_plugin_notification::init())
		.register_uri_scheme_protocol("un-avatar-thumbnail", |_ctx, request| thumbnail_protocol_response(request))
		.manage(Mutex::new(SupervisorState::default()))
		.manage(Mutex::new(initial_settings.clone()))
		.setup(move |app| {
			prewarm_runtime_control_session();
			if initial_settings.system_tray_enabled {
				setup_tray(app.handle())?;
			}
			let window = setup_main_window(app)?;
			if initial_settings.system_tray_enabled && initial_settings.start_minimized_to_tray {
				let _ = window.hide();
			}
			attach_hide_on_close(window, app.handle().clone());
			Ok(())
		})
		.invoke_handler(tauri::generate_handler![
			activate_renderer_window,
			app_version,
			open_external_url,
			clear_app_notifications,
			compress_diagnostics,
			duplicate_avatar_setting,
			delete_avatar_setting,
			export_diagnostics,
			get_app_settings,
			get_renderer_runtime_status,
			get_native_notification_status,
			list_app_notifications,
			list_avatar_settings,
			list_diagnostics_exports,
			list_renderers,
			launch_renderer,
			new_avatar_setting,
			pick_file_path,
			read_vrm_metadata,
			save_avatar_thumbnail_icon,
			read_diagnostics_export,
			reveal_profiles_dir,
			reorder_avatar_settings,
			reveal_path,
			save_supervisor_logs,
			reveal_supervisor_logs_dir,
			capture_renderer_screenshot,
			set_renderer_expression_override,
			activate_renderer_runtime_action,
			clear_renderer_expression_overrides,
			set_renderer_look_at,
			set_renderer_show_axes,
			set_renderer_show_bone_colliders,
			set_renderer_camera_lock,
			set_renderer_apply_vmc_root_translation,
			set_renderer_motion_receivers,
			set_renderer_spring_bones,
			set_renderer_all_dynamics_launch_setting,
			set_renderer_dynamics_enabled,
			set_renderer_all_dynamics_enabled,
			set_renderer_primary_motion_source,
			set_renderer_avatar_outline,
			set_renderer_avatar_rim,
			set_renderer_avatar_matcap,
			set_renderer_avatar_specular,
			set_renderer_avatar_ambient_occlusion,
			set_renderer_lighting,
			set_renderer_environment_color,
			set_renderer_bloom,
			set_renderer_ssao,
			set_renderer_contact_shadow,
			set_renderer_camera_fov,
			set_renderer_camera_state,
			save_renderer_camera_to_profile,
			restore_renderer_camera_from_profile,
			save_renderer_window_to_profile,
			restore_renderer_window_from_profile,
			reset_renderer_camera,
			set_renderer_clear_color,
			set_renderer_camera_orbit,
			set_renderer_spout_output,
			set_renderer_window,
			send_test_native_notification,
			stop_renderer,
			stop_all_renderers,
			sync_app_settings,
			read_unavatar_wardrobe_options,
			set_last_selected_setting_id,
			update_avatar_setting_path,
			update_avatar_setting_value,
			update_avatar_setting_values,
			crate::i18n::i18n_get_svelte_bundle,
			crate::i18n::i18n_available_locales,
			crate::i18n::i18n_resolve_default_locale,
		])
		.run(tauri::generate_context!())
		.expect("error while running UN Avatar Supervisor");
}

fn setup_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
	if app.tray_by_id("un-avatar-tray").is_some() {
		return Ok(());
	}
	let menu = build_tray_menu(app)?;
	let icon = Image::from_bytes(include_bytes!("../../../../assets/brand/un-avatar-artwork-supervisor.png"))?;
	TrayIconBuilder::with_id("un-avatar-tray")
		.tooltip(APP_TITLE)
		.icon(icon)
		.menu(&menu)
		.show_menu_on_left_click(false)
		.on_menu_event(handle_tray_menu_event)
		.on_tray_icon_event(|tray, event| {
			if let TrayIconEvent::DoubleClick {
				button: MouseButton::Left, ..
			} = event
			{
				show_main_window(tray.app_handle());
			}
		})
		.build(app)?;
	Ok(())
}

fn build_tray_menu<R: Runtime, M: Manager<R>>(manager: &M) -> tauri::Result<Menu<R>> {
	let launch_menu = Submenu::new(manager, &t!("tray.launch"), true)?;
	let tray_settings = tray_launch_settings().unwrap_or_default();
	if tray_settings.is_empty() {
		let empty = MenuItem::with_id(manager, "launch:none", &t!("tray.launch_none"), false, None::<&str>)?;
		launch_menu.append(&empty)?;
	} else {
		for setting in tray_settings {
			let item = MenuItem::with_id(manager, format!("launch:{}", setting.id), setting.name, true, None::<&str>)?;
			launch_menu.append(&item)?;
		}
	}
	let open = MenuItem::with_id(manager, "open", &t!("tray.open"), true, None::<&str>)?;
	let stop_all = MenuItem::with_id(manager, "stop_all", &t!("tray.stop_all"), true, None::<&str>)?;
	let quit = MenuItem::with_id(manager, "quit", &t!("tray.quit"), true, None::<&str>)?;
	let menu = Menu::new(manager)?;
	menu.append(&open)?;
	menu.append(&launch_menu)?;
	menu.append(&stop_all)?;
	menu.append(&quit)?;
	Ok(menu)
}

fn handle_tray_menu_event(app: &tauri::AppHandle, event: tauri::menu::MenuEvent) {
	match event.id().as_ref() {
		"open" => show_main_window(app),
		id if id.starts_with("launch:") => {
			let setting_id = id.trim_start_matches("launch:");
			if setting_id != "none" {
				if let Some(state) = app.try_state::<Mutex<SupervisorState>>() {
					let settings = app
						.try_state::<Mutex<AppRuntimeSettings>>()
						.and_then(|settings| settings.lock().ok().map(|settings| settings.clone()))
						.unwrap_or_default();
					let _ = launch_renderer_in_state(setting_id, &state, &settings);
				}
			}
		}
		"stop_all" => {
			if let Some(state) = app.try_state::<Mutex<SupervisorState>>() {
				stop_all_in_state(&state);
			}
		}
		"quit" => quit_from_tray(app),
		_ => {}
	}
}

fn refresh_tray_menu(app: &tauri::AppHandle) -> Result<(), String> {
	let Some(tray) = app.tray_by_id("un-avatar-tray") else {
		return Ok(());
	};
	let menu = build_tray_menu(app).map_err(|e| format!("build tray menu: {e}"))?;
	tray.set_menu(Some(menu)).map_err(|e| format!("set tray menu: {e}"))
}

fn quit_from_tray(app: &tauri::AppHandle) {
	let settings = app
		.try_state::<Mutex<AppRuntimeSettings>>()
		.and_then(|settings| settings.lock().ok().map(|settings| settings.clone()))
		.unwrap_or_default();
	match settings.quit_behavior {
		QuitBehavior::StopRenderers => {
			if let Some(state) = app.try_state::<Mutex<SupervisorState>>() {
				stop_all_in_state(&state);
			}
		}
		QuitBehavior::LeaveRenderers => {}
		QuitBehavior::Ask => {
			if renderer_running(app) {
				match ask_quit_with_renderers() {
					rfd::MessageDialogResult::Yes => {
						if let Some(state) = app.try_state::<Mutex<SupervisorState>>() {
							stop_all_in_state(&state);
						}
					}
					rfd::MessageDialogResult::No => {}
					_ => {
						show_main_window(app);
						return;
					}
				}
			}
		}
	}
	app.exit(0);
}

fn ask_quit_with_renderers() -> rfd::MessageDialogResult {
	rfd::MessageDialog::new()
		.set_level(rfd::MessageLevel::Warning)
		.set_title("Quit UN Avatar")
		.set_description("Renderer processes are still running. Stop them before quitting?")
		.set_buttons(rfd::MessageButtons::YesNoCancel)
		.show()
}

fn setup_main_window(app: &mut tauri::App) -> tauri::Result<WebviewWindow> {
	// 終了時に保存した位置・サイズが Settings にあるなら復元する。保存値は logical px。
	// 旧バージョンが誤って physical px を保存していた場合は、ここで sane な範囲にクランプして
	// 「DPI 150% 環境で毎起動 1.5 倍に膨らみ続ける」状態から自動復旧させる。
	let app_settings = app
		.try_state::<Mutex<AppRuntimeSettings>>()
		.and_then(|s| s.lock().ok().map(|s| s.clone()))
		.unwrap_or_default();
	// 1080p < 通常モニタ <= 4K と仮定して、logical 入力としてあり得ない極端値は
	// builder に渡さず既定値にフォールバックする。
	const MIN_LOGICAL_W: u32 = 820;
	const MIN_LOGICAL_H: u32 = 620;
	const MAX_LOGICAL_W: u32 = 3840;
	const MAX_LOGICAL_H: u32 = 2160;
	let width = app_settings
		.console_window_width
		.filter(|w| (MIN_LOGICAL_W..=MAX_LOGICAL_W).contains(w))
		.unwrap_or(1190);
	let height = app_settings
		.console_window_height
		.filter(|h| (MIN_LOGICAL_H..=MAX_LOGICAL_H).contains(h))
		.unwrap_or(620);
	let mut builder = WebviewWindowBuilder::new(app, MAIN_WINDOW_LABEL, WebviewUrl::App("index.html".into()))
		.title(app_title_with_version())
		.icon(Image::from_bytes(include_bytes!(
			"../../../../assets/brand/un-avatar-artwork-supervisor.png"
		))?)?
		.inner_size(f64::from(width), f64::from(height))
		.min_inner_size(f64::from(MIN_LOGICAL_W), f64::from(MIN_LOGICAL_H))
		.resizable(true)
		.visible(true);
	// 位置も logical px。負値や極端な座標は OS が画面外スナップしてくれるが、保存値の sanity check は行う。
	if let (Some(x), Some(y)) = (app_settings.console_window_x, app_settings.console_window_y) {
		if (-16384..=16384).contains(&x) && (-16384..=16384).contains(&y) {
			builder = builder.position(f64::from(x), f64::from(y));
		}
	}
	builder.build()
}

fn attach_hide_on_close(window: WebviewWindow, app_handle: tauri::AppHandle) {
	window.on_window_event(move |event| match event {
		WindowEvent::CloseRequested { api, .. } => {
			persist_console_window_geometry(&app_handle);
			if should_hide_on_close(&app_handle) {
				api.prevent_close();
				if let Some(window) = app_handle.get_webview_window(MAIN_WINDOW_LABEL) {
					let _ = window.hide();
				}
				return;
			}
			if should_stop_all_on_console_exit(&app_handle) {
				api.prevent_close();
				if let Some(window) = app_handle.get_webview_window(MAIN_WINDOW_LABEL) {
					let _ = window.hide();
				}
				let app = app_handle.clone();
				std::thread::spawn(move || {
					if let Some(state) = app.try_state::<Mutex<SupervisorState>>() {
						stop_all_in_state(&state);
					}
					app.exit(0);
				});
			}
		}
		// ユーザーが移動/リサイズしたタイミングでもメモリ更新だけ行い、Disk 書き込みは閉じるとき。
		// （頻繁な move/resize で write_app_settings を毎回叩くと体感が悪く、また権限要求でブロックされかねない）
		WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
			update_console_window_geometry_in_memory(&app_handle);
		}
		_ => {}
	});
}

/// 現在の Console ウィンドウの outer 位置 / inner サイズを `AppRuntimeSettings` (in-memory) に書き込む。
/// Disk への保存は呼び出し側の `persist_console_window_geometry` 経由で行う。
///
/// **DPI 注意**: `outer_position()` / `inner_size()` は OS のスケーリングが乗った physical px を返すが、
/// `WebviewWindowBuilder::position` / `inner_size` は **logical px** を受け取る（Tauri 2 既定）。
/// このため physical 値をそのまま保存・復元すると、終了→起動で `scale_factor` 倍に拡大される
/// （150% DPI モニタでは毎回ウィンドウが 1.5 倍ずつ巨大化していく）。
/// ここで `scale_factor` で割って logical に正規化してから保存する。
fn update_console_window_geometry_in_memory(app_handle: &tauri::AppHandle) {
	let Some(window) = app_handle.get_webview_window(MAIN_WINDOW_LABEL) else {
		return;
	};
	let Some(state) = app_handle.try_state::<Mutex<AppRuntimeSettings>>() else {
		return;
	};
	let scale = window.scale_factor().unwrap_or(1.0).max(0.1);
	let Ok(mut state) = state.lock() else { return };
	if let Ok(pos) = window.outer_position() {
		let logical = pos.to_logical::<f64>(scale);
		state.console_window_x = Some(logical.x.round() as i32);
		state.console_window_y = Some(logical.y.round() as i32);
	}
	if let Ok(size) = window.inner_size() {
		let logical = size.to_logical::<f64>(scale);
		state.console_window_width = Some(logical.width.round().max(0.0) as u32);
		state.console_window_height = Some(logical.height.round().max(0.0) as u32);
	}
}

/// in-memory 更新したあと Disk へ書き出す。CloseRequested など終端で 1 度だけ呼ぶ想定。
fn persist_console_window_geometry(app_handle: &tauri::AppHandle) {
	update_console_window_geometry_in_memory(app_handle);
	let Some(state) = app_handle.try_state::<Mutex<AppRuntimeSettings>>() else {
		return;
	};
	let Ok(state) = state.lock() else { return };
	if let Err(e) = write_app_settings(&state) {
		eprintln!("un-avatar-supervisor: persist console window geometry failed: {e}");
	}
}

fn should_hide_on_close(app_handle: &tauri::AppHandle) -> bool {
	let Some((system_tray_enabled, minimize_to_tray, close_to_tray_while_running)) =
		app_handle.try_state::<Mutex<AppRuntimeSettings>>().and_then(|settings| {
			settings.lock().ok().map(|settings| {
				(
					settings.system_tray_enabled,
					settings.minimize_to_tray,
					settings.close_to_tray_while_running,
				)
			})
		})
	else {
		return false;
	};
	if !system_tray_enabled {
		return false;
	}
	minimize_to_tray || (close_to_tray_while_running && renderer_running(app_handle))
}

fn should_stop_all_on_console_exit(app_handle: &tauri::AppHandle) -> bool {
	app_handle
		.try_state::<Mutex<AppRuntimeSettings>>()
		.and_then(|settings| settings.lock().ok().map(|settings| settings.stop_all_on_console_exit))
		.unwrap_or(false)
}

fn renderer_running(app_handle: &tauri::AppHandle) -> bool {
	app_handle
		.try_state::<Mutex<SupervisorState>>()
		.and_then(|state| {
			state.lock().ok().map(|state| {
				state.renderers.values().any(|renderer| {
					matches!(
						renderer.info.state,
						RendererState::Starting | RendererState::Running | RendererState::Degraded
					)
				})
			})
		})
		.unwrap_or(false)
}

fn show_main_window(app: &tauri::AppHandle) {
	if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
		let _ = window.show();
		let _ = window.set_focus();
	}
}

#[tauri::command]
fn list_avatar_settings() -> Result<Vec<AvatarSetting>, String> {
	let mut by_id = BTreeMap::new();
	let hidden_seed_ids = read_hidden_seed_profile_ids();
	for (storage, dir) in profile_dirs() {
		if !dir.is_dir() {
			continue;
		}
		let entries = fs::read_dir(&dir).map_err(|e| format!("read {}: {e}", dir.display()))?;
		for entry in entries.flatten() {
			let path = entry.path();
			if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
				continue;
			}
			if let Ok(setting) = read_avatar_setting(&path, storage) {
				if storage == ProfileStorage::Seed && hidden_seed_ids.iter().any(|id| id == &setting.id) {
					continue;
				}
				by_id.insert(setting.id.clone(), setting);
			}
		}
	}
	let mut settings = by_id.into_values().collect::<Vec<_>>();
	settings.sort_by(|a, b| {
		a.sort_order
			.cmp(&b.sort_order)
			.then(a.created_at.cmp(&b.created_at))
			.then(a.name.cmp(&b.name))
			.then(a.id.cmp(&b.id))
	});
	Ok(settings)
}

fn tray_launch_settings() -> Result<Vec<AvatarSetting>, String> {
	list_avatar_settings()
}

#[tauri::command]
fn list_renderers(
	app: tauri::AppHandle,
	state: State<'_, Mutex<SupervisorState>>,
	settings: State<'_, Mutex<AppRuntimeSettings>>,
) -> Result<Vec<RendererInstance>, String> {
	let crash_notifications = settings.lock().map(|settings| settings.crash_notifications).unwrap_or(true);
	let mut state = state.lock().map_err(|_| "supervisor state poisoned".to_string())?;
	refresh_renderer_states(&mut state, crash_notifications, Some(&app));
	Ok(state.renderers.values().map(|renderer| renderer.info.clone()).collect())
}

#[tauri::command]
fn get_renderer_runtime_status(id: u32, state: State<'_, Mutex<SupervisorState>>) -> Result<RendererRuntimeStatus, String> {
	let mut state = state.lock().map_err(|_| "supervisor state poisoned".to_string())?;
	refresh_renderer_states(&mut state, false, None);
	let renderer = state.renderers.get(&id).ok_or_else(|| format!("renderer not found: {id}"))?;
	Ok(runtime_status_from_renderer(renderer))
}

#[tauri::command]
fn app_version() -> String {
	env!("CARGO_PKG_VERSION").to_string()
}

#[tauri::command]
fn open_external_url(url: String) -> Result<(), String> {
	if !(url.starts_with("https://") || url.starts_with("http://")) {
		return Err(format!("refused to open non-http(s) url: {url}"));
	}
	#[cfg(windows)]
	{
		Command::new("cmd")
			.args(["/C", "start", "", &url])
			.spawn()
			.map_err(|e| format!("open url: {e}"))?;
		return Ok(());
	}
	#[cfg(target_os = "macos")]
	{
		Command::new("open").arg(&url).spawn().map_err(|e| format!("open url: {e}"))?;
		return Ok(());
	}
	#[cfg(target_os = "linux")]
	{
		Command::new("xdg-open").arg(&url).spawn().map_err(|e| format!("open url: {e}"))?;
		return Ok(());
	}
	#[cfg_attr(any(windows, target_os = "macos", target_os = "linux"), allow(unreachable_code))]
	Err("unsupported platform".to_string())
}

#[tauri::command]
fn list_app_notifications(state: State<'_, Mutex<SupervisorState>>) -> Result<Vec<AppNotification>, String> {
	let state = state.lock().map_err(|_| "supervisor state poisoned".to_string())?;
	Ok(state.notifications.clone())
}

#[tauri::command]
fn clear_app_notifications(state: State<'_, Mutex<SupervisorState>>) -> Result<(), String> {
	let mut state = state.lock().map_err(|_| "supervisor state poisoned".to_string())?;
	state.notifications.clear();
	Ok(())
}

#[tauri::command]
fn get_native_notification_status(app: tauri::AppHandle) -> Result<NativeNotificationStatus, String> {
	native_notification_status(&app)
}

fn native_notification_status(app: &tauri::AppHandle) -> Result<NativeNotificationStatus, String> {
	let permission_state = app
		.notification()
		.permission_state()
		.map_err(|error| format!("native notification status failed: {error}"))?;
	Ok(NativeNotificationStatus {
		permission_state: permission_state_label(permission_state).to_string(),
	})
}

#[tauri::command]
fn send_test_native_notification(app: tauri::AppHandle) -> Result<(), String> {
	app.notification()
		.builder()
		.title("UN Avatar notification test")
		.body("Native crash notifications are ready.")
		.show()
		.map_err(|error| format!("native notification test failed: {error}"))
}

#[tauri::command]
fn export_diagnostics(
	app: tauri::AppHandle,
	state: State<'_, Mutex<SupervisorState>>,
	settings: State<'_, Mutex<AppRuntimeSettings>>,
) -> Result<String, String> {
	let app_settings = settings.lock().map_err(|_| "app settings state poisoned".to_string())?.clone();
	let mut state = state.lock().map_err(|_| "supervisor state poisoned".to_string())?;
	refresh_renderer_states(&mut state, false, None);
	let generated_at_secs = current_unix_secs();
	let bundle = SupervisorDiagnosticsBundle {
		version: env!("CARGO_PKG_VERSION"),
		generated_at_secs,
		repo_root: repo_root().display().to_string(),
		build: supervisor_build_info(),
		app_settings,
		native_notifications: native_notification_status(&app)?,
		profiles: profile_diagnostics(),
		renderers: state.renderers.values().map(renderer_diagnostics).collect(),
		notifications: state.notifications.clone(),
	};
	let dir = diagnostics_dir();
	fs::create_dir_all(&dir).map_err(|e| format!("create diagnostics dir {}: {e}", dir.display()))?;
	let path = dir.join(format!("un-avatar-supervisor-{generated_at_secs}.json"));
	let text = serde_json::to_string_pretty(&bundle).map_err(|e| format!("serialize diagnostics: {e}"))?;
	fs::write(&path, text).map_err(|e| format!("write diagnostics {}: {e}", path.display()))?;
	Ok(path.display().to_string())
}

#[tauri::command]
fn list_diagnostics_exports() -> Result<Vec<DiagnosticsExportEntry>, String> {
	let dir = diagnostics_dir();
	if !dir.is_dir() {
		return Ok(Vec::new());
	}
	let mut entries = Vec::new();
	for entry in fs::read_dir(&dir).map_err(|e| format!("read diagnostics dir {}: {e}", dir.display()))? {
		let entry = entry.map_err(|e| format!("read diagnostics entry: {e}"))?;
		let path = entry.path();
		if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
			continue;
		}
		let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
			continue;
		};
		if !file_name.starts_with("un-avatar-supervisor-") {
			continue;
		}
		let metadata = entry
			.metadata()
			.map_err(|e| format!("read diagnostics metadata {}: {e}", path.display()))?;
		let archive_path = diagnostics_archive_path(&path);
		let archive_metadata = archive_path.metadata().ok();
		entries.push(DiagnosticsExportEntry {
			path: path.display().to_string(),
			archive_path: archive_metadata.as_ref().map(|_| archive_path.display().to_string()),
			generated_at_secs: diagnostics_generated_at_secs(&path),
			modified_at_secs: metadata.modified().ok().and_then(system_time_secs),
			size_bytes: metadata.len(),
			archive_size_bytes: archive_metadata.map(|metadata| metadata.len()),
		});
	}
	entries.sort_by(|a, b| {
		b.generated_at_secs
			.or(b.modified_at_secs)
			.cmp(&a.generated_at_secs.or(a.modified_at_secs))
	});
	Ok(entries)
}

#[tauri::command]
fn read_diagnostics_export(path: String) -> Result<String, String> {
	let path = resolve_repo_path(&path);
	let dir = diagnostics_dir();
	let path = path
		.canonicalize()
		.map_err(|e| format!("read diagnostics {}: {e}", path.display()))?;
	let dir = dir
		.canonicalize()
		.map_err(|e| format!("read diagnostics dir {}: {e}", dir.display()))?;
	if !path.starts_with(&dir) || path.extension().and_then(|ext| ext.to_str()) != Some("json") {
		return Err(format!("not a diagnostics bundle: {}", path.display()));
	}
	let metadata = path
		.metadata()
		.map_err(|e| format!("read diagnostics metadata {}: {e}", path.display()))?;
	if metadata.len() > MAX_DIAGNOSTICS_PREVIEW_BYTES {
		return Err(format!("diagnostics bundle is too large to preview: {}", path.display()));
	}
	fs::read_to_string(&path).map_err(|e| format!("read diagnostics {}: {e}", path.display()))
}

fn profile_diagnostics() -> SupervisorProfileDiagnostics {
	match list_avatar_settings() {
		Ok(settings) => SupervisorProfileDiagnostics {
			seed_dir: profiles_dir().display().to_string(),
			user_dir: user_profiles_dir().display().to_string(),
			tray_launch_settings: settings.clone(),
			settings,
			error: None,
		},
		Err(error) => SupervisorProfileDiagnostics {
			seed_dir: profiles_dir().display().to_string(),
			user_dir: user_profiles_dir().display().to_string(),
			settings: Vec::new(),
			tray_launch_settings: Vec::new(),
			error: Some(error),
		},
	}
}

#[tauri::command]
fn get_app_settings(settings: State<'_, Mutex<AppRuntimeSettings>>) -> Result<AppRuntimeSettings, String> {
	settings
		.lock()
		.map(|settings| settings.clone())
		.map_err(|_| "app settings state poisoned".to_string())
}

/// Renderers / Avatar Settings 画面の選択中アバター設定 ID を記録し、終了 → 再起動時に復元できるようにする。
/// `value` が `None` または空文字列のときは記録を消去（次回起動時は先頭プロファイルを既定にする）。
#[tauri::command]
fn set_last_selected_setting_id(value: Option<String>, state: State<'_, Mutex<AppRuntimeSettings>>) -> Result<(), String> {
	let normalized = value.and_then(|v| {
		let trimmed = v.trim().to_string();
		if trimmed.is_empty() {
			None
		} else {
			Some(trimmed)
		}
	});
	let mut state = state.lock().map_err(|_| "app settings state poisoned".to_string())?;
	if state.last_selected_setting_id == normalized {
		return Ok(());
	}
	state.last_selected_setting_id = normalized;
	write_app_settings(&state)?;
	Ok(())
}

fn renderer_diagnostics(renderer: &ManagedRenderer) -> RendererDiagnostics {
	RendererDiagnostics {
		info: renderer.info.clone(),
		runtime_bus_key: renderer.runtime_bus_key.clone(),
		runtime_status: runtime_status_from_renderer(renderer),
	}
}

#[tauri::command]
fn sync_app_settings(
	app: tauri::AppHandle,
	mut settings: AppRuntimeSettings,
	state: State<'_, Mutex<AppRuntimeSettings>>,
) -> Result<(), String> {
	normalize_app_settings(&mut settings);
	let _reserved_for_future_runtime_hooks = (
		settings.start_minimized_to_tray,
		settings.crash_notifications,
		settings.theme_mode.as_str(),
	);
	let (old_system_tray_enabled, old_locale) = {
		let state = state.lock().map_err(|_| "app settings state poisoned".to_string())?;
		(state.system_tray_enabled, state.locale.clone())
	};
	if old_system_tray_enabled != settings.system_tray_enabled {
		if settings.system_tray_enabled {
			setup_tray(&app).map_err(|e| format!("setup tray: {e}"))?;
		} else {
			drop(app.remove_tray_by_id("un-avatar-tray"));
		}
	}
	// locale が変わったら rust-i18n のグローバル locale を即時切替。Svelte 側は
	// 自前で `locale.set()` → loader 再実行で別途切替する。
	if old_locale != settings.locale && !settings.locale.is_empty() {
		crate::i18n::apply_locale(&settings.locale);
	}
	write_app_settings(&settings)?;
	let mut state = state.lock().map_err(|_| "app settings state poisoned".to_string())?;
	*state = settings;
	Ok(())
}

fn load_app_settings() -> AppRuntimeSettings {
	let mut settings = fs::read_to_string(app_settings_path())
		.ok()
		.and_then(|text| toml::from_str(&text).ok())
		.unwrap_or_default();
	normalize_app_settings(&mut settings);
	settings
}

fn normalize_app_settings(settings: &mut AppRuntimeSettings) {
	if settings.renderer_close_hotkey.trim().is_empty() {
		settings.renderer_close_hotkey = "Escape".to_string();
	} else {
		settings.renderer_close_hotkey = settings.renderer_close_hotkey.trim().to_string();
	}
	settings.last_avatar_model_dir = settings
		.last_avatar_model_dir
		.as_deref()
		.map(str::trim)
		.filter(|value| !value.is_empty())
		.map(str::to_string);
	// 不正な locale (TOML が無い) は空文字 (= 自動解決) に戻す。
	if !settings.locale.is_empty() && !crate::i18n::UN_I18N_STORE.has_locale(&settings.locale) {
		tracing::warn!(locale = %settings.locale, "i18n: unsupported locale value, resetting to auto");
		settings.locale.clear();
	}
}

fn write_app_settings(settings: &AppRuntimeSettings) -> Result<(), String> {
	let path = app_settings_path();
	if let Some(dir) = path.parent() {
		fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
	}
	let text = toml::to_string_pretty(settings).map_err(|e| format!("serialize app settings: {e}"))?;
	fs::write(&path, text).map_err(|e| format!("write {}: {e}", path.display()))
}

#[tauri::command]
fn duplicate_avatar_setting(setting_id: String, app: tauri::AppHandle) -> Result<AvatarSetting, String> {
	let source = resolve_avatar_setting(&setting_id)?;
	let source_path = PathBuf::from(&source.manifest_path);
	let mut manifest = read_manifest_value(&source_path)?;
	let copy_name = unique_profile_name(&format!("{} Copy", source.name))?;
	let created_at = current_timestamp_compact();
	if let Some(table) = manifest.as_table_mut() {
		let profile = table
			.entry("profile")
			.or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
			.as_table_mut()
			.ok_or_else(|| "profile must be a table".to_string())?;
		profile.insert("id".to_string(), toml::Value::String(unique_profile_id_for_name(&copy_name)?));
		profile.insert("display_name".to_string(), toml::Value::String(copy_name.clone()));
		profile.insert("created_at".to_string(), toml::Value::String(created_at.clone()));
		profile.insert("sort_order".to_string(), toml::Value::Integer(next_avatar_sort_order()? as i64));
		table.insert("title".to_string(), toml::Value::String(copy_name));
	}
	let target_path = unique_user_profile_path(&profile_file_stem(
		&created_at,
		manifest_profile_name(&manifest).as_deref().unwrap_or("Avatar"),
	));
	if let Some(dir) = target_path.parent() {
		fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
	}
	write_manifest_value(&target_path, &manifest)?;
	let setting = read_avatar_setting(&target_path, ProfileStorage::User)?;
	refresh_tray_menu(&app)?;
	Ok(setting)
}

#[tauri::command]
fn new_avatar_setting(app: tauri::AppHandle) -> Result<AvatarSetting, String> {
	let name = unique_profile_name("New Avatar")?;
	let id = unique_profile_id_for_name(&name)?;
	let created_at = current_timestamp_compact();
	let mut manifest = toml::map::Map::new();
	manifest.insert("title".to_string(), toml::Value::String(name.clone()));
	manifest.insert("transparent".to_string(), toml::Value::Boolean(false));
	manifest.insert("input_passthrough".to_string(), toml::Value::Boolean(false));
	manifest.insert("decorations".to_string(), toml::Value::Boolean(true));
	manifest.insert("aa".to_string(), toml::Value::String("off".to_string()));
	manifest.insert(
		"background_color".to_string(),
		toml::Value::Array(
			default_background_color()
				.into_iter()
				.map(|value| toml::Value::Float(f64::from(value)))
				.collect(),
		),
	);
	manifest.insert("show_fps_in_title".to_string(), toml::Value::Boolean(true));
	manifest.insert(
		"profile".to_string(),
		toml::Value::Table(toml::map::Map::from_iter([
			("id".to_string(), toml::Value::String(id)),
			("display_name".to_string(), toml::Value::String(name.clone())),
			("created_at".to_string(), toml::Value::String(created_at.clone())),
			("sort_order".to_string(), toml::Value::Integer(next_avatar_sort_order()? as i64)),
			("allow_multiple_renderers".to_string(), toml::Value::Boolean(false)),
			("group".to_string(), toml::Value::String(String::new())),
			("notes".to_string(), toml::Value::String(String::new())),
		])),
	);
	manifest.insert(
		"render_quality".to_string(),
		toml::Value::Table(toml::map::Map::from_iter([
			("aa".to_string(), toml::Value::String("off".to_string())),
			("texture_resolution_limit".to_string(), toml::Value::String("off".to_string())),
			("texture_compression".to_string(), toml::Value::String("balanced".to_string())),
			("mipmap_filter".to_string(), toml::Value::String("mitchell".to_string())),
			("render_backend".to_string(), toml::Value::String("vulkan".to_string())),
			("block_compression_encoder".to_string(), toml::Value::String("gpu".to_string())),
			("block_compression_cpu_threads".to_string(), toml::Value::Integer(4)),
			("processed_texture_cache".to_string(), toml::Value::Boolean(true)),
			("skin_tone_matching".to_string(), toml::Value::Boolean(false)),
		])),
	);
	manifest.insert(
		"window".to_string(),
		toml::Value::Table(toml::map::Map::from_iter([
			("visible".to_string(), toml::Value::Boolean(true)),
			("drag_from_anywhere".to_string(), toml::Value::Boolean(false)),
			("decorations".to_string(), toml::Value::Boolean(true)),
			("transparent".to_string(), toml::Value::Boolean(false)),
			("input_passthrough".to_string(), toml::Value::Boolean(false)),
			("always_on_top".to_string(), toml::Value::Boolean(false)),
			("width".to_string(), toml::Value::Integer(800)),
			("height".to_string(), toml::Value::Integer(600)),
		])),
	);
	manifest.insert(
		"motion".to_string(),
		toml::Value::Table(toml::map::Map::from_iter([
			(
				"vmc_udp".to_string(),
				toml::Value::Table(toml::map::Map::from_iter([
					("enabled".to_string(), toml::Value::Boolean(false)),
					("address".to_string(), toml::Value::String(default_vmc_address())),
				])),
			),
			(
				"unmotion_zenoh".to_string(),
				toml::Value::Table(toml::map::Map::from_iter([
					("enabled".to_string(), toml::Value::Boolean(false)),
					("key".to_string(), toml::Value::String(String::new())),
				])),
			),
			("primary_source".to_string(), toml::Value::String("vmc".to_string())),
		])),
	);
	manifest.insert(
		"physics".to_string(),
		toml::Value::Table(toml::map::Map::from_iter([(
			"bone_colliders".to_string(),
			toml::Value::Table(toml::map::Map::from_iter([
				("enabled".to_string(), toml::Value::Boolean(true)),
				(
					"radius_mm".to_string(),
					toml::Value::Table(toml::map::Map::from_iter([
						("head".to_string(), toml::Value::Float(120.0)),
						("neck_chest".to_string(), toml::Value::Float(80.0)),
						("torso".to_string(), toml::Value::Float(140.0)),
						("upper_arms".to_string(), toml::Value::Float(55.0)),
						("lower_arms".to_string(), toml::Value::Float(45.0)),
						("hands".to_string(), toml::Value::Float(50.0)),
					])),
				),
			])),
		)])),
	);
	manifest.insert(
		"output".to_string(),
		toml::Value::Table(toml::map::Map::from_iter([(
			"spout2".to_string(),
			toml::Value::Table(toml::map::Map::from_iter([
				("enabled".to_string(), toml::Value::Boolean(false)),
				("name".to_string(), toml::Value::String("UN Avatar Spout".to_string())),
			])),
		)])),
	);
	manifest.insert(
		"spout".to_string(),
		toml::Value::Table(toml::map::Map::from_iter([
			("enabled".to_string(), toml::Value::Boolean(false)),
			("name".to_string(), toml::Value::String("UN Avatar Spout".to_string())),
		])),
	);
	let path = unique_user_profile_path(&profile_file_stem(&created_at, &name));
	if let Some(dir) = path.parent() {
		fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
	}
	let text = toml::to_string_pretty(&toml::Value::Table(manifest)).map_err(|e| format!("serialize new manifest: {e}"))?;
	fs::write(&path, text).map_err(|e| format!("write {}: {e}", path.display()))?;
	let setting = read_avatar_setting(&path, ProfileStorage::User)?;
	refresh_tray_menu(&app)?;
	Ok(setting)
}

#[tauri::command]
fn delete_avatar_setting(setting_id: String, app: tauri::AppHandle) -> Result<(), String> {
	let setting = resolve_avatar_setting_direct(&setting_id)?;
	match setting.storage {
		ProfileStorage::User => {
			let manifest_path = PathBuf::from(&setting.manifest_path);
			fs::remove_file(&manifest_path).map_err(|e| format!("delete {}: {e}", manifest_path.display()))?;
			remove_profile_icon_thumbnail_cache(&setting);
			if seed_avatar_setting_exists(&setting.id) {
				hide_seed_avatar_setting(&setting.id)?;
			}
		}
		ProfileStorage::Seed => {
			remove_profile_icon_thumbnail_cache(&setting);
			hide_seed_avatar_setting(&setting.id)?;
		}
	}
	refresh_tray_menu(&app)?;
	Ok(())
}

#[tauri::command]
fn reorder_avatar_settings(setting_ids: Vec<String>, app: tauri::AppHandle) -> Result<Vec<AvatarSetting>, String> {
	for (index, id) in setting_ids.iter().enumerate() {
		let setting = resolve_avatar_setting(id)?;
		let manifest_path = editable_avatar_setting_path(&setting)?;
		let mut manifest = read_manifest_value(&manifest_path)?;
		ensure_avatar_profile_metadata(&mut manifest, &manifest_path, Some(((index as u32) + 1) * 1000))?;
		write_manifest_value(&manifest_path, &manifest)?;
		let setting = read_avatar_setting(&manifest_path, ProfileStorage::User)?;
		let _ = rename_avatar_setting_file_if_needed(&manifest_path, &setting)?;
	}
	refresh_tray_menu(&app)?;
	let order = setting_ids
		.iter()
		.enumerate()
		.map(|(index, id)| (id.as_str(), index))
		.collect::<BTreeMap<_, _>>();
	let mut settings = list_avatar_settings()?;
	settings.sort_by(|a, b| {
		order
			.get(a.id.as_str())
			.copied()
			.unwrap_or(usize::MAX)
			.cmp(&order.get(b.id.as_str()).copied().unwrap_or(usize::MAX))
			.then_with(|| a.sort_order.cmp(&b.sort_order))
			.then_with(|| a.name.cmp(&b.name))
	});
	Ok(settings)
}

fn supervisor_logs_dir() -> PathBuf {
	repo_root().join("target").join("tmp").join("supervisor-logs")
}

#[tauri::command]
fn save_supervisor_logs(content: String, file_prefix: String) -> Result<String, String> {
	let dir = supervisor_logs_dir();
	std::fs::create_dir_all(&dir).map_err(|e| format!("create logs dir: {e}"))?;
	let ts = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map(|d| d.as_secs())
		.unwrap_or(0);
	let prefix: String = file_prefix.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
	let prefix = if prefix.is_empty() { "supervisor".to_string() } else { prefix };
	let path = dir.join(format!("{prefix}-{ts}.txt"));
	std::fs::write(&path, content.as_bytes()).map_err(|e| format!("write logs: {e}"))?;
	Ok(path.display().to_string())
}

#[tauri::command]
fn reveal_supervisor_logs_dir() -> Result<(), String> {
	let dir = supervisor_logs_dir();
	std::fs::create_dir_all(&dir).map_err(|e| format!("create logs dir: {e}"))?;
	open_path_in_file_manager(&dir)
}

#[tauri::command]
fn reveal_profiles_dir() -> Result<(), String> {
	let dir = user_profiles_dir();
	std::fs::create_dir_all(&dir).map_err(|e| format!("create profiles dir: {e}"))?;
	open_path_in_file_manager(&dir)
}

#[tauri::command]
fn reveal_path(path: String) -> Result<(), String> {
	let path = resolve_repo_path(&path);
	if !path.exists() {
		return Err(format!("path does not exist: {}", path.display()));
	}
	open_path_in_file_manager(&path)
}

#[tauri::command]
fn compress_diagnostics(path: String) -> Result<String, String> {
	let path = resolve_repo_path(&path);
	if !path.is_file() {
		return Err(format!("diagnostics file does not exist: {}", path.display()));
	}
	let archive_path = diagnostics_archive_path(&path);
	compress_file_to_zip(&path, &archive_path)?;
	Ok(archive_path.display().to_string())
}

#[tauri::command]
fn activate_renderer_window(id: u32, state: State<'_, Mutex<SupervisorState>>) -> Result<(), String> {
	let mut state = state.lock().map_err(|_| "supervisor state poisoned".to_string())?;
	refresh_renderer_states(&mut state, false, None);
	let renderer = state.renderers.get(&id).ok_or_else(|| format!("renderer not found: {id}"))?;
	let pid = renderer.info.pid.ok_or_else(|| format!("renderer {id} is not running"))?;
	// renderer プロセス自身に winit::Window::focus_window() を呼ばせる。
	// renderer は Supervisor の子プロセスなので Windows のフォアグラウンドポリシー(2) を通常満たし、
	// PowerShell 経由で別プロセスから SetForegroundWindow するより確実。失敗時のみ PowerShell へフォールバック。
	if send_managed_renderer_control(renderer, &RendererControlCommand::Activate).is_ok() {
		return Ok(());
	}
	activate_process_window(pid)
}

#[tauri::command]
fn pick_file_path(kind: String, settings: State<'_, Mutex<AppRuntimeSettings>>) -> Result<Option<String>, String> {
	let start_dir = if kind == "avatar" {
		let settings = settings.lock().map_err(|_| "app settings state poisoned".to_string())?;
		avatar_model_picker_dir(&settings)
	} else {
		repo_root()
	};
	let mut dialog = rfd::FileDialog::new().set_directory(start_dir);
	dialog = match kind.as_str() {
		"avatar" => dialog
			.add_filter("Avatar model", &["vrm", "gltf", "glb", "unavatar"])
			.add_filter("All files", &["*"]),
		"icon" => dialog
			.add_filter("Image", &["png", "jpg", "jpeg", "ico", "webp"])
			.add_filter("All files", &["*"]),
		_ => dialog.add_filter("All files", &["*"]),
	};
	let picked = dialog.pick_file();
	if kind == "avatar" {
		if let Some(path) = picked.as_deref() {
			let mut settings = settings.lock().map_err(|_| "app settings state poisoned".to_string())?;
			remember_avatar_model_picker_dir(&mut settings, path)?;
		}
	}
	Ok(picked.map(|path| path_for_manifest(&path)))
}

fn avatar_model_picker_dir(settings: &AppRuntimeSettings) -> PathBuf {
	settings
		.last_avatar_model_dir
		.as_deref()
		.map(str::trim)
		.filter(|value| !value.is_empty())
		.map(PathBuf::from)
		.filter(|path| path.is_dir())
		.or_else(dirs::document_dir)
		.unwrap_or_else(repo_root)
}

fn remember_avatar_model_picker_dir(settings: &mut AppRuntimeSettings, picked: &Path) -> Result<(), String> {
	let Some(parent) = avatar_model_picker_parent(picked) else {
		return Ok(());
	};
	if settings.last_avatar_model_dir.as_deref() == Some(parent.as_str()) {
		return Ok(());
	}
	settings.last_avatar_model_dir = Some(parent);
	write_app_settings(settings)
}

fn avatar_model_picker_parent(picked: &Path) -> Option<String> {
	picked
		.parent()
		.map(|parent| parent.to_string_lossy().trim().to_string())
		.filter(|parent| !parent.is_empty())
}

#[tauri::command]
fn read_vrm_metadata(path: String, manifest_path: Option<String>) -> Result<Option<VrmMetadataInfo>, String> {
	let resolved = resolve_avatar_metadata_path(&path, manifest_path.as_deref());
	if !resolved.is_file() {
		return Err(format!("avatar file not found: {}", resolved.display()));
	}
	let bytes = fs::read(&resolved).map_err(|e| format!("read {}: {e}", resolved.display()))?;
	let root = un_avatar_io_vrm::gltf_root_json_from_bytes(&bytes).map_err(|e| format!("read VRM metadata: {e}"))?;
	let Some(extensions) = root.get("extensions").and_then(|value| value.as_object()) else {
		return Ok(None);
	};
	let (flavor, vrm) = if let Some(vrm) = extensions.get("VRM") {
		("VRM 0.x", vrm)
	} else if let Some(vrm) = extensions.get("VRMC_vrm") {
		("VRM 1.0", vrm)
	} else {
		return Ok(None);
	};
	let meta = vrm.get("meta").unwrap_or(&serde_json::Value::Null);
	let spec_version = vrm
		.get("specVersion")
		.or_else(|| vrm.get("spec_version"))
		.and_then(|value| value.as_str())
		.map(str::to_string)
		.unwrap_or_else(|| flavor.to_string());
	let file_size = fs::metadata(&resolved).ok().map(|metadata| metadata.len());
	let file_name = resolved
		.file_name()
		.and_then(|name| name.to_str())
		.unwrap_or(path.as_str())
		.to_string();
	Ok(Some(VrmMetadataInfo {
		path,
		file_name,
		vrm_format: flavor.to_string(),
		spec_version,
		title: first_meta_string(meta, &["name", "title"]),
		version: first_meta_string(meta, &["version"]),
		authors: meta_string_array(meta, "authors")
			.or_else(|| first_meta_string(meta, &["author"]).map(|author| vec![author]))
			.unwrap_or_default(),
		contact_information: first_meta_string(meta, &["contactInformation", "contact_information"]),
		references: meta_string_array(meta, "references")
			.or_else(|| first_meta_string(meta, &["reference"]).map(|reference| vec![reference]))
			.unwrap_or_default(),
		copyright_information: first_meta_string(meta, &["copyrightInformation"]),
		third_party_licenses: first_meta_string(meta, &["thirdPartyLicenses"]),
		license_name: first_meta_string(meta, &["licenseName"]),
		other_license_url: first_meta_string(meta, &["otherLicenseUrl"]),
		other_permission_url: first_meta_string(meta, &["otherPermissionUrl"]),
		thumbnail_data_url: vrm_metadata_thumbnail_data_url(meta, &root, &bytes, &resolved),
		technical_stats: vrm_metadata_technical_stats(vrm, &root, &bytes, &resolved, file_size),
		permissions: vrm_metadata_permissions(meta),
	}))
}

#[tauri::command]
fn read_unavatar_wardrobe_options(path: String, manifest_path: Option<String>) -> Result<UnavatarWardrobeOptions, String> {
	let resolved = resolve_avatar_metadata_path(&path, manifest_path.as_deref());
	if !resolved.is_file() {
		return Err(format!("avatar file not found: {}", resolved.display()));
	}
	let bytes = fs::read(&resolved).map_err(|e| format!("read {}: {e}", resolved.display()))?;
	let root = un_avatar_io_vrm::gltf_root_json_from_bytes(&bytes).map_err(|e| format!("read .unavatar metadata: {e}"))?;
	let Some(wardrobe) = root
		.get("extensions")
		.and_then(|extensions| extensions.get("UN_avatar"))
		.and_then(|unavatar| unavatar.get("wardrobe"))
		.and_then(|wardrobe| wardrobe.as_object())
	else {
		return Ok(UnavatarWardrobeOptions {
			available: false,
			base_label: "Base".to_string(),
			sets: Vec::new(),
			error: None,
		});
	};
	let base_set_id = wardrobe.get("baseSet").and_then(|value| value.as_str()).unwrap_or("base");
	let base_label = wardrobe
		.get("baseLabel")
		.or_else(|| wardrobe.get("baseName"))
		.and_then(|value| value.as_str())
		.unwrap_or("Base")
		.to_string();
	let sets = wardrobe
		.get("sets")
		.and_then(|value| value.as_array())
		.into_iter()
		.flatten()
		.filter_map(|set| {
			let id = set.get("id").and_then(|value| value.as_str())?.trim();
			if id.is_empty() || id == base_set_id {
				return None;
			}
			let name = set
				.get("name")
				.or_else(|| set.get("displayName"))
				.and_then(|value| value.as_str())
				.map(str::trim)
				.filter(|name| !name.is_empty())
				.unwrap_or(id);
			Some(UnavatarWardrobeSetOption {
				id: id.to_string(),
				name: name.to_string(),
			})
		})
		.collect::<Vec<_>>();
	Ok(UnavatarWardrobeOptions {
		available: true,
		base_label,
		sets,
		error: None,
	})
}

fn resolve_avatar_metadata_path(path: &str, manifest_path: Option<&str>) -> PathBuf {
	let trimmed = path.trim();
	let path = PathBuf::from(trimmed);
	if path.is_absolute() {
		return path;
	}
	if let Some(candidate) = manifest_path
		.map(PathBuf::from)
		.and_then(|path| path.parent().map(|parent| parent.join(trimmed)))
		.filter(|candidate| candidate.is_file())
	{
		return candidate;
	}
	repo_root().join(trimmed)
}

fn first_meta_string(meta: &serde_json::Value, keys: &[&str]) -> Option<String> {
	keys.iter()
		.find_map(|key| meta.get(*key).and_then(json_value_display_string))
		.filter(|value| !value.trim().is_empty())
}

fn meta_string_array(meta: &serde_json::Value, key: &str) -> Option<Vec<String>> {
	let values = meta
		.get(key)?
		.as_array()?
		.iter()
		.filter_map(json_value_display_string)
		.filter(|value| !value.trim().is_empty())
		.collect::<Vec<_>>();
	(!values.is_empty()).then_some(values)
}

fn json_value_display_string(value: &serde_json::Value) -> Option<String> {
	match value {
		serde_json::Value::String(value) => Some(value.clone()),
		serde_json::Value::Bool(value) => Some(if *value { "true" } else { "false" }.to_string()),
		serde_json::Value::Number(value) => Some(value.to_string()),
		_ => None,
	}
}

fn vrm_metadata_thumbnail_data_url(meta: &serde_json::Value, root: &serde_json::Value, source_bytes: &[u8], path: &Path) -> Option<String> {
	let (mime, bytes) = vrm_metadata_thumbnail_image(meta, root, source_bytes, path)?;
	Some(format!("data:{mime};base64,{}", BASE64_STANDARD.encode(bytes)))
}

fn vrm_metadata_thumbnail_image(
	meta: &serde_json::Value,
	root: &serde_json::Value,
	source_bytes: &[u8],
	path: &Path,
) -> Option<(&'static str, Vec<u8>)> {
	let image_index = meta
		.get("thumbnailImage")
		.or_else(|| meta.get("texture"))
		.and_then(|value| value.as_u64())? as usize;
	let image = root.get("images")?.as_array()?.get(image_index)?;
	if let Some(uri) = image.get("uri").and_then(|value| value.as_str()) {
		if uri.starts_with("data:image/") {
			let (mime, encoded) = data_image_base64_parts(uri)?;
			let bytes = BASE64_STANDARD.decode(encoded).ok()?;
			return Some((mime, bytes));
		}
		let bytes = fs::read(path.parent()?.join(uri)).ok()?;
		let mime = image_mime_type(image, uri)?;
		return Some((mime, bytes));
	}
	let buffer_view_index = image.get("bufferView").and_then(|value| value.as_u64())? as usize;
	let buffer_view = root.get("bufferViews")?.as_array()?.get(buffer_view_index)?;
	let offset = buffer_view.get("byteOffset").and_then(|value| value.as_u64()).unwrap_or(0) as usize;
	let length = buffer_view.get("byteLength").and_then(|value| value.as_u64())? as usize;
	let glb = gltf::Glb::from_slice(source_bytes).ok()?;
	let bin = glb.bin?;
	let end = offset.checked_add(length)?;
	let bytes = bin.get(offset..end)?;
	let mime = image_mime_type(image, "")?;
	Some((mime, bytes.to_vec()))
}

fn data_image_base64_parts(uri: &str) -> Option<(&'static str, &str)> {
	let (header, encoded) = uri.split_once(',')?;
	let mime = match header.strip_prefix("data:")?.split_once(';')?.0 {
		"image/png" => "image/png",
		"image/jpeg" => "image/jpeg",
		"image/webp" => "image/webp",
		_ => return None,
	};
	if !header.ends_with(";base64") {
		return None;
	}
	Some((mime, encoded))
}

fn image_mime_type(image: &serde_json::Value, uri: &str) -> Option<&'static str> {
	match image.get("mimeType").and_then(|value| value.as_str()) {
		Some("image/png") => Some("image/png"),
		Some("image/jpeg") => Some("image/jpeg"),
		Some("image/webp") => Some("image/webp"),
		_ if uri.ends_with(".png") => Some("image/png"),
		_ if uri.ends_with(".jpg") || uri.ends_with(".jpeg") => Some("image/jpeg"),
		_ if uri.ends_with(".webp") => Some("image/webp"),
		_ => None,
	}
}

fn encode_profile_icon_thumbnail_webp(bytes: &[u8]) -> Result<Vec<u8>, String> {
	let image = image::load_from_memory(bytes).map_err(|e| format!("decode avatar thumbnail: {e}"))?;
	let width = image.width();
	let height = image.height();
	let image = if width > PROFILE_ICON_THUMBNAIL_MAX_DIMENSION || height > PROFILE_ICON_THUMBNAIL_MAX_DIMENSION {
		image.resize(
			PROFILE_ICON_THUMBNAIL_MAX_DIMENSION,
			PROFILE_ICON_THUMBNAIL_MAX_DIMENSION,
			image::imageops::FilterType::Lanczos3,
		)
	} else {
		image
	};
	let rgba = image.to_rgba8();
	let mut output = Vec::new();
	image::codecs::webp::WebPEncoder::new_lossless(&mut output)
		.encode(rgba.as_raw(), rgba.width(), rgba.height(), image::ExtendedColorType::Rgba8)
		.map_err(|e| format!("encode avatar thumbnail WebP: {e}"))?;
	Ok(output)
}

fn remove_profile_icon_thumbnail_files(cache_dir: &Path, stem: &str) {
	for extension in ["webp", "png", "jpg", "jpeg"] {
		let _ = fs::remove_file(cache_dir.join(format!("{stem}.{extension}")));
	}
}

fn remove_profile_icon_thumbnail_cache(setting: &AvatarSetting) {
	let cache_dir = user_profiles_dir().join("assets").join("thumbnails");
	let file_stem = format!("{}-avatar-thumbnail", unique_profile_id(&setting.id));
	remove_profile_icon_thumbnail_files(&cache_dir, &file_stem);
}

fn thumbnail_protocol_response(request: tauri::http::Request<Vec<u8>>) -> tauri::http::Response<Vec<u8>> {
	let Some(file_name) = thumbnail_protocol_file_name(request.uri().path()) else {
		return http_response(400, "text/plain; charset=utf-8", b"bad thumbnail path".to_vec());
	};
	let path = user_profiles_dir().join("assets").join("thumbnails").join(file_name);
	let Ok(bytes) = fs::read(&path) else {
		return http_response(404, "text/plain; charset=utf-8", b"thumbnail not found".to_vec());
	};
	http_response(200, "image/webp", bytes)
}

fn thumbnail_protocol_file_name(path: &str) -> Option<String> {
	let encoded = path.trim_start_matches('/');
	if encoded.is_empty() {
		return None;
	}
	let file_name = percent_decode_utf8(encoded)?;
	if file_name.contains('/') || file_name.contains('\\') || !file_name.ends_with(".webp") {
		return None;
	}
	Some(file_name)
}

fn percent_decode_utf8(input: &str) -> Option<String> {
	let bytes = input.as_bytes();
	let mut output = Vec::with_capacity(bytes.len());
	let mut index = 0;
	while index < bytes.len() {
		if bytes[index] == b'%' {
			let hi = hex_value(*bytes.get(index + 1)?)?;
			let lo = hex_value(*bytes.get(index + 2)?)?;
			output.push((hi << 4) | lo);
			index += 3;
		} else {
			output.push(bytes[index]);
			index += 1;
		}
	}
	String::from_utf8(output).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
	match byte {
		b'0'..=b'9' => Some(byte - b'0'),
		b'a'..=b'f' => Some(byte - b'a' + 10),
		b'A'..=b'F' => Some(byte - b'A' + 10),
		_ => None,
	}
}

fn http_response(status: u16, content_type: &'static str, body: Vec<u8>) -> tauri::http::Response<Vec<u8>> {
	tauri::http::Response::builder()
		.status(status)
		.header("Content-Type", content_type)
		.header("Cache-Control", "no-store")
		.body(body)
		.unwrap_or_else(|_| tauri::http::Response::new(Vec::new()))
}

#[tauri::command]
fn save_avatar_thumbnail_icon(setting_id: String, avatar_path: Option<String>, app: tauri::AppHandle) -> Result<AvatarSetting, String> {
	let setting = resolve_avatar_setting(&setting_id)?;
	let manifest_path = editable_avatar_setting_path(&setting)?;
	let avatar_path = avatar_path
		.as_deref()
		.or(setting.avatar_path.as_deref())
		.map(str::trim)
		.filter(|path| !path.is_empty())
		.ok_or_else(|| "avatar_path is not set".to_string())?;
	let resolved_avatar = resolve_avatar_metadata_path(avatar_path, Some(&manifest_path.display().to_string()));
	if !resolved_avatar.is_file() {
		return Err(format!("avatar file not found: {}", resolved_avatar.display()));
	}
	let bytes = fs::read(&resolved_avatar).map_err(|e| format!("read {}: {e}", resolved_avatar.display()))?;
	let root = un_avatar_io_vrm::gltf_root_json_from_bytes(&bytes).map_err(|e| format!("read VRM metadata: {e}"))?;
	let extensions = root
		.get("extensions")
		.and_then(|value| value.as_object())
		.ok_or_else(|| "VRM metadata extension not found".to_string())?;
	let vrm = extensions
		.get("VRM")
		.or_else(|| extensions.get("VRMC_vrm"))
		.ok_or_else(|| "VRM metadata extension not found".to_string())?;
	let meta = vrm.get("meta").unwrap_or(&serde_json::Value::Null);
	let (_mime, thumbnail_bytes) =
		vrm_metadata_thumbnail_image(meta, &root, &bytes, &resolved_avatar).ok_or_else(|| "avatar thumbnail not found".to_string())?;
	let thumbnail_bytes = encode_profile_icon_thumbnail_webp(&thumbnail_bytes)?;
	let cache_dir = user_profiles_dir().join("assets").join("thumbnails");
	fs::create_dir_all(&cache_dir).map_err(|e| format!("create {}: {e}", cache_dir.display()))?;
	let file_stem = format!("{}-avatar-thumbnail", unique_profile_id(&setting.id));
	remove_profile_icon_thumbnail_files(&cache_dir, &file_stem);
	let file_name = format!("{file_stem}.webp");
	let icon_path = cache_dir.join(file_name);
	fs::write(&icon_path, thumbnail_bytes).map_err(|e| format!("write {}: {e}", icon_path.display()))?;

	let mut manifest = read_manifest_value(&manifest_path)?;
	let icon_path_text = icon_path.display().to_string();
	set_optional_root_string(&mut manifest, "icon_path", icon_path_text.clone())?;
	set_optional_nested_string(&mut manifest, &["window", "icon_path"], icon_path_text)?;
	ensure_avatar_profile_metadata(&mut manifest, &manifest_path, None)?;
	write_manifest_value(&manifest_path, &manifest)?;
	let setting = read_avatar_setting(&manifest_path, ProfileStorage::User)?;
	let manifest_path = rename_avatar_setting_file_if_needed(&manifest_path, &setting)?;
	let setting = if manifest_path == Path::new(&setting.manifest_path) {
		Ok(setting)
	} else {
		read_avatar_setting(&manifest_path, ProfileStorage::User)
	}?;
	refresh_tray_menu(&app)?;
	Ok(setting)
}

#[derive(Default)]
struct VrmTechnicalSummary {
	vertex_count: u64,
	triangle_count: u64,
	bone_count: u64,
	texture_count: u64,
	texture_memory_bytes: u64,
	max_texture_size: Option<(u32, u32)>,
	morph_target_count: u64,
	expression_count: u64,
	perfect_sync_hits: usize,
}

fn vrm_metadata_technical_stats(
	vrm: &serde_json::Value,
	root: &serde_json::Value,
	source_bytes: &[u8],
	path: &Path,
	file_size: Option<u64>,
) -> Vec<VrmMetadataField> {
	let summary = vrm_metadata_technical_summary(vrm, root, source_bytes, path);
	let mut stats = Vec::new();
	if let Some(file_size) = file_size {
		stats.push(VrmMetadataField {
			label: "File size".to_string(),
			value: format_bytes(file_size),
		});
	}
	stats.extend([
		VrmMetadataField {
			label: "Vertices".to_string(),
			value: format_count(summary.vertex_count),
		},
		VrmMetadataField {
			label: "Triangles".to_string(),
			value: format_count(summary.triangle_count),
		},
		VrmMetadataField {
			label: "Bones".to_string(),
			value: format_count(summary.bone_count),
		},
		VrmMetadataField {
			label: "Textures".to_string(),
			value: match summary.max_texture_size {
				Some((width, height)) => format!("{} · max {}x{}", format_count(summary.texture_count), width, height),
				None => format_count(summary.texture_count),
			},
		},
		VrmMetadataField {
			label: "Texture RAM".to_string(),
			value: if summary.texture_memory_bytes > 0 {
				format!("{} RGBA", format_bytes(summary.texture_memory_bytes))
			} else {
				"unknown".to_string()
			},
		},
		VrmMetadataField {
			label: "Morph targets".to_string(),
			value: format_count(summary.morph_target_count),
		},
		VrmMetadataField {
			label: "Expressions".to_string(),
			value: format_count(summary.expression_count),
		},
		VrmMetadataField {
			label: "PerfectSync".to_string(),
			value: if summary.perfect_sync_hits >= 45 {
				format!("supported ({}/52)", summary.perfect_sync_hits)
			} else if summary.perfect_sync_hits > 0 {
				format!("partial ({}/52)", summary.perfect_sync_hits)
			} else {
				"not detected".to_string()
			},
		},
	]);
	stats
}

fn vrm_metadata_technical_summary(
	vrm: &serde_json::Value,
	root: &serde_json::Value,
	source_bytes: &[u8],
	path: &Path,
) -> VrmTechnicalSummary {
	let mut summary = VrmTechnicalSummary::default();
	let accessors = root.get("accessors").and_then(|value| value.as_array());
	if let Some(meshes) = root.get("meshes").and_then(|value| value.as_array()) {
		for mesh in meshes {
			let Some(primitives) = mesh.get("primitives").and_then(|value| value.as_array()) else {
				continue;
			};
			for primitive in primitives {
				let position_count = primitive
					.get("attributes")
					.and_then(|value| value.get("POSITION"))
					.and_then(|value| accessor_count(accessors, value));
				if let Some(count) = position_count {
					summary.vertex_count = summary.vertex_count.saturating_add(count);
				}
				let mode = primitive.get("mode").and_then(|value| value.as_u64()).unwrap_or(4);
				if mode == 4 {
					if let Some(index_count) = primitive.get("indices").and_then(|value| accessor_count(accessors, value)) {
						summary.triangle_count = summary.triangle_count.saturating_add(index_count / 3);
					} else if let Some(position_count) = position_count {
						summary.triangle_count = summary.triangle_count.saturating_add(position_count / 3);
					}
				}
				if let Some(targets) = primitive.get("targets").and_then(|value| value.as_array()) {
					summary.morph_target_count = summary.morph_target_count.saturating_add(targets.len() as u64);
				}
			}
		}
	}

	let mut joints = BTreeSet::new();
	if let Some(skins) = root.get("skins").and_then(|value| value.as_array()) {
		for skin in skins {
			if let Some(values) = skin.get("joints").and_then(|value| value.as_array()) {
				for joint in values.iter().filter_map(|value| value.as_u64()) {
					joints.insert(joint);
				}
			}
		}
	}
	summary.bone_count = joints.len() as u64;

	if let Some(images) = root.get("images").and_then(|value| value.as_array()) {
		summary.texture_count = images.len() as u64;
		for image in images {
			let Some(bytes) = gltf_image_bytes(image, root, source_bytes, path) else {
				continue;
			};
			let Some((width, height)) = image_dimensions(&bytes) else {
				continue;
			};
			summary.texture_memory_bytes = summary.texture_memory_bytes.saturating_add(width as u64 * height as u64 * 4);
			summary.max_texture_size = match summary.max_texture_size {
				Some((max_width, max_height)) if max_width as u64 * max_height as u64 >= width as u64 * height as u64 => {
					Some((max_width, max_height))
				}
				_ => Some((width, height)),
			};
		}
	}

	let (expression_names, expression_count) = vrm_expression_summary(vrm, root);
	summary.expression_count = expression_count;
	summary.perfect_sync_hits = perfect_sync_hit_count(&expression_names);
	summary
}

fn accessor_count(accessors: Option<&Vec<serde_json::Value>>, index: &serde_json::Value) -> Option<u64> {
	let index = index.as_u64()? as usize;
	accessors?.get(index)?.get("count")?.as_u64()
}

fn gltf_image_bytes(image: &serde_json::Value, root: &serde_json::Value, source_bytes: &[u8], path: &Path) -> Option<Vec<u8>> {
	if let Some(uri) = image.get("uri").and_then(|value| value.as_str()) {
		if let Some((_, encoded)) = uri.split_once(";base64,").filter(|(prefix, _)| prefix.starts_with("data:image/")) {
			return BASE64_STANDARD.decode(encoded).ok();
		}
		return fs::read(path.parent()?.join(uri)).ok();
	}
	let buffer_view_index = image.get("bufferView").and_then(|value| value.as_u64())? as usize;
	let buffer_view = root.get("bufferViews")?.as_array()?.get(buffer_view_index)?;
	let offset = buffer_view.get("byteOffset").and_then(|value| value.as_u64()).unwrap_or(0) as usize;
	let length = buffer_view.get("byteLength").and_then(|value| value.as_u64())? as usize;
	let glb = gltf::Glb::from_slice(source_bytes).ok()?;
	let bin = glb.bin?;
	let end = offset.checked_add(length)?;
	Some(bin.get(offset..end)?.to_vec())
}

fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
	if bytes.len() >= 24 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
		return Some((
			u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?),
			u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?),
		));
	}
	jpeg_dimensions(bytes).or_else(|| webp_dimensions(bytes))
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
	if !bytes.starts_with(&[0xff, 0xd8]) {
		return None;
	}
	let mut offset = 2usize;
	while offset + 9 < bytes.len() {
		if bytes[offset] != 0xff {
			offset += 1;
			continue;
		}
		let marker = bytes[offset + 1];
		offset += 2;
		if marker == 0xd9 || marker == 0xda {
			break;
		}
		let length = u16::from_be_bytes(bytes.get(offset..offset + 2)?.try_into().ok()?) as usize;
		if length < 2 || offset + length > bytes.len() {
			return None;
		}
		if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) && length >= 7 {
			let height = u16::from_be_bytes(bytes.get(offset + 3..offset + 5)?.try_into().ok()?) as u32;
			let width = u16::from_be_bytes(bytes.get(offset + 5..offset + 7)?.try_into().ok()?) as u32;
			return Some((width, height));
		}
		offset += length;
	}
	None
}

fn webp_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
	if bytes.len() < 30 || !bytes.starts_with(b"RIFF") || bytes.get(8..12)? != b"WEBP" {
		return None;
	}
	match bytes.get(12..16)? {
		b"VP8X" => Some((
			1 + u32::from_le_bytes([bytes[24], bytes[25], bytes[26], 0]),
			1 + u32::from_le_bytes([bytes[27], bytes[28], bytes[29], 0]),
		)),
		b"VP8L" if bytes.len() >= 25 => {
			let b0 = bytes[21] as u32;
			let b1 = bytes[22] as u32;
			let b2 = bytes[23] as u32;
			let b3 = bytes[24] as u32;
			Some((
				((b1 & 0x3f) << 8 | b0) + 1,
				(((b3 & 0x0f) << 10) | (b2 << 2) | ((b1 & 0xc0) >> 6)) + 1,
			))
		}
		b"VP8 " if bytes.len() >= 30 => Some((
			u16::from_le_bytes(bytes.get(26..28)?.try_into().ok()?) as u32 & 0x3fff,
			u16::from_le_bytes(bytes.get(28..30)?.try_into().ok()?) as u32 & 0x3fff,
		)),
		_ => None,
	}
}

fn vrm_expression_summary(vrm: &serde_json::Value, root: &serde_json::Value) -> (BTreeSet<String>, u64) {
	let mut names = BTreeSet::new();
	let mut count = 0u64;
	if let Some(groups) = vrm
		.get("blendShapeMaster")
		.and_then(|value| value.get("blendShapeGroups"))
		.and_then(|value| value.as_array())
		.or_else(|| vrm.get("blendShapeGroups").and_then(|value| value.as_array()))
	{
		count = count.saturating_add(groups.len() as u64);
		for group in groups {
			for key in ["name", "presetName"] {
				if let Some(name) = group.get(key).and_then(|value| value.as_str()) {
					names.extend(normalized_expression_name_variants(name));
				}
			}
		}
	}
	if let Some(expressions) = vrm.get("expressions").and_then(|value| value.as_object()) {
		for bucket in ["preset", "custom"] {
			if let Some(values) = expressions.get(bucket).and_then(|value| value.as_object()) {
				count = count.saturating_add(values.len() as u64);
				for name in values.keys() {
					names.extend(normalized_expression_name_variants(name));
				}
			}
		}
	}
	if let Some(meshes) = root.get("meshes").and_then(|value| value.as_array()) {
		for mesh in meshes {
			if let Some(target_names) = mesh
				.get("extras")
				.and_then(|value| value.get("targetNames"))
				.and_then(|value| value.as_array())
			{
				if count == 0 {
					count = count.saturating_add(target_names.len() as u64);
				}
				names.extend(
					target_names
						.iter()
						.filter_map(|value| value.as_str())
						.flat_map(normalized_expression_name_variants),
				);
			}
		}
	}
	names.retain(|name| !name.is_empty() && name != "unknown");
	(names, count)
}

fn normalize_expression_name(name: &str) -> String {
	name.chars()
		.filter(|c| c.is_ascii_alphanumeric())
		.flat_map(|c| c.to_lowercase())
		.collect()
}

fn normalized_expression_name_variants(name: &str) -> Vec<String> {
	let normalized = normalize_expression_name(name);
	let mut variants = vec![normalized.clone()];
	if let Some(stem) = normalized.strip_suffix('l').filter(|stem| !stem.ends_with("left")) {
		variants.push(format!("{stem}left"));
	}
	if let Some(stem) = normalized.strip_suffix('r').filter(|stem| !stem.ends_with("right")) {
		variants.push(format!("{stem}right"));
	}
	variants
}

fn perfect_sync_hit_count(names: &BTreeSet<String>) -> usize {
	const ARKIT_52: [&str; 52] = [
		"browdownleft",
		"browdownright",
		"browinnerup",
		"browouterupleft",
		"browouterupright",
		"cheekpuff",
		"cheeksquintleft",
		"cheeksquintright",
		"eyeblinkleft",
		"eyeblinkright",
		"eyelookdownleft",
		"eyelookdownright",
		"eyelookinleft",
		"eyelookinright",
		"eyelookoutleft",
		"eyelookoutright",
		"eyelookupleft",
		"eyelookupright",
		"eyesquintleft",
		"eyesquintright",
		"eyewideleft",
		"eyewideright",
		"jawforward",
		"jawleft",
		"jawopen",
		"jawright",
		"mouthclose",
		"mouthdimpleleft",
		"mouthdimpleright",
		"mouthfrownleft",
		"mouthfrownright",
		"mouthfunnel",
		"mouthleft",
		"mouthlowerdownleft",
		"mouthlowerdownright",
		"mouthpressleft",
		"mouthpressright",
		"mouthpucker",
		"mouthright",
		"mouthrolllower",
		"mouthrollupper",
		"mouthshruglower",
		"mouthshrugupper",
		"mouthsmileleft",
		"mouthsmileright",
		"mouthstretchleft",
		"mouthstretchright",
		"mouthupperupleft",
		"mouthupperupright",
		"nosesneerleft",
		"nosesneerright",
		"tongueout",
	];
	ARKIT_52.iter().filter(|name| names.contains(**name)).count()
}

fn format_count(value: u64) -> String {
	let text = value.to_string();
	let mut out = String::with_capacity(text.len() + text.len() / 3);
	for (i, ch) in text.chars().rev().enumerate() {
		if i > 0 && i % 3 == 0 {
			out.push(',');
		}
		out.push(ch);
	}
	out.chars().rev().collect()
}

fn format_bytes(value: u64) -> String {
	const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
	let mut scaled = value as f64;
	let mut unit = 0usize;
	while scaled >= 1024.0 && unit + 1 < UNITS.len() {
		scaled /= 1024.0;
		unit += 1;
	}
	if unit == 0 {
		format!("{value} {}", UNITS[unit])
	} else if scaled >= 10.0 {
		format!("{scaled:.1} {}", UNITS[unit])
	} else {
		format!("{scaled:.2} {}", UNITS[unit])
	}
}

fn vrm_metadata_permissions(meta: &serde_json::Value) -> Vec<VrmMetadataField> {
	[
		("Allowed user", &["allowedUserName", "avatarPermission"][..]),
		("Credit notation", &["creditNotation"]),
		("Redistribution", &["allowRedistribution"]),
		("Modification", &["modification"]),
		("Violent usage", &["violentUssageName", "allowExcessivelyViolentUsage"]),
		("Sexual usage", &["sexualUssageName", "allowExcessivelySexualUsage"]),
		("Commercial usage", &["commercialUssageName", "commercialUsage"]),
		("Political / religious usage", &["allowPoliticalOrReligiousUsage"]),
		("Antisocial / hate usage", &["allowAntisocialOrHateUsage"]),
	]
	.into_iter()
	.filter_map(|(label, keys)| {
		first_meta_string(meta, keys).map(|value| VrmMetadataField {
			label: label.to_string(),
			value,
		})
	})
	.collect()
}

#[tauri::command]
fn update_avatar_setting_path(setting_id: String, field: String, path: String, app: tauri::AppHandle) -> Result<AvatarSetting, String> {
	let setting = resolve_avatar_setting_direct(&setting_id)?;
	let manifest_path = editable_avatar_setting_path(&setting)?;
	let mut manifest = read_manifest_value(&manifest_path)?;
	let table = manifest.as_table_mut().ok_or_else(|| "manifest root must be a table".to_string())?;
	match field.as_str() {
		"avatar_path" => {
			table.insert(
				"avatar_path".to_string(),
				toml::Value::String(avatar_path_for_manifest_value(&path, &manifest_path)),
			);
		}
		"icon_path" => {
			table.insert("icon_path".to_string(), toml::Value::String(path.clone()));
			let window = table
				.entry("window")
				.or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
				.as_table_mut()
				.ok_or_else(|| "window must be a table".to_string())?;
			window.insert("icon_path".to_string(), toml::Value::String(path));
		}
		_ => return Err(format!("unsupported path field: {field}")),
	}
	ensure_avatar_profile_metadata(&mut manifest, &manifest_path, None)?;
	write_manifest_value(&manifest_path, &manifest)?;
	let setting = read_avatar_setting(&manifest_path, ProfileStorage::User)?;
	let manifest_path = rename_avatar_setting_file_if_needed(&manifest_path, &setting)?;
	let setting = if manifest_path == Path::new(&setting.manifest_path) {
		setting
	} else {
		read_avatar_setting(&manifest_path, ProfileStorage::User)?
	};
	refresh_tray_menu(&app)?;
	Ok(setting)
}

#[tauri::command]
fn update_avatar_setting_value(
	setting_id: String,
	field: String,
	value: serde_json::Value,
	app: tauri::AppHandle,
	state: State<'_, Mutex<SupervisorState>>,
) -> Result<AvatarSetting, String> {
	let setting = update_avatar_setting_manifest(&setting_id, |setting, manifest| {
		apply_avatar_setting_value(manifest, setting, &field, value)
	})?;
	if avatar_setting_change_needs_tray_refresh(&field) {
		refresh_tray_menu(&app)?;
	}
	apply_avatar_setting_runtime_side_effects(&setting, &[field.as_str()], state.inner());
	Ok(setting)
}

#[tauri::command]
fn update_avatar_setting_values(
	setting_id: String,
	updates: Vec<AvatarSettingValueUpdate>,
	app: tauri::AppHandle,
	state: State<'_, Mutex<SupervisorState>>,
) -> Result<AvatarSetting, String> {
	if updates.is_empty() {
		return Err("updates must not be empty".to_string());
	}
	let setting = update_avatar_setting_manifest(&setting_id, |setting, manifest| {
		for update in &updates {
			apply_avatar_setting_value(manifest, setting, &update.field, update.value.clone())?;
		}
		Ok(())
	})?;
	let fields: Vec<&str> = updates.iter().map(|update| update.field.as_str()).collect();
	if fields.iter().any(|field| avatar_setting_change_needs_tray_refresh(field)) {
		refresh_tray_menu(&app)?;
	}
	apply_avatar_setting_runtime_side_effects(&setting, &fields, state.inner());
	Ok(setting)
}

fn update_avatar_setting_manifest(
	setting_id: &str,
	apply: impl FnOnce(&AvatarSetting, &mut toml::Value) -> Result<(), String>,
) -> Result<AvatarSetting, String> {
	let setting = resolve_avatar_setting(setting_id)?;
	let manifest_path = editable_avatar_setting_path(&setting)?;
	let mut manifest = read_manifest_value(&manifest_path)?;
	apply(&setting, &mut manifest)?;
	ensure_avatar_profile_metadata(&mut manifest, &manifest_path, None)?;
	write_manifest_value(&manifest_path, &manifest)?;
	let setting = read_avatar_setting(&manifest_path, ProfileStorage::User)?;
	let manifest_path = rename_avatar_setting_file_if_needed(&manifest_path, &setting)?;
	if manifest_path == Path::new(&setting.manifest_path) {
		Ok(setting)
	} else {
		read_avatar_setting(&manifest_path, ProfileStorage::User)
	}
}

fn avatar_setting_change_needs_tray_refresh(field: &str) -> bool {
	matches!(field, "profile.display_name" | "profile.group")
}

type RuntimeSideEffectApply = fn(&AvatarSetting, &Mutex<SupervisorState>) -> Result<usize, String>;

fn apply_avatar_setting_runtime_side_effects(setting: &AvatarSetting, fields: &[&str], state: &Mutex<SupervisorState>) {
	const RUNTIME_SIDE_EFFECTS: &[(&str, &str, RuntimeSideEffectApply)] = &[
		(
			"effects.avatar.outline.",
			"apply avatar outline to running renderers",
			apply_avatar_outline_to_matching_renderers,
		),
		(
			"effects.avatar.rim.",
			"apply avatar rim to running renderers",
			apply_avatar_rim_to_matching_renderers,
		),
		(
			"effects.avatar.matcap.",
			"apply avatar matcap to running renderers",
			apply_avatar_matcap_to_matching_renderers,
		),
		(
			"effects.avatar.specular.",
			"apply avatar specular to running renderers",
			apply_avatar_specular_to_matching_renderers,
		),
		(
			"effects.avatar.ambient_occlusion.",
			"apply avatar ambient occlusion to running renderers",
			apply_avatar_ambient_occlusion_to_matching_renderers,
		),
		(
			"environment.color.",
			"apply environment color to running renderers",
			apply_environment_color_to_matching_renderers,
		),
		(
			"environment.lighting.",
			"apply lighting to running renderers",
			apply_lighting_to_matching_renderers,
		),
		(
			"effects.post.bloom.",
			"apply bloom to running renderers",
			apply_bloom_to_matching_renderers,
		),
		(
			"effects.post.ssao.",
			"apply ssao to running renderers",
			apply_ssao_to_matching_renderers,
		),
		(
			"effects.avatar.contact_shadow.",
			"apply contact shadow to running renderers",
			apply_contact_shadow_to_matching_renderers,
		),
	];

	for (prefix, label, apply) in RUNTIME_SIDE_EFFECTS {
		if fields.iter().any(|field| field.starts_with(prefix)) {
			if let Err(error) = apply(setting, state) {
				eprintln!("un-avatar-supervisor: {label}: {error}");
			}
		}
	}
}

fn apply_avatar_setting_value(
	manifest: &mut toml::Value,
	setting: &AvatarSetting,
	field: &str,
	value: serde_json::Value,
) -> Result<(), String> {
	match field {
		"avatar_path" => {
			let path = json_string(&value, field)?;
			set_optional_root_string(
				manifest,
				"avatar_path",
				avatar_path_for_manifest_value(&path, Path::new(&setting.manifest_path)),
			)?;
		}
		"wardrobe_set" => {
			set_optional_root_string(manifest, "wardrobe_set", json_string(&value, field)?.trim().to_string())?;
		}
		"icon_path" => {
			let path = json_string(&value, field)?;
			set_optional_root_string(manifest, "icon_path", path.clone())?;
			set_optional_nested_string(manifest, &["window", "icon_path"], path)?;
		}
		field if field.starts_with("profile.") => {
			apply_profile_setting_value(manifest, field, value)?;
		}
		field if field.starts_with("motion.") => {
			apply_motion_setting_value(manifest, setting, field, value)?;
		}
		field if field.starts_with("audio_link.") => {
			apply_audio_link_setting_value(manifest, field, value)?;
		}
		field if field.starts_with("render_quality.") => {
			apply_render_quality_setting_value(manifest, field, value)?;
		}
		field if field.starts_with("effects.avatar.contact_shadow.") => {
			apply_contact_shadow_setting_value(manifest, field, value)?;
		}
		field if field.starts_with("effects.avatar.") => {
			apply_avatar_effect_setting_value(manifest, field, value)?;
		}
		field if field.starts_with("environment.") => {
			apply_environment_setting_value(manifest, field, value)?;
		}
		field if field.starts_with("effects.post.") => {
			apply_post_effect_setting_value(manifest, field, value)?;
		}
		"spring_bones"
		| "physics.contacts.parameter_emission"
		| "physics.dynamics.enable_all_on_launch"
		| "physics.spring_bone.simulation_hz"
		| "physics.spring_bone.substeps" => {
			apply_physics_setting_value(manifest, setting, field, value)?;
		}
		field if field.starts_with("physics.") => {
			apply_physics_setting_value(manifest, setting, field, value)?;
		}
		field if field.starts_with("window.") => {
			apply_window_setting_value(manifest, setting, field, value)?;
		}
		field if field.starts_with("debug.") => {
			apply_debug_setting_value(manifest, field, value)?;
		}
		field if field.starts_with("camera.") => {
			apply_camera_setting_value(manifest, field, value)?;
		}
		field if field.starts_with("output.") => {
			apply_output_setting_value(manifest, field, value)?;
		}
		_ => return Err(format!("unsupported setting field: {field}")),
	}
	Ok(())
}

fn apply_environment_setting_value(manifest: &mut toml::Value, field: &str, value: serde_json::Value) -> Result<(), String> {
	match field {
		"environment.color.exposure" => set_nested_ranged_float(
			manifest,
			&["environment", "color", "exposure"],
			&value,
			field,
			-4.0..=4.0,
			"[-4.0, 4.0]",
		),
		"environment.color.contrast" => set_nested_ranged_float(
			manifest,
			&["environment", "color", "contrast"],
			&value,
			field,
			0.0..=4.0,
			"[0.0, 4.0]",
		),
		"environment.color.saturation" => set_nested_ranged_float(
			manifest,
			&["environment", "color", "saturation"],
			&value,
			field,
			0.0..=4.0,
			"[0.0, 4.0]",
		),
		"environment.color.look" => {
			let look = validate_color_look(&json_string(&value, field)?)?;
			set_nested_string(manifest, &["environment", "color", "look"], look)
		}
		"environment.color.intensity" => set_nested_ranged_float(
			manifest,
			&["environment", "color", "intensity"],
			&value,
			field,
			0.0..=1.0,
			"[0.0, 1.0]",
		),
		"environment.color.temperature" => set_nested_ranged_float(
			manifest,
			&["environment", "color", "temperature"],
			&value,
			field,
			-1.0..=1.0,
			"[-1.0, 1.0]",
		),
		"environment.color.tint" => set_nested_ranged_float(
			manifest,
			&["environment", "color", "tint"],
			&value,
			field,
			-1.0..=1.0,
			"[-1.0, 1.0]",
		),
		"environment.lighting.environment.enabled" => {
			set_nested_json_bool(manifest, &["environment", "lighting", "environment", "enabled"], &value, field)
		}
		"environment.lighting.environment.color" => set_nested_rgb_array(
			manifest,
			&["environment", "lighting", "environment", "color"],
			json_rgb(&value, field)?,
		),
		"environment.lighting.environment.intensity" => set_nested_ranged_float(
			manifest,
			&["environment", "lighting", "environment", "intensity"],
			&value,
			field,
			0.0..=2.0,
			"[0.0, 2.0]",
		),
		"environment.lighting.directional.enabled" => {
			set_nested_json_bool(manifest, &["environment", "lighting", "directional", "enabled"], &value, field)
		}
		"environment.lighting.directional.color" => set_nested_rgb_array(
			manifest,
			&["environment", "lighting", "directional", "color"],
			json_rgb(&value, field)?,
		),
		"environment.lighting.directional.intensity" => set_nested_ranged_float(
			manifest,
			&["environment", "lighting", "directional", "intensity"],
			&value,
			field,
			0.0..=4.0,
			"[0.0, 4.0]",
		),
		"environment.lighting.directional.azimuth_deg" => set_nested_ranged_float(
			manifest,
			&["environment", "lighting", "directional", "azimuth_deg"],
			&value,
			field,
			-360.0..=360.0,
			"[-360.0, 360.0]",
		),
		"environment.lighting.directional.elevation_deg" => set_nested_ranged_float(
			manifest,
			&["environment", "lighting", "directional", "elevation_deg"],
			&value,
			field,
			-89.0..=89.0,
			"[-89.0, 89.0]",
		),
		"environment.lighting.directional.follow_camera_yaw" => set_nested_json_bool(
			manifest,
			&["environment", "lighting", "directional", "follow_camera_yaw"],
			&value,
			field,
		),
		"environment.lighting.directional.follow_camera_pitch" => set_nested_json_bool(
			manifest,
			&["environment", "lighting", "directional", "follow_camera_pitch"],
			&value,
			field,
		),
		_ => Err(format!("unsupported setting field: {field}")),
	}
}

fn apply_audio_link_setting_value(manifest: &mut toml::Value, field: &str, value: serde_json::Value) -> Result<(), String> {
	match field {
		"audio_link.source" => {
			let source = match json_string(&value, field)?.trim().to_ascii_lowercase().as_str() {
				"none" => "none".to_string(),
				"input_device" => "input_device".to_string(),
				other => return Err(format!("invalid {field}: {other}; expected none or input_device")),
			};
			set_nested_string(manifest, &["audio_link", "source"], source)
		}
		"audio_link.input_device_id" => {
			set_optional_nested_string(manifest, &["audio_link", "input_device_id"], json_string(&value, field)?)
		}
		"audio_link.input_device_name_hint" => {
			set_optional_nested_string(manifest, &["audio_link", "input_device_name_hint"], json_string(&value, field)?)
		}
		_ => Err(format!("unsupported setting field: {field}")),
	}
}

fn apply_avatar_effect_setting_value(manifest: &mut toml::Value, field: &str, value: serde_json::Value) -> Result<(), String> {
	match field {
		"effects.avatar.outline.policy" => set_nested_string(
			manifest,
			&["effects", "avatar", "outline", "policy"],
			json_outline_policy(&value, field)?,
		),
		"effects.avatar.outline.type" => set_nested_string(
			manifest,
			&["effects", "avatar", "outline", "type"],
			json_outline_type(&value, field)?,
		),
		"effects.avatar.outline.width" => set_nested_ranged_float(
			manifest,
			&["effects", "avatar", "outline", "width"],
			&value,
			field,
			0.0..=0.05,
			"[0.0, 0.05] meters",
		),
		"effects.avatar.outline.color" => {
			set_nested_rgb_array(manifest, &["effects", "avatar", "outline", "color"], json_rgb(&value, field)?)
		}
		"effects.avatar.outline.lighting_mix" => set_nested_ranged_float(
			manifest,
			&["effects", "avatar", "outline", "lighting_mix"],
			&value,
			field,
			0.0..=1.0,
			"[0.0, 1.0]",
		),
		"effects.avatar.outline.roundness" => set_nested_ranged_float(
			manifest,
			&["effects", "avatar", "outline", "roundness"],
			&value,
			field,
			0.0..=1.0,
			"[0.0, 1.0]",
		),
		"effects.avatar.rim.policy" => {
			set_nested_string(manifest, &["effects", "avatar", "rim", "policy"], json_rim_policy(&value, field)?)
		}
		"effects.avatar.rim.color" => set_nested_rgb_array(manifest, &["effects", "avatar", "rim", "color"], json_rgb(&value, field)?),
		"effects.avatar.rim.intensity" => set_nested_ranged_float(
			manifest,
			&["effects", "avatar", "rim", "intensity"],
			&value,
			field,
			0.0..=4.0,
			"[0.0, 4.0]",
		),
		"effects.avatar.rim.lighting_mix" => set_nested_ranged_float(
			manifest,
			&["effects", "avatar", "rim", "lighting_mix"],
			&value,
			field,
			0.0..=1.0,
			"[0.0, 1.0]",
		),
		"effects.avatar.rim.fresnel_power" => set_nested_ranged_float(
			manifest,
			&["effects", "avatar", "rim", "fresnel_power"],
			&value,
			field,
			0.00001..=32.0,
			"[0.00001, 32.0]",
		),
		"effects.avatar.rim.lift" => set_nested_ranged_float(
			manifest,
			&["effects", "avatar", "rim", "lift"],
			&value,
			field,
			-1.0..=1.0,
			"[-1.0, 1.0]",
		),
		"effects.avatar.matcap.scale" => set_nested_ranged_float(
			manifest,
			&["effects", "avatar", "matcap", "scale"],
			&value,
			field,
			0.0..=2.0,
			"[0.0, 2.0]",
		),
		"effects.avatar.specular.enabled" => set_nested_json_bool(manifest, &["effects", "avatar", "specular", "enabled"], &value, field),
		"effects.avatar.specular.intensity" => set_nested_ranged_float(
			manifest,
			&["effects", "avatar", "specular", "intensity"],
			&value,
			field,
			0.0..=2.0,
			"[0.0, 2.0]",
		),
		"effects.avatar.specular.power" => set_nested_ranged_float(
			manifest,
			&["effects", "avatar", "specular", "power"],
			&value,
			field,
			1.0..=128.0,
			"[1.0, 128.0]",
		),
		"effects.avatar.ambient_occlusion.strength" => set_nested_ranged_float(
			manifest,
			&["effects", "avatar", "ambient_occlusion", "strength"],
			&value,
			field,
			0.0..=2.0,
			"[0.0, 2.0]",
		),
		_ => Err(format!("unsupported setting field: {field}")),
	}
}

fn apply_post_effect_setting_value(manifest: &mut toml::Value, field: &str, value: serde_json::Value) -> Result<(), String> {
	match field {
		"effects.post.bloom.enabled" => set_nested_json_bool(manifest, &["effects", "post", "bloom", "enabled"], &value, field),
		"effects.post.bloom.strength" => set_nested_ranged_float(
			manifest,
			&["effects", "post", "bloom", "strength"],
			&value,
			field,
			0.0..=2.0,
			"[0.0, 2.0]",
		),
		"effects.post.bloom.threshold" => set_nested_ranged_float(
			manifest,
			&["effects", "post", "bloom", "threshold"],
			&value,
			field,
			0.0..=2.0,
			"[0.0, 2.0]",
		),
		"effects.post.bloom.radius" => set_nested_ranged_float(
			manifest,
			&["effects", "post", "bloom", "radius"],
			&value,
			field,
			0.0..=32.0,
			"[0.0, 32.0]",
		),
		"effects.post.bloom.quality" => {
			let quality = validate_bloom_quality(&json_string(&value, field)?)?;
			set_nested_string(manifest, &["effects", "post", "bloom", "quality"], quality)
		}
		"effects.post.ssao.enabled" => set_nested_json_bool(manifest, &["effects", "post", "ssao", "enabled"], &value, field),
		"effects.post.ssao.strength" => set_nested_ranged_float(
			manifest,
			&["effects", "post", "ssao", "strength"],
			&value,
			field,
			0.0..=1.0,
			"[0.0, 1.0]",
		),
		"effects.post.ssao.radius" => set_nested_ranged_float(
			manifest,
			&["effects", "post", "ssao", "radius"],
			&value,
			field,
			1.0..=24.0,
			"[1.0, 24.0]",
		),
		"effects.post.ssao.bias" => set_nested_ranged_float(
			manifest,
			&["effects", "post", "ssao", "bias"],
			&value,
			field,
			0.0..=0.02,
			"[0.0, 0.02]",
		),
		"effects.post.ssao.range" => set_nested_ranged_float(
			manifest,
			&["effects", "post", "ssao", "range"],
			&value,
			field,
			0.001..=0.2,
			"[0.001, 0.2]",
		),
		_ => Err(format!("unsupported setting field: {field}")),
	}
}

fn apply_contact_shadow_setting_value(manifest: &mut toml::Value, field: &str, value: serde_json::Value) -> Result<(), String> {
	match field {
		"effects.avatar.contact_shadow.enabled" => {
			set_nested_json_bool(manifest, &["effects", "avatar", "contact_shadow", "enabled"], &value, field)
		}
		"effects.avatar.contact_shadow.strength" => set_nested_ranged_float(
			manifest,
			&["effects", "avatar", "contact_shadow", "strength"],
			&value,
			field,
			0.0..=1.0,
			"[0.0, 1.0]",
		),
		"effects.avatar.contact_shadow.radius" => set_nested_ranged_float(
			manifest,
			&["effects", "avatar", "contact_shadow", "radius"],
			&value,
			field,
			0.05..=3.0,
			"[0.05, 3.0]",
		),
		"effects.avatar.contact_shadow.softness" => set_nested_ranged_float(
			manifest,
			&["effects", "avatar", "contact_shadow", "softness"],
			&value,
			field,
			0.1..=8.0,
			"[0.1, 8.0]",
		),
		"effects.avatar.contact_shadow.height" => set_nested_ranged_float(
			manifest,
			&["effects", "avatar", "contact_shadow", "height"],
			&value,
			field,
			-1.0..=1.0,
			"[-1.0, 1.0]",
		),
		_ => Err(format!("unsupported setting field: {field}")),
	}
}

fn apply_camera_setting_value(manifest: &mut toml::Value, field: &str, value: serde_json::Value) -> Result<(), String> {
	match field {
		"camera.locked" => set_nested_json_bool(manifest, &["camera", "locked"], &value, field),
		"camera.target_x" => update_camera_target_axis(manifest, 0, json_f32(&value, field)?),
		"camera.target_y" => update_camera_target_axis(manifest, 1, json_f32(&value, field)?),
		"camera.target_z" => update_camera_target_axis(manifest, 2, json_f32(&value, field)?),
		"camera.longitude_deg" => set_nested_float(manifest, &["camera", "longitude_deg"], json_f32(&value, field)?),
		"camera.latitude_deg" => set_nested_ranged_float(manifest, &["camera", "latitude_deg"], &value, field, -89.0..=89.0, "[-89, 89]"),
		"camera.radius" => set_nested_ranged_float(manifest, &["camera", "radius"], &value, field, 0.05..=200.0, "[0.05, 200.0]"),
		"camera.diagonal_fov_deg" => set_nested_ranged_float(
			manifest,
			&["camera", "diagonal_fov_deg"],
			&value,
			field,
			1.0..=160.0,
			"[1.0, 160.0]",
		),
		_ => Err(format!("unsupported setting field: {field}")),
	}
}

fn apply_output_setting_value(manifest: &mut toml::Value, field: &str, value: serde_json::Value) -> Result<(), String> {
	match field {
		"output.spout2.enabled" => set_nested_json_bool(manifest, &["output", "spout2", "enabled"], &value, field),
		"output.spout2.name" => set_nested_json_string(manifest, &["output", "spout2", "name"], &value, field),
		"output.spout2.width" => set_nested_integer(manifest, &["output", "spout2", "width"], i64::from(json_u32(&value, field)?)),
		"output.spout2.height" => set_nested_integer(manifest, &["output", "spout2", "height"], i64::from(json_u32(&value, field)?)),
		_ => Err(format!("unsupported setting field: {field}")),
	}
}

fn apply_window_setting_value(
	manifest: &mut toml::Value,
	setting: &AvatarSetting,
	field: &str,
	value: serde_json::Value,
) -> Result<(), String> {
	match field {
		"window.background_color" => {
			let color = json_rgb(&value, field)?;
			set_root_rgb_array(manifest, "background_color", color)
		}
		"window.decorations" => {
			let decorations = json_bool(&value, field)?;
			set_root_bool(manifest, "decorations", decorations)?;
			set_nested_bool(manifest, &["window", "decorations"], decorations)
		}
		"window.transparent" => {
			let transparent = json_bool(&value, field)?;
			set_root_bool(manifest, "transparent", transparent)?;
			set_nested_bool(manifest, &["window", "transparent"], transparent)?;
			if !transparent {
				set_root_bool(manifest, "input_passthrough", false)?;
				set_nested_bool(manifest, &["window", "input_passthrough"], false)?;
			}
			Ok(())
		}
		"window.input_passthrough" => {
			let input_passthrough = json_bool(&value, field)?;
			if input_passthrough && !setting.transparent {
				return Err("Click-through requires Transparent to be enabled".to_string());
			}
			set_root_bool(manifest, "input_passthrough", input_passthrough)?;
			set_nested_bool(manifest, &["window", "input_passthrough"], input_passthrough)
		}
		"window.always_on_top" => set_nested_json_bool(manifest, &["window", "always_on_top"], &value, field),
		"window.width" => {
			let width = i64::from(validate_window_dimension(Some(json_u32(&value, field)?), "width")?.unwrap());
			set_nested_integer(manifest, &["window", "width"], width)
		}
		"window.height" => {
			let height = i64::from(validate_window_dimension(Some(json_u32(&value, field)?), "height")?.unwrap());
			set_nested_integer(manifest, &["window", "height"], height)
		}
		"window.x" => match json_optional_i16_px(&value, field)? {
			Some(v) => set_nested_integer(manifest, &["window", "x"], v),
			None => remove_nested_key(manifest, &["window", "x"]),
		},
		"window.y" => match json_optional_i16_px(&value, field)? {
			Some(v) => set_nested_integer(manifest, &["window", "y"], v),
			None => remove_nested_key(manifest, &["window", "y"]),
		},
		"window.minimized" => set_nested_json_bool(manifest, &["window", "minimized"], &value, field),
		_ => Err(format!("unsupported setting field: {field}")),
	}
}

fn apply_debug_setting_value(manifest: &mut toml::Value, field: &str, value: serde_json::Value) -> Result<(), String> {
	let key = match field {
		"debug.show_axes" => "show_axes",
		"debug.show_bone_colliders" => "show_bone_colliders",
		"debug.disable_mtoon_outlines" => "disable_mtoon_outlines",
		"debug.disable_rim_lighting" => "disable_rim_lighting",
		"debug.force_shading_shift_zero" => "force_shading_shift_zero",
		"debug.disable_matcap" => "disable_matcap",
		"debug.disable_emissive" => "disable_emissive",
		"debug.disable_shade_color" => "disable_shade_color",
		"debug.disable_normal_map" => "disable_normal_map",
		"debug.base_texture_only" => "base_texture_only",
		_ => return Err(format!("unsupported setting field: {field}")),
	};
	set_nested_json_bool(manifest, &["debug", key], &value, field)
}

fn apply_motion_setting_value(
	manifest: &mut toml::Value,
	setting: &AvatarSetting,
	field: &str,
	value: serde_json::Value,
) -> Result<(), String> {
	match field {
		"motion.vmc_udp.enabled" => {
			let enabled = json_bool(&value, field)?;
			set_nested_bool(manifest, &["motion", "vmc_udp", "enabled"], enabled)?;
			if enabled && setting.vmc_address.is_none() {
				set_nested_string(manifest, &["motion", "vmc_udp", "address"], default_vmc_address())?;
				remove_nested_key(manifest, &["motion", "vmc_udp", "port"])?;
				remove_root_key(manifest, "vmc_port")?;
			}
			Ok(())
		}
		"motion.vmc_udp.address" => {
			set_nested_string(manifest, &["motion", "vmc_udp", "address"], json_socket_addr_string(&value, field)?)?;
			remove_nested_key(manifest, &["motion", "vmc_udp", "port"])?;
			remove_root_key(manifest, "vmc_port")
		}
		"motion.unmotion_zenoh.enabled" => set_nested_json_bool(manifest, &["motion", "unmotion_zenoh", "enabled"], &value, field),
		"motion.unmotion_zenoh.key" => set_nested_json_string(manifest, &["motion", "unmotion_zenoh", "key"], &value, field),
		"motion.primary_source" => {
			let raw = json_string(&value, field)?;
			let normalized = match raw.trim().to_ascii_lowercase().as_str() {
				"vmc" => "vmc".to_string(),
				"unmotion_zenoh" => "unmotion_zenoh".to_string(),
				_ => return Err(format!("invalid {field}: {raw} (expected 'vmc' or 'unmotion_zenoh')")),
			};
			set_nested_string(manifest, &["motion", "primary_source"], normalized)
		}
		"motion.apply_vmc_root_translation" => set_nested_json_bool(manifest, &["motion", "apply_vmc_root_translation"], &value, field),
		"motion.look_at.enabled" => set_nested_json_bool(manifest, &["motion", "look_at", "enabled"], &value, field),
		"motion.look_at.clamp_deg" => {
			let clamp_deg = json_f32(&value, field)?.clamp(0.0, 90.0);
			set_nested_float(manifest, &["motion", "look_at", "clamp_deg"], clamp_deg)
		}
		_ => Err(format!("unsupported setting field: {field}")),
	}
}

fn apply_physics_setting_value(
	manifest: &mut toml::Value,
	setting: &AvatarSetting,
	field: &str,
	value: serde_json::Value,
) -> Result<(), String> {
	match field {
		"spring_bones" => set_nested_json_bool(manifest, &["physics", "dynamics", "enabled"], &value, field),
		"physics.contacts.parameter_emission" => {
			set_nested_json_bool(manifest, &["physics", "contacts", "parameter_emission"], &value, field)
		}
		"physics.dynamics.enable_all_on_launch" => {
			set_nested_json_bool(manifest, &["physics", "dynamics", "enable_all_on_launch"], &value, field)
		}
		"physics.spring_bone.simulation_hz" => set_nested_ranged_float(
			manifest,
			&["physics", "spring_bone", "simulation_hz"],
			&value,
			field,
			30.0..=240.0,
			"[30, 240]",
		),
		"physics.spring_bone.substeps" => {
			set_nested_ranged_u32(manifest, &["physics", "spring_bone", "substeps"], &value, field, 1..=8, "[1, 8]")
		}
		field if field.starts_with("physics.spring_bone.overrides.") => {
			apply_spring_bone_category_override_value(manifest, setting, field, value)
		}
		"physics.bone_colliders.enabled" => {
			set_nested_json_bool(manifest, &["physics", "bone_colliders", "enabled"], &value, field)?;
			let _ = remove_nested_key(manifest, &["physics", "bone_colliders", "parts"]);
			Ok(())
		}
		field if field.starts_with("physics.bone_colliders.radius_mm.") => {
			let part = field
				.strip_prefix("physics.bone_colliders.radius_mm.")
				.ok_or_else(|| format!("unsupported setting field: {field}"))?;
			match part {
				"head" | "neck_chest" | "torso" | "upper_arms" | "lower_arms" | "hands" => {
					set_collider_part_radius_mm(manifest, part, json_f32(&value, field)?)
				}
				_ => Err(format!("unsupported setting field: {field}")),
			}
		}
		_ => Err(format!("unsupported setting field: {field}")),
	}
}

fn apply_render_quality_setting_value(manifest: &mut toml::Value, field: &str, value: serde_json::Value) -> Result<(), String> {
	match field {
		"render_quality.aa" => {
			let aa = json_aa_mode(&value, field)?;
			set_root_string(manifest, "aa", aa.clone())?;
			set_nested_string(manifest, &["render_quality", "aa"], aa)
		}
		"render_quality.texture_resolution_limit" => set_nested_string(
			manifest,
			&["render_quality", "texture_resolution_limit"],
			json_texture_resolution_limit(&value, field)?,
		),
		"render_quality.texture_compression" => set_nested_string(
			manifest,
			&["render_quality", "texture_compression"],
			json_texture_compression_mode(&value, field)?,
		),
		"render_quality.mipmap_filter" => {
			set_nested_string(manifest, &["render_quality", "mipmap_filter"], json_mipmap_filter(&value, field)?)
		}
		"render_quality.render_backend" => {
			set_nested_string(manifest, &["render_quality", "render_backend"], json_render_backend(&value, field)?)
		}
		"render_quality.block_compression_encoder" => set_nested_string(
			manifest,
			&["render_quality", "block_compression_encoder"],
			json_block_compression_encoder(&value, field)?,
		),
		"render_quality.block_compression_cpu_threads" => {
			let threads = json_usize(&value, field)?.max(1);
			set_nested_integer(manifest, &["render_quality", "block_compression_cpu_threads"], threads as i64)
		}
		field if field.starts_with("render_quality.texture_compression_advanced.") => {
			let role = field
				.strip_prefix("render_quality.texture_compression_advanced.")
				.ok_or_else(|| format!("invalid advanced compression field: {field}"))?;
			match role {
				"face" | "eyes" | "clothing" | "normal" | "occlusion" | "emissive" | "generic_color" | "data" => {}
				_ => return Err(format!("unknown advanced compression role: {role}")),
			}
			set_nested_string(
				manifest,
				&["render_quality", "texture_compression_advanced", role],
				json_texture_compression_preference(&value, field)?,
			)
		}
		"render_quality.processed_texture_cache" => {
			set_nested_json_bool(manifest, &["render_quality", "processed_texture_cache"], &value, field)
		}
		"render_quality.skin_tone_matching" => set_nested_json_bool(manifest, &["render_quality", "skin_tone_matching"], &value, field),
		_ => Err(format!("unsupported setting field: {field}")),
	}
}

fn apply_profile_setting_value(manifest: &mut toml::Value, field: &str, value: serde_json::Value) -> Result<(), String> {
	match field {
		"profile.display_name" => {
			let name = json_string(&value, field)?;
			set_profile_value(manifest, "display_name", toml::Value::String(name.clone()))?;
			set_root_string(manifest, "title", name)
		}
		"profile.allow_multiple_renderers" => set_profile_value(
			manifest,
			"allow_multiple_renderers",
			toml::Value::Boolean(json_bool(&value, field)?),
		),
		"profile.notes" => set_profile_value(manifest, "notes", toml::Value::String(json_string(&value, field)?)),
		"profile.group" => set_profile_value(
			manifest,
			"group",
			toml::Value::String(json_string(&value, field)?.trim().to_string()),
		),
		_ => Err(format!("unsupported setting field: {field}")),
	}
}

#[tauri::command]
fn launch_renderer(
	setting_id: String,
	state: State<'_, Mutex<SupervisorState>>,
	settings: State<'_, Mutex<AppRuntimeSettings>>,
) -> Result<RendererInstance, String> {
	let settings = settings
		.lock()
		.map(|settings| settings.clone())
		.map_err(|_| "app settings state poisoned".to_string())?;
	launch_renderer_in_state(&setting_id, &state, &settings)
}

fn launch_renderer_in_state(
	setting_id: &str,
	state: &Mutex<SupervisorState>,
	app_settings: &AppRuntimeSettings,
) -> Result<RendererInstance, String> {
	let setting = resolve_avatar_setting(setting_id)?;
	let manifest_path = PathBuf::from(&setting.manifest_path);
	let manifest_path_text = manifest_path.display().to_string();
	{
		let state = state.lock().map_err(|_| "supervisor state poisoned".to_string())?;
		if let Some(info) = existing_renderer_for_setting(&state, &setting, &manifest_path_text) {
			return Ok(info);
		}
	}
	let prewarm_warning = prewarm_renderer_shaders_for_setting(&setting, &manifest_path).err();
	let mut state = state.lock().map_err(|_| "supervisor state poisoned".to_string())?;
	if let Some(info) = existing_renderer_for_setting(&state, &setting, &manifest_path_text) {
		return Ok(info);
	}
	if let Some(warning) = prewarm_warning {
		push_notification(&mut state, NotificationLevel::Warning, "Shader prewarm failed".to_string(), warning);
	}
	push_transparent_window_backend_warning(&mut state, &setting);
	state.next_id = state.next_id.saturating_add(1);
	let id = state.next_id;
	let runtime_bus_key = renderer_runtime_bus_key(id);
	let mut command = renderer_command(
		&manifest_path,
		&runtime_bus_key,
		&app_settings.renderer_close_hotkey,
		resolve_renderer_window_icon_path(&setting).as_deref(),
	)?;
	configure_hidden_child(&mut command);
	let mut child = command
		.stdin(Stdio::null())
		.stdout(Stdio::null())
		.stderr(Stdio::piped())
		.spawn()
		.map_err(|e| format!("launch renderer: {e}"))?;
	let stderr_tail = spawn_stderr_tail(child.stderr.take());
	let pid = child.id();
	let (runtime_status_cache, runtime_status_stream_stop) =
		spawn_runtime_status_stream(runtime_bus_key.clone(), id, manifest_path_text.clone());
	let launch_control_commands = renderer_launch_control_commands(&setting);
	let info = RendererInstance {
		id,
		name: setting.name,
		state: RendererState::Running,
		pid: Some(pid),
		uptime_secs: 0,
		avatar_path: setting.avatar_path,
		manifest_path: Some(manifest_path_text),
		vmc_address: setting.vmc_address,
		vmc_port: setting.vmc_port,
		motion_vmc_enabled: setting.motion_vmc_enabled,
		motion_unmotion_enabled: setting.motion_unmotion_enabled,
		unmotion_zenoh_key: setting.unmotion_zenoh_key,
		primary_motion_source: setting.primary_motion_source,
		spout_enabled: setting.spout_enabled,
		spout_name: setting.spout_name,
		spout_width: setting.spout_width,
		spout_height: setting.spout_height,
		transparent: setting.transparent,
		input_passthrough: setting.input_passthrough,
		decorations: setting.decorations,
		always_on_top: setting.always_on_top,
		window_width: setting.window_width,
		window_height: setting.window_height,
		last_stderr: None,
		stderr_tail: Vec::new(),
		exit_code: None,
	};
	let info_for_return = info.clone();
	state.renderers.insert(
		id,
		ManagedRenderer {
			info,
			child,
			started_at: Instant::now(),
			runtime_bus_key: runtime_bus_key.clone(),
			runtime_status_cache,
			runtime_status_stream_stop,
			stderr_tail,
			crash_notified: false,
		},
	);
	prewarm_runtime_control_session();
	for command in launch_control_commands {
		spawn_renderer_launch_control(runtime_bus_key.clone(), command);
	}
	Ok(info_for_return)
}

fn existing_renderer_for_setting(state: &SupervisorState, setting: &AvatarSetting, manifest_path_text: &str) -> Option<RendererInstance> {
	if setting.allow_multiple_renderers {
		return None;
	}
	state
		.renderers
		.values()
		.find(|renderer| {
			!matches!(renderer.info.state, RendererState::Exited | RendererState::Crashed)
				&& renderer.info.manifest_path.as_deref() == Some(manifest_path_text)
		})
		.map(|renderer| renderer.info.clone())
}

fn prewarm_renderer_shaders_for_setting(setting: &AvatarSetting, manifest_path: &Path) -> Result<(), String> {
	if setting.transparent {
		return Ok(());
	}
	if setting.render_backend != "vulkan" {
		return Ok(());
	}
	let mut command = renderer_prewarm_command(manifest_path)?;
	configure_hidden_child(&mut command);
	let started = Instant::now();
	let output = command
		.stdin(Stdio::null())
		.output()
		.map_err(|e| format!("shader prewarm launch failed: {e}"))?;
	if output.status.success() {
		return Ok(());
	}
	let stderr = String::from_utf8_lossy(&output.stderr);
	let last_line = stderr
		.lines()
		.rev()
		.find(|line| !line.trim().is_empty())
		.unwrap_or("no stderr output");
	Err(format!(
		"Shader prewarm failed after {:.1}s: {last_line}",
		started.elapsed().as_secs_f64()
	))
}

fn push_transparent_window_backend_warning(state: &mut SupervisorState, setting: &AvatarSetting) {
	if !(cfg!(windows) && setting.transparent && setting.render_backend == "vulkan") {
		return;
	}
	push_notification(
		state,
		NotificationLevel::Warning,
		"Transparent Window uses DX12".to_string(),
		"透明 Window は Windows で DX12 バックエンドを使います。Vulkan shader prewarm/cache は効かず、起動時間が長くなります。".to_string(),
	);
}

#[tauri::command]
fn stop_renderer(id: u32, state: State<'_, Mutex<SupervisorState>>) -> Result<(), String> {
	let mut state = state.lock().map_err(|_| "supervisor state poisoned".to_string())?;
	if let Some(renderer) = state.renderers.get_mut(&id) {
		stop_managed_renderer(id, renderer)?;
	}
	Ok(())
}

fn with_running_renderer<T>(
	id: u32,
	state: &Mutex<SupervisorState>,
	callback: impl FnOnce(&ManagedRenderer) -> Result<T, String>,
) -> Result<T, String> {
	let state = state.lock().map_err(|_| "supervisor state poisoned".to_string())?;
	let renderer = state.renderers.get(&id).ok_or_else(|| format!("renderer not found: {id}"))?;
	if matches!(renderer.info.state, RendererState::Exited | RendererState::Crashed) {
		return Err(format!("renderer {id} is not running"));
	}
	callback(renderer)
}

fn send_renderer_command_by_id(id: u32, state: &Mutex<SupervisorState>, command: RendererControlCommand) -> Result<(), String> {
	with_running_renderer(id, state, |renderer| send_managed_renderer_control(renderer, &command))
}

fn renderer_manifest_path(id: u32, state: &Mutex<SupervisorState>) -> Result<String, String> {
	with_running_renderer(id, state, |renderer| {
		renderer
			.info
			.manifest_path
			.clone()
			.ok_or_else(|| format!("renderer {id} has no manifest path"))
	})
}

#[tauri::command]
fn reset_renderer_camera(id: u32, state: State<'_, Mutex<SupervisorState>>) -> Result<(), String> {
	send_renderer_command_by_id(id, state.inner(), RendererControlCommand::ResetCamera)
}

#[tauri::command]
fn set_renderer_camera_orbit(
	id: u32,
	longitude: Option<f32>,
	latitude: Option<f32>,
	radius: Option<f32>,
	transition: Option<RendererCameraTransition>,
	state: State<'_, Mutex<SupervisorState>>,
) -> Result<(), String> {
	send_renderer_command_by_id(
		id,
		state.inner(),
		RendererControlCommand::SetCameraState {
			target: None,
			longitude_deg: longitude,
			latitude_deg: latitude,
			radius,
			diagonal_fov_deg: None,
			transition,
		},
	)
}

#[tauri::command]
fn set_renderer_camera_state(
	id: u32,
	target: Option<[f32; 3]>,
	longitude_deg: Option<f32>,
	latitude_deg: Option<f32>,
	radius: Option<f32>,
	diagonal_fov_deg: Option<f32>,
	transition: Option<RendererCameraTransition>,
	state: State<'_, Mutex<SupervisorState>>,
) -> Result<(), String> {
	send_renderer_command_by_id(
		id,
		state.inner(),
		RendererControlCommand::SetCameraState {
			target,
			longitude_deg,
			latitude_deg,
			radius,
			diagonal_fov_deg,
			transition,
		},
	)
}

#[tauri::command]
fn set_renderer_clear_color(id: u32, r: f64, g: f64, b: f64, a: f64, state: State<'_, Mutex<SupervisorState>>) -> Result<(), String> {
	send_renderer_command_by_id(
		id,
		state.inner(),
		RendererControlCommand::SetClearColor {
			r: validated_color_component(r, "r")?,
			g: validated_color_component(g, "g")?,
			b: validated_color_component(b, "b")?,
			a: validated_color_component(a, "a")?,
		},
	)
}

fn validated_color_component(value: f64, field: &str) -> Result<f64, String> {
	if !value.is_finite() {
		return Err(format!("{field} must be finite"));
	}
	Ok(value.clamp(0.0, 1.0))
}

#[tauri::command]
fn set_renderer_spout_output(
	id: u32,
	enabled: bool,
	width: Option<u32>,
	height: Option<u32>,
	state: State<'_, Mutex<SupervisorState>>,
) -> Result<(), String> {
	let width = validate_spout_dimension(width, "width")?;
	let height = validate_spout_dimension(height, "height")?;
	if width.is_some() != height.is_some() {
		return Err("Spout width and height must be provided together".to_string());
	}
	with_running_renderer(id, state.inner(), |renderer| {
		send_managed_renderer_control(
			renderer,
			&RendererControlCommand::SetSpoutOutput {
				enabled,
				name: renderer.info.spout_name.clone(),
				width: width.or(renderer.info.spout_width),
				height: height.or(renderer.info.spout_height),
			},
		)
	})
}

#[tauri::command]
fn capture_renderer_screenshot(id: u32, path: Option<String>, state: State<'_, Mutex<SupervisorState>>) -> Result<String, String> {
	let renderer_name = with_running_renderer(id, state.inner(), |renderer| Ok(renderer.info.name.clone()))?;
	let resolved = resolve_screenshot_path(path, &renderer_name)?;
	let path_string = resolved.to_string_lossy().to_string();
	send_renderer_command_by_id(id, state.inner(), RendererControlCommand::Screenshot { path: path_string.clone() })?;
	Ok(path_string)
}

fn resolve_screenshot_path(requested: Option<String>, profile_name: &str) -> Result<PathBuf, String> {
	if let Some(req) = requested {
		let trimmed = req.trim();
		if !trimmed.is_empty() {
			return Ok(PathBuf::from(trimmed));
		}
	}
	let dir = default_screenshot_dir();
	std::fs::create_dir_all(&dir).map_err(|e| format!("create screenshots dir {}: {e}", dir.display()))?;
	let timestamp = jiff::Zoned::now().strftime("%Y-%m-%dT%H%M%S");
	let profile = screenshot_profile_filename_stem(profile_name);
	Ok(dir.join(format!("{timestamp}-{profile}.png")))
}

fn default_screenshot_dir() -> PathBuf {
	dirs::picture_dir()
		.unwrap_or_else(|| repo_root().join("target").join("tmp").join("screenshots"))
		.join("un-avatar")
}

fn screenshot_profile_filename_stem(profile_name: &str) -> String {
	let mut slug = String::new();
	let mut last_dash = false;
	for ch in profile_name.trim().chars() {
		let replacement =
			if ch.is_control() || ch.is_whitespace() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' | '.') {
				'-'
			} else {
				ch
			};
		if replacement == '-' {
			if !last_dash && !slug.is_empty() {
				slug.push('-');
			}
			last_dash = true;
		} else {
			slug.push(replacement);
			last_dash = false;
		}
	}
	while slug.ends_with('-') {
		slug.pop();
	}
	if slug.is_empty() {
		"renderer".to_string()
	} else {
		slug
	}
}

#[tauri::command]
fn set_renderer_expression_override(id: u32, name: String, weight: f32, state: State<'_, Mutex<SupervisorState>>) -> Result<(), String> {
	if !weight.is_finite() {
		return Err("weight must be finite".to_string());
	}
	let weight = weight.clamp(0.0, 1.0);
	let trimmed = name.trim().to_string();
	if trimmed.is_empty() {
		return Err("expression name must not be empty".to_string());
	}
	send_renderer_command_by_id(
		id,
		state.inner(),
		RendererControlCommand::SetExpressionOverride { name: trimmed, weight },
	)
}

#[tauri::command]
fn activate_renderer_runtime_action(
	id: u32,
	action_id: Option<String>,
	menu_path: Option<String>,
	wardrobe_set_id: Option<String>,
	state: State<'_, Mutex<SupervisorState>>,
) -> Result<(), String> {
	let action_id = action_id.map(|value| value.trim().to_string()).filter(|value| !value.is_empty());
	let menu_path = menu_path.map(|value| value.trim().to_string()).filter(|value| !value.is_empty());
	let wardrobe_set_id = wardrobe_set_id
		.map(|value| value.trim().to_string())
		.filter(|value| !value.is_empty());
	if action_id.is_none() && menu_path.is_none() {
		return Err("action_id or menu_path is required".to_string());
	}
	send_renderer_command_by_id(
		id,
		state.inner(),
		RendererControlCommand::ActivateAction {
			action_id,
			menu_path,
			wardrobe_set_id,
		},
	)
}

#[tauri::command]
fn clear_renderer_expression_overrides(id: u32, state: State<'_, Mutex<SupervisorState>>) -> Result<(), String> {
	send_renderer_command_by_id(id, state.inner(), RendererControlCommand::ClearExpressionOverrides)
}

#[tauri::command]
fn set_renderer_look_at(id: u32, enabled: bool, clamp_deg: Option<f32>, state: State<'_, Mutex<SupervisorState>>) -> Result<(), String> {
	let clamp_deg = match clamp_deg {
		Some(d) if d.is_finite() && d >= 0.0 => Some(d.clamp(0.0, 90.0)),
		Some(_) => return Err("clamp_deg must be a non-negative finite number".to_string()),
		None => None,
	};
	send_renderer_command_by_id(id, state.inner(), RendererControlCommand::SetLookAt { enabled, clamp_deg })
}

#[tauri::command]
fn set_renderer_show_axes(id: u32, enabled: bool, state: State<'_, Mutex<SupervisorState>>) -> Result<(), String> {
	send_renderer_command_by_id(id, state.inner(), RendererControlCommand::SetShowAxes { enabled })
}

#[tauri::command]
fn set_renderer_show_bone_colliders(id: u32, enabled: bool, state: State<'_, Mutex<SupervisorState>>) -> Result<(), String> {
	send_renderer_command_by_id(id, state.inner(), RendererControlCommand::SetShowBoneColliders { enabled })
}

#[tauri::command]
fn set_renderer_camera_lock(id: u32, locked: bool, state: State<'_, Mutex<SupervisorState>>) -> Result<(), String> {
	send_renderer_command_by_id(id, state.inner(), RendererControlCommand::SetCameraLock { locked })
}

/// 実行中レンダラーの `[motion] apply_vmc_root_translation` をライブ更新する。
/// プロファイル側 (`AvatarSetting`) は別途 `update_avatar_setting_value` で更新するが、ここでは
/// 動作中の renderer に IPC で即時反映するためのコマンド。
#[tauri::command]
fn set_renderer_apply_vmc_root_translation(id: u32, enabled: bool, state: State<'_, Mutex<SupervisorState>>) -> Result<(), String> {
	send_renderer_command_by_id(id, state.inner(), RendererControlCommand::SetApplyVmcRootTranslation { enabled })
}

#[tauri::command]
fn set_renderer_motion_receivers(
	id: u32,
	vmc_address: Option<String>,
	unmotion_zenoh_enabled: bool,
	unmotion_zenoh_key: String,
	state: State<'_, Mutex<SupervisorState>>,
) -> Result<(), String> {
	send_renderer_command_by_id(
		id,
		state.inner(),
		RendererControlCommand::SetMotionReceivers {
			vmc_address,
			unmotion_zenoh_enabled,
			unmotion_zenoh_key,
		},
	)
}

#[tauri::command]
fn set_renderer_spring_bones(id: u32, setting: RendererSpringBoneSetting, state: State<'_, Mutex<SupervisorState>>) -> Result<(), String> {
	send_renderer_command_by_id(
		id,
		state.inner(),
		RendererControlCommand::SetSpringBones {
			enabled: setting.spring_bones,
			bone_colliders: renderer_bone_collider_config(&setting),
			physics_config: renderer_spring_bone_physics_config(&setting),
		},
	)
}

#[tauri::command]
fn set_renderer_all_dynamics_launch_setting(
	id: u32,
	setting: RendererAllDynamicsSetting,
	state: State<'_, Mutex<SupervisorState>>,
) -> Result<(), String> {
	if !setting.dynamics_enable_all_on_launch {
		return Ok(());
	}
	send_renderer_command_by_id(
		id,
		state.inner(),
		RendererControlCommand::SetAllDynamicsEnabled {
			enabled: setting.dynamics_enable_all_on_launch,
		},
	)
}

#[tauri::command]
fn set_renderer_dynamics_enabled(
	id: u32,
	source_id: String,
	enabled: bool,
	state: State<'_, Mutex<SupervisorState>>,
) -> Result<(), String> {
	let source_id = source_id.trim().to_string();
	if source_id.is_empty() {
		return Err("source_id must not be empty".to_string());
	}
	send_renderer_command_by_id(id, state.inner(), RendererControlCommand::SetDynamicsEnabled { source_id, enabled })
}

#[tauri::command]
fn set_renderer_all_dynamics_enabled(id: u32, enabled: bool, state: State<'_, Mutex<SupervisorState>>) -> Result<(), String> {
	send_renderer_command_by_id(id, state.inner(), RendererControlCommand::SetAllDynamicsEnabled { enabled })
}

/// 旧 UI / IPC 互換の primary motion source 更新。
/// `source` は `"vmc"` / `"unmotion_zenoh"`。renderer 側 `PrimaryMotionSource` の serde 表現と一致させる。
/// プロファイル `[motion] primary_source` の永続化は `update_avatar_setting_value` で別途行う。
#[tauri::command]
fn set_renderer_primary_motion_source(id: u32, source: String, state: State<'_, Mutex<SupervisorState>>) -> Result<(), String> {
	match source.as_str() {
		"vmc" | "unmotion_zenoh" => {}
		_ => return Err(format!("invalid primary_motion_source: {source}")),
	}
	send_renderer_command_by_id(id, state.inner(), RendererControlCommand::SetPrimaryMotionSource { source })
}

/// 実行中レンダラーの Avatar outline effect をライブ更新する。
/// プロファイルの永続化は `update_avatar_setting_value` が担当し、ここでは runtime 反映だけを行う。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn set_renderer_avatar_outline(
	id: u32,
	policy: Option<String>,
	outline_type: Option<String>,
	width: Option<f32>,
	color: Option<[f32; 3]>,
	lighting_mix: Option<f32>,
	roundness: Option<f32>,
	state: State<'_, Mutex<SupervisorState>>,
) -> Result<(), String> {
	if let Some(policy) = policy.as_deref() {
		match policy {
			"authored" | "off" | "override" => {}
			_ => return Err(format!("invalid avatar outline policy: {policy}")),
		}
	}
	if let Some(outline_type) = outline_type.as_deref() {
		match outline_type {
			"mtoon" | "ink" | "brush" | "double" => {}
			_ => return Err(format!("invalid avatar outline type: {outline_type}")),
		}
	}
	validate_optional_f32_range(width, "avatar outline width", 0.0..=0.05, "0..=0.05")?;
	validate_optional_f32_range(lighting_mix, "avatar outline lighting_mix", 0.0..=1.0, "0..=1")?;
	validate_optional_f32_range(roundness, "avatar outline roundness", 0.0..=1.0, "0..=1")?;
	let color = color.map(clamp_rgb);
	send_renderer_command_by_id(
		id,
		state.inner(),
		RendererControlCommand::SetAvatarOutline {
			policy,
			r#type: outline_type,
			width,
			color,
			lighting_mix,
			roundness,
		},
	)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn set_renderer_avatar_rim(
	id: u32,
	policy: Option<String>,
	color: Option<[f32; 3]>,
	intensity: Option<f32>,
	lighting_mix: Option<f32>,
	fresnel_power: Option<f32>,
	lift: Option<f32>,
	state: State<'_, Mutex<SupervisorState>>,
) -> Result<(), String> {
	if let Some(policy) = policy.as_deref() {
		match policy {
			"authored" | "off" | "override" => {}
			_ => return Err(format!("invalid avatar rim policy: {policy}")),
		}
	}
	validate_optional_f32_range(intensity, "avatar rim intensity", 0.0..=4.0, "0..=4")?;
	validate_optional_f32_range(lighting_mix, "avatar rim lighting_mix", 0.0..=1.0, "0..=1")?;
	validate_optional_f32_range(fresnel_power, "avatar rim fresnel_power", 0.00001..=32.0, "0.00001..=32")?;
	validate_optional_f32_range(lift, "avatar rim lift", -1.0..=1.0, "-1..=1")?;
	let color = color.map(clamp_rgb);
	send_renderer_command_by_id(
		id,
		state.inner(),
		RendererControlCommand::SetAvatarRim {
			policy,
			color,
			intensity,
			lighting_mix,
			fresnel_power,
			lift,
		},
	)
}

#[tauri::command]
fn set_renderer_avatar_matcap(id: u32, scale: Option<f32>, state: State<'_, Mutex<SupervisorState>>) -> Result<(), String> {
	validate_optional_f32_range(scale, "avatar matcap scale", 0.0..=2.0, "0..=2")?;
	send_renderer_command_by_id(id, state.inner(), RendererControlCommand::SetAvatarMatcap { scale })
}

#[tauri::command]
fn set_renderer_avatar_specular(
	id: u32,
	enabled: Option<bool>,
	intensity: Option<f32>,
	power: Option<f32>,
	state: State<'_, Mutex<SupervisorState>>,
) -> Result<(), String> {
	validate_optional_f32_range(intensity, "avatar specular intensity", 0.0..=2.0, "0..=2")?;
	validate_optional_f32_range(power, "avatar specular power", 1.0..=128.0, "1..=128")?;
	send_renderer_command_by_id(
		id,
		state.inner(),
		RendererControlCommand::SetAvatarSpecular { enabled, intensity, power },
	)
}

#[tauri::command]
fn set_renderer_avatar_ambient_occlusion(id: u32, strength: Option<f32>, state: State<'_, Mutex<SupervisorState>>) -> Result<(), String> {
	validate_optional_f32_range(strength, "avatar ambient occlusion strength", 0.0..=2.0, "0..=2")?;
	send_renderer_command_by_id(id, state.inner(), RendererControlCommand::SetAvatarAmbientOcclusion { strength })
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn set_renderer_lighting(
	id: u32,
	environment_enabled: Option<bool>,
	environment_color: Option<[f32; 3]>,
	environment_intensity: Option<f32>,
	directional_enabled: Option<bool>,
	directional_color: Option<[f32; 3]>,
	directional_intensity: Option<f32>,
	directional_azimuth_deg: Option<f32>,
	directional_elevation_deg: Option<f32>,
	directional_follow_camera_yaw: Option<bool>,
	directional_follow_camera_pitch: Option<bool>,
	state: State<'_, Mutex<SupervisorState>>,
) -> Result<(), String> {
	validate_optional_f32_range(environment_intensity, "environment light intensity", 0.0..=2.0, "0..=2")?;
	validate_optional_f32_range(directional_intensity, "directional light intensity", 0.0..=4.0, "0..=4")?;
	validate_optional_f32_range(directional_azimuth_deg, "directional light azimuth", -360.0..=360.0, "-360..=360")?;
	validate_optional_f32_range(directional_elevation_deg, "directional light elevation", -89.0..=89.0, "-89..=89")?;
	send_renderer_command_by_id(
		id,
		state.inner(),
		RendererControlCommand::SetLighting {
			environment_enabled,
			environment_color: environment_color.map(clamp_rgb),
			environment_intensity,
			directional_enabled,
			directional_color: directional_color.map(clamp_rgb),
			directional_intensity,
			directional_azimuth_deg,
			directional_elevation_deg,
			directional_follow_camera_yaw,
			directional_follow_camera_pitch,
		},
	)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn set_renderer_environment_color(
	id: u32,
	exposure: Option<f32>,
	contrast: Option<f32>,
	saturation: Option<f32>,
	look: Option<String>,
	intensity: Option<f32>,
	temperature: Option<f32>,
	tint: Option<f32>,
	state: State<'_, Mutex<SupervisorState>>,
) -> Result<(), String> {
	validate_optional_f32_range(exposure, "environment color exposure", -4.0..=4.0, "-4..=4")?;
	validate_optional_f32_range(contrast, "environment color contrast", 0.0..=4.0, "0..=4")?;
	validate_optional_f32_range(saturation, "environment color saturation", 0.0..=4.0, "0..=4")?;
	let look = look.map(|value| validate_color_look(&value)).transpose()?;
	validate_optional_f32_range(intensity, "environment color look intensity", 0.0..=1.0, "0..=1")?;
	validate_optional_f32_range(temperature, "environment color temperature", -1.0..=1.0, "-1..=1")?;
	validate_optional_f32_range(tint, "environment color tint", -1.0..=1.0, "-1..=1")?;
	send_renderer_command_by_id(
		id,
		state.inner(),
		RendererControlCommand::SetEnvironmentColor {
			exposure,
			contrast,
			saturation,
			look,
			intensity,
			temperature,
			tint,
		},
	)
}

#[tauri::command]
fn set_renderer_bloom(
	id: u32,
	enabled: Option<bool>,
	strength: Option<f32>,
	threshold: Option<f32>,
	radius: Option<f32>,
	quality: Option<String>,
	state: State<'_, Mutex<SupervisorState>>,
) -> Result<(), String> {
	validate_optional_f32_range(strength, "bloom strength", 0.0..=2.0, "0..=2")?;
	validate_optional_f32_range(threshold, "bloom threshold", 0.0..=2.0, "0..=2")?;
	validate_optional_f32_range(radius, "bloom radius", 0.0..=32.0, "0..=32")?;
	let quality = quality.map(|value| validate_bloom_quality(&value)).transpose()?;
	send_renderer_command_by_id(
		id,
		state.inner(),
		RendererControlCommand::SetBloom {
			enabled,
			strength,
			threshold,
			radius,
			quality,
		},
	)
}

#[tauri::command]
fn set_renderer_ssao(
	id: u32,
	enabled: Option<bool>,
	strength: Option<f32>,
	radius: Option<f32>,
	bias: Option<f32>,
	range: Option<f32>,
	state: State<'_, Mutex<SupervisorState>>,
) -> Result<(), String> {
	validate_optional_f32_range(strength, "ssao strength", 0.0..=1.0, "0..=1")?;
	validate_optional_f32_range(radius, "ssao radius", 1.0..=24.0, "1..=24")?;
	validate_optional_f32_range(bias, "ssao bias", 0.0..=0.02, "0..=0.02")?;
	validate_optional_f32_range(range, "ssao range", 0.001..=0.2, "0.001..=0.2")?;
	send_renderer_command_by_id(
		id,
		state.inner(),
		RendererControlCommand::SetSsao {
			enabled,
			strength,
			radius,
			bias,
			range,
		},
	)
}

#[tauri::command]
fn set_renderer_contact_shadow(
	id: u32,
	enabled: Option<bool>,
	strength: Option<f32>,
	radius: Option<f32>,
	softness: Option<f32>,
	height: Option<f32>,
	state: State<'_, Mutex<SupervisorState>>,
) -> Result<(), String> {
	validate_optional_f32_range(strength, "contact shadow strength", 0.0..=1.0, "0..=1")?;
	validate_optional_f32_range(radius, "contact shadow radius", 0.05..=3.0, "0.05..=3")?;
	validate_optional_f32_range(softness, "contact shadow softness", 0.1..=8.0, "0.1..=8")?;
	validate_optional_f32_range(height, "contact shadow height", -1.0..=1.0, "-1..=1")?;
	send_renderer_command_by_id(
		id,
		state.inner(),
		RendererControlCommand::SetContactShadow {
			enabled,
			strength,
			radius,
			softness,
			height,
		},
	)
}

#[tauri::command]
fn set_renderer_camera_fov(id: u32, diagonal_deg: f32, state: State<'_, Mutex<SupervisorState>>) -> Result<(), String> {
	if !diagonal_deg.is_finite() || !(1.0..=160.0).contains(&diagonal_deg) {
		return Err(format!("diagonal_deg out of range (1..=160): {diagonal_deg}"));
	}
	send_renderer_command_by_id(id, state.inner(), RendererControlCommand::SetCameraFov { diagonal_deg })
}

/// 現在動作中の renderer のカメラ状態（telemetry に乗っている `camera` フィールド）を
/// `Avatar Settings` の manifest `[camera]` に書き込む。`Save to profile` ボタンが呼び出す。
#[tauri::command]
fn save_renderer_camera_to_profile(
	id: u32,
	state: State<'_, Mutex<SupervisorState>>,
	app: tauri::AppHandle,
) -> Result<RendererRuntimeStatus, String> {
	let (camera, manifest_path) = {
		let mut state = state.lock().map_err(|_| "supervisor state poisoned".to_string())?;
		refresh_renderer_states(&mut state, false, None);
		let renderer = state.renderers.get(&id).ok_or_else(|| format!("renderer not found: {id}"))?;
		if matches!(renderer.info.state, RendererState::Exited | RendererState::Crashed) {
			return Err(format!("renderer {id} is not running"));
		}
		let (telemetry, _telemetry_err) = cached_runtime_telemetry(renderer);
		let camera = telemetry
			.as_ref()
			.and_then(|t| t.camera)
			.ok_or_else(|| format!("renderer {id} has not reported camera state yet"))?;
		let manifest_path = renderer.info.manifest_path.clone();
		(camera, manifest_path)
	};
	let manifest_path = manifest_path.ok_or_else(|| format!("renderer {id} has no manifest path"))?;
	let manifest_path = Path::new(&manifest_path);
	write_camera_state_to_manifest(manifest_path, &camera)?;
	// manifest 変更を Avatar Settings の cache に反映するため tray メニュー等を再構築。
	refresh_tray_menu(&app)?;
	get_renderer_runtime_status(id, state)
}

/// 現在の manifest `[camera]` の値を実行中の renderer に適用する（profile から復元）。
/// renderer 起動後にカメラを動かしたあと「profile に保存した値へ戻す」用途。
#[tauri::command]
fn restore_renderer_camera_from_profile(id: u32, state: State<'_, Mutex<SupervisorState>>) -> Result<(), String> {
	let manifest_path = renderer_manifest_path(id, state.inner())?;
	let manifest = read_manifest_value(Path::new(&manifest_path))?;
	let camera_table = manifest.as_table().and_then(|t| t.get("camera")).and_then(|v| v.as_table());
	let camera_table = camera_table.ok_or_else(|| format!("manifest {manifest_path} has no [camera] section"))?;
	let read_f32 = |key: &str| -> Option<f32> {
		camera_table.get(key).and_then(|v| match v {
			toml::Value::Float(f) => Some(*f as f32),
			toml::Value::Integer(i) => Some(*i as f32),
			_ => None,
		})
	};
	let read_target = || -> Option<[f32; 3]> {
		camera_table.get("target").and_then(|v| v.as_array()).and_then(|arr| {
			if arr.len() != 3 {
				return None;
			}
			let mut out = [0.0_f32; 3];
			for (i, item) in arr.iter().enumerate() {
				out[i] = match item {
					toml::Value::Float(f) => *f as f32,
					toml::Value::Integer(n) => *n as f32,
					_ => return None,
				};
			}
			Some(out)
		})
	};
	let cmd = RendererControlCommand::SetCameraState {
		target: read_target(),
		longitude_deg: read_f32("longitude_deg"),
		latitude_deg: read_f32("latitude_deg"),
		radius: read_f32("radius"),
		diagonal_fov_deg: read_f32("diagonal_fov_deg"),
		transition: None,
	};
	send_renderer_command_by_id(id, state.inner(), cmd)
}

/// 動作中の renderer の window 位置・サイズ（telemetry の `window_position` / `window_inner_size`）を
/// プロファイル `[window] x/y/width/height` に書き戻す。`Save window state` ボタンが呼ぶ。
#[tauri::command]
fn save_renderer_window_to_profile(
	id: u32,
	state: State<'_, Mutex<SupervisorState>>,
	app: tauri::AppHandle,
) -> Result<RendererRuntimeStatus, String> {
	let (position, inner_size, manifest_path) = {
		let mut state = state.lock().map_err(|_| "supervisor state poisoned".to_string())?;
		refresh_renderer_states(&mut state, false, None);
		let renderer = state.renderers.get(&id).ok_or_else(|| format!("renderer not found: {id}"))?;
		if matches!(renderer.info.state, RendererState::Exited | RendererState::Crashed) {
			return Err(format!("renderer {id} is not running"));
		}
		let (telemetry, _telemetry_err) = cached_runtime_telemetry(renderer);
		let position = telemetry
			.as_ref()
			.and_then(|t| t.window_position)
			.ok_or_else(|| format!("renderer {id} has not reported window position yet"))?;
		let inner_size = telemetry.as_ref().and_then(|t| t.window_inner_size);
		let manifest_path = renderer.info.manifest_path.clone();
		(position, inner_size, manifest_path)
	};
	let manifest_path = manifest_path.ok_or_else(|| format!("renderer {id} has no manifest path"))?;
	let manifest_path = Path::new(&manifest_path);
	write_window_state_to_manifest(manifest_path, position, inner_size)?;
	refresh_tray_menu(&app)?;
	get_renderer_runtime_status(id, state)
}

/// プロファイル `[window] x/y/width/height` を実行中 renderer に適用する。`Restore from profile` の window 版。
#[tauri::command]
fn restore_renderer_window_from_profile(id: u32, state: State<'_, Mutex<SupervisorState>>) -> Result<(), String> {
	let manifest_path = renderer_manifest_path(id, state.inner())?;
	let manifest = read_manifest_value(Path::new(&manifest_path))?;
	let window_table = manifest.as_table().and_then(|t| t.get("window")).and_then(|v| v.as_table());
	let window_table = window_table.ok_or_else(|| format!("manifest {manifest_path} has no [window] section"))?;
	let read_i32 = |key: &str| -> Option<i32> {
		window_table.get(key).and_then(|v| match v {
			toml::Value::Integer(n) => i32::try_from(*n).ok(),
			toml::Value::Float(f) => Some(*f as i32),
			_ => None,
		})
	};
	let read_u32 = |key: &str| -> Option<u32> {
		window_table.get(key).and_then(|v| match v {
			toml::Value::Integer(n) => u32::try_from(*n).ok(),
			toml::Value::Float(f) => {
				if *f >= 0.0 {
					Some(*f as u32)
				} else {
					None
				}
			}
			_ => None,
		})
	};
	let x = read_i32("x");
	let y = read_i32("y");
	let width = read_u32("width");
	let height = read_u32("height");
	if x.is_none() && y.is_none() && width.is_none() && height.is_none() {
		return Err(format!("manifest {manifest_path} has no [window] x/y/width/height to restore"));
	}
	// 位置とサイズは別コマンドで送る。
	if x.is_some() || y.is_some() {
		send_renderer_command_by_id(id, state.inner(), RendererControlCommand::SetWindowPosition { x, y })?;
	}
	if width.is_some() || height.is_some() {
		send_renderer_command_by_id(
			id,
			state.inner(),
			RendererControlCommand::SetWindow {
				decorations: None,
				transparent: None,
				input_passthrough: None,
				always_on_top: None,
				minimized: None,
				width,
				height,
			},
		)?;
	}
	Ok(())
}

fn write_window_state_to_manifest(manifest_path: &Path, position: [i32; 2], inner_size: Option<[u32; 2]>) -> Result<(), String> {
	let mut manifest = read_manifest_value(manifest_path)?;
	let table = manifest.as_table_mut().ok_or_else(|| "manifest root must be a table".to_string())?;
	let window_table = table
		.entry("window".to_string())
		.or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
		.as_table_mut()
		.ok_or_else(|| "manifest [window] must be a table".to_string())?;
	window_table.insert("x".to_string(), toml::Value::Integer(i64::from(position[0])));
	window_table.insert("y".to_string(), toml::Value::Integer(i64::from(position[1])));
	if let Some([w, h]) = inner_size {
		window_table.insert("width".to_string(), toml::Value::Integer(i64::from(w)));
		window_table.insert("height".to_string(), toml::Value::Integer(i64::from(h)));
	}
	write_manifest_value(manifest_path, &manifest)
}

fn write_camera_state_to_manifest(manifest_path: &Path, camera: &RendererCameraSnapshot) -> Result<(), String> {
	let mut manifest = read_manifest_value(manifest_path)?;
	let table = manifest.as_table_mut().ok_or_else(|| "manifest root must be a table".to_string())?;
	let camera_table = table
		.entry("camera".to_string())
		.or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
		.as_table_mut()
		.ok_or_else(|| "manifest [camera] must be a table".to_string())?;
	let target = toml::Value::Array(camera.target.iter().map(|v| toml::Value::Float(f64::from(*v))).collect());
	camera_table.insert("target".to_string(), target);
	camera_table.insert("longitude_deg".to_string(), toml::Value::Float(f64::from(camera.longitude_deg)));
	camera_table.insert("latitude_deg".to_string(), toml::Value::Float(f64::from(camera.latitude_deg)));
	camera_table.insert("radius".to_string(), toml::Value::Float(f64::from(camera.radius)));
	camera_table.insert(
		"diagonal_fov_deg".to_string(),
		toml::Value::Float(f64::from(camera.diagonal_fov_deg)),
	);
	write_manifest_value(manifest_path, &manifest)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn set_renderer_window(
	id: u32,
	decorations: Option<bool>,
	transparent: Option<bool>,
	input_passthrough: Option<bool>,
	always_on_top: Option<bool>,
	minimized: Option<bool>,
	width: Option<u32>,
	height: Option<u32>,
	state: State<'_, Mutex<SupervisorState>>,
) -> Result<(), String> {
	let width = validate_window_dimension(width, "width")?;
	let height = validate_window_dimension(height, "height")?;
	let mut state = state.lock().map_err(|_| "supervisor state poisoned".to_string())?;
	let renderer = state.renderers.get_mut(&id).ok_or_else(|| format!("renderer not found: {id}"))?;
	if matches!(renderer.info.state, RendererState::Exited | RendererState::Crashed) {
		return Err(format!("renderer {id} is not running"));
	}
	send_managed_renderer_control(
		renderer,
		&RendererControlCommand::SetWindow {
			decorations,
			transparent,
			input_passthrough,
			always_on_top,
			minimized,
			width,
			height,
		},
	)?;
	if let Some(decorations) = decorations {
		renderer.info.decorations = decorations;
	}
	if let Some(transparent) = transparent {
		renderer.info.transparent = transparent;
		if !transparent {
			renderer.info.input_passthrough = false;
		}
	}
	if let Some(input_passthrough) = input_passthrough {
		renderer.info.input_passthrough = input_passthrough;
	}
	if !renderer.info.transparent {
		renderer.info.input_passthrough = false;
	}
	if let Some(always_on_top) = always_on_top {
		renderer.info.always_on_top = always_on_top;
	}
	if let Some(width) = width {
		renderer.info.window_width = width;
	}
	if let Some(height) = height {
		renderer.info.window_height = height;
	}
	Ok(())
}

fn validate_spout_dimension(value: Option<u32>, field: &str) -> Result<Option<u32>, String> {
	let Some(value) = value else {
		return Ok(None);
	};
	if !(1..=8192).contains(&value) {
		return Err(format!("Spout {field} must be between 1 and 8192"));
	}
	Ok(Some(value))
}

fn validate_window_dimension(value: Option<u32>, field: &str) -> Result<Option<u32>, String> {
	let Some(value) = value else {
		return Ok(None);
	};
	if !(160..=8192).contains(&value) {
		return Err(format!("Window {field} must be between 160 and 8192"));
	}
	Ok(Some(value))
}

fn validate_optional_f32_range(
	value: Option<f32>,
	label: &str,
	range: std::ops::RangeInclusive<f32>,
	range_label: &str,
) -> Result<(), String> {
	if let Some(value) = value {
		if !value.is_finite() || !range.contains(&value) {
			return Err(format!("{label} out of range ({range_label}): {value}"));
		}
	}
	Ok(())
}

#[tauri::command]
fn stop_all_renderers(state: State<'_, Mutex<SupervisorState>>) -> Result<(), String> {
	stop_all_in_state(&state);
	Ok(())
}

fn stop_all_in_state(state: &Mutex<SupervisorState>) {
	stop_all_in_state_with_grace(state, RENDERER_STOP_GRACE_NORMAL);
}

fn stop_all_in_state_with_grace(state: &Mutex<SupervisorState>, grace: Duration) {
	let Ok(mut state) = state.lock() else {
		return;
	};
	let mut stopping = Vec::new();
	for (id, renderer) in state.renderers.iter_mut() {
		if matches!(renderer.info.state, RendererState::Exited | RendererState::Crashed) {
			continue;
		}
		renderer.info.state = RendererState::Stopping;
		renderer.runtime_status_stream_stop.store(true, Ordering::Release);
		drop_renderer_control_session(renderer);
		let graceful_requested = send_managed_renderer_shutdown(renderer).is_ok();
		stopping.push((*id, graceful_requested));
	}
	let deadline = Instant::now() + grace;
	for (id, graceful_requested) in stopping {
		let Some(renderer) = state.renderers.get_mut(&id) else {
			continue;
		};
		let exited = if graceful_requested {
			wait_renderer_exit_until(&mut renderer.child, deadline).unwrap_or(false)
		} else {
			false
		};
		if !exited {
			let _ = renderer.child.kill();
			let _ = renderer.child.wait();
		}
		renderer.info.state = RendererState::Exited;
		renderer.info.pid = None;
		refresh_renderer_stderr(renderer);
	}
}

fn stop_managed_renderer(id: u32, renderer: &mut ManagedRenderer) -> Result<(), String> {
	renderer.info.state = RendererState::Stopping;
	renderer.runtime_status_stream_stop.store(true, Ordering::Release);
	let graceful_requested = send_managed_renderer_shutdown(renderer).is_ok();
	drop_renderer_control_session(renderer);
	if graceful_requested && wait_renderer_exit(&mut renderer.child, RENDERER_STOP_GRACE_NORMAL)? {
		renderer.info.state = RendererState::Exited;
		renderer.info.pid = None;
		refresh_renderer_stderr(renderer);
		return Ok(());
	}
	renderer.child.kill().map_err(|e| format!("stop renderer {id}: {e}"))?;
	let _ = renderer.child.wait();
	renderer.info.state = RendererState::Exited;
	renderer.info.pid = None;
	refresh_renderer_stderr(renderer);
	Ok(())
}

fn wait_renderer_exit(child: &mut Child, timeout: Duration) -> Result<bool, String> {
	wait_renderer_exit_until(child, Instant::now() + timeout)
}

fn wait_renderer_exit_until(child: &mut Child, deadline: Instant) -> Result<bool, String> {
	loop {
		match child.try_wait() {
			Ok(Some(_)) => return Ok(true),
			Ok(None) if Instant::now() < deadline => {
				let remaining = deadline.saturating_duration_since(Instant::now());
				std::thread::sleep(remaining.min(Duration::from_millis(25)));
			}
			Ok(None) => return Ok(false),
			Err(e) => return Err(format!("wait renderer exit: {e}")),
		}
	}
}

fn runtime_status_from_renderer(renderer: &ManagedRenderer) -> RendererRuntimeStatus {
	let info = &renderer.info;
	let runtime_running = matches!(
		info.state,
		RendererState::Running | RendererState::Starting | RendererState::Degraded
	);
	let (cached_telemetry, cache_note) = if runtime_running {
		cached_runtime_telemetry(renderer)
	} else {
		(None, None)
	};
	let telemetry = if runtime_running { cached_telemetry } else { None };
	let cache_note = if telemetry.is_some() { None } else { cache_note };
	let note = telemetry
		.as_ref()
		.and_then(|telemetry| {
			telemetry
				.note
				.clone()
				.or_else(|| spout_runtime_note(telemetry))
				.or_else(|| texture_runtime_note(telemetry))
		})
		.or(cache_note)
		.or_else(|| match &info.state {
			RendererState::Running | RendererState::Starting | RendererState::Degraded => {
				Some(format!("runtime status unavailable on {}", renderer.runtime_bus_key))
			}
			RendererState::Stopping | RendererState::Exited | RendererState::Crashed => None,
		});
	RendererRuntimeStatus {
		id: info.id,
		state: info.state.clone(),
		pid: info.pid,
		connected: telemetry.as_ref().is_some_and(|telemetry| telemetry.connected),
		protocol: telemetry.as_ref().and_then(|telemetry| telemetry.protocol.clone()),
		control_capabilities: telemetry
			.as_ref()
			.map_or_else(Vec::new, |telemetry| telemetry.control_capabilities.clone()),
		scene_state: telemetry
			.as_ref()
			.map(|telemetry| telemetry.scene_state.clone())
			.filter(|state| !state.is_empty())
			.unwrap_or_else(|| "unknown".to_string()),
		uptime_secs: telemetry.as_ref().map_or(info.uptime_secs, |telemetry| telemetry.uptime_secs),
		fps: telemetry.as_ref().and_then(|telemetry| telemetry.fps),
		cpu_ms: telemetry.as_ref().and_then(|telemetry| telemetry.cpu_ms),
		frame_cpu_total_ms: telemetry.as_ref().and_then(|telemetry| telemetry.frame_cpu_total_ms),
		frame_motion_apply_ms: telemetry.as_ref().and_then(|telemetry| telemetry.frame_motion_apply_ms),
		frame_dynamics_step_ms: telemetry.as_ref().and_then(|telemetry| telemetry.frame_dynamics_step_ms),
		frame_globals_ms: telemetry.as_ref().and_then(|telemetry| telemetry.frame_globals_ms),
		frame_surface_acquire_ms: telemetry.as_ref().and_then(|telemetry| telemetry.frame_surface_acquire_ms),
		frame_target_prepare_ms: telemetry.as_ref().and_then(|telemetry| telemetry.frame_target_prepare_ms),
		frame_draw_state_refresh_ms: telemetry.as_ref().and_then(|telemetry| telemetry.frame_draw_state_refresh_ms),
		frame_scene_world_ms: telemetry.as_ref().and_then(|telemetry| telemetry.frame_scene_world_ms),
		frame_draw_skin_palette_ms: telemetry.as_ref().and_then(|telemetry| telemetry.frame_draw_skin_palette_ms),
		frame_draw_skin_palette_write_ms: telemetry.as_ref().and_then(|telemetry| telemetry.frame_draw_skin_palette_write_ms),
		frame_draw_fur_source_vertices_ms: telemetry.as_ref().and_then(|telemetry| telemetry.frame_draw_fur_source_vertices_ms),
		frame_draw_expression_values_ms: telemetry.as_ref().and_then(|telemetry| telemetry.frame_draw_expression_values_ms),
		frame_draw_morph_weights_ms: telemetry.as_ref().and_then(|telemetry| telemetry.frame_draw_morph_weights_ms),
		frame_draw_transform_loop_ms: telemetry.as_ref().and_then(|telemetry| telemetry.frame_draw_transform_loop_ms),
		frame_bone_collider_debug_ms: telemetry.as_ref().and_then(|telemetry| telemetry.frame_bone_collider_debug_ms),
		frame_command_encode_ms: telemetry.as_ref().and_then(|telemetry| telemetry.frame_command_encode_ms),
		frame_submit_present_ms: telemetry.as_ref().and_then(|telemetry| telemetry.frame_submit_present_ms),
		frame_spout_cpu_ms: telemetry.as_ref().and_then(|telemetry| telemetry.frame_spout_cpu_ms),
		frame_contact_eval_ms: telemetry.as_ref().and_then(|telemetry| telemetry.frame_contact_eval_ms),
		frame_runtime_action_eval_ms: telemetry.as_ref().and_then(|telemetry| telemetry.frame_runtime_action_eval_ms),
		gpu_ms: telemetry.as_ref().and_then(|telemetry| telemetry.gpu_ms),
		ram_mb: telemetry.as_ref().and_then(|telemetry| telemetry.ram_mb),
		surface_width: telemetry
			.as_ref()
			.and_then(|telemetry| telemetry.surface_width)
			.or(info.spout_width),
		surface_height: telemetry
			.as_ref()
			.and_then(|telemetry| telemetry.surface_height)
			.or(info.spout_height),
		aa: telemetry.as_ref().and_then(|telemetry| telemetry.aa.clone()),
		texture_resolution_limit: telemetry.as_ref().and_then(|telemetry| telemetry.texture_resolution_limit.clone()),
		texture_compression: telemetry.as_ref().and_then(|telemetry| telemetry.texture_compression.clone()),
		mipmap_filter: telemetry.as_ref().and_then(|telemetry| telemetry.mipmap_filter.clone()),
		processed_texture_cache: telemetry.as_ref().and_then(|telemetry| telemetry.processed_texture_cache),
		texture_summary: telemetry.as_ref().and_then(|telemetry| telemetry.texture_summary.clone()),
		spout_available: telemetry.as_ref().is_some_and(|telemetry| telemetry.spout_available),
		spout_enabled: telemetry.as_ref().map_or(info.spout_enabled, |telemetry| telemetry.spout_enabled),
		spout_name: telemetry
			.as_ref()
			.and_then(|telemetry| telemetry.spout_name.clone())
			.or_else(|| info.spout_name.clone()),
		spout_width: telemetry.as_ref().and_then(|telemetry| telemetry.spout_width).or(info.spout_width),
		spout_height: telemetry
			.as_ref()
			.and_then(|telemetry| telemetry.spout_height)
			.or(info.spout_height),
		spout_frames_attempted: telemetry.as_ref().map_or(0, |telemetry| telemetry.spout_frames_attempted),
		spout_frames_sent: telemetry.as_ref().map_or(0, |telemetry| telemetry.spout_frames_sent),
		spout_frame_failures: telemetry.as_ref().map_or(0, |telemetry| telemetry.spout_frame_failures),
		spout_consecutive_failures: telemetry.as_ref().map_or(0, |telemetry| telemetry.spout_consecutive_failures),
		spout_last_send_ok: telemetry.as_ref().and_then(|telemetry| telemetry.spout_last_send_ok),
		spout_last_readback_ms: telemetry.as_ref().and_then(|telemetry| telemetry.spout_last_readback_ms),
		spout_last_send_ms: telemetry.as_ref().and_then(|telemetry| telemetry.spout_last_send_ms),
		spout_last_total_ms: telemetry.as_ref().and_then(|telemetry| telemetry.spout_last_total_ms),
		spout_sender_initialized: telemetry.as_ref().and_then(|telemetry| telemetry.spout_sender_initialized),
		spout_sender_width: telemetry.as_ref().and_then(|telemetry| telemetry.spout_sender_width),
		spout_sender_height: telemetry.as_ref().and_then(|telemetry| telemetry.spout_sender_height),
		expression_presets: telemetry
			.as_ref()
			.map_or_else(Vec::new, |telemetry| telemetry.expression_presets.clone()),
		look_at_enabled: telemetry.as_ref().is_some_and(|telemetry| telemetry.look_at_enabled),
		look_at_clamp_deg: telemetry.as_ref().and_then(|telemetry| telemetry.look_at_clamp_deg),
		apply_vmc_root_translation: telemetry.as_ref().is_some_and(|telemetry| telemetry.apply_vmc_root_translation),
		unmotion_zenoh_enabled: telemetry.as_ref().is_some_and(|telemetry| telemetry.unmotion_zenoh_enabled),
		unmotion_zenoh_key: telemetry
			.as_ref()
			.map(|telemetry| telemetry.unmotion_zenoh_key.clone())
			.unwrap_or_else(|| info.unmotion_zenoh_key.clone().unwrap_or_default()),
		unmotion_zenoh_received_frames: telemetry.as_ref().map_or(0, |telemetry| telemetry.unmotion_zenoh_received_frames),
		motion_applied_frames: telemetry.as_ref().map_or(0, |telemetry| telemetry.motion_applied_frames),
		audio_link_texture_needed: telemetry.as_ref().is_some_and(|telemetry| telemetry.audio_link_texture_needed),
		primary_motion_source: telemetry
			.as_ref()
			.map(|telemetry| telemetry.primary_motion_source.clone())
			.filter(|s| !s.is_empty())
			.unwrap_or_else(|| info.primary_motion_source.clone()),
		show_axes: telemetry.as_ref().is_some_and(|telemetry| telemetry.show_axes),
		show_bone_colliders: telemetry.as_ref().is_some_and(|telemetry| telemetry.show_bone_colliders),
		bone_collider_count: telemetry.as_ref().map_or(0, |telemetry| telemetry.bone_collider_count),
		bone_collider_source: telemetry
			.as_ref()
			.map(|telemetry| telemetry.bone_collider_source.clone())
			.unwrap_or_else(|| "off".to_string()),
		dynamics_group_count: telemetry.as_ref().map_or(0, |telemetry| telemetry.dynamics_group_count),
		dynamics_enabled_group_count: telemetry.as_ref().map_or(0, |telemetry| telemetry.dynamics_enabled_group_count),
		dynamics_source_enabled_group_count: telemetry
			.as_ref()
			.map_or(0, |telemetry| telemetry.dynamics_source_enabled_group_count),
		dynamics_enabled_override_count: telemetry.as_ref().map_or(0, |telemetry| telemetry.dynamics_enabled_override_count),
		dynamics_vrm_spring_bone_group_count: telemetry
			.as_ref()
			.map_or(0, |telemetry| telemetry.dynamics_vrm_spring_bone_group_count),
		dynamics_vrc_physbone_group_count: telemetry
			.as_ref()
			.map_or(0, |telemetry| telemetry.dynamics_vrc_physbone_group_count),
		dynamics_unknown_group_count: telemetry.as_ref().map_or(0, |telemetry| telemetry.dynamics_unknown_group_count),
		dynamics_limit_group_count: telemetry.as_ref().map_or(0, |telemetry| telemetry.dynamics_limit_group_count),
		dynamics_angle_limit_group_count: telemetry.as_ref().map_or(0, |telemetry| telemetry.dynamics_angle_limit_group_count),
		dynamics_stretch_limit_group_count: telemetry
			.as_ref()
			.map_or(0, |telemetry| telemetry.dynamics_stretch_limit_group_count),
		dynamics_rotation_translation_writeback_group_count: telemetry
			.as_ref()
			.map_or(0, |telemetry| telemetry.dynamics_rotation_translation_writeback_group_count),
		dynamics_translation_writeback_candidate_count: telemetry
			.as_ref()
			.map_or(0, |telemetry| telemetry.dynamics_translation_writeback_candidate_count),
		dynamics_translation_writeback_target_count: telemetry
			.as_ref()
			.map_or(0, |telemetry| telemetry.dynamics_translation_writeback_target_count),
		dynamics_stretch_translation_writeback_group_count: telemetry
			.as_ref()
			.map_or(0, |telemetry| telemetry.dynamics_stretch_translation_writeback_group_count),
		dynamics_stretch_translation_writeback_target_group_count: telemetry
			.as_ref()
			.map_or(0, |telemetry| telemetry.dynamics_stretch_translation_writeback_target_group_count),
		dynamics_grabbing_enabled_group_count: telemetry
			.as_ref()
			.map_or(0, |telemetry| telemetry.dynamics_grabbing_enabled_group_count),
		dynamics_posing_enabled_group_count: telemetry
			.as_ref()
			.map_or(0, |telemetry| telemetry.dynamics_posing_enabled_group_count),
		dynamics_collider_count: telemetry.as_ref().map_or(0, |telemetry| telemetry.dynamics_collider_count),
		dynamics_vrm_spring_bone_collider_count: telemetry
			.as_ref()
			.map_or(0, |telemetry| telemetry.dynamics_vrm_spring_bone_collider_count),
		dynamics_vrc_physbone_collider_count: telemetry
			.as_ref()
			.map_or(0, |telemetry| telemetry.dynamics_vrc_physbone_collider_count),
		dynamics_unknown_collider_count: telemetry.as_ref().map_or(0, |telemetry| telemetry.dynamics_unknown_collider_count),
		dynamics_contact_count: telemetry.as_ref().map_or(0, |telemetry| telemetry.dynamics_contact_count),
		dynamics_vrc_contact_sender_count: telemetry
			.as_ref()
			.map_or(0, |telemetry| telemetry.dynamics_vrc_contact_sender_count),
		dynamics_vrc_contact_receiver_count: telemetry
			.as_ref()
			.map_or(0, |telemetry| telemetry.dynamics_vrc_contact_receiver_count),
		dynamics_contact_parameter_declaration_count: telemetry
			.as_ref()
			.map_or(0, |telemetry| telemetry.dynamics_contact_parameter_declaration_count),
		dynamics_contact_probe_count: telemetry.as_ref().map_or(0, |telemetry| telemetry.dynamics_contact_probe_count),
		dynamics_contact_probe_would_emit_count: telemetry
			.as_ref()
			.map_or(0, |telemetry| telemetry.dynamics_contact_probe_would_emit_count),
		dynamics_contact_parameter_emission_count: telemetry
			.as_ref()
			.map_or(0, |telemetry| telemetry.dynamics_contact_parameter_emission_count),
		dynamics_contact_parameter_emitted_count: telemetry
			.as_ref()
			.map_or(0, |telemetry| telemetry.dynamics_contact_parameter_emitted_count),
		dynamics_contact_parameter_reset_to_zero_count: telemetry
			.as_ref()
			.map_or(0, |telemetry| telemetry.dynamics_contact_parameter_reset_to_zero_count),
		dynamics_constraint_ref_count: telemetry.as_ref().map_or(0, |telemetry| telemetry.dynamics_constraint_ref_count),
		dynamics_vrc_constraint_ref_count: telemetry
			.as_ref()
			.map_or(0, |telemetry| telemetry.dynamics_vrc_constraint_ref_count),
		runtime_parameter_definitions: telemetry
			.as_ref()
			.map(|telemetry| telemetry.runtime_parameter_definitions.clone())
			.unwrap_or_default(),
		runtime_parameter_conflicts: telemetry
			.as_ref()
			.map(|telemetry| telemetry.runtime_parameter_conflicts.clone())
			.unwrap_or_default(),
		runtime_actions: telemetry
			.as_ref()
			.map(|telemetry| telemetry.runtime_actions.clone())
			.unwrap_or_default(),
		runtime_action_target_write_collisions: telemetry
			.as_ref()
			.map(|telemetry| telemetry.runtime_action_target_write_collisions.clone())
			.unwrap_or_default(),
		runtime_action_restore_readiness: telemetry
			.as_ref()
			.map(|telemetry| telemetry.runtime_action_restore_readiness.clone())
			.unwrap_or_default(),
		runtime_action_restore_baseline_candidates: telemetry
			.as_ref()
			.map(|telemetry| telemetry.runtime_action_restore_baseline_candidates.clone())
			.unwrap_or_default(),
		runtime_action_restore_baseline_capture_plan: telemetry
			.as_ref()
			.map(|telemetry| telemetry.runtime_action_restore_baseline_capture_plan.clone())
			.unwrap_or_default(),
		runtime_action_restore_apply_plan: telemetry
			.as_ref()
			.map(|telemetry| telemetry.runtime_action_restore_apply_plan.clone())
			.unwrap_or_default(),
		menu_action_candidates: telemetry
			.as_ref()
			.map(|telemetry| telemetry.menu_action_candidates.clone())
			.unwrap_or_default(),
		menu_wardrobe_candidates: telemetry
			.as_ref()
			.map(|telemetry| telemetry.menu_wardrobe_candidates.clone())
			.unwrap_or_default(),
		contact_parameter_declarations: telemetry
			.as_ref()
			.map(|telemetry| telemetry.contact_parameter_declarations.clone())
			.unwrap_or_default(),
		contact_parameter_emission_enabled: telemetry
			.as_ref()
			.is_some_and(|telemetry| telemetry.contact_parameter_emission_enabled),
		contact_parameter_emissions: telemetry
			.as_ref()
			.map(|telemetry| telemetry.contact_parameter_emissions.clone())
			.unwrap_or_default(),
		contact_probes: telemetry
			.as_ref()
			.map(|telemetry| telemetry.contact_probes.clone())
			.unwrap_or_default(),
		dynamics_groups: telemetry
			.as_ref()
			.map(|telemetry| telemetry.dynamics_groups.clone())
			.unwrap_or_default(),
		dynamics_interaction_hooks: telemetry
			.as_ref()
			.map(|telemetry| telemetry.dynamics_interaction_hooks.clone())
			.unwrap_or_default(),
		dynamics_colliders: telemetry
			.as_ref()
			.map(|telemetry| telemetry.dynamics_colliders.clone())
			.unwrap_or_default(),
		dynamics_constraint_refs: telemetry
			.as_ref()
			.map(|telemetry| telemetry.dynamics_constraint_refs.clone())
			.unwrap_or_default(),
		dynamics_warnings: telemetry
			.as_ref()
			.map(|telemetry| telemetry.dynamics_warnings.clone())
			.unwrap_or_default(),
		camera_locked: telemetry.as_ref().is_some_and(|telemetry| telemetry.camera_locked),
		window_focused: telemetry.as_ref().is_some_and(|telemetry| telemetry.window_focused),
		window_activation_seq: telemetry.as_ref().map_or(0, |telemetry| telemetry.window_activation_seq),
		minimized: telemetry.as_ref().is_some_and(|telemetry| telemetry.minimized),
		camera: telemetry.as_ref().and_then(|telemetry| telemetry.camera),
		clear_color: telemetry.as_ref().map(|telemetry| telemetry.clear_color).unwrap_or_default(),
		transparent_window: telemetry.as_ref().is_some_and(|telemetry| telemetry.transparent_window),
		input_passthrough: telemetry.as_ref().is_some_and(|telemetry| telemetry.input_passthrough),
		startup_phase: telemetry.as_ref().and_then(|telemetry| telemetry.startup_phase.clone()),
		startup_progress: telemetry.as_ref().and_then(|telemetry| telemetry.startup_progress),
		startup_message: telemetry.as_ref().and_then(|telemetry| telemetry.startup_message.clone()),
		note,
	}
}

fn spout_runtime_note(telemetry: &RendererRuntimeTelemetry) -> Option<String> {
	if !telemetry.spout_enabled {
		return None;
	}
	if telemetry.spout_consecutive_failures > 0 {
		return Some(format!(
			"Spout2 send failed for {} consecutive frame(s)",
			telemetry.spout_consecutive_failures
		));
	}
	if telemetry.spout_sender_initialized == Some(false) {
		return Some("Spout2 sender is not initialized".to_string());
	}
	if let (Some(requested_width), Some(requested_height), Some(sender_width), Some(sender_height)) = (
		telemetry.spout_width,
		telemetry.spout_height,
		telemetry.spout_sender_width,
		telemetry.spout_sender_height,
	) {
		if requested_width != sender_width || requested_height != sender_height {
			return Some(format!(
				"Spout2 sender size {sender_width} x {sender_height} differs from requested {requested_width} x {requested_height}"
			));
		}
	}
	None
}

fn texture_runtime_note(telemetry: &RendererRuntimeTelemetry) -> Option<String> {
	let summary = telemetry.texture_summary.as_ref()?;
	if summary.cubemap_fallback_count > 0 {
		return Some(format!(
			"Cubemap upload used fallback for {}/{} cube texture(s); re-export or check sourceLayout metadata",
			summary.cubemap_fallback_count, summary.cubemap_count
		));
	}
	if telemetry.texture_compression.as_deref() == Some("source") {
		return None;
	}
	if summary.compression_fallback_count == 0 {
		return None;
	}
	let fallback = summary.compression_fallback_count;
	if summary.compressed_count == 0 && !summary.compression_bc_supported {
		return Some(format!(
			"Texture compression fell back to RGBA for {fallback} image(s) because BC upload is unavailable"
		));
	}
	if summary.compressed_count == 0 {
		return Some(format!("Texture compression kept {fallback} requested image(s) as RGBA"));
	}
	Some(format!(
		"Texture compression used {} image(s), kept {fallback} as RGBA",
		summary.compressed_count
	))
}

fn cached_runtime_telemetry(renderer: &ManagedRenderer) -> (Option<RendererRuntimeTelemetry>, Option<String>) {
	let Ok(cache) = renderer.runtime_status_cache.lock() else {
		return (None, Some("runtime status cache unavailable".to_string()));
	};
	if let (Some(telemetry), Some(updated_at)) = (&cache.telemetry, cache.updated_at) {
		if updated_at.elapsed() <= Duration::from_secs(2) {
			return (Some(telemetry.clone()), None);
		}
		return (
			None,
			Some(format!(
				"runtime status stream stale for {:.1}s",
				updated_at.elapsed().as_secs_f32()
			)),
		);
	}
	(None, cache.last_error.clone())
}

fn runtime_session_id() -> &'static str {
	RUNTIME_SESSION_ID
		.get_or_init(|| {
			let pid = std::process::id();
			let millis = SystemTime::now()
				.duration_since(UNIX_EPOCH)
				.map(|duration| duration.as_millis())
				.unwrap_or_default();
			format!("{pid}-{millis}")
		})
		.as_str()
}

fn renderer_runtime_bus_key(renderer_id: u32) -> String {
	format!("un-avatar/runtime/{}/renderer/{renderer_id}", runtime_session_id())
}

#[cfg(test)]
fn read_runtime_telemetry(address: SocketAddr) -> Result<RendererRuntimeTelemetry, String> {
	use std::io::Read as _;
	let mut stream = std::net::TcpStream::connect_timeout(&address, Duration::from_millis(80))
		.map_err(|e| format!("connect runtime status {address}: {e}"))?;
	stream
		.set_read_timeout(Some(Duration::from_millis(120)))
		.map_err(|e| format!("runtime status timeout: {e}"))?;
	let mut text = String::new();
	stream
		.read_to_string(&mut text)
		.map_err(|e| format!("read runtime status {address}: {e}"))?;
	serde_json::from_str(&text).map_err(|e| format!("parse runtime status {address}: {e}"))
}

fn spawn_runtime_status_stream(
	base_key: String,
	renderer_id: u32,
	manifest_path: String,
) -> (Arc<Mutex<RendererRuntimeTelemetryCache>>, Arc<AtomicBool>) {
	let cache = Arc::new(Mutex::new(RendererRuntimeTelemetryCache::default()));
	let stop = Arc::new(AtomicBool::new(false));
	let thread_cache = Arc::clone(&cache);
	let thread_stop = Arc::clone(&stop);
	std::thread::spawn(move || {
		while !thread_stop.load(Ordering::Acquire) {
			if let Err(error) = read_runtime_telemetry_stream(&base_key, renderer_id, &manifest_path, &thread_cache, &thread_stop) {
				if let Ok(mut cache) = thread_cache.lock() {
					cache.last_error = Some(error);
				}
			}
			for _ in 0..10 {
				if thread_stop.load(Ordering::Acquire) {
					return;
				}
				std::thread::sleep(Duration::from_millis(50));
			}
		}
	});
	(cache, stop)
}

fn read_runtime_telemetry_stream(
	base_key: &str,
	_renderer_id: u32,
	_manifest_path: &str,
	cache: &Arc<Mutex<RendererRuntimeTelemetryCache>>,
	stop: &AtomicBool,
) -> Result<(), String> {
	use zenoh::Wait as _;
	let session = zenoh::open(zenoh::Config::default())
		.wait()
		.map_err(|e| format!("open runtime bus: {e}"))?;
	let status_key = format!("{base_key}/status");
	let subscriber = session
		.declare_subscriber(&status_key)
		.wait()
		.map_err(|e| format!("subscribe runtime status {status_key}: {e}"))?;
	let mut last_activation_seq = cache
		.lock()
		.ok()
		.and_then(|cache| cache.telemetry.as_ref().map(|telemetry| telemetry.window_activation_seq))
		.unwrap_or(0);
	loop {
		if stop.load(Ordering::Acquire) {
			return Ok(());
		}
		let Some(sample) = subscriber
			.recv_timeout(Duration::from_millis(250))
			.map_err(|e| format!("read runtime status {status_key}: {e}"))?
		else {
			continue;
		};
		let telemetry = serde_json::from_slice::<RendererRuntimeTelemetry>(&sample.payload().to_bytes())
			.map_err(|e| format!("parse runtime status {status_key}: {e}"))?;
		if telemetry.window_focused && telemetry.window_activation_seq > last_activation_seq {
			last_activation_seq = telemetry.window_activation_seq;
		}
		let mut cache = cache.lock().map_err(|_| "runtime status cache poisoned".to_string())?;
		cache.telemetry = Some(telemetry);
		cache.updated_at = Some(Instant::now());
		cache.last_error = None;
	}
}

fn send_managed_renderer_control(renderer: &ManagedRenderer, command: &RendererControlCommand) -> Result<(), String> {
	send_renderer_control_bus(&renderer.runtime_bus_key, command)
}

fn renderer_launch_control_commands(setting: &AvatarSetting) -> Vec<RendererControlCommand> {
	if setting.dynamics_enable_all_on_launch {
		vec![RendererControlCommand::SetAllDynamicsEnabled { enabled: true }]
	} else {
		Vec::new()
	}
}

fn spawn_renderer_launch_control(runtime_bus_key: String, command: RendererControlCommand) {
	let _ = std::thread::Builder::new()
		.name("un-avatar-renderer-launch-control".into())
		.spawn(move || {
			for _ in 0..40 {
				if send_renderer_control_bus(&runtime_bus_key, &command).is_ok() {
					return;
				}
				std::thread::sleep(Duration::from_millis(250));
			}
		});
}

fn send_managed_renderer_shutdown(renderer: &ManagedRenderer) -> Result<(), String> {
	let runtime_bus_key = renderer.runtime_bus_key.clone();
	std::thread::Builder::new()
		.name("un-avatar-renderer-shutdown-publish".into())
		.spawn(move || {
			let _ = send_renderer_control_bus_one_way(&runtime_bus_key, &RendererControlCommand::Shutdown);
		})
		.map(|_| ())
		.map_err(|e| format!("spawn renderer shutdown publisher: {e}"))
}

fn renderer_bone_collider_config(setting: &RendererSpringBoneSetting) -> RendererBoneColliderConfig {
	RendererBoneColliderConfig {
		enabled: setting.bone_colliders_enabled,
		radius_mm: RendererBoneColliderRadiiMm {
			head: setting.bone_collider_head,
			neck_chest: setting.bone_collider_neck_chest,
			torso: setting.bone_collider_torso,
			upper_arms: setting.bone_collider_upper_arms,
			lower_arms: setting.bone_collider_lower_arms,
			hands: setting.bone_collider_hands,
		},
	}
}

fn renderer_spring_bone_physics_config(setting: &RendererSpringBoneSetting) -> Option<RendererSpringBonePhysicsConfig> {
	if !setting.spring_bone_physics_configured {
		return None;
	}
	let overrides: Vec<_> = setting
		.spring_bone_category_overrides
		.iter()
		.filter_map(renderer_spring_bone_category_override)
		.collect();
	Some(RendererSpringBonePhysicsConfig {
		time_mode: "time_based".to_string(),
		simulation_hz: setting.spring_bone_simulation_hz.clamp(30.0, 240.0),
		substeps: setting.spring_bone_substeps.clamp(1, 8),
		overrides,
	})
}

fn renderer_spring_bone_category_override(
	override_setting: &SpringBoneCategoryOverrideSetting,
) -> Option<RendererSpringBoneCategoryOverride> {
	if override_setting.mode == "authored" {
		return None;
	}
	let solver = normalize_spring_bone_solver(&override_setting.solver).unwrap_or_else(|| "verlet".to_string());
	Some(RendererSpringBoneCategoryOverride {
		category: normalize_spring_bone_category_id(&override_setting.category),
		params: RendererSpringBonePhysicsParams {
			solver: Some(solver),
			damping_half_life_ms: override_setting
				.damping_configured
				.then_some(override_setting.damping_half_life_ms.clamp(1.0, 10_000.0)),
			stiffness_hz: override_setting
				.stiffness_configured
				.then_some(override_setting.stiffness_hz.clamp(0.0, 60.0)),
			xpbd_compliance: override_setting
				.xpbd_compliance_configured
				.then_some(override_setting.xpbd_compliance.clamp(0.0, 10.0)),
			gravity_scale: None,
			drag_scale: None,
			constraint_iterations: override_setting
				.constraint_iterations_configured
				.then_some(override_setting.constraint_iterations.clamp(1, 32)),
		},
	})
}

fn manifest_path_key(path: &str) -> String {
	path.replace('\\', "/").to_ascii_lowercase()
}

fn same_manifest_path(left: Option<&str>, right: &str) -> bool {
	left.map(|left| manifest_path_key(left) == manifest_path_key(right))
		.unwrap_or(false)
}

fn send_renderer_command_to_matching_renderers(
	setting: &AvatarSetting,
	state: &Mutex<SupervisorState>,
	command: &RendererControlCommand,
) -> Result<usize, String> {
	let state = state.lock().map_err(|_| "supervisor state poisoned".to_string())?;
	let mut applied = 0;
	let mut failures = Vec::new();
	for renderer in state.renderers.values().filter(|renderer| {
		!matches!(renderer.info.state, RendererState::Exited | RendererState::Crashed)
			&& same_manifest_path(renderer.info.manifest_path.as_deref(), &setting.manifest_path)
	}) {
		match send_managed_renderer_control(renderer, command) {
			Ok(()) => applied += 1,
			Err(error) => failures.push(format!("{}: {error}", renderer.info.name)),
		}
	}
	if applied == 0 && !failures.is_empty() {
		return Err(failures.join("; "));
	}
	Ok(applied)
}

fn apply_avatar_outline_to_matching_renderers(setting: &AvatarSetting, state: &Mutex<SupervisorState>) -> Result<usize, String> {
	send_renderer_command_to_matching_renderers(
		setting,
		state,
		&RendererControlCommand::SetAvatarOutline {
			policy: Some(setting.outline_policy.clone()),
			r#type: Some(setting.outline_type.clone()),
			width: setting.outline_width,
			color: setting.outline_color,
			lighting_mix: setting.outline_lighting_mix,
			roundness: setting.outline_roundness,
		},
	)
}

fn apply_avatar_rim_to_matching_renderers(setting: &AvatarSetting, state: &Mutex<SupervisorState>) -> Result<usize, String> {
	send_renderer_command_to_matching_renderers(
		setting,
		state,
		&RendererControlCommand::SetAvatarRim {
			policy: Some(setting.rim_policy.clone()),
			color: setting.rim_color,
			intensity: setting.rim_intensity,
			lighting_mix: setting.rim_lighting_mix,
			fresnel_power: setting.rim_fresnel_power,
			lift: setting.rim_lift,
		},
	)
}

fn apply_avatar_matcap_to_matching_renderers(setting: &AvatarSetting, state: &Mutex<SupervisorState>) -> Result<usize, String> {
	send_renderer_command_to_matching_renderers(
		setting,
		state,
		&RendererControlCommand::SetAvatarMatcap {
			scale: Some(setting.matcap_scale),
		},
	)
}

fn apply_avatar_specular_to_matching_renderers(setting: &AvatarSetting, state: &Mutex<SupervisorState>) -> Result<usize, String> {
	send_renderer_command_to_matching_renderers(
		setting,
		state,
		&RendererControlCommand::SetAvatarSpecular {
			enabled: Some(setting.specular_enabled),
			intensity: Some(setting.specular_intensity),
			power: Some(setting.specular_power),
		},
	)
}

fn apply_avatar_ambient_occlusion_to_matching_renderers(setting: &AvatarSetting, state: &Mutex<SupervisorState>) -> Result<usize, String> {
	send_renderer_command_to_matching_renderers(
		setting,
		state,
		&RendererControlCommand::SetAvatarAmbientOcclusion {
			strength: Some(setting.ambient_occlusion_strength),
		},
	)
}

fn apply_lighting_to_matching_renderers(setting: &AvatarSetting, state: &Mutex<SupervisorState>) -> Result<usize, String> {
	send_renderer_command_to_matching_renderers(
		setting,
		state,
		&RendererControlCommand::SetLighting {
			environment_enabled: Some(setting.lighting_environment_enabled),
			environment_color: Some(setting.lighting_environment_color),
			environment_intensity: Some(setting.lighting_environment_intensity),
			directional_enabled: Some(setting.lighting_directional_enabled),
			directional_color: Some(setting.lighting_directional_color),
			directional_intensity: Some(setting.lighting_directional_intensity),
			directional_azimuth_deg: Some(setting.lighting_directional_azimuth_deg),
			directional_elevation_deg: Some(setting.lighting_directional_elevation_deg),
			directional_follow_camera_yaw: Some(setting.lighting_directional_follow_camera_yaw),
			directional_follow_camera_pitch: Some(setting.lighting_directional_follow_camera_pitch),
		},
	)
}

fn apply_environment_color_to_matching_renderers(setting: &AvatarSetting, state: &Mutex<SupervisorState>) -> Result<usize, String> {
	send_renderer_command_to_matching_renderers(
		setting,
		state,
		&RendererControlCommand::SetEnvironmentColor {
			exposure: Some(setting.color_exposure),
			contrast: Some(setting.color_contrast),
			saturation: Some(setting.color_saturation),
			look: Some(setting.color_look.clone()),
			intensity: Some(setting.color_look_intensity),
			temperature: Some(setting.color_temperature),
			tint: Some(setting.color_tint),
		},
	)
}

fn apply_bloom_to_matching_renderers(setting: &AvatarSetting, state: &Mutex<SupervisorState>) -> Result<usize, String> {
	send_renderer_command_to_matching_renderers(
		setting,
		state,
		&RendererControlCommand::SetBloom {
			enabled: Some(setting.bloom_enabled),
			strength: Some(setting.bloom_strength),
			threshold: Some(setting.bloom_threshold),
			radius: Some(setting.bloom_radius),
			quality: Some(setting.bloom_quality.clone()),
		},
	)
}

fn apply_ssao_to_matching_renderers(setting: &AvatarSetting, state: &Mutex<SupervisorState>) -> Result<usize, String> {
	send_renderer_command_to_matching_renderers(
		setting,
		state,
		&RendererControlCommand::SetSsao {
			enabled: Some(setting.ssao_enabled),
			strength: Some(setting.ssao_strength),
			radius: Some(setting.ssao_radius),
			bias: Some(setting.ssao_bias),
			range: Some(setting.ssao_range),
		},
	)
}

fn apply_contact_shadow_to_matching_renderers(setting: &AvatarSetting, state: &Mutex<SupervisorState>) -> Result<usize, String> {
	send_renderer_command_to_matching_renderers(
		setting,
		state,
		&RendererControlCommand::SetContactShadow {
			enabled: Some(setting.contact_shadow_enabled),
			strength: Some(setting.contact_shadow_strength),
			radius: Some(setting.contact_shadow_radius),
			softness: Some(setting.contact_shadow_softness),
			height: Some(setting.contact_shadow_height),
		},
	)
}

fn drop_renderer_control_session(renderer: &ManagedRenderer) {
	let _ = renderer;
}

#[derive(Serialize)]
struct RuntimeBusControlRequest {
	request_id: String,
	command: serde_json::Value,
}

#[derive(Deserialize)]
struct RuntimeBusControlResponse {
	request_id: String,
	ok: bool,
	error: Option<String>,
}

fn runtime_control_request(command: &RendererControlCommand) -> Result<RuntimeBusControlRequest, String> {
	let request_id = format!(
		"{}-{}",
		std::process::id(),
		SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.map(|duration| duration.as_nanos())
			.unwrap_or_default()
	);
	Ok(RuntimeBusControlRequest {
		request_id,
		command: serde_json::to_value(command).map_err(|e| format!("serialize renderer control command: {e}"))?,
	})
}

fn runtime_control_session() -> Result<zenoh::Session, String> {
	use zenoh::Wait as _;
	let cell = RUNTIME_CONTROL_SESSION.get_or_init(|| Mutex::new(None));
	if let Some(session) = cell
		.lock()
		.map_err(|_| "runtime control session poisoned".to_string())?
		.as_ref()
		.cloned()
	{
		return Ok(session);
	}
	let opened = zenoh::open(zenoh::Config::default())
		.wait()
		.map_err(|e| format!("open renderer control bus: {e}"))?;
	let mut session = cell.lock().map_err(|_| "runtime control session poisoned".to_string())?;
	if let Some(session) = session.as_ref() {
		Ok(session.clone())
	} else {
		*session = Some(opened.clone());
		Ok(opened)
	}
}

fn prewarm_runtime_control_session() {
	let _ = std::thread::Builder::new()
		.name("un-avatar-runtime-control-prewarm".into())
		.spawn(|| {
			let _ = runtime_control_session();
		});
}

fn publish_runtime_control_request(base_key: &str, request: &RuntimeBusControlRequest) -> Result<(), String> {
	use zenoh::Wait as _;
	let control_key = format!("{base_key}/control");
	let session = runtime_control_session()?;
	let request_json = serde_json::to_string(request).map_err(|e| format!("serialize renderer control request: {e}"))?;
	session
		.put(&control_key, request_json)
		.wait()
		.map_err(|e| format!("publish renderer control {control_key}: {e}"))
}

fn send_renderer_control_bus_one_way(base_key: &str, command: &RendererControlCommand) -> Result<(), String> {
	let request = runtime_control_request(command)?;
	publish_runtime_control_request(base_key, &request)
}

fn send_renderer_control_bus(base_key: &str, command: &RendererControlCommand) -> Result<(), String> {
	use zenoh::Wait as _;
	let request = runtime_control_request(command)?;
	let response_key = format!("{base_key}/control/response/{}", request.request_id);
	let session = runtime_control_session()?;
	let response_subscriber = session
		.declare_subscriber(&response_key)
		.wait()
		.map_err(|e| format!("subscribe renderer control response {response_key}: {e}"))?;
	let control_key = format!("{base_key}/control");
	let request_json = serde_json::to_string(&request).map_err(|e| format!("serialize renderer control request: {e}"))?;
	session
		.put(&control_key, request_json)
		.wait()
		.map_err(|e| format!("publish renderer control {control_key}: {e}"))?;
	let Some(sample) = response_subscriber
		.recv_timeout(Duration::from_secs(10))
		.map_err(|e| format!("read renderer control response {response_key}: {e}"))?
	else {
		return Err(format!("renderer control response timeout on {response_key}"));
	};
	let response = serde_json::from_slice::<RuntimeBusControlResponse>(&sample.payload().to_bytes())
		.map_err(|e| format!("parse renderer control response {response_key}: {e}"))?;
	if response.request_id != request.request_id {
		return Err(format!("renderer control response id mismatch: {}", response.request_id));
	}
	if response.ok {
		Ok(())
	} else {
		Err(response.error.unwrap_or_else(|| "renderer control failed".to_string()))
	}
}

#[cfg(test)]
fn send_renderer_control_session(
	session: &Arc<Mutex<Option<std::net::TcpStream>>>,
	address: SocketAddr,
	command: &RendererControlCommand,
) -> Result<(), String> {
	let command = serde_json::to_string(command).map_err(|e| format!("serialize renderer control command: {e}"))?;
	let mut session = session.lock().map_err(|_| "renderer control session poisoned".to_string())?;
	if let Some(stream) = session.as_mut() {
		match send_renderer_control_on_stream(address, stream, &command) {
			Ok(()) => return Ok(()),
			Err(_) => {
				let _ = stream.shutdown(std::net::Shutdown::Both);
				*session = None;
			}
		}
	}
	let mut stream = connect_renderer_control_stream(address)?;
	send_renderer_control_on_stream(address, &mut stream, &command)?;
	*session = Some(stream);
	Ok(())
}

#[cfg(test)]
fn send_renderer_control(address: SocketAddr, command: &RendererControlCommand) -> Result<(), String> {
	let mut stream = connect_renderer_control_stream(address)?;
	let command = serde_json::to_string(command).map_err(|e| format!("serialize renderer control command: {e}"))?;
	send_renderer_control_on_stream(address, &mut stream, &command)
}

#[cfg(test)]
fn connect_renderer_control_stream(address: SocketAddr) -> Result<std::net::TcpStream, String> {
	let stream = std::net::TcpStream::connect_timeout(&address, Duration::from_millis(120))
		.map_err(|e| format!("connect renderer control {address}: {e}"))?;
	stream
		.set_read_timeout(Some(Duration::from_millis(150)))
		.map_err(|e| format!("renderer control timeout: {e}"))?;
	stream
		.set_write_timeout(Some(Duration::from_millis(150)))
		.map_err(|e| format!("renderer control timeout: {e}"))?;
	Ok(stream)
}

#[cfg(test)]
fn send_renderer_control_on_stream(address: SocketAddr, stream: &mut std::net::TcpStream, command: &str) -> Result<(), String> {
	use std::io::Write as _;
	stream
		.write_all(format!("{command}\n").as_bytes())
		.map_err(|e| format!("write renderer control {address}: {e}"))?;
	stream.flush().map_err(|e| format!("flush renderer control {address}: {e}"))?;
	let reader = stream.try_clone().map_err(|e| format!("clone renderer control {address}: {e}"))?;
	let mut response = String::new();
	let bytes = BufReader::new(reader)
		.read_line(&mut response)
		.map_err(|e| format!("read renderer control {address}: {e}"))?;
	if bytes == 0 {
		return Err(format!("renderer control {address}: connection closed"));
	}
	if response.trim() == "ok" {
		Ok(())
	} else {
		Err(format!("renderer control {address}: {}", response.trim()))
	}
}

fn supervisor_build_info() -> SupervisorBuildInfo {
	SupervisorBuildInfo {
		supervisor_version: env!("CARGO_PKG_VERSION"),
		frontend_version: supervisor_frontend_version(),
		git_head: current_git_head(),
		current_exe: std::env::current_exe().ok().map(|path| path.display().to_string()),
		renderer_exe: renderer_executable_path().display().to_string(),
	}
}

fn supervisor_frontend_version() -> Option<String> {
	let text = fs::read_to_string(repo_root().join("apps").join("un-avatar-supervisor").join("package.json")).ok()?;
	let package: serde_json::Value = serde_json::from_str(&text).ok()?;
	package.get("version")?.as_str().map(str::to_string)
}

fn current_git_head() -> Option<String> {
	let git_dir = repo_root().join(".git");
	let head = fs::read_to_string(git_dir.join("HEAD")).ok()?;
	let head = head.trim();
	let Some(ref_name) = head.strip_prefix("ref: ") else {
		return Some(head.to_string());
	};
	fs::read_to_string(git_dir.join(ref_name)).ok().map(|sha| sha.trim().to_string())
}

fn refresh_renderer_states(state: &mut SupervisorState, crash_notifications: bool, app: Option<&tauri::AppHandle>) {
	let mut notifications = Vec::new();
	for renderer in state.renderers.values_mut() {
		renderer.info.uptime_secs = renderer.started_at.elapsed().as_secs();
		refresh_renderer_stderr(renderer);
		if matches!(renderer.info.state, RendererState::Exited | RendererState::Crashed) {
			continue;
		}
		match renderer.child.try_wait() {
			Ok(Some(status)) => {
				renderer.runtime_status_stream_stop.store(true, Ordering::Release);
				renderer.info.exit_code = status.code();
				renderer.info.pid = None;
				renderer.info.state = if status.success() {
					RendererState::Exited
				} else {
					if crash_notifications && !renderer.crash_notified {
						renderer.crash_notified = true;
						notifications.push((renderer.info.name.clone(), status.code(), renderer.info.last_stderr.clone()));
					}
					RendererState::Crashed
				};
			}
			Ok(None) => renderer.info.state = RendererState::Running,
			Err(e) => {
				renderer.info.last_stderr = Some(e.to_string());
				renderer.info.state = RendererState::Degraded;
			}
		}
	}
	for (name, code, last_stderr) in notifications {
		let title = format!("{name} crashed");
		let body = last_stderr.unwrap_or_else(|| match code {
			Some(code) => format!("Renderer exited with code {code}"),
			None => "Renderer exited without an exit code".to_string(),
		});
		push_notification(state, NotificationLevel::Error, title.clone(), body.clone());
		if let Some(app) = app {
			show_native_notification(app, &title, &body);
		}
	}
	prune_stopped_renderer_history(state);
}

fn prune_stopped_renderer_history(state: &mut SupervisorState) {
	let stopped: Vec<u32> = state
		.renderers
		.iter()
		.filter_map(|(id, renderer)| matches!(renderer.info.state, RendererState::Exited | RendererState::Crashed).then_some(*id))
		.collect();
	let overflow = stopped.len().saturating_sub(MAX_STOPPED_RENDERER_HISTORY);
	for id in stopped.into_iter().take(overflow) {
		state.renderers.remove(&id);
	}
}

fn show_native_notification(app: &tauri::AppHandle, title: &str, body: &str) {
	if let Err(error) = app.notification().builder().title(title).body(body).show() {
		eprintln!("un-avatar-supervisor: failed to show native notification: {error}");
	}
}

fn permission_state_label(state: PermissionState) -> &'static str {
	match state {
		PermissionState::Granted => "granted",
		PermissionState::Denied => "denied",
		PermissionState::Prompt => "prompt",
		PermissionState::PromptWithRationale => "prompt_with_rationale",
	}
}

fn push_notification(state: &mut SupervisorState, level: NotificationLevel, title: String, body: String) {
	state.next_notification_id = state.next_notification_id.saturating_add(1);
	state.notifications.push(AppNotification {
		id: state.next_notification_id,
		level,
		title,
		body,
		created_at_secs: current_unix_secs(),
	});
	let overflow = state.notifications.len().saturating_sub(40);
	if overflow > 0 {
		state.notifications.drain(0..overflow);
	}
}

fn current_unix_secs() -> u64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|duration| duration.as_secs())
		.unwrap_or_default()
}

fn spawn_stderr_tail(stderr: Option<ChildStderr>) -> Arc<Mutex<Vec<String>>> {
	let tail = Arc::new(Mutex::new(Vec::new()));
	let Some(stderr) = stderr else {
		return tail;
	};
	let thread_tail = Arc::clone(&tail);
	std::thread::spawn(move || {
		for line in BufReader::new(stderr).lines().map_while(Result::ok) {
			let Ok(mut tail) = thread_tail.lock() else {
				return;
			};
			tail.push(line);
			let overflow = tail.len().saturating_sub(MAX_RENDERER_LOG_LINES);
			if overflow > 0 {
				tail.drain(0..overflow);
			}
		}
	});
	tail
}

fn refresh_renderer_stderr(renderer: &mut ManagedRenderer) {
	let Ok(tail) = renderer.stderr_tail.lock() else {
		return;
	};
	renderer.info.stderr_tail = tail.clone();
	renderer.info.last_stderr = tail.last().cloned();
}

fn read_avatar_setting(path: &Path, storage: ProfileStorage) -> Result<AvatarSetting, String> {
	let text = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
	let manifest: AvatarManifestSummary = toml::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))?;
	let background_color = manifest_background_color(&manifest);
	let profile = manifest.profile.unwrap_or_default();
	let file_stem = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("avatar");
	let motion = motion_settings(manifest.motion.unwrap_or_default(), manifest.vmc_address, manifest.vmc_port);
	let audio_link = audio_link_settings(manifest.audio_link.unwrap_or_default());
	let output = output_settings(manifest.output, manifest.spout);
	let avatar_path_for_spring_bones = manifest.avatar_path.clone();
	let window = window_settings(
		manifest.window.unwrap_or_default(),
		manifest.icon_path,
		manifest.transparent,
		manifest.input_passthrough,
		manifest.decorations,
	);
	let render_quality = render_quality_settings(manifest.render_quality.unwrap_or_default(), manifest.aa);
	let environment = manifest.environment.unwrap_or_default();
	let color_adjustment = color_adjustment_settings(environment.color.unwrap_or_default());
	let lighting = lighting_settings(environment.lighting.unwrap_or_default());
	let debug = debug_settings(manifest.debug.as_ref());
	let physics = physics_settings(manifest.physics.as_ref(), avatar_path_for_spring_bones.as_ref(), path);
	let effects = manifest.effects.unwrap_or_default();
	let avatar_effects = avatar_effect_settings(effects.avatar);
	let post_effects = post_effect_settings(effects.post);
	let camera = camera_settings(manifest.camera.as_ref());
	Ok(AvatarSetting {
		id: profile.id.unwrap_or_else(|| path.display().to_string()),
		name: profile.display_name.or(manifest.title).unwrap_or_else(|| file_stem.to_string()),
		created_at: normalize_created_at(profile.created_at.as_deref().unwrap_or_default(), path),
		sort_order: profile.sort_order.unwrap_or(u32::MAX),
		storage,
		manifest_path: path.display().to_string(),
		avatar_path: manifest
			.avatar_path
			.map(|avatar_path| avatar_path_for_manifest_value(&avatar_path.display().to_string(), path)),
		wardrobe_set: manifest.wardrobe_set.and_then(|set| {
			let set = set.trim().to_string();
			(!set.is_empty()).then_some(set)
		}),
		vmc_address: motion.vmc_address,
		vmc_port: motion.vmc_port,
		motion_vmc_enabled: motion.motion_vmc_enabled,
		motion_unmotion_enabled: motion.motion_unmotion_enabled,
		unmotion_zenoh_key: motion.unmotion_zenoh_key,
		audio_link_source: audio_link.source,
		audio_link_input_device_id: audio_link.input_device_id,
		audio_link_input_device_name_hint: audio_link.input_device_name_hint,
		look_at_enabled: motion.look_at_enabled,
		look_at_clamp_deg: motion.look_at_clamp_deg,
		primary_motion_source: motion.primary_motion_source,
		spring_bones: physics.dynamics_enabled.unwrap_or(true),
		dynamics_enable_all_on_launch: physics.dynamics_enable_all_on_launch,
		contact_parameter_emission: physics.contact_parameter_emission,
		spring_bone_physics_configured: physics.spring_bone_physics_configured,
		spring_bone_simulation_hz: physics.spring_bone_simulation_hz,
		spring_bone_substeps: physics.spring_bone_substeps,
		spring_bone_category_overrides: physics.spring_bone_category_overrides,
		apply_vmc_root_translation: motion.apply_vmc_root_translation,
		spout_enabled: output.spout_enabled,
		spout_name: output.spout_name,
		spout_width: output.spout_width,
		spout_height: output.spout_height,
		aa: render_quality.aa,
		texture_resolution_limit: render_quality.texture_resolution_limit,
		texture_compression: render_quality.texture_compression,
		mipmap_filter: render_quality.mipmap_filter,
		render_backend: render_quality.render_backend,
		block_compression_encoder: render_quality.block_compression_encoder,
		block_compression_cpu_threads: render_quality.block_compression_cpu_threads,
		texture_compression_advanced: render_quality.texture_compression_advanced,
		processed_texture_cache: render_quality.processed_texture_cache,
		skin_tone_matching: render_quality.skin_tone_matching,
		background_color,
		transparent: window.transparent,
		input_passthrough: window.input_passthrough,
		decorations: window.decorations,
		always_on_top: window.always_on_top,
		minimized: window.minimized,
		show_axes: debug.show_axes,
		show_bone_colliders: debug.show_bone_colliders,
		bone_colliders_enabled: physics.bone_colliders_enabled,
		bone_collider_head: physics.bone_collider_head,
		bone_collider_neck_chest: physics.bone_collider_neck_chest,
		bone_collider_torso: physics.bone_collider_torso,
		bone_collider_upper_arms: physics.bone_collider_upper_arms,
		bone_collider_lower_arms: physics.bone_collider_lower_arms,
		bone_collider_hands: physics.bone_collider_hands,
		debug_disable_mtoon_outlines: debug.disable_mtoon_outlines,
		debug_disable_rim_lighting: debug.disable_rim_lighting,
		debug_force_shading_shift_zero: debug.force_shading_shift_zero,
		debug_disable_matcap: debug.disable_matcap,
		debug_disable_emissive: debug.disable_emissive,
		debug_disable_shade_color: debug.disable_shade_color,
		debug_disable_normal_map: debug.disable_normal_map,
		debug_base_texture_only: debug.base_texture_only,
		outline_policy: avatar_effects.outline_policy,
		outline_type: avatar_effects.outline_type,
		outline_width: avatar_effects.outline_width,
		outline_color: avatar_effects.outline_color,
		outline_lighting_mix: avatar_effects.outline_lighting_mix,
		outline_roundness: avatar_effects.outline_roundness,
		rim_policy: avatar_effects.rim_policy,
		rim_color: avatar_effects.rim_color,
		rim_intensity: avatar_effects.rim_intensity,
		rim_lighting_mix: avatar_effects.rim_lighting_mix,
		rim_fresnel_power: avatar_effects.rim_fresnel_power,
		rim_lift: avatar_effects.rim_lift,
		matcap_scale: avatar_effects.matcap_scale,
		specular_enabled: avatar_effects.specular_enabled,
		specular_intensity: avatar_effects.specular_intensity,
		specular_power: avatar_effects.specular_power,
		ambient_occlusion_strength: avatar_effects.ambient_occlusion_strength,
		lighting_environment_enabled: lighting.lighting_environment_enabled,
		lighting_environment_color: lighting.lighting_environment_color,
		lighting_environment_intensity: lighting.lighting_environment_intensity,
		lighting_directional_enabled: lighting.lighting_directional_enabled,
		lighting_directional_color: lighting.lighting_directional_color,
		lighting_directional_intensity: lighting.lighting_directional_intensity,
		lighting_directional_azimuth_deg: lighting.lighting_directional_azimuth_deg,
		lighting_directional_elevation_deg: lighting.lighting_directional_elevation_deg,
		lighting_directional_follow_camera_yaw: lighting.lighting_directional_follow_camera_yaw,
		lighting_directional_follow_camera_pitch: lighting.lighting_directional_follow_camera_pitch,
		color_exposure: color_adjustment.color_exposure,
		color_contrast: color_adjustment.color_contrast,
		color_saturation: color_adjustment.color_saturation,
		color_look: color_adjustment.color_look,
		color_look_intensity: color_adjustment.color_look_intensity,
		color_temperature: color_adjustment.color_temperature,
		color_tint: color_adjustment.color_tint,
		bloom_enabled: post_effects.bloom_enabled,
		bloom_strength: post_effects.bloom_strength,
		bloom_threshold: post_effects.bloom_threshold,
		bloom_radius: post_effects.bloom_radius,
		bloom_quality: post_effects.bloom_quality,
		ssao_enabled: post_effects.ssao_enabled,
		ssao_strength: post_effects.ssao_strength,
		ssao_radius: post_effects.ssao_radius,
		ssao_bias: post_effects.ssao_bias,
		ssao_range: post_effects.ssao_range,
		contact_shadow_enabled: avatar_effects.contact_shadow_enabled,
		contact_shadow_strength: avatar_effects.contact_shadow_strength,
		contact_shadow_radius: avatar_effects.contact_shadow_radius,
		contact_shadow_softness: avatar_effects.contact_shadow_softness,
		contact_shadow_height: avatar_effects.contact_shadow_height,
		camera_locked: camera.locked,
		window_x: window.x,
		window_y: window.y,
		camera_target: camera.target,
		camera_longitude_deg: camera.longitude_deg,
		camera_latitude_deg: camera.latitude_deg,
		camera_radius: camera.radius,
		camera_diagonal_fov_deg: camera.diagonal_fov_deg,
		window_width: window.width,
		window_height: window.height,
		icon_path: window.icon_path,
		allow_multiple_renderers: profile.allow_multiple_renderers.unwrap_or(false),
		notes: profile.notes,
		group: profile.group.unwrap_or_default().trim().to_string(),
	})
}

fn resolve_avatar_setting(setting_id: &str) -> Result<AvatarSetting, String> {
	for setting in list_avatar_settings()? {
		if setting.id == setting_id || setting.manifest_path == setting_id {
			return Ok(setting);
		}
	}
	Err(format!("avatar setting not found: {setting_id}"))
}

fn resolve_avatar_setting_direct(setting_id: &str) -> Result<AvatarSetting, String> {
	let mut matched: Option<(PathBuf, ProfileStorage)> = None;
	for (storage, dir) in profile_dirs() {
		if !dir.is_dir() {
			continue;
		}
		let entries = fs::read_dir(&dir).map_err(|e| format!("read {}: {e}", dir.display()))?;
		for entry in entries.flatten() {
			let path = entry.path();
			if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
				continue;
			}
			let path_text = path.display().to_string();
			if manifest_path_key(&path_text) == manifest_path_key(setting_id) {
				return read_avatar_setting(&path, storage);
			}
			let Ok(text) = fs::read_to_string(&path) else {
				continue;
			};
			let Ok(manifest) = parse_manifest_value(&text, &path) else {
				continue;
			};
			let id = manifest
				.get("profile")
				.and_then(|profile| profile.get("id"))
				.and_then(toml::Value::as_str);
			if id == Some(setting_id) {
				matched = Some((path, storage));
			}
		}
	}
	if let Some((path, storage)) = matched {
		return read_avatar_setting(&path, storage);
	}
	Err(format!("avatar setting not found: {setting_id}"))
}

fn renderer_command(manifest_path: &Path, runtime_bus_key: &str, close_hotkey: &str, icon_path: Option<&Path>) -> Result<Command, String> {
	let repo = repo_root();
	let exe = renderer_executable_path();
	if exe.is_file() {
		let mut command = Command::new(exe);
		command
			.arg("--manifest")
			.arg(manifest_path)
			.arg("--runtime-bus-key")
			.arg(runtime_bus_key)
			.arg("--close-hotkey")
			.arg(close_hotkey);
		if let Some(icon_path) = icon_path {
			command.arg("--icon").arg(icon_path);
		}
		prepend_spout2_runtime_path(&mut command);
		return Ok(command);
	}
	let mut command = Command::new("cargo");
	command
		.current_dir(repo)
		.args(["run", "-q", "-p", "un-avatar-render-wgpu", "--bin", "un-avatar-renderer", "--"])
		.arg("--manifest")
		.arg(manifest_path)
		.arg("--runtime-bus-key")
		.arg(runtime_bus_key)
		.arg("--close-hotkey")
		.arg(close_hotkey);
	if let Some(icon_path) = icon_path {
		command.arg("--icon").arg(icon_path);
	}
	prepend_spout2_runtime_path(&mut command);
	Ok(command)
}

fn renderer_prewarm_command(manifest_path: &Path) -> Result<Command, String> {
	let repo = repo_root();
	let exe = renderer_executable_path();
	if exe.is_file() {
		let mut command = Command::new(exe);
		command.arg("--manifest").arg(manifest_path).arg("--prewarm-shaders");
		prepend_spout2_runtime_path(&mut command);
		return Ok(command);
	}
	let mut command = Command::new("cargo");
	command
		.current_dir(repo)
		.args(["run", "-q", "-p", "un-avatar-render-wgpu", "--bin", "un-avatar-renderer", "--"])
		.arg("--manifest")
		.arg(manifest_path)
		.arg("--prewarm-shaders");
	prepend_spout2_runtime_path(&mut command);
	Ok(command)
}

fn resolve_renderer_window_icon_path(setting: &AvatarSetting) -> Option<PathBuf> {
	setting
		.icon_path
		.as_deref()
		.and_then(|path| resolve_manifest_asset_path(path, Path::new(&setting.manifest_path)))
		.or_else(|| {
			let fallback = repo_root().join("assets").join("brand").join("un-avatar-artwork-renderer.png");
			fallback.is_file().then_some(fallback)
		})
}

fn resolve_manifest_asset_path(path: &str, manifest_path: &Path) -> Option<PathBuf> {
	let trimmed = path.trim();
	if trimmed.is_empty() {
		return None;
	}
	let path = PathBuf::from(trimmed);
	if path.is_absolute() {
		return path.is_file().then_some(path);
	}
	let manifest_relative = manifest_path.parent().map(|parent| parent.join(&path));
	if let Some(candidate) = manifest_relative.filter(|candidate| candidate.is_file()) {
		return Some(candidate);
	}
	let repo_relative = repo_root().join(&path);
	repo_relative.is_file().then_some(repo_relative)
}

fn renderer_executable_path() -> PathBuf {
	renderer_executable_candidates()
		.into_iter()
		.find(|path| path.is_file())
		.unwrap_or_else(|| repo_root().join("target").join("debug").join(exe_name("un-avatar-renderer")))
}

fn renderer_executable_candidates() -> Vec<PathBuf> {
	let current = std::env::current_exe().ok();
	let mut candidates = Vec::new();
	if let Some(dir) = current.as_ref().and_then(|path| path.parent()) {
		candidates.push(dir.join(exe_name("un-avatar-renderer")));
		candidates.push(dir.join("runtimes").join(exe_name("un-avatar-renderer")));
	}
	let repo = repo_root();
	candidates.push(repo.join("target").join("release").join(exe_name("un-avatar-renderer")));
	candidates.push(repo.join("target").join("debug").join(exe_name("un-avatar-renderer")));
	candidates
}

fn prepend_spout2_runtime_path(command: &mut Command) {
	let Some(runtime_dir) = spout2_runtime_dir() else {
		return;
	};
	let old_path = env::var_os("PATH").unwrap_or_default();
	let mut paths = vec![runtime_dir];
	paths.extend(env::split_paths(&old_path));
	if let Ok(path_value) = env::join_paths(paths) {
		command.env("PATH", path_value);
	}
}

fn spout2_runtime_dir() -> Option<PathBuf> {
	spout2_runtime_candidates().into_iter().find(|dir| dir.join("Spout.dll").is_file())
}

fn spout2_runtime_candidates() -> Vec<PathBuf> {
	let mut candidates = Vec::new();
	if let Ok(current) = std::env::current_exe() {
		if let Some(dir) = current.parent() {
			candidates.push(dir.to_path_buf());
			candidates.push(dir.join("runtimes").join("spout2"));
			candidates.push(dir.join("spout2"));
		}
	}
	candidates.push(repo_root().join("target").join("package").join("un-avatar"));
	candidates
}

fn profiles_dir() -> PathBuf {
	repo_root().join("profiles")
}

fn user_profiles_dir() -> PathBuf {
	app_config_dir().join("profiles")
}

fn profile_dirs() -> [(ProfileStorage, PathBuf); 2] {
	[(ProfileStorage::Seed, profiles_dir()), (ProfileStorage::User, user_profiles_dir())]
}

/// Phase E settings policy (UN Motion と統一): UN Avatar も "Seed 廃止 +
/// bundled templates + 初回コピー" 方式へ寄せる第一歩。
///
/// 起動時に `user_profiles_dir()` が空 (= `*.toml` 0 件) の場合に限り、
/// repo 同梱の `profiles_dir()` (= `<workspace_root>/profiles/`) を **テンプレート**
/// として user dir へ全部コピーする。これにより:
///
/// * 新規ユーザー: user dir に最初から個別 `*.toml` が並ぶ。`list_avatar_settings`
///   は両 dir を walk して `BTreeMap` で id 衝突を後勝ち (= User 優先) に解消する
///   ので、Seed dir のオリジナルは事実上見えなくなる。すべての profile が
///   `ProfileStorage::User` として扱われ、編集時の lazy-copy も不要になる。
/// * 既存ユーザー: user dir は既に空ではないので no-op。`hidden_seed_profiles.json`
///   + Seed/User 二重ソースの旧挙動が完全に維持される (user 決定 2 の主旨)。
///
/// 失敗してもアプリ起動を止めない (warn ログのみ) — `list_avatar_settings` は
/// Seed dir もまだ読むので、コピー失敗時はそのまま Seed が見える形に degrade する。
fn ensure_user_profiles_seeded() {
	let user_dir = user_profiles_dir();
	if has_any_toml_file(&user_dir) {
		return;
	}
	let template_dir = profiles_dir();
	if !template_dir.is_dir() {
		return;
	}
	if let Err(error) = fs::create_dir_all(&user_dir) {
		eprintln!(
			"un-avatar-supervisor: failed to create user profiles dir for seeding ({}): {error} (continuing without initial copy)",
			user_dir.display(),
		);
		return;
	}
	let entries = match fs::read_dir(&template_dir) {
		Ok(entries) => entries,
		Err(error) => {
			eprintln!(
				"un-avatar-supervisor: failed to read bundled profile templates ({}): {error}",
				template_dir.display(),
			);
			return;
		}
	};
	let mut copied = 0_usize;
	for entry in entries.flatten() {
		let path = entry.path();
		if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
			continue;
		}
		let Some(file_name) = path.file_name() else { continue };
		let dest = user_dir.join(file_name);
		if dest.exists() {
			continue;
		}
		match fs::copy(&path, &dest) {
			Ok(_) => copied += 1,
			Err(error) => eprintln!(
				"un-avatar-supervisor: failed to copy bundled profile template {} -> {}: {error}; that profile will continue to be served from the repo seed dir",
				path.display(),
				dest.display(),
			),
		}
	}
	if copied > 0 {
		eprintln!(
			"un-avatar-supervisor: seeded user profile dir from bundled templates: copied {copied} file(s) from {} to {} (first run); Seed profiles are now visible as user-editable copies",
			template_dir.display(),
			user_dir.display(),
		);
	}
}

/// `dir/*.toml` が 1 件以上あるかどうかを軽量にチェックする。`dir` 自体が
/// 無い / 読めない場合は `false`。`ensure_user_profiles_seeded` の早期 return 用。
fn has_any_toml_file(dir: &Path) -> bool {
	let Ok(entries) = fs::read_dir(dir) else {
		return false;
	};
	for entry in entries.flatten() {
		let path = entry.path();
		if path.extension().and_then(|ext| ext.to_str()) == Some("toml") {
			return true;
		}
	}
	false
}

fn app_config_dir() -> PathBuf {
	if let Some(path) = env::var_os("APPDATA") {
		return PathBuf::from(path).join("UN Avatar");
	}
	if let Some(path) = env::var_os("XDG_CONFIG_HOME") {
		return PathBuf::from(path).join("un-avatar");
	}
	if let Some(path) = env::var_os("HOME") {
		return PathBuf::from(path).join(".config").join("un-avatar");
	}
	repo_root().join("target").join("tmp").join("un-avatar-config")
}

fn app_settings_path() -> PathBuf {
	app_config_dir().join("settings.toml")
}

fn hidden_seed_profiles_path() -> PathBuf {
	app_config_dir().join("hidden_seed_profiles.json")
}

fn read_hidden_seed_profile_ids() -> Vec<String> {
	fs::read_to_string(hidden_seed_profiles_path())
		.ok()
		.and_then(|text| serde_json::from_str::<Vec<String>>(&text).ok())
		.unwrap_or_default()
}

fn hide_seed_avatar_setting(setting_id: &str) -> Result<(), String> {
	let mut ids = read_hidden_seed_profile_ids();
	if !ids.iter().any(|id| id == setting_id) {
		ids.push(setting_id.to_string());
		ids.sort();
	}
	let path = hidden_seed_profiles_path();
	if let Some(dir) = path.parent() {
		fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
	}
	let text = serde_json::to_string_pretty(&ids).map_err(|e| format!("serialize hidden seed profiles: {e}"))?;
	fs::write(&path, text).map_err(|e| format!("write {}: {e}", path.display()))
}

fn seed_avatar_setting_exists(setting_id: &str) -> bool {
	let dir = profiles_dir();
	let Ok(entries) = fs::read_dir(dir) else {
		return false;
	};
	for entry in entries.flatten() {
		let path = entry.path();
		if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
			continue;
		}
		if read_avatar_setting(&path, ProfileStorage::Seed)
			.map(|setting| setting.id == setting_id)
			.unwrap_or(false)
		{
			return true;
		}
	}
	false
}

fn editable_avatar_setting_path(setting: &AvatarSetting) -> Result<PathBuf, String> {
	let source_path = PathBuf::from(&setting.manifest_path);
	if setting.storage == ProfileStorage::User {
		return Ok(source_path);
	}
	let file_name = source_path.file_name().and_then(|name| name.to_str()).unwrap_or("avatar.toml");
	let target_path = user_profiles_dir().join(file_name);
	if target_path.exists() {
		return Ok(target_path);
	}
	if let Some(dir) = target_path.parent() {
		fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
	}
	fs::copy(&source_path, &target_path).map_err(|e| format!("copy {} to {}: {e}", source_path.display(), target_path.display()))?;
	Ok(target_path)
}

fn ensure_avatar_profile_metadata(manifest: &mut toml::Value, path: &Path, sort_order: Option<u32>) -> Result<(), String> {
	let table = manifest.as_table_mut().ok_or_else(|| "manifest root must be a table".to_string())?;
	let profile = table
		.entry("profile".to_string())
		.or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
		.as_table_mut()
		.ok_or_else(|| "profile must be a table".to_string())?;
	let created_at = profile
		.get("created_at")
		.and_then(toml::Value::as_str)
		.filter(|value| is_compact_timestamp(value))
		.map(ToString::to_string)
		.unwrap_or_else(|| normalize_created_at("", path));
	profile.insert("created_at".to_string(), toml::Value::String(created_at));
	let order = sort_order
		.or_else(|| {
			profile
				.get("sort_order")
				.and_then(toml::Value::as_integer)
				.and_then(|value| u32::try_from(value).ok())
		})
		.unwrap_or(u32::MAX);
	profile.insert("sort_order".to_string(), toml::Value::Integer(order as i64));
	Ok(())
}

fn rename_avatar_setting_file_if_needed(path: &Path, setting: &AvatarSetting) -> Result<PathBuf, String> {
	if setting.storage != ProfileStorage::User && !path.starts_with(user_profiles_dir()) {
		return Ok(path.to_path_buf());
	}
	let target = unique_user_profile_path_except(&profile_file_stem(&setting.created_at, &setting.name), path);
	if target == path {
		return Ok(path.to_path_buf());
	}
	if let Some(dir) = target.parent() {
		fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
	}
	fs::rename(path, &target).map_err(|e| format!("rename {} to {}: {e}", path.display(), target.display()))?;
	Ok(target)
}

fn configure_hidden_child(command: &mut Command) {
	#[cfg(windows)]
	{
		command.creation_flags(CREATE_NO_WINDOW);
	}
}

fn unique_user_profile_path(stem: &str) -> PathBuf {
	unique_user_profile_path_except(stem, Path::new(""))
}

fn unique_user_profile_path_except(stem: &str, current: &Path) -> PathBuf {
	let dir = user_profiles_dir();
	let mut index = 1;
	loop {
		let suffix = if index == 1 { String::new() } else { format!("-{index}") };
		let path = dir.join(format!("{stem}{suffix}.toml"));
		if path == current || !path.exists() {
			return path;
		}
		index += 1;
	}
}

fn profile_file_stem(created_at: &str, name: &str) -> String {
	format!(
		"{}-{}",
		normalize_created_at(created_at, Path::new("")),
		sanitize_profile_file_label(name)
	)
}

fn manifest_profile_name(manifest: &toml::Value) -> Option<String> {
	manifest
		.get("profile")
		.and_then(|profile| profile.get("display_name"))
		.and_then(toml::Value::as_str)
		.or_else(|| manifest.get("title").and_then(toml::Value::as_str))
		.map(ToString::to_string)
}

fn next_avatar_sort_order() -> Result<u32, String> {
	Ok(list_avatar_settings()?
		.iter()
		.map(|setting| setting.sort_order)
		.filter(|value| *value != u32::MAX)
		.max()
		.unwrap_or(0)
		.saturating_add(1000))
}

fn normalize_created_at(value: &str, path: &Path) -> String {
	let trimmed = value.trim();
	if is_compact_timestamp(trimmed) {
		return trimmed.to_string();
	}
	if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
		let prefix = stem.split('-').next().unwrap_or_default();
		if is_compact_timestamp(prefix) {
			return prefix.to_string();
		}
	}
	current_timestamp_compact()
}

fn is_compact_timestamp(value: &str) -> bool {
	value.len() == 16
		&& value.as_bytes().get(8) == Some(&b'T')
		&& value.as_bytes().get(15) == Some(&b'Z')
		&& value
			.chars()
			.enumerate()
			.all(|(index, ch)| matches!(index, 8 | 15) || ch.is_ascii_digit())
}

fn sanitize_profile_file_label(name: &str) -> String {
	let mut out = String::new();
	let mut prev_dash = false;
	for lower in name.trim().chars().flat_map(char::to_lowercase) {
		let invalid = matches!(lower, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || lower.is_control();
		let ch = if invalid || lower.is_whitespace() { '-' } else { lower };
		if ch == '-' {
			if !prev_dash {
				out.push('-');
			}
			prev_dash = true;
		} else {
			out.push(ch);
			prev_dash = false;
		}
		if out.len() >= 96 {
			break;
		}
	}
	let trimmed = out.trim_matches(|ch| matches!(ch, '-' | '.' | ' ')).to_string();
	let label = if trimmed.is_empty() { "profile".to_string() } else { trimmed };
	let upper = label.to_ascii_uppercase();
	let reserved = matches!(
		upper.as_str(),
		"CON"
			| "PRN" | "AUX"
			| "NUL" | "COM1"
			| "COM2" | "COM3"
			| "COM4" | "COM5"
			| "COM6" | "COM7"
			| "COM8" | "COM9"
			| "LPT1" | "LPT2"
			| "LPT3" | "LPT4"
			| "LPT5" | "LPT6"
			| "LPT7" | "LPT8"
			| "LPT9"
	);
	if reserved {
		format!("{label}-profile")
	} else {
		label
	}
}

fn current_timestamp_compact() -> String {
	let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
	let (year, month, day, hour, minute, second) = unix_seconds_to_utc(secs);
	format!("{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}Z")
}

fn unix_seconds_to_utc(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
	let days = secs.div_euclid(86_400);
	let rem = secs.rem_euclid(86_400);
	let (year, month, day) = civil_from_days(days);
	(
		year,
		month,
		day,
		(rem / 3_600) as u32,
		((rem % 3_600) / 60) as u32,
		(rem % 60) as u32,
	)
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
	let z = days + 719_468;
	let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
	let doe = z - era * 146_097;
	let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
	let y = yoe + era * 400;
	let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
	let mp = (5 * doy + 2) / 153;
	let d = doy - (153 * mp + 2) / 5 + 1;
	let m = mp + if mp < 10 { 3 } else { -9 };
	let year = y + if m <= 2 { 1 } else { 0 };
	(year as i32, m as u32, d as u32)
}

fn unique_profile_name(base: &str) -> Result<String, String> {
	let existing = list_avatar_settings()?;
	if !existing.iter().any(|setting| setting.name == base) {
		return Ok(base.to_string());
	}
	let mut index = 2;
	loop {
		let name = format!("{base} {index}");
		if !existing.iter().any(|setting| setting.name == name) {
			return Ok(name);
		}
		index += 1;
	}
}

fn unique_profile_id_for_name(name: &str) -> Result<String, String> {
	let base = unique_profile_id(name);
	let existing = list_avatar_settings()?;
	if !existing.iter().any(|setting| setting.id == base) {
		return Ok(base);
	}
	let mut index = 2;
	loop {
		let id = format!("{base}-{index}");
		if !existing.iter().any(|setting| setting.id == id) {
			return Ok(id);
		}
		index += 1;
	}
}

fn unique_profile_id(name: &str) -> String {
	let mut slug = String::new();
	for ch in name.chars() {
		if ch.is_ascii_alphanumeric() {
			slug.push(ch.to_ascii_lowercase());
		} else if !slug.ends_with('-') {
			slug.push('-');
		}
	}
	let slug = slug.trim_matches('-');
	if slug.is_empty() {
		"avatar-copy".to_string()
	} else {
		slug.to_string()
	}
}

fn json_bool(value: &serde_json::Value, field: &str) -> Result<bool, String> {
	value.as_bool().ok_or_else(|| format!("{field} must be a boolean"))
}

fn json_string(value: &serde_json::Value, field: &str) -> Result<String, String> {
	value
		.as_str()
		.map(str::to_string)
		.ok_or_else(|| format!("{field} must be a string"))
}

fn json_socket_addr_string(value: &serde_json::Value, field: &str) -> Result<String, String> {
	let address = json_string(value, field)?.trim().to_string();
	address
		.parse::<SocketAddr>()
		.map(|_| address)
		.map_err(|_| format!("{field} must be an IP:port address"))
}

fn json_lowercase_choice(value: &serde_json::Value, field: &str, choices: &[&str]) -> Result<String, String> {
	let choice = json_string(value, field)?.trim().to_ascii_lowercase();
	if choices.contains(&choice.as_str()) {
		Ok(choice)
	} else {
		Err(format!("{field} must be one of {}", choices.join(", ")))
	}
}

fn json_lowercase_slug_choice(value: &serde_json::Value, field: &str, choices: &[&str]) -> Result<String, String> {
	let choice = json_string(value, field)?.trim().to_ascii_lowercase().replace('-', "_");
	if choices.contains(&choice.as_str()) {
		Ok(choice)
	} else {
		Err(format!("{field} must be one of {}", choices.join(", ")))
	}
}

fn json_aa_mode(value: &serde_json::Value, field: &str) -> Result<String, String> {
	json_lowercase_choice(value, field, &["off", "fxaa", "smaa", "msaa"])
}

fn json_texture_resolution_limit(value: &serde_json::Value, field: &str) -> Result<String, String> {
	json_lowercase_choice(value, field, &["off", "auto", "8k", "4k", "2k", "1k"])
}

fn json_texture_compression_preference(value: &serde_json::Value, field: &str) -> Result<String, String> {
	let preference = json_string(value, field)?;
	match preference.as_str() {
		"source" | "auto" | "high_quality" | "small" | "gpu_native" => Ok(preference),
		_ => Err(format!("{field} must be one of source, auto, high_quality, small, gpu_native")),
	}
}

fn json_texture_compression_mode(value: &serde_json::Value, field: &str) -> Result<String, String> {
	let mode = json_string(value, field)?;
	match mode.as_str() {
		"auto" | "advanced" => Ok("balanced".to_string()),
		"source" | "balanced" | "memory" | "compat" => Ok(mode),
		_ => Err(format!("{field} must be one of source, balanced, memory, compat")),
	}
}

fn json_mipmap_filter(value: &serde_json::Value, field: &str) -> Result<String, String> {
	json_lowercase_slug_choice(
		value,
		field,
		&["box2x2", "bilinear", "bicubic", "catmull_rom", "lanczos3", "mitchell"],
	)
}

fn json_render_backend(value: &serde_json::Value, field: &str) -> Result<String, String> {
	json_lowercase_choice(value, field, &["vulkan", "dx12", "auto"])
}

fn json_block_compression_encoder(value: &serde_json::Value, field: &str) -> Result<String, String> {
	json_lowercase_choice(value, field, &["gpu", "cpu"])
}

fn normalize_outline_policy(value: &str) -> Option<String> {
	match value.trim().to_ascii_lowercase().as_str() {
		"authored" => Some("authored".to_string()),
		"off" | "none" | "disabled" => Some("off".to_string()),
		"override" | "custom" => Some("override".to_string()),
		_ => None,
	}
}

fn normalize_outline_type(value: &str) -> Option<String> {
	match value.trim().to_ascii_lowercase().as_str() {
		"mtoon" | "geometry" => Some("mtoon".to_string()),
		"ink" => Some("ink".to_string()),
		"brush" | "hake" | "fude" => Some("brush".to_string()),
		"double" | "double_outline" => Some("double".to_string()),
		_ => None,
	}
}

fn normalize_rim_policy(value: &str) -> Option<String> {
	match value.trim().to_ascii_lowercase().as_str() {
		"authored" => Some("authored".to_string()),
		"off" | "none" | "disabled" => Some("off".to_string()),
		"override" | "custom" => Some("override".to_string()),
		_ => None,
	}
}

fn normalize_color_look(value: &str) -> Option<String> {
	match value.trim().to_ascii_lowercase().as_str() {
		"neutral" | "off" | "none" => Some("neutral".to_string()),
		"warm" => Some("warm".to_string()),
		"cool" => Some("cool".to_string()),
		"film" | "cinematic" => Some("film".to_string()),
		"soft" => Some("soft".to_string()),
		"pop" | "vivid" => Some("pop".to_string()),
		_ => None,
	}
}

fn validate_color_look(value: &str) -> Result<String, String> {
	normalize_color_look(value).ok_or_else(|| "color look must be one of neutral, warm, cool, film, soft, pop".to_string())
}

fn normalize_light_reference(value: &str) -> Option<&'static str> {
	match value.trim().to_ascii_lowercase().as_str() {
		"world" => Some("world"),
		"camera" => Some("camera"),
		"model" => Some("model"),
		_ => None,
	}
}

fn normalize_spring_bone_solver(value: &str) -> Option<String> {
	match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
		"verlet" | "balanced" | "compat_univrm" | "compat_euler" | "compat" | "univrm" | "euler" => Some("verlet".to_string()),
		"xpbd" | "quality" => Some("xpbd".to_string()),
		_ => None,
	}
}

fn validate_spring_bone_solver(value: &str) -> Result<String, String> {
	normalize_spring_bone_solver(value).ok_or_else(|| "spring bone solver must be one of verlet, xpbd".to_string())
}

fn normalize_spring_bone_category_id(value: &str) -> String {
	let normalized = value.trim().to_ascii_lowercase().replace([' ', '-'], "_");
	if normalized.is_empty() {
		"other".to_string()
	} else {
		normalized
	}
}

fn builtin_spring_bone_categories() -> &'static [(&'static str, &'static str)] {
	&[
		("hair", "Hair"),
		("ears", "Ears"),
		("tail", "Tail"),
		("cloth", "Cloth"),
		("accessory", "Accessory"),
		("other", "Other"),
	]
}

fn spring_bone_category_override_settings(
	physics: Option<&ManifestSpringBonePhysics>,
	avatar_path: Option<&PathBuf>,
	manifest_path: &Path,
) -> Vec<SpringBoneCategoryOverrideSetting> {
	let authored_by_category = avatar_path
		.and_then(|path| model_spring_bone_category_authored_params(path, manifest_path))
		.unwrap_or_default();
	let category_ids: Vec<String> = if authored_by_category.is_empty() {
		builtin_spring_bone_categories()
			.iter()
			.map(|(category, _)| (*category).to_string())
			.collect()
	} else {
		builtin_spring_bone_categories()
			.iter()
			.filter(|(category, _)| authored_by_category.contains_key(*category))
			.map(|(category, _)| (*category).to_string())
			.collect()
	};
	let mut settings: Vec<SpringBoneCategoryOverrideSetting> = category_ids
		.iter()
		.map(|category| {
			let authored = authored_by_category.get(category.as_str()).copied().unwrap_or_default();
			spring_bone_category_setting(category, category_label_from_id(category), authored)
		})
		.collect();
	let Some(overrides) = physics.and_then(|physics| physics.overrides.as_ref()) else {
		return settings;
	};
	for override_item in overrides {
		let category = normalize_spring_bone_category_id(&override_item.category);
		let index = settings.iter().position(|setting| setting.category == category).unwrap_or_else(|| {
			let authored = authored_by_category.get(category.as_str()).copied().unwrap_or_default();
			settings.push(spring_bone_category_setting(&category, category_label_from_id(&category), authored));
			settings.len() - 1
		});
		let setting = &mut settings[index];
		if let Some(solver) = override_item.solver.as_deref().and_then(normalize_spring_bone_solver) {
			setting.mode = if solver == "xpbd" {
				"override_xpbd".to_string()
			} else {
				"override_verlet".to_string()
			};
			setting.solver = solver;
		}
		if let Some(value) = override_item.damping_half_life_ms.filter(|value| value.is_finite()) {
			setting.damping_configured = true;
			setting.damping_half_life_ms = value.clamp(1.0, 10_000.0);
		}
		if let Some(value) = override_item.stiffness_hz.filter(|value| value.is_finite()) {
			setting.stiffness_configured = true;
			setting.stiffness_hz = value.clamp(0.0, 60.0);
		}
		if let Some(value) = override_item.xpbd_compliance.filter(|value| value.is_finite()) {
			setting.xpbd_compliance_configured = true;
			setting.xpbd_compliance = value.clamp(0.0, 10.0);
		}
		if let Some(value) = override_item.constraint_iterations {
			setting.constraint_iterations_configured = true;
			setting.constraint_iterations = value.clamp(1, 32);
		}
		if setting.mode == "authored"
			&& (setting.damping_configured
				|| setting.stiffness_configured
				|| setting.xpbd_compliance_configured
				|| setting.constraint_iterations_configured)
		{
			setting.mode = if setting.xpbd_compliance_configured || setting.constraint_iterations_configured {
				"override_xpbd".to_string()
			} else {
				"override_verlet".to_string()
			};
			setting.solver = if setting.mode == "override_xpbd" {
				"xpbd".to_string()
			} else {
				"verlet".to_string()
			};
		}
	}
	settings
}

fn spring_bone_category_setting(
	category: &str,
	name: String,
	authored: SpringBoneCategoryAuthoredParams,
) -> SpringBoneCategoryOverrideSetting {
	SpringBoneCategoryOverrideSetting {
		category: category.to_string(),
		name,
		mode: "authored".to_string(),
		spring_bone_count: authored.count,
		solver: "verlet".to_string(),
		damping_configured: false,
		damping_half_life_ms: 120.0,
		stiffness_configured: false,
		stiffness_hz: authored.stiffness_hz,
		xpbd_compliance_configured: false,
		xpbd_compliance: authored.xpbd_compliance,
		constraint_iterations_configured: false,
		constraint_iterations: 4,
		authored_stiffness_hz: authored.stiffness_hz,
		authored_xpbd_compliance: authored.xpbd_compliance,
	}
}

fn category_label_from_id(category: &str) -> String {
	let mut out = String::with_capacity(category.len());
	let mut upper_next = true;
	for ch in category.chars() {
		if ch == '_' {
			out.push(' ');
			upper_next = true;
		} else if upper_next {
			out.extend(ch.to_uppercase());
			upper_next = false;
		} else {
			out.push(ch);
		}
	}
	out
}

#[derive(Clone, Copy)]
struct SpringBoneCategoryAuthoredParams {
	count: usize,
	stiffness_hz: f32,
	xpbd_compliance: f32,
}

impl Default for SpringBoneCategoryAuthoredParams {
	fn default() -> Self {
		Self {
			count: 0,
			stiffness_hz: 1.0,
			xpbd_compliance: xpbd_compliance_from_vrm_stiffness(1.0),
		}
	}
}

fn model_spring_bone_category_authored_params(avatar_path: &Path, manifest_path: &Path) -> Option<SpringBoneAuthoredParamsByCategory> {
	let resolved = resolve_avatar_metadata_path(&avatar_path.display().to_string(), Some(&manifest_path.display().to_string()));
	let ext = resolved.extension().and_then(|ext| ext.to_str()).unwrap_or_default();
	if !matches!(ext.to_ascii_lowercase().as_str(), "vrm" | "glb") {
		return None;
	}
	let cache_key = spring_bone_authored_params_cache_key(&resolved)?;
	if let Some(cached) = SPRING_BONE_AUTHORED_PARAMS_CACHE
		.get_or_init(|| Mutex::new(BTreeMap::new()))
		.lock()
		.ok()
		.and_then(|cache| cache.get(&cache_key).cloned())
	{
		return Some(cached);
	}
	let bytes = fs::read(&resolved).ok()?;
	let root = un_avatar_io_vrm::gltf_root_json_from_bytes(&bytes).ok();
	let result = un_avatar_io_vrm::import_vrm_bytes(Some(&resolved), &bytes, root).ok()?;
	let document = result.document;
	let scene = document.scene.as_ref()?;
	let spring_bones = document.spring_bones.as_ref()?;
	let mut sums = BTreeMap::<String, (f32, usize)>::new();
	for group in &spring_bones.groups {
		let category = classify_spring_bone_group_for_profile(scene, group);
		let entry = sums.entry(category).or_insert((0.0, 0));
		entry.0 += group.stiffness.max(0.0);
		entry.1 += 1;
	}
	let mut out = BTreeMap::new();
	for (category, (sum, count)) in sums {
		if count == 0 {
			continue;
		}
		let stiffness = sum / count as f32;
		out.insert(
			category,
			SpringBoneCategoryAuthoredParams {
				count,
				stiffness_hz: stiffness,
				xpbd_compliance: xpbd_compliance_from_vrm_stiffness(stiffness),
			},
		);
	}
	if let Ok(mut cache) = SPRING_BONE_AUTHORED_PARAMS_CACHE.get_or_init(|| Mutex::new(BTreeMap::new())).lock() {
		cache.insert(cache_key, out.clone());
	}
	Some(out)
}

fn spring_bone_authored_params_cache_key(path: &Path) -> Option<String> {
	let metadata = fs::metadata(path).ok()?;
	let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
	Some(format!(
		"{}|{}|{}|{}",
		path.display(),
		metadata.len(),
		modified.as_secs(),
		modified.subsec_nanos()
	))
}

fn classify_spring_bone_group_for_profile(scene: &un_avatar_core::UnaSceneSnapshot, group: &un_avatar_core::UnaSpringBoneGroup) -> String {
	let explicit = normalize_spring_bone_category_id(&group.category);
	if explicit != "other" {
		return explicit;
	}
	let mut haystack = group.comment.to_ascii_lowercase();
	for &node_index in &group.bone_node_indices {
		if let Some(name) = scene.nodes.get(node_index).and_then(|node| node.name.as_deref()) {
			haystack.push(' ');
			haystack.push_str(&name.to_ascii_lowercase());
		}
	}
	for (category, _) in builtin_spring_bone_categories() {
		if *category == "other" {
			continue;
		}
		if haystack.contains(category) {
			return (*category).to_string();
		}
	}
	for (category, aliases) in [
		("hair", ["hair", "bang", "髪", "前髪", "横髪", "後ろ髪"].as_slice()),
		("ears", ["ear", "耳", "ミミ", "けもみみ"].as_slice()),
		("tail", ["tail", "尻尾", "しっぽ"].as_slice()),
		("cloth", ["cloth", "skirt", "sleeve", "cape", "布", "スカート", "袖"].as_slice()),
		(
			"accessory",
			["accessory", "ornament", "chain", "cord", "ribbon", "装飾", "飾り"].as_slice(),
		),
	] {
		if aliases.iter().any(|alias| haystack.contains(alias)) {
			return category.to_string();
		}
	}
	"other".to_string()
}

fn xpbd_compliance_from_vrm_stiffness(stiffness: f32) -> f32 {
	if !stiffness.is_finite() || stiffness <= f32::EPSILON {
		return 10.0;
	}
	let effective_hz = (stiffness * 10.0).clamp(0.1, 32.0);
	let omega = std::f32::consts::TAU * effective_hz;
	(1.0 / (omega * omega)).clamp(0.0, 10.0)
}

fn normalize_bloom_quality(value: &str) -> Option<String> {
	match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
		"compact" | "standard" | "fast" => Some("compact".to_string()),
		"high_quality" | "quality" | "hq" => Some("high_quality".to_string()),
		_ => None,
	}
}

fn collider_radius_mm_value(value: Option<f32>, default: f32) -> f32 {
	value.unwrap_or(default).clamp(0.0, 1000.0)
}

fn clamped_f32_or(value: Option<f32>, default: f32, min: f32, max: f32) -> f32 {
	value.unwrap_or(default).clamp(min, max)
}

fn color_adjustment_settings(color: ManifestEnvironmentColor) -> ColorAdjustmentSettings {
	let color_look = color
		.look
		.as_deref()
		.and_then(normalize_color_look)
		.unwrap_or_else(|| "neutral".to_string());
	let mut color_look_intensity = clamped_f32_or(color.intensity, 0.0, 0.0, 1.0);
	if color_look == "neutral" {
		color_look_intensity = 0.0;
	}
	ColorAdjustmentSettings {
		color_exposure: clamped_f32_or(color.exposure, 0.0, -4.0, 4.0),
		color_contrast: clamped_f32_or(color.contrast, 1.0, 0.0, 4.0),
		color_saturation: clamped_f32_or(color.saturation, 1.0, 0.0, 4.0),
		color_look,
		color_look_intensity,
		color_temperature: clamped_f32_or(color.temperature, 0.0, -1.0, 1.0),
		color_tint: clamped_f32_or(color.tint, 0.0, -1.0, 1.0),
	}
}

fn lighting_settings(lighting: ManifestLighting) -> LightingSettings {
	let environment = lighting.environment.unwrap_or_default();
	let directional = lighting.directional.unwrap_or_default();
	let legacy_follow_camera_yaw = directional
		.legacy_reference
		.as_deref()
		.and_then(normalize_light_reference)
		.map(|reference| reference == "camera");
	LightingSettings {
		lighting_environment_enabled: environment.enabled.unwrap_or(true),
		lighting_environment_color: environment.color.map(clamp_rgb).unwrap_or([1.0, 1.0, 1.0]),
		lighting_environment_intensity: clamped_f32_or(environment.intensity, 0.35, 0.0, 2.0),
		lighting_directional_enabled: directional.enabled.unwrap_or(true),
		lighting_directional_color: directional.color.map(clamp_rgb).unwrap_or([1.0, 1.0, 1.0]),
		lighting_directional_intensity: clamped_f32_or(directional.intensity, 1.0, 0.0, 4.0),
		lighting_directional_azimuth_deg: clamped_f32_or(directional.azimuth_deg, 0.0, -360.0, 360.0),
		lighting_directional_elevation_deg: clamped_f32_or(directional.elevation_deg, 33.84, -89.0, 89.0),
		lighting_directional_follow_camera_yaw: directional.follow_camera_yaw.or(legacy_follow_camera_yaw).unwrap_or(true),
		lighting_directional_follow_camera_pitch: directional.follow_camera_pitch.unwrap_or(false),
	}
}

fn debug_settings(debug: Option<&ManifestDebug>) -> DebugSettings {
	DebugSettings {
		show_axes: debug.and_then(|d| d.show_axes).unwrap_or(false),
		show_bone_colliders: debug.and_then(|d| d.show_bone_colliders).unwrap_or(false),
		disable_mtoon_outlines: debug.and_then(|d| d.disable_mtoon_outlines).unwrap_or(false),
		disable_rim_lighting: debug.and_then(|d| d.disable_rim_lighting).unwrap_or(false),
		force_shading_shift_zero: debug.and_then(|d| d.force_shading_shift_zero).unwrap_or(false),
		disable_matcap: debug.and_then(|d| d.disable_matcap).unwrap_or(false),
		disable_emissive: debug.and_then(|d| d.disable_emissive).unwrap_or(false),
		disable_shade_color: debug.and_then(|d| d.disable_shade_color).unwrap_or(false),
		disable_normal_map: debug.and_then(|d| d.disable_normal_map).unwrap_or(false),
		base_texture_only: debug.and_then(|d| d.base_texture_only).unwrap_or(false),
	}
}

fn physics_settings(physics: Option<&ManifestPhysics>, avatar_path: Option<&PathBuf>, manifest_path: &Path) -> PhysicsSettings {
	let bone_colliders = physics.and_then(|physics| physics.bone_colliders.as_ref());
	let contacts = physics.and_then(|physics| physics.contacts.as_ref());
	let dynamics = physics.and_then(|physics| physics.dynamics.as_ref());
	let spring_bone_physics = physics.and_then(|physics| physics.spring_bone.as_ref());
	let bone_collider_radius_mm = bone_colliders.and_then(|bone_colliders| bone_colliders.radius_mm.as_ref());
	PhysicsSettings {
		dynamics_enabled: dynamics.and_then(|dynamics| dynamics.enabled),
		dynamics_enable_all_on_launch: dynamics.and_then(|dynamics| dynamics.enable_all_on_launch).unwrap_or(false),
		contact_parameter_emission: contacts
			.and_then(|contacts| contacts.parameter_emission.or(contacts.parameter_emission_enabled))
			.unwrap_or(false),
		spring_bone_physics_configured: spring_bone_physics.is_some(),
		spring_bone_simulation_hz: spring_bone_physics
			.and_then(|physics| physics.simulation_hz)
			.filter(|value| value.is_finite())
			.unwrap_or(60.0)
			.clamp(30.0, 240.0),
		spring_bone_substeps: spring_bone_physics.and_then(|physics| physics.substeps).unwrap_or(1).clamp(1, 8),
		spring_bone_category_overrides: spring_bone_category_override_settings(spring_bone_physics, avatar_path, manifest_path),
		bone_colliders_enabled: bone_colliders.and_then(|bone_colliders| bone_colliders.enabled).unwrap_or(true),
		bone_collider_head: collider_radius_mm_value(bone_collider_radius_mm.and_then(|parts| parts.head), 120.0),
		bone_collider_neck_chest: collider_radius_mm_value(bone_collider_radius_mm.and_then(|parts| parts.neck_chest), 80.0),
		bone_collider_torso: collider_radius_mm_value(bone_collider_radius_mm.and_then(|parts| parts.torso), 140.0),
		bone_collider_upper_arms: collider_radius_mm_value(bone_collider_radius_mm.and_then(|parts| parts.upper_arms), 55.0),
		bone_collider_lower_arms: collider_radius_mm_value(bone_collider_radius_mm.and_then(|parts| parts.lower_arms), 45.0),
		bone_collider_hands: collider_radius_mm_value(bone_collider_radius_mm.and_then(|parts| parts.hands), 50.0),
	}
}

fn render_quality_settings(render_quality: ManifestRenderQuality, legacy_aa: Option<String>) -> RenderQualitySettings {
	RenderQualitySettings {
		aa: render_quality.aa.or(legacy_aa).unwrap_or_else(|| "off".to_string()),
		texture_resolution_limit: render_quality.texture_resolution_limit.unwrap_or_else(|| "off".to_string()),
		texture_compression: render_quality.texture_compression.unwrap_or_else(|| "balanced".to_string()),
		mipmap_filter: render_quality.mipmap_filter.unwrap_or_else(|| "mitchell".to_string()),
		render_backend: render_quality.render_backend.unwrap_or_else(|| "vulkan".to_string()),
		block_compression_encoder: render_quality.block_compression_encoder.unwrap_or_else(|| "gpu".to_string()),
		block_compression_cpu_threads: render_quality.block_compression_cpu_threads.unwrap_or(4).max(1),
		texture_compression_advanced: TextureCompressionAdvancedSetting::from_manifest(render_quality.texture_compression_advanced),
		processed_texture_cache: render_quality.processed_texture_cache.unwrap_or(true),
		skin_tone_matching: render_quality.skin_tone_matching.unwrap_or(false),
	}
}

fn motion_settings(motion: ManifestMotion, legacy_vmc_address: Option<String>, legacy_vmc_port: Option<u16>) -> MotionSettings {
	let vmc_udp = motion.vmc_udp.unwrap_or_default();
	let unmotion_zenoh = motion.unmotion_zenoh.unwrap_or_default();
	let look_at = motion.look_at.unwrap_or_default();
	let vmc_port = vmc_udp.port.or(legacy_vmc_port);
	let vmc_address = vmc_udp
		.address
		.or(legacy_vmc_address)
		.or_else(|| vmc_port.map(vmc_address_from_port));
	let primary_motion_source = match motion
		.primary_source
		.as_deref()
		.map(str::trim)
		.map(str::to_ascii_lowercase)
		.as_deref()
	{
		Some("unmotion_zenoh") => "unmotion_zenoh".to_string(),
		Some("vmc") => "vmc".to_string(),
		_ => "vmc".to_string(),
	};
	MotionSettings {
		motion_vmc_enabled: vmc_udp.enabled.unwrap_or(vmc_address.is_some()),
		vmc_address,
		vmc_port,
		motion_unmotion_enabled: unmotion_zenoh.enabled.unwrap_or(false),
		unmotion_zenoh_key: unmotion_zenoh.key,
		look_at_enabled: look_at.enabled.unwrap_or(false),
		look_at_clamp_deg: Some(clamped_f32_or(look_at.clamp_deg, 30.0, 0.0, 90.0)),
		apply_vmc_root_translation: motion.apply_vmc_root_translation.unwrap_or(false),
		primary_motion_source,
	}
}

fn audio_link_settings(audio_link: ManifestAudioLink) -> AudioLinkSettings {
	let source = match audio_link.source.as_deref().map(str::trim).map(str::to_ascii_lowercase).as_deref() {
		Some("input_device") => "input_device".to_string(),
		_ => "none".to_string(),
	};
	AudioLinkSettings {
		source,
		input_device_id: audio_link
			.input_device_id
			.and_then(|value| (!value.trim().is_empty()).then(|| value.trim().to_string())),
		input_device_name_hint: audio_link
			.input_device_name_hint
			.and_then(|value| (!value.trim().is_empty()).then(|| value.trim().to_string())),
	}
}

fn window_settings(
	window: ManifestWindow,
	legacy_icon_path: Option<PathBuf>,
	legacy_transparent: Option<bool>,
	legacy_input_passthrough: Option<bool>,
	legacy_decorations: Option<bool>,
) -> WindowSettings {
	let transparent = window.transparent.or(legacy_transparent).unwrap_or(false);
	WindowSettings {
		icon_path: window.icon_path.or(legacy_icon_path).map(|path| path.display().to_string()),
		transparent,
		input_passthrough: transparent && window.input_passthrough.or(legacy_input_passthrough).unwrap_or(false),
		decorations: window.decorations.or(legacy_decorations).unwrap_or(true),
		always_on_top: window.always_on_top.unwrap_or(false),
		minimized: window.minimized.unwrap_or(false),
		x: window.x,
		y: window.y,
		width: window.width.unwrap_or(800),
		height: window.height.unwrap_or(600),
	}
}

fn camera_settings(camera: Option<&ManifestCameraSetting>) -> CameraSettings {
	CameraSettings {
		locked: camera.and_then(|camera| camera.locked).unwrap_or(false),
		target: camera.and_then(|camera| camera.target),
		longitude_deg: camera.and_then(|camera| camera.longitude_deg),
		latitude_deg: camera.and_then(|camera| camera.latitude_deg),
		radius: camera.and_then(|camera| camera.radius),
		diagonal_fov_deg: camera.and_then(|camera| camera.diagonal_fov_deg),
	}
}

fn output_settings(output: Option<ManifestOutput>, legacy_spout: Option<ManifestSpout>) -> OutputSettings {
	let spout = output.and_then(|output| output.spout2).or(legacy_spout).unwrap_or_default();
	OutputSettings {
		spout_enabled: spout.enabled.unwrap_or(false),
		spout_name: spout.name,
		spout_width: spout.width,
		spout_height: spout.height,
	}
}

fn post_effect_settings(post: Option<ManifestPostEffects>) -> PostEffectSettings {
	let mut settings = PostEffectSettings {
		bloom_enabled: false,
		bloom_strength: 0.35,
		bloom_threshold: 0.65,
		bloom_radius: 8.0,
		bloom_quality: "compact".to_string(),
		ssao_enabled: false,
		ssao_strength: 0.25,
		ssao_radius: 4.0,
		ssao_bias: 0.0015,
		ssao_range: 0.03,
	};
	let Some(post) = post else {
		return settings;
	};
	if let Some(bloom) = post.bloom {
		settings.bloom_enabled = bloom.enabled.unwrap_or(false);
		settings.bloom_strength = clamped_f32_or(bloom.strength, 0.35, 0.0, 2.0);
		settings.bloom_threshold = clamped_f32_or(bloom.threshold, 0.65, 0.0, 2.0);
		settings.bloom_radius = clamped_f32_or(bloom.radius, 8.0, 0.0, 32.0);
		settings.bloom_quality = bloom
			.quality
			.as_deref()
			.and_then(normalize_bloom_quality)
			.unwrap_or_else(|| "compact".to_string());
	}
	if let Some(ssao) = post.ssao {
		settings.ssao_enabled = ssao.enabled.unwrap_or(false);
		if let Some(strength) = ssao.strength {
			settings.ssao_strength = strength.clamp(0.0, 1.0);
		}
		if let Some(radius) = ssao.radius {
			settings.ssao_radius = radius.clamp(1.0, 24.0);
		}
		if let Some(bias) = ssao.bias {
			settings.ssao_bias = bias.clamp(0.0, 0.02);
		}
		if let Some(range) = ssao.range {
			settings.ssao_range = range.clamp(0.001, 0.2);
		}
	}
	settings
}

fn avatar_effect_settings(avatar_effects: Option<ManifestAvatarEffects>) -> AvatarEffectSettings {
	let mut settings = AvatarEffectSettings {
		outline_policy: "authored".to_string(),
		outline_type: "mtoon".to_string(),
		outline_width: None,
		outline_color: None,
		outline_lighting_mix: None,
		outline_roundness: None,
		rim_policy: "authored".to_string(),
		rim_color: None,
		rim_intensity: None,
		rim_lighting_mix: None,
		rim_fresnel_power: None,
		rim_lift: None,
		matcap_scale: 1.0,
		specular_enabled: false,
		specular_intensity: 0.25,
		specular_power: 24.0,
		ambient_occlusion_strength: 1.0,
		contact_shadow_enabled: false,
		contact_shadow_strength: 0.35,
		contact_shadow_radius: 0.55,
		contact_shadow_softness: 1.8,
		contact_shadow_height: 0.0,
	};
	let Some(avatar_effects) = avatar_effects else {
		return settings;
	};
	if let Some(policy) = avatar_effects.outline_policy.as_deref().and_then(normalize_outline_policy) {
		settings.outline_policy = policy;
	}
	if let Some(kind) = avatar_effects.outline_type.as_deref().and_then(normalize_outline_type) {
		settings.outline_type = kind;
	}
	if let Some(width) = avatar_effects.outline_width {
		settings.outline_width = Some(width.max(0.0));
	}
	if let Some(color) = avatar_effects.outline_color {
		settings.outline_color = Some(clamp_rgb(color));
	}
	if let Some(lighting_mix) = avatar_effects.outline_lighting_mix {
		settings.outline_lighting_mix = Some(lighting_mix.clamp(0.0, 1.0));
	}
	if let Some(roundness) = avatar_effects.outline_roundness {
		settings.outline_roundness = Some(roundness.clamp(0.0, 1.0));
	}
	if let Some(outline) = avatar_effects.outline {
		if let Some(policy) = outline.policy.as_deref().and_then(normalize_outline_policy) {
			settings.outline_policy = policy;
		}
		if let Some(kind) = outline.r#type.as_deref().and_then(normalize_outline_type) {
			settings.outline_type = kind;
		}
		if let Some(width) = outline.width {
			settings.outline_width = Some(width.max(0.0));
		}
		if let Some(color) = outline.color {
			settings.outline_color = Some(clamp_rgb(color));
		}
		if let Some(lighting_mix) = outline.lighting_mix {
			settings.outline_lighting_mix = Some(lighting_mix.clamp(0.0, 1.0));
		}
		if let Some(roundness) = outline.roundness {
			settings.outline_roundness = Some(roundness.clamp(0.0, 1.0));
		}
	}
	if let Some(rim) = avatar_effects.rim {
		if let Some(policy) = rim.policy.as_deref().and_then(normalize_rim_policy) {
			settings.rim_policy = policy;
		}
		if let Some(color) = rim.color {
			settings.rim_color = Some(clamp_rgb(color));
		}
		if let Some(intensity) = rim.intensity {
			settings.rim_intensity = Some(intensity.clamp(0.0, 4.0));
		}
		if let Some(lighting_mix) = rim.lighting_mix {
			settings.rim_lighting_mix = Some(lighting_mix.clamp(0.0, 1.0));
		}
		if let Some(power) = rim.fresnel_power {
			settings.rim_fresnel_power = Some(power.max(0.00001));
		}
		if let Some(lift) = rim.lift {
			settings.rim_lift = Some(lift.clamp(-1.0, 1.0));
		}
	}
	if let Some(matcap) = avatar_effects.matcap {
		if let Some(scale) = matcap.scale {
			settings.matcap_scale = scale.clamp(0.0, 2.0);
		}
	}
	if let Some(specular) = avatar_effects.specular {
		settings.specular_enabled = specular.enabled.unwrap_or(false);
		if let Some(intensity) = specular.intensity {
			settings.specular_intensity = intensity.clamp(0.0, 2.0);
		}
		if let Some(power) = specular.power {
			settings.specular_power = power.clamp(1.0, 128.0);
		}
	}
	if let Some(ambient_occlusion) = avatar_effects.ambient_occlusion {
		if let Some(strength) = ambient_occlusion.strength {
			settings.ambient_occlusion_strength = strength.clamp(0.0, 2.0);
		}
	}
	if let Some(contact_shadow) = avatar_effects.contact_shadow {
		settings.contact_shadow_enabled = contact_shadow.enabled.unwrap_or(false);
		if let Some(strength) = contact_shadow.strength {
			settings.contact_shadow_strength = strength.clamp(0.0, 1.0);
		}
		if let Some(radius) = contact_shadow.radius {
			settings.contact_shadow_radius = radius.clamp(0.05, 3.0);
		}
		if let Some(softness) = contact_shadow.softness {
			settings.contact_shadow_softness = softness.clamp(0.1, 8.0);
		}
		if let Some(height) = contact_shadow.height {
			settings.contact_shadow_height = height.clamp(-1.0, 1.0);
		}
	}
	settings
}

fn validate_bloom_quality(value: &str) -> Result<String, String> {
	normalize_bloom_quality(value).ok_or_else(|| "bloom quality must be compact or high_quality".to_string())
}

fn json_outline_policy(value: &serde_json::Value, field: &str) -> Result<String, String> {
	let raw = json_string(value, field)?;
	normalize_outline_policy(&raw).ok_or_else(|| format!("{field} must be one of authored, off, override"))
}

fn json_rim_policy(value: &serde_json::Value, field: &str) -> Result<String, String> {
	let raw = json_string(value, field)?;
	normalize_rim_policy(&raw).ok_or_else(|| format!("{field} must be one of authored, off, override"))
}

fn json_outline_type(value: &serde_json::Value, field: &str) -> Result<String, String> {
	let raw = json_string(value, field)?;
	normalize_outline_type(&raw).ok_or_else(|| format!("{field} must be one of mtoon, ink, brush, double"))
}

fn clamp_rgb(rgb: [f32; 3]) -> [f32; 3] {
	[rgb[0].clamp(0.0, 1.0), rgb[1].clamp(0.0, 1.0), rgb[2].clamp(0.0, 1.0)]
}

fn json_rgb(value: &serde_json::Value, field: &str) -> Result<[f32; 3], String> {
	let values = value.as_array().ok_or_else(|| format!("{field} must be an RGB array"))?;
	if values.len() != 3 {
		return Err(format!("{field} must have exactly 3 components"));
	}
	let mut rgb = [0.0_f32; 3];
	for (i, slot) in rgb.iter_mut().enumerate() {
		*slot = values[i]
			.as_f64()
			.map(|v| v as f32)
			.ok_or_else(|| format!("{field}[{i}] must be a number"))?;
	}
	Ok(clamp_rgb(rgb))
}

fn default_vmc_address() -> String {
	"0.0.0.0:39539".to_string()
}

fn vmc_address_from_port(port: u16) -> String {
	format!("0.0.0.0:{port}")
}

fn json_u32(value: &serde_json::Value, field: &str) -> Result<u32, String> {
	let value = value.as_u64().ok_or_else(|| format!("{field} must be a number"))?;
	u32::try_from(value).map_err(|_| format!("{field} must fit in u32"))
}

fn json_usize(value: &serde_json::Value, field: &str) -> Result<usize, String> {
	let value = value.as_u64().ok_or_else(|| format!("{field} must be a number"))?;
	usize::try_from(value).map_err(|_| format!("{field} must fit in usize"))
}

/// Camera/物理量で使う f32 入力。`as_f64` を経由するので JSON 上は整数でも float でも受け付ける。
fn json_f32(value: &serde_json::Value, field: &str) -> Result<f32, String> {
	value.as_f64().map(|v| v as f32).ok_or_else(|| format!("{field} must be a number"))
}

fn json_optional_i16_px(value: &serde_json::Value, field: &str) -> Result<Option<i64>, String> {
	if value.is_null() {
		return Ok(None);
	}
	let value = value.as_i64().ok_or_else(|| format!("{field} must be an integer or null"))?;
	if !(-32768..=32767).contains(&value) {
		return Err(format!("{field} must fit in i16 px (-32768..=32767)"));
	}
	Ok(Some(value))
}

fn parse_manifest_value(text: &str, path: &Path) -> Result<toml::Value, String> {
	let table = toml::from_str::<toml::Table>(text).map_err(|e| format!("parse {}: {e}", path.display()))?;
	Ok(toml::Value::Table(table))
}

fn read_manifest_value(path: &Path) -> Result<toml::Value, String> {
	let text = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
	parse_manifest_value(&text, path)
}

fn write_manifest_value(path: &Path, manifest: &toml::Value) -> Result<(), String> {
	let text = toml::to_string_pretty(manifest).map_err(|e| format!("serialize manifest: {e}"))?;
	fs::write(path, text).map_err(|e| format!("write {}: {e}", path.display()))
}

fn remove_root_key(manifest: &mut toml::Value, key: &str) -> Result<(), String> {
	let table = manifest.as_table_mut().ok_or_else(|| "manifest root must be a table".to_string())?;
	table.remove(key);
	Ok(())
}

fn set_optional_root_string(manifest: &mut toml::Value, key: &str, value: String) -> Result<(), String> {
	if value.trim().is_empty() {
		remove_root_key(manifest, key)
	} else {
		set_root_string(manifest, key, value)
	}
}

fn set_root_bool(manifest: &mut toml::Value, key: &str, value: bool) -> Result<(), String> {
	let table = manifest.as_table_mut().ok_or_else(|| "manifest root must be a table".to_string())?;
	table.insert(key.to_string(), toml::Value::Boolean(value));
	Ok(())
}

fn set_root_string(manifest: &mut toml::Value, key: &str, value: String) -> Result<(), String> {
	let table = manifest.as_table_mut().ok_or_else(|| "manifest root must be a table".to_string())?;
	table.insert(key.to_string(), toml::Value::String(value));
	Ok(())
}

fn set_profile_value(manifest: &mut toml::Value, key: &str, value: toml::Value) -> Result<(), String> {
	let table = manifest.as_table_mut().ok_or_else(|| "manifest root must be a table".to_string())?;
	let profile = table
		.entry("profile".to_string())
		.or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
		.as_table_mut()
		.ok_or_else(|| "profile must be a table".to_string())?;
	profile.insert(key.to_string(), value);
	Ok(())
}

fn set_root_array(manifest: &mut toml::Value, key: &str, value: Vec<toml::Value>) -> Result<(), String> {
	let table = manifest.as_table_mut().ok_or_else(|| "manifest root must be a table".to_string())?;
	table.insert(key.to_string(), toml::Value::Array(value));
	Ok(())
}

fn set_root_rgb_array(manifest: &mut toml::Value, key: &str, color: [f32; 3]) -> Result<(), String> {
	set_root_array(manifest, key, toml_rgb_array(color))
}

fn set_nested_bool(manifest: &mut toml::Value, path: &[&str], value: bool) -> Result<(), String> {
	set_nested_value(manifest, path, toml::Value::Boolean(value))
}

fn set_nested_json_bool(manifest: &mut toml::Value, path: &[&str], value: &serde_json::Value, field: &str) -> Result<(), String> {
	set_nested_bool(manifest, path, json_bool(value, field)?)
}

fn set_nested_integer(manifest: &mut toml::Value, path: &[&str], value: i64) -> Result<(), String> {
	set_nested_value(manifest, path, toml::Value::Integer(value))
}

fn set_nested_string(manifest: &mut toml::Value, path: &[&str], value: String) -> Result<(), String> {
	set_nested_value(manifest, path, toml::Value::String(value))
}

fn set_nested_json_string(manifest: &mut toml::Value, path: &[&str], value: &serde_json::Value, field: &str) -> Result<(), String> {
	set_nested_string(manifest, path, json_string(value, field)?)
}

fn set_nested_float(manifest: &mut toml::Value, path: &[&str], value: f32) -> Result<(), String> {
	set_nested_value(manifest, path, toml::Value::Float(f64::from(value)))
}

fn set_nested_rgb_array(manifest: &mut toml::Value, path: &[&str], color: [f32; 3]) -> Result<(), String> {
	set_nested_value(manifest, path, toml::Value::Array(toml_rgb_array(color)))
}

fn toml_rgb_array(color: [f32; 3]) -> Vec<toml::Value> {
	color.into_iter().map(|v| toml::Value::Float(f64::from(v))).collect()
}

fn set_nested_ranged_float(
	manifest: &mut toml::Value,
	path: &[&str],
	value: &serde_json::Value,
	field: &str,
	range: std::ops::RangeInclusive<f32>,
	range_label: &str,
) -> Result<(), String> {
	let value = json_f32(value, field)?;
	if !range.contains(&value) {
		return Err(format!("{field} must be in {range_label}"));
	}
	set_nested_float(manifest, path, value)
}

fn ranged_float_toml_value(
	value: &serde_json::Value,
	field: &str,
	range: std::ops::RangeInclusive<f32>,
	range_label: &str,
) -> Result<toml::Value, String> {
	let value = json_f32(value, field)?;
	if !range.contains(&value) {
		return Err(format!("{field} must be in {range_label}"));
	}
	Ok(toml::Value::Float(f64::from(value)))
}

fn set_nested_ranged_u32(
	manifest: &mut toml::Value,
	path: &[&str],
	value: &serde_json::Value,
	field: &str,
	range: std::ops::RangeInclusive<u32>,
	range_label: &str,
) -> Result<(), String> {
	let value = ranged_u32(value, field, range, range_label)?;
	set_nested_integer(manifest, path, i64::from(value))
}

fn ranged_u32_toml_value(
	value: &serde_json::Value,
	field: &str,
	range: std::ops::RangeInclusive<u32>,
	range_label: &str,
) -> Result<toml::Value, String> {
	let value = ranged_u32(value, field, range, range_label)?;
	Ok(toml::Value::Integer(i64::from(value)))
}

fn ranged_u32(value: &serde_json::Value, field: &str, range: std::ops::RangeInclusive<u32>, range_label: &str) -> Result<u32, String> {
	let value = json_u32(value, field)?;
	if !range.contains(&value) {
		return Err(format!("{field} must be in {range_label}"));
	}
	Ok(value)
}

fn set_collider_part_radius_mm(manifest: &mut toml::Value, part: &str, value: f32) -> Result<(), String> {
	if !value.is_finite() || !(0.0..=1000.0).contains(&value) {
		return Err(format!("physics.bone_colliders.radius_mm.{part} must be in [0.0, 1000.0]"));
	}
	let _ = remove_nested_key(manifest, &["physics", "bone_colliders", "parts"]);
	set_nested_float(manifest, &["physics", "bone_colliders", "radius_mm", part], value)
}

fn apply_spring_bone_category_override_value(
	manifest: &mut toml::Value,
	setting: &AvatarSetting,
	field: &str,
	value: serde_json::Value,
) -> Result<(), String> {
	let rest = field
		.strip_prefix("physics.spring_bone.overrides.")
		.ok_or_else(|| format!("invalid Dynamics override field: {field}"))?;
	let (category, key) = rest
		.split_once('.')
		.ok_or_else(|| format!("invalid Dynamics override field: {field}"))?;
	let category = normalize_spring_bone_category_id(category);
	let authored = spring_bone_authored_params_for_setting(setting, &category);
	if key == "mode" {
		let mode = validate_spring_bone_override_mode(json_string(&value, field)?.as_str())?;
		set_spring_bone_category_mode(manifest, &category, &mode, authored)?;
		return Ok(());
	}
	if key == "reset" {
		let mode = setting
			.spring_bone_category_overrides
			.iter()
			.find(|item| item.category == category)
			.map(|item| item.mode.as_str())
			.unwrap_or("authored")
			.to_string();
		set_spring_bone_category_mode(manifest, &category, &mode, authored)?;
		return Ok(());
	}
	if key == "preset" {
		let preset = json_string(&value, field)?;
		if spring_bone_category_override_solver(manifest, &category).as_deref() != Some("xpbd") {
			return Err(format!("{field} can be applied only when Dynamics mode is Override: XPBD"));
		}
		set_spring_bone_category_recommended_preset(manifest, &category, &preset)?;
		return Ok(());
	}
	let (toml_key, toml_value) = match key {
		"solver" => (
			"solver",
			toml::Value::String(validate_spring_bone_solver(json_string(&value, field)?.as_str())?),
		),
		"damping_half_life_ms" => (
			"damping_half_life_ms",
			ranged_float_toml_value(&value, field, 1.0..=10_000.0, "[1, 10000]")?,
		),
		"stiffness_hz" => ("stiffness_hz", ranged_float_toml_value(&value, field, 0.0..=60.0, "[0, 60]")?),
		"xpbd_compliance" => ("xpbd_compliance", ranged_float_toml_value(&value, field, 0.0..=10.0, "[0, 10]")?),
		"constraint_iterations" => ("constraint_iterations", ranged_u32_toml_value(&value, field, 1..=32, "[1, 32]")?),
		_ => return Err(format!("unknown Dynamics override field: {field}")),
	};
	set_spring_bone_category_override_value(manifest, &category, toml_key, toml_value)
}

fn spring_bone_category_override_solver(manifest: &toml::Value, category: &str) -> Option<String> {
	manifest
		.get("physics")?
		.get("spring_bone")?
		.get("overrides")?
		.as_array()?
		.iter()
		.find(|item| {
			item.get("category")
				.and_then(toml::Value::as_str)
				.map(normalize_spring_bone_category_id)
				.as_deref() == Some(category)
		})?
		.get("solver")?
		.as_str()
		.and_then(normalize_spring_bone_solver)
}

fn spring_bone_authored_params_for_setting(setting: &AvatarSetting, category: &str) -> SpringBoneCategoryAuthoredParams {
	setting
		.spring_bone_category_overrides
		.iter()
		.find(|item| normalize_spring_bone_category_id(&item.category) == category)
		.map(|item| SpringBoneCategoryAuthoredParams {
			count: item.spring_bone_count,
			stiffness_hz: item.authored_stiffness_hz,
			xpbd_compliance: item.authored_xpbd_compliance,
		})
		.unwrap_or_default()
}

fn validate_spring_bone_override_mode(value: &str) -> Result<String, String> {
	match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
		"authored" | "authored_verlet" => Ok("authored".to_string()),
		"override_verlet" | "verlet" => Ok("override_verlet".to_string()),
		"override_xpbd" | "xpbd" => Ok("override_xpbd".to_string()),
		_ => Err("spring bone override mode must be authored, override_verlet, or override_xpbd".to_string()),
	}
}

fn set_spring_bone_category_mode(
	manifest: &mut toml::Value,
	category: &str,
	mode: &str,
	authored: SpringBoneCategoryAuthoredParams,
) -> Result<(), String> {
	match mode {
		"authored" => remove_spring_bone_category_override(manifest, category),
		"override_verlet" => replace_spring_bone_category_override(
			manifest,
			category,
			[
				("solver".to_string(), toml::Value::String("verlet".to_string())),
				(
					"stiffness_hz".to_string(),
					toml::Value::Float(f64::from(authored.stiffness_hz.clamp(0.0, 60.0))),
				),
			],
		),
		"override_xpbd" => replace_spring_bone_category_override(
			manifest,
			category,
			[
				("solver".to_string(), toml::Value::String("xpbd".to_string())),
				(
					"xpbd_compliance".to_string(),
					toml::Value::Float(f64::from(authored.xpbd_compliance.clamp(0.0, 10.0))),
				),
				("constraint_iterations".to_string(), toml::Value::Integer(4)),
			],
		),
		_ => Err("spring bone override mode must be authored, override_verlet, or override_xpbd".to_string()),
	}
}

fn set_spring_bone_category_recommended_preset(manifest: &mut toml::Value, category: &str, preset: &str) -> Result<(), String> {
	let preset = spring_bone_recommended_preset(category, preset)
		.ok_or_else(|| format!("unknown Dynamics recommended preset: {category}.{preset}"))?;
	replace_spring_bone_category_override(
		manifest,
		category,
		[
			("solver".to_string(), toml::Value::String("xpbd".to_string())),
			(
				"damping_half_life_ms".to_string(),
				toml::Value::Float(f64::from(preset.damping_half_life_ms)),
			),
			("xpbd_compliance".to_string(), toml::Value::Float(f64::from(preset.xpbd_compliance))),
			(
				"constraint_iterations".to_string(),
				toml::Value::Integer(i64::from(preset.constraint_iterations)),
			),
		],
	)
}

#[derive(Clone, Copy)]
struct SpringBoneRecommendedPreset {
	damping_half_life_ms: f32,
	xpbd_compliance: f32,
	constraint_iterations: u32,
}

fn spring_bone_recommended_preset(category: &str, preset: &str) -> Option<SpringBoneRecommendedPreset> {
	let preset = preset.trim().to_ascii_lowercase().replace('-', "_");
	match (category, preset.as_str()) {
		("hair", "soft") => Some(SpringBoneRecommendedPreset {
			damping_half_life_ms: 190.0,
			xpbd_compliance: 0.018,
			constraint_iterations: 5,
		}),
		("hair", "natural") => Some(SpringBoneRecommendedPreset {
			damping_half_life_ms: 130.0,
			xpbd_compliance: 0.009,
			constraint_iterations: 6,
		}),
		("hair", "snappy") => Some(SpringBoneRecommendedPreset {
			damping_half_life_ms: 80.0,
			xpbd_compliance: 0.0045,
			constraint_iterations: 6,
		}),
		("ears", "soft") => Some(SpringBoneRecommendedPreset {
			damping_half_life_ms: 160.0,
			xpbd_compliance: 0.012,
			constraint_iterations: 5,
		}),
		("ears", "natural") => Some(SpringBoneRecommendedPreset {
			damping_half_life_ms: 95.0,
			xpbd_compliance: 0.004,
			constraint_iterations: 6,
		}),
		("ears", "snappy") => Some(SpringBoneRecommendedPreset {
			damping_half_life_ms: 55.0,
			xpbd_compliance: 0.0018,
			constraint_iterations: 7,
		}),
		("tail", "soft") => Some(SpringBoneRecommendedPreset {
			damping_half_life_ms: 260.0,
			xpbd_compliance: 0.028,
			constraint_iterations: 5,
		}),
		("tail", "natural") => Some(SpringBoneRecommendedPreset {
			damping_half_life_ms: 180.0,
			xpbd_compliance: 0.014,
			constraint_iterations: 6,
		}),
		("tail", "heavy") => Some(SpringBoneRecommendedPreset {
			damping_half_life_ms: 320.0,
			xpbd_compliance: 0.006,
			constraint_iterations: 8,
		}),
		("cloth", "light") => Some(SpringBoneRecommendedPreset {
			damping_half_life_ms: 150.0,
			xpbd_compliance: 0.018,
			constraint_iterations: 5,
		}),
		("cloth", "natural") => Some(SpringBoneRecommendedPreset {
			damping_half_life_ms: 110.0,
			xpbd_compliance: 0.007,
			constraint_iterations: 6,
		}),
		("cloth", "firm") => Some(SpringBoneRecommendedPreset {
			damping_half_life_ms: 70.0,
			xpbd_compliance: 0.0025,
			constraint_iterations: 8,
		}),
		_ => None,
	}
}

fn replace_spring_bone_category_override<const N: usize>(
	manifest: &mut toml::Value,
	category: &str,
	values: [(String, toml::Value); N],
) -> Result<(), String> {
	remove_spring_bone_category_override(manifest, category)?;
	let spring_bone = spring_bone_table_mut(manifest)?;
	let overrides = spring_bone
		.entry("overrides".to_string())
		.or_insert_with(|| toml::Value::Array(Vec::new()))
		.as_array_mut()
		.ok_or_else(|| "physics.spring_bone.overrides must be an array".to_string())?;
	let mut table = toml::map::Map::new();
	table.insert("category".to_string(), toml::Value::String(category.to_string()));
	for (key, value) in values {
		table.insert(key, value);
	}
	overrides.push(toml::Value::Table(table));
	Ok(())
}

fn set_spring_bone_category_override_value(
	manifest: &mut toml::Value,
	category: &str,
	key: &str,
	value: toml::Value,
) -> Result<(), String> {
	let spring_bone = spring_bone_table_mut(manifest)?;
	let overrides = spring_bone
		.entry("overrides".to_string())
		.or_insert_with(|| toml::Value::Array(Vec::new()))
		.as_array_mut()
		.ok_or_else(|| "physics.spring_bone.overrides must be an array".to_string())?;
	let index = overrides
		.iter()
		.position(|item| {
			item.get("category")
				.and_then(toml::Value::as_str)
				.map(normalize_spring_bone_category_id)
				.as_deref() == Some(category)
		})
		.unwrap_or_else(|| {
			let mut table = toml::map::Map::new();
			table.insert("category".to_string(), toml::Value::String(category.to_string()));
			overrides.push(toml::Value::Table(table));
			overrides.len() - 1
		});
	let table = overrides[index]
		.as_table_mut()
		.ok_or_else(|| "physics.spring_bone.overrides item must be a table".to_string())?;
	table.insert(key.to_string(), value);
	Ok(())
}

fn remove_spring_bone_category_override(manifest: &mut toml::Value, category: &str) -> Result<(), String> {
	let spring_bone = spring_bone_table_mut(manifest)?;
	let Some(overrides) = spring_bone.get_mut("overrides").and_then(toml::Value::as_array_mut) else {
		return Ok(());
	};
	overrides.retain(|item| {
		item.get("category")
			.and_then(toml::Value::as_str)
			.map(normalize_spring_bone_category_id)
			.as_deref()
			!= Some(category)
	});
	Ok(())
}

fn spring_bone_table_mut(manifest: &mut toml::Value) -> Result<&mut toml::map::Map<String, toml::Value>, String> {
	let table = manifest.as_table_mut().ok_or_else(|| "manifest root must be a table".to_string())?;
	let physics = table
		.entry("physics".to_string())
		.or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
		.as_table_mut()
		.ok_or_else(|| "physics must be a table".to_string())?;
	physics
		.entry("spring_bone".to_string())
		.or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
		.as_table_mut()
		.ok_or_else(|| "physics.spring_bone must be a table".to_string())
}

/// `[camera] target = [x, y, z]` の特定軸だけを更新する。
/// target が存在しなければ `[0.0, 0.0, 0.0]` で初期化してから書き換える。
fn update_camera_target_axis(manifest: &mut toml::Value, axis: usize, value: f32) -> Result<(), String> {
	if axis >= 3 {
		return Err(format!("camera target axis must be 0..=2, got {axis}"));
	}
	let table = manifest.as_table_mut().ok_or_else(|| "manifest root must be a table".to_string())?;
	let camera_table = table
		.entry("camera".to_string())
		.or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
		.as_table_mut()
		.ok_or_else(|| "manifest [camera] must be a table".to_string())?;
	let mut target = camera_table
		.get("target")
		.and_then(|v| v.as_array())
		.map(|a| {
			let mut t = [0.0f32; 3];
			for (i, slot) in t.iter_mut().enumerate() {
				// TOML はスカラ float/int を別型として持つので両方拾う。
				*slot = a
					.get(i)
					.and_then(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64)))
					.map(|x| x as f32)
					.unwrap_or(0.0);
			}
			t
		})
		.unwrap_or([0.0, 0.0, 0.0]);
	target[axis] = value;
	camera_table.insert(
		"target".to_string(),
		toml::Value::Array(target.iter().map(|v| toml::Value::Float(f64::from(*v))).collect()),
	);
	Ok(())
}

fn set_optional_nested_string(manifest: &mut toml::Value, path: &[&str], value: String) -> Result<(), String> {
	if value.trim().is_empty() {
		remove_nested_key(manifest, path)
	} else {
		set_nested_string(manifest, path, value)
	}
}

fn set_nested_value(manifest: &mut toml::Value, path: &[&str], value: toml::Value) -> Result<(), String> {
	let (leaf, parents) = path.split_last().ok_or_else(|| "setting path must not be empty".to_string())?;
	let mut table = manifest.as_table_mut().ok_or_else(|| "manifest root must be a table".to_string())?;
	for parent in parents {
		table = table
			.entry((*parent).to_string())
			.or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
			.as_table_mut()
			.ok_or_else(|| format!("{parent} must be a table"))?;
	}
	table.insert((*leaf).to_string(), value);
	Ok(())
}

fn remove_nested_key(manifest: &mut toml::Value, path: &[&str]) -> Result<(), String> {
	let (leaf, parents) = path.split_last().ok_or_else(|| "setting path must not be empty".to_string())?;
	let mut table = manifest.as_table_mut().ok_or_else(|| "manifest root must be a table".to_string())?;
	for parent in parents {
		let Some(next) = table.get_mut(*parent) else {
			return Ok(());
		};
		table = next.as_table_mut().ok_or_else(|| format!("{parent} must be a table"))?;
	}
	table.remove(*leaf);
	Ok(())
}

fn resolve_repo_path(path: &str) -> PathBuf {
	let path = PathBuf::from(path);
	if path.is_absolute() {
		path
	} else {
		repo_root().join(path)
	}
}

fn path_for_manifest(path: &Path) -> String {
	path.to_string_lossy().trim().to_string()
}

fn avatar_path_for_manifest_value(path: &str, manifest_path: &Path) -> String {
	let trimmed = path.trim();
	if trimmed.is_empty() {
		return String::new();
	}
	resolve_avatar_metadata_path(trimmed, Some(&manifest_path.display().to_string()))
		.display()
		.to_string()
}

fn diagnostics_dir() -> PathBuf {
	repo_root().join("target").join("tmp").join("diagnostics")
}

fn diagnostics_generated_at_secs(path: &Path) -> Option<u64> {
	let stem = path.file_stem()?.to_str()?;
	stem.strip_prefix("un-avatar-supervisor-")?.parse().ok()
}

fn system_time_secs(time: SystemTime) -> Option<u64> {
	time.duration_since(UNIX_EPOCH).ok().map(|duration| duration.as_secs())
}

fn diagnostics_archive_path(path: &Path) -> PathBuf {
	let mut archive_path = path.to_path_buf();
	archive_path.set_extension("zip");
	archive_path
}

#[cfg(windows)]
fn compress_file_to_zip(path: &Path, archive_path: &Path) -> Result<(), String> {
	let mut command = Command::new("powershell.exe");
	configure_hidden_child(&mut command);
	let status = command
		.args(["-NoProfile", "-NonInteractive", "-Command", "Compress-Archive", "-LiteralPath"])
		.arg(path)
		.arg("-DestinationPath")
		.arg(archive_path)
		.arg("-Force")
		.status()
		.map_err(|e| format!("compress diagnostics: {e}"))?;
	if !status.success() {
		return Err(format!("compress diagnostics failed: {}", path.display()));
	}
	Ok(())
}

#[cfg(not(windows))]
fn compress_file_to_zip(path: &Path, archive_path: &Path) -> Result<(), String> {
	let status = Command::new("zip")
		.args(["-j", "-q"])
		.arg(archive_path)
		.arg(path)
		.status()
		.map_err(|e| format!("compress diagnostics: {e}"))?;
	if !status.success() {
		return Err(format!("compress diagnostics failed: {}", path.display()));
	}
	Ok(())
}

#[allow(clippy::needless_return)]
fn open_path_in_file_manager(path: &Path) -> Result<(), String> {
	#[cfg(windows)]
	{
		let mut command = Command::new("explorer.exe");
		if path.is_file() {
			command.arg(format!("/select,{}", path.display()));
		} else {
			command.arg(path);
		}
		command.spawn().map_err(|e| format!("open explorer: {e}"))?;
		return Ok(());
	}

	#[cfg(target_os = "macos")]
	{
		let mut command = Command::new("open");
		if path.is_file() {
			command.arg("-R").arg(path);
		} else {
			command.arg(path);
		}
		command.spawn().map_err(|e| format!("open finder: {e}"))?;
		return Ok(());
	}

	#[cfg(all(unix, not(target_os = "macos")))]
	{
		let target = if path.is_file() { path.parent().unwrap_or(path) } else { path };
		Command::new("xdg-open")
			.arg(target)
			.spawn()
			.map_err(|e| format!("open file manager: {e}"))?;
		return Ok(());
	}
}

#[cfg(windows)]
fn activate_process_window(pid: u32) -> Result<(), String> {
	let script = r#"
param([int]$RendererPid)
$process = Get-Process -Id $RendererPid -ErrorAction Stop
$handle = $process.MainWindowHandle
if ($handle -eq 0) { throw "renderer window not found for pid $RendererPid" }
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class WindowActivation {
    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")]
    public static extern bool ShowWindowAsync(IntPtr hWnd, int nCmdShow);
}
"@
[WindowActivation]::ShowWindowAsync($handle, 9) | Out-Null
if (-not [WindowActivation]::SetForegroundWindow($handle)) { throw "activate renderer window for pid $RendererPid failed" }
"#;
	let mut command = Command::new("powershell.exe");
	configure_hidden_child(&mut command);
	let status = command
		.args([
			"-NoProfile",
			"-ExecutionPolicy",
			"Bypass",
			"-Command",
			script,
			"-RendererPid",
			&pid.to_string(),
		])
		.status()
		.map_err(|e| format!("activate renderer window: {e}"))?;
	if !status.success() {
		return Err(format!("activate renderer window for pid {pid} failed"));
	}
	Ok(())
}

#[cfg(not(windows))]
fn activate_process_window(_pid: u32) -> Result<(), String> {
	Err("renderer window activation is only implemented on Windows".to_string())
}

fn repo_root() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR"))
		.parent()
		.and_then(Path::parent)
		.and_then(Path::parent)
		.expect("src-tauri is under apps/un-avatar-supervisor/src-tauri")
		.to_path_buf()
}

fn exe_name(name: &str) -> String {
	if cfg!(windows) {
		format!("{name}.exe")
	} else {
		name.to_string()
	}
}

#[cfg(test)]
mod tests {
	use std::{
		fs,
		io::{BufRead, BufReader, Cursor, Write},
		net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
		path::Path,
		sync::{atomic::Ordering, Arc, Mutex},
		thread,
		time::{Duration, Instant},
	};

	use super::{
		apply_avatar_setting_value, avatar_model_picker_parent, data_image_base64_parts, diagnostics_archive_path,
		diagnostics_generated_at_secs, encode_profile_icon_thumbnail_webp, parse_manifest_value, path_for_manifest, percent_decode_utf8,
		perfect_sync_hit_count, read_avatar_setting, read_runtime_telemetry, read_unavatar_wardrobe_options, read_vrm_metadata,
		renderer_launch_control_commands, repo_root, resolve_renderer_window_icon_path, resolve_screenshot_path,
		screenshot_profile_filename_stem, send_renderer_control, send_renderer_control_session, spawn_runtime_status_stream,
		spout_runtime_note, texture_runtime_note, thumbnail_protocol_file_name, unique_profile_id, validate_spout_dimension, AvatarSetting,
		ProfileStorage, RendererControlCommand, RendererRuntimeTelemetry, TextureRuntimeSummary, PROFILE_ICON_THUMBNAIL_MAX_DIMENSION,
	};

	fn runtime_telemetry_fixture() -> RendererRuntimeTelemetry {
		RendererRuntimeTelemetry {
			connected: true,
			protocol: Some("local-tcp-json-v2".to_string()),
			control_capabilities: Vec::new(),
			scene_state: "avatar_scene".to_string(),
			uptime_secs: 1,
			fps: Some(60.0),
			cpu_ms: Some(1.0),
			frame_cpu_total_ms: Some(1.4),
			frame_motion_apply_ms: None,
			frame_dynamics_step_ms: Some(0.2),
			frame_globals_ms: None,
			frame_surface_acquire_ms: None,
			frame_target_prepare_ms: None,
			frame_draw_state_refresh_ms: None,
			frame_scene_world_ms: None,
			frame_draw_skin_palette_ms: None,
			frame_draw_skin_palette_write_ms: None,
			frame_draw_fur_source_vertices_ms: None,
			frame_draw_expression_values_ms: None,
			frame_draw_morph_weights_ms: None,
			frame_draw_transform_loop_ms: None,
			frame_bone_collider_debug_ms: None,
			frame_command_encode_ms: None,
			frame_submit_present_ms: None,
			frame_spout_cpu_ms: None,
			frame_contact_eval_ms: Some(0.1),
			frame_runtime_action_eval_ms: Some(0.1),
			gpu_ms: Some(2.0),
			ram_mb: None,
			surface_width: Some(1280),
			surface_height: Some(720),
			window_position: Some([120, 80]),
			window_inner_size: Some([1280, 720]),
			aa: Some("fxaa".to_string()),
			texture_resolution_limit: Some("off".to_string()),
			texture_compression: Some("source".to_string()),
			mipmap_filter: Some("mitchell".to_string()),
			processed_texture_cache: Some(true),
			texture_summary: Some(TextureRuntimeSummary {
				image_count: 2,
				uploaded_mip_bytes: 2048,
				..TextureRuntimeSummary::default()
			}),
			spout_available: true,
			spout_enabled: true,
			spout_name: Some("UN Avatar Spout".to_string()),
			spout_width: Some(1280),
			spout_height: Some(720),
			spout_frames_attempted: 0,
			spout_frames_sent: 0,
			spout_frame_failures: 0,
			spout_consecutive_failures: 0,
			spout_last_send_ok: None,
			spout_last_readback_ms: None,
			spout_last_send_ms: None,
			spout_last_total_ms: None,
			spout_sender_initialized: Some(true),
			spout_sender_width: Some(1280),
			spout_sender_height: Some(720),
			expression_presets: Vec::new(),
			look_at_enabled: true,
			look_at_clamp_deg: Some(30.0),
			apply_vmc_root_translation: false,
			unmotion_zenoh_enabled: false,
			unmotion_zenoh_key: String::new(),
			unmotion_zenoh_received_frames: 0,
			motion_applied_frames: 0,
			audio_link_texture_needed: false,
			primary_motion_source: "vmc".to_string(),
			show_axes: false,
			show_bone_colliders: false,
			bone_collider_count: 0,
			bone_collider_source: "off".to_string(),
			dynamics_group_count: 0,
			dynamics_enabled_group_count: 0,
			dynamics_source_enabled_group_count: 0,
			dynamics_enabled_override_count: 0,
			dynamics_vrm_spring_bone_group_count: 0,
			dynamics_vrc_physbone_group_count: 0,
			dynamics_unknown_group_count: 0,
			dynamics_limit_group_count: 0,
			dynamics_angle_limit_group_count: 0,
			dynamics_stretch_limit_group_count: 0,
			dynamics_rotation_translation_writeback_group_count: 0,
			dynamics_translation_writeback_candidate_count: 0,
			dynamics_translation_writeback_target_count: 0,
			dynamics_stretch_translation_writeback_group_count: 0,
			dynamics_stretch_translation_writeback_target_group_count: 0,
			dynamics_grabbing_enabled_group_count: 0,
			dynamics_posing_enabled_group_count: 0,
			dynamics_collider_count: 0,
			dynamics_vrm_spring_bone_collider_count: 0,
			dynamics_vrc_physbone_collider_count: 0,
			dynamics_unknown_collider_count: 0,
			dynamics_contact_count: 0,
			dynamics_vrc_contact_sender_count: 0,
			dynamics_vrc_contact_receiver_count: 0,
			dynamics_contact_parameter_declaration_count: 0,
			dynamics_contact_probe_count: 0,
			dynamics_contact_probe_would_emit_count: 0,
			dynamics_contact_parameter_emission_count: 0,
			dynamics_contact_parameter_emitted_count: 0,
			dynamics_contact_parameter_reset_to_zero_count: 0,
			dynamics_constraint_ref_count: 0,
			dynamics_vrc_constraint_ref_count: 0,
			runtime_parameter_definitions: Vec::new(),
			runtime_parameter_conflicts: Vec::new(),
			runtime_actions: Vec::new(),
			runtime_action_target_write_collisions: Vec::new(),
			runtime_action_restore_readiness: Vec::new(),
			runtime_action_restore_baseline_candidates: Vec::new(),
			runtime_action_restore_baseline_capture_plan: Vec::new(),
			runtime_action_restore_apply_plan: Vec::new(),
			menu_action_candidates: Vec::new(),
			menu_wardrobe_candidates: Vec::new(),
			contact_parameter_declarations: Vec::new(),
			contact_parameter_emission_enabled: false,
			contact_parameter_emissions: Vec::new(),
			contact_probes: Vec::new(),
			dynamics_groups: Vec::new(),
			dynamics_interaction_hooks: Vec::new(),
			dynamics_colliders: Vec::new(),
			dynamics_constraint_refs: Vec::new(),
			dynamics_warnings: Vec::new(),
			camera_locked: false,
			window_focused: false,
			window_activation_seq: 0,
			minimized: false,
			camera: None,
			clear_color: [0.02, 0.025, 0.035, 1.0],
			transparent_window: false,
			input_passthrough: false,
			startup_phase: None,
			startup_progress: None,
			startup_message: None,
			note: None,
		}
	}

	#[test]
	fn screenshot_profile_filename_stem_removes_separators() {
		assert_eq!(screenshot_profile_filename_stem(" model:1 / capture?. "), "model-1-capture");
		assert_eq!(screenshot_profile_filename_stem(""), "renderer");
	}

	#[test]
	fn screenshot_path_preserves_explicit_request() {
		let path = resolve_screenshot_path(Some("C:/tmp/custom-shot.png".to_string()), "model1").unwrap();
		assert_eq!(path, Path::new("C:/tmp/custom-shot.png"));
	}

	#[test]
	fn avatar_model_picker_parent_uses_selected_file_directory() {
		assert_eq!(
			avatar_model_picker_parent(Path::new("C:/Users/the/Documents/models/avatar.vrm")).as_deref(),
			Some("C:/Users/the/Documents/models")
		);
	}

	#[test]
	fn picked_avatar_path_preserves_absolute_path_for_portable_packages() {
		let path = Path::new("C:/Users/the/Documents/models/avatar.vrm");
		assert_eq!(path_for_manifest(path), "C:/Users/the/Documents/models/avatar.vrm");
	}

	#[test]
	fn profile_id_slug_is_stable_for_copied_settings() {
		assert_eq!(unique_profile_id("Main Avatar Copy"), "main-avatar-copy");
		assert_eq!(unique_profile_id("  Debug/View Copy!  "), "debug-view-copy");
	}

	#[test]
	fn runtime_telemetry_reads_json_snapshot() {
		let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
		let address = listener.local_addr().unwrap();
		let server = thread::spawn(move || {
			let (mut stream, _) = listener.accept().unwrap();
			writeln!(
				stream,
				r#"{{"connected":true,"uptime_secs":7,"fps":59.5,"cpu_ms":1.25,"gpu_ms":2.5,"ram_mb":null,"surface_width":800,"surface_height":600,"aa":"smaa","texture_resolution_limit":"4k","texture_compression":"auto","processed_texture_cache":true,"texture_summary":{{"image_count":3,"resized_count":1,"compression_mode":"auto","compression_bc_supported":true,"compression_astc_supported":false,"compression_etc2_supported":false,"compressed_count":2,"compression_fallback_count":1,"compressed_mip_bytes":1024,"cache_enabled":true,"cache_hits":1,"cache_misses":2,"cache_writes":2,"compressed_cache_hits":0,"compressed_cache_misses":2,"compressed_cache_writes":1,"source_bytes":4096,"uploaded_mip_bytes":2048,"max_source_dimension":2048,"max_uploaded_dimension":1024,"limit_max_dimension":4096}},"spout_enabled":false,"spout_name":null,"spout_width":null,"spout_height":null,"dynamics_group_count":9,"dynamics_limit_group_count":8,"dynamics_angle_limit_group_count":7,"dynamics_stretch_limit_group_count":6,"dynamics_rotation_translation_writeback_group_count":2,"dynamics_translation_writeback_candidate_count":3,"dynamics_translation_writeback_target_count":2,"dynamics_stretch_translation_writeback_group_count":1,"dynamics_stretch_translation_writeback_target_group_count":1,"dynamics_grabbing_enabled_group_count":5,"dynamics_posing_enabled_group_count":4,"dynamics_contact_count":3,"dynamics_contact_parameter_declaration_count":2,"dynamics_contact_probe_count":1,"dynamics_contact_probe_would_emit_count":1,"dynamics_contact_parameter_emission_count":1,"dynamics_contact_parameter_emitted_count":1,"dynamics_contact_parameter_reset_to_zero_count":0,"dynamics_constraint_ref_count":2,"runtime_parameter_definitions":[{{"name":"Hat","owner_keys":["action:hat:on"],"source_kinds":["action_condition"],"value_samples":[1.0],"current_value":1.0}}],"runtime_parameter_conflicts":[{{"name":"Hat","reason":"contact_transient_overlaps_action_parameter","owner_keys":["action:hat:on","contact:hand"],"source_kinds":["action_condition","contact_receiver"],"value_samples":[0.0,1.0]}}],"runtime_actions":[{{"action_id":"hat:on","condition_parameter_names":["Hat"],"current_condition_state":"active"}}],"runtime_action_target_write_collisions":[{{"target_kind":"node_visibility","target_key":"Root/Hat","owner_keys":["action:hat:on","action:hat:off"],"action_ids":["hat:on","hat:off"],"writes":[]}}],"runtime_action_restore_readiness":[{{"owner_key":"action:hat:on","action_id":"hat:on","effect_kind":"node_visibility","target_kind":"node_visibility","target_key":"Root/Hat","restore_target":true,"current_value_available":true,"current_value":true,"baseline_required":true,"ready":false,"reason":"baseline_not_captured"}}],"runtime_action_restore_baseline_candidates":[{{"owner_key":"action:hat:on","action_id":"hat:on","effect_kind":"node_visibility","target_kind":"node_visibility","target_key":"Root/Hat","baseline_value":true}}],"runtime_action_restore_baseline_capture_plan":[{{"owner_key":"action:hat:on","target_kind":"node_visibility","target_key":"Root/Hat","baseline_value":true,"source_action_ids":["hat:on"],"source_effect_kinds":["node_visibility"]}}],"runtime_action_restore_apply_plan":[{{"owner_key":"action:hat:on","action_id":"hat:on","condition_state":"inactive","target_kind":"node_visibility","target_key":"Root/Hat","baseline_value":true,"current_value_available":true,"current_value":false,"ready":true,"reason":"ready"}}],"menu_wardrobe_candidates":[{{"menu_path":["Wardrobe"],"menu_label":"Wardrobe","action_id":"wardrobe:field_drape","wardrobe_set_id":"field_drape","match_kind":"condition","inverted":false}}],"contact_parameter_declarations":[{{"owner_key":"contact:hand","node":1,"parameter":"ContactHand"}}],"contact_parameter_emission_enabled":true,"contact_parameter_emissions":[{{"owner_key":"contact:hand","receiver_index":0,"receiver_node":1,"parameter":"ContactHand","value":1.0,"emitted":true,"sender_source_ids":["contact:sender"]}}],"contact_probes":[{{"index":0,"would_emit":true}}],"dynamics_groups":[{{"index":0,"source_id":"physbone:hair"}}],"dynamics_interaction_hooks":[{{"group_index":0,"source_id":"physbone:hair","parameter":"HairPB","suffix_parameters":["HairPB_IsGrabbed"],"metadata_only":true}}],"dynamics_colliders":[{{"index":0,"node_path":"root/collider"}}],"dynamics_constraint_refs":[{{"index":0,"source_id":"constraint:parent"}}],"dynamics_warnings":["6 dynamics groups carry stretch limits; targetless stretch groups remain metadata-only in the current solver"],"note":null}}"#
			)
			.unwrap();
		});

		let telemetry = read_runtime_telemetry(address).unwrap();
		server.join().unwrap();

		assert!(telemetry.connected);
		assert_eq!(telemetry.uptime_secs, 7);
		assert_eq!(telemetry.surface_width, Some(800));
		assert_eq!(telemetry.surface_height, Some(600));
		assert_eq!(telemetry.aa.as_deref(), Some("smaa"));
		assert_eq!(telemetry.texture_resolution_limit.as_deref(), Some("4k"));
		assert_eq!(telemetry.texture_compression.as_deref(), Some("auto"));
		assert_eq!(telemetry.processed_texture_cache, Some(true));
		assert_eq!(telemetry.texture_summary.as_ref().map(|summary| summary.image_count), Some(3));
		assert_eq!(telemetry.fps, Some(59.5));
		assert_eq!(telemetry.dynamics_group_count, 9);
		assert_eq!(telemetry.dynamics_limit_group_count, 8);
		assert_eq!(telemetry.dynamics_angle_limit_group_count, 7);
		assert_eq!(telemetry.dynamics_stretch_limit_group_count, 6);
		assert_eq!(telemetry.dynamics_rotation_translation_writeback_group_count, 2);
		assert_eq!(telemetry.dynamics_translation_writeback_candidate_count, 3);
		assert_eq!(telemetry.dynamics_translation_writeback_target_count, 2);
		assert_eq!(telemetry.dynamics_stretch_translation_writeback_group_count, 1);
		assert_eq!(telemetry.dynamics_stretch_translation_writeback_target_group_count, 1);
		assert_eq!(telemetry.dynamics_grabbing_enabled_group_count, 5);
		assert_eq!(telemetry.dynamics_posing_enabled_group_count, 4);
		assert_eq!(telemetry.dynamics_contact_count, 3);
		assert_eq!(telemetry.dynamics_contact_parameter_declaration_count, 2);
		assert_eq!(telemetry.dynamics_contact_probe_count, 1);
		assert_eq!(telemetry.dynamics_contact_probe_would_emit_count, 1);
		assert_eq!(telemetry.dynamics_contact_parameter_emission_count, 1);
		assert_eq!(telemetry.dynamics_contact_parameter_emitted_count, 1);
		assert_eq!(telemetry.dynamics_contact_parameter_reset_to_zero_count, 0);
		assert_eq!(telemetry.dynamics_constraint_ref_count, 2);
		assert_eq!(
			telemetry
				.runtime_parameter_definitions
				.first()
				.and_then(|value| value.get("name"))
				.and_then(serde_json::Value::as_str),
			Some("Hat")
		);
		assert_eq!(
			telemetry
				.runtime_parameter_conflicts
				.first()
				.and_then(|value| value.get("reason"))
				.and_then(serde_json::Value::as_str),
			Some("contact_transient_overlaps_action_parameter")
		);
		assert_eq!(
			telemetry
				.runtime_actions
				.first()
				.and_then(|value| value.get("current_condition_state"))
				.and_then(serde_json::Value::as_str),
			Some("active")
		);
		assert_eq!(
			telemetry
				.runtime_action_target_write_collisions
				.first()
				.and_then(|value| value.get("target_key"))
				.and_then(serde_json::Value::as_str),
			Some("Root/Hat")
		);
		assert_eq!(
			telemetry
				.runtime_action_restore_readiness
				.first()
				.and_then(|value| value.get("reason"))
				.and_then(serde_json::Value::as_str),
			Some("baseline_not_captured")
		);
		assert_eq!(
			telemetry
				.runtime_action_restore_baseline_candidates
				.first()
				.and_then(|value| value.get("baseline_value"))
				.and_then(serde_json::Value::as_bool),
			Some(true)
		);
		assert_eq!(
			telemetry
				.runtime_action_restore_baseline_capture_plan
				.first()
				.and_then(|value| value.get("target_key"))
				.and_then(serde_json::Value::as_str),
			Some("Root/Hat")
		);
		assert_eq!(
			telemetry
				.runtime_action_restore_apply_plan
				.first()
				.and_then(|value| value.get("ready"))
				.and_then(serde_json::Value::as_bool),
			Some(true)
		);
		assert_eq!(
			telemetry
				.menu_wardrobe_candidates
				.first()
				.and_then(|value| value.get("wardrobe_set_id"))
				.and_then(serde_json::Value::as_str),
			Some("field_drape")
		);
		assert_eq!(
			telemetry
				.contact_parameter_declarations
				.first()
				.and_then(|value| value.get("parameter"))
				.and_then(serde_json::Value::as_str),
			Some("ContactHand")
		);
		assert!(telemetry.contact_parameter_emission_enabled);
		assert_eq!(
			telemetry
				.contact_parameter_emissions
				.first()
				.and_then(|value| value.get("value"))
				.and_then(serde_json::Value::as_f64),
			Some(1.0)
		);
		assert_eq!(
			telemetry
				.dynamics_interaction_hooks
				.first()
				.and_then(|value| value.get("parameter"))
				.and_then(serde_json::Value::as_str),
			Some("HairPB")
		);
		assert_eq!(
			telemetry
				.dynamics_colliders
				.first()
				.and_then(|value| value.get("node_path"))
				.and_then(serde_json::Value::as_str),
			Some("root/collider")
		);
		assert_eq!(
			telemetry.dynamics_warnings.first().map(String::as_str),
			Some("6 dynamics groups carry stretch limits; targetless stretch groups remain metadata-only in the current solver")
		);
	}

	#[test]
	fn vrm_metadata_reads_vrm1_usage_terms() {
		let path = std::env::temp_dir().join(format!(
			"un-avatar-vrm-meta-test-{}-{}.gltf",
			std::process::id(),
			Instant::now().elapsed().as_nanos()
		));
		fs::write(
			&path,
			r#"{
  "asset": { "version": "2.0" },
  "extensions": {
    "VRMC_vrm": {
      "specVersion": "1.0",
      "meta": {
        "name": "Metadata Test Avatar",
        "version": "1.2.3",
        "thumbnailImage": 0,
        "authors": ["Author A", "Author B"],
        "copyrightInformation": "Copyright Test",
        "contactInformation": "contact@example.invalid",
        "references": ["https://example.invalid/ref"],
        "thirdPartyLicenses": "Third party license text",
        "avatarPermission": "OnlyAuthor",
        "commercialUsage": "PersonalNonProfit",
        "allowRedistribution": false,
        "otherLicenseUrl": "https://example.invalid/license",
        "otherPermissionUrl": "https://example.invalid/permission"
      },
      "blendShapeMaster": {
        "blendShapeGroups": [
          { "name": "EyeBlink_L", "presetName": "unknown" },
          { "name": "EyeBlink_R", "presetName": "unknown" },
          { "name": "Joy", "presetName": "joy" }
        ]
      }
    }
  },
  "images": [
    {
      "mimeType": "image/png",
      "uri": "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII="
    }
  ]
}"#,
		)
		.unwrap();
		let metadata = read_vrm_metadata(path.display().to_string(), None).unwrap().unwrap();
		let _ = fs::remove_file(&path);
		assert_eq!(metadata.title.as_deref(), Some("Metadata Test Avatar"));
		assert_eq!(metadata.vrm_format, "VRM 1.0");
		assert_eq!(metadata.spec_version, "1.0");
		assert_eq!(metadata.authors, vec!["Author A".to_string(), "Author B".to_string()]);
		assert_eq!(metadata.third_party_licenses.as_deref(), Some("Third party license text"));
		assert_eq!(metadata.other_permission_url.as_deref(), Some("https://example.invalid/permission"));
		assert!(metadata
			.thumbnail_data_url
			.as_deref()
			.is_some_and(|value| value.starts_with("data:image/png;base64,")));
		assert!(metadata
			.technical_stats
			.iter()
			.any(|field| field.label == "Texture RAM" && field.value == "4 B RGBA"));
		assert!(metadata
			.technical_stats
			.iter()
			.any(|field| field.label == "Expressions" && field.value == "3"));
		assert!(metadata
			.technical_stats
			.iter()
			.any(|field| field.label == "PerfectSync" && field.value == "partial (2/52)"));
		assert!(metadata
			.permissions
			.iter()
			.any(|field| field.label == "Redistribution" && field.value == "false"));
	}

	#[test]
	fn thumbnail_data_uri_parser_accepts_supported_lossless_cache_inputs() {
		let (mime, encoded) = data_image_base64_parts("data:image/webp;name=thumb;base64,AAAA").unwrap();
		assert_eq!(mime, "image/webp");
		assert_eq!(encoded, "AAAA");
		assert!(data_image_base64_parts("data:image/svg+xml;base64,AAAA").is_none());
		assert!(data_image_base64_parts("data:image/png,AAAA").is_none());
	}

	#[test]
	fn profile_icon_thumbnail_cache_encodes_resized_webp() {
		let source = image::RgbaImage::from_pixel(512, 384, image::Rgba([255, 128, 96, 220]));
		let mut png = Vec::new();
		image::DynamicImage::ImageRgba8(source)
			.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
			.unwrap();
		let webp = encode_profile_icon_thumbnail_webp(&png).unwrap();
		assert!(webp.starts_with(b"RIFF"));
		assert_eq!(&webp[8..12], b"WEBP");
		let decoded = image::load_from_memory(&webp).unwrap();
		assert!(decoded.width() <= PROFILE_ICON_THUMBNAIL_MAX_DIMENSION);
		assert!(decoded.height() <= PROFILE_ICON_THUMBNAIL_MAX_DIMENSION);
	}

	#[test]
	fn thumbnail_protocol_accepts_only_cache_file_names() {
		assert_eq!(
			thumbnail_protocol_file_name("/new-avatar-avatar-thumbnail.webp").as_deref(),
			Some("new-avatar-avatar-thumbnail.webp")
		);
		assert_eq!(
			thumbnail_protocol_file_name("/model%201-avatar-thumbnail.webp").as_deref(),
			Some("model 1-avatar-thumbnail.webp")
		);
		assert_eq!(percent_decode_utf8("model%201.webp").as_deref(), Some("model 1.webp"));
		assert!(thumbnail_protocol_file_name("/../secret.webp").is_none());
		assert!(thumbnail_protocol_file_name("/nested%2Fsecret.webp").is_none());
		assert!(thumbnail_protocol_file_name("/icon.png").is_none());
	}

	#[test]
	fn perfect_sync_detection_uses_arkit_52_names() {
		let names = [
			"browdownleft",
			"browdownright",
			"browinnerup",
			"browouterupleft",
			"browouterupright",
			"cheekpuff",
			"cheeksquintleft",
			"cheeksquintright",
			"eyeblinkleft",
			"eyeblinkright",
			"eyelookdownleft",
			"eyelookdownright",
			"eyelookinleft",
			"eyelookinright",
			"eyelookoutleft",
			"eyelookoutright",
			"eyelookupleft",
			"eyelookupright",
			"eyesquintleft",
			"eyesquintright",
			"eyewideleft",
			"eyewideright",
			"jawforward",
			"jawleft",
			"jawopen",
			"jawright",
			"mouthclose",
			"mouthdimpleleft",
			"mouthdimpleright",
			"mouthfrownleft",
			"mouthfrownright",
			"mouthfunnel",
			"mouthleft",
			"mouthlowerdownleft",
			"mouthlowerdownright",
			"mouthpressleft",
			"mouthpressright",
			"mouthpucker",
			"mouthright",
			"mouthrolllower",
			"mouthrollupper",
			"mouthshruglower",
			"mouthshrugupper",
			"mouthsmileleft",
			"mouthsmileright",
			"mouthstretchleft",
			"mouthstretchright",
			"mouthupperupleft",
			"mouthupperupright",
			"nosesneerleft",
			"nosesneerright",
			"tongueout",
		]
		.into_iter()
		.map(str::to_string)
		.collect();
		assert_eq!(perfect_sync_hit_count(&names), 52);
	}

	#[test]
	fn runtime_status_stream_updates_cache() {
		use zenoh::Wait as _;
		let key = format!(
			"un-avatar/test/runtime-status/{}",
			std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.unwrap()
				.as_nanos()
		);
		let (cache, stop) = spawn_runtime_status_stream(key.clone(), 1, String::new());
		let session = zenoh::open(zenoh::Config::default()).wait().unwrap();
		let started = Instant::now();
		loop {
			let _ = session.put(
				format!("{key}/status"),
				r#"{"connected":true,"protocol":"zenoh-json-v1","control_capabilities":["shutdown"],"uptime_secs":9,"fps":60.0,"cpu_ms":1.0,"gpu_ms":2.0,"ram_mb":null,"surface_width":640,"surface_height":360,"aa":"msaa","texture_resolution_limit":"off","texture_compression":"source","processed_texture_cache":true,"texture_summary":{"image_count":1,"uploaded_mip_bytes":512},"spout_available":true,"spout_enabled":false,"spout_name":null,"spout_width":null,"spout_height":null,"note":null}"#,
			).wait();
			if let Some(telemetry) = cache.lock().unwrap().telemetry.clone() {
				stop.store(true, Ordering::Release);
				assert_eq!(telemetry.uptime_secs, 9);
				assert_eq!(telemetry.surface_width, Some(640));
				assert_eq!(telemetry.protocol.as_deref(), Some("zenoh-json-v1"));
				assert_eq!(telemetry.aa.as_deref(), Some("msaa"));
				assert_eq!(
					telemetry.texture_summary.as_ref().map(|summary| summary.uploaded_mip_bytes),
					Some(512)
				);
				return;
			}
			assert!(
				started.elapsed() < Duration::from_secs(2),
				"runtime status stream cache was not updated"
			);
			thread::sleep(Duration::from_millis(20));
		}
	}

	#[test]
	fn seed_profile_reports_seed_storage() {
		let setting = read_avatar_setting(&repo_root().join("profiles").join("main.toml"), ProfileStorage::Seed).unwrap();
		assert_eq!(setting.id, "main-avatar");
		assert_eq!(setting.storage, ProfileStorage::Seed);
		assert_eq!(setting.icon_path.as_deref(), None);
		assert!(!setting.allow_multiple_renderers);
		assert_eq!(setting.aa, "off");
		assert_eq!(setting.texture_resolution_limit, "off");
		assert_eq!(setting.texture_compression, "source");
		assert_eq!(setting.mipmap_filter, "mitchell");
		assert_eq!(setting.render_backend, "dx12");
		assert_eq!(setting.block_compression_encoder, "gpu");
		assert_eq!(setting.block_compression_cpu_threads, 4);
		assert!(setting.processed_texture_cache);
		assert_eq!(setting.audio_link_source, "none");
		assert_eq!(setting.audio_link_input_device_id, None);
		assert_eq!(setting.audio_link_input_device_name_hint, None);
		assert!(setting.transparent);
		assert!(!setting.input_passthrough);
	}

	#[test]
	fn renderer_window_icon_falls_back_to_renderer_artwork() {
		let setting = read_avatar_setting(&repo_root().join("profiles").join("main.toml"), ProfileStorage::Seed).unwrap();
		assert_eq!(
			resolve_renderer_window_icon_path(&setting),
			Some(repo_root().join("assets").join("brand").join("un-avatar-artwork-renderer.png"))
		);
	}

	#[test]
	fn renderer_window_icon_prefers_profile_icon() {
		let setting = read_avatar_setting(&repo_root().join("profiles").join("debug.toml"), ProfileStorage::Seed).unwrap();
		let icon = resolve_renderer_window_icon_path(&AvatarSetting {
			icon_path: Some("assets/brand/un-avatar-artwork-supervisor.png".to_string()),
			..setting
		});
		assert_eq!(
			icon,
			Some(repo_root().join("assets").join("brand").join("un-avatar-artwork-supervisor.png"))
		);
	}

	#[test]
	fn editable_manifest_parser_accepts_root_keys_before_tables() {
		let text = r#"clear_color = [
    0.12,
    0.14,
    0.18,
    1.0,
]
title = "New Avatar"

[profile]
id = "new-avatar"
display_name = "New Avatar"
"#;
		let manifest = parse_manifest_value(text, Path::new("new-avatar.toml")).unwrap();
		assert_eq!(manifest.get("title").and_then(toml::Value::as_str), Some("New Avatar"));
	}

	#[test]
	fn wardrobe_setting_writes_root_set_and_empty_removes_it() {
		let setting = read_avatar_setting(&repo_root().join("profiles").join("main.toml"), ProfileStorage::Seed).unwrap();
		let mut manifest = parse_manifest_value(
			r#"title = "Test"
wardrobe_set = "old"

[profile]
id = "test"
"#,
			Path::new("test.toml"),
		)
		.unwrap();

		apply_avatar_setting_value(&mut manifest, &setting, "wardrobe_set", serde_json::json!("noble1")).unwrap();
		assert_eq!(manifest.get("wardrobe_set").and_then(toml::Value::as_str), Some("noble1"));

		apply_avatar_setting_value(&mut manifest, &setting, "wardrobe_set", serde_json::json!("")).unwrap();
		assert!(manifest.get("wardrobe_set").is_none());
	}

	#[test]
	fn dynamics_enable_all_on_launch_setting_round_trips_manifest_value() {
		let setting = read_avatar_setting(&repo_root().join("profiles").join("main.toml"), ProfileStorage::Seed).unwrap();
		assert!(!setting.dynamics_enable_all_on_launch);
		let mut manifest = parse_manifest_value(
			r#"title = "Test"

[profile]
id = "test"
"#,
			Path::new("test.toml"),
		)
		.unwrap();

		apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"physics.dynamics.enable_all_on_launch",
			serde_json::json!(true),
		)
		.unwrap();
		assert_eq!(
			manifest
				.get("physics")
				.and_then(toml::Value::as_table)
				.and_then(|physics| physics.get("dynamics"))
				.and_then(toml::Value::as_table)
				.and_then(|dynamics| dynamics.get("enable_all_on_launch"))
				.and_then(toml::Value::as_bool),
			Some(true)
		);

		let path = std::env::temp_dir().join(format!(
			"un-avatar-dynamics-enable-all-test-{}-{}.toml",
			std::process::id(),
			Instant::now().elapsed().as_nanos()
		));
		fs::write(&path, toml::to_string(&manifest).unwrap()).unwrap();
		let parsed = read_avatar_setting(&path, ProfileStorage::User).unwrap();
		let _ = fs::remove_file(path);
		assert!(parsed.dynamics_enable_all_on_launch);
	}

	#[test]
	fn unphysics_enabled_setting_writes_v2_dynamics_manifest_value() {
		let setting = read_avatar_setting(&repo_root().join("profiles").join("main.toml"), ProfileStorage::Seed).unwrap();
		let mut manifest = parse_manifest_value(
			r#"title = "Test"
spring_bones = false

[profile]
id = "test"
"#,
			Path::new("test.toml"),
		)
		.unwrap();

		apply_avatar_setting_value(&mut manifest, &setting, "spring_bones", serde_json::json!(true)).unwrap();
		assert_eq!(
			manifest
				.get("physics")
				.and_then(toml::Value::as_table)
				.and_then(|physics| physics.get("dynamics"))
				.and_then(toml::Value::as_table)
				.and_then(|dynamics| dynamics.get("enabled"))
				.and_then(toml::Value::as_bool),
			Some(true)
		);
		assert_eq!(manifest.get("spring_bones").and_then(toml::Value::as_bool), Some(false));
	}

	#[test]
	fn read_avatar_setting_ignores_legacy_spring_bones_for_unphysics_enabled() {
		let path = std::env::temp_dir().join(format!(
			"un-avatar-unphysics-enabled-test-{}-{}.toml",
			std::process::id(),
			Instant::now().elapsed().as_nanos()
		));
		fs::write(
			&path,
			r#"title = "Test"
spring_bones = false

[profile]
id = "test"

[physics.dynamics]
enabled = true
"#,
		)
		.unwrap();
		let parsed = read_avatar_setting(&path, ProfileStorage::User).unwrap();
		let _ = fs::remove_file(path);
		assert!(parsed.spring_bones);
	}

	#[test]
	fn read_avatar_setting_requires_v2_unphysics_enabled_to_disable_dynamics() {
		let path = std::env::temp_dir().join(format!(
			"un-avatar-unphysics-ignores-legacy-test-{}-{}.toml",
			std::process::id(),
			Instant::now().elapsed().as_nanos()
		));
		fs::write(
			&path,
			r#"title = "Test"
spring_bones = false

[profile]
id = "test"
"#,
		)
		.unwrap();
		let parsed = read_avatar_setting(&path, ProfileStorage::User).unwrap();
		let _ = fs::remove_file(path);
		assert!(parsed.spring_bones);
	}

	#[test]
	fn contact_parameter_emission_setting_round_trips_manifest_value() {
		let setting = read_avatar_setting(&repo_root().join("profiles").join("main.toml"), ProfileStorage::Seed).unwrap();
		assert!(!setting.contact_parameter_emission);
		let mut manifest = parse_manifest_value(
			r#"title = "Test"

[profile]
id = "test"
"#,
			Path::new("test.toml"),
		)
		.unwrap();

		apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"physics.contacts.parameter_emission",
			serde_json::json!(true),
		)
		.unwrap();
		assert_eq!(
			manifest
				.get("physics")
				.and_then(toml::Value::as_table)
				.and_then(|physics| physics.get("contacts"))
				.and_then(toml::Value::as_table)
				.and_then(|contacts| contacts.get("parameter_emission"))
				.and_then(toml::Value::as_bool),
			Some(true)
		);

		let path = std::env::temp_dir().join(format!(
			"un-avatar-contact-emission-test-{}-{}.toml",
			std::process::id(),
			Instant::now().elapsed().as_nanos()
		));
		fs::write(&path, toml::to_string(&manifest).unwrap()).unwrap();
		let parsed = read_avatar_setting(&path, ProfileStorage::User).unwrap();
		let _ = fs::remove_file(path);
		assert!(parsed.contact_parameter_emission);
	}

	#[test]
	fn launch_control_commands_enable_all_dynamics_only_when_opted_in() {
		let mut setting = read_avatar_setting(&repo_root().join("profiles").join("main.toml"), ProfileStorage::Seed).unwrap();
		setting.dynamics_enable_all_on_launch = false;
		assert!(renderer_launch_control_commands(&setting).is_empty());

		setting.dynamics_enable_all_on_launch = true;
		let commands = renderer_launch_control_commands(&setting);
		assert_eq!(commands.len(), 1);
		assert_eq!(
			serde_json::to_string(&commands[0]).unwrap(),
			r#"{"command":"set_all_dynamics_enabled","enabled":true}"#
		);
	}

	#[test]
	fn audio_link_setting_updates_write_expected_manifest_values() {
		let setting = read_avatar_setting(&repo_root().join("profiles").join("main.toml"), ProfileStorage::Seed).unwrap();
		let mut manifest = parse_manifest_value(
			r#"title = "Test"

[profile]
id = "test"
"#,
			Path::new("test.toml"),
		)
		.unwrap();

		apply_avatar_setting_value(&mut manifest, &setting, "audio_link.source", serde_json::json!("input_device")).unwrap();
		apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"audio_link.input_device_id",
			serde_json::json!("cpal:device-1"),
		)
		.unwrap();
		apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"audio_link.input_device_name_hint",
			serde_json::json!("Main Mix"),
		)
		.unwrap();

		let audio_link = manifest
			.get("audio_link")
			.and_then(toml::Value::as_table)
			.expect("audio_link table");
		assert_eq!(audio_link.get("source").and_then(toml::Value::as_str), Some("input_device"));
		assert_eq!(
			audio_link.get("input_device_id").and_then(toml::Value::as_str),
			Some("cpal:device-1")
		);
		assert_eq!(
			audio_link.get("input_device_name_hint").and_then(toml::Value::as_str),
			Some("Main Mix")
		);

		let invalid =
			apply_avatar_setting_value(&mut manifest, &setting, "audio_link.source", serde_json::json!("system_mix")).unwrap_err();
		assert!(invalid.contains("expected none or input_device"));
	}

	#[test]
	fn read_unavatar_wardrobe_options_reads_sets_and_hides_base_duplicate() {
		let dir = std::env::temp_dir().join(format!("un-avatar-wardrobe-options-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		fs::create_dir_all(&dir).unwrap();
		let avatar_path = dir.join("avatar.unavatar");
		fs::write(
			&avatar_path,
			r#"{
				"asset": {"version": "2.0"},
				"extensions": {
					"UN_avatar": {
						"wardrobe": {
							"baseSet": "base",
							"sets": [
								{"id": "base", "displayName": "Base"},
								{"id": "original", "displayName": "Original"},
								{"id": "noble13", "name": "Noble 13"}
							]
						}
					}
				}
			}"#,
		)
		.unwrap();

		let options = read_unavatar_wardrobe_options(avatar_path.display().to_string(), None).unwrap();

		assert!(options.available);
		assert_eq!(options.base_label, "Base");
		assert_eq!(options.error, None);
		assert_eq!(options.sets.len(), 2);
		assert_eq!(options.sets[0].id, "original");
		assert_eq!(options.sets[0].name, "Original");
		assert_eq!(options.sets[1].id, "noble13");
		assert_eq!(options.sets[1].name, "Noble 13");
		let _ = fs::remove_dir_all(&dir);
	}

	#[test]
	fn render_quality_setting_updates_write_expected_manifest_values() {
		let setting = read_avatar_setting(&repo_root().join("profiles").join("main.toml"), ProfileStorage::Seed).unwrap();
		let mut manifest = parse_manifest_value(
			r#"title = "Test"
aa = "off"

[profile]
id = "test"

[render_quality]
aa = "off"
texture_resolution_limit = "off"
texture_compression = "source"
mipmap_filter = "mitchell"
processed_texture_cache = true
"#,
			Path::new("test.toml"),
		)
		.unwrap();

		apply_avatar_setting_value(&mut manifest, &setting, "render_quality.aa", serde_json::json!("msaa")).unwrap();
		apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"render_quality.texture_resolution_limit",
			serde_json::json!("2k"),
		)
		.unwrap();
		apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"render_quality.texture_compression",
			serde_json::json!("balanced"),
		)
		.unwrap();
		apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"render_quality.mipmap_filter",
			serde_json::json!("lanczos3"),
		)
		.unwrap();
		apply_avatar_setting_value(&mut manifest, &setting, "render_quality.render_backend", serde_json::json!("dx12")).unwrap();
		apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"render_quality.block_compression_encoder",
			serde_json::json!("cpu"),
		)
		.unwrap();
		apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"render_quality.block_compression_cpu_threads",
			serde_json::json!(8),
		)
		.unwrap();
		apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"render_quality.processed_texture_cache",
			serde_json::json!(false),
		)
		.unwrap();
		apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"render_quality.skin_tone_matching",
			serde_json::json!(true),
		)
		.unwrap();

		let render_quality = manifest
			.get("render_quality")
			.and_then(toml::Value::as_table)
			.expect("render_quality table");
		assert_eq!(manifest.get("aa").and_then(toml::Value::as_str), Some("msaa"));
		assert_eq!(render_quality.get("aa").and_then(toml::Value::as_str), Some("msaa"));
		assert_eq!(
			render_quality.get("texture_resolution_limit").and_then(toml::Value::as_str),
			Some("2k")
		);
		assert_eq!(
			render_quality.get("texture_compression").and_then(toml::Value::as_str),
			Some("balanced")
		);
		assert_eq!(render_quality.get("mipmap_filter").and_then(toml::Value::as_str), Some("lanczos3"));
		assert_eq!(render_quality.get("render_backend").and_then(toml::Value::as_str), Some("dx12"));
		assert_eq!(
			render_quality.get("block_compression_encoder").and_then(toml::Value::as_str),
			Some("cpu")
		);
		assert_eq!(
			render_quality
				.get("block_compression_cpu_threads")
				.and_then(toml::Value::as_integer),
			Some(8)
		);
		assert_eq!(
			render_quality.get("processed_texture_cache").and_then(toml::Value::as_bool),
			Some(false)
		);
		assert_eq!(render_quality.get("skin_tone_matching").and_then(toml::Value::as_bool), Some(true));
	}

	#[test]
	fn render_quality_setting_rejects_invalid_policy_values() {
		let setting = read_avatar_setting(&repo_root().join("profiles").join("main.toml"), ProfileStorage::Seed).unwrap();
		let mut manifest = parse_manifest_value(
			r#"title = "Test"

[profile]
id = "test"
"#,
			Path::new("test.toml"),
		)
		.unwrap();

		let invalid_limit = apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"render_quality.texture_resolution_limit",
			serde_json::json!("16k"),
		)
		.unwrap_err();
		assert!(invalid_limit.contains("off, auto, 8k, 4k, 2k, 1k"));

		let invalid_compression = apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"render_quality.texture_compression",
			serde_json::json!("lossless"),
		)
		.unwrap_err();
		assert!(invalid_compression.contains("source, balanced, memory, compat"));
		let invalid_mipmap = apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"render_quality.mipmap_filter",
			serde_json::json!("nearest"),
		)
		.unwrap_err();
		assert!(invalid_mipmap.contains("box2x2, bilinear, bicubic, catmull_rom, lanczos3, mitchell"));
		let invalid_backend =
			apply_avatar_setting_value(&mut manifest, &setting, "render_quality.render_backend", serde_json::json!("metal")).unwrap_err();
		assert!(invalid_backend.contains("vulkan, dx12, auto"));

		let invalid_encoder = apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"render_quality.block_compression_encoder",
			serde_json::json!("magic"),
		)
		.unwrap_err();
		assert!(invalid_encoder.contains("gpu, cpu"));
	}

	#[test]
	fn spring_bone_category_override_setting_updates_manifest_values() {
		let setting = read_avatar_setting(&repo_root().join("profiles").join("main.toml"), ProfileStorage::Seed).unwrap();
		let mut manifest = parse_manifest_value(
			r#"title = "Test"

[profile]
id = "test"
"#,
			Path::new("test.toml"),
		)
		.unwrap();

		apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"physics.spring_bone.overrides.ears.mode",
			serde_json::json!("override_xpbd"),
		)
		.unwrap();
		apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"physics.spring_bone.overrides.ears.xpbd_compliance",
			serde_json::json!(0.015),
		)
		.unwrap();
		apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"physics.spring_bone.overrides.ears.constraint_iterations",
			serde_json::json!(8),
		)
		.unwrap();

		let overrides = manifest
			.get("physics")
			.and_then(|v| v.get("spring_bone"))
			.and_then(|v| v.get("overrides"))
			.and_then(toml::Value::as_array)
			.expect("overrides");
		assert_eq!(overrides.len(), 1);
		let item = overrides[0].as_table().expect("override item");
		assert_eq!(item.get("category").and_then(toml::Value::as_str), Some("ears"));
		assert_eq!(item.get("solver").and_then(toml::Value::as_str), Some("xpbd"));
		assert!((item.get("xpbd_compliance").and_then(toml::Value::as_float).unwrap_or_default() - 0.015).abs() < 1e-6);
		assert_eq!(item.get("constraint_iterations").and_then(toml::Value::as_integer), Some(8));

		apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"physics.spring_bone.overrides.ears.preset",
			serde_json::json!("snappy"),
		)
		.unwrap();
		let overrides = manifest
			.get("physics")
			.and_then(|v| v.get("spring_bone"))
			.and_then(|v| v.get("overrides"))
			.and_then(toml::Value::as_array)
			.expect("overrides");
		let item = overrides[0].as_table().expect("override item");
		assert_eq!(item.get("solver").and_then(toml::Value::as_str), Some("xpbd"));
		assert_eq!(item.get("constraint_iterations").and_then(toml::Value::as_integer), Some(7));
		assert!((item.get("damping_half_life_ms").and_then(toml::Value::as_float).unwrap_or_default() - 55.0).abs() < 1e-6);

		apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"physics.spring_bone.overrides.ears.mode",
			serde_json::json!("authored"),
		)
		.unwrap();
		let overrides = manifest
			.get("physics")
			.and_then(|v| v.get("spring_bone"))
			.and_then(|v| v.get("overrides"))
			.and_then(toml::Value::as_array)
			.expect("overrides");
		assert!(overrides.is_empty());
	}

	#[test]
	fn avatar_outline_setting_updates_write_expected_manifest_values() {
		let setting = read_avatar_setting(&repo_root().join("profiles").join("main.toml"), ProfileStorage::Seed).unwrap();
		let mut manifest = parse_manifest_value(
			r#"title = "Test"

[profile]
id = "test"
"#,
			Path::new("test.toml"),
		)
		.unwrap();

		apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"effects.avatar.outline.policy",
			serde_json::json!("override"),
		)
		.unwrap();
		apply_avatar_setting_value(&mut manifest, &setting, "effects.avatar.outline.type", serde_json::json!("double")).unwrap();
		apply_avatar_setting_value(&mut manifest, &setting, "effects.avatar.outline.width", serde_json::json!(0.004)).unwrap();
		apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"effects.avatar.outline.color",
			serde_json::json!([0.02, 0.01, 0.03]),
		)
		.unwrap();
		apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"effects.avatar.outline.lighting_mix",
			serde_json::json!(0.25),
		)
		.unwrap();
		apply_avatar_setting_value(&mut manifest, &setting, "effects.avatar.outline.roundness", serde_json::json!(0.75)).unwrap();

		let outline = manifest
			.get("effects")
			.and_then(toml::Value::as_table)
			.and_then(|effects| effects.get("avatar"))
			.and_then(toml::Value::as_table)
			.and_then(|avatar| avatar.get("outline"))
			.and_then(toml::Value::as_table)
			.expect("effects.avatar.outline table");
		assert_eq!(outline.get("policy").and_then(toml::Value::as_str), Some("override"));
		assert_eq!(outline.get("type").and_then(toml::Value::as_str), Some("double"));
		assert!((outline.get("width").and_then(toml::Value::as_float).unwrap_or_default() - 0.004).abs() < 1e-6);
		assert!((outline.get("lighting_mix").and_then(toml::Value::as_float).unwrap_or_default() - 0.25).abs() < 1e-6);
		assert!((outline.get("roundness").and_then(toml::Value::as_float).unwrap_or_default() - 0.75).abs() < 1e-6);
		let color = outline
			.get("color")
			.and_then(toml::Value::as_array)
			.map(|values| values.iter().filter_map(toml::Value::as_float).collect::<Vec<_>>())
			.expect("outline color");
		assert_eq!(color.len(), 3);
		assert!((color[0] - 0.02).abs() < 1e-6);
		assert!((color[1] - 0.01).abs() < 1e-6);
		assert!((color[2] - 0.03).abs() < 1e-6);
	}

	#[test]
	fn environment_color_setting_updates_write_expected_manifest_values() {
		let setting = read_avatar_setting(&repo_root().join("profiles").join("main.toml"), ProfileStorage::Seed).unwrap();
		let mut manifest = parse_manifest_value(
			r#"title = "Test"

[profile]
id = "test"
"#,
			Path::new("test.toml"),
		)
		.unwrap();

		apply_avatar_setting_value(&mut manifest, &setting, "environment.color.exposure", serde_json::json!(0.25)).unwrap();
		apply_avatar_setting_value(&mut manifest, &setting, "environment.color.contrast", serde_json::json!(1.2)).unwrap();
		apply_avatar_setting_value(&mut manifest, &setting, "environment.color.saturation", serde_json::json!(0.8)).unwrap();
		apply_avatar_setting_value(&mut manifest, &setting, "environment.color.look", serde_json::json!("film")).unwrap();
		apply_avatar_setting_value(&mut manifest, &setting, "environment.color.intensity", serde_json::json!(0.45)).unwrap();
		apply_avatar_setting_value(&mut manifest, &setting, "environment.color.temperature", serde_json::json!(0.2)).unwrap();
		apply_avatar_setting_value(&mut manifest, &setting, "environment.color.tint", serde_json::json!(-0.15)).unwrap();

		let color = manifest
			.get("environment")
			.and_then(toml::Value::as_table)
			.and_then(|environment| environment.get("color"))
			.and_then(toml::Value::as_table)
			.expect("environment.color table");
		assert!((color.get("exposure").and_then(toml::Value::as_float).unwrap_or_default() - 0.25).abs() < 1e-6);
		assert!((color.get("contrast").and_then(toml::Value::as_float).unwrap_or_default() - 1.2).abs() < 1e-6);
		assert!((color.get("saturation").and_then(toml::Value::as_float).unwrap_or_default() - 0.8).abs() < 1e-6);
		assert_eq!(color.get("look").and_then(toml::Value::as_str), Some("film"));
		assert!((color.get("intensity").and_then(toml::Value::as_float).unwrap_or_default() - 0.45).abs() < 1e-6);
		assert!((color.get("temperature").and_then(toml::Value::as_float).unwrap_or_default() - 0.2).abs() < 1e-6);
		assert!((color.get("tint").and_then(toml::Value::as_float).unwrap_or_default() + 0.15).abs() < 1e-6);
	}

	#[test]
	fn avatar_matcap_setting_updates_write_expected_manifest_values() {
		let setting = read_avatar_setting(&repo_root().join("profiles").join("main.toml"), ProfileStorage::Seed).unwrap();
		let mut manifest = parse_manifest_value(
			r#"title = "Test"

[profile]
id = "test"
"#,
			Path::new("test.toml"),
		)
		.unwrap();

		apply_avatar_setting_value(&mut manifest, &setting, "effects.avatar.matcap.scale", serde_json::json!(1.35)).unwrap();

		let matcap = manifest
			.get("effects")
			.and_then(toml::Value::as_table)
			.and_then(|effects| effects.get("avatar"))
			.and_then(toml::Value::as_table)
			.and_then(|avatar| avatar.get("matcap"))
			.and_then(toml::Value::as_table)
			.expect("effects.avatar.matcap table");
		assert!((matcap.get("scale").and_then(toml::Value::as_float).unwrap_or_default() - 1.35).abs() < 1e-6);
	}

	#[test]
	fn avatar_specular_setting_updates_write_expected_manifest_values() {
		let setting = read_avatar_setting(&repo_root().join("profiles").join("main.toml"), ProfileStorage::Seed).unwrap();
		let mut manifest = parse_manifest_value(
			r#"title = "Test"

[profile]
id = "test"
"#,
			Path::new("test.toml"),
		)
		.unwrap();

		apply_avatar_setting_value(&mut manifest, &setting, "effects.avatar.specular.enabled", serde_json::json!(true)).unwrap();
		apply_avatar_setting_value(&mut manifest, &setting, "effects.avatar.specular.intensity", serde_json::json!(0.5)).unwrap();
		apply_avatar_setting_value(&mut manifest, &setting, "effects.avatar.specular.power", serde_json::json!(32.0)).unwrap();

		let specular = manifest
			.get("effects")
			.and_then(toml::Value::as_table)
			.and_then(|effects| effects.get("avatar"))
			.and_then(toml::Value::as_table)
			.and_then(|avatar| avatar.get("specular"))
			.and_then(toml::Value::as_table)
			.expect("effects.avatar.specular table");
		assert_eq!(specular.get("enabled").and_then(toml::Value::as_bool), Some(true));
		assert!((specular.get("intensity").and_then(toml::Value::as_float).unwrap_or_default() - 0.5).abs() < 1e-6);
		assert!((specular.get("power").and_then(toml::Value::as_float).unwrap_or_default() - 32.0).abs() < 1e-6);
	}

	#[test]
	fn avatar_ambient_occlusion_setting_updates_write_expected_manifest_values() {
		let setting = read_avatar_setting(&repo_root().join("profiles").join("main.toml"), ProfileStorage::Seed).unwrap();
		let mut manifest = parse_manifest_value(
			r#"title = "Test"

[profile]
id = "test"
"#,
			Path::new("test.toml"),
		)
		.unwrap();

		apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"effects.avatar.ambient_occlusion.strength",
			serde_json::json!(1.4),
		)
		.unwrap();

		let ambient_occlusion = manifest
			.get("effects")
			.and_then(toml::Value::as_table)
			.and_then(|effects| effects.get("avatar"))
			.and_then(toml::Value::as_table)
			.and_then(|avatar| avatar.get("ambient_occlusion"))
			.and_then(toml::Value::as_table)
			.expect("effects.avatar.ambient_occlusion table");
		assert!(
			(ambient_occlusion
				.get("strength")
				.and_then(toml::Value::as_float)
				.unwrap_or_default()
				- 1.4)
				.abs() < 1e-6
		);
	}

	#[test]
	fn bloom_setting_updates_write_expected_manifest_values() {
		let setting = read_avatar_setting(&repo_root().join("profiles").join("main.toml"), ProfileStorage::Seed).unwrap();
		let mut manifest = parse_manifest_value(
			r#"title = "Test"

[profile]
id = "test"
"#,
			Path::new("test.toml"),
		)
		.unwrap();

		apply_avatar_setting_value(&mut manifest, &setting, "effects.post.bloom.enabled", serde_json::json!(true)).unwrap();
		apply_avatar_setting_value(&mut manifest, &setting, "effects.post.bloom.strength", serde_json::json!(0.4)).unwrap();
		apply_avatar_setting_value(&mut manifest, &setting, "effects.post.bloom.threshold", serde_json::json!(0.9)).unwrap();
		apply_avatar_setting_value(&mut manifest, &setting, "effects.post.bloom.radius", serde_json::json!(12.0)).unwrap();
		apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"effects.post.bloom.quality",
			serde_json::json!("high_quality"),
		)
		.unwrap();

		let bloom = manifest
			.get("effects")
			.and_then(toml::Value::as_table)
			.and_then(|effects| effects.get("post"))
			.and_then(toml::Value::as_table)
			.and_then(|post| post.get("bloom"))
			.and_then(toml::Value::as_table)
			.expect("effects.post.bloom table");
		assert_eq!(bloom.get("enabled").and_then(toml::Value::as_bool), Some(true));
		assert!((bloom.get("strength").and_then(toml::Value::as_float).unwrap_or_default() - 0.4).abs() < 1e-6);
		assert!((bloom.get("threshold").and_then(toml::Value::as_float).unwrap_or_default() - 0.9).abs() < 1e-6);
		assert!((bloom.get("radius").and_then(toml::Value::as_float).unwrap_or_default() - 12.0).abs() < 1e-6);
		assert_eq!(bloom.get("quality").and_then(toml::Value::as_str), Some("high_quality"));
	}

	#[test]
	fn ssao_setting_updates_write_expected_manifest_values() {
		let setting = read_avatar_setting(&repo_root().join("profiles").join("main.toml"), ProfileStorage::Seed).unwrap();
		let mut manifest = parse_manifest_value(
			r#"title = "Test"

[profile]
id = "test"
"#,
			Path::new("test.toml"),
		)
		.unwrap();

		apply_avatar_setting_value(&mut manifest, &setting, "effects.post.ssao.enabled", serde_json::json!(true)).unwrap();
		apply_avatar_setting_value(&mut manifest, &setting, "effects.post.ssao.strength", serde_json::json!(0.25)).unwrap();
		apply_avatar_setting_value(&mut manifest, &setting, "effects.post.ssao.radius", serde_json::json!(4.0)).unwrap();
		apply_avatar_setting_value(&mut manifest, &setting, "effects.post.ssao.bias", serde_json::json!(0.001)).unwrap();
		apply_avatar_setting_value(&mut manifest, &setting, "effects.post.ssao.range", serde_json::json!(0.03)).unwrap();

		let ssao = manifest
			.get("effects")
			.and_then(toml::Value::as_table)
			.and_then(|effects| effects.get("post"))
			.and_then(toml::Value::as_table)
			.and_then(|post| post.get("ssao"))
			.and_then(toml::Value::as_table)
			.expect("effects.post.ssao table");
		assert_eq!(ssao.get("enabled").and_then(toml::Value::as_bool), Some(true));
		assert!((ssao.get("strength").and_then(toml::Value::as_float).unwrap_or_default() - 0.25).abs() < 1e-6);
		assert!((ssao.get("radius").and_then(toml::Value::as_float).unwrap_or_default() - 4.0).abs() < 1e-6);
		assert!((ssao.get("bias").and_then(toml::Value::as_float).unwrap_or_default() - 0.001).abs() < 1e-6);
		assert!((ssao.get("range").and_then(toml::Value::as_float).unwrap_or_default() - 0.03).abs() < 1e-6);
	}

	#[test]
	fn contact_shadow_setting_updates_write_expected_manifest_values() {
		let setting = read_avatar_setting(&repo_root().join("profiles").join("main.toml"), ProfileStorage::Seed).unwrap();
		let mut manifest = parse_manifest_value(
			r#"title = "Test"

[profile]
id = "test"
"#,
			Path::new("test.toml"),
		)
		.unwrap();

		apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"effects.avatar.contact_shadow.enabled",
			serde_json::json!(true),
		)
		.unwrap();
		apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"effects.avatar.contact_shadow.strength",
			serde_json::json!(0.4),
		)
		.unwrap();
		apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"effects.avatar.contact_shadow.radius",
			serde_json::json!(0.7),
		)
		.unwrap();
		apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"effects.avatar.contact_shadow.softness",
			serde_json::json!(2.0),
		)
		.unwrap();
		apply_avatar_setting_value(
			&mut manifest,
			&setting,
			"effects.avatar.contact_shadow.height",
			serde_json::json!(0.02),
		)
		.unwrap();

		let shadow = manifest
			.get("effects")
			.and_then(toml::Value::as_table)
			.and_then(|effects| effects.get("avatar"))
			.and_then(toml::Value::as_table)
			.and_then(|avatar| avatar.get("contact_shadow"))
			.and_then(toml::Value::as_table)
			.expect("effects.avatar.contact_shadow table");
		assert_eq!(shadow.get("enabled").and_then(toml::Value::as_bool), Some(true));
		assert!((shadow.get("strength").and_then(toml::Value::as_float).unwrap_or_default() - 0.4).abs() < 1e-6);
		assert!((shadow.get("radius").and_then(toml::Value::as_float).unwrap_or_default() - 0.7).abs() < 1e-6);
		assert!((shadow.get("softness").and_then(toml::Value::as_float).unwrap_or_default() - 2.0).abs() < 1e-6);
		assert!((shadow.get("height").and_then(toml::Value::as_float).unwrap_or_default() - 0.02).abs() < 1e-6);
	}

	#[test]
	fn diagnostics_export_paths_parse_timestamp_and_archive_name() {
		let path = Path::new("target/tmp/diagnostics/un-avatar-supervisor-1715000000.json");
		assert_eq!(diagnostics_generated_at_secs(path), Some(1_715_000_000));
		assert_eq!(
			diagnostics_archive_path(path),
			Path::new("target/tmp/diagnostics/un-avatar-supervisor-1715000000.zip")
		);
	}

	#[test]
	fn spout_dimension_validation_bounds_runtime_presets() {
		assert_eq!(validate_spout_dimension(None, "width").unwrap(), None);
		assert_eq!(validate_spout_dimension(Some(1280), "width").unwrap(), Some(1280));
		assert!(validate_spout_dimension(Some(0), "width").is_err());
		assert!(validate_spout_dimension(Some(8193), "height").is_err());
	}

	#[test]
	fn spout_runtime_note_reports_sender_not_initialized() {
		let mut telemetry = runtime_telemetry_fixture();
		telemetry.spout_sender_initialized = Some(false);

		assert_eq!(spout_runtime_note(&telemetry).as_deref(), Some("Spout2 sender is not initialized"));
	}

	#[test]
	fn spout_runtime_note_reports_sender_size_mismatch() {
		let mut telemetry = runtime_telemetry_fixture();
		telemetry.spout_sender_width = Some(640);
		telemetry.spout_sender_height = Some(360);

		assert_eq!(
			spout_runtime_note(&telemetry).as_deref(),
			Some("Spout2 sender size 640 x 360 differs from requested 1280 x 720")
		);
	}

	#[test]
	fn spout_runtime_note_prioritizes_send_failures() {
		let mut telemetry = runtime_telemetry_fixture();
		telemetry.spout_consecutive_failures = 3;
		telemetry.spout_sender_initialized = Some(false);

		assert_eq!(
			spout_runtime_note(&telemetry).as_deref(),
			Some("Spout2 send failed for 3 consecutive frame(s)")
		);
	}

	#[test]
	fn texture_runtime_note_reports_bc_unavailable_fallback() {
		let mut telemetry = runtime_telemetry_fixture();
		telemetry.texture_compression = Some("auto".to_string());
		telemetry.texture_summary = Some(TextureRuntimeSummary {
			image_count: 2,
			compression_bc_supported: false,
			compression_fallback_count: 2,
			..TextureRuntimeSummary::default()
		});

		assert_eq!(
			texture_runtime_note(&telemetry).as_deref(),
			Some("Texture compression fell back to RGBA for 2 image(s) because BC upload is unavailable")
		);
	}

	#[test]
	fn texture_runtime_note_reports_partial_fallback() {
		let mut telemetry = runtime_telemetry_fixture();
		telemetry.texture_compression = Some("advanced".to_string());
		telemetry.texture_summary = Some(TextureRuntimeSummary {
			image_count: 3,
			compression_bc_supported: true,
			compressed_count: 2,
			compression_fallback_count: 1,
			..TextureRuntimeSummary::default()
		});

		assert_eq!(
			texture_runtime_note(&telemetry).as_deref(),
			Some("Texture compression used 2 image(s), kept 1 as RGBA")
		);
	}

	#[test]
	fn texture_runtime_note_reports_cubemap_fallback_first() {
		let mut telemetry = runtime_telemetry_fixture();
		telemetry.texture_compression = Some("source".to_string());
		telemetry.texture_summary = Some(TextureRuntimeSummary {
			image_count: 3,
			cubemap_count: 2,
			cubemap_fallback_count: 1,
			compression_fallback_count: 1,
			..TextureRuntimeSummary::default()
		});

		assert_eq!(
			texture_runtime_note(&telemetry).as_deref(),
			Some("Cubemap upload used fallback for 1/2 cube texture(s); re-export or check sourceLayout metadata")
		);
	}

	#[test]
	fn texture_runtime_note_ignores_source_mode() {
		let mut telemetry = runtime_telemetry_fixture();
		telemetry.texture_compression = Some("source".to_string());
		telemetry.texture_summary = Some(TextureRuntimeSummary {
			image_count: 1,
			compression_fallback_count: 1,
			..TextureRuntimeSummary::default()
		});

		assert!(texture_runtime_note(&telemetry).is_none());
	}

	#[test]
	fn renderer_control_sends_shutdown_command() {
		let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
		let address = listener.local_addr().unwrap();
		let server = thread::spawn(move || {
			let (mut stream, _) = listener.accept().unwrap();
			let mut command = String::new();
			BufReader::new(stream.try_clone().unwrap()).read_line(&mut command).unwrap();
			writeln!(stream, "ok").unwrap();
			command
		});

		send_renderer_control(address, &RendererControlCommand::Shutdown).unwrap();
		assert_eq!(server.join().unwrap().trim(), r#"{"command":"shutdown"}"#);
	}

	#[test]
	fn renderer_control_sends_environment_color_command() {
		let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
		let address = listener.local_addr().unwrap();
		let server = thread::spawn(move || {
			let (mut stream, _) = listener.accept().unwrap();
			let mut command = String::new();
			BufReader::new(stream.try_clone().unwrap()).read_line(&mut command).unwrap();
			writeln!(stream, "ok").unwrap();
			command
		});

		send_renderer_control(
			address,
			&RendererControlCommand::SetEnvironmentColor {
				exposure: Some(0.25),
				contrast: Some(1.2),
				saturation: Some(0.8),
				look: Some("film".to_string()),
				intensity: Some(0.45),
				temperature: Some(0.2),
				tint: Some(-0.15),
			},
		)
		.unwrap();
		assert_eq!(
			server.join().unwrap().trim(),
			r#"{"command":"set_environment_color","exposure":0.25,"contrast":1.2,"saturation":0.8,"look":"film","intensity":0.45,"temperature":0.2,"tint":-0.15}"#
		);
	}

	#[test]
	fn renderer_control_sends_activate_action_command() {
		let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
		let address = listener.local_addr().unwrap();
		let server = thread::spawn(move || {
			let (mut stream, _) = listener.accept().unwrap();
			let mut command = String::new();
			BufReader::new(stream.try_clone().unwrap()).read_line(&mut command).unwrap();
			writeln!(stream, "ok").unwrap();
			command
		});

		send_renderer_control(
			address,
			&RendererControlCommand::ActivateAction {
				action_id: None,
				menu_path: Some("Wardrobe".to_string()),
				wardrobe_set_id: Some("field_drape".to_string()),
			},
		)
		.unwrap();
		assert_eq!(
			server.join().unwrap().trim(),
			r#"{"command":"activate_action","menu_path":"Wardrobe","wardrobe_set_id":"field_drape"}"#
		);
	}

	#[test]
	fn renderer_control_sends_activate_action_command_by_action_id() {
		let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
		let address = listener.local_addr().unwrap();
		let server = thread::spawn(move || {
			let (mut stream, _) = listener.accept().unwrap();
			let mut command = String::new();
			BufReader::new(stream.try_clone().unwrap()).read_line(&mut command).unwrap();
			writeln!(stream, "ok").unwrap();
			command
		});

		send_renderer_control(
			address,
			&RendererControlCommand::ActivateAction {
				action_id: Some("wardrobe:field_drape".to_string()),
				menu_path: None,
				wardrobe_set_id: None,
			},
		)
		.unwrap();
		assert_eq!(
			server.join().unwrap().trim(),
			r#"{"command":"activate_action","action_id":"wardrobe:field_drape"}"#
		);
	}

	#[test]
	fn renderer_control_sends_dynamics_enabled_command() {
		let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
		let address = listener.local_addr().unwrap();
		let server = thread::spawn(move || {
			let (mut stream, _) = listener.accept().unwrap();
			let mut command = String::new();
			BufReader::new(stream.try_clone().unwrap()).read_line(&mut command).unwrap();
			writeln!(stream, "ok").unwrap();
			command
		});

		send_renderer_control(
			address,
			&RendererControlCommand::SetDynamicsEnabled {
				source_id: "physbone:hair".to_string(),
				enabled: true,
			},
		)
		.unwrap();
		assert_eq!(
			server.join().unwrap().trim(),
			r#"{"command":"set_dynamics_enabled","source_id":"physbone:hair","enabled":true}"#
		);
	}

	#[test]
	fn renderer_control_sends_all_dynamics_enabled_command() {
		let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
		let address = listener.local_addr().unwrap();
		let server = thread::spawn(move || {
			let (mut stream, _) = listener.accept().unwrap();
			let mut command = String::new();
			BufReader::new(stream.try_clone().unwrap()).read_line(&mut command).unwrap();
			writeln!(stream, "ok").unwrap();
			command
		});

		send_renderer_control(address, &RendererControlCommand::SetAllDynamicsEnabled { enabled: true }).unwrap();
		assert_eq!(
			server.join().unwrap().trim(),
			r#"{"command":"set_all_dynamics_enabled","enabled":true}"#
		);
	}

	#[test]
	fn renderer_control_sends_bloom_command() {
		let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
		let address = listener.local_addr().unwrap();
		let server = thread::spawn(move || {
			let (mut stream, _) = listener.accept().unwrap();
			let mut command = String::new();
			BufReader::new(stream.try_clone().unwrap()).read_line(&mut command).unwrap();
			writeln!(stream, "ok").unwrap();
			command
		});

		send_renderer_control(
			address,
			&RendererControlCommand::SetBloom {
				enabled: Some(true),
				strength: Some(0.4),
				threshold: Some(0.9),
				radius: Some(12.0),
				quality: Some("high_quality".to_string()),
			},
		)
		.unwrap();
		assert_eq!(
			server.join().unwrap().trim(),
			r#"{"command":"set_bloom","enabled":true,"strength":0.4,"threshold":0.9,"radius":12.0,"quality":"high_quality"}"#
		);
	}

	#[test]
	fn renderer_control_sends_ssao_command() {
		let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
		let address = listener.local_addr().unwrap();
		let server = thread::spawn(move || {
			let (mut stream, _) = listener.accept().unwrap();
			let mut command = String::new();
			BufReader::new(stream.try_clone().unwrap()).read_line(&mut command).unwrap();
			writeln!(stream, "ok").unwrap();
			command
		});

		send_renderer_control(
			address,
			&RendererControlCommand::SetSsao {
				enabled: Some(true),
				strength: Some(0.25),
				radius: Some(4.0),
				bias: Some(0.001),
				range: Some(0.03),
			},
		)
		.unwrap();
		assert_eq!(
			server.join().unwrap().trim(),
			r#"{"command":"set_ssao","enabled":true,"strength":0.25,"radius":4.0,"bias":0.001,"range":0.03}"#
		);
	}

	#[test]
	fn renderer_control_sends_contact_shadow_command() {
		let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
		let address = listener.local_addr().unwrap();
		let server = thread::spawn(move || {
			let (mut stream, _) = listener.accept().unwrap();
			let mut command = String::new();
			BufReader::new(stream.try_clone().unwrap()).read_line(&mut command).unwrap();
			writeln!(stream, "ok").unwrap();
			command
		});

		send_renderer_control(
			address,
			&RendererControlCommand::SetContactShadow {
				enabled: Some(true),
				strength: Some(0.4),
				radius: Some(0.7),
				softness: Some(2.0),
				height: Some(0.02),
			},
		)
		.unwrap();
		assert_eq!(
			server.join().unwrap().trim(),
			r#"{"command":"set_contact_shadow","enabled":true,"strength":0.4,"radius":0.7,"softness":2.0,"height":0.02}"#
		);
	}

	#[test]
	fn renderer_control_sends_avatar_matcap_command() {
		let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
		let address = listener.local_addr().unwrap();
		let server = thread::spawn(move || {
			let (mut stream, _) = listener.accept().unwrap();
			let mut command = String::new();
			BufReader::new(stream.try_clone().unwrap()).read_line(&mut command).unwrap();
			writeln!(stream, "ok").unwrap();
			command
		});

		send_renderer_control(address, &RendererControlCommand::SetAvatarMatcap { scale: Some(1.35) }).unwrap();
		assert_eq!(server.join().unwrap().trim(), r#"{"command":"set_avatar_matcap","scale":1.35}"#);
	}

	#[test]
	fn renderer_control_sends_avatar_specular_command() {
		let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
		let address = listener.local_addr().unwrap();
		let server = thread::spawn(move || {
			let (mut stream, _) = listener.accept().unwrap();
			let mut command = String::new();
			BufReader::new(stream.try_clone().unwrap()).read_line(&mut command).unwrap();
			writeln!(stream, "ok").unwrap();
			command
		});

		send_renderer_control(
			address,
			&RendererControlCommand::SetAvatarSpecular {
				enabled: Some(true),
				intensity: Some(0.5),
				power: Some(32.0),
			},
		)
		.unwrap();
		assert_eq!(
			server.join().unwrap().trim(),
			r#"{"command":"set_avatar_specular","enabled":true,"intensity":0.5,"power":32.0}"#
		);
	}

	#[test]
	fn renderer_control_sends_avatar_ambient_occlusion_command() {
		let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
		let address = listener.local_addr().unwrap();
		let server = thread::spawn(move || {
			let (mut stream, _) = listener.accept().unwrap();
			let mut command = String::new();
			BufReader::new(stream.try_clone().unwrap()).read_line(&mut command).unwrap();
			writeln!(stream, "ok").unwrap();
			command
		});

		send_renderer_control(address, &RendererControlCommand::SetAvatarAmbientOcclusion { strength: Some(1.4) }).unwrap();
		assert_eq!(
			server.join().unwrap().trim(),
			r#"{"command":"set_avatar_ambient_occlusion","strength":1.4}"#
		);
	}

	#[test]
	fn renderer_control_session_reuses_connection() {
		let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
		let address = listener.local_addr().unwrap();
		let server = thread::spawn(move || {
			let (mut stream, _) = listener.accept().unwrap();
			let mut reader = BufReader::new(stream.try_clone().unwrap());
			let mut commands = Vec::new();
			for _ in 0..2 {
				let mut command = String::new();
				reader.read_line(&mut command).unwrap();
				commands.push(command.trim().to_string());
				writeln!(stream, "ok").unwrap();
			}
			commands
		});

		let session = Arc::new(Mutex::new(None));
		send_renderer_control_session(&session, address, &RendererControlCommand::Shutdown).unwrap();
		send_renderer_control_session(&session, address, &RendererControlCommand::ResetCamera).unwrap();
		drop(session.lock().unwrap().take());

		assert_eq!(
			server.join().unwrap(),
			vec![r#"{"command":"shutdown"}"#.to_string(), r#"{"command":"reset_camera"}"#.to_string()]
		);
	}
}
