<script lang="ts">
  import { _ } from "svelte-i18n";
  import { Trash2 } from "lucide-svelte";
  import type { RendererLogFilter } from "./rendererLogs";

  export let logsTextFilter: string;
  export let logsRendererFilter: RendererLogFilter;

  $: hasActiveFilter = logsTextFilter !== "" || logsRendererFilter !== "all";
</script>

<div class="logs-filter-group">
  <label class="logs-filter-field logs-filter-search">
    <span>{$_("logs.toolbar.filter")}</span>
    <input
      type="text"
      placeholder={$_("logs.toolbar.filter_placeholder")}
      bind:value={logsTextFilter}
    />
  </label>
  <button
    class="ghost-button logs-filter-clear"
    onclick={() => {
      logsTextFilter = "";
      logsRendererFilter = "all";
    }}
    disabled={!hasActiveFilter}
    title={$_("logs.toolbar.reset_title")}
  >
    <Trash2 size={14} />
    {$_("logs.toolbar.reset_filters")}
  </button>
</div>
