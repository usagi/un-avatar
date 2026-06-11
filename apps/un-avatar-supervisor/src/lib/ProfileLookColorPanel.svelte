<script lang="ts">
	import type { ProfileSettingValue } from "./profileTypes";
	import { _ } from "svelte-i18n";
	import { formatFixed } from "./formatting";
	import ProfileLookColorBasicFields from "./ProfileLookColorBasicFields.svelte";
	import ProfileLookColorPresetFields from "./ProfileLookColorPresetFields.svelte";
	import ProfileLookColorWhiteBalanceFields from "./ProfileLookColorWhiteBalanceFields.svelte";
	import type { ProfileColorGradingSetting } from "./profileLookTypes";

	export let setting: ProfileColorGradingSetting;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
</script>

<details class="effect-panel">
	<summary>
		<span>{$_("profiles.editor.look_color_grading")}</span>
		<small>{$_("profiles.editor.look_color_grading_summary")}</small>
		<span class="effect-panel-status">{setting.color_look} · exp {formatFixed(setting.color_exposure)}</span>
	</summary>
	<ProfileLookColorBasicFields {setting} {busy} {onUpdateSettingValue} />
	<ProfileLookColorPresetFields {setting} {busy} {onUpdateSettingValue} />
	<ProfileLookColorWhiteBalanceFields {setting} {busy} {onUpdateSettingValue} />
</details>
