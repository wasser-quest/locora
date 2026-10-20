# Vendored

- `splinely-0.5.4.js`: the Splinely browser runtime, unchanged from
  [wasser-quest/splinely](https://github.com/wasser-quest/splinely) tag `v0.5.4`
  (`runtime/src/splinely.js`, MIT). It is embedded into every `.sply` the app shows
  (`Project::to_svg`). Update together with the `splinely_document` tag in `Cargo.toml`.
- `splinely-mount.js`: the inline mount from the same tag (`runtime/src/splinely-mount.js`, MIT).
  The web app loads it as `shell/splinely-mount.js`; graphics are mounted in the page, not in a frame.
