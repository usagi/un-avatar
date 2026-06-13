<script lang="ts">
	import type { ProfileSettingValue, WindowSetting } from "./profileTypes";
	import { _ } from "svelte-i18n";
	import ProfileWindowAppearanceFields from "./ProfileWindowAppearanceFields.svelte";
	import ProfileWindowBehaviorFields from "./ProfileWindowBehaviorFields.svelte";
	import ProfileWindowDebugFields from "./ProfileWindowDebugFields.svelte";
	import ProfileWindowGeometryFields from "./ProfileWindowGeometryFields.svelte";
	import type { ColorModeChangeHandler } from "./profileColorActions";
	import type { ColorDisplayMode } from "./storageState";

	export let setting: WindowSetting;
	export let busy = false;
	export let colorDisplayMode: ColorDisplayMode;
	export let onColorModeChange: ColorModeChangeHandler;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
	export let onBackgroundColorChange: (value: [number, number, number]) => void;
	export let onActivate: () => void;
</script>

<section
	class="editor-section section-grid profile-window-section"
	data-profile-section="window"
	onfocusin={onActivate}
	data-hint={$_("profiles.hints.window.section")}
>
	<div class="section-title-row">
		<h3>{$_("profiles.editor.window")}</h3>
	</div>
	<ProfileWindowAppearanceFields
		decorations={setting.decorations}
		transparent={setting.transparent}
		backgroundColor={setting.background_color}
		{busy}
		{colorDisplayMode}
		{onColorModeChange}
		{onBackgroundColorChange}
		{onUpdateSettingValue}
	/>
	<ProfileWindowBehaviorFields
		transparent={setting.transparent}
		inputPassthrough={setting.input_passthrough}
		alwaysOnTop={setting.always_on_top}
		minimized={setting.minimized}
		{busy}
		{onUpdateSettingValue}
	/>
	<ProfileWindowDebugFields showAxes={setting.show_axes} showBoneColliders={setting.show_bone_colliders} {busy} {onUpdateSettingValue} />
	<ProfileWindowGeometryFields
		width={setting.window_width}
		height={setting.window_height}
		x={setting.window_x}
		y={setting.window_y}
		{busy}
		{onUpdateSettingValue}
	/>
</section>
