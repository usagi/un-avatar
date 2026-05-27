<script lang="ts">
  import { _ } from "svelte-i18n";
  import { runtimeMetric } from "./formatting";
  import { motionLabel, type MotionLabelData } from "./profileLabels";
  import type { RendererOverviewMotionStatus } from "./rendererTypes";

  export let renderer: MotionLabelData;
  export let runtimeStatus: RendererOverviewMotionStatus | null;
</script>

<section class="renderer-info-card">
  <div class="renderer-info-card-heading">
    <h3>{$_("renderers.details.motion_card")}</h3>
    <span>{motionLabel(renderer)}</span>
  </div>
  {#if runtimeStatus?.unmotion_zenoh_enabled}
    <dl class="renderer-card-kv">
      <dt>UNMF/Z</dt>
      <dd
        >recv {runtimeStatus.unmotion_zenoh_received_frames}
        ({runtimeMetric(runtimeStatus.unmotion_zenoh_received_fps)}
        fps) / apply {runtimeStatus.motion_applied_frames}
        ({runtimeMetric(runtimeStatus.motion_applied_fps)}
        fps)</dd
      >
    </dl>
  {/if}
</section>
