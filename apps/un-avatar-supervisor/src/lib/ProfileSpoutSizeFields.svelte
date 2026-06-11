<script lang="ts">
	import { _ } from "svelte-i18n";
	import ProfileNumberInputField from "./ProfileNumberInputField.svelte";
	import type { ProfileSettingValue, SpoutOutputSetting } from "./profileTypes";

	export let setting: Pick<SpoutOutputSetting, "spout_width" | "spout_height">;
	export let disabled = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
</script>

<div class="spout-size-fields">
	<ProfileNumberInputField
		label={$_("profiles.editor.spout2_width")}
		hint={$_("profiles.hints.output.spout_width")}
		value={setting.spout_width ?? 1280}
		min={1}
		max={16384}
		step={1}
		{disabled}
		onChange={(value) => onUpdateSettingValue("output.spout2.width", value)}
	/>
	<ProfileNumberInputField
		label={$_("profiles.editor.spout2_height")}
		hint={$_("profiles.hints.output.spout_height")}
		value={setting.spout_height ?? 720}
		min={1}
		max={16384}
		step={1}
		{disabled}
		onChange={(value) => onUpdateSettingValue("output.spout2.height", value)}
	/>
</div>
