<script lang="ts">
  import { _ } from "svelte-i18n";
  import type { ProfileSettingValue } from "./profileTypes";
  import {
    springBonePresetLabel,
    type SpringBoneCategoryOverrideSetting,
  } from "./springBonePresets";

  export let override: SpringBoneCategoryOverrideSetting;
  export let presets: readonly string[] = [];
  export let disabled = false;
  export let onUpdateSettingValue: (
    field: string,
    value: ProfileSettingValue,
  ) => void | Promise<void>;
</script>

{#if presets.length > 0}
  <div class="spring-bone-preset-actions">
    <span>{$_("profiles.editor.spring_bone_recommended_presets")}</span>
    {#each presets as preset}
      <button
        type="button"
        class="secondary"
        {disabled}
        onclick={() =>
          onUpdateSettingValue(
            `physics.spring_bone.overrides.${override.category}.preset`,
            preset,
          )}
      >
        {springBonePresetLabel(override.category, preset, $_)}
      </button>
    {/each}
  </div>
{/if}
