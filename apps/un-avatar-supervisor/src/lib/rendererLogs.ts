export type RendererLogData = {
	id: number;
	name: string;
	state: string;
	stderr_tail: string[];
};

export type RendererLogFilter = "all" | number;

const rendererLogErrorPattern = /\b(ERROR|FATAL)\b/;
const rendererLogWarnPattern = /\b(WARN|WARNING)\b/;
const rendererLogInfoPattern = /\b(INFO)\b/;
const rendererLogDebugPattern = /\b(DEBUG|TRACE)\b/;

export function rendererLogFilterFromValue(value: string): RendererLogFilter {
	return value === "all" ? "all" : Number(value);
}

export function filteredRendererLogLines(
	renderers: readonly RendererLogData[],
	textFilter: string,
	rendererFilter: RendererLogFilter
): string[] {
	const query = textFilter.trim().toLowerCase();
	const includeAll = rendererFilter === "all";
	const lines: string[] = [];
	for (const renderer of renderers) {
		if (!includeAll && renderer.id !== rendererFilter) continue;
		if (renderer.stderr_tail.length === 0) {
			const placeholder = `${renderer.name}: ${renderer.state}`;
			if (!query || placeholder.toLowerCase().includes(query)) {
				lines.push(placeholder);
			}
			continue;
		}
		for (const raw of renderer.stderr_tail) {
			const prefixed = `${renderer.name}: ${raw}`;
			if (!query || prefixed.toLowerCase().includes(query)) {
				lines.push(prefixed);
			}
		}
	}
	return lines;
}

export function filteredLinesForRenderer(renderers: readonly RendererLogData[], rendererId: number, textFilter: string): string[] {
	const renderer = renderers.find((item) => item.id === rendererId);
	if (!renderer) return [];
	return filteredLinesForRendererData(renderer, textFilter);
}

export function filteredLinesForRendererData(renderer: RendererLogData, textFilter: string): string[] {
	const query = textFilter.trim().toLowerCase();
	if (renderer.stderr_tail.length === 0) return [];
	if (!query) return renderer.stderr_tail;
	return renderer.stderr_tail.filter((line) => line.toLowerCase().includes(query));
}

export function rendererLogText(lines: readonly string[]): string {
	return lines.join("\n") || "No renderer logs yet.";
}

export function rendererLineSeverity(line: string): "error" | "warn" | "info" | "debug" | "" {
	if (rendererLogErrorPattern.test(line)) return "error";
	if (rendererLogWarnPattern.test(line)) return "warn";
	if (rendererLogInfoPattern.test(line)) return "info";
	if (rendererLogDebugPattern.test(line)) return "debug";
	return "";
}

export function defaultRendererLogExpanded(renderer: RendererLogData): boolean {
	return (
		renderer.state === "Running" ||
		renderer.state === "Starting" ||
		renderer.state === "Degraded" ||
		((renderer.state === "Exited" || renderer.state === "Crashed") && renderer.stderr_tail.length > 0)
	);
}
