//! The built-in formula guide, a separate memory palace with animated loci.

use anyhow::Result;
use locora_store::PalaceTree;

use crate::{Profile, tutorial};

pub const NAME: &str = "Formeln verstehen";
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
    Ok(profile
        .store
        .find_palace(&locora_source::bundled_palace_id("formula-guide/v1"))?
        .or(profile.store.find_palace(NAME)?))
}

/// Build or replace the guide palace; the prior review plan is removed.
pub fn install(profile: &mut Profile) -> Result<(PalaceTree, Vec<String>)> {
    tutorial::install_files(profile, NAME, FILES, SOURCE, "formula-guide/v1")
}
