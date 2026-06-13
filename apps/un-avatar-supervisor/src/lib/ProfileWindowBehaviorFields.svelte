<script lang="ts">
	import { _ } from "svelte-i18n";
	import ProfileToggleField from "./ProfileToggleField.svelte";
	import type { ProfileSettingValue } from "./profileTypes";

	export let transparent = false;
	export let inputPassthrough = false;
	export let alwaysOnTop = false;
	export let minimized = false;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
</script>

<div class="window-setting-cluster window-behavior-cluster">
	<span class="window-cluster-title">{$_("profiles.editor.window_behavior")}</span>
	<ProfileToggleField
		label={$_("profiles.editor.click_through")}
		checked={inputPassthrough}
		disabled={busy || !transparent}
		onChange={(checked) => onUpdateSettingValue("window.input_passthrough", checked)}
	/>
	<ProfileToggleField
		label={$_("profiles.editor.always_on_top")}
		checked={alwaysOnTop}
		disabled={busy}
		onChange={(checked) => onUpdateSettingValue("window.always_on_top", checked)}
	/>
	<ProfileToggleField
		label={$_("profiles.editor.start_minimized")}
		hint={$_("profiles.hints.window.start_minimized")}
		checked={minimized}
		disabled={busy}
		onChange={(checked) => onUpdateSettingValue("window.minimized", checked)}
	/>
</div>
