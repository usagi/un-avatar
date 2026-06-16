<script lang="ts">
	import type { CameraLensPreset, CameraSetting, CameraTargetPreset, ProfileSettingValue } from "./profileTypes";
	import { _ } from "svelte-i18n";
	import ProfileCameraFovControl from "./ProfileCameraFovControl.svelte";
	import ProfileCameraOrbitControls from "./ProfileCameraOrbitControls.svelte";
	import ProfileCameraPreview from "./ProfileCameraPreview.svelte";
	import ProfileCameraRadiusControl from "./ProfileCameraRadiusControl.svelte";
	import ProfileCameraTargetControls from "./ProfileCameraTargetControls.svelte";
	import ProfileToggleField from "./ProfileToggleField.svelte";
	import type { CameraOrbitPreset } from "./cameraPresets";
	import type { RendererCameraSnapshot } from "./rendererTypes";

	export let setting: CameraSetting;
	export let runtimeCamera: RendererCameraSnapshot | null = null;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
	export let onApplyTargetPreset: (preset: CameraTargetPreset) => void | Promise<void>;
	export let onApplyOrbitPreset: (preset: CameraOrbitPreset) => void | Promise<void>;
	export let onApplyLensPreset: (focalLengthMm: CameraLensPreset) => void | Promise<void>;
	export let onCaptureRuntimeCamera: () => void | Promise<void>;
	export let onActivate: () => void;
</script>

<section
	class="editor-section section-grid profile-camera-section"
	data-profile-section="camera"
	onfocusin={onActivate}
	data-hint={$_("profiles.hints.camera.section")}
>
	<div class="section-title-row">
		<h3>{$_("profiles.editor.camera")}</h3>
		<button
			type="button"
			class="secondary compact"
			disabled={busy || !runtimeCamera}
			title={$_("profiles.editor.capture_runtime_camera_title")}
			onclick={onCaptureRuntimeCamera}>{$_("profiles.editor.capture_runtime_camera")}</button
		>
	</div>
	<ProfileToggleField
		label={$_("profiles.editor.lock_camera")}
		hint={$_("profiles.hints.camera.lock")}
		checked={setting.camera_locked}
		onChange={(checked) => onUpdateSettingValue("camera.locked", checked)}
	/>
	<ProfileCameraPreview {setting} />
	<ProfileCameraTargetControls {setting} {busy} {onUpdateSettingValue} {onApplyTargetPreset} />
	<ProfileCameraOrbitControls {setting} {busy} {onUpdateSettingValue} {onApplyOrbitPreset} />
	<ProfileCameraRadiusControl {setting} {onUpdateSettingValue} />
	<ProfileCameraFovControl {setting} {busy} {onUpdateSettingValue} {onApplyLensPreset} />
</section>
