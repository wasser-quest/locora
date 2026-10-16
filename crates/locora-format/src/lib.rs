//! The `.locora` file format (version 1).
//!
//! A `.locora` file is a ZIP archive:
//!
//! ```text
//! manifest.json            format, version, palaces, media, required plugins
//! palaces/<id>.json        one palace: stages and loci in route order
//! media/<sha256>.<ext>     images and videos, named by their SHA-256
//! plugins/<plugin-id>/…    reserved for plugin data (ignored by version 1 readers)
//! ```
//!
//! Reading is strict and fails closed: unknown fields, unknown files outside
//! `plugins/`, unsafe paths, broken references, wrong hashes and oversized
//! content are errors. Importing always creates new copies with new ids.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Seek, Write};

use locora_store::{NewLocus, PalaceTree, StageKind, Store};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

pub const FORMAT: &str = "locora";
pub const VERSION: u32 = 1;
pub const EXTENSION: &str = "locora";

/// Limits that keep hostile or broken files from exhausting the device.
pub mod limits {
    pub const PALACES: usize = 500;
    pub const STAGES_PER_PALACE: usize = 200;
    pub const LOCI: usize = 50_000;
    pub const MEDIA: usize = 5_000;
    pub const ENTRIES: usize = 10_000;
    /// Largest JSON document (manifest or palace).
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    /// Program that wrote the file, e.g. `locora 0.1.0`.
    pub generator: String,
    pub palaces: Vec<PalaceEntry>,
    #[serde(default)]
    pub media: Vec<MediaEntry>,
    /// Plugins whose data or features the file uses.
    #[serde(default)]
    pub requires: Vec<Requirement>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PalaceEntry {
    pub id: String,
    pub name: String,
    pub file: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaEntry {
    pub hash: String,
    pub mime: String,
    pub size: u64,
    pub file: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requirement {
    pub plugin: String,
    pub version: String,
    /// Without a required plugin the file cannot be used meaningfully.
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PalaceDoc {
    pub id: String,
    pub name: String,
    pub stages: Vec<StageDoc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageDoc {
    pub id: String,
    pub name: String,
    pub kind: StageKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<String>,
    pub loci: Vec<LocusDoc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocusDoc {
    pub id: String,
    pub label: String,
    pub memory: String,
    pub x: f64,
    pub y: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<f64>,
}

impl From<&PalaceTree> for PalaceDoc {
    fn from(tree: &PalaceTree) -> Self {
        Self {
            id: tree.palace.id.clone(),
            name: tree.palace.name.clone(),
            stages: tree
                .stages
                .iter()
                .map(|s| StageDoc {
                    id: s.stage.id.clone(),
                    name: s.stage.name.clone(),
                    kind: s.stage.kind,
                    media: s.stage.media.clone(),
                    loci: s
                        .loci
                        .iter()
                        .map(|l| LocusDoc {
                            id: l.id.clone(),
                            label: l.label.clone(),
                            memory: l.memory.clone(),
                            x: l.x,
                            y: l.y,
                            at: l.at,
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

fn extension(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/avif" => "avif",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/quicktime" => "mov",
        _ => "bin",
    }
}

const MIME_TYPES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/webp",
    "image/gif",
    "image/avif",
    "video/mp4",
    "video/webm",
    "video/quicktime",
];

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// What an export wrote.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportSummary {
    pub palaces: usize,
    pub loci: usize,
    pub media: usize,
}

/// Write the given palaces (store queries: ids or names) into a `.locora` archive.
pub fn export<W: Write + Seek>(store: &Store, palaces: &[String], out: W) -> Result<ExportSummary> {
    if palaces.is_empty() {
        return Err(invalid("keine Paläste zum Exportieren"));
    }
    let mut zip = ZipWriter::new(out);
    let json = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    // Media is already compressed; storing it keeps exports fast and seekable.
    let stored = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .large_file(true);
    let mut manifest = Manifest {
        format: FORMAT.into(),
        version: VERSION,
        generator: concat!("locora ", env!("CARGO_PKG_VERSION")).into(),
        palaces: Vec::new(),
        media: Vec::new(),
        requires: Vec::new(),
    };
    let mut written = BTreeSet::new();
    let mut loci = 0;
    for query in palaces {
        let tree = store.palace(query)?;
        if manifest.palaces.iter().any(|p| p.id == tree.palace.id) {
            continue;
        }
        let doc = PalaceDoc::from(&tree);
        loci += tree.route().count();
        for hash in doc.stages.iter().filter_map(|s| s.media.as_ref()) {
            if !written.insert(hash.clone()) {
                continue;
            }
            let media = store
                .media(hash)?
                .ok_or_else(|| invalid(format!("Medium {hash} fehlt im Speicher")))?;
            let path = store
                .media_file(hash)
                .ok_or_else(|| invalid("dieser Speicher hat keine Mediendateien"))?;
            let file = format!("media/{hash}.{}", extension(&media.mime));
            zip.start_file(file.as_str(), stored)?;
            std::io::copy(&mut std::fs::File::open(path)?, &mut zip)?;
            manifest.media.push(MediaEntry {
                hash: hash.clone(),
                mime: media.mime,
                size: media.size,
                file,
            });
        }
        let file = format!("palaces/{}.json", doc.id);
        zip.start_file(file.as_str(), json)?;
        zip.write_all(&serde_json::to_vec_pretty(&doc).expect("serializable"))?;
        manifest.palaces.push(PalaceEntry {
            id: doc.id,
            name: doc.name,
            file,
        });
    }
    zip.start_file("manifest.json", json)?;
    zip.write_all(&serde_json::to_vec_pretty(&manifest).expect("serializable"))?;
    zip.finish()?;
    Ok(ExportSummary {
        palaces: manifest.palaces.len(),
        loci,
        media: manifest.media.len(),
    })
}

/// A validated `.locora` archive, ready to preview or import.
pub struct Package<R> {
    archive: ZipArchive<R>,
    pub manifest: Manifest,
    pub palaces: Vec<PalaceDoc>,
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
        let manifest: Manifest = read_json(&mut archive, "manifest.json")?;
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
            return Err(invalid("die Datei muss 1 bis 500 Paläste enthalten"));
        }
        if manifest.media.len() > limits::MEDIA {
            return Err(invalid("zu viele Medien"));
        }

        let mut known = BTreeSet::from(["manifest.json".to_owned()]);
        let mut media = BTreeMap::new();
        for m in &manifest.media {
            let valid_hash = m.hash.len() == 64
                && m.hash
                    .bytes()
                    .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
            if !valid_hash {
                return Err(invalid(format!("ungültiger Hash „{}“", m.hash)));
            }
            if !MIME_TYPES.contains(&m.mime.as_str()) {
                return Err(invalid(format!(
                    "Medientyp „{}“ wird nicht unterstützt",
                    m.mime
                )));
            }
            if m.file != format!("media/{}.{}", m.hash, extension(&m.mime)) {
                return Err(invalid(format!(
                    "Mediendatei „{}“ passt nicht zum Hash",
                    m.file
                )));
            }
            if m.size > limits::MEDIA_BYTES {
                return Err(invalid(format!("{} ist zu groß", m.file)));
            }
            let entry = archive
                .by_name(&m.file)
                .map_err(|_| invalid(format!("{} fehlt", m.file)))?;
            if entry.size() != m.size {
                return Err(invalid(format!("{}: Größe stimmt nicht", m.file)));
            }
            if media.insert(m.hash.clone(), m.file.clone()).is_some() {
                return Err(invalid(format!("Medium {} doppelt", m.hash)));
            }
            known.insert(m.file.clone());
        }

        let mut palaces = Vec::new();
        let mut loci_total = 0;
        let mut ids = BTreeSet::new();
        for entry in &manifest.palaces {
            if entry.file != format!("palaces/{}.json", entry.id) {
                return Err(invalid(format!(
                    "Palastdatei „{}“ passt nicht zur Id",
                    entry.file
                )));
            }
            let doc: PalaceDoc = read_json(&mut archive, &entry.file)?;
            if doc.id != entry.id || doc.name != entry.name {
                return Err(invalid(format!(
                    "{}: Id oder Name passt nicht zum Manifest",
                    entry.file
                )));
            }
            check_text("Palastname", &doc.name)?;
            if doc.name.trim().is_empty() {
                return Err(invalid("ein Palast hat keinen Namen"));
            }
            if doc.stages.len() > limits::STAGES_PER_PALACE {
                return Err(invalid(format!("„{}“ hat zu viele Stages", doc.name)));
            }
            for id in std::iter::once(&doc.id)
                .chain(doc.stages.iter().map(|s| &s.id))
                .chain(doc.stages.iter().flat_map(|s| s.loci.iter().map(|l| &l.id)))
            {
                if id.is_empty() || !ids.insert(id.clone()) {
                    return Err(invalid(format!("Id „{id}“ ist leer oder doppelt")));
                }
            }
            for stage in &doc.stages {
                check_text("Stagename", &stage.name)?;
                match (&stage.kind, &stage.media) {
                    (StageKind::Blank, None) => {}
                    (StageKind::Blank, Some(_)) => {
                        return Err(invalid(format!(
                            "Stage „{}“ ohne Bild hat ein Medium",
                            stage.name
                        )));
                    }
                    (_, None) => {
                        return Err(invalid(format!(
                            "Stage „{}“ braucht ein Medium",
                            stage.name
                        )));
                    }
                    (kind, Some(hash)) => {
                        let file = media.get(hash).ok_or_else(|| {
                            invalid(format!("Stage „{}“: Medium {hash} fehlt", stage.name))
                        })?;
                        let video = file.ends_with(".mp4")
                            || file.ends_with(".webm")
                            || file.ends_with(".mov");
                        if (*kind == StageKind::Video) != video {
                            return Err(invalid(format!(
                                "Stage „{}“: Medium passt nicht zur Art",
                                stage.name
                            )));
                        }
                    }
                }
                for locus in &stage.loci {
                    check_text("Ort", &locus.label)?;
                    check_text("Erinnerung", &locus.memory)?;
                    let inside = |v: f64| v.is_finite() && (0.0..=1.0).contains(&v);
                    if !inside(locus.x) || !inside(locus.y) {
                        return Err(invalid(format!(
                            "Locus „{}“ liegt außerhalb der Stage",
                            locus.label
                        )));
                    }
                    if let Some(at) = locus.at
                        && !(at.is_finite() && at >= 0.0)
                    {
                        return Err(invalid(format!(
                            "Locus „{}“ hat eine ungültige Zeit",
                            locus.label
                        )));
                    }
                    if stage.kind == StageKind::Video && locus.at.is_none() {
                        return Err(invalid(format!(
                            "Video-Locus „{}“ hat keine Zeit",
                            locus.label
                        )));
                    }
                }
                loci_total += stage.loci.len();
            }
            if loci_total > limits::LOCI {
                return Err(invalid("zu viele Loci"));
            }
            known.insert(entry.file.clone());
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
        })
    }

    /// Number of loci in all palaces.
    pub fn loci(&self) -> usize {
        self.palaces
            .iter()
            .flat_map(|p| &p.stages)
            .map(|s| s.loci.len())
            .sum()
    }

    /// Import everything as new copies. Media hashes are verified while copying.
    /// Palace names that already exist get a suffix: „Planeten (2)“.
    pub fn import(mut self, store: &Store) -> Result<Vec<Imported>> {
        // Verify and store all media first, so a broken file adds no palaces.
        let mime: BTreeMap<String, String> = self
            .manifest
            .media
            .iter()
            .map(|m| (m.hash.clone(), m.mime.clone()))
            .collect();
        for m in self.manifest.media.clone() {
            let mut bytes = Vec::with_capacity(m.size.min(64 << 20) as usize);
            self.archive
                .by_name(&m.file)?
                .take(limits::MEDIA_BYTES + 1)
                .read_to_end(&mut bytes)?;
            if sha256_hex(&bytes) != m.hash {
                return Err(invalid(format!("{}: Inhalt passt nicht zum Hash", m.file)));
            }
            store.put_media(&bytes, &mime[&m.hash])?;
        }
        let mut imported = Vec::new();
        for doc in &self.palaces {
            let name = free_name(store, &doc.name)?;
            let palace = store.create_palace(&name)?;
            let mut loci = 0;
            for stage in &doc.stages {
                let created =
                    store.add_stage(&palace.id, &stage.name, stage.kind, stage.media.as_deref())?;
                for locus in &stage.loci {
                    store.add_locus(
                        &created.id,
                        NewLocus {
                            label: locus.label.clone(),
                            memory: locus.memory.clone(),
                            x: locus.x,
                            y: locus.y,
                            at: locus.at,
                        },
                    )?;
                    loci += 1;
                }
            }
            imported.push(Imported {
                id: palace.id,
                name,
                original_name: doc.name.clone(),
                loci,
            });
        }
        Ok(imported)
    }
}

/// A palace created by [`Package::import`].
#[derive(Debug, Clone, PartialEq)]
pub struct Imported {
    pub id: String,
    pub name: String,
    pub original_name: String,
    pub loci: usize,
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
