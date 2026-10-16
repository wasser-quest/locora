# locora

<p align="center"><img src="assets/mascot/mnemo.gif" width="300" alt="Mnemo, the Locora mascot: a character with brown skin and a big afro, a glowing loci hairpin and a firefly companion"> <img src="assets/mascot/simo.gif" width="200" alt="Simo: a full-body character with light brown skin and an afro, holding a glowing lantern"></p>

**Offline-first memory palaces, written in pure Rust.** Build palaces from images or videos, place loci in a few clicks, and practice recall on desktop, Android, iOS and the command line.

> **Status:** early development. The runtime plugin host works; the app itself is not built yet. See [`PLAN.md`](PLAN.md) for the architecture and roadmap (German).

## Try the plugin system

```bash
cargo build --release
./target/release/locora plugin new "Mein Plugin" --template kachel
cd mein-plugin
../target/release/locora plugin dev --demo   # builds to WebAssembly, loads it, hot-reloads on every save
```

## Repository layout

```text
crates/locora-api          shared event types (host ⇄ plugins)
crates/locora-sdk          write plugins in plain Rust (#[locora_plugin])
crates/locora-sdk-macros   the #[locora_plugin] attribute
crates/locora-host         loads plugins at runtime (wasmtime), sandbox, events, hot reload
apps/cli                   the `locora` command
plugins/                   plugins compiled to wasm32-wasip2 (example: lernzeit)
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

## Mnemo

Mnemo is Locora's mascot, named after **mnemonics**. The three lights in Mnemo's hairpin glow one after another, like the places along a route through a memory palace, and a firefly companion lights the way. Mnemo is joined by **Simo**, named after Simonides, the inventor of the method of loci, who carries a lantern full of firefly light. All graphics are made with [Splinely](assets/README.md); see the [style guide](assets/STIL.md).

## Open core

The whole app is MIT-licensed and free, including the `core`, `review`, `notify`, `tutorial` and `gamification` plugins and the CLI. Gamification (XP, trophies, daily challenges) works fully offline. Only the optional **Locora Cloud** web service (leaderboards, friends, shared challenges, sync across devices) is a paid, proprietary service at an affordable price.

## License

[MIT](LICENSE)
