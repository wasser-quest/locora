//! FSRS (Free Spaced Repetition Scheduler) for Locora.
//!
//! Implements the FSRS-5 memory model with its published default parameters:
//! every locus has a *stability* (days until recall probability falls to 90 %)
//! and a *difficulty* (1–10). Each answer updates both and schedules the next
//! review so that recall probability stays at the desired retention.

use locora_api::core::Grade;
use locora_api::review::CardState;

const DECAY: f64 = -0.5;
/// Chosen so that retrievability is exactly 90 % after `stability` days.
const FACTOR: f64 = 19.0 / 81.0;
const DAY: u64 = 86_400;
/// After a lapse the locus comes back after this many seconds.
const RELEARN_SECS: u64 = 10 * 60;

/// FSRS-5 default weights.
pub const DEFAULT_WEIGHTS: [f64; 19] = [
    0.40255, 1.18385, 3.173, 15.69105, 7.1949, 0.5345, 1.4604, 0.0046, 1.54575, 0.1192, 1.01925,
    1.9395, 0.11, 0.29605, 2.2698, 0.2315, 2.9898, 0.51655, 0.6621,
];

/// Scheduler settings.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    pub weights: [f64; 19],
    /// Target probability of remembering at review time (0.7–0.99).
    pub desired_retention: f64,
    pub maximum_interval_days: f64,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            weights: DEFAULT_WEIGHTS,
            desired_retention: 0.9,
            maximum_interval_days: 36_500.0,
        }
    }
}

fn rating(grade: Grade) -> f64 {
    match grade {
        Grade::Again => 1.0,
        Grade::Hard => 2.0,
        Grade::Good => 3.0,
        Grade::Easy => 4.0,
    }
}

/// Probability of recalling a card `elapsed_days` after its last review.
pub fn retrievability(stability: f64, elapsed_days: f64) -> f64 {
    (1.0 + FACTOR * elapsed_days.max(0.0) / stability.max(0.01)).powf(DECAY)
}

/// Probability of recalling `card` at `now` (seconds since the epoch).
pub fn recall_probability(card: &CardState, now: u64) -> f64 {
    retrievability(
        card.stability,
        now.saturating_sub(card.last_review) as f64 / DAY as f64,
    )
}

impl Params {
    fn w(&self, i: usize) -> f64 {
        self.weights[i]
    }

    fn initial_stability(&self, g: f64) -> f64 {
        self.w(g as usize - 1).max(0.01)
    }

    fn initial_difficulty(&self, g: f64) -> f64 {
        self.w(4) - (self.w(5) * (g - 1.0)).exp() + 1.0
    }

    fn next_difficulty(&self, d: f64, g: f64) -> f64 {
        let delta = -self.w(6) * (g - 3.0);
        let damped = d + delta * (10.0 - d) / 9.0;
        // Mean reversion towards the difficulty of a first "Easy".
        let reverted = self.w(7) * self.initial_difficulty(4.0) + (1.0 - self.w(7)) * damped;
        reverted.clamp(1.0, 10.0)
    }

    fn recall_stability(&self, d: f64, s: f64, r: f64, g: f64) -> f64 {
        let hard = if g == 2.0 { self.w(15) } else { 1.0 };
        let easy = if g == 4.0 { self.w(16) } else { 1.0 };
        s * (self.w(8).exp()
            * (11.0 - d)
            * s.powf(-self.w(9))
            * ((self.w(10) * (1.0 - r)).exp() - 1.0)
            * hard
            * easy
            + 1.0)
    }

    fn forget_stability(&self, d: f64, s: f64, r: f64) -> f64 {
        let next = self.w(11)
            * d.powf(-self.w(12))
            * ((s + 1.0).powf(self.w(13)) - 1.0)
            * (self.w(14) * (1.0 - r)).exp();
        next.min(s)
    }

    fn short_term_stability(&self, s: f64, g: f64) -> f64 {
        s * (self.w(17) * (g - 3.0 + self.w(18))).exp()
    }

    /// Days until recall probability falls to the desired retention.
    pub fn interval_days(&self, stability: f64) -> f64 {
        let days = stability / FACTOR * (self.desired_retention.powf(1.0 / DECAY) - 1.0);
        days.round().clamp(1.0, self.maximum_interval_days)
    }

    /// Apply one answer at `now` (seconds since the epoch) and return the new state.
    /// `card` is `None` for a locus that was never answered.
    pub fn review(&self, card: Option<&CardState>, grade: Grade, now: u64) -> CardState {
        let g = rating(grade);
        let (stability, difficulty, reps, lapses) = match card {
            None => (
                self.initial_stability(g),
                self.initial_difficulty(g).clamp(1.0, 10.0),
                1,
                0,
            ),
            Some(c) => {
                let elapsed = now.saturating_sub(c.last_review) as f64 / DAY as f64;
                let r = retrievability(c.stability, elapsed);
                let s = if elapsed < 1.0 {
                    self.short_term_stability(c.stability, g)
                } else if grade == Grade::Again {
                    self.forget_stability(c.difficulty, c.stability, r)
                } else {
                    self.recall_stability(c.difficulty, c.stability, r, g)
                };
                let lapses = c.lapses + u32::from(grade == Grade::Again);
                (
                    s.max(0.01),
                    self.next_difficulty(c.difficulty, g),
                    c.reps + 1,
                    lapses,
                )
            }
        };
        let due = if grade == Grade::Again {
            now + RELEARN_SECS
        } else {
            now + self.interval_days(stability) as u64 * DAY
        };
        CardState {
            stability,
            difficulty,
            last_review: now,
            due,
            reps,
            lapses,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: u64 = 1_800_000_000;

    #[test]
    fn retention_is_ninety_percent_after_stability_days() {
        assert!((retrievability(5.0, 5.0) - 0.9).abs() < 1e-9);
        assert!((retrievability(5.0, 0.0) - 1.0).abs() < 1e-12);
        assert!(retrievability(5.0, 30.0) < 0.9);
    }

    #[test]
    fn first_answers_use_the_initial_weights() {
        let p = Params::default();
        let good = p.review(None, Grade::Good, T0);
        assert!((good.stability - 3.173).abs() < 1e-9);
        assert_eq!(good.due, T0 + 3 * DAY);
        assert_eq!((good.reps, good.lapses), (1, 0));
        let again = p.review(None, Grade::Again, T0);
        assert_eq!(again.due, T0 + RELEARN_SECS);
        let easy = p.review(None, Grade::Easy, T0);
        assert!(easy.due > good.due);
        assert!(easy.difficulty < good.difficulty);
    }

    #[test]
    fn successful_reviews_grow_the_interval() {
        let p = Params::default();
        let mut card = p.review(None, Grade::Good, T0);
        let mut last_interval = card.due - card.last_review;
        for _ in 0..5 {
            let next = p.review(Some(&card), Grade::Good, card.due);
            let interval = next.due - next.last_review;
            assert!(interval > last_interval, "{interval} <= {last_interval}");
            last_interval = interval;
            card = next;
        }
        assert_eq!(card.reps, 6);
    }

    #[test]
    fn lapses_shrink_stability_and_count() {
        let p = Params::default();
        let card = p.review(None, Grade::Good, T0);
        let card = p.review(Some(&card), Grade::Good, card.due);
        let lapse = p.review(Some(&card), Grade::Again, card.due);
        assert!(lapse.stability < card.stability);
        assert!(lapse.difficulty > card.difficulty);
        assert_eq!(lapse.lapses, 1);
        assert_eq!(lapse.due, card.due + RELEARN_SECS);
    }

    #[test]
    fn difficulty_stays_in_range() {
        let p = Params::default();
        let mut card = p.review(None, Grade::Again, T0);
        for i in 0..50 {
            card = p.review(Some(&card), Grade::Again, T0 + i * DAY);
            assert!((1.0..=10.0).contains(&card.difficulty));
        }
        for i in 0..50 {
            card = p.review(Some(&card), Grade::Easy, T0 + (60 + i) * DAY);
            assert!((1.0..=10.0).contains(&card.difficulty));
        }
    }
}
