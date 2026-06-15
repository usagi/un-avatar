<script lang="ts">
	import { _ } from "svelte-i18n";
	import { Activity, Play } from "lucide-svelte";
	import RendererLaunchTargetSelect from "./RendererLaunchTargetSelect.svelte";
	import type { ProfileLaunchSetting } from "./profileTypes";

	export let busy = false;
	export let launchTargetSetting: ProfileLaunchSetting | null;
	export let launchGroupName: string | null;
	export let launchTargetId: string | null;
	export let launchMenuOpen = false;
	export let showStoppedRenderers = false;
	export let avatarSettings: ProfileLaunchSetting[];
	export let profileGroups: string[];
	export let message = "";
	export let iconSrc: (path: string | null) => string;
	export let groupCount: (group: string) => number;
	export let onLaunch: () => void | Promise<void>;
	export let onRefresh: () => void | Promise<void>;
	export let onToggleLaunchMenu: () => void;
	export let onShowStoppedRenderersChange: (checked: boolean) => void;
	export let onSelectGroup: (group: string) => void;
	export let onSelectSetting: (settingId: string) => void;
</script>

<div class="toolbar">
	<button type="button" class="primary" disabled={busy || (!launchTargetSetting && !launchGroupName)} onclick={() => onLaunch()}
		><Play size={16} />{$_("renderers.toolbar.launch")}</button
	>
	<RendererLaunchTargetSelect
		{launchTargetSetting}
		{launchGroupName}
		{launchTargetId}
		bind:launchMenuOpen
		{avatarSettings}
		{profileGroups}
		{iconSrc}
		{groupCount}
		onToggleOpen={onToggleLaunchMenu}
		{onSelectGroup}
		{onSelectSetting}
	/>
	<button type="button" onclick={() => onRefresh()}><Activity size={16} />{$_("renderers.toolbar.refresh")}</button>
	<label class="toggle-field renderer-toolbar-toggle">
		<input type="checkbox" checked={showStoppedRenderers} onchange={(event) => onShowStoppedRenderersChange(event.currentTarget.checked)} />
		<span>{$_("renderers.toolbar.show_stopped")}</span>
	</label>
	<span class="toolbar-message">{message}</span>
</div>
