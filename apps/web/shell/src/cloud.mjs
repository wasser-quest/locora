// The cloud client of the browser. The core runs the cloud commands as steps: each step either
// finishes or asks for HTTP calls (one, or a few side by side for media). This module sends those
// calls with fetch and hands the replies back to the core, one command at a time, so two cloud
// commands never share the core's waiting step.
//
// Auto-sync keeps the account key on this device: wrapped by an AES-GCM key that WebCrypto
// created as not extractable, both in IndexedDB. Script on this page can use the wrapping key but
// never read it; the stored ciphertext alone is useless elsewhere. The key is forgotten when
// auto-sync is turned off, on sign-out, and when the cloud data or the account is deleted.
//
// The encryption password is checked with Argon2 (64 MiB, a second or more) in a worker
// (unlock-worker.mjs), so the page does not freeze; without workers the core does it itself.

let lane = Promise.resolve();

/**
 * Runs one cloud command to its result. `core` is the instance's call function (see core.mjs), and
 * `persist` saves the core's files after each step (the cloud account is one of them).
 */
export function runCloud(core, persist, command, args) {
  const run = lane.then(() => handle(core, persist, command, args));
  lane = run.catch(() => {});
  return run;
}

async function handle(core, persist, command, args = {}) {
  const drive = (name, values) => step(core, persist, name, values);
  switch (command) {
    case 'cloud_sync': {
      if (args.key) throw 'Ungültige Anfrage';
      // Refused before the password is checked or anything is sent, as the core does.
      if (!core('cloud_status', {}).ok?.done?.access) {
        throw 'Keine gültige Cloud-Freischaltung. Abo prüfen und Freischaltung aktualisieren';
      }
      const key = await derive(core, persist, args.passphrase);
      if (key === null) return drive('cloud_sync', args);
      return drive('cloud_sync', { key, confirmed: args.confirmed ?? [] });
    }
    case 'cloud_auto_enable': {
      if (!keysAvailable()) throw 'Dieser Browser kann den Schlüssel nicht sicher aufbewahren. Auto-Sync ist hier nicht möglich.';
      let key = await derive(core, persist, args.passphrase);
      if (key === null) {
        // No worker: the core checks the password itself.
        key = (await drive('cloud_unlock', { bundle: await drive('cloud_keys_bundle', {}), passphrase: args.passphrase })).key;
      }
      await keepKey(await entryName(core), key);
      return drive('cloud_auto_set', { enabled: true });
    }
    case 'cloud_auto_disable':
      await forgetKey();
      return drive('cloud_auto_set', { enabled: false });
    case 'cloud_auto_state': {
      const state = await drive('cloud_auto_state', {});
      return { ...state, enabled: state.enabled && (await keptKey(await entryName(core))) !== null };
    }
    case 'cloud_auto_sync': {
      const key = await keptKey(await entryName(core));
      if (key === null) {
        const state = await drive('cloud_auto_state', {});
        if (state.enabled) await drive('cloud_auto_set', { enabled: false });
        return { enabled: false, result: null, confirm: [], paused: null, error: null, rate_limited: false };
      }
      return drive('cloud_sync', { key, confirmed: args.confirmed ?? [], auto: true });
    }
    case 'cloud_logout':
    case 'cloud_delete_account':
    case 'cloud_erase': {
      const result = await drive(command, args);
      await forgetKey();
      return result;
    }
    default:
      return drive(command, args);
  }
}

/** Runs a core command and sends the calls it asks for until it is done. */
async function step(core, persist, command, args) {
  let reply = core(command, args);
  for (;;) {
    persist();
    if (reply.err !== undefined) throw reply.err;
    const next = reply.ok;
    if (next.done !== undefined) return next.done;
    if (next.call !== undefined) {
      reply = core.cloudReply(...(await send(next.call, core.cloudBody())));
      continue;
    }
    // Calls side by side: their bodies in order, the replies handed back in the same order.
    const bodies = next.calls.map(() => core.cloudBody());
    const replies = await Promise.all(next.calls.map((call, index) => send(call, bodies[index])));
    for (const [status, body] of replies) reply = core.cloudReply(status, body);
  }
}

/** Sends one call. A call that never gets an answer is reported as status 0. */
async function send(call, bytes) {
  const headers = {};
  if (call.token) headers.authorization = `Bearer ${call.token}`;
  let body;
  if (call.body !== 'none') {
    body = bytes;
    headers['content-type'] = call.body === 'json' ? 'application/json' : 'application/octet-stream';
  }
  try {
    const response = await fetch(call.url, {
      method: call.method,
      headers,
      body,
      signal: AbortSignal.timeout(call.timeout * 1000),
    });
    return [response.status, new Uint8Array(await response.arrayBuffer())];
  } catch {
    return [0, new Uint8Array(0)];
  }
}

/**
 * The account key from the password, derived in a worker: the wrapped keys come from the server
 * first. `null` when this browser has no module workers (the core then derives it).
 */
async function derive(core, persist, passphrase) {
  if (typeof Worker !== 'function') return null;
  const bundle = await step(core, persist, 'cloud_keys_bundle', {});
  let worker;
  try {
    worker = new Worker(new URL('./unlock-worker.mjs', import.meta.url), { type: 'module' });
  } catch {
    return null;
  }
  try {
    const reply = await new Promise((resolve, reject) => {
      worker.onmessage = (event) => resolve(event.data);
      worker.onerror = (event) => reject(event);
      worker.postMessage({ bundle, passphrase });
    });
    if (reply.fail !== undefined) return null;
    if (reply.err !== undefined) throw reply.err;
    return reply.ok.done.key;
  } catch (error) {
    if (typeof error === 'string') throw error;
    return null;
  } finally {
    worker.terminate();
  }
}

// ---------------------------------------------------------------- the kept key

const DB = 'locora-cloud-key';
const STORE = 'keys';
const ENTRY = 'auto-sync';

function keysAvailable() {
  return typeof indexedDB !== 'undefined' && !!globalThis.crypto?.subtle;
}

/** The account this key belongs to: a kept key of another account is never used. */
async function entryName(core) {
  const status = core('cloud_status', {});
  const done = status.ok?.done ?? {};
  return `${done.server ?? ''} ${done.account ?? ''}`;
}

function database() {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB, 1);
    request.onupgradeneeded = () => request.result.createObjectStore(STORE);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

async function transact(mode, work) {
  const db = await database();
  try {
    return await new Promise((resolve, reject) => {
      const tx = db.transaction(STORE, mode);
      const request = work(tx.objectStore(STORE));
      tx.oncomplete = () => resolve(request?.result);
      tx.onerror = () => reject(tx.error);
      tx.onabort = () => reject(tx.error);
    });
  } finally {
    db.close();
  }
}

const hexToBytes = (hex) => new Uint8Array(hex.match(/../g).map((pair) => parseInt(pair, 16)));
const bytesToHex = (bytes) => Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('');

async function keepKey(account, keyHex) {
  const wrapping = await crypto.subtle.generateKey({ name: 'AES-GCM', length: 256 }, false, ['encrypt', 'decrypt']);
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const plain = hexToBytes(keyHex);
  const sealed = new Uint8Array(await crypto.subtle.encrypt({ name: 'AES-GCM', iv, additionalData: new TextEncoder().encode(account) }, wrapping, plain));
  plain.fill(0);
  await transact('readwrite', (store) => store.put({ account, wrapping, iv, sealed }, ENTRY));
}

async function keptKey(account) {
  if (!keysAvailable()) return null;
  try {
    const kept = await transact('readonly', (store) => store.get(ENTRY));
    if (!kept || kept.account !== account) return null;
    const plain = new Uint8Array(await crypto.subtle.decrypt({ name: 'AES-GCM', iv: kept.iv, additionalData: new TextEncoder().encode(account) }, kept.wrapping, kept.sealed));
    const hex = bytesToHex(plain);
    plain.fill(0);
    return hex;
  } catch {
    return null;
  }
}

async function forgetKey() {
  if (!keysAvailable()) return;
  try {
    await transact('readwrite', (store) => store.delete(ENTRY));
  } catch (error) {
    console.warn('the kept cloud key could not be removed', error);
  }
}
