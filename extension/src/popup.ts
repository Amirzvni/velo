// Status and one button. Port and token are handled automatically; the user
// only ever clicks Allow inside Velo itself.

import { connect, getConfig, ping, setConfig } from "./velo";

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const dot = $<HTMLSpanElement>("dot");
const stateText = $<HTMLSpanElement>("stateText");
const interceptEl = $<HTMLInputElement>("intercept");
const connectBtn = $<HTMLButtonElement>("connect");
const msgEl = $<HTMLDivElement>("msg");

function setState(kind: "ok" | "bad" | "unknown", text: string) {
  dot.className = `dot ${kind === "unknown" ? "" : kind}`;
  stateText.textContent = text;
}

function say(text: string, bad = false) {
  msgEl.textContent = text;
  msgEl.style.color = bad ? "#ff003c" : "#39ff14";
}

async function refresh() {
  const cfg = await getConfig();
  interceptEl.checked = cfg.intercept;

  if (cfg.port && cfg.token && (await ping(cfg))) {
    setState("ok", `Connected on port ${cfg.port}`);
    connectBtn.textContent = "Reconnect";
  } else {
    setState("bad", "Not connected");
    connectBtn.textContent = "Connect to Velo";
  }
}

connectBtn.addEventListener("click", async () => {
  connectBtn.disabled = true;
  say("Look for the Allow prompt in the Velo window…");
  try {
    const cfg = await connect();
    say(`Connected on port ${cfg.port}.`);
  } catch (e) {
    say(String(e instanceof Error ? e.message : e), true);
  } finally {
    connectBtn.disabled = false;
    await refresh();
  }
});

interceptEl.addEventListener("change", async () => {
  await setConfig({ intercept: interceptEl.checked });
});

void refresh();
