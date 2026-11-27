//! Orders session views and click replies, and plays the prompt's table changes.

use std::collections::VecDeque;

use beryl_protocol::GameUpdate;
use beryl_protocol::prompt::{Click, ClickReply, ClickRequest, PromptView, Purpose, ShapeBody};
use beryl_protocol::protocol::{DecisionId, ObjectId, Side, Status};
use beryl_protocol::script::{Arrow, OwnStep, OwnTimings, Subject, Verb};
use game_play::light::Aim;
use game_play::play::{PlacedLetGo, Show};
use game_table::table::{Held, TrayPoses};
use glam::Vec2;
use kit_base::pose::Pose;

use crate::prompts;

/// The session's current decision input, the replies still expected for it or
/// earlier decisions, and the local presentation of its marks and tray.
#[derive(Default)]
pub struct Router {
    prompt: Option<(DecisionId, PromptView)>,
    was: Option<(DecisionId, PromptView)>,
    own_due: bool,
    suggesting: Suggesting,
    /// Each reply consumes the oldest click, even if its decision has closed.
    /// Keep those entries when a view opens the next decision.
    clicks: VecDeque<ClickRequest>,
    /// The hand card clicked or dropped at a priority, until the answer's script arrives.
    hand_play: Option<(DecisionId, ObjectId)>,
    record: Option<Vec<prompts::OwnRecord>>,
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

#[derive(Default)]
struct Suggesting {
    now: bool,
    was: bool,
}

impl Router {
    /// Calls `prompts::own` for the change of prompt not yet passed to it, if there is one. The
    /// marks of a prompt that closed go at once, even while the script of the view that
    /// answered it plays; the marks of the open prompt come up once its view's script has
    /// played out, as its bar does.
    pub fn own(&mut self, now: u64, views: usize, show: &mut Show, timings: &OwnTimings) {
        if !self.own_due {
            return;
        }
        let decision = self.prompt.as_ref().map(|(decision, _)| *decision);
        if let Some((_, closed)) = self.was.take_if(|(was, _)| Some(*was) != decision) {
            self.own_beat((now, self.was_view), None, Some(&closed), show, timings);
        }
        if self.prompt.is_some() && !show.settled() {
            return;
        }
        self.own_due = false;
        let shown = self.prompt.take();
        let was = self.was.take();
        self.own_beat(
            (now, views),
            shown.as_ref().map(|(_, prompt)| prompt),
            was.as_ref().map(|(_, prompt)| prompt),
            show,
            timings,
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
        show: &mut Show,
        timings: &OwnTimings,
    ) {
        let record = (self.record.as_mut()).map(|record| (view, record));
        let recorded = record.as_ref().map_or(0, |(_, record)| record.len());
        let (latest, lit) = show.latest_and_lit_mut();
        let shown = shown.map(|prompt| prompts::Shown {
            prompt,
            suggesting: self.suggesting.now,
            source: self.source,
            view: latest,
            carried: self.carried,
        });
        let was = was.map(|prompt| prompts::Shown {
            prompt,
            suggesting: self.suggesting.was,
            source: self.was_source,
            view: latest,
            carried: self.was_carried,
        });
        let mut steps = prompts::Steps::new(lit, timings, now, record);
        prompts::own(shown, was, &mut steps);
        let tray = steps.tray_step();
        if let Some(record) = &self.record {
            let times = record[recorded..].iter().flat_map(|step| {
                [
                    now + step.from,
                    now + u64::midpoint(step.from, step.to),
                    now + step.to,
                ]
            });
            self.times.extend(times);
        }
        let Some((step, over, curve)) = tray else {
            return;
        };
        // A closed prompt's tray lets its cards go in the beat of the answer's script that
        // `Show::place_let_go` finds, as the web interface lets it go as a step of that script
        // (`lowered` in `ui/src/stage/prompt.ts`), not after the ends of its marks.
        let answer = (shown.is_none() && !matches!(step, OwnStep::TrayLifts { .. }))
            .then_some(self.was_given);
        if let Some(record) = &self.record {
            if answer.is_some() {
                // The tray's step is the last of its beat (`prompts::own`).
                self.letting_record =
                    (record.len().checked_sub(1)).filter(|&last| record[last].verb == Verb::Lift);
            } else {
                self.times.extend([over.from, over.to]);
            }
        }
        if let OwnStep::TrayLifts { .. } = step {
            if let Some((held, poses)) = self.lift.take() {
                show.lift_tray(held, poses, (over.from, over.to), curve);
            }
        } else {
            let rests = std::mem::take(&mut self.rests);
            show.let_go_tray(&rests, (over.to - over.from, answer), curve);
            self.rests = rests;
        }
    }

    /// Writes where the show placed a closed prompt's tray letting go, once the script that
    /// answered the prompt started, into the record of the steps the client made itself and
    /// the times at which what the prompts draw changes.
    pub fn placed_let_go(&mut self, show: &mut Show) {
        let Some(placed) = show.take_placed_let_go() else {
            return;
        };
        let PlacedLetGo {
            view,
            beat,
            from,
            length,
            start,
        } = placed;
        let Some(record) = &mut self.record else {
            return;
        };
        self.times.extend([start, start + length]);
        if let Some(step) = (self.letting_record.take()).and_then(|at| record.get_mut(at)) {
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
    fn set_prompt(&mut self, prompt: Option<(DecisionId, PromptView)>, views: usize, show: &Show) {
        let changed = self.prompt != prompt;
        let was = std::mem::replace(&mut self.prompt, prompt);
        if changed {
            let suggesting = self.suggesting.now
                && self.prompt.as_ref().is_some_and(|(_, prompt)| {
                    (prompt.buttons.iter()).any(|button| prompts::shows(prompt, button.id))
                });
            self.changed_prompt(was, suggesting, views, show);
        }
    }

    /// Takes the session's answer to the oldest click in flight. It changes the prompt only
    /// while that click's decision is still open: the session may already have sent the view
    /// of the next decision, which an `Answered` for the one before must not close. A reply
    /// with no click in flight is for the open decision.
    pub fn reply(&mut self, reply: ClickReply, views: usize, show: &mut Show) -> bool {
        let open = self.decision();
        let clicked = self.clicks.pop_front().map(|click| click.decision).or(open);
        if open.is_none() || clicked != open {
            return false;
        }
        let next = match reply {
            ClickReply::Open { prompt } => {
                // A prompt opened instead of playing at once, so let a dropped card return to the
                // hand. Keep remembering which twin was chosen while Choose One is open, since
                // picking a way may still play that card.
                if prompt.is(Purpose::Act) {
                    if let Some((_, slot)) = self.hand_play {
                        show.cancel_hand_play(slot);
                    }
                } else {
                    self.cancel_hand_play(show);
                }
                open.map(|decision| (decision, *prompt))
            }
            // The views that follow the answer bring the next prompt.
            ClickReply::Answered => None,
        };
        self.set_prompt(next, views, show);
        true
    }

    /// The rejected click still takes its place in the session's reply order.
    pub fn rejected(&mut self, show: &mut Show) {
        if let Some(request) = self.clicks.pop_front()
            && let Click::On {
                on: Subject::Object { slot },
            } = request.click
            && self.hand_play == Some((request.decision, slot))
        {
            self.cancel_hand_play(show);
        }
    }

    fn cancel_hand_play(&mut self, show: &mut Show) {
        if let Some((_, slot)) = self.hand_play.take() {
            show.cancel_hand_play(slot);
        }
    }

    /// A click names the decision and revision the player saw. The reply is
    /// associated with that decision even if a newer view arrives first.
    pub fn click(&mut self, click: Click, show: &mut Show) -> Option<ClickRequest> {
        let (decision, prompt) = self.prompt.as_ref()?;
        // Play and Draw both commit the same decision. A second press while the first is in
        // flight cannot change the starting player.
        if prompt.is(Purpose::PlayOrDraw)
            && self.clicks.iter().any(|click| click.decision == *decision)
        {
            return None;
        }
        let request = ClickRequest {
            decision: *decision,
            revision: prompt.revision,
            click,
        };
        if self.hand_play.is_none()
            && prompt.is(Purpose::Priority)
            && let Click::On {
                on: Subject::Object { slot },
            } = click
            && show.begin_hand_play(slot)
        {
            self.hand_play = Some((*decision, slot));
        }
        self.clicks.push_back(request);
        Some(request)
    }

    #[must_use]
    pub fn waiting_for_reply(&self) -> bool {
        !self.clicks.is_empty()
    }

    /// Accepts the prompt sent with a view. A view can replace a decision while
    /// that decision's click is still in flight.
    pub fn take_view(&mut self, update: &GameUpdate, views: usize, show: &mut Show) {
        let prompt = if let Status::Deciding { decision } = &update.view.status {
            self.source = decision.source;
            Some((decision.id, (*decision.prompt).clone()))
        } else {
            None
        };
        let next = prompt.as_ref().map(|(decision, _)| *decision);
        if let Some((decision, slot)) = self.hand_play
            && next != Some(decision)
        {
            self.hand_play = None;
            if update.session == decision.session {
                show.finish_hand_play(slot, &update.view);
            } else {
                show.cancel_hand_play(slot);
            }
        }
        self.set_prompt(prompt, views, show);
    }

    #[must_use]
    pub fn decision(&self) -> Option<DecisionId> {
        self.prompt.as_ref().map(|(decision, _)| *decision)
    }

    #[must_use]
    pub fn current(&self) -> Option<&PromptView> {
        self.prompt.as_ref().map(|(_, prompt)| prompt)
    }

    #[must_use]
    pub fn due(&self) -> bool {
        self.own_due
    }

    pub fn record(&mut self) {
        self.record.get_or_insert_default();
    }

    #[must_use]
    pub fn history(&self) -> &[prompts::OwnRecord] {
        self.record.as_deref().unwrap_or_default()
    }

    #[must_use]
    pub fn times(&self) -> &[u64] {
        &self.times
    }

    /// Screen transitions add their middle and end to the recording's check times.
    pub fn record_transition(&mut self, now: u64, length: u64) {
        if self.record.is_some() {
            self.times
                .extend([now.saturating_add(length / 2), now.saturating_add(length)]);
        }
    }

    /// The pointer went onto or off the open prompt's button that shows what it would tap.
    pub fn suggest(&mut self, suggesting: bool, views: usize, show: &Show) {
        if suggesting != self.suggesting.now {
            let was = self.prompt.clone();
            self.changed_prompt(was, suggesting, views, show);
        }
    }

    /// The pointer picked a card up out of the hand, or let it go: what paying for it would tap
    /// is outlined while it is carried.
    pub fn carry(&mut self, carried: Option<usize>, views: usize, show: &Show) {
        let carried = carried
            .and_then(|slot| u16::try_from(slot).ok())
            .map(ObjectId);
        if carried != self.carried {
            let was = self.prompt.clone();
            let suggesting = self.suggesting.now;
            self.changed_prompt(was, suggesting, views, show);
            self.carried = carried;
        }
    }

    /// The prompt as the table shows it changed from `was` to the open prompt with `suggesting`.
    /// A change the next frame has not yet passed to `prompts::own` keeps the state before it,
    /// which is the last one `prompts::own` saw. Where the open prompt's tray's cards would lie,
    /// and where those held up now would go back to, are worked out now.
    fn changed_prompt(
        &mut self,
        was: Option<(DecisionId, PromptView)>,
        suggesting: bool,
        views: usize,
        show: &Show,
    ) {
        if !self.own_due {
            self.was = was;
            self.suggesting.was = self.suggesting.now;
            self.was_source = self.source;
            self.was_carried = self.carried;
            self.was_view = views;
            self.was_given = show.given();
        }
        self.suggesting.now = suggesting;
        self.own_due = true;
        let piles = (self.prompt.as_ref()).and_then(|(_, prompt)| prompts::Piles::of(prompt));
        let up = show.table().held();
        self.lift = piles.map(|piles| {
            let held = piles.held(up);
            let poses = show.table().tray_poses(&held);
            (held, poses)
        });
        self.rests = show.zone_rests();
    }

    /// Draws the arrows to the pointer the open prompt draws, if it is `open`, as the web
    /// interface's `Aim` (`ui/src/stage/Aim.tsx`) draws them: while a choice of targets has
    /// fewer targets picked than it takes, one from the spell or ability that asks; and while
    /// creatures declared await what they are aimed at, a line from each, in the look of the
    /// line their aim will draw. A touch has no pointer to follow, and nothing follows the
    /// pointer while a tray holds cards up over the table. Allocates nothing.
    pub fn set_aims(&self, show: &mut Show, open: bool, mouse: Option<Vec2>) {
        let prompt = (self.prompt.as_ref())
            .filter(|_| open)
            .map(|(_, prompt)| prompt);
        let (target, arrow, aiming): (_, _, &[ObjectId]) = match prompt.map(|p| (p, &p.body)) {
            Some((prompt, ShapeBody::Pick { .. })) => (
                self.source.filter(|_| prompts::pick::seeks_target(prompt)),
                Arrow::HostileTarget,
                &[],
            ),
            Some((_, ShapeBody::Declare { arrow, aiming, .. })) => (None, *arrow, aiming),
            _ => (None, Arrow::HostileTarget, &[]),
        };
        let aims = mouse.into_iter().flat_map(|to| {
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
        show.lit_mut().set_aims(aims);
    }

    /// Which players, the viewer first, the open prompt marks, and so picks by a press on their
    /// life total, while it is `open`.
    pub fn targets(&self, open: bool) -> [bool; 2] {
        let marks = (self.prompt.as_ref())
            .filter(|_| open)
            .map_or(&[][..], |(_, prompt)| &prompt.marks[..]);
        [Side::You, Side::Opponent].map(|player| {
            marks
                .iter()
                .any(|marked| marked.on == Subject::Player { player })
        })
    }
}
