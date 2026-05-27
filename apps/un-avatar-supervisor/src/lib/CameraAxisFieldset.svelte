<script lang="ts">
  import { formatFixed } from "./formatting";
  import { clampedNumberFromInput, finiteNumberFromInput } from "./formInputs";
  import type { CameraAxisField } from "./cameraSectionFields";

  export let legend: string;
  export let hint: string;
  export let className = "";
  export let unit: string;
  export let fields: readonly CameraAxisField[];
  export let onChange: (field: string, value: number) => void | Promise<void>;

  function axisNumberFromInput(event: Event, field: CameraAxisField): number {
    if (field.min === undefined || field.max === undefined) {
      return finiteNumberFromInput(event);
    }
    return clampedNumberFromInput(event, field.min, field.max);
  }
</script>

<fieldset class={`axis-fieldset camera-axis-fieldset ${className}`} data-hint={hint}>
  <legend>{legend}</legend>
  {#each fields as field}
    <label
      >{field.label}
      <input
        type="number"
        step={field.step}
        min={field.min}
        max={field.max}
        value={formatFixed(field.value)}
        onchange={(event) => onChange(field.field, axisNumberFromInput(event, field))}
      /></label
    >
  {/each}
  <span class="axis-unit">{unit}</span>
</fieldset>
