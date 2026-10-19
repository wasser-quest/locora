// The browser side of Locora's plugin host. It follows the rules of the native host (locora-host):
//  - plugin storage needs the `storage` permission, and each plugin sees only its own keys;
//  - a plugin is only called after `activate`, and events it publishes are delivered after the
//    call that published them has returned;
//  - filters run in priority order (lower first, ties by plugin id).
// WIT calls are synchronous, so plugin storage is loaded into memory before a plugin starts,
// and changes are collected as `dirty` entries for the app to write to IndexedDB.

const ok = (val) => ({ tag: 'ok', val });
const err = (val) => ({ tag: 'err', val: String(val) });

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
  }

  /** Registers a transpiled plugin module under its id (the id is checked against the manifest). */
  register(id, module, initialStorage = new Map()) {
    if (this.plugins.has(id)) throw new Error(`plugin ${id} is already registered`);
    this.plugins.set(id, {
      id, module, manifest: null, active: false,
      store: new Map(initialStorage), dirty: new Set(),
      subscriptions: new Set(), provided: new Set(), filters: new Map(),
    });
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
        const value = plugin().store.get(key);
        return ok(value === undefined ? undefined : new Uint8Array(value));
      },
      kvSet(key, value) {
        if (!storage()) return err('the storage permission is missing');
        const p = plugin();
        p.store.set(key, new Uint8Array(value));
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
