<script lang="ts">
	import { _ } from "svelte-i18n";
	import ProfileNumberInputField from "./ProfileNumberInputField.svelte";
	import type { ProfileSettingValue } from "./profileTypes";
	import {
		DYNAMICS_DAMPING_FIELD,
		DYNAMICS_VERLET_FIELDS,
		DYNAMICS_XPBD_FIELDS,
		type DynamicsCategoryOverrideSetting,
	} from "./dynamicsPresets";

	export let override: DynamicsCategoryOverrideSetting;
	export let disabled = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;

	$: fieldPrefix = `physics.spring_bone.overrides.${override.category}`;
</script>

<ProfileNumberInputField
	label={$_(DYNAMICS_DAMPING_FIELD.labelKey)}
	hint={$_(DYNAMICS_DAMPING_FIELD.hintKey)}
	value={override[DYNAMICS_DAMPING_FIELD.key]}
	min={DYNAMICS_DAMPING_FIELD.min}
	max={DYNAMICS_DAMPING_FIELD.max}
	step={DYNAMICS_DAMPING_FIELD.step}
	{disabled}
	onChange={(value) => onUpdateSettingValue(`${fieldPrefix}.${DYNAMICS_DAMPING_FIELD.key}`, value)}
/>
{#if override.mode === "override_xpbd"}
	{#each DYNAMICS_XPBD_FIELDS as field}
		<ProfileNumberInputField
			label={$_(field.labelKey)}
			hint={$_(field.hintKey)}
			value={override[field.key]}
			min={field.min}
			max={field.max}
			step={field.step}
			{disabled}
			onChange={(value) => onUpdateSettingValue(`${fieldPrefix}.${field.key}`, value)}
		/>
	{/each}
{:else}
	{#each DYNAMICS_VERLET_FIELDS as field}
		<ProfileNumberInputField
			label={$_(field.labelKey)}
			hint={$_(field.hintKey)}
			value={override[field.key]}
			min={field.min}
			max={field.max}
			step={field.step}
			{disabled}
			onChange={(value) => onUpdateSettingValue(`${fieldPrefix}.${field.key}`, value)}
		/>
	{/each}
{/if}
