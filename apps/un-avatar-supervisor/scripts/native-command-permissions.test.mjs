import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

// Cross-file contract: a newly registered desktop command must actually be
// callable by the local main window, rather than silently failing at runtime.
const entry = await readFile(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
const permissions = await readFile(new URL("../src-tauri/permissions/supervisor-invoke.toml", import.meta.url), "utf8");
const capability = JSON.parse(await readFile(new URL("../src-tauri/capabilities/main.json", import.meta.url), "utf8"));
function registeredCommands(text) {
 const match = text.match(/^\s*\.invoke_handler\(tauri::generate_handler!\[([\s\S]*?)\]\)/m);
 assert.ok(match, "Cannot locate the actual Tauri handler; update contract parser explicitly");
 return match[1].replace(/\/\/[^\n]*/g, "").split(",").map(s => s.trim()).filter(Boolean).map(s => {
  assert.match(s, /^(?:[a-z_][a-z0-9_]*::)*[a-z_][a-z0-9_]*$/i);
  return s.split("::").at(-1);
 });
}
function commandList(key) {
 const match = permissions.match(new RegExp(`commands\\.${key}\\s*=\\s*\\[([^\\]]*)\\]`));
 return match ? [...match[1].matchAll(/"([a-z][a-z0-9_]+)"/g)].map(m => m[1]) : [];
}
const allowed = new Set(commandList("allow"));
const denied = new Set(commandList("deny"));

test("every registered command is permitted for the local main window", () => {
 assert.ok(allowed.size > 0, "Permission list was not parsed");
 assert.ok(capability.windows.includes("main"));
 assert.ok(capability.permissions.includes("supervisor-invoke"));
 assert.notEqual(capability.local, false);
 assert.deepEqual(registeredCommands(entry).filter(c => !allowed.has(c) || denied.has(c)), []);
});

test("registration parser checks qualified command names without treating modules as commands", () => {
 assert.deepEqual(registeredCommands(" .invoke_handler(tauri::generate_handler![plain, crate::i18n::localized,])"), ["plain", "localized"]);
});
