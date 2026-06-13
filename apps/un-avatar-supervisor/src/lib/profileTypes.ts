import type { SettingSummaryLabelData } from "./profileLabels";
import type { LightingDiagramSource } from "./profileDiagrams";
import type { CameraDiagramSource } from "./profileDiagrams";
import type { PrimaryMotionSource } from "./rendererTypes";
import type { DynamicsCategoryOverrideSetting } from "./dynamicsPresets";

export type ProfileSettingValue = boolean | string | number | string[] | number[] | null;

export type TextureCompressionPreference = "source" | "auto" | "high_quality" | "small" | "gpu_native";

export type TextureCompressionMode = "source" | "balanced" | "memory" | "compat";

export type TextureCompressionAdvanced = {
	face: TextureCompressionPreference;
	eyes: TextureCompressionPreference;
	clothing: TextureCompressionPreference;
	normal: TextureCompressionPreference;
	occlusion: TextureCompressionPreference;
	emissive: TextureCompressionPreference;
	generic_color: TextureCompressionPreference;
	data: TextureCompressionPreference;
};

export type ProfileSectionId = "identity" | "avatar" | "quality" | "lighting" | "look" | "window" | "camera" | "motion" | "output";

export type RenderQualityRecommendation = "light" | "balanced" | "quality";
export type LookRecommendation = "natural" | "clear" | "pop" | "soft";
export type CameraTargetPreset = "face" | "neck" | "chest";
export type CameraLensPreset = 10 | 16 | 35 | 70 | 200;
export type SpoutResolutionPreset = "720p" | "1080p" | "1440p" | "4k";
export type OutputModePreset = "window_preview" | "spout2_preview" | "spout2_only";
export type PreviewWindowPreset = "compact" | "half_hd" | "hd";
export type AudioLinkSource = "none" | "input_device";

export type QualitySetting = {
	aa: string;
	texture_resolution_limit: string;
	texture_compression: TextureCompressionMode;
	mipmap_filter: string;
	render_backend: string;
	block_compression_encoder: string;
	block_compression_cpu_threads: number;
	processed_texture_cache: boolean;
	skin_tone_matching: boolean;
	texture_compression_advanced: TextureCompressionAdvanced;
	debug_disable_rim_lighting: boolean;
	debug_force_shading_shift_zero: boolean;
	debug_disable_matcap: boolean;
	debug_disable_emissive: boolean;
	debug_disable_shade_color: boolean;
	debug_disable_normal_map: boolean;
	debug_base_texture_only: boolean;
};

export type LightingSetting = LightingDiagramSource & {
	lighting_environment_enabled: boolean;
	lighting_environment_color: [number, number, number];
	lighting_environment_intensity: number;
	lighting_directional_enabled: boolean;
	lighting_directional_color: [number, number, number];
	lighting_directional_intensity: number;
};

export type CameraSetting = CameraDiagramSource & {
	camera_locked: boolean;
	camera_target: [number, number, number] | null;
	camera_longitude_deg: number | null;
	camera_latitude_deg: number | null;
	camera_radius: number | null;
	camera_diagonal_fov_deg: number | null;
	window_width: number | null;
	window_height: number | null;
};

export type ProfileLaunchSetting = SettingSummaryLabelData & {
	id: string;
	name: string;
	icon_path: string | null;
	avatar_path: string | null;
};

export type RendererReadySetting = Pick<
	AvatarSetting,
	"name" | "icon_path" | "avatar_path" | "render_backend" | "texture_resolution_limit"
> &
	SettingSummaryLabelData;

export type AvatarSetting = ProfileLaunchSetting & {
	created_at: string;
	sort_order: number;
	storage: "seed" | "user";
	manifest_path: string;
	wardrobe_set: string | null;
	motion_vmc_enabled: boolean;
	motion_unmotion_enabled: boolean;
	unmotion_zenoh_key: string | null;
	audio_link_source: AudioLinkSource;
	audio_link_input_device_id: string | null;
	audio_link_input_device_name_hint: string | null;
	look_at_enabled: boolean;
	look_at_clamp_deg: number | null;
	primary_motion_source: PrimaryMotionSource;
	spring_bones: boolean;
	dynamics_enable_all_on_launch: boolean;
	contact_parameter_emission: boolean;
	spring_bone_physics_configured: boolean;
	spring_bone_simulation_hz: number;
	spring_bone_substeps: number;
	spring_bone_category_overrides: DynamicsCategoryOverrideSetting[];
	apply_vmc_root_translation: boolean;
	camera_target: [number, number, number] | null;
	camera_longitude_deg: number | null;
	camera_latitude_deg: number | null;
	camera_radius: number | null;
	camera_diagonal_fov_deg: number | null;
	spout_enabled: boolean;
	spout_name: string | null;
	spout_width: number | null;
	spout_height: number | null;
	aa: string;
	texture_resolution_limit: string;
	texture_compression: TextureCompressionMode;
	mipmap_filter: string;
	render_backend: string;
	block_compression_encoder: string;
	block_compression_cpu_threads: number;
	texture_compression_advanced: TextureCompressionAdvanced;
	processed_texture_cache: boolean;
	skin_tone_matching: boolean;
	background_color: [number, number, number];
	minimized: boolean;
	input_passthrough: boolean;
	show_axes: boolean;
	show_bone_colliders: boolean;
	bone_colliders_enabled: boolean;
	bone_collider_head: number;
	bone_collider_neck_chest: number;
	bone_collider_torso: number;
	bone_collider_upper_arms: number;
	bone_collider_lower_arms: number;
	bone_collider_hands: number;
	debug_disable_mtoon_outlines: boolean;
	debug_disable_rim_lighting: boolean;
	debug_force_shading_shift_zero: boolean;
	debug_disable_matcap: boolean;
	debug_disable_emissive: boolean;
	debug_disable_shade_color: boolean;
	debug_disable_normal_map: boolean;
	debug_base_texture_only: boolean;
	outline_policy: string;
	outline_type: string;
	outline_width: number | null;
	outline_color: [number, number, number] | null;
	outline_lighting_mix: number | null;
	outline_roundness: number | null;
	lighting_environment_enabled: boolean;
	lighting_environment_color: [number, number, number];
	lighting_environment_intensity: number;
	lighting_directional_enabled: boolean;
	lighting_directional_color: [number, number, number];
	lighting_directional_intensity: number;
	lighting_directional_azimuth_deg: number;
	lighting_directional_elevation_deg: number;
	lighting_directional_follow_camera_yaw: boolean;
	lighting_directional_follow_camera_pitch: boolean;
	color_exposure: number;
	color_contrast: number;
	color_saturation: number;
	color_look: string;
	color_look_intensity: number;
	color_temperature: number;
	color_tint: number;
	bloom_enabled: boolean;
	bloom_strength: number;
	bloom_threshold: number;
	bloom_radius: number;
	bloom_quality: string;
	ssao_enabled: boolean;
	ssao_strength: number;
	ssao_radius: number;
	ssao_bias: number;
	ssao_range: number;
	contact_shadow_enabled: boolean;
	contact_shadow_strength: number;
	contact_shadow_radius: number;
	contact_shadow_softness: number;
	contact_shadow_height: number;
	camera_locked: boolean;
	window_width: number;
	window_height: number;
	window_x: number | null;
	window_y: number | null;
	allow_multiple_renderers: boolean;
	notes: string | null;
	group: string;
};

export type SpoutOutputSetting = Pick<
	AvatarSetting,
	"spout_enabled" | "spout_name" | "spout_width" | "spout_height" | "minimized" | "window_width" | "window_height"
>;

export type IdentitySetting = Pick<AvatarSetting, "name" | "group" | "icon_path" | "avatar_path" | "allow_multiple_renderers" | "notes">;

export type WardrobeSetOption = {
	id: string;
	name: string;
};

export type UnavatarWardrobeOptions = {
	available: boolean;
	base_label: string;
	sets: WardrobeSetOption[];
	error?: string | null;
};

export type AvatarFileSetting = Pick<AvatarSetting, "avatar_path" | "wardrobe_set">;

export type WindowSetting = Pick<
	AvatarSetting,
	| "decorations"
	| "transparent"
	| "background_color"
	| "input_passthrough"
	| "always_on_top"
	| "minimized"
	| "show_axes"
	| "show_bone_colliders"
	| "window_width"
	| "window_height"
	| "window_x"
	| "window_y"
>;

export type MotionSetting = Pick<
	AvatarSetting,
	| "motion_unmotion_enabled"
	| "unmotion_zenoh_key"
	| "motion_vmc_enabled"
	| "audio_link_source"
	| "audio_link_input_device_id"
	| "audio_link_input_device_name_hint"
	| "vmc_address"
	| "vmc_port"
	| "look_at_enabled"
	| "look_at_clamp_deg"
	| "spring_bones"
	| "dynamics_enable_all_on_launch"
	| "contact_parameter_emission"
	| "spring_bone_category_overrides"
	| "apply_vmc_root_translation"
	| "bone_colliders_enabled"
	| "bone_collider_head"
	| "bone_collider_neck_chest"
	| "bone_collider_torso"
	| "bone_collider_upper_arms"
	| "bone_collider_lower_arms"
	| "bone_collider_hands"
>;
