<script lang="ts">
  import { _ } from "svelte-i18n";
  import type { RendererPaneActions } from "./rendererPaneActions";

  export let disabled = false;
  export let onSetCameraOrbitPreset: RendererPaneActions["onSetCameraOrbitPreset"];
  export let onSaveCamera: RendererPaneActions["onSaveCamera"];
  export let onRestoreCamera: RendererPaneActions["onRestoreCamera"];
</script>

<div class="runtime-button-row">
  <button
    {disabled}
    onclick={() => onSetCameraOrbitPreset("left")}
    title={$_("profiles.hints.camera.orbit_left")}
    >{$_("profiles.editor.options.camera_orbit_left")}</button
  >
  <button
    {disabled}
    onclick={() => onSetCameraOrbitPreset("front")}
    title={$_("profiles.hints.camera.orbit_front")}
    >{$_("profiles.editor.options.camera_orbit_front")}</button
  >
  <button
    {disabled}
    onclick={() => onSetCameraOrbitPreset("right")}
    title={$_("profiles.hints.camera.orbit_right")}
    >{$_("profiles.editor.options.camera_orbit_right")}</button
  >
</div>
<div class="runtime-button-row">
  <button
    {disabled}
    onclick={onSaveCamera}
    title={$_("renderers.controls.save_camera_title")}
    >{$_("renderers.details.save_to_profile")}</button
  >
  <button
    {disabled}
    onclick={onRestoreCamera}
    title={$_("renderers.controls.restore_camera_title")}
    >{$_("renderers.details.restore_from_profile")}</button
  >
</div>
