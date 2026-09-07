<script setup lang="ts">
import type { Status } from "../api";

defineProps<{
  active: "all" | Status;
  counts: Record<string, number>;
}>();
const emit = defineEmits<{ (e: "select", v: "all" | Status): void; (e: "settings"): void }>();

const items: { key: "all" | Status; label: string }[] = [
  { key: "all", label: "All" },
  { key: "running", label: "Downloading" },
  { key: "queued", label: "Queued" },
  { key: "paused", label: "Paused" },
  { key: "completed", label: "Finished" },
  { key: "failed", label: "Failed" },
];
</script>

<template>
  <nav class="side">
    <button
      v-for="it in items"
      :key="it.key"
      class="item"
      :class="{ on: active === it.key }"
      @click="emit('select', it.key)"
    >
      <span class="txt">{{ it.label }}</span>
      <span class="num mono">{{ counts[it.key] ?? 0 }}</span>
    </button>

    <button class="item settings" @click="emit('settings')">
      <span class="txt">Settings</span>
    </button>
  </nav>
</template>

<style scoped>
.side {
  width: var(--sidebar-w);
  flex: 0 0 auto;
  background: var(--surface);
  border-right: 1px solid var(--border);
  padding: 12px 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 16px;
  background: transparent;
  border: none;
  border-left: 2px solid transparent;
  color: var(--text-dim);
  font-family: var(--font-ui);
  font-size: 13px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.07em;
  cursor: pointer;
  transition: color var(--fast) linear, background var(--fast) linear;
}
.item:hover {
  background: var(--surface-2);
  color: var(--text);
}
.item.on {
  color: var(--yellow);
  border-left-color: var(--yellow);
  background: var(--surface-2);
}
.num {
  font-size: 11px;
  color: var(--text-faint);
}
.item.on .num {
  color: var(--yellow);
}
.settings {
  margin-top: auto;
}
</style>
