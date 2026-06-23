<script lang="ts">
	import { onMount } from "svelte";
	import { invoke } from "@tauri-apps/api/core";
	import type { GpuAdapterOption, IdentitySetting, ProfileSettingValue } from "./profileTypes";
	import { _ } from "svelte-i18n";
	import { FolderOpen } from "lucide-svelte";
	import { hasTauriRuntime } from "./environment";
	import ProfileIdentityIconField from "./ProfileIdentityIconField.svelte";
	import ProfileIdentityTextFields from "./ProfileIdentityTextFields.svelte";
	import ProfileSelectField from "./ProfileSelectField.svelte";
	import ProfileToggleField from "./ProfileToggleField.svelte";
	import { RENDER_BACKEND_OPTIONS } from "./qualityOptions";

	export let setting: IdentitySetting;
	export let iconUrl: string;
	export let busy = false;
	export let onBrowseIcon: () => void | Promise<void>;
	export let onApplyAvatarThumbnail: () => void | Promise<void>;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
	export let onActivate: () => void = () => {};

	let gpuAdapters: GpuAdapterOption[] = [];
	let gpuAdapterLoadFailed = false;

	$: savedGpuAdapter = setting.gpu_adapter?.trim() || "auto";
	$: listedGpuAdapterOptions = gpuAdapters.map((adapter) => [adapter.value, adapter.label] as const);
	$: gpuAdapterOptions = [
		["auto", $_("profiles.editor.gpu_adapter_auto")] as const,
		...(savedGpuAdapter !== "auto" && !listedGpuAdapterOptions.some(([value]) => value === savedGpuAdapter)
			? ([[savedGpuAdapter, `${savedGpuAdapter} (${$_("profiles.editor.gpu_adapter_missing")})`]] as const)
			: []),
		...listedGpuAdapterOptions,
	];
	$: gpuAdapterValue = gpuAdapterOptions.some(([value]) => value === savedGpuAdapter) ? savedGpuAdapter : "auto";

	onMount(async () => {
		if (!hasTauriRuntime()) return;
		try {
			gpuAdapters = await invoke<GpuAdapterOption[]>("list_gpu_adapters");
		} catch (error) {
			gpuAdapterLoadFailed = true;
			console.warn("list_gpu_adapters failed", error);
		}
	});
</script>

<section
	class="editor-section profile-section profile-identity-section"
	data-profile-section="identity"
	onfocusin={onActivate}
	data-hint={$_("profiles.hints.identity.section")}
>
	<div class="section-title-row">
		<h3>{$_("profiles.editor.profile_setting_heading")}</h3>
	</div>
	<div class="identity-row">
		<button class="icon-picker" disabled={busy} onclick={() => onBrowseIcon()}>
			<img src={iconUrl} alt="" />
			<span><FolderOpen size={13} /></span>
		</button>
		<label data-hint={$_("profiles.hints.identity.name")}
			>{$_("profiles.editor.name")}<input
				value={setting.name}
				onchange={(event) => onUpdateSettingValue("profile.display_name", (event.currentTarget as HTMLInputElement).value)}
			/></label
		>
	</div>
	<ProfileIdentityTextFields {setting} {onUpdateSettingValue} />
	<ProfileIdentityIconField {setting} {busy} {onBrowseIcon} {onApplyAvatarThumbnail} {onUpdateSettingValue} />
	<ProfileToggleField
		label={$_("profiles.editor.allow_multiple_renderers")}
		hint={$_("profiles.hints.identity.allow_multiple")}
		checked={setting.allow_multiple_renderers}
		onChange={(checked) => onUpdateSettingValue("profile.allow_multiple_renderers", checked)}
	/>
	<div class="identity-gpu-row">
		<ProfileSelectField
			label={$_("profiles.editor.gpu_adapter")}
			hint={gpuAdapterLoadFailed ? $_("profiles.hints.identity.gpu_adapter_unavailable") : $_("profiles.hints.identity.gpu_adapter")}
			value={gpuAdapterValue}
			disabled={busy}
			options={gpuAdapterOptions}
			onChange={(value) => onUpdateSettingValue("profile.gpu_adapter", value)}
		/>
		<ProfileSelectField
			label={$_("profiles.editor.render_backend")}
			hint={$_("profiles.hints.identity.render_backend")}
			value={setting.render_backend}
			disabled={busy}
			options={RENDER_BACKEND_OPTIONS}
			onChange={(value) => onUpdateSettingValue("render_quality.render_backend", value)}
		/>
	</div>
</section>
