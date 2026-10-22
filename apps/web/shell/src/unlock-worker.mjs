// Checks the encryption password off the page: Argon2 needs 64 MiB and a second or more, which
// would freeze the page. A second, empty instance of the core does only this step
// (`cloud_unlock`) and answers with the account key; the worker ends after one request.
import { MemFs } from './wasi-memfs.mjs';
import { instantiateCore } from './core.mjs';

self.onmessage = async ({ data }) => {
  try {
    const wasm = await (await fetch(new URL('../core/locora_web_core.wasm', import.meta.url))).arrayBuffer();
    const core = await instantiateCore(wasm, { fs: new MemFs() });
    self.postMessage(core('cloud_unlock', { bundle: data.bundle, passphrase: data.passphrase }));
  } catch (error) {
    // The page then lets the core check the password itself.
    self.postMessage({ fail: String(error?.message ?? error) });
  }
};
