<script lang="ts">
	import RendererOverviewPane from "./RendererOverviewPane.svelte";
	import type { ExpressionOverrides } from "./rendererExpressions";
	import type { RendererPaneActions } from "./rendererPaneActions";
	import type { RendererInstance, RendererPaneTab, RendererRuntimeStatus } from "./rendererTypes";
	import type { ColorDisplayMode } from "./storageState";

	export let renderer: RendererInstance;
	export let runtimeStatus: RendererRuntimeStatus | null;
	export let rendererPaneTab: RendererPaneTab;
	export let busy = false;
	export let colorDisplayMode: ColorDisplayMode;
	export let expressionOverrides: ExpressionOverrides = {};
	export let expressionFilter = "";
	export let onSetSpoutOutput: RendererPaneActions["onSetSpoutOutput"];
	export let onSaveSpoutProfile: RendererPaneActions["onSaveSpoutProfile"];
	export let onRestoreOutput: RendererPaneActions["onRestoreOutput"];
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
	export let onSetRuntimeParameter: RendererPaneActions["onSetRuntimeParameter"];
	export let onActivateRuntimeAction: RendererPaneActions["onActivateRuntimeAction"];
	export let onActivateWardrobeMenuCandidate: RendererPaneActions["onActivateWardrobeMenuCandidate"];
	export let onSetDynamicsEnabled: RendererPaneActions["onSetDynamicsEnabled"];
</script>

{#if rendererPaneTab === "overview"}
	<RendererOverviewPane {renderer} {runtimeStatus} />
{:else if rendererPaneTab === "controls"}
	{#await import("./RendererControlsPane.svelte") then module}
		{@const RendererControlsPane = module.default}
		<RendererControlsPane
			{renderer}
			{runtimeStatus}
			{busy}
			{colorDisplayMode}
			{onSetSpoutOutput}
			{onSaveSpoutProfile}
			{onRestoreOutput}
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
	{/await}
{:else if rendererPaneTab === "animator"}
	{#await import("./RendererExpressionsPane.svelte") then module}
		{@const RendererExpressionsPane = module.default}
		<RendererExpressionsPane
			rendererId={renderer.id}
			rendererPid={renderer.pid}
			{busy}
			expressionPresets={runtimeStatus?.expression_presets ?? []}
			{expressionOverrides}
			runtimeActions={runtimeStatus?.runtime_actions ?? []}
			menuActionCandidates={runtimeStatus?.menu_action_candidates ?? []}
			runtimeParameterValues={runtimeStatus?.runtime_parameter_values ?? {}}
			bind:expressionFilter
			{onClearExpressionOverrides}
			{onSetExpressionOverride}
			{onSetRuntimeParameter}
			{onActivateRuntimeAction}
		/>
	{/await}
{:else if rendererPaneTab === "diagnostics"}
	{#await import("./RendererDiagnosticsPane.svelte") then module}
		{@const RendererDiagnosticsPane = module.default}
		<RendererDiagnosticsPane {renderer} {runtimeStatus} {busy} {onSetDynamicsEnabled} />
	{/await}
{/if}
