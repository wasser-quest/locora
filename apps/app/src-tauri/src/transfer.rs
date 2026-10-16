//! Import and export `.locora` files (with the review schedule) through
//! native file dialogs; the logic is `locora_runtime::transfer`.

use std::path::{Path, PathBuf};

use anyhow::Result;
use locora_app_api::{ImportResult, ImportedPalace};
use locora_runtime::Profile;
use locora_runtime::transfer::{self, file_name};

/// Import a file: always new copies, the review schedule when it is there.
pub fn import_path(profile: &mut Profile, path: &Path) -> Result<ImportResult> {
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
