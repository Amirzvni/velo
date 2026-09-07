// Service worker. Two jobs:
//   1. take over downloads the browser is about to start (feature 2)
//   2. run the context menu entries and relay messages from the page

import { connect, contextHeaders, getConfig, ping, sendBatch, sendDownload } from "./velo";

/**
 * Get a working config, pairing on the fly if needed. Velo picks a port from
 * a fixed range and the token is stored after the user clicks Allow once, so
 * this normally returns instantly with no user interaction at all.
 */
async function ready(): Promise<import("./velo").VeloConfig> {
  const cfg = await getConfig();
  if (cfg.port && cfg.token && (await ping(cfg))) return cfg;
  return connect();
}

/** Schemes and cases we must never touch, or we break the browser. */
function shouldIntercept(item: chrome.downloads.DownloadItem): boolean {
  if (!item.url) return false;
  if (!/^https?:\/\//i.test(item.url)) return false; // blob:, data:, file:
  if (item.url.startsWith("http://127.0.0.1")) return false; // our own api
  return true;
}

// Firing at the filename stage cancels the download before the browser
// commits to it, which keeps the browser's own download bubble from showing.
chrome.downloads.onDeterminingFilename?.addListener((item, suggest) => {
  void (async () => {
    const stored = await getConfig();
    if (!stored.intercept || !shouldIntercept(item)) {
      suggest();
      return;
    }
    // Suggest a throwaway name; we cancel this download immediately anyway.
    suggest({ filename: item.filename, conflictAction: "overwrite" });
  })();
  return true;
});

chrome.downloads.onCreated.addListener(async (item) => {
  const stored = await getConfig();
  if (!stored.intercept || !shouldIntercept(item)) return;

  try {
    const cfg = await ready();
    // Stop the browser copy first so we never download the same file twice,
    // then erase it so no row is left in the browser's download list.
    await chrome.downloads.cancel(item.id).catch(() => {});
    await chrome.downloads.erase({ id: item.id }).catch(() => {});

    const headers = await contextHeaders(item.url, item.referrer || undefined);
    await sendDownload(cfg, {
      url: item.url,
      file_name: item.filename ? item.filename.split(/[\\/]/).pop() : undefined,
      headers,
    });
    notify("Sent to Velo", item.filename || item.url);
  } catch (e) {
    // If Velo is closed, let the browser keep the download rather than lose it.
    notify("Velo could not take this download", String(e));
    chrome.downloads.download({ url: item.url });
  }
});

function notify(title: string, message: string) {
  console.info(`[velo] ${title}: ${message}`);
}

// ---- context menus -------------------------------------------------------

chrome.runtime.onInstalled.addListener(() => {
  // Try to pair right away while the user still has Velo in mind.
  void connect().catch(() => {
    /* Velo may not be running yet; we retry on the first download. */
  });

  chrome.contextMenus.create({
    id: "velo-link",
    title: "Download with Velo",
    contexts: ["link"],
  });
  chrome.contextMenus.create({
    id: "velo-page",
    title: "Download all links with Velo",
    contexts: ["page", "selection"],
  });
});

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  if (info.menuItemId === "velo-page" && tab?.id) {
    // Ask the content script to open its link picker.
    chrome.tabs.sendMessage(tab.id, { type: "velo:open-picker" });
    return;
  }

  const cfg = await ready();

  if (info.menuItemId === "velo-link" && info.linkUrl) {
    const headers = await contextHeaders(info.linkUrl, info.pageUrl);
    await sendDownload(cfg, { url: info.linkUrl, headers });
  }
});

// ---- messages from the content script ------------------------------------

chrome.runtime.onMessage.addListener((msg, sender, reply) => {
  if (msg?.type !== "velo:batch") return;

  (async () => {
    try {
      const cfg = await ready();
      const pageUrl = sender.tab?.url;
      const items = await Promise.all(
        (msg.urls as string[]).map(async (url) => ({
          url,
          headers: await contextHeaders(url, pageUrl),
        })),
      );
      const ids = await sendBatch(cfg, msg.name ?? `${items.length} links`, pageUrl ?? null, items);
      reply({ ok: true, count: ids.length });
    } catch (e) {
      reply({ ok: false, error: String(e) });
    }
  })();

  return true; // keep the channel open for the async reply
});
