#!/usr/bin/env node
// Builds the website the way the Docs workflow does (mdBook docs under
// /docs/, the landing page at the root) and serves it on localhost.
// Rebuilds when anything in docs/site changes; reload the page to see it.
//
//   npm run docs              # http://localhost:3000
//   PORT=4000 npm run docs
//   npm run docs:build        # build only, into docs/site/book
//
// Needs mdBook on PATH (CI pins 0.4.52; 0.5 builds it too):
// `brew install mdbook` or `cargo install mdbook --version 0.4.52`.

import { spawnSync } from "node:child_process";
import { cpSync, createReadStream, existsSync, statSync, watch } from "node:fs";
import { createServer } from "node:http";
import { extname, join, normalize, resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const site = join(root, "docs/site");
const out = join(site, "book");
const port = Number(process.env.PORT) || 3000;

function build() {
  const r = spawnSync("mdbook", ["build", site], { stdio: ["ignore", "ignore", "inherit"] });
  if (r.error?.code === "ENOENT") {
    console.error("mdbook not found. Install it with `cargo install mdbook --version 0.4.52` or `brew install mdbook`.");
    process.exit(1);
  }
  if (r.status !== 0) {
    console.error("mdbook build failed");
    return false;
  }
  cpSync(join(site, "landing"), out, { recursive: true });
  return true;
}

if (!build()) process.exit(1);
if (process.argv.includes("--build")) process.exit(0);

const types = {
  ".html": "text/html; charset=utf-8", ".css": "text/css", ".js": "text/javascript",
  ".json": "application/json", ".svg": "image/svg+xml", ".png": "image/png",
  ".webp": "image/webp", ".gif": "image/gif", ".woff2": "font/woff2", ".woff": "font/woff",
  ".ttf": "font/ttf", ".txt": "text/plain",
};

createServer((req, res) => {
  const path = decodeURIComponent(new URL(req.url, "http://x").pathname);
  let file = normalize(join(out, path));
  if (!file.startsWith(out)) return res.writeHead(403).end();
  if (existsSync(file) && statSync(file).isDirectory()) file = join(file, "index.html");
  if (!existsSync(file)) {
    res.writeHead(404, { "content-type": types[".html"] });
    return createReadStream(join(out, "docs/404.html")).pipe(res);
  }
  res.writeHead(200, { "content-type": types[extname(file)] ?? "application/octet-stream", "cache-control": "no-store" });
  createReadStream(file).pipe(res);
}).listen(port, () => {
  console.log(`Site: http://localhost:${port}/   Docs: http://localhost:${port}/docs/`);
});

let timer;
watch(site, { recursive: true }, (_, name) => {
  if (!name || name.startsWith("book")) return;
  clearTimeout(timer);
  timer = setTimeout(() => build() && console.log(`rebuilt (${name})`), 200);
});
