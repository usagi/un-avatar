<script lang="ts">
	import { _ } from "svelte-i18n";
	import RendererProcessRow from "./RendererProcessRow.svelte";
	import type { RuntimeTableStatusData } from "./runtimeLabels";
	import type { RendererTableView } from "./rendererTypes";

	export let renderers: RendererTableView[];
	export let selectedRendererId: number | null;
	export let showStoppedRenderers = false;
	export let emptySummary: string;
	export let statusForRenderer: (rendererId: number) => RuntimeTableStatusData | null;
	export let iconSrcForManifest: (manifestPath: string | null) => string;
	export let onSelectRenderer: (rendererId: number) => void;
	export let onOpenRendererLog: (rendererId: number) => void;
</script>

<section class="panel table-panel" aria-label="Renderer processes">
	<table class="process-table renderer-process-table">
		<colgroup>
			<col class="process-col-state" />
			<col class="process-col-profile" />
			<col class="process-col-perf" />
			<col class="process-col-io" />
		</colgroup>
		<thead>
			<tr>
				<th>{$_("renderers.columns.state")}</th>
				<th>{$_("renderers.columns.profile")}</th>
				<th>{$_("renderers.columns.performance")}</th>
				<th>{$_("renderers.columns.io")}</th>
			</tr>
		</thead>
		<tbody>
			{#each renderers as renderer}
				<RendererProcessRow
					{renderer}
					{selectedRendererId}
					status={statusForRenderer(renderer.id)}
					iconSrc={iconSrcForManifest(renderer.manifest_path)}
					{onSelectRenderer}
					{onOpenRendererLog}
				/>
			{:else}
				<tr>
					<td colspan="4" class="renderer-table-empty-cell">
						<div class="renderer-table-empty">
							<strong>{showStoppedRenderers ? $_("renderers.empty.none_visible") : $_("renderers.empty.no_running")}</strong>
							<span>{emptySummary}</span>
						</div>
					</td>
				</tr>
			{/each}
		</tbody>
	</table>
</section>
