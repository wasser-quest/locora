//! Planning a journey (which loci, in which order) and resolving what a
//! question shows. Shared by the CLI and the app; asking and grading stay
//! with the client, evaluation goes through [`crate::Profile::answer`].

use std::collections::HashMap;

use locora_api::review::CardState;
use locora_formula::workbook::{BindingField, LocusInput, Run};
use locora_store::{Locus, PalaceTree};

/// Which loci a journey asks.
#[derive(Debug, Clone, Default)]
pub struct PlanOptions {
    /// Only loci that are due (plus up to `new` never-answered ones).
    pub due_only: bool,
    pub new: usize,
    /// Random order instead of the route.
    pub shuffle: bool,
    /// Seed for `shuffle` (e.g. the current time).
    pub seed: u64,
}

/// One planned question: indices into the palace trees plus the route number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stop {
    pub palace: usize,
    pub room: usize,
    pub locus: usize,
    /// Position on the palace's route, counted from 1 across rooms.
    pub number: usize,
}

impl Stop {
    pub fn tree<'a>(&self, trees: &'a [PalaceTree]) -> &'a PalaceTree {
        &trees[self.palace]
    }

    pub fn room_name<'a>(&self, trees: &'a [PalaceTree]) -> &'a str {
        &trees[self.palace].rooms[self.room].room.name
    }

    pub fn locus<'a>(&self, trees: &'a [PalaceTree]) -> &'a Locus {
        &trees[self.palace].rooms[self.room].loci[self.locus]
    }
}

#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub stops: Vec<Stop>,
    /// Problems worth telling (broken `active` formulas; those loci stay in).
    pub warnings: Vec<String>,
}

/// Plan a journey over `trees` (one run per tree, same order).
pub fn plan(
    trees: &[PalaceTree],
    runs: &[Run],
    cards: &HashMap<String, CardState>,
    now: u64,
    options: &PlanOptions,
) -> Plan {
    let mut out = Plan::default();
    let mut new_left = options.new;
    for (p, tree) in trees.iter().enumerate() {
        let mut number = 0;
        for (r, room) in tree.rooms.iter().enumerate() {
            for (l, locus) in room.loci.iter().enumerate() {
                number += 1;
                match runs[p].active(&locus.bindings) {
                    Ok(false) => continue,
                    Ok(true) => {}
                    Err(err) => out.warnings.push(format!(
                        "⚠ {} · {}: active-Formel fehlerhaft: {err}",
                        tree.palace.name, locus.place
                    )),
                }
                let include = !options.due_only
                    || match cards.get(&locus.id) {
                        Some(card) => card.due <= now,
                        None if new_left > 0 => {
                            new_left -= 1;
                            true
                        }
                        None => false,
                    };
                if include {
                    out.stops.push(Stop {
                        palace: p,
                        room: r,
                        locus: l,
                        number,
                    });
                }
            }
        }
    }
    if options.shuffle {
        locora_api::journey::shuffle(&mut out.stops, options.seed);
    }
    out
}

/// The place (`reveal = false`) or item (`reveal = true`) of a locus after its
/// formulas. Only the shown field is resolved, so a hidden answer can neither
/// leak nor fail early. The error is a message for the learner.
pub fn resolve_text(run: &Run, locus: &Locus, reveal: bool) -> Result<String, String> {
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
    let input = LocusInput {
        place: &locus.place,
        item: &locus.item,
        x: locus.x,
        y: locus.y,
        bindings: &bindings,
    };
    match run.resolve(&input, reveal) {
        Ok(resolved) if reveal => Ok(resolved.item),
        Ok(resolved) => Ok(resolved.place),
        Err(err) => Err(format!("⚠ Formel für {} fehlerhaft: {err}", field.name())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use locora_store::{NewLocus, RoomKind, Store};

    fn palace(store: &Store, name: &str, items: &[&str]) -> PalaceTree {
        let p = store.create_palace(name).unwrap();
        for (i, chunk) in items.chunks(2).enumerate() {
            let room = store
                .add_room(&p.id, &format!("Raum {}", i + 1), RoomKind::Blank, None)
                .unwrap();
            for item in chunk {
                store
                    .add_locus(
                        &room.id,
                        NewLocus {
                            image_size: locora_store::default_image_size(),
                            place: format!("bei {item}"),
                            item: (*item).into(),
                            x: 0.5,
                            y: 0.5,
                            time: None,
                            answer: Default::default(),
                            image: None,
                            note: String::new(),
                        },
                    )
                    .unwrap();
            }
        }
        store.palace(&p.id).unwrap()
    }

    #[test]
    fn plans_route_numbers_due_and_new_limits_and_shuffles() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let trees = vec![
            palace(&store, "A", &["a", "b", "c"]),
            palace(&store, "B", &["d"]),
        ];
        let runs = vec![Run::new(vec![]).unwrap(), Run::new(vec![]).unwrap()];
        let all = plan(&trees, &runs, &HashMap::new(), 100, &PlanOptions::default());
        let items: Vec<_> = all
            .stops
            .iter()
            .map(|s| s.locus(&trees).item.as_str())
            .collect();
        assert_eq!(items, ["a", "b", "c", "d"]);
        assert_eq!(all.stops[2].number, 3, "numbers run across rooms");
        assert_eq!(all.stops[2].room_name(&trees), "Raum 2");
        assert_eq!(all.stops[3].number, 1, "each palace counts on its own");

        let b = &trees[0].rooms[0].loci[1];
        let mut cards = HashMap::new();
        cards.insert(
            b.id.clone(),
            CardState {
                stability: 1.0,
                difficulty: 5.0,
                last_review: 0,
                due: 50,
                reps: 1,
                lapses: 0,
            },
        );
        let due = plan(
            &trees,
            &runs,
            &cards,
            100,
            &PlanOptions {
                due_only: true,
                new: 1,
                ..Default::default()
            },
        );
        let items: Vec<_> = due
            .stops
            .iter()
            .map(|s| s.locus(&trees).item.as_str())
            .collect();
        assert_eq!(items, ["a", "b"], "one new plus the due one");

        let shuffled = plan(
            &trees,
            &runs,
            &HashMap::new(),
            100,
            &PlanOptions {
                shuffle: true,
                seed: 3,
                ..Default::default()
            },
        );
        let mut sorted = shuffled.stops.clone();
        sorted.sort_by_key(|s| (s.palace, s.room, s.locus));
        assert_eq!(sorted, all.stops);
    }
}
