<script lang="ts">
	import { _ } from "svelte-i18n";
	import { localizedWindowLabel } from "./profileStageSummary";
	import RendererWindowActionButtons from "./RendererWindowActionButtons.svelte";
	import RendererWindowProfileButtons from "./RendererWindowProfileButtons.svelte";
	import RendererWindowStateTable from "./RendererWindowStateTable.svelte";
	import type { RendererWindowData, RendererWindowStatus } from "./rendererControlTypes";
	import type { RendererPaneActions } from "./rendererPaneActions";

	export let renderer: RendererWindowData;
	export let runtimeStatus: RendererWindowStatus | null;
	export let busy = false;
	export let onSetWindow: RendererPaneActions["onSetWindow"];
	export let onSaveWindow: RendererPaneActions["onSaveWindow"];
	export let onRestoreWindow: RendererPaneActions["onRestoreWindow"];

	$: rendererRunning = renderer.pid != null;
</script>

<section class="renderer-control-card renderer-control-window">
	<div class="renderer-control-card-heading">
		<h3>{$_("renderers.controls.window")}</h3>
		<span>{localizedWindowLabel(renderer, $_)}</span>
	</div>
	<RendererWindowActionButtons {renderer} {runtimeStatus} disabled={busy || !rendererRunning} {onSetWindow} />
	<RendererWindowStateTable {runtimeStatus} />
	<RendererWindowProfileButtons disabled={busy || !rendererRunning} {onSaveWindow} {onRestoreWindow} />
</section>
