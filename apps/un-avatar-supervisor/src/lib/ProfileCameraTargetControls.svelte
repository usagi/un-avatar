<script lang="ts">
	import { _ } from "svelte-i18n";
	import CameraAxisFieldset from "./CameraAxisFieldset.svelte";
	import CameraPresetRow from "./CameraPresetRow.svelte";
	import { cameraTargetAxisFields, cameraTargetPresetOptions } from "./cameraSectionFields";
	import type { CameraSetting, CameraTargetPreset, ProfileSettingValue } from "./profileTypes";

	export let setting: CameraSetting;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
	export let onApplyTargetPreset: (preset: CameraTargetPreset) => void | Promise<void>;

	$: targetPresetOptions = cameraTargetPresetOptions($_);
	$: targetAxisFields = cameraTargetAxisFields(setting);
</script>

<CameraPresetRow
	title={$_("profiles.editor.camera_target_position")}
	ariaLabel={$_("profiles.editor.camera_target_presets")}
	className="target-preset-row"
	{busy}
	options={targetPresetOptions}
	onApply={(preset) => onApplyTargetPreset(preset as CameraTargetPreset)}
/>
<CameraAxisFieldset
	legend={$_("profiles.editor.camera_target_position")}
	hint={$_("profiles.hints.camera.target_position")}
	className="camera-target-control"
	unit="m"
	fields={targetAxisFields}
	onChange={onUpdateSettingValue}
/>
