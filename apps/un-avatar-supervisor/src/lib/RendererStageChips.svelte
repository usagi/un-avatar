<script lang="ts">
  import { _ } from "svelte-i18n";
  import { motionLabel, windowLabel } from "./profileLabels";
  import {
    rendererHealthKind,
    rendererHealthLabel,
    runtimeAaLabel,
    runtimeOutputLabel,
    runtimeResolution,
    type RuntimeStageStatusData,
  } from "./runtimeLabels";
  import type { RendererStageView } from "./rendererTypes";

  export let renderer: RendererStageView;
  export let runtimeStatus: RuntimeStageStatusData | null;

  $: health = rendererHealthKind(renderer, runtimeStatus);
</script>

<div class="stage-chip-row">
  <span class={`runtime-chip-health-${health}`}
    >{rendererHealthLabel(renderer, runtimeStatus, {
      pending: $_("renderers.summary.pending"),
      connected: $_("renderers.summary.connected"),
    })}</span
  >
  <span>{motionLabel(renderer)}</span>
  <span>{runtimeOutputLabel(renderer, runtimeStatus)}</span>
  <span>{windowLabel(renderer)}</span>
  <span>{runtimeResolution(runtimeStatus)}</span>
  <span>{runtimeAaLabel(runtimeStatus)}</span>
</div>
