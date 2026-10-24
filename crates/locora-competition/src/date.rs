//! Civil dates (`YYYY-MM-DD`) in the proleptic Gregorian calendar, for daily
//! seeds and the weekly schedule. The learning day that a date stands for is
//! the runtime's business (`Profile::learning_day`); this crate only needs the
//! label and the day count.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// A calendar date. Build it with [`Date::new`] or [`Date::parse`], which refuse
/// dates that do not exist; the fields are private, so every `Date` is a real
/// day. In JSON and postcard it is the string `"YYYY-MM-DD"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Date {
    year: i32,
    month: u8,
    day: u8,
}

const MIN_YEAR: i32 = 0;
const MAX_YEAR: i32 = 9_999;

fn leap(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        2 if leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

impl Date {
    /// The date, or `None` if it does not exist or its year is outside 0000–9999.
    pub fn new(year: i32, month: u8, day: u8) -> Option<Date> {
        let valid = (MIN_YEAR..=MAX_YEAR).contains(&year)
            && (1..=12).contains(&month)
            && day >= 1
            && day <= days_in_month(year, month);
        valid.then_some(Date { year, month, day })
    }

    /// 0 to 9999.
    pub fn year(self) -> i32 {
        self.year
    }

    /// 1 to 12.
    pub fn month(self) -> u8 {
        self.month
    }

    /// 1 to the last day of the month.
    pub fn day(self) -> u8 {
        self.day
    }

    /// Exactly `YYYY-MM-DD` with ASCII digits.
    pub fn parse(text: &str) -> Option<Date> {
        let bytes = text.as_bytes();
        if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
            return None;
        }
        let number = |range: std::ops::Range<usize>| -> Option<u32> {
            bytes[range].iter().try_fold(0u32, |acc, &b| {
                b.is_ascii_digit().then(|| acc * 10 + u32::from(b - b'0'))
            })
        };
        Date::new(
            number(0..4)? as i32,
            number(5..7)? as u8,
            number(8..10)? as u8,
        )
    }

    /// Days since 1970-01-01 (negative before it). Howard Hinnant's algorithm.
    pub fn days_since_epoch(self) -> i64 {
        let month = i64::from(self.month);
        let year = i64::from(self.year) - i64::from(month <= 2);
        let era = year.div_euclid(400);
        let yoe = year - era * 400;
        let doy =
            (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(self.day) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// The date `days` after 1970-01-01, clamped to 0000-01-01 … 9999-12-31.
    pub fn from_days(days: i64) -> Date {
        let first = Date {
            year: MIN_YEAR,
            month: 1,
            day: 1,
        }
        .days_since_epoch();
        let last = Date {
            year: MAX_YEAR,
            month: 12,
            day: 31,
        }
        .days_since_epoch();
        let z = days.clamp(first, last) + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        Date {
            year: year as i32,
            month: month as u8,
            day: day as u8,
        }
    }

    /// The date `days` later (earlier for a negative count).
    pub fn add_days(self, days: i64) -> Date {
        Date::from_days(self.days_since_epoch().saturating_add(days))
    }

    /// 0 for Monday … 6 for Sunday.
    pub fn weekday(self) -> u8 {
        (self.days_since_epoch() + 3).rem_euclid(7) as u8
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl Serialize for Date {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Date {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Date, D::Error> {
        struct Visitor;
        impl serde::de::Visitor<'_> for Visitor {
            type Value = Date;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a date as YYYY-MM-DD")
            }
            fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<Date, E> {
                Date::parse(text)
                    .ok_or_else(|| E::custom(format_args!("kein gültiges Datum: {text}")))
            }
        }
        deserializer.deserialize_str(Visitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(text: &str) -> Date {
        Date::parse(text).unwrap()
    }

    #[test]
    fn parse_and_format_round_trip() {
        for text in [
            "1970-01-01",
            "2026-10-23",
            "2000-02-29",
            "0000-01-01",
            "9999-12-31",
        ] {
            assert_eq!(date(text).to_string(), text);
        }
        assert_eq!(
            date("2026-10-23"),
            Date {
                year: 2026,
                month: 10,
                day: 23
            }
        );
        let d = date("2024-02-29");
        assert_eq!((d.year(), d.month(), d.day()), (2024, 2, 29));
        assert_eq!(Date::new(2024, 2, 29), Some(d));
    }

    #[test]
    fn dates_that_do_not_exist_are_refused() {
        for text in [
            "2026-02-30",
            "2026-02-29",
            "1900-02-29",
            "2026-13-01",
            "2026-00-10",
            "2026-04-31",
            "2026-10-00",
            "2026-1-01",
            "2026-10-1",
            "26-10-01",
            "2026/10/01",
            "2026-10-01 ",
            "+026-10-01",
            "２０２６-10-01",
            "",
        ] {
            assert_eq!(Date::parse(text), None, "{text}");
        }
        assert_eq!(Date::new(10_000, 1, 1), None);
        assert_eq!(Date::new(-1, 1, 1), None);
    }

    #[test]
    fn leap_years_follow_the_gregorian_rule() {
        assert!(Date::new(2024, 2, 29).is_some());
        assert!(Date::new(2000, 2, 29).is_some());
        assert!(Date::new(2100, 2, 29).is_none());
        assert_eq!(date("2024-02-28").add_days(1), date("2024-02-29"));
        assert_eq!(date("2023-02-28").add_days(1), date("2023-03-01"));
        assert_eq!(date("2024-12-31").add_days(1), date("2025-01-01"));
    }

    #[test]
    fn days_and_weekdays() {
        assert_eq!(date("1970-01-01").days_since_epoch(), 0);
        assert_eq!(date("1970-01-01").weekday(), 3); // a Thursday
        assert_eq!(date("1969-12-31").days_since_epoch(), -1);
        assert_eq!(date("1969-12-29").weekday(), 0); // a Monday before the epoch
        assert_eq!(date("2026-10-23").days_since_epoch(), 20_749);
        assert_eq!(date("2026-10-23").weekday(), 4); // a Friday
        for days in [-719_528, -1, 0, 1, 20_749, 2_932_896] {
            assert_eq!(Date::from_days(days).days_since_epoch(), days);
        }
        assert_eq!(Date::from_days(i64::MIN), date("0000-01-01"));
        assert_eq!(Date::from_days(i64::MAX), date("9999-12-31"));
        assert_eq!(date("2026-10-23").add_days(-23), date("2026-09-30"));
    }

    #[test]
    fn serde_uses_the_text_form() {
        let d = date("2026-10-28");
        assert_eq!(serde_json::to_string(&d).unwrap(), "\"2026-10-28\"");
        assert_eq!(serde_json::from_str::<Date>("\"2026-10-28\"").unwrap(), d);
        assert!(serde_json::from_str::<Date>("\"2026-02-30\"").is_err());
        assert!(serde_json::from_str::<Date>("20261028").is_err());
        let bytes = postcard::to_allocvec(&d).unwrap();
        assert_eq!(postcard::from_bytes::<Date>(&bytes).unwrap(), d);
    }
}
