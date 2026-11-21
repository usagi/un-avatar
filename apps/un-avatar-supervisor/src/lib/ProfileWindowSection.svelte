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
	export let runtimeWindowAvailable = false;
	export let reviewRendererIds: number[] = [];
	export let busy = false;
	export let colorDisplayMode: ColorDisplayMode;
	export let onColorModeChange: ColorModeChangeHandler;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
	export let onBackgroundColorChange: (value: [number, number, number]) => void;
	export let onCaptureRuntimeWindow: () => void | Promise<void>;
	export let onActivate: () => void;
</script>

<section
	class="editor-section section-grid profile-window-section"
	tabindex="-1"
	data-profile-section="window"
	onfocusin={onActivate}
	data-hint={$_("profiles.hints.window.section")}
>
	<div class="section-title-row">
		<h3>{$_("profiles.editor.window")}</h3>
		<button
			type="button"
			class="secondary compact"
			disabled={busy || !runtimeWindowAvailable}
			title={$_("profiles.editor.capture_runtime_window_title")}
			onclick={onCaptureRuntimeWindow}>{$_("profiles.editor.capture_runtime_window")}</button
		>
	</div>
	{#if import.meta.env.VITE_UN_DESIGN_PREVIEW === "1"}
        <p class="profile-window-target-scope" role="note">
            {#if reviewRendererIds.length}
                ウィンドウ変更の反映先・現在値の取得元：個体 #{reviewRendererIds[0]}。
                背景色の反映先：{reviewRendererIds.map(id => `#${id}`).join("、")}。
                {#if reviewRendererIds.length > 1}上部の表示対象を切り替えても、この反映先は変わらない。{/if}
            {:else}
                稼働個体なし。変更はこのプロファイルに保存し、次回起動に使用する。
            {/if}
        </p>
    {/if}
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
