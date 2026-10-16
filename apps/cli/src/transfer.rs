//! `locora export` / `locora import`: palaces as portable .locora files.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use locora_api::Service;
use locora_api::core::PalaceCreated;
use locora_api::review::{CardState, Restore, RestoreRequest, Schedule, ScheduleRequest};
use locora_format::{EXTENSION, Package, PluginEntry, PluginFile};
use serde::{Deserialize, Serialize};

use crate::app::App;

/// `plugins/review/progress.json`: the review schedule of the exported loci.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Progress {
    version: u32,
    cards: Vec<ProgressCard>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProgressCard {
    palace: String,
    locus: String,
    stability: f64,
    difficulty: f64,
    /// Seconds since the Unix epoch.
    last_review: u64,
    due: u64,
    reps: u32,
    lapses: u32,
}

const PROGRESS_FILE: &str = "progress.json";
const PROGRESS_VERSION: u32 = 1;

fn review_loaded(app: &App) -> bool {
    app.host.provides(Schedule::NAME) && app.host.provides(Restore::NAME)
}

/// The review schedule of the given palaces as a plugin file, or `None` without cards.
fn progress_file(app: &mut App, palace_ids: &[String]) -> Result<Option<PluginFile>> {
    if !review_loaded(app) {
        bail!("--with-progress braucht das Plugin `review` (Wiederholung)");
    }
    let mut cards = Vec::new();
    for id in palace_ids {
        let scheduled = app.host.call::<Schedule>(&ScheduleRequest {
            palace_id: Some(id.clone()),
        })?;
        cards.extend(scheduled.into_iter().map(|s| ProgressCard {
            palace: s.palace_id,
            locus: s.locus_id,
            stability: s.card.stability,
            difficulty: s.card.difficulty,
            last_review: s.card.last_review,
            due: s.card.due,
            reps: s.card.reps,
            lapses: s.card.lapses,
        }));
    }
    app.report();
    if cards.is_empty() {
        return Ok(None);
    }
    let version = app
        .host
        .plugins()
        .find(|m| m.id == "review")
        .map_or_else(|| "0".into(), |m| m.version.clone());
    let progress = Progress {
        version: PROGRESS_VERSION,
        cards,
    };
    Ok(Some(PluginFile {
        plugin: PluginEntry {
            id: "review".into(),
            version,
            required: false,
        },
        path: PROGRESS_FILE.into(),
        bytes: serde_json::to_vec_pretty(&progress)?,
    }))
}

/// Read and check the progress file against the palaces in the package.
fn read_progress<R: std::io::Read + std::io::Seek>(
    package: &mut Package<R>,
) -> Result<Option<Progress>> {
    let Some(bytes) = package.plugin_file("review", PROGRESS_FILE)? else {
        return Ok(None);
    };
    let progress: Progress =
        serde_json::from_slice(&bytes).context("plugins/review/progress.json ist ungültig")?;
    if progress.version != PROGRESS_VERSION {
        bail!(
            "plugins/review/progress.json: Version {} wird nicht unterstützt",
            progress.version
        );
    }
    let known: BTreeSet<(&str, &str)> = package
        .palaces
        .iter()
        .flat_map(|p| {
            p.rooms
                .iter()
                .flat_map(move |r| r.loci.iter().map(move |l| (p.id.as_str(), l.id.as_str())))
        })
        .collect();
    let mut seen = BTreeSet::new();
    for card in &progress.cards {
        let state = CardState {
            stability: card.stability,
            difficulty: card.difficulty,
            last_review: card.last_review,
            due: card.due,
            reps: card.reps,
            lapses: card.lapses,
        };
        if !state.is_valid() {
            bail!(
                "plugins/review/progress.json: ungültige Werte für Locus {}",
                card.locus
            );
        }
        if !known.contains(&(card.palace.as_str(), card.locus.as_str())) {
            bail!(
                "plugins/review/progress.json: Locus {} gibt es in der Datei nicht",
                card.locus
            );
        }
        if !seen.insert((card.palace.as_str(), card.locus.as_str())) {
            bail!(
                "plugins/review/progress.json: Locus {} ist doppelt",
                card.locus
            );
        }
    }
    Ok(Some(progress))
}

fn file_name(name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let safe = safe.trim_matches('-');
    format!(
        "{}.{EXTENSION}",
        if safe.is_empty() { "palast" } else { safe }
    )
}

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
    let progress = if with_progress {
        let mut ids = Vec::new();
        for query in &queries {
            let id = app.store.palace(query)?.palace.id;
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        progress_file(app, &ids)?
    } else {
        None
    };
    let cards = progress.as_ref().map_or(0, |f| {
        serde_json::from_slice::<Progress>(&f.bytes).map_or(0, |p| p.cards.len())
    });
    let tmp = output.with_extension("locora.tmp");
    let result = (|| {
        let file = File::create(&tmp).with_context(|| format!("{} anlegen", tmp.display()))?;
        let summary = locora_format::export_with(&app.store, &queries, progress.as_slice(), file)?;
        std::fs::rename(&tmp, &output)?;
        anyhow::Ok(summary)
    })();
    let summary = match result {
        Ok(summary) => summary,
        Err(err) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(err);
        }
    };
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
        println!("  mit Wiederholungsstand für {cards} Loci");
    }
    Ok(())
}

pub fn import(app: &mut App, path: &Path, dry_run: bool, without_progress: bool) -> Result<()> {
    let file = File::open(path).with_context(|| format!("{} öffnen", path.display()))?;
    let mut package = Package::open(BufReader::new(file))?;
    // Checked before anything is imported, so a broken file changes nothing.
    let progress = if without_progress {
        None
    } else {
        read_progress(&mut package)?
    };
    println!("{}: {} Loci", path.display(), package.loci());
    for p in &package.palaces {
        let loci: usize = p.rooms.iter().map(|r| r.loci.len()).sum();
        println!("  • {} ({loci} Loci)", p.name);
    }
    for r in package.manifest.plugins.iter().filter(|r| r.required) {
        if !app.host.plugins().any(|m| m.id == r.id) {
            bail!("Diese Datei braucht das Plugin `{}` {}", r.id, r.version);
        }
    }
    if let Some(progress) = &progress {
        println!("  mit Wiederholungsstand für {} Loci", progress.cards.len());
        if !review_loaded(app) {
            println!("  ⚠ Plugin `review` fehlt: Der Wiederholungsstand wird nicht übernommen.");
        }
    }
    if dry_run {
        println!("(nur geprüft, nichts importiert)");
        return Ok(());
    }
    let imported = package.import(&app.store)?;
    for p in &imported {
        app.publish(&PalaceCreated {
            palace_id: p.id.clone(),
            name: p.name.clone(),
        })?;
        if p.name != p.original_name {
            println!("✓ „{}“ importiert als „{}“", p.original_name, p.name);
        } else {
            println!("✓ „{}“ importiert", p.name);
        }
    }
    if let Some(progress) = progress.filter(|_| review_loaded(app)) {
        let mut restored = 0;
        for p in &imported {
            let ids: BTreeMap<&str, &str> = p
                .locus_ids
                .iter()
                .map(|(old, new)| (old.as_str(), new.as_str()))
                .collect();
            let cards: Vec<(String, CardState)> = progress
                .cards
                .iter()
                .filter(|c| c.palace == p.original_id)
                .map(|c| {
                    (
                        ids[c.locus.as_str()].to_owned(),
                        CardState {
                            stability: c.stability,
                            difficulty: c.difficulty,
                            last_review: c.last_review,
                            due: c.due,
                            reps: c.reps,
                            lapses: c.lapses,
                        },
                    )
                })
                .collect();
            if !cards.is_empty() {
                restored += app.host.call::<Restore>(&RestoreRequest {
                    palace_id: p.id.clone(),
                    cards,
                })?;
            }
        }
        app.report();
        println!("✓ Wiederholungsstand für {restored} Loci übernommen");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn file_names_are_safe() {
        assert_eq!(super::file_name("Planeten"), "Planeten.locora");
        assert_eq!(
            super::file_name("Vokabeln: Küche/Bad"),
            "Vokabeln--Küche-Bad.locora"
        );
        assert_eq!(super::file_name("///"), "palast.locora");
    }
}
