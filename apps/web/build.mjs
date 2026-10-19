// Builds the browser app into apps/web/dist: the Leptos UI (trunk), the Rust core (WASI), the official
// plugins (transpiled by apps/web/plugin-host), and the shell that connects them. Serve the folder
// from any static server; the app keeps all of its data in the browser.
//
// Usage: node apps/web/build.mjs            (needs trunk, and the wasm32-wasip1 C toolchain for the core)
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync, copyFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse } from 'parse5';

const web = dirname(fileURLToPath(import.meta.url));
const repo = resolve(web, '..', '..');
const ui = join(repo, 'apps', 'app', 'ui');
const out = join(web, 'dist');

// The core's WebAssembly module, as cargo writes it.
const coreWasm = join(repo, 'target', 'wasm32-wasip1', 'release', 'locora_web_core.wasm');
const pluginDist = join(web, 'plugin-host', 'dist');
const pluginSrc = join(web, 'plugin-host', 'src');
const shell = join(web, 'shell');
// The preview 2 shim the plugins import (browser build, relative imports only).
const shim = join(web, 'plugin-host', 'node_modules', '@bytecodealliance', 'preview2-shim', 'dist', 'browser');
const SHIM_MODULES = ['cli', 'clocks', 'io', 'random'];

function run(command, args, cwd) {
  execFileSync(command, args, { cwd, stdio: 'inherit' });
}

// 1. The UI, the core and the plugins are built by their own tools; this only assembles them.
// TRUNK_PROFILE=debug keeps function names in the UI for diagnosing a panic; release is the default.
run('trunk', process.env.TRUNK_PROFILE === 'debug' ? ['build'] : ['build', '--release'], ui);
run('cargo', ['build', '--release', '--target', 'wasm32-wasip1', '-p', 'locora-web-core'], repo);
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
// The splash graphics are Splinely SVGs. They load as their own documents (see graphic.js), not as
// markup in the page: inside HTML, an SVG script is parsed as HTML and breaks the runtime.
const splashSource = join(ui, 'splash');
mkdirSync(join(out, 'splash'), { recursive: true });
for (const file of readdirSync(splashSource)) {
  if (file.endsWith('.svg')) copyFileSync(join(splashSource, file), join(out, 'splash', file));
}
copyFileSync(join(shell, 'graphic.js'), join(out, 'shell', 'graphic.js'));
cpSync(shim, join(out, 'shim'), { recursive: true });

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
const csp = [
  "default-src 'self'",
  `script-src 'self' 'wasm-unsafe-eval' ${hashes.join(' ')}`,
  "style-src 'self' 'unsafe-inline'",
  "img-src 'self' blob: data:",
  "media-src 'self' blob: data:",
  "connect-src 'self' blob:",
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
  `<head>\n    <meta http-equiv="Content-Security-Policy" content="${finalCsp}" />\n    <script type="importmap">${importMap}</script>\n    <script type="module" src="shell/boot.mjs"></script>`,
);
writeFileSync(indexPath, html);
console.log(`built ${out} (${hashes.length} inline scripts allowed by hash)`);
