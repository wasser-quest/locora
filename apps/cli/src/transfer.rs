//! `locora export` / `locora import`: palaces as portable .locora files.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use locora_api::core::PalaceCreated;
use locora_format::{EXTENSION, Package};

use crate::app::App;

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
    let tmp = output.with_extension("locora.tmp");
    let result = (|| {
        let file = File::create(&tmp).with_context(|| format!("{} anlegen", tmp.display()))?;
        let summary = locora_format::export(&app.store, &queries, file)?;
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
    Ok(())
}

pub fn import(app: &mut App, path: &Path, dry_run: bool) -> Result<()> {
    let file = File::open(path).with_context(|| format!("{} öffnen", path.display()))?;
    let package = Package::open(BufReader::new(file))?;
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
