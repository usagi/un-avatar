<script lang="ts">
  import type { ProfileSettingValue } from "./profileTypes";
  import { _ } from "svelte-i18n";
  import ProfileLookContactShadowFields from "./ProfileLookContactShadowFields.svelte";
  import ProfileLookSsaoFields from "./ProfileLookSsaoFields.svelte";
  import type { ProfileShadowsSetting } from "./profileLookTypes";

  export let setting: ProfileShadowsSetting;
  export let busy = false;
  export let onUpdateSettingValue: (
    field: string,
    value: ProfileSettingValue,
  ) => void | Promise<void>;

</script>

<details class="effect-panel">
  <summary>
    <span>{$_("profiles.editor.look_shadows")}</span>
    <small>{$_("profiles.editor.look_shadows_summary")}</small>
    <span class="effect-panel-status"
      >shadow {setting.contact_shadow_enabled
        ? $_("profiles.editor.look_status_on")
        : $_("profiles.editor.look_status_off")} · SSAO {setting.ssao_enabled
        ? $_("profiles.editor.look_status_on")
        : $_("profiles.editor.look_status_off")}</span
    >
  </summary>
  <ProfileLookContactShadowFields {setting} {busy} {onUpdateSettingValue} />
  <ProfileLookSsaoFields {setting} {busy} {onUpdateSettingValue} />
</details>
