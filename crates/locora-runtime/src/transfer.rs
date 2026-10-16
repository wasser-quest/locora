//! Palaces as portable `.locora` files, with the review schedule as the
//! optional plugin file `plugins/review/progress.json`. Shared by
//! `locora export`/`import` and the app.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

use anyhow::{Context, Result, bail};
use locora_api::Service;
use locora_api::core::PalaceCreated;
use locora_api::review::{CardState, Restore, RestoreRequest, Schedule, ScheduleRequest};
use locora_format::{EXTENSION, ExportSummary, Imported, Package, PluginEntry, PluginFile};
use serde::{Deserialize, Serialize};

use crate::Profile;

/// `plugins/review/progress.json`: the review schedule of the exported loci.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Progress {
    version: u32,
    pub cards: Vec<ProgressCard>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProgressCard {
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

impl ProgressCard {
    fn state(&self) -> CardState {
        CardState {
            stability: self.stability,
            difficulty: self.difficulty,
            last_review: self.last_review,
            due: self.due,
            reps: self.reps,
            lapses: self.lapses,
        }
    }
}

const PROGRESS_FILE: &str = "progress.json";
const PROGRESS_VERSION: u32 = 1;

/// A `.locora` file opened and checked, ready to import.
pub struct Opened {
    pub package: Package<BufReader<File>>,
    /// The review schedule in the file (`None` when absent or not wanted).
    pub progress: Option<Progress>,
}

pub struct ImportDone {
    pub imported: Vec<Imported>,
    /// Loci whose review schedule was restored; `None` when the file had none
    /// or the review plugin is missing.
    pub restored: Option<u32>,
    pub notes: Vec<String>,
}

pub struct ExportDone {
    pub summary: ExportSummary,
    /// Loci with a review schedule in the file (with progress only).
    pub cards: usize,
}

/// A safe file name for a palace: `Vokabeln: Küche` → `Vokabeln--Küche.locora`.
pub fn file_name(name: &str) -> String {
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

/// `<dir>/.<name>.<pid>-<nanos>.tmp` next to `output`.
fn temporary_sibling(output: &Path) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let name = output
        .file_name()
        .map_or_else(|| "export".into(), |n| n.to_string_lossy().into_owned());
    output.with_file_name(format!(".{name}.{}-{nanos}.tmp", std::process::id()))
}

/// Open a `.locora` file and check everything before anything is imported,
/// so a broken file changes nothing.
pub fn open(path: &Path, with_progress: bool) -> Result<Opened> {
    let file = File::open(path).with_context(|| format!("{} öffnen", path.display()))?;
    let mut package = Package::open(BufReader::new(file))?;
    let progress = if with_progress {
        read_progress(&mut package)?
    } else {
        None
    };
    Ok(Opened { package, progress })
}

/// Read and check the progress file against the palaces in the package.
fn read_progress<R: Read + Seek>(package: &mut Package<R>) -> Result<Option<Progress>> {
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
        if !card.state().is_valid() {
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

impl Profile {
    /// Can the review schedule be exported and restored?
    pub fn review_transferable(&self) -> bool {
        self.host.provides(Schedule::NAME) && self.host.provides(Restore::NAME)
    }

    /// Fail when the file needs a plugin that is not loaded.
    pub fn check_required_plugins<R: Read + Seek>(&self, package: &Package<R>) -> Result<()> {
        for r in package.manifest.plugins.iter().filter(|r| r.required) {
            if !self.host.plugins().any(|m| m.id == r.id) {
                bail!("Diese Datei braucht das Plugin `{}` {}", r.id, r.version);
            }
        }
        Ok(())
    }

    /// Import an opened file: always new copies, then the review schedule.
    /// Once the palaces are stored, later problems (plugin events, schedule)
    /// become notes: failing then would invite a retry that copies again.
    pub fn import(&mut self, opened: Opened) -> Result<ImportDone> {
        self.check_required_plugins(&opened.package)?;
        let imported = opened.package.import(&self.store)?;
        let mut notes = Vec::new();
        for p in &imported {
            match self.publish(&PalaceCreated {
                palace_id: p.id.clone(),
                name: p.name.clone(),
            }) {
                Ok(more) => notes.extend(more),
                Err(e) => notes.push(format!("„{}“: Plugins nicht benachrichtigt: {e:#}", p.name)),
            }
        }
        let mut restored = None;
        if let Some(progress) = opened.progress.filter(|_| self.review_transferable()) {
            let mut count = 0;
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
                    .filter_map(|c| Some((ids.get(c.locus.as_str())?.to_string(), c.state())))
                    .collect();
                if !cards.is_empty() {
                    match self.host.call::<Restore>(&RestoreRequest {
                        palace_id: p.id.clone(),
                        cards,
                    }) {
                        Ok(n) => count += n,
                        Err(e) => notes.push(format!(
                            "„{}“: Wiederholungsstand nicht übernommen: {e:#}",
                            p.name
                        )),
                    }
                }
            }
            restored = Some(count);
        }
        Ok(ImportDone {
            imported,
            restored,
            notes,
        })
    }

    /// The review schedule of the given palaces as a plugin file, or `None` without cards.
    fn progress_file(&mut self, palace_ids: &[String]) -> Result<Option<PluginFile>> {
        if !self.review_transferable() {
            bail!("Der Wiederholungsstand braucht das Plugin `review` (Wiederholung)");
        }
        let mut cards = Vec::new();
        for id in palace_ids {
            let scheduled = self.host.call::<Schedule>(&ScheduleRequest {
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
        if cards.is_empty() {
            return Ok(None);
        }
        let version = self
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

    /// Write palaces (ids or names) to `output`, atomically via a temporary
    /// file. An existing file is only replaced with `overwrite`.
    pub fn export(
        &mut self,
        queries: &[String],
        output: &Path,
        with_progress: bool,
        overwrite: bool,
    ) -> Result<ExportDone> {
        if queries.is_empty() {
            bail!("Keine Paläste zum Exportieren");
        }
        if output.exists() && !overwrite {
            bail!("{} gibt es schon", output.display());
        }
        let progress = if with_progress {
            let mut ids = Vec::new();
            for query in queries {
                let id = self.store.palace(query)?.palace.id;
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            self.progress_file(&ids)?
        } else {
            None
        };
        let cards = progress.as_ref().map_or(0, |f| {
            serde_json::from_slice::<Progress>(&f.bytes).map_or(0, |p| p.cards.len())
        });
        // A temporary file of our own next to the target: two exports (CLI and
        // app) to the same name never truncate each other's work.
        let tmp = temporary_sibling(output);
        let result = (|| {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)
                .with_context(|| format!("{} anlegen", tmp.display()))?;
            let summary =
                locora_format::export_with(&self.store, queries, progress.as_slice(), file)?;
            std::fs::rename(&tmp, output)?;
            anyhow::Ok(summary)
        })();
        match result {
            Ok(summary) => Ok(ExportDone { summary, cards }),
            Err(err) => {
                let _ = std::fs::remove_file(&tmp);
                Err(err)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn temporary_files_are_unique_siblings() {
        let target = std::path::Path::new("/tmp/x/Planeten.locora");
        let a = super::temporary_sibling(target);
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = super::temporary_sibling(target);
        assert_ne!(a, b);
        assert_eq!(a.parent(), target.parent());
        assert!(
            a.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".Planeten.locora.")
        );
    }

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
