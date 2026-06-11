<script lang="ts">
	import { _ } from "svelte-i18n";
	import { metadataInitial, type VrmMetadataInfo } from "./vrmMetadata";

	export let metadata: VrmMetadataInfo;
</script>

<div class="vrm-metadata-portrait" aria-hidden="true">
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
</div>
