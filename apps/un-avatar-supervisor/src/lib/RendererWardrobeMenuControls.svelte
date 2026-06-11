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
  $: candidates = (runtimeStatus?.menu_wardrobe_candidates ?? []).slice(0, candidateLimit);

  function candidateLabel(candidate: RendererRuntimeMenuWardrobeCandidateStatus): string {
    if (candidate.menu_path?.length) return candidate.menu_path.join(" / ");
    return candidate.menu_label || candidate.wardrobe_set_id;
  }

  function activate(candidate: RendererRuntimeMenuWardrobeCandidateStatus): void {
    void onActivateWardrobeMenuCandidate(renderer.id, candidate.menu_path ?? [], candidate.wardrobe_set_id);
  }
</script>

{#if candidates.length}
  <section class="renderer-control-card renderer-control-wardrobe-menu">
    <div class="renderer-control-card-heading">
      <h3>{$_("renderers.controls.wardrobe_menu")}</h3>
      <span>{$_("renderers.controls.wardrobe_menu_count", { values: { count: candidates.length } })}</span>
    </div>
    <div class="runtime-button-row wardrobe-menu-grid">
      {#each candidates as candidate}
        <button
          type="button"
          class:active={runtimeStatus?.active_wardrobe_set === candidate.wardrobe_set_id}
          disabled={busy || !rendererRunning}
          title={candidate.action_id}
          onclick={() => activate(candidate)}
        >
          <Shirt size={14} />
          <span>{candidateLabel(candidate)}</span>
        </button>
      {/each}
    </div>
  </section>
{/if}
