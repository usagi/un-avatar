<script lang="ts">
	import { _ } from "svelte-i18n";
	import { Camera, DatabaseZap, ExternalLink, Monitor, Play, RefreshCw } from "lucide-svelte";
	import type { ProfilePendingRestart } from "./profileStageTypes";
	import type { RendererRef } from "./rendererTypes";

	export let settingId: string;
	export let liveRenderer: RendererRef | null;
	export let pendingRestart: ProfilePendingRestart | null;
	export let busy = false;
	export let onRestartPending: () => void | Promise<void>;
	export let onViewRenderer: (rendererId: number) => void;
	export let onActivateRenderer: (rendererId: number) => void | Promise<void>;
	export let onCaptureRendererScreenshot: (rendererId: number) => void | Promise<void>;
	export let onLaunchProfile: (settingId: string) => void | Promise<void>;
	export let onPrewarmSceneCache: (settingId: string) => void | Promise<void>;
</script>

<div class="profile-stage-actions">
	<div class:profile-pending-action-slot={true} class:profile-pending-action-slot-visible={!!pendingRestart} aria-live="polite">
		{#if pendingRestart}
			<span class="profile-pending-action">
				<span>{$_("profiles.live.apply_requires_restart_prefix")}</span>
				<button
					type="button"
					disabled={busy}
					title={$_("profiles.live.pending_field", {
						values: { field: pendingRestart.fieldLabel },
					})}
					onclick={() => onRestartPending()}><RefreshCw size={14} />{$_("renderers.toolbar.restart")}</button
				>
				<span>{$_("profiles.live.apply_requires_restart_suffix")}</span>
			</span>
		{/if}
	</div>
	<button type="button" disabled={busy} onclick={() => onPrewarmSceneCache(settingId)}
		><DatabaseZap size={14} />{$_("profiles.actions.warm_cache")}</button
	>
	{#if liveRenderer}
		<button type="button" onclick={() => onViewRenderer(liveRenderer.id)}
			><Monitor size={14} />{$_("profiles.live.view_renderer")}</button
		>
		<button type="button" disabled={!liveRenderer.pid} onclick={() => onActivateRenderer(liveRenderer.id)}
			><ExternalLink size={14} />{$_("renderers.toolbar.activate")}</button
		>
		<button type="button" disabled={busy || !liveRenderer.pid} onclick={() => onCaptureRendererScreenshot(liveRenderer.id)}
			><Camera size={14} />{$_("renderers.toolbar.screenshot")}</button
		>
	{:else}
		<button type="button" disabled={busy} onclick={() => onLaunchProfile(settingId)}
			><Play size={14} />{$_("profiles.actions.quick_run")}</button
		>
	{/if}
</div>
