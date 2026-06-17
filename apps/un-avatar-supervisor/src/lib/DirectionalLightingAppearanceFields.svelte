<script lang="ts">
	import { _ } from "svelte-i18n";
	import ColorField from "../ColorField.svelte";
	import ProfileRangeNumberField from "./ProfileRangeNumberField.svelte";
	import type { ColorModeChangeHandler } from "./profileColorActions";
	import type { ProfileSettingValue } from "./profileTypes";
	import type { ColorDisplayMode } from "./storageState";

	export let enabled = false;
	export let color: [number, number, number];
	export let intensity = 1;
	export let busy = false;
	export let colorDisplayMode: ColorDisplayMode;
	export let onColorModeChange: ColorModeChangeHandler;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
</script>

<div class="lighting-control-cluster lighting-appearance-cluster">
	<span class="lighting-cluster-title">{$_("profiles.editor.lighting_appearance")}</span>
	<ColorField
		label={$_("profiles.editor.lighting_color")}
		value={color}
		fallback={[1, 1, 1]}
		disabled={busy || !enabled}
		mode={colorDisplayMode}
		onModeChange={onColorModeChange}
		onChange={(nextColor) => onUpdateSettingValue("environment.lighting.directional.color", nextColor)}
	/>
	<ProfileRangeNumberField
		label={$_("profiles.editor.lighting_intensity")}
		value={intensity}
		decimals={2}
		rangeMin={0}
		rangeMax={4}
		step={0.01}
		disabled={busy || !enabled}
		onChange={(value) => onUpdateSettingValue("environment.lighting.directional.intensity", value)}
	/>
</div>
