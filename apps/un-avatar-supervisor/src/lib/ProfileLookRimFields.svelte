<script lang="ts">
  import { _ } from "svelte-i18n";
  import ColorField from "../ColorField.svelte";
  import ProfileNumberInputField from "./ProfileNumberInputField.svelte";
  import ProfileSelectField from "./ProfileSelectField.svelte";
  import {
    LOOK_POLICY_OPTIONS,
    RIM_COLOR_FALLBACK,
    RIM_NUMBER_FIELDS,
  } from "./lookOptions";
  import type { ColorModeChangeHandler } from "./profileColorActions";
  import type { ProfileRimSetting } from "./profileLookTypes";
  import type { ProfileSettingValue } from "./profileTypes";
  import type { ColorDisplayMode } from "./storageState";

  export let setting: ProfileRimSetting;
  export let busy = false;
  export let colorDisplayMode: ColorDisplayMode;
  export let onColorModeChange: ColorModeChangeHandler;
  export let onUpdateSettingValue: (
    field: string,
    value: ProfileSettingValue,
  ) => void | Promise<void>;

</script>

<ProfileSelectField
  label={$_("profiles.editor.look_rim_policy")}
  hint={$_("profiles.hints.look.rim_policy")}
  value={setting.rim_policy}
  disabled={busy}
  options={LOOK_POLICY_OPTIONS}
  onChange={(value) => onUpdateSettingValue("effects.avatar.rim.policy", value)}
/>
{#each RIM_NUMBER_FIELDS as field}
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
  label={$_("profiles.editor.look_rim_color")}
  value={setting.rim_color}
  fallback={RIM_COLOR_FALLBACK}
  disabled={busy}
  mode={colorDisplayMode}
  onModeChange={onColorModeChange}
  onChange={(color) => onUpdateSettingValue("effects.avatar.rim.color", color)}
/>
