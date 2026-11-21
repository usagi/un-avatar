<script lang="ts">
	import { clampedNumberFromInput } from "./formInputs";

	export let label: string;
	export let hint: string | undefined = undefined;
	export let value: number | null | undefined;
	export let min: number;
	export let max: number;
	export let step: number;
	export let placeholder = "";
	export let disabled = false;
	export let onChange: (value: number) => void | Promise<void>;
	function commitInput(event: Event) {
		const next = clampedNumberFromInput(event, min, max);
		// A clamped edit can equal the existing prop, so Svelte need not update
		// the DOM. Explicitly show the same value that is sent for persistence.
		(event.currentTarget as HTMLInputElement).value = String(next);
		void onChange(next);
	}
</script>

<label data-hint={hint}>
	<span>{label}</span>
	<input
		type="number"
		{min}
		{max}
		{step}
		{value}
		{placeholder}
		{disabled}
		onchange={commitInput}
	/>
</label>
