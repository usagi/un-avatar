<script lang="ts">
  import { _ } from "svelte-i18n";
  import {
    rendererLogFilterFromValue,
    type RendererLogData,
    type RendererLogFilter,
  } from "./rendererLogs";

  export let renderers: RendererLogData[];
  export let logsRendererFilter: RendererLogFilter;
</script>

<label class="logs-filter-field logs-filter-field--select">
  <span>{$_("logs.toolbar.renderer")}</span>
  <select
    value={String(logsRendererFilter)}
    onchange={(e) => {
      logsRendererFilter = rendererLogFilterFromValue(
        (e.currentTarget as HTMLSelectElement).value,
      );
    }}
  >
    <option value="all">{$_("logs.toolbar.all_renderers")}</option>
    {#each renderers as renderer (renderer.id)}
      <option value={String(renderer.id)}>#{renderer.id} {renderer.name}</option>
    {/each}
  </select>
</label>
