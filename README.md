# locora

<p align="center"><img src="assets/mascot/simo.gif" width="260" alt="Simo, the Locora mascot: a man with light brown skin and an afro, holding a glowing lantern, with a firefly companion"></p>

**Offline-first memory palaces, written in pure Rust.** Build palaces from images or videos, place loci in a few clicks, and practice recall on desktop, Android, iOS and the command line.

> **Status:** early development. The command line works end to end (palaces, practice, spaced repetition through the `review` plugin, local SQLite storage); the desktop/mobile app is next. See [`PLAN.md`](PLAN.md) for the architecture and roadmap (German).

## Try it

```bash
cargo build --release          # also compiles the built-in plugins to WebAssembly
alias locora=./target/release/locora

locora new "Planeten" -- Merkur Venus Erde Mars Jupiter Saturn Uranus Neptun
locora practice Planeten       # walk the route: Enter reveals, 1–4 grades
locora edit Planeten 1 --answer '{"type":"guess"}'  # typed answer, checked by Core
locora due                     # what is due today (FSRS via the review plugin)
locora review                  # only what is due (plus new loci up to the daily limit)
locora stats                   # answers, hit rate, streak
locora settings                # new loci per day, target retention
locora optimize                # personal FSRS weights from your own history
locora show Planeten
locora new Vortrag --video talk.mp4 -- 0:12=Einleitung 1:23=These
locora new Vokabeln --image kueche.jpg --from vokabeln.txt   # lines: `Ort | Erinnerung | x,y`
locora build planeten.locora.toml   # a palace from a text file (see format/locora-toml.md)
locora dump Planeten -o quelle/     # and back: source + media files
locora export Planeten         # Planeten.locora, with images and videos (--with-progress: plus review schedule)
locora import Planeten.locora  # always a new copy; --dry-run only checks
```

The `.locora` format is documented in [`format/README.md`](format/README.md).

Data lives in your platform data directory (`--data DIR` or `$LOCORA_DATA` to change it).

## Try the plugin system

```bash
cargo build --release
./target/release/locora plugin new "Mein Plugin" --template kachel
cd mein-plugin
../target/release/locora plugin dev --demo   # builds to WebAssembly, loads it, hot-reloads on every save
```

Host-driven filter chains and capability-checked palace services are documented
in the [SDK guide](crates/locora-sdk/README.md), including API 0.4 migration,
Core journey evaluation, answer authoring, scoring hooks, limitations and
integration-test commands. `plugin dev/run` use isolated in-memory storage;
`plugin check` inspects metadata without activation or profile access.

Install local components into the selected profile (no registry or signatures yet):

```bash
locora plugin check ./mein_plugin.wasm
locora plugin install ./mein_plugin.wasm   # warning, permissions, default = cancel
locora plugin ls                           # metadata only; also shows broken files
locora plugin remove mein-plugin           # or the .wasm filename shown by ls
# Scripts must explicitly accept the warning and all displayed rights:
locora --data ./test-profile plugin install ./mein_plugin.wasm --yes
```

Installation never calls `activate`; activation happens on the next normal profile
command. Only `storage`, `palaces:read` and `palaces:write` are supported; all other
permissions (including `notify`/`network`) are refused. Components must match the
host API (currently 0.4). Files live in `<data>/plugins/<id>.wasm`, alongside the
approved manifest in `<id>.wasm.json`. Keep both files: manually copied components
without approval are not activated. Changed manifests are rejected before activation.
Existing destinations and duplicate IDs (even under another filename) are refused;
there is no update command. Remove first to reinstall; plugin KV/palaces are retained.
Removal by ID resolves renamed components and refuses ambiguous IDs; an explicit
`.wasm` filename removes broken files without probing them. The `plugins` directory
itself must not be a symlink.
Embedded `core`/`review` cannot be replaced or removed; broken profile copies can be
removed without loading them. See the SDK guide for trust and sandbox limitations.

## Repository layout

```text
crates/locora-api          shared event types (host ⇄ plugins)
crates/locora-sdk          write plugins in plain Rust (#[locora_plugin])
crates/locora-sdk-macros   the #[locora_plugin] attribute
crates/locora-host         loads plugins at runtime (wasmtime), sandbox, events, services, hot reload
crates/locora-store        offline-first SQLite storage: palaces, stages, loci, media, plugin data, op log
crates/locora-fsrs         FSRS-5 spaced-repetition scheduling
crates/locora-format       the .locora file format (export, strict import)
crates/locora-formula      safe, bounded formula language for workbook tables (Major system, functions)
crates/locora-source       locora.toml source files: build palaces from text, dump them back
apps/cli                   the `locora` command (palaces, practice, review, plugin development)
plugins/core               answer evaluation and base scoring (WASM, built into the CLI)
plugins/review             official spaced-repetition plugin (WASM, built into the CLI)
plugins/                   other plugins compiled to wasm32-wasip2 (example: lernzeit)
```

## Highlights

- **Pure Rust:** core, plugins, CLI and UI (Tauri 2 + Leptos). No JavaScript framework.
- **Offline first:** a local SQLite database is the source of truth. No account required.
- **Plugins everywhere:** `core`, `review`, `notify`, `tutorial` and `gamification` are written in Rust, compiled to WebAssembly components and loaded at runtime (hot reload, sandboxed). They talk to each other through events, services and hooks.
- **Build your own plugins:** `locora plugin new` + `locora plugin dev` gives you a running, hot-reloading plugin in minutes.
- **Spaced repetition:** the `review` plugin schedules due loci with FSRS and keeps the route order of your palace.
- **Notifications:** the `notify` plugin bundles reminders from all plugins, with quiet hours and one-tap actions.
- **Video palaces:** the video pauses at each locus and continues once you have recalled it.
- **Fast CLI:** `locora new "Planets" --image flat.jpg -- Mercury Venus Earth`
- **Minimal clicks:** one click to start a journey; drop an image or video to create a palace.
- **Portable `.locora` files** with embedded media.

## Simo

Simo is Locora's mascot, named after **Simonides**, who is credited with inventing the method of loci. The three lights on his jacket glow one after another, like the places along a route through a memory palace, and his lantern and firefly light the way. All graphics are made with [Splinely](assets/README.md) in a lineless, cel-shaded style; see the [style guide](assets/STIL.md).

## Open core

The whole app is MIT-licensed and free, including the `core`, `review`, `notify`, `tutorial` and `gamification` plugins and the CLI. Gamification (XP, trophies, daily challenges) works fully offline. Only the optional **Locora Cloud** web service (leaderboards, friends, shared challenges, sync across devices) is a paid, proprietary service at an affordable price.

## License

[MIT](LICENSE)
