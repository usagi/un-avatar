<script lang="ts">
  import { _ } from "svelte-i18n";
  import ColorField from "../ColorField.svelte";
  import ProfileNumberInputField from "./ProfileNumberInputField.svelte";
  import ProfileRangeNumberField from "./ProfileRangeNumberField.svelte";
  import ProfileSelectField from "./ProfileSelectField.svelte";
  import {
    LOOK_POLICY_OPTIONS,
    OUTLINE_COLOR_FALLBACK,
    OUTLINE_NUMBER_FIELDS,
    OUTLINE_RANGE_FIELDS,
    OUTLINE_TYPE_OPTIONS,
  } from "./lookOptions";
  import type { ColorModeChangeHandler } from "./profileColorActions";
  import type { ProfileOutlineSetting } from "./profileLookTypes";
  import type { ProfileSettingValue } from "./profileTypes";
  import type { ColorDisplayMode } from "./storageState";

  export let setting: ProfileOutlineSetting;
  export let busy = false;
  export let colorDisplayMode: ColorDisplayMode;
  export let onColorModeChange: ColorModeChangeHandler;
  export let onUpdateSettingValue: (
    field: string,
    value: ProfileSettingValue,
  ) => void | Promise<void>;

</script>

<ProfileSelectField
  label={$_("profiles.editor.look_outline_policy")}
  hint={$_("profiles.hints.look.outline_policy")}
  value={setting.outline_policy}
  disabled={busy}
  options={LOOK_POLICY_OPTIONS}
  onChange={(value) =>
    onUpdateSettingValue("effects.avatar.outline.policy", value)}
/>
<ProfileSelectField
  label={$_("profiles.editor.look_outline_type")}
  hint={$_("profiles.hints.look.outline_type")}
  value={setting.outline_type}
  disabled={busy}
  options={OUTLINE_TYPE_OPTIONS}
  onChange={(value) => onUpdateSettingValue("effects.avatar.outline.type", value)}
/>
{#each OUTLINE_RANGE_FIELDS as field}
  <ProfileRangeNumberField
    label={$_(field.labelKey)}
    hint={$_(field.hintKey)}
    value={(setting[field.key] ?? field.fallback) * field.scale}
    rangeMin={field.rangeMin}
    rangeMax={field.rangeMax}
    step={field.step}
    disabled={busy}
    onChange={(value) => onUpdateSettingValue(field.field, value / field.scale)}
  />
{/each}
{#each OUTLINE_NUMBER_FIELDS as field}
  <ProfileNumberInputField
    label={$_(field.labelKey)}
    hint={$_(field.hintKey)}
    value={setting[field.key] ?? field.fallback}
    min={field.min}
    max={field.max}
    step={field.step}
    disabled={busy}
    onChange={(value) => onUpdateSettingValue(field.field, value)}
  />
{/each}
<ColorField
  label={$_("profiles.editor.look_outline_color")}
  value={setting.outline_color}
  fallback={OUTLINE_COLOR_FALLBACK}
  disabled={busy}
  mode={colorDisplayMode}
  onModeChange={onColorModeChange}
  onChange={(color) =>
    onUpdateSettingValue("effects.avatar.outline.color", color)}
/>
