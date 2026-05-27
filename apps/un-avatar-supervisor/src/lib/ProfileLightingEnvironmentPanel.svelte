<script lang="ts">
  import { _ } from "svelte-i18n";
  import ColorField from "../ColorField.svelte";
  import { formatFixed } from "./formatting";
  import ProfileRangeNumberField from "./ProfileRangeNumberField.svelte";
  import ProfileToggleField from "./ProfileToggleField.svelte";
  import type { ColorModeChangeHandler } from "./profileColorActions";
  import type { LightingSetting, ProfileSettingValue } from "./profileTypes";
  import type { ColorDisplayMode } from "./storageState";

  export let setting: LightingSetting;
  export let busy = false;
  export let colorDisplayMode: ColorDisplayMode;
  export let onColorModeChange: ColorModeChangeHandler;
  export let onUpdateSettingValue: (
    field: string,
    value: ProfileSettingValue,
  ) => void | Promise<void>;

</script>

<details class="effect-panel" open>
  <summary>
    <span>{$_("profiles.editor.lighting_environment")}</span>
    <small>{$_("profiles.editor.lighting_environment_summary")}</small>
    <span class="effect-panel-status"
      >{setting.lighting_environment_enabled
        ? $_("profiles.editor.look_status_on")
        : $_("profiles.editor.look_status_off")} · {formatFixed(
        setting.lighting_environment_intensity,
        2,
      )}</span
    >
  </summary>
  <ProfileToggleField
    label={$_("profiles.editor.lighting_enabled")}
    checked={setting.lighting_environment_enabled}
    disabled={busy}
    onChange={(checked) =>
      onUpdateSettingValue("environment.lighting.environment.enabled", checked)}
  />
  <ColorField
    label={$_("profiles.editor.lighting_color")}
    value={setting.lighting_environment_color}
    fallback={[1, 1, 1]}
    disabled={busy || !setting.lighting_environment_enabled}
    mode={colorDisplayMode}
    onModeChange={onColorModeChange}
    onChange={(color) =>
      onUpdateSettingValue("environment.lighting.environment.color", color)}
  />
  <ProfileRangeNumberField
    label={$_("profiles.editor.lighting_intensity")}
    value={setting.lighting_environment_intensity}
    decimals={2}
    rangeMin={0}
    rangeMax={2}
    step={0.01}
    disabled={busy || !setting.lighting_environment_enabled}
    onChange={(value) =>
      onUpdateSettingValue("environment.lighting.environment.intensity", value)}
  />
</details>
