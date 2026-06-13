<script lang="ts">
	import type { ProfileSettingValue } from "./profileTypes";
	import { _ } from "svelte-i18n";
	import ProfileDynamicsOverrideRow from "./ProfileDynamicsOverrideRow.svelte";
	import type { DynamicsCategoryOverrideSetting } from "./dynamicsPresets";

	export let overrides: DynamicsCategoryOverrideSetting[] = [];
	export let dynamicsEnabled = false;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
</script>

<div class="profile-wide-field dynamics-overrides">
	<div class="field-note">{$_("profiles.editor.dynamics_category_overrides")}</div>
	<div class="dynamics-override-list">
		{#each overrides as override}
			<ProfileDynamicsOverrideRow {override} {dynamicsEnabled} {busy} {onUpdateSettingValue} />
		{/each}
	</div>
</div>
