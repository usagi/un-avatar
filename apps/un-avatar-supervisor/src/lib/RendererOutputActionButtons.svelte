<script lang="ts">
  import { _ } from "svelte-i18n";
  import type { RendererOutputData, RendererOutputStatus } from "./rendererControlTypes";
  import type { RendererPaneActions } from "./rendererPaneActions";

  export let renderer: Pick<RendererOutputData, "spout_enabled">;
  export let runtimeStatus: RendererOutputStatus | null;
  export let disabled = false;
  export let canMatchSpoutToWindow = false;
  export let onSetSpoutOutput: RendererPaneActions["onSetSpoutOutput"];
  export let onMatchSpoutToWindow: RendererPaneActions["onMatchSpoutToWindow"];
</script>

<div class="runtime-button-row">
  <label class="toggle-field renderer-output-toggle">
    <input
      type="checkbox"
      checked={runtimeStatus?.connected
        ? runtimeStatus.spout_enabled
        : renderer.spout_enabled}
      disabled={disabled ||
        (runtimeStatus?.connected === true && !runtimeStatus.spout_available)}
      onchange={(event) =>
        onSetSpoutOutput(
          (event.currentTarget as HTMLInputElement).checked,
          null,
        )}
    />
    <span>{$_("renderers.controls.spout")}</span>
  </label>
  <button
    {disabled}
    onclick={() => onSetSpoutOutput(true, { width: 1280, height: 720 }, "720p")}
    >720p</button
  >
  <button
    {disabled}
    onclick={() => onSetSpoutOutput(true, { width: 1920, height: 1080 }, "1080p")}
    >1080p</button
  >
  <button
    disabled={disabled || !canMatchSpoutToWindow}
    onclick={onMatchSpoutToWindow}>{$_("renderers.controls.match_window")}</button
  >
</div>
