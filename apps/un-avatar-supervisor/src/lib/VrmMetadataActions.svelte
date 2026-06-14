<script lang="ts">
	import { _ } from "svelte-i18n";

	export let pendingPath: string | null;
	export let busy = false;
	export let onClose: () => void | Promise<void>;
	export let onAcceptAndUse: () => void | Promise<void>;
</script>

<footer class="vrm-metadata-actions">
	<span class="metadata-action-mode">
		{pendingPath ? $_("vrm_metadata.review_before_use") : $_("vrm_metadata.title")}
	</span>
	<button class="secondary" onclick={() => onClose()}>
		{pendingPath ? $_("common.cancel") : $_("common.close")}
	</button>
	{#if pendingPath}
		<button class="primary" disabled={busy} onclick={() => onAcceptAndUse()}>
			{$_("vrm_metadata.accept_and_use")}
		</button>
	{/if}
</footer>
