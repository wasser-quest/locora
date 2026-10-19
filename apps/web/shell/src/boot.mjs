// The browser side of the Locora app. The UI calls `window.__TAURI__.core` exactly as in the desktop
// app; this file provides that object for the browser, backed by the Rust core (WebAssembly),
// the official plugins (plugin host) and the device's own storage (IndexedDB). Nothing is sent
// anywhere: the only requests are for this site's own files.
//
// Loaded before the UI (see build.mjs), so the UI's first call already finds `__TAURI__`. Calls wait
// until the core is ready.

import { MemFs } from './wasi-memfs.mjs';
import { instantiateCore } from './core.mjs';
import { openFileStorage, loadFiles, saveChanges } from './file-storage.mjs';
import { openStorage as openPluginStorage, loadPluginStorage, flushPluginStorage } from '../src/storage-idb.mjs';
import { host } from '../src/host-core.mjs';

const PLUGINS = ['core', 'notify', 'review', 'tutorial', 'gamification'];
const MEDIA_ACCEPT = 'image/*,video/*,.sply,.png,.jpg,.jpeg,.webp,.gif,.avif,.mp4,.m4v,.webm,.mov';
const ARCHIVE_ACCEPT = '.locora,.toml';
/** The core's folder as the page sees it; paths the core returns start with it. */
const CORE_FOLDER = /^\/locora\//;
const inFolder = (path) => path.replace(CORE_FOLDER, '');

/** Tickets for files dropped on the window: the UI gets a ticket, not the file. */
const tickets = new Map();
/** Blob URLs of stored media, by media path (`media/<hash>`). */
const mediaUrls = new Map();

let core = null;
let files = null;
let pluginDb = null;
let fileSystem = null;
let saving = Promise.resolve();

// The app works offline through its service worker, and keeps its data safe from the browser's
// cleanup: a persistent request asks the browser not to evict the device's storage.
if ('serviceWorker' in navigator) {
  navigator.serviceWorker.register('sw.js').catch((error) => console.warn('offline mode unavailable', error));
}
navigator.storage?.persist?.().catch(() => {});

const started = start();
started.catch((error) => console.error('Locora could not start', error));

async function start() {
  files = await openFileStorage(indexedDB);
  pluginDb = await openPluginStorage(indexedDB);
  fileSystem = new MemFs();
  fileSystem.load(await loadFiles(files));
  for (const id of PLUGINS) {
    const module = await import(`../plugins/${id}/${id}.js`);
    host.register(id, module, await loadPluginStorage(pluginDb, id));
    host.start(id);
  }
  const wasm = await (await fetch('core/locora_web_core.wasm')).arrayBuffer();
  core = await instantiateCore(wasm, { fs: fileSystem, bridge: host.asBridge() });
  const opened = core('open', '/locora');
  if (opened.err) throw new Error(opened.err);
  for (const id of PLUGINS) await flushPluginStorage(pluginDb, id, host.takeDirty(id));
}

/** Saves what changed since the last save, in order. */
function persist() {
  const changes = fileSystem.takeChanges();
  saving = saving.then(() => saveChanges(files, changes)).catch((error) => console.error('save failed', error));
  for (const id of PLUGINS) {
    const changed = host.takeDirty(id);
    if (changed.length > 0) saving = saving.then(() => flushPluginStorage(pluginDb, id, changed));
  }
}

/** Runs a command, saves, and delivers the events it published to the plugins. */
function command(name, args) {
  const reply = core(name, args);
  persist();
  if (reply.ok !== undefined) {
    for (const event of reply.events ?? []) host.publish(event.topic, new Uint8Array(event.payload));
    persist();
  }
  return reply;
}

async function invoke(cmd, args = {}) {
  await started;
  let reply;
  switch (cmd) {
    case 'home_pick_media':
      return pickAndImport(MEDIA_ACCEPT, { kind: 'palace' });
    case 'editor_pick_picture':
      return pickAndImport(MEDIA_ACCEPT, { kind: 'picture', target: args.target });
    case 'import_pick':
      return pickAndImport(ARCHIVE_ACCEPT, { kind: 'archive' });
    case 'home_import_drop':
      return importTicket(args.ticket, { kind: 'palace' });
    case 'editor_import_drop':
      return importTicket(args.ticket, { kind: 'picture', target: args.target });
    case 'export_pick':
      return exportFile('export_file', { palaceId: args.id });
    case 'dump_pick':
      return exportFile('dump_file', { palaceId: args.id });
    case 'notifications':
      return deliverNotifications();
    default:
      reply = command(cmd, args);
      if (reply.err !== undefined) throw reply.err;
      return reply.ok;
  }
}

function pickFile(accept) {
  return new Promise((resolve) => {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = accept;
    input.hidden = true;
    // Some browsers open no dialog for an input that is not part of the page.
    document.body.append(input);
    const done = (file) => {
      input.remove();
      resolve(file);
    };
    input.addEventListener('change', () => done(input.files[0] ?? null));
    input.addEventListener('cancel', () => done(null));
    input.click();
  });
}

/** Opens the file dialog; `null` when it was cancelled (the UI reads that as "nothing chosen"). */
async function pickAndImport(accept, options) {
  const file = await pickFile(accept);
  if (!file) return null;
  return stageAndImport(file, options);
}

/** A file dropped on the window, by its ticket. The UI's drop targets always need a result. */
async function importTicket(ticket, options) {
  const file = tickets.get(ticket);
  tickets.delete(ticket);
  if (!file) throw 'Die Dateifreigabe ist abgelaufen. Bitte die Datei erneut hineinziehen.';
  const imported = await stageAndImport(file, options);
  if (imported === null) throw 'Keine Datei verfügbar';
  return imported;
}

/** Puts the file in the core's inbox, imports it through the core, and removes the copy again. */
async function stageAndImport(file, options) {
  // Each import gets a folder of its own, so the file keeps its name (the palace takes its name from it).
  const name = `${crypto.randomUUID()}/${file.name.replace(/[^\w.-]/g, '_')}`;
  const path = `inbox/${name}`;
  fileSystem.putScratch(path, new Uint8Array(await file.arrayBuffer()));
  try {
    const reply = command('import_file', { name, kind: options.kind, target: options.target ?? null });
    if (reply.err !== undefined) throw reply.err;
    return reply.ok;
  } finally {
    fileSystem.removeScratch(path);
    persist();
  }
}

/**
 * Runs an export command. The core writes the file into its outbox; the page saves it to the device
 * and removes the copy. Returns the status message the UI shows.
 */
async function exportFile(name, args) {
  const reply = command(name, args);
  if (reply.err !== undefined) throw reply.err;
  const { message, file } = reply.ok;
  const path = inFolder(file);
  const bytes = fileSystem.readFile(path);
  fileSystem.removeScratch(path);
  persist();
  const link = document.createElement('a');
  link.href = URL.createObjectURL(new Blob([bytes], { type: 'application/octet-stream' }));
  link.download = path.split('/').pop();
  link.click();
  setTimeout(() => URL.revokeObjectURL(link.href), 60000);
  return message;
}

/**
 * Shows the due messages as system notifications, then tells the core which ones were shown, so
 * the queue drops them. A message whose notification was refused stays queued. Like the desktop
 * app, the UI itself gets no list.
 */
async function deliverNotifications() {
  const reply = command('notifications', {});
  if (reply.err !== undefined) throw reply.err;
  for (const item of reply.ok) {
    if (await showNotification(item)) command('notification_shown', { notification: item });
  }
  return [];
}

async function showNotification(item) {
  if (!('Notification' in window)) return false;
  let permission = Notification.permission;
  if (permission === 'default') permission = await Notification.requestPermission().catch(() => 'denied');
  if (permission !== 'granted') return false;
  const options = { body: item.body, tag: item.channel, data: { action: item.action }, icon: 'shell/icons/icon-192.png' };
  const registration = await navigator.serviceWorker?.getRegistration();
  if (registration) await registration.showNotification(item.title, options);
  else new Notification(item.title, options);
  return true;
}

/**
 * The URL of a media path, as the UI shows it: images and videos as blob URLs of the stored bytes;
 * animated graphics as their own frame document.
 */
function convertFileSrc(path) {
  if (path.startsWith('sply/')) return `shell/graphic.html?hash=${path.split('/')[1]}`;
  if (mediaUrls.has(path)) return mediaUrls.get(path);
  const reply = core ? core('media_file', { hash: path.split('/')[1] }) : { err: 'not ready' };
  if (reply.err !== undefined) return '';
  const bytes = fileSystem.readFile(inFolder(reply.ok.path));
  if (!bytes) return '';
  const url = URL.createObjectURL(new Blob([bytes], { type: reply.ok.mime }));
  mediaUrls.set(path, url);
  return url;
}

// An animated graphic's own document asks for its SVG. Stored graphics come from the core; the
// app's static graphics (the splash) are its own files, fetched here, never from elsewhere.
window.addEventListener('message', (event) => {
  if (event.data?.locoraGraphic !== 'ready' || !event.source) return;
  const frame = [...document.querySelectorAll('iframe')].find((element) => element.contentWindow === event.source);
  if (!frame) return;
  const { hash, file } = event.data;
  if (typeof file === 'string') {
    if (!/^splash\/[a-z-]+\.svg$/.test(file)) return;
    fetch(file)
      .then((response) => (response.ok ? response.text() : null))
      .then((svg) => svg && event.source.postMessage({ locoraGraphic: 'svg', svg }, '*'));
    return;
  }
  if (!core || !/^[0-9a-f]{64}$/.test(String(hash))) return;
  const reply = core('sply_svg', { hash, markers: false });
  if (reply.ok !== undefined) event.source.postMessage({ locoraGraphic: 'svg', svg: reply.ok }, '*');
});

window.addEventListener('dragover', (event) => event.preventDefault());
window.addEventListener('drop', (event) => {
  event.preventDefault();
  for (const file of event.dataTransfer?.files ?? []) {
    const ticket = crypto.randomUUID();
    tickets.set(ticket, file);
    window.dispatchEvent(new CustomEvent('locora-file-drop', { detail: { ticket } }));
  }
});

// Installable: when the browser offers the app for the home screen, a button offers it too.
let installPrompt = null;
window.addEventListener('beforeinstallprompt', (event) => {
  event.preventDefault();
  installPrompt = event;
  const button = document.createElement('button');
  button.textContent = 'Install Locora';
  button.setAttribute('style', 'position:fixed;right:12px;bottom:12px;z-index:9;padding:8px 12px;border-radius:8px;border:0;background:#3a3c8c;color:#fff;font:inherit;cursor:pointer');
  button.addEventListener('click', async () => {
    button.remove();
    await installPrompt.prompt();
    installPrompt = null;
  });
  document.body.append(button);
});

window.__TAURI__ = { core: { invoke, convertFileSrc } };
