<script lang="ts">
	import { _ } from "svelte-i18n";
	import ProfileRangeNumberField from "./ProfileRangeNumberField.svelte";
	import type { ProfileSettingValue } from "./profileTypes";
	import type { ProfileSurfaceSetting } from "./profileLookTypes";

	export let setting: Pick<ProfileSurfaceSetting, "matcap_scale" | "ambient_occlusion_strength">;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
</script>

<ProfileRangeNumberField
	label={$_("profiles.editor.look_matcap")}
	hint={$_("profiles.hints.look.matcap")}
	value={setting.matcap_scale}
	rangeMin={0}
	rangeMax={2}
	step={0.05}
	disabled={busy}
	onChange={(value) => onUpdateSettingValue("effects.avatar.matcap.scale", value)}
/>
<ProfileRangeNumberField
	label={$_("profiles.editor.look_ambient_occlusion")}
	hint={$_("profiles.hints.look.ambient_occlusion")}
	value={setting.ambient_occlusion_strength}
	rangeMin={0}
	rangeMax={2}
	step={0.05}
	disabled={busy}
	onChange={(value) => onUpdateSettingValue("effects.avatar.ambient_occlusion.strength", value)}
/>
