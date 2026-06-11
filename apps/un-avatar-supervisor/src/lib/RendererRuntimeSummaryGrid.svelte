<script lang="ts">
	import { _ } from "svelte-i18n";
	import {
		runtimeConnectionLabel,
		runtimeOutputLabel,
		runtimeResolution,
		spoutHealthLabel,
		type RuntimeSummaryStatusData,
	} from "./runtimeLabels";
	import type { RendererOutputView } from "./rendererTypes";

	export let renderer: RendererOutputView;
	export let runtimeStatus: RuntimeSummaryStatusData | null;
</script>

<div class="runtime-summary-grid" aria-label="Runtime summary">
	<span>
		<small>{$_("renderers.summary.status")}</small>
		<strong
			>{runtimeConnectionLabel(runtimeStatus, {
				pending: $_("renderers.summary.pending"),
				connected: $_("renderers.summary.connected"),
			})}</strong
		>
	</span>
	<span>
		<small>{$_("renderers.summary.surface")}</small>
		<strong>{runtimeResolution(runtimeStatus)}</strong>
	</span>
	<span>
		<small>{$_("renderers.summary.output")}</small>
		<strong>{runtimeOutputLabel(renderer, runtimeStatus)}</strong>
	</span>
	<span>
		<small>{$_("renderers.summary.spout")}</small>
		<strong>{spoutHealthLabel(runtimeStatus, $_("renderers.summary.pending"))}</strong>
	</span>
</div>
