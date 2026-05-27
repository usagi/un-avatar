<script lang="ts">
  import type { ProfileSettingValue } from "./profileTypes";
  import { _ } from "svelte-i18n";
  import { formatFixed } from "./formatting";
  import ProfileLookSpecularFields from "./ProfileLookSpecularFields.svelte";
  import ProfileLookSurfaceBaseFields from "./ProfileLookSurfaceBaseFields.svelte";
  import type { ProfileSurfaceSetting } from "./profileLookTypes";

  export let setting: ProfileSurfaceSetting;
  export let busy = false;
  export let onUpdateSettingValue: (
    field: string,
    value: ProfileSettingValue,
  ) => void | Promise<void>;

</script>

<details class="effect-panel">
  <summary>
    <span>{$_("profiles.editor.look_surface")}</span>
    <small>{$_("profiles.editor.look_surface_summary")}</small>
    <span class="effect-panel-status"
      >matcap {formatFixed(setting.matcap_scale)} · {setting.specular_enabled
        ? $_("profiles.editor.look_status_on")
        : $_("profiles.editor.look_status_off")}</span
    >
  </summary>
  <ProfileLookSurfaceBaseFields {setting} {busy} {onUpdateSettingValue} />
  <ProfileLookSpecularFields {setting} {busy} {onUpdateSettingValue} />
</details>
