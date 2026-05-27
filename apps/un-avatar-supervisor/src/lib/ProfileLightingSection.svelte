<script lang="ts">
  import type { LightingSetting, ProfileSettingValue } from "./profileTypes";
  import { _ } from "svelte-i18n";
  import ProfileLightingDirectionalPanel from "./ProfileLightingDirectionalPanel.svelte";
  import ProfileLightingEnvironmentPanel from "./ProfileLightingEnvironmentPanel.svelte";
  import type { ColorModeChangeHandler } from "./profileColorActions";
  import type { ColorDisplayMode } from "./storageState";

  export let setting: LightingSetting;
  export let busy = false;
  export let colorDisplayMode: ColorDisplayMode;
  export let onColorModeChange: ColorModeChangeHandler;
  export let onUpdateSettingValue: (
    field: string,
    value: ProfileSettingValue,
  ) => void | Promise<void>;
  export let onActivate: () => void;

</script>

<section
  class="editor-section section-grid lighting-section"
  data-profile-section="lighting"
  onfocusin={onActivate}
>
  <div class="section-title-row">
    <h3>{$_("profiles.editor.lighting")}</h3>
  </div>
  <ProfileLightingEnvironmentPanel
    {setting}
    {busy}
    {colorDisplayMode}
    {onColorModeChange}
    {onUpdateSettingValue}
  />
  <ProfileLightingDirectionalPanel
    {setting}
    {busy}
    {colorDisplayMode}
    {onColorModeChange}
    {onUpdateSettingValue}
  />
</section>
