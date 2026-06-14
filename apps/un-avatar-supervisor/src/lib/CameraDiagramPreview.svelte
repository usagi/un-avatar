<script lang="ts">
	import { _ } from "svelte-i18n";
	import type { CameraDiagram } from "./profileDiagrams";
	import CameraDiagramSideView from "./CameraDiagramSideView.svelte";
	import CameraDiagramTopView from "./CameraDiagramTopView.svelte";

	export let diagram: CameraDiagram;
	export let className = "";
	export let ariaLabel: string | null = null;
	export let showPointLabels = false;
	export let title: string | null = null;
	export let targetYLabel: string;
	export let radiusLabel: string;
	export let fovLabel: string;
</script>

<div class={`camera-diagram ${className}`.trim()} aria-label={ariaLabel ?? $_("profiles.editor.camera_preview_aria")}>
	<CameraDiagramTopView {diagram} {showPointLabels} />
	<CameraDiagramSideView {diagram} {showPointLabels} />
	<div class="camera-legend">
		<strong>{title ?? $_("profiles.editor.camera_preview")}</strong>
		<span>{targetYLabel}</span>
		<span>{radiusLabel}</span>
		<span>{fovLabel}</span>
		<span><i class="legend-line legend-grid"></i>{$_("profiles.editor.diagram_grid")}</span>
		<span><i class="legend-line legend-orbit"></i>{$_("profiles.editor.diagram_orbit_radius")}</span>
		<span><i class="legend-line legend-frustum"></i>{$_("profiles.editor.diagram_view_frustum")}</span>
		<span><i class="legend-dot legend-origin"></i>{$_("profiles.editor.diagram_world_origin")}</span>
		<span><i class="legend-dot legend-target"></i>{$_("profiles.editor.diagram_model_target")}</span>
		<span><i class="legend-dot legend-camera"></i>{$_("profiles.editor.diagram_camera_position")}</span>
	</div>
</div>
