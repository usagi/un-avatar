import type {
	CameraTargetPreset,
	LookRecommendation,
	OutputModePreset,
	ProfileSettingValue,
	PreviewWindowPreset,
	RenderQualityRecommendation,
	SpoutResolutionPreset,
} from "./profileTypes";

export type ProfilePresetUpdate = [field: string, value: ProfileSettingValue];

const DEFAULT_TEXTURE_COMPRESSION_ADVANCED_UPDATES: readonly ProfilePresetUpdate[] = [
	["render_quality.texture_compression_advanced.face", "source"],
	["render_quality.texture_compression_advanced.eyes", "source"],
	["render_quality.texture_compression_advanced.clothing", "auto"],
	["render_quality.texture_compression_advanced.normal", "gpu_native"],
	["render_quality.texture_compression_advanced.occlusion", "gpu_native"],
	["render_quality.texture_compression_advanced.emissive", "high_quality"],
	["render_quality.texture_compression_advanced.generic_color", "auto"],
	["render_quality.texture_compression_advanced.data", "source"],
];

export const RENDER_QUALITY_RECOMMENDATIONS: Record<RenderQualityRecommendation, readonly ProfilePresetUpdate[]> = {
	light: [
		["render_quality.aa", "fxaa"],
		["render_quality.texture_resolution_limit", "2k"],
		["render_quality.texture_compression", "memory"],
		["render_quality.mipmap_filter", "bilinear"],
		["render_quality.processed_texture_cache", true],
		...DEFAULT_TEXTURE_COMPRESSION_ADVANCED_UPDATES,
	],
	balanced: [
		["render_quality.aa", "smaa"],
		["render_quality.texture_resolution_limit", "auto"],
		["render_quality.texture_compression", "balanced"],
		["render_quality.mipmap_filter", "mitchell"],
		["render_quality.processed_texture_cache", true],
		...DEFAULT_TEXTURE_COMPRESSION_ADVANCED_UPDATES,
	],
	quality: [
		["render_quality.aa", "smaa"],
		["render_quality.texture_resolution_limit", "off"],
		["render_quality.texture_compression", "balanced"],
		["render_quality.mipmap_filter", "mitchell"],
		["render_quality.processed_texture_cache", true],
		...DEFAULT_TEXTURE_COMPRESSION_ADVANCED_UPDATES,
		["render_quality.texture_compression_advanced.clothing", "high_quality"],
		["render_quality.texture_compression_advanced.generic_color", "high_quality"],
	],
};

export const LOOK_RECOMMENDATIONS: Record<LookRecommendation, readonly ProfilePresetUpdate[]> = {
	natural: [
		["effects.avatar.outline.policy", "authored"],
		["effects.avatar.contact_shadow.enabled", false],
		["effects.post.ssao.enabled", false],
		["environment.color.look", "neutral"],
		["environment.color.intensity", 0],
		["environment.color.exposure", 0],
		["environment.color.contrast", 1],
		["environment.color.saturation", 1],
		["effects.post.bloom.enabled", false],
	],
	clear: [
		["effects.avatar.outline.policy", "override"],
		["effects.avatar.outline.width", 0.0035],
		["effects.avatar.contact_shadow.enabled", true],
		["effects.avatar.contact_shadow.strength", 0.24],
		["effects.post.ssao.enabled", false],
		["environment.color.look", "neutral"],
		["environment.color.contrast", 1.05],
		["environment.color.saturation", 1.03],
		["effects.post.bloom.enabled", false],
	],
	pop: [
		["effects.avatar.outline.policy", "override"],
		["effects.avatar.outline.width", 0.0045],
		["effects.avatar.contact_shadow.enabled", true],
		["environment.color.look", "pop"],
		["environment.color.intensity", 0.45],
		["environment.color.contrast", 1.12],
		["environment.color.saturation", 1.12],
		["effects.post.bloom.enabled", true],
		["effects.post.bloom.quality", "compact"],
		["effects.post.bloom.strength", 0.12],
	],
	soft: [
		["effects.avatar.outline.policy", "authored"],
		["effects.avatar.contact_shadow.enabled", true],
		["effects.avatar.contact_shadow.strength", 0.2],
		["effects.post.ssao.enabled", true],
		["effects.post.ssao.strength", 0.12],
		["environment.color.look", "soft"],
		["environment.color.intensity", 0.4],
		["environment.color.contrast", 0.96],
		["effects.post.bloom.enabled", true],
		["effects.post.bloom.strength", 0.08],
	],
};

export const CAMERA_TARGET_PRESETS: Record<CameraTargetPreset, readonly ProfilePresetUpdate[]> = {
	face: [["camera.target_y", 1.42]],
	neck: [["camera.target_y", 1.25]],
	chest: [["camera.target_y", 1.05]],
};

export const SPOUT_RESOLUTION_PRESETS: Record<SpoutResolutionPreset, readonly ProfilePresetUpdate[]> = {
	"720p": [
		["output.spout2.width", 1280],
		["output.spout2.height", 720],
	],
	"1080p": [
		["output.spout2.width", 1920],
		["output.spout2.height", 1080],
	],
	"1440p": [
		["output.spout2.width", 2560],
		["output.spout2.height", 1440],
	],
	"4k": [
		["output.spout2.width", 3840],
		["output.spout2.height", 2160],
	],
};

export const OUTPUT_MODE_PRESETS: Record<OutputModePreset, readonly ProfilePresetUpdate[]> = {
	window_preview: [
		["output.spout2.enabled", false],
		["window.minimized", false],
	],
	spout2_preview: [
		["output.spout2.enabled", true],
		["window.minimized", false],
	],
	spout2_only: [
		["output.spout2.enabled", true],
		["window.minimized", true],
	],
};

export const PREVIEW_WINDOW_PRESETS: Record<PreviewWindowPreset, readonly ProfilePresetUpdate[]> = {
	compact: [
		["window.width", 640],
		["window.height", 360],
	],
	half_hd: [
		["window.width", 960],
		["window.height", 540],
	],
	hd: [
		["window.width", 1280],
		["window.height", 720],
	],
};
