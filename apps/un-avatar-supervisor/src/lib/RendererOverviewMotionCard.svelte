<script lang="ts">
	import { _ } from "svelte-i18n";
	import { runtimeMetric } from "./formatting";
	import type { MotionLabelData } from "./profileLabels";
	import { localizedMotionLabel } from "./profileStageSummary";
	import type { RendererOverviewMotionStatus } from "./rendererTypes";

	export let renderer: MotionLabelData;
	export let runtimeStatus: RendererOverviewMotionStatus | null;
</script>

<section class="renderer-info-card">
	<div class="renderer-info-card-heading">
		<h3>{$_("renderers.details.motion_card")}</h3>
		<span>{localizedMotionLabel(renderer, $_)}</span>
	</div>
	{#if runtimeStatus?.unmotion_zenoh_enabled}
		<dl class="renderer-card-kv">
			<dt>{$_("renderers.details.motion_unmfz")}</dt>
			<dd>
				{$_("renderers.details.motion_received", {
					values: {
						frames: runtimeStatus.unmotion_zenoh_received_frames,
						fps: runtimeMetric(runtimeStatus.unmotion_zenoh_received_fps),
					},
				})}
				/
				{$_("renderers.details.motion_applied", {
					values: {
						frames: runtimeStatus.motion_applied_frames,
						fps: runtimeMetric(runtimeStatus.motion_applied_fps),
					},
				})}
			</dd>
		</dl>
	{/if}
</section>
