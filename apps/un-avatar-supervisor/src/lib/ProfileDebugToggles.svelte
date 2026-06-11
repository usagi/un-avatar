<script lang="ts">
	import { _ } from "svelte-i18n";
	import { debugToggleFields, type DebugToggleSetting } from "./debugToggleFields";
	import type { ProfileSettingValue } from "./profileTypes";
	import ProfileToggleField from "./ProfileToggleField.svelte";

	export let setting: DebugToggleSetting;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
</script>

<details class="debug-toggles-fold">
	<summary>{$_("profiles.editor.debug_toggles")} <span class="phase-tag">debug</span></summary>
	<div class="debug-toggles-grid">
		{#each debugToggleFields as item}
			<ProfileToggleField
				label={item.label}
				hint={item.hint}
				checked={setting[item.key]}
				disabled={busy}
				onChange={(checked) => onUpdateSettingValue(item.field, checked)}
			/>
		{/each}
	</div>
</details>
