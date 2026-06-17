<script lang="ts">
	import { _ } from "svelte-i18n";
	import { Copy, Download, FolderOpen } from "lucide-svelte";

	export let disabled = false;
	export let rendererLogsCopyFlash = false;
	export let onCopyAllRendererLogs: () => void;
	export let onSaveAllRendererLogs: () => void;
	export let onRevealSupervisorLogsDir: () => void;
</script>

<div class="logs-toolbar-actions">
	<button class="ghost-button" onclick={onCopyAllRendererLogs} {disabled} title={$_("logs.toolbar.copy_title")}>
		<Copy size={14} />
		{rendererLogsCopyFlash ? $_("logs.toolbar.copied") : $_("logs.toolbar.copy_all")}
	</button>
	<button class="ghost-button" onclick={onSaveAllRendererLogs} {disabled} title={$_("logs.toolbar.save_title")}>
		<Download size={14} />
		{$_("logs.toolbar.save_txt")}
	</button>
	<button class="ghost-button" onclick={onRevealSupervisorLogsDir} title={$_("logs.toolbar.open_folder_title")}>
		<FolderOpen size={14} />
		{$_("logs.toolbar.open_folder")}
	</button>
</div>
