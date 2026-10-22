# Contributing to Locora

Thanks for helping. Locora is MIT-licensed; by sending a change you agree to publish it under that
licence.

## Branches

| Branch | What goes there | What runs |
|---|---|---|
| `unstable` | **Your pull requests.** Open them against `unstable`. Changes are collected and tried here. | CI on every pull request (Linux: format, clippy, tests). |
| `testing` | Changes taken over from `unstable` and the maintainer's own work, before they go out. | The web app is built, tested in Chromium and deployed as a preview: https://testing--locora-app.netlify.app. |
| `main` | What the next release is made of. | Full CI on Linux, macOS and Windows, the native smoke test and the mobile builds. |
| tag `vX.Y.Z` on `main` | A release. | Installers, CLI, APK, `.ipa` and the live web app at app.locora.cloud. Notes come from `docs/releases/vX.Y.Z.md`. |

Please don't open pull requests against `testing` or `main`.

## Before you send a change

- Rust: `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test --workspace`,
  in the root and in `apps/app` (the app is its own workspace; the UI also has
  `cargo clippy -p locora-ui --target wasm32-unknown-unknown`). Plugins: `cd plugins && cargo clippy`.
- Every text the app shows goes through `t("…")` with a row in `apps/app/ui/src/i18n_catalog.rs`
  (English, German, French, Japanese, Chinese). A test fails on a missing row.
- The apps never show prices, purchase buttons or links to a checkout (store rules); the test in
  `apps/app/ui/src/store_rules.rs` checks this.
- Web app: `node apps/web/build.mjs`, then `npm test` in `apps/web` and in `apps/web/e2e`.
- Commit messages: `<type>(<scope>): <description>` with the types `feat`, `fix`, `docs`, `style`,
  `refactor`, `perf`, `test`, `chore`.

More on building and the layout: [`README.md`](README.md), [`apps/app/README.md`](apps/app/README.md),
[`apps/web/README.md`](apps/web/README.md). Wrong translation? The app's lessons have a „Report“
link; otherwise open an issue with the label `translation`.
