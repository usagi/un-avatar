<script lang="ts">
  import { _ } from "svelte-i18n";
  import { FileCog } from "lucide-svelte";
  import { RENDERER_STAGE_ACTIONS, type RendererStageActionKey } from "./rendererStageActionOptions";
  import type { RendererStageActionData, RendererStageProfile } from "./rendererTypes";
  import RendererStageActionButton from "./RendererStageActionButton.svelte";

  export let renderer: RendererStageActionData;
  export let profile: RendererStageProfile | null;
  export let busy = false;
  export let onViewProfile: (profileId: string) => void;
  export let onActivateRenderer: (rendererId: number) => void | Promise<void>;
  export let onResetCamera: (rendererId: number) => void | Promise<void>;
  export let onCaptureScreenshot: (rendererId: number) => void | Promise<void>;
  export let onRestartRenderer: (rendererId: number) => void | Promise<void>;
  export let onStopRenderer: (rendererId: number) => void | Promise<void>;

  function stageActionDisabled(key: RendererStageActionKey): boolean {
    if (key === "restart") return busy || !renderer.manifest_path;
    return key === "activate" ? !renderer.pid : busy || !renderer.pid;
  }

  function runStageAction(key: RendererStageActionKey): void {
    if (key === "activate") {
      void onActivateRenderer(renderer.id);
      return;
    }
    if (key === "resetCamera") {
      void onResetCamera(renderer.id);
      return;
    }
    if (key === "screenshot") {
      void onCaptureScreenshot(renderer.id);
      return;
    }
    if (key === "restart") {
      void onRestartRenderer(renderer.id);
      return;
    }
    void onStopRenderer(renderer.id);
  }
</script>

<div class="stage-actions">
  {#if profile}
    <RendererStageActionButton
      icon={FileCog}
      label={$_("profiles.live.view_profile")}
      onClick={() => onViewProfile(profile.id)}
    />
  {/if}
  {#each RENDERER_STAGE_ACTIONS as action}
    <RendererStageActionButton
      icon={action.icon}
      label={$_(action.labelKey)}
      danger={action.danger === true}
      disabled={stageActionDisabled(action.key)}
      onClick={() => runStageAction(action.key)}
    />
  {/each}
</div>
