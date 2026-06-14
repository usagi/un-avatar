<script lang="ts">
	import { _ } from "svelte-i18n";
	import { DYNAMICS_ENABLE_ALL_ON_LAUNCH_FIELD, DYNAMICS_ENABLED_FIELD } from "./dynamicsPresets";
	import ProfileDynamicsOverrides from "./ProfileDynamicsOverrides.svelte";
	import ProfileToggleField from "./ProfileToggleField.svelte";
	import type { MotionSetting, ProfileSettingValue } from "./profileTypes";

	export let setting: Pick<
		MotionSetting,
		| "dynamics_enabled"
		| "dynamics_enable_all_on_launch"
		| "contact_parameter_emission"
		| "dynamics_category_overrides"
	>;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
</script>

<div class="section-grid profile-channel-fields">
	<div class="physics-main-control" data-hint={$_("profiles.hints.motion.dynamics_compat")}>
		<ProfileToggleField
			label={$_("profiles.editor.dynamics")}
			hint={$_("profiles.hints.motion.dynamics")}
			checked={setting.dynamics_enabled}
			onChange={(checked) => onUpdateSettingValue(DYNAMICS_ENABLED_FIELD, checked)}
		/>
		<div class="physics-source-note">{$_("profiles.editor.dynamics_compat_sources")}</div>
	</div>
	<ProfileToggleField
		label={$_("profiles.editor.dynamics_enable_all_on_launch")}
		hint={$_("profiles.hints.motion.dynamics_enable_all_on_launch")}
		checked={setting.dynamics_enable_all_on_launch}
		onChange={(checked) => onUpdateSettingValue(DYNAMICS_ENABLE_ALL_ON_LAUNCH_FIELD, checked)}
	/>
	<ProfileToggleField
		label={$_("profiles.editor.contact_parameter_emission")}
		hint={$_("profiles.hints.motion.contact_parameter_emission")}
		checked={setting.contact_parameter_emission}
		onChange={(checked) => onUpdateSettingValue("physics.contacts.parameter_emission", checked)}
	/>
	<ProfileDynamicsOverrides
		overrides={setting.dynamics_category_overrides}
		dynamicsEnabled={setting.dynamics_enabled}
		{busy}
		{onUpdateSettingValue}
	/>
</div>
