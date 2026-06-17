<script lang="ts">
	import { _ } from "svelte-i18n";
	import LightingDiagramPreview from "./LightingDiagramPreview.svelte";
	import { lightingDiagram } from "./profileDiagrams";
	import type { LightingSetting } from "./profileTypes";

	export let setting: LightingSetting;

	$: diagram = lightingDiagram(setting);
</script>

<LightingDiagramPreview
	{diagram}
	title={$_("profiles.editor.lighting_preview")}
	basisLabel={`${$_("profiles.editor.lighting_basis")}: ${diagram.basisLabel}`}
	azimuthLabel={`${$_("profiles.editor.lighting_azimuth")}: ${diagram.azimuthLabel}`}
	elevationLabel={`${$_("profiles.editor.lighting_elevation")}: ${diagram.elevationLabel}`}
	basisLegend={$_("profiles.editor.lighting_basis")}
	projectionLegend={$_("profiles.editor.lighting_projection")}
	rayLegend={$_("profiles.editor.lighting_ray")}
/>
