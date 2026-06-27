<script lang="ts">
	import { _ } from "svelte-i18n";
	import { DYNAMICS_ENABLED_FIELD } from "./dynamicsPresets";
	import ProfileDynamicsGroupOverrides from "./ProfileDynamicsGroupOverrides.svelte";
	import ProfileDynamicsMatchOverrides from "./ProfileDynamicsMatchOverrides.svelte";
	import ProfileDynamicsOverrides from "./ProfileDynamicsOverrides.svelte";
	import ProfileToggleField from "./ProfileToggleField.svelte";
	import type { MotionSetting, ProfileSettingValue } from "./profileTypes";

	export let setting: Pick<
		MotionSetting,
		| "dynamics_enabled"
		| "contact_parameter_emission"
		| "dynamics_category_overrides"
		| "dynamics_match_overrides"
		| "dynamics_group_overrides"
	>;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
</script>

<div class="section-grid profile-channel-fields">
	<div class="physics-main-control" data-hint={$_("profiles.hints.motion.dynamics_compat")}>
		<ProfileToggleField
			label={$_("profiles.editor.dynamics")}
			phaseTag={$_("profiles.editor.dynamics_source_badge")}
			hint={$_("profiles.hints.motion.dynamics")}
			checked={setting.dynamics_enabled}
			onChange={(checked) => onUpdateSettingValue(DYNAMICS_ENABLED_FIELD, checked)}
		/>
	</div>
	<ProfileToggleField
		label={$_("profiles.editor.contact_parameter_emission")}
		hint={$_("profiles.hints.motion.contact_parameter_emission")}
		checked={setting.contact_parameter_emission}
		onChange={(checked) => onUpdateSettingValue("physics.contacts.parameter_emission", checked)}
	/>
	<ProfileDynamicsMatchOverrides
		overrides={setting.dynamics_match_overrides}
		dynamicsEnabled={setting.dynamics_enabled}
		{busy}
		{onUpdateSettingValue}
	/>
	<ProfileDynamicsGroupOverrides
		overrides={setting.dynamics_group_overrides}
		dynamicsEnabled={setting.dynamics_enabled}
		{busy}
		{onUpdateSettingValue}
	/>
	<ProfileDynamicsOverrides
		overrides={setting.dynamics_category_overrides}
		dynamicsEnabled={setting.dynamics_enabled}
		{busy}
		{onUpdateSettingValue}
	/>
</div>
