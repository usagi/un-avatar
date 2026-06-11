<script lang="ts">
	import type { LightingDiagram } from "./profileDiagrams";
	import LightingDiagramSideView from "./LightingDiagramSideView.svelte";
	import LightingDiagramTopView from "./LightingDiagramTopView.svelte";

	export let diagram: LightingDiagram;
	export let title: string;
	export let basisLabel: string;
	export let azimuthLabel: string;
	export let elevationLabel: string;
	export let basisLegend: string;
	export let projectionLegend: string;
	export let rayLegend: string;
</script>

<div class="camera-diagram lighting-diagram" aria-label="Lighting configuration preview">
	<LightingDiagramTopView {diagram} />
	<LightingDiagramSideView {diagram} />
	<div class="camera-legend">
		<strong>{title}</strong>
		<span>{basisLabel}</span>
		<span>{azimuthLabel}</span>
		<span>{elevationLabel}</span>
		<span><i class="legend-line legend-grid"></i>1m grid</span>
		<span><i class="legend-line legend-lighting-basis"></i>{basisLegend}</span>
		<span><i class="legend-line legend-lighting-projection"></i>{projectionLegend}</span>
		<span><i class="legend-line legend-lighting-ray"></i>{rayLegend}</span>
		<span><i class="legend-dot legend-camera"></i>camera</span>
		<span><i class="legend-dot legend-target"></i>target</span>
	</div>
</div>
