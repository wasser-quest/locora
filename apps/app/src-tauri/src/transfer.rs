//! Import `.locora` archives or TOML sources and export `.locora` files through
//! native file dialogs; the logic is `locora_runtime::transfer`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use locora_app_api::{ImportResult, ImportedPalace};
use locora_runtime::Profile;
use locora_runtime::transfer::{self, file_name};

/// Import a file: always new copies, the review schedule when it is there.
pub fn import_path(profile: &mut Profile, path: &Path) -> Result<ImportResult> {
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("toml"))
    {
        return import_source(profile, path);
    }
    let opened = transfer::open(path, true)?;
    let done = profile.import(opened)?;
    Ok(ImportResult {
        palaces: done
            .imported
            .into_iter()
            .map(|p| ImportedPalace {
                id: p.id,
                name: p.name,
                original_name: p.original_name,
            })
            .collect(),
        restored: done.restored,
        notes: done.notes,
    })
}

/// Build a checked TOML source as a new copy, resolving media beside the file.
fn import_source(profile: &mut Profile, path: &Path) -> Result<ImportResult> {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(path)
        .with_context(|| format!("{} lesen", path.display()))?
        .take(locora_format::limits::JSON_BYTES + 1)
        .read_to_string(&mut text)?;
    ensure!(
        text.len() as u64 <= locora_format::limits::JSON_BYTES,
        "TOML-Datei ist zu groß"
    );
    let source = locora_source::parse(&text)?;
    let base = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut plan = locora_source::check(&source, base)?;
    ensure!(
        plan.rooms.len() <= locora_format::limits::ROOMS_PER_PALACE,
        "Zu viele Räume"
    );
    ensure!(plan.loci() <= locora_format::limits::LOCI, "Zu viele Loci");
    ensure!(
        plan.tables.len() <= locora_format::limits::TABLE_ROWS,
        "Zu viele Tabellenzeilen"
    );
    let media = plan
        .marker
        .iter()
        .chain(plan.rooms.iter().filter_map(|r| r.media.as_ref()))
        .chain(
            plan.rooms
                .iter()
                .flat_map(|r| r.loci.iter().filter_map(|(_, _, image)| image.as_ref())),
        );
    for (path, _) in media {
        let metadata = std::fs::metadata(path)?;
        ensure!(
            metadata.is_file() && metadata.len() <= 64 << 20,
            "Medien müssen reguläre Dateien mit höchstens 64 MiB sein"
        );
    }
    let original_name = plan.name.clone();
    let mut n = 2;
    while profile.store.find_palace(&plan.name)?.is_some() {
        plan.name = format!("{original_name} ({n})");
        n += 1;
    }
    let tree = locora_source::build(&profile.store, &plan)?;
    // Once stored, notification failures are notes, not failed imports that
    // would invite the user to retry and create another copy.
    let notes = match profile.publish(&locora_api::core::PalaceCreated {
        palace_id: tree.palace.id.clone(),
        name: tree.palace.name.clone(),
    }) {
        Ok(notes) => notes,
        Err(error) => vec![format!("Plugins nicht benachrichtigt: {error:#}")],
    };
    Ok(ImportResult {
        palaces: vec![ImportedPalace {
            id: tree.palace.id,
            name: tree.palace.name,
            original_name,
        }],
        restored: None,
        notes,
    })
}

/// The file name the save dialog suggests.
pub fn suggested_name(profile: &Profile, palace_id: &str) -> Result<String> {
    Ok(file_name(&profile.store.palace(palace_id)?.palace.name))
}

const EXTENSION: &str = "locora";

/// `.locora` is appended when the chosen name has another extension.
pub fn with_extension(path: PathBuf) -> PathBuf {
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
    {
        path
    } else {
        let mut name = path.into_os_string();
        name.push(".");
        name.push(EXTENSION);
        PathBuf::from(name)
    }
}

/// Export one palace with its review schedule; the dialog already asked
/// before replacing a file. Returns a message for people.
pub fn export_path(profile: &mut Profile, palace_id: &str, path: &Path) -> Result<String> {
    let with_progress = profile.review_transferable();
    let done = profile.export(&[palace_id.to_owned()], path, with_progress, true)?;
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let mut text = format!("Gespeichert: {name} ({} Loci", done.summary.loci);
    if done.cards > 0 {
        text.push_str(&format!(", Wiederholungsstand für {}", done.cards));
    }
    text.push(')');
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_import_resolves_media_and_makes_copies() {
        let dir = tempfile::tempdir().unwrap();
        let profile_path = dir.path().join("profile");
        let source_dir = dir.path().join("source");
        std::fs::create_dir(&source_dir).unwrap();
        std::fs::write(source_dir.join("room.png"), b"picture").unwrap();
        let file = source_dir.join("locora.TOML");
        std::fs::write(&file, "name = \"Planeten\"\n[[room]]\nname = \"Zimmer\"\nimage = \"room.png\"\nitems = [\"Merkur\", \"Venus\"]\n").unwrap();
        let mut profile = Profile::open(Some(&profile_path)).unwrap();
        let first = import_path(&mut profile, &file).unwrap();
        assert_eq!(first.palaces[0].name, "Planeten");
        assert_eq!(first.restored, None);
        let second = import_path(&mut profile, &file).unwrap();
        assert_eq!(second.palaces[0].name, "Planeten (2)");
        assert_eq!(second.palaces[0].original_name, "Planeten");
        assert_ne!(first.palaces[0].id, second.palaces[0].id);
        drop(profile);
        let profile = Profile::open(Some(&profile_path)).unwrap();
        let tree = profile.store.palace(&first.palaces[0].id).unwrap();
        assert_eq!(tree.route().count(), 2);
        assert!(tree.rooms[0].room.media.is_some());
    }

    #[test]
    fn toml_import_rejects_too_many_rooms_before_building() {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = Profile::open(Some(&dir.path().join("profile"))).unwrap();
        let file = dir.path().join("locora.toml");
        let mut text = String::from("name = \"P\"\n");
        for i in 0..=locora_format::limits::ROOMS_PER_PALACE {
            text.push_str(&format!("[[room]]\nname = \"R{i}\"\n"));
        }
        std::fs::write(&file, text).unwrap();
        assert!(
            import_path(&mut profile, &file)
                .unwrap_err()
                .to_string()
                .contains("Zu viele Räume")
        );
        assert!(profile.store.palaces().unwrap().is_empty());
    }

    #[test]
    fn invalid_toml_import_does_not_create_a_palace() {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = Profile::open(Some(&dir.path().join("profile"))).unwrap();
        let file = dir.path().join("locora.toml");
        for text in [
            "not toml",
            "name = \"P\"\n[[room]]\nname = \"R\"\nimage = \"missing.png\"\n",
        ] {
            std::fs::write(&file, text).unwrap();
            assert!(import_path(&mut profile, &file).is_err());
            assert!(profile.store.palaces().unwrap().is_empty());
        }
    }

    #[test]
    fn export_then_import_makes_a_copy() {
        let dir = tempfile::tempdir().unwrap();
        let mut profile = Profile::open(Some(&dir.path().join("profil"))).unwrap();
        let (tree, _) = locora_runtime::tutorial::install(&mut profile).unwrap();
        let file = with_extension(dir.path().join("tutorial"));
        assert!(file.ends_with("tutorial.locora"));
        let message = export_path(&mut profile, &tree.palace.id, &file).unwrap();
        assert!(
            message.starts_with("Gespeichert: tutorial.locora (9 Loci"),
            "{message}"
        );
        assert!(
            export_path(&mut profile, &tree.palace.id, &file).is_ok(),
            "replacing is allowed"
        );

        let result = import_path(&mut profile, &file).unwrap();
        assert_eq!(result.palaces.len(), 1);
        assert_ne!(result.palaces[0].id, tree.palace.id);
        assert_ne!(
            result.palaces[0].name, result.palaces[0].original_name,
            "name was taken"
        );
        assert_eq!(profile.store.palaces().unwrap().len(), 2);

        std::fs::write(dir.path().join("kaputt.locora"), b"kein zip").unwrap();
        assert!(import_path(&mut profile, &dir.path().join("kaputt.locora")).is_err());
        assert_eq!(
            profile.store.palaces().unwrap().len(),
            2,
            "nothing imported"
        );
    }
}
