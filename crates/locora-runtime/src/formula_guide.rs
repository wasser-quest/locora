//! The built-in formula guide, a separate memory palace with animated loci.

use anyhow::Result;
use locora_store::PalaceTree;

use crate::{Profile, tutorial};

pub const NAME: &str = "Formeln verstehen";

/// Namespaces of the formula guide, one per language (German keeps the original).
pub const NAMESPACES: [&str; 5] = [
    "formula-guide/v1",
    "formula-guide/v1/en",
    "formula-guide/v1/fr",
    "formula-guide/v1/ja",
    "formula-guide/v1/zh",
];
const SOURCE: &str = "tutorial/formel-guide/guide.locora.toml";

const FILES: &[(&str, &[u8])] = &[
    (
        SOURCE,
        include_bytes!("../../../assets/tutorial/formel-guide/guide.locora.toml"),
    ),
    (
        "tutorial/bibliothek.sply",
        include_bytes!("../../../assets/tutorial/bibliothek.sply"),
    ),
    (
        "tutorial/halle.sply",
        include_bytes!("../../../assets/tutorial/halle.sply"),
    ),
    (
        "tutorial/formel-guide/eingabe.sply",
        include_bytes!("../../../assets/tutorial/formel-guide/eingabe.sply"),
    ),
    (
        "tutorial/formel-guide/rechnen.sply",
        include_bytes!("../../../assets/tutorial/formel-guide/rechnen.sply"),
    ),
    (
        "tutorial/formel-guide/tabelle.sply",
        include_bytes!("../../../assets/tutorial/formel-guide/tabelle.sply"),
    ),
    (
        "tutorial/formel-guide/bindung.sply",
        include_bytes!("../../../assets/tutorial/formel-guide/bindung.sply"),
    ),
    (
        "tutorial/formel-guide/aktiv.sply",
        include_bytes!("../../../assets/tutorial/formel-guide/aktiv.sply"),
    ),
    (
        "tutorial/formel-guide/quelle.sply",
        include_bytes!("../../../assets/tutorial/formel-guide/quelle.sply"),
    ),
];

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

/// The formula guide in a language, if it exists.
pub fn find_in(profile: &Profile, code: &str) -> Result<Option<locora_store::Palace>> {
    Ok(profile
        .store
        .find_palace(&locora_source::bundled_palace_id(namespace_for(code)))?)
}

fn namespace_for(code: &str) -> &'static str {
    NAMESPACES[match tutorial::language(code) {
        "en" => 1,
        "fr" => 2,
        "ja" => 3,
        "zh" => 4,
        _ => 0,
    }]
}

/// The name of the formula guide in a language.
pub fn name_for(code: &str) -> &'static str {
    match tutorial::language(code) {
        "en" => "Understand the formulas",
        "fr" => "Comprendre les formules",
        "ja" => "数式を理解する",
        "zh" => "理解公式",
        _ => NAME,
    }
}

/// Build the German guide palace; the prior review plan is removed.
pub fn install(profile: &mut Profile) -> Result<(PalaceTree, Vec<String>)> {
    install_in(profile, "de")
}

/// Build or replace the guide palace in a language.
pub fn install_in(profile: &mut Profile, code: &str) -> Result<(PalaceTree, Vec<String>)> {
    let files = tutorial::localized(FILES, code);
    tutorial::install_files(profile, name_for(code), &files, SOURCE, namespace_for(code))
}
