<script lang="ts">
	import { _ } from "svelte-i18n";

	export let appVersion: string;
	export let onOpenExternalLink: (url: string) => void | Promise<void>;
</script>

<section class="settings-card" data-hint={$_("settings.hints.about")}>
	<header class="settings-card-header">
		<h2>{$_("settings.about.title")}</h2>
	</header>
	<dl class="about-grid">
		<dt>{$_("settings.about.app")}</dt>
		<dd>{$_("settings.about.app_value")}</dd>
		<dt>{$_("settings.about.version")}</dt>
		<dd>{appVersion || $_("app.version_unknown")}</dd>
		<dt>{$_("settings.about.repository")}</dt>
		<dd>
			<a
				href={$_("settings.about.repository_url")}
				target="_blank"
				rel="noreferrer"
				onclick={(event) => {
					event.preventDefault();
					void onOpenExternalLink($_("settings.about.repository_url"));
				}}>{$_("settings.about.repository_label")}</a
			>
		</dd>
		<dt>{$_("settings.about.license")}</dt>
		<dd>{$_("settings.about.license_value")}</dd>
	</dl>
</section>
