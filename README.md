# locora

**Offline-first memory palaces, written in pure Rust.** Build palaces from images or videos, place loci in a few clicks, and practice recall on desktop, Android, iOS and the command line.

> **Status:** planning. See [`PLAN.md`](PLAN.md) for the architecture and roadmap (German).

## Highlights

- **Pure Rust:** core, plugins, CLI and UI (Tauri 2 + Leptos). No JavaScript framework.
- **Offline first:** a local SQLite database is the source of truth. No account required.
- **Plugins everywhere:** `core`, `tutorial` and `gamification` use the same plugin API and talk to each other through events, services and hooks.
- **Video palaces:** the video pauses at each locus and continues once you have recalled it.
- **Fast CLI:** `locora new "Planets" --image flat.jpg -- Mercury Venus Earth`
- **Minimal clicks:** one click to start a journey; drop an image or video to create a palace.
- **Portable `.locora` files** with import of legacy `.mnemodim` packages.

## Open core

The host, SDK, `core` and `tutorial` plugins and the CLI are MIT-licensed. The optional `gamification` web service (leaderboards, shared challenges, sync) is a paid, proprietary plugin.

## License

[MIT](LICENSE)
