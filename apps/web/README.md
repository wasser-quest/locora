# Locora in the browser

The app for a browser, as an installable, offline-capable web app. Everything stays on the device:
the core's files and the plugins' storage live in IndexedDB, and nothing is sent anywhere.

## Build

    node apps/web/build.mjs

Needs `trunk` (the Leptos UI) and the `wasm32-wasip1` target with a C toolchain for the core (see
`apps/app/README.md`). Plugins are transpiled once in `plugin-host` (`npm run transpile`). The result
is `apps/web/dist`, a folder of static files.

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

## Tests

- `apps/web/shell`: the core in WebAssembly with the in-memory file system and the storage
  (`npm test` in `apps/web`).
- `apps/web/e2e`: the app in Chromium (`npm test` in `apps/web/e2e`, after the build).

## Not in the browser build (yet)

- Locora Cloud: connect, sync and key setup answer with a message; the status shows "signed out".
- Notifications are shown by the browser, once the browser allows it (the permission is asked on
  the first due message).
