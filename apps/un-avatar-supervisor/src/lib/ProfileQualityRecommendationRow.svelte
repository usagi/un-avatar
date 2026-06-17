<script lang="ts">
	import { _ } from "svelte-i18n";
	import type { RenderQualityRecommendation } from "./profileTypes";

	export let busy = false;
	export let onApplyRenderQualityRecommendation: (recommendation: RenderQualityRecommendation) => void | Promise<void>;
</script>

<div class="recommendation-row" aria-label={$_("profiles.hints.quality.recommendations_label")}>
	<span>{$_("profiles.editor.recommendation")}</span>
	<button
		type="button"
		disabled={busy}
		data-hint={$_("profiles.hints.quality.light")}
		onclick={() => onApplyRenderQualityRecommendation("light")}>{$_("profiles.editor.level_light")}</button
	>
	<button
		type="button"
		disabled={busy}
		data-hint={$_("profiles.hints.quality.balanced")}
		onclick={() => onApplyRenderQualityRecommendation("balanced")}>{$_("profiles.editor.level_medium")}</button
	>
	<button
		type="button"
		disabled={busy}
		data-hint={$_("profiles.hints.quality.high")}
		onclick={() => onApplyRenderQualityRecommendation("quality")}>{$_("profiles.editor.level_high")}</button
	>
</div>
