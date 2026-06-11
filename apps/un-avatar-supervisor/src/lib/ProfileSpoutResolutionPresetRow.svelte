<script lang="ts">
	import { _ } from "svelte-i18n";
	import type { SpoutResolutionPreset } from "./profileTypes";

	export let disabled = false;
	export let onApplySpoutResolutionPreset: (preset: SpoutResolutionPreset) => void | Promise<void>;
</script>

<div class="recommendation-row" aria-label="Spout2 resolution presets">
	<span>{$_("profiles.editor.recommendation")}</span>
	<button
		type="button"
		{disabled}
		data-hint={$_("profiles.hints.output.resolution_720p")}
		onclick={() => onApplySpoutResolutionPreset("720p")}>720p</button
	>
	<button
		type="button"
		{disabled}
		data-hint={$_("profiles.hints.output.resolution_1080p")}
		onclick={() => onApplySpoutResolutionPreset("1080p")}>1080p</button
	>
	<button
		type="button"
		{disabled}
		data-hint={$_("profiles.hints.output.resolution_1440p")}
		onclick={() => onApplySpoutResolutionPreset("1440p")}>1440p</button
	>
	<button
		type="button"
		{disabled}
		data-hint={$_("profiles.hints.output.resolution_4k")}
		onclick={() => onApplySpoutResolutionPreset("4k")}>4K</button
	>
</div>
