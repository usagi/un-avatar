<script lang="ts">
	import { _ } from "svelte-i18n";
	import { localizedRuntimeOutputLabel } from "./profileStageSummary";
	import { localizedSpoutHealthLabel, runtimeResolution, type RuntimeSummaryStatusData } from "./runtimeLabels";
	import type { RendererOutputView } from "./rendererTypes";

	export let renderer: RendererOutputView;
	export let runtimeStatus: RuntimeSummaryStatusData | null;
</script>

<div class="runtime-summary-grid" aria-label={$_("renderers.summary.aria")}>
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
		<strong
			>{localizedSpoutHealthLabel(runtimeStatus, {
				pending: $_("renderers.summary.pending"),
				connected: $_("renderers.summary.connected"),
				spoutDisabled: $_("renderers.summary.spout_disabled"),
				spoutBackendUnavailable: $_("renderers.summary.spout_backend_unavailable"),
				spoutWaitingFirstFrame: $_("renderers.summary.spout_waiting_first_frame"),
				spoutSending: $_("renderers.summary.spout_sending"),
				spoutFailingState: $_("renderers.summary.spout_failing_state"),
			})}</strong
		>
	</span>
</div>
