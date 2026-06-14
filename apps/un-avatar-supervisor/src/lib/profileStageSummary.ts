import { aaModeLabel, basename } from "./formatting";
import {
	cameraSummaryLabel,
	lightingSummaryLabel,
	motionLabel,
	type LookSummaryLabelData,
	type OutputLabelData,
	type SettingSummaryLabelData,
	type WindowLabelData,
} from "./profileLabels";
import type { ProfileSummaryItem } from "./profileSummary";
import { qualitySummaryLabel, type RuntimeOutputStatusData } from "./runtimeLabels";

type Translate = (key: string, options?: { values?: Record<string, string | number> }) => string;

export type ProfileStageSummarySetting = Parameters<typeof motionLabel>[0] &
	OutputLabelData &
	Parameters<typeof qualitySummaryLabel>[0] &
	Parameters<typeof lightingSummaryLabel>[0] &
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

export function localizedSettingSummary(setting: SettingSummaryLabelData, translate: Translate): string {
	const parts = [motionLabel(setting), localizedOutputLabel(setting, translate), localizedWindowLabel(setting, translate)];
	if (setting.aa) parts.push(`AA ${aaModeLabel(setting.aa)}`);
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
			value: motionLabel(setting),
		},
		{
			section: "output",
			label: translate("profiles.sections.output"),
			value: localizedOutputLabel(setting, translate),
		},
		{
			section: "quality",
			label: translate("profiles.sections.quality"),
			value: qualitySummaryLabel(setting),
		},
		{
			section: "lighting",
			label: translate("profiles.editor.lighting"),
			value: lightingSummaryLabel(setting),
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
