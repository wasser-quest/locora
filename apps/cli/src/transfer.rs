//! `locora export` / `locora import`: palaces as portable .locora files
//! (logic in `locora_runtime::transfer`).

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use locora_runtime::transfer::{self, file_name};

use crate::app::App;

pub fn export(
    app: &mut App,
    palaces: Vec<String>,
    all: bool,
    output: Option<PathBuf>,
    with_progress: bool,
) -> Result<()> {
    let queries: Vec<String> = if all {
        app.store.palaces()?.into_iter().map(|p| p.id).collect()
    } else {
        palaces
    };
    if queries.is_empty() {
        bail!(
            "Welche Paläste? z. B. `locora export Planeten` oder `locora export --all -o alles.locora`"
        );
    }
    let output = match output {
        Some(path) => path,
        None if queries.len() == 1 => {
            PathBuf::from(file_name(&app.store.palace(&queries[0])?.palace.name))
        }
        None => bail!("Mehrere Paläste brauchen eine Zieldatei: -o paläste.locora"),
    };
    if output.exists() {
        bail!(
            "{} gibt es schon; wähle einen anderen Namen mit -o",
            output.display()
        );
    }
    if with_progress && !app.review_transferable() {
        bail!("--with-progress braucht das Plugin `review` (Wiederholung)");
    }
    let done = app.export(&queries, &output, with_progress, false)?;
    app.report();
    let summary = done.summary;
    let media = match summary.media {
        0 => String::new(),
        1 => ", 1 Medium".into(),
        n => format!(", {n} Medien"),
    };
    println!(
        "✓ {}: {} {}, {} Loci{media}",
        output.display(),
        summary.palaces,
        if summary.palaces == 1 {
            "Palast"
        } else {
            "Paläste"
        },
        summary.loci
    );
    if with_progress {
        println!("  mit Wiederholungsstand für {} Loci", done.cards);
    }
    Ok(())
}

pub fn import(app: &mut App, path: &Path, dry_run: bool, without_progress: bool) -> Result<()> {
    let opened = transfer::open(path, !without_progress)?;
    let package = &opened.package;
    println!("{}: {} Loci", path.display(), package.loci());
    for p in &package.palaces {
        let loci: usize = p.rooms.iter().map(|r| r.loci.len()).sum();
        println!("  • {} ({loci} Loci)", p.name);
    }
    app.check_required_plugins(package)?;
    if let Some(progress) = &opened.progress {
        println!("  mit Wiederholungsstand für {} Loci", progress.cards.len());
        if !app.review_transferable() {
            println!("  ⚠ Plugin `review` fehlt: Der Wiederholungsstand wird nicht übernommen.");
        }
    }
    if dry_run {
        println!("(nur geprüft, nichts importiert)");
        return Ok(());
    }
    let done = app.import(opened)?;
    crate::app::print(done.notes);
    for p in &done.imported {
        if p.name != p.original_name {
            println!("✓ „{}“ importiert als „{}“", p.original_name, p.name);
        } else {
            println!("✓ „{}“ importiert", p.name);
        }
    }
    if let Some(restored) = done.restored {
        app.report();
        println!("✓ Wiederholungsstand für {restored} Loci übernommen");
    }
    Ok(())
}
