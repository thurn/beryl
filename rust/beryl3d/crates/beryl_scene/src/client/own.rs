//! The steps the client makes itself while a prompt is open (docs/beryl-3d.md, "Timing a
//! script": a client makes a few steps itself), and what they depend on besides the prompt: the
//! spell or ability the decision is for, the card the pointer carries out of the hand, where
//! the mouse is, and where the cards of a prompt's tray lie. `prompts::own` begins and ends the
//! marks and arrows; the client moves the tray's cards when a beat has a tray step
//! (`Steps::tray_step`), from places it worked out as the prompt changed, so that the frame
//! that moves them lays nothing out.

use beryl_protocol::prompt::{ClickReply, PromptView, Purpose, ShapeBody};
use beryl_protocol::protocol::{DecisionId, ObjectId, Side, Status};
use beryl_protocol::script::{Arrow, OwnStep, Subject, Verb};
use glam::Vec2;

use super::Client;
use crate::hud::notices::NoticeFloats;
use crate::light::Aim;
use crate::play::PlacedLetGo;
use crate::prompts;
use crate::table::{Held, TrayPoses};
use kit_base::pose::Pose;

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
    /// How many scripts the show had been given then: the number of the script that answers
    /// the prompt if it closed, the first given after it closed, whose beat a closed prompt's
    /// tray lets go in.
    was_given: usize,
    /// The record of a closed prompt's tray letting go whose start waits for that script, by
    /// its index in the client's record.
    letting_record: Option<usize>,
    /// Where the mouse is over the window, while it is.
    pub(super) mouse: Option<Vec2>,
    /// The cards the open prompt's tray holds up and where each lies, and where the cards the
    /// tray holds now go back to in their zones.
    lift: Option<(Held, TrayPoses)>,
    rests: Vec<(ObjectId, Pose, bool)>,
    /// While the client keeps a record, the times at which what the prompts draw changes: the
    /// start, middle and end of each of their steps and of each move of the tray's cards, and
    /// the middle and end of each transition of the flat layer's screen, in nanoseconds since
    /// the client started.
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
        // A closed prompt's tray lets its cards go in the beat of the answer's script that
        // `Show::place_let_go` finds, as the web interface lets it go as a step of that script
        // (`lowered` in `ui/src/stage/prompt.ts`), not after the ends of its marks.
        let answer = (shown.is_none() && !matches!(step, OwnStep::TrayLifts { .. }))
            .then_some(self.owned.was_given);
        if let Some(record) = &self.own_record {
            if answer.is_some() {
                // The tray's step is the last of its beat (`prompts::own`).
                self.owned.letting_record =
                    (record.len().checked_sub(1)).filter(|&last| record[last].verb == Verb::Lift);
            } else {
                self.owned.times.extend([over.from, over.to]);
            }
        }
        if let OwnStep::TrayLifts { .. } = step {
            if let Some((held, poses)) = self.owned.lift.take() {
                self.show
                    .lift_tray(held, poses, (over.from, over.to), curve);
            }
        } else {
            let rests = std::mem::take(&mut self.owned.rests);
            self.show
                .let_go_tray(&rests, (over.to - over.from, answer), curve);
            self.owned.rests = rests;
        }
    }

    /// Writes where the show placed a closed prompt's tray letting go, once the script that
    /// answered the prompt started, into the record of the steps the client made itself and
    /// the times at which what the prompts draw changes.
    pub(super) fn placed_let_go(&mut self) {
        let Some(placed) = self.show.take_placed_let_go() else {
            return;
        };
        let PlacedLetGo {
            view,
            beat,
            from,
            length,
            start,
        } = placed;
        let Some(record) = &mut self.own_record else {
            return;
        };
        self.owned.times.extend([start, start + length]);
        if let Some(step) = (self.owned.letting_record.take()).and_then(|at| record.get_mut(at)) {
            step.view = view;
            step.beat = beat;
            step.index = 0;
            step.from = from;
            step.to = from + length;
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

    /// Takes the session's answer to the oldest click in flight. It changes the prompt only
    /// while that click's decision is still open: the session may already have sent the view
    /// of the next decision, which an `Answered` for the one before must not close. A reply
    /// with no click in flight is for the open decision.
    pub(super) fn click_replied(&mut self, reply: ClickReply) {
        let open = self.prompt.as_ref().map(|(decision, _)| *decision);
        let clicked = self.clicks.pop_front().or(open);
        if clicked.is_none() || clicked != open {
            return;
        }
        match reply {
            ClickReply::Open { prompt } => {
                self.set_prompt(open.map(|decision| (decision, *prompt)));
            }
            // The views that follow the answer bring the next prompt.
            ClickReply::Answered => self.set_prompt(None),
        }
        self.prepare_counts();
        self.show_screen();
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
            self.owned.was_given = self.show.given();
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

    /// Compiles what the frame at `now`, in nanoseconds since the client started, needs of the
    /// scripts to come (`Show::prepare`), and lays out the floats of the life totals, pools,
    /// notices and row-end counts that adds. A host calls it before each `frame`, in the same tick: it may
    /// allocate, where `frame` allocates nothing. `frame` calls it too, for callers that do
    /// not, and it then does nothing more for the same `now`.
    pub fn prepare(&mut self, now: u64) {
        self.prepare_table(self.bug.table(now));
    }

    /// From now on, compiles every script whole as it arrives (`Show::compile_whole`): for the
    /// tests that compare that with compiling over frames.
    pub fn compile_whole(&mut self) {
        self.show.compile_whole();
    }

    /// `prepare` at `table`, the table's own time (`bug`).
    pub(super) fn prepare_table(&mut self, table: u64) {
        let rows = self.show.rows_key();
        if self.show.prepare(table) {
            self.prepare_counts();
            NoticeFloats::prepare(
                &mut self.layer,
                self.show.coming_notices(),
                self.show.notices(),
            );
        }
        if self.show.rows_key() != rows {
            (self.row_counts).prepare(&mut self.layer, self.show.coming_rows());
        }
        self.prepare_steps();
    }

    /// Lays out again, for a window of a new size or a table placed again (`Show::resize`), the
    /// counts each row of cards shows, the life totals, whose type depends on the layout, and
    /// the notices of the scripts to come: a script that waited to be compiled is compiled
    /// whole as the table is placed again, and its notices are laid out only now.
    pub(super) fn prepare_resized(&mut self) {
        (self.row_counts).prepare(&mut self.layer, self.show.coming_rows());
        self.prepare_counts();
        NoticeFloats::prepare(
            &mut self.layer,
            self.show.coming_notices(),
            self.show.notices(),
        );
    }

    /// Lays out every life total and pool the scripts to come will show, so that a frame lays
    /// out none of them, each total of a player the open prompt marks as a target too.
    pub(super) fn prepare_counts(&mut self) {
        let targets = self.targets(true);
        self.counts.passes = self.screen == crate::screens::Screen::Game;
        self.counts.phone = self.window.phone();
        self.counts.pools = self.screen != crate::screens::Screen::Table;
        (self.counts).prepare(
            &mut self.layer,
            self.show.counts(),
            self.show.coming_counts(),
            targets,
        );
    }

    /// The times at which what the prompts draw changes, or the flat layer's screen moves or
    /// fades, since `record` was called, in nanoseconds since the client started: for the
    /// interface checks' check times.
    #[must_use]
    pub fn own_times(&self) -> &[u64] {
        &self.owned.times
    }

    /// Ticks the flat layer at `now` (`Layer::tick`) and returns whether a transition of its
    /// screen is still under way. While the client keeps a record, a transition that began at
    /// `now` adds its middle and end to the check times: a screen fading in can be pressed from
    /// its second frame, before the script, if any, ends. Its first frame needs no check: a
    /// node fading in is wholly transparent there, so nothing in it can be pressed, and a node
    /// moving stands where the frame before it judged it.
    pub(super) fn tick_layer(&mut self, now: u64) -> bool {
        let ticked = self.layer.tick(now);
        if !ticked.began || self.own_record.is_none() {
            return ticked.moving;
        }
        let length = crate::layout::transition_nanoseconds();
        self.owned.times.extend([now + length / 2, now + length]);
        ticked.moving
    }
}
