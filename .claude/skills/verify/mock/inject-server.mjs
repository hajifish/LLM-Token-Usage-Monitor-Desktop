#!/usr/bin/env node
// Verification scaffolding — do not ship, do not import from app code.
// Proxies the Vite dev server (default http://localhost:1420) on :1421 and
// injects <script src="/__verify__/tauri-shim.js"></script> into index.html
// before the app bundle loads, so a plain browser (Playwright) runs the real
// React app against the mock IPC. No app file is modified; HMR websockets are
// not proxied (console shows an HMR connect error — expected, ignore it).
//
// Usage: node .claude/skills/verify/mock/inject-server.mjs
//   VERIFY_UPSTREAM=http://localhost:1420 VERIFY_PORT=1421 env-overridable
// Ready when `curl -sf http://localhost:1421/ | grep -q __verify__`.
import http from 'node:http';
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';

const UPSTREAM = process.env.VERIFY_UPSTREAM || 'http://localhost:1420';
const PORT = Number(process.env.VERIFY_PORT || 1421);
const SHIM_URL_PATH = '/__verify__/tauri-shim.js';
const INJECT_TAG = `<script src="${SHIM_URL_PATH}"></script>`;
const shim = fs.readFileSync(fileURLToPath(new URL('./tauri-shim.js', import.meta.url)), 'utf8');

const HOP_HEADERS = new Set([
  'connection', 'keep-alive', 'transfer-encoding', 'upgrade', 'proxy-authenticate',
  'proxy-authorization', 'te', 'trailers', 'host', 'content-length', 'accept-encoding',
]);

const server = http.createServer(async (req, res) => {
  try {
    if (req.url === SHIM_URL_PATH) {
      res.writeHead(200, { 'content-type': 'application/javascript; charset=utf-8', 'cache-control': 'no-store' });
      res.end(shim);
      return;
    }
    const chunks = [];
    for await (const c of req) chunks.push(c);
    const body = chunks.length ? Buffer.concat(chunks) : undefined;

    const headers = {};
    for (const [k, v] of Object.entries(req.headers)) if (!HOP_HEADERS.has(k)) headers[k] = v;
    headers['accept-encoding'] = 'identity'; // keep HTML re-writable

    const upstream = await fetch(UPSTREAM + req.url, {
      method: req.method,
      headers,
      body: ['GET', 'HEAD'].includes(req.method) ? undefined : body,
      redirect: 'manual',
    });

    const outHeaders = {};
    upstream.headers.forEach((v, k) => {
      if (!HOP_HEADERS.has(k)) outHeaders[k] = v;
    });
    outHeaders['cache-control'] = 'no-store';

    const buf = Buffer.from(await upstream.arrayBuffer());
    const ct = String(upstream.headers.get('content-type') || '');
    if (ct.includes('text/html')) {
      const html = buf.toString('utf8');
      if (html.includes('<head>') && !html.includes(SHIM_URL_PATH)) {
        res.writeHead(upstream.status, outHeaders);
        res.end(html.replace('<head>', `<head>${INJECT_TAG}`));
        return;
      }
    }
    res.writeHead(upstream.status, outHeaders);
    res.end(buf);
  } catch (e) {
    res.writeHead(502, { 'content-type': 'text/plain' });
    res.end(`verify inject-server: upstream ${UPSTREAM} unreachable: ${e.message}`);
  }
});

server.listen(PORT, () => {
  console.log(`verify inject-server: http://localhost:${PORT} -> ${UPSTREAM} (shim injected)`);
});
