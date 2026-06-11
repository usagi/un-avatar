<script lang="ts">
	import { _ } from "svelte-i18n";
	import { BLOOM_QUALITY_OPTIONS } from "./lookOptions";
	import ProfileRangeNumberField from "./ProfileRangeNumberField.svelte";
	import ProfileSelectField from "./ProfileSelectField.svelte";
	import ProfileToggleField from "./ProfileToggleField.svelte";
	import type { ProfileBloomSetting } from "./profileLookTypes";
	import type { ProfileSettingValue } from "./profileTypes";

	export let setting: ProfileBloomSetting;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
</script>

<ProfileToggleField
	label={$_("profiles.editor.look_bloom")}
	hint={$_("profiles.hints.look.bloom")}
	checked={setting.bloom_enabled}
	disabled={busy}
	onChange={(checked) => onUpdateSettingValue("effects.post.bloom.enabled", checked)}
/>
<ProfileSelectField
	label={$_("profiles.editor.look_bloom_quality")}
	hint={$_("profiles.hints.look.bloom_quality")}
	value={setting.bloom_quality}
	disabled={busy || !setting.bloom_enabled}
	options={BLOOM_QUALITY_OPTIONS}
	onChange={(value) => onUpdateSettingValue("effects.post.bloom.quality", value)}
/>
<ProfileRangeNumberField
	label={$_("profiles.editor.look_bloom_strength")}
	hint={$_("profiles.hints.look.bloom_strength")}
	value={setting.bloom_strength}
	rangeMin={0}
	rangeMax={2}
	step={0.05}
	disabled={busy || !setting.bloom_enabled}
	onChange={(value) => onUpdateSettingValue("effects.post.bloom.strength", value)}
/>
<ProfileRangeNumberField
	label={$_("profiles.editor.look_bloom_threshold")}
	hint={$_("profiles.hints.look.bloom_threshold")}
	value={setting.bloom_threshold}
	rangeMin={0}
	rangeMax={2}
	step={0.01}
	disabled={busy || !setting.bloom_enabled}
	onChange={(value) => onUpdateSettingValue("effects.post.bloom.threshold", value)}
/>
<ProfileRangeNumberField
	label={$_("profiles.editor.look_bloom_radius")}
	hint={$_("profiles.hints.look.bloom_radius")}
	value={setting.bloom_radius}
	rangeMin={0}
	rangeMax={32}
	step={1}
	disabled={busy || !setting.bloom_enabled}
	onChange={(value) => onUpdateSettingValue("effects.post.bloom.radius", value)}
/>
