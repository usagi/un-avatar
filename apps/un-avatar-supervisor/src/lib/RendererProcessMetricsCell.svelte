<script lang="ts">
  import { _ } from "svelte-i18n";
  import {
    fpsHealthClass,
    gpuHealthClass,
    ramHealthClass,
    runtimeMetric,
  } from "./formatting";
  import {
    startupProgressPercent,
    startupStatusLabel,
    type RuntimeTableStatusData,
  } from "./runtimeLabels";

  export let status: RuntimeTableStatusData | null;

  $: startupLabel = startupStatusLabel(status);
</script>

<td class="process-cell-perf">
  {#if startupLabel}
    <span>{startupLabel}</span>
  {:else}
    <div class="process-metric-row">
      <span class={fpsHealthClass(status?.fps)}
        ><strong>{runtimeMetric(status?.fps)}</strong><small>{$_("renderers.metrics.fps")}</small></span
      >
      <span class={gpuHealthClass(status?.gpu_ms)}
        ><strong>{runtimeMetric(status?.gpu_ms, " ms")}</strong><small>{$_("renderers.metrics.gpu")}</small></span
      >
      <span class={ramHealthClass(status?.ram_mb)}
        ><strong>{runtimeMetric(status?.ram_mb, " MB")}</strong><small>{$_("renderers.metrics.ram")}</small></span
      >
    </div>
  {/if}
  <small>{$_("renderers.metrics.cpu")} {runtimeMetric(status?.cpu_ms, " ms")}</small>
  {#if startupLabel}
    <div
      class="startup-progress"
      class:indeterminate={!status?.startup_progress || status.startup_progress[1] <= 0}
    >
      <span style={`width: ${startupProgressPercent(status).toFixed(1)}%`}></span>
    </div>
  {/if}
</td>
