<script lang="ts">
	import { _ } from "svelte-i18n";
	import { ChevronDown } from "lucide-svelte";
	import RendererLaunchGroupMenuItem from "./RendererLaunchGroupMenuItem.svelte";
	import RendererLaunchProfileMenuItem from "./RendererLaunchProfileMenuItem.svelte";
	import { localizedSettingSummary } from "./profileStageSummary";
	import type { ProfileLaunchSetting } from "./profileTypes";

	export let launchTargetSetting: ProfileLaunchSetting | null;
	export let launchGroupName: string | null;
	export let launchTargetId: string | null;
	export let launchMenuOpen = false;
	export let avatarSettings: ProfileLaunchSetting[];
	export let profileGroups: string[];
	export let iconSrc: (path: string | null) => string;
	export let groupCount: (group: string) => number;
	export let onToggleOpen: () => void;
	export let onSelectGroup: (group: string) => void;
	export let onSelectSetting: (settingId: string) => void;

	$: selectedTargetTitle = launchGroupName
		? $_("renderers.ready.group_title", {
				values: { group: launchGroupName },
		})
	: launchTargetSetting
		? localizedSettingSummary(launchTargetSetting, $_)
		: $_("renderers.toolbar.no_manifest_selected");
</script>

<div class="launch-target">
	<button
		type="button"
		class="launch-select-button"
		class:launch-select-text-only={Boolean(launchGroupName)}
		disabled={avatarSettings.length === 0}
		title={selectedTargetTitle}
		onclick={onToggleOpen}
	>
		{#if launchGroupName}
			<strong>{$_("renderers.ready.group_title", { values: { group: launchGroupName } })}</strong>
		{:else if launchTargetSetting}
			<img src={iconSrc(launchTargetSetting.icon_path)} alt="" />
			<strong>{launchTargetSetting.name}</strong>
		{:else}
			<strong>{$_("renderers.toolbar.no_settings_found")}</strong>
		{/if}
		<ChevronDown size={15} />
	</button>
	{#if launchMenuOpen}
		<div class="launch-menu">
			{#if profileGroups.length > 0}
				{#each profileGroups as group (group)}
					<RendererLaunchGroupMenuItem
						{group}
						selected={launchTargetId === `group:${group}`}
						count={groupCount(group)}
						onSelect={onSelectGroup}
					/>
				{/each}
				<div class="launch-menu-divider"></div>
			{/if}
			{#each avatarSettings as setting}
				<RendererLaunchProfileMenuItem
					{setting}
					selected={launchTargetId === setting.id || (!launchTargetId && launchTargetSetting?.id === setting.id)}
					{iconSrc}
					onSelect={onSelectSetting}
				/>
			{/each}
		</div>
	{/if}
</div>
