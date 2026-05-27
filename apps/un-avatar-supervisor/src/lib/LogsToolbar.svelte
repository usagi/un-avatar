<script lang="ts">
  import { _ } from "svelte-i18n";
  import LogsLayoutSwitch from "./LogsLayoutSwitch.svelte";
  import LogsRendererFilterField from "./LogsRendererFilterField.svelte";
  import LogsTextFilterGroup from "./LogsTextFilterGroup.svelte";
  import LogsToolbarActions from "./LogsToolbarActions.svelte";
  import type { RendererLogData, RendererLogFilter } from "./rendererLogs";

  export let renderers: RendererLogData[];
  export let rendererLogsLayout: "per-renderer" | "unified";
  export let logsRendererFilter: RendererLogFilter;
  export let logsTextFilter: string;
  export let logsAutoscroll: boolean;
  export let rendererLogsCopyFlash = false;
  export let onCopyAllRendererLogs: () => void;
  export let onSaveAllRendererLogs: () => void;
  export let onRevealSupervisorLogsDir: () => void;

</script>

<div class="toolbar logs-toolbar">
  <LogsLayoutSwitch bind:rendererLogsLayout />
  {#if rendererLogsLayout === "unified"}
    <LogsRendererFilterField {renderers} bind:logsRendererFilter />
  {/if}
  <LogsTextFilterGroup bind:logsTextFilter bind:logsRendererFilter />
  <label class="logs-filter-field toggle-field">
    <input type="checkbox" bind:checked={logsAutoscroll} />
    <span>{$_("logs.toolbar.autoscroll")}</span>
  </label>
  <LogsToolbarActions
    disabled={renderers.length === 0}
    {rendererLogsCopyFlash}
    {onCopyAllRendererLogs}
    {onSaveAllRendererLogs}
    {onRevealSupervisorLogsDir}
  />
</div>
