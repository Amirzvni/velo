<script setup lang="ts">
import { ref } from "vue";
import { api, type Settings } from "../api";

const props = defineProps<{ settings: Settings }>();
const emit = defineEmits<{ (e: "close"): void; (e: "saved", s: Settings): void }>();

const concurrent = ref(props.settings.max_concurrent);
const scanlines = ref(localStorage.getItem("velo.scanlines") !== "off");

async function save() {
  await api.setMaxConcurrent(concurrent.value);
  localStorage.setItem("velo.scanlines", scanlines.value ? "on" : "off");
  document.getElementById("app")?.setAttribute("data-scanlines", scanlines.value ? "on" : "off");
  emit("saved", { ...props.settings, max_concurrent: concurrent.value });
  emit("close");
}
</script>

<template>
  <div class="scrim" @click.self="emit('close')">
    <div class="panel dlg">
      <div class="title label">Settings</div>

      <label class="field">
        <span class="label">Downloads at the same time</span>
        <input v-model.number="concurrent" class="input" type="number" min="1" max="16" />
        <span class="note">The rest wait in the queue and start automatically.</span>
      </label>

      <label class="field">
        <span class="label">Save files to</span>
        <input :value="settings.default_dir" class="input" readonly />
      </label>

      <label class="check">
        <input v-model="scanlines" type="checkbox" />
        <span>Scanline effect</span>
      </label>

      <div class="actions">
        <button class="btn" @click="emit('close')">Cancel</button>
        <button class="btn btn-primary" @click="save">Save</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.scrim {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.72);
  display: grid;
  place-items: center;
  z-index: 100;
}
.dlg {
  width: 460px;
  max-width: 92vw;
  padding: 20px;
  display: flex;
  flex-direction: column;
  gap: 16px;
  border-color: var(--border-hot);
}
.title {
  color: var(--yellow);
  font-size: 12px;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.note {
  font-size: 11px;
  color: var(--text-faint);
}
.check {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  color: var(--text-dim);
  cursor: pointer;
}
.check input {
  accent-color: var(--yellow);
}
.actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
</style>
