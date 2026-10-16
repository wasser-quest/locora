# The `.locora` file format, version 1

A `.locora` file holds one or more memory palaces with their images and videos. It is a ZIP archive (entries *stored* or *deflated*):

```text
manifest.json            what the file contains
palaces/<id>.json        one palace: stages and loci in route order
media/<sha256>.<ext>     images and videos, named by the SHA-256 of their bytes
plugins/<plugin-id>/…    reserved for plugin data; version 1 readers ignore it
```

The reference implementation is [`crates/locora-format`](../crates/locora-format). `locora export` and `locora import` use it.

## `manifest.json`

```json
{
  "format": "locora",
  "version": 1,
  "generator": "locora 0.1.0",
  "palaces": [{ "id": "0199f…", "name": "Planeten", "file": "palaces/0199f….json" }],
  "media": [{ "hash": "9f86d0…", "mime": "image/png", "size": 48213, "file": "media/9f86d0….png" }],
  "requires": [{ "plugin": "review", "version": "0.1", "required": false }]
}
```

- `version` greater than the reader's version: the file is rejected with "needs a newer Locora".
- `media[].file` is always `media/<hash>.<ext>`; extensions: `png jpg webp gif avif mp4 webm mov`.
- `requires`: plugins the file uses. A missing `required` plugin blocks the import.

## `palaces/<id>.json`

```json
{
  "id": "0199f…",
  "name": "Planeten",
  "stages": [
    {
      "id": "0199f…",
      "name": "Wohnung",
      "kind": "image",
      "media": "9f86d0…",
      "loci": [
        { "id": "0199f…", "label": "Herd", "memory": "Merkur", "x": 0.42, "y": 0.61 },
        { "id": "0199f…", "label": "Tür", "memory": "These", "x": 0.5, "y": 0.5, "at": 83.0 }
      ]
    }
  ]
}
```

- `kind`: `blank` (no media), `image` or `video` (`media` required and must match the kind).
- `x`, `y`: position on the stage from 0 to 1 (left/top = 0).
- `at`: seconds into the video; required on video stages.
- Order in the arrays is the route order.

## Rules for readers

Readers fail closed instead of guessing:

- unknown fields, unknown files outside `plugins/`, duplicate entries and ids;
- absolute paths, `..`, `.` or backslashes in entry names;
- media whose size or SHA-256 does not match the manifest (hashes are checked while importing, before anything is written);
- positions outside 0–1, non-finite numbers, video loci without `at`;
- limits: 500 palaces, 200 stages per palace, 50 000 loci, 5 000 media, 10 000 entries, 16 MiB per JSON document, 1 GiB per media file, 10 000 characters per text.

Imports always create new copies with new ids. A palace whose name already exists is imported as `Name (2)`.
