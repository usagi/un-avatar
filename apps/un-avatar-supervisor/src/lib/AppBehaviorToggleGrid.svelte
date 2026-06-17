<script lang="ts">
	import { _ } from "svelte-i18n";
	import { APP_BEHAVIOR_TOGGLES } from "./appBehaviorOptions";
	import type { AppSettings } from "./appSettings";
	import type { AppSettingValue } from "./appTypes";
	import ProfileToggleField from "./ProfileToggleField.svelte";

	export let appSettings: AppSettings;
	export let onSetAppSetting: (key: keyof AppSettings, value: AppSettingValue) => void;
</script>

<div class="settings-toggle-grid">
	{#each APP_BEHAVIOR_TOGGLES as toggle}
		<ProfileToggleField
			label={$_(toggle.labelKey)}
			hint={$_(toggle.hintKey)}
			checked={Boolean(appSettings[toggle.key])}
			disabled={toggle.requiresSystemTray === true && !appSettings.system_tray_enabled}
			onChange={(checked) => onSetAppSetting(toggle.key, checked)}
		/>
	{/each}
</div>
