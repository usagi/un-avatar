<script lang="ts">
  import { _ } from "svelte-i18n";
  import ProfileRangeNumberField from "./ProfileRangeNumberField.svelte";
  import ProfileToggleField from "./ProfileToggleField.svelte";
  import type { ProfileSettingValue } from "./profileTypes";
  import type { ProfileSurfaceSetting } from "./profileLookTypes";

  export let setting: Pick<
    ProfileSurfaceSetting,
    "specular_enabled" | "specular_intensity" | "specular_power"
  >;
  export let busy = false;
  export let onUpdateSettingValue: (
    field: string,
    value: ProfileSettingValue,
  ) => void | Promise<void>;
</script>

<ProfileToggleField
  label={$_("profiles.editor.look_specular")}
  hint={$_("profiles.hints.look.specular")}
  checked={setting.specular_enabled}
  disabled={busy}
  onChange={(checked) =>
    onUpdateSettingValue("effects.avatar.specular.enabled", checked)}
/>
<ProfileRangeNumberField
  label={$_("profiles.editor.look_specular_strength")}
  hint={$_("profiles.hints.look.specular_strength")}
  value={setting.specular_intensity}
  rangeMin={0}
  rangeMax={2}
  step={0.05}
  disabled={busy || !setting.specular_enabled}
  onChange={(value) =>
    onUpdateSettingValue("effects.avatar.specular.intensity", value)}
/>
<ProfileRangeNumberField
  label={$_("profiles.editor.look_specular_power")}
  hint={$_("profiles.hints.look.specular_power")}
  value={setting.specular_power}
  rangeMin={1}
  rangeMax={128}
  step={1}
  disabled={busy || !setting.specular_enabled}
  onChange={(value) =>
    onUpdateSettingValue("effects.avatar.specular.power", value)}
/>
