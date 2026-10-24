// The core's WebAssembly module for the tests: the copy the build ships (apps/web/dist/core, made
// right after cargo built it), else the one cargo wrote (in CARGO_TARGET_DIR, or the repository's
// target folder).
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

export async function coreWasm() {
  try {
    return await readFile(new URL('../../dist/core/locora_web_core.wasm', import.meta.url));
  } catch {
    const target = process.env.CARGO_TARGET_DIR ?? fileURLToPath(new URL('../../../../target', import.meta.url));
    return readFile(join(target, 'wasm32-wasip1', 'release', 'locora_web_core.wasm'));
  }
}
