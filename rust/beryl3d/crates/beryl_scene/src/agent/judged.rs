//! How much each interface check judged: for each rule, how many frames (or, for the checks that
//! read a script's records, scripts) it looked at with something to judge, and how many elements
//! (or segments, steps and arrows) it judged in them. A rule that judges nothing passes on every
//! recording without checking anything, so `just b3d test` reports these counts and a rule at
//! zero is a fault of the checks, not a pass.

use std::ops::AddAssign;

use super::checks::{KeptArrow, arrows_held};
use super::fault::{Layout, Rule};
use crate::frame::{Frame, Place, Role, StateBits};
use crate::play::record::ScriptRecord;

/// What one rule judged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Count {
    /// Frames, or scripts for the checks that read a script's records, in which the rule had at
    /// least one thing to judge.
    pub frames: u64,
    /// The things it judged in them.
    pub elements: u64,
}

/// What every rule judged, by [`Rule::index`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Judged(pub [Count; Rule::ALL.len()]);

impl Judged {
    /// What `rule` judged.
    #[must_use]
    pub fn of(&self, rule: Rule) -> Count {
        self.0[rule.index()]
    }

    fn add(&mut self, rule: Rule, elements: usize) {
        if elements > 0 {
            let count = &mut self.0[rule.index()];
            count.frames += 1;
            count.elements += elements as u64;
        }
    }

    /// Counts what the frame checks (`agent::check`) judge in `frame`, shown in `layout`. Each
    /// count follows the filter its check applies before it judges an element.
    pub fn frame(&mut self, frame: &Frame, layout: Layout) {
        let mut interactive = 0;
        let mut layer = 0;
        let mut cards = 0;
        let mut held = 0;
        // Flat-layer targets other than cards, which "phone sizes" holds to a finger's size.
        let mut targets = 0;
        let mut moving = false;
        let mut face_up = 0;
        for element in &frame.resolved {
            moving |= element.state.has(StateBits::MOVING) || element.state.has(StateBits::HELD);
            if element.role == Role::Card
                && element.opacity > 0.0
                && !element.state.has(StateBits::FACE_DOWN)
                && !frame.name(element.name).is_empty()
            {
                face_up += 1;
            }
            if !element.interactive {
                continue;
            }
            interactive += 1;
            if element.state.has(StateBits::HELD) {
                held += 1;
            }
            if element.role == Role::Card {
                cards += 1;
            } else if element.place == Place::Layer {
                layer += 1;
            }
            if element.place == Place::Layer && element.role != Role::Card {
                targets += 1;
            }
        }
        self.add(Rule::OnScreen, interactive - held);
        self.add(Rule::Overlap, layer);
        // Cards are judged only on a frame where nothing on the table moves.
        self.add(Rule::Reachable, layer + if moving { 0 } else { cards });
        self.add(Rule::Duplicate, frame.resolved.len());
        self.add(Rule::TextFits, frame.lines.len());
        self.add(Rule::Named, interactive);
        self.add(Rule::BlankCard, face_up);
        self.add(
            Rule::LayerDrawn,
            frame.draws.len().saturating_sub(frame.layer as usize),
        );
        // "Phone sizes" judges only the phone layout.
        if layout == Layout::Phone {
            self.add(Rule::PhoneSizes, targets + frame.lines.len());
        }
        // "Clips fit" judges every frame's list of clip rectangles, entry 0 of which is no clip;
        // its elements are the rectangles the frame asked for.
        let clips = &mut self.0[Rule::Clips.index()];
        clips.frames += 1;
        clips.elements += frame.clips[1..]
            .iter()
            .filter(|clip| clip.width > 0.0)
            .count() as u64;
    }

    /// Counts the arrows "lines hold" compares between the frame whose arrows `before` holds and
    /// `after`.
    pub fn lines(&mut self, before: &[KeptArrow], after: &Frame) {
        self.add(Rule::LinesHold, arrows_held(before, after));
    }

    /// Counts what the segment and step checks (`steps::segment_checks`) judge in `script`.
    pub fn script(&mut self, script: &ScriptRecord) {
        if script.start.is_none() {
            return;
        }
        self.add(Rule::Jump, script.segments.len());
        self.add(Rule::RestPlacement, script.slots().len());
        self.add(Rule::StepTime, script.steps.len());
        self.add(
            Rule::Crowded,
            script
                .steps
                .iter()
                .filter(|step| step.key.is_some())
                .count(),
        );
    }

    /// Counts one step "every step shown" judged from `samples` frames.
    pub fn step_shown(&mut self, samples: usize) {
        let count = &mut self.0[Rule::StepShown.index()];
        count.frames += samples as u64;
        count.elements += 1;
    }
}

impl AddAssign<&Self> for Judged {
    fn add_assign(&mut self, other: &Self) {
        for (count, more) in self.0.iter_mut().zip(other.0) {
            count.frames += more.frames;
            count.elements += more.elements;
        }
    }
}
