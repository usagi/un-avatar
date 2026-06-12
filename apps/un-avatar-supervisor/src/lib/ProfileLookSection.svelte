<script lang="ts">
	import type { LookRecommendation, ProfileSettingValue } from "./profileTypes";
	import { _ } from "svelte-i18n";
	import ProfileLookBloomPanel from "./ProfileLookBloomPanel.svelte";
	import ProfileLookColorPanel from "./ProfileLookColorPanel.svelte";
	import ProfileLookOutlinePanel from "./ProfileLookOutlinePanel.svelte";
	import ProfileLookRecommendationRow from "./ProfileLookRecommendationRow.svelte";
	import ProfileLookShadowsPanel from "./ProfileLookShadowsPanel.svelte";
	import type { ColorModeChangeHandler } from "./profileColorActions";
	import type { ProfileLookSetting } from "./profileLookTypes";
	import type { ColorDisplayMode } from "./storageState";

	export let setting: ProfileLookSetting;
	export let busy = false;
	export let colorDisplayMode: ColorDisplayMode;
	export let onColorModeChange: ColorModeChangeHandler;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
	export let onApplyLookRecommendation: (look: LookRecommendation) => void | Promise<void>;
	export let onActivate: () => void;
</script>

<section
	class="editor-section section-grid rendering-presentation-section"
	data-profile-section="look"
	onfocusin={onActivate}
	data-hint={$_("profiles.hints.look.section")}
>
	<div class="section-title-row">
		<h3>{$_("profiles.editor.rendering_presentation")}</h3>
		<ProfileLookRecommendationRow {busy} {onApplyLookRecommendation} />
	</div>
	<ProfileLookOutlinePanel {setting} {busy} {colorDisplayMode} {onColorModeChange} {onUpdateSettingValue} />
	<ProfileLookShadowsPanel {setting} {busy} {onUpdateSettingValue} />
	<ProfileLookColorPanel {setting} {busy} {onUpdateSettingValue} />
	<ProfileLookBloomPanel {setting} {busy} {onUpdateSettingValue} />
</section>
