<script lang="ts">
	import { _ } from "svelte-i18n";
	import ProfileSelectField from "./ProfileSelectField.svelte";
	import ProfileToggleField from "./ProfileToggleField.svelte";
	import type { ProfileSettingValue, QualitySetting } from "./profileTypes";
	import { BCN_CPU_THREAD_SELECT_OPTIONS, TEXTURE_BASE_SELECT_FIELDS, TEXTURE_COMPRESSION_SELECT_FIELDS } from "./qualityOptions";

	export let setting: Pick<
		QualitySetting,
		| "texture_resolution_limit"
		| "texture_compression"
		| "mipmap_filter"
		| "block_compression_encoder"
		| "block_compression_cpu_threads"
		| "processed_texture_cache"
		| "skin_tone_matching"
	>;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;

	function updateCpuThreadCount(value: string): void | Promise<void> {
		return onUpdateSettingValue("render_quality.block_compression_cpu_threads", Number(value));
	}
</script>

<fieldset class="quality-fieldset">
	<legend>{$_("profiles.editor.texture_sampling")}</legend>
	{#each TEXTURE_BASE_SELECT_FIELDS as field}
		<ProfileSelectField
			label={$_(field.labelKey)}
			hint={$_(field.hintKey)}
			value={setting[field.key]}
			disabled={busy}
			options={field.options}
			onChange={(value) => onUpdateSettingValue(field.field, value)}
		/>
	{/each}
</fieldset>

<fieldset class="quality-fieldset">
	<legend>{$_("profiles.editor.texture_compression_cache")}</legend>
	{#each TEXTURE_COMPRESSION_SELECT_FIELDS as field}
		<ProfileSelectField
			label={$_(field.labelKey)}
			hint={$_(field.hintKey)}
			value={setting[field.key]}
			disabled={busy}
			options={field.options}
			onChange={(value) => onUpdateSettingValue(field.field, value)}
		/>
	{/each}
	<ProfileSelectField
		label={$_("profiles.editor.bcn_cpu_threads")}
		hint={$_("profiles.hints.quality.bcn_cpu_threads")}
		value={String(setting.block_compression_cpu_threads)}
		disabled={busy}
		options={BCN_CPU_THREAD_SELECT_OPTIONS}
		onChange={updateCpuThreadCount}
	/>
	<ProfileToggleField
		label={$_("profiles.editor.processed_cache")}
		hint={$_("profiles.editor.processed_cache_hint")}
		checked={setting.processed_texture_cache}
		disabled={busy}
		onChange={(checked) => onUpdateSettingValue("render_quality.processed_texture_cache", checked)}
	/>
</fieldset>

<ProfileToggleField
	label={$_("profiles.editor.skin_tone_matching")}
	hint={$_("profiles.editor.skin_tone_matching_hint")}
	checked={setting.skin_tone_matching}
	disabled={busy}
	onChange={(checked) => onUpdateSettingValue("render_quality.skin_tone_matching", checked)}
/>
