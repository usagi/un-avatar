<script lang="ts">
	import type { AvatarFileSetting, ProfileSettingValue, UnavatarWardrobeOptions } from "./profileTypes";
	import { _ } from "svelte-i18n";
	import { FolderOpen } from "lucide-svelte";

	export let setting: AvatarFileSetting;
	export let wardrobeOptions: UnavatarWardrobeOptions | null = null;
	export let busy = false;
	export let onBrowseAvatar: () => void | Promise<void>;
	export let onReviewMetadata: () => void | Promise<void>;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
	export let onActivate: () => void = () => {};

	$: isUnavatar = (setting.avatar_path ?? "").trim().toLowerCase().endsWith(".unavatar");
</script>

<section
	class="editor-section section-grid profile-avatar-section"
	data-profile-section="avatar"
	onfocusin={onActivate}
	data-hint={$_("profiles.hints.avatar.section")}
>
	<div class="section-title-row">
		<h3>{$_("profiles.editor.avatar")}</h3>
		<span class="setting-scope">{$_("profiles.editor.launch_time")}</span>
	</div>
	<label class="path-field avatar-path-field"
		><span>{$_("profiles.editor.avatar_file")}</span><input
			value={setting.avatar_path ?? ""}
			disabled={busy}
			onchange={(event) => onUpdateSettingValue("avatar_path", (event.currentTarget as HTMLInputElement).value)}
		/>
		<button class="field-button" disabled={busy} onclick={() => onBrowseAvatar()}
			><FolderOpen size={15} />{$_("profiles.editor.browse")}</button
		>
		{#if setting.avatar_path}
			<button class="field-button metadata-review-button" disabled={busy} onclick={() => onReviewMetadata()}
				>{$_("profiles.editor.review_metadata")}</button
			>
		{/if}</label
	>
	{#if wardrobeOptions?.available}
		<label class="select-field"
			><span>{$_("profiles.editor.wardrobe")}</span><select
				value={setting.wardrobe_set ?? ""}
				disabled={busy}
				onchange={(event) => onUpdateSettingValue("wardrobe_set", (event.currentTarget as HTMLSelectElement).value)}
			>
				<option value="">{wardrobeOptions.base_label || "Base"}</option>
				{#each wardrobeOptions.sets as set}
					<option value={set.id}>{set.name || set.id}</option>
				{/each}
			</select></label
		>
	{:else if isUnavatar}
		<div class="profile-inline-note profile-inline-note-warning">
			Wardrobe unavailable{wardrobeOptions?.error ? `: ${wardrobeOptions.error}` : ""}
		</div>
	{/if}
</section>
