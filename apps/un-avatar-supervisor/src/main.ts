import "./styles.css";
import App from "./App.svelte";
import { mount } from "svelte";
import { invoke } from "@tauri-apps/api/core";
import { waitLocale } from "svelte-i18n";
import { setupI18n } from "@usagi.network/un-i18n-svelte";

function stringifyFrontendError(value: unknown): string {
	if (value instanceof Error) return `${value.name}: ${value.message}\n${value.stack ?? ""}`.trim();
	if (typeof value === "string") return value;
	try {
		return JSON.stringify(value);
	} catch {
		return String(value);
	}
}

function reportFrontendError(kind: string, value: unknown): void {
	if (!("__TAURI_INTERNALS__" in window)) return;
	void invoke("log_frontend_error", {
		message: `${kind}: ${stringifyFrontendError(value)}`,
	}).catch(() => undefined);
}

window.addEventListener("error", (event) => {
	reportFrontendError("error", event.error ?? event.message);
});
window.addEventListener("unhandledrejection", (event) => {
	reportFrontendError("unhandledrejection", event.reason);
});

// i18n bundle (ja-JP / en-US) を svelte-i18n に register し、初期 locale を確定するまで
// マウントを遅延する。これにより `$_(key)` が最初のレンダリングから正しい言語を返す。
// `init()` 直後はローダー非同期のため locale store がまだ null の間があり、本番バンドルでは
// ここで `$_()` が先に走って例外 → 白画面になる。`waitLocale()` で辞書ロード完了を待つ。
if (import.meta.env.DEV) {
	const { installDevIpcMock } = await import("./dev-ipc-mock");
	installDevIpcMock();
}
await setupI18n();
await waitLocale();

const app = mount(App, {
	target: document.getElementById("app") as HTMLElement,
});

export default app;
