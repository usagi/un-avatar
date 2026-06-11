<script lang="ts">
	import type { ProfileSettingValue } from "./profileTypes";
	import { _ } from "svelte-i18n";
	import { formatFixed } from "./formatting";
	import ProfileLookRimFields from "./ProfileLookRimFields.svelte";
	import type { ColorModeChangeHandler } from "./profileColorActions";
	import type { ProfileRimSetting } from "./profileLookTypes";
	import type { ColorDisplayMode } from "./storageState";

	export let setting: ProfileRimSetting;
	export let busy = false;
	export let colorDisplayMode: ColorDisplayMode;
	export let onColorModeChange: ColorModeChangeHandler;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
</script>

<details class="effect-panel">
	<summary>
		<span>{$_("profiles.editor.look_rim")}</span>
		<small>{$_("profiles.editor.look_rim_summary")}</small>
		<span class="effect-panel-status">{setting.rim_policy} · {formatFixed(setting.rim_intensity ?? 0.35)}</span>
	</summary>
	<ProfileLookRimFields {setting} {busy} {colorDisplayMode} {onColorModeChange} {onUpdateSettingValue} />
</details>
