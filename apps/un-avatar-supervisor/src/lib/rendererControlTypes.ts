import type { OutputLabelData, WindowLabelData } from "./profileLabels";
import type { RuntimeOutputStatusData } from "./runtimeLabels";
import type { RendererCameraSnapshot, RendererInstance, RendererRuntimeStatus } from "./rendererTypes";

export type RendererControlsData = OutputLabelData &
	WindowLabelData &
	Pick<RendererInstance, "id" | "pid" | "window_width" | "window_height">;

export type RendererControlsStatus = RuntimeOutputStatusData &
	Pick<
		RendererRuntimeStatus,
		| "input_passthrough"
		| "minimized"
		| "window_position"
		| "window_inner_size"
		| "bone_collider_count"
		| "show_axes"
		| "show_bone_colliders"
		| "camera_locked"
		| "camera"
		| "clear_color"
		| "runtime_actions"
		| "runtime_parameter_values"
		| "menu_action_candidates"
		| "menu_wardrobe_candidates"
		| "active_wardrobe_set"
	>;

export type RendererOutputData = OutputLabelData & {
	pid: number | null;
};

export type RendererOutputStatus = RuntimeOutputStatusData;

export type RendererWindowData = WindowLabelData & {
	pid: number | null;
};

export type RendererWindowStatus = Pick<RendererRuntimeStatus, "input_passthrough" | "minimized" | "window_position" | "window_inner_size">;

export type RendererDisplayData = Pick<RendererInstance, "pid">;

export type RendererDisplayStatus = Pick<
	RendererRuntimeStatus,
	"bone_collider_count" | "show_axes" | "show_bone_colliders" | "camera_locked"
>;

export type RendererBackgroundData = Pick<RendererInstance, "pid">;

export type RendererBackgroundStatus = Pick<RendererRuntimeStatus, "clear_color">;

export type RendererCameraData = Pick<RendererInstance, "pid" | "window_width" | "window_height">;

export type RendererCameraStatus = Pick<RendererRuntimeStatus, "camera" | "window_inner_size">;

export const RENDERER_CLEAR_COLOR_FALLBACK: [number, number, number] = [0.12, 0.14, 0.18];

export function rendererClearColorRgb(status: RendererBackgroundStatus | null): [number, number, number] {
	return [
		status?.clear_color?.[0] ?? RENDERER_CLEAR_COLOR_FALLBACK[0],
		status?.clear_color?.[1] ?? RENDERER_CLEAR_COLOR_FALLBACK[1],
		status?.clear_color?.[2] ?? RENDERER_CLEAR_COLOR_FALLBACK[2],
	];
}

export function rendererCameraTargetLabel(camera: RendererCameraSnapshot): string {
	return `${camera.target[0].toFixed(2)}, ${camera.target[1].toFixed(2)}, ${camera.target[2].toFixed(2)}`;
}

export function rendererCameraOrbitLabel(camera: RendererCameraSnapshot): string {
	return `long ${camera.longitude_deg.toFixed(1)}° / lat ${camera.latitude_deg.toFixed(1)}°`;
}

export function rendererCameraStatusValues(camera: RendererCameraSnapshot): { fov: string; radius: string } {
	return {
		fov: camera.diagonal_fov_deg.toFixed(1),
		radius: camera.radius.toFixed(2),
	};
}
