<script lang="ts">
	import { _ } from "svelte-i18n";
	import { CAMERA_FOV_FIELDS, cameraFovFieldValue, cameraWindowBasisValues, diagonalFovFromFieldValue } from "./cameraFovOptions";
	import CameraFovNumberField from "./CameraFovNumberField.svelte";
	import type { ProfileSettingValue } from "./profileTypes";

	export let diagonalFovDeg = 35;
	export let windowWidth = 1280;
	export let windowHeight = 720;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
</script>

<div class="fov-grid">
	{#each CAMERA_FOV_FIELDS as field}
		<CameraFovNumberField
			label={$_(field.labelKey)}
			value={cameraFovFieldValue(field.key, diagonalFovDeg, windowWidth, windowHeight)}
			min={field.min}
			max={field.max}
			step={field.step}
			unit={field.unit}
			decimals={field.decimals ?? 1}
			onChange={(value) =>
				onUpdateSettingValue("camera.diagonal_fov_deg", diagonalFovFromFieldValue(field.key, value, windowWidth, windowHeight))}
		/>
	{/each}
	<small
		>{$_("profiles.editor.camera_window_basis", {
			values: cameraWindowBasisValues(windowWidth, windowHeight),
		})}</small
	>
</div>
