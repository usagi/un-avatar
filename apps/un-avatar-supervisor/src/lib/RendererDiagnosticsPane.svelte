<script lang="ts">
  import { _ } from "svelte-i18n";
  import type {
    RendererDiagnosticsData,
    RendererRuntimeDiagnosticsData,
  } from "./rendererTypes";

  export let renderer: RendererDiagnosticsData;
  export let runtimeStatus: RendererRuntimeDiagnosticsData | null;
</script>

<div class="renderer-pane-scroll">
  <dl>
    <dt>{$_("renderers.details.diag_last_stderr")}</dt>
    <dd class="stderr-block">
      {renderer.last_stderr ?? $_("renderers.details.value_none")}
    </dd>
    {#if renderer.stderr_tail?.length}
      <dt>{$_("renderers.details.diag_stderr_tail")}</dt>
      <dd class="stderr-block">
        {renderer.stderr_tail.join("\n")}
      </dd>
    {/if}
    {#if renderer.exit_code != null}
      <dt>{$_("renderers.details.diag_exit_code")}</dt>
      <dd>{renderer.exit_code}</dd>
    {/if}
    {#if runtimeStatus?.note}
      <dt>{$_("renderers.details.diag_runtime_note")}</dt>
      <dd>{runtimeStatus.note}</dd>
    {/if}
  </dl>
</div>
