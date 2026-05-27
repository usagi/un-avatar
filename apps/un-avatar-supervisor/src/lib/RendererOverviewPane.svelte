<script lang="ts">
  import RendererOverviewMotionCard from "./RendererOverviewMotionCard.svelte";
  import RendererOverviewProfileCard from "./RendererOverviewProfileCard.svelte";
  import RendererOverviewRenderingCard from "./RendererOverviewRenderingCard.svelte";
  import RendererOverviewRuntimeCard from "./RendererOverviewRuntimeCard.svelte";
  import type { RendererOverviewData, RendererOverviewStatus } from "./rendererTypes";

  export let renderer: RendererOverviewData;
  export let runtimeStatus: RendererOverviewStatus | null;
</script>

<div class="renderer-pane-scroll renderer-overview-list">
  <RendererOverviewProfileCard
    name={renderer.name}
    pid={renderer.pid}
    manifestPath={renderer.manifest_path}
    uptimeSecs={runtimeStatus?.uptime_secs ?? renderer.uptime_secs}
  />
  <RendererOverviewRuntimeCard {runtimeStatus} />
  <RendererOverviewRenderingCard {runtimeStatus} />
  <RendererOverviewMotionCard {renderer} {runtimeStatus} />
</div>
