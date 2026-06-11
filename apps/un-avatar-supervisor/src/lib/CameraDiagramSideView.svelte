<script lang="ts">
	import type { CameraDiagram } from "./profileDiagrams";

	export let diagram: CameraDiagram;
	export let showPointLabels = false;
</script>

<svg viewBox="0 0 220 136" role="img">
	<title>Side view from X+: Z and Y axes</title>
	{#each diagram.gridOffsets as offset}
		<line
			x1={diagram.sideOriginX - offset * diagram.sideScale}
			y1="10"
			x2={diagram.sideOriginX - offset * diagram.sideScale}
			y2="126"
			class="camera-grid-line"
		/>
		<line
			x1="10"
			y1={diagram.sideOriginY - offset * diagram.sideScale}
			x2="210"
			y2={diagram.sideOriginY - offset * diagram.sideScale}
			class="camera-grid-line"
		/>
	{/each}
	<line x1="12" y1={diagram.sideOriginY} x2="208" y2={diagram.sideOriginY} class="camera-axis-line camera-axis-z" />
	<line x1={diagram.sideOriginX} y1="12" x2={diagram.sideOriginX} y2="124" class="camera-axis-line camera-axis-y" />
	<circle cx={diagram.sideOriginX} cy={diagram.sideOriginY} r="3" class="camera-origin-dot" />
	<ellipse
		cx={diagram.sideTargetX}
		cy={diagram.sideTargetY}
		rx={diagram.sideOrbitRx}
		ry={diagram.sideOrbitRy}
		class="camera-orbit-line"
	/>
	<text x="18" y={diagram.sideOriginY + 4} class="camera-axis-label">Z+</text>
	<text x="190" y={diagram.sideOriginY + 4} class="camera-axis-label">Z-</text>
	<text x={diagram.sideOriginX} y="14" class="camera-axis-label">Y+</text>
	<text x={diagram.sideOriginX} y="130" class="camera-axis-label">Y-</text>
	<path
		d={`M ${diagram.sideCameraX} ${diagram.sideCameraY} L ${diagram.sideFovLeftX} ${diagram.sideFovLeftY} L ${diagram.sideFovRightX} ${diagram.sideFovRightY} Z`}
		class="camera-frustum-fill"
	/>
	<line
		x1={diagram.sideCameraX}
		y1={diagram.sideCameraY}
		x2={diagram.sideFovLeftX}
		y2={diagram.sideFovLeftY}
		class="camera-frustum-line"
	/>
	<line
		x1={diagram.sideCameraX}
		y1={diagram.sideCameraY}
		x2={diagram.sideFovRightX}
		y2={diagram.sideFovRightY}
		class="camera-frustum-line"
	/>
	<line x1={diagram.sideCameraX} y1={diagram.sideCameraY} x2={diagram.sideTargetX} y2={diagram.sideTargetY} class="camera-diagram-line" />
	<circle cx={diagram.sideTargetX} cy={diagram.sideTargetY} r="8" class="camera-target-dot" />
	<circle cx={diagram.sideCameraX} cy={diagram.sideCameraY} r="7" class="camera-dot" />
	{#if showPointLabels}
		<text x={diagram.sideTargetX + 10} y={diagram.sideTargetY - 10} class="camera-point-label">Target</text>
		<text x={diagram.sideCameraX + 9} y={diagram.sideCameraY - 9} class="camera-point-label">Camera</text>
	{/if}
</svg>
