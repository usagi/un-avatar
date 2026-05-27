export function numberFromInput(event: Event): number {
  return Number((event.currentTarget as HTMLInputElement).value);
}

export function numberFromSelect(event: Event): number {
  return Number((event.currentTarget as HTMLSelectElement).value);
}

export function finiteNumberFromInput(event: Event): number {
  const value = numberFromInput(event);
  return Number.isFinite(value) ? value : 0;
}

export function optionalNumberFromInput(event: Event): number | null {
  const raw = (event.currentTarget as HTMLInputElement).value;
  return raw.trim() === "" ? null : Number(raw);
}

export function clampedNumberFromInput(
  event: Event,
  min: number,
  max: number,
  fallback = min,
): number {
  const value = numberFromInput(event);
  return Math.min(max, Math.max(min, Number.isFinite(value) ? value : fallback));
}

export function clampedNumberFromInputOrFallback(
  event: Event,
  min: number,
  max: number,
  fallback: number,
): number {
  const value = numberFromInput(event) || fallback;
  return Math.min(max, Math.max(min, value));
}

export function colliderRadiusMmText(value: number): string {
  return value < 0.001 ? "OFF" : value.toFixed(0);
}

export function colliderRadiusMmFromInput(event: Event): number {
  const raw = (event.currentTarget as HTMLInputElement).value.trim();
  return raw.toUpperCase() === "OFF" ? 0 : Number(raw);
}
