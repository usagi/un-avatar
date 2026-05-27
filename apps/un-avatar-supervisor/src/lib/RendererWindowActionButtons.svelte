<script lang="ts">
  import { _ } from "svelte-i18n";
  import type { RendererPaneActions } from "./rendererPaneActions";
  import {
    borderlessWindowPatch,
    clickThroughWindowPatch,
    minimizedWindowPatch,
    topmostWindowPatch,
    transparentWindowPatch,
    type RendererWindowActionData,
    type RendererWindowActionPatch,
    type RendererWindowActionStatus,
  } from "./rendererWindowActions";

  export let renderer: RendererWindowActionData;
  export let runtimeStatus: RendererWindowActionStatus | null;
  export let disabled = false;
  export let onSetWindow: RendererPaneActions["onSetWindow"];

  function applyWindowPatch(patch: RendererWindowActionPatch): void {
    void onSetWindow(patch[0], patch[1]);
  }
</script>

<div class="runtime-button-row">
  <button
    class:active={!renderer.decorations}
    {disabled}
    onclick={() => applyWindowPatch(borderlessWindowPatch(renderer))}
    >{$_("renderers.controls.borderless")}</button
  >
  <button
    class:active={renderer.transparent}
    {disabled}
    onclick={() => applyWindowPatch(transparentWindowPatch(renderer))}
    title={$_("renderers.controls.transparent_title")}
    >{$_("renderers.controls.transparent")}</button
  >
  <button
    class:active={runtimeStatus?.input_passthrough}
    disabled={disabled || !renderer.transparent}
    onclick={() => applyWindowPatch(clickThroughWindowPatch(runtimeStatus))}
    >{$_("renderers.controls.click_through")}</button
  >
  <button
    class:active={renderer.always_on_top}
    {disabled}
    onclick={() => applyWindowPatch(topmostWindowPatch(renderer))}
    >{$_("renderers.controls.topmost")}</button
  >
  <button
    class:active={runtimeStatus?.minimized}
    {disabled}
    onclick={() => applyWindowPatch(minimizedWindowPatch(runtimeStatus))}
    title={$_("renderers.controls.minimized_title")}
    >{$_("renderers.controls.minimized")}</button
  >
</div>
