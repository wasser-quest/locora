//! `locora build` / `locora dump`: palaces as locora.toml source files.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use locora_api::core::{PalaceCreated, PalaceDeleted};

use crate::app::App;
use crate::palace::confirm;

pub fn build(
    data: Option<&Path>,
    file: &Path,
    check_only: bool,
    replace: bool,
    yes: bool,
) -> Result<()> {
    let text = fs::read_to_string(file).with_context(|| format!("{} lesen", file.display()))?;
    let source = locora_source::parse(&text).with_context(|| file.display().to_string())?;
    let base = file
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let plan = locora_source::check(&source, base).with_context(|| file.display().to_string())?;
    let summary = format!(
        "„{}“: {} {}, {} Loci, {} Tabellenzeilen",
        plan.name,
        plan.rooms.len(),
        if plan.rooms.len() == 1 {
            "Raum"
        } else {
            "Räume"
        },
        plan.loci(),
        plan.tables.len()
    );
    if check_only {
        // Checking never opens the profile.
        println!("✓ {summary} (nur geprüft)");
        return Ok(());
    }

    let mut app = App::open(data)?;
    let existing = app.store.find_palace(&plan.name)?;
    if let Some(old) = &existing {
        if !replace {
            bail!(
                "Es gibt schon einen Palast „{}“. Ersetzen mit --replace (der Wiederholungsplan des alten Palasts geht verloren)",
                old.name
            );
        }
        if !yes
            && !confirm(&format!(
                "Palast „{}“ ersetzen? Sein Wiederholungsplan geht verloren.",
                old.name
            ))?
        {
            println!("Abgebrochen.");
            return Ok(());
        }
    }
    // Build under a temporary name first, so a failure never loses the old palace.
    let mut staged = plan.clone();
    if existing.is_some() {
        let mut n = 1;
        staged.name = loop {
            let candidate = format!("{} (neu {n})", plan.name);
            if app.store.find_palace(&candidate)?.is_none() {
                break candidate;
            }
            n += 1;
        };
    }
    let tree = locora_source::build(&app.store, &staged)?;
    if let Some(old) = existing {
        app.store.delete_palace(&old.id)?;
        app.publish(&PalaceDeleted { palace_id: old.id })?;
        app.store.rename_palace(&tree.palace.id, &plan.name)?;
    }
    app.publish(&PalaceCreated {
        palace_id: tree.palace.id.clone(),
        name: plan.name.clone(),
    })?;
    println!("✓ {summary} gebaut");
    println!("  Üben: locora practice \"{}\"", plan.name);
    Ok(())
}

pub fn dump(app: &mut App, palace: &str, dir: &Path) -> Result<()> {
    let file = locora_source::dump(&app.store, palace, dir)?;
    println!("✓ {}", file.display());
    println!("  Wieder bauen: locora build {}", file.display());
    Ok(())
}
