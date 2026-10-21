// The browser side of the Locora app. The UI calls `window.__TAURI__.core` exactly as in the desktop
// app; this file provides that object for the browser, backed by the Rust core (WebAssembly),
// the official plugins (plugin host) and the device's own storage (IndexedDB). Nothing is sent
// anywhere: the only requests are for this site's own files.
//
// Loaded before the UI (see build.mjs), so the UI's first call already finds `__TAURI__`. Calls wait
// until the core is ready.

import { MemFs } from './wasi-memfs.mjs';
import { instantiateCore } from './core.mjs';
import { runCloud } from './cloud.mjs';
import { asMp4, isVideo } from './video.mjs';
import { openFileStorage, loadFiles, saveChanges } from './file-storage.mjs';
import { openStorage as openPluginStorage, loadPluginStorage, flushPluginStorage } from '../src/storage-idb.mjs';
import { host } from '../src/host-core.mjs';

const PLUGINS = ['core', 'notify', 'review', 'tutorial', 'gamification'];
const MEDIA_ACCEPT = 'image/*,video/*,.sply,.png,.jpg,.jpeg,.webp,.gif,.avif,.mp4,.m4v,.webm,.mov';
const ARCHIVE_ACCEPT = '.locora,.toml';
const AUDIO_ACCEPT = 'audio/*,.mp3,.m4a,.wav,.ogg,.oga,.opus';
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
  const wasm = await (await fetch('core/locora_web_core.wasm')).arrayBuffer();
  core = await instantiateCore(wasm, { fs: fileSystem, bridge: host.asBridge() });
  // The plugins reach the core while they run: palace reads, tour steps and the events they publish.
  host.provideHostService('core.palaces@1', (_plugin, request) => unwrap(core.palaceService(request)));
  host.provideHostService('ui.highlight@1', (plugin, request) => {
    if (!host.require(plugin).manifest?.permissions?.includes('ui:highlight')) {
      throw new Error('the ui:highlight permission is missing');
    }
    return unwrap(core.highlight(plugin, request));
  });
  host.onPublish = (plugin, topic, payload) => {
    try {
      unwrap(core.publish(plugin, topic, payload));
    } catch (error) {
      console.warn(`the core did not take an event of ${plugin}: ${error.message}`);
    }
  };
  for (const id of PLUGINS) {
    const module = await import(`../plugins/${id}/${id}.js`);
    host.register(id, module, await loadPluginStorage(pluginDb, id));
    host.start(id);
  }
  const opened = core('open', '/locora');
  if (opened.err) throw new Error(opened.err);
  // The core shows the tiles and blocks of the plugins that run here.
  const listed = core('set_plugins', { plugins: PLUGINS.map((id) => host.require(id).manifest) });
  if (listed.err) throw new Error(listed.err);
  for (const id of PLUGINS) await flushPluginStorage(pluginDb, id, host.takeDirty(id));
}

/** A WIT result as a value: the bytes, or an error thrown with the plugin's message. */
function unwrap(result) {
  if (result.tag === 'err') throw new Error(result.val);
  return result.val;
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
    case 'editor_pick_room':
      return pickAndImport(MEDIA_ACCEPT, { kind: 'room', palaceId: args.palaceId });
    case 'editor_pick_picture':
      return pickAndImport(MEDIA_ACCEPT, { kind: 'picture', target: args.target });
    case 'editor_pick_audio':
      return pickAndImport(AUDIO_ACCEPT, { kind: 'audio', target: args.target });
    case 'import_pick':
      return importFolder(`${ARCHIVE_ACCEPT},${MEDIA_ACCEPT}`);
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
      if (cmd.startsWith('cloud_')) return runCloud(core, persist, cmd, args);
      reply = command(cmd, args);
      if (reply.err !== undefined) throw reply.err;
      return reply.ok;
  }
}

/** Several files at once (a folder's source with its pictures and videos). */
function pickFiles(accept) {
  return new Promise((resolve) => {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = accept;
    input.multiple = true;
    input.hidden = true;
    document.body.append(input);
    const done = (files) => {
      input.remove();
      resolve(files);
    };
    input.addEventListener('change', () => done(Array.from(input.files ?? [])));
    input.addEventListener('cancel', () => done([]));
    input.click();
  });
}

/**
 * Imports a `.locora` file, or a `locora.toml` with the files it names (its pictures and videos are
 * chosen with it). The files are stored in one inbox folder under their own names, as on disk.
 */
async function importFolder(accept) {
  const files = await pickFiles(accept);
  if (files.length === 0) return null;
  const main = files.find((file) => /\.(locora|toml)$/i.test(file.name));
  if (!main) throw 'Choose a .locora file, or a locora.toml with its pictures and videos.';
  const folder = crypto.randomUUID();
  const staged = [];
  try {
    for (const file of files) {
      const path = `inbox/${folder}/${file.name}`;
      fileSystem.putScratch(path, new Uint8Array(await file.arrayBuffer()));
      staged.push(path);
    }
    const reply = command('import_file', { name: `${folder}/${main.name}`, kind: 'archive', target: null });
    if (reply.err !== undefined) throw reply.err;
    return reply.ok;
  } finally {
    for (const path of staged) fileSystem.removeScratch(path);
    persist();
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

/**
 * Puts the file in the core's inbox, imports it through the core, and removes the copy again. A video
 * becomes MP4 with H.264 first (see video.mjs); the core removes the metadata of every file. A sound
 * clip (kind `audio`) is never converted.
 */
async function stageAndImport(file, options) {
  if (options.kind !== 'audio' && isVideo(file)) file = await asMp4(file);
  // Each import gets a folder of its own, so the file keeps its name (the palace takes its name from it).
  const name = `${crypto.randomUUID()}/${file.name.replace(/[^\w.-]/g, '_')}`;
  const path = `inbox/${name}`;
  fileSystem.putScratch(path, new Uint8Array(await file.arrayBuffer()));
  try {
    const reply = command('import_file', { name, kind: options.kind, target: options.target ?? null, palaceId: options.palaceId ?? null });
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

// Chrome for Android: the browser closes while an animated graphic runs in its frame, so there a
// graphic is shown as an image (an image runs no script). The UI makes the same choice (sply.rs).
const holdsFirstFrame = /Android/.test(navigator.userAgent) && /Chrome\//.test(navigator.userAgent) && !/Firefox\//.test(navigator.userAgent);
const stillGraphics = new Map();

/** The SVG of a stored graphic as a blob URL, for an image element. */
function stillGraphicUrl(hash) {
  if (stillGraphics.has(hash)) return stillGraphics.get(hash);
  const reply = core ? core('sply_svg', { hash }) : { err: 'not ready' };
  if (reply.err !== undefined) return '';
  const url = URL.createObjectURL(new Blob([reply.ok], { type: 'image/svg+xml' }));
  stillGraphics.set(hash, url);
  return url;
}

/** The SVG text of a stored graphic, from the core; null when it is not available. */
function storedGraphicText(hash) {
  const reply = core ? core('sply_svg', { hash }) : { err: 'not ready' };
  return reply.err === undefined ? reply.ok : null;
}

// The UI shows a graphic inline (see splinely-mount.js): no frame of its own. `still` holds the first
// frame; `reply` gets the graphic's replies (its runtime, its timed playback), and the bridge
// (sply-bridge.js) runs the same protocol the frame did.
window.locoraMountGraphic = (element, path, still, reply) => {
  const text = path.startsWith('sply/') ? storedGraphicText(path.split('/')[1]) : fetch(path).then((response) => response.text());
  return Promise.resolve(text).then((svgText) => {
    if (!svgText || !window.SplinelyMount) return null;
    const graphic = window.SplinelyMount.mount(element, svgText, { autoTick: !still, focusRing: false });
    element.splinelyGraphic = graphic;
    element.splinelyListeners = [];
    if (graphic.svg && window.SplyBridge) {
      element.splinelyStop = window.SplyBridge.attach(graphic.svg, {
        reply: (message) => reply(message),
        onMessage: (listener) => element.splinelyListeners.push(listener),
      });
    }
    return graphic;
  });
};
// The app's messages for the graphic (state, timed commands, pointers) reach its bridge.
window.locoraSendGraphic = (element, message) => {
  (element.splinelyListeners || []).forEach((listener) => listener(message));
};
window.locoraUnmountGraphic = (element) => {
  element.splinelyStop?.();
  element.splinelyStop = null;
  element.splinelyGraphic?.destroy();
  element.splinelyGraphic = null;
  element.splinelyListeners = [];
};

/**
 * The URL of a media path, as the UI shows it: images and videos as blob URLs of the stored bytes;
 * animated graphics as their own frame document (as an image on Chrome for Android).
 */
function convertFileSrc(path) {
  if (path.startsWith('sply/')) {
    return holdsFirstFrame ? stillGraphicUrl(path.split('/')[1]) : `shell/graphic.html?hash=${path.split('/')[1]}`;
  }
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
  const reply = core('sply_svg', { hash });
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

// Installable: a button offers the app for the home screen. Chromium browsers make the offer
// (beforeinstallprompt) and the button starts it. Safari on iPhone and iPad makes no offer, so there
// the button explains the Share menu instead. The × hides the offer for good; the choice is kept on
// this device (localStorage), and nothing else changes.
const installWrapStyle = 'position:fixed;right:12px;bottom:80px;z-index:1000;display:flex;gap:6px;align-items:center;max-width:calc(100vw - 24px)';
const installStyle = 'padding:8px 12px;border-radius:8px;border:0;background:#3a3c8c;color:#fff;font:inherit;text-align:left;cursor:pointer';
const installCloseStyle = 'padding:6px 10px;border-radius:8px;border:0;background:#2a2c5c;color:#fff;font:inherit;cursor:pointer';
const installHintKey = 'locora-install-hint';
const installed = window.matchMedia('(display-mode: standalone)').matches || navigator.standalone === true;
const installHidden = () => {
  try {
    return localStorage.getItem(installHintKey) === 'hidden';
  } catch (_) {
    return false;
  }
};
let installPrompt = null;
let installOffered = false;
window.addEventListener('beforeinstallprompt', (event) => {
  event.preventDefault();
  installPrompt = event;
  installOffered = true;
  if (installHidden()) return;
  document.querySelectorAll('[data-install]').forEach((element) => element.remove());
  offerInstall('Install Locora', async () => {
    await installPrompt.prompt();
    installPrompt = null;
  });
});
if (/iPad|iPhone|iPod/.test(navigator.userAgent) && !installed && !installHidden()) {
  offerInstall('To install: tap Share, then Add to Home Screen', () => {});
}
// Chrome makes its own offer only when its checks pass (a worker in control, some engagement), and it
// may not make it at all. When it has not after a few seconds, the button shows where the menu has it.
if (/Android/.test(navigator.userAgent) && !installed) {
  setTimeout(() => {
    if (!installOffered && !installHidden()) offerInstall('Install Locora: menu ⋮ → Install app', () => {});
  }, 3000);
}

function offerInstall(label, onClick) {
  const wrap = document.createElement('div');
  wrap.dataset.install = '';
  wrap.setAttribute('style', installWrapStyle);
  const button = document.createElement('button');
  button.textContent = label;
  button.setAttribute('style', installStyle);
  button.addEventListener('click', () => {
    wrap.remove();
    onClick();
  });
  const close = document.createElement('button');
  close.textContent = '×';
  close.setAttribute('aria-label', 'Hide the install hint');
  close.setAttribute('style', installCloseStyle);
  close.addEventListener('click', () => {
    try {
      localStorage.setItem(installHintKey, 'hidden');
    } catch (_) {
      // Storage is not available: the hint is hidden for this page view only.
    }
    document.querySelectorAll('[data-install]').forEach((element) => element.remove());
  });
  wrap.append(button, close);
  document.body.append(wrap);
}

window.__TAURI__ = { core: { invoke, convertFileSrc } };
