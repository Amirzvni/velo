// Client-side view of the download list. The Rust side is the source of truth;
// this just keeps a fast copy so the UI does not re-query on every event.
import { defineStore } from "pinia";
import { api, type DownloadRow, type Settings, type Status, type VeloEvent } from "./api";

interface Live {
  bytesPerSec: number;
  segments: number;
}

export const useDownloads = defineStore("downloads", {
  state: () => ({
    rows: [] as DownloadRow[],
    live: {} as Record<number, Live>,
    settings: null as Settings | null,
    loading: true,
    filter: "all" as "all" | Status,
  }),

  getters: {
    visible(state): DownloadRow[] {
      if (state.filter === "all") return state.rows;
      return state.rows.filter((r) => r.status === state.filter);
    },
    counts(state) {
      const c: Record<string, number> = { all: state.rows.length };
      for (const r of state.rows) c[r.status] = (c[r.status] ?? 0) + 1;
      return c;
    },
    totalSpeed(state): number {
      return Object.values(state.live).reduce((a, l) => a + l.bytesPerSec, 0);
    },
  },

  actions: {
    async init() {
      this.settings = await api.settings();
      await this.refresh();
      this.loading = false;
      await api.onEvent((e) => this.apply(e));
    },

    async refresh() {
      this.rows = await api.list();
    },

    row(id: number) {
      return this.rows.find((r) => r.id === id);
    },

    apply(e: VeloEvent) {
      switch (e.type) {
        case "progress": {
          const r = this.row(e.id);
          if (r) {
            r.downloaded = e.downloaded;
            if (e.total !== null) r.total_size = e.total;
            r.status = "running";
          }
          this.live[e.id] = { bytesPerSec: e.bytes_per_sec, segments: e.segments };
          break;
        }
        case "started": {
          const r = this.row(e.id);
          if (r) {
            r.file_name = e.file_name;
            r.total_size = e.total;
            r.status = "running";
          } else {
            void this.refresh();
          }
          break;
        }
        case "finished": {
          const r = this.row(e.id);
          if (r) {
            r.status = "completed";
            r.path = e.path;
            if (r.total_size) r.downloaded = r.total_size;
          }
          delete this.live[e.id];
          break;
        }
        case "failed": {
          const r = this.row(e.id);
          if (r) {
            r.status = "failed";
            r.error = e.error;
          }
          delete this.live[e.id];
          break;
        }
        case "paused": {
          const r = this.row(e.id);
          if (r) r.status = "paused";
          delete this.live[e.id];
          break;
        }
        case "queued":
          void this.refresh();
          break;
        case "removed":
          this.rows = this.rows.filter((r) => r.id !== e.id);
          delete this.live[e.id];
          break;
        case "confirm":
          // App.vue owns the prompt; the row appears once it is accepted.
          break;
      }
    },
  },
});
