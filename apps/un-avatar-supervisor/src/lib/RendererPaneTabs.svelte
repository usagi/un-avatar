<script lang="ts">
	import { _ } from "svelte-i18n";
	import type { RendererPaneTab } from "./rendererTypes";

	export let rendererPaneTab: RendererPaneTab;
	export let animatorItemCount = 0;
	export let onSelectTab: (tab: RendererPaneTab) => void;
</script>

<div class="renderer-pane-tabs" role="tablist">
	<button
		type="button"
		role="tab"
		aria-selected={rendererPaneTab === "overview"}
		class:active={rendererPaneTab === "overview"}
		onclick={() => onSelectTab("overview")}
		><strong>{$_("renderers.details.overview")}</strong><small>{$_("renderers.details.overview_subtitle")}</small></button
	>
	<button
		type="button"
		role="tab"
		aria-selected={rendererPaneTab === "controls"}
		class:active={rendererPaneTab === "controls"}
		onclick={() => onSelectTab("controls")}
		><strong>{$_("renderers.details.controls")}</strong><small>{$_("renderers.details.controls_subtitle")}</small></button
	>
	<button
		type="button"
		role="tab"
		aria-selected={rendererPaneTab === "animator"}
		class:active={rendererPaneTab === "animator"}
		disabled={animatorItemCount === 0}
		onclick={() => onSelectTab("animator")}
		title={animatorItemCount > 0 ? $_("renderers.details.animator_tip_available") : $_("renderers.details.animator_tip_unavailable")}
		><strong>{$_("renderers.details.animator")}</strong><small>{$_("renderers.details.animator_subtitle")}</small
		>{#if animatorItemCount > 0}<span class="tab-count">{animatorItemCount}</span>{/if}</button
	>
	<button
		type="button"
		role="tab"
		aria-selected={rendererPaneTab === "diagnostics"}
		class:active={rendererPaneTab === "diagnostics"}
		onclick={() => onSelectTab("diagnostics")}
		><strong>{$_("renderers.details.diagnostics")}</strong><small>{$_("renderers.details.diagnostics_subtitle")}</small></button
	>
</div>
