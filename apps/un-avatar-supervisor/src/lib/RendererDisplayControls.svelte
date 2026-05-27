<script lang="ts">
  import { _ } from "svelte-i18n";
  import RendererDisplayActionButtons from "./RendererDisplayActionButtons.svelte";
  import type { RendererDisplayData, RendererDisplayStatus } from "./rendererControlTypes";
  import type { RendererPaneActions } from "./rendererPaneActions";

  export let renderer: RendererDisplayData;
  export let runtimeStatus: RendererDisplayStatus | null;
  export let busy = false;
  export let onSetShowAxes: RendererPaneActions["onSetShowAxes"];
  export let onSetShowBoneColliders: RendererPaneActions["onSetShowBoneColliders"];
  export let onSetCameraLock: RendererPaneActions["onSetCameraLock"];

  $: rendererRunning = renderer.pid != null;
</script>

<section class="renderer-control-card renderer-control-display">
  <div class="renderer-control-card-heading">
    <h3>{$_("renderers.controls.display")}</h3>
    <span
      >{$_("renderers.controls.display_colliders", {
        values: { count: runtimeStatus?.bone_collider_count ?? 0 },
      })}</span
    >
  </div>
  <RendererDisplayActionButtons
    {runtimeStatus}
    disabled={busy || !rendererRunning}
    {onSetShowAxes}
    {onSetShowBoneColliders}
    {onSetCameraLock}
  />
</section>
