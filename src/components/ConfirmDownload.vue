<script setup lang="ts">
import { computed, ref } from "vue";
import { api, humanBytes, type ConfirmRequest } from "../api";

const props = defineProps<{ req: ConfirmRequest }>();
const emit = defineEmits<{ (e: "done"): void }>();

const always = ref(false);
const busy = ref(false);
const showUrl = ref(false);

const ext = computed(() => {
  const dot = props.req.file_name.lastIndexOf(".");
  return dot > 0 ? props.req.file_name.slice(dot + 1).toUpperCase() : "FILE";
});

const size = computed(() => {
  if (!props.req.probed) return "checking…";
  return props.req.total ? humanBytes(props.req.total) : "unknown size";
});

async function answer(start: boolean) {
  busy.value = true;
  try {
    if (start && always.value) await api.setConfirmDownloads(false);
    await api.confirmDownload(props.req.id, start);
  } finally {
    busy.value = false;
    emit("done");
  }
}
</script>

<template>
  <div class="scrim">
    <div class="panel dlg">
      <div class="title label">Download this file?</div>

      <div class="fileline">
        <span class="tag">{{ ext }}</span>
        <span class="file">{{ req.file_name }}</span>
      </div>

      <div class="facts">
        <span class="fact mono" :class="{ dim: !req.probed }">{{ size }}</span>
        <span class="sep" v-if="req.mime">·</span>
        <span class="fact mono" v-if="req.mime">{{ req.mime }}</span>
        <span class="sep" v-if="req.probed">·</span>
        <span class="fact mono" v-if="req.probed">
          {{ req.resumable ? "resumable" : "not resumable" }}
        </span>
      </div>

      <button class="urltoggle label" @click="showUrl = !showUrl">
        {{ showUrl ? "Hide link" : "Show link" }}
      </button>
      <div class="url mono" v-if="showUrl">{{ req.url }}</div>

      <div class="dest">
        <span class="label">Saving to</span>
        <span class="path mono">{{ req.out_dir }}</span>
      </div>

      <label class="always">
        <input v-model="always" type="checkbox" />
        <span>Start browser downloads without asking</span>
      </label>

      <div class="actions">
        <button class="btn btn-danger" :disabled="busy" @click="answer(false)">Cancel</button>
        <button class="btn btn-primary" :disabled="busy" @click="answer(true)">Start</button>
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
  z-index: 150;
}
.dlg {
  width: 480px;
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

.fileline {
  display: flex;
  align-items: flex-start;
  gap: 9px;
}
.tag {
  flex: 0 0 auto;
  font-family: var(--font-mono);
  font-size: 10px;
  letter-spacing: 0.05em;
  color: var(--on-accent);
  background: var(--yellow);
  padding: 3px 7px;
  margin-top: 2px;
}
.file {
  font-size: 15px;
  color: var(--text);
  word-break: break-all;
  line-height: 1.4;
}

.facts {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  font-size: 11px;
  color: var(--cyan);
}
.fact.dim {
  color: var(--text-faint);
}
.sep {
  color: var(--text-faint);
}

.urltoggle {
  align-self: flex-start;
  background: none;
  border: none;
  color: var(--text-dim);
  cursor: pointer;
  padding: 0;
  text-decoration: underline;
}
.urltoggle:hover {
  color: var(--cyan);
}
.url {
  font-size: 10px;
  color: var(--text-faint);
  background: var(--surface-2);
  border: 1px solid var(--border);
  padding: 7px 9px;
  max-height: 76px;
  overflow-y: auto;
  word-break: break-all;
  line-height: 1.5;
  user-select: text;
}

.dest {
  display: flex;
  flex-direction: column;
  gap: 4px;
  background: var(--surface-2);
  border: 1px solid var(--border);
  padding: 8px 10px;
}
.path {
  font-size: 11px;
  color: var(--cyan);
  word-break: break-all;
}

.always {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--text-dim);
  cursor: pointer;
}
.always input {
  accent-color: var(--yellow);
}
.actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 4px;
}
</style>
