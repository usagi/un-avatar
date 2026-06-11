<script lang="ts">
	import { _ } from "svelte-i18n";
	import ProfileSpoutResolutionPresetRow from "./ProfileSpoutResolutionPresetRow.svelte";
	import ProfileSpoutSizeFields from "./ProfileSpoutSizeFields.svelte";
	import type { ProfileSettingValue, SpoutOutputSetting, SpoutResolutionPreset } from "./profileTypes";

	export let setting: SpoutOutputSetting;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
	export let onApplySpoutResolutionPreset: (preset: SpoutResolutionPreset) => void | Promise<void>;
</script>

<div class="subgroup">
	<label class="profile-channel-heading" data-hint={$_("profiles.hints.output.spout_sender")}>
		<input
			type="checkbox"
			checked={setting.spout_enabled}
			onchange={(event) => onUpdateSettingValue("output.spout2.enabled", (event.currentTarget as HTMLInputElement).checked)}
		/>
		<span>{$_("profiles.editor.spout2_sender")}</span>
	</label>
	<div class="section-grid profile-channel-fields">
		<label class="profile-field-full" data-hint={$_("profiles.hints.output.spout_name")}>
			<span>{$_("profiles.editor.spout2_name")}</span>
			<input
				value={setting.spout_name ?? ""}
				disabled={!setting.spout_enabled || busy}
				onchange={(event) => onUpdateSettingValue("output.spout2.name", (event.currentTarget as HTMLInputElement).value)}
			/>
		</label>
		<div class="spout-resolution-fields">
			<div class="subgroup-heading output-resolution-preset-row">
				<span>{$_("profiles.editor.spout2_resolution")}</span>
				<ProfileSpoutResolutionPresetRow disabled={!setting.spout_enabled || busy} {onApplySpoutResolutionPreset} />
			</div>
			<ProfileSpoutSizeFields {setting} disabled={!setting.spout_enabled || busy} {onUpdateSettingValue} />
		</div>
	</div>
</div>
