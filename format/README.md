# The `.locora` file format, versions 1 and 2

A `.locora` file holds one or more memory palaces with their rooms, loci, workbook tables, images and videos. It is a ZIP archive:

```text
mimetype                 application/vnd.locora+zip   (first entry, stored uncompressed)
manifest.json            what the file contains
palaces/<id>.json        one palace: rooms, loci in route order, its own tables
tables.json              tables shared by all palaces (only if manifest.sharedTables)
media/<sha256>.<ext>     images and videos, named by the SHA-256 of their bytes
plugins/<plugin-id>/…    reserved for plugin data; readers ignore it
```

- Like EPUB and ODF, the first entry `mimetype` is stored uncompressed, so tools can recognise the file from its first bytes. `manifest.json` follows directly, so readers can stream.
- JSON documents use **camelCase** and are deflated; media is stored as is.
- Writing is **deterministic**: entries have a fixed timestamp and order (mimetype, manifest, palaces in export order, `tables.json`, media sorted by SHA-256). The same content always produces the same bytes, which suits Git and sync.

The reference implementation is [`crates/locora-format`](../crates/locora-format); `locora export` and `locora import` use it.

## Words

| Term | Meaning |
| --- | --- |
| palace | a memory palace |
| room | one room of a palace, optionally with a backdrop image or video |
| locus | a stop on the route: a **place** in the room and the **item** stored there |
| table | workbook data used by formulas (`Table.key.column`) |
| binding | a formula computing a locus field |

## `manifest.json`

```json
{
  "format": "locora",
  "version": 1,
  "createdBy": "locora 0.1.0",
  "palaces": [{ "id": "0199f3a2…", "name": "Planeten" }],
  "media": [{ "sha256": "9f86d081…", "mediaType": "image/png", "size": 48213 }],
  "plugins": [{ "id": "review", "version": "0.1", "required": false }],
  "sharedTables": true
}
```

- `version` above the reader's version → rejected with "needs a newer Locora".
- Current readers accept versions 1 and 2. Recall-only exports still write version
  1 and omit `answer`, remaining readable by existing version 1 implementations.
  Any Quiz/Guess locus makes the entire archive version 2. Version 1 with a
  non-Recall answer is rejected; older readers reject version 2 rather than
  silently converting automatic questions into Recall.
- File names are derived, never stored: `palaces/<id>.json`, `media/<sha256>.<ext>`.
- Media types and extensions: `image/png` png, `image/jpeg` jpg, `image/webp` webp, `image/gif` gif, `image/avif` avif, `video/mp4` mp4, `video/webm` webm, `video/quicktime` mov.
- `plugins`: plugins the file uses. A missing `required` plugin blocks the import.

## `palaces/<id>.json`

```json
{
  "id": "0199f3a2…",
  "name": "Planeten",
  "rooms": [
    {
      "id": "0199f3a3…",
      "name": "Wohnung",
      "backdrop": "9f86d081…",
      "loci": [
        { "id": "0199f3a4…", "place": "Herd", "item": "Merkur", "x": 0.42, "y": 0.61 },
        { "id": "0199f3a5…", "place": "Tür", "item": "?", "x": 0.5, "y": 0.5,
          "bindings": { "item": "Major[\"7\"].object", "active": "Integer($zahl) > 2" } }
      ]
    }
  ],
  "tables": [
    { "table": "Inputs", "key": "zahl", "cells": [{ "column": "value", "type": "text", "value": "47" }] }
  ]
}
```

- `backdrop`: SHA-256 of a media entry. Its media type decides the room: `image/*` image room, `video/*` video room; no backdrop = plain room.
- `x`, `y`: position in the room from 0 to 1 (left/top = 0).
- `time`: seconds into the video; required in video rooms and not allowed elsewhere.
- `bindings`: formulas for `place`, `item`, `x`, `y`, `visible`, `active` (each at most once).
- `tables`: rows `{ table, key, cells: [{ column, type, value }] }`; `type` is `text`, `number`, `boolean`, `json`, `formula`, `asset`, `palace` or `room`.
- Array order is route order.

## Locus answers (version 2)

An absent `answer` defaults to Recall, including in version 1 files. Explicit
Recall is `{"type":"recall"}`. Guess is `{"type":"guess"}` and checks the
resolved `item`, not a second stored answer string. Quiz specifies authored
options with one or more correct selections:

```json
{
  "type": "quiz",
  "options": [
    { "text": "Merkur", "correct": true },
    { "text": "Venus", "correct": false },
    { "text": "Erde", "correct": true }
  ]
}
```

Quiz requires **2–9 total options**, at least one correct and one incorrect.
Labels must be nonblank and unique after trimming and lowercasing, and respect
text limits. Order is preserved; all correct options must be selected, with no
partial credit. Recall/Guess cannot contain options (an empty list is allowed).
Unknown types/fields and malformed configurations are rejected before import.
These are explicit options, not legacy comma-separated correct answers or
randomly generated distractors. For Quiz, `item` remains an explanation shown
after answering; it need not contain the correct labels.

Correct Guess matching trims and lowercases both whole strings. Core maps
incorrect answers to Again and automatic correct answers to Easy at <=5,000 ms,
otherwise Good. Recall stays self-rated. See the
[SDK journey guide](../crates/locora-sdk/README.md#core-journeys) for runtime,
forfeiture, hooks and limitations. Answer configuration travels with exports;
Review state and earned points do not.

## `tables.json`

An array of rows (same shape as above) shared by all palaces. A palace's own row with the same `table` and `key` replaces the shared one.

## Plugin data: `plugins/review/progress.json`

`locora export --with-progress` adds the review schedule of the exported loci (plugin `review`) and lists the plugin in `manifest.plugins` with `required: false`:

```json
{
  "version": 1,
  "cards": [
    { "palace": "0199f3a2…", "locus": "0199f3a4…", "stability": 3.17, "difficulty": 5.28,
      "lastReview": 1800000000, "due": 1800259200, "reps": 1, "lapses": 0 }
  ]
}
```

- Ids are the ids inside the archive; importing maps them to the new ids of the copies.
- Readers check everything before importing: known version, no unknown fields, every card belongs to a locus in the file and appears once, stability > 0, difficulty 1–10. A broken file imports nothing.
- `locora import --without-progress` ignores the file; without the `review` plugin it is skipped with a note.
- Other plugins may store data under `plugins/<plugin-id>/` the same way (plain file names, at most 16 MiB each).

## Rules for readers

Readers fail closed instead of guessing:

- a missing or wrong `mimetype` entry; unknown fields; unknown files outside `plugins/`; duplicate entries, ids or table rows;
- absolute paths, `..`, `.` or backslashes in entry names; ids outside `[A-Za-z0-9_-]{1,64}`;
- media whose size or SHA-256 does not match the manifest (hashes are checked while importing, before anything is written);
- positions outside 0–1, non-finite numbers, video loci without `time`, `time` outside video rooms, formulas that do not parse, invalid table rows;
- limits: 500 palaces, 200 rooms per palace, 50 000 loci, 5 000 media, 10 000 entries, 1 000 table rows per scope, 16 MiB per JSON document, 1 GiB per media file, 10 000 characters per text.

## Importing

Imports always create new copies with new ids; a name that exists becomes `Name (2)`. Shared rows that already exist with the same content are skipped; a shared row that clashes with an existing different row is copied into each imported palace as its own row, so the imported formulas keep their meaning and existing palaces are untouched.
