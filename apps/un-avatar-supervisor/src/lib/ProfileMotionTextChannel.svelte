<script lang="ts">
	import type { ProfileSettingValue } from "./profileTypes";

	export let enabled: boolean;
	export let value: string;
	export let placeholder = "";
	export let busy = false;
	export let headingLabel: string;
	export let headingHint: string;
	export let fieldLabel: string;
	export let fieldHint: string;
	export let enabledField: string;
	export let valueField: string;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
</script>

<div class="subgroup">
	<label class="profile-channel-heading" data-hint={headingHint}>
		<input
			type="checkbox"
			checked={enabled}
			onchange={(event) => onUpdateSettingValue(enabledField, (event.currentTarget as HTMLInputElement).checked)}
		/>
		<span>{headingLabel}</span>
	</label>
	<div class="section-grid profile-channel-fields">
		<label data-hint={fieldHint}>
			<span>{fieldLabel}</span>
			<input
				{value}
				{placeholder}
				disabled={!enabled || busy}
				onchange={(event) => onUpdateSettingValue(valueField, (event.currentTarget as HTMLInputElement).value)}
			/>
		</label>
	</div>
</div>
