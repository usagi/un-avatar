<script lang="ts">
  import type {
    ProfileSettingValue,
    SpoutOutputSetting,
    SpoutResolutionPreset,
  } from "./profileTypes";
  import { _ } from "svelte-i18n";
  import ProfileSpoutOutputFields from "./ProfileSpoutOutputFields.svelte";

  export let setting: SpoutOutputSetting;
  export let busy = false;
  export let onUpdateSettingValue: (
    field: string,
    value: ProfileSettingValue,
  ) => void | Promise<void>;
  export let onApplySpoutResolutionPreset: (
    preset: SpoutResolutionPreset,
  ) => void | Promise<void>;
  export let onActivate: () => void;

</script>

<section
  class="editor-section profile-output-section"
  data-profile-section="output"
  onfocusin={onActivate}
  data-hint={$_("profiles.hints.output.section")}
>
  <div class="section-title-row">
    <h3>{$_("profiles.editor.output")}</h3>
    <span class="setting-scope">{$_("profiles.editor.launch_time")}</span>
  </div>

  <ProfileSpoutOutputFields
    {setting}
    {busy}
    {onUpdateSettingValue}
    {onApplySpoutResolutionPreset}
  />
</section>
