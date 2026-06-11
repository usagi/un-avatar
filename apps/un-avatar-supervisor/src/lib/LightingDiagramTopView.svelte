<script lang="ts">
	import type { LightingDiagram } from "./profileDiagrams";

	export let diagram: LightingDiagram;
</script>

<svg viewBox="0 0 220 136" role="img">
	<title>Top view: X and Z axes</title>
	<defs>
		<marker id="lighting-arrow-top" markerWidth="6" markerHeight="6" refX="5" refY="3" orient="auto">
			<path d="M0,0 L6,3 L0,6 Z" class="lighting-arrow-head" />
		</marker>
	</defs>
	{#each diagram.gridOffsets as offset}
		<line
			x1={diagram.originX + offset * diagram.scale}
			y1="10"
			x2={diagram.originX + offset * diagram.scale}
			y2="126"
			class="camera-grid-line"
		/>
		<line
			x1="10"
			y1={diagram.originY + offset * diagram.scale}
			x2="210"
			y2={diagram.originY + offset * diagram.scale}
			class="camera-grid-line"
		/>
	{/each}
	<line x1="12" y1={diagram.originY} x2="208" y2={diagram.originY} class="camera-axis-line camera-axis-x" />
	<line x1={diagram.originX} y1="12" x2={diagram.originX} y2="124" class="camera-axis-line camera-axis-z" />
	<circle cx={diagram.originX} cy={diagram.originY} r="3" class="camera-origin-dot" />
	<line x1={diagram.rayEndX} y1={diagram.rayEndY} x2={diagram.basisEndX} y2={diagram.basisEndY} class="lighting-basis-line" />
	<line
		x1={diagram.rayStartX}
		y1={diagram.rayStartY}
		x2={diagram.rayEndX}
		y2={diagram.rayEndY}
		class="lighting-ray-line"
		marker-end="url(#lighting-arrow-top)"
	/>
	<line x1={diagram.cameraX} y1={diagram.cameraY} x2={diagram.targetX} y2={diagram.targetY} class="camera-diagram-line" />
	<circle cx={diagram.targetX} cy={diagram.targetY} r="6" class="camera-target-dot" />
	<circle cx={diagram.cameraX} cy={diagram.cameraY} r="5" class="camera-dot" />
	<text x={diagram.originX} y="12" class="camera-axis-label">Z-</text>
	<text x={diagram.originX} y="130" class="camera-axis-label">Z+</text>
	<text x="10" y={diagram.originY + 4} class="camera-axis-label">X-</text>
	<text x="198" y={diagram.originY + 4} class="camera-axis-label">X+</text>
</svg>
