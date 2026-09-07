// Shared client for Velo's local API. Both the background worker and the
// popup use this; nothing else talks to the network.

export interface VeloConfig {
  port: number;
  token: string;
  /** Master on/off switch for taking over browser downloads. */
  intercept: boolean;
}

export interface DownloadItem {
  url: string;
  file_name?: string;
  headers?: Record<string, string>;
}

const DEFAULTS: VeloConfig = { port: 0, token: "", intercept: true };

/// Ports Velo tries, in the same order the app does.
const PORTS = [48211, 48212, 48213, 48214, 48215];

export async function getConfig(): Promise<VeloConfig> {
  const v = await chrome.storage.local.get(DEFAULTS);
  return { ...DEFAULTS, ...v } as VeloConfig;
}

export async function setConfig(patch: Partial<VeloConfig>): Promise<void> {
  await chrome.storage.local.set(patch);
}

function base(cfg: VeloConfig): string {
  return `http://127.0.0.1:${cfg.port}`;
}

/** Is Velo running and are our credentials good? */
export async function ping(cfg: VeloConfig): Promise<boolean> {
  if (!cfg.port) return false;
  try {
    const r = await fetch(`${base(cfg)}/api/ping`, { method: "GET" });
    if (!r.ok) return false;
    const j = await r.json();
    return j.app === "velo";
  } catch {
    return false;
  }
}

async function post(cfg: VeloConfig, path: string, body: unknown): Promise<number[]> {
  const r = await fetch(`${base(cfg)}${path}`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      authorization: `Bearer ${cfg.token}`,
    },
    body: JSON.stringify(body),
  });
  if (r.status === 401) throw new Error("Velo rejected the token. Re-pair in the popup.");
  if (!r.ok) throw new Error(`Velo returned ${r.status}`);
  const j = await r.json();
  return j.ids ?? [];
}

export function sendDownload(cfg: VeloConfig, item: DownloadItem): Promise<number[]> {
  return post(cfg, "/api/download", item);
}

export function sendBatch(
  cfg: VeloConfig,
  name: string,
  sourcePage: string | null,
  items: DownloadItem[],
): Promise<number[]> {
  return post(cfg, "/api/batch", { name, source_page: sourcePage, items });
}

/**
 * Find Velo and get a token, asking the user for approval inside the app.
 * Called on install and whenever a request fails, so the user never types
 * a port or a token by hand.
 */
export async function connect(): Promise<VeloConfig> {
  for (const port of PORTS) {
    const probe: VeloConfig = { port, token: "", intercept: true };
    if (!(await ping(probe))) continue;

    const cfg = await getConfig();
    // Already have a token? Check it still works before asking again.
    if (cfg.token && (await tokenWorks({ ...cfg, port }))) {
      await setConfig({ port });
      return { ...cfg, port };
    }

    const token = await handshake(port);
    await setConfig({ port, token });
    return { ...cfg, port, token };
  }
  throw new Error("Velo is not running.");
}

/** Cheap authenticated call to see whether our token is still valid. */
async function tokenWorks(cfg: VeloConfig): Promise<boolean> {
  try {
    const r = await fetch(`${base(cfg)}/api/batch`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        authorization: `Bearer ${cfg.token}`,
      },
      // Empty items is a 400 when authorized, 401 when not. That tells us
      // what we need without queueing anything.
      body: JSON.stringify({ items: [] }),
    });
    return r.status !== 401;
  } catch {
    return false;
  }
}

/** Velo shows an Allow prompt; this resolves once the user answers. */
async function handshake(port: number): Promise<string> {
  const r = await fetch(`http://127.0.0.1:${port}/api/handshake`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      extension_id: chrome.runtime.id,
      browser: browserName(),
    }),
  });
  if (r.status === 403) throw new Error("You denied the connection in Velo.");
  if (r.status === 408) throw new Error("Velo timed out waiting for approval.");
  if (!r.ok) throw new Error(`Velo returned ${r.status}`);
  const j = await r.json();
  return j.token as string;
}

function browserName(): string {
  const ua = navigator.userAgent;
  if (ua.includes("Edg/")) return "Microsoft Edge";
  if (ua.includes("Firefox/")) return "Firefox";
  if (ua.includes("OPR/")) return "Opera";
  if (ua.includes("Brave")) return "Brave";
  if (ua.includes("Chrome/")) return "Chrome";
  return "Browser";
}

/** Cookies and referer so the server treats Velo like the browser did. */
export async function contextHeaders(url: string, pageUrl?: string): Promise<Record<string, string>> {
  const headers: Record<string, string> = {
    "user-agent": navigator.userAgent,
  };
  if (pageUrl) headers.referer = pageUrl;
  try {
    // Requires no extra permission: we only read what the page itself could.
    const cookie = await readCookies(url);
    if (cookie) headers.cookie = cookie;
  } catch {
    // Cookie access is optional; plenty of links work without it.
  }
  return headers;
}

async function readCookies(url: string): Promise<string> {
  if (!chrome.cookies) return "";
  const list = await chrome.cookies.getAll({ url });
  return list.map((c) => `${c.name}=${c.value}`).join("; ");
}
