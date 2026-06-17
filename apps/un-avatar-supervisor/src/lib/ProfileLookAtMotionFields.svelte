<script lang="ts">
	import { _ } from "svelte-i18n";
	import ProfileNumberInputField from "./ProfileNumberInputField.svelte";
	import type { MotionSetting, ProfileSettingValue } from "./profileTypes";

	export let setting: Pick<MotionSetting, "look_at_enabled" | "look_at_clamp_deg">;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
</script>

<div class="subgroup">
	<label class="profile-channel-heading" data-hint={$_("profiles.hints.motion.look_at")}>
		<input
			type="checkbox"
			checked={setting.look_at_enabled}
			onchange={(event) => onUpdateSettingValue("motion.look_at.enabled", (event.currentTarget as HTMLInputElement).checked)}
		/>
		<span>{$_("profiles.editor.look_at")}</span>
	</label>
	<div class="section-grid profile-channel-fields">
		<ProfileNumberInputField
			label={$_("profiles.editor.look_at_clamp_deg")}
			hint={$_("profiles.hints.motion.look_at_clamp_deg")}
			value={setting.look_at_clamp_deg ?? 30}
			min={0}
			max={90}
			step={1}
			disabled={busy}
			onChange={(value) => onUpdateSettingValue("motion.look_at.clamp_deg", value)}
		/>
	</div>
</div>
