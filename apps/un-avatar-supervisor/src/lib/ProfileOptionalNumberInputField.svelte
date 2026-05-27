<script lang="ts">
  import { optionalNumberFromInput } from "./formInputs";

  export let label: string;
  export let hint: string | undefined = undefined;
  export let value: number | null | undefined;
  export let min: number;
  export let max: number;
  export let step: number;
  export let placeholder = "";
  export let disabled = false;
  export let onChange: (value: number | null) => void | Promise<void>;
</script>

<label data-hint={hint}>
  <span>{label}</span>
  <input
    type="number"
    {min}
    {max}
    {step}
    value={value ?? ""}
    {placeholder}
    {disabled}
    onchange={(event) => onChange(optionalNumberFromInput(event))}
  />
</label>
