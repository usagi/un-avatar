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
	export let onUpdateSettingValues: (updates: readonly [field: string, value: ProfileSettingValue][]) => void | Promise<void>;

	const colorLookLabelKeys: Record<string, string> = {
		neutral: "profiles.editor.color_look_neutral",
		warm: "profiles.editor.color_look_warm",
		cool: "profiles.editor.color_look_cool",
		film: "profiles.editor.color_look_film",
		soft: "profiles.editor.color_look_soft",
		pop: "profiles.editor.color_look_pop",
	};

	$: colorLookLabel = $_(colorLookLabelKeys[setting.color_look] ?? "profiles.editor.color_look_neutral");
</script>

<details class="effect-panel">
	<summary>
		<span>{$_("profiles.editor.look_color_grading")}</span>
		<small>{$_("profiles.editor.look_color_grading_summary")}</small>
		<span class="effect-panel-status"
			>{colorLookLabel} · {$_("profiles.editor.look_strength")} {formatFixed(setting.color_look_intensity, 2)} · {$_(
				"profiles.editor.look_exposure"
			)} {formatFixed(setting.color_exposure, 2)}</span
		>
	</summary>
	<div class="subgroup effect-subgroup color-grading-subgroup color-look-subgroup">
		<div class="profile-group-heading">{$_("profiles.editor.look_color_look")}</div>
		<ProfileLookColorPresetFields {setting} {busy} {onUpdateSettingValue} {onUpdateSettingValues} />
	</div>
	<div class="subgroup effect-subgroup color-grading-subgroup color-white-balance-subgroup">
		<div class="profile-group-heading">{$_("profiles.editor.look_white_balance")}</div>
		<ProfileLookColorWhiteBalanceFields {setting} {busy} {onUpdateSettingValue} />
	</div>
	<div class="subgroup effect-subgroup color-grading-subgroup color-basic-subgroup">
		<div class="profile-group-heading">{$_("profiles.editor.look_color_basic")}</div>
		<ProfileLookColorBasicFields {setting} {busy} {onUpdateSettingValue} />
	</div>
</details>
