//! How fast each machine runs the same work, relative to the fastest, and which machine a ticket
//! that holds `master` back goes to.
//!
//! Seen on 2026-11-27: `master` waited more than eight hours behind a regression pinned to a
//! machine at 99% CPU and swapping, while a machine several times faster had idle workers. A
//! repair of the work that holds `master` back now goes to the fastest machine that can take it.
//! The culprit's machine, which holds its landing's transcript, is preferred only when it is
//! nearly as fast.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A machine's speed relative to the fastest machine, which has 1.0. Stored in hundredths so
/// that settings stay comparable with `Eq`, and written as a decimal number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(into = "f64", try_from = "f64")]
pub struct Speed(u32);

impl Speed {
    pub const ONE: Speed = Speed(100);

    pub fn value(self) -> f64 {
        f64::from(self.0) / 100.0
    }
}

impl From<Speed> for f64 {
    fn from(speed: Speed) -> Self {
        speed.value()
    }
}

impl TryFrom<f64> for Speed {
    type Error = String;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        let hundredths = (value * 100.0).round();
        if !value.is_finite() || hundredths < 1.0 || hundredths > 10_000.0 {
            return Err(format!("speed must be from 0.01 to 100, not {value}"));
        }
        // Checked to be from 1 to 10,000 just above.
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Ok(Speed(hundredths as u32))
    }
}

impl FromStr for Speed {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let value: f64 = text
            .trim()
            .parse()
            .map_err(|_| format!("speed must be a number, not {text:?}"))?;
        Speed::try_from(value)
    }
}

impl fmt::Display for Speed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.2}", self.value())
    }
}

/// How many days of check records the measurement reads.
pub const WINDOW_DAYS: f64 = 7.0;

/// The culprit's machine keeps a repair when its speed is at least this fraction of the fastest
/// eligible machine's, because its transcript saves the repair some reading.
pub const AFFINITY_SPEED_FRACTION: f64 = 0.75;

/// One check command's duration on one machine.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub check: String,
    pub machine: String,
    pub seconds: f64,
}

fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    Some(if values.len().is_multiple_of(2) {
        f64::midpoint(values[mid - 1], values[mid])
    } else {
        values[mid]
    })
}

/// Each machine's speed from `runs`. For every check run on two or more machines, a machine's
/// ratio is its median duration over the median of all the check's runs. A machine's slowness is
/// the median of its ratios, and its speed is the fastest machine's slowness over its own, so
/// the fastest has 1.0. A machine with no shared check has no measurement.
pub fn measure(runs: &[Run]) -> BTreeMap<String, Speed> {
    let mut by_check: BTreeMap<&str, BTreeMap<&str, Vec<f64>>> = BTreeMap::new();
    for run in runs.iter().filter(|run| run.seconds > 0.0) {
        by_check
            .entry(&run.check)
            .or_default()
            .entry(&run.machine)
            .or_default()
            .push(run.seconds);
    }
    let mut ratios: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
    for machines in by_check.values_mut().filter(|machines| machines.len() >= 2) {
        let mut all: Vec<f64> = machines.values().flatten().copied().collect();
        let Some(fleet) = median(&mut all) else {
            continue;
        };
        for (machine, seconds) in machines.iter_mut() {
            if let Some(own) = median(seconds) {
                ratios.entry(machine).or_default().push(own / fleet);
            }
        }
    }
    let slowness: BTreeMap<&str, f64> = ratios
        .into_iter()
        .filter_map(|(machine, mut ratios)| Some((machine, median(&mut ratios)?)))
        .collect();
    let Some(fastest) = slowness.values().copied().min_by(f64::total_cmp) else {
        return BTreeMap::new();
    };
    slowness
        .into_iter()
        .filter_map(|(machine, slow)| {
            Some((machine.to_owned(), Speed::try_from(fastest / slow).ok()?))
        })
        .collect()
}

/// A machine that could take a ticket now: one whose platform fits and that has a free slot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate<'a> {
    pub name: &'a str,
    pub speed: Speed,
}

/// The machine a ticket holding `master` back should go to among `candidates`: the culprit's
/// machine `affinity` when it is a candidate within [`AFFINITY_SPEED_FRACTION`] of the fastest,
/// else the fastest. `asker` wins a tie, so a machine never defers to one no faster than itself.
pub fn preferred<'a>(
    candidates: &[Candidate<'a>],
    asker: &str,
    affinity: Option<&str>,
) -> Option<Candidate<'a>> {
    let fastest = candidates.iter().copied().max_by(|first, second| {
        first
            .speed
            .cmp(&second.speed)
            .then_with(|| second.name.cmp(first.name))
    })?;
    let near = |candidate: &Candidate<'_>| {
        candidate.speed.value() >= AFFINITY_SPEED_FRACTION * fastest.speed.value()
    };
    if let Some(machine) = affinity
        && let Some(kept) = candidates
            .iter()
            .find(|candidate| candidate.name.eq_ignore_ascii_case(machine))
        && near(kept)
    {
        return Some(*kept);
    }
    // The asker is as good as the fastest when it is equally fast.
    if let Some(here) = candidates
        .iter()
        .find(|candidate| candidate.name.eq_ignore_ascii_case(asker))
        && here.speed == fastest.speed
    {
        return Some(*here);
    }
    Some(fastest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(check: &str, machine: &str, seconds: f64) -> Run {
        Run {
            check: check.to_owned(),
            machine: machine.to_owned(),
            seconds,
        }
    }

    fn speed(value: f64) -> Speed {
        Speed::try_from(value).unwrap()
    }

    /// Seeded records where multivac takes 2.5 times as long as jarvis on every shared check.
    /// A check run on one machine only, and a machine with no shared check, measure nothing.
    #[test]
    fn speeds_are_median_ratios_relative_to_the_fastest() {
        let runs = [
            run("rust", "jarvis", 100.0),
            run("rust", "jarvis", 120.0),
            run("rust", "multivac", 250.0),
            run("rust", "multivac", 300.0),
            run("client", "jarvis", 40.0),
            run("client", "multivac", 100.0),
            run("client", "cygnus", 60.0),
            run("windows", "cygnus", 500.0),
            run("solo", "plan9", 10.0),
        ];
        let speeds = measure(&runs);
        assert_eq!(speeds.get("jarvis"), Some(&Speed::ONE));
        assert_eq!(speeds.get("multivac"), Some(&speed(0.4)));
        // Cygnus shares only client, where its ratio is 1.0. Jarvis's slowness is the median
        // of 110/185 on rust and 40/60 on client, about 0.63.
        assert_eq!(speeds.get("cygnus"), Some(&speed(0.63)));
        assert_eq!(speeds.get("plan9"), None);
        assert!(measure(&[run("rust", "jarvis", 1.0)]).is_empty());
    }

    #[test]
    fn a_speed_is_written_and_read_as_a_decimal() {
        assert_eq!("0.4".parse::<Speed>(), Ok(speed(0.4)));
        assert!("0".parse::<Speed>().is_err());
        assert!("fast".parse::<Speed>().is_err());
        assert_eq!(serde_json::to_string(&speed(0.4)).unwrap(), "0.4");
        assert_eq!(speed(0.4).to_string(), "0.40");
    }

    #[test]
    fn the_culprit_machine_keeps_the_ticket_only_when_nearly_as_fast() {
        let candidates = |culprit: f64| {
            [
                Candidate {
                    name: "jarvis",
                    speed: Speed::ONE,
                },
                Candidate {
                    name: "multivac",
                    speed: speed(culprit),
                },
            ]
        };
        let pick = |culprit: f64, asker: &str| {
            preferred(&candidates(culprit), asker, Some("multivac"))
                .unwrap()
                .name
        };
        assert_eq!(pick(0.4, "multivac"), "jarvis");
        assert_eq!(pick(0.8, "jarvis"), "multivac");
        assert_eq!(pick(0.75, "jarvis"), "multivac");
        assert_eq!(pick(0.74, "multivac"), "jarvis");
        // Without a culprit machine, an equally fast asker keeps the ticket.
        let tied = [
            Candidate {
                name: "jarvis",
                speed: Speed::ONE,
            },
            Candidate {
                name: "cygnus",
                speed: Speed::ONE,
            },
        ];
        assert_eq!(preferred(&tied, "jarvis", None).unwrap().name, "jarvis");
        assert_eq!(preferred(&tied, "cygnus", None).unwrap().name, "cygnus");
    }
}
