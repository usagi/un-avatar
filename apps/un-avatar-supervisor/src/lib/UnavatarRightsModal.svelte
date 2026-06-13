<script lang="ts">
	import { _ } from "svelte-i18n";
	import type { UnavatarMetadataDialogState, UnavatarProfileIconCrop } from "./unavatarMetadata";

	export let modal: UnavatarMetadataDialogState;
	export let busy = false;
	export let profileIconCrop: UnavatarProfileIconCrop;
	export let onClose: () => void | Promise<void>;
	export let onAcceptAndUse: () => void | Promise<void>;

	$: metadata = modal.metadata;
	$: title = metadata.name?.trim() || metadata.file_name;
	let selectedSetId = "";
	let selectedPreviewIndex = 0;
	$: previewSets = metadata.preview_sets.length
		? metadata.preview_sets
		: metadata.preview_images.length
			? [{ id: "", name: "Preview", preview_images: metadata.preview_images }]
			: [];
	$: if (!previewSets.some((set) => set.id === selectedSetId)) selectedSetId = previewSets[0]?.id ?? "";
	$: selectedSet = previewSets.find((set) => set.id === selectedSetId) ?? previewSets[0] ?? null;
	$: selectedPreviews = selectedSet?.preview_images ?? [];
	$: if (selectedPreviewIndex >= selectedPreviews.length) selectedPreviewIndex = 0;
	$: selectedPreview = selectedPreviews[selectedPreviewIndex] ?? null;
	$: if (profileIconCrop.imageDataUrl !== (selectedPreview?.data_url ?? null)) {
		profileIconCrop = {
			...profileIconCrop,
			imageDataUrl: selectedPreview?.data_url ?? null,
		};
	}
	$: stats = [
		[$_("unavatar_rights.stats.wardrobe"), metadata.wardrobe_set_count],
		[$_("unavatar_rights.stats.dynamics"), metadata.dynamics_count],
		[$_("unavatar_rights.stats.contacts"), metadata.contact_count],
		[$_("unavatar_rights.stats.modular_avatar"), metadata.modular_avatar_component_count],
	];
</script>

<div class="vrm-metadata-backdrop" role="presentation">
	<div class="vrm-metadata-dialog unavatar-rights-dialog" role="dialog" aria-modal="true" aria-label={$_("unavatar_rights.title")}>
		<div class="vrm-metadata-portrait">
			{#if selectedPreview}
				<div class="vrm-metadata-preview-frame unavatar-crop-frame">
					<img
						class="vrm-metadata-thumbnail"
						src={selectedPreview.data_url}
						alt=""
						style={`transform: translate(${profileIconCrop.offsetX * 32}px, ${profileIconCrop.offsetY * 32}px) scale(${profileIconCrop.zoom});`}
					/>
				</div>
			{:else}
				<div class="vrm-metadata-sigil">UN</div>
			{/if}
			{#if previewSets.length > 1}
				<select
					class="unavatar-preview-set-select"
					aria-label={$_("unavatar_rights.preview_set")}
					bind:value={selectedSetId}
					onchange={() => (selectedPreviewIndex = 0)}
				>
					{#each previewSets as set}
						<option value={set.id}>{set.name || set.id}</option>
					{/each}
				</select>
			{/if}
			{#if selectedPreviews.length > 1}
				<div class="unavatar-preview-strip" aria-label={$_("unavatar_rights.preview_images")}>
					{#each selectedPreviews as preview, index}
						<button
							type="button"
							class:active={index === selectedPreviewIndex}
							title={preview.view ?? `preview ${index + 1}`}
							onclick={() => (selectedPreviewIndex = index)}
						>
							<img src={preview.data_url} alt="" />
						</button>
					{/each}
				</div>
			{/if}
			<span>.unavatar {metadata.spec_version ?? ""}</span>
		</div>
		<div class="vrm-metadata-body">
			<header class="vrm-metadata-header">
				<div>
					<p>{$_("unavatar_rights.eyebrow")}</p>
					<h2>{title}</h2>
					<span>{metadata.source_type ?? "UN Avatar"}{metadata.export_mode ? ` / ${metadata.export_mode}` : ""}</span>
				</div>
			</header>
			<div class="vrm-metadata-scroll">
				<section class="vrm-tech-list" aria-label={$_("unavatar_rights.summary")}>
					{#each stats as [label, value]}
						<div>
							<span>{label}</span>
							<strong>{value}</strong>
						</div>
					{/each}
				</section>
				<section class="vrm-license-block unavatar-rights-warning">
					<h3>{$_("unavatar_rights.important")}</h3>
					<p>{$_("unavatar_rights.body_1")}</p>
					<p>{$_("unavatar_rights.body_2")}</p>
					<p>{$_("unavatar_rights.body_3")}</p>
					<p>{$_("unavatar_rights.body_4")}</p>
				</section>
				<section class="vrm-license-block">
					<h3>{$_("unavatar_rights.confirm_heading")}</h3>
					<ul class="unavatar-rights-checks">
						<li>{$_("unavatar_rights.check_terms")}</li>
						<li>{$_("unavatar_rights.check_usage")}</li>
						<li>{$_("unavatar_rights.check_responsibility")}</li>
					</ul>
				</section>
			</div>
			<footer class="vrm-metadata-actions">
				{#if modal.pendingPath && selectedPreview}
					<div class="unavatar-icon-crop-controls">
						<label class="vrm-thumbnail-icon-toggle" title={$_("vrm_metadata.use_thumbnail_as_profile_icon_hint")}>
							<input type="checkbox" bind:checked={profileIconCrop.enabled} />
							<span>{$_("vrm_metadata.use_thumbnail_as_profile_icon")}</span>
						</label>
						{#if profileIconCrop.enabled}
							<label>
								<span>{$_("unavatar_rights.icon_zoom")}</span>
								<input type="range" min="1" max="4" step="0.05" bind:value={profileIconCrop.zoom} />
							</label>
							<label>
								<span>{$_("unavatar_rights.icon_x")}</span>
								<input type="range" min="-1" max="1" step="0.02" bind:value={profileIconCrop.offsetX} />
							</label>
							<label>
								<span>{$_("unavatar_rights.icon_y")}</span>
								<input type="range" min="-1" max="1" step="0.02" bind:value={profileIconCrop.offsetY} />
							</label>
						{/if}
					</div>
				{/if}
				<button class="secondary" onclick={() => onClose()}>{modal.pendingPath ? $_("common.cancel") : $_("common.close")}</button>
				{#if modal.pendingPath}
					<button class="primary" disabled={busy} onclick={() => onAcceptAndUse()}>{$_("unavatar_rights.accept_and_use")}</button>
				{/if}
			</footer>
		</div>
	</div>
</div>
