import { aaModeLabel, basename } from "./formatting";
import {
	cameraSummaryLabel,
	motionLabel,
	type LightingSummaryLabelData,
	type LookSummaryLabelData,
	type MotionLabelData,
	type OutputLabelData,
	type SettingSummaryLabelData,
	type WindowLabelData,
} from "./profileLabels";
import type { ProfileSummaryItem } from "./profileSummary";
import { type ProfileQualityLabelData, type RuntimeOutputStatusData } from "./runtimeLabels";

type Translate = (key: string, options?: { values?: Record<string, string | number> }) => string;

export type ProfileStageSummarySetting = Parameters<typeof motionLabel>[0] &
	OutputLabelData &
	ProfileQualityLabelData &
	LightingSummaryLabelData &
	LookSummaryLabelData &
	WindowLabelData &
	Parameters<typeof cameraSummaryLabel>[0] & {
		avatar_path: string | null;
	};

export function localizedOutputLabel(setting: OutputLabelData, translate: Translate): string {
	if (!setting.spout_enabled) return translate("profiles.summary.output_window_preview");
	const mode = setting.minimized ? translate("profiles.summary.output_spout_only") : translate("profiles.summary.output_spout_preview");
	const size = setting.spout_width && setting.spout_height ? ` / ${setting.spout_width} x ${setting.spout_height}` : "";
	const name = setting.spout_name ? ` / ${setting.spout_name}` : "";
	return `${mode}${size}${name}`;
}

export function localizedRuntimeOutputLabel(
	renderer: OutputLabelData,
	status: RuntimeOutputStatusData | null | undefined,
	translate: Translate
): string {
	if (!status?.connected) return localizedOutputLabel(renderer, translate);
	if (renderer.spout_enabled && !status.spout_available) return translate("profiles.summary.output_spout_unavailable");
	if (!status.spout_enabled) return translate("profiles.summary.output_window");
	const name = status.spout_name ? ` / ${status.spout_name}` : "";
	const size =
		status.spout_sender_width && status.spout_sender_height ? ` / ${status.spout_sender_width} x ${status.spout_sender_height}` : "";
	return `Spout2${name}${size}`;
}

export function localizedMotionLabel(setting: MotionLabelData, translate: Translate): string {
	const label = motionLabel(setting);
	return label === "None" ? translate("profiles.summary.motion_none") : label;
}

export function localizedAaModeLabel(aa: string | null | undefined, translate: Translate): string {
	return aa === "off" ? translate("profiles.editor.options.aa_off") : aaModeLabel(aa);
}

export function localizedWindowLabel(setting: WindowLabelData, translate: Translate): string {
	const frame = setting.decorations ? translate("profiles.summary.window_framed") : translate("profiles.summary.window_borderless");
	const alpha = setting.transparent ? translate("profiles.summary.window_transparent") : translate("profiles.summary.window_opaque");
	const level = setting.always_on_top ? translate("profiles.summary.window_topmost") : translate("profiles.summary.window_normal");
	const passthrough = setting.input_passthrough ? ` / ${translate("profiles.summary.window_click_through")}` : "";
	return `${frame} / ${alpha} / ${level}${passthrough}`;
}

export function localizedLookLabel(setting: LookSummaryLabelData, translate: Translate): string {
	const colorKey = setting.color_look === "neutral" || setting.color_look_intensity <= 0 ? "neutral" : setting.color_look;
	const color = translate(`profiles.editor.color_look_${colorKey}`);
	const intensity = colorKey === "neutral" ? "" : ` ${Math.round(setting.color_look_intensity * 100)}%`;
	const bloom = setting.bloom_enabled ? translate("profiles.summary.bloom_on") : translate("profiles.summary.bloom_off");
	return `${color}${intensity} / ${bloom}`;
}

export function localizedQualitySummaryLabel(setting: ProfileQualityLabelData, translate: Translate): string {
	const textureLimit =
		setting.texture_resolution_limit === "off" ? translate("profiles.summary.texture_unlimited") : setting.texture_resolution_limit;
	const compression = setting.texture_compression;
	const advanced = localizedTextureCompressionAdvancedSummary(setting.texture_compression_advanced, translate);
	const cache = setting.processed_texture_cache ? translate("profiles.summary.cache_on") : translate("profiles.summary.cache_off");
	return `AA: ${localizedAaModeLabel(setting.aa, translate)} / Tex: ${textureLimit} / ${compression}${advanced} / ${cache}`;
}

function localizedTextureCompressionAdvancedSummary(
	advanced: ProfileQualityLabelData["texture_compression_advanced"],
	translate: Translate
): string {
	const colorBc7 = advanced.clothing === "high_quality" && advanced.generic_color === "high_quality";
	const dataBc7 = advanced.data === "high_quality";
	if (colorBc7 && dataBc7) return ` + ${translate("profiles.summary.bc7_color_data")}`;
	if (colorBc7) return ` + ${translate("profiles.summary.bc7_color")}`;
	if (dataBc7) return ` + ${translate("profiles.summary.bc7_data")}`;
	return "";
}

export function localizedLightingSummaryLabel(setting: LightingSummaryLabelData, translate: Translate): string {
	const env = setting.lighting_environment_enabled
		? `${translate("profiles.summary.lighting_env")} ${setting.lighting_environment_intensity.toFixed(2)}`
		: translate("profiles.summary.lighting_env_off");
	const dir = setting.lighting_directional_enabled
		? `${translate("profiles.summary.lighting_dir")} ${
				setting.lighting_directional_follow_camera_yaw
					? translate("profiles.summary.lighting_camera_azimuth")
					: translate("profiles.summary.lighting_world")
			} ${setting.lighting_directional_intensity.toFixed(2)}`
		: translate("profiles.summary.lighting_dir_off");
	return `${env} / ${dir}`;
}

export function localizedSettingSummary(setting: SettingSummaryLabelData, translate: Translate): string {
	const parts = [localizedMotionLabel(setting, translate), localizedOutputLabel(setting, translate), localizedWindowLabel(setting, translate)];
	if (setting.aa) parts.push(`AA ${localizedAaModeLabel(setting.aa, translate)}`);
	return parts.join(" · ");
}

export function profileStageSummaryItems(setting: ProfileStageSummarySetting, translate: Translate): ProfileSummaryItem[] {
	return [
		{
			section: "avatar",
			label: translate("profiles.sections.avatar"),
			value: basename(setting.avatar_path),
		},
		{
			section: "motion",
			label: translate("profiles.sections.motion"),
			value: localizedMotionLabel(setting, translate),
		},
		{
			section: "output",
			label: translate("profiles.sections.output"),
			value: localizedOutputLabel(setting, translate),
		},
		{
			section: "quality",
			label: translate("profiles.sections.quality"),
			value: localizedQualitySummaryLabel(setting, translate),
		},
		{
			section: "lighting",
			label: translate("profiles.editor.lighting"),
			value: localizedLightingSummaryLabel(setting, translate),
		},
		{
			section: "look",
			label: translate("profiles.sections.look"),
			value: localizedLookLabel(setting, translate),
		},
		{
			section: "window",
			label: translate("profiles.sections.window"),
			value: localizedWindowLabel(setting, translate),
		},
		{
			section: "camera",
			label: translate("profiles.sections.camera"),
			value: cameraSummaryLabel(setting),
		},
	];
}
