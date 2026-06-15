<script lang="ts">
	import { _ } from "svelte-i18n";
	import { Camera, DatabaseZap, ExternalLink, FilePlus2, Monitor, Pin, PinOff, Play, RefreshCw } from "lucide-svelte";
	import type { ProfilePendingRestart } from "./profileStageTypes";
	import type { RendererRef } from "./rendererTypes";

	export let settingId: string;
	export let sceneCacheFingerprint = "";
	export let sceneCachePrewarmedFingerprint: string | null = null;
	export let sceneCachePrewarmedAt: string | null = null;
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
	export let taskbarPinned = false;
	export let onSetTaskbarPinned: (settingId: string, pinned: boolean) => void | Promise<void>;

	$: sceneCacheReady =
		Boolean(sceneCacheFingerprint) &&
		Boolean(sceneCachePrewarmedFingerprint) &&
		sceneCachePrewarmedFingerprint === sceneCacheFingerprint;
	$: sceneCacheNeedsRefresh = Boolean(sceneCachePrewarmedFingerprint) && !sceneCacheReady;
	$: sceneCacheActionLabel = sceneCacheReady
		? $_("profiles.actions.cache_ready")
		: sceneCacheNeedsRefresh
			? $_("profiles.actions.cache_refresh_needed")
			: $_("profiles.actions.cache_not_ready");
	$: sceneCacheButtonLabel = sceneCacheReady
		? $_("profiles.actions.warm_cache_again")
		: sceneCacheNeedsRefresh
			? $_("profiles.actions.warm_cache_again")
			: $_("profiles.actions.warm_cache");
	$: actionGroupTitle = $_("profiles.action_groups.prepare");
	$: sceneCacheActionHint = sceneCacheReady
		? $_("profiles.actions.cache_ready_hint", { values: { at: sceneCachePrewarmedAt ?? "-" } })
		: sceneCacheNeedsRefresh
			? $_("profiles.actions.cache_refresh_needed_hint")
		: liveRenderer
			? $_("profiles.actions.warm_cache_live_hint")
			: $_("profiles.actions.warm_cache_hint");
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
	<div class="profile-stage-action-group">
		<div
			class="profile-cache-callout"
			class:profile-cache-callout-ready={sceneCacheReady}
			class:profile-cache-callout-refresh={sceneCacheNeedsRefresh}
			class:profile-cache-callout-missing={!sceneCacheReady && !sceneCacheNeedsRefresh}
		>
			<div class="profile-stage-action-heading">
				<DatabaseZap size={15} aria-hidden="true" />
				<strong>{actionGroupTitle}</strong>
				<span
					class="profile-cache-state"
					class:profile-cache-state-ready={sceneCacheReady}
					class:profile-cache-state-refresh={sceneCacheNeedsRefresh}
					class:profile-cache-state-missing={!sceneCacheReady && !sceneCacheNeedsRefresh}
					role="status"
					aria-live="polite"
					aria-label={sceneCacheActionHint}>{sceneCacheActionLabel}</span
				>
			</div>
			<p>{sceneCacheActionHint}</p>
		</div>
		<div class="profile-stage-action-buttons" aria-label={$_("profiles.action_groups.prepare")}>
			<button
				type="button"
				class:profile-stage-primary-action={!sceneCacheReady}
				class:profile-cache-cta={!sceneCacheReady}
				disabled={busy}
				data-hint={sceneCacheActionHint}
				title={sceneCacheActionHint}
				aria-label={sceneCacheButtonLabel}
				onclick={() => onPrewarmSceneCache(settingId)}
				><DatabaseZap size={14} /><span>{sceneCacheButtonLabel}</span></button
			>
			{#if liveRenderer}
				<button type="button" class="profile-stage-primary-action" onclick={() => onViewRenderer(liveRenderer.id)}
					><Monitor size={14} /><span>{$_("profiles.live.view_renderer")}</span></button
				>
			{:else}
				<button
					type="button"
					class="profile-stage-primary-action"
					disabled={busy}
					data-hint={$_("profiles.actions.quick_run_hint")}
					title={$_("profiles.actions.quick_run_hint")}
					onclick={() => onLaunchProfile(settingId)}><Play size={14} /><span>{$_("profiles.actions.quick_run")}</span></button
				>
			{/if}
			<button
				type="button"
				disabled={busy}
				data-hint={$_("profiles.actions.desktop_shortcut_hint")}
				title={$_("profiles.actions.desktop_shortcut_hint")}
				aria-label={$_("profiles.actions.desktop_shortcut")}
				onclick={() => onCreateDesktopShortcut(settingId)}><FilePlus2 size={14} /><span>{$_("profiles.actions.desktop_shortcut_short")}</span></button
			>
			<button
				type="button"
				class:profile-stage-pinned-action={taskbarPinned}
				disabled={busy}
				data-hint={$_(taskbarPinned ? "profiles.actions.taskbar_unpin_hint" : "profiles.actions.taskbar_launcher_hint")}
				title={$_(taskbarPinned ? "profiles.actions.taskbar_unpin_hint" : "profiles.actions.taskbar_launcher_hint")}
				aria-label={$_("profiles.actions.taskbar_launcher")}
				onclick={() => onSetTaskbarPinned(settingId, !taskbarPinned)}
				>{#if taskbarPinned}<PinOff size={14} />{:else}<Pin size={14} />{/if}<span>{$_(taskbarPinned ? "profiles.actions.taskbar_unpin_short" : "profiles.actions.taskbar_launcher_short")}</span></button
			>
			{#if liveRenderer}
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
			{/if}
		</div>
	</div>
</div>
