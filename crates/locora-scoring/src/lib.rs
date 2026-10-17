//! Journey scoring: the XP rule and a deterministic replay of a journey.
//!
//! The same code runs in the `gamification` plugin (on the device) and on
//! Locora Cloud, which replays submitted transcripts before it ranks anyone.
//! A replay can only prove that a transcript is *consistent* with the rules;
//! it cannot prove that the learner really knew the answers.

/// Longest transcript accepted by [`replay`]; a journey is a few dozen loci.
pub const MAX_ANSWERS: usize = 500;
/// Shortest plausible time between two answers in milliseconds.
pub const MIN_ANSWER_GAP_MS: u64 = 300;

/// One graded answer of a journey, in the order it was given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Answer {
    pub correct: bool,
    /// Milliseconds since the journey started (or any fixed epoch, as long as it is monotonic).
    pub at_ms: u64,
}

/// The score that a replay derives from a transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Score {
    pub correct: u32,
    pub total: u32,
    /// Longest run of correct answers without a mistake.
    pub max_combo: u32,
    pub xp: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayError {
    Empty,
    TooLong,
    /// Timestamps must not go backwards.
    NotMonotonic {
        index: usize,
    },
    /// Answers closer together than [`MIN_ANSWER_GAP_MS`].
    TooFast {
        index: usize,
    },
}

impl std::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "Die Journey enthält keine Antworten"),
            Self::TooLong => write!(f, "Die Journey hat mehr als {MAX_ANSWERS} Antworten"),
            Self::NotMonotonic { index } => write!(f, "Zeitstempel {index} läuft rückwärts"),
            Self::TooFast { index } => {
                write!(f, "Antwort {index} kam zu schnell nach der vorherigen")
            }
        }
    }
}

impl std::error::Error for ReplayError {}

/// XP for a finished journey. Port of mnemodim/shared/journeyScoring.ts:
/// rounded integer accuracy picks a multiplier of 1, 2 or 3.
pub fn journey_xp(correct: u32, total: u32, max_combo: u32) -> u64 {
    let accuracy = if total == 0 {
        0
    } else {
        (u64::from(correct).saturating_mul(100) + u64::from(total) / 2) / u64::from(total)
    };
    let multiplier = if accuracy >= 90 {
        3
    } else if accuracy >= 70 {
        2
    } else {
        1
    };
    (u64::from(correct)
        .saturating_mul(10)
        .saturating_add(u64::from(max_combo).saturating_mul(5)))
    .saturating_mul(multiplier)
}

/// Replays a transcript: counts the answers, finds the longest combo and
/// computes the XP. Rejects transcripts that break the timing rules.
pub fn replay(answers: &[Answer]) -> Result<Score, ReplayError> {
    if answers.is_empty() {
        return Err(ReplayError::Empty);
    }
    if answers.len() > MAX_ANSWERS {
        return Err(ReplayError::TooLong);
    }
    let mut correct = 0u32;
    let mut combo = 0u32;
    let mut max_combo = 0u32;
    let mut previous: Option<u64> = None;
    for (index, answer) in answers.iter().enumerate() {
        if let Some(before) = previous {
            if answer.at_ms < before {
                return Err(ReplayError::NotMonotonic { index });
            }
            if answer.at_ms - before < MIN_ANSWER_GAP_MS {
                return Err(ReplayError::TooFast { index });
            }
        }
        previous = Some(answer.at_ms);
        if answer.correct {
            correct += 1;
            combo += 1;
            max_combo = max_combo.max(combo);
        } else {
            combo = 0;
        }
    }
    let total = answers.len() as u32;
    Ok(Score {
        correct,
        total,
        max_combo,
        xp: journey_xp(correct, total, max_combo),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(correct: bool, at_ms: u64) -> Answer {
        Answer { correct, at_ms }
    }

    #[test]
    fn xp_follows_the_accuracy_multiplier() {
        assert_eq!(journey_xp(0, 0, 0), 0);
        // 60 % accuracy: multiplier 1.
        assert_eq!(journey_xp(3, 5, 2), 3 * 10 + 2 * 5);
        // 70 % and 90 % thresholds.
        assert_eq!(journey_xp(7, 10, 0), 70 * 2);
        assert_eq!(journey_xp(9, 10, 0), 90 * 3);
        // Rounding: 89.5 % rounds to 90 %.
        assert_eq!(journey_xp(179, 200, 0), 1790 * 3);
    }

    #[test]
    fn replay_counts_combo_and_xp() {
        let answers = [
            answer(true, 1_000),
            answer(true, 2_000),
            answer(false, 3_000),
            answer(true, 4_000),
            answer(true, 5_000),
            answer(true, 6_000),
        ];
        let score = replay(&answers).unwrap();
        assert_eq!(score.correct, 5);
        assert_eq!(score.total, 6);
        assert_eq!(score.max_combo, 3);
        assert_eq!(score.xp, journey_xp(5, 6, 3));
    }

    #[test]
    fn replay_rejects_implausible_transcripts() {
        assert_eq!(replay(&[]), Err(ReplayError::Empty));
        let long = vec![answer(true, 0); MAX_ANSWERS + 1];
        assert_eq!(replay(&long), Err(ReplayError::TooLong));
        let backwards = [answer(true, 5_000), answer(true, 4_000)];
        assert_eq!(
            replay(&backwards),
            Err(ReplayError::NotMonotonic { index: 1 })
        );
        let fast = [answer(true, 5_000), answer(true, 5_100)];
        assert_eq!(replay(&fast), Err(ReplayError::TooFast { index: 1 }));
    }
}
