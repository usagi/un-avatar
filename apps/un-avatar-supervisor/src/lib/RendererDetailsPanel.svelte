<script lang="ts">
	import { _ } from "svelte-i18n";
	import RendererPaneContent from "./RendererPaneContent.svelte";
	import RendererPaneTabs from "./RendererPaneTabs.svelte";
	import RendererReadyPanel from "./RendererReadyPanel.svelte";
	import RendererRuntimeSummaryGrid from "./RendererRuntimeSummaryGrid.svelte";
	import type { ProfileLaunchSetting } from "./profileTypes";
	import type { ExpressionOverrides } from "./rendererExpressions";
	import type { RendererPaneActions } from "./rendererPaneActions";
	import type { RendererInstance, RendererPaneTab, RendererRuntimeStatus } from "./rendererTypes";
	import type { ColorDisplayMode } from "./storageState";

	export let renderer: RendererInstance | null;
	export let runtimeStatus: RendererRuntimeStatus | null;
	export let rendererPaneTab: RendererPaneTab;
	export let launchGroupName: string | null;
	export let launchTargetSetting: ProfileLaunchSetting | null;
	export let launchGroupCount = 0;
	export let runningCount = 0;
	export let issueCount = 0;
	export let profileCount = 0;
	export let profileGroupCount = 0;
	export let busy = false;
	export let colorDisplayMode: ColorDisplayMode;
	export let expressionOverrides: ExpressionOverrides = {};
	export let expressionFilter = "";
	export let onSetSpoutOutput: RendererPaneActions["onSetSpoutOutput"];
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
	export let onSetAllDynamicsEnabled: RendererPaneActions["onSetAllDynamicsEnabled"];
	export let onOpenProfile: () => void;
	export let onRevealProfilesDir: () => void | Promise<void>;
	export let onSelectRendererPaneTab: (tab: RendererPaneTab) => void;
</script>

<aside class="panel details-panel">
	<h2>{renderer ? $_("renderers.details.title") : $_("renderers.ready.title")}</h2>
	{#if renderer}
		<RendererRuntimeSummaryGrid {renderer} {runtimeStatus} />
		<RendererPaneTabs {rendererPaneTab} expressionPresetCount={runtimeStatus?.expression_presets?.length ?? 0} onSelectTab={onSelectRendererPaneTab} />
		<RendererPaneContent
			{renderer}
			{runtimeStatus}
			{rendererPaneTab}
			{busy}
			{colorDisplayMode}
			{expressionOverrides}
			bind:expressionFilter
			{onSetSpoutOutput}
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
			{onClearExpressionOverrides}
			{onSetExpressionOverride}
			{onActivateWardrobeMenuCandidate}
			{onSetDynamicsEnabled}
			{onSetAllDynamicsEnabled}
		/>
	{:else}
		<RendererReadyPanel
			{launchGroupName}
			{launchTargetSetting}
			{launchGroupCount}
			{runningCount}
			{issueCount}
			{profileCount}
			{profileGroupCount}
			{busy}
			{onOpenProfile}
			{onRevealProfilesDir}
		/>
	{/if}
</aside>
