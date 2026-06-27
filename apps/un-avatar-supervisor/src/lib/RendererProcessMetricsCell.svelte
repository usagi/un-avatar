<script lang="ts">
	import { _ } from "svelte-i18n";
	import { fpsHealthClass, gpuHealthClass, ramHealthClass, runtimeMetric } from "./formatting";
	import { startupProgressPercent, startupStatusLabel, type RuntimeTableStatusData } from "./runtimeLabels";

	export let status: RuntimeTableStatusData | null;

	$: startupLabel = startupStatusLabel(status);
	$: frameDetails = [
		["wall", runtimeMetric(status?.frame_wall_ms, " ms")],
		["wall max", runtimeMetric(status?.frame_wall_max_recent_ms, " ms")],
		["spikes", runtimeMetric(status?.frame_wall_spike_count_recent)],
		[$_("renderers.metrics.frame_total"), runtimeMetric(status?.frame_cpu_total_ms, " ms")],
		[$_("renderers.metrics.frame_wait"), runtimeMetric(status?.frame_surface_acquire_ms, " ms")],
		[$_("renderers.metrics.frame_motion"), runtimeMetric(status?.frame_motion_apply_ms, " ms")],
		[$_("renderers.metrics.frame_dynamics"), runtimeMetric(status?.frame_dynamics_step_ms, " ms")],
		[$_("renderers.metrics.frame_draw"), runtimeMetric(status?.frame_draw_state_refresh_ms, " ms")],
		[$_("renderers.metrics.frame_world"), runtimeMetric(status?.frame_scene_world_ms, " ms")],
		[$_("renderers.metrics.frame_skin"), runtimeMetric(status?.frame_draw_skin_palette_ms, " ms")],
		[$_("renderers.metrics.frame_skin_write"), runtimeMetric(status?.frame_draw_skin_palette_write_ms, " ms")],
		[$_("renderers.metrics.frame_fur_source"), runtimeMetric(status?.frame_draw_fur_source_vertices_ms, " ms")],
		[$_("renderers.metrics.frame_expression"), runtimeMetric(status?.frame_draw_expression_values_ms, " ms")],
		[$_("renderers.metrics.frame_morph"), runtimeMetric(status?.frame_draw_morph_weights_ms, " ms")],
		[$_("renderers.metrics.frame_transform"), runtimeMetric(status?.frame_draw_transform_loop_ms, " ms")],
		[$_("renderers.metrics.frame_encode"), runtimeMetric(status?.frame_command_encode_ms, " ms")],
		[$_("renderers.metrics.frame_submit"), runtimeMetric(status?.frame_submit_present_ms, " ms")],
		[$_("renderers.metrics.frame_spout"), runtimeMetric(status?.frame_spout_cpu_ms, " ms")],
		[$_("renderers.metrics.frame_contact"), runtimeMetric(status?.frame_contact_eval_ms, " ms")],
		[$_("renderers.metrics.frame_action"), runtimeMetric(status?.frame_runtime_action_eval_ms, " ms")],
	];
	$: frameDetailTitle = frameDetails.map(([label, value]) => `${label} ${value}`).join(" / ");
</script>

<td class="process-cell-perf">
	{#if startupLabel}
		<span>{startupLabel}</span>
	{:else}
		<div class="process-metric-row">
			<span class={fpsHealthClass(status?.fps)}
				><strong>{runtimeMetric(status?.fps)}</strong><small>{$_("renderers.metrics.fps")}</small></span
			>
			<span class={gpuHealthClass(status?.gpu_ms)}
				><strong>{runtimeMetric(status?.gpu_ms, " ms")}</strong><small>{$_("renderers.metrics.gpu")}</small></span
			>
			<span class={ramHealthClass(status?.ram_mb)}
				><strong>{runtimeMetric(status?.ram_mb, " MB")}</strong><small>{$_("renderers.metrics.ram")}</small></span
			>
		</div>
	{/if}
	<small>{$_("renderers.metrics.cpu")} {runtimeMetric(status?.cpu_ms, " ms")}</small>
	<small title={frameDetailTitle}>
		wall {runtimeMetric(status?.frame_wall_ms, " ms")} / max {runtimeMetric(status?.frame_wall_max_recent_ms, " ms")} / spikes
		{runtimeMetric(status?.frame_wall_spike_count_recent)} / {$_("renderers.metrics.frame_total")}
		{runtimeMetric(status?.frame_cpu_total_ms, " ms")} / {$_("renderers.metrics.frame_draw")}
		{runtimeMetric(status?.frame_draw_state_refresh_ms, " ms")} / {$_("renderers.metrics.frame_wait")}
		{runtimeMetric(status?.frame_surface_acquire_ms, " ms")}
	</small>
	{#if startupLabel}
		<div class="startup-progress" class:indeterminate={!status?.startup_progress || status.startup_progress[1] <= 0}>
			<span style={`width: ${startupProgressPercent(status).toFixed(1)}%`}></span>
		</div>
	{/if}
</td>
