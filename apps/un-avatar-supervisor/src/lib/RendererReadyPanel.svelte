<script lang="ts">
	import { _ } from "svelte-i18n";
	import { FileCog, FolderOpen } from "lucide-svelte";
	import { RENDERER_READY_STATS, rendererReadySubtitle, rendererReadyTitle } from "./rendererReadyLabels";
	import type { ProfileLaunchSetting } from "./profileTypes";

	export let launchGroupName: string | null;
	export let launchTargetSetting: ProfileLaunchSetting | null;
	export let launchGroupCount = 0;
	export let runningCount = 0;
	export let issueCount = 0;
	export let profileCount = 0;
	export let profileGroupCount = 0;
	export let busy = false;
	export let onOpenProfile: () => void;
	export let onRevealProfilesDir: () => void | Promise<void>;

	$: readyStats = {
		runningCount,
		issueCount,
		profileCount,
		profileGroupCount,
	};

	$: readyTitle = rendererReadyTitle(launchGroupName, launchTargetSetting, $_);
	$: readySubtitle = rendererReadySubtitle(launchGroupName, launchTargetSetting, launchGroupCount, $_);
</script>

<div class="renderer-ready-panel">
	<div class="renderer-ready-heading">
		<small>{$_("renderers.ready.kicker")}</small>
		<strong>{readyTitle}</strong>
		<span>{readySubtitle}</span>
	</div>
	<div class="renderer-ready-actions">
		<button type="button" disabled={!launchTargetSetting} onclick={onOpenProfile}
			><FileCog size={14} />{$_("renderers.ready.open_profile")}</button
		>
		<button type="button" disabled={busy} onclick={() => void onRevealProfilesDir()}
			><FolderOpen size={14} />{$_("profiles.actions.open_folder")}</button
		>
	</div>
	<div class="runtime-summary-grid renderer-ready-stats" aria-label={$_("renderers.ready.stats_aria")}>
		{#each RENDERER_READY_STATS as stat}
			<span>
				<small>{$_(stat.labelKey)}</small>
				<strong>{readyStats[stat.key]}</strong>
			</span>
		{/each}
	</div>
</div>
