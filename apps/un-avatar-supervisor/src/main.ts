import "./styles.css";
import App from "./App.svelte";
import { mount } from "svelte";
import { waitLocale } from "svelte-i18n";
import { setupI18n } from "@usagi.network/un-i18n-svelte";
import { installDevIpcMock } from "./dev-ipc-mock";

// i18n bundle (ja-JP / en-US) を svelte-i18n に register し、初期 locale を確定するまで
// マウントを遅延する。これにより `$_(key)` が最初のレンダリングから正しい言語を返す。
// `init()` 直後はローダー非同期のため locale store がまだ null の間があり、本番バンドルでは
// ここで `$_()` が先に走って例外 → 白画面になる。`waitLocale()` で辞書ロード完了を待つ。
installDevIpcMock();
await setupI18n();
await waitLocale();

const app = mount(App, {
	target: document.getElementById("app") as HTMLElement,
});

export default app;
