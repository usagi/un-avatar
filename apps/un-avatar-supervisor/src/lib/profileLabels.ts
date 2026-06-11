import { aaModeLabel, formatFixed, formatPercent, textureModeLabel } from "./formatting";
import { lensMmFromDiagonalFov } from "./profileDiagrams";

export type MotionLabelData = {
	motion_vmc_enabled: boolean;
	vmc_address: string | null;
	vmc_port: number | null;
	motion_unmotion_enabled: boolean;
	unmotion_zenoh_key: string | null;
};

export type OutputLabelData = {
	spout_enabled: boolean;
	spout_name: string | null;
};

export type WindowLabelData = {
	decorations: boolean;
	transparent: boolean;
	always_on_top: boolean;
	input_passthrough?: boolean;
};

export type CameraSummaryLabelData = {
	camera_diagonal_fov_deg: number | null;
	camera_radius: number | null;
};

export type LookSummaryLabelData = {
	color_look: string;
	color_look_intensity: number;
	bloom_enabled: boolean;
};

export type LightingSummaryLabelData = {
	lighting_environment_enabled: boolean;
	lighting_environment_intensity: number;
	lighting_directional_enabled: boolean;
	lighting_directional_follow_camera_yaw: boolean;
	lighting_directional_intensity: number;
};

export type SettingSummaryLabelData = MotionLabelData & OutputLabelData & WindowLabelData & Partial<{ aa: string }>;

export function motionLabel(setting: MotionLabelData): string {
	const parts: string[] = [];
	if (setting.motion_vmc_enabled) {
		const vmcLabel = `VMC/UDP ${setting.vmc_address ?? `0.0.0.0:${setting.vmc_port ?? 39539}`}`;
		parts.push(vmcLabel);
	}
	if (setting.motion_unmotion_enabled) {
		const key = setting.unmotion_zenoh_key ?? "un-motion/frame";
		const zenohLabel = `UNMF/Z ${key}/v1`;
		parts.push(zenohLabel);
	}
	return parts.length > 0 ? parts.join(" + ") : "None";
}

export function outputLabel(setting: OutputLabelData): string {
	return setting.spout_enabled ? (setting.spout_name ? `Spout2 / ${setting.spout_name}` : "Spout2") : "Window";
}

export function windowLabel(setting: WindowLabelData): string {
	const level = setting.always_on_top ? "Topmost" : "Normal";
	const passthrough = setting.input_passthrough ? " / Click-through" : "";
	return `${setting.decorations ? "Framed" : "Borderless"} / ${setting.transparent ? "Transparent" : "Opaque"} / ${level}${passthrough}`;
}

export function cameraSummaryLabel(setting: CameraSummaryLabelData): string {
	const lens = lensMmFromDiagonalFov(setting.camera_diagonal_fov_deg ?? 35);
	const radius = (setting.camera_radius ?? 1.5) * 1000;
	return `${formatFixed(lens, 0)}mm / ${formatFixed(radius, 0)}mm`;
}

export function lookSummaryLabel(setting: LookSummaryLabelData): string {
	const color =
		setting.color_look === "neutral" || setting.color_look_intensity <= 0
			? "Neutral"
			: `${textureModeLabel(setting.color_look)} ${formatPercent(setting.color_look_intensity)}%`;
	const bloom = setting.bloom_enabled ? "Bloom" : "No bloom";
	return `${color} / ${bloom}`;
}

export function lightingSummaryLabel(setting: LightingSummaryLabelData): string {
	const env = setting.lighting_environment_enabled ? `Env ${formatFixed(setting.lighting_environment_intensity, 2)}` : "Env off";
	const dir = setting.lighting_directional_enabled
		? `Dir ${setting.lighting_directional_follow_camera_yaw ? "cam az" : "world"} ${formatFixed(setting.lighting_directional_intensity, 2)}`
		: "Dir off";
	return `${env} / ${dir}`;
}

export function settingSummary(setting: SettingSummaryLabelData): string {
	const parts = [motionLabel(setting), outputLabel(setting), windowLabel(setting)];
	if (setting.aa) parts.push(`AA ${aaModeLabel(setting.aa)}`);
	return parts.join(" · ");
}
