<script lang="ts">
	import { _ } from "svelte-i18n";
	import { type VrmMetadataDialogState } from "./vrmMetadata";
	import VrmMetadataActions from "./VrmMetadataActions.svelte";
	import VrmMetadataDetails from "./VrmMetadataDetails.svelte";
	import VrmMetadataPortrait from "./VrmMetadataPortrait.svelte";

	export let modal: VrmMetadataDialogState;
	export let busy = false;
	export let useThumbnailForProfileIconOnAccept = false;
	export let onClose: () => void | Promise<void>;
	export let onAcceptAndUse: () => void | Promise<void>;
</script>

<div class="vrm-metadata-backdrop" role="presentation">
	<div class="vrm-metadata-dialog" role="dialog" aria-modal="true" aria-label={$_("vrm_metadata.title")}>
		<VrmMetadataPortrait metadata={modal.metadata} />
		<div class="vrm-metadata-body">
			<VrmMetadataDetails metadata={modal.metadata} pendingPath={modal.pendingPath} />
			<VrmMetadataActions
				pendingPath={modal.pendingPath}
				hasThumbnail={Boolean(modal.metadata.thumbnail_data_url)}
				{busy}
				bind:useThumbnailForProfileIconOnAccept
				{onClose}
				{onAcceptAndUse}
			/>
		</div>
	</div>
</div>
