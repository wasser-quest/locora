# Locora app

Tauri 2 backend (`src-tauri/`) and Leptos UI (`ui/`, WebAssembly via `trunk`), with
shared IPC types in `shared/`. Its own Cargo workspace, so the main workspace builds
without WebKit/GTK.

## Once: tools and system packages

```sh
rustup target add wasm32-unknown-unknown wasm32-wasip2
cargo install trunk --locked
cargo install tauri-cli --version "^2" --locked
# Linux (Fedora); Debian/Ubuntu: libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev librsvg2-dev libssl-dev
sudo dnf install webkit2gtk4.1-devel gtk3-devel libsoup3-devel javascriptcoregtk4.1-devel librsvg2-devel openssl-devel
```

## Start and build

```sh
cd apps/app
cargo tauri dev      # starts the UI dev server and the app window, rebuilds on change
cargo tauri build    # release build + installers in target/release/bundle/
```

On the desktop the app opens the same profile as the CLI (`$LOCORA_DATA` or the platform
data folder). To try it without touching your own palaces:

```sh
LOCORA_DATA=/tmp/locora-test locora tutorial
LOCORA_DATA=/tmp/locora-test cargo tauri dev
```

## How it fits together

- `src-tauri/src/views.rs`: start screen and room view from store + review schedule
- `src-tauri/src/journey.rs`: a journey over IPC, same plan and grading as `locora practice`
  (`locora_runtime::journey`, Core plugin)
- `src-tauri/src/sply.rs`: `.sply` → interactive SVG with the vendored Splinely runtime,
  served as `locora://localhost/sply/<sha256>` into sandboxed frames; markers switch state
  via `postMessage` (`{ locora: 1, machine: "status", send: "faellig" }`)
- `ui/src`: screens (start, palace with rooms and markers, journey); no domain logic
