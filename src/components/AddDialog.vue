<script setup lang="ts">
import { ref, computed } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../api";

const props = defineProps<{ defaultDir: string }>();
const emit = defineEmits<{ (e: "close"): void; (e: "added"): void }>();

const text = ref("");
const dir = ref(props.defaultDir);
const busy = ref(false);
const error = ref("");

// One url per line, so paste-a-list works without a separate screen.
const urls = computed(() =>
  text.value
    .split(/\s+/)
    .map((s) => s.trim())
    .filter((s) => /^https?:\/\//i.test(s)),
);

async function pickDir() {
  const picked = await open({ directory: true, defaultPath: dir.value });
  if (typeof picked === "string") dir.value = picked;
}

async function submit() {
  if (!urls.value.length) {
    error.value = "Paste at least one http or https link.";
    return;
  }
  busy.value = true;
  error.value = "";
  try {
    if (urls.value.length === 1) {
      await api.add({ url: urls.value[0], out_dir: dir.value });
    } else {
      await api.addBatch(
        `${urls.value.length} links`,
        null,
        urls.value.map((u) => ({ url: u, out_dir: dir.value })),
      );
    }
    emit("added");
    emit("close");
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <div class="scrim" @click.self="emit('close')">
    <div class="panel dlg">
      <div class="title label">Add download</div>

      <textarea
        v-model="text"
        class="input area"
        rows="5"
        placeholder="https://example.com/file.zip&#10;paste more links, one per line"
        autofocus
      />

      <div class="dirline">
        <input v-model="dir" class="input" spellcheck="false" />
        <button class="btn" @click="pickDir">Browse</button>
      </div>

      <div class="hint label" v-if="urls.length > 1">
        {{ urls.length }} links — they will download 4 at a time
      </div>
      <div class="err" v-if="error">{{ error }}</div>

      <div class="actions">
        <button class="btn" @click="emit('close')">Cancel</button>
        <button class="btn btn-primary" :disabled="busy || !urls.length" @click="submit">
          {{ urls.length > 1 ? `Add ${urls.length}` : "Add" }}
        </button>
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
  width: 540px;
  max-width: 92vw;
  padding: 20px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  border-color: var(--border-hot);
}
.title {
  color: var(--yellow);
  font-size: 12px;
}
.area {
  resize: vertical;
  min-height: 96px;
  line-height: 1.5;
}
.dirline {
  display: flex;
  gap: 8px;
}
.hint {
  color: var(--cyan);
}
.err {
  color: var(--magenta);
  font-size: 12px;
  font-family: var(--font-mono);
}
.actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 4px;
}
</style>
