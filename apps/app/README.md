# Locora app

Tauri 2 backend (`src-tauri/`) and Leptos UI (`ui/`, WebAssembly via `trunk`), with
shared IPC types in `shared/`. Its own Cargo workspace, so the main workspace builds
without WebKit/GTK.

## Once: tools and system packages

```sh
rustup target add wasm32-unknown-unknown wasm32-wasip2
cargo install trunk --locked
cargo install tauri-cli --version "^2" --locked
# Linux (Fedora); Debian/Ubuntu: libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev librsvg2-dev libssl-dev
sudo dnf install webkit2gtk4.1-devel gtk3-devel libsoup3-devel javascriptcoregtk4.1-devel librsvg2-devel openssl-devel
```

## Start and build

```sh
cd apps/app
cargo tauri dev      # starts the UI dev server and the app window, rebuilds on change
cargo tauri build    # release build + installers in target/release/bundle/
```

On the desktop the app opens the same profile as the CLI (`$LOCORA_DATA` or the platform
data folder). To try it without touching your own palaces:

```sh
LOCORA_DATA=/tmp/locora-test locora tutorial
LOCORA_DATA=/tmp/locora-test cargo tauri dev
```

## How it fits together

- `src-tauri/src/views.rs`: start screen and room view from store + review schedule
- `src-tauri/src/journey.rs`: a journey over IPC, same plan and grading as `locora practice`
  (`locora_runtime::journey`, Core plugin)
- `src-tauri/src/sply.rs`: `.sply` → interactive SVG with the vendored Splinely runtime,
  served as `locora://localhost/sply/<sha256>` into sandboxed frames; markers switch state
  via `postMessage` (`{ locora: 1, machine: "status", send: "faellig" }`)
- `ui/src`: screens (start, palace with rooms and markers, journey); no domain logic

## Locus pictures

Revealing a locus shows its picture at the locus position as well as in the answer
panel. Its width is `image_size` of the room width (default 0.25, range 0.05–1).
Raster pictures retain their aspect ratio; `.sply` frames have a square viewport.
Before reveal only the marker is shown. Reverse practice keeps the picture in the
question rather than placing it on the stage as an answer.

Enable **Bearbeiten** in a palace room to edit places, items, notes, Recall/Guess/
Quiz answers and picture widths inline. Valid
content saves automatically; incomplete or invalid input stays visible until
corrected or discarded. Navigation is blocked while edits are pending. Click the
stage in **Ort setzen** mode to add a locus, drag markers to move them, or focus a
marker and use arrow keys (Shift for larger steps). A button also adds a locus at
the center. Reveal a picture to use its resize handle; its numeric width field is
the keyboard alternative. Deletion requires a second confirmation.

In **Bearbeiten**, focus the stage and paste with Ctrl/Cmd+V, or paste into
**Liste einfügen** and choose **Liste als Route hinzufügen**. Each nonempty trimmed line becomes a Recall item at a new numbered
place. Limits: 100 items, 500 UTF-8 bytes per item, 64 KiB input. New places are
appended in list order along an inset serpentine route; existing places are not
moved. Drag or use arrow keys to adjust positions. Video rooms reject lists.
The backend checks exact palace/room ownership and all items; the store commits
the entire batch and operation log in one transaction or rolls everything back.
Failed textarea submissions retain the text; successful submissions clear it.
Stage paste uses the clipboard event, without requesting clipboard permissions.
Ordinary paste in other input fields is never intercepted.
The list is an explicit submission, not an autosaved or durable draft.

**Bild wählen…** opens a native file picker for the selected locus; **Bild entfernen**
autosaves removal. Supported: PNG, JPEG, WebP, GIF, AVIF and binary `.sply`, up to
64 MiB. The backend reuses source/CLI extension classification, checks raster MIME
signatures and validates `.sply` with the existing SVG converter. Cancellation is a
no-op; errors appear in German and leave the current assignment intact. Choose only
when edits have saved; form input is disabled while the picker is open. Files are
copied into the profile media store, so originals need not remain in place.
Only exact palace/room/locus IDs cross IPC, never a file path. The dialog plugin is
backend-only: no dialog/filesystem permissions were added to the WebView and CSP
is unchanged. Mobile content-URI selections are not supported yet (local files only).
Native dialog interaction still needs a manual desktop/WebView smoke test.

## New palace from media

On Home, **Bild/Video wählen…** opens the backend-only native picker for the same
pictures plus MP4/M4V, WebM and MOV. Extension and content signatures must match;
regular local files only, bounded to 64 MiB (also for video). This checks signatures,
not complete video decoding. The filename stem names the palace and first room
(fallback **Neuer Palast**); existing palace names get ** (2)**, ** (3)**, etc.,
using the store's NOCASE comparison. The room type follows the media type.
Success opens the returned snapshot directly in editing mode; Back reloads Home.
Cancellation writes nothing. Media metadata, palace, room and operation log commit
in one transaction; failures leave no empty palace or partial database writes.
Immutable unreferenced hash files can remain after a database failure.
No paths cross IPC and no WebView permissions or CSP changes are needed.
Tests cover cancellation, invalid signatures, size limits, persistence, name
collisions and SQL rollback. Native desktop/WebView interaction remains unverified.

Drag-and-drop import, video-locus creation, undo and durable draft recovery are deferred. Content with
workbook bindings stays read-only; geometry remains editable.
CLI `add/edit --image-size` and `image_size` in `locora.toml`
remain available. Browser reload protection is best-effort; do not close the app
while an error or pending edit is shown.

Rust tests cover editor mutations, validation, persistence, geometry helpers and
incomplete draft parsing, list parsing/route geometry, batch persistence and rollback
(including database failures). Browser/WebView list submission, gesture and autosave-focus checks remain
pending; see [`docs/app-editor-testplan.md`](../../docs/app-editor-testplan.md).
