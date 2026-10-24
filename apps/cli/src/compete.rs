//! `locora compete`: the memory contest in the terminal. Today's challenge, rounds of a shopping
//! list, numbers or cards against the clock, the evening recall, challenge codes, records and the
//! history. Rules and stored rounds are the `competition` plugin's, the same as in the app.
//!
//! A round needs a terminal: the task is shown on the alternate screen, so the scrollback keeps
//! nothing of it, and Enter ends memorizing. Each phase ends at its limit, also in the middle of
//! a line. `LOCORA_COMPETE_SCRIPTED=1` reads the input from a pipe instead and prints the task
//! plainly (for tests).

use std::io::{self, BufRead, IsTerminal, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use locora_api::competition::{
    Counted, EveningState, FinishResponse, Hint, Kind, LevelState, RecordsResponse,
};
use locora_competition::{
    Date, Discipline, Lang, Level, Mark, Recall, Score, Spec, Task, cards, code, numbers, schedule,
    words,
};
use locora_runtime::competition::{HandIn, Round, capped};

use crate::app::App;

#[derive(Subcommand)]
pub enum CompeteCommand {
    /// Heute: Disziplin, Stufen, Stand, Vorbereitung (Orte, passende Paläste), morgen
    Today,
    /// Eine Runde spielen: Übung, die Tages-Challenge (--daily) oder einen Challenge-Code (--code)
    Play {
        /// list, numbers oder cards (bei --daily und --code: die des Tages bzw. des Codes)
        discipline: Option<String>,
        /// Stufe 1, 2 oder 3 (Standard: 1; ein Code bringt seine eigene)
        #[arg(long, value_parser = clap::value_parser!(u8).range(1..=3))]
        level: Option<u8>,
        /// Die Tages-Challenge von heute (ein Versuch je Stufe)
        #[arg(long, conflicts_with = "code")]
        daily: bool,
        /// Die Aufgabe eines Challenge-Codes, z. B. C2-28T5-CY4T-QKFF-K
        #[arg(long)]
        code: Option<String>,
        /// Ohne Zeitlimit üben (zählt nicht für Rekorde)
        #[arg(long, conflicts_with = "daily")]
        untimed: bool,
        /// Die Orte nach der Route eines Palasts benennen
        #[arg(long, value_name = "PALAST")]
        route: Option<String>,
        /// Sprache der Einkaufsliste und der Kartennamen: de, en, fr, ja oder zh
        #[arg(long, default_value = "de")]
        lang: String,
    },
    /// Abendabfrage einer Tagesrunde (ohne RUNDE: die offene von heute)
    Evening {
        round: Option<String>,
        /// Sprache der Antworten: de, en, fr, ja oder zh (Standard: die der Runde am Morgen)
        #[arg(long)]
        lang: Option<String>,
    },
    /// Einen neuen Challenge-Code ausgeben
    Code {
        /// list, numbers oder cards
        discipline: String,
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=3))]
        level: u8,
    },
    /// Rekorde je Disziplin und Stufe, Tage mit Tages-Challenge, Trophäen
    Records {
        #[arg(long)]
        json: bool,
    },
    /// Die gespielten Runden, neueste zuerst
    History {
        #[arg(long, default_value_t = 20)]
        limit: u32,
        #[arg(long)]
        json: bool,
    },
}

pub fn run(app: &mut App, command: Option<CompeteCommand>) -> Result<()> {
    if !app.has_competition() {
        bail!("Dafür braucht es das Plugin `competition` (Gedächtnis-Wettkampf)");
    }
    match command.unwrap_or(CompeteCommand::Today) {
        CompeteCommand::Today => today(app),
        CompeteCommand::Play {
            discipline,
            level,
            daily,
            code,
            untimed,
            route,
            lang,
        } => {
            let lang = language(&lang)?;
            let discipline = discipline.as_deref().map(parse_discipline).transpose()?;
            let level = level.map(|n| Level::new(n).expect("1 bis 3, von clap geprüft"));
            let (kind, discipline, level, seed) = if let Some(text) = code {
                let (coded, coded_level, seed) =
                    code::decode(&text).map_err(|error| anyhow::anyhow!("{error}"))?;
                if discipline.is_some_and(|d| d != coded) {
                    bail!("Dieser Code gehört zur Disziplin „{}“", name(coded));
                }
                if level.is_some_and(|l| l != coded_level) {
                    bail!("Dieser Code gehört zur Stufe {}", coded_level.get());
                }
                (Kind::Code, coded, Some(coded_level), Some(seed))
            } else if daily {
                let planned = schedule::daily(app.learning_date()?).discipline;
                (Kind::Daily, discipline.unwrap_or(planned), level, None)
            } else {
                let discipline = discipline.context(
                    "Welche Disziplin? list, numbers oder cards (oder --daily, --code CODE)",
                )?;
                (Kind::Practice, discipline, level, None)
            };
            let route = route.as_deref().map(|p| route_names(app, p)).transpose()?;
            let play = Play {
                kind,
                discipline,
                level: level.unwrap_or(Level::ALL[0]),
                seed,
                untimed,
                route,
                lang,
            };
            play.run(app)
        }
        CompeteCommand::Evening { round, lang } => {
            let lang = lang.as_deref().map(language).transpose()?;
            evening(app, round, lang)
        }
        CompeteCommand::Code { discipline, level } => {
            let discipline = parse_discipline(&discipline)?;
            let level = Level::new(level).expect("1 bis 3, von clap geprüft");
            let code = code::encode(discipline, level, locora_runtime::competition::new_seed());
            println!("{code}");
            println!("Spielen: locora compete play --code {code}");
            Ok(())
        }
        CompeteCommand::Records { json } => records(app, json),
        CompeteCommand::History { limit, json } => history(app, limit, json),
    }
}

fn parse_discipline(text: &str) -> Result<Discipline> {
    Discipline::parse(text)
        .with_context(|| format!("Unbekannte Disziplin „{text}“: list, numbers oder cards"))
}

fn language(text: &str) -> Result<Lang> {
    Lang::parse(text)
        .with_context(|| format!("Unbekannte Sprache „{text}“: de, en, fr, ja oder zh"))
}

/// The discipline as a title.
fn name(discipline: Discipline) -> &'static str {
    match discipline {
        Discipline::List => "Einkaufsliste",
        Discipline::Numbers => "Zahlen",
        Discipline::Cards => "Karten",
    }
}

/// „10 Dinge“, „40 Ziffern“, „26 Karten“.
fn size(spec: &Spec) -> String {
    let unit = match spec.discipline {
        Discipline::List => "Dinge",
        Discipline::Numbers => "Ziffern",
        Discipline::Cards => "Karten",
    };
    format!("{} {unit}", spec.items)
}

/// Milliseconds as `m:ss`.
fn clock(ms: u32) -> String {
    let seconds = ms / 1_000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn weekday(date: Date) -> &'static str {
    [
        "Montag",
        "Dienstag",
        "Mittwoch",
        "Donnerstag",
        "Freitag",
        "Samstag",
        "Sonntag",
    ][usize::from(date.weekday())]
}

/// `28.10.2026`
fn german(date: Date) -> String {
    format!("{:02}.{:02}.{}", date.day(), date.month(), date.year())
}

/// A time of day on this device's clock, `14:05`.
fn local_time(app: &App, at_ms: u64) -> Result<String> {
    let seconds = at_ms / 1_000;
    let offset = app.day_clock()?.offset_at(seconds);
    let local = (seconds as i64 + i64::from(offset) * 60).rem_euclid(86_400);
    Ok(format!("{}:{:02}", local / 3_600, local % 3_600 / 60))
}

fn trophy(id: &str) -> &str {
    match id {
        "compete-first" => "Erste Runde",
        "compete-daily-7" => "7 Tages-Challenges",
        "compete-daily-30" => "30 Tages-Challenges",
        "compete-list-30" => "Volle Einkaufsliste",
        "compete-numbers-40" => "40 Ziffern ohne Fehler",
        "compete-deck" => "Volles Deck",
        "compete-evening" => "Alles behalten",
        "compete-triple" => "Alle drei",
        other => other,
    }
}

fn mark(mark: Mark) -> &'static str {
    match mark {
        Mark::Right => "richtig",
        Mark::Tolerated => "richtig (Tippfehler zählt)",
        Mark::Wrong => "falsch",
        Mark::Empty => "leer",
        Mark::Duplicate => "schon gezählt",
    }
}

fn kind(kind: Kind) -> &'static str {
    match kind {
        Kind::Practice => "Übung",
        Kind::Daily => "Tages-Challenge",
        Kind::Code => "Code",
    }
}

/// A palace's route: its id and the place names, in order.
fn route_names(app: &App, query: &str) -> Result<(String, Vec<String>)> {
    let tree = app.store.palace(query)?;
    if locora_runtime::built_in_kind(&tree.palace.id).is_some() {
        bail!("Die eingebauten Paläste sind keine Route für den Wettkampf");
    }
    let names = tree.route().map(|locus| locus.place.clone()).collect();
    Ok((tree.palace.id, names))
}

/// „Ort 3“ or „Ort 3 · Sofa“.
fn place(route: &[String], index: usize) -> String {
    match route.get(index).filter(|name| !name.is_empty()) {
        Some(name) => format!("Ort {} · {name}", index + 1),
        None => format!("Ort {}", index + 1),
    }
}

fn today(app: &mut App) -> Result<()> {
    let today = app.compete_today()?;
    app.report();
    let day = today.today;
    println!(
        "Heute, {} {}: {}",
        weekday(day.date),
        german(day.date),
        name(day.discipline)
    );
    for level in &today.levels {
        let state = match &level.state {
            LevelState::Open => "noch nicht gespielt".to_owned(),
            LevelState::Started { .. } => "läuft gerade".to_owned(),
            LevelState::Unfinished { .. } => "begonnen, nicht beendet".to_owned(),
            LevelState::Done {
                points,
                max,
                counted,
                ..
            } => {
                let practice = *counted == Counted::Practice;
                let note = if practice { " (zählt als Übung)" } else { "" };
                format!("{points} von {max} Punkten{note}")
            }
        };
        println!(
            "  Stufe {} · {} · {state}",
            level.spec.level.get(),
            size(&level.spec)
        );
    }
    if day.long_day {
        println!("Langer Tag: morgens einprägen, abends noch einmal abrufen.");
    }
    for evening in &today.evenings {
        let what = format!("{} Stufe {}", name(evening.discipline), evening.level.get());
        match evening.state {
            EveningState::Locked => println!(
                "Abendabfrage {what}: ab {}",
                local_time(app, evening.opens_at_ms)?
            ),
            EveningState::Open => println!(
                "Abendabfrage {what}: offen bis {} (`locora compete evening`)",
                local_time(app, evening.closes_at_ms)?
            ),
            EveningState::Done { points, max } => {
                println!("Abendabfrage {what}: {points} von {max} behalten")
            }
            EveningState::Closed => {}
        }
    }
    println!("Vorbereitung (Orte je Stufe):");
    for level in &today.levels {
        let options: Vec<String> = schedule::prep(&level.spec)
            .options
            .iter()
            .map(|o| format!("{} ({} pro Ort)", o.places, o.per_place))
            .collect();
        println!(
            "  Stufe {}: {}",
            level.spec.level.get(),
            options.join(" · ")
        );
    }
    let palaces: Vec<String> = app
        .store
        .palaces()?
        .into_iter()
        .filter(|p| locora_runtime::built_in_kind(&p.id).is_none())
        .map(|palace| {
            let fits: Vec<String> = today
                .levels
                .iter()
                .filter(|level| {
                    let first = schedule::prep(&level.spec).options[0].places;
                    palace.loci >= u32::from(first)
                })
                .map(|level| level.spec.level.get().to_string())
                .collect();
            if fits.is_empty() {
                format!("{} ({} Orte, zu kurz)", palace.name, palace.loci)
            } else {
                format!(
                    "{} ({} Orte, reicht für Stufe {})",
                    palace.name,
                    palace.loci,
                    fits.join(", ")
                )
            }
        })
        .collect();
    if palaces.is_empty() {
        println!("Paläste: noch keiner – eine Route braucht so viele Orte wie oben.");
    } else {
        println!("Paläste: {}", palaces.join(" · "));
    }
    println!("Morgen: {}", name(today.tomorrow.discipline));
    println!("Alle bekommen heute dieselbe Aufgabe. Deine Ergebnisse bleiben auf deinen Geräten.");
    Ok(())
}

/// Lines from stdin, read on a thread of their own, so a phase can end at its limit while a line
/// is still being typed.
struct Input {
    lines: Receiver<String>,
}

enum Next {
    Line(String),
    /// The phase's time is up.
    Timeout,
    /// The input ended.
    End,
}

impl Input {
    fn new() -> Self {
        let (sender, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        Input { lines }
    }

    fn next(&self, deadline: Option<Instant>) -> Next {
        let line = match deadline {
            None => self
                .lines
                .recv()
                .map_err(|_| RecvTimeoutError::Disconnected),
            Some(at) => self
                .lines
                .recv_timeout(at.saturating_duration_since(Instant::now())),
        };
        match line {
            Ok(line) => Next::Line(line.trim_end_matches('\r').to_owned()),
            Err(RecvTimeoutError::Timeout) => Next::Timeout,
            Err(RecvTimeoutError::Disconnected) => Next::End,
        }
    }

    /// Asks for one line until `deadline`.
    fn ask(&self, prompt: &str, deadline: Option<Instant>) -> Result<Next> {
        print!("{prompt}");
        io::stdout().flush()?;
        let next = self.next(deadline);
        if !matches!(next, Next::Line(_)) {
            println!();
        }
        Ok(next)
    }
}

/// The task on the alternate screen, which a terminal clears again; leaves it also on an error.
struct Screen {
    alternate: bool,
}

impl Screen {
    fn enter(scripted: bool) -> Result<Self> {
        if !scripted {
            print!("\x1b[?1049h\x1b[H\x1b[2J");
            io::stdout().flush()?;
        }
        Ok(Screen {
            alternate: !scripted,
        })
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        if self.alternate {
            print!("\x1b[?1049l");
            let _ = io::stdout().flush();
        }
    }
}

fn scripted() -> bool {
    std::env::var_os("LOCORA_COMPETE_SCRIPTED").is_some_and(|value| value == "1")
}

/// The task as the memorize screen shows it.
fn show_task(task: &Task, route: &[String], lang: Lang) {
    match task {
        Task::List(items) => {
            for (i, id) in items.iter().enumerate() {
                let thing = words::display(*id, lang).unwrap_or_default();
                println!("  {:>2}  {thing}  ({})", i + 1, place(route, i));
            }
        }
        Task::Numbers(digits) => {
            for (row, cells) in digits.chunks(numbers::ROW).enumerate() {
                let text: String = cells.iter().map(u8::to_string).collect();
                println!("  Reihe {:>2}  {text}", row + 1);
            }
        }
        Task::Cards(deck) => {
            for (i, card) in deck.iter().enumerate() {
                println!(
                    "  {:>2}  {} {}  ({})",
                    i + 1,
                    cards::short(*card, lang),
                    cards::name_capitalized(*card, lang),
                    place(route, i)
                );
            }
        }
    }
}

/// Reads a card as typed: `7h`, `10s`, `kd`, `as` (rank and suit with English letters or the suit
/// symbol), or `7 2` (rank, then the suit 1 ♠, 2 ♥, 3 ♦, 4 ♣). Jack, queen and king also take the
/// letters of `lang`, as the task shows them (`B♦` in German).
fn parse_card(text: &str, lang: Lang) -> Option<u8> {
    let text = text.trim().to_lowercase();
    let rank = |rank: &str| -> Option<u8> {
        if rank == "10" {
            return Some(9);
        }
        let mut chars = rank.chars();
        let key = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        cards::rank_from_key(key, Lang::En).or_else(|| cards::rank_from_key(key, lang))
    };
    let parts: Vec<&str> = text.split_whitespace().collect();
    let (rank, suit) = match parts[..] {
        [r, s] => {
            let suit: u8 = s.parse().ok().filter(|s| (1..=4).contains(s))?;
            (rank(r)?, suit - 1)
        }
        [one] => {
            let symbol = one.chars().last()?;
            let suit = match symbol {
                's' | '♠' => 0,
                'h' | '♥' => 1,
                'd' | '♦' => 2,
                'c' | '♣' => 3,
                _ => return None,
            };
            (rank(&one[..one.len() - symbol.len_utf8()])?, suit)
        }
        _ => return None,
    };
    cards::card(suit, rank)
}

/// Reads a recall for `spec`, place by place, until `deadline`. A phase that ends early leaves
/// the rest empty.
fn read_recall(
    input: &Input,
    spec: &Spec,
    route: &[String],
    lang: Lang,
    deadline: Option<Instant>,
) -> Result<Recall> {
    let items = usize::from(spec.items);
    let mut recall = Recall::blank(spec.discipline, spec.items);
    match &mut recall {
        Recall::List(answers) => {
            for (i, answer) in answers.iter_mut().enumerate() {
                match input.ask(&format!("{}: ", place(route, i)), deadline)? {
                    Next::Line(line) => *answer = line.trim().to_owned(),
                    Next::Timeout | Next::End => break,
                }
            }
        }
        Recall::Numbers(cells) => {
            println!("Ein . oder Leerzeichen steht für eine Ziffer, die du nicht weißt.");
            for row in 0..items.div_ceil(numbers::ROW) {
                let from = row * numbers::ROW;
                let to = (from + numbers::ROW).min(items);
                let prompt = format!("Reihe {} (Ziffern {}–{}): ", row + 1, from + 1, to);
                match input.ask(&prompt, deadline)? {
                    Next::Line(line) => {
                        let typed = numbers::parse_row(&line);
                        cells[from..to].copy_from_slice(&typed[..to - from]);
                    }
                    Next::Timeout | Next::End => break,
                }
            }
        }
        Recall::Cards(places) => {
            println!(
                "Karten als 7h, 10s, kd, as oder 7 2 (Rang, dann Farbe 1 ♠ 2 ♥ 3 ♦ 4 ♣), leer für keine."
            );
            let mut i = 0;
            while i < items {
                let line = match input.ask(&format!("{}: ", place(route, i)), deadline)? {
                    Next::Line(line) => line,
                    Next::Timeout | Next::End => break,
                };
                if line.trim().is_empty() {
                    places[i] = None;
                    i += 1;
                    continue;
                }
                let Some(card) = parse_card(&line, lang) else {
                    println!("  Unbekannte Karte „{}“", line.trim());
                    continue;
                };
                // A card lies on one place only: placing it again moves it.
                if let Some(earlier) = places.iter().position(|p| *p == Some(card)) {
                    places[earlier] = None;
                    println!(
                        "  {} von Ort {} nach Ort {} verschoben",
                        cards::name_capitalized(card, lang),
                        earlier + 1,
                        i + 1
                    );
                }
                places[i] = Some(card);
                i += 1;
            }
        }
    }
    Ok(recall)
}

/// A round to play: what it is, and how it is shown.
struct Play {
    kind: Kind,
    discipline: Discipline,
    level: Level,
    /// Practice without it gets a fresh seed; the daily has the date's.
    seed: Option<locora_competition::Seed>,
    untimed: bool,
    /// The palace whose route names the places, and those names.
    route: Option<(String, Vec<String>)>,
    lang: Lang,
}

impl Play {
    /// Begins the round, shows the task until Enter or its limit, reads the recall until its
    /// limit, hands it in and prints the result.
    fn run(self, app: &mut App) -> Result<()> {
        let scripted = scripted();
        if !scripted && !io::stdin().is_terminal() {
            bail!("`locora compete play` braucht ein Terminal");
        }
        let Play {
            kind,
            discipline,
            level,
            seed,
            untimed,
            route,
            lang,
        } = self;
        let (palace, route) = route.unzip();
        let route = route.unwrap_or_default();
        let input = Input::new();
        let round = app.compete_begin(kind, discipline, level, seed)?;
        app.report();
        let spec = round.spec;
        let limit = |ms: u32| (!untimed).then(|| Duration::from_millis(u64::from(ms)));
        let t0 = Instant::now();
        let done = {
            let _screen = Screen::enter(scripted)?;
            println!(
                "{} · Stufe {} · {} · {}",
                name(spec.discipline),
                spec.level.get(),
                size(&spec),
                kind_line(&round, untimed)
            );
            match limit(spec.memorize_ms) {
                Some(_) => println!(
                    "Einprägen: bis {}. Abrufen: bis {}.",
                    clock(spec.memorize_ms),
                    clock(spec.recall_ms)
                ),
                None => println!("Ohne Zeitlimit – zählt nicht für Rekorde."),
            }
            println!();
            show_task(&round.task, &route, lang);
            println!();
            let deadline = limit(spec.memorize_ms).map(|limit| t0 + limit);
            match input.ask("Enter: fertig – jetzt abrufen ", deadline)? {
                Next::Timeout => {
                    println!("Die Zeit ist um – jetzt abrufen.");
                    Instant::now()
                }
                Next::Line(_) | Next::End => Instant::now(),
            }
        };
        println!(
            "Abrufen: {}, Stufe {}",
            name(spec.discipline),
            spec.level.get()
        );
        let deadline = limit(spec.recall_ms).map(|limit| done + limit);
        let recall = read_recall(&input, &spec, &route, lang, deadline)?;
        if deadline.is_some_and(|at| Instant::now() >= at) {
            println!("Die Zeit ist um – deine Antworten wurden abgegeben.");
        }
        let (memorize_ms, recall_ms) = capped(&spec, untimed, done - t0, done.elapsed());
        let hand_in = HandIn {
            recall: recall.clone(),
            lang,
            untimed,
            route: palace,
            memorize_ms,
            recall_ms,
        };
        let finished = app.compete_finish(&round, hand_in)?;
        app.report();
        print_result(app, &round, &recall, &finished, untimed, memorize_ms, lang)
    }
}

fn kind_line(round: &Round, untimed: bool) -> String {
    match round.kind {
        Kind::Daily => "Tages-Challenge: ein Versuch heute".to_owned(),
        Kind::Code => "Challenge-Code".to_owned(),
        Kind::Practice if untimed => "Übung ohne Zeitlimit".to_owned(),
        Kind::Practice => "Übung".to_owned(),
    }
}

/// The result: points, time, record, trophies, the information line, every place, the hint.
fn print_result(
    app: &App,
    round: &Round,
    recall: &Recall,
    finished: &FinishResponse,
    untimed: bool,
    memorize_ms: u32,
    lang: Lang,
) -> Result<()> {
    let score = &finished.score;
    println!();
    println!("Ergebnis: {} von {} Punkten", score.points, score.max);
    if score.perfect {
        println!("Perfekt!");
    }
    println!("Eingeprägt in {}", clock(memorize_ms));
    if let Some(line) = info_line(score) {
        println!("{line}");
    }
    if finished.record.new {
        println!("Neuer persönlicher Rekord!");
    } else if let Some(best) = &finished.record.best {
        println!(
            "Dein Rekord: {}/{} in {}",
            best.points,
            best.max,
            clock(best.memorize_ms)
        );
    }
    for id in &finished.trophies {
        println!("Neue Trophäe: {}", trophy(id));
    }
    print_places(&round.task, recall, score, lang);
    if let Some(hint) = finished.hint {
        println!("Tipp: {} (Lektion „{}“)", hint_text(hint), hint.lesson());
    }
    if finished.counted_as_practice {
        println!(
            "Die Tages-Challenge hast du schon auf einem anderen Gerät gespielt. Diese Runde zählt als Übung."
        );
    } else if finished.repeat_seed && round.kind != Kind::Daily {
        println!("Diesen Code hast du schon gespielt – er zählt nicht für Rekorde.");
    }
    if untimed {
        println!("Übung ohne Zeitlimit – zählt nicht für Rekorde.");
    }
    if let Some(code) = round.code() {
        println!("Challenge-Code: {code}");
    }
    if round.kind == Kind::Daily {
        let opens = locora_runtime::competition::now_ms()
            .saturating_add(locora_api::competition::EVENING_OPENS_MS);
        println!(
            "Abendabfrage: ab {} noch einmal abrufen, ohne Hinsehen (`locora compete evening`).",
            local_time(app, opens)?
        );
    }
    Ok(())
}

fn info_line(score: &Score) -> Option<String> {
    let max = score.max;
    if let Some(n) = score.info.in_order {
        return Some(format!("In Routen-Reihenfolge: {n} von {max}"));
    }
    if let Some(n) = score.info.digits_right {
        return Some(format!("Ziffern richtig: {n} von {max}"));
    }
    score
        .info
        .first_error
        .map(|n| format!("Richtig bis zum ersten Fehler: {n}"))
}

fn hint_text(hint: Hint) -> &'static str {
    match hint {
        Hint::Route => {
            "Mehrere Orte hintereinander waren leer oder falsch. Geh diesen Teil deiner Route noch einmal ab."
        }
        Hint::Loci => {
            "Zwei Antworten haben die Plätze getauscht. Mach diese beiden Orte unterschiedlicher."
        }
        Hint::Exaggerate => {
            "Manche Bilder sind nicht hängen geblieben. Mach sie größer, lauter, seltsamer."
        }
    }
}

/// Every place: the answer, what was right, and the verdict.
fn print_places(task: &Task, recall: &Recall, score: &Score, lang: Lang) {
    match (task, recall) {
        (Task::List(items), Recall::List(answers)) => {
            println!("Ort  Deine Antwort → gezählt für");
            for (i, answer) in answers.iter().enumerate() {
                let counted = score
                    .matched
                    .get(i)
                    .copied()
                    .flatten()
                    .and_then(|n| items.get(usize::from(n)))
                    .and_then(|id| words::display(*id, lang))
                    .unwrap_or("–");
                let answer = if answer.trim().is_empty() {
                    "–"
                } else {
                    answer
                };
                println!(
                    "{:>3}  {answer} → {counted} · {}",
                    i + 1,
                    mark(score.marks[i])
                );
            }
            let missing: Vec<&str> = items
                .iter()
                .enumerate()
                .filter(|(n, _)| !score.matched.contains(&Some(*n as u16)))
                .filter_map(|(_, id)| words::display(*id, lang))
                .collect();
            if !missing.is_empty() {
                println!("Nicht genannt: {}", missing.join(", "));
            }
        }
        (Task::Numbers(digits), Recall::Numbers(cells)) => {
            for (row, (right, typed)) in digits
                .chunks(numbers::ROW)
                .zip(cells.chunks(numbers::ROW))
                .enumerate()
            {
                let right: String = right.iter().map(u8::to_string).collect();
                let typed: String = typed
                    .iter()
                    .map(|cell| cell.map_or('.', |d| char::from(b'0' + d)))
                    .collect();
                let wrong: Vec<String> = score.marks[row * numbers::ROW..]
                    .iter()
                    .take(numbers::ROW)
                    .enumerate()
                    .filter(|(_, m)| **m != Mark::Right)
                    .map(|(n, _)| (row * numbers::ROW + n + 1).to_string())
                    .collect();
                let errors = match wrong.len() {
                    0 => "ohne Fehler".to_owned(),
                    _ => format!("falsch oder leer an Stelle {}", wrong.join(", ")),
                };
                println!(
                    "Reihe {:>2}  deine: {typed}  richtig: {right}  {errors}",
                    row + 1
                );
            }
        }
        (Task::Cards(deck), Recall::Cards(places)) => {
            println!("Ort  Deine Karte → richtig");
            for (i, (right, placed)) in deck.iter().zip(places).enumerate() {
                let placed = placed.map_or("–".to_owned(), |c| cards::short(c, lang));
                println!(
                    "{:>3}  {placed} → {} {} · {}",
                    i + 1,
                    cards::short(*right, lang),
                    cards::name_capitalized(*right, lang),
                    mark(score.marks[i])
                );
            }
        }
        _ => {}
    }
}

/// The evening recall, scored in the language of the morning round unless `lang` says another.
fn evening(app: &mut App, round: Option<String>, lang: Option<Lang>) -> Result<()> {
    let scripted = scripted();
    if !scripted && !io::stdin().is_terminal() {
        bail!("`locora compete evening` braucht ein Terminal");
    }
    let round_id = match round {
        Some(id) => id,
        None => {
            let today = app.compete_today()?;
            let open = today
                .evenings
                .iter()
                .find(|e| e.state == EveningState::Open);
            match open {
                Some(evening) => evening.round_id.clone(),
                None => {
                    match today
                        .evenings
                        .iter()
                        .find(|e| e.state == EveningState::Locked)
                    {
                        Some(locked) => println!(
                            "Die Abendabfrage öffnet um {}.",
                            local_time(app, locked.opens_at_ms)?
                        ),
                        None => println!("Gerade ist keine Abendabfrage offen."),
                    }
                    return Ok(());
                }
            }
        }
    };
    let view = app.compete_evening(&round_id)?;
    let lang = lang.unwrap_or(view.lang);
    match view.state {
        EveningState::Open => {}
        EveningState::Locked => bail!(
            "Die Abendabfrage öffnet um {}",
            local_time(app, view.opens_at_ms)?
        ),
        EveningState::Closed => bail!("Diese Abendabfrage ist geschlossen"),
        EveningState::Done { .. } => bail!("Diese Abendabfrage ist schon abgegeben"),
    }
    let route = match &view.route {
        Some(palace) => route_names(app, palace)
            .map(|(_, names)| names)
            .unwrap_or_default(),
        None => Vec::new(),
    };
    let spec = view.spec;
    println!(
        "Abendabfrage: {}, Stufe {} · {}",
        name(spec.discipline),
        spec.level.get(),
        size(&spec)
    );
    println!("Ruf ab, was du vorhin eingeprägt hast. Kein zweiter Blick.");
    let input = Input::new();
    let deadline = Instant::now() + Duration::from_millis(u64::from(spec.recall_ms));
    let recall = read_recall(&input, &spec, &route, lang, Some(deadline))?;
    let finished = app.compete_evening_finish(&round_id, recall, lang)?;
    app.report();
    println!(
        "Behalten: {} von {} (vorhin: {})",
        finished.score.points, finished.score.max, finished.morning_points
    );
    for id in &finished.trophies {
        println!("Neue Trophäe: {}", trophy(id));
    }
    Ok(())
}

fn records(app: &mut App, json: bool) -> Result<()> {
    let records: RecordsResponse = app.compete_records()?;
    app.report();
    if json {
        println!("{}", serde_json::to_string_pretty(&records)?);
        return Ok(());
    }
    println!("Rekorde");
    for level in &records.levels {
        let best = match &level.best {
            Some(best) => format!(
                "{}/{} in {}",
                best.points,
                best.max,
                clock(best.memorize_ms)
            ),
            None => "–".to_owned(),
        };
        println!(
            "  {:<14} Stufe {}  {best:<14} {} Runden",
            name(level.discipline),
            level.level.get(),
            level.rounds
        );
    }
    println!("Tages-Challenges gespielt: {}", records.daily_days);
    let earned: Vec<String> = records
        .trophies
        .iter()
        .filter_map(|t| {
            t.unlocked
                .map(|day| format!("{} ({})", trophy(&t.id), german(day)))
        })
        .collect();
    let open = records.trophies.len() - earned.len();
    println!(
        "Trophäen: {} · noch offen: {open}",
        if earned.is_empty() {
            "–".to_owned()
        } else {
            earned.join(", ")
        }
    );
    Ok(())
}

fn history(app: &mut App, limit: u32, json: bool) -> Result<()> {
    let entries = app.compete_history(limit, None, None)?;
    app.report();
    if json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
        return Ok(());
    }
    if entries.is_empty() {
        println!("Noch keine Runde. Deine erste Runde ist dein erster Rekord.");
        return Ok(());
    }
    for entry in &entries {
        let spec = Spec::standard(entry.discipline, entry.level);
        let mut line = format!(
            "{} · {} {} · {}/{} · {}",
            entry.date,
            name(entry.discipline),
            size(&spec),
            entry.points,
            entry.max,
            kind(entry.kind)
        );
        if entry.counted_as_practice {
            line.push_str(" (zählt als Übung)");
        }
        if entry.untimed {
            line.push_str(" (ohne Zeitlimit)");
        }
        if let Some(evening) = entry.evening {
            line.push_str(&format!(
                " · Abendabfrage {}/{}",
                evening.points, evening.max
            ));
        }
        println!("{line}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cards_are_typed_with_letters_symbols_or_numbers() {
        let card = |suit, rank| cards::card(suit, rank);
        let en = |text| parse_card(text, Lang::En);
        assert_eq!(en("7h"), card(1, 6));
        assert_eq!(en(" 10S "), card(0, 9));
        assert_eq!(en("0s"), card(0, 9));
        assert_eq!(en("kd"), card(2, 12));
        assert_eq!(en("as"), card(0, 0));
        assert_eq!(en("1c"), card(3, 0));
        assert_eq!(en("Q♥"), card(1, 11));
        assert_eq!(en("7 2"), card(1, 6));
        assert_eq!(en("j 3"), card(2, 10));
        for bad in ["", "7", "7x", "11h", "7 5", "7 0", "h7", "7 2 1", "bd"] {
            assert_eq!(en(bad), None, "{bad}");
        }
        // The court letters of the round's language: Bube and Dame, valet and roi.
        assert_eq!(parse_card("bd", Lang::De), card(2, 10));
        assert_eq!(parse_card("dh", Lang::De), card(1, 11));
        assert_eq!(parse_card("rs", Lang::Fr), card(0, 12));
    }

    #[test]
    fn times_and_dates_read_as_people_write_them() {
        assert_eq!(clock(181_000), "3:01");
        assert_eq!(clock(59_999), "0:59");
        let date = Date::parse("2026-10-28").unwrap();
        assert_eq!(
            (weekday(date), german(date)),
            ("Mittwoch", "28.10.2026".into())
        );
        assert_eq!(place(&["Tür".into(), String::new()], 0), "Ort 1 · Tür");
        assert_eq!(place(&["Tür".into(), String::new()], 1), "Ort 2");
        assert_eq!(place(&[], 4), "Ort 5");
        let spec = Spec::standard(Discipline::Numbers, Level::new(2).unwrap());
        assert_eq!(size(&spec), "40 Ziffern");
    }
}
