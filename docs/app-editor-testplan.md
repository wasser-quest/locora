# App editor acceptance checks

These are manual WebView checks, not automated test results. Run them after the
Rust tests and the `wasm32-unknown-unknown` UI check. Use a disposable profile so
that drag and autosave checks cannot affect real learning data.

## Setup

1. Set `LOCORA_DATA` to a new temporary directory in both terminals.
2. Run `cargo run -p locora -- tutorial` from the repository root.
3. Run `cargo tauri dev` from `apps/app` and open the tutorial palace.
4. Keep a second terminal ready for `cargo run -p locora -- show "Locora lernen"`
   (or use the palace ID shown by `ls`).

## Persistence and selection

- Edit a place, item and memory aid. Leave each field as the UI requests, then
  verify the changes in the CLI and after restarting the app.
- Switch loci and rooms while an edit is pending. The update must never change
  a different locus, replace newly typed text with an older response or expose
  an answer picture belonging to the previous selection.
- Click empty stage space once. Exactly one locus should be created at that
  position. Selecting a marker or a form control must not create another locus.
- On a blank room, test creation without a backdrop. For video rooms, check
  whether creation is supported; an unsupported operation must give a clear
  error and must not leave a partial locus.

## Pointer and keyboard

- Drag a marker to the center, all four corners and outside the stage. Coordinates
  must remain within the room bounds and survive restarting the app.
- Release outside the stage and cancel a pointer gesture. Neither case may leave
  the next click in a stuck dragging state or create an extra locus.
- Repeat after scrolling and resizing the window, and with touch input where
  available. Stage-relative coordinates must remain stable.
- If a resize handle is available, resize a revealed picture to the minimum and
  maximum width. Check persistence, non-square raster images and `.sply` frames.
  Resizing must not move the locus or create another one.
- Navigate with Tab. Controls need understandable labels and visible focus;
  position/size editing must have a keyboard alternative to pointer gestures.

## Answer safety and validation

- Before revealing, no answer picture should appear at the locus. After reveal,
  it should appear both there and in the answer panel. Switching selection or
  rooms must hide it again.
- Try Recall, Guess (including accepted alternatives) and Quiz if these fields
  are editable. Reopen and practice from both CLI and app to confirm the same
  evaluation. Invalid quiz definitions must fail without partial changes.
- Enter invalid numeric values and empty required fields. Errors must be visible;
  rejected input must not be presented as saved. Valid corrections must work.
- Trigger a Journey after editing. The latest saved item must be used, not a stale
  editor snapshot. Reverse practice must keep its picture on the question side.

## Native picture selection

- Select a PNG/JPEG and a valid binary `.sply` file for a locus. Reopen the app
  and verify assignment, reveal rendering and CLI export. Check portrait images
  and filenames with Unicode and spaces.
- Cancel the native chooser. The old picture, content, operation log and current
  selection must remain unchanged. Removing a picture should clear its reference,
  not delete media still used elsewhere.
- Try an unsupported file, a renamed video and a malformed `.sply`. Errors must
  leave the original assignment intact. A filename extension alone must not
  authorize an incompatible file type.
- While the chooser is open, try navigation and repeated picker activation.
  Returning a selection must never assign it to another locus or overwrite a
  more recent content draft. Verify correction after a failed selection.
- Verify no arbitrary filesystem-read command or broadly scoped frontend file
  access was added. Only native user-selected files should be imported.
- For the home-screen new-palace import, verify cancel/error leaves no empty
  palace behind and success opens the newly created room with its backdrop.
  Test supported image, `.sply` and video files; confirm the stored room kind
  matches the selected media and the CLI can export/reimport the new palace.
- Import the same file twice and files with identical stems from different
  directories. Both successful imports must be reachable without replacing the
  previous palace. Empty/unusual filenames must still produce a usable name.
- Try rapid repeated import activation and navigation while the chooser is open.
  Only the completed selection should navigate; cancellation must leave the
  current screen usable. A failed refresh must not report a committed creation
  as a failed import and encourage duplicate retries.

## List-to-route paste

- Paste ten nonempty lines into the route input. Verify exactly ten loci, in input
  order, with distinct in-bounds positions, then reopen the palace in the CLI.
- Repeat in a room that already contains loci. Existing content and coordinates
  must stay unchanged; the new loci must append in a deterministic order.
- Test CRLF, blank lines, surrounding whitespace, Unicode and duplicate terms.
  Verify the preview and resulting route agree about which lines are imported.
- Try an empty list and a list above the documented limit. Neither may produce
  partial loci or operation-log entries. An error must remain visible.
- Paste inside a place, note or quiz text field. Ordinary text paste must not
  create additional loci. Stage-level paste, if supported, must only apply to
  the active editor room and must not run during an outstanding mutation.
- Trigger import twice rapidly and change rooms during import. The editor must
  not duplicate a pending batch or apply its result to another room.
- Try list creation in a video room. Until timeline authoring exists it should
  reject clearly without partially creating loci that have no timestamps.
- Confirm a newly imported locus can be selected, edited and moved, and that its
  recall item is correct in a Journey.

## Layout

- Test a narrow window, long German labels and long memory aids.
- Use large landscape and portrait raster images. Images must not cover grading
  or editing controls. Verify markers remain selectable above revealed images.
- Confirm sandboxed `.sply` animations still play and marker states still update.
