<script lang="ts">
	import type { MotionSetting, ProfileSettingValue } from "./profileTypes";
	import { _ } from "svelte-i18n";
	import ProfileLookAtMotionFields from "./ProfileLookAtMotionFields.svelte";
	import ProfileMotionChannels from "./ProfileMotionChannels.svelte";
	import ProfileToggleField from "./ProfileToggleField.svelte";

	export let setting: MotionSetting;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
	export let onActivate: () => void;
</script>

<section
	class="editor-section profile-motion-section"
	data-profile-section="motion"
	onfocusin={onActivate}
	data-hint={$_("profiles.hints.motion.section")}
>
	<div class="section-title-row">
		<h3>{$_("profiles.editor.motion")}</h3>
	</div>

	<ProfileMotionChannels {setting} {busy} {onUpdateSettingValue} />

	<ProfileLookAtMotionFields {setting} {busy} {onUpdateSettingValue} />

	<div class="section-grid profile-channel-fields">
		<ProfileToggleField
			label={$_("profiles.editor.apply_root_translation")}
			hint={$_("profiles.hints.motion.root_translation")}
			checked={setting.apply_vmc_root_translation}
			onChange={(checked) => onUpdateSettingValue("motion.apply_vmc_root_translation", checked)}
		/>
	</div>
</section>
