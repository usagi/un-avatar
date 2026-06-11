<script lang="ts">
	import { _ } from "svelte-i18n";
	import type { CameraPresetOption } from "./cameraSectionFields";

	export let title: string;
	export let ariaLabel: string;
	export let className = "";
	export let busy = false;
	export let options: readonly CameraPresetOption[];
	export let onApply: (preset: string) => void | Promise<void>;
</script>

<div class={`subgroup-heading ${className}`}>
	<span>{title}</span>
	<div class="recommendation-row" aria-label={ariaLabel}>
		<span>{$_("profiles.editor.recommendation")}</span>
		{#each options as option}
			<button type="button" disabled={busy} data-hint={option.hint} onclick={() => onApply(option.value)}>{option.label}</button>
		{/each}
	</div>
</div>
