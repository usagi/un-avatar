<script lang="ts">
	import { _ } from "svelte-i18n";
	import { COLOR_BASIC_RANGE_FIELDS } from "./lookOptions";
	import ProfileRangeNumberField from "./ProfileRangeNumberField.svelte";
	import type { ProfileSettingValue } from "./profileTypes";
	import type { ProfileColorGradingSetting } from "./profileLookTypes";

	export let setting: Pick<ProfileColorGradingSetting, "color_exposure" | "color_contrast" | "color_saturation">;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
</script>

{#each COLOR_BASIC_RANGE_FIELDS as field}
	<ProfileRangeNumberField
		label={$_(field.labelKey)}
		hint={$_(field.hintKey)}
		value={setting[field.key]}
		rangeMin={field.rangeMin}
		rangeMax={field.rangeMax}
		numberMin={field.numberMin}
		numberMax={field.numberMax}
		step={field.step}
		disabled={busy}
		onChange={(value) => onUpdateSettingValue(field.field, value)}
	/>
{/each}
