import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// Tauri serves the UI from a fixed port in dev and from disk in release.
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    port: 5183,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**", "**/crates/**", "**/target/**"] },
  },
  build: {
    target: "chrome110",
    minify: "esbuild",
    sourcemap: false,
  },
});
