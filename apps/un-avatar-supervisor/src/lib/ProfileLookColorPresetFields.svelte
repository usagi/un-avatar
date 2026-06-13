<script lang="ts">
	import { _ } from "svelte-i18n";
	import ProfileRangeNumberField from "./ProfileRangeNumberField.svelte";
	import type { ProfileSettingValue } from "./profileTypes";
	import type { ProfileColorGradingSetting } from "./profileLookTypes";

	export let setting: Pick<ProfileColorGradingSetting, "color_look" | "color_look_intensity">;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;

	async function applyColorPreset(value: string, intensity: number): Promise<void> {
		await onUpdateSettingValue("environment.color.look", value);
		await onUpdateSettingValue("environment.color.intensity", intensity);
	}

	const presets = [
		{ value: "neutral", label: "Neutral", intensity: 0 },
		{ value: "warm", label: "Warm", intensity: 0.45 },
		{ value: "cool", label: "Cool", intensity: 0.45 },
		{ value: "film", label: "Film", intensity: 0.5 },
		{ value: "soft", label: "Soft", intensity: 0.4 },
		{ value: "pop", label: "Pop", intensity: 0.45 },
	] as const;
</script>

<div class="look-preset-row">
	{#each presets as preset}
		<button
			type="button"
			disabled={busy}
			class:active={setting.color_look === preset.value}
			aria-pressed={setting.color_look === preset.value}
			onclick={() => applyColorPreset(preset.value, preset.intensity)}
		>
			{preset.label}
		</button>
	{/each}
</div>
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
