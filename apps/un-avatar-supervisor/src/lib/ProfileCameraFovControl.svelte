<script lang="ts">
	import { _ } from "svelte-i18n";
	import CameraFovFields from "./CameraFovFields.svelte";
	import CameraFovPresetRow from "./CameraFovPresetRow.svelte";
	import type { CameraLensPreset, CameraSetting, ProfileSettingValue } from "./profileTypes";

	export let setting: CameraSetting;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
	export let onApplyLensPreset: (focalLengthMm: CameraLensPreset) => void | Promise<void>;

	$: diagonalFovDeg = setting.camera_diagonal_fov_deg ?? 35;
	$: windowWidth = setting.window_width ?? 1280;
	$: windowHeight = setting.window_height ?? 720;
</script>

<div class="fov-control profile-field-full" data-hint={$_("profiles.hints.camera.fov")}>
	<div class="subgroup-heading">
		<span>{$_("profiles.editor.camera_fov")}</span>
		<CameraFovPresetRow {busy} {onApplyLensPreset} />
	</div>
	<CameraFovFields {diagonalFovDeg} {windowWidth} {windowHeight} {onUpdateSettingValue} />
</div>
