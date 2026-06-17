<script lang="ts">
	import { _ } from "svelte-i18n";
	import type { LookRecommendation } from "./profileTypes";

	export let busy = false;
	export let onApplyLookRecommendation: (look: LookRecommendation) => void | Promise<void>;
</script>

<div class="recommendation-row" aria-label={$_("profiles.hints.look.recommendations_label")}>
	<span>{$_("profiles.editor.recommendation")}</span>
	<button type="button" disabled={busy} data-hint={$_("profiles.hints.look.natural")} onclick={() => onApplyLookRecommendation("natural")}
		>{$_("profiles.editor.look_preset_natural")}</button
	>
	<button type="button" disabled={busy} data-hint={$_("profiles.hints.look.clear")} onclick={() => onApplyLookRecommendation("clear")}
		>{$_("profiles.editor.look_preset_clear")}</button
	>
	<button type="button" disabled={busy} data-hint={$_("profiles.hints.look.pop")} onclick={() => onApplyLookRecommendation("pop")}
		>{$_("profiles.editor.look_preset_pop")}</button
	>
	<button type="button" disabled={busy} data-hint={$_("profiles.hints.look.soft")} onclick={() => onApplyLookRecommendation("soft")}
		>{$_("profiles.editor.look_preset_soft")}</button
	>
</div>
