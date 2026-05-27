<script lang="ts">
  import { _ } from "svelte-i18n";
  import CameraAxisFieldset from "./CameraAxisFieldset.svelte";
  import CameraPresetRow from "./CameraPresetRow.svelte";
  import type { CameraOrbitPreset } from "./cameraPresets";
  import { cameraOrbitAxisFields, cameraOrbitPresetOptions } from "./cameraSectionFields";
  import type { CameraSetting, ProfileSettingValue } from "./profileTypes";

  export let setting: CameraSetting;
  export let busy = false;
  export let onUpdateSettingValue: (
    field: string,
    value: ProfileSettingValue,
  ) => void | Promise<void>;
  export let onApplyOrbitPreset: (preset: CameraOrbitPreset) => void | Promise<void>;

  $: orbitPresetOptions = cameraOrbitPresetOptions($_);
  $: orbitAxisFields = cameraOrbitAxisFields(setting, $_);
</script>

<CameraPresetRow
  title={$_("profiles.editor.camera_orbit")}
  ariaLabel="Camera orbit presets"
  className="orbit-preset-row"
  {busy}
  options={orbitPresetOptions}
  onApply={(preset) => onApplyOrbitPreset(preset as CameraOrbitPreset)}
/>
<CameraAxisFieldset
  legend={$_("profiles.editor.camera_orbit_angle")}
  hint={$_("profiles.hints.camera.orbit_angle")}
  className="camera-orbit-control"
  unit="deg"
  fields={orbitAxisFields}
  onChange={onUpdateSettingValue}
/>
