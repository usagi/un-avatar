<script lang="ts">
	import { _ } from "svelte-i18n";
	import type { RendererPaneTab } from "./rendererTypes";

	export let rendererPaneTab: RendererPaneTab;
	export let expressionPresetCount = 0;
	export let onSelectTab: (tab: RendererPaneTab) => void;
</script>

<div class="renderer-pane-tabs" role="tablist">
	<button
		role="tab"
		aria-selected={rendererPaneTab === "overview"}
		class:active={rendererPaneTab === "overview"}
		onclick={() => onSelectTab("overview")}
		><strong>{$_("renderers.details.overview")}</strong><small>{$_("renderers.details.overview_subtitle")}</small></button
	>
	<button
		role="tab"
		aria-selected={rendererPaneTab === "controls"}
		class:active={rendererPaneTab === "controls"}
		onclick={() => onSelectTab("controls")}
		><strong>{$_("renderers.details.controls")}</strong><small>{$_("renderers.details.controls_subtitle")}</small></button
	>
	<button
		role="tab"
		aria-selected={rendererPaneTab === "expressions"}
		class:active={rendererPaneTab === "expressions"}
		disabled={expressionPresetCount === 0}
		onclick={() => onSelectTab("expressions")}
		title={expressionPresetCount > 0
			? $_("renderers.details.expressions_tip_available")
			: $_("renderers.details.expressions_tip_unavailable")}
		><strong>{$_("renderers.details.expressions")}</strong><small>{$_("renderers.details.expressions_subtitle")}</small
		>{#if expressionPresetCount > 0}<span class="tab-count">{expressionPresetCount}</span>{/if}</button
	>
	<button
		role="tab"
		aria-selected={rendererPaneTab === "diagnostics"}
		class:active={rendererPaneTab === "diagnostics"}
		onclick={() => onSelectTab("diagnostics")}
		><strong>{$_("renderers.details.diagnostics")}</strong><small>{$_("renderers.details.diagnostics_subtitle")}</small></button
	>
</div>
