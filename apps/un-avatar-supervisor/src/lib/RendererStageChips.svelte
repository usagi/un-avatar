<script lang="ts">
	import { _ } from "svelte-i18n";
	import { motionLabel } from "./profileLabels";
	import { localizedRuntimeOutputLabel, localizedWindowLabel } from "./profileStageSummary";
	import {
		rendererHealthKind,
		rendererHealthLabel,
		runtimeAaLabel,
		runtimeResolution,
		type RuntimeStageStatusData,
	} from "./runtimeLabels";
	import type { RendererStageView } from "./rendererTypes";

	export let renderer: RendererStageView;
	export let runtimeStatus: RuntimeStageStatusData | null;

	$: health = rendererHealthKind(renderer, runtimeStatus);
</script>

<div class="stage-chip-row">
	<span class={`runtime-chip-health-${health}`}
		>{rendererHealthLabel(renderer, runtimeStatus, {
			pending: $_("renderers.summary.pending"),
			connected: $_("renderers.summary.connected"),
		})}</span
	>
	<span>{motionLabel(renderer)}</span>
	<span>{localizedRuntimeOutputLabel(renderer, runtimeStatus, $_)}</span>
	<span>{localizedWindowLabel(renderer, $_)}</span>
	<span>{runtimeResolution(runtimeStatus)}</span>
	<span>{runtimeAaLabel(runtimeStatus)}</span>
</div>
