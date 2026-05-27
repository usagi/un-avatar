export type RendererStateCounts = {
  running: number;
  issues: number;
};

export function rendererStateClass(state: string): string {
  return `state state-${state.toLowerCase()}`;
}

export function countRendererStates<T extends { state: string }>(
  items: readonly T[],
): RendererStateCounts {
  let running = 0;
  let issues = 0;
  for (const renderer of items) {
    if (renderer.state === "Running") running += 1;
    if (renderer.state === "Crashed" || renderer.state === "Degraded") issues += 1;
  }
  return { running, issues };
}

export function countErrorNotifications<T extends { level: string }>(
  items: readonly T[],
): number {
  let count = 0;
  for (const notification of items) {
    if (notification.level === "error") count += 1;
  }
  return count;
}
