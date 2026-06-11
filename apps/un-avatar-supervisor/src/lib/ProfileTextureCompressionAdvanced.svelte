<script lang="ts">
	import { _ } from "svelte-i18n";
	import type { ProfileSettingValue, TextureCompressionAdvanced } from "./profileTypes";
	import { TEXTURE_COMPRESSION_PREF_OPTIONS, TEXTURE_COMPRESSION_ROLES } from "./qualityOptions";
	import ProfileSelectField from "./ProfileSelectField.svelte";

	export let value: TextureCompressionAdvanced;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
</script>

<fieldset class="advanced-compression">
	<legend data-hint={$_("profiles.hints.quality.advanced_compression")}>{$_("profiles.editor.advanced_compression")}</legend>
	<div class="advanced-compression-grid">
		{#each TEXTURE_COMPRESSION_ROLES as role}
			<ProfileSelectField
				className="advanced-compression-row"
				label={role.label}
				hint={role.hint}
				value={value[role.key]}
				disabled={busy}
				options={TEXTURE_COMPRESSION_PREF_OPTIONS}
				onChange={(next) => onUpdateSettingValue(`render_quality.texture_compression_advanced.${role.key}`, next)}
			/>
		{/each}
	</div>
</fieldset>
