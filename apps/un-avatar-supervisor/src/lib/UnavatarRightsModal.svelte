<script lang="ts">
	import { _ } from "svelte-i18n";
	import type { UnavatarMetadataDialogState } from "./unavatarMetadata";

	export let modal: UnavatarMetadataDialogState;
	export let busy = false;
	export let onClose: () => void | Promise<void>;
	export let onAcceptAndUse: () => void | Promise<void>;

	$: metadata = modal.metadata;
	$: title = metadata.name?.trim() || metadata.file_name;
	$: stats = [
		[$_("unavatar_rights.stats.wardrobe"), metadata.wardrobe_set_count],
		[$_("unavatar_rights.stats.dynamics"), metadata.dynamics_count],
		[$_("unavatar_rights.stats.contacts"), metadata.contact_count],
		[$_("unavatar_rights.stats.modular_avatar"), metadata.modular_avatar_component_count],
	];
</script>

<div class="vrm-metadata-backdrop" role="presentation">
	<div class="vrm-metadata-dialog unavatar-rights-dialog" role="dialog" aria-modal="true" aria-label={$_("unavatar_rights.title")}>
		<div class="vrm-metadata-portrait" aria-hidden="true">
			{#if metadata.sample_screenshot_data_url}
				<div class="vrm-metadata-preview-frame">
					<img class="vrm-metadata-thumbnail" src={metadata.sample_screenshot_data_url} alt="" />
				</div>
			{:else}
				<div class="vrm-metadata-sigil">UN</div>
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
				{/if}
			</footer>
		</div>
	</div>
</div>
