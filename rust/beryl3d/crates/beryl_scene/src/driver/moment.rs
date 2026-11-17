//! Durations and moments, written one way in every command, test and drive script
//! (docs/beryl-3d.md, "Times, moments, locators and actions").

use std::fmt;
use std::str::FromStr;

const NANOS_PER_MILLISECOND: u64 = 1_000_000;

/// A length of time: a number and a unit, `850ms` or `1.25s`. Printed in milliseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Duration(pub u64);

impl Duration {
    #[must_use]
    pub const fn from_millis(milliseconds: u64) -> Self {
        Self(milliseconds * NANOS_PER_MILLISECOND)
    }

    /// The length in nanoseconds.
    #[must_use]
    pub const fn nanos(self) -> u64 {
        self.0
    }
}

impl fmt::Display for Duration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let whole = self.0 / NANOS_PER_MILLISECOND;
        let part = self.0 % NANOS_PER_MILLISECOND;
        if part == 0 {
            write!(f, "{whole}ms")
        } else {
            let fraction = format!("{part:06}");
            write!(f, "{whole}.{}ms", fraction.trim_end_matches('0'))
        }
    }
}

/// Why a duration or a moment could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseError {}

impl FromStr for Duration {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<Self, ParseError> {
        let expected = || {
            ParseError(format!(
                "'{text}' is not a duration: write a number and a unit, ms or s, as in 850ms or \
                 1.25s"
            ))
        };
        let (number, scale) = if let Some(number) = text.strip_suffix("ms") {
            (number, 1e6)
        } else if let Some(number) = text.strip_suffix('s') {
            (number, 1e9)
        } else {
            return Err(expected());
        };
        if number.is_empty() || !number.chars().all(|c| c.is_ascii_digit() || c == '.') {
            return Err(expected());
        }
        let value: f64 = number.parse().map_err(|_| expected())?;
        let nanos = (value * scale).round();
        if !nanos.is_finite() || nanos > u64::MAX as f64 {
            return Err(expected());
        }
        #[expect(clippy::cast_sign_loss, reason = "the number has no sign")]
        Ok(Self(nanos as u64))
    }
}

/// A point in a recording: a duration since its start, or a name `info` prints, with an
/// optional offset (`view:12+250ms`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Moment {
    At(Duration),
    Named { name: Name, offset: Offset },
}

/// A named moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Name {
    Start,
    End,
    /// The first frame in which the view, counted from 1, is still.
    View(u32),
    /// The first frame in which the prompt, counted from 1, is open and still.
    Prompt(u32),
    /// The middle of a step: the view counted from 1, then the beat and the step counted from
    /// 0 (`step:12.0.1`).
    Step(u32, u32, u32),
}

/// An offset after (or, negative, before) a named moment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Offset {
    pub before: bool,
    pub length: Duration,
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start => f.write_str("start"),
            Self::End => f.write_str("end"),
            Self::View(view) => write!(f, "view:{view}"),
            Self::Prompt(prompt) => write!(f, "prompt:{prompt}"),
            Self::Step(view, beat, step) => write!(f, "step:{view}.{beat}.{step}"),
        }
    }
}

impl fmt::Display for Moment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::At(duration) => write!(f, "{duration}"),
            Self::Named { name, offset } => {
                write!(f, "{name}")?;
                if offset.length.0 > 0 {
                    let sign = if offset.before { '-' } else { '+' };
                    write!(f, "{sign}{}", offset.length)?;
                }
                Ok(())
            }
        }
    }
}

impl FromStr for Moment {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<Self, ParseError> {
        let expected = |why: &str| {
            ParseError(format!(
                "'{text}' is not a moment: {why}. A moment is a duration (850ms, 1.25s) or a \
                 name: start, end, view:12, prompt:1 or step:12.0.1, optionally followed by an \
                 offset, as in view:12+250ms"
            ))
        };
        if text.starts_with(|c: char| c.is_ascii_digit()) {
            return text.parse().map(Self::At);
        }
        let (head, offset) = match text.find(['+', '-']) {
            None => (text, Offset::default()),
            Some(at) => {
                let length = text[at + 1..]
                    .parse()
                    .map_err(|ParseError(why)| expected(&why))?;
                let offset = Offset {
                    before: text.as_bytes()[at] == b'-',
                    length,
                };
                (&text[..at], offset)
            }
        };
        let number = |digits: &str| {
            digits
                .parse::<u32>()
                .map_err(|_| expected(&format!("'{digits}' is not a whole number")))
        };
        let name = match head.split_once(':') {
            None if head == "start" => Name::Start,
            None if head == "end" => Name::End,
            Some(("view", n)) => Name::View(
                positive(number(n)?).ok_or_else(|| expected("views are counted from 1"))?,
            ),
            Some(("prompt", n)) => Name::Prompt(
                positive(number(n)?).ok_or_else(|| expected("prompts are counted from 1"))?,
            ),
            Some(("step", path)) => {
                let parts: Vec<&str> = path.split('.').collect();
                let [view, beat, step] = parts.as_slice() else {
                    return Err(expected(
                        "a step is a view, a beat and a step, as in step:12.0.1",
                    ));
                };
                let view =
                    positive(number(view)?).ok_or_else(|| expected("views are counted from 1"))?;
                Name::Step(view, number(beat)?, number(step)?)
            }
            _ => return Err(expected(&format!("'{head}' is not a moment's name"))),
        };
        Ok(Self::Named { name, offset })
    }
}

fn positive(number: u32) -> Option<u32> {
    (number > 0).then_some(number)
}

/// How far apart two moments' names are: whether they are of different kinds, then how far
/// apart their numbers are, the view first.
pub(super) fn moment_distance(a: Name, b: Name) -> (bool, u32, u32, u32) {
    match (a, b) {
        (Name::Step(v, e, s), Name::Step(w, f, t)) => {
            (false, v.abs_diff(w), e.abs_diff(f), s.abs_diff(t))
        }
        (Name::View(v), Name::View(w)) | (Name::Prompt(v), Name::Prompt(w)) => {
            (false, v.abs_diff(w), 0, 0)
        }
        _ => (
            std::mem::discriminant(&a) != std::mem::discriminant(&b),
            0,
            0,
            0,
        ),
    }
}

/// `time` moved by `offset`, in nanoseconds.
pub(super) fn offset_from(time: u64, offset: Offset) -> u64 {
    if offset.before {
        time.saturating_sub(offset.length.0)
    } else {
        time + offset.length.0
    }
}
