<script lang="ts">
	import { _ } from "svelte-i18n";
	import { SSAO_RANGE_FIELDS } from "./lookOptions";
	import ProfileRangeNumberField from "./ProfileRangeNumberField.svelte";
	import ProfileToggleField from "./ProfileToggleField.svelte";
	import type { ProfileSettingValue } from "./profileTypes";
	import type { ProfileShadowsSetting } from "./profileLookTypes";

	export let setting: Pick<ProfileShadowsSetting, "ssao_enabled" | "ssao_strength" | "ssao_radius" | "ssao_bias" | "ssao_range">;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
</script>

<ProfileToggleField
	label={$_("profiles.editor.look_ssao")}
	hint={$_("profiles.hints.look.ssao")}
	checked={setting.ssao_enabled}
	disabled={busy}
	onChange={(checked) => onUpdateSettingValue("effects.post.ssao.enabled", checked)}
/>
{#each SSAO_RANGE_FIELDS as field}
	<ProfileRangeNumberField
		label={$_(field.labelKey)}
		hint={$_(field.hintKey)}
		value={setting[field.key]}
		decimals={"decimals" in field ? field.decimals : 0}
		rangeMin={field.rangeMin}
		rangeMax={field.rangeMax}
		step={field.step}
		disabled={busy || !setting.ssao_enabled}
		onChange={(value) => onUpdateSettingValue(field.field, value)}
	/>
{/each}
