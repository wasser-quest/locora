// Loads the browser core (locora_web_core.wasm, a WASI reactor module) and calls its commands.
// The page and the Node tests use this one loader: the file system comes from wasi-memfs.mjs and
// plugin services from `bridge`, which has the shape of PluginHost.asBridge() in plugin-host.
//
//   bridge.provides(plugin | null, service) -> boolean
//   bridge.call(plugin | null, service, request: Uint8Array) -> { tag: 'ok', val } | { tag: 'err', val }

const NO_PLUGINS = { provides: () => false, call: () => ({ tag: 'err', val: 'no plugins are connected' }) };
// `plugin` is null when a call may go to any plugin; the core sends an empty name for that.

/** Instantiates the core. `fs` is a MemFs; the core's folder is its preopened `/locora`. */
export async function instantiateCore(wasmBytes, { fs, bridge = NO_PLUGINS }) {
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();
  let core; // the instance's exports; the bridge only runs once the module is instantiated

  const text = (pointer, length) => decoder.decode(new Uint8Array(core.memory.buffer, pointer, length));
  const optionalText = (pointer, length) => (length === 0 ? null : text(pointer, length));

  const callBridge = (plugin, service, request) => {
    try {
      return bridge.call(plugin, service, request);
    } catch (error) {
      return { tag: 'err', val: String(error?.message ?? error) };
    }
  };

  // A filter that fails keeps the value it was given (the plugin host logs it).
  const runFilter = (hook, value) => {
    try {
      return bridge.filter(hook, value);
    } catch {
      return { tag: 'ok', val: value };
    }
  };

  /** Hands a WIT result to the core: one tag byte (0 = ok, 1 = error), then the bytes or the message. */
  const reply = (result) => {
    const body = result.tag === 'ok' ? new Uint8Array(result.val) : encoder.encode(result.val);
    const bytes = new Uint8Array(body.length + 1);
    bytes[0] = result.tag === 'ok' ? 0 : 1;
    bytes.set(body, 1);
    const pointer = core.alloc(bytes.length);
    new Uint8Array(core.memory.buffer, pointer, bytes.length).set(bytes);
    return (BigInt(pointer) << 32n) | BigInt(bytes.length);
  };

  const imports = {
    wasi_snapshot_preview1: fs.imports(),
    locora_bridge: {
      provides: (pluginPointer, pluginLength, servicePointer, serviceLength) =>
        (bridge.provides(optionalText(pluginPointer, pluginLength), text(servicePointer, serviceLength)) ? 1 : 0),
      call: (pluginPointer, pluginLength, servicePointer, serviceLength, requestPointer, requestLength) => {
        const plugin = optionalText(pluginPointer, pluginLength);
        const service = text(servicePointer, serviceLength);
        // Copied first: reserving the reply may grow the memory and detach the old buffer.
        const request = new Uint8Array(core.memory.buffer.slice(requestPointer, requestPointer + requestLength));
        return reply(callBridge(plugin, service, request));
      },
      filter: (hookPointer, hookLength, valuePointer, valueLength) => {
        const hook = text(hookPointer, hookLength);
        const value = new Uint8Array(core.memory.buffer.slice(valuePointer, valuePointer + valueLength));
        return reply(runFilter(hook, value));
      },
    },
  };
  const { instance } = await WebAssembly.instantiate(wasmBytes, imports);
  core = instance.exports;
  fs.memory = core.memory;
  if (typeof core._initialize === 'function') core._initialize();
  const { alloc, dealloc, invoke } = core;

  const write = (value) => {
    const bytes = encoder.encode(value);
    const pointer = alloc(bytes.length);
    new Uint8Array(core.memory.buffer, pointer, bytes.length).set(bytes);
    return { pointer, length: bytes.length };
  };

  /**
   * The core's entry points for plugins that call back while a command runs (see palace_service,
   * plugin_highlight and plugin_publish in the core). Inputs are byte arrays; the result is a WIT result.
   */
  const nested = (name, ...inputs) => {
    const written = inputs.map((value) => {
      const bytes = typeof value === 'string' ? encoder.encode(value) : value;
      const pointer = alloc(bytes.length);
      new Uint8Array(core.memory.buffer, pointer, bytes.length).set(bytes);
      return { pointer, length: bytes.length };
    });
    const packed = core[name](...written.flatMap((w) => [w.pointer, w.length]));
    for (const w of written) dealloc(w.pointer, w.length);
    const pointer = Number(packed >> 32n);
    const length = Number(packed & 0xffffffffn);
    const bytes = new Uint8Array(core.memory.buffer, pointer, length).slice();
    dealloc(pointer, length);
    return bytes[0] === 0 ? { tag: 'ok', val: bytes.subarray(1) } : { tag: 'err', val: decoder.decode(bytes.subarray(1)) };
  };

  /** Runs one command and returns the parsed reply: { ok } or { err }. */
  const call = function call(command, args = {}) {
    const c = write(command);
    const a = write(JSON.stringify(args));
    const packed = invoke(c.pointer, c.length, a.pointer, a.length);
    dealloc(c.pointer, c.length);
    dealloc(a.pointer, a.length);
    const pointer = Number(packed >> 32n);
    const length = Number(packed & 0xffffffffn);
    const output = decoder.decode(new Uint8Array(core.memory.buffer, pointer, length));
    dealloc(pointer, length);
    return JSON.parse(output);
  };
  call.palaceService = (request) => nested('palace_service', request);
  call.highlight = (plugin, request) => nested('plugin_highlight', plugin, request);
  call.publish = (plugin, topic, payload) => nested('plugin_publish', plugin, topic, payload);
  return call;
}
