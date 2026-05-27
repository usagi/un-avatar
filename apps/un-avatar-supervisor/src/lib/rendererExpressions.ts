export type ExpressionOverrides = Record<number, Record<string, number>>;

export function expressionOverrideValue(
  overrides: ExpressionOverrides,
  rendererId: number | undefined | null,
  name: string,
): number {
  if (rendererId == null) return 0;
  return overrides[rendererId]?.[name] ?? 0;
}

export function expressionOverrideCount(
  overrides: ExpressionOverrides,
  rendererId: number | undefined | null,
): number {
  if (rendererId == null) return 0;
  const rendererOverrides = overrides[rendererId];
  if (!rendererOverrides) return 0;
  let count = 0;
  for (const name in rendererOverrides) {
    const value = rendererOverrides[name];
    if (value > 0.0001) count += 1;
  }
  return count;
}

export function filteredExpressionPresets(
  presets: string[],
  filter: string,
): string[] {
  const needle = filter.trim().toLowerCase();
  if (!needle) return presets;
  return presets.filter((preset) => preset.toLowerCase().includes(needle));
}
