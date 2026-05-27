<script lang="ts">
  import { _ } from "svelte-i18n";

  export let className = "";
  export let label: string;
  export let hint: string | undefined = undefined;
  export let value: string;
  export let disabled = false;
  export let options: readonly (readonly [string, string])[];
  export let onChange: (value: string) => void | Promise<void>;

  function optionLabel(optionLabelValue: string): string {
    return optionLabelValue.startsWith("profiles.")
      ? $_(optionLabelValue)
      : optionLabelValue;
  }
</script>

<label class={className} data-hint={hint}
  >{label}<select
    {value}
    {disabled}
    onchange={(event) =>
      onChange((event.currentTarget as HTMLSelectElement).value)}
  >
    {#each options as [optionValue, optionLabelValue]}
      <option value={optionValue}>{optionLabel(optionLabelValue)}</option>
    {/each}
  </select></label
>
