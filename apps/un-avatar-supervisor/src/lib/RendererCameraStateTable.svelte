<script lang="ts">
	import { _ } from "svelte-i18n";
	import { rendererCameraOrbitLabel, rendererCameraTargetLabel } from "./rendererControlTypes";
	import type { RendererCameraSnapshot } from "./rendererTypes";

	export let camera: RendererCameraSnapshot;
</script>

<table class="camera-state-table">
	<tbody>
		<tr>
			<th>{$_("renderers.controls.camera_target")}</th>
			<td>{rendererCameraTargetLabel(camera)}</td>
		</tr>
		<tr>
			<th>{$_("renderers.controls.camera_orbit")}</th>
			<td>{rendererCameraOrbitLabel(camera)}</td>
		</tr>
		<tr>
			<th>{$_("renderers.controls.camera_radius")}</th>
			<td>{camera.radius.toFixed(2)}</td>
		</tr>
	</tbody>
</table>
