//! Numbers: uniform random digits in rows of ten, recalled row by row.
//!
//! The championship rule scaled to a phone (spec §3.3): a full row scores 10,
//! a row with one error 5, more errors 0. The row with the last digit entered
//! counts only up to that digit, so stopping early costs nothing extra and
//! guessing the rest is worthless.

use crate::rng::SplitMix64;
use crate::{Info, Mark, Score};

/// Digits in a row.
pub const ROW: usize = 10;

pub(crate) fn generate(rng: &mut SplitMix64, items: u8) -> Vec<u8> {
    (0..items).map(|_| rng.below(10) as u8).collect()
}

/// Scores digits row by row. A blank (`None`) before the last digit entered is
/// an error like a wrong digit.
pub(crate) fn score(task: &[u8], recall: &[Option<u8>]) -> Score {
    let marks: Vec<Mark> = task
        .iter()
        .zip(recall)
        .map(|(&digit, answer)| match *answer {
            None => Mark::Empty,
            Some(a) if a == digit => Mark::Right,
            Some(_) => Mark::Wrong,
        })
        .collect();
    // Cells up to and including the last digit entered; nothing after it counts.
    let counted = recall
        .iter()
        .rposition(Option::is_some)
        .map_or(0, |last| (last + 1).min(marks.len()));
    let points: usize = marks[..counted]
        .chunks(ROW)
        .map(|row| {
            let errors = row.iter().filter(|&&m| m != Mark::Right).count();
            // Rows before the last one are full rows of ten, so this is 10, 5 or 0 for them.
            match errors {
                0 => row.len(),
                1 => row.len() / 2,
                _ => 0,
            }
        })
        .sum();
    let digits_right = marks.iter().filter(|&&m| m == Mark::Right).count() as u16;
    crate::finish(
        points as u16,
        marks,
        Vec::new(),
        Info {
            digits_right: Some(digits_right),
            ..Info::default()
        },
    )
}

/// Reads one row as typed: `0`–`9` (full-width too) are digits, and anything
/// else (`.`, `,`, `-`, `_`, a space, a stray letter) stands for a digit not
/// known. Always [`ROW`] cells: a short row ends in blanks, a long one is cut.
pub fn parse_row(text: &str) -> Vec<Option<u8>> {
    let mut cells: Vec<Option<u8>> = text
        .chars()
        .filter(|c| !matches!(c, '\n' | '\r'))
        .map(|c| match c {
            '0'..='9' => Some(c as u8 - b'0'),
            '０'..='９' => Some((u32::from(c) - u32::from('０')) as u8),
            _ => None,
        })
        .take(ROW)
        .collect();
    cells.resize(ROW, None);
    cells
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One row per string, `.` for a blank.
    fn rows(text: &[&str]) -> Vec<Option<u8>> {
        text.iter().flat_map(|row| parse_row(row)).collect()
    }

    fn digits(text: &str) -> Vec<u8> {
        text.bytes()
            .filter(u8::is_ascii_digit)
            .map(|b| b - b'0')
            .collect()
    }

    const TASK: &str = "0816007591 8455281058 1234567890";

    fn points(recall: &[&str]) -> u16 {
        score(&digits(TASK), &rows(recall)).points
    }

    #[test]
    fn rows_score_ten_five_or_nothing() {
        assert_eq!(points(&["0816007591", "8455281058", "1234567890"]), 30);
        // One error in a full row halves it, two errors cost the row.
        assert_eq!(points(&["0816007511", "8455281058", "1234567890"]), 25);
        assert_eq!(points(&["0816007500", "8455281058", "1234567890"]), 20);
        // A blank inside a row before the last one is an error too.
        assert_eq!(points(&["08160075.1", "8455281058", "1234567890"]), 25);
    }

    #[test]
    fn the_last_row_counts_up_to_the_last_digit() {
        // Full rows 1 and 2, then three digits: 10 + 10 + 3.
        assert_eq!(points(&["0816007591", "8455281058", "123"]), 23);
        // One error among those three: 3 / 2 rounded down.
        assert_eq!(points(&["0816007591", "8455281058", "193"]), 21);
        // Two errors among them: nothing for that row.
        assert_eq!(points(&["0816007591", "8455281058", "993"]), 20);
        // Blanks after the last digit do not count, blanks before it do.
        assert_eq!(points(&["0816007591", "8455281058", "12...."]), 22);
        assert_eq!(points(&["0816007591", "8455281058", "1.3"]), 21);
    }

    #[test]
    fn an_empty_row_in_the_middle_costs_that_row() {
        assert_eq!(points(&["0816007591", "", "1234567890"]), 20);
        assert_eq!(points(&["0816007591", "", ""]), 10);
        assert_eq!(points(&["", "", ""]), 0);
        let empty = score(&digits(TASK), &rows(&["", "", ""]));
        assert_eq!(empty.marks, vec![Mark::Empty; 30]);
        assert_eq!(empty.info.digits_right, Some(0));
    }

    #[test]
    fn marks_and_digits_right() {
        let s = score(&digits(TASK), &rows(&["0816007511", "84552810", ""]));
        assert_eq!(s.points, 5 + 8);
        assert_eq!((s.max, s.perfect), (30, false));
        assert_eq!(s.info.digits_right, Some(17));
        assert_eq!(s.marks[8], Mark::Wrong);
        assert_eq!(s.marks[9], Mark::Right);
        assert_eq!(s.marks[18], Mark::Empty);
        assert!(s.matched.is_empty());
        assert_eq!((s.info.in_order, s.info.first_error), (None, None));
    }

    #[test]
    fn a_row_shorter_than_ten_follows_the_same_rule() {
        // A test spec of 15 digits: the second row has five cells.
        let task = digits("0816007591 84552");
        assert_eq!(
            score(&task, &rows(&["0816007591", "84552"])[..15]).points,
            15
        );
        assert_eq!(
            score(&task, &rows(&["0816007591", "84562"])[..15]).points,
            12
        );
    }

    #[test]
    fn rows_are_parsed_as_typed() {
        assert_eq!(
            parse_row("3.1,4-1_5 9"),
            [
                Some(3),
                None,
                Some(1),
                None,
                Some(4),
                None,
                Some(1),
                None,
                Some(5),
                None
            ]
        );
        assert_eq!(parse_row(""), [None; ROW]);
        assert_eq!(parse_row("12\r\n")[..3], [Some(1), Some(2), None]);
        assert_eq!(parse_row("１２x3")[..4], [Some(1), Some(2), None, Some(3)]);
        assert_eq!(parse_row("123456789012"), rows(&["1234567890"]));
    }

    #[test]
    fn generated_digits_are_in_range() {
        let mut rng = SplitMix64::new(crate::Seed::masked(9));
        let digits = generate(&mut rng, 200);
        assert_eq!(digits.len(), 200);
        assert!(digits.iter().all(|&d| d < 10));
        for d in 0..10 {
            assert!(digits.contains(&d));
        }
    }
}
