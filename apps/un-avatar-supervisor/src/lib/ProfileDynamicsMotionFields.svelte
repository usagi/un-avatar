<script lang="ts">
	import { _ } from "svelte-i18n";
	import { DYNAMICS_ENABLE_ALL_ON_LAUNCH_FIELD, DYNAMICS_ENABLED_FIELD } from "./dynamicsPresets";
	import ProfileDynamicsOverrides from "./ProfileDynamicsOverrides.svelte";
	import ProfileToggleField from "./ProfileToggleField.svelte";
	import type { MotionSetting, ProfileSettingValue } from "./profileTypes";

	export let setting: Pick<
		MotionSetting,
		| "spring_bones"
		| "dynamics_enable_all_on_launch"
		| "contact_parameter_emission"
		| "spring_bone_category_overrides"
		| "apply_vmc_root_translation"
	>;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
</script>

<div class="section-grid profile-channel-fields">
	<ProfileToggleField
		label={$_("profiles.editor.spring_bone")}
		phaseTag="UNPhysics"
		hint={$_("profiles.hints.motion.spring_bone")}
		checked={setting.spring_bones}
		onChange={(checked) => onUpdateSettingValue(DYNAMICS_ENABLED_FIELD, checked)}
	/>
	<ProfileToggleField
		label={$_("profiles.editor.dynamics_enable_all_on_launch")}
		phaseTag="UNAvatar"
		hint={$_("profiles.hints.motion.dynamics_enable_all_on_launch")}
		checked={setting.dynamics_enable_all_on_launch}
		onChange={(checked) => onUpdateSettingValue(DYNAMICS_ENABLE_ALL_ON_LAUNCH_FIELD, checked)}
	/>
	<ProfileToggleField
		label={$_("profiles.editor.contact_parameter_emission")}
		phaseTag="Contacts"
		hint={$_("profiles.hints.motion.contact_parameter_emission")}
		checked={setting.contact_parameter_emission}
		onChange={(checked) => onUpdateSettingValue("physics.contacts.parameter_emission", checked)}
	/>
	<ProfileDynamicsOverrides
		overrides={setting.spring_bone_category_overrides}
		dynamicsEnabled={setting.spring_bones}
		{busy}
		{onUpdateSettingValue}
	/>
	<ProfileToggleField
		label={$_("profiles.editor.apply_vmc_root_translation")}
		phaseTag="VMC"
		hint={$_("profiles.hints.motion.vmc_root_translation")}
		checked={setting.apply_vmc_root_translation}
		onChange={(checked) => onUpdateSettingValue("motion.apply_vmc_root_translation", checked)}
	/>
</div>
