<script lang="ts">
	import { _ } from "svelte-i18n";
	import { localizedSettingSummary } from "./profileStageSummary";
	import type { ProfileLaunchSetting } from "./profileTypes";

	export let setting: ProfileLaunchSetting;
	export let selected = false;
	export let iconSrc: (path: string | null) => string;
	export let onSelect: (settingId: string) => void;
</script>

<button class:selected onclick={() => onSelect(setting.id)}>
	<img src={iconSrc(setting.icon_path)} alt="" />
	<span>
		<strong>{setting.name}</strong>
		<small>{localizedSettingSummary(setting, $_)}</small>
	</span>
</button>
