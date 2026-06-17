<script lang="ts">
	import { _ } from "svelte-i18n";
	import ColorField from "../ColorField.svelte";
	import ProfileNumberInputField from "./ProfileNumberInputField.svelte";
	import ProfileRangeNumberField from "./ProfileRangeNumberField.svelte";
	import {
		OUTLINE_COLOR_FALLBACK,
		OUTLINE_NUMBER_FIELDS,
		OUTLINE_RANGE_FIELDS,
	} from "./lookOptions";
	import ProfileToggleField from "./ProfileToggleField.svelte";
	import type { ColorModeChangeHandler } from "./profileColorActions";
	import type { ProfileOutlineSetting } from "./profileLookTypes";
	import type { ProfileSettingValue } from "./profileTypes";
	import type { ColorDisplayMode } from "./storageState";

	export let setting: ProfileOutlineSetting;
	export let busy = false;
	export let colorDisplayMode: ColorDisplayMode;
	export let onColorModeChange: ColorModeChangeHandler;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;

	$: silhouetteEnabled = setting.outline_policy === "override";
</script>

<ProfileToggleField
	label={$_("profiles.editor.look_outline_policy")}
	hint={$_("profiles.hints.look.outline_policy")}
	checked={silhouetteEnabled}
	disabled={busy}
	onChange={(checked) => onUpdateSettingValue("effects.avatar.outline.policy", checked ? "override" : "off")}
/>
{#each OUTLINE_RANGE_FIELDS as field}
	<ProfileRangeNumberField
		label={$_(field.labelKey)}
		hint={$_(field.hintKey)}
		value={(setting[field.key] ?? field.fallback) * field.scale}
		rangeMin={field.rangeMin}
		rangeMax={field.rangeMax}
		step={field.step}
		decimals={field.decimals}
		disabled={busy || !silhouetteEnabled}
		onChange={(value) => onUpdateSettingValue(field.field, value / field.scale)}
	/>
{/each}
{#each OUTLINE_NUMBER_FIELDS as field}
	<ProfileNumberInputField
		label={$_(field.labelKey)}
		hint={$_(field.hintKey)}
		value={setting[field.key] ?? field.fallback}
		min={field.min}
		max={field.max}
		step={field.step}
		disabled={busy || !silhouetteEnabled}
		onChange={(value) => onUpdateSettingValue(field.field, value)}
	/>
{/each}
<ColorField
	label={$_("profiles.editor.look_outline_color")}
	value={setting.outline_color}
	fallback={OUTLINE_COLOR_FALLBACK}
	disabled={busy || !silhouetteEnabled}
	mode={colorDisplayMode}
	onModeChange={onColorModeChange}
	onChange={(color) => onUpdateSettingValue("effects.avatar.outline.color", color)}
/>
