<script lang="ts">
	import { _ } from "svelte-i18n";
	import { PinOff } from "lucide-svelte";
	import type { AppSettings } from "./appSettings";

	export let appSettings: AppSettings;
	export let busy = false;
	export let onClearTaskbarProfilePins: () => void | Promise<void>;
</script>

<section class="settings-card" data-hint={$_("settings.hints.taskbar_launcher")}>
	<header class="settings-card-header">
		<h2>{$_("settings.taskbar_launcher.title")}</h2>
	</header>
	<div class="setting-row" data-hint={$_("settings.hints.taskbar_pinned_profiles")}>
		<span>{$_("settings.taskbar_launcher.pinned_profiles")}</span>
		<div class="setting-inline-actions">
			<strong>{appSettings.pinned_taskbar_profile_ids.length}</strong>
			<button disabled={busy || appSettings.pinned_taskbar_profile_ids.length === 0} onclick={() => onClearTaskbarProfilePins()}
				><PinOff size={14} />{$_("settings.taskbar_launcher.clear_pinned_profiles")}</button
			>
		</div>
	</div>
</section>
