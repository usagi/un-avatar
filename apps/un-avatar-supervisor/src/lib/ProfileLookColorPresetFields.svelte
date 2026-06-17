<script lang="ts">
	import { _ } from "svelte-i18n";
	import ProfileRangeNumberField from "./ProfileRangeNumberField.svelte";
	import type { ProfileSettingValue } from "./profileTypes";
	import type { ProfileColorGradingSetting } from "./profileLookTypes";

	export let setting: Pick<ProfileColorGradingSetting, "color_look" | "color_look_intensity">;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
	export let onUpdateSettingValues: (updates: readonly [field: string, value: ProfileSettingValue][]) => void | Promise<void> = async (
		updates
	) => {
		for (const [field, value] of updates) {
			await onUpdateSettingValue(field, value);
		}
	};

	async function applyColorPreset(value: string, intensity: number): Promise<void> {
		await onUpdateSettingValues([
			["environment.color.look", value],
			["environment.color.intensity", intensity],
		]);
	}

	const presets = [
		{ value: "neutral", labelKey: "profiles.editor.color_look_neutral", intensity: 0 },
		{ value: "warm", labelKey: "profiles.editor.color_look_warm", intensity: 0.45 },
		{ value: "cool", labelKey: "profiles.editor.color_look_cool", intensity: 0.45 },
		{ value: "film", labelKey: "profiles.editor.color_look_film", intensity: 0.5 },
		{ value: "soft", labelKey: "profiles.editor.color_look_soft", intensity: 0.4 },
		{ value: "pop", labelKey: "profiles.editor.color_look_pop", intensity: 0.45 },
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
			{$_(preset.labelKey)}
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
