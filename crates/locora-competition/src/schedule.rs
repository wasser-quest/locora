//! The weekly rhythm of the daily challenge (schedule v1, spec §5.1) and the
//! route a round needs.
//!
//! Fixed weekdays let people prepare: Monday and Thursday a shopping list,
//! Tuesday and Friday numbers, Wednesday and Saturday cards. Sunday is the long
//! day, whose discipline rotates by week.

use crate::{Date, Discipline, Spec};
use serde::{Deserialize, Serialize};

/// The discipline of a day, the same for every level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Daily {
    pub discipline: Discipline,
    /// Sunday: the preparation and the result put the evening recall first.
    pub long_day: bool,
}

/// The daily discipline of `date`. With `d` days since 1970-01-01 (a Thursday),
/// the weekday is `(d + 3) mod 7` (0 Monday) and the week `(d + 3) div 7`; a
/// Sunday takes `[list, numbers, cards][week mod 3]`.
pub fn daily(date: Date) -> Daily {
    let shifted = date.days_since_epoch() + 3;
    let weekday = shifted.rem_euclid(7);
    let week = shifted.div_euclid(7);
    let rotation = [Discipline::List, Discipline::Numbers, Discipline::Cards];
    let discipline = match weekday {
        0 | 3 => Discipline::List,
        1 | 4 => Discipline::Numbers,
        2 | 5 => Discipline::Cards,
        _ => rotation[week.rem_euclid(3) as usize],
    };
    Daily {
        discipline,
        long_day: weekday == 6,
    }
}

/// How many things can go on one place: shopping list and cards 1, 2 or 3,
/// numbers 2, 3, 4 or 6 digits. It changes the grouping on screen, never the task.
pub fn per_place_options(discipline: Discipline) -> &'static [u8] {
    match discipline {
        Discipline::List | Discipline::Cards => &[1, 2, 3],
        Discipline::Numbers => &[2, 3, 4, 6],
    }
}

/// Places needed for `items` things with `per_place` on each place (at least 1 per place).
pub fn places(items: u8, per_place: u8) -> u8 {
    items.div_ceil(per_place.max(1))
}

/// One way to lay a round onto a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaceOption {
    pub per_place: u8,
    pub places: u8,
}

/// The places a round needs, for each "per place" option of its discipline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prep {
    pub options: Vec<PlaceOption>,
}

pub fn prep(spec: &Spec) -> Prep {
    let options = per_place_options(spec.discipline)
        .iter()
        .map(|&per_place| PlaceOption {
            per_place,
            places: places(spec.items, per_place),
        })
        .collect();
    Prep { options }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Level;

    fn day(text: &str) -> Daily {
        daily(Date::parse(text).unwrap())
    }

    #[test]
    fn weekdays_follow_the_rhythm() {
        // 2026-10-26 is a Monday.
        let monday = Date::parse("2026-10-26").unwrap();
        let disciplines: Vec<_> = (0..6).map(|i| daily(monday.add_days(i))).collect();
        assert_eq!(
            disciplines.iter().map(|d| d.discipline).collect::<Vec<_>>(),
            [
                Discipline::List,
                Discipline::Numbers,
                Discipline::Cards,
                Discipline::List,
                Discipline::Numbers,
                Discipline::Cards
            ]
        );
        assert!(disciplines.iter().all(|d| !d.long_day));
    }

    #[test]
    fn sundays_rotate_by_week() {
        let sundays: Vec<_> = ["2026-10-25", "2026-11-01", "2026-11-08", "2026-11-15"]
            .into_iter()
            .map(day)
            .collect();
        assert!(sundays.iter().all(|d| d.long_day));
        assert_eq!(
            sundays.iter().map(|d| d.discipline).collect::<Vec<_>>(),
            [
                Discipline::List,
                Discipline::Numbers,
                Discipline::Cards,
                Discipline::List
            ]
        );
        // Before 1970 the rotation keeps going without a jump.
        assert_eq!(day("1969-12-28").discipline, Discipline::Cards);
        assert_eq!(day("1970-01-04").discipline, Discipline::List);
    }

    #[test]
    fn prep_counts_places_for_every_option() {
        let cards = prep(&Spec::standard(Discipline::Cards, Level::new(2).unwrap()));
        assert_eq!(
            cards.options,
            [
                PlaceOption {
                    per_place: 1,
                    places: 26
                },
                PlaceOption {
                    per_place: 2,
                    places: 13
                },
                PlaceOption {
                    per_place: 3,
                    places: 9
                }
            ]
        );
        let numbers = prep(&Spec::standard(Discipline::Numbers, Level::new(3).unwrap()));
        let places: Vec<_> = numbers
            .options
            .iter()
            .map(|o| (o.per_place, o.places))
            .collect();
        assert_eq!(places, [(2, 40), (3, 27), (4, 20), (6, 14)]);
        let list = prep(&Spec::standard(Discipline::List, Level::new(1).unwrap()));
        let places: Vec<_> = list.options.iter().map(|o| o.places).collect();
        assert_eq!(places, [10, 5, 4]);
        assert_eq!(super::places(13, 0), 13);
    }
}
