<script lang="ts">
	import { _ } from "svelte-i18n";
	import type { LightingDiagram } from "./profileDiagrams";

	export let diagram: LightingDiagram;
</script>

<svg viewBox="0 0 220 136" role="img">
	<title>{$_("profiles.editor.diagram_side_view_title")}</title>
	<defs>
		<marker id="lighting-arrow-side" markerWidth="6" markerHeight="6" refX="5" refY="3" orient="auto">
			<path d="M0,0 L6,3 L0,6 Z" class="lighting-arrow-head" />
		</marker>
	</defs>
	{#each diagram.gridOffsets as offset}
		<line
			x1={diagram.sideOriginX - offset * diagram.scale}
			y1="10"
			x2={diagram.sideOriginX - offset * diagram.scale}
			y2="126"
			class="camera-grid-line"
		/>
		<line
			x1="10"
			y1={diagram.sideOriginY - offset * diagram.scale}
			x2="210"
			y2={diagram.sideOriginY - offset * diagram.scale}
			class="camera-grid-line"
		/>
	{/each}
	<line x1="12" y1={diagram.sideOriginY} x2="208" y2={diagram.sideOriginY} class="camera-axis-line camera-axis-z" />
	<line x1={diagram.sideOriginX} y1="12" x2={diagram.sideOriginX} y2="124" class="camera-axis-line camera-axis-y" />
	<circle cx={diagram.sideOriginX} cy={diagram.sideOriginY} r="3" class="camera-origin-dot" />
	<line
		x1={diagram.sideRayEndX}
		y1={diagram.sideRayEndY}
		x2={diagram.sideProjectionX}
		y2={diagram.sideProjectionY}
		class="lighting-projection-line"
	/>
	<line
		x1={diagram.sideRayStartX}
		y1={diagram.sideRayStartY}
		x2={diagram.sideRayEndX}
		y2={diagram.sideRayEndY}
		class="lighting-ray-line"
		marker-end="url(#lighting-arrow-side)"
	/>
	<line x1={diagram.sideCameraX} y1={diagram.sideCameraY} x2={diagram.sideTargetX} y2={diagram.sideTargetY} class="camera-diagram-line" />
	<circle cx={diagram.sideTargetX} cy={diagram.sideTargetY} r="6" class="camera-target-dot" />
	<circle cx={diagram.sideCameraX} cy={diagram.sideCameraY} r="5" class="camera-dot" />
	<text x="18" y={diagram.sideOriginY + 4} class="camera-axis-label">Z+</text>
	<text x="190" y={diagram.sideOriginY + 4} class="camera-axis-label">Z-</text>
	<text x={diagram.sideOriginX} y="14" class="camera-axis-label">Y+</text>
	<text x={diagram.sideOriginX} y="130" class="camera-axis-label">Y-</text>
</svg>
