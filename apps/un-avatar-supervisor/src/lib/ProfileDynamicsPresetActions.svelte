<script lang="ts">
	import { _ } from "svelte-i18n";
	import type { ProfileSettingValue } from "./profileTypes";
	import { dynamicsOverrideFieldPrefix, dynamicsPresetLabel, type DynamicsCategoryOverrideSetting } from "./dynamicsPresets";

	export let override: DynamicsCategoryOverrideSetting;
	export let presets: readonly string[] = [];
	export let disabled = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;

	$: fieldPrefix = dynamicsOverrideFieldPrefix(override.category);
</script>

{#if presets.length > 0}
	<div class="dynamics-preset-actions">
		<span>{$_("profiles.editor.spring_bone_recommended_presets")}</span>
		{#each presets as preset}
			<button type="button" class="secondary" {disabled} onclick={() => onUpdateSettingValue(`${fieldPrefix}.preset`, preset)}>
				{dynamicsPresetLabel(override.category, preset, $_)}
			</button>
		{/each}
	</div>
{/if}
