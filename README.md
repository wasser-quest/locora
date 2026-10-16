# locora

**Offline-first memory palaces, written in pure Rust.** Build palaces from images or videos, place loci in a few clicks, and practice recall on desktop, Android, iOS and the command line.

> **Status:** planning. See [`PLAN.md`](PLAN.md) for the architecture and roadmap (German).

## Highlights

- **Pure Rust:** core, plugins, CLI and UI (Tauri 2 + Leptos). No JavaScript framework.
- **Offline first:** a local SQLite database is the source of truth. No account required.
- **Plugins everywhere:** `core`, `review`, `notify`, `tutorial` and `gamification` are written in Rust, compiled to WebAssembly components and loaded at runtime (hot reload, sandboxed). They talk to each other through events, services and hooks.
- **Spaced repetition:** the `review` plugin schedules due loci with FSRS and keeps the route order of your palace.
- **Notifications:** the `notify` plugin bundles reminders from all plugins, with quiet hours and one-tap actions.
- **Video palaces:** the video pauses at each locus and continues once you have recalled it.
- **Fast CLI:** `locora new "Planets" --image flat.jpg -- Mercury Venus Earth`
- **Minimal clicks:** one click to start a journey; drop an image or video to create a palace.
- **Portable `.locora` files** with embedded media.

## Open core

The whole app is MIT-licensed and free, including the `core`, `review`, `notify`, `tutorial` and `gamification` plugins and the CLI. Gamification (XP, trophies, daily challenges) works fully offline. Only the optional **Locora Cloud** web service (leaderboards, friends, shared challenges, sync across devices) is a paid, proprietary service at an affordable price.

## License

[MIT](LICENSE)
