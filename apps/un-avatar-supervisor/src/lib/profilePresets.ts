import type {
  CameraTargetPreset,
  LookRecommendation,
  ProfileSettingValue,
  RenderQualityRecommendation,
  SpoutResolutionPreset,
} from "./profileTypes";

export type ProfilePresetUpdate = [
  field: string,
  value: ProfileSettingValue,
];

export const RENDER_QUALITY_RECOMMENDATIONS: Record<
  RenderQualityRecommendation,
  readonly ProfilePresetUpdate[]
> = {
  light: [
    ["render_quality.aa", "fxaa"],
    ["render_quality.texture_resolution_limit", "2k"],
    ["render_quality.texture_compression", "memory"],
    ["render_quality.mipmap_filter", "bilinear"],
    ["render_quality.processed_texture_cache", true],
  ],
  balanced: [
    ["render_quality.aa", "smaa"],
    ["render_quality.texture_resolution_limit", "auto"],
    ["render_quality.texture_compression", "balanced"],
    ["render_quality.mipmap_filter", "mitchell"],
    ["render_quality.processed_texture_cache", true],
  ],
  quality: [
    ["render_quality.aa", "smaa"],
    ["render_quality.texture_resolution_limit", "off"],
    ["render_quality.texture_compression", "balanced"],
    ["render_quality.mipmap_filter", "mitchell"],
    ["render_quality.processed_texture_cache", true],
  ],
};

export const LOOK_RECOMMENDATIONS: Record<
  LookRecommendation,
  readonly ProfilePresetUpdate[]
> = {
  natural: [
    ["effects.avatar.outline.policy", "authored"],
    ["effects.avatar.rim.policy", "authored"],
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
    ["effects.avatar.rim.policy", "override"],
    ["effects.avatar.rim.intensity", 0.25],
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
    ["effects.avatar.rim.policy", "override"],
    ["effects.avatar.rim.intensity", 0.18],
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

export const CAMERA_TARGET_PRESETS: Record<
  CameraTargetPreset,
  readonly ProfilePresetUpdate[]
> = {
  face: [["camera.target_y", 1.42]],
  neck: [["camera.target_y", 1.25]],
  chest: [["camera.target_y", 1.05]],
};

export const SPOUT_RESOLUTION_PRESETS: Record<
  SpoutResolutionPreset,
  readonly ProfilePresetUpdate[]
> = {
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
