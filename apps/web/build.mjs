// Builds the browser app into apps/web/dist: the Leptos UI (trunk), the Rust core (WASI), the official
// plugins (transpiled by apps/web/plugin-host), and the shell that connects them. Serve the folder
// from any static server; the app keeps all of its data in the browser.
//
// Usage: node apps/web/build.mjs            (needs trunk, and the wasm32-wasip1 C toolchain for the core)
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { chmodSync, cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync, copyFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse } from 'parse5';

const web = dirname(fileURLToPath(import.meta.url));
const repo = resolve(web, '..', '..');
const ui = join(repo, 'apps', 'app', 'ui');
const out = join(web, 'dist');

// The core's WebAssembly module, as cargo writes it (CARGO_TARGET_DIR moves cargo's folder).
const coreWasm = join(process.env.CARGO_TARGET_DIR ? resolve(process.env.CARGO_TARGET_DIR) : join(repo, 'target'), 'wasm32-wasip1', 'release', 'locora_web_core.wasm');
const pluginDist = join(web, 'plugin-host', 'dist');
const pluginSrc = join(web, 'plugin-host', 'src');
const shell = join(web, 'shell');
// The preview 2 shim the plugins import (browser build, relative imports only).
const shim = join(web, 'plugin-host', 'node_modules', '@bytecodealliance', 'preview2-shim', 'dist', 'browser');
const SHIM_MODULES = ['cli', 'clocks', 'io', 'random'];

function run(command, args, cwd, env = process.env) {
  execFileSync(command, args, { cwd, stdio: 'inherit', env });
}

// 1. The UI, the core and the plugins are built by their own tools; this only assembles them.
// TRUNK_PROFILE=debug keeps function names in the UI for diagnosing a panic; release is the default.
// The UI is a workspace of its own (apps/app) and builds in its own target folder, whatever
// CARGO_TARGET_DIR says for the core.
const { CARGO_TARGET_DIR: _core, ...uiEnv } = process.env;
run('trunk', process.env.TRUNK_PROFILE === 'debug' ? ['build'] : ['build', '--release'], ui, uiEnv);
// The core's SQLite is C code: it needs a clang that knows the WASI target. Where none is installed,
// cargo-zigbuild brings one (zig, from the ziglang package), so `cargo zigbuild` is used instead.
// LOCORA_ZIGBUILD=1 uses zig even where a clang without the WASI libraries is installed (CI).
const hasClang = process.env.LOCORA_ZIGBUILD !== '1' && (() => {
  try {
    execFileSync('clang', ['--version'], { stdio: 'ignore' });
    return true;
  } catch {
    return false;
  }
})();
run('cargo', [hasClang ? 'build' : 'zigbuild', '--release', '--target', 'wasm32-wasip1', '-p', 'locora-web-core'], repo);
for (const id of ['core', 'notify', 'review', 'tutorial', 'gamification']) {
  if (!existsSync(join(pluginDist, id))) {
    throw new Error(`plugin ${id} is not transpiled: run npm run transpile in apps/web/plugin-host`);
  }
}

// 2. Assemble the folder from scratch, so nothing stale stays behind.
rmSync(out, { recursive: true, force: true });
cpSync(join(ui, 'dist'), out, { recursive: true });
mkdirSync(join(out, 'core'), { recursive: true });
copyFileSync(coreWasm, join(out, 'core', 'locora_web_core.wasm'));
cpSync(pluginDist, join(out, 'plugins'), { recursive: true });
mkdirSync(join(out, 'src'), { recursive: true });
for (const file of readdirSync(pluginSrc)) {
  if (file.endsWith('.mjs')) copyFileSync(join(pluginSrc, file), join(out, 'src', file));
}
mkdirSync(join(out, 'shell'), { recursive: true });
for (const file of readdirSync(join(shell, 'src'))) {
  if (file.endsWith('.mjs')) copyFileSync(join(shell, 'src', file), join(out, 'shell', file));
}
copyFileSync(join(shell, 'graphic.html'), join(out, 'shell', 'graphic.html'));
// Headers for hosts that read a _headers file (Netlify): the clickjacking guard and the cache rules the
// app needs for its start page and its service worker. Other hosts ignore the file.
copyFileSync(join(web, '_headers'), join(out, '_headers'));
// The splash graphics are Splinely SVGs. They load as their own documents (see graphic.js), not as
// markup in the page: inside HTML, an SVG script is parsed as HTML and breaks the runtime.
const splashSource = join(ui, 'splash');
mkdirSync(join(out, 'splash'), { recursive: true });
for (const file of readdirSync(splashSource)) {
  if (file.endsWith('.svg')) copyFileSync(join(splashSource, file), join(out, 'splash', file));
}
copyFileSync(join(shell, 'graphic.js'), join(out, 'shell', 'graphic.js'));
// The Splinely runtime and its inline mount (see splinely-mount.js): graphics run in the page, not in frames.
const vendor = join(web, '..', '..', 'crates', 'locora-app-core', 'vendor');
copyFileSync(join(vendor, 'splinely-0.5.4.js'), join(out, 'shell', 'splinely.js'));
copyFileSync(join(vendor, 'splinely-mount.js'), join(out, 'shell', 'splinely-mount.js'));
// The Locora frame bridge (sply_bridge.js) runs in the page too: the same code, as `SplyBridge.attach`.
const bridge = join(web, '..', '..', 'crates', 'locora-app-core', 'src', 'sply_bridge.js');
writeFileSync(join(out, 'shell', 'sply-bridge.js'), `${readFileSync(bridge, 'utf8')}\nwindow.SplyBridge = { attach };\n`);
cpSync(shim, join(out, 'shim'), { recursive: true });
// The video converter (mediabunny, MPL-2.0), loaded by video.mjs only when a video is added.
const mediabunny = join(web, 'node_modules', 'mediabunny');
copyFileSync(join(mediabunny, 'dist', 'bundles', 'mediabunny.min.mjs'), join(out, 'shell', 'mediabunny.mjs'));
copyFileSync(join(mediabunny, 'LICENSE'), join(out, 'shell', 'mediabunny.LICENSE.txt'));

// 3. The page's content security policy. Scripts run only from this site, or inline with a hash
// that is listed here: the splash screen and Trunk's loader, and the scripts inside the splash
// graphics. A real HTML parser finds them, the way the browser does.
const indexPath = join(out, 'index.html');
let html = readFileSync(indexPath, 'utf8');
// Each splash template becomes a frame of its own graphic document; the splash script picks one of
// them and inserts it, as it does with the templates.
html = html.replace(/<template id="(splash-[a-z-]+)"[^>]*>[\s\S]*?<\/template>/g, (_, id) =>
  `<template id="${id}"><iframe class="sply" title="Animation" sandbox="allow-scripts" tabindex="-1" src="shell/graphic.html?file=splash/${id}.svg"></iframe></template>`,
);
const inlineScripts = [];
const collect = (node) => {
  if (node.nodeName === 'script' && !node.attrs?.some((attr) => attr.name === 'src')) {
    inlineScripts.push(node.childNodes.map((child) => child.value ?? '').join(''));
  }
  for (const child of node.childNodes ?? []) collect(child);
  if (node.content) collect(node.content);
};
collect(parse(html));
const hashes = [...new Set(inlineScripts)].map(
  (code) => `'sha256-${createHash('sha256').update(code).digest('base64')}'`,
);
// The cloud servers the app may talk to (sync). LOCORA_CLOUD_ORIGINS adds others, space-separated
// (e.g. a self-hosted server); nothing else may be contacted.
const cloudOrigins = (process.env.LOCORA_CLOUD_ORIGINS ?? 'https://api.locora.cloud').trim();
const csp = [
  "default-src 'self'",
  `script-src 'self' 'wasm-unsafe-eval' ${hashes.join(' ')}`,
  "style-src 'self' 'unsafe-inline'",
  "img-src 'self' blob: data:",
  "media-src 'self' blob: data:",
  `connect-src 'self' blob: ${cloudOrigins}`,
  "frame-src 'self' blob:",
  "object-src 'none'",
  "base-uri 'none'",
  "form-action 'none'",
].join('; ');
// Bare module names of the plugins map to the copied shim. The map comes first in the page.
const importMap = JSON.stringify({
  imports: Object.fromEntries(SHIM_MODULES.map((name) => [`@bytecodealliance/preview2-shim/${name}`, `./shim/${name}.js`])),
});
// The map is an inline script too, so it needs its hash in the policy as well.
const mapHash = `'sha256-${createHash('sha256').update(importMap).digest('base64')}'`;
const finalCsp = csp.replace("script-src 'self' 'wasm-unsafe-eval' ", `script-src 'self' 'wasm-unsafe-eval' ${mapHash} `);
html = html.replace(
  '<head>',
  `<head>\n    <meta http-equiv="Content-Security-Policy" content="${finalCsp}" />\n    <script type="importmap">${importMap}</script>\n    <script type="module" src="shell/boot.mjs"></script>` +
    // The inline mount for graphics (see splinely-mount.js), loaded before the splash runs. Only the web
    // app has these files; the desktop and Android apps use the splash templates and frames instead.
    `\n    <script src="shell/splinely.js"></script>\n    <script src="shell/splinely-mount.js"></script>\n    <script src="shell/sply-bridge.js"></script>`,
);
// 4. Installable as an app: the manifest, the icons, and the links that browsers look for. The tab
// shows the website's favicon, an SVG without scripts (img-src 'self' allows it, and as a file in
// shell/icons the service worker stores it); browsers without SVG favicons take the PNG.
copyFileSync(join(shell, 'manifest.webmanifest'), join(out, 'manifest.webmanifest'));
cpSync(join(shell, 'icons'), join(out, 'shell', 'icons'), { recursive: true });
html = html.replace(
  '<head>',
  `<head>\n    <link rel="manifest" href="manifest.webmanifest" />\n    <meta name="theme-color" content="#16173a" />\n    <link rel="icon" type="image/png" sizes="192x192" href="shell/icons/icon-192.png" />\n    <link rel="icon" type="image/svg+xml" href="shell/icons/favicon.svg" />\n    <link rel="apple-touch-icon" href="shell/icons/apple-touch-icon.png" />`,
);

// The files are copied with the modes of their sources (some are 0600): the server must read them.
// Type declarations of the plugins are not used in a browser, so they are left out.
const tidy = (dir) => {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      chmodSync(path, 0o755);
      tidy(path);
    } else if (entry.name.endsWith('.d.ts')) {
      rmSync(path);
    } else {
      chmodSync(path, 0o644);
    }
  }
};
chmodSync(out, 0o755);
tidy(out);

// 5. Offline: the service worker stores every file of this build. Its cache name is a hash of the
// build's contents, so a new build installs a new cache and the old one is removed.
writeFileSync(indexPath, html);
const everyFile = (dir, prefix = '') =>
  readdirSync(dir, { withFileTypes: true }).flatMap((entry) =>
    entry.isDirectory()
      ? everyFile(join(dir, entry.name), `${prefix}${entry.name}/`)
      : [`${prefix}${entry.name}`],
  );
const precache = everyFile(out).filter((path) => path !== 'sw.js').sort();
const version = createHash('sha256');
for (const path of precache) version.update(path).update(readFileSync(join(out, path)));
const swTemplate = readFileSync(join(shell, 'sw.js'), 'utf8');
writeFileSync(
  join(out, 'sw.js'),
  swTemplate
    .replace("'__VERSION__'", `'${version.digest('hex').slice(0, 16)}'`)
    .replace('__PRECACHE__', JSON.stringify(precache)),
);
console.log(`built ${out} (${hashes.length} inline scripts allowed by hash, ${precache.length} files for offline use)`);
