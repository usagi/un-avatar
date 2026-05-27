<script lang="ts">
  import { _ } from "svelte-i18n";
  import DirectionalLightingBody from "./DirectionalLightingBody.svelte";
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

  $: directionalStatus = setting.lighting_directional_enabled
    ? $_("profiles.editor.look_status_on")
    : $_("profiles.editor.look_status_off");
  $: directionalBasis = setting.lighting_directional_follow_camera_yaw
    ? "camera"
    : "world";
</script>

<details class="effect-panel lighting-directional-panel" open>
  <summary>
    <span>{$_("profiles.editor.lighting_directional")}</span>
    <small>{$_("profiles.editor.lighting_directional_summary")}</small>
    <span class="effect-panel-status">{directionalStatus} · {directionalBasis}</span>
  </summary>
  <DirectionalLightingBody
    {setting}
    {busy}
    {colorDisplayMode}
    {onColorModeChange}
    {onUpdateSettingValue}
  />
</details>
