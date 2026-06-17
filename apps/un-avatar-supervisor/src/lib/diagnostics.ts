import {
	aaModeLabel,
	basename,
	formatBytes,
	formatShortDateTimeFromUnixSecs,
	formatSignedBytes,
	formatUnixSecsLabel,
	runtimeMetric,
	textureModeLabel,
} from "./formatting";

export type DiagnosticsLevel = "error" | "warning" | "info";

export type DiagnosticsExportEntryLabelData = {
	path: string;
	archive_path: string | null;
	size_bytes: number | null;
	archive_size_bytes: number | null;
	generated_at_secs: number | null;
	modified_at_secs: number | null;
};

export type DiagnosticsRendererSnapshotLabelData = {
	name?: string;
	state: string | null;
	connected: boolean;
	aa: string | null;
	texturePolicy: string;
	textureSummary: string;
	spoutEnabled: boolean;
	spoutName: string | null;
	spoutAvailable: boolean | null;
	spoutFramesAttempted: number;
	spoutFramesSent: number;
	spoutFailures: number;
	spoutConsecutiveFailures: number;
};

export type DiagnosticsBundleStats = {
	generated: string;
	git: string;
	renderers: number;
	connected: number;
	issueRenderers: number;
	notifications: number;
	profiles: number;
	launcher: number;
	spoutActive: number;
	spoutFailures: number;
	spoutConsecutiveFailures: number;
	textureImages: number;
	textureUploadedBytes: number;
	textureCompressed: number;
	textureFallbacks: number;
	nativeNotification: string;
	disconnected: string[];
};

export type DiagnosticsSummaryItem = {
	label: string;
	value: string;
};

export type DiagnosticsFinding = {
	level: DiagnosticsLevel;
	title: string;
	body: string;
};

export type DiagnosticsCompareItem = {
	label: string;
	before: string;
	after: string;
	level: "warning" | "info";
};

export type DiagnosticsRendererInsight = {
	name: string;
	level: DiagnosticsLevel;
	state: string;
	runtime: string;
	frame: string;
	texture: string;
	output: string;
	note: string | null;
};

export type DiagnosticsRuntimeLabels = {
	pending: string;
	connected: string;
	disconnected: string;
};

export type DiagnosticsRendererSnapshot = DiagnosticsRendererSnapshotLabelData & {
	name: string;
	state: string;
	protocol: string | null;
	fps: number | null;
	gpuMs: number | null;
	surface: string;
	textureNote: string | null;
	textureUploadedBytes: number;
	note: string | null;
};

export function countTextMatches(text: string, query: string): number {
	if (!query) return 0;
	let count = 0;
	let start = 0;
	const lowerText = text.toLowerCase();
	while (true) {
		const index = lowerText.indexOf(query, start);
		if (index < 0) return count;
		count += 1;
		start = index + query.length;
	}
}

export function diagnosticsLevelWeight(level: DiagnosticsLevel): number {
	if (level === "error") return 2;
	if (level === "warning") return 1;
	return 0;
}

export function diagnosticsBundleFromText(text: string): Record<string, unknown> | null {
	try {
		return JSON.parse(text) as Record<string, unknown>;
	} catch {
		return null;
	}
}

export function diagnosticsEntryTime(entry: DiagnosticsExportEntryLabelData): string {
	return formatShortDateTimeFromUnixSecs(entry.generated_at_secs ?? entry.modified_at_secs);
}

export function diagnosticsEntrySearchText(entry: DiagnosticsExportEntryLabelData): string {
	return [
		basename(entry.path),
		basename(entry.archive_path),
		diagnosticsEntryTime(entry),
		formatBytes(entry.size_bytes),
		formatBytes(entry.archive_size_bytes),
	]
		.join(" ")
		.toLowerCase();
}

export function diagnosticsEntrySecs(entry: DiagnosticsExportEntryLabelData): number | null {
	return entry.generated_at_secs ?? entry.modified_at_secs;
}

export function diagnosticsComparisonSummary(left: DiagnosticsExportEntryLabelData, right: DiagnosticsExportEntryLabelData): string {
	const sizeDelta = (right.size_bytes ?? 0) - (left.size_bytes ?? 0);
	const leftSecs = diagnosticsEntrySecs(left);
	const rightSecs = diagnosticsEntrySecs(right);
	const timeDelta = leftSecs == null || rightSecs == null ? "time unknown" : `${Math.abs(rightSecs - leftSecs)}s apart`;
	return `Size ${formatSignedBytes(sizeDelta)} / ${timeDelta}`;
}

export function diagnosticsSpoutNote(note: string | null): string | null {
	return note?.startsWith("Spout2") ? note : null;
}

export function diagnosticsTextureNote(note: string | null): string | null {
	return note?.startsWith("Texture") ? note : null;
}

export function diagnosticsTextureFallbackNote(runtimeStatus: Record<string, unknown>): string | null {
	if (stringField(runtimeStatus, "texture_compression") === "source") {
		return null;
	}
	const summary = objectField(runtimeStatus, "texture_summary");
	const fallbackCount = numberField(summary, "compression_fallback_count") ?? 0;
	if (fallbackCount <= 0) return null;
	const compressedCount = numberField(summary, "compressed_count") ?? 0;
	if (compressedCount === 0 && booleanField(summary, "compression_bc_supported") === false) {
		return `Texture compression fell back to RGBA for ${fallbackCount} image${fallbackCount === 1 ? "" : "s"} because BC upload is unavailable`;
	}
	if (compressedCount === 0) {
		return `Texture compression kept ${fallbackCount} requested image${fallbackCount === 1 ? "" : "s"} as RGBA`;
	}
	return `Texture compression used ${compressedCount} image${compressedCount === 1 ? "" : "s"}, kept ${fallbackCount} as RGBA`;
}

export function diagnosticsRendererSpoutLabel(snapshot: DiagnosticsRendererSnapshotLabelData): string {
	if (!snapshot.spoutEnabled) return "Window";
	const name = snapshot.spoutName ? ` / ${snapshot.spoutName}` : "";
	if (snapshot.spoutAvailable === false) return `Spout2${name} unavailable`;
	if (snapshot.spoutFramesAttempted === 0) return `Spout2${name} waiting`;
	return `Spout2${name}: ${snapshot.spoutFramesSent}/${snapshot.spoutFramesAttempted}, ${snapshot.spoutFailures} failed`;
}

export function diagnosticsTexturePolicyLabel(runtimeStatus: Record<string, unknown>, pendingLabel: string): string {
	if (booleanField(runtimeStatus, "connected") !== true) return pendingLabel;
	const limit = textureModeLabel(stringField(runtimeStatus, "texture_resolution_limit"));
	const compression = textureModeLabel(stringField(runtimeStatus, "texture_compression"));
	const cache = booleanField(runtimeStatus, "processed_texture_cache");
	const cacheLabel = cache == null ? "cache --" : cache ? "cache on" : "cache off";
	return `${limit} / ${compression} / ${cacheLabel}`;
}

export function diagnosticsTextureSummaryLabel(runtimeStatus: Record<string, unknown>): string {
	const summary = objectField(runtimeStatus, "texture_summary");
	const imageCount = numberField(summary, "image_count");
	if (imageCount == null) return "--";
	const resizedCount = numberField(summary, "resized_count") ?? 0;
	const compressedCount = numberField(summary, "compressed_count") ?? 0;
	const fallbackCount = numberField(summary, "compression_fallback_count") ?? 0;
	const resized = resizedCount > 0 ? `, ${resizedCount} resized` : "";
	const compressed = compressedCount > 0 ? `, ${compressedCount} compressed` : "";
	const fallback = fallbackCount > 0 ? `, ${fallbackCount} fallback` : "";
	return `${imageCount} images${resized}${compressed}${fallback}, ${formatBytes(numberField(summary, "uploaded_mip_bytes"))} uploaded`;
}

export function diagnosticsRendererCompareLabel(snapshot: DiagnosticsRendererSnapshotLabelData): string {
	const runtime = snapshot.connected ? "live" : "no response";
	const spout = snapshot.spoutEnabled ? `${snapshot.spoutFailures}/${snapshot.spoutConsecutiveFailures} Spout failures` : "Spout off";
	return `${snapshot.state} / ${runtime} / AA ${aaModeLabel(snapshot.aa)} / ${snapshot.texturePolicy} / ${snapshot.textureSummary} / ${spout}`;
}

export function diagnosticsBundleStats(bundle: Record<string, unknown>): DiagnosticsBundleStats {
	const renderers = arrayField(bundle, "renderers");
	const notifications = arrayField(bundle, "notifications");
	const nativeNotifications = objectField(bundle, "native_notifications");
	const profiles = objectField(bundle, "profiles");
	const profileSettings = arrayField(profiles, "settings");
	const launcherSettings = profileLauncherSettings(profiles);
	let connected = 0;
	let issueRenderers = 0;
	let spoutActive = 0;
	let spoutFailures = 0;
	let spoutConsecutiveFailures = 0;
	let textureImages = 0;
	let textureUploadedBytes = 0;
	let textureCompressed = 0;
	let textureFallbacks = 0;
	const disconnected: string[] = [];
	for (const renderer of renderers) {
		const info = objectField(renderer, "info");
		const runtimeStatus = objectField(renderer, "runtime_status");
		const textureSummary = objectField(runtimeStatus, "texture_summary");
		const name = stringField(info, "name") ?? "Unnamed";
		const state = stringField(info, "state");
		if (state === "Crashed" || state === "Degraded") issueRenderers += 1;
		if (booleanField(runtimeStatus, "connected") === true) {
			connected += 1;
		} else {
			disconnected.push(name);
		}
		if (booleanField(runtimeStatus, "spout_enabled") === true) {
			spoutActive += 1;
		}
		spoutFailures += numberField(runtimeStatus, "spout_frame_failures") ?? 0;
		spoutConsecutiveFailures += numberField(runtimeStatus, "spout_consecutive_failures") ?? 0;
		textureImages += numberField(textureSummary, "image_count") ?? 0;
		textureUploadedBytes += numberField(textureSummary, "uploaded_mip_bytes") ?? 0;
		textureCompressed += numberField(textureSummary, "compressed_count") ?? 0;
		textureFallbacks += numberField(textureSummary, "compression_fallback_count") ?? 0;
	}
	return {
		generated: formatUnixSecsLabel(numberField(bundle, "generated_at_secs")),
		git: stringField(objectField(bundle, "build"), "git_head") ?? "unknown",
		renderers: renderers.length,
		connected,
		issueRenderers,
		notifications: notifications.length,
		profiles: profileSettings.length,
		launcher: launcherSettings.length,
		spoutActive,
		spoutFailures,
		spoutConsecutiveFailures,
		textureImages,
		textureUploadedBytes,
		textureCompressed,
		textureFallbacks,
		nativeNotification: stringField(nativeNotifications, "permission_state") ?? "unknown",
		disconnected,
	};
}

export function diagnosticsRendererSnapshotMap(
	bundle: Record<string, unknown>,
	pendingLabel: string
): Map<string, DiagnosticsRendererSnapshot> {
	const map = new Map<string, DiagnosticsRendererSnapshot>();
	for (const renderer of arrayField(bundle, "renderers")) {
		const snapshot = diagnosticsRendererSnapshot(objectField(renderer, "info"), objectField(renderer, "runtime_status"), pendingLabel);
		map.set(snapshot.name, snapshot);
	}
	return map;
}

export function diagnosticsRendererSnapshot(
	info: Record<string, unknown>,
	runtimeStatus: Record<string, unknown>,
	pendingLabel: string
): DiagnosticsRendererSnapshot {
	const width = numberField(runtimeStatus, "surface_width");
	const height = numberField(runtimeStatus, "surface_height");
	const textureSummary = objectField(runtimeStatus, "texture_summary");
	return {
		name: stringField(info, "name") ?? "Renderer",
		state: stringField(info, "state") ?? "Unknown",
		connected: booleanField(runtimeStatus, "connected") === true,
		protocol: stringField(runtimeStatus, "protocol"),
		fps: numberField(runtimeStatus, "fps"),
		gpuMs: numberField(runtimeStatus, "gpu_ms"),
		surface: width && height ? `${width} x ${height}` : "--",
		aa: stringField(runtimeStatus, "aa"),
		texturePolicy: diagnosticsTexturePolicyLabel(runtimeStatus, pendingLabel),
		textureSummary: diagnosticsTextureSummaryLabel(runtimeStatus),
		textureNote: diagnosticsTextureNote(stringField(runtimeStatus, "note")) ?? diagnosticsTextureFallbackNote(runtimeStatus),
		textureUploadedBytes: numberField(textureSummary, "uploaded_mip_bytes") ?? 0,
		spoutEnabled: booleanField(runtimeStatus, "spout_enabled") === true,
		spoutName: stringField(runtimeStatus, "spout_name"),
		spoutAvailable: booleanField(runtimeStatus, "spout_available"),
		spoutFramesAttempted: numberField(runtimeStatus, "spout_frames_attempted") ?? 0,
		spoutFramesSent: numberField(runtimeStatus, "spout_frames_sent") ?? 0,
		spoutFailures: numberField(runtimeStatus, "spout_frame_failures") ?? 0,
		spoutConsecutiveFailures: numberField(runtimeStatus, "spout_consecutive_failures") ?? 0,
		note: stringField(runtimeStatus, "note") ?? stringField(info, "last_stderr") ?? null,
	};
}

export function diagnosticsRendererLevel(snapshot: DiagnosticsRendererSnapshot): DiagnosticsLevel {
	if (snapshot.state === "Crashed") return "error";
	if (
		snapshot.state === "Degraded" ||
		!snapshot.connected ||
		snapshot.spoutConsecutiveFailures > 0 ||
		diagnosticsSpoutNote(snapshot.note) ||
		snapshot.textureNote ||
		(snapshot.spoutEnabled && snapshot.spoutAvailable === false)
	) {
		return "warning";
	}
	return "info";
}

export function diagnosticsRendererCompareLevel(
	before: DiagnosticsRendererSnapshot | null,
	after: DiagnosticsRendererSnapshot | null
): "warning" | "info" {
	if (!before || !after) return "warning";
	if (after.state === "Crashed" || after.state === "Degraded") return "warning";
	if (before.connected && !after.connected) return "warning";
	if (before.aa !== after.aa) return "warning";
	if (before.texturePolicy !== after.texturePolicy) return "warning";
	if (before.textureSummary !== after.textureSummary) return "warning";
	if (after.spoutConsecutiveFailures > before.spoutConsecutiveFailures) {
		return "warning";
	}
	if (after.spoutFailures > before.spoutFailures) return "warning";
	return "info";
}

export function diagnosticsBundleSummary(bundle: Record<string, unknown>): DiagnosticsSummaryItem[] {
	const renderers = arrayField(bundle, "renderers");
	const notifications = arrayField(bundle, "notifications");
	const nativeNotifications = objectField(bundle, "native_notifications");
	const profiles = objectField(bundle, "profiles");
	const profileSettings = arrayField(profiles, "settings");
	const launcherSettings = profileLauncherSettings(profiles);
	let connectedRenderers = 0;
	let issueRenderers = 0;
	let spoutActive = 0;
	let spoutFailures = 0;
	let textureImages = 0;
	let textureUploadedBytes = 0;
	let textureCompressed = 0;
	let textureFallbacks = 0;
	for (const renderer of renderers) {
		const info = objectField(renderer, "info");
		const runtimeStatus = objectField(renderer, "runtime_status");
		const textureSummary = objectField(runtimeStatus, "texture_summary");
		const state = stringField(info, "state");
		if (booleanField(runtimeStatus, "connected") === true) connectedRenderers += 1;
		if (state === "Crashed" || state === "Degraded") issueRenderers += 1;
		if (booleanField(runtimeStatus, "spout_enabled") === true) spoutActive += 1;
		spoutFailures += numberField(runtimeStatus, "spout_frame_failures") ?? 0;
		textureImages += numberField(textureSummary, "image_count") ?? 0;
		textureUploadedBytes += numberField(textureSummary, "uploaded_mip_bytes") ?? 0;
		textureCompressed += numberField(textureSummary, "compressed_count") ?? 0;
		textureFallbacks += numberField(textureSummary, "compression_fallback_count") ?? 0;
	}
	return [
		{
			label: "Version",
			value: stringField(bundle, "version") ?? "unknown",
		},
		{
			label: "Generated",
			value: formatUnixSecsLabel(numberField(bundle, "generated_at_secs")),
		},
		{
			label: "Renderers",
			value: `${renderers.length} (${connectedRenderers} live)`,
		},
		{
			label: "Issues",
			value: `${issueRenderers} renderer / ${notifications.length} notices`,
		},
		{
			label: "Spout",
			value: `${spoutActive} active / ${spoutFailures} failed sends`,
		},
		{
			label: "Textures",
			value:
				textureImages > 0
					? `${textureImages} images / ${formatBytes(textureUploadedBytes)} uploaded / ${textureCompressed} compressed / ${textureFallbacks} fallback`
					: "unknown",
		},
		{
			label: "Profiles",
			value: `${profileSettings.length} settings / ${launcherSettings.length} launcher`,
		},
		{
			label: "Git",
			value: stringField(objectField(bundle, "build"), "git_head") ?? "unknown",
		},
		{
			label: "Native notice",
			value: stringField(nativeNotifications, "permission_state") ?? "unknown",
		},
	];
}

export function diagnosticsBundleFindings(bundle: Record<string, unknown>): DiagnosticsFinding[] {
	const findings: DiagnosticsFinding[] = [];
	const profiles = objectField(bundle, "profiles");
	const profileError = stringField(profiles, "error");
	if (profileError) {
		findings.push({
			level: "error",
			title: "Profile discovery failed",
			body: profileError,
		});
	}
	for (const entry of arrayField(bundle, "renderers")) {
		const info = objectField(entry, "info");
		const runtimeStatus = objectField(entry, "runtime_status");
		const name = stringField(info, "name") ?? "Renderer";
		const state = stringField(info, "state") ?? "Unknown";
		const runtimeNote = stringField(runtimeStatus, "note");
		if (state === "Crashed" || state === "Degraded") {
			findings.push({
				level: state === "Crashed" ? "error" : "warning",
				title: `${name} is ${state}`,
				body: stringField(info, "last_stderr") ?? "No stderr captured.",
			});
		}
		if (booleanField(runtimeStatus, "connected") === false) {
			findings.push({
				level: "warning",
				title: `${name} runtime status has no response`,
				body: runtimeNote ?? "Runtime status endpoint did not return a live snapshot.",
			});
		}
		const spoutConsecutiveFailures = numberField(runtimeStatus, "spout_consecutive_failures") ?? 0;
		const spoutNote = diagnosticsSpoutNote(runtimeNote ?? null);
		const textureNote = diagnosticsTextureNote(runtimeNote ?? null) ?? diagnosticsTextureFallbackNote(runtimeStatus);
		if (booleanField(runtimeStatus, "spout_enabled") === true && spoutConsecutiveFailures > 0) {
			findings.push({
				level: "warning",
				title: `${name} Spout2 send is failing`,
				body:
					spoutNote ?? `${spoutConsecutiveFailures} consecutive failed send attempt${spoutConsecutiveFailures === 1 ? "" : "s"}.`,
			});
		} else if (booleanField(runtimeStatus, "spout_enabled") === true && spoutNote) {
			findings.push({
				level: "warning",
				title: `${name} Spout2 needs attention`,
				body: spoutNote,
			});
		}
		if (textureNote) {
			findings.push({
				level: "warning",
				title: `${name} texture upload needs attention`,
				body: textureNote,
			});
		}
	}
	let errorNotificationCount = 0;
	const errorNotificationTitles: string[] = [];
	for (const notification of arrayField(bundle, "notifications")) {
		if (stringField(notification, "level") !== "error") continue;
		errorNotificationCount += 1;
		if (errorNotificationTitles.length < 3) {
			errorNotificationTitles.push(stringField(notification, "title") ?? "Untitled");
		}
	}
	if (errorNotificationCount > 0) {
		findings.push({
			level: "error",
			title: `${errorNotificationCount} error notification${errorNotificationCount === 1 ? "" : "s"}`,
			body: errorNotificationTitles.join("; "),
		});
	}
	return findings;
}

export function diagnosticsComparisonDetails(
	leftBundle: Record<string, unknown>,
	rightBundle: Record<string, unknown>
): DiagnosticsCompareItem[] {
	const leftStats = diagnosticsBundleStats(leftBundle);
	const rightStats = diagnosticsBundleStats(rightBundle);
	return [
		{
			label: "Generated",
			before: leftStats.generated,
			after: rightStats.generated,
			level: "info",
		},
		{
			label: "Git",
			before: leftStats.git,
			after: rightStats.git,
			level: leftStats.git === rightStats.git ? "info" : "warning",
		},
		{
			label: "Renderers",
			before: `${leftStats.renderers} / ${leftStats.connected} live`,
			after: `${rightStats.renderers} / ${rightStats.connected} live`,
			level: leftStats.renderers === rightStats.renderers && leftStats.connected === rightStats.connected ? "info" : "warning",
		},
		{
			label: "Issues",
			before: `${leftStats.issueRenderers} renderer / ${leftStats.notifications} notices`,
			after: `${rightStats.issueRenderers} renderer / ${rightStats.notifications} notices`,
			level:
				rightStats.issueRenderers > leftStats.issueRenderers || rightStats.notifications > leftStats.notifications
					? "warning"
					: "info",
		},
		{
			label: "Profiles",
			before: `${leftStats.profiles} settings / ${leftStats.launcher} launcher`,
			after: `${rightStats.profiles} settings / ${rightStats.launcher} launcher`,
			level: leftStats.profiles === rightStats.profiles && leftStats.launcher === rightStats.launcher ? "info" : "warning",
		},
		{
			label: "Spout",
			before: `${leftStats.spoutActive} active`,
			after: `${rightStats.spoutActive} active`,
			level: leftStats.spoutActive === rightStats.spoutActive ? "info" : "warning",
		},
		{
			label: "Spout failures",
			before: `${leftStats.spoutFailures} total / ${leftStats.spoutConsecutiveFailures} consecutive`,
			after: `${rightStats.spoutFailures} total / ${rightStats.spoutConsecutiveFailures} consecutive`,
			level: rightStats.spoutFailures <= leftStats.spoutFailures && rightStats.spoutConsecutiveFailures === 0 ? "info" : "warning",
		},
		{
			label: "Textures",
			before: `${leftStats.textureImages} images / ${formatBytes(leftStats.textureUploadedBytes)} uploaded / ${leftStats.textureCompressed} compressed / ${leftStats.textureFallbacks} fallback`,
			after: `${rightStats.textureImages} images / ${formatBytes(rightStats.textureUploadedBytes)} uploaded / ${rightStats.textureCompressed} compressed / ${rightStats.textureFallbacks} fallback`,
			level:
				leftStats.textureImages === rightStats.textureImages &&
				leftStats.textureUploadedBytes === rightStats.textureUploadedBytes &&
				leftStats.textureCompressed === rightStats.textureCompressed &&
				leftStats.textureFallbacks === rightStats.textureFallbacks
					? "info"
					: "warning",
		},
		{
			label: "Native notice",
			before: leftStats.nativeNotification,
			after: rightStats.nativeNotification,
			level: leftStats.nativeNotification === rightStats.nativeNotification ? "info" : "warning",
		},
		(() => {
			const before = leftStats.disconnected.join(", ") || "none";
			const after = rightStats.disconnected.join(", ") || "none";
			const beforeKey = leftStats.disconnected.join("\n");
			const afterKey = rightStats.disconnected.join("\n");
			return {
				label: "No response",
				before,
				after,
				level: beforeKey === afterKey ? "info" : "warning",
			};
		})(),
	];
}

export function diagnosticsRendererComparisonDetails(
	leftBundle: Record<string, unknown>,
	rightBundle: Record<string, unknown>,
	labels: Pick<DiagnosticsRuntimeLabels, "pending">
): DiagnosticsCompareItem[] {
	const leftRenderers = diagnosticsRendererSnapshotMap(leftBundle, labels.pending);
	const rightRenderers = diagnosticsRendererSnapshotMap(rightBundle, labels.pending);
	const names = new Set<string>();
	for (const name of leftRenderers.keys()) names.add(name);
	for (const name of rightRenderers.keys()) names.add(name);
	const details: DiagnosticsCompareItem[] = [];
	for (const name of names) {
		const before = leftRenderers.get(name) ?? null;
		const after = rightRenderers.get(name) ?? null;
		details.push({
			label: name,
			before: before ? diagnosticsRendererCompareLabel(before) : "missing",
			after: after ? diagnosticsRendererCompareLabel(after) : "missing",
			level: diagnosticsRendererCompareLevel(before, after),
		});
	}
	return details.sort((a, b) => a.label.localeCompare(b.label));
}

export function diagnosticsRendererInsights(
	bundle: Record<string, unknown>,
	labels: DiagnosticsRuntimeLabels
): DiagnosticsRendererInsight[] {
	return arrayField(bundle, "renderers")
		.map((renderer) => diagnosticsRendererInsight(renderer, labels))
		.sort((a, b) => diagnosticsLevelWeight(b.level) - diagnosticsLevelWeight(a.level) || a.name.localeCompare(b.name));
}

export function diagnosticsRendererInsight(
	renderer: Record<string, unknown>,
	labels: DiagnosticsRuntimeLabels
): DiagnosticsRendererInsight {
	const info = objectField(renderer, "info");
	const runtimeStatus = objectField(renderer, "runtime_status");
	const snapshot = diagnosticsRendererSnapshot(info, runtimeStatus, labels.pending);
	return {
		name: snapshot.name,
		level: diagnosticsRendererLevel(snapshot),
		state: snapshot.state,
		runtime: snapshot.connected ? labels.connected : labels.disconnected,
		frame: `FPS ${runtimeMetric(snapshot.fps)} / GPU ${runtimeMetric(snapshot.gpuMs, " ms")} / ${snapshot.surface}`,
		texture: `AA ${aaModeLabel(snapshot.aa)} / ${snapshot.texturePolicy} / ${snapshot.textureSummary}`,
		output: diagnosticsRendererSpoutLabel(snapshot),
		note: snapshot.note,
	};
}

export function objectField(value: unknown, key: string): Record<string, unknown> {
	if (!value || typeof value !== "object" || Array.isArray(value)) return {};
	const field = (value as Record<string, unknown>)[key];
	return field && typeof field === "object" && !Array.isArray(field) ? (field as Record<string, unknown>) : {};
}

export function arrayField(value: unknown, key: string): Record<string, unknown>[] {
	if (!value || typeof value !== "object" || Array.isArray(value)) return [];
	const field = (value as Record<string, unknown>)[key];
	return Array.isArray(field)
		? field.filter((item): item is Record<string, unknown> => Boolean(item) && typeof item === "object" && !Array.isArray(item))
		: [];
}

function profileLauncherSettings(profiles: Record<string, unknown>): Record<string, unknown>[] {
	const launcherSettings = arrayField(profiles, "launcher_settings");
	return launcherSettings.length > 0 ? launcherSettings : arrayField(profiles, "tray_launch_settings");
}

export function stringField(value: Record<string, unknown>, key: string): string | null {
	const field = value[key];
	return typeof field === "string" ? field : null;
}

export function numberField(value: Record<string, unknown>, key: string): number | null {
	const field = value[key];
	return typeof field === "number" && Number.isFinite(field) ? field : null;
}

export function booleanField(value: Record<string, unknown>, key: string): boolean | null {
	const field = value[key];
	return typeof field === "boolean" ? field : null;
}
