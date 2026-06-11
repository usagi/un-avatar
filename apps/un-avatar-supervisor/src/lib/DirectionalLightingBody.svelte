<script lang="ts">
	import DirectionalLightingAppearanceFields from "./DirectionalLightingAppearanceFields.svelte";
	import DirectionalLightingBasisFields from "./DirectionalLightingBasisFields.svelte";
	import DirectionalLightingDirectionFields from "./DirectionalLightingDirectionFields.svelte";
	import DirectionalLightingPreview from "./DirectionalLightingPreview.svelte";
	import type { ColorModeChangeHandler } from "./profileColorActions";
	import type { LightingSetting, ProfileSettingValue } from "./profileTypes";
	import type { ColorDisplayMode } from "./storageState";

	export let setting: LightingSetting;
	export let busy = false;
	export let colorDisplayMode: ColorDisplayMode;
	export let onColorModeChange: ColorModeChangeHandler;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
</script>

<div class="lighting-directional-body">
	<DirectionalLightingBasisFields
		enabled={setting.lighting_directional_enabled}
		followCameraYaw={setting.lighting_directional_follow_camera_yaw}
		followCameraPitch={setting.lighting_directional_follow_camera_pitch}
		{busy}
		{onUpdateSettingValue}
	/>
	<DirectionalLightingDirectionFields
		enabled={setting.lighting_directional_enabled}
		azimuthDeg={setting.lighting_directional_azimuth_deg}
		elevationDeg={setting.lighting_directional_elevation_deg}
		{busy}
		{onUpdateSettingValue}
	/>
	<DirectionalLightingAppearanceFields
		enabled={setting.lighting_directional_enabled}
		color={setting.lighting_directional_color}
		intensity={setting.lighting_directional_intensity}
		{busy}
		{colorDisplayMode}
		{onColorModeChange}
		{onUpdateSettingValue}
	/>
	<DirectionalLightingPreview {setting} />
</div>
