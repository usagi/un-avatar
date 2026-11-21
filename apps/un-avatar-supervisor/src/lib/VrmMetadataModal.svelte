<script lang="ts">
	import ProfileDestination from "./ProfileDestination.svelte";
	import type { AvatarSetting } from "./profileTypes";
	export let targetProfileId = "";
	export let targetProfile: AvatarSetting | null = null;
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
		<VrmMetadataPortrait
			metadata={modal.metadata}
		/>
		<div class="vrm-metadata-body">
			<ProfileDestination profileId={targetProfileId} profile={targetProfile} />
			{#if modal.saveError}
				<section class="unavatar-save-error" role="alert">
					<strong>{$_(modal.avatarPathSaved ? "unavatar_rights.save_path_succeeded_icon_failed" : "unavatar_rights.save_failed")}</strong>
					<p>{$_(modal.saveError.includes("profile_conflict") ? "unavatar_rights.save_conflict_help" : "vrm_metadata.save_retry_help")}</p>
					<details><summary>{$_("unavatar_rights.save_error_details")}</summary><p>{modal.saveError}</p></details>
				</section>
			{/if}
			<VrmMetadataDetails metadata={modal.metadata} />
			<VrmMetadataActions
				pendingPath={modal.pendingPath}
				avatarPathSaved={modal.avatarPathSaved ?? false}
				hasThumbnail={Boolean(modal.metadata.thumbnail_data_url)}
				{busy}
				bind:useThumbnailForProfileIconOnAccept
				{onClose}
				{onAcceptAndUse}
			/>
		</div>
	</div>
</div>
