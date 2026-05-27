<script lang="ts">
  type ColorDisplayMode = "rgb_unorm" | "rgb_uint8" | "rgb_hex" | "hsl_unorm";

  type RgbColor = [number, number, number];
  type HslColor = { h: number; s: number; l: number };

  export let label: string;
  export let value: RgbColor | null;
  export let fallback: RgbColor;
  export let disabled = false;
  export let mode: ColorDisplayMode = "rgb_uint8";
  export let hint: string | null = null;
  export let className = "";
  export let onChange: (value: RgbColor) => void;
  export let onModeChange: (mode: ColorDisplayMode) => void;

  const clamp01 = (value: number): number => Math.min(1, Math.max(0, Number.isFinite(value) ? value : 0));
  const clampRgb = (color: RgbColor): RgbColor => color.map(clamp01) as RgbColor;

  $: rgb = clampRgb((value ?? fallback) as RgbColor);
  $: hex = rgbToHex(rgb);
  $: hsl = rgbToHsl(rgb);

  function rgbToHex(color: RgbColor): string {
    return `#${color
      .map((channel) =>
        Math.round(clamp01(channel) * 255)
          .toString(16)
          .padStart(2, "0"),
      )
      .join("")}`;
  }

  function hexToRgb(value: string): RgbColor | null {
    const hex = value.trim().replace(/^#/, "");
    if (!/^[0-9a-fA-F]{6}$/.test(hex)) return null;
    return [0, 2, 4].map((offset) => parseInt(hex.slice(offset, offset + 2), 16) / 255) as RgbColor;
  }

  function rgbToHsl(color: RgbColor): HslColor {
    const [r, g, b] = clampRgb(color);
    const max = Math.max(r, g, b);
    const min = Math.min(r, g, b);
    const l = (max + min) / 2;
    if (max === min) return { h: 0, s: 0, l };
    const d = max - min;
    const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    const h =
      max === r
        ? ((g - b) / d + (g < b ? 6 : 0)) / 6
        : max === g
          ? ((b - r) / d + 2) / 6
          : ((r - g) / d + 4) / 6;
    return { h: h * 360, s, l };
  }

  function hslToRgb(h: number, s: number, l: number): RgbColor {
    const hue = (((h % 360) + 360) % 360) / 360;
    const sat = clamp01(s);
    const light = clamp01(l);
    if (sat === 0) return [light, light, light];
    const q = light < 0.5 ? light * (1 + sat) : light + sat - light * sat;
    const p = 2 * light - q;
    const hueToRgb = (t0: number) => {
      let t = t0;
      if (t < 0) t += 1;
      if (t > 1) t -= 1;
      if (t < 1 / 6) return p + (q - p) * 6 * t;
      if (t < 1 / 2) return q;
      if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6;
      return p;
    };
    return [hueToRgb(hue + 1 / 3), hueToRgb(hue), hueToRgb(hue - 1 / 3)];
  }

  function setRgbChannel(axis: 0 | 1 | 2, channel: number): void {
    const next = [...rgb] as RgbColor;
    next[axis] = clamp01(channel);
    onChange(next);
  }

  function setRgbByte(axis: 0 | 1 | 2, channel: number): void {
    setRgbChannel(axis, Math.min(255, Math.max(0, channel)) / 255);
  }

  function setHsl(part: keyof HslColor, nextValue: number): void {
    const next = { ...hsl };
    next[part] = part === "h" ? nextValue : clamp01(nextValue);
    onChange(hslToRgb(next.h, next.s, next.l));
  }

  function setHex(value: string): void {
    const next = hexToRgb(value);
    if (next) onChange(next);
  }
</script>

<fieldset class={`unified-color-field ${className}`} data-hint={hint}>
  <legend>{label}</legend>
  <div class="color-summary">
    <label class="color-picker-swatch" aria-label={`${label} picker`}>
      <span class="color-preview" style={`background: ${hex}`} aria-hidden="true"></span>
      <input
        type="color"
        value={hex}
        {disabled}
        oninput={(event) => setHex((event.currentTarget as HTMLInputElement).value)}
      />
    </label>
    <select
      value={mode}
      {disabled}
      onchange={(event) => onModeChange((event.currentTarget as HTMLSelectElement).value as ColorDisplayMode)}
      aria-label={`${label} display mode`}
    >
      <option value="rgb_uint8">RGB 0-255</option>
      <option value="rgb_unorm">RGB 0-1</option>
      <option value="rgb_hex">#RRGGBB</option>
      <option value="hsl_unorm">HSL</option>
    </select>
  </div>

  {#if mode === "rgb_uint8"}
    <div class="color-channel-grid">
      <label>R<input type="number" min="0" max="255" step="1" value={Math.round(rgb[0] * 255)} {disabled} onchange={(event) => setRgbByte(0, Number((event.currentTarget as HTMLInputElement).value))} /></label>
      <label>G<input type="number" min="0" max="255" step="1" value={Math.round(rgb[1] * 255)} {disabled} onchange={(event) => setRgbByte(1, Number((event.currentTarget as HTMLInputElement).value))} /></label>
      <label>B<input type="number" min="0" max="255" step="1" value={Math.round(rgb[2] * 255)} {disabled} onchange={(event) => setRgbByte(2, Number((event.currentTarget as HTMLInputElement).value))} /></label>
    </div>
  {:else if mode === "rgb_unorm"}
    <div class="color-channel-grid">
      <label>R<input type="number" min="0" max="1" step="0.01" value={rgb[0].toFixed(2)} {disabled} onchange={(event) => setRgbChannel(0, Number((event.currentTarget as HTMLInputElement).value))} /></label>
      <label>G<input type="number" min="0" max="1" step="0.01" value={rgb[1].toFixed(2)} {disabled} onchange={(event) => setRgbChannel(1, Number((event.currentTarget as HTMLInputElement).value))} /></label>
      <label>B<input type="number" min="0" max="1" step="0.01" value={rgb[2].toFixed(2)} {disabled} onchange={(event) => setRgbChannel(2, Number((event.currentTarget as HTMLInputElement).value))} /></label>
    </div>
  {:else if mode === "rgb_hex"}
    <label class="hex-field">Hex<input type="text" value={hex} maxlength="7" pattern="#?[0-9a-fA-F]{6}" {disabled} onchange={(event) => setHex((event.currentTarget as HTMLInputElement).value)} /></label>
  {:else}
    <div class="color-channel-grid">
      <label>H<input type="number" min="0" max="360" step="1" value={hsl.h.toFixed(0)} {disabled} onchange={(event) => setHsl("h", Number((event.currentTarget as HTMLInputElement).value))} /></label>
      <label>S<input type="number" min="0" max="1" step="0.01" value={hsl.s.toFixed(2)} {disabled} onchange={(event) => setHsl("s", Number((event.currentTarget as HTMLInputElement).value))} /></label>
      <label>L<input type="number" min="0" max="1" step="0.01" value={hsl.l.toFixed(2)} {disabled} onchange={(event) => setHsl("l", Number((event.currentTarget as HTMLInputElement).value))} /></label>
    </div>
  {/if}
</fieldset>

<style>
  .unified-color-field {
    display: grid;
    gap: 8px;
    min-width: 0;
    margin: 0;
    padding: 10px;
    border: 1px solid var(--border);
    border-radius: 8px;
  }

  .unified-color-field legend {
    padding: 0 4px;
    color: var(--muted);
    font-size: 12px;
    font-weight: 700;
  }

  .color-summary {
    display: grid;
    grid-template-columns: 54px minmax(120px, 1fr);
    align-items: center;
    gap: 10px;
    min-width: 0;
  }

  .color-picker-swatch {
    position: relative;
    display: block;
    width: 54px;
    height: 34px;
    cursor: pointer;
  }

  .color-picker-swatch input[type="color"] {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    padding: 0;
    border: 0;
    opacity: 0;
    cursor: pointer;
  }

  .color-picker-swatch:has(input:disabled) {
    cursor: not-allowed;
    opacity: 0.58;
  }

  .color-preview {
    display: block;
    width: 100%;
    height: 100%;
    border: 1px solid color-mix(in srgb, var(--border) 72%, var(--accent-cool));
    border-radius: 6px;
    box-shadow:
      inset 0 0 0 1px color-mix(in srgb, #ffffff 14%, transparent),
      0 4px 14px color-mix(in srgb, #000000 16%, transparent);
  }

  .color-channel-grid {
    display: grid;
    grid-template-columns: repeat(3, minmax(70px, 1fr));
    gap: 8px;
  }

  .color-channel-grid label,
  .hex-field {
    display: grid;
    gap: 5px;
    min-width: 0;
    color: var(--muted);
    font-size: 12px;
  }

  .hex-field input {
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    text-transform: uppercase;
  }

  @media (max-width: 720px) {
    .color-summary,
    .color-channel-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
