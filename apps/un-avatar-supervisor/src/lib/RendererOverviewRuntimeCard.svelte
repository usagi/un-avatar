<script lang="ts">
	import { _ } from "svelte-i18n";
	import { runtimeConnectionLabel, startupProgressPercent, startupStatusLabel } from "./runtimeLabels";
	import type { RendererOverviewRuntimeStatus } from "./rendererTypes";

	export let runtimeStatus: RendererOverviewRuntimeStatus | null;

	$: startupLabel = startupStatusLabel(runtimeStatus);
</script>

<section class="renderer-info-card">
	<div class="renderer-info-card-heading">
		<h3>{$_("renderers.details.runtime_card")}</h3>
		<span
			>{runtimeConnectionLabel(runtimeStatus, {
				pending: $_("renderers.summary.pending"),
				connected: $_("renderers.summary.connected"),
			})}</span
		>
	</div>
	{#if startupLabel}
		<div class="renderer-card-note">
			<span>{startupLabel}</span>
			<div
				class="startup-progress details-startup-progress"
				class:indeterminate={!runtimeStatus?.startup_progress || runtimeStatus.startup_progress[1] <= 0}
			>
				<span style={`width: ${startupProgressPercent(runtimeStatus).toFixed(1)}%`}></span>
			</div>
		</div>
	{/if}
	<dl class="renderer-card-kv">
		<dt>{$_("renderers.details.controls")}</dt>
		<dd>{runtimeStatus?.control_capabilities?.length ?? 0}</dd>
		{#if runtimeStatus?.note}
			<dt>{$_("renderers.details.diag_runtime_note")}</dt>
			<dd>{runtimeStatus.note}</dd>
		{/if}
	</dl>
</section>
