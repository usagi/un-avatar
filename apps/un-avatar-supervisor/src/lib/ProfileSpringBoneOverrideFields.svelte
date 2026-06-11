<script lang="ts">
	import { _ } from "svelte-i18n";
	import ProfileNumberInputField from "./ProfileNumberInputField.svelte";
	import type { ProfileSettingValue } from "./profileTypes";
	import {
		SPRING_BONE_DAMPING_FIELD,
		SPRING_BONE_VERLET_FIELDS,
		SPRING_BONE_XPBD_FIELDS,
		type SpringBoneCategoryOverrideSetting,
	} from "./springBonePresets";

	export let override: SpringBoneCategoryOverrideSetting;
	export let disabled = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;

	$: fieldPrefix = `physics.spring_bone.overrides.${override.category}`;
</script>

<ProfileNumberInputField
	label={$_(SPRING_BONE_DAMPING_FIELD.labelKey)}
	hint={$_(SPRING_BONE_DAMPING_FIELD.hintKey)}
	value={override[SPRING_BONE_DAMPING_FIELD.key]}
	min={SPRING_BONE_DAMPING_FIELD.min}
	max={SPRING_BONE_DAMPING_FIELD.max}
	step={SPRING_BONE_DAMPING_FIELD.step}
	{disabled}
	onChange={(value) => onUpdateSettingValue(`${fieldPrefix}.${SPRING_BONE_DAMPING_FIELD.key}`, value)}
/>
{#if override.mode === "override_xpbd"}
	{#each SPRING_BONE_XPBD_FIELDS as field}
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
	{#each SPRING_BONE_VERLET_FIELDS as field}
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
