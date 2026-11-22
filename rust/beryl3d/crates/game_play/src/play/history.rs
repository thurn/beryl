//! The record the show keeps of every script it is given, for the interface checks, and the
//! faults a test plants in scripts not yet given.

use beryl_protocol::protocol::{ObjectId, ZoneName};
use beryl_protocol::script::Subject;
use game_table::table::{Placement, ability_track};

use super::Show;
use super::compile::Compiled;
use super::record::{Plant, ScriptRecord, StepRecord};

impl Show {
    /// Whether a script that starts with many updates waiting plays faster (`rate_for`); on
    /// unless turned off.
    pub fn set_catch_up(&mut self, catch_up: bool) {
        self.catch_up = catch_up;
    }

    /// From now on, keeps a record of every script the show is given (`history`).
    pub fn record(&mut self) {
        if self.history.is_none() {
            self.history = Some(Vec::new());
            self.recorded_from = self.given;
        }
    }

    /// The record of every script given since `record` was called, in order.
    #[must_use]
    pub fn history(&self) -> &[ScriptRecord] {
        self.history.as_deref().unwrap_or_default()
    }

    /// The step that supplied the segment currently moving this object or ability.
    /// The inspection driver enables the step record before replay.
    #[must_use]
    pub fn moving_step(&self, subject: Subject, now: u64) -> Option<StepRecord> {
        let playing = self.playing.as_ref()?;
        let track = match subject {
            Subject::Object { slot } => playing.compiled.tracks.get(usize::from(slot.0)),
            Subject::Ability { id } => ability_track(&playing.compiled.ability_tracks, id),
            Subject::Player { .. } => None,
        }?;
        let time = playing.clock(now);
        let segment = track
            .segments()
            .iter()
            .rev()
            .find(|segment| segment.start <= time)?;
        if time >= segment.end() || !segment.moves() {
            return None;
        }
        let step = playing.compiled.steps.get(segment.step?.number)?;
        Some(StepRecord {
            view: playing.number + 1,
            ..step.clone()
        })
    }

    /// Inspects the table's resting layout for the view that supplies a card's current zone.
    /// While a script is running the card may already show the destination zone.
    #[must_use]
    pub fn placement_for_probe(&self, slot: Option<ObjectId>, zone: ZoneName) -> Option<Placement> {
        let shown = self.shown()?;
        let destination = self
            .playing
            .as_ref()
            .and_then(|playing| Some((playing.update.as_deref()?, &playing.compiled.finals)));
        let was_in_zone = slot.is_none_or(|slot| {
            shown
                .view
                .objects
                .iter()
                .any(|card| card.slot == slot && card.zone == zone)
        });
        let (update, objects) = if let (false, Some(destination)) = (was_in_zone, destination) {
            destination
        } else {
            (shown, self.table.objects())
        };
        Some(
            self.table
                .placement_for_probe(&update.view, objects.ability_numbers()),
        )
    }

    /// Plants a fault in a script the show has not been given yet, to test the interface checks.
    pub fn plant(&mut self, plant: Plant) {
        self.plants.push(plant);
    }

    /// Writes `compiled`, the `number`th script, into the history, if the show keeps one.
    pub(super) fn write_record(&mut self, number: usize, compiled: &Compiled) {
        let Some(history) = &mut self.history else {
            return;
        };
        let Some(index) = number.checked_sub(self.recorded_from) else {
            return;
        };
        let mut record = ScriptRecord {
            start: None,
            rate: 1,
            end: compiled.end,
            // The script at `index` of the history leads to view `index + 1`.
            steps: compiled
                .steps
                .iter()
                .map(|step| StepRecord {
                    view: index + 1,
                    ..step.clone()
                })
                .collect(),
            segments: compiled.segments.clone(),
            finals: compiled.finals.card_poses().collect(),
            effects: compiled.effects.clone(),
        };
        match history.get_mut(index) {
            // Compiled again for a window of a new shape: it keeps when it started.
            Some(kept) => {
                record.start = kept.start;
                record.rate = kept.rate;
                *kept = record;
            }
            None => history.push(record),
        }
    }

    /// Notes in the history that the `number`th script started at `start`, its clock running
    /// `rate` times faster than real time.
    pub(super) fn record_start(&mut self, number: usize, start: u64, rate: u64) {
        let Some(history) = &mut self.history else {
            return;
        };
        let Some(index) = number.checked_sub(self.recorded_from) else {
            return;
        };
        // A script compiled over frames is written into the history once it is compiled whole,
        // which may be after it starts.
        if index == history.len() {
            history.push(ScriptRecord {
                start: None,
                rate: 1,
                end: 0,
                steps: Vec::new(),
                segments: Vec::new(),
                finals: Vec::new(),
                effects: Vec::new(),
            });
        }
        if let Some(record) = history.get_mut(index) {
            record.start = Some(start);
            record.rate = rate;
        }
    }
}
