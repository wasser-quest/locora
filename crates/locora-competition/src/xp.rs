//! XP for taking part (rules v1; addendum D6). The daily challenge earns XP by
//! level, the evening recall a little, practice and challenge codes nothing.
//!
//! XP never depends on the points: writing things down cannot buy XP, and a bad
//! round costs nothing. A round earns XP only when at least one place was
//! answered, as opening the app is not a learning day. Gamification adds it
//! once per learning day and level, so a daily played on two devices counts
//! once. A day brings at most 60: 10 + 15 + 20 for the three levels and 5 for
//! each of their evening recalls.

use crate::{Level, Recall};

/// XP of the daily challenge (local or cloud) at a level: 10, 15 or 20.
pub fn daily(level: Level) -> u32 {
    match level.get() {
        1 => 10,
        2 => 15,
        _ => 20,
    }
}

/// XP of an evening recall.
pub const EVENING: u32 = 5;

/// Whether a handed-in round earns XP at all: at least one place answered.
pub fn earns(recall: &Recall) -> bool {
    recall.answered() > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Discipline;

    #[test]
    fn xp_follows_the_level_not_the_points() {
        let xp: Vec<u32> = Level::ALL.into_iter().map(daily).collect();
        assert_eq!(xp, [10, 15, 20]);
        assert_eq!(EVENING, 5);
        // The most a learning day can bring: every level and every evening recall.
        let most: u32 = Level::ALL.into_iter().map(|l| daily(l) + EVENING).sum();
        assert_eq!(most, 60);
    }

    #[test]
    fn an_empty_hand_in_earns_nothing() {
        assert!(!earns(&Recall::blank(Discipline::Cards, 13)));
        assert!(!earns(&Recall::List(vec![" ".into(), String::new()])));
        assert!(earns(&Recall::List(vec![String::new(), "Milch".into()])));
        assert!(earns(&Recall::Numbers(vec![None, Some(0)])));
        assert!(earns(&Recall::Cards(vec![Some(51), None])));
    }
}
