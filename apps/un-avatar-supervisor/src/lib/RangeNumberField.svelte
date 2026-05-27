<script lang="ts">
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

</script>

<div class="range-number-field">
  <input
    type="range"
    min={rangeMin}
    max={rangeMax}
    {step}
    value={formatFixed(value, decimals)}
    {disabled}
    oninput={(event) => onChange(finiteNumberFromInput(event))}
  />
  <input
    type="number"
    min={numberMin}
    max={numberMax}
    {step}
    value={formatFixed(value, decimals)}
    {disabled}
    onchange={(event) => onChange(finiteNumberFromInput(event))}
  />
</div>
