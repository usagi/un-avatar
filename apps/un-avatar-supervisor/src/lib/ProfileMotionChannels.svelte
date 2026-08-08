<script lang="ts">
	import { _ } from "svelte-i18n";
	import { UNMOTION_CHANNEL_CONFIG, VMC_CHANNEL_CONFIG, vmcAddressValue } from "./motionOptions";
	import ProfileMotionTextChannel from "./ProfileMotionTextChannel.svelte";
	import type { MotionSetting, ProfileSettingValue } from "./profileTypes";

	export let setting: MotionSetting;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;

	let observedSetting = setting;
	let directModeSelected = Boolean(setting.unmotion_zenoh_connect);

	$: if (setting !== observedSetting) {
		observedSetting = setting;
		directModeSelected = Boolean(setting.unmotion_zenoh_connect);
	}

	function useAutomaticConnection(): void {
		directModeSelected = false;
		void onUpdateSettingValue("motion.unmotion_zenoh.connect", null);
	}

	function useDirectConnection(): void {
		directModeSelected = true;
	}
</script>

<div class="subgroup unmfz-channel">
	<label class="profile-channel-heading" data-hint={$_(UNMOTION_CHANNEL_CONFIG.headingHintKey)}>
		<input
			type="checkbox"
			checked={setting.motion_unmotion_enabled}
			onchange={(event) =>
				onUpdateSettingValue(UNMOTION_CHANNEL_CONFIG.enabledField, (event.currentTarget as HTMLInputElement).checked)}
		/>
		<span>{$_(UNMOTION_CHANNEL_CONFIG.headingLabelKey)}</span>
	</label>
	<div class="unmfz-connection-controls">
		<span class="field-label">{$_("profiles.editor.unmotion_zenoh_connection")}</span>
		<div class="segmented-control unmfz-connection-mode" aria-label={$_("profiles.editor.unmotion_zenoh_connection")}>
			<button
				type="button"
				class:active={!directModeSelected}
				disabled={!setting.motion_unmotion_enabled || busy}
				aria-pressed={!directModeSelected}
				onclick={useAutomaticConnection}>{$_("profiles.editor.unmotion_zenoh_auto")}</button
			>
			<button
				type="button"
				class:active={directModeSelected}
				disabled={!setting.motion_unmotion_enabled || busy}
				aria-pressed={directModeSelected}
				onclick={useDirectConnection}>{$_("profiles.editor.unmotion_zenoh_address")}</button
			>
		</div>
		<small>{$_(directModeSelected ? "profiles.hints.motion.unmotion_address" : "profiles.hints.motion.unmotion_auto")}</small>
	</div>
	{#if directModeSelected}
		<label class="unmfz-connect-field" data-hint={$_("profiles.hints.motion.unmotion_connect")}>
			<span>{$_("profiles.editor.unmotion_zenoh_connect")}</span>
			<input
				value={setting.unmotion_zenoh_connect ?? ""}
				placeholder="192.168.1.20:39542"
				disabled={!setting.motion_unmotion_enabled || busy}
				onchange={(event) => onUpdateSettingValue("motion.unmotion_zenoh.connect", (event.currentTarget as HTMLInputElement).value)}
			/>
		</label>
	{/if}
	<details class="unmfz-advanced">
		<summary>{$_("profiles.editor.advanced_settings")}</summary>
		<div class="section-grid profile-channel-fields">
			<label data-hint={$_(UNMOTION_CHANNEL_CONFIG.fieldHintKey)}>
				<span>{$_(UNMOTION_CHANNEL_CONFIG.fieldLabelKey)}</span>
				<input
					value={setting.unmotion_zenoh_key ?? ""}
					placeholder={UNMOTION_CHANNEL_CONFIG.placeholder}
					disabled={!setting.motion_unmotion_enabled || busy}
					onchange={(event) =>
						onUpdateSettingValue(UNMOTION_CHANNEL_CONFIG.valueField, (event.currentTarget as HTMLInputElement).value)}
				/>
			</label>
		</div>
	</details>
</div>

<ProfileMotionTextChannel
	enabled={setting.motion_vmc_enabled}
	value={vmcAddressValue(setting.vmc_address, setting.vmc_port)}
	{busy}
	headingLabel={$_(VMC_CHANNEL_CONFIG.headingLabelKey)}
	headingHint={$_(VMC_CHANNEL_CONFIG.headingHintKey)}
	fieldLabel={$_(VMC_CHANNEL_CONFIG.fieldLabelKey)}
	fieldHint={$_(VMC_CHANNEL_CONFIG.fieldHintKey)}
	enabledField={VMC_CHANNEL_CONFIG.enabledField}
	valueField={VMC_CHANNEL_CONFIG.valueField}
	{onUpdateSettingValue}
/>
