<script lang="ts">
	import { _ } from "svelte-i18n";
	import type { RendererWindowStatus } from "./rendererControlTypes";

	export let runtimeStatus: RendererWindowStatus | null;
</script>

<table class="camera-state-table">
	<tbody>
		<tr>
			<th>{$_("renderers.controls.window_position")}</th>
			<td
				>{#if runtimeStatus?.window_position}{runtimeStatus.window_position[0]},
					{runtimeStatus.window_position[1]} (px outer){:else}<span class="muted-small"
						>{$_("renderers.controls.unreported")}</span
					>{/if}</td
			>
		</tr>
		<tr>
			<th>{$_("renderers.controls.window_size")}</th>
			<td
				>{#if runtimeStatus?.window_inner_size}{runtimeStatus.window_inner_size[0]}
					×
					{runtimeStatus.window_inner_size[1]} (px inner){:else}<span class="muted-small"
						>{$_("renderers.controls.unreported")}</span
					>{/if}</td
			>
		</tr>
	</tbody>
</table>
