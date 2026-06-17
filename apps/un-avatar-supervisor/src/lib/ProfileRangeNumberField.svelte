<script lang="ts">
	import RangeNumberField from "./RangeNumberField.svelte";

	export let className = "";
	export let label: string;
	export let hint: string | undefined = undefined;
	export let unit: string | null = null;
	export let value: number;
	export let decimals = 0;
	export let rangeMin: number;
	export let rangeMax: number;
	export let numberMin: number = rangeMin;
	export let numberMax: number = rangeMax;
	export let step: number;
	export let disabled = false;
	export let onChange: (value: number) => void | Promise<void>;
</script>

<label class={className} data-hint={hint}
	>{label}<RangeNumberField
		{value}
		{decimals}
		{rangeMin}
		{rangeMax}
		{numberMin}
		{numberMax}
		{step}
		{disabled}
		{onChange}
	/>{#if unit}<small class="field-unit">{unit}</small>{/if}</label
>
