<script lang="ts">
	import type { ComponentType } from "svelte";

	export let icon: ComponentType;
	export let label: string;
	export let disabled = false;
	export let danger = false;
	export let onClick: () => void | Promise<void>;
</script>

<button type="button" class:danger {disabled} onclick={() => onClick()} aria-label={label} title={label}>
	<svelte:component this={icon} size={15} />
</button>
