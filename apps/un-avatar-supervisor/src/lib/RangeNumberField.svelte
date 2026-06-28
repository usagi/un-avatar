<script lang="ts">
	import { onDestroy } from "svelte";
	import { formatFixed } from "./formatting";
	import { finiteNumberFromInput } from "./formInputs";

	export let value: number;
	export let decimals = 0;
	export let rangeMin: number;
	export let rangeMax: number;
	export let numberMin: number = rangeMin;
	export let numberMax: number = rangeMax;
	export let step: number;
	export let disabled = false;
	export let onChange: (value: number) => void | Promise<void>;

	const inputCoalesceMs = 80;
	let pendingRangeValue: number | null = null;
	let pendingRangeTimer: number | null = null;

	function clearPendingRangeTimer(): void {
		if (pendingRangeTimer === null) return;
		window.clearTimeout(pendingRangeTimer);
		pendingRangeTimer = null;
	}

	function emitChange(value: number): void {
		void onChange(value);
	}

	function queueRangeInput(value: number): void {
		pendingRangeValue = value;
		clearPendingRangeTimer();
		pendingRangeTimer = window.setTimeout(() => {
			pendingRangeTimer = null;
			if (pendingRangeValue === null) return;
			const next = pendingRangeValue;
			pendingRangeValue = null;
			emitChange(next);
		}, inputCoalesceMs);
	}

	function flushRangeInput(value: number): void {
		clearPendingRangeTimer();
		pendingRangeValue = null;
		emitChange(value);
	}

	onDestroy(() => {
		clearPendingRangeTimer();
	});
</script>

<div class="range-number-field">
	<input
		type="range"
		min={rangeMin}
		max={rangeMax}
		{step}
		value={formatFixed(value, decimals)}
		{disabled}
		oninput={(event) => queueRangeInput(finiteNumberFromInput(event))}
		onchange={(event) => flushRangeInput(finiteNumberFromInput(event))}
	/>
	<input
		type="number"
		min={numberMin}
		max={numberMax}
		{step}
		value={formatFixed(value, decimals)}
		{disabled}
		onchange={(event) => flushRangeInput(finiteNumberFromInput(event))}
	/>
</div>
