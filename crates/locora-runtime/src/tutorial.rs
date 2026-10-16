//! The built-in tutorial palace "Locora lernen" and the default locus marker.
//!
//! The tutorial teaches Locora with the method of loci itself: three rooms,
//! nine places, each with an animated Splinely picture. Its source and all
//! pictures are compiled in and built like any locora.toml.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use locora_api::core::{PalaceCreated, PalaceDeleted};
use locora_store::PalaceTree;

use crate::Profile;

pub const NAME: &str = "Locora lernen";

/// The default locus marker (assets/STIL.md 8b).
pub const DEFAULT_MARKER: &[u8] = include_bytes!("../../../assets/marker/locus.sply");

/// (path relative to `assets/`, bytes)
pub const FILES: &[(&str, &[u8])] = &[
    (
        "tutorial/locora-lernen.locora.toml",
        include_bytes!("../../../assets/tutorial/locora-lernen.locora.toml"),
    ),
    (
        "tutorial/halle.sply",
        include_bytes!("../../../assets/tutorial/halle.sply"),
    ),
    (
        "tutorial/bibliothek.sply",
        include_bytes!("../../../assets/tutorial/bibliothek.sply"),
    ),
    (
        "tutorial/garten.sply",
        include_bytes!("../../../assets/tutorial/garten.sply"),
    ),
    (
        "tutorial/teppich.sply",
        include_bytes!("../../../assets/tutorial/teppich.sply"),
    ),
    (
        "tutorial/laterne.sply",
        include_bytes!("../../../assets/tutorial/laterne.sply"),
    ),
    (
        "tutorial/regal.sply",
        include_bytes!("../../../assets/tutorial/regal.sply"),
    ),
    (
        "tutorial/leseplatz.sply",
        include_bytes!("../../../assets/tutorial/leseplatz.sply"),
    ),
    (
        "tutorial/uhr.sply",
        include_bytes!("../../../assets/tutorial/uhr.sply"),
    ),
    (
        "tutorial/brunnen.sply",
        include_bytes!("../../../assets/tutorial/brunnen.sply"),
    ),
    (
        "tutorial/baum.sply",
        include_bytes!("../../../assets/tutorial/baum.sply"),
    ),
    (
        "mascot/simo.sply",
        include_bytes!("../../../assets/mascot/simo.sply"),
    ),
    (
        "mascot/gluehwuermchen.sply",
        include_bytes!("../../../assets/mascot/gluehwuermchen.sply"),
    ),
];

/// The tutorial palace, if it exists.
pub fn find(profile: &Profile) -> Result<Option<locora_store::Palace>> {
    Ok(profile.store.find_palace(NAME)?)
}

/// Build the tutorial palace, replacing an existing one. Returns the new
/// palace and messages from plugins.
pub fn install(profile: &mut Profile) -> Result<(PalaceTree, Vec<String>)> {
    let dir = tempfile::tempdir().context("Temporären Ordner anlegen")?;
    for (path, bytes) in FILES {
        let target = dir.path().join(path);
        fs::create_dir_all(target.parent().expect("files live in folders"))?;
        fs::write(&target, bytes)?;
    }
    let source_file = dir.path().join(FILES[0].0);
    let source = locora_source::parse(&fs::read_to_string(&source_file)?)?;
    let plan = locora_source::check(&source, source_file.parent().unwrap_or(Path::new(".")))?;
    let mut notes = Vec::new();
    if let Some(old) = find(profile)? {
        profile.store.delete_palace(&old.id)?;
        notes.extend(profile.publish(&PalaceDeleted { palace_id: old.id })?);
    }
    let tree = locora_source::build(&profile.store, &plan)?;
    notes.extend(profile.publish(&PalaceCreated {
        palace_id: tree.palace.id.clone(),
        name: tree.palace.name.clone(),
    })?);
    Ok((tree, notes))
}
