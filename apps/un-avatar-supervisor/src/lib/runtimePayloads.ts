export type RuntimeWindowSetting = {
	decorations: boolean;
	transparent: boolean;
	input_passthrough: boolean;
	always_on_top: boolean;
	window_width: number;
	window_height: number;
};

export type RuntimeBackgroundColorSetting = {
	background_color: [number, number, number];
	transparent: boolean;
};

export type RuntimeMotionSetting = {
	apply_vmc_root_translation: boolean;
	look_at_enabled: boolean;
	look_at_clamp_deg: number | null;
	motion_vmc_enabled: boolean;
	vmc_address: string | null;
	vmc_port: number | null;
	motion_unmotion_enabled: boolean;
	unmotion_zenoh_key: string | null;
};

export type RuntimeAvatarEffectsSetting = {
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
};

export type RuntimePostEffectsSetting = {
	ssao_enabled: boolean;
	ssao_strength: number;
	ssao_radius: number;
	ssao_bias: number;
	ssao_range: number;
	bloom_enabled: boolean;
	bloom_strength: number;
	bloom_threshold: number;
	bloom_radius: number;
	bloom_quality: string;
};

export type RuntimeEnvironmentSetting = {
	color_exposure: number;
	color_contrast: number;
	color_saturation: number;
	color_look: string;
	color_look_intensity: number;
	color_temperature: number;
	color_tint: number;
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
};

export function rendererWindowPayload(setting: RuntimeWindowSetting) {
	return {
		decorations: setting.decorations,
		transparent: setting.transparent,
		inputPassthrough: setting.input_passthrough,
		alwaysOnTop: setting.always_on_top,
		width: setting.window_width,
		height: setting.window_height,
	};
}

export function rendererClearColorPayload(setting: RuntimeBackgroundColorSetting) {
	return {
		r: setting.background_color[0],
		g: setting.background_color[1],
		b: setting.background_color[2],
		a: setting.transparent ? 0 : 1,
	};
}

export function rendererLookAtPayload(setting: RuntimeMotionSetting) {
	return {
		enabled: setting.look_at_enabled,
		clampDeg: setting.look_at_enabled ? (setting.look_at_clamp_deg ?? 30) : null,
	};
}

export function rendererMotionReceiversPayload(setting: RuntimeMotionSetting) {
	return {
		vmcAddress: setting.motion_vmc_enabled ? (setting.vmc_address ?? `0.0.0.0:${setting.vmc_port ?? 39539}`) : null,
		unmotionZenohEnabled: setting.motion_unmotion_enabled,
		unmotionZenohKey: setting.unmotion_zenoh_key ?? "un-motion/frame",
	};
}

export function rendererAvatarOutlinePayload(setting: RuntimeAvatarEffectsSetting) {
	return {
		policy: setting.outline_policy,
		outlineType: setting.outline_type,
		width: setting.outline_width,
		color: setting.outline_color,
		lightingMix: setting.outline_lighting_mix,
		roundness: setting.outline_roundness,
	};
}

export function rendererAvatarRimPayload(setting: RuntimeAvatarEffectsSetting) {
	return {
		policy: setting.rim_policy,
		color: setting.rim_color,
		intensity: setting.rim_intensity,
		lightingMix: setting.rim_lighting_mix,
		fresnelPower: setting.rim_fresnel_power,
		lift: setting.rim_lift,
	};
}

export function rendererAvatarSpecularPayload(setting: RuntimeAvatarEffectsSetting) {
	return {
		enabled: setting.specular_enabled,
		intensity: setting.specular_intensity,
		power: setting.specular_power,
	};
}

export function rendererContactShadowPayload(setting: RuntimeAvatarEffectsSetting) {
	return {
		enabled: setting.contact_shadow_enabled,
		strength: setting.contact_shadow_strength,
		radius: setting.contact_shadow_radius,
		softness: setting.contact_shadow_softness,
		height: setting.contact_shadow_height,
	};
}

export function rendererSsaoPayload(setting: RuntimePostEffectsSetting) {
	return {
		enabled: setting.ssao_enabled,
		strength: setting.ssao_strength,
		radius: setting.ssao_radius,
		bias: setting.ssao_bias,
		range: setting.ssao_range,
	};
}

export function rendererBloomPayload(setting: RuntimePostEffectsSetting) {
	return {
		enabled: setting.bloom_enabled,
		strength: setting.bloom_strength,
		threshold: setting.bloom_threshold,
		radius: setting.bloom_radius,
		quality: setting.bloom_quality,
	};
}

export function rendererEnvironmentColorPayload(setting: RuntimeEnvironmentSetting) {
	return {
		exposure: setting.color_exposure,
		contrast: setting.color_contrast,
		saturation: setting.color_saturation,
		look: setting.color_look,
		intensity: setting.color_look_intensity,
		temperature: setting.color_temperature,
		tint: setting.color_tint,
	};
}

export function rendererLightingPayload(setting: RuntimeEnvironmentSetting) {
	return {
		environmentEnabled: setting.lighting_environment_enabled,
		environmentColor: setting.lighting_environment_color,
		environmentIntensity: setting.lighting_environment_intensity,
		directionalEnabled: setting.lighting_directional_enabled,
		directionalColor: setting.lighting_directional_color,
		directionalIntensity: setting.lighting_directional_intensity,
		directionalAzimuthDeg: setting.lighting_directional_azimuth_deg,
		directionalElevationDeg: setting.lighting_directional_elevation_deg,
		directionalFollowCameraYaw: setting.lighting_directional_follow_camera_yaw,
		directionalFollowCameraPitch: setting.lighting_directional_follow_camera_pitch,
	};
}
