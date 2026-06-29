<script lang="ts">
	import { _ } from "svelte-i18n";
	import ProfileNumberInputField from "./ProfileNumberInputField.svelte";
	import type { ProfileSettingValue } from "./profileTypes";
	import {
		DYNAMICS_BOUNCE_FIELD,
		DYNAMICS_DAMPING_FIELD,
		DYNAMICS_MOTION_COUPLING_FIELD,
		DYNAMICS_SHAPE_FIELD,
		DYNAMICS_STRETCH_MOTION_FIELD,
		DYNAMICS_STRETCH_RANGE_FIELD,
		DYNAMICS_VERLET_FIELDS,
		DYNAMICS_XPBD_FIELDS,
		dynamicsOverrideFieldPrefix,
		type DynamicsCategoryOverrideSetting,
	} from "./dynamicsPresets";

	export let override: DynamicsCategoryOverrideSetting;
	export let disabled = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;

	$: fieldPrefix = dynamicsOverrideFieldPrefix(override.category);
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
<ProfileNumberInputField
	label={$_(DYNAMICS_BOUNCE_FIELD.labelKey)}
	hint={$_(DYNAMICS_BOUNCE_FIELD.hintKey)}
	value={override[DYNAMICS_BOUNCE_FIELD.key]}
	min={DYNAMICS_BOUNCE_FIELD.min}
	max={DYNAMICS_BOUNCE_FIELD.max}
	step={DYNAMICS_BOUNCE_FIELD.step}
	{disabled}
	onChange={(value) => onUpdateSettingValue(`${fieldPrefix}.${DYNAMICS_BOUNCE_FIELD.key}`, value)}
/>
<ProfileNumberInputField
	label={$_(DYNAMICS_MOTION_COUPLING_FIELD.labelKey)}
	hint={$_(DYNAMICS_MOTION_COUPLING_FIELD.hintKey)}
	value={override[DYNAMICS_MOTION_COUPLING_FIELD.key]}
	min={DYNAMICS_MOTION_COUPLING_FIELD.min}
	max={DYNAMICS_MOTION_COUPLING_FIELD.max}
	step={DYNAMICS_MOTION_COUPLING_FIELD.step}
	{disabled}
	onChange={(value) => onUpdateSettingValue(`${fieldPrefix}.${DYNAMICS_MOTION_COUPLING_FIELD.key}`, value)}
/>
<ProfileNumberInputField
	label={$_(DYNAMICS_STRETCH_RANGE_FIELD.labelKey)}
	hint={$_(DYNAMICS_STRETCH_RANGE_FIELD.hintKey)}
	value={override[DYNAMICS_STRETCH_RANGE_FIELD.key]}
	min={DYNAMICS_STRETCH_RANGE_FIELD.min}
	max={DYNAMICS_STRETCH_RANGE_FIELD.max}
	step={DYNAMICS_STRETCH_RANGE_FIELD.step}
	{disabled}
	onChange={(value) => onUpdateSettingValue(`${fieldPrefix}.${DYNAMICS_STRETCH_RANGE_FIELD.key}`, value)}
/>
<ProfileNumberInputField
	label={$_(DYNAMICS_STRETCH_MOTION_FIELD.labelKey)}
	hint={$_(DYNAMICS_STRETCH_MOTION_FIELD.hintKey)}
	value={override[DYNAMICS_STRETCH_MOTION_FIELD.key]}
	min={DYNAMICS_STRETCH_MOTION_FIELD.min}
	max={DYNAMICS_STRETCH_MOTION_FIELD.max}
	step={DYNAMICS_STRETCH_MOTION_FIELD.step}
	{disabled}
	onChange={(value) => onUpdateSettingValue(`${fieldPrefix}.${DYNAMICS_STRETCH_MOTION_FIELD.key}`, value)}
/>
<ProfileNumberInputField
	label={$_(DYNAMICS_SHAPE_FIELD.labelKey)}
	hint={$_(DYNAMICS_SHAPE_FIELD.hintKey)}
	value={override[DYNAMICS_SHAPE_FIELD.key]}
	min={DYNAMICS_SHAPE_FIELD.min}
	max={DYNAMICS_SHAPE_FIELD.max}
	step={DYNAMICS_SHAPE_FIELD.step}
	{disabled}
	onChange={(value) => onUpdateSettingValue(`${fieldPrefix}.${DYNAMICS_SHAPE_FIELD.key}`, value)}
/>
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
{/if}
