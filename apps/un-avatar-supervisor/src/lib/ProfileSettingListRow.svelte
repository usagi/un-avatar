<script lang="ts">
	import { _ } from "svelte-i18n";
	import { GripVertical } from "lucide-svelte";
	import { aaModeLabel, basename } from "./formatting";
	import { settingDragStyleValues, type ProfileListSetting, type SettingPointerDrag } from "./profileListTypes";
	import { localizedLookLabel, localizedOutputLabel } from "./profileStageSummary";

	export let setting: ProfileListSetting;
	export let selected = false;
	export let dragging = false;
	export let runningCount = 0;
	export let settingPointerDrag: SettingPointerDrag | null;
	export let iconSrc: (path: string | null) => string;
	export let onSelect: (settingId: string) => void;
	export let onBeginDrag: (event: PointerEvent, settingId: string) => void;

	$: dragStyle = settingDragStyleValues(dragging, settingPointerDrag);
</script>

<button
	data-hint={$_("profiles.hints.identity.list_row", {
		values: { name: setting.name },
	})}
	data-profile-id={setting.id}
	class:selected
	class:dragging
	class:running={runningCount > 0}
	style:--drag-left={dragStyle.left}
	style:--drag-top={dragStyle.top}
	style:--drag-width={dragStyle.width}
	style:--drag-height={dragStyle.height}
	draggable="false"
	onclick={() => onSelect(setting.id)}
>
	<span
		class="drag-handle"
		data-hint={$_("profiles.hints.identity.drag_reorder")}
		aria-label={$_("profiles.hints.identity.drag_reorder")}
		role="button"
		tabindex="0"
		onpointerdown={(event) => onBeginDrag(event, setting.id)}
	>
		<GripVertical size={16} />
	</span>
	<img src={iconSrc(setting.icon_path)} alt="" />
	<span class="setting-card-body">
		<strong>{setting.name}</strong>
		<small>{setting.group ? `${setting.group} · ` : ""}{basename(setting.avatar_path)}</small>
		<span class="setting-card-chips">
			<span>{localizedOutputLabel(setting, $_)}</span>
			<span>{aaModeLabel(setting.aa)}</span>
			<span>{localizedLookLabel(setting, $_)}</span>
		</span>
	</span>
	{#if runningCount > 0}
		<span class="storage-badge storage-user">
			{runningCount === 1
				? $_("profiles.live.running")
				: $_("profiles.live.running_count", {
						values: { count: runningCount },
					})}
		</span>
	{/if}
</button>
