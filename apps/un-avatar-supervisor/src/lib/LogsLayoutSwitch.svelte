<script lang="ts">
  import { _ } from "svelte-i18n";

  export let rendererLogsLayout: "per-renderer" | "unified";
</script>

<div class="segmented-control logs-layout-switch" aria-label="Renderer logs layout">
  <button class:active={rendererLogsLayout === "per-renderer"} onclick={() => (rendererLogsLayout = "per-renderer")}>
    {$_("logs.toolbar.per_renderer")}
  </button>
  <button class:active={rendererLogsLayout === "unified"} onclick={() => (rendererLogsLayout = "unified")}>
    {$_("logs.toolbar.unified")}
  </button>
</div>
