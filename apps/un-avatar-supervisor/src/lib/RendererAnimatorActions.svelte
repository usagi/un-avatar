<script lang="ts">
	import { _ } from "svelte-i18n";
	import { ListTree } from "lucide-svelte";
	import {
		animatorCandidateVisible,
		animatorFallbackActionVisible,
		animatorNormalizedToggleLabel,
		animatorToggleStateLabel,
	} from "./rendererAnimator";
	import type { RendererRuntimeActionStatus, RendererRuntimeMenuActionCandidateStatus } from "./rendererTypes";

	export let rendererId: number | null = null;
	export let rendererPid: number | null = null;
	export let busy = false;
	export let runtimeActions: RendererRuntimeActionStatus[] = [];
	export let menuActionCandidates: RendererRuntimeMenuActionCandidateStatus[] = [];
	export let runtimeParameterValues: Record<string, number> = {};
	export let onSetRuntimeParameter: (rendererId: number, name: string, value: number, label: string) => void | Promise<void>;
	export let onActivateRuntimeAction: (rendererId: number, actionId: string, label: string) => void | Promise<void>;

	$: hasRunningRenderer = rendererId != null && rendererPid != null;
	$: candidates = menuActionCandidates.filter(animatorCandidateVisible);
	$: fallbackActions = candidates.length ? [] : runtimeActions.filter(animatorFallbackActionVisible);
	$: actionCount = candidates.length || fallbackActions.length;
	$: activeActionIds = new Set(
		runtimeActions.filter((action) => action.current_condition_state === "active").map((action) => action.action_id)
	);

	function candidateActive(candidate: RendererRuntimeMenuActionCandidateStatus): boolean {
		if (candidate.match_kind !== "metadata") return activeActionIds.has(candidate.action_id);
		const current = runtimeParameterValues?.[candidate.parameter_name];
		return typeof current === "number" && Math.abs(current - candidate.parameter_value) <= 0.005;
	}

	function candidateLabel(candidate: RendererRuntimeMenuActionCandidateStatus): string {
		const path = candidate.menu_path?.length ? candidate.menu_path : [candidate.menu_label || candidate.action_label || candidate.action_id];
		const last = path[path.length - 1] ?? candidate.action_id;
		const normalized = animatorNormalizedToggleLabel(last);
		return [...path.slice(0, -1), normalized.label].join(" / ");
	}

	function candidateStateLabel(candidate: RendererRuntimeMenuActionCandidateStatus): string {
		const raw = candidate.menu_path?.at(-1) ?? candidate.menu_label ?? candidate.action_label ?? candidate.action_id;
		const normalized = animatorNormalizedToggleLabel(raw);
		return animatorToggleStateLabel(candidateActive(candidate), normalized.polarity);
	}

	function candidateTitle(candidate: RendererRuntimeMenuActionCandidateStatus): string {
		const dispatch = `${candidate.parameter_name}=${candidate.parameter_value}`;
		const source = candidate.menu_label || candidate.action_label || candidate.action_id;
		const title = `${source} / ${dispatch}`;
		if (!candidate.menu_path_truncated) return title;
		return $_("renderers.animator.path_truncated_title", {
			values: { action: candidate.action_id, dispatch: title },
		});
	}

	function activate(candidate: RendererRuntimeMenuActionCandidateStatus): void {
		if (rendererId == null) return;
		const value = candidateActive(candidate) ? 0 : candidate.parameter_value;
		void onSetRuntimeParameter(rendererId, candidate.parameter_name, value, candidateLabel(candidate));
	}

	function fallbackActionLabel(action: RendererRuntimeActionStatus): string {
		const path = action.expression_menu_path?.trim()
			? action.expression_menu_path.split("/").map((segment) => segment.trim())
			: [action.label || action.action_id];
		const last = path[path.length - 1] ?? action.action_id;
		const normalized = animatorNormalizedToggleLabel(last);
		return [...path.slice(0, -1), normalized.label].join(" / ");
	}

	function fallbackActionStateLabel(action: RendererRuntimeActionStatus): string {
		const raw = action.expression_menu_path?.split("/").at(-1)?.trim() || action.label || action.action_id;
		const normalized = animatorNormalizedToggleLabel(raw);
		return animatorToggleStateLabel(action.current_condition_state === "active", normalized.polarity);
	}

	function fallbackActionTitle(action: RendererRuntimeActionStatus): string {
		if (action.parameter_name && action.parameter_value != null) return `${action.parameter_name}=${action.parameter_value}`;
		return action.action_id;
	}

	function activateFallbackAction(action: RendererRuntimeActionStatus): void {
		if (rendererId == null) return;
		const label = fallbackActionLabel(action);
		if (action.parameter_name && action.parameter_value != null) {
			const value = action.current_condition_state === "active" ? 0 : action.parameter_value;
			void onSetRuntimeParameter(rendererId, action.parameter_name, value, label);
			return;
		}
		void onActivateRuntimeAction(rendererId, action.action_id, label);
	}
</script>

<section class="renderer-control-card renderer-control-animator">
	<div class="renderer-control-card-heading">
		<h3>{$_("renderers.animator.actions")}</h3>
		<span>{$_("renderers.animator.action_count", { values: { count: actionCount } })}</span>
	</div>
	{#if actionCount}
		<div class="runtime-button-row vrc-menu-grid">
			{#if candidates.length}
				{#each candidates as candidate}
					<button
						type="button"
						class:active={candidateActive(candidate)}
						disabled={busy || !hasRunningRenderer}
						title={candidateTitle(candidate)}
						onclick={() => activate(candidate)}
					>
						<ListTree size={14} />
						<span>{candidateLabel(candidate)}</span>
						<small>{candidateStateLabel(candidate)}</small>
					</button>
				{/each}
			{:else}
				{#each fallbackActions as action}
					<button
						type="button"
						class:active={action.current_condition_state === "active"}
						disabled={busy || !hasRunningRenderer}
						title={fallbackActionTitle(action)}
						onclick={() => activateFallbackAction(action)}
					>
						<ListTree size={14} />
						<span>{fallbackActionLabel(action)}</span>
						<small>{fallbackActionStateLabel(action)}</small>
					</button>
				{/each}
			{/if}
		</div>
	{:else}
		<p class="empty">{$_("renderers.animator.actions_empty")}</p>
	{/if}
</section>
