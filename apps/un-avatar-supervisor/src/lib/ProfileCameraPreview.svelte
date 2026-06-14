<script lang="ts">
	import { _ } from "svelte-i18n";
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
	targetYLabel={`${$_("profiles.editor.camera_target_position")} Y ${formatFixed(setting.camera_target?.[1] ?? 0)} m`}
	radiusLabel={`${$_("profiles.editor.camera_radius")} ${diagram.radiusLabel}`}
	fovLabel={`FOV ${diagram.fovLabel}`}
/>
