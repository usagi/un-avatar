/**
 * UN Avatar Supervisor の WebView2 を CDP 経由で観測する。
 * 事前: プロセス起動時に
 * WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222
 * を付与すること。
 */
import { chromium } from "playwright";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const outDir = path.resolve(__dirname, "..", "..", "..", "output", "playwright");
fs.mkdirSync(outDir, { recursive: true });

const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
const report = { contexts: browser.contexts().length, pages: [] };

for (const ctx of browser.contexts()) {
  for (const page of ctx.pages()) {
    const consoleMessages = [];
    const errors = [];
    page.on("console", (msg) =>
      consoleMessages.push({ type: msg.type(), text: msg.text() }),
    );
    page.on("pageerror", (e) => errors.push(String(e)));

    const url = page.url();
    const title = await page.title().catch(() => "");
    await page.waitForTimeout(1500);
    const appHtml =
      (await page.locator("#app").innerHTML().catch(() => "<#app missing>")) ?? "";

    const shot = path.join(outDir, "un-avatar-tauri-webview.png");
    await page.screenshot({ path: shot, fullPage: true }).catch((e) => {
      errors.push(String(e));
    });

    report.pages.push({
      url,
      title,
      appInnerLength: appHtml.length,
      appInnerPreview: appHtml.slice(0, 400),
      screenshot: shot,
      consoleTail: consoleMessages.slice(-15),
      errors,
    });
  }
}

await browser.close();

const reportPath = path.join(outDir, "un-avatar-webview-report.json");
fs.writeFileSync(reportPath, JSON.stringify(report, null, 2), "utf8");
console.log(JSON.stringify(report, null, 2));
