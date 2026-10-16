# The `.locora` file format, version 1

A `.locora` file holds one or more memory palaces with their rooms, loci, workbook tables, images and videos. It is a ZIP archive:

```text
mimetype                 application/vnd.locora+zip   (first entry, stored uncompressed)
manifest.json            what the file contains
palaces/<id>.json        one palace: rooms, loci in route order, its own tables
tables.json              tables shared by all palaces (only if manifest.sharedTables)
media/<sha256>.<ext>     images and videos, named by the SHA-256 of their bytes
plugins/<plugin-id>/…    reserved for plugin data; version 1 readers ignore it
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

## `tables.json`

An array of rows (same shape as above) shared by all palaces. A palace's own row with the same `table` and `key` replaces the shared one.

## Rules for readers

Readers fail closed instead of guessing:

- a missing or wrong `mimetype` entry; unknown fields; unknown files outside `plugins/`; duplicate entries, ids or table rows;
- absolute paths, `..`, `.` or backslashes in entry names; ids outside `[A-Za-z0-9_-]{1,64}`;
- media whose size or SHA-256 does not match the manifest (hashes are checked while importing, before anything is written);
- positions outside 0–1, non-finite numbers, video loci without `time`, `time` outside video rooms, formulas that do not parse, invalid table rows;
- limits: 500 palaces, 200 rooms per palace, 50 000 loci, 5 000 media, 10 000 entries, 1 000 table rows per scope, 16 MiB per JSON document, 1 GiB per media file, 10 000 characters per text.

## Importing

Imports always create new copies with new ids; a name that exists becomes `Name (2)`. Shared rows that already exist with the same content are skipped; a shared row that clashes with an existing different row is copied into each imported palace as its own row, so the imported formulas keep their meaning and existing palaces are untouched.
