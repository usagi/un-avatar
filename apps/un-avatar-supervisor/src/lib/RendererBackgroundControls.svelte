<script lang="ts">
	import { _ } from "svelte-i18n";
	import ColorField from "../ColorField.svelte";
	import {
		RENDERER_CLEAR_COLOR_FALLBACK,
		rendererClearColorRgb,
		type RendererBackgroundData,
		type RendererBackgroundStatus,
	} from "./rendererControlTypes";
	import type { RendererPaneActions } from "./rendererPaneActions";
	import type { ColorDisplayMode } from "./storageState";

	export let renderer: RendererBackgroundData;
	export let runtimeStatus: RendererBackgroundStatus | null;
	export let busy = false;
	export let colorDisplayMode: ColorDisplayMode;
	export let onSetClearColor: RendererPaneActions["onSetClearColor"];
	export let onColorModeChange: RendererPaneActions["onColorModeChange"];

	$: rendererRunning = renderer.pid != null;
	$: clearColor = rendererClearColorRgb(runtimeStatus);
</script>

<section class="renderer-control-card renderer-control-background">
	<div class="renderer-control-card-heading">
		<h3>{$_("renderers.controls.background")}</h3>
	</div>
	<ColorField
		label={$_("renderers.controls.runtime_background")}
		value={clearColor}
		fallback={RENDERER_CLEAR_COLOR_FALLBACK}
		disabled={busy || !rendererRunning}
		mode={colorDisplayMode}
		onModeChange={onColorModeChange}
		onChange={([r, g, b]) => onSetClearColor([r, g, b])}
	/>
</section>
