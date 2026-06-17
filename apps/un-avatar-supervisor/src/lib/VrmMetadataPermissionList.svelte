<script lang="ts">
	import { _ } from "svelte-i18n";
	import { vrmMetadataFieldLabel, vrmMetadataFieldValue, vrmMetadataPermissionTone, type VrmMetadataField } from "./vrmMetadata";

	export let items: VrmMetadataField[] = [];
</script>

{#if items.length > 0}
	<div class="vrm-permission-list">
		{#each items as item}
			<div class={`vrm-permission-card is-${vrmMetadataPermissionTone(item)}`}>
				<span>{vrmMetadataFieldLabel(item.label, $_)}</span>
				<strong>{vrmMetadataFieldValue(item, $_)}</strong>
			</div>
		{/each}
	</div>
{/if}
