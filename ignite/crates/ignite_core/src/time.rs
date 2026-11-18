//! Timestamps.
//!
//! A [`Timestamp`] is milliseconds since the Unix epoch, in UTC. It is written as RFC 3339 with
//! millisecond precision (`2026-11-17T14:12:00.000Z`), the form the ticket service already
//! stores, so the databases and the protocol hold sortable text. Code never reads the system
//! clock directly; it asks a [`crate::traits::Clock`], so tests can set the time.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::traits::Clock;

/// A moment in UTC, to the millisecond.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Timestamp {
    millis: i64,
}

/// Why a string is not an RFC 3339 UTC timestamp.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimeError(pub String);

impl fmt::Display for TimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid timestamp {:?}: expected YYYY-MM-DDTHH:MM:SS[.fff]Z",
            self.0
        )
    }
}

impl std::error::Error for TimeError {}

const MILLIS_PER_DAY: i64 = 86_400_000;

impl Timestamp {
    pub const EPOCH: Timestamp = Timestamp { millis: 0 };

    pub const fn from_millis(millis: i64) -> Self {
        Self { millis }
    }

    pub const fn millis(self) -> i64 {
        self.millis
    }

    /// This moment moved by `seconds`, which may be negative or fractional.
    #[must_use]
    pub fn plus_seconds(self, seconds: f64) -> Self {
        Self {
            millis: self.millis + (seconds * 1000.0).round() as i64,
        }
    }

    /// Seconds from `earlier` to this moment; negative if `earlier` is later.
    pub fn seconds_since(self, earlier: Timestamp) -> f64 {
        (self.millis - earlier.millis) as f64 / 1000.0
    }

    /// The UTC date, as `YYYY-MM-DD`.
    pub fn day(self) -> String {
        let (year, month, day) = civil_from_days(self.millis.div_euclid(MILLIS_PER_DAY));
        format!("{year:04}-{month:02}-{day:02}")
    }

    /// Parses `YYYY-MM-DDTHH:MM:SS[.fff]Z`.
    pub fn parse(text: &str) -> Result<Self, TimeError> {
        let error = || TimeError(text.to_owned());
        let body = text.strip_suffix('Z').ok_or_else(error)?;
        let (date, clock) = body.split_once('T').ok_or_else(error)?;
        let number = |part: &str, len: usize| -> Result<i64, TimeError> {
            if part.len() != len || !part.bytes().all(|b| b.is_ascii_digit()) {
                return Err(error());
            }
            part.parse().map_err(|_| error())
        };
        let mut date_parts = date.split('-');
        let year = number(date_parts.next().ok_or_else(error)?, 4)?;
        let month = number(date_parts.next().ok_or_else(error)?, 2)?;
        let day = number(date_parts.next().ok_or_else(error)?, 2)?;
        if date_parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return Err(error());
        }
        let (whole, fraction) = clock.split_once('.').unwrap_or((clock, ""));
        let mut clock_parts = whole.split(':');
        let hour = number(clock_parts.next().ok_or_else(error)?, 2)?;
        let minute = number(clock_parts.next().ok_or_else(error)?, 2)?;
        let second = number(clock_parts.next().ok_or_else(error)?, 2)?;
        if clock_parts.next().is_some() || hour > 23 || minute > 59 || second > 60 {
            return Err(error());
        }
        let millis = match fraction.len() {
            0 => 0,
            1..=9 if fraction.bytes().all(|b| b.is_ascii_digit()) => {
                let padded = format!("{fraction:0<3}");
                padded[..3].parse::<i64>().map_err(|_| error())?
            }
            _ => return Err(error()),
        };
        let days = days_from_civil(year, month, day);
        let seconds = days * 86_400 + hour * 3600 + minute * 60 + second;
        Ok(Self {
            millis: seconds * 1000 + millis,
        })
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let days = self.millis.div_euclid(MILLIS_PER_DAY);
        let rest = self.millis.rem_euclid(MILLIS_PER_DAY);
        let (year, month, day) = civil_from_days(days);
        let (hour, minute) = (rest / 3_600_000, rest / 60_000 % 60);
        let (second, millis) = (rest / 1000 % 60, rest % 1000);
        write!(
            f,
            "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z"
        )
    }
}

impl FromStr for Timestamp {
    type Err = TimeError;
    fn from_str(text: &str) -> Result<Self, TimeError> {
        Self::parse(text)
    }
}

impl TryFrom<String> for Timestamp {
    type Error = TimeError;
    fn try_from(text: String) -> Result<Self, TimeError> {
        Self::parse(&text)
    }
}

impl From<Timestamp> for String {
    fn from(time: Timestamp) -> String {
        time.to_string()
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The date of a count of days since 1970-01-01; the inverse of [`days_from_civil`].
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// The real clock, read from the operating system.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let since_epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        Timestamp::from_millis(since_epoch.as_millis() as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_text() {
        for text in [
            "1970-01-01T00:00:00.000Z",
            "2026-11-17T14:12:05.250Z",
            "2000-02-29T23:59:59.999Z",
        ] {
            assert_eq!(Timestamp::parse(text).unwrap().to_string(), text);
        }
    }

    #[test]
    fn accepts_whole_seconds_and_long_fractions() {
        let whole = Timestamp::parse("2026-11-17T14:12:05Z").unwrap();
        assert_eq!(whole.to_string(), "2026-11-17T14:12:05.000Z");
        let long = Timestamp::parse("2026-11-17T14:12:05.123456Z").unwrap();
        assert_eq!(long.millis() - whole.millis(), 123);
    }

    #[test]
    fn refuses_other_forms() {
        for text in [
            "2026-11-17",
            "2026-11-17T14:12:05+01:00",
            "2026-13-01T00:00:00Z",
            "x",
        ] {
            assert!(Timestamp::parse(text).is_err(), "{text}");
        }
    }

    #[test]
    fn arithmetic_and_days() {
        let start = Timestamp::parse("2026-11-17T23:59:30.000Z").unwrap();
        let later = start.plus_seconds(45.0);
        assert!((later.seconds_since(start) - 45.0).abs() < 1e-9);
        assert_eq!(later.day(), "2026-11-18");
        assert_eq!(Timestamp::from_millis(-1).day(), "1969-12-31");
    }
}
