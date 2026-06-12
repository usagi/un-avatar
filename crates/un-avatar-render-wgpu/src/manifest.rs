use std::{fs, net::SocketAddr, path::Path};

use serde::Deserialize;

use crate::{
	mesh_pass::{
		AvatarAmbientOcclusionOptions, AvatarMatcapOptions, AvatarOutlineKind, AvatarOutlinePolicy, AvatarRimPolicy, AvatarSpecularOptions,
	},
	options::{
		AudioLinkSource, AvatarWindowOptions, BloomOptions, BloomQuality, ColorGradingLook, ContactShadowOptions, DirectionalLightOptions,
		EnvironmentColorOptions, EnvironmentLightOptions, PrimaryMotionSource, SsaoOptions,
	},
	AaMode, BlockCompressionEncoder, RenderBackend, SceneMeshLoadOpts, SpoutWindowOptions, TextureCompressionAdvancedOptions,
	TextureCompressionMode, TextureMipmapFilter, TextureResolutionLimit, WindowDebugOptions,
};
use un_avatar_skeleton::SpringBonePhysicsConfig;

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct RendererManifest {
	pub title: Option<String>,
	pub decorations: Option<bool>,
	pub transparent: Option<bool>,
	pub input_passthrough: Option<bool>,
	#[serde(alias = "gltf_path", alias = "model_path")]
	pub avatar_path: Option<std::path::PathBuf>,
	/// `.unavatar` wardrobe set id. Base は常に import 時に適用し、ここでは追加セットだけを指定する。
	pub wardrobe_set: Option<String>,
	#[serde(alias = "window_icon", alias = "icon")]
	pub icon_path: Option<std::path::PathBuf>,
	pub background_color: Option<[f64; 3]>,
	pub clear_color: Option<[f64; 4]>,
	pub show_fps_in_title: Option<bool>,
	pub vmc_address: Option<SocketAddr>,
	pub vmc_port: Option<u16>,
	pub motion: Option<MotionManifest>,
	pub audio_link: Option<AudioLinkManifest>,
	pub physics: Option<PhysicsManifest>,
	pub spring_bones: Option<bool>,
	pub aa: Option<AaMode>,
	pub render_quality: Option<RenderQualityManifest>,
	pub environment: Option<EnvironmentManifest>,
	/// 旧 manifest 互換。新規 profile は `[output.spout2]` を使う。
	pub spout: Option<SpoutManifest>,
	pub output: Option<OutputManifest>,
	pub ipc: Option<IpcManifest>,
	pub debug: Option<DebugManifest>,
	pub diagnostics: Option<MeshDiagnosticsManifest>,
	pub effects: Option<EffectsManifest>,
	pub window: Option<WindowManifest>,
	pub camera: Option<CameraManifest>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct CameraManifest {
	/// マウス操作によるカメラ操作をロックする（既定 false）。
	/// `[camera] locked = true` で起動時から操作不可、IPC や CLI で動的に変更可能。
	pub locked: Option<bool>,
	/// target ワールド座標 \[x, y, z\]（既定はモデル胸元相当 \[0, 1.05, 0\]）。
	pub target: Option<[f32; 3]>,
	/// orbit 経度（度）。
	pub longitude_deg: Option<f32>,
	/// orbit 緯度（度）。
	pub latitude_deg: Option<f32>,
	/// orbit 半径（target からカメラ位置までの距離）。
	pub radius: Option<f32>,
	/// 対角画角（度）。35mm 換算で言う焦点距離 35mm = 約 63.45°。
	pub diagonal_fov_deg: Option<f32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct WindowManifest {
	pub icon_path: Option<std::path::PathBuf>,
	pub decorations: Option<bool>,
	pub transparent: Option<bool>,
	pub input_passthrough: Option<bool>,
	pub always_on_top: Option<bool>,
	pub width: Option<u32>,
	pub height: Option<u32>,
	/// 起動時の outer 位置（px）。`x` / `y` 両方が指定されたときだけ適用される。
	/// 片方だけの指定は無視（モニタ移行時に意図しない位置に飛ばないようにするため）。
	pub x: Option<i32>,
	pub y: Option<i32>,
	/// 起動直後にウィンドウを最小化するか（既定 false）。
	/// `[window] minimized = true` で起動時から最小化、IPC `set_window` で切替も可能。
	pub minimized: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct SpoutManifest {
	pub enabled: Option<bool>,
	pub name: Option<String>,
	pub width: Option<u32>,
	pub height: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct RenderQualityManifest {
	pub aa: Option<AaMode>,
	pub texture_resolution_limit: Option<TextureResolutionLimit>,
	pub texture_compression: Option<TextureCompressionMode>,
	pub mipmap_filter: Option<TextureMipmapFilter>,
	pub render_backend: Option<RenderBackend>,
	pub block_compression_encoder: Option<BlockCompressionEncoder>,
	pub block_compression_cpu_threads: Option<usize>,
	pub texture_compression_advanced: Option<TextureCompressionAdvancedOptions>,
	pub processed_texture_cache: Option<bool>,
	pub skin_tone_matching: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct EnvironmentManifest {
	pub color: Option<EnvironmentColorManifest>,
	pub lighting: Option<LightingManifest>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct EnvironmentColorManifest {
	pub exposure: Option<f32>,
	pub contrast: Option<f32>,
	pub saturation: Option<f32>,
	pub look: Option<String>,
	pub intensity: Option<f32>,
	pub temperature: Option<f32>,
	pub tint: Option<f32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct LightingManifest {
	pub environment: Option<EnvironmentLightManifest>,
	pub directional: Option<DirectionalLightManifest>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct EnvironmentLightManifest {
	pub enabled: Option<bool>,
	pub color: Option<[f32; 3]>,
	pub intensity: Option<f32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct DirectionalLightManifest {
	pub enabled: Option<bool>,
	pub color: Option<[f32; 3]>,
	pub intensity: Option<f32>,
	#[serde(alias = "longitude_deg")]
	pub azimuth_deg: Option<f32>,
	#[serde(alias = "latitude_deg")]
	pub elevation_deg: Option<f32>,
	pub follow_camera_yaw: Option<bool>,
	#[serde(rename = "reference")]
	/// Deprecated manifest spelling. Kept only to read existing profiles.
	pub legacy_reference: Option<String>,
	pub follow_camera_pitch: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct EffectsManifest {
	pub avatar: Option<AvatarEffectsManifest>,
	pub post: Option<PostEffectsManifest>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct PostEffectsManifest {
	pub bloom: Option<BloomManifest>,
	pub ssao: Option<SsaoManifest>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct BloomManifest {
	pub enabled: Option<bool>,
	pub strength: Option<f32>,
	pub threshold: Option<f32>,
	pub radius: Option<f32>,
	pub quality: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct SsaoManifest {
	pub enabled: Option<bool>,
	pub strength: Option<f32>,
	pub radius: Option<f32>,
	pub bias: Option<f32>,
	pub range: Option<f32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct AvatarEffectsManifest {
	pub outline_policy: Option<String>,
	#[serde(alias = "outline_kind")]
	pub outline_type: Option<String>,
	pub outline_width: Option<f32>,
	pub outline_color: Option<[f32; 3]>,
	pub outline_lighting_mix: Option<f32>,
	pub outline_roundness: Option<f32>,
	pub outline: Option<AvatarOutlineManifest>,
	pub rim: Option<AvatarRimManifest>,
	pub matcap: Option<AvatarMatcapManifest>,
	pub specular: Option<AvatarSpecularManifest>,
	pub ambient_occlusion: Option<AvatarAmbientOcclusionManifest>,
	pub contact_shadow: Option<ContactShadowManifest>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct ContactShadowManifest {
	pub enabled: Option<bool>,
	pub strength: Option<f32>,
	pub radius: Option<f32>,
	pub softness: Option<f32>,
	pub height: Option<f32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct AvatarOutlineManifest {
	pub policy: Option<String>,
	#[serde(alias = "kind")]
	pub r#type: Option<String>,
	pub width: Option<f32>,
	pub color: Option<[f32; 3]>,
	pub lighting_mix: Option<f32>,
	pub roundness: Option<f32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct AvatarRimManifest {
	pub policy: Option<String>,
	pub color: Option<[f32; 3]>,
	pub intensity: Option<f32>,
	pub lighting_mix: Option<f32>,
	pub fresnel_power: Option<f32>,
	pub lift: Option<f32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct AvatarMatcapManifest {
	pub scale: Option<f32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct AvatarSpecularManifest {
	pub enabled: Option<bool>,
	pub intensity: Option<f32>,
	pub power: Option<f32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct AvatarAmbientOcclusionManifest {
	pub strength: Option<f32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct MotionManifest {
	pub vmc_udp: Option<VmcUdpManifest>,
	pub unmotion_zenoh: Option<UnmotionZenohManifest>,
	pub look_at: Option<LookAtManifest>,
	/// VMC `Root.translation` を scene root へ加算するか。既定 `false`。詳細は
	/// [`AvatarWindowOptions::apply_vmc_root_translation`] のドキュメントを参照。
	pub apply_vmc_root_translation: Option<bool>,
	/// 旧 manifest 互換の primary source 選択。現在の姿勢適用は key 単位の後着優先。
	pub primary_source: Option<PrimaryMotionSource>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct AudioLinkManifest {
	pub source: Option<AudioLinkSource>,
	pub input_device_id: Option<String>,
	pub input_device_name_hint: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct PhysicsManifest {
	pub bone_colliders: Option<BoneCollidersManifest>,
	pub contacts: Option<ContactsPhysicsManifest>,
	pub dynamics: Option<DynamicsPhysicsManifest>,
	pub spring_bone: Option<SpringBonePhysicsConfig>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct DynamicsPhysicsManifest {
	pub enable_all_on_launch: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct ContactsPhysicsManifest {
	pub parameter_emission: Option<bool>,
	pub parameter_emission_enabled: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct BoneCollidersManifest {
	pub enabled: Option<bool>,
	pub radius_mm: Option<BoneColliderRadiiMmManifest>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct BoneColliderRadiiMmManifest {
	pub head: Option<f32>,
	pub neck_chest: Option<f32>,
	pub torso: Option<f32>,
	pub upper_arms: Option<f32>,
	pub lower_arms: Option<f32>,
	pub hands: Option<f32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct LookAtManifest {
	/// LookAt 補正の有効化（true なら eye bone を yaw/pitch クランプ）。
	pub enabled: Option<bool>,
	/// クランプ角度（度）。VRM 1.0 デフォルト 30°。
	pub clamp_deg: Option<f32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct VmcUdpManifest {
	pub enabled: Option<bool>,
	pub address: Option<SocketAddr>,
	pub port: Option<u16>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct UnmotionZenohManifest {
	pub enabled: Option<bool>,
	pub key: Option<String>,
}

impl UnmotionZenohManifest {
	fn apply_to(self, opts: &mut AvatarWindowOptions) {
		if let Some(enabled) = self.enabled {
			opts.unmotion_zenoh.enabled = enabled;
		}
		if let Some(key) = self.key {
			// 空文字は「明示的に空白を残したい」ではなく「既定にリセット」と解釈する。
			let trimmed = key.trim();
			if trimmed.is_empty() {
				opts.unmotion_zenoh.base_key_expr = "un-motion/frame".to_string();
			} else {
				opts.unmotion_zenoh.base_key_expr = trimmed.to_string();
			}
		}
	}
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct OutputManifest {
	pub spout2: Option<SpoutManifest>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct IpcManifest {
	pub runtime_status_address: Option<SocketAddr>,
	pub runtime_control_address: Option<SocketAddr>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct DebugManifest {
	pub log_path: Option<std::path::PathBuf>,
	pub mirror_stderr: Option<bool>,
	pub vmc: Option<bool>,
	pub scene: Option<bool>,
	pub morph: Option<bool>,
	pub disable_expression_morphs: Option<bool>,
	pub disable_vmc_eye_look: Option<bool>,
	pub simple_basecolor_only: Option<bool>,
	pub material_dump: Option<bool>,
	/// XYZ デバッグ軸表示の初期値（既定 false）。
	pub show_axes: Option<bool>,
	/// ボーンベースコライダー debug 表示の初期値（既定 false）。
	pub show_bone_colliders: Option<bool>,
	/// MToon outline 描画を完全に無効化する診断 toggle（既定 false）。
	/// 一部の VRM モデルで目周辺に肌色寄りの outline が太く出る現象の切り分け用。
	pub disable_mtoon_outlines: Option<bool>,
	/// MToon の parametric Rim Lighting 寄与を 0 にする診断 toggle（既定 false）。
	pub disable_rim_lighting: Option<bool>,
	/// `shading_shift_factor` と `shadingShiftTexture` の寄与を 0 固定にする診断 toggle（既定 false）。
	pub force_shading_shift_zero: Option<bool>,
	/// matcap (sphere add) 寄与を 0 にする診断 toggle（既定 false）。
	pub disable_matcap: Option<bool>,
	/// emissive (`emissive_factor × emissive_tex`) 寄与を 0 にする診断 toggle（既定 false）。
	pub disable_emissive: Option<bool>,
	/// MToon `shade_color × shade_tex` の代わりに base を使う診断 toggle（既定 false）。
	pub disable_shade_color: Option<bool>,
	/// normalTexture を使わず頂点法線のみで shading / rim を計算する診断 toggle（既定 false）。
	pub disable_normal_map: Option<bool>,
	/// toon path を `base` のみで早期 return する診断 toggle（既定 false）。
	pub base_texture_only: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub(crate) struct MeshDiagnosticsManifest {
	pub bind_pose: Option<bool>,
	pub primitive_colors: Option<bool>,
	pub zero_morphs: Option<bool>,
	pub relax_iris_alpha: Option<bool>,
	pub skin_legacy_no_inv_mesh: Option<bool>,
	pub disable_reflection: Option<bool>,
	pub disable_fur: Option<bool>,
}

impl RendererManifest {
	pub(crate) fn load(path: &Path) -> Result<Self, String> {
		let text = fs::read_to_string(path).map_err(|e| format!("manifest {}: {e}", path.display()))?;
		match path
			.extension()
			.and_then(|e| e.to_str())
			.unwrap_or("")
			.to_ascii_lowercase()
			.as_str()
		{
			"toml" => toml::from_str(&text).map_err(|e| format!("manifest TOML {}: {e}", path.display())),
			other => Err(format!("manifest {}: unsupported extension {:?}; use .toml", path.display(), other)),
		}
	}

	pub(crate) fn apply_to(self, opts: &mut AvatarWindowOptions) {
		if let Some(title) = self.title {
			opts.title = title;
		}
		if let Some(decorations) = self.decorations {
			opts.decorations = decorations;
		}
		if let Some(transparent) = self.transparent {
			opts.transparent = transparent;
		}
		if let Some(input_passthrough) = self.input_passthrough {
			opts.input_passthrough = input_passthrough;
		}
		if let Some(path) = self.avatar_path {
			opts.gltf_path = Some(path);
		}
		if let Some(set_id) = self.wardrobe_set {
			opts.wardrobe_set = Some(set_id);
		}
		if let Some(path) = self.icon_path {
			opts.icon_path = Some(path);
		}
		if let Some([r, g, b, a]) = self.clear_color {
			opts.clear_color = wgpu::Color { r, g, b, a };
		}
		if let Some([r, g, b]) = self.background_color {
			opts.clear_color = wgpu::Color {
				r,
				g,
				b,
				a: if opts.transparent { 0.0 } else { 1.0 },
			};
		}
		if let Some(show_fps) = self.show_fps_in_title {
			opts.show_fps_in_title = show_fps;
		}
		if let Some(vmc_address) = self.vmc_address {
			opts.vmc_address = Some(vmc_address);
		} else if let Some(vmc_port) = self.vmc_port {
			opts.vmc_address = Some(vmc_addr_from_port(vmc_port));
		}
		if let Some(motion) = self.motion {
			motion.apply_to(opts);
		}
		if let Some(audio_link) = self.audio_link {
			audio_link.apply_to(opts);
		}
		if let Some(physics) = self.physics {
			physics.apply_to(opts);
		}
		if let Some(spring_bones) = self.spring_bones {
			opts.enable_spring_bones = spring_bones;
		}
		if let Some(aa) = self.aa {
			opts.aa = aa;
		}
		if let Some(render_quality) = self.render_quality {
			render_quality.apply_to(opts);
		}
		if let Some(spout) = self.spout {
			spout.apply_to(&mut opts.spout);
		}
		if let Some(environment) = self.environment {
			environment.apply_to(opts);
		}
		if let Some(output) = self.output {
			output.apply_to(&mut opts.spout);
		}
		if let Some(ipc) = self.ipc {
			ipc.apply_to(opts);
		}
		if let Some(debug) = self.debug {
			debug.apply_to(
				&mut opts.debug,
				&mut opts.disable_expression_morphs,
				&mut opts.disable_vmc_eye_look,
				&mut opts.simple_basecolor_only,
				&mut opts.debug_material_dump,
				&mut opts.show_axes,
				&mut opts.show_bone_colliders,
				&mut opts.disable_mtoon_outlines,
				&mut opts.debug_disable_rim_lighting,
				&mut opts.debug_force_shading_shift_zero,
				&mut opts.debug_disable_matcap,
				&mut opts.debug_disable_emissive,
				&mut opts.debug_disable_shade_color,
				&mut opts.debug_disable_normal_map,
				&mut opts.debug_base_texture_only,
			);
		}
		if let Some(diagnostics) = self.diagnostics {
			diagnostics.apply_to(&mut opts.mesh_diagnostics);
		}
		if let Some(effects) = self.effects {
			effects.apply_to(opts);
		}
		if let Some(camera) = self.camera {
			if let Some(locked) = camera.locked {
				opts.camera_locked = locked;
			}
			if camera.target.is_some()
				|| camera.longitude_deg.is_some()
				|| camera.latitude_deg.is_some()
				|| camera.radius.is_some()
				|| camera.diagonal_fov_deg.is_some()
			{
				opts.initial_camera_state = Some(crate::options::InitialCameraState {
					target: camera.target,
					longitude_deg: camera.longitude_deg,
					latitude_deg: camera.latitude_deg,
					radius: camera.radius,
					diagonal_fov_deg: camera.diagonal_fov_deg,
				});
			}
		}
		if let Some(window) = self.window {
			window.apply_to(opts);
		}
	}
}

impl EnvironmentManifest {
	fn apply_to(self, opts: &mut AvatarWindowOptions) {
		if let Some(color) = self.color {
			color.apply_to(&mut opts.environment_color);
		}
		if let Some(lighting) = self.lighting {
			lighting.apply_to(opts);
		}
	}
}

impl EnvironmentColorManifest {
	fn apply_to(self, color: &mut EnvironmentColorOptions) {
		if let Some(exposure) = self.exposure {
			color.exposure = exposure.clamp(-4.0, 4.0);
		}
		if let Some(contrast) = self.contrast {
			color.contrast = contrast.clamp(0.0, 4.0);
		}
		if let Some(saturation) = self.saturation {
			color.saturation = saturation.clamp(0.0, 4.0);
		}
		if let Some(look) = self.look {
			color.look = look.parse::<ColorGradingLook>().unwrap_or(ColorGradingLook::Neutral);
		}
		if let Some(intensity) = self.intensity {
			color.look_intensity = intensity.clamp(0.0, 1.0);
		}
		if let Some(temperature) = self.temperature {
			color.temperature = temperature.clamp(-1.0, 1.0);
		}
		if let Some(tint) = self.tint {
			color.tint = tint.clamp(-1.0, 1.0);
		}
		if matches!(color.look, ColorGradingLook::Neutral) {
			color.look_intensity = 0.0;
		}
	}
}

impl LightingManifest {
	fn apply_to(self, opts: &mut AvatarWindowOptions) {
		if let Some(environment) = self.environment {
			environment.apply_to(&mut opts.lighting.environment);
		}
		if let Some(directional) = self.directional {
			directional.apply_to(&mut opts.lighting.directional);
		}
	}
}

impl EnvironmentLightManifest {
	fn apply_to(self, light: &mut EnvironmentLightOptions) {
		if let Some(enabled) = self.enabled {
			light.enabled = enabled;
		}
		if let Some(color) = self.color {
			light.color = clamp_rgb(color);
		}
		if let Some(intensity) = self.intensity {
			light.intensity = intensity.clamp(0.0, 2.0);
		}
	}
}

impl DirectionalLightManifest {
	fn apply_to(self, light: &mut DirectionalLightOptions) {
		if let Some(enabled) = self.enabled {
			light.enabled = enabled;
		}
		if let Some(color) = self.color {
			light.color = clamp_rgb(color);
		}
		if let Some(intensity) = self.intensity {
			light.intensity = intensity.clamp(0.0, 4.0);
		}
		if let Some(azimuth_deg) = self.azimuth_deg {
			light.azimuth_deg = azimuth_deg.clamp(-360.0, 360.0);
		}
		if let Some(elevation_deg) = self.elevation_deg {
			light.elevation_deg = elevation_deg.clamp(-89.0, 89.0);
		}
		if let Some(reference) = self.legacy_reference {
			light.follow_camera_yaw = match reference.trim().to_ascii_lowercase().as_str() {
				"camera" => true,
				"world" | "model" => false,
				_ => light.follow_camera_yaw,
			};
		}
		if let Some(follow_camera_yaw) = self.follow_camera_yaw {
			light.follow_camera_yaw = follow_camera_yaw;
		}
		if let Some(follow_camera_pitch) = self.follow_camera_pitch {
			light.follow_camera_pitch = follow_camera_pitch;
		}
	}
}

impl EffectsManifest {
	fn apply_to(self, opts: &mut AvatarWindowOptions) {
		if let Some(avatar) = self.avatar {
			avatar.apply_to(opts);
		}
		if let Some(post) = self.post {
			post.apply_to(opts);
		}
	}
}

impl PostEffectsManifest {
	fn apply_to(self, opts: &mut AvatarWindowOptions) {
		if let Some(bloom) = self.bloom {
			bloom.apply_to(&mut opts.bloom);
		}
		if let Some(ssao) = self.ssao {
			ssao.apply_to(&mut opts.ssao);
		}
	}
}

impl BloomManifest {
	fn apply_to(self, bloom: &mut BloomOptions) {
		if let Some(enabled) = self.enabled {
			bloom.enabled = enabled;
		}
		if let Some(strength) = self.strength {
			bloom.strength = strength.clamp(0.0, 2.0);
		}
		if let Some(threshold) = self.threshold {
			bloom.threshold = threshold.clamp(0.0, 2.0);
		}
		if let Some(radius) = self.radius {
			bloom.radius = radius.clamp(0.0, 32.0);
		}
		if let Some(quality) = self.quality {
			bloom.quality = quality.parse::<BloomQuality>().unwrap_or(BloomQuality::Compact);
		}
	}
}

impl SsaoManifest {
	fn apply_to(self, ssao: &mut SsaoOptions) {
		if let Some(enabled) = self.enabled {
			ssao.enabled = enabled;
		}
		if let Some(strength) = self.strength {
			ssao.strength = strength.clamp(0.0, 1.0);
		}
		if let Some(radius) = self.radius {
			ssao.radius = radius.clamp(1.0, 24.0);
		}
		if let Some(bias) = self.bias {
			ssao.bias = bias.clamp(0.0, 0.02);
		}
		if let Some(range) = self.range {
			ssao.range = range.clamp(0.001, 0.2);
		}
	}
}

impl AvatarEffectsManifest {
	fn apply_to(self, opts: &mut AvatarWindowOptions) {
		let diagnostics = &mut opts.mesh_diagnostics;
		if let Some(policy) = self.outline_policy.as_deref().and_then(parse_outline_policy) {
			diagnostics.avatar_outline.policy = policy;
		}
		if let Some(kind) = self.outline_type.as_deref().and_then(parse_outline_kind) {
			diagnostics.avatar_outline.kind = kind;
		}
		if let Some(width) = self.outline_width {
			diagnostics.avatar_outline.width = Some(width.max(0.0));
		}
		if let Some(color) = self.outline_color {
			diagnostics.avatar_outline.color = Some(clamp_rgb(color));
		}
		if let Some(lighting_mix) = self.outline_lighting_mix {
			diagnostics.avatar_outline.lighting_mix = Some(lighting_mix.clamp(0.0, 1.0));
		}
		if let Some(roundness) = self.outline_roundness {
			diagnostics.avatar_outline.roundness = Some(roundness.clamp(0.0, 1.0));
		}
		if let Some(outline) = self.outline {
			outline.apply_to(diagnostics);
		}
		if let Some(rim) = self.rim {
			rim.apply_to(diagnostics);
		}
		if let Some(matcap) = self.matcap {
			matcap.apply_to(diagnostics);
		}
		if let Some(specular) = self.specular {
			specular.apply_to(diagnostics);
		}
		if let Some(ambient_occlusion) = self.ambient_occlusion {
			ambient_occlusion.apply_to(diagnostics);
		}
		if let Some(contact_shadow) = self.contact_shadow {
			contact_shadow.apply_to(&mut opts.contact_shadow);
		}
	}
}

impl AvatarOutlineManifest {
	fn apply_to(self, diagnostics: &mut SceneMeshLoadOpts) {
		if let Some(policy) = self.policy.as_deref().and_then(parse_outline_policy) {
			diagnostics.avatar_outline.policy = policy;
		}
		if let Some(kind) = self.r#type.as_deref().and_then(parse_outline_kind) {
			diagnostics.avatar_outline.kind = kind;
		}
		if let Some(width) = self.width {
			diagnostics.avatar_outline.width = Some(width.max(0.0));
		}
		if let Some(color) = self.color {
			diagnostics.avatar_outline.color = Some(clamp_rgb(color));
		}
		if let Some(lighting_mix) = self.lighting_mix {
			diagnostics.avatar_outline.lighting_mix = Some(lighting_mix.clamp(0.0, 1.0));
		}
		if let Some(roundness) = self.roundness {
			diagnostics.avatar_outline.roundness = Some(roundness.clamp(0.0, 1.0));
		}
	}
}

impl AvatarRimManifest {
	fn apply_to(self, diagnostics: &mut SceneMeshLoadOpts) {
		if let Some(policy) = self.policy.as_deref().and_then(parse_rim_policy) {
			diagnostics.avatar_rim.policy = policy;
		}
		if let Some(color) = self.color {
			diagnostics.avatar_rim.color = Some(clamp_rgb(color));
		}
		if let Some(intensity) = self.intensity {
			diagnostics.avatar_rim.intensity = Some(intensity.clamp(0.0, 4.0));
		}
		if let Some(lighting_mix) = self.lighting_mix {
			diagnostics.avatar_rim.lighting_mix = Some(lighting_mix.clamp(0.0, 1.0));
		}
		if let Some(power) = self.fresnel_power {
			diagnostics.avatar_rim.fresnel_power = Some(power.max(0.00001));
		}
		if let Some(lift) = self.lift {
			diagnostics.avatar_rim.lift = Some(lift.clamp(-1.0, 1.0));
		}
	}
}

impl AvatarMatcapManifest {
	fn apply_to(self, diagnostics: &mut SceneMeshLoadOpts) {
		if let Some(scale) = self.scale {
			diagnostics.avatar_matcap = AvatarMatcapOptions {
				scale: scale.clamp(0.0, 2.0),
			};
		}
	}
}

impl AvatarSpecularManifest {
	fn apply_to(self, diagnostics: &mut SceneMeshLoadOpts) {
		diagnostics.avatar_specular = AvatarSpecularOptions {
			enabled: self.enabled.unwrap_or(diagnostics.avatar_specular.enabled),
			intensity: self.intensity.unwrap_or(diagnostics.avatar_specular.intensity).clamp(0.0, 2.0),
			power: self.power.unwrap_or(diagnostics.avatar_specular.power).clamp(1.0, 128.0),
		};
	}
}

impl AvatarAmbientOcclusionManifest {
	fn apply_to(self, diagnostics: &mut SceneMeshLoadOpts) {
		diagnostics.avatar_ambient_occlusion = AvatarAmbientOcclusionOptions {
			strength: self
				.strength
				.unwrap_or(diagnostics.avatar_ambient_occlusion.strength)
				.clamp(0.0, 2.0),
		};
	}
}

impl ContactShadowManifest {
	fn apply_to(self, contact_shadow: &mut ContactShadowOptions) {
		if let Some(enabled) = self.enabled {
			contact_shadow.enabled = enabled;
		}
		if let Some(strength) = self.strength {
			contact_shadow.strength = strength.clamp(0.0, 1.0);
		}
		if let Some(radius) = self.radius {
			contact_shadow.radius = radius.clamp(0.05, 3.0);
		}
		if let Some(softness) = self.softness {
			contact_shadow.softness = softness.clamp(0.1, 8.0);
		}
		if let Some(height) = self.height {
			contact_shadow.height = height.clamp(-1.0, 1.0);
		}
	}
}

fn parse_outline_policy(value: &str) -> Option<AvatarOutlinePolicy> {
	match value.trim().to_ascii_lowercase().as_str() {
		"authored" => Some(AvatarOutlinePolicy::Authored),
		"off" | "none" | "disabled" => Some(AvatarOutlinePolicy::Off),
		"override" | "custom" => Some(AvatarOutlinePolicy::Override),
		_ => None,
	}
}

fn parse_rim_policy(value: &str) -> Option<AvatarRimPolicy> {
	match value.trim().to_ascii_lowercase().as_str() {
		"authored" => Some(AvatarRimPolicy::Authored),
		"off" | "none" | "disabled" => Some(AvatarRimPolicy::Off),
		"override" | "custom" => Some(AvatarRimPolicy::Override),
		_ => None,
	}
}

fn parse_outline_kind(value: &str) -> Option<AvatarOutlineKind> {
	match value.trim().to_ascii_lowercase().as_str() {
		"mtoon" | "geometry" => Some(AvatarOutlineKind::Mtoon),
		"ink" => Some(AvatarOutlineKind::Ink),
		"brush" | "hake" | "fude" => Some(AvatarOutlineKind::Brush),
		"double" | "double_outline" => Some(AvatarOutlineKind::Double),
		_ => None,
	}
}

fn clamp_rgb(rgb: [f32; 3]) -> [f32; 3] {
	[rgb[0].clamp(0.0, 1.0), rgb[1].clamp(0.0, 1.0), rgb[2].clamp(0.0, 1.0)]
}

impl IpcManifest {
	fn apply_to(self, opts: &mut AvatarWindowOptions) {
		if let Some(address) = self.runtime_status_address {
			opts.runtime_status_address = Some(address);
		}
		if let Some(address) = self.runtime_control_address {
			opts.runtime_control_address = Some(address);
		}
	}
}

impl WindowManifest {
	fn apply_to(self, opts: &mut AvatarWindowOptions) {
		if let Some(path) = self.icon_path {
			opts.icon_path = Some(path);
		}
		if let Some(decorations) = self.decorations {
			opts.decorations = decorations;
		}
		if let Some(transparent) = self.transparent {
			opts.transparent = transparent;
		}
		if let Some(input_passthrough) = self.input_passthrough {
			opts.input_passthrough = input_passthrough;
		}
		if let Some(always_on_top) = self.always_on_top {
			opts.always_on_top = always_on_top;
		}
		if let Some(width) = self.width {
			opts.window_width = width;
		}
		if let Some(height) = self.height {
			opts.window_height = height;
		}
		if let (Some(x), Some(y)) = (self.x, self.y) {
			opts.window_position = Some([x, y]);
		}
		if let Some(minimized) = self.minimized {
			opts.start_minimized = minimized;
		}
	}
}

impl SpoutManifest {
	fn apply_to(self, spout: &mut SpoutWindowOptions) {
		if let Some(enabled) = self.enabled {
			spout.enabled = enabled;
		}
		if let Some(name) = self.name {
			spout.name = name;
		}
		if let Some(width) = self.width {
			spout.width = Some(width);
		}
		if let Some(height) = self.height {
			spout.height = Some(height);
		}
	}
}

impl RenderQualityManifest {
	fn apply_to(self, opts: &mut AvatarWindowOptions) {
		if let Some(aa) = self.aa {
			opts.aa = aa;
		}
		if let Some(texture_resolution_limit) = self.texture_resolution_limit {
			opts.texture_resolution_limit = texture_resolution_limit;
		}
		if let Some(texture_compression) = self.texture_compression {
			opts.texture_compression = texture_compression;
		}
		if let Some(mipmap_filter) = self.mipmap_filter {
			opts.mipmap_filter = mipmap_filter;
		}
		if let Some(render_backend) = self.render_backend {
			opts.render_backend = render_backend;
		}
		if let Some(block_compression_encoder) = self.block_compression_encoder {
			opts.block_compression_encoder = block_compression_encoder;
		}
		if let Some(block_compression_cpu_threads) = self.block_compression_cpu_threads {
			opts.block_compression_cpu_threads = block_compression_cpu_threads.max(1);
		}
		if let Some(texture_compression_advanced) = self.texture_compression_advanced {
			opts.texture_compression_advanced = texture_compression_advanced;
		}
		if let Some(processed_texture_cache) = self.processed_texture_cache {
			opts.processed_texture_cache = processed_texture_cache;
		}
		if let Some(skin_tone_matching) = self.skin_tone_matching {
			opts.skin_tone_matching = skin_tone_matching;
		}
	}
}

impl MotionManifest {
	fn apply_to(self, opts: &mut AvatarWindowOptions) {
		if let Some(vmc_udp) = self.vmc_udp {
			vmc_udp.apply_to(opts);
		}
		if let Some(unmotion_zenoh) = self.unmotion_zenoh {
			unmotion_zenoh.apply_to(opts);
		}
		if let Some(look_at) = self.look_at {
			look_at.apply_to(opts);
		}
		if let Some(v) = self.apply_vmc_root_translation {
			opts.apply_vmc_root_translation = v;
		}
		if let Some(primary) = self.primary_source {
			opts.primary_motion_source = primary;
		}
	}
}

impl AudioLinkManifest {
	fn apply_to(self, opts: &mut AvatarWindowOptions) {
		if let Some(source) = self.source {
			opts.audio_link.source = source;
		}
		if let Some(device_id) = self.input_device_id {
			let trimmed = device_id.trim();
			opts.audio_link.input_device_id = (!trimmed.is_empty()).then(|| trimmed.to_string());
		}
		if let Some(name_hint) = self.input_device_name_hint {
			let trimmed = name_hint.trim();
			opts.audio_link.input_device_name_hint = (!trimmed.is_empty()).then(|| trimmed.to_string());
		}
	}
}

impl PhysicsManifest {
	fn apply_to(self, opts: &mut AvatarWindowOptions) {
		if let Some(bone_colliders) = self.bone_colliders {
			bone_colliders.apply_to(&mut opts.bone_colliders);
		}
		if let Some(contacts) = self.contacts {
			contacts.apply_to(opts);
		}
		if let Some(dynamics) = self.dynamics {
			dynamics.apply_to(opts);
		}
		if let Some(spring_bone) = self.spring_bone {
			opts.spring_bone_physics = spring_bone.normalized();
		}
	}
}

impl DynamicsPhysicsManifest {
	fn apply_to(self, opts: &mut AvatarWindowOptions) {
		if let Some(enabled) = self.enable_all_on_launch {
			opts.dynamics_enable_all_on_launch = enabled;
		}
	}
}

impl ContactsPhysicsManifest {
	fn apply_to(self, opts: &mut AvatarWindowOptions) {
		if let Some(enabled) = self.parameter_emission.or(self.parameter_emission_enabled) {
			opts.contact_parameter_emission = enabled;
		}
	}
}

impl BoneCollidersManifest {
	fn apply_to(self, config: &mut un_avatar_skeleton::BoneColliderConfig) {
		if let Some(enabled) = self.enabled {
			config.enabled = enabled;
		}
		if let Some(radius_mm) = self.radius_mm {
			radius_mm.apply_to(&mut config.radius_mm);
		}
	}
}

impl BoneColliderRadiiMmManifest {
	fn apply_to(self, radius_mm: &mut un_avatar_skeleton::BoneColliderPartRadiiMm) {
		if let Some(value) = self.head {
			radius_mm.head = clamp_collider_radius_mm(value);
		}
		if let Some(value) = self.neck_chest {
			radius_mm.neck_chest = clamp_collider_radius_mm(value);
		}
		if let Some(value) = self.torso {
			radius_mm.torso = clamp_collider_radius_mm(value);
		}
		if let Some(value) = self.upper_arms {
			radius_mm.upper_arms = clamp_collider_radius_mm(value);
		}
		if let Some(value) = self.lower_arms {
			radius_mm.lower_arms = clamp_collider_radius_mm(value);
		}
		if let Some(value) = self.hands {
			radius_mm.hands = clamp_collider_radius_mm(value);
		}
	}
}

fn clamp_collider_radius_mm(value: f32) -> f32 {
	if value.is_finite() {
		value.clamp(0.0, 1000.0)
	} else {
		0.0
	}
}

impl LookAtManifest {
	fn apply_to(self, opts: &mut AvatarWindowOptions) {
		let enabled = self.enabled;
		let deg = self.clamp_deg;
		match (enabled, deg) {
			(Some(false), _) => opts.eye_look_at_clamp_deg = None,
			(Some(true), Some(d)) => opts.eye_look_at_clamp_deg = Some(d),
			(Some(true), None) => opts.eye_look_at_clamp_deg = Some(opts.eye_look_at_clamp_deg.unwrap_or(30.0)),
			(None, Some(d)) => opts.eye_look_at_clamp_deg = Some(d),
			(None, None) => {}
		}
	}
}

impl VmcUdpManifest {
	fn apply_to(self, opts: &mut AvatarWindowOptions) {
		if let Some(false) = self.enabled {
			opts.vmc_address = None;
			return;
		}
		if self.enabled.unwrap_or(false) {
			opts.vmc_address = Some(self.address.unwrap_or_else(|| vmc_addr_from_port(self.port.unwrap_or(39539))));
		} else if let Some(address) = self.address {
			opts.vmc_address = Some(address);
		} else if let Some(port) = self.port {
			opts.vmc_address = Some(vmc_addr_from_port(port));
		}
	}
}

fn vmc_addr_from_port(port: u16) -> SocketAddr {
	SocketAddr::from(([0, 0, 0, 0], port))
}

impl OutputManifest {
	fn apply_to(self, spout: &mut SpoutWindowOptions) {
		if let Some(spout2) = self.spout2 {
			spout2.apply_to(spout);
		}
	}
}

impl DebugManifest {
	#[allow(clippy::too_many_arguments)]
	fn apply_to(
		self,
		debug: &mut WindowDebugOptions,
		disable_expression_morphs: &mut bool,
		disable_vmc_eye_look: &mut bool,
		simple_basecolor_only: &mut bool,
		debug_material_dump: &mut bool,
		show_axes: &mut bool,
		show_bone_colliders: &mut bool,
		disable_mtoon_outlines: &mut bool,
		debug_disable_rim_lighting: &mut bool,
		debug_force_shading_shift_zero: &mut bool,
		debug_disable_matcap: &mut bool,
		debug_disable_emissive: &mut bool,
		debug_disable_shade_color: &mut bool,
		debug_disable_normal_map: &mut bool,
		debug_base_texture_only: &mut bool,
	) {
		if let Some(path) = self.log_path {
			debug.log_path = Some(path);
		}
		if let Some(mirror_stderr) = self.mirror_stderr {
			debug.mirror_stderr = mirror_stderr;
		}
		if let Some(vmc) = self.vmc {
			debug.vmc = vmc;
		}
		if let Some(scene) = self.scene {
			debug.scene = scene;
		}
		if let Some(morph) = self.morph {
			debug.morph = morph;
		}
		if let Some(value) = self.disable_expression_morphs {
			*disable_expression_morphs = value;
		}
		if let Some(value) = self.disable_vmc_eye_look {
			*disable_vmc_eye_look = value;
		}
		if let Some(value) = self.simple_basecolor_only {
			*simple_basecolor_only = value;
		}
		if let Some(value) = self.material_dump {
			*debug_material_dump = value;
		}
		if let Some(value) = self.show_axes {
			*show_axes = value;
		}
		if let Some(value) = self.show_bone_colliders {
			*show_bone_colliders = value;
		}
		if let Some(value) = self.disable_mtoon_outlines {
			*disable_mtoon_outlines = value;
		}
		if let Some(value) = self.disable_rim_lighting {
			*debug_disable_rim_lighting = value;
		}
		if let Some(value) = self.force_shading_shift_zero {
			*debug_force_shading_shift_zero = value;
		}
		if let Some(value) = self.disable_matcap {
			*debug_disable_matcap = value;
		}
		if let Some(value) = self.disable_emissive {
			*debug_disable_emissive = value;
		}
		if let Some(value) = self.disable_shade_color {
			*debug_disable_shade_color = value;
		}
		if let Some(value) = self.disable_normal_map {
			*debug_disable_normal_map = value;
		}
		if let Some(value) = self.base_texture_only {
			*debug_base_texture_only = value;
		}
	}
}

impl MeshDiagnosticsManifest {
	fn apply_to(self, diagnostics: &mut SceneMeshLoadOpts) {
		if let Some(value) = self.bind_pose {
			diagnostics.debug_bind_pose = value;
		}
		if let Some(value) = self.primitive_colors {
			diagnostics.debug_primitive_colors = value;
		}
		if let Some(value) = self.zero_morphs {
			diagnostics.debug_zero_morphs = value;
		}
		if let Some(value) = self.relax_iris_alpha {
			diagnostics.relax_iris_alpha = value;
		}
		if let Some(value) = self.skin_legacy_no_inv_mesh {
			diagnostics.debug_skin_legacy_no_inv_mesh = value;
		}
		if let Some(value) = self.disable_reflection {
			diagnostics.debug_disable_reflection = value;
		}
		if let Some(value) = self.disable_fur {
			diagnostics.disable_fur = value;
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn toml_manifest_applies_renderer_options() {
		let manifest: RendererManifest = toml::from_str(
			r#"
title = "Manifest Title"
avatar_path = "target/tmp/model1.vrm"
wardrobe_set = "noble1"
icon_path = "assets/brand/un-avatar-artwork-renderer.png"
vmc_address = "127.0.0.1:39539"
transparent = true
input_passthrough = true
clear_color = [0.0, 0.0, 0.0, 0.0]

[render_quality]
aa = "fxaa"
texture_resolution_limit = "4k"
texture_compression = "balanced"
mipmap_filter = "lanczos3"
processed_texture_cache = false
skin_tone_matching = true

[render_quality.texture_compression_advanced]
face = "source"
eyes = "source"
clothing = "high_quality"
normal = "gpu_native"
occlusion = "gpu_native"
emissive = "high_quality"
generic_color = "auto"
data = "source"

[motion.vmc_udp]
enabled = true
port = 39540
address = "0.0.0.0:39541"

[audio_link]
source = "input_device"
input_device_id = "cpal:device-1"
input_device_name_hint = "Main Mix"

[spout]
enabled = false
name = "UN Avatar Spout"
width = 1280
height = 720

[output.spout2]
enabled = true
name = "UN Avatar Spout2"
width = 1920
height = 1080

[debug]
vmc = true
scene = true
disable_expression_morphs = true

[diagnostics]
zero_morphs = true
relax_iris_alpha = true

[environment.color]
exposure = 0.35
contrast = 1.15
saturation = 0.9
look = "film"
intensity = 0.45
temperature = 0.2
tint = -0.15

[effects.avatar.outline]
policy = "override"
type = "mtoon"
width = 0.004
color = [0.02, 0.01, 0.03]
lighting_mix = 0.25
roundness = 0.5

[effects.post.bloom]
enabled = true
strength = 0.4
threshold = 0.9
radius = 12.0
quality = "high_quality"

[effects.post.ssao]
enabled = true
strength = 0.25
radius = 4.0
bias = 0.001
range = 0.03

[effects.avatar.matcap]
scale = 1.35

[effects.avatar.specular]
enabled = true
intensity = 0.5
power = 32.0

[effects.avatar.ambient_occlusion]
strength = 1.4

[effects.avatar.contact_shadow]
enabled = true
strength = 0.4
radius = 0.7
softness = 2.0
height = 0.02

[physics.bone_colliders]
enabled = true

[physics.bone_colliders.parts]
head = 1.0

[physics.bone_colliders.radius_mm]
head = 180
hands = 70

[physics.contacts]
parameter_emission = true

[physics.dynamics]
enable_all_on_launch = true

[physics.spring_bone]
simulation_hz = 240
substeps = 2

[[physics.spring_bone.categories]]
id = "ears"
name = "Ears"
matches = ["ears", "耳", "ミミ"]

[[physics.spring_bone.overrides]]
category = "ears"
solver = "xpbd"
damping_half_life_ms = 90
xpbd_compliance = 0.02
constraint_iterations = 6
"#,
		)
		.unwrap();
		let mut opts = AvatarWindowOptions::default();
		manifest.apply_to(&mut opts);

		assert_eq!(opts.title, "Manifest Title");
		assert_eq!(opts.gltf_path.as_deref(), Some(std::path::Path::new("target/tmp/model1.vrm")));
		assert_eq!(opts.wardrobe_set.as_deref(), Some("noble1"));
		assert_eq!(
			opts.icon_path.as_deref(),
			Some(std::path::Path::new("assets/brand/un-avatar-artwork-renderer.png"))
		);
		assert_eq!(opts.vmc_address, Some("0.0.0.0:39541".parse().unwrap()));
		assert_eq!(opts.audio_link.source, AudioLinkSource::InputDevice);
		assert_eq!(opts.audio_link.input_device_id.as_deref(), Some("cpal:device-1"));
		assert_eq!(opts.audio_link.input_device_name_hint.as_deref(), Some("Main Mix"));
		assert!(opts.transparent);
		assert!(opts.input_passthrough);
		assert_eq!(opts.clear_color.a, 0.0);
		assert_eq!(opts.aa, AaMode::Fxaa);
		assert_eq!(opts.texture_resolution_limit, TextureResolutionLimit::K4);
		assert_eq!(opts.texture_compression, TextureCompressionMode::Balanced);
		assert_eq!(opts.mipmap_filter, TextureMipmapFilter::Lanczos3);
		assert_eq!(
			opts.texture_compression_advanced.clothing,
			crate::TextureCompressionPreference::HighQuality
		);
		assert_eq!(
			opts.texture_compression_advanced.normal,
			crate::TextureCompressionPreference::GpuNative
		);
		assert!(!opts.processed_texture_cache);
		assert!(opts.skin_tone_matching);
		assert!(opts.spout.enabled);
		assert_eq!(opts.spout.name, "UN Avatar Spout2");
		assert_eq!(opts.spout.width, Some(1920));
		assert_eq!(opts.spout.height, Some(1080));
		assert!(opts.debug.vmc);
		assert!(opts.debug.scene);
		assert!(opts.disable_expression_morphs);
		assert_eq!(opts.environment_color.exposure, 0.35);
		assert_eq!(opts.environment_color.contrast, 1.15);
		assert_eq!(opts.environment_color.saturation, 0.9);
		assert_eq!(opts.environment_color.look, ColorGradingLook::Film);
		assert_eq!(opts.environment_color.look_intensity, 0.45);
		assert_eq!(opts.environment_color.temperature, 0.2);
		assert_eq!(opts.environment_color.tint, -0.15);
		assert!(opts.mesh_diagnostics.debug_zero_morphs);
		assert!(opts.mesh_diagnostics.relax_iris_alpha);
		assert_eq!(opts.mesh_diagnostics.avatar_outline.policy, AvatarOutlinePolicy::Override);
		assert_eq!(opts.mesh_diagnostics.avatar_outline.kind, AvatarOutlineKind::Mtoon);
		assert_eq!(opts.mesh_diagnostics.avatar_outline.width, Some(0.004));
		assert_eq!(opts.mesh_diagnostics.avatar_outline.color, Some([0.02, 0.01, 0.03]));
		assert_eq!(opts.mesh_diagnostics.avatar_outline.lighting_mix, Some(0.25));
		assert_eq!(opts.mesh_diagnostics.avatar_outline.roundness, Some(0.5));
		assert_eq!(opts.mesh_diagnostics.avatar_matcap.scale, 1.35);
		assert!(opts.mesh_diagnostics.avatar_specular.enabled);
		assert_eq!(opts.mesh_diagnostics.avatar_specular.intensity, 0.5);
		assert_eq!(opts.mesh_diagnostics.avatar_specular.power, 32.0);
		assert_eq!(opts.mesh_diagnostics.avatar_ambient_occlusion.strength, 1.4);
		assert!(opts.contact_shadow.enabled);
		assert_eq!(opts.contact_shadow.strength, 0.4);
		assert_eq!(opts.contact_shadow.radius, 0.7);
		assert_eq!(opts.contact_shadow.softness, 2.0);
		assert_eq!(opts.contact_shadow.height, 0.02);
		assert!(opts.bloom.enabled);
		assert_eq!(opts.bloom.strength, 0.4);
		assert_eq!(opts.bloom.threshold, 0.9);
		assert_eq!(opts.bloom.radius, 12.0);
		assert_eq!(opts.bloom.quality, BloomQuality::HighQuality);
		assert!(opts.ssao.enabled);
		assert_eq!(opts.ssao.strength, 0.25);
		assert_eq!(opts.ssao.radius, 4.0);
		assert_eq!(opts.ssao.bias, 0.001);
		assert_eq!(opts.ssao.range, 0.03);
		assert!(opts.bone_colliders.enabled);
		assert_eq!(opts.bone_colliders.radius_mm.head, 180.0);
		assert_eq!(opts.bone_colliders.radius_mm.hands, 70.0);
		assert_eq!(opts.bone_colliders.radius_mm.torso, 140.0);
		assert!(opts.contact_parameter_emission);
		assert!(opts.dynamics_enable_all_on_launch);
		assert_eq!(opts.spring_bone_physics.simulation_hz, 240.0);
		assert_eq!(opts.spring_bone_physics.substeps, 2);
		assert_eq!(opts.spring_bone_physics.categories[0].id, "ears");
		assert_eq!(opts.spring_bone_physics.categories[0].matches, vec!["ears", "耳", "ミミ"]);
		assert_eq!(opts.spring_bone_physics.overrides[0].category, "ears");
		assert_eq!(
			opts.spring_bone_physics.overrides[0].params.solver,
			Some(un_avatar_skeleton::SpringBoneSolver::Xpbd)
		);
		assert_eq!(opts.spring_bone_physics.overrides[0].params.damping_half_life_ms, Some(90.0));
		assert_eq!(opts.spring_bone_physics.overrides[0].params.xpbd_compliance, Some(0.02));
		assert_eq!(opts.spring_bone_physics.overrides[0].params.constraint_iterations, Some(6));
	}

	#[test]
	fn toml_manifest_applies_root_aa_mode() {
		let manifest: RendererManifest = toml::from_str(
			r#"
aa = "msaa"
"#,
		)
		.unwrap();
		let mut opts = AvatarWindowOptions::default();
		manifest.apply_to(&mut opts);

		assert_eq!(opts.aa, AaMode::Msaa);
	}
}
