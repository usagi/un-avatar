<script lang="ts">
	import { _ } from "svelte-i18n";

	export let rendererId: number | null = null;
	export let busy = false;
	export let expressionPresetCount = 0;
	export let activeExpressionCount = 0;
	export let hasRunningRenderer = false;
	export let onClearExpressionOverrides: (rendererId: number) => void | Promise<void>;
</script>

<section class="renderer-expression-header">
	<div>
		<h3>{$_("renderers.animator.expressions")}</h3>
		<span
			>{$_("renderers.details.expressions_summary", {
				values: {
					count: expressionPresetCount,
					active: activeExpressionCount,
				},
			})}</span
		>
	</div>
	<button
		disabled={busy || !hasRunningRenderer || activeExpressionCount === 0}
		onclick={() => {
			if (rendererId != null) void onClearExpressionOverrides(rendererId);
		}}
		title={$_("renderers.details.clear_overrides_title")}>{$_("renderers.details.clear_overrides")}</button
	>
</section>
