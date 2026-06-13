<script lang="ts">
	import { _ } from "svelte-i18n";
	import ProfileDynamicsOverrideFields from "./ProfileDynamicsOverrideFields.svelte";
	import ProfileDynamicsPresetActions from "./ProfileDynamicsPresetActions.svelte";
	import ProfileSelectField from "./ProfileSelectField.svelte";
	import type { ProfileSettingValue } from "./profileTypes";
	import { DYNAMICS_MODE_OPTIONS, dynamicsRecommendedPresets, type DynamicsCategoryOverrideSetting } from "./dynamicsPresets";

	export let override: DynamicsCategoryOverrideSetting;
	export let dynamicsEnabled = false;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;

	$: recommendedPresets = dynamicsRecommendedPresets(override.category);
	$: disabled = !dynamicsEnabled || busy;
</script>

<div class="dynamics-override-row">
	<div class="dynamics-override-title">
		<strong
			>{override.name}{#if override.spring_bone_count > 0}
				<small>{override.spring_bone_count}</small>
			{/if}</strong
		>
		{#if override.mode === "authored"}
			<span>{$_("profiles.editor.spring_bone_category_inherit")}</span>
		{:else}
			<span>{$_("profiles.editor.spring_bone_category_override")}</span>
		{/if}
	</div>
	<ProfileSelectField
		label={$_("profiles.editor.spring_bone_mode")}
		hint={$_("profiles.editor.spring_bone_mode_hint")}
		value={override.mode}
		{disabled}
		options={DYNAMICS_MODE_OPTIONS}
		onChange={(value) => onUpdateSettingValue(`physics.spring_bone.overrides.${override.category}.mode`, value)}
	/>
	{#if override.mode !== "authored"}
		<ProfileDynamicsOverrideFields {override} {disabled} {onUpdateSettingValue} />
		<button
			type="button"
			class="secondary"
			{disabled}
			onclick={() => onUpdateSettingValue(`physics.spring_bone.overrides.${override.category}.reset`, true)}
		>
			{$_("profiles.editor.spring_bone_category_reset")}
		</button>
	{/if}
	{#if override.mode === "override_xpbd" && recommendedPresets.length > 0}
		<ProfileDynamicsPresetActions {override} presets={recommendedPresets} {disabled} {onUpdateSettingValue} />
	{/if}
</div>
