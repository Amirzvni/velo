# Velo

Open-source download manager for Windows. Fast, resumable, beginner friendly.

- Multi-segment downloading with dynamic segment stealing
- Browser integration: downloads start in Velo instead of the browser
- Grab all links on a page, queue them, 4 running at a time
- Cyberpunk UI theme

## Stack
Rust engine (`crates/velo-core`) - Tauri v2 shell - Vue 3 + TypeScript UI - SQLite - MV3 browser extension.

## Build (Windows)
Requires: Rust (MSVC toolchain), Node 20+, pnpm, WebView2 runtime.

    pnpm install
    pnpm tauri dev

## License
MIT
