<script lang="ts">
  import type { CameraDiagram } from "./profileDiagrams";
  import CameraDiagramSideView from "./CameraDiagramSideView.svelte";
  import CameraDiagramTopView from "./CameraDiagramTopView.svelte";

  export let diagram: CameraDiagram;
  export let className = "";
  export let ariaLabel = "Camera configuration preview";
  export let showPointLabels = false;
  export let title = "Camera preview";
  export let targetYLabel: string;
  export let radiusLabel: string;
  export let fovLabel: string;
</script>

<div class={`camera-diagram ${className}`.trim()} aria-label={ariaLabel}>
  <CameraDiagramTopView {diagram} {showPointLabels} />
  <CameraDiagramSideView {diagram} {showPointLabels} />
  <div class="camera-legend">
    <strong>{title}</strong>
    <span>{targetYLabel}</span>
    <span>{radiusLabel}</span>
    <span>{fovLabel}</span>
    <span><i class="legend-line legend-grid"></i>1m grid</span>
    <span><i class="legend-line legend-orbit"></i>orbit radius</span>
    <span><i class="legend-line legend-frustum"></i>view frustum</span>
    <span><i class="legend-dot legend-origin"></i>world origin</span>
    <span><i class="legend-dot legend-target"></i>model target</span>
    <span><i class="legend-dot legend-camera"></i>camera position</span>
  </div>
</div>
