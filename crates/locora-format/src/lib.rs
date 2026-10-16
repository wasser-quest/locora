//! The `.locora` file format: version 1 (Recall) and version 2 (Quiz/Guess).
//!
//! A `.locora` file is a ZIP archive:
//!
//! ```text
//! mimetype                 "application/vnd.locora+zip", first entry, uncompressed
//! manifest.json            format, version, palaces, media, plugins
//! palaces/<id>.json        one palace: rooms, loci in route order, its own tables
//! tables.json              tables shared by all palaces (optional)
//! media/<sha256>.<ext>     images and videos, named by the SHA-256 of their bytes
//! plugins/<plugin-id>/…    reserved for plugin data; ignored by version 1 readers
//! ```
//!
//! Writing is deterministic: the same content always gives the same bytes.
//! Reading is strict and fails closed: unknown fields, unknown files outside
//! `plugins/`, unsafe paths or ids, broken references, wrong sizes or hashes,
//! invalid formulas and oversized content are errors. Importing always creates
//! new copies with new ids.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Seek, Write};

use locora_formula::Row;
use locora_formula::workbook::{Binding, BindingField};
use locora_store::{NewLocus, PalaceTree, RoomKind, Store};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

pub const FORMAT: &str = "locora";
pub const VERSION: u32 = 3;
pub const EXTENSION: &str = "locora";
/// Content of the first entry, `mimetype`.
pub const MEDIA_TYPE: &str = "application/vnd.locora+zip";

const MANIFEST: &str = "manifest.json";
const SHARED_TABLES: &str = "tables.json";

/// Limits that keep hostile or broken files from exhausting the device.
pub mod limits {
    pub const PALACES: usize = 500;
    pub const ROOMS_PER_PALACE: usize = 200;
    pub const LOCI: usize = 50_000;
    pub const MEDIA: usize = 5_000;
    pub const ENTRIES: usize = 10_000;
    pub const TABLE_ROWS: usize = 1000;
    /// Largest JSON document (manifest, palace or tables).
    pub const JSON_BYTES: u64 = 16 << 20;
    /// Largest single media file.
    pub const MEDIA_BYTES: u64 = 1 << 30;
    pub const TEXT_CHARS: usize = 10_000;
}

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("Keine gültige .locora-Datei: {0}")]
    Invalid(String),
    #[error("Diese Datei braucht eine neuere Locora-Version (Format {0}, unterstützt: {VERSION})")]
    TooNew(u32),
    #[error("ZIP: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("Datei: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON in {0}: {1}")]
    Json(String, serde_json::Error),
    #[error(transparent)]
    Store(#[from] locora_store::StoreError),
}

pub type Result<T, E = FormatError> = std::result::Result<T, E>;

fn invalid(message: impl Into<String>) -> FormatError {
    FormatError::Invalid(message.into())
}

// ---------------------------------------------------------------- documents

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    /// Always `locora`.
    pub format: String,
    pub version: u32,
    /// Program that wrote the file, e.g. `locora 0.1.0`.
    pub created_by: String,
    pub palaces: Vec<PalaceEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<MediaEntry>,
    /// Plugins whose data or features the file uses.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plugins: Vec<PluginEntry>,
    /// Whether `tables.json` holds shared tables.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shared_tables: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PalaceEntry {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MediaEntry {
    /// SHA-256 of the bytes, lowercase hex. The file is `media/<sha256>.<ext>`.
    pub sha256: String,
    /// IANA media type, e.g. `image/png`.
    pub media_type: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginEntry {
    pub id: String,
    pub version: String,
    /// Without a required plugin the file cannot be used meaningfully.
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PalaceDoc {
    pub id: String,
    pub name: String,
    /// SHA-256 of a custom locus marker (`.sply`; format 2). None = default marker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
    pub rooms: Vec<RoomDoc>,
    /// Workbook tables of this palace (shared tables are in `tables.json`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tables: Vec<Row>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoomDoc {
    pub id: String,
    pub name: String,
    /// SHA-256 of the room's image or video; none for a plain room.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backdrop: Option<String>,
    pub loci: Vec<LocusDoc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocusDoc {
    /// Picture width as a fraction of the room width (custom widths require format 3).
    #[serde(
        default = "locora_store::default_image_size",
        skip_serializing_if = "default_size"
    )]
    pub image_size: f64,
    /// Zoom on the locus while asking, 1 (whole room) to 4 (other than 1 requires format 3).
    #[serde(
        default = "locora_store::default_zoom",
        skip_serializing_if = "default_zoom"
    )]
    pub zoom: f64,
    pub id: String,
    /// The place in the room (Ort), e.g. "Herd".
    pub place: String,
    /// What is stored at the place, e.g. "Merkur".
    pub item: String,
    /// Position in the room, 0..1 from the left / top.
    pub x: f64,
    pub y: f64,
    /// Seconds into the video (video rooms only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<f64>,
    #[serde(default, skip_serializing_if = "Bindings::is_empty")]
    pub bindings: Bindings,
    #[serde(default, skip_serializing_if = "locora_store::Answer::is_recall")]
    pub answer: locora_store::Answer,
    /// SHA-256 of a picture shown at the locus (image or `.sply` animation; format 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Memory aid shown after the answer (format 2).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

fn default_size(size: &f64) -> bool {
    *size == locora_store::default_image_size()
}

fn default_zoom(zoom: &f64) -> bool {
    *zoom == locora_store::default_zoom()
}

/// Formulas computing locus fields, one per field.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Bindings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
}

impl Bindings {
    pub fn is_empty(&self) -> bool {
        self.list().is_empty()
    }

    /// The bindings as store records, in a fixed field order.
    pub fn list(&self) -> Vec<Binding> {
        [
            (BindingField::Place, &self.place),
            (BindingField::Item, &self.item),
            (BindingField::X, &self.x),
            (BindingField::Y, &self.y),
            (BindingField::Visible, &self.visible),
            (BindingField::Active, &self.active),
        ]
        .into_iter()
        .filter_map(|(field, expr)| {
            expr.as_ref().map(|e| Binding {
                field,
                expression: e.clone(),
            })
        })
        .collect()
    }

    pub fn from_list(list: &[Binding]) -> Self {
        let mut out = Self::default();
        for b in list {
            let slot = match b.field {
                BindingField::Place => &mut out.place,
                BindingField::Item => &mut out.item,
                BindingField::X => &mut out.x,
                BindingField::Y => &mut out.y,
                BindingField::Visible => &mut out.visible,
                BindingField::Active => &mut out.active,
            };
            *slot = Some(b.expression.clone());
        }
        out
    }
}

impl From<&PalaceTree> for PalaceDoc {
    fn from(tree: &PalaceTree) -> Self {
        Self {
            id: tree.palace.id.clone(),
            name: tree.palace.name.clone(),
            marker: tree.palace.marker.clone(),
            rooms: tree
                .rooms
                .iter()
                .map(|r| RoomDoc {
                    id: r.room.id.clone(),
                    name: r.room.name.clone(),
                    backdrop: r.room.media.clone(),
                    loci: r
                        .loci
                        .iter()
                        .map(|l| LocusDoc {
                            id: l.id.clone(),
                            place: l.place.clone(),
                            item: l.item.clone(),
                            x: l.x,
                            y: l.y,
                            time: l.time,
                            bindings: Bindings::from_list(&l.bindings),
                            answer: l.answer.clone(),
                            image: l.image.clone(),
                            image_size: l.image_size,
                            zoom: l.zoom,
                            note: l.note.clone(),
                        })
                        .collect(),
                })
                .collect(),
            tables: tree.tables.clone(),
        }
    }
}

// ---------------------------------------------------------------- helpers

const MEDIA_TYPES: &[(&str, &str)] = &[
    ("image/png", "png"),
    ("image/jpeg", "jpg"),
    ("image/webp", "webp"),
    ("image/gif", "gif"),
    ("image/avif", "avif"),
    ("video/mp4", "mp4"),
    ("video/webm", "webm"),
    ("video/quicktime", "mov"),
    (locora_store::SPLINELY, "sply"),
];

fn extension(media_type: &str) -> Option<&'static str> {
    MEDIA_TYPES
        .iter()
        .find(|(t, _)| *t == media_type)
        .map(|(_, e)| *e)
}

fn media_path(m: &MediaEntry) -> String {
    format!(
        "media/{}.{}",
        m.sha256,
        extension(&m.media_type).unwrap_or("bin")
    )
}

fn palace_path(id: &str) -> String {
    format!("palaces/{id}.json")
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Ids end up in file names, so they are restricted to a safe alphabet.
fn safe_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn options(method: CompressionMethod) -> SimpleFileOptions {
    // A fixed timestamp keeps exports byte-for-byte reproducible.
    SimpleFileOptions::default()
        .compression_method(method)
        .last_modified_time(DateTime::default())
        .unix_permissions(0o644)
}

// ---------------------------------------------------------------- export

/// What an export wrote.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportSummary {
    pub palaces: usize,
    pub loci: usize,
    pub media: usize,
}

/// Write the given palaces (store queries: ids or names) into a `.locora` archive.
pub fn export<W: Write + Seek>(store: &Store, palaces: &[String], out: W) -> Result<ExportSummary> {
    export_with(store, palaces, &[], out)
}

/// Data a plugin adds to an export, stored as `plugins/<plugin>/<path>`.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginFile {
    pub plugin: PluginEntry,
    /// Path inside the plugin's folder, e.g. `progress.json`.
    pub path: String,
    pub bytes: Vec<u8>,
}

/// File names inside `plugins/<id>/`: plain segments, no `..`, bounded length.
fn safe_plugin_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 200
        && path.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        })
}

/// Like [`export`], plus plugin data files (e.g. review progress).
pub fn export_with<W: Write + Seek>(
    store: &Store,
    palaces: &[String],
    plugin_files: &[PluginFile],
    out: W,
) -> Result<ExportSummary> {
    let mut plugin_files: Vec<&PluginFile> = plugin_files.iter().collect();
    plugin_files.sort_by(|a, b| (&a.plugin.id, &a.path).cmp(&(&b.plugin.id, &b.path)));
    for f in &plugin_files {
        if !safe_id(&f.plugin.id) || !safe_plugin_path(&f.path) {
            return Err(invalid(format!(
                "ungültige Plugin-Datei {}/{}",
                f.plugin.id, f.path
            )));
        }
        if f.bytes.len() as u64 > limits::JSON_BYTES {
            return Err(invalid(format!(
                "Plugin-Datei {}/{} ist zu groß",
                f.plugin.id, f.path
            )));
        }
    }
    let mut plugin_entries: Vec<PluginEntry> = Vec::new();
    for f in &plugin_files {
        if !plugin_entries.iter().any(|e| e.id == f.plugin.id) {
            plugin_entries.push(f.plugin.clone());
        }
    }
    if palaces.is_empty() {
        return Err(invalid("keine Paläste zum Exportieren"));
    }
    // Collect everything first so the manifest can come right after `mimetype`.
    let mut docs: Vec<PalaceDoc> = Vec::new();
    let mut media: BTreeMap<String, MediaEntry> = BTreeMap::new();
    for query in palaces {
        let tree = store.palace(query)?;
        if docs.iter().any(|d| d.id == tree.palace.id) {
            continue;
        }
        let pictures = tree
            .route()
            .filter_map(|l| l.image.as_ref())
            .chain(tree.palace.marker.as_ref());
        for hash in tree
            .rooms
            .iter()
            .filter_map(|r| r.room.media.as_ref())
            .chain(pictures)
        {
            if media.contains_key(hash) {
                continue;
            }
            let m = store
                .media(hash)?
                .ok_or_else(|| invalid(format!("Medium {hash} fehlt im Speicher")))?;
            media.insert(
                hash.clone(),
                MediaEntry {
                    sha256: m.hash,
                    media_type: m.mime,
                    size: m.size,
                },
            );
        }
        docs.push(PalaceDoc::from(&tree));
    }
    let shared = store.rows(None)?;
    let manifest = Manifest {
        format: FORMAT.into(),
        version: if docs
            .iter()
            .flat_map(|p| &p.rooms)
            .flat_map(|r| &r.loci)
            .all(|l| {
                l.answer.is_recall()
                    && l.image.is_none()
                    && l.note.is_empty()
                    && default_size(&l.image_size)
                    && default_zoom(&l.zoom)
            })
            && docs.iter().all(|d| d.marker.is_none())
        {
            1
        } else {
            if docs
                .iter()
                .flat_map(|p| &p.rooms)
                .flat_map(|r| &r.loci)
                .any(|l| !default_size(&l.image_size) || !default_zoom(&l.zoom))
            {
                3
            } else {
                2
            }
        },
        created_by: concat!("locora ", env!("CARGO_PKG_VERSION")).into(),
        palaces: docs
            .iter()
            .map(|d| PalaceEntry {
                id: d.id.clone(),
                name: d.name.clone(),
            })
            .collect(),
        media: media.values().cloned().collect(),
        plugins: plugin_entries,
        shared_tables: !shared.is_empty(),
    };

    let mut zip = ZipWriter::new(out);
    let json = options(CompressionMethod::Deflated);
    // Media is already compressed; storing it keeps exports fast and seekable.
    let stored = options(CompressionMethod::Stored).large_file(true);
    zip.start_file("mimetype", options(CompressionMethod::Stored))?;
    zip.write_all(MEDIA_TYPE.as_bytes())?;
    zip.start_file(MANIFEST, json)?;
    zip.write_all(&serde_json::to_vec_pretty(&manifest).expect("serializable"))?;
    for doc in &docs {
        zip.start_file(palace_path(&doc.id).as_str(), json)?;
        zip.write_all(&serde_json::to_vec_pretty(doc).expect("serializable"))?;
    }
    if !shared.is_empty() {
        zip.start_file(SHARED_TABLES, json)?;
        zip.write_all(&serde_json::to_vec_pretty(&shared).expect("serializable"))?;
    }
    for f in &plugin_files {
        zip.start_file(format!("plugins/{}/{}", f.plugin.id, f.path).as_str(), json)?;
        zip.write_all(&f.bytes)?;
    }
    for m in media.values() {
        if extension(&m.media_type).is_none() {
            return Err(invalid(format!(
                "Medientyp „{}“ wird nicht unterstützt",
                m.media_type
            )));
        }
        let path = store
            .media_file(&m.sha256)
            .ok_or_else(|| invalid("dieser Speicher hat keine Mediendateien"))?;
        zip.start_file(media_path(m).as_str(), stored)?;
        std::io::copy(&mut std::fs::File::open(path)?, &mut zip)?;
    }
    zip.finish()?;
    Ok(ExportSummary {
        palaces: docs.len(),
        loci: docs
            .iter()
            .flat_map(|d| &d.rooms)
            .map(|r| r.loci.len())
            .sum(),
        media: media.len(),
    })
}

// ---------------------------------------------------------------- import

/// A validated `.locora` archive, ready to preview or import.
pub struct Package<R> {
    archive: ZipArchive<R>,
    pub manifest: Manifest,
    pub palaces: Vec<PalaceDoc>,
    /// Rows of `tables.json`.
    pub shared_tables: Vec<Row>,
}

fn read_json<R: Read + Seek, T: serde::de::DeserializeOwned>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<T> {
    let file = archive
        .by_name(name)
        .map_err(|_| invalid(format!("{name} fehlt")))?;
    if file.size() > limits::JSON_BYTES {
        return Err(invalid(format!("{name} ist zu groß")));
    }
    let mut bytes = Vec::new();
    file.take(limits::JSON_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limits::JSON_BYTES {
        return Err(invalid(format!("{name} ist zu groß")));
    }
    serde_json::from_slice(&bytes).map_err(|e| FormatError::Json(name.into(), e))
}

fn check_text(what: &str, text: &str) -> Result<()> {
    if text.chars().count() > limits::TEXT_CHARS {
        return Err(invalid(format!("{what} ist zu lang")));
    }
    Ok(())
}

/// Validate one scope of tables: valid rows, unique `Table.key`, bounded size.
fn check_tables(scope: &str, rows: &[Row]) -> Result<()> {
    if rows.len() > limits::TABLE_ROWS {
        return Err(invalid(format!(
            "{scope}: höchstens {} Tabellenzeilen",
            limits::TABLE_ROWS
        )));
    }
    let mut keys = BTreeSet::new();
    for row in rows {
        row.validate().map_err(|e| {
            invalid(format!(
                "{scope}: Tabelle {}.{}: {}",
                row.table, row.key, e.0
            ))
        })?;
        if !keys.insert((row.table.clone(), row.key.clone())) {
            return Err(invalid(format!(
                "{scope}: Zeile {}.{} doppelt",
                row.table, row.key
            )));
        }
    }
    Ok(())
}

impl<R: Read + Seek> Package<R> {
    /// Open and validate an archive. Nothing is written anywhere.
    pub fn open(reader: R) -> Result<Self> {
        let mut archive = ZipArchive::new(reader)?;
        if archive.len() > limits::ENTRIES {
            return Err(invalid("zu viele Einträge"));
        }
        let mut names = BTreeSet::new();
        for i in 0..archive.len() {
            let file = archive.by_index_raw(i)?;
            let name = file.name()?.into_owned();
            if file.enclosed_name().is_none()
                || name.starts_with('/')
                || name.contains('\\')
                || name.split('/').any(|part| part == ".." || part == ".")
            {
                return Err(invalid(format!("unsicherer Pfad „{name}“")));
            }
            if !names.insert(name.clone()) {
                return Err(invalid(format!("doppelter Eintrag „{name}“")));
            }
        }
        {
            let mut first = archive.by_index(0)?;
            if first.name()? != "mimetype" || first.compression() != CompressionMethod::Stored {
                return Err(invalid(
                    "der erste Eintrag muss `mimetype` sein (unkomprimiert)",
                ));
            }
            let mut kind = String::new();
            first.by_ref().take(64).read_to_string(&mut kind)?;
            if kind != MEDIA_TYPE {
                return Err(invalid(format!(
                    "`mimetype` ist „{kind}“ statt „{MEDIA_TYPE}“"
                )));
            }
        }
        let manifest: Manifest = read_json(&mut archive, MANIFEST)?;
        if manifest.format != FORMAT {
            return Err(invalid(format!(
                "Format „{}“ statt „{FORMAT}“",
                manifest.format
            )));
        }
        if manifest.version > VERSION {
            return Err(FormatError::TooNew(manifest.version));
        }
        if manifest.version == 0 {
            return Err(invalid("Version 0 gibt es nicht"));
        }
        if manifest.palaces.is_empty() || manifest.palaces.len() > limits::PALACES {
            return Err(invalid(format!(
                "die Datei muss 1 bis {} Paläste enthalten",
                limits::PALACES
            )));
        }
        if manifest.media.len() > limits::MEDIA {
            return Err(invalid("zu viele Medien"));
        }
        for p in &manifest.plugins {
            if !safe_id(&p.id) {
                return Err(invalid(format!("ungültige Plugin-Id „{}“", p.id)));
            }
        }

        let mut known = BTreeSet::from(["mimetype".to_owned(), MANIFEST.to_owned()]);
        let shared_tables: Vec<Row> = if manifest.shared_tables {
            known.insert(SHARED_TABLES.into());
            read_json(&mut archive, SHARED_TABLES)?
        } else {
            Vec::new()
        };
        check_tables("gemeinsame Tabellen", &shared_tables)?;

        let mut media: BTreeMap<String, &str> = BTreeMap::new();
        for m in &manifest.media {
            let valid_hash = m.sha256.len() == 64
                && m.sha256
                    .bytes()
                    .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
            if !valid_hash {
                return Err(invalid(format!("ungültiger SHA-256 „{}“", m.sha256)));
            }
            if extension(&m.media_type).is_none() {
                return Err(invalid(format!(
                    "Medientyp „{}“ wird nicht unterstützt",
                    m.media_type
                )));
            }
            if m.size > limits::MEDIA_BYTES {
                return Err(invalid(format!("{} ist zu groß", media_path(m))));
            }
            let path = media_path(m);
            let entry = archive
                .by_name(&path)
                .map_err(|_| invalid(format!("{path} fehlt")))?;
            if entry.size() != m.size {
                return Err(invalid(format!("{path}: Größe stimmt nicht")));
            }
            if media.insert(m.sha256.clone(), &m.media_type).is_some() {
                return Err(invalid(format!("Medium {} doppelt", m.sha256)));
            }
            known.insert(path);
        }

        let mut palaces = Vec::new();
        let mut loci_total = 0;
        let mut ids = BTreeSet::new();
        for entry in &manifest.palaces {
            if !safe_id(&entry.id) {
                return Err(invalid(format!("ungültige Palast-Id „{}“", entry.id)));
            }
            let path = palace_path(&entry.id);
            let doc: PalaceDoc = read_json(&mut archive, &path)?;
            if doc.id != entry.id || doc.name != entry.name {
                return Err(invalid(format!(
                    "{path}: Id oder Name passt nicht zum Manifest"
                )));
            }
            check_text("Palastname", &doc.name)?;
            if let Some(hash) = &doc.marker {
                if manifest.version == 1 {
                    return Err(invalid("Eigene Marker benötigen Format 2"));
                }
                match media.get(hash) {
                    Some(&t) if t == locora_store::SPLINELY => {}
                    Some(t) => {
                        return Err(invalid(format!(
                            "„{}“: der Marker muss eine .sply-Animation sein, nicht {t}",
                            doc.name
                        )));
                    }
                    None => return Err(invalid(format!("„{}“: Marker {hash} fehlt", doc.name))),
                }
            }
            if doc.name.trim().is_empty() {
                return Err(invalid("ein Palast hat keinen Namen"));
            }
            if doc.rooms.len() > limits::ROOMS_PER_PALACE {
                return Err(invalid(format!("„{}“ hat zu viele Räume", doc.name)));
            }
            for id in std::iter::once(&doc.id)
                .chain(doc.rooms.iter().map(|r| &r.id))
                .chain(doc.rooms.iter().flat_map(|r| r.loci.iter().map(|l| &l.id)))
            {
                if !safe_id(id) || !ids.insert(id.clone()) {
                    return Err(invalid(format!("Id „{id}“ ist ungültig oder doppelt")));
                }
            }
            for room in &doc.rooms {
                check_text("Raumname", &room.name)?;
                let video = match &room.backdrop {
                    None => false,
                    Some(hash) => media
                        .get(hash)
                        .ok_or_else(|| {
                            invalid(format!("Raum „{}“: Medium {hash} fehlt", room.name))
                        })?
                        .starts_with("video/"),
                };
                for locus in &room.loci {
                    locora_store::check_image_size(locus.image_size)
                        .map_err(|e| invalid(e.to_string()))?;
                    if manifest.version < 3 && !default_size(&locus.image_size) {
                        return Err(invalid("Eigene Locus-Bildgrößen benötigen Format 3"));
                    }
                    locora_store::check_zoom(locus.zoom).map_err(|e| invalid(e.to_string()))?;
                    if manifest.version < 3 && !default_zoom(&locus.zoom) {
                        return Err(invalid("Zoom auf einen Locus benötigt Format 3"));
                    }
                    check_text("Ort", &locus.place)?;
                    check_text("Inhalt", &locus.item)?;
                    locus.answer.validate().map_err(invalid)?;
                    if manifest.version == 1 && !locus.answer.is_recall() {
                        return Err(invalid("Antworttypen benötigen Format 2"));
                    }
                    if let Some(hash) = &locus.image {
                        if manifest.version == 1 {
                            return Err(invalid("Locus-Bilder benötigen Format 2"));
                        }
                        let kind = media.get(hash).ok_or_else(|| {
                            invalid(format!("Locus „{}“: Bild {hash} fehlt", locus.place))
                        })?;
                        if !locora_store::is_picture(kind) {
                            return Err(invalid(format!(
                                "Locus „{}“: {kind} ist kein Bild",
                                locus.place
                            )));
                        }
                    }
                    for option in &locus.answer.options {
                        check_text("Quizoption", &option.text)?;
                    }
                    let inside = |v: f64| v.is_finite() && (0.0..=1.0).contains(&v);
                    if !inside(locus.x) || !inside(locus.y) {
                        return Err(invalid(format!(
                            "Locus „{}“ liegt außerhalb des Raums",
                            locus.place
                        )));
                    }
                    match (video, locus.time) {
                        (true, None) => {
                            return Err(invalid(format!(
                                "Video-Locus „{}“ hat keine Zeit",
                                locus.place
                            )));
                        }
                        (false, Some(_)) => {
                            return Err(invalid(format!(
                                "Locus „{}“ hat eine Zeit, aber der Raum hat kein Video",
                                locus.place
                            )));
                        }
                        (true, Some(t)) if !(t.is_finite() && t >= 0.0) => {
                            return Err(invalid(format!(
                                "Locus „{}“ hat eine ungültige Zeit",
                                locus.place
                            )));
                        }
                        _ => {}
                    }
                    for binding in locus.bindings.list() {
                        check_text("Formel", &binding.expression)?;
                        binding.validate().map_err(|e| {
                            invalid(format!(
                                "Locus „{}“: Formel für {}: {}",
                                locus.place,
                                binding.field.name(),
                                e.0
                            ))
                        })?;
                    }
                }
                loci_total += room.loci.len();
            }
            if loci_total > limits::LOCI {
                return Err(invalid("zu viele Loci"));
            }
            check_tables(&format!("„{}“", doc.name), &doc.tables)?;
            known.insert(path);
            palaces.push(doc);
        }
        for name in &names {
            if !known.contains(name) && !name.starts_with("plugins/") {
                return Err(invalid(format!("unbekannte Datei „{name}“")));
            }
        }
        Ok(Self {
            archive,
            manifest,
            palaces,
            shared_tables,
        })
    }

    /// A plugin's data file (`plugins/<plugin>/<path>`), if the archive has one.
    /// Plugin data is untrusted input; the plugin glue must validate it.
    pub fn plugin_file(&mut self, plugin: &str, path: &str) -> Result<Option<Vec<u8>>> {
        if !safe_id(plugin) || !safe_plugin_path(path) {
            return Err(invalid(format!("ungültige Plugin-Datei {plugin}/{path}")));
        }
        let name = format!("plugins/{plugin}/{path}");
        let Ok(file) = self.archive.by_name(&name) else {
            return Ok(None);
        };
        if file.size() > limits::JSON_BYTES {
            return Err(invalid(format!("{name} ist zu groß")));
        }
        let mut bytes = Vec::new();
        file.take(limits::JSON_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > limits::JSON_BYTES {
            return Err(invalid(format!("{name} ist zu groß")));
        }
        Ok(Some(bytes))
    }

    /// Number of loci in all palaces.
    pub fn loci(&self) -> usize {
        self.palaces
            .iter()
            .flat_map(|p| &p.rooms)
            .map(|r| r.loci.len())
            .sum()
    }

    /// Import everything as new copies. Media hashes are verified before
    /// anything else is written. Palace names that already exist get a
    /// suffix: „Planeten (2)“.
    pub fn import(mut self, store: &Store) -> Result<Vec<Imported>> {
        let mut kinds: BTreeMap<String, RoomKind> = BTreeMap::new();
        for m in self.manifest.media.clone() {
            let mut bytes = Vec::with_capacity(m.size.min(64 << 20) as usize);
            self.archive
                .by_name(&media_path(&m))?
                .take(limits::MEDIA_BYTES + 1)
                .read_to_end(&mut bytes)?;
            if sha256_hex(&bytes) != m.sha256 {
                return Err(invalid(format!(
                    "{}: Inhalt passt nicht zum SHA-256",
                    media_path(&m)
                )));
            }
            store.put_media(&bytes, &m.media_type)?;
            let kind = if m.media_type.starts_with("video/") {
                RoomKind::Video
            } else {
                RoomKind::Image
            };
            kinds.insert(m.sha256, kind);
        }
        // Shared rows: add new ones, skip identical ones. A clashing row becomes
        // a local row of every imported palace, so its formulas keep seeing
        // exactly the values they were written for.
        let existing = store.rows(None)?;
        let mut clashing = Vec::new();
        for row in &self.shared_tables {
            match existing
                .iter()
                .find(|r| r.table == row.table && r.key == row.key)
            {
                None => store.set_row(None, row)?,
                Some(same) if same == row => {}
                Some(_) => clashing.push(row.clone()),
            }
        }
        let mut imported = Vec::new();
        for doc in &self.palaces {
            let name = free_name(store, &doc.name)?;
            let palace = store.create_palace(&name)?;
            if let Some(marker) = &doc.marker {
                store.set_marker(&palace.id, Some(marker))?;
            }
            for row in &clashing {
                if !doc
                    .tables
                    .iter()
                    .any(|r| r.table == row.table && r.key == row.key)
                {
                    store.set_row(Some(&palace.id), row)?;
                }
            }
            for row in &doc.tables {
                store.set_row(Some(&palace.id), row)?;
            }
            let mut loci = 0;
            let mut locus_ids = Vec::new();
            for room in &doc.rooms {
                let kind = room.backdrop.as_ref().map_or(RoomKind::Blank, |h| kinds[h]);
                let created =
                    store.add_room(&palace.id, &room.name, kind, room.backdrop.as_deref())?;
                for locus in &room.loci {
                    let new = store.add_locus(
                        &created.id,
                        NewLocus {
                            place: locus.place.clone(),
                            item: locus.item.clone(),
                            x: locus.x,
                            y: locus.y,
                            time: locus.time,
                            answer: locus.answer.clone(),
                            image: locus.image.clone(),
                            image_size: locus.image_size,
                            zoom: locus.zoom,
                            note: locus.note.clone(),
                        },
                    )?;
                    for binding in locus.bindings.list() {
                        store.set_binding(&new.id, &binding)?;
                    }
                    locus_ids.push((locus.id.clone(), new.id));
                    loci += 1;
                }
            }
            imported.push(Imported {
                id: palace.id,
                original_id: doc.id.clone(),
                name,
                original_name: doc.name.clone(),
                loci,
                locus_ids,
                localized_rows: clashing.len(),
            });
        }
        Ok(imported)
    }
}

/// A palace created by [`Package::import`].
#[derive(Debug, Clone, PartialEq)]
pub struct Imported {
    pub id: String,
    /// The palace id inside the archive.
    pub original_id: String,
    pub name: String,
    pub original_name: String,
    pub loci: usize,
    /// (id in the archive, new id) for every locus, in route order.
    pub locus_ids: Vec<(String, String)>,
    /// Shared rows that clashed with existing shared rows and were copied
    /// into this palace instead.
    pub localized_rows: usize,
}

fn free_name(store: &Store, name: &str) -> Result<String> {
    if store.find_palace(name)?.is_none() {
        return Ok(name.to_owned());
    }
    for n in 2.. {
        let candidate = format!("{name} ({n})");
        if store.find_palace(&candidate)?.is_none() {
            return Ok(candidate);
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests;
