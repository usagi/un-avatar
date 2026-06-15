<script lang="ts">
	import { _ } from "svelte-i18n";
	import type { RendererOutputData, RendererOutputStatus } from "./rendererControlTypes";
	import type { RendererPaneActions } from "./rendererPaneActions";

	export let renderer: Pick<RendererOutputData, "spout_enabled" | "minimized">;
	export let runtimeStatus: RendererOutputStatus | null;
	export let disabled = false;
	export let onSetSpoutOutput: RendererPaneActions["onSetSpoutOutput"];
	export let onSetWindow: RendererPaneActions["onSetWindow"];

	$: spoutDisabled = disabled || (runtimeStatus?.connected === true && !runtimeStatus.spout_available);
	$: windowPreviewActive = runtimeStatus?.connected ? !runtimeStatus.spout_enabled && !runtimeStatus.minimized : !renderer.spout_enabled && !renderer.minimized;
	$: spoutPreviewActive = runtimeStatus?.connected ? runtimeStatus.spout_enabled && !runtimeStatus.minimized : renderer.spout_enabled && !renderer.minimized;
	$: spoutOnlyActive = runtimeStatus?.connected ? runtimeStatus.spout_enabled && Boolean(runtimeStatus.minimized) : renderer.spout_enabled && Boolean(renderer.minimized);

	async function setWindowPreview(): Promise<void> {
		await onSetWindow({ minimized: false }, "window preview");
		await onSetSpoutOutput(false, null, "window preview");
	}

	async function setSpoutPreview(): Promise<void> {
		await onSetSpoutOutput(true, null, "spout2 preview");
		await onSetWindow({ minimized: false }, "spout2 preview");
	}

	async function setSpoutOnly(): Promise<void> {
		await onSetSpoutOutput(true, null, "spout2 only");
		await onSetWindow({ minimized: true }, "spout2 only");
	}
</script>

<div class="runtime-button-row">
	<button class:active={windowPreviewActive} disabled={disabled} onclick={() => setWindowPreview()}>{$_("renderers.controls.window_preview")}</button>
	<button class:active={spoutPreviewActive} disabled={spoutDisabled} onclick={() => setSpoutPreview()}>{$_("renderers.controls.spout_preview")}</button>
	<button disabled={spoutDisabled} onclick={() => onSetSpoutOutput(true, { width: 1280, height: 720 }, "720p")}>720p</button>
	<button disabled={spoutDisabled} onclick={() => onSetSpoutOutput(true, { width: 1920, height: 1080 }, "1080p")}>1080p</button>
	<button class:active={spoutOnlyActive} disabled={spoutDisabled} onclick={() => setSpoutOnly()} title={$_("renderers.controls.spout_only_title")}
		>{$_("renderers.controls.spout_only")}</button
	>
</div>
