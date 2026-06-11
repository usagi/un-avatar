<script lang="ts">
	import CameraDiagramPreview from "./CameraDiagramPreview.svelte";
	import { formatFixed } from "./formatting";
	import { cameraDiagram } from "./profileDiagrams";
	import type { RendererCameraSnapshot } from "./rendererTypes";

	export let camera: RendererCameraSnapshot;
	export let windowWidth: number;
	export let windowHeight: number;

	$: diagram = cameraDiagram({
		camera_target: camera.target,
		camera_longitude_deg: camera.longitude_deg,
		camera_latitude_deg: camera.latitude_deg,
		camera_radius: camera.radius,
		camera_diagonal_fov_deg: camera.diagonal_fov_deg,
		window_width: windowWidth,
		window_height: windowHeight,
	});
</script>

<CameraDiagramPreview
	{diagram}
	className="runtime-camera-diagram"
	ariaLabel="Runtime camera preview"
	targetYLabel={`Target Y ${formatFixed(camera.target[1])} m`}
	radiusLabel={`Radius ${diagram.radiusLabel}`}
	fovLabel={`FOV ${diagram.fovLabel}`}
/>
