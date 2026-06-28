<script lang="ts">
	import { _ } from "svelte-i18n";
	import ProfileSelectField from "./ProfileSelectField.svelte";
	import RangeNumberField from "./RangeNumberField.svelte";
	import type { ProfileSettingValue, QualitySetting } from "./profileTypes";
	import { AA_MODE_OPTIONS } from "./qualityOptions";

	export let setting: Pick<QualitySetting, "aa" | "target_fps">;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;

	const MIN_TARGET_FPS = 30;
	const MAX_TARGET_FPS = 300;

	function clampTargetFps(value: number): number {
		return Number.isFinite(value) ? Math.min(MAX_TARGET_FPS, Math.max(MIN_TARGET_FPS, Math.round(value))) : 60;
	}
</script>

<fieldset class="quality-fieldset">
	<legend>{$_("profiles.editor.frame_pacing")}</legend>
	<label class="field-row range-setting-row">
		<span>{$_("profiles.editor.target_fps")}</span>
		<RangeNumberField
			value={clampTargetFps(setting.target_fps)}
			rangeMin={MIN_TARGET_FPS}
			rangeMax={MAX_TARGET_FPS}
			numberMin={MIN_TARGET_FPS}
			numberMax={MAX_TARGET_FPS}
			step={1}
			disabled={busy}
			onChange={(value) => onUpdateSettingValue("runtime.target_fps", clampTargetFps(value))}
		/>
	</label>
	<ProfileSelectField
		label={$_("profiles.editor.anti_aliasing")}
		hint={$_("profiles.hints.quality.anti_aliasing")}
		value={setting.aa}
		disabled={busy}
		options={AA_MODE_OPTIONS}
		onChange={(value) => onUpdateSettingValue("render_quality.aa", value)}
	/>
</fieldset>
