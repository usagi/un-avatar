<script lang="ts">
	import { _ } from "svelte-i18n";
	import { CONTACT_SHADOW_RANGE_FIELDS } from "./lookOptions";
	import ProfileRangeNumberField from "./ProfileRangeNumberField.svelte";
	import ProfileToggleField from "./ProfileToggleField.svelte";
	import type { ProfileSettingValue } from "./profileTypes";
	import type { ProfileShadowsSetting } from "./profileLookTypes";

	export let setting: Pick<
		ProfileShadowsSetting,
		"contact_shadow_enabled" | "contact_shadow_strength" | "contact_shadow_radius" | "contact_shadow_softness" | "contact_shadow_height"
	>;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
</script>

<ProfileToggleField
	label={$_("profiles.editor.look_contact_shadow")}
	hint={$_("profiles.hints.look.contact_shadow")}
	checked={setting.contact_shadow_enabled}
	disabled={busy}
	onChange={(checked) => onUpdateSettingValue("effects.avatar.contact_shadow.enabled", checked)}
/>
{#each CONTACT_SHADOW_RANGE_FIELDS as field}
	<ProfileRangeNumberField
		label={$_(field.labelKey)}
		hint={$_(field.hintKey)}
		value={setting[field.key]}
		rangeMin={field.rangeMin}
		rangeMax={field.rangeMax}
		step={field.step}
		disabled={busy || !setting.contact_shadow_enabled}
		onChange={(value) => onUpdateSettingValue(field.field, value)}
	/>
{/each}
