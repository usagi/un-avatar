use std::{net::SocketAddr, path::PathBuf};

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::{debug_log::WindowDebugOptions, mesh_pass::SceneMeshLoadOpts};
use un_avatar_skeleton::{BoneColliderConfig, SpringBonePhysicsConfig};

/// 旧プロファイル互換の primary motion source。
///
/// 現在の renderer は受信した VMC / UNMotionFrame を key 単位の pending buffer に集約し、描画直前に
/// 後着優先で適用する。値は古い manifest / CLI / IPC の読み書き互換のために残している。
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum PrimaryMotionSource {
	#[default]
	Vmc,
	UnmotionZenoh,
}

/// AudioLink GPU texture source policy.
///
/// `None` keeps lilToon-compatible shader fallback waveforms only. `InputDevice`
/// allows the renderer/supervisor audio service to capture an OS audio input
/// device and generate a VRChat AudioLink-compatible texture, but the worker
/// should still start lazily only when the active wardrobe actually uses
/// AudioLink.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum AudioLinkSource {
	#[default]
	None,
	InputDevice,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct AudioLinkOptions {
	pub source: AudioLinkSource,
	pub input_device_id: Option<String>,
	pub input_device_name_hint: Option<String>,
}

impl Default for AudioLinkOptions {
	fn default() -> Self {
		Self {
			source: AudioLinkSource::None,
			input_device_id: None,
			input_device_name_hint: None,
		}
	}
}

/// UNMotion/Zenoh 受信の設定。`[motion.unmotion_zenoh]` 由来。
#[derive(Clone, Debug)]
pub struct UnmotionZenohOptions {
	/// 受信を有効にするか。OFF のときは subscriber スレッドを起動しない。
	pub enabled: bool,
	/// `ZenohTopicStrategy::base_key_expr` に渡すベース key。空文字なら既定 `"un-motion/frame"` を使う。
	pub base_key_expr: String,
}

impl Default for UnmotionZenohOptions {
	fn default() -> Self {
		Self {
			enabled: false,
			base_key_expr: "un-motion/frame".to_string(),
		}
	}
}

/// Renderer anti-aliasing mode.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum AaMode {
	#[default]
	Off,
	Fxaa,
	Smaa,
	Msaa,
}

/// Optional load-time texture resolution clamp. OFF is the default because resizing can visibly degrade avatars.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum TextureResolutionLimit {
	#[default]
	Off,
	Auto,
	#[serde(rename = "8k")]
	#[value(name = "8k")]
	K8,
	#[serde(rename = "4k")]
	#[value(name = "4k")]
	K4,
	#[serde(rename = "2k")]
	#[value(name = "2k")]
	K2,
	#[serde(rename = "1k")]
	#[value(name = "1k")]
	K1,
}

impl TextureResolutionLimit {
	pub(crate) fn max_dimension(self, target_width: u32, target_height: u32) -> Option<u32> {
		match self {
			Self::Off => None,
			Self::Auto => Some(auto_texture_max_dimension(target_width, target_height)),
			Self::K8 => Some(8192),
			Self::K4 => Some(4096),
			Self::K2 => Some(2048),
			Self::K1 => Some(1024),
		}
	}
}

/// Texture upload / compression policy.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum TextureCompressionMode {
	/// Prefer source/native upload fidelity and avoid lossy compression.
	Source,
	/// Balanced default: conservative role-based compression/cache when safe.
	#[default]
	#[serde(alias = "auto", alias = "advanced")]
	#[value(alias = "auto", alias = "advanced")]
	Balanced,
	/// Prefer smaller GPU/cache footprint even when that can reduce texture fidelity.
	Memory,
	/// Prefer broadly compatible upload formats and avoid GPU-specific compression.
	Compat,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum TextureMipmapFilter {
	/// Legacy 2x2 box downsample. Fast and stable, but rougher than high-quality reconstruction filters.
	Box2x2,
	/// pic-scale bilinear resize.
	Bilinear,
	/// pic-scale bicubic resize.
	Bicubic,
	/// pic-scale Catmull-Rom resize.
	CatmullRom,
	/// pic-scale Lanczos3 resize.
	Lanczos3,
	/// pic-scale Mitchell-Netravali resize. Balanced quality/performance default.
	#[default]
	Mitchell,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum RenderBackend {
	Auto,
	#[default]
	Vulkan,
	Dx12,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum BlockCompressionEncoder {
	Cpu,
	#[default]
	Gpu,
}

/// Per-role compression preference used as an advanced override for balanced/memory policies.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TextureCompressionPreference {
	/// Preserve decoded source fidelity for this role.
	Source,
	/// Use the renderer's role-specific default.
	#[default]
	Auto,
	/// Prefer high quality: UASTC as cache/intermediate, BC7 on Windows when supported.
	HighQuality,
	/// Prefer smaller cache artifacts: ETC1S/BasisLZ class compression when acceptable.
	Small,
	/// Prefer runtime-native GPU formats. On Windows this usually means BCn first.
	GpuNative,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct TextureCompressionAdvancedOptions {
	pub face: TextureCompressionPreference,
	pub eyes: TextureCompressionPreference,
	pub clothing: TextureCompressionPreference,
	pub normal: TextureCompressionPreference,
	pub occlusion: TextureCompressionPreference,
	pub emissive: TextureCompressionPreference,
	pub generic_color: TextureCompressionPreference,
	pub data: TextureCompressionPreference,
}

impl Default for TextureCompressionAdvancedOptions {
	fn default() -> Self {
		Self {
			face: TextureCompressionPreference::Source,
			eyes: TextureCompressionPreference::Source,
			clothing: TextureCompressionPreference::Auto,
			normal: TextureCompressionPreference::GpuNative,
			occlusion: TextureCompressionPreference::GpuNative,
			emissive: TextureCompressionPreference::HighQuality,
			generic_color: TextureCompressionPreference::Auto,
			data: TextureCompressionPreference::Source,
		}
	}
}

fn auto_texture_max_dimension(target_width: u32, target_height: u32) -> u32 {
	let long_edge = target_width.max(target_height).max(1);
	if long_edge <= 1024 {
		1024
	} else if long_edge <= 2048 {
		2048
	} else if long_edge <= 4096 {
		4096
	} else {
		8192
	}
}

/// Spout2 送出（Windows のみ有効。OBS などで `Spout2` 受信）。
#[derive(Clone, Debug)]
pub struct SpoutWindowOptions {
	pub enabled: bool,
	pub name: String,
	pub width: Option<u32>,
	pub height: Option<u32>,
}

impl Default for SpoutWindowOptions {
	fn default() -> Self {
		Self {
			enabled: false,
			name: "UN Avatar".into(),
			width: None,
			height: None,
		}
	}
}

/// manifest `[camera]` から渡される起動時カメラ初期値（profile 保存・復元用）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InitialCameraState {
	pub target: Option<[f32; 3]>,
	pub longitude_deg: Option<f32>,
	pub latitude_deg: Option<f32>,
	pub radius: Option<f32>,
	pub diagonal_fov_deg: Option<f32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ColorGradingLook {
	#[default]
	Neutral,
	Warm,
	Cool,
	Film,
	Soft,
	Pop,
}

impl ColorGradingLook {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Neutral => "neutral",
			Self::Warm => "warm",
			Self::Cool => "cool",
			Self::Film => "film",
			Self::Soft => "soft",
			Self::Pop => "pop",
		}
	}

	pub fn shader_id(self) -> f32 {
		match self {
			Self::Neutral => 0.0,
			Self::Warm => 1.0,
			Self::Cool => 2.0,
			Self::Film => 3.0,
			Self::Soft => 4.0,
			Self::Pop => 5.0,
		}
	}
}

impl std::str::FromStr for ColorGradingLook {
	type Err = String;

	fn from_str(value: &str) -> Result<Self, Self::Err> {
		match value.trim().to_ascii_lowercase().as_str() {
			"neutral" | "off" | "none" => Ok(Self::Neutral),
			"warm" => Ok(Self::Warm),
			"cool" => Ok(Self::Cool),
			"film" | "cinematic" => Ok(Self::Film),
			"soft" => Ok(Self::Soft),
			"pop" | "vivid" => Ok(Self::Pop),
			other => Err(format!("unknown color grading look: {other}")),
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvironmentColorOptions {
	pub exposure: f32,
	pub contrast: f32,
	pub saturation: f32,
	pub look: ColorGradingLook,
	pub look_intensity: f32,
	pub temperature: f32,
	pub tint: f32,
}

impl EnvironmentColorOptions {
	pub fn is_identity(self) -> bool {
		(self.exposure.abs() <= f32::EPSILON)
			&& ((self.contrast - 1.0).abs() <= f32::EPSILON)
			&& ((self.saturation - 1.0).abs() <= f32::EPSILON)
			&& (matches!(self.look, ColorGradingLook::Neutral) || self.look_intensity <= f32::EPSILON)
			&& (self.temperature.abs() <= f32::EPSILON)
			&& (self.tint.abs() <= f32::EPSILON)
	}
}

impl Default for EnvironmentColorOptions {
	fn default() -> Self {
		Self {
			exposure: 0.0,
			contrast: 1.0,
			saturation: 1.0,
			look: ColorGradingLook::Neutral,
			look_intensity: 0.0,
			temperature: 0.0,
			tint: 0.0,
		}
	}
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct EnvironmentLightOptions {
	pub enabled: bool,
	pub color: [f32; 3],
	pub intensity: f32,
}

impl Default for EnvironmentLightOptions {
	fn default() -> Self {
		Self {
			enabled: true,
			color: [1.0, 1.0, 1.0],
			intensity: 0.35,
		}
	}
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct DirectionalLightOptions {
	pub enabled: bool,
	pub color: [f32; 3],
	pub intensity: f32,
	pub azimuth_deg: f32,
	pub elevation_deg: f32,
	pub follow_camera_yaw: bool,
	pub follow_camera_pitch: bool,
}

impl Default for DirectionalLightOptions {
	fn default() -> Self {
		Self {
			enabled: true,
			color: [1.0, 1.0, 1.0],
			intensity: 1.0,
			azimuth_deg: 0.0,
			elevation_deg: 33.84,
			follow_camera_yaw: true,
			follow_camera_pitch: false,
		}
	}
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
pub struct LightingOptions {
	pub environment: EnvironmentLightOptions,
	pub directional: DirectionalLightOptions,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum BloomQuality {
	#[default]
	Compact,
	HighQuality,
}

impl BloomQuality {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Compact => "compact",
			Self::HighQuality => "high_quality",
		}
	}

	pub fn is_high_quality(self) -> bool {
		matches!(self, Self::HighQuality)
	}
}

impl std::str::FromStr for BloomQuality {
	type Err = String;

	fn from_str(value: &str) -> Result<Self, Self::Err> {
		match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
			"compact" | "standard" | "fast" => Ok(Self::Compact),
			"high_quality" | "quality" | "hq" => Ok(Self::HighQuality),
			other => Err(format!("unknown bloom quality: {other}")),
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BloomOptions {
	pub enabled: bool,
	pub strength: f32,
	pub threshold: f32,
	pub radius: f32,
	pub quality: BloomQuality,
}

impl BloomOptions {
	pub fn is_enabled(self) -> bool {
		self.enabled && self.strength > 0.0 && self.radius > 0.0
	}
}

impl Default for BloomOptions {
	fn default() -> Self {
		Self {
			enabled: false,
			strength: 0.35,
			threshold: 0.65,
			radius: 8.0,
			quality: BloomQuality::Compact,
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContactShadowOptions {
	pub enabled: bool,
	pub strength: f32,
	pub radius: f32,
	pub softness: f32,
	pub height: f32,
}

impl ContactShadowOptions {
	pub fn is_enabled(self) -> bool {
		self.enabled && self.strength > 0.0 && self.radius > 0.0
	}
}

impl Default for ContactShadowOptions {
	fn default() -> Self {
		Self {
			enabled: false,
			strength: 0.35,
			radius: 0.55,
			softness: 1.8,
			height: 0.0,
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SsaoOptions {
	pub enabled: bool,
	pub strength: f32,
	pub radius: f32,
	pub bias: f32,
	pub range: f32,
}

impl SsaoOptions {
	pub fn is_enabled(self) -> bool {
		self.enabled && self.strength > 0.0 && self.radius > 0.0
	}
}

impl Default for SsaoOptions {
	fn default() -> Self {
		Self {
			enabled: false,
			strength: 0.25,
			radius: 4.0,
			bias: 0.0015,
			range: 0.03,
		}
	}
}

/// ウィンドウ起動オプション（背景・装飾・透過）。レンダラー本体の設定は今後のクレートに移す想定。
#[derive(Clone, Debug)]
pub struct AvatarWindowOptions {
	pub title: String,
	/// `false` で枠・タイトルバーなし（ボーダレス）。
	pub decorations: bool,
	pub transparent: bool,
	/// When true with transparent windows, mouse hit-testing passes through to windows behind the renderer.
	pub input_passthrough: bool,
	pub always_on_top: bool,
	/// Renderer-process-local close hotkey used while the window is borderless.
	pub close_hotkey: String,
	pub window_width: u32,
	pub window_height: u32,
	/// 起動時の outer 位置（px）。`None` は OS 既定（前回位置 or プライマリモニタ中央）。
	/// `manifest [window] x = ... y = ...` から渡される。
	pub window_position: Option<[i32; 2]>,
	/// 表示するモデル（glTF `.gltf` / `.glb` または VRM `.vrm` / VRM 入り `.glb`）。シーンがあればメッシュモード。
	pub gltf_path: Option<PathBuf>,
	/// この Renderer を起動した profile manifest。Supervisor / tray handoff 用。
	pub manifest_path: Option<PathBuf>,
	/// `.unavatar` 起動時に Base 適用後へ重ねる wardrobe set id。未指定なら Base のみ。
	pub wardrobe_set: Option<String>,
	/// ウィンドウ・タスクバー用アイコン。未指定時はexe埋め込みアイコンを使う。
	pub icon_path: Option<PathBuf>,
	pub clear_color: wgpu::Color,
	/// タイトルバーに FPS と概算 CPU／GPU 時間（ms）を表示する。
	pub show_fps_in_title: bool,
	/// CLI benchmark: after startup completes, collect this many rendered frames, print timing summary, then exit.
	pub bench_frames: Option<u32>,
	/// VMC Marionette 待受 UDP アドレス。`Humanoid` とシーンがあるモデルで骨・式（名前一致時）を更新。
	pub vmc_address: Option<SocketAddr>,
	/// UNMotion/Zenoh 経由でのモーションフレーム受信設定 (Phase 2)。
	pub unmotion_zenoh: UnmotionZenohOptions,
	/// AudioLink texture generation source. `None` keeps shader fallback only.
	pub audio_link: AudioLinkOptions,
	/// 旧 manifest / CLI 互換の primary source。現在の姿勢適用は key 単位の後着優先。
	pub primary_motion_source: PrimaryMotionSource,
	/// Spout2 送出（Windows）。`--spout` で有効。
	pub spout: SpoutWindowOptions,
	/// Final post color adjustment for avatar presentation. Identity by default.
	pub environment_color: EnvironmentColorOptions,
	/// Scene lighting used by Lit/MToon mesh shaders.
	pub lighting: LightingOptions,
	/// Lightweight final-pass bloom. Disabled by default.
	pub bloom: BloomOptions,
	/// Depth-based screen-space ambient occlusion. Disabled by default.
	pub ssao: SsaoOptions,
	/// Lightweight avatar contact shadow drawn under the model origin.
	pub contact_shadow: ContactShadowOptions,
	/// Explicit opt-in for VRC Contact Receiver parameter emission.
	pub contact_parameter_emission: bool,
	/// Anti-aliasing mode. OFF keeps the direct path; FXAA uses a fullscreen post pass.
	pub aa: AaMode,
	/// Optional load-time texture clamp. Default OFF preserves source texture fidelity.
	pub texture_resolution_limit: TextureResolutionLimit,
	/// Texture upload / compression policy. Default Balanced is conservative but cache-friendly.
	pub texture_compression: TextureCompressionMode,
	/// Mipmap generation filter. High-quality filters use pic-scale; box2x2 keeps the legacy path.
	pub mipmap_filter: TextureMipmapFilter,
	/// WGPU backend preference. Vulkan is the default for cross-platform behavior and BC7 compute stability.
	pub render_backend: RenderBackend,
	/// BCn encoder used when textures are uploaded as BC1/BC5/BC7.
	pub block_compression_encoder: BlockCompressionEncoder,
	/// CPU BCn worker count. Clamped to the system logical CPU count at use sites.
	pub block_compression_cpu_threads: usize,
	/// Advanced texture compression preferences used by balanced/memory policies.
	pub texture_compression_advanced: TextureCompressionAdvancedOptions,
	/// Cache resized RGBA mip chains on disk so repeated launches skip CPU texture processing.
	pub processed_texture_cache: bool,
	/// Experimental load-time texture correction that nudges face/body skin colors toward the same CIELAB target.
	pub skin_tone_matching: bool,
	/// Supervisorが観測用に接続するローカルruntime status endpoint。
	pub runtime_status_address: Option<SocketAddr>,
	/// Supervisorが低頻度制御に使うローカルcontrol endpoint。
	pub runtime_control_address: Option<SocketAddr>,
	/// Supervisor と renderer の runtime IPC に使う Zenoh base key。
	pub runtime_bus_key: Option<String>,
	/// UNPhysics / UNDynamics を毎フレームシミュレーションする（既定 ON。揺れもの表現はアバターの基本機能のため）。
	/// 静止画として表示したいときだけ manifest `[physics.dynamics] enabled = false` で OFF にする。
	pub dynamics_enabled: bool,
	/// 起動直後にすべての runtime dynamics group を明示 ON にする。VRC PhysBone 既定 OFF モデル向けの opt-in。
	pub dynamics_enable_all_on_launch: bool,
	/// UNDynamics めり込み抑制用のボーンベースコライダー設定。
	pub bone_colliders: BoneColliderConfig,
	/// UNDynamics solver backend / time model / category override 設定。
	pub spring_bone_physics: SpringBonePhysicsConfig,
	/// 調査用ログ（`run_cli` の `--debug-*` と対応）。
	pub debug: WindowDebugOptions,
	/// 式プリセット（VMC Blend 等）をモーフ合成に使わない（目まわりの切り分け用）。
	pub disable_expression_morphs: bool,
	/// VMC 由来の LeftEye / RightEye 骨行列を適用しない（視線の切り分け用）。
	pub disable_vmc_eye_look: bool,
	/// VMC eye bone rotation の yaw/pitch クランプ角度（度）。
	///
	/// VRM 1.0 LookAt curve の簡易版として、`Some(deg)` のとき eye bone の yaw/pitch を
	/// ±deg に制限してから書き込む。VRM 1.0 spec のデフォルト 30°。
	/// `None` ならクランプなし（実 VMC が送ってきた回転をそのまま使う）。
	pub eye_look_at_clamp_deg: Option<f32>,
	/// VMC `/VMC/Ext/Root/Pos` の **translation** を scene root へ加算するか。既定 `false`。
	///
	/// Waidayo 系の Sender はキャリブレーション計算の都合で意図せず非ゼロな `Root.translation` を
	/// 送ってくる場合があり、armature root を最初の scene root に持つモデル（例: model1.vrm の `Root`）
	/// では VMC ON 切替時にアバター全体が前後に 1m 程度ズレて表示されてしまう。
	/// Sender に依存しない default として OFF にし、フルボディトラッカー等で位置移動も載せたいユースケースで
	/// manifest `[motion] apply_vmc_root_translation = true` / IPC で明示的に ON にする。
	/// **rotation は本フラグに関わらず常に適用される**。
	pub apply_vmc_root_translation: bool,
	/// 診断用: 全 draw を不透明 LitLambert + baseColor×texture のみに固定（MToon シェーダ分岐・base_color.a を無視）。
	pub simple_basecolor_only: bool,
	/// ロード時にマテリアル・スキン情報を stderr へ出力。
	pub debug_material_dump: bool,
	/// XYZ デバッグ軸表示の初期値（CLI / manifest / IPC で切替可能）。デフォルトは Off。
	pub show_axes: bool,
	/// ボーンベースコライダーの debug 表示。デフォルトは Off。
	pub show_bone_colliders: bool,
	/// MToon outline 描画を完全に無効化する診断フラグ。
	/// 一部 VRM モデルで `_OutlineColor` が肌色寄りに設定されていると、目周辺の outline が
	/// 「太い肌色のリング」として目立つ場合がある（VSeeFace では薄く出るが、UN Avatar の
	/// 単純 mix(1, lighting, mix_factor) 式では明るすぎる傾向）。この toggle で outline 描画を
	/// バイパスして原因切り分けに使う。manifest `[debug] disable_mtoon_outlines = true` で有効化。
	pub disable_mtoon_outlines: bool,
	/// MToon の parametric Rim Lighting 寄与を 0 にする診断フラグ。
	/// manifest `[debug] disable_rim_lighting = true`。
	pub debug_disable_rim_lighting: bool,
	/// `shading_shift_factor` と `shadingShiftTexture` の寄与を 0 固定にする診断フラグ。
	/// manifest `[debug] force_shading_shift_zero = true`。
	pub debug_force_shading_shift_zero: bool,
	/// matcap (sphere add) 寄与を 0 にする診断フラグ。
	/// manifest `[debug] disable_matcap = true`。
	pub debug_disable_matcap: bool,
	/// emissive 寄与を 0 にする診断フラグ。
	/// manifest `[debug] disable_emissive = true`。
	pub debug_disable_emissive: bool,
	/// MToon `shade_color × shade_tex` の代わりに base を使う診断フラグ。
	/// manifest `[debug] disable_shade_color = true`。
	pub debug_disable_shade_color: bool,
	/// normalTexture を使わず頂点法線のみで shading / rim を計算する診断フラグ。
	/// manifest `[debug] disable_normal_map = true`。
	pub debug_disable_normal_map: bool,
	/// toon path を `base` のみで早期 return する診断フラグ（shading / rim / matcap / GI / emissive 全 skip）。
	/// manifest `[debug] base_texture_only = true`。
	pub debug_base_texture_only: bool,
	/// カメラ操作ロック。true の間はマウスドラッグ / ホイールでカメラ操作不可。
	/// manifest `[camera] locked = true` / CLI `--lock-camera` / IPC `set_camera_lock` で切替。
	pub camera_locked: bool,
	/// 起動直後にウィンドウを最小化するか（既定 false）。manifest `[window] minimized = true` / CLI `--start-minimized`。
	pub start_minimized: bool,
	/// 起動時に適用するカメラ状態（manifest `[camera] target / longitude_deg / latitude_deg / radius / diagonal_fov_deg`）。
	/// `None` の場合は `OrbitCamera::default()` 値（モデル胸元の真正面 +Z 側、35mm 相当の対角画角）が使われる。
	pub initial_camera_state: Option<InitialCameraState>,
	/// メッシュパス用の切り分けオプション（bind pose / 単色 / モーフゼロ / 虹彩 Opaque / スキン旧式など）。
	pub mesh_diagnostics: SceneMeshLoadOpts,
}

impl Default for AvatarWindowOptions {
	fn default() -> Self {
		Self {
			title: "UN Avatar".to_owned(),
			decorations: true,
			transparent: false,
			input_passthrough: false,
			always_on_top: false,
			close_hotkey: "Escape".to_string(),
			window_width: 800,
			window_height: 600,
			window_position: None,
			clear_color: wgpu::Color {
				r: 0.12,
				g: 0.14,
				b: 0.18,
				a: 1.0,
			},
			gltf_path: None,
			manifest_path: None,
			wardrobe_set: None,
			icon_path: None,
			show_fps_in_title: true,
			bench_frames: None,
			vmc_address: None,
			unmotion_zenoh: UnmotionZenohOptions::default(),
			audio_link: AudioLinkOptions::default(),
			primary_motion_source: PrimaryMotionSource::default(),
			spout: SpoutWindowOptions::default(),
			environment_color: EnvironmentColorOptions::default(),
			lighting: LightingOptions::default(),
			bloom: BloomOptions::default(),
			ssao: SsaoOptions::default(),
			contact_shadow: ContactShadowOptions::default(),
			contact_parameter_emission: false,
			aa: AaMode::Off,
			texture_resolution_limit: TextureResolutionLimit::Off,
			texture_compression: TextureCompressionMode::Balanced,
			mipmap_filter: TextureMipmapFilter::default(),
			render_backend: RenderBackend::Vulkan,
			block_compression_encoder: BlockCompressionEncoder::Gpu,
			block_compression_cpu_threads: 4,
			texture_compression_advanced: TextureCompressionAdvancedOptions::default(),
			processed_texture_cache: true,
			skin_tone_matching: false,
			runtime_status_address: None,
			runtime_control_address: None,
			runtime_bus_key: None,
			dynamics_enabled: true,
			dynamics_enable_all_on_launch: false,
			bone_colliders: BoneColliderConfig::default(),
			spring_bone_physics: SpringBonePhysicsConfig::default(),
			debug: WindowDebugOptions::default(),
			disable_expression_morphs: false,
			disable_vmc_eye_look: false,
			// LookAt クランプはオプトイン。manifest `[motion.look_at] enabled = true` で明示的に有効化する。
			eye_look_at_clamp_deg: None,
			apply_vmc_root_translation: false,
			simple_basecolor_only: false,
			debug_material_dump: false,
			show_axes: false,
			show_bone_colliders: false,
			disable_mtoon_outlines: false,
			debug_disable_rim_lighting: false,
			debug_force_shading_shift_zero: false,
			debug_disable_matcap: false,
			debug_disable_emissive: false,
			debug_disable_shade_color: false,
			debug_disable_normal_map: false,
			debug_base_texture_only: false,
			camera_locked: false,
			start_minimized: false,
			initial_camera_state: None,
			mesh_diagnostics: SceneMeshLoadOpts::default(),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::TextureCompressionMode;

	#[test]
	fn texture_compression_mode_uses_v2_names_and_legacy_aliases() {
		assert_eq!(
			serde_json::from_str::<TextureCompressionMode>(r#""source""#).unwrap(),
			TextureCompressionMode::Source
		);
		assert_eq!(
			serde_json::from_str::<TextureCompressionMode>(r#""balanced""#).unwrap(),
			TextureCompressionMode::Balanced
		);
		assert_eq!(
			serde_json::from_str::<TextureCompressionMode>(r#""memory""#).unwrap(),
			TextureCompressionMode::Memory
		);
		assert_eq!(
			serde_json::from_str::<TextureCompressionMode>(r#""compat""#).unwrap(),
			TextureCompressionMode::Compat
		);
		assert_eq!(
			serde_json::from_str::<TextureCompressionMode>(r#""auto""#).unwrap(),
			TextureCompressionMode::Balanced
		);
		assert_eq!(
			serde_json::from_str::<TextureCompressionMode>(r#""advanced""#).unwrap(),
			TextureCompressionMode::Balanced
		);
	}
}
