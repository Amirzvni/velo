// Runs on every page. Finds downloadable links and shows a picker panel.
// Injected UI lives in a shadow root so page CSS cannot break it and our CSS
// cannot break the page.

const DOWNLOADABLE = new Set([
  "zip", "rar", "7z", "tar", "gz", "xz", "bz2",
  "exe", "msi", "dmg", "pkg", "deb", "rpm", "appimage",
  "mp4", "mkv", "avi", "mov", "webm", "flv", "wmv", "m4v",
  "mp3", "flac", "wav", "aac", "ogg", "m4a",
  "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "epub",
  "iso", "img", "apk", "jar", "bin",
]);

interface Found {
  url: string;
  name: string;
  ext: string;
}

function extOf(url: string): string {
  try {
    const path = new URL(url, location.href).pathname;
    const last = path.split("/").pop() ?? "";
    const dot = last.lastIndexOf(".");
    return dot > 0 ? last.slice(dot + 1).toLowerCase() : "";
  } catch {
    return "";
  }
}

function nameOf(url: string): string {
  try {
    const path = new URL(url, location.href).pathname;
    return decodeURIComponent(path.split("/").pop() || url);
  } catch {
    return url;
  }
}

/** Every anchor that looks like a file, deduplicated, in page order. */
function scan(): Found[] {
  const seen = new Set<string>();
  const out: Found[] = [];
  for (const a of Array.from(document.querySelectorAll<HTMLAnchorElement>("a[href]"))) {
    const href = a.href;
    if (!/^https?:\/\//i.test(href) || seen.has(href)) continue;
    const ext = extOf(href);
    // Either a known file extension, or the author marked it as a download.
    if (!DOWNLOADABLE.has(ext) && !a.hasAttribute("download")) continue;
    seen.add(href);
    out.push({ url: href, name: nameOf(href), ext: ext || "file" });
  }
  return out;
}

// ---- panel ---------------------------------------------------------------

let host: HTMLDivElement | null = null;

function closePanel() {
  host?.remove();
  host = null;
}

function openPanel(found: Found[]) {
  closePanel();

  host = document.createElement("div");
  host.id = "velo-host";
  const root = host.attachShadow({ mode: "closed" });

  const selected = new Set(found.map((f) => f.url));

  const wrap = document.createElement("div");
  wrap.className = "velo-panel";
  wrap.innerHTML = `
    <div class="velo-head">
      <span class="velo-mark">V</span>
      <span class="velo-title">Download with Velo</span>
      <button class="velo-x" title="Close">&#10005;</button>
    </div>
    <div class="velo-sub"><span class="velo-count">${found.length}</span> links found</div>
    <div class="velo-tools">
      <button class="velo-mini" data-act="all">All</button>
      <button class="velo-mini" data-act="none">None</button>
      <input class="velo-filter" placeholder="filter by name or type" />
    </div>
    <div class="velo-list"></div>
    <div class="velo-foot">
      <span class="velo-note"></span>
      <button class="velo-go">Download <span class="velo-n">${found.length}</span></button>
    </div>
  `;

  const list = wrap.querySelector(".velo-list") as HTMLDivElement;
  const nEl = wrap.querySelector(".velo-n") as HTMLSpanElement;
  const note = wrap.querySelector(".velo-note") as HTMLSpanElement;
  const go = wrap.querySelector(".velo-go") as HTMLButtonElement;

  function refreshCount() {
    nEl.textContent = String(selected.size);
    go.disabled = selected.size === 0;
  }

  function render(filter: string) {
    list.textContent = "";
    const q = filter.trim().toLowerCase();
    for (const f of found) {
      if (q && !f.name.toLowerCase().includes(q) && !f.ext.includes(q)) continue;

      const row = document.createElement("label");
      row.className = "velo-row";

      const cb = document.createElement("input");
      cb.type = "checkbox";
      cb.checked = selected.has(f.url);
      cb.addEventListener("change", () => {
        if (cb.checked) selected.add(f.url);
        else selected.delete(f.url);
        refreshCount();
      });

      const name = document.createElement("span");
      name.className = "velo-name";
      name.textContent = f.name;
      name.title = f.url;

      const tag = document.createElement("span");
      tag.className = "velo-tag";
      tag.textContent = f.ext;

      row.append(cb, name, tag);
      list.append(row);
    }
  }

  wrap.querySelector(".velo-x")!.addEventListener("click", closePanel);

  wrap.querySelectorAll<HTMLButtonElement>(".velo-mini").forEach((b) => {
    b.addEventListener("click", () => {
      const all = b.dataset.act === "all";
      selected.clear();
      if (all) found.forEach((f) => selected.add(f.url));
      refreshCount();
      render((wrap.querySelector(".velo-filter") as HTMLInputElement).value);
    });
  });

  (wrap.querySelector(".velo-filter") as HTMLInputElement).addEventListener("input", (e) => {
    render((e.target as HTMLInputElement).value);
  });

  go.addEventListener("click", () => {
    const urls = found.filter((f) => selected.has(f.url)).map((f) => f.url);
    go.disabled = true;
    note.textContent = "sending…";
    chrome.runtime.sendMessage(
      { type: "velo:batch", urls, name: document.title || "links" },
      (res) => {
        if (res?.ok) {
          note.textContent = `queued ${res.count}`;
          setTimeout(closePanel, 900);
        } else {
          note.textContent = res?.error ?? "failed";
          go.disabled = false;
        }
      },
    );
  });

  const style = document.createElement("link");
  style.rel = "stylesheet";
  style.href = chrome.runtime.getURL("panel.css");

  root.append(style, wrap);
  document.documentElement.append(host);

  render("");
  refreshCount();
}

// ---- triggers ------------------------------------------------------------

chrome.runtime.onMessage.addListener((msg) => {
  if (msg?.type !== "velo:open-picker") return;
  const found = scan();
  if (!found.length) {
    alert("Velo found no downloadable links on this page.");
    return;
  }
  openPanel(found);
});

// Selecting several links and pressing Alt+V opens the picker for them.
document.addEventListener("keydown", (e) => {
  if (!e.altKey || e.key.toLowerCase() !== "v") return;
  const found = scan();
  if (found.length) openPanel(found);
});
