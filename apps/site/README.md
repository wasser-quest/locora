# Locora website (static)

A plain website: HTML and CSS only. No script runs, nothing is loaded from other sites, and nothing is embedded. Because no code runs in the visitor's browser, the usual web attacks (cross-site scripting, injected scripts, malicious frames) have nothing to act on.

## Files

- `index.html`: the page. Its meta tag carries the same strict policy as the headers.
- `style.css`: the only stylesheet. No `@import`, no `url()`, no fonts from elsewhere.
- `_headers`: security headers for hosts that read this file (Netlify, Cloudflare Pages). It sets a strict Content-Security-Policy (`default-src 'none'`), `X-Frame-Options: DENY`, `Referrer-Policy: no-referrer`, and restrictive `Permissions-Policy`.
- `check.py`: fails if anything makes the site non-static: a script, an iframe, an embedded object, an inline handler, a `javascript:` or `data:` URL, an external link other than the project repository, `@import`, or `url()` in the CSS.

## Check and publish

    python3 apps/site/check.py

The site has no build step. Publish the folder `apps/site` as it is, to any static host that honours `_headers` (or copy the same headers into your server configuration).

## Why not Leptos or Tauri here

Tauri is a desktop and mobile wrapper and does not serve websites. Leptos can run as a web app, but a client-side Leptos app is WebAssembly plus JavaScript that runs in every visitor's browser. For a site that must run no code, plain static HTML is the safer choice.
