<script lang="ts">
  import { _ } from "svelte-i18n";
  import ProfileToggleField from "./ProfileToggleField.svelte";
  import type { ProfileSettingValue } from "./profileTypes";

  export let enabled = false;
  export let followCameraYaw = false;
  export let followCameraPitch = false;
  export let busy = false;
  export let onUpdateSettingValue: (
    field: string,
    value: ProfileSettingValue,
  ) => void | Promise<void>;
</script>

<div class="lighting-control-cluster lighting-state-cluster">
  <span class="lighting-cluster-title">{$_("profiles.editor.lighting_state")}</span>
  <ProfileToggleField
    label={$_("profiles.editor.lighting_enabled")}
    checked={enabled}
    disabled={busy}
    onChange={(checked) =>
      onUpdateSettingValue("environment.lighting.directional.enabled", checked)}
  />
</div>
<div class="lighting-control-cluster lighting-basis-cluster">
  <span class="lighting-cluster-title">{$_("profiles.editor.lighting_basis")}</span>
  <ProfileToggleField
    label={$_("profiles.editor.lighting_follow_camera_yaw")}
    checked={followCameraYaw}
    disabled={busy || !enabled}
    onChange={(checked) =>
      onUpdateSettingValue(
        "environment.lighting.directional.follow_camera_yaw",
        checked,
      )}
  />
  <ProfileToggleField
    label={$_("profiles.editor.lighting_follow_camera_pitch")}
    checked={followCameraPitch}
    disabled={busy || !enabled}
    onChange={(checked) =>
      onUpdateSettingValue(
        "environment.lighting.directional.follow_camera_pitch",
        checked,
      )}
  />
</div>
