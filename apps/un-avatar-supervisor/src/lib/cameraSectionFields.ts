import type { CameraSetting } from "./profileTypes";

type Translate = (key: string) => string;

export type CameraPresetOption = {
	value: string;
	label: string;
	hint: string;
};

export type CameraAxisField = {
	label: string;
	value: number;
	field: string;
	min?: number;
	max?: number;
	step: number;
};

export function cameraTargetPresetOptions(translate: Translate): CameraPresetOption[] {
	return [
		{
			value: "face",
			label: translate("profiles.editor.options.camera_target_face"),
			hint: translate("profiles.hints.camera.target_face"),
		},
		{
			value: "neck",
			label: translate("profiles.editor.options.camera_target_neck"),
			hint: translate("profiles.hints.camera.target_neck"),
		},
		{
			value: "chest",
			label: translate("profiles.editor.options.camera_target_chest"),
			hint: translate("profiles.hints.camera.target_chest"),
		},
	];
}

export function cameraOrbitPresetOptions(translate: Translate): CameraPresetOption[] {
	return [
		{
			value: "left",
			label: translate("profiles.editor.options.camera_orbit_left"),
			hint: translate("profiles.hints.camera.orbit_left"),
		},
		{
			value: "front",
			label: translate("profiles.editor.options.camera_orbit_front"),
			hint: translate("profiles.hints.camera.orbit_front"),
		},
		{
			value: "right",
			label: translate("profiles.editor.options.camera_orbit_right"),
			hint: translate("profiles.hints.camera.orbit_right"),
		},
	];
}

export function cameraTargetAxisFields(setting: CameraSetting): CameraAxisField[] {
	return [
		{
			label: "X",
			value: setting.camera_target?.[0] ?? 0,
			field: "camera.target_x",
			step: 0.01,
		},
		{
			label: "Y",
			value: setting.camera_target?.[1] ?? 0,
			field: "camera.target_y",
			step: 0.01,
		},
		{
			label: "Z",
			value: setting.camera_target?.[2] ?? 0,
			field: "camera.target_z",
			step: 0.01,
		},
	];
}

export function cameraOrbitAxisFields(setting: CameraSetting, translate: Translate): CameraAxisField[] {
	return [
		{
			label: translate("profiles.editor.camera_longitude"),
			value: setting.camera_longitude_deg ?? 0,
			field: "camera.longitude_deg",
			min: -360,
			max: 360,
			step: 1,
		},
		{
			label: translate("profiles.editor.camera_latitude"),
			value: setting.camera_latitude_deg ?? 0,
			field: "camera.latitude_deg",
			min: -89,
			max: 89,
			step: 1,
		},
	];
}
