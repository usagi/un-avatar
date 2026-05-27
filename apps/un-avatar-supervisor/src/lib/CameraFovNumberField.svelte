<script lang="ts">
  import { formatFixed } from "./formatting";
  import { clampedNumberFromInput } from "./formInputs";

  export let label: string;
  export let value: number;
  export let min: number;
  export let max: number;
  export let step: number;
  export let unit: string;
  export let decimals = 1;
  export let onChange: (value: number) => void | Promise<void>;
</script>

<label>
  {label}<input
    type="number"
    {min}
    {max}
    {step}
    value={formatFixed(value, decimals)}
    onchange={(event) => onChange(clampedNumberFromInput(event, min, max))}
  /><span>{unit}</span>
</label>
