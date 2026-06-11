<script lang="ts">
	import type { IdentitySetting, ProfileSettingValue } from "./profileTypes";
	import { _ } from "svelte-i18n";
	import { FolderOpen } from "lucide-svelte";
	import ProfileIdentityIconField from "./ProfileIdentityIconField.svelte";
	import ProfileIdentityTextFields from "./ProfileIdentityTextFields.svelte";
	import ProfileToggleField from "./ProfileToggleField.svelte";

	export let setting: IdentitySetting;
	export let iconUrl: string;
	export let busy = false;
	export let onBrowseIcon: () => void | Promise<void>;
	export let onApplyAvatarThumbnail: () => void | Promise<void>;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
	export let onActivate: () => void = () => {};
</script>

<section
	class="editor-section profile-section profile-identity-section"
	data-profile-section="identity"
	onfocusin={onActivate}
	data-hint={$_("profiles.hints.identity.section")}
>
	<div class="section-title-row">
		<h3>{$_("profiles.editor.profile_setting_heading")}</h3>
	</div>
	<div class="identity-row">
		<button class="icon-picker" disabled={busy} onclick={() => onBrowseIcon()}>
			<img src={iconUrl} alt="" />
			<span><FolderOpen size={13} /></span>
		</button>
		<label data-hint={$_("profiles.hints.identity.name")}
			>{$_("profiles.editor.name")}<input
				value={setting.name}
				onchange={(event) => onUpdateSettingValue("profile.display_name", (event.currentTarget as HTMLInputElement).value)}
			/></label
		>
	</div>
	<ProfileIdentityTextFields {setting} {onUpdateSettingValue} />
	<ProfileIdentityIconField {setting} {busy} {onBrowseIcon} {onApplyAvatarThumbnail} {onUpdateSettingValue} />
	<ProfileToggleField
		label={$_("profiles.editor.allow_multiple_renderers")}
		hint={$_("profiles.hints.identity.allow_multiple")}
		checked={setting.allow_multiple_renderers}
		onChange={(checked) => onUpdateSettingValue("profile.allow_multiple_renderers", checked)}
	/>
</section>
