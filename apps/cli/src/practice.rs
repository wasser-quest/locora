//! Practising in the terminal: walk the route, recall each locus, grade it.

use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use locora_api::Service;
use locora_api::core::{Grade, JourneyCompleted, JourneyStarted};
use locora_api::journey::{AnswerKind, Evaluate, Submission};
use locora_formula::workbook::{ActionOutcome, Run};
use locora_store::JourneyAnswer;
use locora_store::{Locus, PalaceTree};

use crate::app::{App, now};
use crate::palace::{count_due, schedule};

pub struct Options {
    /// Only loci that are due (plus up to `new` never-answered ones).
    pub due_only: bool,
    pub new: usize,
    /// `new` comes from the daily limit (not from an explicit `--new`).
    pub daily_limit: bool,
    pub modes: Modes,
}

/// How the route is asked.
#[derive(clap::Args, Debug, Default, Clone, Copy)]
pub struct Modes {
    /// Rückwärts: Inhalt zeigen, Ort erinnern (immer als Erinnern bewertet)
    #[arg(long)]
    pub reverse: bool,
    /// Nur die Ortsnummer zeigen („Ort 7 · Bibliothek“) statt des Ortsnamens
    #[arg(long)]
    pub numbers: bool,
    /// In zufälliger Reihenfolge abfragen
    #[arg(long)]
    pub shuffle: bool,
}

/// One planned question: palace index, palace, locus, route number, room name.
struct Entry<'a> {
    index: usize,
    tree: &'a PalaceTree,
    locus: &'a Locus,
    number: usize,
    room: &'a str,
}

/// What one question shows and reveals, depending on the modes.
struct View {
    reverse: bool,
    /// „Ort 7 · Bibliothek“ in numbers mode.
    label: Option<String>,
}

impl View {
    fn question(&self, run: &Run, locus: &Locus) -> String {
        match (&self.label, self.reverse) {
            (_, true) => display(run, locus, true),
            (Some(label), false) => label.clone(),
            (None, false) => display(run, locus, false),
        }
    }

    fn solution(&self, run: &Run, locus: &Locus) -> String {
        match (&self.label, self.reverse) {
            (Some(label), true) => format!("{label}: {}", display(run, locus, false)),
            (None, true) => display(run, locus, false),
            (Some(_), false) => format!(
                "{} → {}",
                display(run, locus, false),
                display(run, locus, true)
            ),
            (None, false) => display(run, locus, true),
        }
    }

    /// Reverse questions are always recalled, whatever the locus asks for.
    fn kind(&self, locus: &Locus) -> AnswerKind {
        if self.reverse {
            AnswerKind::Recall
        } else {
            locus.answer.kind
        }
    }
}

/// The memory aid, shown once the answer is visible.
fn print_note(locus: &Locus) {
    if !locus.note.is_empty() {
        println!("        ✎ {}", locus.note);
    }
}

enum Step {
    Answered(Submission),
    Quit,
}

fn read_line(input: &mut impl BufRead) -> Result<Option<String>> {
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    Ok(Some(line.trim().to_owned()))
}

use locora_runtime::journey::resolve_text;

fn display(run: &Run, locus: &Locus, reveal: bool) -> String {
    resolve_text(run, locus, reveal).unwrap_or_else(|error| error)
}

fn command(run: &mut Run, line: &str, reveal: bool) -> Option<bool> {
    if line == "?" {
        let actions = run.actions();
        if actions.is_empty() {
            println!("Keine Aktionen vorhanden.");
        } else {
            println!(
                "Aktionen: {}",
                actions
                    .iter()
                    .map(|a| format!("!{a}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        return Some(reveal);
    }
    let key = line.strip_prefix('!')?;
    match run.execute(key, reveal) {
        Ok(ActionOutcome::Reveal) => Some(true),
        Ok(ActionOutcome::Set | ActionOutcome::Reset) => Some(reveal),
        Err(err) => {
            println!("⚠ Aktion fehlgeschlagen: {err}");
            Some(reveal)
        }
    }
}

fn ask(
    input: &mut impl BufRead,
    n: usize,
    total: usize,
    locus: &Locus,
    run: &mut Run,
    shown: &[u32],
    view: &View,
) -> Result<Step> {
    if view.kind(locus) != AnswerKind::Recall {
        return ask_automatic(input, n, total, locus, run, shown, view);
    }
    let mut revealed = false;
    loop {
        if revealed {
            break;
        }
        print!(
            "\n[{n}/{total}] {}   (Enter = aufdecken, q = beenden) ",
            view.question(run, locus)
        );
        io::stdout().flush()?;
        match read_line(input)?.as_deref() {
            None | Some("q" | "Q") => return Ok(Step::Quit),
            Some(line) => match command(run, line, false) {
                Some(next) => revealed = next,
                None => revealed = true,
            },
        }
    }
    println!("        → {}", view.solution(run, locus));
    print_note(locus);
    loop {
        print!("        1 Nochmal · 2 Schwer · 3 Gut · 4 Leicht  (Enter = Gut) ");
        io::stdout().flush()?;
        let grade = match read_line(input)?.as_deref() {
            None | Some("q" | "Q") => return Ok(Step::Quit),
            Some("1") => Grade::Again,
            Some("2") => Grade::Hard,
            Some("3") | Some("") => Grade::Good,
            Some("4") => Grade::Easy,
            Some(line) => {
                if command(run, line, true).is_some() {
                    println!("        → {}", view.solution(run, locus));
                }
                continue;
            }
        };
        return Ok(Step::Answered(Submission::Recall(grade)));
    }
}

fn ask_automatic(
    input: &mut impl BufRead,
    n: usize,
    total: usize,
    locus: &Locus,
    run: &mut Run,
    shown: &[u32],
    view: &View,
) -> Result<Step> {
    loop {
        println!("\n[{n}/{total}] {}", view.question(run, locus));
        if locus.answer.kind == AnswerKind::Quiz {
            for (number, index) in shown.iter().enumerate() {
                println!(
                    "        {} {}",
                    number + 1,
                    locus.answer.options[*index as usize].text
                );
            }
            print!("Auswahl (z. B. 1,3; alle richtigen wählen)");
        } else {
            print!("Antwort");
        }
        if locus.answer.kind == AnswerKind::Guess {
            print!(" (=Text für wörtliche Antworten wie q, ? oder !name)");
        }
        print!(" (Enter = aufdecken/Nochmal, q = beenden): ");
        io::stdout().flush()?;
        let Some(line) = read_line(input)? else {
            return Ok(Step::Quit);
        };
        if locus.answer.kind == AnswerKind::Guess
            && let Some(text) = line.strip_prefix('=')
        {
            if !text.trim().is_empty() {
                return Ok(Step::Answered(Submission::Guess(text.to_owned())));
            }
            println!("⚠ Bitte eine nichtleere Antwort eingeben.");
            continue;
        }
        if line.eq_ignore_ascii_case("q") {
            return Ok(Step::Quit);
        }
        if line.is_empty() {
            return Ok(Step::Answered(Submission::Forfeit));
        }
        if let Some(reveal) = command(run, &line, false) {
            if reveal {
                return Ok(Step::Answered(Submission::Forfeit));
            }
            continue;
        }
        if locus.answer.kind == AnswerKind::Guess {
            return Ok(Step::Answered(Submission::Guess(line)));
        }
        let selected: Option<Vec<u32>> = line
            .split(',')
            .map(|part| {
                let number: u32 = part.trim().parse().ok()?;
                (number >= 1).then(|| shown.get(number as usize - 1).copied())?
            })
            .collect();
        if let Some(selected) = selected {
            let unique: std::collections::HashSet<_> = selected.iter().collect();
            if unique.len() == selected.len() {
                return Ok(Step::Answered(Submission::Quiz(selected)));
            }
        }
        println!("⚠ Bitte gültige Optionsnummern ohne Duplikate eingeben.");
    }
}

/// Keep workbook actions on the revealed locus; none of them evaluates it again.
/// With no authored actions, automatic questions keep their single-submit flow.
fn after_automatic(
    input: &mut impl BufRead,
    locus: &Locus,
    run: &mut Run,
    view: &View,
) -> Result<bool> {
    if run.actions().is_empty() {
        return Ok(true);
    }
    loop {
        print!("        Enter = weiter · ?/!name = Aktion · q = beenden: ");
        io::stdout().flush()?;
        match read_line(input)?.as_deref() {
            None | Some("q" | "Q") => return Ok(false),
            Some("") => return Ok(true),
            Some(line) if command(run, line, true).is_some() => {
                println!("        → {}", view.solution(run, locus));
            }
            Some(_) => println!("        Bereits bewertet; Enter zum Weitergehen."),
        }
    }
}

/// Practise one palace (`palace` = Some) or, in due mode, every palace.
pub fn run(app: &mut App, palace: Option<&str>, options: Options) -> Result<()> {
    if !app.host.plugin_provides("core", Evaluate::NAME) {
        bail!(
            "Das Core-Plugin fehlt. CLI mit wasm32-wasip2 und ohne LOCORA_SKIP_PLUGINS neu bauen oder ein passendes Core-Plugin in <Datenordner>/plugins ablegen."
        );
    }
    let trees: Vec<PalaceTree> = match palace {
        Some(query) => vec![app.store.palace(query)?],
        None => app
            .store
            .palaces()?
            .into_iter()
            .map(|p| app.store.palace(&p.id))
            .collect::<Result<_, _>>()?,
    };
    let mut input = io::stdin().lock();
    let mut runs = Vec::new();
    for tree in &trees {
        let mut run = Run::new(app.store.effective_rows(&tree.palace.id)?)?;
        for spec in run.inputs()? {
            loop {
                print!(
                    "{} · Eingabe {} [{}] (Enter = Standard): ",
                    tree.palace.name, spec.key, spec.default
                );
                io::stdout().flush()?;
                match read_line(&mut input)? {
                    None => {
                        println!("\nÜbung beendet.");
                        return Ok(());
                    }
                    Some(text) if text.is_empty() => break,
                    Some(text) => match run.set_input(&spec.key, &text) {
                        Ok(()) => break,
                        Err(err) => println!("⚠ Ungültige Eingabe: {err}"),
                    },
                }
            }
        }
        runs.push(run);
    }
    let cards = schedule(
        app,
        palace
            .and_then(|_| trees.first())
            .map(|t| t.palace.id.as_str()),
    );
    let now_at_start = now();
    let seed = locora_runtime::now();
    let planned = locora_runtime::journey::plan(
        &trees,
        &runs,
        &cards,
        now_at_start,
        &locora_runtime::journey::PlanOptions {
            due_only: options.due_only,
            new: options.new,
            shuffle: options.modes.shuffle,
            seed,
        },
    );
    for warning in &planned.warnings {
        println!("{warning}");
    }
    let plan: Vec<Entry> = planned
        .stops
        .iter()
        .map(|stop| Entry {
            index: stop.palace,
            tree: stop.tree(&trees),
            locus: stop.locus(&trees),
            number: stop.number,
            room: stop.room_name(&trees),
        })
        .collect();
    if plan.is_empty() {
        if trees.iter().all(|t| t.route().next().is_none()) {
            bail!("Keine Loci zum Üben. Hinzufügen: locora add <Palast> <Inhalt>");
        }
        let (due, fresh): (usize, usize) = trees
            .iter()
            .map(|t| count_due(t, &cards, now_at_start))
            .fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
        if options.due_only && due == 0 && fresh > 0 && options.new == 0 && options.daily_limit {
            println!(
                "Nichts fällig. Das Tageslimit für neue Loci ist erreicht ({fresh} warten).\n  Mehr heute: locora review --new 5 · dauerhaft: locora settings new-per-day 20"
            );
        } else if options.due_only && due == 0 && fresh > 0 && options.new == 0 {
            println!("Nichts fällig; {fresh} neue Loci warten (mit --new 0 ausgeschlossen).");
        } else if options.due_only && due == 0 {
            println!("Nichts fällig. Alles üben: locora practice <Palast>");
        } else {
            println!(
                "Keine aktiven Loci ausgewählt ({due} fällig, {fresh} neu). Alles üben: locora practice <Palast>"
            );
        }
        return Ok(());
    }

    let mut palace_ids = Vec::new();
    for entry in &plan {
        let id = &entry.tree.palace.id;
        if !palace_ids.contains(id) {
            palace_ids.push(id.clone());
        }
    }
    app.publish(&JourneyStarted { palace_ids })?;

    let title = match palace {
        Some(_) => trees[0].palace.name.clone(),
        None => "alle Paläste".into(),
    };
    let count = plan.len();
    println!(
        "▶ {title}: {count} {}",
        if count == 1 { "Locus" } else { "Loci" }
    );
    println!("? = Aktionen anzeigen · !name = Aktion ausführen · q = beenden");
    // Time spent per palace: only the time at its own loci, so a review across
    // several palaces does not report the whole session to each of them.
    let mut spent: HashMap<String, Duration> = HashMap::new();
    let mut results: Vec<(String, bool)> = Vec::new();
    // Graded answers per palace with their time from the start, for rankings.
    let session_start = Instant::now();
    let mut transcripts: HashMap<String, Vec<JourneyAnswer>> = HashMap::new();
    let mut points = 0u64;
    let total = plan.len();
    let recall = locora_api::journey::Answer::default();
    for (i, entry) in plan.iter().enumerate() {
        let Entry {
            index,
            tree,
            locus,
            number,
            room,
        } = entry;
        let view = View {
            reverse: options.modes.reverse,
            label: options.modes.numbers.then(|| {
                if trees.len() > 1 {
                    format!("{} · Ort {number} · {room}", tree.palace.name)
                } else {
                    format!("Ort {number} · {room}")
                }
            }),
        };
        let kind = view.kind(locus);
        let asked = Instant::now();
        let track = |spent: &mut HashMap<String, Duration>| {
            *spent.entry(tree.palace.id.clone()).or_default() += asked.elapsed();
        };
        let shown = locus.answer.quiz_selection(seed ^ i as u64);
        match ask(
            &mut input,
            i + 1,
            total,
            locus,
            &mut runs[*index],
            &shown,
            &view,
        )? {
            Step::Quit => break,
            Step::Answered(submission) => {
                let elapsed_ms = asked.elapsed().as_millis().min(u64::MAX as u128) as u64;
                let expected = if kind == AnswerKind::Guess && submission != Submission::Forfeit {
                    match resolve_text(&runs[*index], locus, true) {
                        Ok(text) if !text.trim().is_empty() => text,
                        Ok(_) => {
                            println!("⚠ Leere erwartete Antwort – nicht bewertet.");
                            track(&mut spent);
                            continue;
                        }
                        Err(error) => {
                            println!("{error} – nicht bewertet.");
                            track(&mut spent);
                            continue;
                        }
                    }
                } else {
                    String::new()
                };
                let evaluation = app.answer(locora_runtime::AnswerRequest {
                    palace_id: &tree.palace.id,
                    locus_id: &locus.id,
                    answer: if view.reverse { &recall } else { &locus.answer },
                    expected: &expected,
                    submission,
                    elapsed_ms,
                })?;
                points = points.saturating_add(evaluation.points);
                results.push((tree.palace.id.clone(), evaluation.correct));
                transcripts
                    .entry(tree.palace.id.clone())
                    .or_default()
                    .push(JourneyAnswer {
                        correct: evaluation.correct,
                        at_ms: session_start.elapsed().as_millis() as u64,
                    });
                if kind != AnswerKind::Recall {
                    println!(
                        "        {} → {}",
                        if evaluation.correct {
                            "Richtig"
                        } else {
                            "Nochmal"
                        },
                        view.solution(&runs[*index], locus)
                    );
                    if locus.answer.kind == AnswerKind::Quiz {
                        let correct: Vec<_> = shown
                            .iter()
                            .enumerate()
                            .map(|(number, index)| (number, &locus.answer.options[*index as usize]))
                            .filter(|(_, option)| option.correct)
                            .map(|(number, option)| format!("{} {}", number + 1, option.text))
                            .collect();
                        println!("        Richtige Auswahl: {}", correct.join(", "));
                    }
                    print_note(locus);
                    if !after_automatic(&mut input, locus, &mut runs[*index], &view)? {
                        track(&mut spent);
                        break;
                    }
                }
                track(&mut spent);
            }
        }
    }
    for tree in &trees {
        let mine: Vec<bool> = results
            .iter()
            .filter(|(id, _)| *id == tree.palace.id)
            .map(|(_, ok)| *ok)
            .collect();
        let planned = plan
            .iter()
            .filter(|entry| entry.tree.palace.id == tree.palace.id)
            .count();
        if mine.is_empty() || mine.len() != planned {
            continue;
        }
        app.publish(&JourneyCompleted {
            palace_id: tree.palace.id.clone(),
            correct: mine.iter().filter(|ok| **ok).count() as u32,
            total: mine.len() as u32,
            duration_secs: spent
                .get(&tree.palace.id)
                .map_or(0, |d| d.as_secs_f64().round() as u64),
        })?;
        if let Some(answers) = transcripts.get(&tree.palace.id) {
            app.store.record_journey(&tree.palace.id, answers)?;
        }
    }
    let answered = results.len();
    let correct = results.iter().filter(|(_, ok)| *ok).count();
    println!("\n✓ {answered} von {total} geübt, {correct} gewusst. {points} Punkte.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_core_is_reported_before_any_input() {
        let mut app = App::for_tests();
        let error = run(
            &mut app,
            None,
            Options {
                due_only: false,
                new: 10,
                daily_limit: false,
                modes: Modes::default(),
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("Core-Plugin fehlt"));
    }
}
