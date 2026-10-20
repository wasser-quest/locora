# Locora plugin host (browser)

Runs Locora's official plugins (`core`, `notify`, `review`, `tutorial`, `gamification`) in the browser
with the browser's own WebAssembly engine, instead of wasmtime. All plugin data stays on the device:
storage is IndexedDB, and nothing is sent anywhere.

## Use

    npm install
    npm run transpile -- <path to plugins/target/wasm32-wasip2/release/*.wasm>
    npm test

`transpile` turns each component into an ES module (with `jco`) and writes `dist/<plugin>/host.mjs`,
which connects the plugin's host imports to `src/host-core.mjs`.

## Rules (the same as the native host)

- Storage needs the `storage` permission; each plugin sees only its own keys.
- A plugin is called only after it has been activated.
- Events are delivered after the call that published them returns.
- Filters run in priority order.

## Not yet

- `call-host` services that read palaces (`core.palaces@1` and friends) need the Rust core in the
  browser. They return an error until a handler is registered with `provideHostService`.
- The Rust core (`locora-runtime`) does not build for the browser yet: `locora-host` (wasmtime) is
  still required, so the plugin host here is not yet connected to the rest of the app.
