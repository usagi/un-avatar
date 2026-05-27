<script lang="ts">
  import type { ProfileSettingValue } from "./profileTypes";
  import { _ } from "svelte-i18n";
  import ProfileLookBloomFields from "./ProfileLookBloomFields.svelte";
  import type { ProfileBloomSetting } from "./profileLookTypes";

  export let setting: ProfileBloomSetting;
  export let busy = false;
  export let onUpdateSettingValue: (
    field: string,
    value: ProfileSettingValue,
  ) => void | Promise<void>;

</script>

<details class="effect-panel">
  <summary>
    <span>{$_("profiles.editor.look_bloom")}</span>
    <small>{$_("profiles.editor.look_bloom_summary")}</small>
    <span class="effect-panel-status"
      >{setting.bloom_enabled
        ? $_("profiles.editor.look_status_on")
        : $_("profiles.editor.look_status_off")} · {setting.bloom_quality}</span
    >
  </summary>
  <ProfileLookBloomFields {setting} {busy} {onUpdateSettingValue} />
</details>
