<script lang="ts">
	import { _ } from "svelte-i18n";
	import type { AppSettings } from "./appSettings";

	export let appSettings: AppSettings;
	export let availableLocales: string[];
	export let onSetLocale: (locale: string) => void | Promise<void>;

	function localeLabel(locale: string): string {
		if (locale === "ja-JP") return "日本語 (ja-JP)";
		if (locale === "en-US") return "English (en-US)";
		return locale;
	}
</script>

<section class="settings-card" data-hint={$_("settings.hints.language")}>
	<header class="settings-card-header">
		<h2>{$_("settings.language.title")}</h2>
	</header>
	<div class="setting-row setting-row--wide-control">
		<span>{$_("language.label")}</span>
		<select value={appSettings.locale} onchange={(event) => onSetLocale((event.currentTarget as HTMLSelectElement).value)}>
			<option value="">{$_("language.system_option")}</option>
			{#each availableLocales as tag (tag)}
				<option value={tag}>{localeLabel(tag)}</option>
			{/each}
		</select>
	</div>
</section>
