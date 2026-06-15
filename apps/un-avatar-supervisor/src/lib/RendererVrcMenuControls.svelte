<script lang="ts">
	import { _ } from "svelte-i18n";
	import { ListTree } from "lucide-svelte";
	import type { RendererControlsData, RendererControlsStatus } from "./rendererControlTypes";
	import type { RendererPaneActions } from "./rendererPaneActions";
	import type { RendererRuntimeActionStatus, RendererRuntimeMenuActionCandidateStatus } from "./rendererTypes";

	export let renderer: RendererControlsData;
	export let runtimeStatus: RendererControlsStatus | null;
	export let busy = false;
	export let onSetRuntimeParameter: RendererPaneActions["onSetRuntimeParameter"];
	export let onActivateRuntimeAction: RendererPaneActions["onActivateRuntimeAction"];

	$: rendererRunning = renderer.pid != null;
	$: candidates = (runtimeStatus?.menu_action_candidates ?? []).filter(
		(candidate) =>
			candidate.available !== false && !candidate.wardrobe_set_ids?.length && metadataCandidateVisible(candidate)
	);
	$: fallbackActions = candidates.length
		? []
		: (runtimeStatus?.runtime_actions ?? []).filter(
				(action) => !action.wardrobe_set_id && Boolean(action.expression_menu_path?.trim())
			);
	$: actionCount = candidates.length || fallbackActions.length;
	$: activeActionIds = new Set(
		(runtimeStatus?.runtime_actions ?? [])
			.filter((action) => action.current_condition_state === "active")
			.map((action) => action.action_id)
	);

	function metadataCandidateVisible(candidate: RendererRuntimeMenuActionCandidateStatus): boolean {
		if (candidate.match_kind !== "metadata") return true;
		if (candidate.control_type === "Button") return false;
		const path = candidate.menu_path ?? [];
		if (path.length > 2) return false;
		if (path.some((segment) => segment === "Face_Tracking" || segment.includes("VRCFT") || segment.includes("<"))) {
			return false;
		}
		return !candidate.menu_label?.includes("<") && !candidate.menu_label?.includes("VRCFT");
	}

	function candidateActive(candidate: RendererRuntimeMenuActionCandidateStatus): boolean {
		if (candidate.match_kind !== "metadata") return activeActionIds.has(candidate.action_id);
		const current = runtimeStatus?.runtime_parameter_values?.[candidate.parameter_name];
		return typeof current === "number" && Math.abs(current - candidate.parameter_value) <= 0.005;
	}

	function candidateLabel(candidate: RendererRuntimeMenuActionCandidateStatus): string {
		if (candidate.menu_path?.length) return candidate.menu_path.join(" / ");
		return candidate.menu_label || candidate.action_label || candidate.action_id;
	}

	function candidateTitle(candidate: RendererRuntimeMenuActionCandidateStatus): string {
		const dispatch = `${candidate.parameter_name}=${candidate.parameter_value}`;
		if (!candidate.menu_path_truncated) return dispatch;
		return $_("renderers.controls.vrc_menu_path_truncated_title", {
			values: { action: candidate.action_id, dispatch },
		});
	}

	function activate(candidate: RendererRuntimeMenuActionCandidateStatus): void {
		const value = candidateActive(candidate) ? 0 : candidate.parameter_value;
		void onSetRuntimeParameter(renderer.id, candidate.parameter_name, value, candidateLabel(candidate));
	}

	function fallbackActionLabel(action: RendererRuntimeActionStatus): string {
		if (action.expression_menu_path?.trim()) return action.expression_menu_path.replaceAll("/", " / ");
		return action.label || action.action_id;
	}

	function fallbackActionTitle(action: RendererRuntimeActionStatus): string {
		if (action.parameter_name && action.parameter_value != null) return `${action.parameter_name}=${action.parameter_value}`;
		return action.action_id;
	}

	function activateFallbackAction(action: RendererRuntimeActionStatus): void {
		const label = fallbackActionLabel(action);
		if (action.parameter_name && action.parameter_value != null) {
			const value = action.current_condition_state === "active" ? 0 : action.parameter_value;
			void onSetRuntimeParameter(renderer.id, action.parameter_name, value, label);
			return;
		}
		void onActivateRuntimeAction(renderer.id, action.action_id, label);
	}
</script>

{#if actionCount}
	<section class="renderer-control-card renderer-control-vrc-menu">
		<div class="renderer-control-card-heading">
			<h3>{$_("renderers.controls.vrc_menu")}</h3>
			<span>{$_("renderers.controls.vrc_menu_count", { values: { count: actionCount } })}</span>
		</div>
		<div class="runtime-button-row vrc-menu-grid">
			{#if candidates.length}
				{#each candidates as candidate}
					<button
						type="button"
						class:active={candidateActive(candidate)}
						disabled={busy || !rendererRunning}
						title={candidateTitle(candidate)}
						onclick={() => activate(candidate)}
					>
						<ListTree size={14} />
						<span>{candidateLabel(candidate)}</span>
					</button>
				{/each}
			{:else}
				{#each fallbackActions as action}
					<button
						type="button"
						class:active={action.current_condition_state === "active"}
						disabled={busy || !rendererRunning}
						title={fallbackActionTitle(action)}
						onclick={() => activateFallbackAction(action)}
					>
						<ListTree size={14} />
						<span>{fallbackActionLabel(action)}</span>
					</button>
				{/each}
			{/if}
		</div>
	</section>
{/if}
