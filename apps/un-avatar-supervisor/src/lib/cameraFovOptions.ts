import {
	diagonalFovFromLensMm,
	diagonalFovToHorizontal,
	diagonalFovToVertical,
	horizontalFovToDiagonal,
	lensMmFromDiagonalFov,
	verticalFovToDiagonal,
} from "./profileDiagrams";

export type CameraFovFieldKey = "lens" | "diagonal" | "vertical" | "horizontal";

export type CameraFovField = {
	key: CameraFovFieldKey;
	labelKey: string;
	min: number;
	max: number;
	step: number;
	unit: string;
	decimals?: number;
};

export const CAMERA_FOV_FIELDS: readonly CameraFovField[] = [
	{
		key: "lens",
		labelKey: "profiles.editor.camera_lens",
		min: 1,
		max: 400,
		step: 1,
		unit: "mm",
		decimals: 0,
	},
	{
		key: "diagonal",
		labelKey: "profiles.editor.camera_diagonal_fov",
		min: 1,
		max: 160,
		step: 0.1,
		unit: "deg",
	},
	{
		key: "vertical",
		labelKey: "profiles.editor.camera_vertical_fov",
		min: 1,
		max: 160,
		step: 0.1,
		unit: "deg",
	},
	{
		key: "horizontal",
		labelKey: "profiles.editor.camera_horizontal_fov",
		min: 1,
		max: 160,
		step: 0.1,
		unit: "deg",
	},
];

export function cameraFovFieldValue(key: CameraFovFieldKey, diagonalFovDeg: number, windowWidth: number, windowHeight: number): number {
	if (key === "lens") return lensMmFromDiagonalFov(diagonalFovDeg);
	if (key === "vertical") {
		return diagonalFovToVertical(diagonalFovDeg, windowWidth, windowHeight);
	}
	if (key === "horizontal") {
		return diagonalFovToHorizontal(diagonalFovDeg, windowWidth, windowHeight);
	}
	return diagonalFovDeg;
}

export function diagonalFovFromFieldValue(key: CameraFovFieldKey, value: number, windowWidth: number, windowHeight: number): number {
	if (key === "lens") return diagonalFovFromLensMm(value);
	if (key === "vertical") {
		return verticalFovToDiagonal(value, windowWidth, windowHeight);
	}
	if (key === "horizontal") {
		return horizontalFovToDiagonal(value, windowWidth, windowHeight);
	}
	return value;
}

export function cameraWindowBasisValues(windowWidth: number, windowHeight: number): { width: number; height: number } {
	return {
		width: windowWidth,
		height: windowHeight,
	};
}
