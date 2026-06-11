<script lang="ts">
	import type { ProfileSettingValue } from "./profileTypes";
	import { _ } from "svelte-i18n";
	import { formatFixed } from "./formatting";
	import ProfileLookOutlineFields from "./ProfileLookOutlineFields.svelte";
	import type { ColorModeChangeHandler } from "./profileColorActions";
	import type { ProfileOutlineSetting } from "./profileLookTypes";
	import type { ColorDisplayMode } from "./storageState";

	export let setting: ProfileOutlineSetting;
	export let busy = false;
	export let colorDisplayMode: ColorDisplayMode;
	export let onColorModeChange: ColorModeChangeHandler;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
</script>

<details class="effect-panel" open>
	<summary>
		<span>{$_("profiles.editor.look_outline")}</span>
		<small>{$_("profiles.editor.look_outline_summary")}</small>
		<span class="effect-panel-status">{setting.outline_policy} · {formatFixed((setting.outline_width ?? 0.003) * 1000)}mm</span>
	</summary>
	<ProfileLookOutlineFields {setting} {busy} {colorDisplayMode} {onColorModeChange} {onUpdateSettingValue} />
</details>
