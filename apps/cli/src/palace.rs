//! Palace commands: new, add, ls, show, rm, due.

use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use locora_api::core::{LocusDeleted, PalaceCreated, PalaceDeleted};
use locora_api::review::{CardState, Schedule, ScheduleRequest};
use locora_store::{Locus, LocusPatch, NewLocus, PalaceTree, StageKind};

use crate::app::{App, now};
use crate::places;

/// One locus to create: from a command-line item or a line of a file.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub label: Option<String>,
    pub memory: String,
    pub at: Option<(f64, f64)>,
    pub time: Option<f64>,
}

/// Parse `mm:ss`, `h:mm:ss` or plain seconds.
pub fn parse_time(text: &str) -> Option<f64> {
    let parts: Vec<&str> = text.trim().split(':').collect();
    if parts.len() > 3 || parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    let mut seconds = 0.0;
    for part in &parts {
        seconds = seconds * 60.0 + part.parse::<f64>().ok().filter(|v| *v >= 0.0)?;
    }
    Some(seconds)
}

fn parse_point(text: &str) -> Option<(f64, f64)> {
    let (x, y) = text.split_once(',')?;
    let (x, y) = (x.trim().parse::<f64>().ok()?, y.trim().parse::<f64>().ok()?);
    ((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y)).then_some((x, y))
}

/// `Merkur`, `1:23=These` (video), or a file line
/// `Herd | Merkur | 0.4,0.6` / `1:23 | These`.
pub fn parse_item(text: &str) -> Result<Item> {
    let text = text.trim();
    if text.contains('|') {
        let fields: Vec<&str> = text.split('|').map(str::trim).collect();
        let mut item = Item {
            label: None,
            memory: String::new(),
            at: None,
            time: None,
        };
        let mut rest = fields.as_slice();
        if let Some(time) = rest
            .first()
            .and_then(|f| f.contains(':').then(|| parse_time(f)).flatten())
        {
            item.time = Some(time);
            rest = &rest[1..];
        }
        if let Some(point) = rest.last().and_then(|f| parse_point(f)) {
            item.at = Some(point);
            rest = &rest[..rest.len() - 1];
        }
        match rest {
            [memory] => item.memory = memory.to_string(),
            [label, memory] => {
                item.label = Some(label.to_string()).filter(|l| !l.is_empty());
                item.memory = memory.to_string();
            }
            _ => bail!(
                "Zeile „{text}“: erwartet `Erinnerung`, `Ort | Erinnerung` oder `Ort | Erinnerung | x,y`"
            ),
        }
        if item.memory.is_empty() {
            bail!("Zeile „{text}“ hat keine Erinnerung");
        }
        return Ok(item);
    }
    if let Some((time, memory)) = text.split_once('=')
        && let Some(time) = parse_time(time)
    {
        return Ok(Item {
            label: None,
            memory: memory.trim().to_owned(),
            at: None,
            time: Some(time),
        });
    }
    Ok(Item {
        label: None,
        memory: text.to_owned(),
        at: None,
        time: None,
    })
}

fn read_items(file: &Path) -> Result<Vec<Item>> {
    let text = fs::read_to_string(file).with_context(|| format!("{} lesen", file.display()))?;
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(parse_item)
        .collect()
}

fn media_kind(path: &Path) -> Result<(StageKind, &'static str)> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    Ok(match ext.as_str() {
        "png" => (StageKind::Image, "image/png"),
        "jpg" | "jpeg" => (StageKind::Image, "image/jpeg"),
        "webp" => (StageKind::Image, "image/webp"),
        "gif" => (StageKind::Image, "image/gif"),
        "avif" => (StageKind::Image, "image/avif"),
        "mp4" | "m4v" => (StageKind::Video, "video/mp4"),
        "webm" => (StageKind::Video, "video/webm"),
        "mov" => (StageKind::Video, "video/quicktime"),
        _ => bail!(
            "{}: unbekanntes Format. Bilder: png, jpg, webp, gif, avif · Videos: mp4, webm, mov",
            path.display()
        ),
    })
}

pub struct NewArgs {
    pub name: String,
    pub media: Option<PathBuf>,
    pub from: Option<PathBuf>,
    pub items: Vec<String>,
}

pub fn new(app: &mut App, args: NewArgs) -> Result<()> {
    let mut items: Vec<Item> = args
        .items
        .iter()
        .map(|i| parse_item(i))
        .collect::<Result<_>>()?;
    if let Some(file) = &args.from {
        items.extend(read_items(file)?);
    }
    let (kind, media) = match &args.media {
        Some(path) => {
            let (kind, mime) = media_kind(path)?;
            let bytes = fs::read(path).with_context(|| format!("{} lesen", path.display()))?;
            (kind, Some(app.store.put_media(&bytes, mime)?.hash))
        }
        None => (StageKind::Blank, None),
    };
    if kind == StageKind::Video
        && let Some(item) = items.iter().find(|i| i.time.is_none())
    {
        bail!(
            "„{}“ braucht im Video-Palast eine Zeit, z. B. `1:23={}`",
            item.memory,
            item.memory
        );
    }
    let palace = app.store.create_palace(&args.name)?;
    let stage_name = args
        .media
        .as_deref()
        .and_then(|p| p.file_stem())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Raum 1".into());
    let stage = app
        .store
        .add_stage(&palace.id, &stage_name, kind, media.as_deref())?;
    let spots = places::route(items.len());
    for (i, item) in items.iter().enumerate() {
        let (x, y) = item.at.unwrap_or(spots[i]);
        app.store.add_locus(
            &stage.id,
            NewLocus {
                label: item
                    .label
                    .clone()
                    .unwrap_or_else(|| format!("Ort {}", i + 1)),
                memory: item.memory.clone(),
                x,
                y,
                at: item.time,
            },
        )?;
    }
    app.publish(&PalaceCreated {
        palace_id: palace.id.clone(),
        name: palace.name.clone(),
    })?;
    let what = match kind {
        StageKind::Blank => String::new(),
        StageKind::Image => " mit Bild".into(),
        StageKind::Video => " mit Video".into(),
    };
    let n = items.len();
    println!(
        "✓ Palast „{}“{what} angelegt: {n} {}",
        palace.name,
        if n == 1 { "Locus" } else { "Loci" }
    );
    if items.is_empty() {
        println!(
            "  Loci hinzufügen: locora add \"{}\" <Erinnerung>",
            palace.name
        );
    } else {
        println!("  Üben: locora practice \"{}\"", palace.name);
    }
    Ok(())
}

pub struct AddArgs {
    pub palace: String,
    pub memory: String,
    pub label: Option<String>,
    pub at: Option<String>,
    pub time: Option<String>,
}

pub fn add(app: &mut App, args: AddArgs) -> Result<()> {
    let tree = app.store.palace(&args.palace)?;
    let Some(stage) = tree.stages.last() else {
        bail!("Palast „{}“ hat noch keine Stage", tree.palace.name);
    };
    let count = stage.loci.len();
    let (x, y) = match &args.at {
        Some(text) => {
            parse_point(text).context("--at erwartet x,y zwischen 0 und 1, z. B. 0.4,0.6")?
        }
        None => *places::route(count + 1).last().expect("one point"),
    };
    let time = match &args.time {
        Some(t) => Some(parse_time(t).context("--time erwartet mm:ss oder Sekunden")?),
        None => None,
    };
    if stage.stage.kind == StageKind::Video && time.is_none() {
        bail!("Im Video-Palast braucht ein Locus eine Zeit: --time 1:23");
    }
    let locus = app.store.add_locus(
        &stage.stage.id,
        NewLocus {
            label: args
                .label
                .unwrap_or_else(|| format!("Ort {}", tree.route().count() + 1)),
            memory: args.memory,
            x,
            y,
            at: time,
        },
    )?;
    println!(
        "✓ Locus {} in „{}“: {} → {}",
        tree.route().count() + 1,
        tree.palace.name,
        locus.label,
        locus.memory
    );
    Ok(())
}

/// Cards of the review plugin by locus id (empty without the plugin).
pub fn schedule(app: &mut App, palace: Option<&str>) -> HashMap<String, CardState> {
    if !app.host.provides(<Schedule as locora_api::Service>::NAME) {
        return HashMap::new();
    }
    let request = ScheduleRequest {
        palace_id: palace.map(str::to_owned),
    };
    match app.host.call::<Schedule>(&request) {
        Ok(list) => list.into_iter().map(|s| (s.locus_id, s.card)).collect(),
        Err(err) => {
            eprintln!("⚠ Wiederholungsplan nicht verfügbar: {err:#}");
            HashMap::new()
        }
    }
}

/// Human-readable relative time ("jetzt", "in 3 Tagen", "seit 2 Std.").
pub fn relative(target: u64, now: u64) -> String {
    let (past, secs) = if target <= now {
        (true, now - target)
    } else {
        (false, target - now)
    };
    let text = match secs {
        0..60 => {
            return if past {
                "jetzt".into()
            } else {
                "gleich".into()
            };
        }
        60..3600 => format!("{} Min.", secs / 60),
        3600..86_400 => format!("{} Std.", secs / 3600),
        86_400..172_800 => "1 Tag".into(),
        _ => format!("{} Tagen", secs / 86_400),
    };
    if past {
        format!("seit {text}")
    } else {
        format!("in {text}")
    }
}

pub fn list(app: &mut App) -> Result<()> {
    let palaces = app.store.palaces()?;
    if palaces.is_empty() {
        println!("Noch keine Paläste. Anlegen: locora new \"Planeten\" -- Merkur Venus Erde");
        return Ok(());
    }
    let cards = schedule(app, None);
    let now = now();
    println!(
        "{:<28} {:>5} {:>7} {:>5}",
        "Palast", "Loci", "fällig", "neu"
    );
    for summary in palaces {
        let tree = app.store.palace(&summary.id)?;
        let (due, fresh) = count_due(&tree, &cards, now);
        println!(
            "{:<28} {:>5} {:>7} {:>5}",
            truncate(&summary.name, 28),
            summary.loci,
            due,
            fresh
        );
    }
    Ok(())
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        text.chars().take(max - 1).chain(['…']).collect()
    }
}

/// (due now, never answered) loci of a palace.
pub fn count_due(
    tree: &PalaceTree,
    cards: &HashMap<String, CardState>,
    now: u64,
) -> (usize, usize) {
    tree.route()
        .fold((0, 0), |(due, fresh), l| match cards.get(&l.id) {
            Some(card) if card.due <= now => (due + 1, fresh),
            Some(_) => (due, fresh),
            None => (due, fresh + 1),
        })
}

pub fn show(app: &mut App, query: &str) -> Result<()> {
    let tree = app.store.palace(query)?;
    let cards = schedule(app, Some(&tree.palace.id));
    let now = now();
    println!("{}", tree.palace.name);
    let mut n = 0;
    for stage in &tree.stages {
        let kind = match stage.stage.kind {
            StageKind::Blank => "",
            StageKind::Image => " 🖼",
            StageKind::Video => " 🎞",
        };
        println!("  {}{kind}", stage.stage.name);
        for locus in &stage.loci {
            n += 1;
            let when = match cards.get(&locus.id) {
                None => "neu".into(),
                Some(card) if card.due <= now => "fällig".into(),
                Some(card) => relative(card.due, now),
            };
            let time = locus
                .at
                .map(|t| format!("{:>2}:{:02} ", t as u64 / 60, t as u64 % 60))
                .unwrap_or_default();
            println!(
                "  {n:>3}. {time}{:<18} → {:<24} {when}",
                truncate(&locus.label, 18),
                truncate(&locus.memory, 24)
            );
        }
    }
    if n == 0 {
        println!("  (noch keine Loci)");
    }
    Ok(())
}

fn confirm(question: &str) -> Result<bool> {
    print!("{question} [j/N] ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().lock().read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_lowercase().as_str(),
        "j" | "ja" | "y" | "yes"
    ))
}

pub fn remove(app: &mut App, query: &str, locus: Option<usize>, yes: bool) -> Result<()> {
    let tree = app.store.palace(query)?;
    match locus {
        Some(n) => {
            let target: &Locus = tree
                .route()
                .nth(n.checked_sub(1).context("Loci zählen ab 1")?)
                .with_context(|| format!("„{}“ hat keinen Locus {n}", tree.palace.name))?;
            if !yes
                && !confirm(&format!(
                    "Locus {n} ({} → {}) löschen?",
                    target.label, target.memory
                ))?
            {
                println!("Abgebrochen.");
                return Ok(());
            }
            app.store.delete_locus(&target.id)?;
            app.publish(&LocusDeleted {
                palace_id: tree.palace.id.clone(),
                locus_id: target.id.clone(),
            })?;
            println!("✓ Locus {n} gelöscht");
        }
        None => {
            let count = tree.route().count();
            if !yes
                && !confirm(&format!(
                    "Palast „{}“ mit {count} Loci löschen?",
                    tree.palace.name
                ))?
            {
                println!("Abgebrochen.");
                return Ok(());
            }
            app.store.delete_palace(&tree.palace.id)?;
            app.publish(&PalaceDeleted {
                palace_id: tree.palace.id.clone(),
            })?;
            println!("✓ Palast „{}“ gelöscht", tree.palace.name);
        }
    }
    Ok(())
}

pub fn edit(app: &mut App, query: &str, n: usize, patch: LocusPatch) -> Result<()> {
    let tree = app.store.palace(query)?;
    let target = tree
        .route()
        .nth(n.checked_sub(1).context("Loci zählen ab 1")?)
        .with_context(|| format!("„{}“ hat keinen Locus {n}", tree.palace.name))?;
    let locus = app.store.update_locus(&target.id, &patch)?;
    println!("✓ Locus {n}: {} → {}", locus.label, locus.memory);
    Ok(())
}

pub fn due(app: &mut App) -> Result<()> {
    let cards = schedule(app, None);
    if !app.host.provides(<Schedule as locora_api::Service>::NAME) {
        bail!("Das Plugin `review` (Wiederholung) ist nicht geladen");
    }
    let now = now();
    let mut any = false;
    let mut next: Option<u64> = None;
    for summary in app.store.palaces()? {
        let tree = app.store.palace(&summary.id)?;
        let (due, fresh) = count_due(&tree, &cards, now);
        for l in tree.route() {
            if let Some(card) = cards.get(&l.id).filter(|c| c.due > now) {
                next = Some(next.map_or(card.due, |n| n.min(card.due)));
            }
        }
        if due + fresh > 0 {
            any = true;
            let mut parts = Vec::new();
            if due > 0 {
                parts.push(format!("{due} fällig"));
            }
            if fresh > 0 {
                parts.push(format!("{fresh} neu"));
            }
            println!("▶ {}: {}", tree.palace.name, parts.join(", "));
        }
    }
    if any {
        println!("  Wiederholen: locora review");
    } else {
        match next {
            Some(t) => println!("Nichts fällig. Nächste Wiederholung {}.", relative(t, now)),
            None => println!("Nichts fällig."),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_parse_from_args_and_lines() {
        assert_eq!(parse_item("Merkur").unwrap().memory, "Merkur");
        let video = parse_item("1:23=These").unwrap();
        assert_eq!((video.time, video.memory.as_str()), (Some(83.0), "These"));
        let full = parse_item("Herd | Merkur | 0.4,0.6").unwrap();
        assert_eq!(full.label.as_deref(), Some("Herd"));
        assert_eq!(
            (full.memory.as_str(), full.at),
            ("Merkur", Some((0.4, 0.6)))
        );
        let timed = parse_item("0:05 | Tür | Einleitung").unwrap();
        assert_eq!(
            (timed.time, timed.label.as_deref()),
            (Some(5.0), Some("Tür"))
        );
        assert!(parse_item("a | b | c | d").is_err());
        assert_eq!(parse_item("Gleich = 3").unwrap().memory, "Gleich = 3");
    }

    #[test]
    fn times_parse() {
        assert_eq!(parse_time("83"), Some(83.0));
        assert_eq!(parse_time("1:23"), Some(83.0));
        assert_eq!(parse_time("1:00:05"), Some(3605.0));
        assert_eq!(parse_time("a:1"), None);
        assert_eq!(parse_time("1::2"), None);
    }

    #[test]
    fn relative_times_read_naturally() {
        assert_eq!(relative(1000, 1000), "jetzt");
        assert_eq!(relative(1000 + 3 * 86_400, 1000), "in 3 Tagen");
        assert_eq!(relative(1000 + 7200, 1000), "in 2 Std.");
        assert_eq!(relative(1000, 1000 + 600), "seit 10 Min.");
    }
}
