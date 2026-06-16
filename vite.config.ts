import { defineConfig } from "vite";
import { resolve } from "path";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // src-tauri is handled by cargo's own watcher. The asset/reference dirs
      // aren't build inputs and can hold files locked by other tools (e.g.
      // image editors), which crashes Vite's FSWatcher with EBUSY — ignore them.
      ignored: [
        "**/src-tauri/**",
        "**/_heroart/**",
        "**/docs/**",
      ],
    },
  },
  build: {
    rollupOptions: {
      input: {
        main: resolve(__dirname, "index.html"),
        settings: resolve(__dirname, "settings.html"),
        scan: resolve(__dirname, "scan.html"),
      },
    },
  },
}));
