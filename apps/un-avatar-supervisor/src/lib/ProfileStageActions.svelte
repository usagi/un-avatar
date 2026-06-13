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
	{#if liveRenderer}
		<div class="profile-stage-action-buttons" aria-label={$_("profiles.action_groups.live_renderer")}>
			<button type="button" class="profile-stage-primary-action" onclick={() => onViewRenderer(liveRenderer.id)}
				><Monitor size={14} /><span>{$_("profiles.live.view_renderer")}</span></button
			>
			<button
				type="button"
				disabled={busy}
				data-hint={$_("profiles.actions.warm_cache_live_hint")}
				title={$_("profiles.actions.warm_cache_live_hint")}
				aria-label={$_("profiles.actions.warm_cache")}
				onclick={() => onPrewarmSceneCache(settingId)}><DatabaseZap size={14} /></button
			>
			<button
				type="button"
				disabled={busy}
				data-hint={$_("profiles.actions.desktop_shortcut_hint")}
				title={$_("profiles.actions.desktop_shortcut_hint")}
				aria-label={$_("profiles.actions.desktop_shortcut")}
				onclick={() => onCreateDesktopShortcut(settingId)}><Monitor size={14} /></button
			>
			<button
				type="button"
				disabled={busy}
				data-hint={$_("profiles.actions.taskbar_launcher_hint")}
				title={$_("profiles.actions.taskbar_launcher_hint")}
				aria-label={$_("profiles.actions.taskbar_launcher")}
				onclick={() => onCreateTaskbarLauncher(settingId)}><Pin size={14} /></button
			>
			<button
				type="button"
				disabled={!liveRenderer.pid}
				title={$_("renderers.toolbar.activate")}
				aria-label={$_("renderers.toolbar.activate")}
				onclick={() => onActivateRenderer(liveRenderer.id)}><ExternalLink size={14} /></button
			>
			<button
				type="button"
				disabled={busy || !liveRenderer.pid}
				title={$_("renderers.toolbar.screenshot")}
				aria-label={$_("renderers.toolbar.screenshot")}
				onclick={() => onCaptureRendererScreenshot(liveRenderer.id)}><Camera size={14} /></button
			>
		</div>
	{:else}
		<div class="profile-stage-action-buttons" aria-label={$_("profiles.action_groups.launch")}>
			<button
				type="button"
				class="profile-stage-primary-action"
				disabled={busy}
				data-hint={$_("profiles.actions.quick_run_hint")}
				title={$_("profiles.actions.quick_run_hint")}
				onclick={() => onLaunchProfile(settingId)}><Play size={14} /><span>{$_("profiles.actions.quick_run")}</span></button
			>
			<button
				type="button"
				disabled={busy}
				data-hint={$_("profiles.actions.warm_cache_hint")}
				title={$_("profiles.actions.warm_cache_hint")}
				aria-label={$_("profiles.actions.warm_cache")}
				onclick={() => onPrewarmSceneCache(settingId)}><DatabaseZap size={14} /></button
			>
			<button
				type="button"
				disabled={busy}
				data-hint={$_("profiles.actions.desktop_shortcut_hint")}
				title={$_("profiles.actions.desktop_shortcut_hint")}
				aria-label={$_("profiles.actions.desktop_shortcut")}
				onclick={() => onCreateDesktopShortcut(settingId)}><Monitor size={14} /></button
			>
			<button
				type="button"
				disabled={busy}
				data-hint={$_("profiles.actions.taskbar_launcher_hint")}
				title={$_("profiles.actions.taskbar_launcher_hint")}
				aria-label={$_("profiles.actions.taskbar_launcher")}
				onclick={() => onCreateTaskbarLauncher(settingId)}><Pin size={14} /></button
			>
		</div>
	{/if}
</div>
