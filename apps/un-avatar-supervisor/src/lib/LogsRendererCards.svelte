<script lang="ts">
	import LogsRendererCard from "./LogsRendererCard.svelte";
	import type { RendererLogData } from "./rendererLogs";

	export let renderers: RendererLogData[];
	export let logsTextFilter: string;
	export let rendererLogsExpanded: Record<number, boolean>;
	export let onCopyRendererLog: (renderer: RendererLogData) => void;
	export let onToggleRendererLogExpanded: (renderer: RendererLogData) => void;
</script>

<div class="logs-cards">
	{#each renderers as renderer (renderer.id)}
		<LogsRendererCard {renderer} {logsTextFilter} {rendererLogsExpanded} {onCopyRendererLog} {onToggleRendererLogExpanded} />
	{/each}
</div>
