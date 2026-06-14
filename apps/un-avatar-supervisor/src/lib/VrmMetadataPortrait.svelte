<script lang="ts">
	import { _ } from "svelte-i18n";
	import { metadataInitial, type VrmMetadataInfo } from "./vrmMetadata";

	export let metadata: VrmMetadataInfo;
	export let pendingPath: string | null = null;
	export let useThumbnailForProfileIconOnAccept = false;
</script>

<div class="vrm-metadata-portrait">
	{#if metadata.thumbnail_data_url}
		<div class="vrm-metadata-preview-frame">
			<img class="vrm-metadata-thumbnail" src={metadata.thumbnail_data_url} alt="" />
		</div>
	{:else}
		<div class="vrm-metadata-sigil">
			{metadataInitial(metadata)}
		</div>
	{/if}
	<span
		>{$_("vrm_metadata.format_spec", {
			values: {
				format: metadata.vrm_format,
				spec: metadata.spec_version,
			},
		})}</span
	>
	{#if pendingPath && metadata.thumbnail_data_url}
		<div class="unavatar-icon-crop-panel vrm-profile-icon-panel" class:enabled={useThumbnailForProfileIconOnAccept}>
			<label class="unavatar-icon-crop-toggle" title={$_("vrm_metadata.use_thumbnail_as_profile_icon_hint")}>
				<input type="checkbox" bind:checked={useThumbnailForProfileIconOnAccept} />
				<span>{$_("vrm_metadata.use_thumbnail_as_profile_icon")}</span>
			</label>
		</div>
	{/if}
</div>
