<script lang="ts">
	import { _ } from "svelte-i18n";
	import { COLOR_LOOK_OPTIONS } from "./lookOptions";
	import ProfileRangeNumberField from "./ProfileRangeNumberField.svelte";
	import ProfileSelectField from "./ProfileSelectField.svelte";
	import type { ProfileSettingValue } from "./profileTypes";
	import type { ProfileColorGradingSetting } from "./profileLookTypes";

	export let setting: Pick<ProfileColorGradingSetting, "color_look" | "color_look_intensity">;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;

	async function setColorLook(value: string): Promise<void> {
		await onUpdateSettingValue("environment.color.look", value);
		if (value === "neutral") {
			await onUpdateSettingValue("environment.color.intensity", 0);
		} else if (setting.color_look_intensity <= 0.001) {
			await onUpdateSettingValue("environment.color.intensity", 0.45);
		}
	}

	async function applyColorPreset(value: string, intensity: number): Promise<void> {
		await onUpdateSettingValue("environment.color.look", value);
		await onUpdateSettingValue("environment.color.intensity", intensity);
	}
</script>

<div class="look-preset-row">
	<button type="button" disabled={busy} class:active={setting.color_look === "neutral"} onclick={() => applyColorPreset("neutral", 0)}>Neutral</button>
	<button type="button" disabled={busy} class:active={setting.color_look === "warm"} onclick={() => applyColorPreset("warm", 0.45)}>Warm</button>
	<button type="button" disabled={busy} class:active={setting.color_look === "cool"} onclick={() => applyColorPreset("cool", 0.45)}>Cool</button>
	<button type="button" disabled={busy} class:active={setting.color_look === "film"} onclick={() => applyColorPreset("film", 0.5)}>Film</button>
	<button type="button" disabled={busy} class:active={setting.color_look === "pop"} onclick={() => applyColorPreset("pop", 0.45)}>Pop</button>
</div>

<ProfileSelectField
	label={$_("profiles.editor.look_color_look")}
	hint={$_("profiles.hints.look.color_look")}
	value={setting.color_look}
	disabled={busy}
	options={COLOR_LOOK_OPTIONS}
	onChange={setColorLook}
/>
<ProfileRangeNumberField
	label={$_("profiles.editor.look_strength")}
	hint={$_("profiles.hints.look.look_strength")}
	value={setting.color_look_intensity}
	rangeMin={0}
	rangeMax={1}
	step={0.01}
	decimals={2}
	disabled={busy || setting.color_look === "neutral"}
	onChange={(value) => onUpdateSettingValue("environment.color.intensity", value)}
/>
