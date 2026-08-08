<script lang="ts">
	import { tick } from "svelte";
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

	function subscriptionsFromSetting(): UnmotionZenohSubscriptionSetting[] {
		const configured = (setting.unmotion_zenoh_subscriptions ?? []) as UnmotionZenohSubscriptionSetting[];
		return configured.length > 0 ? configured.map((item) => ({ ...item })) : [defaultSubscription()];
	}

	let sourceSubscriptions = setting.unmotion_zenoh_subscriptions;
	let draftSubscriptions = subscriptionsFromSetting();
	let queuedSubscriptions: UnmotionZenohSubscriptionSetting[] | null = null;
	let saveRunning = false;

	$: if (setting.unmotion_zenoh_subscriptions !== sourceSubscriptions && !saveRunning && queuedSubscriptions === null) {
		sourceSubscriptions = setting.unmotion_zenoh_subscriptions;
		draftSubscriptions = subscriptionsFromSetting();
	}

	function saveSubscriptions(next: UnmotionZenohSubscriptionSetting[]): void {
		draftSubscriptions = next;
		queuedSubscriptions = next.map((item) => ({ ...item }));
		void flushSubscriptions();
	}

	async function flushSubscriptions(): Promise<void> {
		if (saveRunning) return;
		saveRunning = true;
		try {
			while (queuedSubscriptions !== null) {
				const next = queuedSubscriptions;
				queuedSubscriptions = null;
				await onUpdateSettingValue("motion.unmotion_zenoh.subscriptions", next as unknown as Record<string, unknown>[]);
			}
		} finally {
			saveRunning = false;
			await tick();
			sourceSubscriptions = setting.unmotion_zenoh_subscriptions;
			draftSubscriptions = subscriptionsFromSetting();
		}
	}

	function updateSubscription(index: number, patch: Partial<UnmotionZenohSubscriptionSetting>): void {
		saveSubscriptions(draftSubscriptions.map((item, itemIndex) => (itemIndex === index ? { ...item, ...patch } : item)));
	}

	function addSubscription(): void {
		saveSubscriptions([...draftSubscriptions, defaultSubscription()]);
	}

	function removeSubscription(index: number): void {
		if (draftSubscriptions.length <= 1) return;
		saveSubscriptions(draftSubscriptions.filter((_, itemIndex) => itemIndex !== index));
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
			disabled={!setting.motion_unmotion_enabled || busy || draftSubscriptions.length >= 16}
			onclick={addSubscription}><Plus size={17} /></button
		>
	</div>

	<div class="unmfz-subscriptions">
		{#each draftSubscriptions as subscription, index (subscription)}
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
					<span class="unmfz-remove-slot">
						{#if draftSubscriptions.length > 1}
							<button
								type="button"
								class="icon-button danger-subtle"
								title={$_("profiles.editor.unmotion_zenoh_remove")}
								aria-label={$_("profiles.editor.unmotion_zenoh_remove")}
								disabled={!setting.motion_unmotion_enabled || busy}
								onclick={() => removeSubscription(index)}><Trash2 size={16} /></button
							>
						{/if}
					</span>
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
