<script lang="ts">
	import { _ } from "svelte-i18n";
	import { onMount } from "svelte";
	import { profileStageSummaryItems } from "./profileStageSummary";
	import ProfileStageActions from "./ProfileStageActions.svelte";
	import ProfileStageIdentity from "./ProfileStageIdentity.svelte";
	import ProfileStageTitle from "./ProfileStageTitle.svelte";
	import ProfileSummaryGrid from "./ProfileSummaryGrid.svelte";
	import type { ProfileSectionId } from "./profileTypes";
	import type { ProfilePendingRestart, ProfileStageSetting } from "./profileStageTypes";
	import type { RendererRef } from "./rendererTypes";

	export let setting: ProfileStageSetting;
	export let iconUrl: string;
	export let liveRenderer: RendererRef | null;
	export let liveRendererCount = 0;
	export let targetOverview = false;
	export let reviewRenderers: RendererRef[] = [];
	export let onSelectReviewRenderer: (id: number) => void = () => {};

	export let pendingRestart: ProfilePendingRestart | null;
	export let activeSection: ProfileSectionId;
	export let cacheProgress: { phase: string; current: number; total: number; detail: string; elapsed_secs: number } | null = null;
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
	export let onScrollSection: (section: ProfileSectionId) => void;

	export let compactTools = false;
	let focusedReview = true;
	onMount(() => {
		if (import.meta.env.VITE_UN_DESIGN_PREVIEW !== "1") return;
		// Unspecified isolated views must match the normal application. The review
		// toolbar can still explicitly select the historical expanded overview.
		const update = () => { focusedReview = document.documentElement.dataset.seriesSummary !== "existing"; };
		update();
		window.addEventListener("un-series-summary", update);
		return () => window.removeEventListener("un-series-summary", update);
	});
	$: summaryItems = profileStageSummaryItems(setting, $_);
</script>

<section class="profile-stage" class:profile-stage-focused={focusedReview} aria-label={$_("profiles.selected_summary")}>
	<ProfileStageIdentity {iconUrl} {liveRendererCount} />
	<div class="profile-stage-main">
        {#if targetOverview}
            <div class="profile-review-targets">
                <span>{$_("profiles.review_scope.edit_target", { values: { name: setting.name } })} <small>({setting.id})</small></span>
                {#if reviewRenderers.length}
                    <label>{$_("profiles.review_scope.action_target")}
                        <select value={liveRenderer?.id ?? ""} onchange={(event) => onSelectReviewRenderer(Number(event.currentTarget.value))}>
                            <option value="" disabled>{$_("profiles.review_scope.choose_instance")}</option>
                            {#each reviewRenderers as renderer (renderer.id)}
                                <option value={renderer.id}>Renderer #{renderer.id} · PID {renderer.pid ?? "—"}</option>
                            {/each}
                        </select>
                    </label>
                {:else}<span>{$_("profiles.review_scope.no_instances")}</span>{/if}
            </div>
        {/if}
        {#if targetOverview}
            <details class="profile-review-apply-scope">
                <summary>{$_("profiles.review_scope.title")}</summary>
                <p>{$_("profiles.review_scope.saved_profile")}</p>
                {#if reviewRenderers.length}
                    <p>{$_("profiles.review_scope.live_instances", { values: { ids: reviewRenderers.map(renderer => `#${renderer.id}`).join(", ") } })}</p>
                    <p>{$_("profiles.review_scope.window_instance", { values: { id: reviewRenderers[0].id } })}</p>
                {:else}<p>{$_("profiles.review_scope.next_launch")}</p>{/if}
                <p>{$_("profiles.review_scope.restart_required")}</p>
            </details>
        {/if}
		<div class="stage-title-row profile-stage-title-row">
			<ProfileStageTitle {setting} nameShownInTarget={targetOverview && compactTools} />
            <svelte:element this={compactTools ? "details" : "div"} class="profile-stage-tools" class:profile-stage-tools-disclosure={compactTools}>
                {#if compactTools}<summary>{$_("profiles.tools_disclosure")}{#if pendingRestart} · {$_("profiles.live.restart_pending")}{/if}{#if cacheProgress} · {cacheProgress.phase} {cacheProgress.current}/{cacheProgress.total}{/if}</summary>{/if}
			<ProfileStageActions
				settingId={setting.id}
				sceneCacheFingerprint={setting.scene_cache_fingerprint}
				sceneCachePrewarmedFingerprint={setting.scene_cache_prewarmed_fingerprint}
				sceneCachePrewarmedAt={setting.scene_cache_prewarmed_at}
				{liveRenderer}
                requireReviewTarget={targetOverview && reviewRenderers.length > 0 && !liveRenderer}
				{pendingRestart}
				{cacheProgress}
				{busy}
				{onRestartPending}
				{onViewRenderer}
				{onActivateRenderer}
				{onCaptureRendererScreenshot}
				{onLaunchProfile}
				{onPrewarmSceneCache}
				{onCreateDesktopShortcut}
				{taskbarPinned}
				{onSetTaskbarPinned}
			/>
            </svelte:element>
		</div>
		{#if focusedReview}
			<details class="profile-summary-disclosure">
				<summary>{$_("profiles.selected_summary")}</summary>
				<ProfileSummaryGrid items={summaryItems} {activeSection} {onScrollSection} />
			</details>
		{:else}
			<ProfileSummaryGrid items={summaryItems} {activeSection} {onScrollSection} />
		{/if}
	</div>
</section>
