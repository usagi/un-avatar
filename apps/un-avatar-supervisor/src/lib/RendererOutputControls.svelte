<script lang="ts">
	import { _ } from "svelte-i18n";
	import RendererOutputActionButtons from "./RendererOutputActionButtons.svelte";
	import { runtimeOutputLabel, type RuntimeOutputStatusData } from "./runtimeLabels";
	import type { RendererOutputData } from "./rendererControlTypes";
	import type { RendererPaneActions } from "./rendererPaneActions";

	export let renderer: RendererOutputData;
	export let runtimeStatus: RuntimeOutputStatusData | null;
	export let busy = false;
	export let onSetSpoutOutput: RendererPaneActions["onSetSpoutOutput"];
	export let onSetWindow: RendererPaneActions["onSetWindow"];

	$: rendererRunning = renderer.pid != null;
</script>

<section class="renderer-control-card renderer-control-output">
	<div class="renderer-control-card-heading">
		<h3>{$_("renderers.controls.output")}</h3>
		<span>{runtimeOutputLabel(renderer, runtimeStatus)}</span>
	</div>
	<RendererOutputActionButtons {renderer} {runtimeStatus} disabled={busy || !rendererRunning} {onSetSpoutOutput} {onSetWindow} />
</section>
