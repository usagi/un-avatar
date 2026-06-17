<script lang="ts">
	import type { MotionSetting, ProfileSettingValue } from "./profileTypes";
	import { _ } from "svelte-i18n";
	import ProfileBoneColliderSettings from "./ProfileBoneColliderSettings.svelte";
	import ProfileDynamicsMotionFields from "./ProfileDynamicsMotionFields.svelte";

	export let setting: MotionSetting;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
	export let onActivate: () => void;
</script>

<section
	class="editor-section profile-physics-section"
	data-profile-section="physics"
	onfocusin={onActivate}
	data-hint={$_("profiles.hints.physics.section")}
>
	<div class="section-title-row">
		<h3>{$_("profiles.editor.physics")}</h3>
	</div>

	<ProfileDynamicsMotionFields {setting} {busy} {onUpdateSettingValue} />
	<ProfileBoneColliderSettings {setting} {busy} {onUpdateSettingValue} />
</section>
