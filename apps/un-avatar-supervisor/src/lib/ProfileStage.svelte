<script lang="ts">
	import { _ } from "svelte-i18n";
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

	$: summaryItems = profileStageSummaryItems(setting, $_);
</script>

<section class="profile-stage" aria-label={$_("profiles.selected_summary")}>
	<ProfileStageIdentity {iconUrl} {liveRendererCount} />
	<div class="profile-stage-main">
		<div class="stage-title-row profile-stage-title-row">
			<ProfileStageTitle {setting} />
			<ProfileStageActions
				settingId={setting.id}
				sceneCacheFingerprint={setting.scene_cache_fingerprint}
				sceneCachePrewarmedFingerprint={setting.scene_cache_prewarmed_fingerprint}
				sceneCachePrewarmedAt={setting.scene_cache_prewarmed_at}
				{liveRenderer}
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
		</div>
		<ProfileSummaryGrid items={summaryItems} {activeSection} {onScrollSection} />
	</div>
</section>
