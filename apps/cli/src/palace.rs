//! Palace commands: new, add, ls, show, rm, due.

use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use locora_api::core::{LocusDeleted, PalaceCreated, PalaceDeleted};
use locora_api::review::{CardState, Schedule, ScheduleRequest};
use locora_store::{Locus, LocusPatch, NewLocus, PalaceTree, RoomKind};

use crate::app::{App, now};

/// One locus to create: from a command-line item or a line of a file.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub place: Option<String>,
    pub item: String,
    pub at: Option<(f64, f64)>,
    pub time: Option<f64>,
}

pub use locora_source::parse_time;

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
            place: None,
            item: String::new(),
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
            [memory] => item.item = memory.to_string(),
            [label, memory] => {
                item.place = Some(label.to_string()).filter(|l| !l.is_empty());
                item.item = memory.to_string();
            }
            _ => {
                bail!("Zeile „{text}“: erwartet `Inhalt`, `Ort | Inhalt` oder `Ort | Inhalt | x,y`")
            }
        }
        if item.item.is_empty() {
            bail!("Zeile „{text}“ hat keinen Inhalt");
        }
        return Ok(item);
    }
    if let Some((time, memory)) = text.split_once('=')
        && let Some(time) = parse_time(time)
    {
        return Ok(Item {
            place: None,
            item: memory.trim().to_owned(),
            at: None,
            time: Some(time),
        });
    }
    Ok(Item {
        place: None,
        item: text.to_owned(),
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

fn media_kind(path: &Path) -> Result<(RoomKind, &'static str)> {
    locora_source::media_type(path).with_context(|| {
        format!(
            "{}: unbekanntes Format. Bilder: png, jpg, webp, gif, avif · Videos: mp4, webm, mov",
            path.display()
        )
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
        None => (RoomKind::Blank, None),
    };
    if kind == RoomKind::Video
        && let Some(item) = items.iter().find(|i| i.time.is_none())
    {
        bail!(
            "„{}“ braucht im Video-Palast eine Zeit, z. B. `1:23={}`",
            item.item,
            item.item
        );
    }
    let palace = app.store.create_palace(&args.name)?;
    let room_name = args
        .media
        .as_deref()
        .and_then(|p| p.file_stem())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Raum 1".into());
    let room = app
        .store
        .add_room(&palace.id, &room_name, kind, media.as_deref())?;
    let spots = locora_source::places::route(items.len());
    for (i, item) in items.iter().enumerate() {
        let (x, y) = item.at.unwrap_or(spots[i]);
        app.store.add_locus(
            &room.id,
            NewLocus {
                place: item
                    .place
                    .clone()
                    .unwrap_or_else(|| format!("Ort {}", i + 1)),
                item: item.item.clone(),
                x,
                y,
                time: item.time,
                answer: Default::default(),
                image: None,
                note: String::new(),
            },
        )?;
    }
    app.publish(&PalaceCreated {
        palace_id: palace.id.clone(),
        name: palace.name.clone(),
    })?;
    let what = match kind {
        RoomKind::Blank => String::new(),
        RoomKind::Image => " mit Bild".into(),
        RoomKind::Video => " mit Video".into(),
    };
    let n = items.len();
    println!(
        "✓ Palast „{}“{what} angelegt: {n} {}",
        palace.name,
        if n == 1 { "Locus" } else { "Loci" }
    );
    if items.is_empty() {
        println!("  Loci hinzufügen: locora add \"{}\" <Inhalt>", palace.name);
    } else {
        println!("  Üben: locora practice \"{}\"", palace.name);
    }
    Ok(())
}

pub struct AddArgs {
    pub palace: String,
    pub item: String,
    pub place: Option<String>,
    pub at: Option<String>,
    pub time: Option<String>,
    pub image: Option<PathBuf>,
    pub note: Option<String>,
}

/// Store a picture file (image or `.sply` animation) for a locus.
pub fn store_picture(app: &App, path: &Path) -> Result<String> {
    let (kind, media_type) = media_kind(path)?;
    if kind == RoomKind::Video {
        bail!("{}: Ein Locus-Bild kann kein Video sein", path.display());
    }
    let bytes = fs::read(path).with_context(|| format!("{} lesen", path.display()))?;
    if media_type == locora_store::SPLINELY && !bytes.starts_with(b"\x89SPLY\r\n\x1a") {
        bail!("{} ist keine binäre .sply-Datei", path.display());
    }
    Ok(app.store.put_media(&bytes, media_type)?.hash)
}

pub fn add(app: &mut App, args: AddArgs) -> Result<()> {
    let tree = app.store.palace(&args.palace)?;
    let Some(room) = tree.rooms.last() else {
        bail!("Palast „{}“ hat noch keinen Raum", tree.palace.name);
    };
    let count = room.loci.len();
    let (x, y) = match &args.at {
        Some(text) => {
            parse_point(text).context("--at erwartet x,y zwischen 0 und 1, z. B. 0.4,0.6")?
        }
        None => *locora_source::places::route(count + 1)
            .last()
            .expect("one point"),
    };
    let time = match &args.time {
        Some(t) => Some(parse_time(t).context("--time erwartet mm:ss oder Sekunden")?),
        None => None,
    };
    if room.room.kind == RoomKind::Video && time.is_none() {
        bail!("Im Video-Palast braucht ein Locus eine Zeit: --time 1:23");
    }
    let locus = app.store.add_locus(
        &room.room.id,
        NewLocus {
            place: args
                .place
                .unwrap_or_else(|| format!("Ort {}", tree.route().count() + 1)),
            item: args.item,
            x,
            y,
            time,
            answer: Default::default(),
            image: args
                .image
                .as_deref()
                .map(|path| store_picture(app, path))
                .transpose()?,
            note: args.note.unwrap_or_default(),
        },
    )?;
    println!(
        "✓ Locus {} in „{}“: {} → {}",
        tree.route().count() + 1,
        tree.palace.name,
        locus.place,
        locus.item
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
    for room in &tree.rooms {
        let kind = match room.room.kind {
            RoomKind::Blank => "",
            RoomKind::Image => " 🖼",
            RoomKind::Video => " 🎞",
        };
        println!("  {}{kind}", room.room.name);
        for locus in &room.loci {
            n += 1;
            let when = match cards.get(&locus.id) {
                None => "neu".into(),
                Some(card) if card.due <= now => "fällig".into(),
                Some(card) => relative(card.due, now),
            };
            let time = locus
                .time
                .map(|t| format!("{:>2}:{:02} ", t as u64 / 60, t as u64 % 60))
                .unwrap_or_default();
            // ▣ marks a locus with its own picture (image or animation).
            let picture = if locus.image.is_some() { "▣ " } else { "  " };
            println!(
                "  {n:>3}. {picture}{time}{:<18} → {:<24} {when}",
                truncate(&locus.place, 18),
                truncate(&locus.item, 24)
            );
            if !locus.answer.accept.is_empty() {
                println!(
                    "              auch richtig: {}",
                    locus.answer.accept.join(" · ")
                );
            }
            if !locus.note.is_empty() {
                println!("              ✎ {}", truncate(&locus.note, 60));
            }
        }
    }
    if n == 0 {
        println!("  (noch keine Loci)");
    }
    Ok(())
}

pub fn confirm(question: &str) -> Result<bool> {
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
                    target.place, target.item
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
    println!("✓ Locus {n}: {} → {}", locus.place, locus.item);
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

/// `locora marker PALACE [FILE | --reset]`: show or change a palace's locus marker.
pub fn marker(app: &mut App, query: &str, file: Option<&Path>, reset: bool) -> Result<()> {
    let tree = app.store.palace(query)?;
    let name = &tree.palace.name;
    match (file, reset) {
        (Some(path), _) => {
            if locora_source::media_type(path).map(|(_, t)| t) != Some(locora_store::SPLINELY) {
                bail!("{}: Ein Marker muss eine .sply-Datei sein", path.display());
            }
            let hash = store_picture(app, path)?;
            app.store.set_marker(&tree.palace.id, Some(&hash))?;
            println!("✓ „{name}“ zeigt seine Orte jetzt mit {}", path.display());
        }
        (None, true) => {
            app.store.set_marker(&tree.palace.id, None)?;
            println!("✓ „{name}“ nutzt wieder den Standard-Marker");
        }
        (None, false) => match &tree.palace.marker {
            Some(hash) => println!("„{name}“ hat einen eigenen Marker ({}…)", &hash[..12]),
            None => println!("„{name}“ nutzt den Standard-Marker"),
        },
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_parse_from_args_and_lines() {
        assert_eq!(parse_item("Merkur").unwrap().item, "Merkur");
        let video = parse_item("1:23=These").unwrap();
        assert_eq!((video.time, video.item.as_str()), (Some(83.0), "These"));
        let full = parse_item("Herd | Merkur | 0.4,0.6").unwrap();
        assert_eq!(full.place.as_deref(), Some("Herd"));
        assert_eq!((full.item.as_str(), full.at), ("Merkur", Some((0.4, 0.6))));
        let timed = parse_item("0:05 | Tür | Einleitung").unwrap();
        assert_eq!(
            (timed.time, timed.place.as_deref()),
            (Some(5.0), Some("Tür"))
        );
        assert!(parse_item("a | b | c | d").is_err());
        assert_eq!(parse_item("Gleich = 3").unwrap().item, "Gleich = 3");
    }

    #[test]
    fn relative_times_read_naturally() {
        assert_eq!(relative(1000, 1000), "jetzt");
        assert_eq!(relative(1000 + 3 * 86_400, 1000), "in 3 Tagen");
        assert_eq!(relative(1000 + 7200, 1000), "in 2 Std.");
        assert_eq!(relative(1000, 1000 + 600), "seit 10 Min.");
    }
}
