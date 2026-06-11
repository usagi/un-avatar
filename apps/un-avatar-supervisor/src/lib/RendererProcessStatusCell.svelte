<script lang="ts">
	import { _ } from "svelte-i18n";
	import type { RendererTableView } from "./rendererTypes";
	import { rendererHealthKind, rendererHealthLabel, type RuntimeTableStatusData } from "./runtimeLabels";
	import { rendererStateClass } from "./runtimeState";

	export let renderer: RendererTableView;
	export let status: RuntimeTableStatusData | null;
	export let onOpenRendererLog: (rendererId: number) => void;

	$: health = rendererHealthKind(renderer, status);
</script>

<td class="process-cell-status">
	<button
		type="button"
		class="process-status-log-button"
		title={$_("renderers.process.open_log_hint")}
		onclick={(event) => {
			event.stopPropagation();
			onOpenRendererLog(renderer.id);
		}}
	>
		<span class={rendererStateClass(renderer.state)}>{renderer.state}</span>
	</button>
	<span class="process-health-line">
		<span class={`process-health-dot health-${health}`}></span>
		<small
			>{renderer.pid ? `PID ${renderer.pid}` : `exit ${renderer.exit_code ?? "--"}`}
			· {rendererHealthLabel(renderer, status, {
				pending: $_("renderers.summary.pending"),
				connected: $_("renderers.summary.connected"),
			})}</small
		>
	</span>
</td>
