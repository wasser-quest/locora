//! Personal FSRS weights from the review history.
//!
//! Replays every locus's answers with candidate weights and minimises the log
//! loss of the predicted recall probability against what actually happened
//! (Again = forgotten). Only answers given at least one day after the previous
//! one count, like the long-term part of FSRS. A small penalty keeps weights
//! close to the defaults, so little data cannot push them to extremes.

use locora_api::core::Grade;

#[cfg(test)]
use crate::DEFAULT_WEIGHTS;
use crate::{DAY, Params, retrievability};

/// Lower and upper bound for every weight (as in the FSRS reference optimiser).
const BOUNDS: [(f64, f64); 19] = [
    (0.001, 100.0),
    (0.001, 100.0),
    (0.001, 100.0),
    (0.001, 100.0),
    (1.0, 10.0),
    (0.001, 4.0),
    (0.001, 4.0),
    (0.001, 0.75),
    (0.0, 4.5),
    (0.0, 0.8),
    (0.001, 3.5),
    (0.001, 5.0),
    (0.001, 0.25),
    (0.001, 0.9),
    (0.0, 4.0),
    (0.0, 1.0),
    (1.0, 6.0),
    (0.0, 2.0),
    (0.0, 2.0),
];

/// Answers needed before optimising makes sense.
pub const MIN_REVIEWS: usize = 100;

/// One locus's answers in time order: (seconds since the epoch, grade).
pub type History = Vec<(u64, Grade)>;

#[derive(Debug, Clone, PartialEq)]
pub struct Fitted {
    pub weights: [f64; 19],
    /// Mean log loss with the starting weights.
    pub loss_before: f64,
    /// Mean log loss with the fitted weights.
    pub loss_after: f64,
    /// Answers the fit was based on.
    pub reviews: usize,
}

/// Number of answers that count for the fit (at least a day after the previous one).
pub fn usable_reviews(histories: &[History]) -> usize {
    histories
        .iter()
        .map(|h| h.windows(2).filter(|w| w[1].0 >= w[0].0 + DAY).count())
        .sum()
}

fn log_loss(weights: &[f64; 19], histories: &[History]) -> (f64, usize) {
    let params = Params {
        weights: *weights,
        ..Params::default()
    };
    let mut total = 0.0;
    let mut n = 0;
    for history in histories {
        let mut card = None;
        for &(at, grade) in history {
            if let Some(c) = &card {
                let c: &locora_api::review::CardState = c;
                if at >= c.last_review + DAY {
                    let elapsed = (at - c.last_review) as f64 / DAY as f64;
                    let r = retrievability(c.stability, elapsed).clamp(1e-6, 1.0 - 1e-6);
                    let recalled = grade != Grade::Again;
                    total -= if recalled { r.ln() } else { (1.0 - r).ln() };
                    n += 1;
                }
            }
            card = Some(params.review(card.as_ref(), grade, at));
        }
    }
    (if n == 0 { 0.0 } else { total / n as f64 }, n)
}

fn objective(weights: &[f64; 19], start: &[f64; 19], histories: &[History]) -> f64 {
    let (loss, _) = log_loss(weights, histories);
    let penalty: f64 = weights
        .iter()
        .zip(start)
        .map(|(w, s)| ((w - s) / s.abs().max(0.1)).powi(2))
        .sum();
    loss + 0.002 * penalty
}

/// Fit weights to the histories, starting from `start` (usually the current
/// weights). Returns `None` with fewer than [`MIN_REVIEWS`] usable answers.
/// The result never has a higher training loss than the start.
pub fn optimize(histories: &[History], start: &[f64; 19]) -> Option<Fitted> {
    let (loss_before, reviews) = log_loss(start, histories);
    if reviews < MIN_REVIEWS {
        return None;
    }
    // Adam on finite-difference gradients, in units of each weight's range.
    let scale: Vec<f64> = BOUNDS.iter().map(|(lo, hi)| (hi - lo).min(10.0)).collect();
    let mut w = *start;
    let (mut m, mut v) = ([0.0; 19], [0.0; 19]);
    let (lr, b1, b2) = (0.01, 0.9, 0.999);
    let mut best = (objective(&w, start, histories), w);
    for step in 1..=150 {
        let base = objective(&w, start, histories);
        let mut grad = [0.0; 19];
        for i in 0..19 {
            let h = 1e-4 * scale[i];
            let mut probe = w;
            probe[i] = (probe[i] + h).min(BOUNDS[i].1);
            let dh = probe[i] - w[i];
            if dh > 0.0 {
                grad[i] = (objective(&probe, start, histories) - base) / dh * scale[i];
            }
        }
        for i in 0..19 {
            m[i] = b1 * m[i] + (1.0 - b1) * grad[i];
            v[i] = b2 * v[i] + (1.0 - b2) * grad[i] * grad[i];
            let m_hat = m[i] / (1.0 - b1.powi(step));
            let v_hat = v[i] / (1.0 - b2.powi(step));
            w[i] = (w[i] - lr * scale[i] * m_hat / (v_hat.sqrt() + 1e-8))
                .clamp(BOUNDS[i].0, BOUNDS[i].1);
        }
        let current = objective(&w, start, histories);
        if current < best.0 {
            best = (current, w);
        }
    }
    let (loss_after, _) = log_loss(&best.1, histories);
    let weights = if loss_after <= loss_before {
        best.1
    } else {
        *start
    };
    Some(Fitted {
        weights,
        loss_before,
        loss_after: loss_after.min(loss_before),
        reviews,
    })
}

/// Whether stored weights are usable (finite and within bounds).
pub fn valid_weights(weights: &[f64]) -> bool {
    weights.len() == 19
        && weights
            .iter()
            .zip(BOUNDS)
            .all(|(w, (lo, hi))| w.is_finite() && (lo..=hi).contains(w))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pseudo-random numbers in 0..1.
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (self.0 >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    /// Simulate a learner whose memory is described by `truth`, reviewed on
    /// the schedule produced by the default weights.
    fn simulate(truth: &[f64; 19], loci: usize, seed: u64) -> Vec<History> {
        let real = Params {
            weights: *truth,
            ..Params::default()
        };
        let schedule = Params::default();
        let mut rng = Lcg(seed);
        let mut out = Vec::new();
        for l in 0..loci {
            let mut at = 1_800_000_000 + l as u64 * 600;
            let mut history = vec![(at, Grade::Good)];
            let mut real_card = real.review(None, Grade::Good, at);
            let mut sched_card = schedule.review(None, Grade::Good, at);
            for _ in 0..6 {
                at = sched_card.due;
                let elapsed = (at - real_card.last_review) as f64 / DAY as f64;
                let recalled = rng.next() < retrievability(real_card.stability, elapsed);
                let grade = if recalled { Grade::Good } else { Grade::Again };
                history.push((at, grade));
                real_card = real.review(Some(&real_card), grade, at);
                sched_card = schedule.review(Some(&sched_card), grade, at);
            }
            out.push(history);
        }
        out
    }

    #[test]
    fn needs_enough_answers() {
        let few = simulate(&DEFAULT_WEIGHTS, 5, 1);
        assert!(usable_reviews(&few) < MIN_REVIEWS);
        assert!(optimize(&few, &DEFAULT_WEIGHTS).is_none());
    }

    #[test]
    fn fits_a_learner_who_forgets_faster() {
        // This learner's memories are only half as stable as the defaults assume.
        let mut truth = DEFAULT_WEIGHTS;
        for w in truth.iter_mut().take(4) {
            *w *= 0.4;
        }
        truth[8] -= 0.7;
        let histories = simulate(&truth, 120, 7);
        let fitted = optimize(&histories, &DEFAULT_WEIGHTS).expect("enough data");
        assert!(fitted.reviews >= MIN_REVIEWS);
        assert!(
            fitted.loss_after < fitted.loss_before * 0.97,
            "{} -> {}",
            fitted.loss_before,
            fitted.loss_after
        );
        assert!(valid_weights(&fitted.weights));
        // A held-out set of the same learner also gets better predictions.
        let held_out = simulate(&truth, 80, 99);
        assert!(log_loss(&fitted.weights, &held_out).0 < log_loss(&DEFAULT_WEIGHTS, &held_out).0);
    }

    #[test]
    fn weights_are_checked() {
        assert!(valid_weights(&DEFAULT_WEIGHTS));
        let mut bad = DEFAULT_WEIGHTS;
        bad[4] = 11.0;
        assert!(!valid_weights(&bad));
        assert!(!valid_weights(&DEFAULT_WEIGHTS[..18]));
        bad[4] = f64::NAN;
        assert!(!valid_weights(&bad));
    }
}
