// The only place that talks to the Rust side. Everything else imports from here.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type Status = "queued" | "running" | "paused" | "completed" | "failed";

export interface DownloadRow {
  id: number;
  url: string;
  final_url: string | null;
  file_name: string;
  out_dir: string;
  path: string | null;
  total_size: number | null;
  downloaded: number;
  supports_range: boolean;
  segments: number;
  headers: Record<string, string>;
  mime: string | null;
  status: Status;
  error: string | null;
  batch_id: number | null;
  scheduled_at: number | null;
  created_at: number;
  completed_at: number | null;
}

export interface Settings {
  max_concurrent: number;
  default_dir: string;
  default_segments: number;
}

export interface AddRequest {
  url: string;
  out_dir?: string;
  file_name?: string;
  headers?: Record<string, string>;
  segments?: number;
  scheduled_at?: number;
}

export type VeloEvent =
  | { type: "progress"; id: number; downloaded: number; total: number | null; bytes_per_sec: number; segments: number }
  | { type: "started"; id: number; file_name: string; total: number | null }
  | { type: "finished"; id: number; path: string }
  | { type: "failed"; id: number; error: string }
  | { type: "paused"; id: number }
  | { type: "queued"; id: number }
  | { type: "removed"; id: number };

export const api = {
  add: (req: AddRequest) => invoke<number>("add_download", { req }),
  addBatch: (name: string, sourcePage: string | null, items: AddRequest[]) =>
    invoke<number[]>("add_batch", { name, sourcePage, items }),
  list: (filter?: Status) => invoke<DownloadRow[]>("list_downloads", { filter: filter ?? null }),
  pause: (id: number) => invoke<void>("pause_download", { id }),
  resume: (id: number) => invoke<void>("resume_download", { id }),
  remove: (id: number, deleteFile: boolean) =>
    invoke<void>("remove_download", { id, deleteFile }),
  settings: () => invoke<Settings>("get_settings"),
  setMaxConcurrent: (n: number) => invoke<void>("set_max_concurrent", { n }),
  onEvent: (cb: (e: VeloEvent) => void) => listen<VeloEvent>("velo://event", (ev) => cb(ev.payload)),
};

export function humanBytes(n: number | null | undefined): string {
  if (n === null || n === undefined) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v >= 100 || i === 0 ? 0 : 1)} ${units[i]}`;
}

export function humanTime(seconds: number): string {
  if (!isFinite(seconds) || seconds < 0) return "—";
  if (seconds < 60) return `${Math.round(seconds)}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${Math.round(seconds % 60)}s`;
  return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
}
