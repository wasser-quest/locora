//! Practising in the terminal: walk the route, recall each locus, grade it.

use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use locora_api::Service;
use locora_api::core::{Grade, JourneyCompleted, JourneyScore, LocusAnswered, Score};
use locora_api::journey::{AnswerKind, Evaluate, EvaluateRequest, Scored, Submission};
use locora_formula::workbook::{ActionOutcome, BindingField, LocusInput, Run};
use locora_store::{Locus, PalaceTree};

use crate::app::{App, now};
use crate::palace::{count_due, schedule};

pub struct Options {
    /// Only loci that are due (plus up to `new` never-answered ones).
    pub due_only: bool,
    pub new: usize,
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

fn resolve_text(run: &Run, locus: &Locus, reveal: bool) -> Result<String, String> {
    // Resolve only the displayed field: a hidden answer must not leak or fail early.
    let field = if reveal {
        BindingField::Item
    } else {
        BindingField::Place
    };
    let bindings: Vec<_> = locus
        .bindings
        .iter()
        .filter(|b| b.field == field)
        .cloned()
        .collect();
    match run.resolve(
        &LocusInput {
            place: &locus.place,
            item: &locus.item,
            x: locus.x,
            y: locus.y,
            bindings: &bindings,
        },
        reveal,
    ) {
        Ok(resolved) => {
            if reveal {
                Ok(resolved.item)
            } else {
                Ok(resolved.place)
            }
        }
        Err(err) => Err(format!("⚠ Formel für {} fehlerhaft: {err}", field.name())),
    }
}

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
) -> Result<Step> {
    if locus.answer.kind != AnswerKind::Recall {
        return ask_automatic(input, n, total, locus, run);
    }
    let mut revealed = false;
    loop {
        if revealed {
            break;
        }
        print!(
            "\n[{n}/{total}] {}   (Enter = aufdecken, q = beenden) ",
            display(run, locus, false)
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
    println!("        → {}", display(run, locus, true));
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
                    println!("        → {}", display(run, locus, true));
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
) -> Result<Step> {
    loop {
        println!("\n[{n}/{total}] {}", display(run, locus, false));
        if locus.answer.kind == AnswerKind::Quiz {
            for (index, option) in locus.answer.options.iter().enumerate() {
                println!("        {} {}", index + 1, option.text);
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
                (number >= 1 && number as usize <= locus.answer.options.len()).then(|| number - 1)
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
fn after_automatic(input: &mut impl BufRead, locus: &Locus, run: &mut Run) -> Result<bool> {
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
                println!("        → {}", display(run, locus, true));
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
    let mut plan: Vec<(usize, &PalaceTree, &Locus)> = Vec::new();
    let mut new_left = options.new;
    for (index, tree) in trees.iter().enumerate() {
        for locus in tree.route() {
            match runs[index].active(&locus.bindings) {
                Ok(false) => continue,
                Ok(true) => {}
                Err(err) => println!(
                    "⚠ {} · {}: active-Formel fehlerhaft: {err}",
                    tree.palace.name, locus.place
                ),
            }
            let include = if !options.due_only {
                true
            } else {
                match cards.get(&locus.id) {
                    Some(card) => card.due <= now_at_start,
                    None if new_left > 0 => {
                        new_left -= 1;
                        true
                    }
                    None => false,
                }
            };
            if include {
                plan.push((index, tree, locus));
            }
        }
    }
    if plan.is_empty() {
        if trees.iter().all(|t| t.route().next().is_none()) {
            bail!("Keine Loci zum Üben. Hinzufügen: locora add <Palast> <Inhalt>");
        }
        let (due, fresh): (usize, usize) = trees
            .iter()
            .map(|t| count_due(t, &cards, now_at_start))
            .fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
        println!(
            "Keine aktiven Loci ausgewählt ({due} fällig, {fresh} neu). Alles üben: locora practice <Palast>"
        );
        return Ok(());
    }

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
    let mut points = 0u64;
    let total = plan.len();
    for (i, (index, tree, locus)) in plan.iter().enumerate() {
        let asked = Instant::now();
        let track = |spent: &mut HashMap<String, Duration>| {
            *spent.entry(tree.palace.id.clone()).or_default() += asked.elapsed();
        };
        match ask(&mut input, i + 1, total, locus, &mut runs[*index])? {
            Step::Quit => break,
            Step::Answered(submission) => {
                let elapsed_ms = asked.elapsed().as_millis().min(u64::MAX as u128) as u64;
                let expected = if locus.answer.kind == AnswerKind::Guess
                    && submission != Submission::Forfeit
                {
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
                let evaluation = app.host.call_plugin::<Evaluate>(
                    "core",
                    &EvaluateRequest {
                        answer: locus.answer.clone(),
                        expected,
                        submission,
                        elapsed_ms,
                    },
                )?;
                app.report();
                let score = app.host.filter::<JourneyScore>(Score {
                    palace_id: tree.palace.id.clone(),
                    locus_id: locus.id.clone(),
                    points: evaluation.points,
                })?;
                app.report();
                points = points.saturating_add(score.points);
                results.push((tree.palace.id.clone(), evaluation.correct));
                app.publish(&LocusAnswered {
                    palace_id: tree.palace.id.clone(),
                    locus_id: locus.id.clone(),
                    correct: evaluation.correct,
                    grade: evaluation.grade,
                    elapsed_ms,
                    answered_at: now(),
                })?;
                // Preserve trusted route identity even if a points filter changes ids.
                app.publish(&Scored {
                    palace_id: tree.palace.id.clone(),
                    locus_id: locus.id.clone(),
                    points: score.points,
                })?;
                if locus.answer.kind != AnswerKind::Recall {
                    println!(
                        "        {} → {}",
                        if evaluation.correct {
                            "Richtig"
                        } else {
                            "Nochmal"
                        },
                        display(&runs[*index], locus, true)
                    );
                    if locus.answer.kind == AnswerKind::Quiz {
                        let correct: Vec<_> = locus
                            .answer
                            .options
                            .iter()
                            .enumerate()
                            .filter(|(_, option)| option.correct)
                            .map(|(index, option)| format!("{} {}", index + 1, option.text))
                            .collect();
                        println!("        Richtige Auswahl: {}", correct.join(", "));
                    }
                    if !after_automatic(&mut input, locus, &mut runs[*index])? {
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
            .filter(|(_, t, _)| t.palace.id == tree.palace.id)
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
        let mut app = App {
            store: std::sync::Arc::new(locora_store::Store::in_memory().unwrap()),
            host: locora_host::PluginHost::new(None).unwrap(),
        };
        let error = run(
            &mut app,
            None,
            Options {
                due_only: false,
                new: 10,
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("Core-Plugin fehlt"));
    }
}
