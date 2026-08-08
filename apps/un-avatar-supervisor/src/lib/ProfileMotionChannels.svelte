<script lang="ts">
	import { Plus, Trash2 } from "lucide-svelte";
	import { _ } from "svelte-i18n";
	import { UNMOTION_CHANNEL_CONFIG, VMC_CHANNEL_CONFIG, vmcAddressValue } from "./motionOptions";
	import ProfileMotionTextChannel from "./ProfileMotionTextChannel.svelte";
	import type { MotionSetting, ProfileSettingValue, UnmotionZenohSubscriptionSetting } from "./profileTypes";

	export let setting: MotionSetting;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;

	const defaultSubscription = (): UnmotionZenohSubscriptionSetting => ({
		id: "un-motion/frame",
		lan_enabled: false,
		host: null,
		port: 47447,
	});

	function subscriptions(): UnmotionZenohSubscriptionSetting[] {
		const configured = setting.unmotion_zenoh_subscriptions ?? [];
		return configured.length > 0 ? configured : [defaultSubscription()];
	}

	function saveSubscriptions(next: UnmotionZenohSubscriptionSetting[]): void {
		void onUpdateSettingValue("motion.unmotion_zenoh.subscriptions", next as unknown as Record<string, unknown>[]);
	}

	function updateSubscription(index: number, patch: Partial<UnmotionZenohSubscriptionSetting>): void {
		saveSubscriptions(subscriptions().map((item, itemIndex) => (itemIndex === index ? { ...item, ...patch } : item)));
	}

	function addSubscription(): void {
		saveSubscriptions([...subscriptions(), defaultSubscription()]);
	}

	function removeSubscription(index: number): void {
		const current = subscriptions();
		if (current.length <= 1) return;
		saveSubscriptions(current.filter((_, itemIndex) => itemIndex !== index));
	}
</script>

<div class="subgroup unmfz-channel">
	<div class="unmfz-heading-row">
		<label class="profile-channel-heading" data-hint={$_(UNMOTION_CHANNEL_CONFIG.headingHintKey)}>
			<input
				type="checkbox"
				checked={setting.motion_unmotion_enabled}
				onchange={(event) =>
					onUpdateSettingValue(UNMOTION_CHANNEL_CONFIG.enabledField, (event.currentTarget as HTMLInputElement).checked)}
			/>
			<span>{$_(UNMOTION_CHANNEL_CONFIG.headingLabelKey)}</span>
		</label>
		<button
			type="button"
			class="icon-button unmfz-add"
			title={$_("profiles.editor.unmotion_zenoh_add")}
			aria-label={$_("profiles.editor.unmotion_zenoh_add")}
			disabled={!setting.motion_unmotion_enabled || busy || subscriptions().length >= 16}
			onclick={addSubscription}><Plus size={17} /></button
		>
	</div>

	<div class="unmfz-subscriptions">
		{#each subscriptions() as subscription, index}
			<div class="unmfz-subscription-row">
				<div class="unmfz-subscription-primary">
					<label>
						<span>{$_("profiles.editor.unmotion_zenoh_id")}</span>
						<input
							value={subscription.id}
							placeholder="un-motion/frame"
							disabled={!setting.motion_unmotion_enabled || busy}
							onchange={(event) => updateSubscription(index, { id: (event.currentTarget as HTMLInputElement).value })}
						/>
					</label>
					<button
						type="button"
						class="icon-button danger-subtle"
						title={$_("profiles.editor.unmotion_zenoh_remove")}
						aria-label={$_("profiles.editor.unmotion_zenoh_remove")}
						disabled={!setting.motion_unmotion_enabled || busy || subscriptions().length <= 1}
						onclick={() => removeSubscription(index)}><Trash2 size={16} /></button
					>
				</div>

				<label class="unmfz-lan-toggle">
					<input
						type="checkbox"
						checked={subscription.lan_enabled}
						disabled={!setting.motion_unmotion_enabled || busy}
						onchange={(event) => updateSubscription(index, { lan_enabled: (event.currentTarget as HTMLInputElement).checked })}
					/>
					<span>
						<strong>{$_("profiles.editor.unmotion_zenoh_lan")}</strong>
						<small>{$_("profiles.hints.motion.unmotion_lan")}</small>
					</span>
				</label>

				{#if subscription.lan_enabled}
					<div class="unmfz-endpoint-fields">
						<label>
							<span>{$_("profiles.editor.unmotion_zenoh_host")}</span>
							<input
								value={subscription.host ?? ""}
								placeholder="192.168.1.20"
								disabled={!setting.motion_unmotion_enabled || busy}
								onchange={(event) => updateSubscription(index, { host: (event.currentTarget as HTMLInputElement).value })}
							/>
						</label>
						<label>
							<span>{$_("profiles.editor.unmotion_zenoh_port")}</span>
							<input
								type="number"
								min="1"
								max="65535"
								value={subscription.port}
								disabled={!setting.motion_unmotion_enabled || busy}
								onchange={(event) =>
									updateSubscription(index, { port: Number((event.currentTarget as HTMLInputElement).value) })}
							/>
						</label>
					</div>
				{/if}
			</div>
		{/each}
	</div>
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
