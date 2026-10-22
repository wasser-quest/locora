# Locora in the browser

The app for a browser, as an installable, offline-capable web app. Everything stays on the device:
the core's files and the plugins' storage live in IndexedDB. Only Locora Cloud, when you sign in,
talks to a server (see below).

## Build

    node apps/web/build.mjs

Needs `trunk` (the Leptos UI) and the `wasm32-wasip1` target with a C toolchain for the core (see
`apps/app/README.md`). Plugins are transpiled once in `plugin-host` (`npm run transpile`). The result
is `apps/web/dist`, a folder of static files. The page's content security policy lets it reach
only its own origin and `https://api.locora.cloud`; `LOCORA_CLOUD_ORIGINS` (space-separated) at
build time adds other cloud servers, for example a self-hosted one.

## Serve

Offline use, the service worker and installing need a secure address: `localhost`, or HTTPS.

    node apps/web/serve.mjs                       # http://127.0.0.1:8080
    node apps/web/serve.mjs --host 0.0.0.0 --port 8443 --tls-cert cert.pem --tls-key key.pem

`serve.mjs` explains how to make a self-signed certificate for another device on your network.

## Install and offline

- Chrome, Edge: the install icon in the address bar, or the "Install Locora" button.
- Android: "Add to home screen".
- The service worker (`sw.js`) stores every file of a build on the first visit. The app then opens
  and reloads without a network; the palaces and progress are still in the browser's storage.
- A new build gets a new cache; the old one is removed when the new worker takes over.

## Locora Cloud

The same cloud settings as in the desktop app: sign in or create an account, confirm the e-mail
address, set up the encryption, sync by hand with the summary before it, and auto-sync. The core
(`crates/locora-web-core`) runs the cloud commands as steps; `shell/src/cloud.mjs` sends their
HTTP calls with `fetch`. The encryption password is checked with Argon2 in a worker
(`shell/src/unlock-worker.mjs`), so the page does not freeze. For auto-sync the account key is
kept in IndexedDB, wrapped by a non-extractable WebCrypto AES-GCM key; it is forgotten when
auto-sync is turned off, on sign-out, and when the cloud data or the account is deleted. How the
cloud works is described in [`docs/CLOUD.md`](../../docs/CLOUD.md) (German).

## Notifications

Reminders are shown by the browser, once the browser allows it (the permission is asked on the
first due message).

## Tests

- `apps/web/shell`: the core in WebAssembly with the in-memory file system, the storage, the
  plugins and the cloud client (`npm test` in `apps/web`).
- `apps/web/e2e`: the app in Chromium (`npm test` in `apps/web/e2e`, after the build), including
  a cloud sync against a fake cloud inside the test. `LOCORA_URL` tests a running deployment
  instead of the local build.
