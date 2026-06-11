<script lang="ts">
	import CameraDiagramPreview from "./CameraDiagramPreview.svelte";
	import { formatFixed } from "./formatting";
	import { cameraDiagram } from "./profileDiagrams";
	import type { CameraSetting } from "./profileTypes";

	export let setting: CameraSetting;

	$: diagram = cameraDiagram(setting);
</script>

<CameraDiagramPreview
	{diagram}
	showPointLabels={true}
	targetYLabel={`Target Y ${formatFixed(setting.camera_target?.[1] ?? 0)} m`}
	radiusLabel={`Radius ${diagram.radiusLabel}`}
	fovLabel={`FOV ${diagram.fovLabel}`}
/>
