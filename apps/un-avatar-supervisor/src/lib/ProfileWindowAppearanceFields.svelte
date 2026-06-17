<script lang="ts">
	import { _ } from "svelte-i18n";
	import ColorField from "../ColorField.svelte";
	import ProfileToggleField from "./ProfileToggleField.svelte";
	import type { ColorModeChangeHandler } from "./profileColorActions";
	import type { ProfileSettingValue } from "./profileTypes";
	import type { ColorDisplayMode } from "./storageState";
	import { WINDOW_BACKGROUND_FALLBACK } from "./windowOptions";

	export let decorations = true;
	export let transparent = false;
	export let backgroundColor: [number, number, number];
	export let busy = false;
	export let colorDisplayMode: ColorDisplayMode;
	export let onColorModeChange: ColorModeChangeHandler;
	export let onBackgroundColorChange: (value: [number, number, number]) => void;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
</script>

<div class="window-setting-cluster window-appearance-cluster">
	<span class="window-cluster-title">{$_("profiles.editor.window_appearance")}</span>
	<ProfileToggleField
		label={$_("profiles.editor.borderless")}
		checked={!decorations}
		disabled={busy}
		onChange={(checked) => onUpdateSettingValue("window.decorations", !checked)}
	/>
	<ProfileToggleField
		label={$_("profiles.editor.transparent")}
		hint={$_("profiles.editor.transparent_hint")}
		checked={transparent}
		disabled={busy}
		onChange={(checked) => onUpdateSettingValue("window.transparent", checked)}
	/>
	<ColorField
		className="window-background-field"
		label={$_("profiles.editor.background")}
		value={backgroundColor}
		fallback={WINDOW_BACKGROUND_FALLBACK}
		hint={$_("profiles.hints.window.background")}
		disabled={busy}
		mode={colorDisplayMode}
		onModeChange={onColorModeChange}
		onChange={onBackgroundColorChange}
	/>
</div>
