<script lang="ts">
	import { _ } from "svelte-i18n";
	import { defaultAppSettings, type AppSettings, type ThemeMode } from "./appSettings";
	import type { AppSettingValue, NativeNotificationStatus } from "./appTypes";
	import AppBehaviorToggleGrid from "./AppBehaviorToggleGrid.svelte";
	import ThemeModeSwitch from "./ThemeModeSwitch.svelte";

	export let appSettings: AppSettings;
	export let nativeNotificationStatus: NativeNotificationStatus | null;
	export let busy = false;
	export let onSetThemeMode: (mode: ThemeMode) => void;
	export let onSetAppSetting: (key: keyof AppSettings, value: AppSettingValue) => void;
	export let onSendTestNativeNotification: () => void | Promise<void>;
</script>

<section class="settings-card settings-card--wide" data-hint={$_("settings.hints.app_behavior")}>
	<header class="settings-card-header">
		<h2>{$_("settings.app_behavior.title")}</h2>
	</header>
	<div class="settings-form-grid">
		<div class="setting-row setting-row--wide-control" data-hint={$_("settings.hints.theme")}>
			<span>{$_("settings.theme.label")}</span>
			<ThemeModeSwitch mode={appSettings.theme_mode} onChange={onSetThemeMode} />
		</div>
		<AppBehaviorToggleGrid {appSettings} {onSetAppSetting} />
		<div class="setting-row" data-hint={$_("settings.hints.renderer_close_key")}>
			<span>{$_("settings.app_behavior.renderer_close_label")}</span>
			<input
				type="text"
				value={appSettings.renderer_close_hotkey}
				placeholder="Escape"
				autocomplete="off"
				spellcheck="false"
				onchange={(event) =>
					onSetAppSetting(
						"renderer_close_hotkey",
						(event.currentTarget as HTMLInputElement).value.trim() || defaultAppSettings.renderer_close_hotkey
					)}
			/>
		</div>
		<div class="setting-row" data-hint={$_("settings.hints.notifications")}>
			<span>{$_("settings.app_behavior.notifications_label")}</span>
			<div class="setting-inline-actions">
				<strong>{nativeNotificationStatus?.permission_state ?? $_("settings.app_behavior.permission_unknown")}</strong>
				<button disabled={busy} onclick={() => onSendTestNativeNotification()}
					>{$_("settings.app_behavior.notifications_test")}</button
				>
			</div>
		</div>
	</div>
</section>
