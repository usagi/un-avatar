<script lang="ts">
	import { _ } from "svelte-i18n";
	import type { CameraLensPreset } from "./profileTypes";

	export let busy = false;
	export let onApplyLensPreset: (focalLengthMm: CameraLensPreset) => void | Promise<void>;
</script>

<div class="recommendation-row" aria-label={$_("profiles.editor.camera_fov_presets")}>
	<span>{$_("profiles.editor.recommendation")}</span>
	<button type="button" disabled={busy} data-hint={$_("profiles.hints.camera.fov_10")} onclick={() => onApplyLensPreset(10)}
		>{$_("profiles.editor.options.fov_ultra_wide_10")}</button
	>
	<button type="button" disabled={busy} data-hint={$_("profiles.hints.camera.fov_16")} onclick={() => onApplyLensPreset(16)}
		>{$_("profiles.editor.options.fov_wide_16")}</button
	>
	<button type="button" disabled={busy} data-hint={$_("profiles.hints.camera.fov_35")} onclick={() => onApplyLensPreset(35)}
		>{$_("profiles.editor.options.fov_standard_35")}</button
	>
	<button type="button" disabled={busy} data-hint={$_("profiles.hints.camera.fov_70")} onclick={() => onApplyLensPreset(70)}
		>{$_("profiles.editor.options.fov_tele_70")}</button
	>
	<button type="button" disabled={busy} data-hint={$_("profiles.hints.camera.fov_200")} onclick={() => onApplyLensPreset(200)}
		>{$_("profiles.editor.options.fov_super_tele_200")}</button
	>
</div>
