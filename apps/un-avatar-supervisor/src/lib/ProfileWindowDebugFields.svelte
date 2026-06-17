<script lang="ts">
	import { _ } from "svelte-i18n";
	import ProfileToggleField from "./ProfileToggleField.svelte";
	import type { ProfileSettingValue } from "./profileTypes";

	export let showAxes = false;
	export let showBoneColliders = false;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
</script>

<div class="window-setting-cluster window-debug-cluster">
	<span class="window-cluster-title">{$_("profiles.editor.window_debug_display")}</span>
	<ProfileToggleField
		label={$_("profiles.editor.show_axes")}
		hint={$_("profiles.hints.window.show_axes")}
		checked={showAxes}
		disabled={busy}
		onChange={(checked) => onUpdateSettingValue("debug.show_axes", checked)}
	/>
	<ProfileToggleField
		label={$_("profiles.editor.show_bone_colliders")}
		hint={$_("profiles.editor.show_bone_colliders_hint")}
		checked={showBoneColliders}
		disabled={busy}
		onChange={(checked) => onUpdateSettingValue("debug.show_bone_colliders", checked)}
	/>
</div>
