<script lang="ts">
	import { _ } from "svelte-i18n";
	import { filteredRendererLogLines, rendererLineSeverity, type RendererLogData, type RendererLogFilter } from "./rendererLogs";

	export let renderers: RendererLogData[];
	export let logsTextFilter: string;
	export let logsRendererFilter: RendererLogFilter;
	export let onLogsViewRef: (element: HTMLElement | null) => void;

	let logsViewRef: HTMLElement | null = null;

	$: lines = filteredRendererLogLines(renderers, logsTextFilter, logsRendererFilter);
	$: onLogsViewRef(logsViewRef);
</script>

<div class="logs-stream" bind:this={logsViewRef}>
	{#if lines.length === 0}
		<p class="logs-empty">{$_("logs.body.no_lines_filter")}</p>
	{:else}
		{#each lines as line, idx (idx)}
			<div class={`logs-line sev-${rendererLineSeverity(line)}`}>{line}</div>
		{/each}
	{/if}
</div>
