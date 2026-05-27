import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  // Tauri 本番 (custom protocol) では `/assets/...` 絶対パスが解決されず白画面になるため相対にする
  base: "./",
  plugins: [svelte()],
  clearScreen: false,
  server: {
    strictPort: true,
    host: "127.0.0.1",
    port: 1420,
  },
  envPrefix: ["VITE_", "TAURI_"],
});
