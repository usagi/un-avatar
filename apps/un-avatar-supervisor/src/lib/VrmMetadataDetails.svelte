<script lang="ts">
	import { _ } from "svelte-i18n";
	import type { VrmMetadataInfo } from "./vrmMetadata";
	import VrmMetadataHeader from "./VrmMetadataHeader.svelte";
	import VrmMetadataInfoGrid from "./VrmMetadataInfoGrid.svelte";
	import VrmMetadataPermissionList from "./VrmMetadataPermissionList.svelte";
	import VrmMetadataTechList from "./VrmMetadataTechList.svelte";

	export let metadata: VrmMetadataInfo;
</script>

<VrmMetadataHeader {metadata} />
<div class="vrm-metadata-scroll">
	<VrmMetadataInfoGrid {metadata} />
	<VrmMetadataTechList items={metadata.technical_stats} />
	<VrmMetadataPermissionList items={metadata.permissions} />
	{#if metadata.third_party_licenses}
		<section class="vrm-license-block">
			<h3>{$_("vrm_metadata.third_party_licenses")}</h3>
			<p>{metadata.third_party_licenses}</p>
		</section>
	{/if}
</div>
