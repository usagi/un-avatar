<script lang="ts">
	import type { ProfileSettingValue } from "./profileTypes";
	import { _ } from "svelte-i18n";
	import { BONE_COLLIDER_PARTS, type BoneColliderSetting } from "./boneColliderFields";
	import ProfileBoneColliderRadiusRow from "./ProfileBoneColliderRadiusRow.svelte";

	export let setting: BoneColliderSetting;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
</script>

<div class="subgroup bone-collider-group">
	<label class="profile-channel-heading" data-hint={$_("profiles.editor.bone_colliders_hint")}>
		<input
			type="checkbox"
			checked={setting.bone_colliders_enabled}
			onchange={(event) => onUpdateSettingValue("physics.bone_colliders.enabled", (event.currentTarget as HTMLInputElement).checked)}
		/>
		<span>{$_("profiles.editor.bone_colliders")}</span>
	</label>
	<div class="collider-scale-grid">
		{#each BONE_COLLIDER_PARTS as part}
			{@const radiusMm = setting[part.settingKey]}
			<ProfileBoneColliderRadiusRow
				labelKey={part.labelKey}
				fieldKey={part.key}
				{radiusMm}
				disabled={!setting.bone_colliders_enabled || busy}
				{onUpdateSettingValue}
			/>
		{/each}
	</div>
</div>
