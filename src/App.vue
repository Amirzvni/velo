<script setup lang="ts">
import { onMounted, ref, computed } from "vue";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  api,
  humanBytes,
  type ConfirmRequest,
  type DownloadRow,
  type PairRequest,
  type Status,
} from "./api";
import { useDownloads } from "./store";
import TitleBar from "./components/TitleBar.vue";
import Sidebar from "./components/Sidebar.vue";
import DownloadItem from "./components/DownloadItem.vue";
import AddDialog from "./components/AddDialog.vue";
import SettingsDialog from "./components/SettingsDialog.vue";
import PairPrompt from "./components/PairPrompt.vue";
import ConfirmDownload from "./components/ConfirmDownload.vue";

const store = useDownloads();
const showAdd = ref(false);
const showSettings = ref(false);
const pairReq = ref<PairRequest | null>(null);
// Browser downloads waiting for a yes. A queue, so a burst of links does not
// drop prompts on the floor.
const confirmQueue = ref<ConfirmRequest[]>([]);

const speedLabel = computed(() =>
  store.totalSpeed > 0 ? `${humanBytes(store.totalSpeed)}/s` : "",
);

onMounted(async () => {
  const sl = localStorage.getItem("velo.scanlines") === "off" ? "off" : "on";
  document.getElementById("app")?.setAttribute("data-scanlines", sl);
  await store.init();

  await api.onEvent((e) => {
    if (e.type === "confirm") {
      confirmQueue.value.push({
        id: e.id,
        url: e.url,
        file_name: e.file_name,
        out_dir: e.out_dir,
        total: null,
        mime: null,
        resumable: false,
        probed: false,
      });
    } else if (e.type === "confirm_details") {
      // The probe came back; fill in the real name and size in place.
      const q = confirmQueue.value.find((c) => c.id === e.id);
      if (q) {
        q.file_name = e.file_name;
        q.total = e.total;
        q.mime = e.mime;
        q.resumable = e.resumable;
        q.probed = true;
      }
    }
  });

  // A browser may have asked to pair before the window finished loading.
  pairReq.value = await api.pendingPairing();
  await api.onPairRequest((r) => (pairReq.value = r));
});

function select(f: "all" | Status) {
  store.filter = f;
}

async function openFile(row: DownloadRow) {
  if (row.path) await revealItemInDir(row.path);
}
</script>

<template>
  <div class="shell">
    <TitleBar :speed="speedLabel" />

    <div class="body">
      <Sidebar
        :active="store.filter"
        :counts="store.counts"
        @select="select"
        @settings="showSettings = true"
      />

      <main class="main">
        <div class="toolbar">
          <button class="btn btn-primary" @click="showAdd = true">+ Add download</button>
          <span class="label spacer" v-if="store.settings">
            {{ store.settings.max_concurrent }} at a time
          </span>
        </div>

        <div class="list">
          <div v-if="store.loading" class="empty label">Loading…</div>

          <div v-else-if="!store.visible.length" class="empty">
            <div class="big">NO DOWNLOADS</div>
            <div class="label">Paste a link to get started</div>
          </div>

          <DownloadItem
            v-for="row in store.visible"
            :key="row.id"
            :row="row"
            :speed="store.live[row.id]?.bytesPerSec ?? 0"
            :segments="store.live[row.id]?.segments ?? 0"
            @pause="api.pause"
            @resume="api.resume"
            @remove="(id) => api.remove(id, false)"
            @open="openFile"
          />
        </div>
      </main>
    </div>

    <AddDialog
      v-if="showAdd && store.settings"
      :default-dir="store.settings.default_dir"
      @close="showAdd = false"
      @added="store.refresh()"
    />
    <ConfirmDownload
      v-if="confirmQueue.length"
      :key="confirmQueue[0].id"
      :req="confirmQueue[0]"
      @done="confirmQueue.shift()"
    />
    <PairPrompt
      v-if="pairReq"
      :browser="pairReq.browser"
      :extension-id="pairReq.extension_id"
      @done="pairReq = null"
    />
    <SettingsDialog
      v-if="showSettings && store.settings"
      :settings="store.settings"
      @close="showSettings = false"
      @saved="(s) => (store.settings = s)"
    />
  </div>
</template>

<style scoped>
.shell {
  display: flex;
  flex-direction: column;
  height: 100%;
}
.body {
  flex: 1;
  display: flex;
  min-height: 0;
}
.main {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
}
.toolbar {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 16px;
  border-bottom: 1px solid var(--border);
  flex: 0 0 auto;
}
.spacer {
  margin-left: auto;
}
.list {
  flex: 1;
  overflow-y: auto;
}
.empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  height: 100%;
  min-height: 260px;
  color: var(--text-faint);
}
.big {
  font-size: 22px;
  font-weight: 700;
  letter-spacing: 0.2em;
  color: var(--surface-3);
}
</style>
