<script setup lang="ts">
import { computed } from "vue";
import { humanBytes, humanTime, type DownloadRow } from "../api";

const props = defineProps<{
  row: DownloadRow;
  speed: number;
  segments: number;
}>();
const emit = defineEmits<{
  (e: "pause", id: number): void;
  (e: "resume", id: number): void;
  (e: "remove", id: number): void;
  (e: "open", row: DownloadRow): void;
}>();

const pct = computed(() => {
  if (!props.row.total_size) return 0;
  return Math.min(100, (props.row.downloaded / props.row.total_size) * 100);
});

const eta = computed(() => {
  if (props.row.status !== "running" || !props.row.total_size || props.speed <= 0) return "";
  return humanTime((props.row.total_size - props.row.downloaded) / props.speed);
});

const label = computed(() => {
  switch (props.row.status) {
    case "running":
      return `${humanBytes(props.speed)}/s · ${props.segments} conn${eta.value ? " · " + eta.value + " left" : ""}`;
    case "queued":
      return "Waiting for a free slot";
    case "paused":
      return "Paused";
    case "completed":
      return "Finished";
    case "failed":
      return props.row.error ?? "Failed";
  }
});
</script>

<template>
  <div class="row" :class="row.status">
    <div class="head">
      <span class="dot" />
      <span class="name" :title="row.url">{{ row.file_name }}</span>
      <span class="size mono">
        {{ humanBytes(row.downloaded) }} / {{ humanBytes(row.total_size) }}
      </span>
    </div>

    <div class="track">
      <div class="fill" :style="{ width: pct + '%' }" />
    </div>

    <div class="foot">
      <span class="meta mono">{{ label }}</span>
      <span class="pct mono">{{ pct.toFixed(0) }}%</span>

      <div class="acts">
        <button v-if="row.status === 'running'" class="act" @click="emit('pause', row.id)">Pause</button>
        <button
          v-if="row.status === 'paused' || row.status === 'failed'"
          class="act"
          @click="emit('resume', row.id)"
        >
          Resume
        </button>
        <button v-if="row.status === 'completed'" class="act" @click="emit('open', row)">Open</button>
        <button class="act danger" @click="emit('remove', row.id)">Remove</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.row {
  padding: 12px 16px;
  border-bottom: 1px solid var(--border);
  background: var(--bg);
  transition: background var(--fast) linear;
}
.row:hover {
  background: var(--surface);
}
.head {
  display: flex;
  align-items: center;
  gap: 9px;
  margin-bottom: 8px;
}
.dot {
  width: 7px;
  height: 7px;
  flex: 0 0 auto;
  background: var(--text-faint);
}
.running .dot { background: var(--yellow); animation: blink 1s steps(2) infinite; }
.completed .dot { background: var(--green); }
.failed .dot { background: var(--magenta); }
.paused .dot { background: var(--orange); }
@keyframes blink { 50% { opacity: 0.25; } }

.name {
  font-size: 14px;
  font-weight: 600;
  color: var(--text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.size {
  margin-left: auto;
  font-size: 11px;
  color: var(--text-dim);
  flex: 0 0 auto;
}

.track {
  height: 4px;
  background: var(--surface-3);
  overflow: hidden;
}
.fill {
  height: 100%;
  background: var(--yellow);
  transition: width 200ms linear;
}
.completed .fill { background: var(--green); }
.failed .fill { background: var(--magenta); }
.paused .fill { background: var(--orange); }

.foot {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-top: 7px;
  min-height: 22px;
}
.meta {
  font-size: 11px;
  color: var(--text-dim);
}
.pct {
  font-size: 11px;
  color: var(--text-faint);
}
.acts {
  margin-left: auto;
  display: flex;
  gap: 4px;
  opacity: 0;
  transition: opacity var(--fast) linear;
}
.row:hover .acts {
  opacity: 1;
}
.act {
  font-family: var(--font-ui);
  font-size: 11px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  padding: 4px 10px;
  background: transparent;
  color: var(--text-dim);
  border: 1px solid var(--border);
  cursor: pointer;
  transition: color var(--fast) linear, border-color var(--fast) linear;
}
.act:hover {
  color: var(--yellow);
  border-color: var(--yellow);
}
.act.danger:hover {
  color: var(--magenta);
  border-color: var(--magenta);
}
</style>
