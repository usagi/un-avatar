<script lang="ts">
	import { _ } from "svelte-i18n";
	import { startupProgressPercent, startupStatusLabel } from "./runtimeLabels";
	import type { RendererOverviewRuntimeStatus } from "./rendererTypes";

	export let runtimeStatus: RendererOverviewRuntimeStatus | null;

	$: startupLabel = startupStatusLabel(runtimeStatus);
</script>

{#if startupLabel || runtimeStatus?.note}
	<section class="renderer-info-card">
		<div class="renderer-info-card-heading">
			<h3>{$_("renderers.details.runtime_card")}</h3>
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
		{#if runtimeStatus?.note}
			<dl class="renderer-card-kv">
				<dt>{$_("renderers.details.diag_runtime_note")}</dt>
				<dd>{runtimeStatus.note}</dd>
			</dl>
		{/if}
	</section>
{/if}
