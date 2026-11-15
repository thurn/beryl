//! Checking a whole recording (docs/beryl-3d.md, "Interface checks" and "Test speed"): every
//! interface check, at the check times or on every tick.
//!
//! A recording is played twice. The first time the client keeps a record of every script, which
//! gives the check times and the segments and steps that the segment checks read. The second
//! time the frame checks run on the frame at each check time, and the subject of each step is
//! sampled for "every step shown". Both plays are the same, because a recording plays on its own
//! clock, whenever its frames are drawn.

use std::collections::BTreeSet;

use beryl_protocol::script::Verb;

use super::{DriverError, Name, Page};
use crate::agent::steps::{self, Found, Sample, card_of, sample, step_nanoseconds};
use crate::agent::tree::locator_of;
use crate::agent::{self, Fault, Subject, hit::bounds};
use crate::frame::Frame;
use crate::input::Rect;
use crate::play::record::{Plant, Planted, ScriptRecord};

/// One step of a script, as `Page::steps` lists it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepMoment {
    /// `step:V.B.S`: the view its script leads to, counted from 1, then its beat and its index
    /// in the beat, counted from 0.
    pub name: Name,
    pub verb: Verb,
    /// The slot of the scene object it moves, if it moves one, which is its card's id in a
    /// frame (`agent::steps::card_of`).
    pub subject: Option<usize>,
    /// When it begins, ends, and is halfway through, in nanoseconds since the recording's start.
    pub from: u64,
    pub to: u64,
    pub middle: u64,
}

/// Which frames `check_all` evaluates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The frames at the check times: where a fault can begin.
    AtCheckTimes,
    /// The frame at every tick of the clock, the exhaustive form the sweep runs.
    EveryTick,
}

/// What checking a whole recording found.
#[derive(Clone, Debug, PartialEq)]
pub struct Checked {
    /// Every fault, each once: a fault a frame check finds again in a later frame, about the same
    /// elements, is not repeated.
    pub faults: Vec<Fault>,
    /// How many frames were drawn: those the frame checks ran on, those in which a step's
    /// subject was only sampled, and those that name the objects of a fault.
    pub frames: usize,
}

/// One step of one script that "every step shown" judges, with what the frames showed of it.
struct Judged {
    script: usize,
    step: usize,
    /// When it plays, in real time, and until (not including) when its subject is sampled.
    from: u64,
    to: u64,
    until: u64,
    samples: Vec<Sample>,
    /// Its subject as the first frame that drew it named it.
    subject: Option<Subject>,
    /// Whether any frame was evaluated while it played.
    evaluated: bool,
}

impl Page {
    /// The check times of the recording as it plays with no action taken, in order, each once,
    /// in nanoseconds since its start: the frame after each input; for each segment its start,
    /// its midpoint and its end, the midpoint being a flight's apex too; each step's start and
    /// end; and the first still frame after each script, which is its end. The flat layer has no
    /// transitions yet, so it adds no times.
    pub fn check_times(&mut self) -> Vec<u64> {
        let (played, end) = self.played();
        times(&played.given_at, played.client.history(), end)
    }

    /// Runs every interface check over the whole recording, as it plays with no action taken,
    /// on the frames `mode` names, and returns the faults and how many frames were evaluated.
    pub fn check_all(&mut self, mode: Mode) -> Checked {
        let (played, end) = self.played();
        let history = played.client.history().to_vec();
        let checked: Vec<u64> = match mode {
            Mode::AtCheckTimes => times(&played.given_at, &history, end),
            Mode::EveryTick => {
                let mut ticks: Vec<u64> =
                    (0..=end / self.tick.0).map(|n| n * self.tick.0).collect();
                ticks.push(end);
                ticks.dedup();
                ticks
            }
        };
        let found = steps::segment_checks(&history);
        let mut judged = judged(&history);
        // The frames that name the objects of the faults the segment checks found.
        let naming: BTreeSet<u64> = found.iter().map(|found| found.at).collect();
        // "Every step shown" judges the share of frames in which a step's subject is seen, so
        // its subject is sampled at every tick while the step plays, in both modes: a sparser
        // sample would judge the same step differently.
        let sampled = sampled(&judged, self.tick.0, end);
        let all: BTreeSet<u64> = checked
            .iter()
            .copied()
            .chain(naming.iter().copied())
            .chain(sampled.iter().copied())
            .collect();
        let checked: BTreeSet<u64> = checked.into_iter().collect();

        let mut page = self.fresh(true);
        let mut faults = Vec::new();
        let mut named: Vec<Vec<Subject>> = vec![Vec::new(); found.len()];
        for &t in &all {
            page.advance_to(t);
            let mut context = page.context();
            context.at = t;
            let size = page.size;
            let frame = page.frame();
            if checked.contains(&t) {
                faults.extend(agent::check(frame, &context));
            }
            if sampled.contains(&t) {
                for step in judged
                    .iter_mut()
                    .filter(|step| step.from <= t && t < step.until)
                {
                    let slot = history[step.script].steps[step.step]
                        .subject
                        .expect("only steps with a subject are judged");
                    step.evaluated |= t <= step.to;
                    if let Some(sample) = sample(frame, slot, t, size) {
                        step.samples.push(sample);
                        step.subject.get_or_insert_with(|| subject(frame, slot));
                    }
                }
            }
            if naming.contains(&t) {
                for (index, found) in found.iter().enumerate().filter(|(_, f)| f.at == t) {
                    named[index] = found
                        .slots
                        .iter()
                        .map(|&slot| subject(frame, slot))
                        .collect();
                }
            }
        }
        let mut faults = unique(faults);
        for (found, elements) in found.iter().zip(named) {
            faults.push(fault(found, elements));
        }
        for step in judged.iter().filter(|step| step.evaluated) {
            let record = &history[step.script].steps[step.step];
            if let Some(message) = steps::step_shown(record, step.from, step.to, &step.samples) {
                let elements: Vec<Subject> = step.subject.clone().into_iter().collect();
                let found = Found {
                    rule: agent::Rule::StepShown,
                    slots: record.subject.into_iter().collect(),
                    at: step.from,
                    message,
                };
                faults.push(fault(&found, elements));
            }
        }
        faults.sort_by_key(|fault| fault.at);
        Checked {
            faults,
            frames: all.len(),
        }
    }

    /// Plants a fault of kind `kind` in the script that plays at `at`, in nanoseconds since the
    /// recording's start, and starts the recording over so that it plays with the fault. It
    /// concerns the step that plays at `at` or, failing that, the next one.
    ///
    /// # Errors
    ///
    /// When no script with a step plays at `at`.
    pub fn plant(&mut self, kind: Planted, at: u64) -> Result<(), DriverError> {
        let (played, _) = self.played();
        let history = played.client.history();
        let script = history.iter().enumerate().rev().find(|(_, script)| {
            !script.steps.is_empty()
                && script.start.is_some_and(|start| start <= at)
                && script.real(script.end).is_some_and(|end| at <= end)
        });
        let Some((number, script)) = script else {
            return Err(DriverError(format!(
                "no script with a step plays at t={}ms",
                at / 1_000_000
            )));
        };
        let start = script.start.expect("only started scripts are found");
        self.plants.push(Plant {
            kind,
            script: number,
            t: (at - start) * script.rate,
        });
        self.restart();
        Ok(())
    }

    /// Every step of every script the recording plays with no action taken, in the order the
    /// scripts are given, each with its moment, its verb, and when it begins and ends in real
    /// time. Its middle is the time `moments` gives the step.
    pub fn steps(&mut self) -> Vec<StepMoment> {
        if self.steps.is_none() {
            let (played, _) = self.played();
            let mut steps = Vec::new();
            // The client is given one script per view, in order, from the first: the script at
            // index `i` of the history leads to view `i + 1`.
            for (number, script) in played.client.history().iter().enumerate() {
                for step in &script.steps {
                    let Some((from, to)) = script.span(step) else {
                        continue;
                    };
                    steps.push(StepMoment {
                        name: Name::Step(number as u32 + 1, step.beat as u32, step.index as u32),
                        verb: step.verb,
                        subject: step.subject,
                        from,
                        to,
                        middle: from + (to - from) / 2,
                    });
                }
            }
            self.steps = Some(steps);
        }
        self.steps.clone().unwrap_or_default()
    }

    /// The recording played to its end with no action taken, the client keeping a record of its
    /// scripts, and the time of its end.
    fn played(&self) -> (Page, u64) {
        let mut played = self.fresh(true);
        let moments = played.play_through();
        let end = moments.last().map_or(0, |&(_, time)| time);
        (played, end)
    }
}

/// The check times of a recording whose inputs were given at `given_at` and whose scripts are
/// `history`, up to `end`, in order, each once.
fn times(given_at: &[u64], history: &[ScriptRecord], end: u64) -> Vec<u64> {
    let mut times: BTreeSet<u64> = given_at.iter().copied().collect();
    for script in history {
        let Some(start) = script.start else {
            continue;
        };
        let real = |t: u64| script.real(t).unwrap_or(start);
        times.insert(start);
        times.insert(real(script.end));
        for record in &script.segments {
            let segment = record.segment;
            times.insert(real(segment.start));
            times.insert(real(segment.start + segment.duration / 2));
            times.insert(real(segment.end()));
        }
        for step in &script.steps {
            times.insert(real(step.from));
            times.insert(real(step.to));
        }
    }
    times.into_iter().filter(|&t| t <= end).collect()
}

/// The times at which `judged`'s subjects are sampled: each tick of the clock (a multiple of
/// `tick`, or the recording's `end`) from when each step begins until its subject stops being
/// sampled. They are the frames checking every tick evaluates in the same span.
fn sampled(judged: &[Judged], tick: u64, end: u64) -> BTreeSet<u64> {
    let mut times = BTreeSet::new();
    for step in judged {
        let mut t = step.from.div_ceil(tick) * tick;
        while t < step.until && t <= end {
            times.insert(t);
            t += tick;
        }
        if step.from <= end && end < step.until {
            times.insert(end);
        }
    }
    times
}

/// Every step of `history` that "every step shown" judges: those with a subject whose verb it
/// judges, in scripts that started.
fn judged(history: &[ScriptRecord]) -> Vec<Judged> {
    let mut judged = Vec::new();
    for (number, script) in history.iter().enumerate() {
        if script.start.is_none() {
            continue;
        }
        for (index, step) in script.steps.iter().enumerate() {
            if step.subject.is_none()
                || !matches!(
                    step.verb,
                    beryl_protocol::script::Verb::Fly | beryl_protocol::script::Verb::Appear
                )
            {
                continue;
            }
            let from = script.real(step.from).unwrap_or_default();
            let to = script.real(step.to).unwrap_or_default();
            judged.push(Judged {
                script: number,
                step: index,
                from,
                to,
                until: to + step_nanoseconds(),
                samples: Vec::new(),
                subject: None,
                evaluated: false,
            });
        }
    }
    judged
}

/// The object in `slot` as `frame` names it, or by its slot alone if the frame does not draw it.
fn subject(frame: &Frame, slot: usize) -> Subject {
    match card_of(frame, slot) {
        Some(index) => Subject {
            locator: locator_of(frame, index),
            rect: bounds(&frame.resolved[index as usize]),
        },
        None => Subject {
            locator: format!("object #{slot}"),
            rect: Rect::default(),
        },
    }
}

/// The fault for `found`, naming `elements` before what is wrong with them.
fn fault(found: &Found, elements: Vec<Subject>) -> Fault {
    let names: Vec<String> = elements.iter().map(Subject::describe).collect();
    let message = if names.is_empty() {
        found.message.clone()
    } else {
        format!("{} {}", names.join(" and "), found.message)
    };
    Fault {
        rule: found.rule,
        elements,
        at: found.at,
        message,
    }
}

/// `faults` with each rule's fault about the same elements kept once, the first. A board with
/// many cards finds many faults in every frame, so the faults already kept are looked up in a
/// set rather than scanned.
fn unique(faults: Vec<Fault>) -> Vec<Fault> {
    let mut seen: BTreeSet<(&'static str, Vec<String>)> = BTreeSet::new();
    let mut kept = Vec::new();
    for fault in faults {
        let key = (
            fault.rule.word(),
            fault
                .elements
                .iter()
                .map(|subject| subject.locator.clone())
                .collect(),
        );
        if seen.insert(key) {
            kept.push(fault);
        }
    }
    kept
}
