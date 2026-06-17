<script lang="ts">
	import { _ } from "svelte-i18n";
	import { boneColliderRadiusField } from "./boneColliderFields";
	import { colliderRadiusMmFromInput, colliderRadiusMmText } from "./formInputs";
	import type { ProfileSettingValue } from "./profileTypes";

	export let labelKey: string;
	export let fieldKey: string;
	export let radiusMm = 0;
	export let disabled = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;

	$: radiusField = boneColliderRadiusField(fieldKey);
</script>

<label class="collider-scale-row">
	<span>{$_(labelKey)}</span>
	<input
		type="range"
		min="0"
		max="300"
		step="1"
		value={radiusMm}
		{disabled}
		onchange={(event) => onUpdateSettingValue(radiusField, colliderRadiusMmFromInput(event))}
	/>
	<input
		type="text"
		min="0"
		max="1000"
		step="1"
		value={colliderRadiusMmText(radiusMm)}
		{disabled}
		onchange={(event) => onUpdateSettingValue(radiusField, colliderRadiusMmFromInput(event))}
	/>
	<span class="unit">mm</span>
</label>
