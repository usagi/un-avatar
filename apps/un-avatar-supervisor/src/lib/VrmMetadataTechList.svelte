<script lang="ts">
	import { _ } from "svelte-i18n";
	import { vrmMetadataFieldLabel, vrmMetadataFieldValue, type VrmMetadataField } from "./vrmMetadata";

	export let items: VrmMetadataField[] = [];
</script>

{#if items.length > 0}
	<div class="vrm-tech-list" aria-label={$_("vrm_metadata.model_statistics")}>
		{#each items as item}
			<div>
				<span>{vrmMetadataFieldLabel(item.label, $_)}</span>
				<strong>{vrmMetadataFieldValue(item, $_)}</strong>
			</div>
		{/each}
	</div>
{/if}
