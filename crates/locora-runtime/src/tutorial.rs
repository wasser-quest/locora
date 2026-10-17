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

/// Simo, the mascot (512 × 768, assets/STIL.md), for welcome and empty states.
pub const SIMO: &[u8] = include_bytes!("../../../assets/mascot/simo.sply");

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
    Ok(profile
        .store
        .find_palace(&locora_source::bundled_palace_id("tutorial/v1"))?
        .or(profile.store.find_palace(NAME)?))
}

/// Build the tutorial palace, replacing an existing one. Returns the new
/// palace and messages from plugins.
pub fn install(profile: &mut Profile) -> Result<(PalaceTree, Vec<String>)> {
    install_files(profile, NAME, FILES, FILES[0].0, "tutorial/v1")
}

/// Import one bundled palace and its local media assets.
pub(crate) fn install_files(
    profile: &mut Profile,
    name: &str,
    files: &[(&str, &[u8])],
    source: &str,
    namespace: &str,
) -> Result<(PalaceTree, Vec<String>)> {
    let dir = tempfile::tempdir().context("Temporären Ordner anlegen")?;
    for (path, bytes) in files {
        let target = dir.path().join(path);
        fs::create_dir_all(target.parent().expect("files live in folders"))?;
        fs::write(&target, bytes)?;
    }
    let source_file = dir.path().join(source);
    let source = locora_source::parse(&fs::read_to_string(&source_file)?)?;
    let plan = locora_source::check(&source, source_file.parent().unwrap_or(Path::new(".")))?;
    let mut notes = Vec::new();
    for query in [name, &locora_source::bundled_palace_id(namespace)] {
        if let Some(old) = profile.store.find_palace(query)?
            && profile.store.delete_palace(&old.id)?
        {
            notes.extend(profile.publish(&PalaceDeleted { palace_id: old.id })?);
        }
    }
    let tree = locora_source::build_bundled(&profile.store, &plan, namespace)?;
    notes.extend(profile.publish(&PalaceCreated {
        palace_id: tree.palace.id.clone(),
        name: tree.palace.name.clone(),
    })?);
    Ok((tree, notes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(tree: &PalaceTree) -> Vec<String> {
        std::iter::once(tree.palace.id.clone())
            .chain(tree.rooms.iter().flat_map(|room| {
                std::iter::once(room.room.id.clone()).chain(room.loci.iter().map(|l| l.id.clone()))
            }))
            .collect()
    }

    #[test]
    fn bundled_palaces_have_stable_ids_across_devices_and_rebuilds() {
        let first_dir = tempfile::tempdir().unwrap();
        let second_dir = tempfile::tempdir().unwrap();
        let mut first = Profile::open(Some(first_dir.path())).unwrap();
        let mut second = Profile::open(Some(second_dir.path())).unwrap();
        let original = install(&mut first).unwrap().0;
        let other = install(&mut second).unwrap().0;
        assert_eq!(ids(&original), ids(&other));
        assert_eq!(ids(&original), ids(&install(&mut first).unwrap().0));
        let guide = crate::formula_guide::install(&mut first).unwrap().0;
        assert_ne!(original.palace.id, guide.palace.id);
    }
}
