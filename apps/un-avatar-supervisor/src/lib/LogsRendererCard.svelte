<script lang="ts">
	import { _ } from "svelte-i18n";
	import { Copy } from "lucide-svelte";
	import { defaultRendererLogExpanded, filteredLinesForRendererData, rendererLineSeverity, type RendererLogData } from "./rendererLogs";
	import { rendererStateLabel } from "./runtimeState";

	export let renderer: RendererLogData;
	export let logsTextFilter: string;
	export let rendererLogsExpanded: Record<number, boolean>;
	export let onCopyRendererLog: (renderer: RendererLogData) => void;
	export let onToggleRendererLogExpanded: (renderer: RendererLogData) => void;

	$: rendererLines = filteredLinesForRendererData(renderer, logsTextFilter);
	$: expanded = rendererLogsExpanded[renderer.id] ?? defaultRendererLogExpanded(renderer);
</script>

<article class={`logs-card state-${renderer.state}`}>
	<header class="logs-card-header">
		<button class="logs-card-toggle" aria-expanded={expanded} onclick={() => onToggleRendererLogExpanded(renderer)}>
			{expanded ? "▼" : "▶"}
			<strong>#{renderer.id} {renderer.name}</strong>
			<span class={`state state-${renderer.state}`}>{rendererStateLabel(renderer.state, $_)}</span>
			<span class="logs-card-count">
				{$_("logs.body.card_count", {
					values: {
						shown: rendererLines.length,
						total: renderer.stderr_tail.length,
					},
				})}
			</span>
		</button>
		<div class="logs-card-actions">
			<button class="ghost-button" onclick={() => onCopyRendererLog(renderer)} title={$_("logs.renderer.card_copy_title")}>
				<Copy size={13} />{$_("logs.renderer.card_copy")}
			</button>
		</div>
	</header>
	{#if expanded}
		{#if renderer.stderr_tail.length === 0}
			<p class="logs-empty">
				{$_("logs.renderer.no_stderr_yet")}
			</p>
		{:else if rendererLines.length === 0}
			<p class="logs-empty">{$_("logs.body.no_lines_filter")}</p>
		{:else}
			<div class="logs-stream logs-stream-card">
				{#each rendererLines as line, idx (idx)}
					<div class={`logs-line sev-${rendererLineSeverity(line)}`}>{line}</div>
				{/each}
			</div>
		{/if}
	{/if}
</article>
