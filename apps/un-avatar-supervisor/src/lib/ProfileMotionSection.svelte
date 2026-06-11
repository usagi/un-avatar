<script lang="ts">
	import type { MotionSetting, ProfileSettingValue } from "./profileTypes";
	import { _ } from "svelte-i18n";
	import ProfileBoneColliderSettings from "./ProfileBoneColliderSettings.svelte";
	import ProfileLookAtMotionFields from "./ProfileLookAtMotionFields.svelte";
	import ProfileMotionChannels from "./ProfileMotionChannels.svelte";
	import ProfileSpringBoneMotionFields from "./ProfileSpringBoneMotionFields.svelte";

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

	<ProfileSpringBoneMotionFields {setting} {busy} {onUpdateSettingValue} />
	<ProfileBoneColliderSettings {setting} {busy} {onUpdateSettingValue} />
</section>
