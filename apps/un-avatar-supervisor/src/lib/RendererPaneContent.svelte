<script lang="ts">
	import RendererControlsPane from "./RendererControlsPane.svelte";
	import RendererDiagnosticsPane from "./RendererDiagnosticsPane.svelte";
	import RendererExpressionsPane from "./RendererExpressionsPane.svelte";
	import RendererOverviewPane from "./RendererOverviewPane.svelte";
	import type { ExpressionOverrides } from "./rendererExpressions";
	import type { RendererPaneActions } from "./rendererPaneActions";
	import type { RendererInstance, RendererPaneTab, RendererRuntimeStatus } from "./rendererTypes";
	import type { ColorDisplayMode } from "./storageState";

	export let renderer: RendererInstance;
	export let runtimeStatus: RendererRuntimeStatus | null;
	export let rendererPaneTab: RendererPaneTab;
	export let busy = false;
	export let canMatchSpoutToWindow = false;
	export let colorDisplayMode: ColorDisplayMode;
	export let expressionOverrides: ExpressionOverrides = {};
	export let expressionFilter = "";
	export let onSetSpoutOutput: RendererPaneActions["onSetSpoutOutput"];
	export let onMatchSpoutToWindow: RendererPaneActions["onMatchSpoutToWindow"];
	export let onSetWindow: RendererPaneActions["onSetWindow"];
	export let onSaveWindow: RendererPaneActions["onSaveWindow"];
	export let onRestoreWindow: RendererPaneActions["onRestoreWindow"];
	export let onSetShowAxes: RendererPaneActions["onSetShowAxes"];
	export let onSetShowBoneColliders: RendererPaneActions["onSetShowBoneColliders"];
	export let onSetCameraLock: RendererPaneActions["onSetCameraLock"];
	export let onSetCameraOrbitPreset: RendererPaneActions["onSetCameraOrbitPreset"];
	export let onSaveCamera: RendererPaneActions["onSaveCamera"];
	export let onRestoreCamera: RendererPaneActions["onRestoreCamera"];
	export let onSetClearColor: RendererPaneActions["onSetClearColor"];
	export let onColorModeChange: RendererPaneActions["onColorModeChange"];
	export let onClearExpressionOverrides: RendererPaneActions["onClearExpressionOverrides"];
	export let onSetExpressionOverride: RendererPaneActions["onSetExpressionOverride"];
	export let onActivateWardrobeMenuCandidate: RendererPaneActions["onActivateWardrobeMenuCandidate"];
	export let onSetDynamicsEnabled: RendererPaneActions["onSetDynamicsEnabled"];
</script>

{#if rendererPaneTab === "overview"}
	<RendererOverviewPane {renderer} {runtimeStatus} />
{:else if rendererPaneTab === "controls"}
	<RendererControlsPane
		{renderer}
		{runtimeStatus}
		{busy}
		{canMatchSpoutToWindow}
		{colorDisplayMode}
		{onSetSpoutOutput}
		{onMatchSpoutToWindow}
		{onSetWindow}
		{onSaveWindow}
		{onRestoreWindow}
		{onSetShowAxes}
		{onSetShowBoneColliders}
		{onSetCameraLock}
		{onSetCameraOrbitPreset}
		{onSaveCamera}
		{onRestoreCamera}
		{onSetClearColor}
		{onColorModeChange}
		{onActivateWardrobeMenuCandidate}
	/>
{:else if rendererPaneTab === "expressions"}
	<RendererExpressionsPane
		rendererId={renderer.id}
		rendererPid={renderer.pid}
		{busy}
		expressionPresets={runtimeStatus?.expression_presets ?? []}
		{expressionOverrides}
		bind:expressionFilter
		{onClearExpressionOverrides}
		{onSetExpressionOverride}
	/>
{:else if rendererPaneTab === "diagnostics"}
	<RendererDiagnosticsPane {renderer} {runtimeStatus} {busy} {onSetDynamicsEnabled} />
{/if}
