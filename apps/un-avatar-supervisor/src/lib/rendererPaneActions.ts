import type { CameraOrbitPreset } from "./cameraPresets";
import type { ColorModeChangeHandler } from "./profileColorActions";
import type { RendererWindowPatch } from "./rendererTypes";

export type RendererPaneActions = {
	onSetSpoutOutput: (enabled: boolean, size: { width: number; height: number } | null, label?: string) => void | Promise<void>;
	onSaveSpoutProfile: () => void | Promise<void>;
	onRestoreOutput: () => void | Promise<void>;
	onSetWindow: (patch: RendererWindowPatch, label: string) => void | Promise<void>;
	onSaveWindow: () => void | Promise<void>;
	onRestoreWindow: () => void | Promise<void>;
	onSetShowAxes: (enabled: boolean) => void | Promise<void>;
	onSetShowBoneColliders: (enabled: boolean) => void | Promise<void>;
	onSetCameraLock: (enabled: boolean) => void | Promise<void>;
	onSetCameraOrbitPreset: (preset: CameraOrbitPreset) => void | Promise<void>;
	onSaveCamera: () => void | Promise<void>;
	onRestoreCamera: () => void | Promise<void>;
	onSetClearColor: (rgb: [number, number, number]) => void | Promise<void>;
	onColorModeChange: ColorModeChangeHandler;
	onClearExpressionOverrides: (rendererId: number) => void | Promise<void>;
	onSetExpressionOverride: (rendererId: number, preset: string, weight: number) => void;
	onSetRuntimeParameter: (rendererId: number, name: string, value: number, label: string) => void | Promise<void>;
	onActivateRuntimeAction: (rendererId: number, actionId: string, label: string) => void | Promise<void>;
	onActivateWardrobeMenuCandidate: (rendererId: number, actionId: string, wardrobeSetId: string) => void | Promise<void>;
	onSetDynamicsEnabled: (rendererId: number, sourceId: string, enabled: boolean) => void | Promise<void>;
};
