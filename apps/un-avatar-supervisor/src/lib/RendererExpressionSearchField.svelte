<script lang="ts">
	import { _ } from "svelte-i18n";

	export let expressionFilter = "";
</script>

<label class="expression-search-field">
	<span>{$_("renderers.details.expression_search")}</span>
	<input bind:value={expressionFilter} placeholder={$_("renderers.details.expression_search_placeholder")} />
</label>
