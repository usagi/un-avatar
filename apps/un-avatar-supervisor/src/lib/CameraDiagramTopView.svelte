<script lang="ts">
  import type { CameraDiagram } from "./profileDiagrams";

  export let diagram: CameraDiagram;
  export let showPointLabels = false;
</script>

<svg viewBox="0 0 220 136" role="img">
  <title>Top view: X and Z axes</title>
  {#each diagram.gridOffsets as offset}
    <line
      x1={diagram.originX + offset * diagram.topScale}
      y1="10"
      x2={diagram.originX + offset * diagram.topScale}
      y2="126"
      class="camera-grid-line"
    />
    <line
      x1="10"
      y1={diagram.originY + offset * diagram.topScale}
      x2="210"
      y2={diagram.originY + offset * diagram.topScale}
      class="camera-grid-line"
    />
  {/each}
  <line x1="12" y1={diagram.originY} x2="208" y2={diagram.originY} class="camera-axis-line camera-axis-x" />
  <line x1={diagram.originX} y1="12" x2={diagram.originX} y2="124" class="camera-axis-line camera-axis-z" />
  <circle cx={diagram.originX} cy={diagram.originY} r="3" class="camera-origin-dot" />
  <ellipse cx={diagram.targetX} cy={diagram.targetY} rx={diagram.topOrbitRx} ry={diagram.topOrbitRy} class="camera-orbit-line" />
  <text x={diagram.originX} y="12" class="camera-axis-label">Z-</text>
  <text x={diagram.originX} y="130" class="camera-axis-label">Z+</text>
  <text x="10" y={diagram.originY + 4} class="camera-axis-label">X-</text>
  <text x="198" y={diagram.originY + 4} class="camera-axis-label">X+</text>
  <path
    d={`M ${diagram.cameraX} ${diagram.cameraY} L ${diagram.fovLeftX} ${diagram.fovLeftY} L ${diagram.fovRightX} ${diagram.fovRightY} Z`}
    class="camera-frustum-fill"
  />
  <line x1={diagram.cameraX} y1={diagram.cameraY} x2={diagram.fovLeftX} y2={diagram.fovLeftY} class="camera-frustum-line" />
  <line x1={diagram.cameraX} y1={diagram.cameraY} x2={diagram.fovRightX} y2={diagram.fovRightY} class="camera-frustum-line" />
  <line x1={diagram.cameraX} y1={diagram.cameraY} x2={diagram.targetX} y2={diagram.targetY} class="camera-diagram-line" />
  <circle cx={diagram.targetX} cy={diagram.targetY} r="8" class="camera-target-dot" />
  <circle cx={diagram.cameraX} cy={diagram.cameraY} r="7" class="camera-dot" />
  {#if showPointLabels}
    <text x={diagram.targetX + 10} y={diagram.targetY - 10} class="camera-point-label">Target</text>
    <text x={diagram.cameraX + 9} y={diagram.cameraY - 9} class="camera-point-label">Camera</text>
  {/if}
</svg>
