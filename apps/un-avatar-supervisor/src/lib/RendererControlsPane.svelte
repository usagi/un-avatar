<script lang="ts">
	import RendererBackgroundControls from "./RendererBackgroundControls.svelte";
	import RendererCameraControls from "./RendererCameraControls.svelte";
	import RendererDisplayControls from "./RendererDisplayControls.svelte";
	import RendererOutputControls from "./RendererOutputControls.svelte";
	import RendererWardrobeMenuControls from "./RendererWardrobeMenuControls.svelte";
	import RendererWindowControls from "./RendererWindowControls.svelte";
	import type { RendererControlsData, RendererControlsStatus } from "./rendererControlTypes";
	import type { RendererPaneActions } from "./rendererPaneActions";
	import type { ColorDisplayMode } from "./storageState";

	export let renderer: RendererControlsData;
	export let runtimeStatus: RendererControlsStatus | null;
	export let busy = false;
	export let colorDisplayMode: ColorDisplayMode;
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
	export let onActivateWardrobeMenuCandidate: RendererPaneActions["onActivateWardrobeMenuCandidate"];
</script>

<div class="renderer-pane-scroll renderer-control-list">
	<RendererOutputControls {renderer} {runtimeStatus} {busy} {onSetSpoutOutput} {onSetWindow} />
	<RendererWindowControls {renderer} {runtimeStatus} {busy} {onSetWindow} {onSaveWindow} {onRestoreWindow} />
	<RendererDisplayControls {renderer} {runtimeStatus} {busy} {onSetShowAxes} {onSetShowBoneColliders} {onSetCameraLock} />
	<RendererWardrobeMenuControls {renderer} {runtimeStatus} {busy} {onActivateWardrobeMenuCandidate} />
	<RendererCameraControls {renderer} {runtimeStatus} {busy} {onSetCameraOrbitPreset} {onSaveCamera} {onRestoreCamera} />
	<RendererBackgroundControls {renderer} {runtimeStatus} {busy} {colorDisplayMode} {onSetClearColor} {onColorModeChange} />
</div>
