//! The open prompt as the session holds it (plan K26), and the run of picks that answers it.
//!
//! A [`Held`] is made when a decision opens a prompt whose shape the session holds, from the
//! decision as its seat is sent it and the game as it stands. Each click changes it and
//! nothing else; it is never saved and never part of an undo point, being a function of the
//! open decision and the clicks since. A click that answers gives a [`Run`]: the picks the
//! prompt stands for, which the game answers one as each decision of the run arrives.

use std::collections::VecDeque;

use beryl_engine::card::{Part, Purpose};
use beryl_engine::layout::Queue as _;
use beryl_engine::{
    Candidates, Choice, Decision, GameTable, Layout, Opt, Scratch, SlotId as _, State,
    interchangeable,
};

use super::pick::Pick;
use super::{Click, PromptView};
use crate::protocol::{DecisionView, PlayerView, Side, Status, ZoneName};
use crate::script::Subject;

/// The open prompt's state.
#[derive(Clone, Debug)]
pub(crate) struct Held {
    /// The prompt as its seat was last sent it: the record and its words, and the state the
    /// shape gives it.
    view: PromptView,
    shape: Shaped,
}

/// The state of each shape the session holds.
#[derive(Clone, Debug)]
enum Shaped {
    Pick(Pick),
}

/// What a click did to the prompt it was made on.
#[derive(Debug)]
pub(crate) enum Clicked {
    /// Nothing: the prompt has no use for it.
    Same,
    /// The prompt's state changed.
    Changed,
    /// It answers the prompt with these picks.
    Answers(Run),
    /// It passes priority on to the viewer's next stop ([`crate::Game::pass_on`]).
    PassesOn,
    /// It opens this prompt over the one clicked, whose state the session does not hold yet.
    Opens(Box<PromptView>),
}

impl Held {
    /// The state the prompt of `view`'s open decision opens in, if the session holds its
    /// shape's: `d` is that decision in the game `s`, which its seat sees as `shown`, or none
    /// at a halt.
    pub(crate) fn open<L: Layout>(
        s: &State<L>,
        shown: &State<L>,
        t: &GameTable,
        d: Option<&Decision<L>>,
        view: &PlayerView,
    ) -> Option<Held> {
        let Status::Deciding { decision } = &view.status else {
            return None;
        };
        let purpose = d.map_or(Purpose::Priority, |d| d.prompt.purpose);
        let table = Table {
            s,
            shown,
            t,
            d,
            view,
        };
        let shape = Shaped::Pick(Pick::open(&table, purpose, decision)?);
        let mut held = Held {
            view: (*decision.prompt).clone(),
            shape,
        };
        held.show();
        Some(held)
    }

    /// The prompt's whole state now.
    pub(crate) fn view(&self) -> &PromptView {
        &self.view
    }

    /// Takes `click` and says what it did. A change counts a revision.
    pub(crate) fn click(&mut self, click: Click) -> Clicked {
        let Shaped::Pick(pick) = &mut self.shape;
        let clicked = pick.click(click);
        if matches!(clicked, Clicked::Changed) {
            self.view.revision += 1;
            self.show();
        }
        clicked
    }

    /// Words the prompt again for `decision`, the open decision drawn again: a priority's pass
    /// button says where passing leads, which a change of stops changes.
    pub(crate) fn reword(&mut self, decision: &DecisionView) {
        let Shaped::Pick(pick) = &mut self.shape;
        pick.reword(decision);
        self.show();
    }

    fn show(&mut self) {
        let Shaped::Pick(pick) = &self.shape;
        pick.show(&mut self.view);
    }
}

/// Writes into `view`, a view of the game `s` at its viewer's decision `d` (none at a halt),
/// the state that decision's prompt opens in, where the session holds its shape's: what a
/// game does as it draws the view of a decision it has just reached. It reads the whole state,
/// but only what its viewer knows decides it.
pub fn add_opening_state<L: Layout>(
    view: &mut PlayerView,
    s: &State<L>,
    t: &GameTable,
    d: Option<&Decision<L>>,
) {
    let mut scratch = Scratch::new();
    let shown = beryl_engine::view(s, t, &mut scratch);
    if let Some(held) = Held::open(s, shown, t, d, view)
        && let Status::Deciding { decision } = &mut view.status
    {
        *decision.prompt = held.view;
    }
}

/// The game a prompt opens in: what a shape reads beyond the decision as its seat is sent it.
pub(crate) struct Table<'a, L: Layout> {
    /// The game as it is played on from: in a paused resolution, as it stood before the
    /// resolution began (architecture §7.3).
    pub s: &'a State<L>,
    /// The game as the decision's seat sees it: a paused resolution done up to its question.
    pub shown: &'a State<L>,
    pub t: &'a GameTable,
    /// The open decision; none at a halt.
    pub d: Option<&'a Decision<L>>,
    pub view: &'a PlayerView,
}

impl<L: Layout> Table<'_, L> {
    /// The cards a click takes as one of `offered`: each on the battlefield or in the viewer's
    /// hand, not offered itself, with the first of `offered` it is interchangeable with, which
    /// the decision offers in its place (architecture §7.2: the engine offers one candidate
    /// for interchangeable objects).
    pub(crate) fn stand_ins(&self, offered: &[u16]) -> Vec<(u16, u16)> {
        let slot = |num: u16| L::Slot::from_num(num);
        let named = |num: u16| self.view.objects.iter().find(|o| o.slot == num);
        (self.view.objects.iter())
            .filter(|o| {
                !offered.contains(&o.slot)
                    && (o.zone == ZoneName::Battlefield
                        || (o.zone == ZoneName::Hand && o.owner == Side::You))
            })
            .filter_map(|o| {
                let stands = offered.iter().copied().find(|&c| {
                    // Interchangeable objects are of one card, so of one name: most are told
                    // apart without asking the engine.
                    named(c).is_some_and(|c| c.name == o.name && c.zone == o.zone)
                        && slot(o.slot)
                            .zip(slot(c))
                            .is_some_and(|(a, b)| interchangeable(self.shown, a, b))
                })?;
                Some((o.slot, stands))
            })
            .collect()
    }
}

/// The picks that answer a prompt, in the engine's order, still to be made: the first answers
/// the open decision, and each one after it the decision of the same prompt that arrives next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Run {
    purpose: Purpose,
    steps: VecDeque<Step>,
}

/// One step of a run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// The candidate at this index of the decision that is open when the run begins.
    Index(u16),
    /// One answer to a pick of this part.
    One(Part, Want),
    /// Each of these as a pick of this part, in whatever order the engine offers them.
    Among(Part, Vec<Subject>),
}

/// An answer a run gives that names no object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Want {
    /// This number: a count of targets.
    Number(u8),
    /// No more picks.
    Done,
}

impl Run {
    /// The picks `steps` of a prompt for `purpose`.
    pub(crate) fn new(purpose: Purpose, steps: impl IntoIterator<Item = Step>) -> Run {
        Run {
            purpose,
            steps: steps.into_iter().collect(),
        }
    }

    /// Whether every pick is made.
    pub(crate) fn is_done(&self) -> bool {
        self.steps.is_empty()
    }

    /// The run's answer to `d`, the next decision of its player in the game `s`, if `d`
    /// belongs to the run's prompt and a step answers it. A step the engine did not ask,
    /// having one answer left (architecture §7.2), is passed over. With no answer the run is
    /// over: the decision opens as a prompt of its own.
    pub(crate) fn answer<L: Layout>(&mut self, s: &State<L>, d: &Decision<L>) -> Option<Choice> {
        if d.prompt.purpose != self.purpose {
            self.steps.clear();
            return None;
        }
        let options = options(d);
        while let Some(step) = self.steps.front_mut() {
            let found = step.answer(s, d, &options);
            if found.is_none() || step.is_spent() {
                self.steps.pop_front();
            }
            if let Some(i) = found {
                return Some(Choice(u16::try_from(i).expect("at most 64 candidates")));
            }
        }
        None
    }
}

/// The candidates of `d` in the order a [`Choice`] counts them, each as an option.
fn options<L: Layout>(d: &Decision<L>) -> Vec<Opt<L>> {
    match &d.candidates {
        Candidates::Objects(_) => d.candidates.objects().map(Opt::Object).collect(),
        Candidates::Options(options) => options.iter().copied().collect(),
    }
}

impl Step {
    /// The index of the candidate of `d`, whose candidates are `options`, that this step
    /// picks, if it is a step for `d`. A step of several objects forgets the one it picked.
    fn answer<L: Layout>(
        &mut self,
        s: &State<L>,
        d: &Decision<L>,
        options: &[Opt<L>],
    ) -> Option<usize> {
        let position = |wanted: Opt<L>| options.iter().position(|&o| o == wanted);
        match self {
            Step::Index(i) => Some(usize::from(*i)).filter(|&i| i < options.len()),
            Step::One(part, _) | Step::Among(part, _) if *part != d.prompt.part => None,
            Step::One(_, Want::Number(n)) => position(Opt::Number(*n)),
            Step::One(_, Want::Done) => position(Opt::Done),
            Step::Among(_, wanted) => {
                // The object itself, or else one the engine offers in its place.
                let (i, found) = (wanted.iter().enumerate())
                    .find_map(|(w, &on)| Some((w, named(d, options, on)?)))
                    .or_else(|| {
                        (wanted.iter().enumerate())
                            .find_map(|(w, &on)| Some((w, alike(s, options, on)?)))
                    })?;
                wanted.swap_remove(i);
                Some(found)
            }
        }
    }

    /// Whether the step has no pick left to make.
    fn is_spent(&self) -> bool {
        !matches!(self, Step::Among(_, wanted) if !wanted.is_empty())
    }
}

/// The candidate among `options`, those of `d`, that names `on` itself.
fn named<L: Layout>(d: &Decision<L>, options: &[Opt<L>], on: Subject) -> Option<usize> {
    options.iter().position(|&o| match (o, on) {
        (Opt::Object(slot), Subject::Object { slot: wanted }) => slot.num() == wanted,
        // The player deciding is the viewer.
        (Opt::Player(player), Subject::Player { player: wanted }) => {
            (player == d.player) == (wanted == Side::You)
        }
        _ => false,
    })
}

/// The candidate among `options` that is an object interchangeable with `on` in `s`, which the
/// engine offers in its place.
fn alike<L: Layout>(s: &State<L>, options: &[Opt<L>], on: Subject) -> Option<usize> {
    let Subject::Object { slot } = on else {
        return None;
    };
    let wanted = L::Slot::from_num(slot)?;
    options
        .iter()
        .position(|&o| matches!(o, Opt::Object(slot) if interchangeable(s, slot, wanted)))
}
