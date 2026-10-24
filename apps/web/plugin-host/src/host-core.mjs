// The browser side of Locora's plugin host. It follows the rules of the native host (locora-host):
//  - plugin storage needs the `storage` permission, and each plugin sees only its own keys;
//  - a plugin is only called after `activate`, and events it publishes are delivered after the
//    call that published them has returned;
//  - filters run in priority order (lower first, ties by plugin id).
// WIT calls are synchronous, so plugin storage is loaded into memory before a plugin starts,
// and changes are collected as `dirty` entries for the app to write to IndexedDB.

const ok = (val) => ({ tag: 'ok', val });
const err = (val) => ({ tag: 'err', val: String(val) });

/** Services only the host answers; a plugin cannot provide them (as in the native host). */
const RESERVED = new Set(['core.palaces@1', 'ui.highlight@1', 'storage.list@1']);
/** Most entries in one `storage.list@1` reply, and the longest prefix (locora_api::storage). */
const LIST_PAGE = 5000;
const MAX_PREFIX = 200;
const encoder = new TextEncoder();
const decoder = new TextDecoder();

// Postcard, as far as `storage.list@1` needs it: unsigned varints, strings and byte lists.
function readVarint(bytes, at) {
  let value = 0n;
  let shift = 0n;
  for (;;) {
    if (at >= bytes.length) throw new Error('the request ends too early');
    const byte = bytes[at++];
    value |= BigInt(byte & 0x7f) << shift;
    if ((byte & 0x80) === 0) return [value, at];
    shift += 7n;
  }
}

function writeVarint(out, value) {
  let rest = BigInt(value);
  do {
    let byte = Number(rest & 0x7fn);
    rest >>= 7n;
    if (rest > 0n) byte |= 0x80;
    out.push(byte);
  } while (rest > 0n);
}

/** `ListRequest { prefix: String, after_rev: u64 }` from its postcard bytes. */
export function decodeListRequest(bytes) {
  const [length, at] = readVarint(bytes, 0);
  const end = at + Number(length);
  if (end > bytes.length) throw new Error('the request ends too early');
  const prefix = decoder.decode(bytes.subarray(at, end));
  const [afterRev] = readVarint(bytes, end);
  return { prefix, afterRev };
}

/** `ListResponse { entries: Vec<ListEntry { key, value }>, rev: u64, more: bool }` as postcard. */
export function encodeListResponse({ entries, rev, more }) {
  const out = [];
  writeVarint(out, entries.length);
  for (const [key, value] of entries) {
    const name = encoder.encode(key);
    writeVarint(out, name.length);
    out.push(...name);
    writeVarint(out, value.length);
    out.push(...value);
  }
  writeVarint(out, rev);
  out.push(more ? 1 : 0);
  return new Uint8Array(out);
}

const sameBytes = (a, b) => a !== undefined && a.length === b.length && a.every((byte, i) => byte === b[i]);

/**
 * Runs one plugin export. jco returns the value of an `ok` and throws the payload of an `err`
 * (a ComponentError); this turns both into WIT results.
 */
function settle(run) {
  try {
    return ok(run());
  } catch (error) {
    return err(error?.payload ?? error?.message ?? error);
  }
}
const MAX_DELIVERIES = 10_000; // a loop of events stops here instead of hanging the page

export class PluginHost {
  constructor() {
    this.plugins = new Map();      // id -> record
    this.topics = new Map();       // topic -> Set of plugin ids
    this.services = new Map();     // service -> plugin id
    this.hostServices = new Map(); // service -> (callerId, Uint8Array) => Uint8Array, synchronous
    this.pending = [];             // events published but not yet delivered
    this.logs = [];                // { plugin, level, message }
    this.onPublish = null;         // (plugin id, topic, payload) for the core, set by the app
    // A plugin's own keys that changed after a revision, as the native host lists them.
    this.provideHostService('storage.list@1', (plugin, request) => this.list(plugin, request));
  }

  /**
   * Registers a transpiled plugin module under its id (the id is checked against the manifest).
   * Its storage lives here, starting with `initialStorage` (written to IndexedDB by the app), unless
   * `synced` is given: then every key is read and written through it – the core's own store, which
   * logs and syncs the changes – as `{ get(key), set(key, value), list(request) }`.
   */
  register(id, module, initialStorage = new Map(), synced = null) {
    if (this.plugins.has(id)) throw new Error(`plugin ${id} is already registered`);
    const store = new Map(initialStorage);
    // Every key gets a revision: storage.list@1 hands out what changed after one.
    const revs = new Map([...store.keys()].map((key, index) => [key, index + 1]));
    this.plugins.set(id, {
      id, module, manifest: null, active: false,
      store, revs, rev: revs.size, synced, dirty: new Set(),
      subscriptions: new Set(), provided: new Set(), filters: new Map(),
    });
  }

  /** `storage.list@1` for the plugin `id` (postcard request in, postcard reply out). */
  list(id, request) {
    const plugin = this.require(id);
    if (!plugin.manifest?.permissions?.includes('storage')) throw new Error('the storage permission is missing');
    if (plugin.synced) return plugin.synced.list(request);
    const { prefix, afterRev } = decodeListRequest(request);
    if (encoder.encode(prefix).length > MAX_PREFIX) throw new Error('the prefix is too long');
    const changed = [...plugin.revs]
      .filter(([key, rev]) => BigInt(rev) > afterRev && key.startsWith(prefix))
      .sort((a, b) => a[1] - b[1]);
    const more = changed.length > LIST_PAGE;
    const page = changed.slice(0, LIST_PAGE);
    const rev = more ? page[page.length - 1][1] : Math.max(plugin.rev, Number(afterRev));
    return encodeListResponse({ entries: page.map(([key]) => [key, plugin.store.get(key) ?? new Uint8Array()]), rev, more });
  }

  /** Activates a plugin: reads its manifest, checks the id, runs `activate`, delivers what it published. */
  start(id) {
    const plugin = this.require(id);
    plugin.manifest = plugin.module.manifest();
    if (plugin.manifest.id !== id) throw new Error(`plugin registered as ${id} identifies as ${plugin.manifest.id}`);
    const result = settle(() => plugin.module.activate());
    if (result.tag === 'err') throw new Error(`plugin ${id} did not activate: ${result.val}`);
    plugin.active = true;
    this.drain();
  }

  /** Stops a plugin: its subscriptions and services are dropped. */
  stop(id) {
    const plugin = this.require(id);
    if (plugin.active) plugin.module.deactivate();
    for (const topic of plugin.subscriptions) this.topics.get(topic)?.delete(id);
    for (const service of plugin.provided) if (this.services.get(service) === id) this.services.delete(service);
    plugin.subscriptions.clear();
    plugin.provided.clear();
    plugin.filters.clear();
    plugin.active = false;
    this.drain();
  }

  /** Publishes an event from outside any plugin (e.g. the app after a journey). */
  publish(topic, payload) {
    this.pending.push({ topic, payload: new Uint8Array(payload), from: null });
    this.drain();
  }

  /** Calls the plugin that provides `service`. Returns the plugin's result, as WIT defines it. */
  callService(service, request) {
    const id = this.services.get(service);
    if (!id) return err(`no plugin provides ${service}`);
    const plugin = this.require(id);
    const result = settle(() => plugin.module.onCall(service, new Uint8Array(request)));
    this.drain();
    return result;
  }

  /**
   * What the core calls to reach plugin services (its `locora_bridge` imports). `plugin` is null for
   * any plugin. Replies are WIT results.
   */
  asBridge() {
    return {
      // Events of the core reach the plugins at once, as in the native host.
      publish: (topic, payload) => {
        this.publish(topic, payload);
        return true;
      },
      provides: (plugin, service) => (plugin ? this.services.get(service) === plugin : this.services.has(service)),
      call: (plugin, service, request) => {
        if (plugin && this.services.get(service) !== plugin) return err(`plugin ${plugin} does not provide ${service}`);
        return this.callService(service, request);
      },
      filter: (hook, value) => ok(this.filter(hook, value)),
    };
  }

  /** Runs the filters registered for `hook` in order; each filter's output is the next one's input. */
  filter(hook, value) {
    let current = new Uint8Array(value);
    const chain = [...this.plugins.values()]
      .filter((p) => p.active && p.filters.has(hook))
      .sort((a, b) => a.filters.get(hook) - b.filters.get(hook) || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
    for (const plugin of chain) {
      // As in the native host: a failing filter keeps the value it was given, and the chain goes on.
      const result = settle(() => plugin.module.onFilter(hook, current));
      if (result.tag === 'err') {
        this.logs.push({ plugin: plugin.id, level: 'error', message: `filter ${hook}: ${result.val}` });
        continue;
      }
      current = new Uint8Array(result.val);
      this.drain();
    }
    return current;
  }

  /** Storage changes since the last call, for the app to write to IndexedDB. */
  takeDirty(id) {
    const plugin = this.require(id);
    const changes = [...plugin.dirty].map((key) => [key, plugin.store.get(key)]);
    plugin.dirty.clear();
    return changes;
  }

  /** Binds a native-style service to plugins: `call-host` requests are answered synchronously. */
  provideHostService(name, handler) {
    this.hostServices.set(name, handler);
  }

  /** Delivers published events to their subscribers until nothing is pending. */
  drain() {
    let deliveries = 0;
    while (this.pending.length > 0) {
      if (++deliveries > MAX_DELIVERIES) throw new Error('event loop stopped: too many deliveries');
      const event = this.pending.shift();
      for (const id of [...(this.topics.get(event.topic) ?? [])]) {
        const plugin = this.plugins.get(id);
        if (!plugin?.active) continue;
        const result = settle(() => plugin.module.onEvent(event.topic, event.payload));
        if (result.tag === 'err') this.logs.push({ plugin: id, level: 'error', message: `onEvent ${event.topic}: ${result.val}` });
      }
    }
  }

  /** The host functions one plugin imports. Each call is attributed to `id`. */
  bindingsFor(id) {
    const host = this;
    const plugin = () => host.require(id);
    const storage = () => plugin().manifest?.permissions?.includes('storage');
    return {
      log(level, message) {
        host.logs.push({ plugin: id, level, message: String(message) });
      },
      subscribe(topic) {
        const p = plugin();
        p.subscriptions.add(topic);
        if (!host.topics.has(topic)) host.topics.set(topic, new Set());
        host.topics.get(topic).add(id);
      },
      publish(topic, payload) {
        host.pending.push({ topic, payload: new Uint8Array(payload), from: id });
        // The core hears every plugin event too: the UI declarations of plugins are read from them.
        host.onPublish?.(id, topic, new Uint8Array(payload));
      },
      provide(service) {
        if (RESERVED.has(service)) {
          host.logs.push({ plugin: id, level: 'warn', message: `${service} is reserved for the host` });
          return ok(undefined);
        }
        const owner = host.services.get(service);
        if (owner && owner !== id) return err(`${service} is already provided by ${owner}`);
        host.services.set(service, id);
        plugin().provided.add(service);
        return ok(undefined);
      },
      registerFilter(hook, priority) {
        plugin().filters.set(hook, priority);
        return ok(undefined);
      },
      callHost(service, request) {
        const handler = host.hostServices.get(service);
        if (!handler) return err(`unknown host service ${service}`);
        try {
          return ok(handler(id, new Uint8Array(request)));
        } catch (error) {
          return err(error?.message ?? error);
        }
      },
      kvGet(key) {
        if (!storage()) return err('the storage permission is missing');
        const p = plugin();
        try {
          const value = p.synced ? p.synced.get(key) : p.store.get(key);
          return ok(value === undefined ? undefined : new Uint8Array(value));
        } catch (error) {
          return err(error?.message ?? error);
        }
      },
      kvSet(key, value) {
        if (!storage()) return err('the storage permission is missing');
        const p = plugin();
        const bytes = new Uint8Array(value);
        if (p.synced) {
          try {
            p.synced.set(key, bytes);
            return ok(undefined);
          } catch (error) {
            return err(error?.message ?? error);
          }
        }
        // The same bytes again are no change (as in the core's store).
        if (sameBytes(p.store.get(key), bytes)) return ok(undefined);
        p.store.set(key, bytes);
        p.rev += 1;
        p.revs.set(key, p.rev);
        p.dirty.add(key);
        return ok(undefined);
      },
    };
  }

  require(id) {
    const plugin = this.plugins.get(id);
    if (!plugin) throw new Error(`plugin ${id} is not registered`);
    return plugin;
  }
}

/** The host used by the generated plugin shims. The app and the tests share this one instance. */
export const host = new PluginHost();

/** The functions a plugin with this id imports (used by the generated shim files). */
export function hostFor(id) {
  return host.bindingsFor(id);
}
