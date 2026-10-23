# Locora

<p align="center"><img src="assets/mascot/simo.gif" width="260" alt="Simo, the Locora mascot: a man with light brown skin and an afro, holding a glowing lantern, with a firefly companion"></p>

<p align="center">
  <strong>Offline-first memory palaces, written in pure Rust.</strong><br>
  Build palaces from your own photos and videos, place loci in a few clicks, and practise recall on
  desktop, Android, the web and the command line.
</p>

<p align="center">
  <a href="https://github.com/wasser-quest/locora/actions/workflows/ci.yml"><img src="https://github.com/wasser-quest/locora/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/wasser-quest/locora/actions/workflows/preview.yml"><img src="https://github.com/wasser-quest/locora/actions/workflows/preview.yml/badge.svg?branch=testing" alt="Web preview"></a>
  <a href="https://github.com/wasser-quest/locora/releases/latest"><img src="https://img.shields.io/github/v/release/wasser-quest/locora?include_prereleases&sort=semver" alt="Latest release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue" alt="MIT license"></a>
</p>

<p align="center">
  <a href="https://locora.cloud">Website</a> ·
  <a href="https://app.locora.cloud">Web app</a> ·
  <a href="https://testing--locora-app.netlify.app">Preview of the next version</a> ·
  <a href="https://github.com/wasser-quest/locora/releases/latest">Downloads</a>
</p>

> **Status:** early development. Expect rough edges; the roadmap lives in [`docs/PLAN.md`](docs/PLAN.md) (German).

## Download

| Platform | File (from the [latest release](https://github.com/wasser-quest/locora/releases/latest)) |
|---|---|
| **Web** | No install: [app.locora.cloud](https://app.locora.cloud), installable as an app, works offline |
| **Preview** | The newest tested state of the `testing` branch: [testing--locora-app.netlify.app](https://testing--locora-app.netlify.app). Updated on every push to `testing`; may change at any time and keeps its own data in the browser. |
| **Android** | `Locora_<version>_android-arm64.apk` |
| **iPhone / iPad** | `Locora_<version>_ios-unsigned.ipa`, install it with AltStore or Sideloadly (they sign it with your Apple ID) |
| **Windows** | `Locora_<version>_x64-setup.exe` or `.msi` |
| **macOS** | `Locora_<version>_aarch64.dmg` (Apple silicon) or `_x64.dmg` (Intel) |
| **Linux** | `.AppImage`, `.deb` or `.rpm` |
| **CLI** | `locora-cli_<version>_<platform>` archives |

The desktop installers are not code-signed yet: on macOS open the app with right-click → *Open*,
on Windows choose *More info → Run anyway*. `SHA256SUMS.txt` lists the checksums of every file.

## Highlights

- **Your own places:** a photo of your flat or a video of your way to work becomes a palace; tap to place loci.
- **Spaced repetition:** the `review` plugin schedules due loci with FSRS and keeps the route order of your palace.
- **Video palaces:** the video pauses at each locus and continues once you have recalled it.
- **Offline first and private:** a local SQLite database is the source of truth; no account, no tracking.
  Pictures, videos and sound clips are stored without location data or other metadata.
- **Phone recordings as they are:** videos and sound keep the codec they were recorded in (H.264, HEVC,
  AV1, VP9, VP8; AAC, Opus, MP3, FLAC …) and your device plays them; Locora bundles no codec. Only
  rare formats (ProRes, AMR …) are converted, by Android, the browser, or an ffmpeg you installed.
- **Sound at every locus:** an optional audio clip per locus, played when the answer is revealed.
- **Minimal clicks:** one click starts a journey; drop an image or video to create a palace.
- **A guided start:** an app tour in which Simo builds an example palace and walks its route with
  you, and an ⓘ next to the editor's fields and the settings that explains each one.
- **Five languages:** English, Deutsch, Français, 日本語 and 中文, chosen on the first start.
- **Sync across devices (optional):** Locora Cloud keeps desktop, phone, browser and CLI in step,
  end-to-end encrypted, by hand or automatically. See [Locora Cloud](#locora-cloud).
- **Formulas:** workbook tables and formula bindings for content that can be calculated.
- **Plugins everywhere:** `core`, `review`, `notify`, `tutorial` and `gamification` are Rust compiled to
  WebAssembly components, loaded at runtime, sandboxed and hot-reloadable.
- **Portable `.locora` files** with embedded media, and `locora.toml` sources you can write by hand.
- **Pure Rust:** core, plugins, CLI and UI (Tauri 2 + Leptos). No JavaScript framework.

## Command line

Download a `locora-cli` archive from the releases, or build it yourself:

```bash
cargo build --release          # also compiles the built-in plugins to WebAssembly
alias locora=./target/release/locora

locora tutorial                # learn Locora with the method of loci itself
locora new "Planeten" -- Merkur Venus Erde Mars Jupiter Saturn Uranus Neptun
locora practice Planeten       # walk the route: Enter reveals, 1–4 grades
locora due                     # what is due today
locora review                  # only what is due (plus new loci up to the daily limit)
locora stats                   # answers, hit rate, streak
```

<details>
<summary>More commands</summary>

```bash
locora edit Planeten 1 --answer '{"type":"guess"}'  # typed answer, checked by Core
locora edit Planeten 1 --accept Hermes --note "Ein Thermometer am Herd"  # more right answers, memory aid
locora edit Planeten 1 --image merkur.png --image-size 0.4  # reveal picture at 40% of room width in the app
locora edit Planeten 1 --audio merkur.mp3   # a sound clip played with the answer (mp3, m4a, wav, ogg); --no-audio removes it
locora practice Planeten --reverse  # item → place; --numbers asks „Ort 7“, --shuffle mixes the order
locora settings                # new loci per day, target retention
locora optimize                # personal FSRS weights from your own history
locora show Planeten
locora new Vortrag --video talk.mp4 -- 0:12=Einleitung 1:23=These
locora new Vokabeln --image kueche.jpg --from vokabeln.txt   # lines: `Ort | Erinnerung | x,y`
locora build planeten.locora.toml   # a palace from a text file (see format/locora-toml.md)
locora dump Planeten -o quelle/     # and back: source + media files
locora export Planeten         # Planeten.locora, with images and videos (--with-progress: plus review schedule)
locora import Planeten.locora  # always a new copy; --dry-run only checks
locora formula-guide           # a palace that teaches the formula language
```

</details>

Data lives in your platform data directory (`--data DIR` or `$LOCORA_DATA` to change it). The
`.locora` format is documented in [`format/README.md`](format/README.md), the text source format in
[`format/locora-toml.md`](format/locora-toml.md).

## With an AI assistant

Let an AI assistant install Locora for you. Paste one of these prompts:

```text
Install Locora. Follow https://raw.githubusercontent.com/wasser-quest/locora/main/INSTALL.md
```

```text
Install the Locora app and the CLI. Follow https://raw.githubusercontent.com/wasser-quest/locora/main/INSTALL.md
```

[`INSTALL.md`](INSTALL.md) says where to get each file and how to install it; without further
details it installs only the CLI. Afterwards [`llm.txt`](llm.txt) (`locora guide`) explains every
command to the model.

## Locora Cloud

An optional service that syncs your palaces, media and progress between your devices. Everything
is encrypted on the device with your own encryption password before it is sent; the server cannot
read it. When you set up the encryption, the app shows a recovery key once: save it as a file or
copy it. Sync by hand (the app first lists what it sends, by palace) or turn on auto-sync, which
keeps the key in the system's keychain (in the browser: wrapped by WebCrypto). A deleted palace is
deleted on your other devices only after you confirm it. New accounts can sync for six months
without a subscription. Manage the account at [web.locora.cloud](https://web.locora.cloud).
Details (German): [`docs/CLOUD.md`](docs/CLOUD.md).

## Building the app

```sh
cd apps/app && cargo tauri dev     # desktop app (Tauri 2 + Leptos); release: cargo tauri build
node apps/web/build.mjs            # the browser app into apps/web/dist
```

Needs `trunk`, `tauri-cli` and the WebKit/GTK development packages; Android and iOS builds and
the release process are described in [`apps/app/README.md`](apps/app/README.md), the web build in
[`apps/web/README.md`](apps/web/README.md).

## Tests

```sh
cargo test                                  # core, CLI, store, sync (builds the test plugins)
(cd plugins && cargo test --workspace --target x86_64-unknown-linux-gnu)  # plugin rules, natively
(cd apps/app && cargo test --workspace)     # app backend and UI, incl. the translation catalog
                                            # and the store rules (no prices or checkout links)
(cd apps/web && npm test)                   # the browser core, plugins and cloud client in Node
(cd apps/web/e2e && npm test)               # the browser app in Chromium, after node apps/web/build.mjs
apps/app/tests/run_smoke.sh                 # the desktop app in WebKitGTK (Linux; build the debug app
                                            # and the UI first, see the script's header)
```

CI runs `cargo fmt --check`, `clippy -D warnings` and the tests of both workspaces, and the native
smoke test of the desktop app on Linux.

## Writing plugins

```bash
locora plugin new "Mein Plugin" --template kachel
cd mein-plugin
locora plugin dev --demo       # builds to WebAssembly, loads it, hot-reloads on every save
locora plugin install ./mein_plugin.wasm   # shows its permissions; the default is to cancel
```

The [SDK guide](crates/locora-sdk/README.md) covers events, filter chains, palace services, Core
journey evaluation, scoring hooks, installation, signing and the sandbox's limits.

## Repository layout

```text
apps/cli                   the `locora` command
apps/app                   the desktop and mobile app (Tauri 2 backend, Leptos UI, Android video plugin)
apps/web                   the browser app (the same UI and core, compiled to WebAssembly)
apps/site                  the website locora.cloud
crates/locora-store        offline-first SQLite storage: palaces, rooms, loci, media, plugin data, op log
crates/locora-runtime      what the CLI and the apps share: journeys, tutorial, cloud sync
crates/locora-app-core     the app's editor, views and workbook logic
crates/locora-media        pictures, videos and sound as Locora stores them (metadata removed, never re-encoded)
crates/locora-host         loads plugins at runtime (wasmtime): sandbox, events, services, hot reload
crates/locora-sdk          write plugins in plain Rust (#[locora_plugin])
crates/locora-fsrs         FSRS-5 spaced-repetition scheduling
crates/locora-format       the .locora file format (export, strict import)
crates/locora-formula      safe, bounded formula language for workbook tables
crates/locora-source       locora.toml source files: build palaces from text, dump them back
crates/locora-crypto       end-to-end encryption of synced data
plugins/                   the official plugins, compiled to wasm32-wasip2
docs/                      plan, open work, Locora Cloud, release notes (German, release notes English)
```

## Simo

Simo is Locora's mascot, named after **Simonides**, who is credited with inventing the method of
loci. The three lights on his jacket glow one after another, like the places along a route through
a memory palace, and his lantern and firefly light the way. All graphics are made with
[Splinely](assets/README.md) in a lineless, cel-shaded style; see the [style guide](assets/STIL.md).

## Open core

The whole app is MIT-licensed and free: the CLI, the desktop, mobile and web app, and the `core`,
`review`, `notify`, `tutorial` and `gamification` plugins. Gamification (XP, trophies, daily
challenges) works fully offline. Only the optional **Locora Cloud** web service (end-to-end
encrypted sync across devices, leaderboards) is a proprietary service. A subscription is sold only
on the web, never inside the apps.

## License

[MIT](LICENSE) © Marvin Olomu
