<script lang="ts">
	import { _ } from "svelte-i18n";
	import { Shirt } from "lucide-svelte";
	import type { RendererControlsData, RendererControlsStatus } from "./rendererControlTypes";
	import type { RendererPaneActions } from "./rendererPaneActions";
	import type { RendererRuntimeMenuWardrobeCandidateStatus } from "./rendererTypes";

	export let renderer: RendererControlsData;
	export let runtimeStatus: RendererControlsStatus | null;
	export let busy = false;
	export let onActivateWardrobeMenuCandidate: RendererPaneActions["onActivateWardrobeMenuCandidate"];

	const candidateLimit = 8;

	$: rendererRunning = renderer.pid != null;
	$: allCandidates = runtimeStatus?.menu_wardrobe_candidates ?? [];
	$: candidates = allCandidates.slice(0, candidateLimit);
	$: hiddenCandidateCount = Math.max(0, allCandidates.length - candidates.length);

	function candidateLabel(candidate: RendererRuntimeMenuWardrobeCandidateStatus): string {
		if (candidate.menu_path?.length) return candidate.menu_path.join(" / ");
		return candidate.menu_label || candidate.wardrobe_set_id;
	}

	function candidateTitle(candidate: RendererRuntimeMenuWardrobeCandidateStatus): string {
		if (!candidate.menu_path_truncated) return candidate.action_id;
		return $_("renderers.controls.wardrobe_menu_path_truncated_title", {
			values: { action: candidate.action_id },
		});
	}

	function activate(candidate: RendererRuntimeMenuWardrobeCandidateStatus): void {
		void onActivateWardrobeMenuCandidate(renderer.id, candidate.action_id, candidate.wardrobe_set_id);
	}
</script>

{#if candidates.length}
	<section class="renderer-control-card renderer-control-wardrobe-menu">
		<div class="renderer-control-card-heading">
			<h3>{$_("renderers.controls.wardrobe_menu")}</h3>
			<span>
				{#if hiddenCandidateCount}
					{$_("renderers.controls.wardrobe_menu_count_limited", {
						values: {
							count: candidates.length,
							total: allCandidates.length,
							hidden: hiddenCandidateCount,
						},
					})}
				{:else}
					{$_("renderers.controls.wardrobe_menu_count", { values: { count: candidates.length } })}
				{/if}
			</span>
		</div>
		<div class="runtime-button-row wardrobe-menu-grid">
			{#each candidates as candidate}
				<button
					type="button"
					class:active={runtimeStatus?.active_wardrobe_set === candidate.wardrobe_set_id}
					disabled={busy || !rendererRunning}
					title={candidateTitle(candidate)}
					onclick={() => activate(candidate)}
				>
					<Shirt size={14} />
					<span>{candidateLabel(candidate)}</span>
				</button>
			{/each}
		</div>
	</section>
{/if}
