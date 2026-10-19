// Loads the browser core (locora_web_core.wasm, a WASI module) and calls its commands.
// Node has a WASI runtime built in; the browser uses a WASI shim with the same imports.
// `bridge` runs the plugin services: { provides(service) -> bool, call(service, bytes) -> WIT result }.
import { readFile } from 'node:fs/promises';
import { WASI } from 'node:wasi';

const NO_PLUGINS = { provides: () => false, call: () => ({ tag: 'err', val: 'no plugins are connected' }) };
// `plugin` is null when a call may go to any plugin; the core sends an empty name for that.

export async function loadCore(wasmPath, dataDir, bridge = NO_PLUGINS) {
  const wasi = new WASI({ version: 'preview1', args: [], env: {}, preopens: { '/locora': dataDir } });
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();
  let core; // the instance's exports; the bridge only runs once the module is instantiated

  const imports = {
    wasi_snapshot_preview1: wasi.wasiImport,
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
    },
  };
  const { instance } = await WebAssembly.instantiate(await readFile(wasmPath), imports);
  wasi.initialize(instance);
  core = instance.exports;
  const { memory, alloc, dealloc, invoke } = core;

  function text(pointer, length) {
    return decoder.decode(new Uint8Array(core.memory.buffer, pointer, length));
  }

  function optionalText(pointer, length) {
    return length === 0 ? null : text(pointer, length);
  }

  function callBridge(plugin, service, request) {
    try {
      return bridge.call(plugin, service, request);
    } catch (error) {
      return { tag: 'err', val: String(error?.message ?? error) };
    }
  }

  /** Hands a WIT result to the core: one tag byte (0 = ok, 1 = error), then the bytes or the message. */
  function reply(result) {
    const body = result.tag === 'ok' ? new Uint8Array(result.val) : encoder.encode(result.val);
    const bytes = new Uint8Array(body.length + 1);
    bytes[0] = result.tag === 'ok' ? 0 : 1;
    bytes.set(body, 1);
    const pointer = alloc(bytes.length);
    new Uint8Array(core.memory.buffer, pointer, bytes.length).set(bytes);
    return (BigInt(pointer) << 32n) | BigInt(bytes.length);
  }

  function write(text) {
    const bytes = encoder.encode(text);
    const pointer = alloc(bytes.length);
    new Uint8Array(memory.buffer, pointer, bytes.length).set(bytes);
    return { pointer, length: bytes.length };
  }

  /** Runs one command and returns the parsed reply: { ok } or { err }. */
  return function call(command, args = {}) {
    const c = write(command);
    const a = write(JSON.stringify(args));
    const packed = invoke(c.pointer, c.length, a.pointer, a.length);
    dealloc(c.pointer, c.length);
    dealloc(a.pointer, a.length);
    const pointer = Number(packed >> 32n);
    const length = Number(packed & 0xffffffffn);
    const output = decoder.decode(new Uint8Array(memory.buffer, pointer, length));
    dealloc(pointer, length);
    return JSON.parse(output);
  };
}
