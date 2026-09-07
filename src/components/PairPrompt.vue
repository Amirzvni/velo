<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";

const props = defineProps<{ browser: string; extensionId: string }>();
const emit = defineEmits<{ (e: "done"): void }>();

async function answer(allow: boolean) {
  await invoke("answer_pairing", { allow });
  emit("done");
}
</script>

<template>
  <div class="scrim">
    <div class="panel dlg">
      <div class="title label">Connect browser</div>

      <p class="body">
        <b>{{ props.browser }}</b> wants to send downloads to Velo.
      </p>
      <p class="sub">
        Allow this only if you just installed the Velo extension yourself.
      </p>
      <p class="id mono">{{ props.extensionId }}</p>

      <div class="actions">
        <button class="btn btn-danger" @click="answer(false)">Deny</button>
        <button class="btn btn-primary" @click="answer(true)">Allow</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.scrim {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.8);
  display: grid;
  place-items: center;
  z-index: 200;
}
.dlg {
  width: 420px;
  max-width: 92vw;
  padding: 22px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  border-color: var(--border-hot);
}
.title {
  color: var(--yellow);
  font-size: 12px;
}
.body {
  font-size: 15px;
  line-height: 1.5;
}
.sub {
  font-size: 12px;
  color: var(--text-dim);
  line-height: 1.5;
}
.id {
  font-size: 10px;
  color: var(--text-faint);
  word-break: break-all;
  background: var(--surface-2);
  border: 1px solid var(--border);
  padding: 6px 8px;
}
.actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 6px;
}
</style>
