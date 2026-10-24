//! Unusually fast rounds. Locora Cloud keeps a round out of the ranking for
//! everyone when it scores at least half the points after less memorizing time
//! than the floor per item allows. It is a plausibility line, not proof of
//! cheating: the round still counts for the person and among friends.

use crate::{Discipline, Spec};

/// Shortest plausible memorizing time per item, in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Floors {
    pub card_ms: u32,
    pub digit_ms: u32,
    pub item_ms: u32,
}

impl Default for Floors {
    fn default() -> Self {
        Self {
            card_ms: 200,
            digit_ms: 100,
            item_ms: 400,
        }
    }
}

impl Floors {
    /// The floor per item of a discipline.
    pub fn per_item(&self, discipline: Discipline) -> u32 {
        match discipline {
            Discipline::List => self.item_ms,
            Discipline::Numbers => self.digit_ms,
            Discipline::Cards => self.card_ms,
        }
    }
}

/// True when `points * 2 >= max` and `memorize_ms < floor_per_item * items`.
pub fn unusual(spec: &Spec, points: u16, memorize_ms: u32, floors: &Floors) -> bool {
    let max = u32::from(spec.items);
    let floor = u64::from(floors.per_item(spec.discipline)) * u64::from(spec.items);
    u32::from(points) * 2 >= max && u64::from(memorize_ms) < floor
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Level;

    #[test]
    fn unusual_at_the_boundaries() {
        let f = Floors::default();
        // 26 cards: the floor is 26 × 200 ms = 5.2 s, half the points are 13.
        let cards = Spec::standard(Discipline::Cards, Level::new(2).unwrap());
        assert!(unusual(&cards, 13, 5_199, &f));
        assert!(!unusual(&cards, 13, 5_200, &f));
        assert!(!unusual(&cards, 12, 5_199, &f));
        assert!(unusual(&cards, 26, 0, &f));
        // 40 digits: 4 s.
        let numbers = Spec::standard(Discipline::Numbers, Level::new(2).unwrap());
        assert!(unusual(&numbers, 20, 3_999, &f));
        assert!(!unusual(&numbers, 20, 4_000, &f));
        assert!(!unusual(&numbers, 19, 1_000, &f));
        // 10 things: 4 s; 5 of 10 is exactly half.
        let list = Spec::standard(Discipline::List, Level::new(1).unwrap());
        assert!(unusual(&list, 5, 3_999, &f));
        assert!(!unusual(&list, 4, 3_999, &f));
        assert!(!unusual(&list, 10, 4_000, &f));
        let strict = Floors {
            item_ms: 1_000,
            ..f
        };
        assert!(unusual(&list, 10, 9_999, &strict));
    }
}
