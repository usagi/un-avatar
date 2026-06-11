//! winit + wgpu で独立アバター用ウィンドウを開く最小ブートストラップ（開発計画 Commit 1.1〜1.2）。

mod audio_link;
mod avatar_material;
mod camera;
mod debug_dump;
mod debug_log;
mod gpu;
mod manifest;
mod mesh_pass;
mod model_loader;
mod options;
mod post_process;
mod scene_transform;
#[cfg(test)]
mod shader_validation;
mod skin_tone;
#[cfg(windows)]
mod spout;
mod texture_pipeline;

use clap::Parser;
use serde::{Deserialize, Serialize};
use std::{
	cell::Cell,
	collections::{BTreeMap, VecDeque},
	io::{BufRead, BufReader, Write},
	net::SocketAddr,
	path::{Path, PathBuf},
	sync::{Arc, Mutex},
	thread,
	time::{Duration, Instant},
};

pub use debug_log::WindowDebugOptions;
pub use gpu::FrameTimings;
use gpu::{wardrobe_asset_upload_plan_is_default, DocumentAttachOptions, GpuState, PreparedDocumentScene, WardrobeAssetUploadPlan};
pub use mesh_pass::{
	AvatarAmbientOcclusionOptions, AvatarMatcapOptions, AvatarOutlineKind, AvatarOutlineOptions, AvatarOutlinePolicy, AvatarRimOptions,
	AvatarRimPolicy, AvatarSpecularOptions, SceneMeshLoadOpts,
};
pub use options::{
	AaMode, AvatarWindowOptions, BlockCompressionEncoder, BloomOptions, BloomQuality, ColorGradingLook, ContactShadowOptions,
	DirectionalLightOptions, EnvironmentColorOptions, EnvironmentLightOptions, LightingOptions, RenderBackend, SpoutWindowOptions,
	SsaoOptions, TextureCompressionAdvancedOptions, TextureCompressionMode, TextureCompressionPreference, TextureMipmapFilter,
	TextureResolutionLimit,
};
use un_avatar_skeleton::{BoneColliderConfig, DynamicsPhysicsConfig};
#[cfg(windows)]
use winit::platform::windows::WindowAttributesExtWindows;
use winit::{
	application::ApplicationHandler,
	dpi::{PhysicalPosition, PhysicalSize},
	event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
	event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
	keyboard::{Key, ModifiersState},
	window::{CursorIcon, Icon, ResizeDirection, Window, WindowLevel},
};
/// イベントループまたは初期化エラー。
#[derive(Debug)]
pub enum RunError {
	/// イベントループ生成・実行の失敗。
	EventLoop(String),
}

impl std::fmt::Display for RunError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			RunError::EventLoop(s) => write!(f, "{s}"),
		}
	}
}

impl std::error::Error for RunError {}

/// `RendererControlEvent` の同期 command 結果をコントロールチャネルへ返すための共有スロット。
type CommandResultSlot = Arc<Mutex<Option<Result<(), String>>>>;
type SceneStateResultSlot = Arc<Mutex<Option<String>>>;

const SCENE_STATE_SPLASH: &str = "splash";
const SCENE_STATE_AVATAR_SCENE: &str = "avatar_scene";
const SCENE_STATE_FAILED: &str = "failed";
const WINDOW_TITLE_STATUS_MAX_CHARS: usize = 120;
const SURFACE_RESIZE_SETTLE_DELAY: Duration = Duration::from_millis(80);
const RENDERER_CONTROL_CAPABILITIES: &[&str] = &[
	"shutdown",
	"reset_camera",
	"set_camera_orbit",
	"set_clear_color",
	"set_spout_output",
	"set_window",
	"screenshot",
	"set_wardrobe",
	"activate_action",
	"set_parameter",
	"set_expression_override",
	"clear_expression_overrides",
	"set_look_at",
	"activate",
	"set_show_axes",
	"set_show_bone_colliders",
	"set_camera_lock",
	"set_camera_fov",
	"set_camera_state",
	"set_apply_vmc_root_translation",
	"set_primary_motion_source",
	"set_motion_receivers",
	"set_dynamics",
	"set_spring_bones",
	"set_avatar_outline",
	"set_avatar_rim",
	"set_avatar_matcap",
	"set_avatar_specular",
	"set_avatar_ambient_occlusion",
	"set_lighting",
	"set_environment_color",
	"set_bloom",
	"set_ssao",
	"set_contact_shadow",
	"scene_state",
];

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CameraTransitionEasing {
	Linear,
	EaseOutCubic,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CameraTransitionMode {
	Queue,
	Replace,
}

impl Default for CameraTransitionMode {
	fn default() -> Self {
		Self::Queue
	}
}

impl Default for CameraTransitionEasing {
	fn default() -> Self {
		Self::EaseOutCubic
	}
}

#[derive(Clone, Copy, Debug, Deserialize)]
struct CameraTransitionOptions {
	#[serde(default = "default_camera_transition_duration_ms")]
	#[serde(alias = "durationMs")]
	duration_ms: u32,
	#[serde(default)]
	easing: CameraTransitionEasing,
	#[serde(default)]
	mode: CameraTransitionMode,
}

fn default_camera_transition_duration_ms() -> u32 {
	320
}

#[derive(Clone, Copy, Debug)]
struct CameraStatePatch {
	target: Option<[f32; 3]>,
	longitude_deg: Option<f32>,
	latitude_deg: Option<f32>,
	radius: Option<f32>,
	diagonal_fov_deg: Option<f32>,
}

#[derive(Clone, Copy, Debug)]
struct QueuedCameraTransition {
	patch: CameraStatePatch,
	options: CameraTransitionOptions,
}

#[derive(Clone, Copy, Debug)]
struct ActiveCameraTransition {
	start: gpu::CameraStateSnapshot,
	end: gpu::CameraStateSnapshot,
	started_at: Instant,
	duration: Duration,
	easing: CameraTransitionEasing,
}

#[allow(clippy::large_enum_variant)]
enum RendererControlEvent {
	Shutdown,
	ResetCamera,
	SetCameraOrbit {
		longitude: Option<f32>,
		latitude: Option<f32>,
		radius: Option<f32>,
	},
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
		minimized: Option<bool>,
		width: Option<u32>,
		height: Option<u32>,
	},
	Screenshot {
		path: std::path::PathBuf,
		result: CommandResultSlot,
	},
	SetWardrobe {
		set_id: String,
		result: CommandResultSlot,
	},
	ActivateAction {
		action_id: Option<String>,
		supervisor_command: Option<String>,
		expression_menu_path: Option<String>,
		menu_path: Option<String>,
		wardrobe_set_id: Option<String>,
		parameter_name: Option<String>,
		parameter_value: Option<f32>,
		result: CommandResultSlot,
	},
	SetParameter {
		name: String,
		value: f32,
		result: CommandResultSlot,
	},
	SceneState {
		result: SceneStateResultSlot,
	},
	SetExpressionOverride {
		name: String,
		weight: f32,
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
		target: Option<[f32; 3]>,
		longitude_deg: Option<f32>,
		latitude_deg: Option<f32>,
		radius: Option<f32>,
		diagonal_fov_deg: Option<f32>,
		transition: Option<CameraTransitionOptions>,
	},
	SetWindowPosition {
		x: Option<i32>,
		y: Option<i32>,
	},
	/// VMC `Root.translation` を scene root へ加算するかの切替。
	SetApplyVmcRootTranslation {
		enabled: bool,
	},
	/// 旧 IPC 互換の primary motion source 更新。現在の姿勢適用は key 単位の後着優先。
	SetPrimaryMotionSource {
		source: crate::options::PrimaryMotionSource,
	},
	SetMotionReceivers {
		vmc_address: Option<SocketAddr>,
		unmotion_zenoh_enabled: bool,
		unmotion_zenoh_key: String,
	},
	SetDynamics {
		enabled: bool,
		bone_colliders: BoneColliderConfig,
		/// None means no dynamics physics override; dynamics itself is controlled by `enabled`.
		physics_config: Option<DynamicsPhysicsConfig>,
	},
	SetAvatarOutline {
		policy: Option<String>,
		#[allow(dead_code)]
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
	StartupProgress {
		phase: StartupPhase,
		current: u32,
		total: u32,
		message: String,
	},
	StartupReady {
		document: Arc<un_avatar_core::UnaDocument>,
		texture_summary: mesh_pass::TextureUploadSummary,
	},
	StartupSceneReady {
		prepared: PreparedDocumentScene,
		options: DocumentAttachOptions,
		fallback_texture_summary: mesh_pass::TextureUploadSummary,
	},
	StartupFailed {
		message: String,
	},
}

#[derive(Debug, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
enum RendererControlCommand {
	Shutdown,
	ResetCamera,
	SetCameraOrbit {
		longitude: Option<f32>,
		latitude: Option<f32>,
		radius: Option<f32>,
	},
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
		#[serde(default)]
		minimized: Option<bool>,
		width: Option<u32>,
		height: Option<u32>,
	},
	Screenshot {
		path: String,
	},
	SetWardrobe {
		set_id: String,
	},
	ActivateAction {
		#[serde(default)]
		action_id: Option<String>,
		#[serde(default)]
		supervisor_command: Option<String>,
		#[serde(default, alias = "expressionMenuPath")]
		expression_menu_path: Option<String>,
		#[serde(default, alias = "menuPath")]
		menu_path: Option<String>,
		#[serde(default, alias = "wardrobeSetId")]
		wardrobe_set_id: Option<String>,
		#[serde(default, alias = "parameterName")]
		parameter_name: Option<String>,
		#[serde(default, alias = "parameterValue")]
		parameter_value: Option<f32>,
	},
	SetParameter {
		#[serde(alias = "parameterName")]
		name: String,
		#[serde(alias = "parameterValue")]
		value: f32,
	},
	SetExpressionOverride {
		name: String,
		weight: f32,
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
		#[serde(default)]
		target: Option<[f32; 3]>,
		#[serde(default)]
		longitude_deg: Option<f32>,
		#[serde(default)]
		latitude_deg: Option<f32>,
		#[serde(default)]
		radius: Option<f32>,
		#[serde(default)]
		diagonal_fov_deg: Option<f32>,
		#[serde(default)]
		transition: Option<CameraTransitionOptions>,
	},
	/// プロファイル `[window] x/y` を実行中レンダラーへ反映するときに送られる。
	/// `SetWindow` は inner サイズと装飾のみだったが、こちらは outer 位置を上書きする。
	SetWindowPosition {
		#[serde(default)]
		x: Option<i32>,
		#[serde(default)]
		y: Option<i32>,
	},
	/// VMC `Root.translation` を scene root へ加算するかの切替。
	SetApplyVmcRootTranslation {
		enabled: bool,
	},
	/// 旧 IPC 互換の primary motion source 更新。現在の姿勢適用は key 単位の後着優先。
	SetPrimaryMotionSource {
		source: crate::options::PrimaryMotionSource,
	},
	SetMotionReceivers {
		#[serde(default)]
		vmc_address: Option<SocketAddr>,
		#[serde(default)]
		unmotion_zenoh_enabled: bool,
		#[serde(default)]
		unmotion_zenoh_key: String,
	},
	SetDynamics {
		enabled: bool,
		bone_colliders: BoneColliderConfig,
		/// None means no dynamics physics override; dynamics itself is controlled by `enabled`.
		#[serde(default)]
		#[serde(alias = "physics")]
		physics_config: Option<DynamicsPhysicsConfig>,
	},
	SetSpringBones {
		enabled: bool,
		bone_colliders: BoneColliderConfig,
		/// None means no SpringBone physics override; SpringBone itself is controlled by `enabled`.
		#[serde(default)]
		#[serde(alias = "physics")]
		physics_config: Option<DynamicsPhysicsConfig>,
	},
	SetAvatarOutline {
		#[serde(default)]
		policy: Option<String>,
		#[serde(default, alias = "kind")]
		r#type: Option<String>,
		#[serde(default)]
		width: Option<f32>,
		#[serde(default)]
		color: Option<[f32; 3]>,
		#[serde(default)]
		lighting_mix: Option<f32>,
		#[serde(default)]
		roundness: Option<f32>,
	},
	SetAvatarRim {
		#[serde(default)]
		policy: Option<String>,
		#[serde(default)]
		color: Option<[f32; 3]>,
		#[serde(default)]
		intensity: Option<f32>,
		#[serde(default)]
		lighting_mix: Option<f32>,
		#[serde(default)]
		fresnel_power: Option<f32>,
		#[serde(default)]
		lift: Option<f32>,
	},
	SetAvatarMatcap {
		#[serde(default)]
		scale: Option<f32>,
	},
	SetAvatarSpecular {
		#[serde(default)]
		enabled: Option<bool>,
		#[serde(default)]
		intensity: Option<f32>,
		#[serde(default)]
		power: Option<f32>,
	},
	SetAvatarAmbientOcclusion {
		#[serde(default)]
		strength: Option<f32>,
	},
	SetLighting {
		#[serde(default)]
		environment_enabled: Option<bool>,
		#[serde(default)]
		environment_color: Option<[f32; 3]>,
		#[serde(default)]
		environment_intensity: Option<f32>,
		#[serde(default)]
		directional_enabled: Option<bool>,
		#[serde(default)]
		directional_color: Option<[f32; 3]>,
		#[serde(default)]
		directional_intensity: Option<f32>,
		#[serde(default)]
		#[serde(alias = "directional_longitude_deg")]
		directional_azimuth_deg: Option<f32>,
		#[serde(default)]
		#[serde(alias = "directional_latitude_deg")]
		directional_elevation_deg: Option<f32>,
		#[serde(default)]
		directional_follow_camera_yaw: Option<bool>,
		#[serde(default)]
		#[serde(rename = "directional_reference")]
		/// Deprecated control spelling. Kept only for existing local tools.
		legacy_directional_reference: Option<String>,
		#[serde(default)]
		directional_follow_camera_pitch: Option<bool>,
	},
	SetEnvironmentColor {
		#[serde(default)]
		exposure: Option<f32>,
		#[serde(default)]
		contrast: Option<f32>,
		#[serde(default)]
		saturation: Option<f32>,
		#[serde(default)]
		look: Option<String>,
		#[serde(default)]
		intensity: Option<f32>,
		#[serde(default)]
		temperature: Option<f32>,
		#[serde(default)]
		tint: Option<f32>,
	},
	SetBloom {
		#[serde(default)]
		enabled: Option<bool>,
		#[serde(default)]
		strength: Option<f32>,
		#[serde(default)]
		threshold: Option<f32>,
		#[serde(default)]
		radius: Option<f32>,
		#[serde(default)]
		quality: Option<String>,
	},
	SetSsao {
		#[serde(default)]
		enabled: Option<bool>,
		#[serde(default)]
		strength: Option<f32>,
		#[serde(default)]
		radius: Option<f32>,
		#[serde(default)]
		bias: Option<f32>,
		#[serde(default)]
		range: Option<f32>,
	},
	SetContactShadow {
		#[serde(default)]
		enabled: Option<bool>,
		#[serde(default)]
		strength: Option<f32>,
		#[serde(default)]
		radius: Option<f32>,
		#[serde(default)]
		softness: Option<f32>,
		#[serde(default)]
		height: Option<f32>,
	},
}

impl RendererControlCommand {
	fn into_event(self) -> RendererControlEvent {
		match self {
			Self::Shutdown => RendererControlEvent::Shutdown,
			Self::ResetCamera => RendererControlEvent::ResetCamera,
			Self::SetCameraOrbit {
				longitude,
				latitude,
				radius,
			} => RendererControlEvent::SetCameraOrbit {
				longitude,
				latitude,
				radius,
			},
			Self::SetClearColor { r, g, b, a } => RendererControlEvent::SetClearColor {
				r: color_component(r),
				g: color_component(g),
				b: color_component(b),
				a: color_component(a),
			},
			Self::SetSpoutOutput {
				enabled,
				name,
				width,
				height,
			} => RendererControlEvent::SetSpoutOutput {
				enabled,
				name,
				width,
				height,
			},
			Self::SetWindow {
				decorations,
				transparent,
				input_passthrough,
				always_on_top,
				minimized,
				width,
				height,
			} => RendererControlEvent::SetWindow {
				decorations,
				transparent,
				input_passthrough,
				always_on_top,
				minimized,
				width,
				height,
			},
			Self::Screenshot { .. } => unreachable!("Screenshot は runtime_control_response で個別に処理する"),
			Self::SetWardrobe { .. } => unreachable!("SetWardrobe は runtime_control_response で個別に処理する"),
			Self::ActivateAction { .. } => unreachable!("ActivateAction は runtime_control_response で個別に処理する"),
			Self::SetParameter { .. } => unreachable!("SetParameter は runtime_control_response で個別に処理する"),
			Self::SetExpressionOverride { name, weight } => RendererControlEvent::SetExpressionOverride { name, weight },
			Self::ClearExpressionOverrides => RendererControlEvent::ClearExpressionOverrides,
			Self::SetLookAt { enabled, clamp_deg } => RendererControlEvent::SetLookAt { enabled, clamp_deg },
			Self::Activate => RendererControlEvent::Activate,
			Self::SetShowAxes { enabled } => RendererControlEvent::SetShowAxes { enabled },
			Self::SetShowBoneColliders { enabled } => RendererControlEvent::SetShowBoneColliders { enabled },
			Self::SetCameraLock { locked } => RendererControlEvent::SetCameraLock { locked },
			Self::SetCameraFov { diagonal_deg } => RendererControlEvent::SetCameraFov { diagonal_deg },
			Self::SetCameraState {
				target,
				longitude_deg,
				latitude_deg,
				radius,
				diagonal_fov_deg,
				transition,
			} => RendererControlEvent::SetCameraState {
				target,
				longitude_deg,
				latitude_deg,
				radius,
				diagonal_fov_deg,
				transition,
			},
			Self::SetWindowPosition { x, y } => RendererControlEvent::SetWindowPosition { x, y },
			Self::SetApplyVmcRootTranslation { enabled } => RendererControlEvent::SetApplyVmcRootTranslation { enabled },
			Self::SetPrimaryMotionSource { source } => RendererControlEvent::SetPrimaryMotionSource { source },
			Self::SetMotionReceivers {
				vmc_address,
				unmotion_zenoh_enabled,
				unmotion_zenoh_key,
			} => RendererControlEvent::SetMotionReceivers {
				vmc_address,
				unmotion_zenoh_enabled,
				unmotion_zenoh_key,
			},
			Self::SetDynamics {
				enabled,
				bone_colliders,
				physics_config,
			}
			| Self::SetSpringBones {
				enabled,
				bone_colliders,
				physics_config,
			} => RendererControlEvent::SetDynamics {
				enabled,
				bone_colliders,
				physics_config,
			},
			Self::SetAvatarOutline {
				policy,
				r#type,
				width,
				color,
				lighting_mix,
				roundness,
			} => RendererControlEvent::SetAvatarOutline {
				policy,
				r#type,
				width,
				color,
				lighting_mix,
				roundness,
			},
			Self::SetAvatarRim {
				policy,
				color,
				intensity,
				lighting_mix,
				fresnel_power,
				lift,
			} => RendererControlEvent::SetAvatarRim {
				policy,
				color,
				intensity,
				lighting_mix,
				fresnel_power,
				lift,
			},
			Self::SetAvatarMatcap { scale } => RendererControlEvent::SetAvatarMatcap { scale },
			Self::SetAvatarSpecular { enabled, intensity, power } => RendererControlEvent::SetAvatarSpecular { enabled, intensity, power },
			Self::SetAvatarAmbientOcclusion { strength } => RendererControlEvent::SetAvatarAmbientOcclusion { strength },
			Self::SetLighting {
				environment_enabled,
				environment_color,
				environment_intensity,
				directional_enabled,
				directional_color,
				directional_intensity,
				directional_azimuth_deg,
				directional_elevation_deg,
				directional_follow_camera_yaw,
				legacy_directional_reference,
				directional_follow_camera_pitch,
			} => RendererControlEvent::SetLighting {
				environment_enabled,
				environment_color,
				environment_intensity,
				directional_enabled,
				directional_color,
				directional_intensity,
				directional_azimuth_deg,
				directional_elevation_deg,
				directional_follow_camera_yaw: directional_follow_camera_yaw.or_else(|| {
					legacy_directional_reference
						.as_deref()
						.and_then(legacy_light_reference_to_follow_camera_yaw)
				}),
				directional_follow_camera_pitch,
			},
			Self::SetEnvironmentColor {
				exposure,
				contrast,
				saturation,
				look,
				intensity,
				temperature,
				tint,
			} => RendererControlEvent::SetEnvironmentColor {
				exposure,
				contrast,
				saturation,
				look,
				intensity,
				temperature,
				tint,
			},
			Self::SetBloom {
				enabled,
				strength,
				threshold,
				radius,
				quality,
			} => RendererControlEvent::SetBloom {
				enabled,
				strength,
				threshold,
				radius,
				quality,
			},
			Self::SetSsao {
				enabled,
				strength,
				radius,
				bias,
				range,
			} => RendererControlEvent::SetSsao {
				enabled,
				strength,
				radius,
				bias,
				range,
			},
			Self::SetContactShadow {
				enabled,
				strength,
				radius,
				softness,
				height,
			} => RendererControlEvent::SetContactShadow {
				enabled,
				strength,
				radius,
				softness,
				height,
			},
		}
	}
}

fn color_component(value: f64) -> f64 {
	if value.is_finite() {
		value.clamp(0.0, 1.0)
	} else {
		0.0
	}
}

fn apply_camera_state_patch(gpu: &mut GpuState, patch: CameraStatePatch) {
	gpu.set_camera_state(
		patch.target,
		patch.longitude_deg,
		patch.latitude_deg,
		patch.radius,
		patch.diagonal_fov_deg,
	);
}

fn patched_camera_state(mut state: gpu::CameraStateSnapshot, patch: CameraStatePatch) -> gpu::CameraStateSnapshot {
	if let Some(target) = patch.target {
		state.target = target;
	}
	if let Some(longitude_deg) = patch.longitude_deg {
		state.longitude_deg = longitude_deg;
	}
	if let Some(latitude_deg) = patch.latitude_deg {
		state.latitude_deg = latitude_deg;
	}
	if let Some(radius) = patch.radius {
		state.radius = radius;
	}
	if let Some(diagonal_fov_deg) = patch.diagonal_fov_deg {
		state.diagonal_fov_deg = diagonal_fov_deg;
	}
	state
}

fn ease_camera_transition(t: f32, easing: CameraTransitionEasing) -> f32 {
	let t = t.clamp(0.0, 1.0);
	match easing {
		CameraTransitionEasing::Linear => t,
		CameraTransitionEasing::EaseOutCubic => 1.0 - (1.0 - t).powi(3),
	}
}

fn lerp_camera_state(start: gpu::CameraStateSnapshot, end: gpu::CameraStateSnapshot, t: f32) -> gpu::CameraStateSnapshot {
	gpu::CameraStateSnapshot {
		target: [
			lerp(start.target[0], end.target[0], t),
			lerp(start.target[1], end.target[1], t),
			lerp(start.target[2], end.target[2], t),
		],
		longitude_deg: lerp_angle_deg(start.longitude_deg, end.longitude_deg, t),
		latitude_deg: lerp(start.latitude_deg, end.latitude_deg, t),
		radius: lerp(start.radius, end.radius, t),
		diagonal_fov_deg: lerp(start.diagonal_fov_deg, end.diagonal_fov_deg, t),
	}
}

fn lerp(start: f32, end: f32, t: f32) -> f32 {
	start + (end - start) * t
}

fn lerp_angle_deg(start: f32, end: f32, t: f32) -> f32 {
	start + shortest_angle_delta_deg(start, end) * t
}

fn shortest_angle_delta_deg(start: f32, end: f32) -> f32 {
	(end - start + 540.0).rem_euclid(360.0) - 180.0
}

fn parse_avatar_outline_policy(value: Option<&str>) -> Option<AvatarOutlinePolicy> {
	match value?.trim().to_ascii_lowercase().as_str() {
		"authored" => Some(AvatarOutlinePolicy::Authored),
		"off" | "none" | "disabled" => Some(AvatarOutlinePolicy::Off),
		"override" | "custom" => Some(AvatarOutlinePolicy::Override),
		_ => None,
	}
}

fn parse_avatar_outline_kind(value: Option<&str>) -> Option<AvatarOutlineKind> {
	match value?.trim().to_ascii_lowercase().as_str() {
		"mtoon" | "geometry" => Some(AvatarOutlineKind::Mtoon),
		"ink" => Some(AvatarOutlineKind::Ink),
		"brush" | "hake" | "fude" => Some(AvatarOutlineKind::Brush),
		"double" | "double_outline" => Some(AvatarOutlineKind::Double),
		_ => None,
	}
}

fn parse_avatar_rim_policy(value: Option<&str>) -> Option<AvatarRimPolicy> {
	match value?.trim().to_ascii_lowercase().as_str() {
		"authored" => Some(AvatarRimPolicy::Authored),
		"off" | "none" | "disabled" => Some(AvatarRimPolicy::Off),
		"override" | "custom" => Some(AvatarRimPolicy::Override),
		_ => None,
	}
}

fn avatar_outline_from_control(
	current: AvatarOutlineOptions,
	policy: Option<String>,
	kind: Option<String>,
	width: Option<f32>,
	color: Option<[f32; 3]>,
	lighting_mix: Option<f32>,
	roundness: Option<f32>,
) -> AvatarOutlineOptions {
	AvatarOutlineOptions {
		policy: parse_avatar_outline_policy(policy.as_deref()).unwrap_or(current.policy),
		kind: parse_avatar_outline_kind(kind.as_deref()).unwrap_or(current.kind),
		width: width.map(|v| v.clamp(0.0, 0.05)).or(current.width),
		color: color
			.map(|rgb| [rgb[0].clamp(0.0, 1.0), rgb[1].clamp(0.0, 1.0), rgb[2].clamp(0.0, 1.0)])
			.or(current.color),
		lighting_mix: lighting_mix.map(|v| v.clamp(0.0, 1.0)).or(current.lighting_mix),
		roundness: roundness.map(|v| v.clamp(0.0, 1.0)).or(current.roundness),
	}
}

fn avatar_rim_from_control(
	current: AvatarRimOptions,
	policy: Option<String>,
	color: Option<[f32; 3]>,
	intensity: Option<f32>,
	lighting_mix: Option<f32>,
	fresnel_power: Option<f32>,
	lift: Option<f32>,
) -> AvatarRimOptions {
	AvatarRimOptions {
		policy: parse_avatar_rim_policy(policy.as_deref()).unwrap_or(current.policy),
		color: color
			.map(|rgb| [rgb[0].clamp(0.0, 1.0), rgb[1].clamp(0.0, 1.0), rgb[2].clamp(0.0, 1.0)])
			.or(current.color),
		intensity: intensity.map(|v| v.clamp(0.0, 4.0)).or(current.intensity),
		lighting_mix: lighting_mix.map(|v| v.clamp(0.0, 1.0)).or(current.lighting_mix),
		fresnel_power: fresnel_power.map(|v| v.max(0.00001)).or(current.fresnel_power),
		lift: lift.map(|v| v.clamp(-1.0, 1.0)).or(current.lift),
	}
}

fn avatar_matcap_from_control(current: AvatarMatcapOptions, scale: Option<f32>) -> AvatarMatcapOptions {
	AvatarMatcapOptions {
		scale: scale.unwrap_or(current.scale).clamp(0.0, 2.0),
	}
}

fn avatar_specular_from_control(
	current: AvatarSpecularOptions,
	enabled: Option<bool>,
	intensity: Option<f32>,
	power: Option<f32>,
) -> AvatarSpecularOptions {
	AvatarSpecularOptions {
		enabled: enabled.unwrap_or(current.enabled),
		intensity: intensity.unwrap_or(current.intensity).clamp(0.0, 2.0),
		power: power.unwrap_or(current.power).clamp(1.0, 128.0),
	}
}

fn avatar_ambient_occlusion_from_control(current: AvatarAmbientOcclusionOptions, strength: Option<f32>) -> AvatarAmbientOcclusionOptions {
	AvatarAmbientOcclusionOptions {
		strength: strength.unwrap_or(current.strength).clamp(0.0, 2.0),
	}
}

fn clamp_rgb_control(rgb: [f32; 3]) -> [f32; 3] {
	[rgb[0].clamp(0.0, 1.0), rgb[1].clamp(0.0, 1.0), rgb[2].clamp(0.0, 1.0)]
}

fn legacy_light_reference_to_follow_camera_yaw(value: &str) -> Option<bool> {
	match value.trim().to_ascii_lowercase().as_str() {
		"camera" => Some(true),
		"world" | "model" => Some(false),
		_ => None,
	}
}

#[allow(clippy::too_many_arguments)]
fn lighting_from_control(
	current: LightingOptions,
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
) -> LightingOptions {
	LightingOptions {
		environment: EnvironmentLightOptions {
			enabled: environment_enabled.unwrap_or(current.environment.enabled),
			color: environment_color.map(clamp_rgb_control).unwrap_or(current.environment.color),
			intensity: environment_intensity.unwrap_or(current.environment.intensity).clamp(0.0, 2.0),
		},
		directional: DirectionalLightOptions {
			enabled: directional_enabled.unwrap_or(current.directional.enabled),
			color: directional_color.map(clamp_rgb_control).unwrap_or(current.directional.color),
			intensity: directional_intensity.unwrap_or(current.directional.intensity).clamp(0.0, 4.0),
			azimuth_deg: directional_azimuth_deg
				.unwrap_or(current.directional.azimuth_deg)
				.clamp(-360.0, 360.0),
			elevation_deg: directional_elevation_deg
				.unwrap_or(current.directional.elevation_deg)
				.clamp(-89.0, 89.0),
			follow_camera_yaw: directional_follow_camera_yaw.unwrap_or(current.directional.follow_camera_yaw),
			follow_camera_pitch: directional_follow_camera_pitch.unwrap_or(current.directional.follow_camera_pitch),
		},
	}
}

#[allow(clippy::too_many_arguments)]
fn environment_color_from_control(
	current: EnvironmentColorOptions,
	exposure: Option<f32>,
	contrast: Option<f32>,
	saturation: Option<f32>,
	look: Option<String>,
	intensity: Option<f32>,
	temperature: Option<f32>,
	tint: Option<f32>,
) -> EnvironmentColorOptions {
	let next_look = look
		.as_deref()
		.and_then(|value| value.parse::<ColorGradingLook>().ok())
		.unwrap_or(current.look);
	let mut next = EnvironmentColorOptions {
		exposure: exposure.unwrap_or(current.exposure).clamp(-4.0, 4.0),
		contrast: contrast.unwrap_or(current.contrast).clamp(0.0, 4.0),
		saturation: saturation.unwrap_or(current.saturation).clamp(0.0, 4.0),
		look: next_look,
		look_intensity: intensity.unwrap_or(current.look_intensity).clamp(0.0, 1.0),
		temperature: temperature.unwrap_or(current.temperature).clamp(-1.0, 1.0),
		tint: tint.unwrap_or(current.tint).clamp(-1.0, 1.0),
	};
	if matches!(next.look, ColorGradingLook::Neutral) {
		next.look_intensity = 0.0;
	}
	next
}

fn bloom_from_control(
	current: BloomOptions,
	enabled: Option<bool>,
	strength: Option<f32>,
	threshold: Option<f32>,
	radius: Option<f32>,
	quality: Option<String>,
) -> BloomOptions {
	BloomOptions {
		enabled: enabled.unwrap_or(current.enabled),
		strength: strength.unwrap_or(current.strength).clamp(0.0, 2.0),
		threshold: threshold.unwrap_or(current.threshold).clamp(0.0, 2.0),
		radius: radius.unwrap_or(current.radius).clamp(0.0, 32.0),
		quality: quality
			.as_deref()
			.and_then(|value| value.parse::<BloomQuality>().ok())
			.unwrap_or(current.quality),
	}
}

fn ssao_from_control(
	current: SsaoOptions,
	enabled: Option<bool>,
	strength: Option<f32>,
	radius: Option<f32>,
	bias: Option<f32>,
	range: Option<f32>,
) -> SsaoOptions {
	SsaoOptions {
		enabled: enabled.unwrap_or(current.enabled),
		strength: strength.unwrap_or(current.strength).clamp(0.0, 1.0),
		radius: radius.unwrap_or(current.radius).clamp(1.0, 24.0),
		bias: bias.unwrap_or(current.bias).clamp(0.0, 0.02),
		range: range.unwrap_or(current.range).clamp(0.001, 0.2),
	}
}

fn contact_shadow_from_control(
	current: ContactShadowOptions,
	enabled: Option<bool>,
	strength: Option<f32>,
	radius: Option<f32>,
	softness: Option<f32>,
	height: Option<f32>,
) -> ContactShadowOptions {
	ContactShadowOptions {
		enabled: enabled.unwrap_or(current.enabled),
		strength: strength.unwrap_or(current.strength).clamp(0.0, 1.0),
		radius: radius.unwrap_or(current.radius).clamp(0.05, 3.0),
		softness: softness.unwrap_or(current.softness).clamp(0.1, 8.0),
		height: height.unwrap_or(current.height).clamp(-1.0, 1.0),
	}
}

/// ダブルクリック判定の最大間隔（Windows のシステムデフォルト 500ms より少し短めの 350ms に設定）。
const DOUBLE_CLICK_THRESHOLD: Duration = Duration::from_millis(350);

#[derive(Clone, Debug)]
enum StartupPhase {
	Model,
	TextureCache,
	TextureCompression,
	GpuUpload,
	Other(String),
}

impl StartupPhase {
	fn as_str(&self) -> &str {
		match self {
			Self::Model => "model",
			Self::TextureCache => "texture-cache",
			Self::TextureCompression => "texture-compression",
			Self::GpuUpload => "gpu-upload",
			Self::Other(phase) => phase.as_str(),
		}
	}

	fn splash_code(&self) -> f32 {
		match self {
			Self::Model => 1.0,
			Self::TextureCache => 2.0,
			Self::TextureCompression => 3.0,
			Self::GpuUpload => 4.0,
			Self::Other(_) => 0.0,
		}
	}
}

impl From<&str> for StartupPhase {
	fn from(value: &str) -> Self {
		match value {
			"model" => Self::Model,
			"texture-cache" => Self::TextureCache,
			"texture-compression" => Self::TextureCompression,
			"gpu-upload" => Self::GpuUpload,
			other => Self::Other(other.to_string()),
		}
	}
}

impl From<String> for StartupPhase {
	fn from(value: String) -> Self {
		match value.as_str() {
			"model" => Self::Model,
			"texture-cache" => Self::TextureCache,
			"texture-compression" => Self::TextureCompression,
			"gpu-upload" => Self::GpuUpload,
			_ => Self::Other(value),
		}
	}
}

fn startup_elapsed_suffix(started_at: Instant) -> String {
	let elapsed = started_at.elapsed();
	if elapsed.as_secs() >= 1 {
		format!(" ({:.1}s)", elapsed.as_secs_f32())
	} else {
		format!(" ({}ms)", elapsed.as_millis())
	}
}

fn startup_message(message: impl AsRef<str>, started_at: Instant) -> String {
	format!("{}{}", message.as_ref(), startup_elapsed_suffix(started_at))
}

fn compact_window_title_status(status: impl AsRef<str>) -> String {
	let status = status.as_ref();
	let mut compact = String::with_capacity(status.len().min(WINDOW_TITLE_STATUS_MAX_CHARS));
	for part in status.split_whitespace() {
		if !compact.is_empty() {
			compact.push(' ');
		}
		compact.push_str(part);
	}
	if compact.len() > WINDOW_TITLE_STATUS_MAX_CHARS {
		let mut char_indices = compact.char_indices();
		let truncate_at = char_indices
			.nth(WINDOW_TITLE_STATUS_MAX_CHARS.saturating_sub(1))
			.and_then(|(index, _)| char_indices.next().map(|_| index));
		if let Some(index) = truncate_at {
			compact.truncate(index);
			compact.push('…');
		}
	}
	compact
}

#[derive(Clone, Debug)]
struct StartupProgressState {
	phase: StartupPhase,
	current: u32,
	total: u32,
	message: String,
}

impl StartupProgressState {
	fn normalized_progress(&self) -> f32 {
		if self.total > 0 {
			(self.current as f32 / self.total as f32).clamp(0.0, 1.0)
		} else {
			-1.0
		}
	}
}

struct AvatarApp {
	opts: AvatarWindowOptions,
	title_base: String,
	event_proxy: EventLoopProxy<RendererControlEvent>,
	window: Option<Arc<Window>>,
	gpu: Option<GpuState>,
	startup_progress: Option<StartupProgressState>,
	startup_pending_document: bool,
	startup_failed: Option<String>,
	runtime_status: Option<Arc<Mutex<RendererRuntimeSnapshot>>>,
	last_wall: Instant,
	started_at: Instant,
	fps_smooth: f32,
	runtime_status_frame_seq: Cell<u32>,
	window_focused: bool,
	window_activation_seq: u64,
	title_refresh: u32,
	pending_surface_size: Option<(u32, u32)>,
	last_resize_at: Option<Instant>,
	right_dragging: bool,
	middle_dragging: bool,
	last_cursor_pos: Option<PhysicalPosition<f64>>,
	/// マウス右ボタン直前 press 時刻（ダブルクリック判定で回転 reset 用）。
	last_right_press: Option<Instant>,
	/// マウス中ボタン直前 press 時刻（ダブルクリック判定で全体 reset 用）。
	last_middle_press: Option<Instant>,
	/// IPC からのカメラロック。true の間はマウスドラッグ / ホイールでカメラ操作不可。
	camera_locked: bool,
	current_modifiers: ModifiersState,
	close_hotkey: Option<CloseHotkey>,
	camera_transition_queue: VecDeque<QueuedCameraTransition>,
	active_camera_transition: Option<ActiveCameraTransition>,
}

impl AvatarApp {
	fn new(mut opts: AvatarWindowOptions, event_proxy: EventLoopProxy<RendererControlEvent>) -> Self {
		if opts.transparent {
			opts.clear_color.a = 0.0;
		} else {
			opts.clear_color.a = 1.0;
		}
		let title_base = opts.title.clone();
		let runtime_status = if let Some(base_key) = opts.runtime_bus_key.clone() {
			Some(start_runtime_bus(base_key, &opts, event_proxy.clone()))
		} else {
			opts.runtime_status_address
				.map(|address| start_runtime_status_server(address, &opts))
		};
		let close_hotkey = match CloseHotkey::parse(&opts.close_hotkey) {
			Ok(close_hotkey) => close_hotkey,
			Err(error) => {
				eprintln!("un-avatar-renderer: invalid --close-hotkey '{}': {error}", opts.close_hotkey);
				CloseHotkey::parse("Escape").ok().flatten()
			}
		};
		let camera_locked = opts.camera_locked;
		Self {
			opts,
			title_base,
			event_proxy,
			window: None,
			gpu: None,
			startup_progress: None,
			startup_pending_document: false,
			startup_failed: None,
			runtime_status,
			last_wall: Instant::now(),
			started_at: Instant::now(),
			fps_smooth: 60.0,
			runtime_status_frame_seq: Cell::new(0),
			window_focused: false,
			window_activation_seq: 0,
			title_refresh: 0,
			pending_surface_size: None,
			last_resize_at: None,
			right_dragging: false,
			middle_dragging: false,
			last_cursor_pos: None,
			last_right_press: None,
			last_middle_press: None,
			camera_locked,
			current_modifiers: ModifiersState::default(),
			close_hotkey,
			camera_transition_queue: VecDeque::new(),
			active_camera_transition: None,
		}
	}

	fn request_redraw(&self) {
		if let Some(w) = &self.window {
			w.request_redraw();
		}
	}

	fn reconfigure(&mut self, width: u32, height: u32) {
		self.pending_surface_size = Some((width, height));
		self.last_resize_at = Some(Instant::now());
		self.update_runtime_surface(width, height);
	}

	fn apply_pending_reconfigure(&mut self, now: Instant, window: &Window) -> bool {
		let Some((pending_width, pending_height)) = self.pending_surface_size else {
			return false;
		};
		if self
			.last_resize_at
			.is_some_and(|resized_at| now.saturating_duration_since(resized_at) < SURFACE_RESIZE_SETTLE_DELAY)
		{
			return true;
		}

		let size = window.inner_size();
		let width = if size.width == 0 { pending_width } else { size.width };
		let height = if size.height == 0 { pending_height } else { size.height };
		if width == 0 || height == 0 {
			return true;
		}
		if let Some(gpu) = self.gpu.as_mut() {
			gpu.resize(width, height);
		}
		self.pending_surface_size = None;
		self.last_resize_at = None;
		self.update_runtime_surface(width, height);
		false
	}

	fn update_runtime_surface(&self, width: u32, height: u32) {
		let Some(status) = &self.runtime_status else {
			return;
		};
		if let Ok(mut status) = status.lock() {
			status.surface_width = Some(width.max(1));
			status.surface_height = Some(height.max(1));
		}
	}

	/// 現在のウィンドウ outer 位置 / inner サイズを runtime telemetry に反映する。
	/// `WindowEvent::Moved` / `WindowEvent::Resized` から呼ばれる。
	fn update_runtime_window_geometry(&self) {
		let Some(window) = self.window.as_ref() else {
			return;
		};
		let Some(status) = &self.runtime_status else {
			return;
		};
		let Ok(mut status) = status.lock() else { return };
		if let Ok(pos) = window.outer_position() {
			status.window_position = Some([pos.x, pos.y]);
		}
		let size = window.inner_size();
		status.window_inner_size = Some([size.width, size.height]);
	}

	fn update_runtime_focus_status(&self) {
		let Some(status) = &self.runtime_status else {
			return;
		};
		if let Ok(mut status) = status.lock() {
			status.window_focused = self.window_focused;
			status.window_activation_seq = self.window_activation_seq;
		}
	}

	#[allow(clippy::too_many_arguments)]
	fn apply_window_runtime(
		&mut self,
		decorations: Option<bool>,
		transparent: Option<bool>,
		input_passthrough: Option<bool>,
		always_on_top: Option<bool>,
		minimized: Option<bool>,
		width: Option<u32>,
		height: Option<u32>,
	) {
		if let Some(decorations) = decorations {
			self.opts.decorations = decorations;
			if let Some(window) = &self.window {
				window.set_decorations(decorations);
				if decorations {
					window.set_cursor(CursorIcon::Default);
				}
			}
		}
		if let Some(always_on_top) = always_on_top {
			self.opts.always_on_top = always_on_top;
			if let Some(window) = &self.window {
				window.set_window_level(if always_on_top {
					WindowLevel::AlwaysOnTop
				} else {
					WindowLevel::Normal
				});
			}
		}
		if let Some(transparent) = transparent {
			self.opts.transparent = transparent;
			if transparent {
				self.opts.clear_color.a = 0.0;
			} else {
				self.opts.clear_color.a = 1.0;
				self.opts.input_passthrough = false;
			}
			if let Some(window) = &self.window {
				window.set_transparent(transparent);
			}
			if let Some(gpu) = self.gpu.as_mut() {
				gpu.set_transparent(transparent);
			}
		}
		if let Some(input_passthrough) = input_passthrough {
			self.opts.input_passthrough = input_passthrough;
		}
		if !self.opts.transparent {
			self.opts.input_passthrough = false;
		}
		if let Some(window) = &self.window {
			apply_window_hittest(window, self.opts.transparent, self.opts.input_passthrough);
		}
		if width.is_some() || height.is_some() {
			let width = width.unwrap_or(self.opts.window_width).max(1);
			let height = height.unwrap_or(self.opts.window_height).max(1);
			self.opts.window_width = width;
			self.opts.window_height = height;
			if let Some(window) = &self.window {
				if let Some(size) = window.request_inner_size(PhysicalSize::new(width, height)) {
					self.reconfigure(size.width, size.height);
				}
			}
		}
		if let Some(minimized) = minimized {
			if let Some(window) = &self.window {
				window.set_minimized(minimized);
				if !minimized {
					window.set_visible(true);
				}
			}
		}
		self.request_redraw();
	}

	fn hide_window_for_shutdown(&self) {
		if let Some(window) = &self.window {
			window.set_visible(false);
		}
	}

	fn update_runtime_frame(&self, timings: &FrameTimings) {
		let Some(status) = &self.runtime_status else {
			return;
		};
		if let Ok(mut status) = status.lock() {
			let gpu = self.gpu.as_ref();
			let runtime_status_frame_seq = self.runtime_status_frame_seq.get().wrapping_add(1);
			self.runtime_status_frame_seq.set(runtime_status_frame_seq);
			status.uptime_secs = self.started_at.elapsed().as_secs();
			status.fps = Some(self.fps_smooth);
			status.cpu_ms = Some(timings.cpu_record_ms);
			status.gpu_ms = Some(timings.gpu_ms);
			if status.ram_mb.is_none() || runtime_status_frame_seq.is_multiple_of(30) {
				status.ram_mb = memory_stats::memory_stats().map(|snapshot| snapshot.physical_mem as u64 / 1_048_576);
			}
			let presets = gpu.map(|g| g.expression_presets()).unwrap_or(&[]);
			if status.expression_presets.as_slice() != presets {
				status.expression_presets = presets.to_vec();
			}
			let clamp = gpu.and_then(|g| g.eye_look_at_clamp_deg());
			status.look_at_enabled = clamp.is_some();
			status.look_at_clamp_deg = clamp;
			status.apply_vmc_root_translation = gpu.is_some_and(|g| g.apply_vmc_root_translation());
			status.unmotion_zenoh_enabled = gpu.is_some_and(|g| g.unmotion_zenoh_live());
			if status.unmotion_zenoh_key != self.opts.unmotion_zenoh.base_key_expr {
				status.unmotion_zenoh_key.clone_from(&self.opts.unmotion_zenoh.base_key_expr);
			}
			status.unmotion_zenoh_received_frames = gpu.map_or(0, |g| g.unmotion_zenoh_received_frames());
			status.motion_applied_frames = gpu.map_or(0, |g| g.motion_applied_frames());
			status.audio_link_texture_needed = gpu.is_some_and(|g| g.audio_link_texture_needed());
			let runtime_requirements = gpu.map(|g| g.runtime_requirements()).unwrap_or_default();
			status.runtime_requires_audio_link_texture = runtime_requirements.audio_link_texture;
			status.runtime_requires_screen_refraction = runtime_requirements.screen_refraction;
			status.runtime_requires_fur = runtime_requirements.fur;
			status.wardrobe_asset_upload = gpu.map(|g| g.wardrobe_asset_upload_plan()).unwrap_or_default();
			status.runtime_parameter_definitions = gpu.map(|g| g.runtime_parameter_definitions()).unwrap_or_default();
			status.runtime_parameter_conflicts = gpu.map(|g| g.runtime_parameter_conflicts()).unwrap_or_default();
			status.wardrobe_actions = gpu.map(|g| g.wardrobe_actions()).unwrap_or_default();
			status.runtime_actions = gpu.map(|g| g.runtime_actions()).unwrap_or_default();
			status.runtime_action_target_write_collisions = gpu.map(|g| g.runtime_action_target_write_collisions()).unwrap_or_default();
			status.runtime_action_restore_readiness = gpu.map(|g| g.runtime_action_restore_readiness()).unwrap_or_default();
			status.runtime_action_restore_baseline_candidates =
				gpu.map(|g| g.runtime_action_restore_baseline_candidates()).unwrap_or_default();
			status.runtime_action_restore_baseline_capture_plan =
				gpu.map(|g| g.runtime_action_restore_baseline_capture_plan()).unwrap_or_default();
			status.runtime_action_restore_apply_plan = gpu.map(|g| g.runtime_action_restore_apply_plan()).unwrap_or_default();
			status.menu_action_candidates = gpu.map(|g| g.menu_action_candidates()).unwrap_or_default();
			status.menu_wardrobe_candidates = gpu.map(|g| g.menu_wardrobe_candidates()).unwrap_or_default();
			status.contact_parameter_declarations = gpu.map(|g| g.contact_parameter_declarations()).unwrap_or_default();
			status.contact_parameter_emission_enabled = gpu.map(|g| g.contact_parameter_emission_enabled()).unwrap_or(false);
			status.primary_motion_source = gpu.map(|g| g.primary_motion_source()).unwrap_or(self.opts.primary_motion_source);
			status.show_axes = gpu.is_some_and(|g| g.show_axes());
			status.show_bone_colliders = gpu.is_some_and(|g| g.show_bone_colliders());
			status.bone_collider_count = gpu.map_or(0, |g| g.bone_collider_count());
			let bone_collider_source = gpu.map_or("off", |g| g.bone_collider_source());
			if status.bone_collider_source != bone_collider_source {
				status.bone_collider_source = bone_collider_source.to_string();
			}
			if runtime_status_frame_seq == 1 || runtime_status_frame_seq.is_multiple_of(30) {
				let dynamics = gpu.map_or(Default::default(), |g| g.dynamics_counts());
				status.dynamics_group_count = dynamics.groups;
				status.dynamics_enabled_group_count = dynamics.enabled_groups;
				status.dynamics_source_enabled_group_count = dynamics.source_enabled_groups;
				status.dynamics_enabled_override_count = dynamics.runtime_enabled_overrides;
				status.dynamics_vrm_spring_bone_group_count = dynamics.vrm_spring_bone_groups;
				status.dynamics_vrc_physbone_group_count = dynamics.vrc_physbone_groups;
				status.dynamics_unknown_group_count = dynamics.unknown_groups;
				status.dynamics_limit_group_count = dynamics.limit_groups;
				status.dynamics_angle_limit_group_count = dynamics.angle_limit_groups;
				status.dynamics_stretch_limit_group_count = dynamics.stretch_limit_groups;
				status.dynamics_grabbing_enabled_group_count = dynamics.grabbing_enabled_groups;
				status.dynamics_posing_enabled_group_count = dynamics.posing_enabled_groups;
				status.dynamics_collider_count = dynamics.colliders;
				status.dynamics_vrm_spring_bone_collider_count = dynamics.vrm_spring_bone_colliders;
				status.dynamics_vrc_physbone_collider_count = dynamics.vrc_physbone_colliders;
				status.dynamics_unknown_collider_count = dynamics.unknown_colliders;
				status.dynamics_contact_count = dynamics.contacts;
				status.dynamics_vrc_contact_sender_count = dynamics.vrc_contact_senders;
				status.dynamics_vrc_contact_receiver_count = dynamics.vrc_contact_receivers;
				status.dynamics_contact_parameter_declaration_count = dynamics.contact_parameter_declarations;
				let contact_probe_status = gpu.map(|g| g.contact_probe_status()).unwrap_or_default();
				status.contact_probes = contact_probe_status.probes;
				status.dynamics_contact_probe_count = contact_probe_status.count;
				status.dynamics_contact_probe_would_emit_count = contact_probe_status.would_emit_count;
				let contact_emission_status = gpu.map(|g| g.contact_parameter_emission_status()).unwrap_or_default();
				status.contact_parameter_emissions = contact_emission_status.emissions;
				status.dynamics_contact_parameter_emission_count = contact_emission_status.count;
				status.dynamics_contact_parameter_emitted_count = contact_emission_status.emitted_count;
				status.dynamics_contact_parameter_reset_to_zero_count = contact_emission_status.reset_to_zero_count;
				status.dynamics_constraint_ref_count = dynamics.constraint_refs;
				status.dynamics_vrc_constraint_ref_count = dynamics.vrc_constraint_refs;
				status.dynamics_groups = gpu.map(|g| g.dynamics_groups()).unwrap_or_default();
				status.dynamics_colliders = gpu.map(|g| g.dynamics_colliders()).unwrap_or_default();
				status.dynamics_constraint_refs = gpu.map(|g| g.dynamics_constraint_refs()).unwrap_or_default();
			}
			status.camera_locked = self.camera_locked;
			status.window_focused = self.window_focused;
			status.window_activation_seq = self.window_activation_seq;
			status.minimized = self.window.as_ref().is_some_and(|w| w.is_minimized().unwrap_or(false));
			status.camera = gpu.map(|g| g.camera_state_snapshot());
			let c = self.opts.clear_color;
			status.clear_color = [c.r, c.g, c.b, c.a];
			status.transparent_window = self.opts.transparent;
			status.input_passthrough = self.opts.input_passthrough;
		}
	}

	fn update_runtime_spout(&self, active: bool) {
		let Some(status) = &self.runtime_status else {
			return;
		};
		if let Ok(mut status) = status.lock() {
			status.spout_enabled = active;
			status.spout_name = if active { Some(self.opts.spout.name.clone()) } else { None };
			status.spout_width = self.opts.spout.width;
			status.spout_height = self.opts.spout.height;
			if !active {
				status.spout_frames_attempted = 0;
				status.spout_frames_sent = 0;
				status.spout_frame_failures = 0;
				status.spout_consecutive_failures = 0;
				status.spout_last_send_ok = None;
				status.spout_last_readback_ms = None;
				status.spout_last_send_ms = None;
				status.spout_last_total_ms = None;
				status.spout_sender_initialized = None;
				status.spout_sender_width = None;
				status.spout_sender_height = None;
			}
		}
	}

	fn update_runtime_spout_stats(&self) {
		#[cfg(windows)]
		{
			let Some(stats) = self.gpu.as_ref().and_then(|gpu| gpu.spout_stats()) else {
				return;
			};
			let Some(status) = &self.runtime_status else {
				return;
			};
			if let Ok(mut status) = status.lock() {
				status.spout_frames_attempted = stats.frames_attempted;
				status.spout_frames_sent = stats.frames_sent;
				status.spout_frame_failures = stats.frame_failures;
				status.spout_consecutive_failures = stats.consecutive_failures;
				status.spout_last_send_ok = stats.last_send_ok;
				status.spout_last_readback_ms = stats.last_readback_ms;
				status.spout_last_send_ms = stats.last_send_ms;
				status.spout_last_total_ms = stats.last_total_ms;
				status.spout_sender_initialized = stats.sender_initialized;
				status.spout_sender_width = stats.sender_width;
				status.spout_sender_height = stats.sender_height;
				status.note = if status.spout_enabled && stats.consecutive_failures > 0 {
					Some(format!(
						"Spout2 send failed for {} consecutive frame(s)",
						stats.consecutive_failures
					))
				} else if status.note.as_deref().is_some_and(|note| note.starts_with("Spout2 send failed")) {
					None
				} else {
					status.note.take()
				};
			}
		}
	}

	fn resolve_action_id_by_menu_path(
		&self,
		action_id: Option<&str>,
		menu_path: Option<&str>,
		wardrobe_set_id: Option<&str>,
	) -> Result<Option<String>, String> {
		if let Some(action_id) = action_id {
			return Ok(Some(action_id.to_string()));
		}
		let Some(menu_path) = menu_path else {
			return Ok(None);
		};
		let candidates = self
			.gpu
			.as_ref()
			.ok_or_else(|| "renderer is not initialized".to_string())?
			.menu_wardrobe_candidates();
		let resolved_action_id = resolve_activate_action_from_menu_path(menu_path, wardrobe_set_id, &candidates).map_err(|error| {
			format!(
				"{error}{}",
				wardrobe_set_id.map_or(String::new(), |set_id| format!(" wardrobe_set_id={set_id}"))
			)
		})?;
		Ok(Some(resolved_action_id))
	}

	fn update_runtime_texture_summary(&self, texture_summary: Option<mesh_pass::TextureUploadSummary>) {
		let Some(status) = &self.runtime_status else {
			return;
		};
		if let Ok(mut status) = status.lock() {
			status.texture_summary = texture_summary;
		}
	}

	fn update_runtime_wardrobe_set(&self, set_id: Option<String>) {
		let Some(status) = &self.runtime_status else {
			return;
		};
		if let Ok(mut status) = status.lock() {
			status.active_wardrobe_set = set_id;
		}
	}

	fn update_runtime_asset_groups(&self, asset_groups: Vec<String>) {
		let Some(status) = &self.runtime_status else {
			return;
		};
		if let Ok(mut status) = status.lock() {
			status.active_asset_groups = asset_groups;
		}
	}

	fn update_runtime_wardrobe_asset_upload(&self, plan: WardrobeAssetUploadPlan) {
		let Some(status) = &self.runtime_status else {
			return;
		};
		if let Ok(mut status) = status.lock() {
			status.wardrobe_asset_upload = plan;
		}
	}

	fn update_runtime_resolver_cache_key(&self, key: Option<un_avatar_core::UnaRuntimeResolverCacheKey>) {
		let Some(status) = &self.runtime_status else {
			return;
		};
		if let Ok(mut status) = status.lock() {
			status.resolver_cache_key = key;
		}
	}

	fn update_runtime_last_action(&self, action_id: Option<String>) {
		let Some(status) = &self.runtime_status else {
			return;
		};
		if let Ok(mut status) = status.lock() {
			status.last_action_id = action_id;
		}
	}

	fn update_runtime_parameters(&self, parameter_values: BTreeMap<String, f32>) {
		let Some(status) = &self.runtime_status else {
			return;
		};
		if let Ok(mut status) = status.lock() {
			status.runtime_parameter_values.extend(parameter_values);
		}
	}

	fn apply_runtime_activation_status(&self, activation: &gpu::RuntimeActionActivation) {
		if let Some(active_set_id) = &activation.active_wardrobe_set {
			self.update_runtime_wardrobe_set(Some(active_set_id.clone()));
			self.update_runtime_asset_groups(self.gpu.as_ref().map(|gpu| gpu.active_asset_groups()).unwrap_or_default());
			self.update_runtime_wardrobe_asset_upload(self.gpu.as_ref().map(|gpu| gpu.wardrobe_asset_upload_plan()).unwrap_or_default());
			self.update_runtime_resolver_cache_key(self.gpu.as_ref().and_then(|gpu| gpu.resolver_cache_key()));
		}
		self.update_runtime_last_action(Some(activation.action_id.clone()));
		self.update_runtime_parameters(activation.parameter_values.clone());
	}

	fn update_runtime_startup(&self) {
		let Some(status) = &self.runtime_status else {
			return;
		};
		if let Ok(mut status) = status.lock() {
			if let Some(progress) = &self.startup_progress {
				status.scene_state = SCENE_STATE_SPLASH.to_string();
				status.startup_phase = Some(progress.phase.as_str().to_string());
				status.startup_progress = Some([progress.current, progress.total]);
				status.startup_message = Some(progress.message.clone());
			} else if let Some(error) = &self.startup_failed {
				status.scene_state = SCENE_STATE_FAILED.to_string();
				status.startup_phase = Some("failed".to_string());
				status.startup_progress = None;
				status.startup_message = Some(error.clone());
			} else {
				status.scene_state = SCENE_STATE_AVATAR_SCENE.to_string();
				status.startup_phase = None;
				status.startup_progress = None;
				status.startup_message = None;
			}
		}
	}

	fn set_startup_progress(&mut self, phase: impl Into<StartupPhase>, current: u32, total: u32, message: impl Into<String>) {
		self.startup_progress = Some(StartupProgressState {
			phase: phase.into(),
			current,
			total,
			message: message.into(),
		});
		self.startup_failed = None;
		self.update_runtime_startup();
		self.update_loading_title();
		self.request_redraw();
	}

	fn clear_startup_progress(&mut self) {
		self.startup_progress = None;
		self.startup_failed = None;
		self.update_runtime_startup();
	}

	fn set_startup_failed(&mut self, message: impl Into<String>) {
		self.startup_pending_document = false;
		self.startup_progress = None;
		self.startup_failed = Some(message.into());
		self.update_runtime_startup();
		self.update_failed_title();
		self.request_redraw();
	}

	fn update_loading_title(&self) {
		let Some(window) = self.window.as_ref() else {
			return;
		};
		let Some(progress) = self.startup_progress.as_ref() else {
			return;
		};
		let diagnostic_suffix = self.title_diagnostic_suffix();
		if progress.total > 0 {
			window.set_title(&format!(
				"{}{} — {} {}/{}",
				self.title_base,
				diagnostic_suffix,
				compact_window_title_status(&progress.message),
				progress.current,
				progress.total
			));
		} else {
			window.set_title(&format!(
				"{}{} — {}",
				self.title_base,
				diagnostic_suffix,
				compact_window_title_status(&progress.message)
			));
		}
	}

	fn update_failed_title(&self) {
		let Some(window) = self.window.as_ref() else {
			return;
		};
		if let Some(error) = self.startup_failed.as_ref() {
			window.set_title(&format!(
				"{}{} — startup failed: {}",
				self.title_base,
				self.title_diagnostic_suffix(),
				compact_window_title_status(error)
			));
		}
	}

	fn title_diagnostic_suffix(&self) -> String {
		let opts = self.scene_mesh_load_opts();
		let mut suffix = String::new();
		let mut push_diagnostic = |label: &str| {
			if suffix.is_empty() {
				suffix.push_str(" [diagnostics: ");
			} else {
				suffix.push_str(", ");
			}
			suffix.push_str(label);
		};
		if opts.disable_fur {
			push_diagnostic("fur OFF");
		}
		if opts.debug_disable_reflection {
			push_diagnostic("reflection OFF");
		}
		if opts.debug_base_texture_only {
			push_diagnostic("base only");
		}
		if opts.debug_zero_morphs {
			push_diagnostic("zero morphs");
		}
		if opts.debug_bind_pose {
			push_diagnostic("bind pose");
		}
		if opts.debug_skin_legacy_no_inv_mesh {
			push_diagnostic("legacy skin");
		}
		if !suffix.is_empty() {
			suffix.push(']');
		}
		suffix
	}

	fn start_async_model_load(&mut self, proxy: EventLoopProxy<RendererControlEvent>) {
		if self.startup_pending_document {
			return;
		}
		let Some(path) = self.opts.gltf_path.clone() else {
			return;
		};
		let wardrobe_set = self.opts.wardrobe_set.clone();
		self.startup_pending_document = true;
		self.set_startup_progress("model", 0, 0, "Loading model");
		let spawn_result = thread::Builder::new().name("un-avatar-startup-load".to_string()).spawn(move || {
			let startup_started = Instant::now();
			let _ = proxy.send_event(RendererControlEvent::StartupProgress {
				phase: StartupPhase::Model,
				current: 0,
				total: 0,
				message: startup_message("Loading model", startup_started),
			});
			let document = match model_loader::load_document(&path, wardrobe_set.as_deref()) {
				Ok(document) => document,
				Err(e) => {
					let _ = proxy.send_event(RendererControlEvent::StartupFailed {
						message: format!("Failed to load model {}: {e}", path.display()),
					});
					return;
				}
			};
			let _ = proxy.send_event(RendererControlEvent::StartupProgress {
				phase: StartupPhase::Model,
				current: 1,
				total: 1,
				message: startup_message("Model imported", startup_started),
			});
			let _ = proxy.send_event(RendererControlEvent::StartupProgress {
				phase: StartupPhase::GpuUpload,
				current: 0,
				total: 0,
				message: startup_message("Waiting for GPU upload", startup_started),
			});
			let texture_summary = mesh_pass::TextureUploadSummary::default();
			let _ = proxy.send_event(RendererControlEvent::StartupReady { document, texture_summary });
		});
		if let Err(e) = spawn_result {
			self.set_startup_failed(format!("spawn startup loader failed: {e}"));
		}
	}

	fn scene_mesh_load_opts(&self) -> SceneMeshLoadOpts {
		let mut mesh_diagnostics = self.opts.mesh_diagnostics.clone();
		self.apply_common_mesh_diagnostic_overrides(&mut mesh_diagnostics);
		mesh_diagnostics.skin_tone_matching |= self.opts.skin_tone_matching;
		mesh_diagnostics
	}

	fn apply_common_mesh_diagnostic_overrides(&self, mesh_diagnostics: &mut SceneMeshLoadOpts) {
		mesh_diagnostics.force_simple_basecolor |= self.opts.simple_basecolor_only;
		mesh_diagnostics.disable_mtoon_outlines |= self.opts.disable_mtoon_outlines;
		mesh_diagnostics.debug_disable_rim_lighting |= self.opts.debug_disable_rim_lighting;
		mesh_diagnostics.debug_force_shading_shift_zero |= self.opts.debug_force_shading_shift_zero;
		mesh_diagnostics.debug_disable_matcap |= self.opts.debug_disable_matcap;
		mesh_diagnostics.debug_disable_emissive |= self.opts.debug_disable_emissive;
		mesh_diagnostics.debug_disable_shade_color |= self.opts.debug_disable_shade_color;
		mesh_diagnostics.debug_disable_normal_map |= self.opts.debug_disable_normal_map;
		mesh_diagnostics.debug_base_texture_only |= self.opts.debug_base_texture_only;
	}

	fn startup_texture_target_size(&self) -> (u32, u32) {
		if self.opts.spout.enabled {
			(
				self.opts.spout.width.unwrap_or(self.opts.window_width).max(1),
				self.opts.spout.height.unwrap_or(self.opts.window_height).max(1),
			)
		} else if let Some(window) = self.window.as_ref() {
			let size = window.inner_size();
			(size.width.max(1), size.height.max(1))
		} else {
			(self.opts.window_width.max(1), self.opts.window_height.max(1))
		}
	}

	fn document_attach_options(&self) -> DocumentAttachOptions {
		let (target_width, target_height) = self.startup_texture_target_size();
		let texture_max_dimension = self.opts.texture_resolution_limit.max_dimension(target_width, target_height);
		let bc_supported = cfg!(windows)
			&& !matches!(
				self.opts.texture_compression,
				TextureCompressionMode::Source | TextureCompressionMode::Compat
			);
		let mesh_diagnostics = self.scene_mesh_load_opts();
		DocumentAttachOptions {
			mesh_diagnostics,
			texture_max_dimension,
			texture_compression: self.opts.texture_compression,
			block_compression_encoder: self.opts.block_compression_encoder,
			block_compression_cpu_threads: self.opts.block_compression_cpu_threads,
			mipmap_filter: self.opts.mipmap_filter,
			texture_compression_advanced: self.opts.texture_compression_advanced.clone(),
			texture_compression_bc_supported: bc_supported,
			texture_compression_astc_supported: false,
			texture_compression_etc2_supported: false,
			processed_texture_cache: self.opts.processed_texture_cache,
			enable_spring_bones: self.opts.enable_spring_bones,
			bone_colliders: self.opts.bone_colliders,
			spring_bone_physics: self.opts.spring_bone_physics.clone(),
			debug_material_dump: self.opts.debug_material_dump,
			vmc_address: self.opts.vmc_address,
			unmotion_zenoh: self.opts.unmotion_zenoh.clone(),
			audio_link: self.opts.audio_link.clone(),
			debug_vmc: self.opts.debug.vmc,
		}
	}

	fn start_async_scene_build(
		&mut self,
		document: Arc<un_avatar_core::UnaDocument>,
		fallback_texture_summary: mesh_pass::TextureUploadSummary,
	) -> Result<(), String> {
		let Some(gpu) = self.gpu.as_ref() else {
			return Err("GPU state is not initialized".to_string());
		};
		let context = gpu.scene_build_context();
		let options = self.document_attach_options();
		let proxy = self.event_proxy.clone();
		self.set_startup_progress(StartupPhase::GpuUpload, 0, 0, "Preparing GPU scene");
		thread::Builder::new()
			.name("un-avatar-gpu-scene-build".to_string())
			.spawn(move || {
				let gpu_started = Instant::now();
				let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
					context.prepare_document_scene(document, &options, |progress| {
						let _ = proxy.send_event(RendererControlEvent::StartupProgress {
							phase: StartupPhase::from(progress.phase),
							current: progress.current,
							total: progress.total,
							message: startup_message(progress.message, gpu_started),
						});
					})
				}));
				match result {
					Ok(Ok(prepared)) => {
						let _ = proxy.send_event(RendererControlEvent::StartupSceneReady {
							prepared,
							options,
							fallback_texture_summary,
						});
					}
					Ok(Err(error)) => {
						let _ = proxy.send_event(RendererControlEvent::StartupFailed { message: error });
					}
					Err(panic) => {
						let message = if let Some(message) = panic.downcast_ref::<&str>() {
							(*message).to_string()
						} else if let Some(message) = panic.downcast_ref::<String>() {
							message.clone()
						} else {
							"GPU scene build panicked".to_string()
						};
						let _ = proxy.send_event(RendererControlEvent::StartupFailed { message });
					}
				}
			})
			.map(|_| ())
			.map_err(|e| format!("spawn GPU scene builder failed: {e}"))
	}

	fn clear_camera_transitions(&mut self) {
		self.camera_transition_queue.clear();
		self.active_camera_transition = None;
	}

	fn enqueue_camera_transition(&mut self, patch: CameraStatePatch, options: CameraTransitionOptions) {
		if matches!(options.mode, CameraTransitionMode::Replace) {
			self.clear_camera_transitions();
		}
		if options.duration_ms == 0 {
			if let Some(gpu) = self.gpu.as_mut() {
				apply_camera_state_patch(gpu, patch);
			} else {
				self.camera_transition_queue.push_back(QueuedCameraTransition {
					patch,
					options: CameraTransitionOptions {
						duration_ms: default_camera_transition_duration_ms(),
						easing: options.easing,
						mode: options.mode,
					},
				});
			}
			self.request_redraw();
			return;
		}
		self.camera_transition_queue.push_back(QueuedCameraTransition { patch, options });
		self.request_redraw();
	}

	fn advance_camera_transition(&mut self, now: Instant) {
		if self.active_camera_transition.is_none() {
			let Some(next) = self.camera_transition_queue.pop_front() else {
				return;
			};
			let Some(gpu) = self.gpu.as_ref() else {
				self.camera_transition_queue.push_front(next);
				return;
			};
			let start = gpu.camera_state_snapshot();
			let end = patched_camera_state(start, next.patch);
			self.active_camera_transition = Some(ActiveCameraTransition {
				start,
				end,
				started_at: now,
				duration: Duration::from_millis(u64::from(next.options.duration_ms.max(1))),
				easing: next.options.easing,
			});
		}

		let Some(active) = self.active_camera_transition else {
			return;
		};
		let elapsed = now.saturating_duration_since(active.started_at);
		let raw_t = (elapsed.as_secs_f32() / active.duration.as_secs_f32()).clamp(0.0, 1.0);
		let eased_t = ease_camera_transition(raw_t, active.easing);
		let state = lerp_camera_state(active.start, active.end, eased_t);
		if let Some(gpu) = self.gpu.as_mut() {
			gpu.set_camera_state(
				Some(state.target),
				Some(state.longitude_deg),
				Some(state.latitude_deg),
				Some(state.radius),
				Some(state.diagonal_fov_deg),
			);
		}
		if raw_t >= 1.0 {
			self.active_camera_transition = None;
		}
		if self.active_camera_transition.is_some() || !self.camera_transition_queue.is_empty() {
			self.request_redraw();
		}
	}

	fn render_frame(&mut self) {
		let now = Instant::now();
		let wall = now.saturating_duration_since(self.last_wall);
		self.last_wall = now;

		let Some(win) = self.window.as_ref().cloned() else {
			return;
		};
		if self.apply_pending_reconfigure(now, win.as_ref()) {
			win.request_redraw();
			return;
		}
		self.advance_camera_transition(now);
		let Some(gpu) = self.gpu.as_mut() else {
			return;
		};

		let wall_clamped = wall.min(Duration::from_millis(500));
		let startup_splash = if let Some(progress) = self.startup_progress.as_ref() {
			Some(gpu::StartupSplashFrame {
				time_secs: self.started_at.elapsed().as_secs_f32(),
				progress: progress.normalized_progress(),
				phase: progress.phase.splash_code(),
			})
		} else {
			self.startup_failed.as_ref().map(|_| gpu::StartupSplashFrame {
				time_secs: self.started_at.elapsed().as_secs_f32(),
				progress: -1.0,
				phase: 9.0,
			})
		};
		let Some(timings) = gpu.render_frame(win.as_ref(), self.opts.clear_color, wall_clamped, startup_splash) else {
			win.request_redraw();
			return;
		};
		let (parameter_updates, runtime_parameter_activations) = {
			let parameter_updates = match gpu.apply_contact_parameter_emissions() {
				Ok(parameter_updates) => parameter_updates,
				Err(err) => {
					eprintln!("un-avatar-renderer: contact parameter emission failed: {err}");
					BTreeMap::new()
				}
			};
			let activations = match gpu.evaluate_runtime_parameter_actions() {
				Ok(activations) => activations,
				Err(err) => {
					eprintln!("un-avatar-renderer: runtime parameter action evaluation failed: {err}");
					Vec::new()
				}
			};
			(parameter_updates, activations)
		};
		if !parameter_updates.is_empty() {
			self.update_runtime_parameters(parameter_updates);
		}
		for activation in &runtime_parameter_activations {
			self.apply_runtime_activation_status(activation);
		}
		if !runtime_parameter_activations.is_empty() {
			self.request_redraw();
		}
		let inst_fps = if timings.wall_since_last_ms > 0.05 {
			1000.0 / timings.wall_since_last_ms
		} else {
			self.fps_smooth
		};
		self.fps_smooth = self.fps_smooth * 0.9 + inst_fps * 0.1;
		self.update_runtime_frame(&timings);
		self.update_runtime_spout_stats();

		if self.startup_progress.is_some() {
			self.title_refresh = self.title_refresh.wrapping_add(1);
			if self.title_refresh.is_multiple_of(16) {
				self.update_loading_title();
			}
		} else if self.startup_failed.is_some() {
			self.title_refresh = self.title_refresh.wrapping_add(1);
			if self.title_refresh.is_multiple_of(16) {
				self.update_failed_title();
			}
		} else if self.opts.show_fps_in_title {
			self.title_refresh = self.title_refresh.wrapping_add(1);
			if self.title_refresh.is_multiple_of(16) {
				win.set_title(&format!(
					"{}{} — {:.0} FPS  cpu {:.2} ms  gpu~ {:.2} ms",
					self.title_base,
					self.title_diagnostic_suffix(),
					self.fps_smooth,
					timings.cpu_record_ms,
					timings.gpu_ms
				));
			}
		}

		win.request_redraw();
	}

	fn close_hotkey_matches(&self, key: &Key) -> bool {
		self.close_hotkey
			.as_ref()
			.is_some_and(|hotkey| hotkey.matches(key, self.current_modifiers))
	}
}

impl ApplicationHandler<RendererControlEvent> for AvatarApp {
	fn resumed(&mut self, event_loop: &ActiveEventLoop) {
		if self.window.is_some() {
			return;
		}

		let mut attrs = Window::default_attributes()
			.with_title(format!("{} — initializing", self.opts.title))
			.with_decorations(self.opts.decorations)
			.with_transparent(true)
			.with_visible(false)
			.with_window_level(if self.opts.always_on_top {
				WindowLevel::AlwaysOnTop
			} else {
				WindowLevel::Normal
			})
			.with_inner_size(PhysicalSize::new(self.opts.window_width.max(1), self.opts.window_height.max(1)));
		// manifest `[window] x = ... y = ...` または CLI `--position X,Y` で指定された場合、
		// プライマリモニタ既定位置ではなく明示位置で開く。OS 側で画面外座標になっても自動補正に任せる。
		if let Some([x, y]) = self.opts.window_position {
			attrs = attrs.with_position(winit::dpi::PhysicalPosition::new(x, y));
		}
		attrs = attrs.with_active(!self.opts.start_minimized);
		#[cfg(windows)]
		{
			attrs = attrs.with_no_redirection_bitmap(true);
		}
		if let Some(path) = self.opts.icon_path.as_deref() {
			if let Some(icon) = load_window_icon(path) {
				attrs = attrs.with_window_icon(Some(icon));
			}
		}

		let win = match event_loop.create_window(attrs) {
			Ok(w) => Arc::new(w),
			Err(e) => {
				eprintln!("un-avatar-renderer: create_window: {e}");
				event_loop.exit();
				return;
			}
		};
		apply_window_hittest(&win, self.opts.transparent, self.opts.input_passthrough);
		if let Some([x, y]) = self.opts.window_position {
			win.set_outer_position(winit::dpi::PhysicalPosition::new(x, y));
		}
		if self.opts.start_minimized {
			win.set_minimized(true);
		}
		win.set_visible(true);

		let mut mesh_diagnostics = self.opts.mesh_diagnostics.clone();
		self.apply_common_mesh_diagnostic_overrides(&mut mesh_diagnostics);
		match GpuState::new_shell(
			win.clone(),
			self.opts.transparent,
			self.opts.primary_motion_source,
			self.opts.spout.clone(),
			self.opts.environment_color,
			self.opts.lighting,
			self.opts.bloom,
			self.opts.ssao,
			self.opts.contact_shadow,
			self.opts.aa,
			self.opts.render_backend,
			self.opts.texture_compression,
			self.opts.debug.clone(),
			self.opts.disable_expression_morphs,
			self.opts.disable_vmc_eye_look,
			self.opts.eye_look_at_clamp_deg,
			self.opts.apply_vmc_root_translation,
			mesh_diagnostics,
		) {
			Ok(mut gpu) => {
				gpu.set_show_axes(self.opts.show_axes);
				gpu.set_show_bone_colliders(self.opts.show_bone_colliders);
				if let Some(init) = self.opts.initial_camera_state {
					gpu.set_camera_state(
						init.target,
						init.longitude_deg,
						init.latitude_deg,
						init.radius,
						init.diagonal_fov_deg,
					);
				}
				self.gpu = Some(gpu);
			}
			Err(e) => {
				eprintln!("un-avatar-renderer: {e}");
				event_loop.exit();
				return;
			}
		}

		win.focus_window();
		self.last_wall = Instant::now();
		let size = win.inner_size();
		self.update_runtime_surface(size.width, size.height);
		win.request_redraw();
		self.window = Some(win);
		self.start_async_model_load(self.event_proxy.clone());
	}

	fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
		if let Some(window) = &self.window {
			window.request_redraw();
		}
	}

	fn window_event(&mut self, event_loop: &ActiveEventLoop, _: winit::window::WindowId, event: WindowEvent) {
		match event {
			WindowEvent::CloseRequested => {
				event_loop.exit();
			}
			WindowEvent::Resized(size) => {
				self.reconfigure(size.width, size.height);
				self.update_runtime_window_geometry();
				if let Some(w) = &self.window {
					w.request_redraw();
				}
			}
			WindowEvent::Moved(_pos) => {
				// outer 位置の最新値を runtime telemetry に書き込み、Supervisor の Save Window State で
				// プロファイルへ保存できるようにする。
				self.update_runtime_window_geometry();
			}
			WindowEvent::Focused(focused) => {
				self.window_focused = focused;
				if focused {
					self.window_activation_seq = self.window_activation_seq.saturating_add(1);
				}
				self.update_runtime_focus_status();
			}
			WindowEvent::RedrawRequested => {
				self.render_frame();
			}
			WindowEvent::MouseInput { state, button, .. } => {
				if button == MouseButton::Left && state == ElementState::Pressed && !self.opts.decorations {
					if let (Some(window), Some(position)) = (&self.window, self.last_cursor_pos) {
						if let Some(direction) = borderless_resize_direction(position, window.inner_size()) {
							let _ = window.drag_resize_window(direction);
						} else {
							let _ = window.drag_window();
						}
					}
				}
				if button == MouseButton::Right {
					self.right_dragging = !self.camera_locked && state == ElementState::Pressed;
					if state == ElementState::Pressed && !self.camera_locked {
						// ダブルクリック判定: 300ms 以内に 2 回 press → 回転 reset
						let now = Instant::now();
						let is_double = self
							.last_right_press
							.is_some_and(|prev| now.duration_since(prev) <= DOUBLE_CLICK_THRESHOLD);
						self.last_right_press = Some(now);
						if is_double {
							self.clear_camera_transitions();
							if let Some(gpu) = self.gpu.as_mut() {
								gpu.reset_camera_rotation();
								self.request_redraw();
							}
							// dbl 後の単発 press は次の double 判定で誤動作しないようリセット
							self.last_right_press = None;
							self.right_dragging = false;
						}
					}
					if !self.right_dragging && state == ElementState::Released {
						self.last_cursor_pos = None;
					}
				}
				if button == MouseButton::Middle {
					self.middle_dragging = !self.camera_locked && state == ElementState::Pressed;
					if state == ElementState::Pressed && !self.camera_locked {
						// ミドルダブルクリック → ミドルドラッグでアサインされている "パン" 操作のリセット。
						// target のみを初期位置に戻し、orbit/radius/FOV は保持する。全リセットは
						// Supervisor の Camera セクションの Reset ボタンに割り当てる。
						let now = Instant::now();
						let is_double = self
							.last_middle_press
							.is_some_and(|prev| now.duration_since(prev) <= DOUBLE_CLICK_THRESHOLD);
						self.last_middle_press = Some(now);
						if is_double {
							self.clear_camera_transitions();
							if let Some(gpu) = self.gpu.as_mut() {
								gpu.reset_camera_pan();
								self.request_redraw();
							}
							self.last_middle_press = None;
							self.middle_dragging = false;
						}
					}
					if !self.middle_dragging && state == ElementState::Released {
						self.last_cursor_pos = None;
					}
				}
			}
			WindowEvent::CursorMoved { position, .. } => {
				if !self.opts.decorations {
					if let Some(window) = &self.window {
						let cursor = borderless_resize_direction(position, window.inner_size())
							.map(CursorIcon::from)
							.unwrap_or(CursorIcon::Default);
						window.set_cursor(cursor);
					}
				}
				if !self.camera_locked && (self.right_dragging || self.middle_dragging) {
					self.clear_camera_transitions();
					if let (Some(prev), Some(gpu)) = (self.last_cursor_pos, self.gpu.as_mut()) {
						let dx = position.x - prev.x;
						let dy = position.y - prev.y;
						if self.middle_dragging {
							gpu.pan_camera_pixels(dx, dy);
						} else {
							gpu.orbit_camera_pixels(dx, dy);
						}
						self.request_redraw();
					}
				}
				self.last_cursor_pos = Some(position);
			}
			WindowEvent::CursorLeft { .. } => {
				self.last_cursor_pos = None;
				if let Some(window) = &self.window {
					window.set_cursor(CursorIcon::Default);
				}
			}
			WindowEvent::MouseWheel { delta, .. } => {
				if self.camera_locked {
					return;
				}
				let units = match delta {
					MouseScrollDelta::LineDelta(_, y) => y,
					MouseScrollDelta::PixelDelta(p) => p.y as f32 * 0.04,
				};
				if units != 0.0 {
					self.clear_camera_transitions();
					if let Some(gpu) = self.gpu.as_mut() {
						gpu.zoom_camera_wheel(units);
						self.request_redraw();
					}
				}
			}
			WindowEvent::ModifiersChanged(modifiers) => {
				self.current_modifiers = modifiers.state();
			}
			WindowEvent::KeyboardInput { event, .. } => {
				if !self.opts.decorations
					&& event.state == ElementState::Pressed
					&& !event.repeat
					&& self.close_hotkey_matches(&event.logical_key)
				{
					event_loop.exit();
				}
			}
			_ => {}
		}
	}

	fn user_event(&mut self, event_loop: &ActiveEventLoop, event: RendererControlEvent) {
		match event {
			RendererControlEvent::Shutdown => {
				self.hide_window_for_shutdown();
				event_loop.exit();
			}
			RendererControlEvent::ResetCamera => {
				self.clear_camera_transitions();
				if let Some(gpu) = self.gpu.as_mut() {
					gpu.reset_camera();
					self.request_redraw();
				}
			}
			RendererControlEvent::SetCameraOrbit {
				longitude,
				latitude,
				radius,
			} => {
				self.clear_camera_transitions();
				if let Some(gpu) = self.gpu.as_mut() {
					gpu.set_camera_orbit(longitude, latitude, radius);
					self.request_redraw();
				}
			}
			RendererControlEvent::SetClearColor { r, g, b, a } => {
				self.opts.clear_color = wgpu::Color {
					r,
					g,
					b,
					a: if self.opts.transparent { 0.0 } else { a },
				};
				self.request_redraw();
			}
			RendererControlEvent::SetSpoutOutput {
				enabled,
				name,
				width,
				height,
			} => {
				if let Some(name) = name.filter(|name| !name.is_empty()) {
					self.opts.spout.name = name;
				}
				if width.is_some() {
					self.opts.spout.width = width;
				}
				if height.is_some() {
					self.opts.spout.height = height;
				}
				self.opts.spout.enabled = enabled;
				let active = if let Some(gpu) = self.gpu.as_mut() {
					let active = gpu.set_spout_output(enabled, self.opts.spout.clone());
					self.request_redraw();
					active
				} else {
					false
				};
				self.update_runtime_spout(active);
			}
			RendererControlEvent::SetWindow {
				decorations,
				transparent,
				input_passthrough,
				always_on_top,
				minimized,
				width,
				height,
			} => self.apply_window_runtime(decorations, transparent, input_passthrough, always_on_top, minimized, width, height),
			RendererControlEvent::Screenshot { path, result } => {
				let outcome = match self.gpu.as_mut() {
					Some(gpu) => gpu.capture_screenshot(&path, self.opts.clear_color),
					None => Err("renderer is not initialized".to_string()),
				};
				if let Ok(mut guard) = result.lock() {
					*guard = Some(outcome);
				}
			}
			RendererControlEvent::SetWardrobe { set_id, result } => {
				let active_set_id = model_loader::normalize_wardrobe_set_id(Some(&set_id)).map(str::to_owned);
				let outcome = match self.gpu.as_mut() {
					Some(gpu) => gpu.apply_wardrobe_set(&set_id),
					None => Err("renderer is not initialized".to_string()),
				};
				if outcome.is_ok() {
					self.update_runtime_wardrobe_set(active_set_id);
					self.update_runtime_asset_groups(self.gpu.as_ref().map(|gpu| gpu.active_asset_groups()).unwrap_or_default());
					self.update_runtime_wardrobe_asset_upload(
						self.gpu.as_ref().map(|gpu| gpu.wardrobe_asset_upload_plan()).unwrap_or_default(),
					);
					self.update_runtime_resolver_cache_key(self.gpu.as_ref().and_then(|gpu| gpu.resolver_cache_key()));
					self.request_redraw();
				}
				if let Ok(mut guard) = result.lock() {
					*guard = Some(outcome);
				}
			}
			RendererControlEvent::ActivateAction {
				action_id,
				supervisor_command,
				expression_menu_path,
				menu_path,
				wardrobe_set_id,
				parameter_name,
				parameter_value,
				result,
			} => {
				let resolved_action_id =
					match self.resolve_action_id_by_menu_path(action_id.as_deref(), menu_path.as_deref(), wardrobe_set_id.as_deref()) {
						Ok(action_id) => action_id,
						Err(e) => {
							if let Ok(mut guard) = result.lock() {
								*guard = Some(Err(e));
							}
							return;
						}
					};
				let outcome = match self.gpu.as_mut() {
					Some(gpu) => gpu.activate_runtime_action(
						resolved_action_id.as_deref(),
						supervisor_command.as_deref(),
						menu_path.or(expression_menu_path).as_deref(),
						parameter_name.as_deref(),
						parameter_value,
					),
					None => Err("renderer is not initialized".to_string()),
				};
				if let Ok(activation) = &outcome {
					self.apply_runtime_activation_status(activation);
				}
				if outcome.is_ok() {
					self.request_redraw();
				}
				if let Ok(mut guard) = result.lock() {
					*guard = Some(outcome.map(|_| ()));
				}
			}
			RendererControlEvent::SetParameter { name, value, result } => {
				let outcome = match self.gpu.as_mut() {
					Some(gpu) => gpu.set_runtime_parameter(&name, value),
					None => Err("renderer is not initialized".to_string()),
				};
				if let Ok(activation) = &outcome {
					self.update_runtime_parameters(BTreeMap::from([(name.clone(), value)]));
					if let Some(activation) = activation {
						self.apply_runtime_activation_status(activation);
					}
				}
				if outcome.is_ok() {
					self.request_redraw();
				}
				if let Ok(mut guard) = result.lock() {
					*guard = Some(outcome.map(|_| ()));
				}
			}
			RendererControlEvent::SceneState { result } => {
				let state = if self.startup_failed.is_some() {
					SCENE_STATE_FAILED
				} else if self.startup_progress.is_some() || self.startup_pending_document {
					SCENE_STATE_SPLASH
				} else {
					SCENE_STATE_AVATAR_SCENE
				};
				if let Ok(mut guard) = result.lock() {
					*guard = Some(state.to_string());
				}
			}
			RendererControlEvent::SetExpressionOverride { name, weight } => {
				if let Some(gpu) = self.gpu.as_mut() {
					gpu.set_expression_override(&name, weight);
					self.request_redraw();
				}
			}
			RendererControlEvent::ClearExpressionOverrides => {
				if let Some(gpu) = self.gpu.as_mut() {
					gpu.clear_expression_overrides();
					self.request_redraw();
				}
			}
			RendererControlEvent::Activate => {
				if let Some(win) = self.window.as_ref() {
					win.set_minimized(false);
					win.set_visible(true);
					win.focus_window();
				}
			}
			RendererControlEvent::SetShowAxes { enabled } => {
				self.opts.show_axes = enabled;
				if let Some(gpu) = self.gpu.as_mut() {
					gpu.set_show_axes(enabled);
				}
				self.request_redraw();
			}
			RendererControlEvent::SetShowBoneColliders { enabled } => {
				self.opts.show_bone_colliders = enabled;
				if let Some(gpu) = self.gpu.as_mut() {
					gpu.set_show_bone_colliders(enabled);
				}
				self.request_redraw();
			}
			RendererControlEvent::SetCameraLock { locked } => {
				self.camera_locked = locked;
				self.opts.camera_locked = locked;
				if locked {
					self.right_dragging = false;
					self.middle_dragging = false;
				}
			}
			RendererControlEvent::SetCameraFov { diagonal_deg } => {
				self.clear_camera_transitions();
				if let Some(gpu) = self.gpu.as_mut() {
					gpu.set_camera_fov_diagonal_deg(diagonal_deg);
					self.request_redraw();
				}
			}
			RendererControlEvent::SetCameraState {
				target,
				longitude_deg,
				latitude_deg,
				radius,
				diagonal_fov_deg,
				transition,
			} => {
				let patch = CameraStatePatch {
					target,
					longitude_deg,
					latitude_deg,
					radius,
					diagonal_fov_deg,
				};
				if let Some(transition) = transition {
					self.enqueue_camera_transition(patch, transition);
				} else {
					self.clear_camera_transitions();
					if let Some(gpu) = self.gpu.as_mut() {
						apply_camera_state_patch(gpu, patch);
						self.request_redraw();
					}
				}
			}
			RendererControlEvent::SetLookAt { enabled, clamp_deg } => {
				let next = if enabled {
					Some(clamp_deg.unwrap_or(30.0).clamp(0.0, 90.0))
				} else {
					None
				};
				self.opts.eye_look_at_clamp_deg = next;
				if let Some(gpu) = self.gpu.as_mut() {
					gpu.set_eye_look_at_clamp_deg(next);
				}
				self.request_redraw();
			}
			RendererControlEvent::SetWindowPosition { x, y } => {
				// 片方欠けても現在値を保ったまま反映できるよう、現在の outer 位置を読み出してから上書きする。
				if let Some(window) = self.window.as_ref() {
					let current = window.outer_position().ok();
					let new_x = x.or_else(|| current.map(|p| p.x));
					let new_y = y.or_else(|| current.map(|p| p.y));
					if let (Some(nx), Some(ny)) = (new_x, new_y) {
						window.set_outer_position(winit::dpi::PhysicalPosition::new(nx, ny));
						self.update_runtime_window_geometry();
					}
				}
			}
			RendererControlEvent::SetApplyVmcRootTranslation { enabled } => {
				self.opts.apply_vmc_root_translation = enabled;
				if let Some(gpu) = self.gpu.as_mut() {
					gpu.set_apply_vmc_root_translation(enabled);
				}
				self.request_redraw();
			}
			RendererControlEvent::SetPrimaryMotionSource { source } => {
				self.opts.primary_motion_source = source;
				if let Some(gpu) = self.gpu.as_ref() {
					gpu.set_primary_motion_source(source);
				}
				self.request_redraw();
			}
			RendererControlEvent::SetMotionReceivers {
				vmc_address,
				unmotion_zenoh_enabled,
				unmotion_zenoh_key,
			} => {
				self.opts.vmc_address = vmc_address;
				self.opts.unmotion_zenoh.enabled = unmotion_zenoh_enabled;
				self.opts.unmotion_zenoh.base_key_expr = if unmotion_zenoh_key.trim().is_empty() {
					"un-motion/frame".to_string()
				} else {
					unmotion_zenoh_key.trim().to_string()
				};
				if let Some(gpu) = self.gpu.as_mut() {
					if let Err(e) =
						gpu.reconfigure_motion_receivers(self.opts.vmc_address, self.opts.unmotion_zenoh.clone(), self.opts.debug.vmc)
					{
						eprintln!("un-avatar-renderer: motion receiver reconfigure failed: {e}");
					}
				}
				self.request_redraw();
			}
			RendererControlEvent::SetDynamics {
				enabled,
				bone_colliders,
				physics_config,
			} => {
				self.opts.enable_spring_bones = enabled;
				self.opts.bone_colliders = bone_colliders;
				self.opts.spring_bone_physics = physics_config.map(|physics| physics.normalized()).unwrap_or_default();
				if let Some(gpu) = self.gpu.as_mut() {
					gpu.reconfigure_dynamics(enabled, bone_colliders, self.opts.spring_bone_physics.clone());
				}
				self.request_redraw();
			}
			RendererControlEvent::SetAvatarOutline {
				policy,
				r#type,
				width,
				color,
				lighting_mix,
				roundness,
			} => {
				let next = avatar_outline_from_control(
					self.opts.mesh_diagnostics.avatar_outline,
					policy,
					r#type,
					width,
					color,
					lighting_mix,
					roundness,
				);
				self.opts.mesh_diagnostics.avatar_outline = next;
				if let Some(gpu) = self.gpu.as_mut() {
					gpu.set_avatar_outline(next);
				}
				self.request_redraw();
			}
			RendererControlEvent::SetAvatarRim {
				policy,
				color,
				intensity,
				lighting_mix,
				fresnel_power,
				lift,
			} => {
				let next = avatar_rim_from_control(
					self.opts.mesh_diagnostics.avatar_rim,
					policy,
					color,
					intensity,
					lighting_mix,
					fresnel_power,
					lift,
				);
				self.opts.mesh_diagnostics.avatar_rim = next;
				if let Some(gpu) = self.gpu.as_mut() {
					gpu.set_avatar_rim(next);
				}
				self.request_redraw();
			}
			RendererControlEvent::SetAvatarMatcap { scale } => {
				let next = avatar_matcap_from_control(self.opts.mesh_diagnostics.avatar_matcap, scale);
				self.opts.mesh_diagnostics.avatar_matcap = next;
				if let Some(gpu) = self.gpu.as_mut() {
					gpu.set_avatar_matcap(next);
				}
				self.request_redraw();
			}
			RendererControlEvent::SetAvatarSpecular { enabled, intensity, power } => {
				let next = avatar_specular_from_control(self.opts.mesh_diagnostics.avatar_specular, enabled, intensity, power);
				self.opts.mesh_diagnostics.avatar_specular = next;
				if let Some(gpu) = self.gpu.as_mut() {
					gpu.set_avatar_specular(next);
				}
				self.request_redraw();
			}
			RendererControlEvent::SetAvatarAmbientOcclusion { strength } => {
				let next = avatar_ambient_occlusion_from_control(self.opts.mesh_diagnostics.avatar_ambient_occlusion, strength);
				self.opts.mesh_diagnostics.avatar_ambient_occlusion = next;
				if let Some(gpu) = self.gpu.as_mut() {
					gpu.set_avatar_ambient_occlusion(next);
				}
				self.request_redraw();
			}
			RendererControlEvent::SetLighting {
				environment_enabled,
				environment_color,
				environment_intensity,
				directional_enabled,
				directional_color,
				directional_intensity,
				directional_azimuth_deg,
				directional_elevation_deg,
				directional_follow_camera_yaw,
				directional_follow_camera_pitch,
			} => {
				let next = lighting_from_control(
					self.opts.lighting,
					environment_enabled,
					environment_color,
					environment_intensity,
					directional_enabled,
					directional_color,
					directional_intensity,
					directional_azimuth_deg,
					directional_elevation_deg,
					directional_follow_camera_yaw,
					directional_follow_camera_pitch,
				);
				self.opts.lighting = next;
				if let Some(gpu) = self.gpu.as_mut() {
					gpu.set_lighting(next);
				}
				self.request_redraw();
			}
			RendererControlEvent::SetEnvironmentColor {
				exposure,
				contrast,
				saturation,
				look,
				intensity,
				temperature,
				tint,
			} => {
				let next = environment_color_from_control(
					self.opts.environment_color,
					exposure,
					contrast,
					saturation,
					look,
					intensity,
					temperature,
					tint,
				);
				self.opts.environment_color = next;
				if let Some(gpu) = self.gpu.as_mut() {
					gpu.set_environment_color(next);
				}
				self.request_redraw();
			}
			RendererControlEvent::SetBloom {
				enabled,
				strength,
				threshold,
				radius,
				quality,
			} => {
				let next = bloom_from_control(self.opts.bloom, enabled, strength, threshold, radius, quality);
				self.opts.bloom = next;
				if let Some(gpu) = self.gpu.as_mut() {
					gpu.set_bloom(next);
				}
				self.request_redraw();
			}
			RendererControlEvent::SetSsao {
				enabled,
				strength,
				radius,
				bias,
				range,
			} => {
				let next = ssao_from_control(self.opts.ssao, enabled, strength, radius, bias, range);
				self.opts.ssao = next;
				if let Some(gpu) = self.gpu.as_mut() {
					gpu.set_ssao(next);
				}
				self.request_redraw();
			}
			RendererControlEvent::SetContactShadow {
				enabled,
				strength,
				radius,
				softness,
				height,
			} => {
				let next = contact_shadow_from_control(self.opts.contact_shadow, enabled, strength, radius, softness, height);
				self.opts.contact_shadow = next;
				if let Some(gpu) = self.gpu.as_mut() {
					gpu.set_contact_shadow(next);
				}
				self.request_redraw();
			}
			RendererControlEvent::StartupProgress {
				phase,
				current,
				total,
				message,
			} => {
				self.set_startup_progress(phase, current, total, message);
			}
			RendererControlEvent::StartupReady { document, texture_summary } => {
				if let Err(e) = self.start_async_scene_build(document, texture_summary) {
					self.set_startup_failed(e.clone());
					eprintln!("un-avatar-renderer: {e}");
				}
			}
			RendererControlEvent::StartupSceneReady {
				prepared,
				options,
				fallback_texture_summary,
			} => {
				let Some(win) = self.window.as_ref().cloned() else {
					return;
				};
				let attach_result = self
					.gpu
					.as_mut()
					.ok_or_else(|| "GPU state is not initialized".to_string())
					.and_then(|gpu| gpu.attach_prepared_document(prepared, options));
				match attach_result {
					Ok(()) => {
						let startup_activations = match self.gpu.as_mut() {
							Some(gpu) => match gpu.evaluate_runtime_parameter_actions() {
								Ok(activations) => activations,
								Err(e) => {
									self.set_startup_failed(e.clone());
									eprintln!("un-avatar-renderer: runtime parameter action evaluation failed: {e}");
									return;
								}
							},
							None => Vec::new(),
						};
						let actual_texture_summary = self
							.gpu
							.as_ref()
							.and_then(|gpu| gpu.texture_summary())
							.or(Some(fallback_texture_summary));
						self.startup_pending_document = false;
						self.clear_startup_progress();
						self.update_runtime_texture_summary(actual_texture_summary);
						self.update_runtime_wardrobe_set(self.gpu.as_ref().and_then(|gpu| gpu.active_wardrobe_set()));
						self.update_runtime_asset_groups(self.gpu.as_ref().map(|gpu| gpu.active_asset_groups()).unwrap_or_default());
						self.update_runtime_wardrobe_asset_upload(
							self.gpu.as_ref().map(|gpu| gpu.wardrobe_asset_upload_plan()).unwrap_or_default(),
						);
						self.update_runtime_resolver_cache_key(self.gpu.as_ref().and_then(|gpu| gpu.resolver_cache_key()));
						self.update_runtime_last_action(self.gpu.as_ref().and_then(|gpu| gpu.last_action_id()));
						self.update_runtime_parameters(self.gpu.as_ref().map(|gpu| gpu.runtime_parameter_values()).unwrap_or_default());
						for activation in &startup_activations {
							self.apply_runtime_activation_status(activation);
						}
						self.update_runtime_spout(self.gpu.as_ref().is_some_and(|gpu| gpu.spout_active()));
						win.set_title(&format!("{}{}", self.title_base, self.title_diagnostic_suffix()));
						self.request_redraw();
					}
					Err(e) => {
						self.set_startup_failed(e.clone());
						eprintln!("un-avatar-renderer: {e}");
					}
				}
			}
			RendererControlEvent::StartupFailed { message } => {
				self.set_startup_failed(message.clone());
				eprintln!("un-avatar-renderer: {message}");
			}
		}
	}
}

fn borderless_resize_direction(position: PhysicalPosition<f64>, size: PhysicalSize<u32>) -> Option<ResizeDirection> {
	let edge = 12.0;
	let x = position.x;
	let y = position.y;
	let width = f64::from(size.width);
	let height = f64::from(size.height);
	let left = x <= edge;
	let right = x >= width - edge;
	let top = y <= edge;
	let bottom = y >= height - edge;
	match (left, right, top, bottom) {
		(true, _, true, _) => Some(ResizeDirection::NorthWest),
		(_, true, true, _) => Some(ResizeDirection::NorthEast),
		(true, _, _, true) => Some(ResizeDirection::SouthWest),
		(_, true, _, true) => Some(ResizeDirection::SouthEast),
		(true, _, _, _) => Some(ResizeDirection::West),
		(_, true, _, _) => Some(ResizeDirection::East),
		(_, _, true, _) => Some(ResizeDirection::North),
		(_, _, _, true) => Some(ResizeDirection::South),
		_ => None,
	}
}

fn apply_window_hittest(window: &Window, transparent: bool, input_passthrough: bool) {
	let _ = window.set_cursor_hittest(!(transparent && input_passthrough));
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CloseHotkey {
	key: String,
	control: bool,
	shift: bool,
	alt: bool,
	super_key: bool,
}

impl CloseHotkey {
	fn parse(text: &str) -> Result<Option<Self>, String> {
		let trimmed = text.trim();
		if trimmed.is_empty() || matches!(normalize_key_name(trimmed).as_str(), "none" | "disabled" | "off") {
			return Ok(None);
		}

		let mut key = None;
		let mut control = false;
		let mut shift = false;
		let mut alt = false;
		let mut super_key = false;
		for part in trimmed.split('+').map(str::trim).filter(|part| !part.is_empty()) {
			match normalize_key_name(part).as_str() {
				"ctrl" | "control" => control = true,
				"shift" => shift = true,
				"alt" | "option" => alt = true,
				"super" | "meta" | "cmd" | "command" | "win" | "windows" => super_key = true,
				candidate => {
					if key.replace(candidate.to_string()).is_some() {
						return Err("hotkey must contain a single non-modifier key".to_string());
					}
				}
			}
		}
		let key = key.ok_or_else(|| "hotkey must contain a non-modifier key".to_string())?;
		Ok(Some(Self {
			key,
			control,
			shift,
			alt,
			super_key,
		}))
	}

	fn matches(&self, key: &Key, modifiers: ModifiersState) -> bool {
		modifiers.control_key() == self.control
			&& modifiers.shift_key() == self.shift
			&& modifiers.alt_key() == self.alt
			&& modifiers.super_key() == self.super_key
			&& logical_key_name(key).as_deref() == Some(self.key.as_str())
	}
}

fn logical_key_name(key: &Key) -> Option<String> {
	match key {
		Key::Named(named) => Some(normalize_key_name(&format!("{named:?}"))),
		Key::Character(character) => Some(normalize_key_name(character.as_str())),
		Key::Dead(_) | Key::Unidentified(_) => None,
	}
}

fn normalize_key_name(name: &str) -> String {
	let normalized = name.trim().to_ascii_lowercase().replace(' ', "");
	match normalized.as_str() {
		"esc" => "escape".to_string(),
		"return" => "enter".to_string(),
		"spacebar" => "space".to_string(),
		"del" => "delete".to_string(),
		"left" => "arrowleft".to_string(),
		"right" => "arrowright".to_string(),
		"up" => "arrowup".to_string(),
		"down" => "arrowdown".to_string(),
		_ => normalized,
	}
}

fn is_false(value: &bool) -> bool {
	!*value
}

#[derive(Clone, Serialize)]
struct RendererRuntimeSnapshot {
	connected: bool,
	protocol: String,
	control_capabilities: Vec<String>,
	#[serde(default)]
	scene_state: String,
	uptime_secs: u64,
	fps: Option<f32>,
	cpu_ms: Option<f32>,
	gpu_ms: Option<f32>,
	ram_mb: Option<u64>,
	surface_width: Option<u32>,
	surface_height: Option<u32>,
	/// 現在のウィンドウ outer 位置（px）。プロファイルへの保存・復元用。
	#[serde(default)]
	window_position: Option<[i32; 2]>,
	/// 現在のウィンドウ inner サイズ（px）。プロファイルへの保存・復元用。
	#[serde(default)]
	window_inner_size: Option<[u32; 2]>,
	aa: AaMode,
	texture_resolution_limit: TextureResolutionLimit,
	texture_compression: TextureCompressionMode,
	mipmap_filter: TextureMipmapFilter,
	texture_compression_advanced: TextureCompressionAdvancedOptions,
	processed_texture_cache: bool,
	texture_summary: Option<mesh_pass::TextureUploadSummary>,
	#[serde(default)]
	active_wardrobe_set: Option<String>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	active_asset_groups: Vec<String>,
	#[serde(default, skip_serializing_if = "wardrobe_asset_upload_plan_is_default")]
	wardrobe_asset_upload: WardrobeAssetUploadPlan,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	resolver_cache_key: Option<un_avatar_core::UnaRuntimeResolverCacheKey>,
	#[serde(default)]
	last_action_id: Option<String>,
	#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
	runtime_parameter_values: BTreeMap<String, f32>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	runtime_parameter_definitions: Vec<un_avatar_core::UnaRuntimeParameterDefinition>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	runtime_parameter_conflicts: Vec<un_avatar_core::UnaRuntimeParameterConflict>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	wardrobe_actions: Vec<gpu::RuntimeWardrobeActionStatus>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	runtime_actions: Vec<gpu::RuntimeActionStatus>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	runtime_action_target_write_collisions: Vec<un_avatar_core::UnaEvaluationTargetWriteCollision>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	runtime_action_restore_readiness: Vec<un_avatar_core::UnaEvaluationRestoreReadiness>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	runtime_action_restore_baseline_candidates: Vec<un_avatar_core::UnaEvaluationRestoreBaselineCandidate>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	runtime_action_restore_baseline_capture_plan: Vec<un_avatar_core::UnaEvaluationRestoreBaselineEntry>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	runtime_action_restore_apply_plan: Vec<un_avatar_core::UnaEvaluationRestoreApplyEntry>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	menu_action_candidates: Vec<gpu::RuntimeMenuActionCandidateStatus>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	menu_wardrobe_candidates: Vec<gpu::RuntimeMenuWardrobeCandidateStatus>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	contact_parameter_declarations: Vec<gpu::RuntimeContactParameterDeclarationStatus>,
	#[serde(default, skip_serializing_if = "is_false")]
	contact_parameter_emission_enabled: bool,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	contact_parameter_emissions: Vec<gpu::RuntimeContactParameterEmissionStatus>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	contact_probes: Vec<gpu::RuntimeContactProbeStatus>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	dynamics_groups: Vec<gpu::RuntimeDynamicsGroupStatus>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	dynamics_colliders: Vec<gpu::RuntimeDynamicsColliderStatus>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	dynamics_constraint_refs: Vec<gpu::RuntimeDynamicsConstraintRefStatus>,
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
	/// VMC `Root.translation` を scene root へ加算しているか（既定 false）。
	/// Waidayo 系のキャリブレーション都合で意図せず非ゼロな translation を送られる場合に、
	/// アバターが前後にズレないよう default OFF。フルボディトラッカー利用時は ON に切替。
	#[serde(default)]
	apply_vmc_root_translation: bool,
	/// UNMotion/Zenoh の receiver が動いているか。OFF (manifest `[motion.unmotion_zenoh] enabled = false`) なら false。
	#[serde(default)]
	unmotion_zenoh_enabled: bool,
	/// `ZenohTopicStrategy::base_key_expr` 現在値。Supervisor が UI に表示するため。
	#[serde(default)]
	unmotion_zenoh_key: String,
	#[serde(default)]
	unmotion_zenoh_received_frames: u64,
	#[serde(default)]
	motion_applied_frames: u64,
	/// 現在の profile と可視 material set が external AudioLink texture を必要としているか。
	#[serde(default)]
	audio_link_texture_needed: bool,
	#[serde(default)]
	runtime_requires_audio_link_texture: bool,
	#[serde(default)]
	runtime_requires_screen_refraction: bool,
	#[serde(default)]
	runtime_requires_fur: bool,
	/// 旧 status 互換の primary motion source。
	#[serde(default)]
	primary_motion_source: crate::options::PrimaryMotionSource,
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
	camera_locked: bool,
	#[serde(default)]
	window_focused: bool,
	#[serde(default)]
	window_activation_seq: u64,
	#[serde(default)]
	minimized: bool,
	#[serde(default)]
	camera: Option<crate::gpu::CameraStateSnapshot>,
	/// 現在の背景クリアカラー（`opts.clear_color`）。Supervisor で `Alpha 0` / `Dark` ボタンの active 表示と
	/// Transparent ショートカットの状態判定に使う。
	#[serde(default)]
	clear_color: [f64; 4],
	/// renderer 起動時に `--transparent` で立ち上がった（≒ winit が透明ウィンドウ属性を持っている）か。
	/// Windows + winit ではランタイム切替が効かないため、ボタンの active 表示と「透けない原因」の説明に使う。
	#[serde(default)]
	transparent_window: bool,
	/// 現在 click-through (input passthrough) が有効か。
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

fn initial_runtime_snapshot(opts: &AvatarWindowOptions) -> RendererRuntimeSnapshot {
	RendererRuntimeSnapshot {
		connected: true,
		protocol: "local-tcp-json-v2".to_string(),
		control_capabilities: RENDERER_CONTROL_CAPABILITIES
			.iter()
			.map(|capability| (*capability).to_string())
			.collect(),
		scene_state: SCENE_STATE_SPLASH.to_string(),
		uptime_secs: 0,
		fps: None,
		cpu_ms: None,
		gpu_ms: None,
		ram_mb: None,
		surface_width: opts.spout.width,
		surface_height: opts.spout.height,
		// 起動直後は未確定。ウィンドウ生成後に逐次更新される（about_to_wait などのフレームループで）。
		window_position: opts.window_position,
		window_inner_size: Some([opts.window_width, opts.window_height]),
		aa: opts.aa,
		texture_resolution_limit: opts.texture_resolution_limit,
		texture_compression: opts.texture_compression,
		mipmap_filter: opts.mipmap_filter,
		texture_compression_advanced: opts.texture_compression_advanced.clone(),
		processed_texture_cache: opts.processed_texture_cache,
		texture_summary: None,
		active_wardrobe_set: model_loader::normalize_wardrobe_set_id(opts.wardrobe_set.as_deref()).map(str::to_owned),
		active_asset_groups: Vec::new(),
		wardrobe_asset_upload: WardrobeAssetUploadPlan::default(),
		resolver_cache_key: None,
		last_action_id: None,
		runtime_parameter_values: BTreeMap::new(),
		runtime_parameter_definitions: Vec::new(),
		runtime_parameter_conflicts: Vec::new(),
		wardrobe_actions: Vec::new(),
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
		dynamics_colliders: Vec::new(),
		dynamics_constraint_refs: Vec::new(),
		spout_available: crate::spout::backend_available(),
		spout_enabled: opts.spout.enabled,
		spout_name: if opts.spout.enabled { Some(opts.spout.name.clone()) } else { None },
		spout_width: opts.spout.width,
		spout_height: opts.spout.height,
		spout_frames_attempted: 0,
		spout_frames_sent: 0,
		spout_frame_failures: 0,
		spout_consecutive_failures: 0,
		spout_last_send_ok: None,
		spout_last_readback_ms: None,
		spout_last_send_ms: None,
		spout_last_total_ms: None,
		spout_sender_initialized: None,
		spout_sender_width: None,
		spout_sender_height: None,
		expression_presets: Vec::new(),
		look_at_enabled: opts.eye_look_at_clamp_deg.is_some(),
		look_at_clamp_deg: opts.eye_look_at_clamp_deg,
		apply_vmc_root_translation: opts.apply_vmc_root_translation,
		unmotion_zenoh_enabled: opts.unmotion_zenoh.enabled,
		unmotion_zenoh_key: opts.unmotion_zenoh.base_key_expr.clone(),
		unmotion_zenoh_received_frames: 0,
		motion_applied_frames: 0,
		audio_link_texture_needed: false,
		runtime_requires_audio_link_texture: false,
		runtime_requires_screen_refraction: false,
		runtime_requires_fur: false,
		primary_motion_source: opts.primary_motion_source,
		show_axes: opts.show_axes,
		show_bone_colliders: opts.show_bone_colliders,
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
		camera_locked: opts.camera_locked,
		window_focused: false,
		window_activation_seq: 0,
		minimized: false,
		camera: None,
		clear_color: [opts.clear_color.r, opts.clear_color.g, opts.clear_color.b, opts.clear_color.a],
		transparent_window: opts.transparent,
		input_passthrough: opts.input_passthrough,
		startup_phase: None,
		startup_progress: None,
		startup_message: None,
		note: None,
	}
}

fn start_runtime_status_server(address: SocketAddr, opts: &AvatarWindowOptions) -> Arc<Mutex<RendererRuntimeSnapshot>> {
	let status = Arc::new(Mutex::new(initial_runtime_snapshot(opts)));
	let thread_status = Arc::clone(&status);
	thread::spawn(move || {
		let listener = match std::net::TcpListener::bind(address) {
			Ok(listener) => listener,
			Err(e) => {
				eprintln!("un-avatar-renderer: runtime status bind {address}: {e}");
				return;
			}
		};
		for stream in listener.incoming() {
			let Ok(stream) = stream else {
				continue;
			};
			let client_status = Arc::clone(&thread_status);
			thread::spawn(move || handle_runtime_status_client(stream, client_status));
		}
	});
	status
}

fn handle_runtime_status_client(mut stream: std::net::TcpStream, status: Arc<Mutex<RendererRuntimeSnapshot>>) {
	let stream_mode = runtime_status_stream_requested(&stream);
	if stream_mode {
		while write_runtime_status_snapshot(&mut stream, &status).is_ok() {
			thread::sleep(Duration::from_millis(250));
		}
	} else {
		let _ = write_runtime_status_snapshot(&mut stream, &status);
	}
}

fn runtime_status_stream_requested(stream: &std::net::TcpStream) -> bool {
	let Ok(reader_stream) = stream.try_clone() else {
		return false;
	};
	let _ = reader_stream.set_read_timeout(Some(Duration::from_millis(20)));
	let mut request = String::new();
	let Ok(bytes_read) = BufReader::new(reader_stream).read_line(&mut request) else {
		return false;
	};
	if bytes_read == 0 {
		return false;
	}
	let request = request.trim();
	request.eq_ignore_ascii_case("stream")
		|| serde_json::from_str::<serde_json::Value>(request)
			.ok()
			.and_then(|value| {
				value
					.get("request")
					.and_then(|request| request.as_str())
					.map(|request| request.eq_ignore_ascii_case("stream"))
			})
			.unwrap_or(false)
}

fn write_runtime_status_snapshot(stream: &mut std::net::TcpStream, status: &Arc<Mutex<RendererRuntimeSnapshot>>) -> std::io::Result<()> {
	let snapshot = status
		.lock()
		.map_err(|_| std::io::Error::other("runtime status lock poisoned"))?
		.clone();
	let json = serde_json::to_string(&snapshot).map_err(std::io::Error::other)?;
	writeln!(stream, "{json}")
}

#[derive(Deserialize)]
struct RuntimeBusControlRequest {
	request_id: String,
	command: serde_json::Value,
}

#[derive(Serialize)]
struct RuntimeBusControlResponse {
	request_id: String,
	ok: bool,
	error: Option<String>,
}

fn start_runtime_bus(
	base_key: String,
	opts: &AvatarWindowOptions,
	proxy: EventLoopProxy<RendererControlEvent>,
) -> Arc<Mutex<RendererRuntimeSnapshot>> {
	let status = Arc::new(Mutex::new(initial_runtime_snapshot(opts)));
	if let Ok(mut status) = status.lock() {
		status.protocol = "zenoh-json-v1".to_string();
	}
	let publish_status = Arc::clone(&status);
	let publish_key = format!("{base_key}/status");
	if let Err(e) = thread::Builder::new()
		.name("un-avatar-runtime-status-zenoh".into())
		.spawn(move || publish_runtime_status_loop(publish_key, publish_status))
	{
		eprintln!("un-avatar-renderer: spawn runtime status bus failed: {e}");
	}

	let control_key = format!("{base_key}/control");
	let response_base_key = format!("{base_key}/control/response");
	if let Err(e) = thread::Builder::new()
		.name("un-avatar-runtime-control-zenoh".into())
		.spawn(move || runtime_control_bus_loop(control_key, response_base_key, proxy))
	{
		eprintln!("un-avatar-renderer: spawn runtime control bus failed: {e}");
	}
	status
}

fn publish_runtime_status_loop(status_key: String, status: Arc<Mutex<RendererRuntimeSnapshot>>) {
	use zenoh::Wait as _;
	const STATUS_PUBLISH_INTERVAL: Duration = Duration::from_millis(100);
	const STATUS_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(1);
	let session = match zenoh::open(zenoh::Config::default()).wait() {
		Ok(session) => session,
		Err(e) => {
			eprintln!("un-avatar-renderer: runtime status bus open failed: {e}");
			return;
		}
	};
	let mut last_json = String::new();
	let mut last_publish = Instant::now() - STATUS_KEEPALIVE_INTERVAL;
	loop {
		let snapshot = match status.lock() {
			Ok(status) => status.clone(),
			Err(_) => return,
		};
		if let Ok(json) = serde_json::to_string(&snapshot) {
			let should_publish = json != last_json || last_publish.elapsed() >= STATUS_KEEPALIVE_INTERVAL;
			if should_publish {
				let _ = session.put(&status_key, json.as_str()).wait();
				last_json = json;
				last_publish = Instant::now();
			}
		}
		thread::sleep(STATUS_PUBLISH_INTERVAL);
	}
}

fn runtime_control_bus_loop(control_key: String, response_base_key: String, proxy: EventLoopProxy<RendererControlEvent>) {
	use zenoh::Wait as _;
	let session = match zenoh::open(zenoh::Config::default()).wait() {
		Ok(session) => session,
		Err(e) => {
			eprintln!("un-avatar-renderer: runtime control bus open failed: {e}");
			return;
		}
	};
	let subscriber = match session.declare_subscriber(&control_key).wait() {
		Ok(subscriber) => subscriber,
		Err(e) => {
			eprintln!("un-avatar-renderer: runtime control bus subscribe {control_key}: {e}");
			return;
		}
	};
	while let Ok(sample) = subscriber.recv() {
		let request = match serde_json::from_slice::<RuntimeBusControlRequest>(&sample.payload().to_bytes()) {
			Ok(request) => request,
			Err(e) => {
				eprintln!("un-avatar-renderer: runtime control bus parse failed: {e}");
				continue;
			}
		};
		let command_text = if let Some(command) = request.command.as_str() {
			command.to_string()
		} else {
			request.command.to_string()
		};
		let response_text = runtime_control_response(&command_text, &proxy);
		let response = RuntimeBusControlResponse {
			request_id: request.request_id.clone(),
			ok: response_text == "ok",
			error: (response_text != "ok").then(|| response_text.strip_prefix("err ").unwrap_or(&response_text).to_string()),
		};
		if let Ok(json) = serde_json::to_string(&response) {
			let _ = session.put(format!("{response_base_key}/{}", request.request_id), json).wait();
		}
	}
}

fn start_runtime_control_server(address: SocketAddr, proxy: EventLoopProxy<RendererControlEvent>) {
	thread::spawn(move || {
		let listener = match std::net::TcpListener::bind(address) {
			Ok(listener) => listener,
			Err(e) => {
				eprintln!("un-avatar-renderer: runtime control bind {address}: {e}");
				return;
			}
		};
		for stream in listener.incoming() {
			let Ok(stream) = stream else {
				continue;
			};
			let client_proxy = proxy.clone();
			thread::spawn(move || handle_runtime_control_client(stream, client_proxy));
		}
	});
}

fn handle_runtime_control_client(mut stream: std::net::TcpStream, proxy: EventLoopProxy<RendererControlEvent>) {
	let Ok(reader_stream) = stream.try_clone() else {
		let _ = writeln!(stream, "err read");
		return;
	};
	let mut reader = BufReader::new(reader_stream);
	loop {
		let mut command = String::new();
		match reader.read_line(&mut command) {
			Ok(0) => return,
			Ok(_) => {}
			Err(_) => {
				let _ = writeln!(stream, "err read");
				return;
			}
		}
		let command = command.trim();
		if command.is_empty() {
			continue;
		}
		let response = runtime_control_response(command, &proxy);
		if writeln!(stream, "{response}").is_err() {
			return;
		}
		let _ = stream.flush();
		if response == "err event-loop-closed" {
			return;
		}
	}
}

fn runtime_control_response(command: &str, proxy: &EventLoopProxy<RendererControlEvent>) -> String {
	if command == "scene_state" {
		return dispatch_scene_state_command(proxy);
	}
	match parse_renderer_control_command(command) {
		Ok(RendererControlCommand::Screenshot { path }) => dispatch_screenshot_command(proxy, path),
		Ok(RendererControlCommand::SetWardrobe { set_id }) => dispatch_set_wardrobe_command(proxy, set_id),
		Ok(RendererControlCommand::ActivateAction {
			action_id,
			supervisor_command,
			expression_menu_path,
			menu_path,
			wardrobe_set_id,
			parameter_name,
			parameter_value,
		}) => dispatch_activate_action_command(
			proxy,
			action_id,
			supervisor_command,
			expression_menu_path,
			menu_path,
			wardrobe_set_id,
			parameter_name,
			parameter_value,
		),
		Ok(RendererControlCommand::SetParameter { name, value }) => dispatch_set_parameter_command(proxy, name, value),
		Ok(command) => match proxy.send_event(command.into_event()) {
			Ok(()) => "ok".to_string(),
			Err(_) => "err event-loop-closed".to_string(),
		},
		Err(e) => format!("err {e}"),
	}
}

fn dispatch_set_parameter_command(proxy: &EventLoopProxy<RendererControlEvent>, name: String, value: f32) -> String {
	let name = name.trim().to_string();
	if name.is_empty() {
		return "err parameter name required".to_string();
	}
	let result: CommandResultSlot = Arc::new(Mutex::new(None));
	let event = RendererControlEvent::SetParameter {
		name,
		value,
		result: Arc::clone(&result),
	};
	if proxy.send_event(event).is_err() {
		return "err event-loop-closed".to_string();
	}
	wait_command_result(result, Duration::from_secs(2), "set_parameter")
}

fn dispatch_set_wardrobe_command(proxy: &EventLoopProxy<RendererControlEvent>, set_id: String) -> String {
	let set_id = match model_loader::require_wardrobe_set_id(&set_id) {
		Ok(set_id) => set_id.to_string(),
		Err(e) => return format!("err {e}"),
	};
	let result: CommandResultSlot = Arc::new(Mutex::new(None));
	let event = RendererControlEvent::SetWardrobe {
		set_id,
		result: Arc::clone(&result),
	};
	if proxy.send_event(event).is_err() {
		return "err event-loop-closed".to_string();
	}
	wait_command_result(result, Duration::from_secs(2), "set_wardrobe")
}

fn dispatch_activate_action_command(
	proxy: &EventLoopProxy<RendererControlEvent>,
	action_id: Option<String>,
	supervisor_command: Option<String>,
	expression_menu_path: Option<String>,
	menu_path: Option<String>,
	wardrobe_set_id: Option<String>,
	parameter_name: Option<String>,
	parameter_value: Option<f32>,
) -> String {
	let action_id = action_id.map(|value| value.trim().to_string()).filter(|value| !value.is_empty());
	let supervisor_command = supervisor_command
		.map(|value| value.trim().to_string())
		.filter(|value| !value.is_empty());
	let expression_menu_path = expression_menu_path
		.map(|value| value.trim().to_string())
		.filter(|value| !value.is_empty());
	let menu_path = menu_path.map(|value| value.trim().to_string()).filter(|value| !value.is_empty());
	let wardrobe_set_id = wardrobe_set_id
		.map(|value| value.trim().to_string())
		.filter(|value| !value.is_empty());
	let parameter_name = parameter_name
		.map(|value| value.trim().to_string())
		.filter(|value| !value.is_empty());
	if parameter_value.is_some() && parameter_name.is_none() {
		return "err parameter_name required when parameter_value is provided".to_string();
	}
	if parameter_name.is_some() && parameter_value.is_none() {
		return "err parameter_value required when parameter_name is provided".to_string();
	}
	if action_id.is_none()
		&& supervisor_command.is_none()
		&& expression_menu_path.is_none()
		&& menu_path.is_none()
		&& parameter_name.is_none()
	{
		return "err action_id, menu_path, supervisor_command, expression_menu_path, wardrobe_set_id, or parameter_name required"
			.to_string();
	}
	if menu_path.is_none() && wardrobe_set_id.is_some() {
		return "err menu_path is required when wardrobe_set_id is provided".to_string();
	}
	let result: CommandResultSlot = Arc::new(Mutex::new(None));
	let event = RendererControlEvent::ActivateAction {
		action_id,
		supervisor_command,
		expression_menu_path,
		menu_path,
		wardrobe_set_id,
		parameter_name,
		parameter_value,
		result: Arc::clone(&result),
	};
	if proxy.send_event(event).is_err() {
		return "err event-loop-closed".to_string();
	}
	wait_command_result(result, Duration::from_secs(2), "activate_action")
}

fn dispatch_scene_state_command(proxy: &EventLoopProxy<RendererControlEvent>) -> String {
	let result: SceneStateResultSlot = Arc::new(Mutex::new(None));
	let event = RendererControlEvent::SceneState {
		result: Arc::clone(&result),
	};
	if proxy.send_event(event).is_err() {
		return "err event-loop-closed".to_string();
	}
	let deadline = Instant::now() + Duration::from_secs(2);
	loop {
		if let Ok(guard) = result.lock() {
			if let Some(state) = guard.as_ref() {
				return state.clone();
			}
		}
		if Instant::now() >= deadline {
			return "err scene_state timeout".to_string();
		}
		thread::sleep(Duration::from_millis(20));
	}
}

fn dispatch_screenshot_command(proxy: &EventLoopProxy<RendererControlEvent>, path: String) -> String {
	if path.trim().is_empty() {
		return "err screenshot path required".to_string();
	}
	let result: CommandResultSlot = Arc::new(Mutex::new(None));
	let event = RendererControlEvent::Screenshot {
		path: std::path::PathBuf::from(path),
		result: Arc::clone(&result),
	};
	if proxy.send_event(event).is_err() {
		return "err event-loop-closed".to_string();
	}
	wait_command_result(result, Duration::from_secs(10), "screenshot")
}

fn wait_command_result(result: CommandResultSlot, timeout: Duration, command_name: &str) -> String {
	let deadline = Instant::now() + timeout;
	loop {
		if let Ok(guard) = result.lock() {
			if let Some(outcome) = guard.as_ref() {
				return match outcome {
					Ok(()) => "ok".to_string(),
					Err(e) => format!("err {e}"),
				};
			}
		}
		if Instant::now() >= deadline {
			return format!("err {command_name} timeout");
		}
		thread::sleep(Duration::from_millis(20));
	}
}

fn parse_renderer_control_command(command: &str) -> Result<RendererControlCommand, String> {
	match command {
		"shutdown" => Ok(RendererControlCommand::Shutdown),
		"reset_camera" => Ok(RendererControlCommand::ResetCamera),
		command if command.starts_with('{') => serde_json::from_str(command).map_err(|e| format!("invalid-json-command: {e}")),
		_ => Err("unsupported-command".to_string()),
	}
}

fn normalize_menu_path(path: &str) -> Vec<String> {
	path.trim()
		.trim_matches('/')
		.split('/')
		.map(str::trim)
		.filter(|part| !part.is_empty())
		.map(str::to_string)
		.collect::<Vec<_>>()
}

fn resolve_activate_action_from_menu_path(
	menu_path: &str,
	wardrobe_set_id: Option<&str>,
	candidates: &[gpu::RuntimeMenuWardrobeCandidateStatus],
) -> Result<String, String> {
	let normalized_menu_path = normalize_menu_path(menu_path);
	if normalized_menu_path.is_empty() {
		return Err("menu_path is required".to_string());
	}
	let mut matching_action_ids = Vec::<String>::new();
	for candidate in candidates {
		let candidate_menu_path = candidate
			.menu_path
			.iter()
			.map(|part| part.trim())
			.filter(|part| !part.is_empty())
			.map(str::to_string)
			.collect::<Vec<_>>();
		let menu_path_matches = if normalized_menu_path.len() == candidate_menu_path.len() {
			normalized_menu_path == candidate_menu_path
		} else if normalized_menu_path.len() == candidate_menu_path.len() + 1 {
			normalized_menu_path.starts_with(&candidate_menu_path)
		} else {
			false
		};
		if menu_path_matches && wardrobe_set_id.is_none_or(|set_id| set_id == candidate.wardrobe_set_id.as_str()) {
			if !matching_action_ids.iter().any(|action_id| action_id == &candidate.action_id) {
				matching_action_ids.push(candidate.action_id.clone());
			}
		}
	}
	let resolved_action_id = matching_action_ids
		.first()
		.cloned()
		.ok_or_else(|| format!("no menu wardrobe candidate found for menu_path={menu_path}"))?;
	if matching_action_ids.len() > 1 {
		return Err(format!(
			"menu path {} is ambiguous across {} action(s): {}",
			menu_path,
			matching_action_ids.len(),
			matching_action_ids.join(", ")
		));
	}
	Ok(resolved_action_id)
}

/// イベントループをブロックしてウィンドウを表示する。
pub fn run(opts: AvatarWindowOptions) -> Result<(), RunError> {
	let event_loop = EventLoop::<RendererControlEvent>::with_user_event()
		.build()
		.map_err(|e| RunError::EventLoop(e.to_string()))?;
	let event_proxy = event_loop.create_proxy();
	if opts.runtime_bus_key.is_none() {
		if let Some(address) = opts.runtime_control_address {
			start_runtime_control_server(address, event_proxy.clone());
		}
	}

	let mut app = AvatarApp::new(opts, event_proxy);
	event_loop.run_app(&mut app).map_err(|e| RunError::EventLoop(e.to_string()))
}

/// `un-avatar-renderer` バイナリと同一の clap CLI で起動する。
pub fn run_cli() -> Result<(), RunError> {
	#[derive(Parser, Debug)]
	#[command(name = "un-avatar-renderer", version)]
	struct Cli {
		#[arg(
			long,
			value_name = "PATH",
			help = "renderer起動manifest（TOML）。CLIで明示した値はmanifestを上書き"
		)]
		manifest: Option<PathBuf>,
		#[arg(long, hide = true)]
		validate_startup: bool,
		#[arg(long, hide = true)]
		dump_skin_tone_matching: bool,
		#[arg(long, default_value = "UN Avatar")]
		title: String,
		#[arg(long, help = "透過ウィンドウ（環境により未対応の場合あり）")]
		transparent: bool,
		#[arg(long, help = "Transparent時にrenderer windowのmouse hit-testを背面へ通す")]
		input_passthrough: bool,
		#[arg(long, help = "タイトルバーと枠を非表示にする")]
		undecorated: bool,
		#[arg(long, help = "ウィンドウを常に最前面にする")]
		always_on_top: bool,
		#[arg(
			long,
			default_value = "Escape",
			value_name = "HOTKEY",
			help = "ボーダーレス時にrenderer process内で閉じるhotkey（例: Escape, Ctrl+Q, None）"
		)]
		close_hotkey: String,
		#[arg(long, default_value_t = 800, value_name = "PX", help = "ウィンドウ初期幅")]
		window_width: u32,
		#[arg(long, default_value_t = 600, value_name = "PX", help = "ウィンドウ初期高さ")]
		window_height: u32,
		#[arg(long, default_value_t = 0.12)]
		cr: f64,
		#[arg(long, default_value_t = 0.14)]
		cg: f64,
		#[arg(long, default_value_t = 0.18)]
		cb: f64,
		#[arg(long, default_value_t = 1.0)]
		ca: f64,
		#[arg(long, help = "タイトルバーへの FPS・計測表示を出さない（ウィンドウタイトルは --title 固定）")]
		no_fps_title: bool,
		#[arg(
			long,
			value_name = "PATH",
			help = "表示するモデル: glTF（.gltf / .glb）または VRM（.vrm / VRM 拡張付き .glb）または .unavatar。メッシュ表示モード（空シーンのスカイに代わる）"
		)]
		gltf: Option<PathBuf>,
		#[arg(
			long,
			value_name = "ID",
			help = ".unavatar wardrobe set id。Base 適用後に指定セットを重ねてからロード"
		)]
		wardrobe_set: Option<String>,
		#[arg(long, value_name = "PATH", help = "ウィンドウ・タスクバー用アイコン画像（PNG/JPEG等）")]
		icon: Option<PathBuf>,
		#[arg(long, value_name = "IP:PORT", help = "VMC Marionette: UDP 待受アドレス（例: 0.0.0.0:39539）")]
		vmc_address: Option<SocketAddr>,
		#[arg(long, value_name = "PORT", hide = true)]
		vmc_port: Option<u16>,
		#[arg(
			long,
			help = "Spout2 送出（Windows）。標準配布は cargo xtask package でSpout2込みビルドを作成。開発手動ビルドは --features spout-sdk と SPOUT2_* / PATH が必要"
		)]
		spout: bool,
		#[arg(long, default_value = "UN Avatar Spout", help = "Spout 送信者名")]
		spout_name: String,
		#[arg(long, value_name = "PX", help = "Spout 解像度幅（省略時はウィンドウ幅）")]
		spout_width: Option<u32>,
		#[arg(long, value_name = "PX", help = "Spout 解像度高さ（省略時はウィンドウ高さ）")]
		spout_height: Option<u32>,
		#[arg(long, value_enum, default_value_t = AaMode::Off, help = "Anti-aliasing mode: off / fxaa / smaa / msaa")]
		aa: AaMode,
		#[arg(
			long,
			value_enum,
			default_value_t = TextureResolutionLimit::Off,
			help = "ロード時のテクスチャ長辺上限: off / auto / 8k / 4k / 2k / 1k。offが既定"
		)]
		texture_resolution_limit: TextureResolutionLimit,
		#[arg(
			long,
			value_enum,
			default_value_t = TextureCompressionMode::Balanced,
			help = "テクスチャ圧縮方針: source / balanced / memory / compat。既定はbalanced。旧auto/advancedはbalancedとして読む"
		)]
		texture_compression: TextureCompressionMode,
		#[arg(
			long,
			value_enum,
			default_value_t = TextureMipmapFilter::Mitchell,
			help = "mipmap生成フィルタ: box2x2 / bilinear / bicubic / catmull_rom / lanczos3 / mitchell。既定はmitchell"
		)]
		mipmap_filter: TextureMipmapFilter,
		#[arg(long, value_enum, default_value_t = RenderBackend::Vulkan, help = "wgpu backend: vulkan / dx12 / auto。既定はvulkan")]
		render_backend: RenderBackend,
		#[arg(long, value_enum, default_value_t = BlockCompressionEncoder::Gpu, help = "BCn encoder: gpu / cpu。既定はgpu")]
		block_compression_encoder: BlockCompressionEncoder,
		#[arg(
			long,
			default_value_t = 4,
			value_name = "N",
			help = "CPU BCn worker threads。実行時に論理CPU数でclamp"
		)]
		block_compression_cpu_threads: usize,
		#[arg(long, help = "processed texture cacheを無効化（resize/mipmap済みRGBAのディスクキャッシュ）")]
		no_processed_texture_cache: bool,
		#[arg(long, help = "実験的な顔/体テクスチャの肌色合わせを有効化（ロード時処理）")]
		skin_tone_matching: bool,
		#[arg(
			long,
			value_name = "IP:PORT",
			help = "Supervisor runtime status endpoint（通常はSupervisorが指定）"
		)]
		runtime_status_address: Option<SocketAddr>,
		#[arg(
			long,
			value_name = "IP:PORT",
			help = "Supervisor runtime control endpoint（通常はSupervisorが指定）"
		)]
		runtime_control_address: Option<SocketAddr>,
		#[arg(
			long = "runtime-bus-key",
			value_name = "KEY",
			help = "Supervisor runtime IPC Zenoh base key（通常はSupervisorが指定）"
		)]
		runtime_bus_key: Option<String>,
		#[arg(
			long,
			help = "VRM SpringBone シミュレーションを無効化（既定 ON。静止画用途で揺れを完全に止めたいときに指定）"
		)]
		no_spring_bones: bool,
		#[arg(
			long,
			value_name = "PATH",
			help = "調査用ログの追記先。--debug-vmc 等と併用。省略時はカテゴリ有効なら stderr のみ"
		)]
		debug_log: Option<PathBuf>,
		#[arg(long, help = "--debug-log 指定時にファイルに加えて標準エラーにも同じ行を出す")]
		debug_stderr: bool,
		#[arg(long, help = "VMC: 受信元・バイト数・OSC デコード失敗時の hex・Humanoid に落ちないボーン名など")]
		debug_vmc: bool,
		#[arg(long, help = "シーン: Humanoid プロファイルのキー・ルートなどを低頻度で記録")]
		debug_scene: bool,
		#[arg(long, help = "モーフ: 式プリセットウェイト要約を低頻度で記録")]
		debug_morph: bool,
		#[arg(long, help = "上記 debug_vmc / debug_scene / debug_morph をまとめて有効化")]
		debug_all: bool,
		#[arg(long, help = "式プリセット・VMC Blend をモーフに適用しない（既定は適用する）")]
		no_expression_morphs: bool,
		#[arg(long, help = "VMC の LeftEye/RightEye 骨変形を適用しない（視線の切り分け用）")]
		no_eye_look: bool,
		#[arg(long, help = "VRM 1.0 LookAt 簡易クランプを無効化（VMC eye 回転をそのまま使う）")]
		no_look_at: bool,
		#[arg(long, help = "LookAt 簡易クランプの yaw/pitch 上限角度（度、既定 30°）", value_name = "DEG")]
		look_at_clamp_deg: Option<f32>,
		#[arg(
			long,
			help = "VMC `/VMC/Ext/Root/Pos` の translation を scene root に加算する（既定 OFF）。フルボディトラッカーで位置移動も載せたい時に ON。"
		)]
		apply_vmc_root_translation: bool,
		#[arg(
			long = "unmotion-zenoh",
			help = "UNMotion/Zenoh で UNMotionFrame を受信する（既定 OFF）。--gltf 必須。"
		)]
		unmotion_zenoh_enabled: bool,
		#[arg(
			long = "unmotion-zenoh-key",
			value_name = "KEY",
			help = "UNMotion/Zenoh の base key (既定 \"un-motion/frame\")。subscribe key は実際には \"<key>/v1\" に展開される。"
		)]
		unmotion_zenoh_key: Option<String>,
		#[arg(
			long = "primary-motion-source",
			value_enum,
			help = "VMC と UNMotion 同時受信時の primary 選択 (既定 vmc)。"
		)]
		primary_motion_source: Option<crate::options::PrimaryMotionSource>,
		#[arg(long, help = "XYZ デバッグ軸を表示（既定は非表示）")]
		show_axes: bool,
		#[arg(long, help = "起動直後にウィンドウを最小化する（既定は非最小化）")]
		start_minimized: bool,
		#[arg(
			long,
			help = "診断用: MToon outline 描画を全 skip（一部 VRM で目周辺に肌色寄りの太い outline が出る現象の切り分け用）"
		)]
		disable_mtoon_outlines: bool,
		#[arg(
			long,
			help = "診断用: 全メッシュを不透明 LitLambert + baseColorTexture のみで描画（MToon/アルファ表現をシェーダ側で無視）"
		)]
		simple_basecolor_only: bool,
		#[arg(long, help = "ロード時にマテリアル名・alphaMode・スキン joint 本数などを stderr に出す")]
		debug_material_dump: bool,
		#[arg(long, help = "スキニングを無効化しメッシュ三角形をバインド姿勢の剛体変形のみで描画")]
		debug_bind_pose: bool,
		#[arg(long, help = "テクスチャを無視してプリミティブごとに異なる単色で描画")]
		debug_primitive_colors: bool,
		#[arg(long, help = "式に加え default モーフウェイトも含めモーフをすべて 0（目の閉じの切り分け）")]
		debug_zero_morphs: bool,
		#[arg(
			long,
			help = "名前が iris/pupil 等に一致するマテリアルをシェーダ上 Opaque 扱いにして MASK discard を避ける"
		)]
		relax_iris_alpha: bool,
		#[arg(
			long,
			help = "ジョイント行列を inv(meshWorld)*joint*IBM ではなく joint*IBM のみに（旧実装・エクスポータ差の確認）"
		)]
		debug_skin_legacy_no_inv_mesh: bool,
		#[arg(long, help = "診断用: MToon parametric Rim Lighting 寄与を 0 に固定")]
		debug_disable_rim_lighting: bool,
		#[arg(long, help = "診断用: shading_shift_factor と shadingShiftTexture の寄与を 0 に固定")]
		debug_force_shading_shift_zero: bool,
		#[arg(long, help = "診断用: MToon matcap (sphere add) 寄与を 0 に固定")]
		debug_disable_matcap: bool,
		#[arg(long, help = "診断用: emissive (emissive_factor × emissive_tex) 寄与を 0 に固定")]
		debug_disable_emissive: bool,
		#[arg(
			long,
			help = "診断用: MToon shade_term を base 色で置換（shade_color × shade_tex を base に差し替え）"
		)]
		debug_disable_shade_color: bool,
		#[arg(long, help = "診断用: normalTexture を使わず頂点法線のみで shading / rim を計算")]
		debug_disable_normal_map: bool,
		#[arg(long, help = "診断用: lilToon reflection / specular / gem reflection 寄与を 0 に固定")]
		debug_disable_reflection: bool,
		#[arg(
			long,
			help = "診断用: toon path を base (alb × base_color) のみで早期 return（shading/GI/rim/matcap/emissive 全 skip）"
		)]
		debug_base_texture_only: bool,
		#[arg(long, help = "診断用: lilToon Fur shell pass を完全に無効化")]
		debug_disable_fur: bool,
	}

	let cli = Cli::parse();
	let debug_all = cli.debug_all;
	let mut opts = if let Some(path) = cli.manifest.as_deref() {
		let mut opts = AvatarWindowOptions::default();
		match manifest::RendererManifest::load(path) {
			Ok(manifest) => manifest.apply_to(&mut opts),
			Err(e) => {
				eprintln!("un-avatar-renderer: {e}");
				return Err(RunError::EventLoop(e));
			}
		}
		opts
	} else {
		AvatarWindowOptions::default()
	};
	let cli_opts = AvatarWindowOptions {
		title: cli.title,
		decorations: !cli.undecorated,
		transparent: cli.transparent,
		input_passthrough: cli.input_passthrough,
		always_on_top: cli.always_on_top,
		close_hotkey: cli.close_hotkey,
		window_width: cli.window_width,
		window_height: cli.window_height,
		// CLI からは位置指定なし。manifest 経由で指定された場合のみ apply される。
		window_position: None,
		show_fps_in_title: !cli.no_fps_title,
		gltf_path: cli.gltf,
		wardrobe_set: cli.wardrobe_set,
		icon_path: cli.icon,
		vmc_address: cli.vmc_address.or_else(|| cli.vmc_port.map(vmc_addr_from_port)),
		unmotion_zenoh: crate::options::UnmotionZenohOptions {
			enabled: cli.unmotion_zenoh_enabled,
			base_key_expr: cli.unmotion_zenoh_key.clone().unwrap_or_else(|| "un-motion/frame".to_string()),
		},
		audio_link: Default::default(),
		primary_motion_source: cli.primary_motion_source.unwrap_or_default(),
		spout: SpoutWindowOptions {
			enabled: cli.spout,
			name: cli.spout_name,
			width: cli.spout_width,
			height: cli.spout_height,
		},
		environment_color: EnvironmentColorOptions::default(),
		lighting: LightingOptions::default(),
		bloom: BloomOptions::default(),
		aa: cli.aa,
		texture_resolution_limit: cli.texture_resolution_limit,
		texture_compression: cli.texture_compression,
		mipmap_filter: cli.mipmap_filter,
		render_backend: cli.render_backend,
		block_compression_encoder: cli.block_compression_encoder,
		block_compression_cpu_threads: cli.block_compression_cpu_threads.max(1),
		texture_compression_advanced: TextureCompressionAdvancedOptions::default(),
		processed_texture_cache: true,
		skin_tone_matching: cli.skin_tone_matching,
		runtime_status_address: cli.runtime_status_address,
		runtime_control_address: cli.runtime_control_address,
		runtime_bus_key: cli.runtime_bus_key.clone(),
		clear_color: wgpu::Color {
			r: cli.cr,
			g: cli.cg,
			b: cli.cb,
			a: cli.ca,
		},
		enable_spring_bones: !cli.no_spring_bones,
		bone_colliders: Default::default(),
		spring_bone_physics: DynamicsPhysicsConfig::default(),
		debug: WindowDebugOptions {
			log_path: cli.debug_log.clone(),
			mirror_stderr: cli.debug_stderr,
			vmc: cli.debug_vmc || debug_all,
			scene: cli.debug_scene || debug_all,
			morph: cli.debug_morph || debug_all,
		},
		disable_expression_morphs: cli.no_expression_morphs,
		disable_vmc_eye_look: cli.no_eye_look,
		eye_look_at_clamp_deg: AvatarWindowOptions::default().eye_look_at_clamp_deg,
		apply_vmc_root_translation: cli.apply_vmc_root_translation,
		simple_basecolor_only: cli.simple_basecolor_only,
		debug_material_dump: cli.debug_material_dump,
		show_axes: false,
		show_bone_colliders: false,
		camera_locked: false,
		start_minimized: cli.start_minimized,
		disable_mtoon_outlines: cli.disable_mtoon_outlines,
		debug_disable_rim_lighting: cli.debug_disable_rim_lighting,
		debug_force_shading_shift_zero: cli.debug_force_shading_shift_zero,
		debug_disable_matcap: cli.debug_disable_matcap,
		debug_disable_emissive: cli.debug_disable_emissive,
		debug_disable_shade_color: cli.debug_disable_shade_color,
		debug_disable_normal_map: cli.debug_disable_normal_map,
		debug_base_texture_only: cli.debug_base_texture_only,
		initial_camera_state: None,
		mesh_diagnostics: SceneMeshLoadOpts {
			force_simple_basecolor: false,
			debug_bind_pose: cli.debug_bind_pose,
			debug_primitive_colors: cli.debug_primitive_colors,
			debug_zero_morphs: cli.debug_zero_morphs,
			relax_iris_alpha: cli.relax_iris_alpha,
			debug_skin_legacy_no_inv_mesh: cli.debug_skin_legacy_no_inv_mesh,
			disable_mtoon_outlines: cli.disable_mtoon_outlines,
			debug_disable_rim_lighting: cli.debug_disable_rim_lighting,
			debug_force_shading_shift_zero: cli.debug_force_shading_shift_zero,
			debug_disable_matcap: cli.debug_disable_matcap,
			debug_disable_emissive: cli.debug_disable_emissive,
			debug_disable_shade_color: cli.debug_disable_shade_color,
			debug_disable_normal_map: cli.debug_disable_normal_map,
			debug_disable_reflection: cli.debug_disable_reflection,
			debug_base_texture_only: cli.debug_base_texture_only,
			disable_fur: cli.debug_disable_fur,
			avatar_outline: Default::default(),
			avatar_rim: Default::default(),
			avatar_matcap: Default::default(),
			avatar_specular: Default::default(),
			avatar_ambient_occlusion: Default::default(),
			skin_tone_matching: cli.skin_tone_matching,
		},
		contact_shadow: Default::default(),
		ssao: Default::default(),
	};
	merge_cli_options(&mut opts, cli_opts);
	if opts.mesh_diagnostics.disable_fur {
		eprintln!("un-avatar-renderer: diagnostics active: lilToon Fur shell pass disabled");
	}
	if opts.mesh_diagnostics.debug_disable_reflection {
		eprintln!("un-avatar-renderer: diagnostics active: lilToon reflection disabled");
	}
	if cli.no_processed_texture_cache {
		opts.processed_texture_cache = false;
	}
	if cli.no_look_at {
		opts.eye_look_at_clamp_deg = None;
	} else if let Some(deg) = cli.look_at_clamp_deg {
		opts.eye_look_at_clamp_deg = Some(deg);
	}
	if cli.apply_vmc_root_translation {
		opts.apply_vmc_root_translation = true;
	}
	if cli.show_axes {
		opts.show_axes = true;
	}
	if cli.start_minimized {
		opts.start_minimized = true;
	}
	if cli.disable_mtoon_outlines {
		opts.disable_mtoon_outlines = true;
	}
	if cli.debug_disable_rim_lighting {
		opts.debug_disable_rim_lighting = true;
	}
	if cli.debug_force_shading_shift_zero {
		opts.debug_force_shading_shift_zero = true;
	}
	if cli.debug_disable_matcap {
		opts.debug_disable_matcap = true;
	}
	if cli.debug_disable_emissive {
		opts.debug_disable_emissive = true;
	}
	if cli.debug_disable_shade_color {
		opts.debug_disable_shade_color = true;
	}
	if cli.debug_disable_normal_map {
		opts.debug_disable_normal_map = true;
	}
	if cli.debug_base_texture_only {
		opts.debug_base_texture_only = true;
	}
	if cli.validate_startup {
		validate_startup_options(&opts).map_err(RunError::EventLoop)?;
		return Ok(());
	}
	if cli.dump_skin_tone_matching {
		dump_skin_tone_matching(&opts).map_err(RunError::EventLoop)?;
		return Ok(());
	}
	run(opts)
}

fn validate_startup_options(opts: &AvatarWindowOptions) -> Result<(), String> {
	if let Some(path) = opts.gltf_path.as_deref() {
		if !path.is_file() {
			return Err(format!("startup validation: model not found: {}", path.display()));
		}
		model_loader::load_document(path, opts.wardrobe_set.as_deref())
			.map(|_| ())
			.map_err(|e| format!("startup validation: model import failed: {}: {e}", path.display()))
	} else {
		Ok(())
	}
}

fn dump_skin_tone_matching(opts: &AvatarWindowOptions) -> Result<(), String> {
	let Some(path) = opts.gltf_path.as_deref() else {
		return Err("skin tone matching dump: --gltf or manifest avatar_path is required".to_string());
	};
	let document = model_loader::load_document(path, opts.wardrobe_set.as_deref())
		.map_err(|e| format!("skin tone matching dump: model import failed: {}: {e}", path.display()))?;
	let runtime_model = document.runtime_model();
	let Some(scene) = runtime_model.scene() else {
		return Err(format!("skin tone matching dump: model has no scene: {}", path.display()));
	};
	let debug = mesh_pass::skin_tone_matching_debug_for_scene(scene);
	let text = serde_json::to_string_pretty(&debug).map_err(|e| format!("skin tone matching dump: serialize debug: {e}"))?;
	println!("{text}");
	Ok(())
}

fn merge_cli_options(opts: &mut AvatarWindowOptions, cli: AvatarWindowOptions) {
	let default = AvatarWindowOptions::default();
	if cli.title != default.title {
		opts.title = cli.title;
	}
	if !cli.decorations {
		opts.decorations = false;
	}
	if cli.transparent {
		opts.transparent = true;
	}
	if cli.input_passthrough {
		opts.input_passthrough = true;
	}
	if cli.always_on_top {
		opts.always_on_top = true;
	}
	if cli.close_hotkey != default.close_hotkey {
		opts.close_hotkey = cli.close_hotkey;
	}
	if cli.window_width != default.window_width {
		opts.window_width = cli.window_width;
	}
	if cli.window_height != default.window_height {
		opts.window_height = cli.window_height;
	}
	if cli.gltf_path.is_some() {
		opts.gltf_path = cli.gltf_path;
	}
	if cli.wardrobe_set.is_some() {
		opts.wardrobe_set = cli.wardrobe_set;
	}
	if cli.icon_path.is_some() {
		opts.icon_path = cli.icon_path;
	}
	if cli.clear_color != default.clear_color {
		opts.clear_color = cli.clear_color;
	}
	if !cli.show_fps_in_title {
		opts.show_fps_in_title = false;
	}
	if cli.vmc_address.is_some() {
		opts.vmc_address = cli.vmc_address;
	}
	if cli.spout.enabled {
		opts.spout.enabled = true;
	}
	if cli.spout.name != SpoutWindowOptions::default().name {
		opts.spout.name = cli.spout.name;
	}
	if cli.spout.width.is_some() {
		opts.spout.width = cli.spout.width;
	}
	if cli.spout.height.is_some() {
		opts.spout.height = cli.spout.height;
	}
	if cli.aa != default.aa {
		opts.aa = cli.aa;
	}
	if cli.texture_resolution_limit != default.texture_resolution_limit {
		opts.texture_resolution_limit = cli.texture_resolution_limit;
	}
	if cli.texture_compression != default.texture_compression {
		opts.texture_compression = cli.texture_compression;
	}
	if cli.mipmap_filter != default.mipmap_filter {
		opts.mipmap_filter = cli.mipmap_filter;
	}
	if cli.render_backend != default.render_backend {
		opts.render_backend = cli.render_backend;
	}
	if cli.block_compression_encoder != default.block_compression_encoder {
		opts.block_compression_encoder = cli.block_compression_encoder;
	}
	if cli.block_compression_cpu_threads != default.block_compression_cpu_threads {
		opts.block_compression_cpu_threads = cli.block_compression_cpu_threads.max(1);
	}
	if cli.skin_tone_matching {
		opts.skin_tone_matching = true;
	}
	if cli.runtime_status_address.is_some() {
		opts.runtime_status_address = cli.runtime_status_address;
	}
	if cli.runtime_control_address.is_some() {
		opts.runtime_control_address = cli.runtime_control_address;
	}
	if cli.runtime_bus_key.is_some() {
		opts.runtime_bus_key = cli.runtime_bus_key;
	}
	if cli.audio_link != default.audio_link {
		opts.audio_link = cli.audio_link;
	}
	// CLI で `--no-spring-bones` が指定されたときだけ強制 OFF。指定なしは
	// manifest 値（または既定値 true）をそのまま使う。
	if !cli.enable_spring_bones {
		opts.enable_spring_bones = false;
	}
	if cli.debug.log_path.is_some() {
		opts.debug.log_path = cli.debug.log_path;
	}
	if cli.debug.mirror_stderr {
		opts.debug.mirror_stderr = true;
	}
	if cli.debug.vmc {
		opts.debug.vmc = true;
	}
	if cli.debug.scene {
		opts.debug.scene = true;
	}
	if cli.debug.morph {
		opts.debug.morph = true;
	}
	if cli.disable_expression_morphs {
		opts.disable_expression_morphs = true;
	}
	if cli.disable_vmc_eye_look {
		opts.disable_vmc_eye_look = true;
	}
	// CLI で `--apply-vmc-root-translation` が指定されたら強制 ON。指定なしは
	// manifest 値（または既定値 false）をそのまま使う。OFF への切替は IPC 経由で行う。
	if cli.apply_vmc_root_translation {
		opts.apply_vmc_root_translation = true;
	}
	// `--unmotion-zenoh` 系: CLI が ON にしていれば強制 ON、key が default と異なれば差し替え。
	if cli.unmotion_zenoh.enabled {
		opts.unmotion_zenoh.enabled = true;
	}
	if cli.unmotion_zenoh.base_key_expr != default.unmotion_zenoh.base_key_expr {
		opts.unmotion_zenoh.base_key_expr = cli.unmotion_zenoh.base_key_expr;
	}
	if cli.primary_motion_source != default.primary_motion_source {
		opts.primary_motion_source = cli.primary_motion_source;
	}
	if cli.simple_basecolor_only {
		opts.simple_basecolor_only = true;
	}
	if cli.debug_material_dump {
		opts.debug_material_dump = true;
	}
	if cli.mesh_diagnostics.debug_bind_pose {
		opts.mesh_diagnostics.debug_bind_pose = true;
	}
	if cli.mesh_diagnostics.debug_primitive_colors {
		opts.mesh_diagnostics.debug_primitive_colors = true;
	}
	if cli.mesh_diagnostics.debug_zero_morphs {
		opts.mesh_diagnostics.debug_zero_morphs = true;
	}
	if cli.mesh_diagnostics.relax_iris_alpha {
		opts.mesh_diagnostics.relax_iris_alpha = true;
	}
	if cli.mesh_diagnostics.debug_skin_legacy_no_inv_mesh {
		opts.mesh_diagnostics.debug_skin_legacy_no_inv_mesh = true;
	}
	if cli.mesh_diagnostics.debug_disable_reflection {
		opts.mesh_diagnostics.debug_disable_reflection = true;
	}
	if cli.mesh_diagnostics.disable_fur {
		opts.mesh_diagnostics.disable_fur = true;
	}
	if cli.debug_disable_rim_lighting {
		opts.debug_disable_rim_lighting = true;
	}
	if cli.debug_force_shading_shift_zero {
		opts.debug_force_shading_shift_zero = true;
	}
	if cli.debug_disable_matcap {
		opts.debug_disable_matcap = true;
	}
	if cli.debug_disable_emissive {
		opts.debug_disable_emissive = true;
	}
	if cli.debug_disable_shade_color {
		opts.debug_disable_shade_color = true;
	}
	if cli.debug_disable_normal_map {
		opts.debug_disable_normal_map = true;
	}
	if cli.debug_base_texture_only {
		opts.debug_base_texture_only = true;
	}
}

fn vmc_addr_from_port(port: u16) -> SocketAddr {
	SocketAddr::from(([0, 0, 0, 0], port))
}

fn load_window_icon(path: &Path) -> Option<Icon> {
	let image = match image::open(path) {
		Ok(image) => image.into_rgba8(),
		Err(e) => {
			eprintln!("un-avatar-renderer: icon {}: {e}", path.display());
			return None;
		}
	};
	let (width, height) = image.dimensions();
	Icon::from_rgba(image.into_raw(), width, height)
		.map_err(|e| eprintln!("un-avatar-renderer: icon {}: {e}", path.display()))
		.ok()
}

#[cfg(test)]
mod tests {
	use std::{
		io::{BufRead, BufReader, Read, Write},
		net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream},
		thread,
		time::{Duration, Instant},
	};

	use super::{
		compact_window_title_status, parse_renderer_control_command, resolve_activate_action_from_menu_path, start_runtime_status_server,
		AvatarWindowOptions, CameraTransitionEasing, CameraTransitionMode, CloseHotkey, RendererControlCommand, WardrobeAssetUploadPlan,
		SCENE_STATE_SPLASH, WINDOW_TITLE_STATUS_MAX_CHARS,
	};
	use winit::keyboard::{Key, ModifiersState};

	fn reserve_runtime_status_address() -> SocketAddr {
		let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
		listener.local_addr().unwrap()
	}

	fn connect_runtime_status(address: SocketAddr) -> TcpStream {
		let started = Instant::now();
		loop {
			match TcpStream::connect(address) {
				Ok(stream) => return stream,
				Err(_error) if started.elapsed() < Duration::from_secs(1) => {
					thread::sleep(Duration::from_millis(10));
				}
				Err(error) => panic!("connect runtime status {address}: {error}"),
			}
		}
	}

	#[test]
	fn compact_window_title_status_collapses_whitespace_and_truncates() {
		assert_eq!(
			compact_window_title_status("  Loading\tmodel\n textures  "),
			"Loading model textures"
		);

		let long = "x".repeat(WINDOW_TITLE_STATUS_MAX_CHARS + 8);
		let compact = compact_window_title_status(long);
		assert_eq!(compact.chars().count(), WINDOW_TITLE_STATUS_MAX_CHARS);
		assert!(compact.ends_with('…'));
	}

	#[test]
	fn runtime_status_server_keeps_one_shot_compatibility() {
		let address = reserve_runtime_status_address();
		let opts = AvatarWindowOptions {
			wardrobe_set: Some(" field_drape ".to_string()),
			..Default::default()
		};
		let status = start_runtime_status_server(address, &opts);
		{
			let mut status = status.lock().unwrap();
			status.wardrobe_asset_upload = WardrobeAssetUploadPlan {
				mode: "draw-scoped-resource-scoped".to_string(),
				total_draw_mesh_primitive_count: 3,
				resident_draw_mesh_primitive_count: 2,
				inactive_draw_mesh_primitive_count: 1,
				total_draw_mesh_buffer_bytes: 3000,
				resident_draw_mesh_buffer_bytes: 2000,
				inactive_draw_mesh_buffer_bytes: 1000,
				total_image_texture_count: 4,
				resident_image_texture_count: 3,
				inactive_image_texture_count: 1,
				draws_using_inactive_image_texture_count: 2,
				active_draws_using_inactive_image_texture_count: 1,
				inactive_image_textures_used_by_active_draw_count: 1,
				inactive_image_textures_used_by_active_draw: vec![3],
				total_material_slot_count: 5,
				resident_material_slot_count: 4,
				inactive_material_slot_count: 1,
				active_draws_using_inactive_material_slot_count: 1,
				inactive_material_slots_used_by_active_draw_count: 1,
				inactive_material_slots_used_by_active_draw: vec![4],
				pending_image_texture_upload_count: 1,
				pending_material_slot_upload_count: 1,
				last_mesh_buffer_scoped_load_count: 1,
				last_mesh_buffer_scoped_unload_count: 2,
				scoped_draw_supported: true,
				scoped_upload_supported: true,
				all_resident: false,
				active_residency_gaps_detected: true,
				residency_gap_index_status_limit: 64,
				..Default::default()
			};
			status.runtime_parameter_definitions = vec![un_avatar_core::UnaRuntimeParameterDefinition {
				name: "Outfit".to_string(),
				owner_keys: vec!["action:wardrobe:field_drape".to_string()],
				source_kinds: vec!["action_condition".to_string(), "action_trigger".to_string()],
				value_samples: vec![1.0],
				current_value: Some(1.0),
				transient: false,
				..Default::default()
			}];
			status.runtime_parameter_conflicts = vec![un_avatar_core::UnaRuntimeParameterConflict {
				name: "ContactHand".to_string(),
				reason: "contact_transient_overlaps_action_parameter".to_string(),
				owner_keys: vec!["action:contact-react".to_string(), "contact:hand".to_string()],
				source_kinds: vec!["action_condition".to_string(), "contact_receiver".to_string()],
				value_samples: vec![0.0, 1.0],
			}];
			status.wardrobe_actions = vec![crate::gpu::RuntimeWardrobeActionStatus {
				action_id: "wardrobe:field_drape".to_string(),
				label: "Field Drape".to_string(),
				set_id: "field_drape".to_string(),
				expression_menu_path: Some("Wardrobe/Field Drape".to_string()),
				supervisor_command: Some("field_drape".to_string()),
				parameter_name: Some("Outfit".to_string()),
				parameter_value: Some(1.0),
			}];
			status.runtime_actions = vec![crate::gpu::RuntimeActionStatus {
				action_id: "wardrobe:field_drape".to_string(),
				label: "Field Drape".to_string(),
				effect_count: 5,
				expression_menu_path: Some("Wardrobe/Field Drape".to_string()),
				supervisor_command: Some("field_drape".to_string()),
				parameter_name: Some("Outfit".to_string()),
				parameter_value: Some(1.0),
				condition_parameter_names: vec!["Outfit".to_string()],
				current_condition_state: Some("active".to_string()),
				wardrobe_set_id: Some("field_drape".to_string()),
				target_writes: vec![un_avatar_core::UnaEvaluationRuntimeActionTargetWrite {
					owner_key: "action:wardrobe:field_drape".to_string(),
					action_id: "wardrobe:field_drape".to_string(),
					effect_kind: "node_visibility".to_string(),
					target_kind: un_avatar_core::UnaEvaluationTargetKind::NodeVisibility,
					target_key: "Avatar/Coat".to_string(),
				}],
				node_visibility_effects: vec![crate::gpu::RuntimeActionNodeVisibilityEffectStatus {
					node_index: Some(3),
					path: Some("Avatar/Coat".to_string()),
					visible: true,
					..Default::default()
				}],
				material_property_effects: vec![crate::gpu::RuntimeActionMaterialPropertyEffectStatus {
					property_kind: "color".to_string(),
					material_index: Some(2),
					material_name: Some("Coat".to_string()),
					parameter: "_Color".to_string(),
					color_value: Some([1.0, 0.5, 0.25, 1.0]),
					..Default::default()
				}],
				material_slot_effects: vec![crate::gpu::RuntimeActionMaterialSlotEffectStatus {
					node_index: Some(3),
					path: Some("Avatar/Coat".to_string()),
					primitive_index: Some(0),
					material_index: Some(2),
					material_name: Some("Coat".to_string()),
					..Default::default()
				}],
				expression_weight_effects: vec![crate::gpu::RuntimeActionExpressionWeightEffectStatus {
					name: "Smile".to_string(),
					weight: 0.75,
				}],
				dynamics_enabled_effects: vec![crate::gpu::RuntimeActionDynamicsEnabledEffectStatus {
					source_id: "physbone:hair".to_string(),
					enabled: true,
				}],
				effect_kinds: [
					("node_visibility".to_string(), 1),
					("expression_weight".to_string(), 1),
					("material_color".to_string(), 1),
					("material_slot".to_string(), 1),
					("dynamics_enabled".to_string(), 1),
				]
				.into_iter()
				.collect(),
			}];
			status.runtime_action_target_write_collisions = vec![un_avatar_core::UnaEvaluationTargetWriteCollision {
				target_kind: un_avatar_core::UnaEvaluationTargetKind::NodeVisibility,
				target_key: "Avatar/Coat".to_string(),
				owner_keys: vec!["action:wardrobe:field_drape".to_string(), "action:wardrobe:coat_off".to_string()],
				action_ids: vec!["wardrobe:field_drape".to_string(), "wardrobe:coat_off".to_string()],
				writes: Vec::new(),
			}];
			status.runtime_action_restore_readiness = vec![un_avatar_core::UnaEvaluationRestoreReadiness {
				owner_key: "action:wardrobe:field_drape".to_string(),
				action_id: "wardrobe:field_drape".to_string(),
				effect_kind: "node_visibility".to_string(),
				target_kind: un_avatar_core::UnaEvaluationTargetKind::NodeVisibility,
				target_key: "Avatar/Coat".to_string(),
				restore_target: true,
				current_value_available: true,
				current_value: Some(serde_json::Value::from(true)),
				baseline_required: true,
				ready: false,
				reason: "baseline_not_captured".to_string(),
			}];
			status.runtime_action_restore_baseline_candidates = vec![un_avatar_core::UnaEvaluationRestoreBaselineCandidate {
				owner_key: "action:wardrobe:field_drape".to_string(),
				action_id: "wardrobe:field_drape".to_string(),
				effect_kind: "node_visibility".to_string(),
				target_kind: un_avatar_core::UnaEvaluationTargetKind::NodeVisibility,
				target_key: "Avatar/Coat".to_string(),
				baseline_value: serde_json::Value::from(true),
			}];
			status.runtime_action_restore_baseline_capture_plan = vec![un_avatar_core::UnaEvaluationRestoreBaselineEntry {
				owner_key: "action:wardrobe:field_drape".to_string(),
				target_kind: un_avatar_core::UnaEvaluationTargetKind::NodeVisibility,
				target_key: "Avatar/Coat".to_string(),
				baseline_value: serde_json::Value::from(true),
				source_action_ids: vec!["wardrobe:field_drape".to_string()],
				source_effect_kinds: vec!["node_visibility".to_string()],
			}];
			status.runtime_action_restore_apply_plan = vec![un_avatar_core::UnaEvaluationRestoreApplyEntry {
				owner_key: "action:wardrobe:field_drape".to_string(),
				action_id: "wardrobe:field_drape".to_string(),
				condition_state: Some("inactive".to_string()),
				target_kind: un_avatar_core::UnaEvaluationTargetKind::NodeVisibility,
				target_key: "Avatar/Coat".to_string(),
				baseline_value: Some(serde_json::Value::from(true)),
				current_value_available: true,
				current_value: Some(serde_json::Value::from(false)),
				ready: true,
				reason: "ready".to_string(),
			}];
			status.menu_action_candidates = vec![crate::gpu::RuntimeMenuActionCandidateStatus {
				menu_component_index: 2,
				menu_key: "component:2".to_string(),
				menu_label: Some("Wardrobe".to_string()),
				parameter_name: "Outfit".to_string(),
				parameter_value: 1.0,
				action_id: "wardrobe:field_drape".to_string(),
				action_label: "Field Drape".to_string(),
				match_kind: "trigger".to_string(),
				inverted: false,
				effect_count: 4,
				effect_kinds: [
					("node_visibility".to_string(), 1),
					("expression_weight".to_string(), 2),
					("material_color".to_string(), 1),
					("material_scalar".to_string(), 1),
				]
				.into_iter()
				.collect(),
				wardrobe_set_ids: vec!["field_drape".to_string()],
			}];
			status.menu_wardrobe_candidates = vec![crate::gpu::RuntimeMenuWardrobeCandidateStatus {
				menu_component_index: 2,
				menu_key: "component:2".to_string(),
				menu_path: vec!["Wardrobe".to_string()],
				menu_path_truncated: false,
				menu_label: Some("Wardrobe".to_string()),
				action_id: "wardrobe:field_drape".to_string(),
				wardrobe_set_id: "field_drape".to_string(),
				match_kind: "trigger".to_string(),
				inverted: false,
			}];
			status.contact_parameter_declarations = vec![crate::gpu::RuntimeContactParameterDeclarationStatus {
				owner_key: "contact:hand".to_string(),
				source_id: "contact:hand".to_string(),
				node: 1,
				node_path: Some("root/receiver".to_string()),
				parameter: "ContactHand".to_string(),
				collision_tags: vec!["Hand".to_string(), "Interact".to_string()],
			}];
			status.contact_parameter_emission_enabled = true;
			status.contact_parameter_emissions = vec![crate::gpu::RuntimeContactParameterEmissionStatus {
				owner_key: "contact:hand".to_string(),
				source_id: "contact:hand".to_string(),
				receiver_index: 0,
				receiver_node: 1,
				receiver_node_path: Some("root/receiver".to_string()),
				parameter: "ContactHand".to_string(),
				value: 1.0,
				emitted: true,
				sender_source_ids: vec!["contact:sender".to_string()],
			}];
			status.contact_probes = vec![crate::gpu::RuntimeContactProbeStatus {
				index: 0,
				receiver_index: 0,
				sender_index: 1,
				receiver_source_id: "contact:hand".to_string(),
				sender_source_id: "contact:sender".to_string(),
				receiver_node: 1,
				receiver_node_path: Some("root/receiver".to_string()),
				sender_node: 2,
				sender_node_path: Some("root/sender".to_string()),
				parameter: "ContactHand".to_string(),
				matched_tags: vec!["Hand".to_string()],
				tag_match: true,
				overlap: true,
				would_emit: true,
				distance: 0.07,
				threshold: 0.09,
				receiver_radius: 0.05,
				sender_radius: 0.04,
				receiver_shape: un_avatar_core::UnaDynamicsColliderShape::Sphere,
				sender_shape: un_avatar_core::UnaDynamicsColliderShape::Sphere,
				approximation: "sphere".to_string(),
			}];
			status.dynamics_contact_count = 2;
			status.dynamics_vrc_contact_sender_count = 1;
			status.dynamics_vrc_contact_receiver_count = 1;
			status.dynamics_collider_count = 1;
			status.dynamics_vrc_physbone_collider_count = 1;
			status.dynamics_contact_parameter_declaration_count = 1;
			status.dynamics_contact_probe_count = 1;
			status.dynamics_contact_probe_would_emit_count = 1;
			status.dynamics_contact_parameter_emission_count = 1;
			status.dynamics_contact_parameter_emitted_count = 1;
			status.dynamics_contact_parameter_reset_to_zero_count = 0;
			status.dynamics_groups = vec![crate::gpu::RuntimeDynamicsGroupStatus {
				index: 0,
				source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
				authored_enabled: false,
				effective_enabled: true,
				source_id: "physbone:hair".to_string(),
				comment: "Hair".to_string(),
				category: "secondary".to_string(),
				bone_count: 3,
				root_node: Some(1),
				root_path: Some("root/hair".to_string()),
				tip_node: Some(3),
				tip_path: Some("root/hair/tip".to_string()),
				stiffness: 0.35,
				drag_force: 0.2,
				gravity_power: 0.1,
				gravity_dir: [0.0, -1.0, 0.0],
				hit_radius: 0.04,
				center_node: Some(0),
				center_path: Some("root".to_string()),
				limit_type: Some("Angle".to_string()),
				max_angle_x: Some(45.0),
				max_angle_z: Some(30.0),
				max_stretch: Some(0.0),
				allow_grabbing: Some(true),
				allow_posing: Some(false),
			}];
			status.dynamics_colliders = vec![crate::gpu::RuntimeDynamicsColliderStatus {
				index: 0,
				source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
				node: 5,
				node_path: Some("root/collider".to_string()),
				shape: un_avatar_core::UnaDynamicsColliderShape::Capsule,
				radius: 0.08,
				height: 0.24,
				position: [0.0, 0.1, 0.0],
				rotation: [0.0, 0.0, 0.0, 1.0],
				inside_bounds: true,
			}];
			status.dynamics_constraint_refs = vec![crate::gpu::RuntimeDynamicsConstraintRefStatus {
				index: 0,
				source_kind: un_avatar_core::UnaDynamicsSourceKind::VrcPhysBone,
				source_id: "constraint:parent".to_string(),
				target_node: 4,
				target_path: Some("root/target".to_string()),
				source_nodes: vec![1, 2],
				source_paths: vec!["root/source-a".to_string(), "root/source-b".to_string()],
				constraint_type: "parent".to_string(),
				weight: 0.75,
			}];
			status.dynamics_constraint_ref_count = 3;
			status.dynamics_vrc_constraint_ref_count = 2;
			status.dynamics_limit_group_count = 4;
			status.dynamics_angle_limit_group_count = 3;
			status.dynamics_stretch_limit_group_count = 1;
			status.dynamics_grabbing_enabled_group_count = 2;
			status.dynamics_posing_enabled_group_count = 1;
		}
		let mut stream = connect_runtime_status(address);
		let mut text = String::new();
		stream.read_to_string(&mut text).unwrap();

		let snapshot: serde_json::Value = serde_json::from_str(&text).unwrap();
		assert_eq!(snapshot.get("connected").and_then(|value| value.as_bool()), Some(true));
		assert_eq!(snapshot.get("protocol").and_then(|value| value.as_str()), Some("local-tcp-json-v2"));
		assert_eq!(
			snapshot.get("scene_state").and_then(|value| value.as_str()),
			Some(SCENE_STATE_SPLASH)
		);
		assert_eq!(
			snapshot.get("active_wardrobe_set").and_then(|value| value.as_str()),
			Some("field_drape")
		);
		assert_eq!(snapshot.get("dynamics_contact_count").and_then(|value| value.as_u64()), Some(2));
		assert_eq!(
			snapshot.get("dynamics_vrc_contact_sender_count").and_then(|value| value.as_u64()),
			Some(1)
		);
		assert_eq!(
			snapshot.get("dynamics_vrc_contact_receiver_count").and_then(|value| value.as_u64()),
			Some(1)
		);
		assert_eq!(
			snapshot
				.get("dynamics_contact_parameter_declaration_count")
				.and_then(|value| value.as_u64()),
			Some(1)
		);
		let declarations = snapshot
			.get("contact_parameter_declarations")
			.and_then(|value| value.as_array())
			.expect("contact parameter declarations");
		assert_eq!(declarations.len(), 1);
		assert_eq!(
			declarations[0].get("owner_key").and_then(|value| value.as_str()),
			Some("contact:hand")
		);
		assert_eq!(
			declarations[0].get("parameter").and_then(|value| value.as_str()),
			Some("ContactHand")
		);
		assert_eq!(
			declarations[0].get("node_path").and_then(|value| value.as_str()),
			Some("root/receiver")
		);
		assert_eq!(
			snapshot.get("contact_parameter_emission_enabled").and_then(|value| value.as_bool()),
			Some(true)
		);
		assert_eq!(
			snapshot
				.get("dynamics_contact_parameter_emission_count")
				.and_then(|value| value.as_u64()),
			Some(1)
		);
		assert_eq!(
			snapshot
				.get("dynamics_contact_parameter_emitted_count")
				.and_then(|value| value.as_u64()),
			Some(1)
		);
		let emissions = snapshot
			.get("contact_parameter_emissions")
			.and_then(|value| value.as_array())
			.expect("contact parameter emissions");
		assert_eq!(emissions.len(), 1);
		assert_eq!(emissions[0].get("parameter").and_then(|value| value.as_str()), Some("ContactHand"));
		assert_eq!(emissions[0].get("value").and_then(|value| value.as_f64()), Some(1.0));
		assert_eq!(emissions[0].get("emitted").and_then(|value| value.as_bool()), Some(true));
		assert_eq!(
			snapshot.get("dynamics_contact_probe_count").and_then(|value| value.as_u64()),
			Some(1)
		);
		assert_eq!(
			snapshot
				.get("dynamics_contact_probe_would_emit_count")
				.and_then(|value| value.as_u64()),
			Some(1)
		);
		let dynamics_groups = snapshot
			.get("dynamics_groups")
			.and_then(|value| value.as_array())
			.expect("dynamics groups");
		assert_eq!(dynamics_groups.len(), 1);
		assert_eq!(
			dynamics_groups[0].get("source_id").and_then(|value| value.as_str()),
			Some("physbone:hair")
		);
		assert_eq!(
			dynamics_groups[0].get("root_path").and_then(|value| value.as_str()),
			Some("root/hair")
		);
		assert_eq!(
			dynamics_groups[0].get("tip_path").and_then(|value| value.as_str()),
			Some("root/hair/tip")
		);
		assert_eq!(
			dynamics_groups[0].get("effective_enabled").and_then(|value| value.as_bool()),
			Some(true)
		);
		assert_eq!(
			dynamics_groups[0].get("allow_grabbing").and_then(|value| value.as_bool()),
			Some(true)
		);
		let dynamics_colliders = snapshot
			.get("dynamics_colliders")
			.and_then(|value| value.as_array())
			.expect("dynamics colliders");
		assert_eq!(dynamics_colliders.len(), 1);
		assert_eq!(
			dynamics_colliders[0].get("node_path").and_then(|value| value.as_str()),
			Some("root/collider")
		);
		assert_eq!(dynamics_colliders[0].get("shape").and_then(|value| value.as_str()), Some("capsule"));
		assert_eq!(
			dynamics_colliders[0].get("inside_bounds").and_then(|value| value.as_bool()),
			Some(true)
		);
		let probes = snapshot
			.get("contact_probes")
			.and_then(|value| value.as_array())
			.expect("contact probes");
		assert_eq!(probes.len(), 1);
		assert_eq!(probes[0].get("parameter").and_then(|value| value.as_str()), Some("ContactHand"));
		assert_eq!(
			probes[0].get("receiver_node_path").and_then(|value| value.as_str()),
			Some("root/receiver")
		);
		assert_eq!(
			probes[0].get("sender_node_path").and_then(|value| value.as_str()),
			Some("root/sender")
		);
		assert_eq!(probes[0].get("would_emit").and_then(|value| value.as_bool()), Some(true));
		assert_eq!(
			snapshot.get("dynamics_constraint_ref_count").and_then(|value| value.as_u64()),
			Some(3)
		);
		let constraint_refs = snapshot
			.get("dynamics_constraint_refs")
			.and_then(|value| value.as_array())
			.expect("dynamics constraint refs");
		assert_eq!(constraint_refs.len(), 1);
		assert_eq!(
			constraint_refs[0].get("source_id").and_then(|value| value.as_str()),
			Some("constraint:parent")
		);
		assert_eq!(
			constraint_refs[0].get("constraint_type").and_then(|value| value.as_str()),
			Some("parent")
		);
		assert_eq!(
			constraint_refs[0].get("target_path").and_then(|value| value.as_str()),
			Some("root/target")
		);
		assert_eq!(
			snapshot.get("dynamics_vrc_constraint_ref_count").and_then(|value| value.as_u64()),
			Some(2)
		);
		assert_eq!(snapshot.get("dynamics_limit_group_count").and_then(|value| value.as_u64()), Some(4));
		assert_eq!(
			snapshot.get("dynamics_angle_limit_group_count").and_then(|value| value.as_u64()),
			Some(3)
		);
		assert_eq!(
			snapshot.get("dynamics_stretch_limit_group_count").and_then(|value| value.as_u64()),
			Some(1)
		);
		assert_eq!(
			snapshot
				.get("dynamics_grabbing_enabled_group_count")
				.and_then(|value| value.as_u64()),
			Some(2)
		);
		assert_eq!(
			snapshot.get("dynamics_posing_enabled_group_count").and_then(|value| value.as_u64()),
			Some(1)
		);
		assert!(snapshot.get("active_asset_groups").is_none());
		let upload = snapshot.get("wardrobe_asset_upload").expect("wardrobe asset upload status");
		assert_eq!(
			upload.get("mode").and_then(|value| value.as_str()),
			Some("draw-scoped-resource-scoped")
		);
		assert_eq!(
			upload.get("total_draw_mesh_primitive_count").and_then(|value| value.as_u64()),
			Some(3)
		);
		assert_eq!(
			upload.get("resident_draw_mesh_primitive_count").and_then(|value| value.as_u64()),
			Some(2)
		);
		assert_eq!(
			upload.get("inactive_draw_mesh_primitive_count").and_then(|value| value.as_u64()),
			Some(1)
		);
		assert_eq!(
			upload.get("total_draw_mesh_buffer_bytes").and_then(|value| value.as_u64()),
			Some(3000)
		);
		assert_eq!(
			upload.get("resident_draw_mesh_buffer_bytes").and_then(|value| value.as_u64()),
			Some(2000)
		);
		assert_eq!(
			upload.get("inactive_draw_mesh_buffer_bytes").and_then(|value| value.as_u64()),
			Some(1000)
		);
		assert_eq!(upload.get("total_image_texture_count").and_then(|value| value.as_u64()), Some(4));
		assert_eq!(upload.get("resident_image_texture_count").and_then(|value| value.as_u64()), Some(3));
		assert_eq!(upload.get("inactive_image_texture_count").and_then(|value| value.as_u64()), Some(1));
		assert_eq!(
			upload
				.get("draws_using_inactive_image_texture_count")
				.and_then(|value| value.as_u64()),
			Some(2)
		);
		assert_eq!(
			upload
				.get("active_draws_using_inactive_image_texture_count")
				.and_then(|value| value.as_u64()),
			Some(1)
		);
		assert_eq!(
			upload
				.get("inactive_image_textures_used_by_active_draw_count")
				.and_then(|value| value.as_u64()),
			Some(1)
		);
		assert_eq!(
			upload
				.get("inactive_image_textures_used_by_active_draw")
				.and_then(|value| value.as_array())
				.map(|values| values.iter().filter_map(|value| value.as_u64()).collect::<Vec<_>>()),
			Some(vec![3])
		);
		assert_eq!(upload.get("total_material_slot_count").and_then(|value| value.as_u64()), Some(5));
		assert_eq!(upload.get("resident_material_slot_count").and_then(|value| value.as_u64()), Some(4));
		assert_eq!(upload.get("inactive_material_slot_count").and_then(|value| value.as_u64()), Some(1));
		assert_eq!(
			upload
				.get("active_draws_using_inactive_material_slot_count")
				.and_then(|value| value.as_u64()),
			Some(1)
		);
		assert_eq!(
			upload
				.get("inactive_material_slots_used_by_active_draw_count")
				.and_then(|value| value.as_u64()),
			Some(1)
		);
		assert_eq!(
			upload
				.get("inactive_material_slots_used_by_active_draw")
				.and_then(|value| value.as_array())
				.map(|values| values.iter().filter_map(|value| value.as_u64()).collect::<Vec<_>>()),
			Some(vec![4])
		);
		assert_eq!(
			upload.get("pending_image_texture_upload_count").and_then(|value| value.as_u64()),
			Some(1)
		);
		assert_eq!(
			upload.get("pending_material_slot_upload_count").and_then(|value| value.as_u64()),
			Some(1)
		);
		assert_eq!(
			upload.get("last_mesh_buffer_scoped_load_count").and_then(|value| value.as_u64()),
			Some(1)
		);
		assert_eq!(
			upload.get("last_mesh_buffer_scoped_unload_count").and_then(|value| value.as_u64()),
			Some(2)
		);
		assert_eq!(upload.get("scoped_draw_supported").and_then(|value| value.as_bool()), Some(true));
		assert_eq!(upload.get("scoped_upload_supported").and_then(|value| value.as_bool()), Some(true));
		assert_eq!(upload.get("all_resident").and_then(|value| value.as_bool()), Some(false));
		assert_eq!(
			upload.get("active_residency_gaps_detected").and_then(|value| value.as_bool()),
			Some(true)
		);
		assert_eq!(
			upload.get("residency_gap_index_status_limit").and_then(|value| value.as_u64()),
			Some(64)
		);
		let parameter_definitions = snapshot
			.get("runtime_parameter_definitions")
			.and_then(|value| value.as_array())
			.expect("runtime parameter definitions");
		assert_eq!(parameter_definitions.len(), 1);
		assert_eq!(
			parameter_definitions[0].get("name").and_then(|value| value.as_str()),
			Some("Outfit")
		);
		let parameter_conflicts = snapshot
			.get("runtime_parameter_conflicts")
			.and_then(|value| value.as_array())
			.expect("runtime parameter conflicts");
		assert_eq!(parameter_conflicts.len(), 1);
		assert_eq!(
			parameter_conflicts[0].get("reason").and_then(|value| value.as_str()),
			Some("contact_transient_overlaps_action_parameter")
		);
		let wardrobe_actions = snapshot
			.get("wardrobe_actions")
			.and_then(|value| value.as_array())
			.expect("wardrobe actions");
		assert_eq!(wardrobe_actions.len(), 1);
		assert_eq!(
			wardrobe_actions[0].get("action_id").and_then(|value| value.as_str()),
			Some("wardrobe:field_drape")
		);
		assert_eq!(
			wardrobe_actions[0].get("label").and_then(|value| value.as_str()),
			Some("Field Drape")
		);
		assert_eq!(
			wardrobe_actions[0].get("set_id").and_then(|value| value.as_str()),
			Some("field_drape")
		);
		assert_eq!(
			wardrobe_actions[0].get("expression_menu_path").and_then(|value| value.as_str()),
			Some("Wardrobe/Field Drape")
		);
		assert_eq!(
			wardrobe_actions[0].get("parameter_name").and_then(|value| value.as_str()),
			Some("Outfit")
		);
		assert_eq!(
			wardrobe_actions[0].get("parameter_value").and_then(|value| value.as_f64()),
			Some(1.0)
		);
		let runtime_actions = snapshot
			.get("runtime_actions")
			.and_then(|value| value.as_array())
			.expect("runtime actions");
		assert_eq!(runtime_actions.len(), 1);
		assert_eq!(
			runtime_actions[0].get("action_id").and_then(|value| value.as_str()),
			Some("wardrobe:field_drape")
		);
		assert_eq!(
			runtime_actions[0].get("label").and_then(|value| value.as_str()),
			Some("Field Drape")
		);
		assert_eq!(
			runtime_actions[0].get("wardrobe_set_id").and_then(|value| value.as_str()),
			Some("field_drape")
		);
		assert_eq!(runtime_actions[0].get("effect_count").and_then(|value| value.as_u64()), Some(5));
		assert_eq!(
			runtime_actions[0]
				.get("condition_parameter_names")
				.and_then(|value| value.as_array())
				.and_then(|values| values.first())
				.and_then(|value| value.as_str()),
			Some("Outfit")
		);
		assert_eq!(
			runtime_actions[0].get("current_condition_state").and_then(|value| value.as_str()),
			Some("active")
		);
		assert_eq!(
			runtime_actions[0]
				.get("target_writes")
				.and_then(|value| value.as_array())
				.and_then(|values| values.first())
				.and_then(|value| value.get("owner_key"))
				.and_then(|value| value.as_str()),
			Some("action:wardrobe:field_drape")
		);
		assert_eq!(
			runtime_actions[0]
				.get("node_visibility_effects")
				.and_then(|value| value.as_array())
				.and_then(|values| values.first())
				.and_then(|value| value.get("path"))
				.and_then(|value| value.as_str()),
			Some("Avatar/Coat")
		);
		assert_eq!(
			runtime_actions[0]
				.get("material_property_effects")
				.and_then(|value| value.as_array())
				.and_then(|values| values.first())
				.and_then(|value| value.get("parameter"))
				.and_then(|value| value.as_str()),
			Some("_Color")
		);
		assert_eq!(
			runtime_actions[0]
				.get("material_slot_effects")
				.and_then(|value| value.as_array())
				.and_then(|values| values.first())
				.and_then(|value| value.get("material_name"))
				.and_then(|value| value.as_str()),
			Some("Coat")
		);
		assert_eq!(
			runtime_actions[0]
				.get("expression_weight_effects")
				.and_then(|value| value.as_array())
				.and_then(|values| values.first())
				.and_then(|value| value.get("name"))
				.and_then(|value| value.as_str()),
			Some("Smile")
		);
		assert_eq!(
			runtime_actions[0]
				.get("dynamics_enabled_effects")
				.and_then(|value| value.as_array())
				.and_then(|values| values.first())
				.and_then(|value| value.get("source_id"))
				.and_then(|value| value.as_str()),
			Some("physbone:hair")
		);
		assert_eq!(
			runtime_actions[0]
				.get("effect_kinds")
				.and_then(|value| value.get("node_visibility"))
				.and_then(|value| value.as_u64()),
			Some(1)
		);
		let action_collisions = snapshot
			.get("runtime_action_target_write_collisions")
			.and_then(|value| value.as_array())
			.expect("runtime action target write collisions");
		assert_eq!(action_collisions.len(), 1);
		assert_eq!(
			action_collisions[0].get("target_key").and_then(|value| value.as_str()),
			Some("Avatar/Coat")
		);
		let restore_readiness = snapshot
			.get("runtime_action_restore_readiness")
			.and_then(|value| value.as_array())
			.expect("runtime action restore readiness");
		assert_eq!(restore_readiness.len(), 1);
		assert_eq!(
			restore_readiness[0].get("reason").and_then(|value| value.as_str()),
			Some("baseline_not_captured")
		);
		let baseline_candidates = snapshot
			.get("runtime_action_restore_baseline_candidates")
			.and_then(|value| value.as_array())
			.expect("runtime action restore baseline candidates");
		assert_eq!(baseline_candidates.len(), 1);
		assert_eq!(
			baseline_candidates[0].get("baseline_value").and_then(|value| value.as_bool()),
			Some(true)
		);
		let capture_plan = snapshot
			.get("runtime_action_restore_baseline_capture_plan")
			.and_then(|value| value.as_array())
			.expect("runtime action restore baseline capture plan");
		assert_eq!(capture_plan.len(), 1);
		assert_eq!(
			capture_plan[0].get("target_key").and_then(|value| value.as_str()),
			Some("Avatar/Coat")
		);
		let apply_plan = snapshot
			.get("runtime_action_restore_apply_plan")
			.and_then(|value| value.as_array())
			.expect("runtime action restore apply plan");
		assert_eq!(apply_plan.len(), 1);
		assert_eq!(apply_plan[0].get("ready").and_then(|value| value.as_bool()), Some(true));
		let menu_action_candidates = snapshot
			.get("menu_action_candidates")
			.and_then(|value| value.as_array())
			.expect("menu action candidates");
		assert_eq!(menu_action_candidates.len(), 1);
		assert_eq!(
			menu_action_candidates[0]
				.get("menu_component_index")
				.and_then(|value| value.as_u64()),
			Some(2)
		);
		assert_eq!(
			menu_action_candidates[0].get("parameter_name").and_then(|value| value.as_str()),
			Some("Outfit")
		);
		assert_eq!(
			menu_action_candidates[0]
				.get("wardrobe_set_ids")
				.and_then(|value| value.as_array())
				.map(|values| values.iter().filter_map(|value| value.as_str()).collect::<Vec<_>>()),
			Some(vec!["field_drape"])
		);
		let menu_wardrobe_candidates = snapshot
			.get("menu_wardrobe_candidates")
			.and_then(|value| value.as_array())
			.expect("menu wardrobe candidates");
		assert_eq!(menu_wardrobe_candidates.len(), 1);
		assert_eq!(
			menu_wardrobe_candidates[0].get("wardrobe_set_id").and_then(|value| value.as_str()),
			Some("field_drape")
		);
		assert!(snapshot.get("resolver_cache_key").is_none());
		assert!(snapshot.get("last_action_id").is_some_and(|value| value.is_null()));
		assert!(snapshot
			.get("control_capabilities")
			.and_then(|value| value.as_array())
			.is_some_and(|capabilities| capabilities.iter().any(|value| value.as_str() == Some("scene_state"))));
		assert!(snapshot
			.get("control_capabilities")
			.and_then(|value| value.as_array())
			.is_some_and(|capabilities| capabilities.iter().any(|value| value.as_str() == Some("set_wardrobe"))));
		assert!(snapshot
			.get("control_capabilities")
			.and_then(|value| value.as_array())
			.is_some_and(|capabilities| capabilities.iter().any(|value| value.as_str() == Some("set_parameter"))));
	}

	#[test]
	fn runtime_status_server_streams_newline_json() {
		let address = reserve_runtime_status_address();
		let _status = start_runtime_status_server(address, &AvatarWindowOptions::default());
		let mut stream = connect_runtime_status(address);
		stream.write_all(b"stream\n").unwrap();
		let mut reader = BufReader::new(stream);
		let mut first = String::new();
		let mut second = String::new();
		reader.read_line(&mut first).unwrap();
		reader.read_line(&mut second).unwrap();

		let first_snapshot: serde_json::Value = serde_json::from_str(&first).unwrap();
		let second_snapshot: serde_json::Value = serde_json::from_str(&second).unwrap();
		assert_eq!(
			first_snapshot.get("protocol").and_then(|value| value.as_str()),
			Some("local-tcp-json-v2")
		);
		assert_eq!(
			second_snapshot.get("protocol").and_then(|value| value.as_str()),
			Some("local-tcp-json-v2")
		);
	}

	#[test]
	fn parses_legacy_shutdown_control_command() {
		assert!(matches!(
			parse_renderer_control_command("shutdown").unwrap(),
			RendererControlCommand::Shutdown
		));
	}

	#[test]
	fn parses_json_reset_camera_control_command() {
		assert!(matches!(
			parse_renderer_control_command(r#"{"command":"reset_camera"}"#).unwrap(),
			RendererControlCommand::ResetCamera
		));
	}

	#[test]
	fn parses_json_set_wardrobe_control_command() {
		let command = parse_renderer_control_command(r#"{"command":"set_wardrobe","set_id":"field_drape"}"#).unwrap();
		let RendererControlCommand::SetWardrobe { set_id } = command else {
			panic!("expected set_wardrobe command");
		};
		assert_eq!(set_id, "field_drape");
	}

	#[test]
	fn parses_json_activate_action_control_command() {
		let command = parse_renderer_control_command(r#"{"command":"activate_action","action_id":"wardrobe:field_drape"}"#).unwrap();
		let RendererControlCommand::ActivateAction {
			action_id,
			supervisor_command,
			expression_menu_path,
			menu_path,
			wardrobe_set_id,
			parameter_name,
			parameter_value,
		} = command
		else {
			panic!("expected activate_action command");
		};
		assert_eq!(action_id.as_deref(), Some("wardrobe:field_drape"));
		assert_eq!(supervisor_command, None);
		assert_eq!(expression_menu_path, None);
		assert_eq!(menu_path, None);
		assert_eq!(wardrobe_set_id, None);
		assert_eq!(parameter_name, None);
		assert_eq!(parameter_value, None);
	}

	#[test]
	fn parses_json_activate_action_control_command_by_expression_menu_path() {
		let command =
			parse_renderer_control_command(r#"{"command":"activate_action","expressionMenuPath":"Wardrobe/Field Drape"}"#).unwrap();
		let RendererControlCommand::ActivateAction {
			action_id,
			supervisor_command,
			expression_menu_path,
			menu_path,
			wardrobe_set_id,
			parameter_name,
			parameter_value,
		} = command
		else {
			panic!("expected activate_action command");
		};
		assert_eq!(action_id, None);
		assert_eq!(supervisor_command, None);
		assert_eq!(expression_menu_path.as_deref(), Some("Wardrobe/Field Drape"));
		assert_eq!(menu_path, None);
		assert_eq!(wardrobe_set_id, None);
		assert_eq!(parameter_name, None);
		assert_eq!(parameter_value, None);
	}

	#[test]
	fn parses_json_activate_action_control_command_by_parameter_value() {
		let command =
			parse_renderer_control_command(r#"{"command":"activate_action","parameterName":"JacketColor","parameterValue":1.0}"#).unwrap();
		let RendererControlCommand::ActivateAction {
			action_id,
			supervisor_command,
			expression_menu_path,
			menu_path,
			wardrobe_set_id,
			parameter_name,
			parameter_value,
		} = command
		else {
			panic!("expected activate_action command");
		};
		assert_eq!(action_id, None);
		assert_eq!(supervisor_command, None);
		assert_eq!(expression_menu_path, None);
		assert_eq!(menu_path, None);
		assert_eq!(wardrobe_set_id, None);
		assert_eq!(parameter_name.as_deref(), Some("JacketColor"));
		assert_eq!(parameter_value, Some(1.0));
	}

	#[test]
	fn parses_json_activate_action_control_command_by_menu_path() {
		let command =
			parse_renderer_control_command(r#"{"command":"activate_action","menuPath":"Wardrobe","wardrobeSetId":"field_drape"}"#).unwrap();
		let RendererControlCommand::ActivateAction {
			action_id,
			supervisor_command,
			expression_menu_path,
			menu_path,
			wardrobe_set_id,
			parameter_name,
			parameter_value,
		} = command
		else {
			panic!("expected activate_action command");
		};
		assert_eq!(action_id, None);
		assert_eq!(supervisor_command, None);
		assert_eq!(expression_menu_path, None);
		assert_eq!(menu_path.as_deref(), Some("Wardrobe"));
		assert_eq!(wardrobe_set_id.as_deref(), Some("field_drape"));
		assert_eq!(parameter_name, None);
		assert_eq!(parameter_value, None);
	}

	#[test]
	fn resolves_activate_action_from_menu_path() {
		let candidates = vec![
			crate::gpu::RuntimeMenuWardrobeCandidateStatus {
				menu_component_index: 1,
				menu_key: "component:1".to_string(),
				menu_path: vec!["Wardrobe".to_string()],
				menu_path_truncated: false,
				menu_label: Some("Wardrobe".to_string()),
				action_id: "wardrobe:field_drape".to_string(),
				wardrobe_set_id: "field_drape".to_string(),
				match_kind: "trigger".to_string(),
				inverted: false,
			},
			crate::gpu::RuntimeMenuWardrobeCandidateStatus {
				menu_component_index: 2,
				menu_key: "component:2".to_string(),
				menu_path: vec!["Helmet".to_string()],
				menu_path_truncated: false,
				menu_label: Some("Helmet".to_string()),
				action_id: "wardrobe:helmet".to_string(),
				wardrobe_set_id: "helmet".to_string(),
				match_kind: "trigger".to_string(),
				inverted: false,
			},
		];
		assert_eq!(
			resolve_activate_action_from_menu_path("Wardrobe/Field Drape", Some("field_drape"), &candidates).as_deref(),
			Ok("wardrobe:field_drape")
		);
		assert_eq!(
			resolve_activate_action_from_menu_path("Wardrobe", None, &candidates).as_deref(),
			Ok("wardrobe:field_drape")
		);
		assert!(resolve_activate_action_from_menu_path("Wardrobe", Some("field_drape"), &candidates).is_ok());
		assert!(resolve_activate_action_from_menu_path("Wardrobe", Some("missing"), &candidates).is_err());
	}

	#[test]
	fn parses_json_set_parameter_control_command() {
		let command =
			parse_renderer_control_command(r#"{"command":"set_parameter","parameterName":"JacketColor","parameterValue":1.0}"#).unwrap();
		let RendererControlCommand::SetParameter { name, value } = command else {
			panic!("expected set_parameter command");
		};
		assert_eq!(name, "JacketColor");
		assert_eq!(value, 1.0);
	}

	#[test]
	fn parses_json_set_dynamics_control_command() {
		let command = parse_renderer_control_command(
			r#"{"command":"set_dynamics","enabled":false,"bone_colliders":{"enabled":false},"physics":{"simulation_hz":120.0}}"#,
		)
		.unwrap();
		let RendererControlCommand::SetDynamics {
			enabled,
			bone_colliders,
			physics_config,
		} = command
		else {
			panic!("expected set_dynamics command");
		};
		assert!(!enabled);
		assert!(!bone_colliders.enabled);
		assert_eq!(physics_config.unwrap().simulation_hz, 120.0);
	}

	#[test]
	fn parses_legacy_json_set_spring_bones_control_command() {
		let command = parse_renderer_control_command(
			r#"{"command":"set_spring_bones","enabled":true,"bone_colliders":{"enabled":true},"physics":{"simulation_hz":90.0}}"#,
		)
		.unwrap();
		let RendererControlCommand::SetSpringBones {
			enabled,
			bone_colliders,
			physics_config,
		} = command
		else {
			panic!("expected set_spring_bones command");
		};
		assert!(enabled);
		assert!(bone_colliders.enabled);
		assert_eq!(physics_config.unwrap().simulation_hz, 90.0);
	}

	#[test]
	fn parses_json_set_camera_orbit_control_command() {
		let command = parse_renderer_control_command(r#"{"command":"set_camera_orbit","longitude":0.5,"radius":2.0}"#).unwrap();
		let RendererControlCommand::SetCameraOrbit {
			longitude,
			latitude,
			radius,
		} = command
		else {
			panic!("expected set_camera_orbit command");
		};
		assert_eq!(longitude, Some(0.5));
		assert_eq!(latitude, None);
		assert_eq!(radius, Some(2.0));
	}

	#[test]
	fn parses_json_set_camera_state_transition_control_command() {
		let command = parse_renderer_control_command(
			r#"{"command":"set_camera_state","longitude_deg":180.0,"radius":3.5,"transition":{"duration_ms":320,"easing":"ease_out_cubic"}}"#,
		)
		.unwrap();
		let RendererControlCommand::SetCameraState {
			longitude_deg,
			radius,
			transition,
			..
		} = command
		else {
			panic!("expected set_camera_state command");
		};
		assert_eq!(longitude_deg, Some(180.0));
		assert_eq!(radius, Some(3.5));
		let transition = transition.expect("transition");
		assert_eq!(transition.duration_ms, 320);
		assert!(matches!(transition.easing, CameraTransitionEasing::EaseOutCubic));
		assert!(matches!(transition.mode, CameraTransitionMode::Queue));
	}

	#[test]
	fn parses_json_set_camera_state_replace_transition_control_command() {
		let command = parse_renderer_control_command(
			r#"{"command":"set_camera_state","target":[0.0,1.25,0.0],"diagonal_fov_deg":45.0,"transition":{"duration_ms":320,"easing":"ease_out_cubic","mode":"replace"}}"#,
		)
		.unwrap();
		let RendererControlCommand::SetCameraState {
			target,
			diagonal_fov_deg,
			transition,
			..
		} = command
		else {
			panic!("expected set_camera_state command");
		};
		assert_eq!(target, Some([0.0, 1.25, 0.0]));
		assert_eq!(diagonal_fov_deg, Some(45.0));
		let transition = transition.expect("transition");
		assert!(matches!(transition.mode, CameraTransitionMode::Replace));
	}

	#[test]
	fn parses_json_set_clear_color_control_command() {
		let command = parse_renderer_control_command(r#"{"command":"set_clear_color","r":1.2,"g":0.5,"b":-1.0,"a":0.25}"#).unwrap();
		let RendererControlCommand::SetClearColor { r, g, b, a } = command else {
			panic!("expected set_clear_color command");
		};
		assert_eq!(r, 1.2);
		assert_eq!(g, 0.5);
		assert_eq!(b, -1.0);
		assert_eq!(a, 0.25);
	}

	#[test]
	fn parses_json_set_spout_output_control_command() {
		let command =
			parse_renderer_control_command(r#"{"command":"set_spout_output","enabled":true,"name":"Live","width":1280,"height":720}"#)
				.unwrap();
		let RendererControlCommand::SetSpoutOutput {
			enabled,
			name,
			width,
			height,
		} = command
		else {
			panic!("expected set_spout_output command");
		};
		assert!(enabled);
		assert_eq!(name.as_deref(), Some("Live"));
		assert_eq!(width, Some(1280));
		assert_eq!(height, Some(720));
	}

	#[test]
	fn parses_json_set_avatar_outline_control_command() {
		let command = parse_renderer_control_command(
			r#"{"command":"set_avatar_outline","policy":"override","type":"mtoon","width":0.004,"color":[1.0,0.5,0.25],"lighting_mix":0.1,"roundness":0.75}"#,
		)
		.unwrap();
		let RendererControlCommand::SetAvatarOutline {
			policy,
			r#type,
			width,
			color,
			lighting_mix,
			roundness,
		} = command
		else {
			panic!("expected set_avatar_outline command");
		};
		assert_eq!(policy.as_deref(), Some("override"));
		assert_eq!(r#type.as_deref(), Some("mtoon"));
		assert_eq!(width, Some(0.004));
		assert_eq!(color, Some([1.0, 0.5, 0.25]));
		assert_eq!(lighting_mix, Some(0.1));
		assert_eq!(roundness, Some(0.75));
	}

	#[test]
	fn parses_json_set_environment_color_control_command() {
		let command =
			parse_renderer_control_command(
				r#"{"command":"set_environment_color","exposure":0.25,"contrast":1.2,"saturation":0.8,"look":"film","intensity":0.45,"temperature":0.2,"tint":-0.15}"#,
			)
				.unwrap();
		let RendererControlCommand::SetEnvironmentColor {
			exposure,
			contrast,
			saturation,
			look,
			intensity,
			temperature,
			tint,
		} = command
		else {
			panic!("expected set_environment_color command");
		};
		assert_eq!(exposure, Some(0.25));
		assert_eq!(contrast, Some(1.2));
		assert_eq!(saturation, Some(0.8));
		assert_eq!(look.as_deref(), Some("film"));
		assert_eq!(intensity, Some(0.45));
		assert_eq!(temperature, Some(0.2));
		assert_eq!(tint, Some(-0.15));
	}

	#[test]
	fn parses_json_set_avatar_matcap_control_command() {
		let command = parse_renderer_control_command(r#"{"command":"set_avatar_matcap","scale":1.35}"#).unwrap();
		let RendererControlCommand::SetAvatarMatcap { scale } = command else {
			panic!("expected set_avatar_matcap command");
		};
		assert_eq!(scale, Some(1.35));
	}

	#[test]
	fn parses_json_set_avatar_specular_control_command() {
		let command =
			parse_renderer_control_command(r#"{"command":"set_avatar_specular","enabled":true,"intensity":0.5,"power":32.0}"#).unwrap();
		let RendererControlCommand::SetAvatarSpecular { enabled, intensity, power } = command else {
			panic!("expected set_avatar_specular command");
		};
		assert_eq!(enabled, Some(true));
		assert_eq!(intensity, Some(0.5));
		assert_eq!(power, Some(32.0));
	}

	#[test]
	fn parses_json_set_avatar_ambient_occlusion_control_command() {
		let command = parse_renderer_control_command(r#"{"command":"set_avatar_ambient_occlusion","strength":1.4}"#).unwrap();
		let RendererControlCommand::SetAvatarAmbientOcclusion { strength } = command else {
			panic!("expected set_avatar_ambient_occlusion command");
		};
		assert_eq!(strength, Some(1.4));
	}

	#[test]
	fn parses_json_set_bloom_control_command() {
		let command = parse_renderer_control_command(
			r#"{"command":"set_bloom","enabled":true,"strength":0.4,"threshold":0.9,"radius":12.0,"quality":"high_quality"}"#,
		)
		.unwrap();
		let RendererControlCommand::SetBloom {
			enabled,
			strength,
			threshold,
			radius,
			quality,
		} = command
		else {
			panic!("expected set_bloom command");
		};
		assert_eq!(enabled, Some(true));
		assert_eq!(strength, Some(0.4));
		assert_eq!(threshold, Some(0.9));
		assert_eq!(radius, Some(12.0));
		assert_eq!(quality.as_deref(), Some("high_quality"));
	}

	#[test]
	fn parses_json_set_ssao_control_command() {
		let command = parse_renderer_control_command(
			r#"{"command":"set_ssao","enabled":true,"strength":0.25,"radius":4.0,"bias":0.001,"range":0.03}"#,
		)
		.unwrap();
		let RendererControlCommand::SetSsao {
			enabled,
			strength,
			radius,
			bias,
			range,
		} = command
		else {
			panic!("expected set_ssao command");
		};
		assert_eq!(enabled, Some(true));
		assert_eq!(strength, Some(0.25));
		assert_eq!(radius, Some(4.0));
		assert_eq!(bias, Some(0.001));
		assert_eq!(range, Some(0.03));
	}

	#[test]
	fn parses_json_set_contact_shadow_control_command() {
		let command = parse_renderer_control_command(
			r#"{"command":"set_contact_shadow","enabled":true,"strength":0.35,"radius":0.7,"softness":2.0,"height":0.02}"#,
		)
		.unwrap();
		let RendererControlCommand::SetContactShadow {
			enabled,
			strength,
			radius,
			softness,
			height,
		} = command
		else {
			panic!("expected set_contact_shadow command");
		};
		assert_eq!(enabled, Some(true));
		assert_eq!(strength, Some(0.35));
		assert_eq!(radius, Some(0.7));
		assert_eq!(softness, Some(2.0));
		assert_eq!(height, Some(0.02));
	}

	#[test]
	fn parses_json_set_window_control_command() {
		let command = parse_renderer_control_command(
			r#"{"command":"set_window","decorations":false,"transparent":true,"input_passthrough":true,"always_on_top":true,"width":960,"height":540}"#,
		)
		.unwrap();
		let RendererControlCommand::SetWindow {
			decorations,
			transparent,
			input_passthrough,
			always_on_top,
			minimized,
			width,
			height,
		} = command
		else {
			panic!("expected set_window command");
		};
		assert_eq!(decorations, Some(false));
		assert_eq!(transparent, Some(true));
		assert_eq!(input_passthrough, Some(true));
		assert_eq!(always_on_top, Some(true));
		assert_eq!(minimized, None);
		assert_eq!(width, Some(960));
		assert_eq!(height, Some(540));
	}

	#[test]
	fn parses_close_hotkey_aliases() {
		let hotkey = CloseHotkey::parse("Esc").unwrap().unwrap();
		assert_eq!(hotkey.key, "escape");
		assert!(!hotkey.control);
		assert!(!hotkey.shift);
		assert!(!hotkey.alt);
		assert!(!hotkey.super_key);
	}

	#[test]
	fn parses_modified_close_hotkey() {
		let hotkey = CloseHotkey::parse("Ctrl+Shift+Q").unwrap().unwrap();
		assert_eq!(hotkey.key, "q");
		assert!(hotkey.control);
		assert!(hotkey.shift);
		assert!(!hotkey.alt);
		assert!(!hotkey.super_key);
	}

	#[test]
	fn close_hotkey_matches_named_key() {
		use winit::keyboard::NamedKey;

		let hotkey = CloseHotkey::parse("Escape").unwrap().unwrap();
		assert!(hotkey.matches(&Key::Named(NamedKey::Escape), ModifiersState::default()));
	}

	#[test]
	fn close_hotkey_can_be_disabled() {
		assert!(CloseHotkey::parse("None").unwrap().is_none());
	}
}
