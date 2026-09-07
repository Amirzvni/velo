// Tiny build: bundle three TS entry points, copy the static files.
// No framework in the extension on purpose. Less code in the browser is
// less attack surface and a faster page load.
import { build } from "esbuild";
import { cp, mkdir, rm } from "node:fs/promises";

const out = "dist";
await rm(out, { recursive: true, force: true });
await mkdir(out, { recursive: true });

await build({
  entryPoints: {
    background: "src/background.ts",
    content: "src/content.ts",
    popup: "src/popup.ts",
  },
  bundle: true,
  format: "esm",
  target: "chrome110",
  outdir: out,
  minify: process.argv.includes("--minify"),
  legalComments: "none",
});

for (const f of ["manifest.json", "src/panel.css", "src/popup.html", "icons"]) {
  const name = f.replace("src/", "");
  await cp(f, `${out}/${name}`, { recursive: true });
}

console.log("extension built to extension/dist");
