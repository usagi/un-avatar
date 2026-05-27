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
    onchange={(event) => onChange(clampedNumberFromInput(event, min, max))}
  />
</label>
