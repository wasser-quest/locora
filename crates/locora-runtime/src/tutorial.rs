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

/// Namespaces of the tutorial palace: German keeps the original one, every other language has its own
/// palace (so progress in one language stays where it is).
pub const NAMESPACES: [&str; 5] = [
    "tutorial/v1",
    "tutorial/v1/en",
    "tutorial/v1/fr",
    "tutorial/v1/ja",
    "tutorial/v1/zh",
];

/// The language a palace is built in. Codes without a translation get English.
pub fn language(code: &str) -> &'static str {
    match code {
        "de" => "de",
        "fr" => "fr",
        "ja" => "ja",
        "zh" => "zh",
        _ => "en",
    }
}

/// The name of the tutorial palace in a language.
pub fn name_for(code: &str) -> &'static str {
    match language(code) {
        "en" => "Learn Locora",
        "fr" => "Apprendre Locora",
        "ja" => "Locora を学ぶ",
        "zh" => "学习 Locora",
        _ => NAME,
    }
}

/// The namespace of the tutorial palace in a language.
pub fn namespace_for(code: &str) -> &'static str {
    NAMESPACES[match language(code) {
        "en" => 1,
        "fr" => 2,
        "ja" => 3,
        "zh" => 4,
        _ => 0,
    }]
}

/// The files of a bundled palace in a language: the translated file where there is one.
pub(crate) fn localized(
    files: &[(&'static str, &'static [u8])],
    code: &str,
) -> Vec<(&'static str, &'static [u8])> {
    let language = language(code);
    files
        .iter()
        .map(|(path, bytes)| {
            let name = path.rsplit('/').next().unwrap_or(path);
            let bytes = match language {
                "de" => *bytes,
                lang => crate::palace_i18n::file(lang, name).unwrap_or(bytes),
            };
            (*path, bytes)
        })
        .collect()
}

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

/// The tutorial palace in any language, if it exists.
pub fn find(profile: &Profile) -> Result<Option<locora_store::Palace>> {
    for namespace in NAMESPACES {
        if let Some(palace) = profile
            .store
            .find_palace(&locora_source::bundled_palace_id(namespace))?
        {
            return Ok(Some(palace));
        }
    }
    Ok(profile.store.find_palace(NAME)?)
}

/// The tutorial palace in a language, if it exists.
pub fn find_in(profile: &Profile, code: &str) -> Result<Option<locora_store::Palace>> {
    Ok(profile
        .store
        .find_palace(&locora_source::bundled_palace_id(namespace_for(code)))?)
}

/// Build the German tutorial palace, replacing an existing one. Returns the new
/// palace and messages from plugins.
pub fn install(profile: &mut Profile) -> Result<(PalaceTree, Vec<String>)> {
    install_in(profile, "de")
}

/// Build the tutorial palace in a language, replacing the one of that language.
pub fn install_in(profile: &mut Profile, code: &str) -> Result<(PalaceTree, Vec<String>)> {
    let files = localized(FILES, code);
    install_files(profile, name_for(code), &files, FILES[0].0, namespace_for(code))
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

    #[test]
    fn every_language_builds_its_own_palace() {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = Profile::open(Some(dir.path())).unwrap();
        let mut ids = Vec::new();
        for code in ["de", "en", "fr", "ja", "zh"] {
            let (tree, _) = install_in(&mut profile, code).unwrap();
            assert_eq!(tree.palace.name, name_for(code));
            assert_eq!(
                tree.rooms.iter().map(|room| room.loci.len()).sum::<usize>(),
                9,
                "every place is kept in {code}"
            );
            assert_eq!(
                find_in(&profile, code).unwrap().unwrap().id,
                tree.palace.id
            );
            ids.push(tree.palace.id);
        }
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 5, "one palace per language");
        assert!(find(&profile).unwrap().is_some());
        assert_eq!(language("pt"), "en", "unknown languages get English");
        for code in ["de", "en", "fr", "ja", "zh"] {
            let (tree, _) = crate::formula_guide::install_in(&mut profile, code).unwrap();
            assert_eq!(tree.palace.name, crate::formula_guide::name_for(code));
        }
    }
}
