<script lang="ts">
  import { _ } from "svelte-i18n";
  import ProfileRangeNumberField from "./ProfileRangeNumberField.svelte";
  import type { CameraSetting, ProfileSettingValue } from "./profileTypes";

  export let setting: CameraSetting;
  export let onUpdateSettingValue: (
    field: string,
    value: ProfileSettingValue,
  ) => void | Promise<void>;
</script>

<ProfileRangeNumberField
  className="profile-field-full camera-radius-control"
  label={$_("profiles.editor.camera_radius")}
  hint={$_("profiles.hints.camera.radius")}
  unit="mm"
  value={(setting.camera_radius ?? 1.5) * 1000}
  rangeMin={50}
  rangeMax={20000}
  numberMin={50}
  numberMax={200000}
  step={10}
  onChange={(value) => onUpdateSettingValue("camera.radius", value / 1000)}
/>
