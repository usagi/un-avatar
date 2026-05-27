<script lang="ts">
  import { _ } from "svelte-i18n";
  import ProfileRangeNumberField from "./ProfileRangeNumberField.svelte";
  import type { ProfileSettingValue } from "./profileTypes";

  export let enabled = false;
  export let azimuthDeg = 0;
  export let elevationDeg = 0;
  export let busy = false;
  export let onUpdateSettingValue: (
    field: string,
    value: ProfileSettingValue,
  ) => void | Promise<void>;
</script>

<div class="lighting-control-cluster lighting-angle-cluster">
  <span class="lighting-cluster-title">{$_("profiles.editor.lighting_direction")}</span>
  <ProfileRangeNumberField
    label={$_("profiles.editor.lighting_azimuth")}
    value={azimuthDeg}
    rangeMin={-180}
    rangeMax={180}
    numberMin={-360}
    numberMax={360}
    step={1}
    disabled={busy || !enabled}
    onChange={(value) =>
      onUpdateSettingValue(
        "environment.lighting.directional.azimuth_deg",
        value,
      )}
  />
  <ProfileRangeNumberField
    label={$_("profiles.editor.lighting_elevation")}
    value={elevationDeg}
    rangeMin={-30}
    rangeMax={80}
    numberMin={-89}
    numberMax={89}
    step={1}
    disabled={busy || !enabled}
    onChange={(value) =>
      onUpdateSettingValue(
        "environment.lighting.directional.elevation_deg",
        value,
      )}
  />
</div>
