<script lang="ts">
	import { _ } from "svelte-i18n";
	import LogsRendererCards from "./LogsRendererCards.svelte";
	import LogsToolbar from "./LogsToolbar.svelte";
	import LogsUnifiedStream from "./LogsUnifiedStream.svelte";
	import type { RendererLogData, RendererLogFilter } from "./rendererLogs";

	export let renderers: RendererLogData[];
	export let rendererLogsLayout: "per-renderer" | "unified";
	export let logsRendererFilter: RendererLogFilter;
	export let logsTextFilter: string;
	export let logsAutoscroll: boolean;
	export let rendererLogsCopyFlash = false;
	export let rendererLogsExpanded: Record<number, boolean>;
	export let onCopyAllRendererLogs: () => void;
	export let onSaveAllRendererLogs: () => void;
	export let onRevealSupervisorLogsDir: () => void;
	export let onCopyRendererLog: (renderer: RendererLogData) => void;
	export let onToggleRendererLogExpanded: (renderer: RendererLogData) => void;
	export let onLogsViewRef: (element: HTMLElement | null) => void;
</script>

<section class="view logs-view">
	<h2>{$_("logs.title")}</h2>
	<LogsToolbar
		{renderers}
		bind:rendererLogsLayout
		bind:logsRendererFilter
		bind:logsTextFilter
		bind:logsAutoscroll
		{rendererLogsCopyFlash}
		{onCopyAllRendererLogs}
		{onSaveAllRendererLogs}
		{onRevealSupervisorLogsDir}
	/>
	<div class="panel logs-panel">
		{#if renderers.length === 0}
			<p class="logs-empty">{$_("logs.renderer.empty")}</p>
		{:else if rendererLogsLayout === "unified"}
			<LogsUnifiedStream {renderers} {logsTextFilter} {logsRendererFilter} {onLogsViewRef} />
		{:else}
			<LogsRendererCards {renderers} {logsTextFilter} {rendererLogsExpanded} {onCopyRendererLog} {onToggleRendererLogExpanded} />
		{/if}
	</div>
</section>
