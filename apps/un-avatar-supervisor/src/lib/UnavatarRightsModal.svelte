<script lang="ts">
	import { _ } from "svelte-i18n";
	import type { UnavatarMetadataDialogState, UnavatarProfileIconCrop } from "./unavatarMetadata";

	export let modal: UnavatarMetadataDialogState;
	export let busy = false;
	export let profileIconCrop: UnavatarProfileIconCrop;
	export let onClose: () => void | Promise<void>;
	export let onAcceptAndUse: () => void | Promise<void>;
	export let onSaveProfileIcon: () => void | Promise<void>;

	$: metadata = modal.metadata;
	$: title = metadata.name?.trim() || metadata.file_name;
	let selectedSetId = "";
	let selectedPreviewIndex = 0;
	let cropFrame: HTMLDivElement | null = null;
	let dragStart: { pointerId: number; x: number; y: number; offsetX: number; offsetY: number } | null = null;
	let naturalPreviewSize: { dataUrl: string; width: number; height: number } | null = null;
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
			zoom: 1,
			offsetX: 0,
			offsetY: 0,
		};
	}
	$: stats = [
		[$_("unavatar_rights.stats.wardrobe"), metadata.wardrobe_set_count],
		[$_("unavatar_rights.stats.dynamics"), metadata.dynamics_count],
		[$_("unavatar_rights.stats.contacts"), metadata.contact_count],
		[$_("unavatar_rights.stats.modular_avatar"), metadata.modular_avatar_component_count],
	];
	$: cropLayout = previewCropLayout(
		selectedPreview?.width ?? (naturalPreviewSize?.dataUrl === selectedPreview?.data_url ? naturalPreviewSize.width : null),
		selectedPreview?.height ?? (naturalPreviewSize?.dataUrl === selectedPreview?.data_url ? naturalPreviewSize.height : null),
		Number(profileIconCrop.zoom) || 1,
		Number(profileIconCrop.offsetX) || 0,
		Number(profileIconCrop.offsetY) || 0
	);

	function clamp(value: number, min: number, max: number) {
		return Math.min(max, Math.max(min, value));
	}

	function previewCropLayout(width: number | null, height: number | null, zoomValue: number, offsetX: number, offsetY: number) {
		const safeWidth = width && width > 0 ? width : 1;
		const safeHeight = height && height > 0 ? height : 1;
		const aspect = safeWidth / safeHeight;
		const imageWidth = aspect >= 1 ? 100 : 100 * aspect;
		const imageHeight = aspect >= 1 ? 100 / aspect : 100;
		const imageLeft = (100 - imageWidth) * 0.5;
		const imageTop = (100 - imageHeight) * 0.5;
		const side = Math.min(imageWidth, imageHeight) / clamp(zoomValue, 1, 4);
		const travelX = Math.max(0, imageWidth - side);
		const travelY = Math.max(0, imageHeight - side);
		return {
			side,
			left: imageLeft + imageWidth * 0.5 + clamp(offsetX, -1, 1) * travelX * 0.5,
			top: imageTop + imageHeight * 0.5 + clamp(offsetY, -1, 1) * travelY * 0.5,
			travelX,
			travelY,
		};
	}

	function setCropEnabled(enabled: boolean) {
		profileIconCrop = { ...profileIconCrop, enabled };
	}

	function adjustZoom(delta: number) {
		profileIconCrop = {
			...profileIconCrop,
			zoom: clamp(Number(profileIconCrop.zoom) + delta, 1, 4),
		};
	}

	function resetCrop() {
		profileIconCrop = {
			...profileIconCrop,
			zoom: 1,
			offsetX: 0,
			offsetY: 0,
		};
	}

	function beginCropDrag(event: PointerEvent) {
		if (!profileIconCrop.enabled || !selectedPreview || !cropFrame) return;
		dragStart = {
			pointerId: event.pointerId,
			x: event.clientX,
			y: event.clientY,
			offsetX: Number(profileIconCrop.offsetX) || 0,
			offsetY: Number(profileIconCrop.offsetY) || 0,
		};
		cropFrame.setPointerCapture(event.pointerId);
		event.preventDefault();
	}

	function moveCropDrag(event: PointerEvent) {
		if (!dragStart || dragStart.pointerId !== event.pointerId || !cropFrame) return;
		const rect = cropFrame.getBoundingClientRect();
		const travelX = Math.max(1, rect.width * (cropLayout.travelX / 100) * 0.5);
		const travelY = Math.max(1, rect.height * (cropLayout.travelY / 100) * 0.5);
		profileIconCrop = {
			...profileIconCrop,
			offsetX: clamp(dragStart.offsetX + (event.clientX - dragStart.x) / travelX, -1, 1),
			offsetY: clamp(dragStart.offsetY + (event.clientY - dragStart.y) / travelY, -1, 1),
		};
	}

	function endCropDrag(event: PointerEvent) {
		if (!dragStart || dragStart.pointerId !== event.pointerId) return;
		cropFrame?.releasePointerCapture(event.pointerId);
		dragStart = null;
	}

	function updateNaturalPreviewSize(event: Event) {
		const image = event.currentTarget as HTMLImageElement;
		if (!selectedPreview || image.naturalWidth <= 0 || image.naturalHeight <= 0) return;
		naturalPreviewSize = {
			dataUrl: selectedPreview.data_url,
			width: image.naturalWidth,
			height: image.naturalHeight,
		};
	}
</script>

<div class="vrm-metadata-backdrop" role="presentation">
	<div class="vrm-metadata-dialog unavatar-rights-dialog" role="dialog" aria-modal="true" aria-label={$_("unavatar_rights.title")}>
		<div class="vrm-metadata-portrait">
			{#if selectedPreview}
				<div
					class="vrm-metadata-preview-frame unavatar-crop-frame"
					class:disabled={!profileIconCrop.enabled}
					bind:this={cropFrame}
					role="application"
					aria-label={$_("unavatar_rights.icon_drag_hint")}
					onpointerdown={beginCropDrag}
					onpointermove={moveCropDrag}
					onpointerup={endCropDrag}
					onpointercancel={endCropDrag}
				>
					<img
						class="vrm-metadata-thumbnail"
						src={selectedPreview.data_url}
						alt=""
						onload={updateNaturalPreviewSize}
					/>
					{#if selectedPreview && profileIconCrop.enabled}
						<span
							class="unavatar-icon-mask"
							style={`width: ${cropLayout.side}%; left: ${cropLayout.left}%; top: ${cropLayout.top}%;`}
						></span>
						<span class="unavatar-crop-hint">{$_("unavatar_rights.icon_drag_hint")}</span>
					{/if}
				</div>
			{:else}
				<div class="vrm-metadata-sigil">UN</div>
			{/if}
			{#if selectedPreview}
				<div class="unavatar-icon-crop-panel">
					<label class="unavatar-icon-crop-toggle" title={$_("unavatar_rights.use_preview_as_profile_icon")}>
						<input type="checkbox" checked={profileIconCrop.enabled} onchange={(event) => setCropEnabled(event.currentTarget.checked)} />
						<span>{$_("unavatar_rights.profile_icon")}</span>
					</label>
					<div class="unavatar-crop-zoom-actions" aria-label={$_("unavatar_rights.icon_zoom")}>
						<button type="button" disabled={!profileIconCrop.enabled} title={$_("unavatar_rights.icon_zoom_out")} onclick={() => adjustZoom(-0.15)}>&minus;</button>
						<button type="button" disabled={!profileIconCrop.enabled} title={$_("unavatar_rights.icon_reset")} onclick={resetCrop}>{$_("unavatar_rights.icon_reset")}</button>
						<button type="button" disabled={!profileIconCrop.enabled} title={$_("unavatar_rights.icon_zoom_in")} onclick={() => adjustZoom(0.15)}>+</button>
					</div>
				</div>
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
				<button class="secondary" onclick={() => onClose()}>{modal.pendingPath ? $_("common.cancel") : $_("common.close")}</button>
				{#if modal.pendingPath}
					<button class="primary" disabled={busy} onclick={() => onAcceptAndUse()}>{$_("unavatar_rights.accept_and_use")}</button>
				{:else if modal.iconSelectionOnly}
					<button class="primary" disabled={busy || !profileIconCrop.enabled || !profileIconCrop.imageDataUrl} onclick={() => onSaveProfileIcon()}>{$_("unavatar_rights.save_profile_icon")}</button>
				{/if}
			</footer>
		</div>
	</div>
</div>
