export type ProfileLookSetting = {
	outline_policy: string;
	outline_type: string;
	outline_width: number | null;
	outline_color: [number, number, number] | null;
	outline_lighting_mix: number | null;
	outline_roundness: number | null;
	rim_policy: string;
	rim_color: [number, number, number] | null;
	rim_intensity: number | null;
	rim_lighting_mix: number | null;
	rim_fresnel_power: number | null;
	rim_lift: number | null;
	matcap_scale: number;
	specular_enabled: boolean;
	specular_intensity: number;
	specular_power: number;
	ambient_occlusion_strength: number;
	contact_shadow_enabled: boolean;
	contact_shadow_strength: number;
	contact_shadow_radius: number;
	contact_shadow_softness: number;
	contact_shadow_height: number;
	ssao_enabled: boolean;
	ssao_strength: number;
	ssao_radius: number;
	ssao_bias: number;
	ssao_range: number;
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
};

export type ProfileOutlineSetting = Pick<
	ProfileLookSetting,
	"outline_policy" | "outline_type" | "outline_width" | "outline_color" | "outline_lighting_mix" | "outline_roundness"
>;

export type ProfileRimSetting = Pick<
	ProfileLookSetting,
	"rim_policy" | "rim_color" | "rim_intensity" | "rim_lighting_mix" | "rim_fresnel_power" | "rim_lift"
>;

export type ProfileSurfaceSetting = Pick<
	ProfileLookSetting,
	"matcap_scale" | "specular_enabled" | "specular_intensity" | "specular_power" | "ambient_occlusion_strength"
>;

export type ProfileShadowsSetting = Pick<
	ProfileLookSetting,
	| "contact_shadow_enabled"
	| "contact_shadow_strength"
	| "contact_shadow_radius"
	| "contact_shadow_softness"
	| "contact_shadow_height"
	| "ssao_enabled"
	| "ssao_strength"
	| "ssao_radius"
	| "ssao_bias"
	| "ssao_range"
>;

export type ProfileColorGradingSetting = Pick<
	ProfileLookSetting,
	"color_exposure" | "color_contrast" | "color_saturation" | "color_look" | "color_look_intensity" | "color_temperature" | "color_tint"
>;

export type ProfileBloomSetting = Pick<
	ProfileLookSetting,
	"bloom_enabled" | "bloom_strength" | "bloom_threshold" | "bloom_radius" | "bloom_quality"
>;
