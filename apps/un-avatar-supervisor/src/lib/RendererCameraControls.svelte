<script lang="ts">
	import { _ } from "svelte-i18n";
	import RendererCameraActionRows from "./RendererCameraActionRows.svelte";
	import RendererCameraPreview from "./RendererCameraPreview.svelte";
	import RendererCameraStateTable from "./RendererCameraStateTable.svelte";
	import { rendererCameraStatusValues, type RendererCameraData, type RendererCameraStatus } from "./rendererControlTypes";
	import type { RendererPaneActions } from "./rendererPaneActions";

	export let renderer: RendererCameraData;
	export let runtimeStatus: RendererCameraStatus | null;
	export let busy = false;
	export let onSetCameraOrbitPreset: RendererPaneActions["onSetCameraOrbitPreset"];
	export let onSaveCamera: RendererPaneActions["onSaveCamera"];
	export let onRestoreCamera: RendererPaneActions["onRestoreCamera"];

	$: rendererRunning = renderer.pid != null;
</script>

<section class="renderer-control-card renderer-control-camera">
	{#if runtimeStatus?.camera}
		{@const camera = runtimeStatus.camera}
		<div class="renderer-control-card-heading">
			<h3>{$_("renderers.controls.camera")}</h3>
			<span
				>{$_("renderers.controls.camera_status", {
					values: {
						...rendererCameraStatusValues(camera),
					},
				})}</span
			>
		</div>
		<RendererCameraPreview
			{camera}
			windowWidth={runtimeStatus.window_inner_size?.[0] ?? renderer.window_width}
			windowHeight={runtimeStatus.window_inner_size?.[1] ?? renderer.window_height}
		/>
		<div class="camera-grid">
			<RendererCameraStateTable {camera} />
			<RendererCameraActionRows disabled={busy || !rendererRunning} {onSetCameraOrbitPreset} {onSaveCamera} {onRestoreCamera} />
		</div>
	{:else}
		<div class="renderer-control-card-heading">
			<h3>{$_("renderers.controls.camera")}</h3>
		</div>
		<span class="muted-small">{$_("renderers.controls.camera_unavailable")}</span>
	{/if}
</section>
