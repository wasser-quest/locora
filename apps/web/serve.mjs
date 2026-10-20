// Serves the browser app (apps/web/dist) for a browser. Offline use, the service worker and the
// install prompt need a secure address: localhost, or HTTPS. To reach the app from another device
// (for example from another Qubes qube), give it a certificate:
//
//   openssl req -x509 -newkey rsa:2048 -nodes -days 365 -subj "/CN=locora" \
//     -addext "subjectAltName=IP:192.168.1.10,DNS:localhost" -keyout key.pem -out cert.pem
//   node apps/web/serve.mjs --host 0.0.0.0 --port 8443 --tls-cert cert.pem --tls-key key.pem
//
// Without --tls-cert/--tls-key it serves plain HTTP on 127.0.0.1:8080.
import { createServer as createHttp } from 'node:http';
import { createServer as createHttps } from 'node:https';
import { readFile } from 'node:fs/promises';
import { dirname, extname, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';

const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.wasm': 'application/wasm',
  '.json': 'application/json',
  '.webmanifest': 'application/manifest+json',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.ico': 'image/x-icon',
};

/** Command-line options: --host, --port, --tls-cert, --tls-key, --dir. */
function options(argv) {
  const values = new Map();
  for (let i = 0; i < argv.length; i += 2) values.set(argv[i].replace(/^--/, ''), argv[i + 1]);
  return {
    host: values.get('host') ?? '127.0.0.1',
    port: Number(values.get('port') ?? 8080),
    cert: values.get('tls-cert'),
    key: values.get('tls-key'),
    dir: values.get('dir') ?? join(dirname(fileURLToPath(import.meta.url)), 'dist'),
  };
}

/** The app's files; a path outside the folder or a missing file is "not found". */
export function handler(root) {
  return async (request, response) => {
    const path = decodeURIComponent(new URL(request.url, 'http://localhost').pathname);
    const file = normalize(join(root, path === '/' ? 'index.html' : path));
    if (!file.startsWith(root)) return send(response, 403, 'text/plain', 'nicht erlaubt');
    try {
      const body = await readFile(file);
      // The service worker and the start page must always be checked against the server.
      const cache = /\/(sw\.js|index\.html)$|^\/?$/.test(path) ? 'no-cache' : 'max-age=3600';
      return send(response, 200, TYPES[extname(file)] ?? 'application/octet-stream', body, cache);
    } catch {
      return send(response, 404, 'text/plain', 'nicht gefunden');
    }
  };
}

function send(response, status, type, body, cache = 'no-cache') {
  response.writeHead(status, { 'Content-Type': type, 'Cache-Control': cache, 'X-Content-Type-Options': 'nosniff' });
  response.end(body);
}

/** Starts the server; returns it, with the address it listens on. */
export async function start(settings) {
  const root = normalize(settings.dir);
  const secure = settings.cert && settings.key;
  const server = secure
    ? createHttps({ cert: await readFile(settings.cert), key: await readFile(settings.key) }, handler(root))
    : createHttp(handler(root));
  await new Promise((resolve) => server.listen(settings.port, settings.host, resolve));
  const { port } = server.address();
  return { server, url: `${secure ? 'https' : 'http'}://${settings.host}:${port}/` };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const settings = options(process.argv.slice(2));
  const { url } = await start(settings);
  console.log(`Locora is served at ${url} (folder ${settings.dir})`);
}
