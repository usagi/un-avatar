export function basename(path: string | null): string {
	if (!path) return "None";
	return path.split(/[\\/]/).pop() ?? path;
}

export function dirname(path: string | null): string | null {
	if (!path) return null;
	const index = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
	return index > 0 ? path.slice(0, index) : null;
}

export function formatBytes(bytes: number | null): string {
	if (bytes == null) return "--";
	if (bytes < 1024) return `${bytes} B`;
	if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
	return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function formatSignedBytes(delta: number): string {
	const sign = delta > 0 ? "+" : delta < 0 ? "-" : "";
	return `${sign}${formatBytes(Math.abs(delta))}`;
}

export function formatFixed(value: number | null | undefined, digits = 2): string {
	if (value == null || !Number.isFinite(value)) return "";
	return value.toFixed(digits);
}

export function formatPercent(value: number | null | undefined): string {
	if (value == null || !Number.isFinite(value)) return "";
	return (value * 100).toFixed(0);
}

export function formatUptime(seconds: number): string {
	const hours = Math.floor(seconds / 3600)
		.toString()
		.padStart(2, "0");
	const minutes = Math.floor((seconds % 3600) / 60)
		.toString()
		.padStart(2, "0");
	const secs = Math.floor(seconds % 60)
		.toString()
		.padStart(2, "0");
	return `${hours}:${minutes}:${secs}`;
}

export function formatClockTimeFromUnixSecs(secs: number): string {
	return new Date(secs * 1000).toLocaleTimeString([], {
		hour: "2-digit",
		minute: "2-digit",
		second: "2-digit",
	});
}

export function formatShortDateTimeFromUnixSecs(secs: number | null | undefined): string {
	if (!secs) return "Unknown";
	return new Date(secs * 1000).toLocaleString([], {
		month: "2-digit",
		day: "2-digit",
		hour: "2-digit",
		minute: "2-digit",
	});
}

export function formatUnixSecsLabel(secs: number | null): string {
	return secs == null ? "unknown" : new Date(secs * 1000).toLocaleString();
}

export function filenameTimestamp(date = new Date()): string {
	return date.toISOString().replace(/[:.]/g, "-").replace("T", "_").slice(0, 19);
}

export function runtimeMetric(value: number | null | undefined, suffix = ""): string {
	if (value === null || value === undefined) return "--";
	return `${value.toFixed(value >= 10 ? 0 : 1)}${suffix}`;
}

export function thresholdHealthClass(
	value: number | null | undefined,
	good: (value: number) => boolean,
	warn: (value: number) => boolean
): string {
	if (value == null) return "metric-idle";
	if (good(value)) return "metric-good";
	if (warn(value)) return "metric-warn";
	return "metric-bad";
}

export function fpsHealthClass(value: number | null | undefined): string {
	return thresholdHealthClass(
		value,
		(fps) => fps >= 50,
		(fps) => fps >= 25
	);
}

export function gpuHealthClass(value: number | null | undefined): string {
	return thresholdHealthClass(
		value,
		(ms) => ms <= 12,
		(ms) => ms <= 25
	);
}

export function ramHealthClass(value: number | null | undefined): string {
	return thresholdHealthClass(
		value,
		(mb) => mb <= 1024,
		(mb) => mb <= 3072
	);
}

export function aaModeLabel(aa: string | null | undefined): string {
	switch (aa) {
		case "off":
			return "Off";
		case "fxaa":
			return "FXAA";
		case "smaa":
			return "SMAA";
		case "msaa":
			return "MSAA";
		case undefined:
		case null:
			return "--";
		default:
			return aa;
	}
}

const textureSizeModes = new Set(["1k", "2k", "4k", "8k"]);

export function textureModeLabel(value: string | null | undefined): string {
	if (!value) return "--";
	if (textureSizeModes.has(value)) return value.toUpperCase();
	return value.replace(/_/g, " ").replace(/\b\w/g, (char) => char.toUpperCase());
}
