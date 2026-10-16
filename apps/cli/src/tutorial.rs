//! `locora tutorial`: the built-in tutorial palace "Locora lernen".
//!
//! The tutorial teaches Locora with the method of loci itself: three rooms,
//! nine places, each with an animated Splinely picture. Its source and all
//! pictures are compiled into the binary and built like any locora.toml.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use locora_api::core::{PalaceCreated, PalaceDeleted};

use crate::app::App;

pub const NAME: &str = "Locora lernen";

/// (path relative to `assets/`, bytes)
const FILES: &[(&str, &[u8])] = &[
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

pub fn run(app: &mut App, reset: bool) -> Result<()> {
    let existing = app.store.find_palace(NAME)?;
    if let Some(old) = &existing
        && !reset
    {
        println!("Das Tutorial „{}“ ist schon da.", old.name);
        println!("  Üben: locora practice \"{NAME}\"   ·   neu anlegen: locora tutorial --reset");
        return Ok(());
    }
    let dir = tempfile::tempdir().context("Temporären Ordner anlegen")?;
    for (path, bytes) in FILES {
        let target = dir.path().join(path);
        fs::create_dir_all(target.parent().expect("files live in folders"))?;
        fs::write(&target, bytes)?;
    }
    let source_file = dir.path().join(FILES[0].0);
    let source = locora_source::parse(&fs::read_to_string(&source_file)?)?;
    let plan = locora_source::check(&source, source_file.parent().unwrap_or(Path::new(".")))?;
    if let Some(old) = existing {
        app.store.delete_palace(&old.id)?;
        app.publish(&PalaceDeleted { palace_id: old.id })?;
    }
    let tree = locora_source::build(&app.store, &plan)?;
    app.publish(&PalaceCreated {
        palace_id: tree.palace.id.clone(),
        name: tree.palace.name.clone(),
    })?;
    println!(
        "✓ Tutorial „{NAME}“ angelegt: {} Räume, {} Orte.",
        plan.rooms.len(),
        plan.loci()
    );
    println!();
    println!("  Das Tutorial ist selbst ein Gedächtnispalast. Geh die Route ab:");
    println!("  an jedem Ort steht eine Frage, du erinnerst dich an die Antwort.");
    println!("  In der App zeigt jeder Ort eine kleine Animation.");
    println!();
    println!("  Los geht's: locora practice \"{NAME}\"");
    Ok(())
}
