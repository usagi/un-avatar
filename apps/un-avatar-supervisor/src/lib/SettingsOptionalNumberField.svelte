<script lang="ts">
  export let label: string;
  export let hint: string | undefined = undefined;
  export let value: number | null | undefined;
  export let placeholder = "";
  export let min: number | null = null;
  export let step = 1;
  export let onChange: (value: number | null) => void;

  function parseNullableNumber(raw: string): number | null {
    return raw.trim() === "" ? null : Number(raw);
  }
</script>

<div class="setting-row" data-hint={hint}>
  <span>{label}</span>
  <input
    type="number"
    {step}
    min={min ?? undefined}
    value={value ?? ""}
    {placeholder}
    oninput={(event) =>
      onChange(parseNullableNumber((event.currentTarget as HTMLInputElement).value))}
  />
</div>
