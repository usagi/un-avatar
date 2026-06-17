<script lang="ts">
	import { _ } from "svelte-i18n";
	import { ListTree } from "lucide-svelte";
	import {
		animatorCandidateVisible,
		animatorFallbackActionVisible,
		animatorInactiveParameterValue,
		animatorNormalizedToggleLabel,
		animatorToggleStateLabel,
	} from "./rendererAnimator";
	import type { RendererRuntimeActionStatus, RendererRuntimeMenuActionCandidateStatus } from "./rendererTypes";

	const MAX_RENDERED_ANIMATOR_ACTIONS = 96;

	export let rendererId: number | null = null;
	export let rendererPid: number | null = null;
	export let busy = false;
	export let runtimeActions: RendererRuntimeActionStatus[] = [];
	export let menuActionCandidates: RendererRuntimeMenuActionCandidateStatus[] = [];
	export let runtimeParameterValues: Record<string, number> = {};
	export let onSetRuntimeParameter: (rendererId: number, name: string, value: number, label: string) => void | Promise<void>;
	export let onActivateRuntimeAction: (rendererId: number, actionId: string, label: string) => void | Promise<void>;

	$: hasRunningRenderer = rendererId != null && rendererPid != null;
	$: activeActionIds = new Set(
		runtimeActions.filter((action) => action.current_condition_state === "active").map((action) => action.action_id)
	);
	$: candidates = dedupeCandidates(menuActionCandidates.filter(animatorCandidateVisible));
	$: candidateActionIds = new Set(candidates.map((candidate) => candidate.action_id));
	$: fallbackActions = dedupeFallbackActions(
		runtimeActions.filter(animatorFallbackActionVisible).filter((action) => !candidateActionIds.has(action.action_id))
	);
	$: visibleCandidates = candidates.slice(0, MAX_RENDERED_ANIMATOR_ACTIONS);
	$: visibleFallbackActions = fallbackActions.slice(0, Math.max(0, MAX_RENDERED_ANIMATOR_ACTIONS - visibleCandidates.length));
	$: actionCount = candidates.length + fallbackActions.length;
	$: renderedActionCount = visibleCandidates.length + visibleFallbackActions.length;

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

	function candidateGroupKey(candidate: RendererRuntimeMenuActionCandidateStatus): string {
		const raw = candidate.menu_path?.at(-1) ?? candidate.menu_label ?? candidate.action_label ?? candidate.action_id;
		const normalized = animatorNormalizedToggleLabel(raw);
		return `${candidate.parameter_name}:${normalized.label.toLowerCase()}`;
	}

	function candidatePreferred(
		current: RendererRuntimeMenuActionCandidateStatus,
		next: RendererRuntimeMenuActionCandidateStatus
	): RendererRuntimeMenuActionCandidateStatus {
		const currentPolarity = animatorNormalizedToggleLabel(
			current.menu_path?.at(-1) ?? current.menu_label ?? current.action_label ?? current.action_id
		).polarity;
		const nextPolarity = animatorNormalizedToggleLabel(
			next.menu_path?.at(-1) ?? next.menu_label ?? next.action_label ?? next.action_id
		).polarity;
		if (candidateActive(next) && !candidateActive(current)) return next;
		if (nextPolarity === "off" && currentPolarity !== "off") return next;
		return current;
	}

	function dedupeCandidates(
		items: RendererRuntimeMenuActionCandidateStatus[]
	): RendererRuntimeMenuActionCandidateStatus[] {
		const grouped = new Map<string, RendererRuntimeMenuActionCandidateStatus>();
		for (const item of items) {
			const key = candidateGroupKey(item);
			const current = grouped.get(key);
			grouped.set(key, current ? candidatePreferred(current, item) : item);
		}
		return [...grouped.values()];
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
		const raw = candidate.menu_path?.at(-1) ?? candidate.menu_label ?? candidate.action_label ?? candidate.action_id;
		const normalized = animatorNormalizedToggleLabel(raw);
		const value = candidateActive(candidate)
			? animatorInactiveParameterValue(candidate.parameter_value, normalized.polarity)
			: candidate.parameter_value;
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

	function fallbackActionGroupKey(action: RendererRuntimeActionStatus): string {
		const raw = action.expression_menu_path?.split("/").at(-1)?.trim() || action.label || action.action_id;
		const normalized = animatorNormalizedToggleLabel(raw);
		return `${action.parameter_name ?? action.supervisor_command ?? action.action_id}:${normalized.label.toLowerCase()}`;
	}

	function fallbackActionPreferred(current: RendererRuntimeActionStatus, next: RendererRuntimeActionStatus): RendererRuntimeActionStatus {
		const currentRaw = current.expression_menu_path?.split("/").at(-1)?.trim() || current.label || current.action_id;
		const nextRaw = next.expression_menu_path?.split("/").at(-1)?.trim() || next.label || next.action_id;
		const currentPolarity = animatorNormalizedToggleLabel(currentRaw).polarity;
		const nextPolarity = animatorNormalizedToggleLabel(nextRaw).polarity;
		if (next.current_condition_state === "active" && current.current_condition_state !== "active") return next;
		if (nextPolarity === "off" && currentPolarity !== "off") return next;
		return current;
	}

	function dedupeFallbackActions(items: RendererRuntimeActionStatus[]): RendererRuntimeActionStatus[] {
		const grouped = new Map<string, RendererRuntimeActionStatus>();
		for (const item of items) {
			const key = fallbackActionGroupKey(item);
			const current = grouped.get(key);
			grouped.set(key, current ? fallbackActionPreferred(current, item) : item);
		}
		return [...grouped.values()];
	}

	function fallbackActionTitle(action: RendererRuntimeActionStatus): string {
		if (action.parameter_name && action.parameter_value != null) return `${action.parameter_name}=${action.parameter_value}`;
		return action.action_id;
	}

	function activateFallbackAction(action: RendererRuntimeActionStatus): void {
		if (rendererId == null) return;
		const label = fallbackActionLabel(action);
		if (action.parameter_name && action.parameter_value != null) {
			const raw = action.expression_menu_path?.split("/").at(-1)?.trim() || action.label || action.action_id;
			const normalized = animatorNormalizedToggleLabel(raw);
			const value =
				action.current_condition_state === "active"
					? animatorInactiveParameterValue(action.parameter_value, normalized.polarity)
					: action.parameter_value;
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
		<div class="runtime-button-row animator-action-grid">
			{#if candidates.length}
				{#each visibleCandidates as candidate}
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
			{/if}
			{#if fallbackActions.length}
				{#each visibleFallbackActions as action}
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
			{#if renderedActionCount < actionCount}
				<div class="runtime-button-note">
					{$_("renderers.animator.action_count", { values: { count: actionCount - renderedActionCount } })}
				</div>
			{/if}
		</div>
	{:else}
		<p class="empty">{$_("renderers.animator.actions_empty")}</p>
	{/if}
</section>
