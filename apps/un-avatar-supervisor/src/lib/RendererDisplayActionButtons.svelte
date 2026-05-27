<script lang="ts">
  import { _ } from "svelte-i18n";
  import type { RendererDisplayStatus } from "./rendererControlTypes";

  export let runtimeStatus: RendererDisplayStatus | null;
  export let disabled = false;
  export let onSetShowAxes: (enabled: boolean) => void;
  export let onSetShowBoneColliders: (enabled: boolean) => void;
  export let onSetCameraLock: (enabled: boolean) => void;
</script>

<div class="runtime-button-row">
  <button
    class:active={runtimeStatus?.show_axes}
    {disabled}
    onclick={() => onSetShowAxes(!(runtimeStatus?.show_axes ?? false))}
    title={$_("renderers.details.show_axes_title")}
    >{$_("renderers.details.show_axes")}</button
  >
  <button
    class:active={runtimeStatus?.show_bone_colliders}
    {disabled}
    onclick={() =>
      onSetShowBoneColliders(!(runtimeStatus?.show_bone_colliders ?? false))}
    title={$_("renderers.details.show_bone_colliders_title", {
      values: { count: runtimeStatus?.bone_collider_count ?? 0 },
    })}
    >{$_("renderers.details.show_bone_colliders")}</button
  >
  <button
    class:active={runtimeStatus?.camera_locked}
    {disabled}
    onclick={() => onSetCameraLock(!(runtimeStatus?.camera_locked ?? false))}
    title={$_("renderers.details.camera_lock_title")}
    >{$_("renderers.details.camera_lock")}</button
  >
</div>
