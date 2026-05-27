<script lang="ts">
  import { _ } from "svelte-i18n";
  import ProfileNumberInputField from "./ProfileNumberInputField.svelte";
  import ProfileOptionalNumberInputField from "./ProfileOptionalNumberInputField.svelte";
  import type { ProfileSettingValue } from "./profileTypes";
  import {
    WINDOW_POSITION_FIELDS,
    WINDOW_POSITION_MAX,
    WINDOW_POSITION_MIN,
    WINDOW_SIZE_FIELDS,
  } from "./windowOptions";

  export let width = 1280;
  export let height = 720;
  export let x: number | null = null;
  export let y: number | null = null;
  export let onUpdateSettingValue: (
    field: string,
    value: ProfileSettingValue,
  ) => void | Promise<void>;
</script>

<div class="window-setting-cluster window-geometry-cluster">
  <span class="window-cluster-title">{$_("profiles.editor.window_geometry")}</span>
  {#each WINDOW_SIZE_FIELDS as field}
    <ProfileNumberInputField
      label={$_(field.labelKey)}
      value={field.key === "width" ? width : height}
      min={field.min}
      max={field.max}
      step={field.step}
      onChange={(value) => onUpdateSettingValue(field.field, value)}
    />
  {/each}
  {#each WINDOW_POSITION_FIELDS as field}
    <ProfileOptionalNumberInputField
      label={field.label}
      hint={$_(field.hintKey)}
      value={field.key === "x" ? x : y}
      min={WINDOW_POSITION_MIN}
      max={WINDOW_POSITION_MAX}
      step={1}
      placeholder="(default)"
      onChange={(value) => onUpdateSettingValue(field.field, value)}
    />
  {/each}
</div>
