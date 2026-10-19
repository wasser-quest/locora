// Loads the browser core (locora_web_core.wasm, a WASI module) and calls its commands.
// Node has a WASI runtime built in; the browser uses a WASI shim with the same imports.
import { readFile } from 'node:fs/promises';
import { WASI } from 'node:wasi';

export async function loadCore(wasmPath, dataDir) {
  const wasi = new WASI({ version: 'preview1', args: [], env: {}, preopens: { '/locora': dataDir } });
  const { instance } = await WebAssembly.instantiate(await readFile(wasmPath), {
    wasi_snapshot_preview1: wasi.wasiImport,
  });
  wasi.initialize(instance);
  const { memory, alloc, dealloc, invoke } = instance.exports;
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();

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
    const text = decoder.decode(new Uint8Array(memory.buffer, pointer, length));
    dealloc(pointer, length);
    return JSON.parse(text);
  };
}
