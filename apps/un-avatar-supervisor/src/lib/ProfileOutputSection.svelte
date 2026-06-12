<script lang="ts">
	import type {
		OutputModePreset,
		PreviewWindowPreset,
		ProfileSettingValue,
		SpoutOutputSetting,
		SpoutResolutionPreset,
	} from "./profileTypes";
	import { _ } from "svelte-i18n";
	import ProfileSpoutOutputFields from "./ProfileSpoutOutputFields.svelte";

	export let setting: SpoutOutputSetting;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
	export let onApplySpoutResolutionPreset: (preset: SpoutResolutionPreset) => void | Promise<void>;
	export let onApplyOutputModePreset: (preset: OutputModePreset) => void | Promise<void>;
	export let onApplyPreviewWindowPreset: (preset: PreviewWindowPreset) => void | Promise<void>;
	export let onActivate: () => void;
</script>

<section
	class="editor-section profile-output-section"
	data-profile-section="output"
	onfocusin={onActivate}
	data-hint={$_("profiles.hints.output.section")}
>
	<div class="section-title-row">
		<h3>{$_("profiles.editor.output")}</h3>
		<span class="setting-scope">{$_("profiles.editor.launch_time")}</span>
	</div>

	<div class="subgroup output-mode-fields">
		<div class="subgroup-heading">
			<span>{$_("profiles.editor.output_mode")}</span>
			<small>{$_("profiles.editor.output_mode_summary")}</small>
		</div>
		<div class="recommendation-row output-mode-row">
			<button
				type="button"
				class:active={!setting.spout_enabled && !setting.minimized}
				disabled={busy}
				data-hint={$_("profiles.hints.output.mode_window_preview")}
				onclick={() => onApplyOutputModePreset("window_preview")}>{$_("profiles.editor.output_mode_window")}</button
			>
			<button
				type="button"
				class:active={setting.spout_enabled && !setting.minimized}
				disabled={busy}
				data-hint={$_("profiles.hints.output.mode_spout_preview")}
				onclick={() => onApplyOutputModePreset("spout2_preview")}>{$_("profiles.editor.output_mode_spout_preview")}</button
			>
			<button
				type="button"
				class:active={setting.spout_enabled && setting.minimized}
				disabled={busy}
				data-hint={$_("profiles.hints.output.mode_spout_only")}
				onclick={() => onApplyOutputModePreset("spout2_only")}>{$_("profiles.editor.output_mode_spout_only")}</button
			>
		</div>
	</div>

	<ProfileSpoutOutputFields {setting} {busy} {onUpdateSettingValue} {onApplySpoutResolutionPreset} />

	<div class="subgroup preview-window-fields">
		<div class="subgroup-heading">
			<span>{$_("profiles.editor.preview_window")}</span>
			<small>{$_("profiles.editor.preview_window_summary")}</small>
		</div>
		<div class="recommendation-row preview-window-row">
			<button
				type="button"
				disabled={busy}
				data-hint={$_("profiles.hints.output.preview_compact")}
				onclick={() => onApplyPreviewWindowPreset("compact")}>640 x 360</button
			>
			<button
				type="button"
				disabled={busy}
				data-hint={$_("profiles.hints.output.preview_half_hd")}
				onclick={() => onApplyPreviewWindowPreset("half_hd")}>960 x 540</button
			>
			<button
				type="button"
				disabled={busy}
				data-hint={$_("profiles.hints.output.preview_hd")}
				onclick={() => onApplyPreviewWindowPreset("hd")}>1280 x 720</button
			>
			<span class="effect-panel-status">{setting.window_width} x {setting.window_height}</span>
		</div>
	</div>
</section>
