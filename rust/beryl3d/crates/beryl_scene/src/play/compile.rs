//! Compiling a script: every step of every beat turned into segments at times on the script's
//! own clock, before the script starts, so that where each object is at any moment of the script
//! is a function of the time alone (docs/beryl-3d.md, "Determinism").
//!
//! The script is played over a copy of the table it starts from: after each step, every object's
//! resting place is worked out again for the table as it now stands, and the step's verb carries
//! its object there. An object no step names whose resting place the step changed (the rest of a
//! hand) slides there over the step, or over the time the step leaves for what it opened or
//! closed.

use beryl_session::protocol::PlayerView;
use beryl_session::script::{Script, Step, StepTiming, Subject, Table as Played, Verb, mover_of};

use crate::beat::{Laid, beat};
use crate::motion::Track;
use crate::pose::{Curve, Pose, Segment};
use crate::table::{Placement, SceneObject, Table};
use crate::verbs::{self, Interval};

/// A script laid out in time, ready to start.
#[derive(Clone, Debug)]
pub(crate) struct Compiled {
    /// One track for each object that moves, by slot. An object whose track is empty stands at
    /// its resting place in the view the table showed before the script.
    pub tracks: Vec<Track>,
    /// The scene objects at rest in the view the script leads to, which the table takes when
    /// the script ends.
    pub finals: Vec<Option<SceneObject>>,
    /// How long the script lasts on its own clock, in nanoseconds.
    pub end: u64,
    /// The verbs among its steps that are drawn as instant changes, one bit per verb in the order
    /// of `Verb::ALL`.
    pub unbuilt: u32,
}

/// Lays out `after`'s script, played from `before`, the view the table shows when it starts,
/// with one timing per step in `timings`. With no `before` (the first view), with a reset, or
/// with timings that do not match the steps, nothing moves: the table takes `after` as it stands
/// when the script ends. A reset's beats still hold the time their timings give.
pub(crate) fn compile(
    before: Option<&PlayerView>,
    after: &PlayerView,
    timings: &[StepTiming],
    table: &Table,
) -> Compiled {
    let mut compiled = Compiled {
        tracks: Vec::new(),
        finals: Table::objects_for(after, &table.placement(after)),
        end: 0,
        unbuilt: 0,
    };
    let Some(script) = after.script.as_ref() else {
        return compiled;
    };
    if timings.len() != script.beats.iter().map(Vec::len).sum::<usize>() {
        return compiled;
    }
    let mut motion = before
        .filter(|_| script.reset.is_none())
        .map(|before| Motion::new(before, table));
    compiled.end = lay_out(
        script,
        timings,
        &mut compiled.unbuilt,
        |step, timing, laid| {
            if let Some(motion) = &mut motion {
                motion.step(step, timing, laid);
            }
        },
    );
    if let Some(motion) = motion {
        compiled.tracks = motion.tracks;
    }
    compiled
}

/// Lays out each beat of `script` after the one before, calling `each` with every step, its
/// timing and when it plays, and returns when the last beat is over, in nanoseconds. Each verb
/// drawn as an instant change sets its bit in `unbuilt`.
fn lay_out(
    script: &Script,
    timings: &[StepTiming],
    unbuilt: &mut u32,
    mut each: impl FnMut(&Step, &StepTiming, &Laid),
) -> u64 {
    let mut laid = Vec::new();
    let mut at = 0.0;
    let mut first = 0;
    for steps in &script.beats {
        let timings = &timings[first..first + steps.len()];
        // No span is open and nothing passes over the table yet: the flat layer draws neither.
        let end = beat(steps, timings, at, |_| None, at, &mut laid);
        for ((step, timing), laid) in steps.iter().zip(timings).zip(&laid) {
            let verb = step.act.verb();
            if !verbs::built(verb) {
                *unbuilt |= bit(verb);
            }
            each(step, timing, laid);
        }
        at = end;
        first += steps.len();
    }
    nanoseconds(at)
}

/// `verb`'s bit in `Compiled::unbuilt`.
pub(crate) fn bit(verb: Verb) -> u32 {
    let index = Verb::ALL
        .iter()
        .position(|other| *other == verb)
        .expect("every verb is in `Verb::ALL`");
    1 << index
}

/// A stage time in milliseconds, as `beat` gives it, in nanoseconds.
fn nanoseconds(milliseconds: f64) -> u64 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a script lasts seconds and starts at 0"
    )]
    let nanoseconds = (milliseconds.max(0.0) * 1e6).round() as u64;
    nanoseconds
}

/// The table as a script plays over it, and the tracks its steps have made so far.
struct Motion<'a> {
    table: &'a Table,
    played: Played,
    /// The view the table shows as it now stands, rewritten after each step.
    view: PlayerView,
    /// Every object's resting place as the table now stands.
    rests: Placement,
    tracks: Vec<Track>,
}

impl<'a> Motion<'a> {
    fn new(before: &PlayerView, table: &'a Table) -> Self {
        Self {
            table,
            played: Played::of(before),
            view: before.clone(),
            rests: table.placement(before),
            tracks: Vec::new(),
        }
    }

    /// Plays one step: applies its act, and gives its object and every object whose resting
    /// place changed a segment to the new place.
    fn step(&mut self, step: &Step, timing: &StepTiming, laid: &Laid) {
        // A step the table cannot take changes nothing; the table takes the view as it stands
        // when the script ends, whatever the steps did.
        if self.played.apply(&step.act).is_err() {
            return;
        }
        write(&self.played, &mut self.view);
        let next = self.table.placement(&self.view);
        let own = Interval {
            from: nanoseconds(laid.from),
            to: nanoseconds(laid.to),
        };
        // What makes room for the step does so before it begins, where its act went ahead of it;
        // after it, where it says how long the room it left takes to close; and otherwise over
        // the step.
        let room = match (laid.opens, timing.closes) {
            (Some(opens), _) => Interval {
                from: nanoseconds(opens.at),
                to: nanoseconds(opens.by),
            },
            (None, Some(closes)) => Interval {
                from: own.to,
                to: nanoseconds(laid.to + closes),
            },
            (None, None) => own,
        };
        let subject = match mover_of(&step.act) {
            Some(Subject::Object { slot }) => Some(usize::from(slot.0)),
            _ => None,
        };
        let slots = self.rests.rests.len().max(next.rests.len());
        for slot in 0..slots {
            let was = self.rests.rests.get(slot).copied().flatten();
            let now = next.rests.get(slot).copied().flatten();
            let segment = if Some(slot) == subject {
                let verb = step.act.verb();
                let from = self.pose(slot, own.from);
                let to = match now {
                    Some(rest) => rest.pose,
                    None if was.is_some() => Pose {
                        opacity: 0.0,
                        ..from
                    },
                    None => continue,
                };
                // An instant change that changes nothing needs no segment.
                let moves = matches!(verb, Verb::Fly | Verb::Appear) || from != to;
                moves.then(|| verbs::segment(verb, from, to, own, Curve::named(timing.curve)))
            } else {
                match (was, now) {
                    (Some(was), Some(now)) if was.pose != now.pose => {
                        Some(verbs::slide(self.pose(slot, room.from), now.pose, room))
                    }
                    (Some(_), None) => {
                        let from = self.pose(slot, own.from);
                        Some(verbs::instant(
                            from,
                            Pose {
                                opacity: 0.0,
                                ..from
                            },
                            own.from,
                        ))
                    }
                    (None, Some(now)) => Some(verbs::instant(
                        Pose {
                            opacity: 0.0,
                            ..now.pose
                        },
                        now.pose,
                        own.from,
                    )),
                    _ => None,
                }
            };
            if let Some(segment) = segment {
                self.push(slot, segment);
            }
        }
        self.rests = next;
    }

    /// Where the object in `slot` is at `t` as the steps so far have it: on its track, or where
    /// it rested before the script if nothing has moved it, or nowhere to be seen.
    fn pose(&self, slot: usize, t: u64) -> Pose {
        match self.tracks.get(slot).filter(|t| !t.segments().is_empty()) {
            Some(track) => track.pose_at(t),
            None => self.rests.rests.get(slot).copied().flatten().map_or(
                Pose {
                    opacity: 0.0,
                    ..Pose::default()
                },
                |rest| rest.pose,
            ),
        }
    }

    /// Adds `segment` to the track of the object in `slot`. An object's first segment comes after
    /// one that holds it where it rested before the script, so that it stands there until it
    /// moves.
    fn push(&mut self, slot: usize, segment: Segment) {
        if self.tracks.len() <= slot {
            self.tracks.resize(slot + 1, Track::default());
        }
        if self.tracks[slot].segments().is_empty() {
            let start = self.pose(slot, 0);
            let hold = verbs::instant(start, start, 0);
            // An empty track has room.
            let _ = self.tracks[slot].push(hold, 0);
        }
        // The segments of one object are added in order of their starts, so any that ended
        // before this one starts no longer decides a pose and can go to make room.
        if self.tracks[slot].push(segment, segment.start).is_err() {
            // Every segment it holds is still under way as this one starts, which only a script
            // that moves one object eight times at once can do. The track starts over from this
            // segment, which decides the object's pose from its start on.
            self.tracks[slot] = Track::default();
            let _ = self.tracks[slot].push(segment, segment.start);
        }
    }
}

/// Writes the parts of `played` that placement reads into `view`: its objects, battlefield,
/// stack and each player's library and graveyard. The rest of `view` is chrome, which no resting
/// place depends on.
fn write(played: &Played, view: &mut PlayerView) {
    view.objects.clear();
    view.objects.extend(played.objects.values().cloned());
    view.battlefield.clone_from(&played.battlefield);
    view.stack.clear();
    view.stack
        .extend(played.stack.iter().map(|entry| entry.item.clone()));
    for (player, piles) in [
        (&mut view.players.you, &played.you),
        (&mut view.players.opponent, &played.opponent),
    ] {
        player.library.clone_from(&piles.library);
        player.graveyard.clone_from(&piles.graveyard);
        player.life = piles.life;
        player.mana = piles.mana;
    }
}
