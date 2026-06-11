<script lang="ts">
	import { _ } from "svelte-i18n";
	import { metadataSubtitle, metadataTitle, type VrmMetadataInfo } from "./vrmMetadata";

	export let metadata: VrmMetadataInfo;
	export let pendingPath: string | null;

	$: title = metadataTitle(metadata);
	$: subtitle = metadataSubtitle(metadata);
</script>

<header class="vrm-metadata-header">
	<p>{pendingPath ? $_("vrm_metadata.review_before_use") : $_("vrm_metadata.title")}</p>
	<h2>{title}</h2>
	{#if subtitle}
		<small>{subtitle}</small>
	{/if}
</header>
