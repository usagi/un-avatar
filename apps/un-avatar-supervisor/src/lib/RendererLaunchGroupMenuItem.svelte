<script lang="ts">
	import { _ } from "svelte-i18n";

	export let group: string;
	export let selected = false;
	export let count = 0;
	export let onSelect: (group: string) => void;
</script>

<button class="launch-menu-item-text-only" class:selected onclick={() => onSelect(group)}>
	<span>
		<strong>{$_("renderers.ready.group_title", { values: { group } })}</strong>
		<small>{$_("renderers.ready.group_count", { values: { count } })}</small>
	</span>
</button>
