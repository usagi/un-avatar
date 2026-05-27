<script lang="ts">
  import { _ } from "svelte-i18n";
  import { COLOR_LOOK_OPTIONS } from "./lookOptions";
  import ProfileRangeNumberField from "./ProfileRangeNumberField.svelte";
  import ProfileSelectField from "./ProfileSelectField.svelte";
  import type { ProfileSettingValue } from "./profileTypes";
  import type { ProfileColorGradingSetting } from "./profileLookTypes";

  export let setting: Pick<
    ProfileColorGradingSetting,
    "color_look" | "color_look_intensity"
  >;
  export let busy = false;
  export let onUpdateSettingValue: (
    field: string,
    value: ProfileSettingValue,
  ) => void | Promise<void>;

</script>

<ProfileSelectField
  label={$_("profiles.editor.look_color_look")}
  hint={$_("profiles.hints.look.color_look")}
  value={setting.color_look}
  disabled={busy}
  options={COLOR_LOOK_OPTIONS}
  onChange={(value) => onUpdateSettingValue("environment.color.look", value)}
/>
<ProfileRangeNumberField
  label={$_("profiles.editor.look_strength")}
  hint={$_("profiles.hints.look.look_strength")}
  value={setting.color_look_intensity}
  rangeMin={0}
  rangeMax={1}
  step={0.01}
  disabled={busy || setting.color_look === "neutral"}
  onChange={(value) => onUpdateSettingValue("environment.color.intensity", value)}
/>
