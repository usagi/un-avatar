<script lang="ts">
	import { settingSummary } from "./profileLabels";
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
		<small>{settingSummary(setting)}</small>
	</span>
</button>
