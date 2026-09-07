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

/** Anchors the user highlighted. Empty when nothing is selected. */
function anchorsInSelection(): HTMLAnchorElement[] {
  const sel = window.getSelection();
  if (!sel || sel.isCollapsed || sel.rangeCount === 0) return [];

  const out: HTMLAnchorElement[] = [];
  const seen = new Set<HTMLAnchorElement>();

  for (let i = 0; i < sel.rangeCount; i++) {
    const range = sel.getRangeAt(i);

    // An anchor counts as selected if the selection touches any part of it.
    // intersectsNode catches partly highlighted links, which is what people
    // get when they drag across a list.
    const all = document.querySelectorAll<HTMLAnchorElement>("a[href]");
    for (const a of Array.from(all)) {
      if (seen.has(a)) continue;
      let hit = false;
      try {
        hit = range.intersectsNode(a);
      } catch {
        hit = false;
      }
      if (!hit) continue;
      seen.add(a);
      out.push(a);
    }
  }
  return out;
}

/** Turn anchors into download candidates, deduplicated, in page order. */
function collect(anchors: HTMLAnchorElement[], strict: boolean): Found[] {
  const seen = new Set<string>();
  const out: Found[] = [];
  for (const a of anchors) {
    const href = a.href;
    if (!/^https?:\/\//i.test(href) || seen.has(href)) continue;
    const ext = extOf(href);
    // In strict mode we only take things that look like files. When the user
    // hand picked the links we trust them and take everything.
    if (strict && !DOWNLOADABLE.has(ext) && !a.hasAttribute("download")) continue;
    seen.add(href);
    out.push({ url: href, name: nameOf(href), ext: ext || "file" });
  }
  return out;
}

/**
 * What the picker should show: the user's selection when there is one,
 * otherwise every file looking link on the page.
 */
function scan(): { found: Found[]; fromSelection: boolean } {
  const selected = anchorsInSelection();
  if (selected.length) {
    return { found: collect(selected, false), fromSelection: true };
  }
  const all = Array.from(document.querySelectorAll<HTMLAnchorElement>("a[href]"));
  return { found: collect(all, true), fromSelection: false };
}

// ---- panel ---------------------------------------------------------------

let host: HTMLDivElement | null = null;

function closePanel() {
  host?.remove();
  host = null;
}

function openPanel(found: Found[], fromSelection: boolean) {
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
    <div class="velo-sub">
      <span class="velo-count">${found.length}</span>
      ${fromSelection ? "links in your selection" : "links found on this page"}
    </div>
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

function trigger() {
  const { found, fromSelection } = scan();
  if (!found.length) {
    alert(
      fromSelection
        ? "Velo found no links inside your selection."
        : "Velo found no downloadable links on this page.",
    );
    return;
  }
  openPanel(found, fromSelection);
}

chrome.runtime.onMessage.addListener((msg) => {
  if (msg?.type !== "velo:open-picker") return;
  trigger();
});

// Select several links and press Alt+V to open the picker for them.
document.addEventListener("keydown", (e) => {
  if (!e.altKey || e.key.toLowerCase() !== "v") return;
  trigger();
});
