<script setup lang="ts">
// Custom title bar because the window is undecorated (decorations: false).
import { getCurrentWindow } from "@tauri-apps/api/window";

const win = getCurrentWindow();
defineProps<{ speed: string }>();
</script>

<template>
  <header class="bar" data-tauri-drag-region>
    <div class="brand" data-tauri-drag-region>
      <span class="mark">V</span>
      <span class="name">VELO</span>
    </div>

    <div class="speed mono" v-if="speed">
      <span class="dot" />{{ speed }}
    </div>

    <div class="controls">
      <button class="ctl" title="Minimize" @click="win.minimize()">&#8211;</button>
      <button class="ctl" title="Maximize" @click="win.toggleMaximize()">&#9633;</button>
      <button class="ctl close" title="Close" @click="win.close()">&#10005;</button>
    </div>
  </header>
</template>

<style scoped>
.bar {
  height: var(--titlebar-h);
  display: flex;
  align-items: center;
  gap: 16px;
  padding-left: 12px;
  background: var(--surface);
  border-bottom: 1px solid var(--border);
  flex: 0 0 auto;
}
.brand {
  display: flex;
  align-items: center;
  gap: 8px;
}
.mark {
  width: 18px;
  height: 18px;
  display: grid;
  place-items: center;
  background: var(--yellow);
  color: var(--on-accent);
  font-weight: 700;
  font-size: 13px;
  clip-path: polygon(0 0, calc(100% - 5px) 0, 100% 5px, 100% 100%, 5px 100%, 0 calc(100% - 5px));
}
.name {
  font-weight: 700;
  letter-spacing: 0.22em;
  font-size: 13px;
  color: var(--text);
}
.speed {
  font-size: 12px;
  color: var(--yellow);
  display: flex;
  align-items: center;
  gap: 6px;
}
.dot {
  width: 6px;
  height: 6px;
  background: var(--yellow);
  animation: pulse 1s steps(2) infinite;
}
@keyframes pulse {
  50% { opacity: 0.2; }
}
.controls {
  margin-left: auto;
  display: flex;
  height: 100%;
}
.ctl {
  width: 44px;
  background: transparent;
  border: none;
  color: var(--text-dim);
  font-size: 13px;
  cursor: pointer;
  transition: background var(--fast) linear, color var(--fast) linear;
}
.ctl:hover {
  background: var(--surface-3);
  color: var(--text);
}
.ctl.close:hover {
  background: var(--magenta);
  color: #fff;
}
</style>
