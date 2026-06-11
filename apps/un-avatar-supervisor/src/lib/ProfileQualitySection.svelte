<script lang="ts">
	import type { ProfileSettingValue, QualitySetting, RenderQualityRecommendation } from "./profileTypes";
	import ProfileDebugToggles from "./ProfileDebugToggles.svelte";
	import ProfileQualityBasicFields from "./ProfileQualityBasicFields.svelte";
	import ProfileQualityRecommendationRow from "./ProfileQualityRecommendationRow.svelte";
	import ProfileQualityTextureFields from "./ProfileQualityTextureFields.svelte";
	import { _ } from "svelte-i18n";

	export let setting: QualitySetting;
	export let busy = false;
	export let showDeveloperControls = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
	export let onApplyRenderQualityRecommendation: (recommendation: RenderQualityRecommendation) => void | Promise<void>;
	export let onActivate: () => void;
</script>

<section
	class="editor-section section-grid render-quality-section"
	data-profile-section="quality"
	onfocusin={onActivate}
	data-hint={$_("profiles.hints.quality.section")}
>
	<div class="section-title-row">
		<h3>{$_("profiles.editor.render_quality")}</h3>
		<span class="setting-scope">{$_("profiles.editor.launch_time")}</span>
		<ProfileQualityRecommendationRow {busy} {onApplyRenderQualityRecommendation} />
	</div>
	<ProfileQualityBasicFields {setting} {busy} {onUpdateSettingValue} />
	<ProfileQualityTextureFields {setting} {busy} {onUpdateSettingValue} />
	{#if showDeveloperControls}
		<ProfileDebugToggles {setting} {busy} {onUpdateSettingValue} />
	{/if}
</section>
