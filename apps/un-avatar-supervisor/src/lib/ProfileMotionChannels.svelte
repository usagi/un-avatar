<script lang="ts">
	import { _ } from "svelte-i18n";
	import { UNMOTION_CHANNEL_CONFIG, VMC_CHANNEL_CONFIG, vmcAddressValue } from "./motionOptions";
	import ProfileMotionTextChannel from "./ProfileMotionTextChannel.svelte";
	import type { MotionSetting, ProfileSettingValue } from "./profileTypes";

	export let setting: MotionSetting;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
</script>

<ProfileMotionTextChannel
	enabled={setting.motion_unmotion_enabled}
	value={setting.unmotion_zenoh_key ?? ""}
	placeholder={UNMOTION_CHANNEL_CONFIG.placeholder}
	{busy}
	headingLabel={$_(UNMOTION_CHANNEL_CONFIG.headingLabelKey)}
	headingHint={$_(UNMOTION_CHANNEL_CONFIG.headingHintKey)}
	fieldLabel={$_(UNMOTION_CHANNEL_CONFIG.fieldLabelKey)}
	fieldHint={$_(UNMOTION_CHANNEL_CONFIG.fieldHintKey)}
	enabledField={UNMOTION_CHANNEL_CONFIG.enabledField}
	valueField={UNMOTION_CHANNEL_CONFIG.valueField}
	{onUpdateSettingValue}
/>

<ProfileMotionTextChannel
	enabled={setting.motion_vmc_enabled}
	value={vmcAddressValue(setting.vmc_address, setting.vmc_port)}
	{busy}
	headingLabel={$_(VMC_CHANNEL_CONFIG.headingLabelKey)}
	headingHint={$_(VMC_CHANNEL_CONFIG.headingHintKey)}
	fieldLabel={$_(VMC_CHANNEL_CONFIG.fieldLabelKey)}
	fieldHint={$_(VMC_CHANNEL_CONFIG.fieldHintKey)}
	enabledField={VMC_CHANNEL_CONFIG.enabledField}
	valueField={VMC_CHANNEL_CONFIG.valueField}
	{onUpdateSettingValue}
/>
