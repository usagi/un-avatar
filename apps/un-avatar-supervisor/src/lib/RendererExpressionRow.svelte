<script lang="ts">
	import { _ } from "svelte-i18n";
	import { numberFromInput } from "./formInputs";

	export let rendererId: number | null = null;
	export let preset: string;
	export let weight: number;
	export let hasRunningRenderer = false;
	export let onSetExpressionOverride: (rendererId: number, preset: string, weight: number) => void;

	$: active = weight > 0.0001;
	$: percent = Math.round(weight * 100);

	function setWeight(value: number): void {
		if (rendererId == null) return;
		onSetExpressionOverride(rendererId, preset, value);
	}
</script>

<div class:active class="expression-row" title={preset}>
	<div class="expression-name">
		<strong>{preset}</strong>
		<small>{active ? $_("renderers.animator.expression_overridden") : $_("renderers.animator.expression_idle")}</small>
	</div>
	<label class="expression-slider">
		<input
			type="range"
			min="0"
			max="1"
			step="0.01"
			aria-label={preset}
			value={weight}
			disabled={!hasRunningRenderer}
			style={`--expression-weight: ${percent}%`}
			oninput={(event) => setWeight(numberFromInput(event))}
		/>
	</label>
	<span class="expression-value">{percent}%</span>
	<div class="expression-quick-actions">
		<button type="button" disabled={!hasRunningRenderer || !active} onclick={() => setWeight(0)}>0</button>
		<button type="button" disabled={!hasRunningRenderer || weight >= 0.999} onclick={() => setWeight(1)}>100</button>
	</div>
</div>
