//! The steps the client makes itself while a prompt is open (docs/beryl-3d.md, "Timing a
//! script": a client makes a few steps itself), and what they depend on besides the prompt: the
//! spell or ability the decision is for, the card the pointer carries out of the hand, where
//! the mouse is, and where the cards of a prompt's tray lie. `prompts::own` begins and ends the
//! marks and arrows; the client moves the tray's cards when a beat has a tray step
//! (`Steps::tray_step`), from places it worked out as the prompt changed, so that the frame
//! that moves them lays nothing out.

use beryl_protocol::prompt::{PromptView, Purpose, ShapeBody};
use beryl_protocol::protocol::{DecisionId, ObjectId, Side, Status};
use beryl_protocol::script::{Arrow, OwnStep, Subject};
use glam::Vec2;

use super::Client;
use crate::light::Aim;
use crate::pose::Pose;
use crate::prompts;
use crate::table::{Held, TrayPoses};

/// What the prompt's own steps depend on besides the prompt, now and when `prompts::own` last
/// ran (beside `Client::was`).
#[derive(Clone, Debug, Default)]
pub(super) struct Owned {
    source: Option<ObjectId>,
    was_source: Option<ObjectId>,
    carried: Option<ObjectId>,
    was_carried: Option<ObjectId>,
    /// How many views the client had been given when the prompt last changed, which the steps
    /// that end a closed prompt's marks are written under: a batch of views that play on can
    /// arrive before the next frame plays those steps.
    was_view: usize,
    /// Where the mouse is over the window, while it is.
    pub(super) mouse: Option<Vec2>,
    /// The cards the open prompt's tray holds up and where each lies, and where the cards the
    /// tray holds now go back to in their zones.
    lift: Option<(Held, TrayPoses)>,
    rests: Vec<(ObjectId, Pose, bool)>,
    /// While the client keeps a record, the times at which what the prompts draw changes: the
    /// start, middle and end of each of their steps and of each move of the tray's cards, in
    /// nanoseconds since the client started.
    times: Vec<u64>,
}

impl Client {
    /// Calls `prompts::own` for the change of prompt not yet passed to it, if there is one. The
    /// marks of a prompt that closed go at once, even while the script of the view that
    /// answered it plays; the marks of the open prompt come up once its view's script has
    /// played out, as its bar does.
    pub(super) fn own(&mut self, now: u64) {
        if !self.own_due {
            return;
        }
        let decision = self.prompt.as_ref().map(|(decision, _)| *decision);
        if let Some((_, closed)) = self.was.take_if(|(was, _)| Some(*was) != decision) {
            self.own_beat((now, self.owned.was_view), None, Some(&closed));
        }
        if self.prompt.is_some() && !self.show.settled() {
            return;
        }
        self.own_due = false;
        let shown = self.prompt.take();
        let was = self.was.take();
        self.own_beat(
            (now, self.views),
            shown.as_ref().map(|(_, prompt)| prompt),
            was.as_ref().map(|(_, prompt)| prompt),
        );
        self.prompt = shown;
        self.was = was;
    }

    /// Plays one beat of `prompts::own` from `was` to `shown` at `now`, written under the view
    /// numbered `view`, and moves the tray's cards if the beat has a tray step.
    fn own_beat(
        &mut self,
        (now, view): (u64, usize),
        shown: Option<&PromptView>,
        was: Option<&PromptView>,
    ) {
        let record = (self.own_record.as_mut()).map(|record| (view, record));
        let recorded = record.as_ref().map_or(0, |(_, record)| record.len());
        let (latest, lit) = self.show.latest_and_lit_mut();
        let owned = &self.owned;
        let shown = shown.map(|prompt| prompts::Shown {
            prompt,
            suggesting: self.suggesting.now,
            source: owned.source,
            view: latest,
            carried: owned.carried,
        });
        let was = was.map(|prompt| prompts::Shown {
            prompt,
            suggesting: self.suggesting.was,
            source: owned.was_source,
            view: latest,
            carried: owned.was_carried,
        });
        let mut steps = prompts::Steps::new(lit, &self.own_timings, now, record);
        prompts::own(shown, was, &mut steps);
        let tray = steps.tray_step();
        if let Some(record) = &self.own_record {
            let times = record[recorded..].iter().flat_map(|step| {
                [
                    now + step.from,
                    now + u64::midpoint(step.from, step.to),
                    now + step.to,
                ]
            });
            self.owned.times.extend(times);
        }
        let Some((step, over, curve)) = tray else {
            return;
        };
        if let Some(record) = &mut self.own_record {
            self.owned.times.extend([over.from, over.to]);
            // The web interface lets a closed prompt's tray go as the first step of the
            // script that plays the answer (`lowered` in `ui/src/stage/prompt.ts`), not after
            // the ends of its marks: the timeline places it first in its view's beat.
            if shown.is_none()
                && let Some(last) = record.last_mut()
            {
                last.index = 0;
            }
        }
        if let OwnStep::TrayLifts { .. } = step {
            if let Some((held, poses)) = self.owned.lift.take() {
                self.show
                    .lift_tray(held, poses, (over.from, over.to), curve);
            }
        } else {
            let rests = std::mem::take(&mut self.owned.rests);
            self.show.let_go_tray(&rests, over.to - over.from, curve);
            self.owned.rests = rests;
        }
    }

    /// Takes `prompt` as the open prompt, which the decision is for `source` (unchanged where
    /// the prompt is the same decision's). If it differs from the one open before, the next
    /// `frame` calls `prompts::own` for the change, a prompt closing included. A new decision
    /// whose prompt is the same as the last one's is a change too: the last one's marks end and
    /// the new one's begin, as the web's `Prompts.play` ends the open prompt's marks for any
    /// decision but the open one shown again (`ui/src/stage/prompt.ts`). A prompt with no
    /// button that shows what it would tap stops the pointer's showing it, as the web's
    /// `usePaying` does: a button that goes while the pointer is on it never says the pointer
    /// left.
    pub(super) fn set_prompt(&mut self, prompt: Option<(DecisionId, PromptView)>) {
        let changed = self.prompt != prompt;
        let was = std::mem::replace(&mut self.prompt, prompt);
        if changed {
            let suggesting = self.suggesting.now
                && self.prompt.as_ref().is_some_and(|(_, prompt)| {
                    (prompt.buttons.iter()).any(|button| prompts::shows(prompt, button.id))
                });
            self.changed_prompt(was, suggesting);
        }
    }

    /// Takes the spell or ability the decision of `status` is for, if it asks one, before the
    /// prompt it brings (`set_prompt`).
    pub(super) fn set_source(&mut self, status: &Status) {
        if let Status::Deciding { decision } = status {
            self.owned.source = decision.source;
        }
    }

    /// The pointer went onto or off the open prompt's button that shows what it would tap.
    pub(super) fn suggest(&mut self, suggesting: bool) {
        if suggesting != self.suggesting.now {
            let was = self.prompt.clone();
            self.changed_prompt(was, suggesting);
        }
    }

    /// The pointer picked a card up out of the hand, or let it go: what paying for it would tap
    /// is outlined while it is carried.
    pub(super) fn carry(&mut self, carried: Option<usize>) {
        let carried = carried
            .and_then(|slot| u16::try_from(slot).ok())
            .map(ObjectId);
        if carried != self.owned.carried {
            let was = self.prompt.clone();
            let suggesting = self.suggesting.now;
            self.changed_prompt(was, suggesting);
            self.owned.carried = carried;
        }
    }

    /// The prompt as the table shows it changed from `was` to the open prompt with `suggesting`.
    /// A change the next frame has not yet passed to `prompts::own` keeps the state before it,
    /// which is the last one `prompts::own` saw. Where the open prompt's tray's cards would lie,
    /// and where those held up now would go back to, are worked out now.
    fn changed_prompt(&mut self, was: Option<(DecisionId, PromptView)>, suggesting: bool) {
        if !self.own_due {
            self.was = was;
            self.suggesting.was = self.suggesting.now;
            self.owned.was_source = self.owned.source;
            self.owned.was_carried = self.owned.carried;
            self.owned.was_view = self.views;
        }
        self.suggesting.now = suggesting;
        self.own_due = true;
        let piles = (self.prompt.as_ref()).and_then(|(_, prompt)| prompts::Piles::of(prompt));
        let up = self.show.table().held();
        self.owned.lift = piles.map(|piles| {
            let held = piles.held(up);
            let poses = self.show.table().tray_poses(&held);
            (held, poses)
        });
        self.owned.rests = self.show.zone_rests();
    }

    /// Draws the arrows to the pointer the open prompt draws, if it is `open`, as the web
    /// interface's `Aim` (`ui/src/stage/Aim.tsx`) draws them: while a choice of targets has
    /// fewer targets picked than it takes, one from the spell or ability that asks; and while
    /// creatures declared await what they are aimed at, a line from each, in the look of the
    /// line their aim will draw. A touch has no pointer to follow, and nothing follows the
    /// pointer while a tray holds cards up over the table. Allocates nothing.
    pub(super) fn set_aims(&mut self, open: bool) {
        let prompt = (self.prompt.as_ref())
            .filter(|_| open)
            .map(|(_, prompt)| prompt);
        let (target, arrow, aiming): (_, _, &[ObjectId]) = match prompt.map(|p| (p, &p.body)) {
            Some((
                prompt,
                ShapeBody::Pick {
                    tray, most, picked, ..
                },
            )) => {
                let seeks = prompt.is(Purpose::Target)
                    && tray.is_empty()
                    && picked.len() < usize::from(*most);
                (
                    self.owned.source.filter(|_| seeks),
                    Arrow::HostileTarget,
                    &[],
                )
            }
            Some((_, ShapeBody::Declare { arrow, aiming, .. })) => (None, *arrow, aiming),
            _ => (None, Arrow::HostileTarget, &[]),
        };
        let aims = self.owned.mouse.into_iter().flat_map(|to| {
            let aim = move |arrow, slot| Aim {
                arrow,
                from: Subject::Object { slot },
                to,
            };
            let target = target.map(move |slot| aim(Arrow::HostileTarget, slot));
            target
                .into_iter()
                .chain(aiming.iter().map(move |&slot| aim(arrow, slot)))
        });
        self.show.lit_mut().set_aims(aims);
    }

    /// Which players, the viewer first, the open prompt marks, and so picks by a press on their
    /// life total, while it is `open`.
    pub(super) fn targets(&self, open: bool) -> [bool; 2] {
        let marks = (self.prompt.as_ref())
            .filter(|_| open)
            .map_or(&[][..], |(_, prompt)| &prompt.marks[..]);
        [Side::You, Side::Opponent].map(|player| {
            marks
                .iter()
                .any(|marked| marked.on == Subject::Player { player })
        })
    }

    /// Lays out every life total and pool the scripts to come will show, so that a frame lays
    /// out none of them, each total of a player the open prompt marks as a target too.
    pub(super) fn prepare_counts(&mut self) {
        let targets = self.targets(true);
        (self.counts).prepare(
            &mut self.layer,
            self.show.counts(),
            self.show.coming_counts(),
            targets,
        );
    }

    /// The times at which what the prompts draw changes, since `record` was called, in
    /// nanoseconds since the client started: for the interface checks' check times.
    #[must_use]
    pub fn own_times(&self) -> &[u64] {
        &self.owned.times
    }
}
