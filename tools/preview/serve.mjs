// Static server for the UI with the Tauri stub injected into index.html (ui/ itself is never modified).
//   node tools/preview/serve.mjs [port]      then open http://127.0.0.1:4173/?scenario=normal
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join, normalize, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const ui = resolve(here, "../../ui");
const TYPES = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".json": "application/json", ".png": "image/png", ".svg": "image/svg+xml" };

export function startServer(port = 4173) {
  const server = createServer(async (req, res) => {
    const url = new URL(req.url, "http://x");
    let path = url.pathname === "/" ? "/index.html" : decodeURIComponent(url.pathname);
    try {
      if (path === "/__preview/stub.js") {
        res.writeHead(200, { "content-type": "text/javascript" });
        return res.end(await readFile(join(here, "stub.js")));
      }
      const file = normalize(join(ui, path));
      if (!file.startsWith(ui)) { res.writeHead(403); return res.end(); }
      let body = await readFile(file);
      if (path === "/index.html") {
        body = Buffer.from(body.toString("utf8").replace('<script type="module"', '<script src="/__preview/stub.js"></script>\n  <script type="module"'));
      }
      res.writeHead(200, { "content-type": TYPES[extname(file)] ?? "application/octet-stream", "cache-control": "no-store" });
      res.end(body);
    } catch {
      res.writeHead(404); res.end("not found");
    }
  });
  return new Promise((ok) => server.listen(port, "127.0.0.1", () => ok(server)));
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const port = Number(process.argv[2] ?? 4173);
  await startServer(port);
  console.log(`preview at http://127.0.0.1:${port}/?scenario=normal`);
}
