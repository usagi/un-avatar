<script lang="ts">
	import { _ } from "svelte-i18n";
	import { localizedRuntimeOutputLabel } from "./profileStageSummary";
	import { runtimeResolution, spoutHealthLabel, type RuntimeSummaryStatusData } from "./runtimeLabels";
	import type { RendererOutputView } from "./rendererTypes";

	export let renderer: RendererOutputView;
	export let runtimeStatus: RuntimeSummaryStatusData | null;
</script>

<div class="runtime-summary-grid" aria-label="Runtime summary">
	<span>
		<small>{$_("renderers.summary.surface")}</small>
		<strong>{runtimeResolution(runtimeStatus)}</strong>
	</span>
	<span>
		<small>{$_("renderers.summary.output")}</small>
		<strong>{localizedRuntimeOutputLabel(renderer, runtimeStatus, $_)}</strong>
	</span>
	<span>
		<small>{$_("renderers.summary.spout")}</small>
		<strong>{spoutHealthLabel(runtimeStatus, $_("renderers.summary.pending"))}</strong>
	</span>
</div>
