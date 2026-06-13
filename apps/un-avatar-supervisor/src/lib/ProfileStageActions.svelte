<script lang="ts">
	import { _ } from "svelte-i18n";
	import { Camera, DatabaseZap, ExternalLink, Monitor, Pin, Play, RefreshCw } from "lucide-svelte";
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
	export let onCreateDesktopShortcut: (settingId: string) => void | Promise<void>;
	export let onCreateTaskbarLauncher: (settingId: string) => void | Promise<void>;
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
	<div class="profile-stage-action-group" aria-label={$_("profiles.action_groups.prepare")}>
		<button
			type="button"
			disabled={busy}
			data-hint={liveRenderer ? $_("profiles.actions.warm_cache_live_hint") : $_("profiles.actions.warm_cache_hint")}
			title={liveRenderer ? $_("profiles.actions.warm_cache_live_hint") : $_("profiles.actions.warm_cache_hint")}
			onclick={() => onPrewarmSceneCache(settingId)}><DatabaseZap size={14} /><span>{$_("profiles.actions.warm_cache")}</span></button
		>
		<button
			type="button"
			disabled={busy}
			data-hint={$_("profiles.actions.desktop_shortcut_hint")}
			title={$_("profiles.actions.desktop_shortcut_hint")}
			onclick={() => onCreateDesktopShortcut(settingId)}><Monitor size={14} /><span>{$_("profiles.actions.desktop_shortcut")}</span></button
		>
		<button
			type="button"
			disabled={busy}
			data-hint={$_("profiles.actions.taskbar_launcher_hint")}
			title={$_("profiles.actions.taskbar_launcher_hint")}
			onclick={() => onCreateTaskbarLauncher(settingId)}><Pin size={14} /><span>{$_("profiles.actions.taskbar_launcher")}</span></button
		>
	</div>
	{#if liveRenderer}
		<div class="profile-stage-action-group" aria-label={$_("profiles.action_groups.live_renderer")}>
			<button type="button" onclick={() => onViewRenderer(liveRenderer.id)}
				><Monitor size={14} /><span>{$_("profiles.live.view_renderer")}</span></button
			>
			<button type="button" disabled={!liveRenderer.pid} onclick={() => onActivateRenderer(liveRenderer.id)}
				><ExternalLink size={14} /><span>{$_("renderers.toolbar.activate")}</span></button
			>
			<button type="button" disabled={busy || !liveRenderer.pid} onclick={() => onCaptureRendererScreenshot(liveRenderer.id)}
				><Camera size={14} /><span>{$_("renderers.toolbar.screenshot")}</span></button
			>
		</div>
	{:else}
		<div class="profile-stage-action-group" aria-label={$_("profiles.action_groups.launch")}>
			<button
				type="button"
				disabled={busy}
				data-hint={$_("profiles.actions.quick_run_hint")}
				title={$_("profiles.actions.quick_run_hint")}
				onclick={() => onLaunchProfile(settingId)}><Play size={14} /><span>{$_("profiles.actions.quick_run")}</span></button
			>
		</div>
	{/if}
</div>
