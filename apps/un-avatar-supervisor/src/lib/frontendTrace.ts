import { invoke } from "@tauri-apps/api/core";

let seq = 0;
let lastTick = typeof performance === "undefined" ? 0 : performance.now();
const slowTraceThresholdMs = 150;
const traceRingCapacity = 256;
const traceRing: string[] = [];

function keepTrace(payload: string): void {
	traceRing.push(payload);
	if (traceRing.length > traceRingCapacity) traceRing.splice(0, traceRing.length - traceRingCapacity);
	if (typeof window !== "undefined") {
		(window as unknown as { __unAvatarTrace?: readonly string[] }).__unAvatarTrace = traceRing;
	}
}

function shouldPersistTrace(name: string, detail: Record<string, unknown>): boolean {
	return (
		name.endsWith(":error") ||
		name === "event-loop-lag" ||
		name === "native-startup-timing" ||
		(typeof detail.elapsedMs === "number" && detail.elapsedMs >= slowTraceThresholdMs)
	);
}

function targetSummary(target: EventTarget | null): string {
	if (!(target instanceof HTMLElement)) return "";
	const role = target.getAttribute("role") ?? "";
	const aria = target.getAttribute("aria-label") ?? "";
	const text = (target.textContent ?? "").replace(/\s+/g, " ").trim().slice(0, 80);
	const id = target.id ? `#${target.id}` : "";
	const cls = target.className && typeof target.className === "string" ? `.${target.className.replace(/\s+/g, ".")}` : "";
	return [target.tagName.toLowerCase() + id + cls, role && `role=${role}`, aria && `aria=${aria}`, text && `text=${text}`]
		.filter(Boolean)
		.join(" ");
}

export function traceFrontendEvent(name: string, detail: Record<string, unknown> = {}): void {
	if (!("__TAURI_INTERNALS__" in window)) return;
	const at = typeof performance === "undefined" ? 0 : Math.round(performance.now());
	const payload = JSON.stringify({ seq: ++seq, at, name, ...detail });
	keepTrace(payload);
	if (shouldPersistTrace(name, detail)) {
		void invoke("log_frontend_error", { message: `trace: ${payload}` }).catch(() => undefined);
	}
}

export async function traceAsync<T>(name: string, run: () => Promise<T>, detail: Record<string, unknown> = {}): Promise<T> {
	traceFrontendEvent(`${name}:start`, detail);
	const started = typeof performance === "undefined" ? 0 : performance.now();
	try {
		const value = await run();
		const elapsedMs = typeof performance === "undefined" ? null : Math.round(performance.now() - started);
		if (elapsedMs == null || elapsedMs >= slowTraceThresholdMs) {
			traceFrontendEvent(`${name}:ok`, { ...detail, elapsedMs });
		}
		return value;
	} catch (error) {
		const elapsedMs = typeof performance === "undefined" ? null : Math.round(performance.now() - started);
		traceFrontendEvent(`${name}:error`, { ...detail, elapsedMs, error: String(error) });
		throw error;
	}
}

export function installFrontendTrace(): void {
	if (typeof window === "undefined" || (window as unknown as { __unAvatarTraceInstalled?: boolean }).__unAvatarTraceInstalled) {
		return;
	}
	(window as unknown as { __unAvatarTraceInstalled?: boolean }).__unAvatarTraceInstalled = true;
	traceFrontendEvent("trace-installed", { href: window.location.href });
	document.addEventListener(
		"click",
		(event) => {
			traceFrontendEvent("dom-click", { target: targetSummary(event.target) });
		},
		true
	);
	window.setInterval(() => {
		const now = performance.now();
		const lagMs = Math.round(now - lastTick - 1000);
		lastTick = now;
		if (lagMs > 300) {
			traceFrontendEvent("event-loop-lag", { lagMs });
		}
	}, 1000);
}
