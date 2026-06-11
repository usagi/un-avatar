<script lang="ts">
	import { _ } from "svelte-i18n";
	import AboutSettingsCard from "./AboutSettingsCard.svelte";
	import AppBehaviorSettingsCard from "./AppBehaviorSettingsCard.svelte";
	import ConsoleWindowSettingsCard from "./ConsoleWindowSettingsCard.svelte";
	import LanguageSettingsCard from "./LanguageSettingsCard.svelte";
	import type { AppSettings, ThemeMode } from "./appSettings";
	import type { AppSettingValue, NativeNotificationStatus } from "./appTypes";

	export let appSettings: AppSettings;
	export let availableLocales: string[];
	export let appVersion: string;
	export let nativeNotificationStatus: NativeNotificationStatus | null;
	export let busy = false;
	export let settingsHint: string;
	export let defaultSettingsHint: string;
	export let onSettingsHintEvent: (event: Event) => void;
	export let onClearSettingsHint: () => void;
	export let onSetThemeMode: (mode: ThemeMode) => void;
	export let onSetAppSetting: (key: keyof AppSettings, value: AppSettingValue) => void;
	export let onSetLocale: (locale: string) => void | Promise<void>;
	export let onSendTestNativeNotification: () => void | Promise<void>;
	export let onOpenExternalLink: (url: string) => void | Promise<void>;
</script>

<section
	class="view app-settings-view"
	aria-label="Settings"
	onpointerover={onSettingsHintEvent}
	onfocusin={onSettingsHintEvent}
	onpointerleave={onClearSettingsHint}
>
	<div class="settings-scroll">
		<AppBehaviorSettingsCard
			{appSettings}
			{nativeNotificationStatus}
			{busy}
			{onSetThemeMode}
			{onSetAppSetting}
			{onSendTestNativeNotification}
		/>

		<LanguageSettingsCard {appSettings} {availableLocales} {onSetLocale} />

		<ConsoleWindowSettingsCard {appSettings} {onSetAppSetting} />

		<AboutSettingsCard {appVersion} {onOpenExternalLink} />
	</div>
	<div class="profile-hint-bar settings-hint-bar" aria-live="polite">
		<span>{settingsHint || defaultSettingsHint}</span>
	</div>
</section>
