//! Practising in the terminal: walk the route, recall each locus, grade it.

use std::io::{self, BufRead, Write};
use std::time::Instant;

use anyhow::{Result, bail};
use locora_api::core::{Grade, JourneyCompleted, LocusAnswered};
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
    Graded(Grade),
    Quit,
}

fn read_line(input: &mut impl BufRead) -> Result<Option<String>> {
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    Ok(Some(line.trim().to_owned()))
}

fn display(run: &Run, locus: &Locus, reveal: bool) -> String {
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
                resolved.item
            } else {
                resolved.place
            }
        }
        Err(err) => format!("⚠ Formel für {} fehlerhaft: {err}", field.name()),
    }
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
        return Ok(Step::Graded(grade));
    }
}

/// Practise one palace (`palace` = Some) or, in due mode, every palace.
pub fn run(app: &mut App, palace: Option<&str>, options: Options) -> Result<()> {
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
    let started = Instant::now();
    let mut results: Vec<(String, bool)> = Vec::new();
    let total = plan.len();
    for (i, (index, tree, locus)) in plan.iter().enumerate() {
        let asked = Instant::now();
        match ask(&mut input, i + 1, total, locus, &mut runs[*index])? {
            Step::Quit => break,
            Step::Graded(grade) => {
                results.push((tree.palace.id.clone(), grade != Grade::Again));
                app.publish(&LocusAnswered {
                    palace_id: tree.palace.id.clone(),
                    locus_id: locus.id.clone(),
                    correct: grade != Grade::Again,
                    grade,
                    elapsed_ms: asked.elapsed().as_millis() as u64,
                    answered_at: now(),
                })?;
            }
        }
    }
    for tree in &trees {
        let mine: Vec<bool> = results
            .iter()
            .filter(|(id, _)| *id == tree.palace.id)
            .map(|(_, ok)| *ok)
            .collect();
        if mine.is_empty() {
            continue;
        }
        app.publish(&JourneyCompleted {
            palace_id: tree.palace.id.clone(),
            correct: mine.iter().filter(|ok| **ok).count() as u32,
            total: mine.len() as u32,
            duration_secs: started.elapsed().as_secs(),
        })?;
    }
    let answered = results.len();
    let correct = results.iter().filter(|(_, ok)| *ok).count();
    println!("\n✓ {answered} von {total} geübt, {correct} gewusst.");
    Ok(())
}
