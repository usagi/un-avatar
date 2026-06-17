<script lang="ts">
	import { _ } from "svelte-i18n";
	import ProfileRangeNumberField from "./ProfileRangeNumberField.svelte";
	import type { ProfileSettingValue } from "./profileTypes";
	import type { ProfileColorGradingSetting } from "./profileLookTypes";

	export let setting: Pick<ProfileColorGradingSetting, "color_temperature" | "color_tint">;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
</script>

<ProfileRangeNumberField
	label={$_("profiles.editor.look_temperature")}
	hint={$_("profiles.hints.look.temperature")}
	value={setting.color_temperature}
	rangeMin={-1}
	rangeMax={1}
	step={0.01}
	decimals={2}
	disabled={busy}
	onChange={(value) => onUpdateSettingValue("environment.color.temperature", value)}
/>
<ProfileRangeNumberField
	label={$_("profiles.editor.look_tint")}
	hint={$_("profiles.hints.look.tint")}
	value={setting.color_tint}
	rangeMin={-1}
	rangeMax={1}
	step={0.01}
	decimals={2}
	disabled={busy}
	onChange={(value) => onUpdateSettingValue("environment.color.tint", value)}
/>
