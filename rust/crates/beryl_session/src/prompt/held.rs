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

use super::catalog::Catalog;
use super::divide::Divide;
use super::mode::Mode;
use super::number::Number;
use super::order::Order;
use super::pick::Pick;
use super::split::Split;
use super::yes_no::YesNo;
use super::{ButtonId, Click, Lies, PromptButton, PromptView, ShapeName};
use crate::protocol::{Candidate, DecisionView, ObjectView, PlayerView, Side, Status, ZoneName};
use crate::script::Subject;

/// The open prompt's state.
#[derive(Clone, Debug)]
pub struct Held {
    /// The prompt as its seat was last sent it: the record and its words, and the state the
    /// shape gives it.
    view: PromptView,
    shape: Shaped,
}

/// The state of each shape the session holds.
#[derive(Clone, Debug)]
enum Shaped {
    Pick(Pick),
    Order(Order),
    Split(Split),
    YesNo(YesNo),
    Number(Number),
    Mode(Mode),
    Catalog(Catalog),
    Divide(Divide),
}

/// What a click did to the prompt it was made on.
#[derive(Debug)]
pub enum Clicked {
    /// Nothing: the prompt has no use for it.
    Same,
    /// The prompt's state changed.
    Changed,
    /// It answers the prompt with these picks.
    Answers(Run),
    /// It passes priority on to the viewer's next stop ([`crate::Game::pass_on`]).
    PassesOn,
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
        let shape = match decision.prompt.shape {
            ShapeName::Pick => Shaped::Pick(Pick::open(&table, purpose, decision)?),
            ShapeName::Order => Shaped::Order(Order::open(&table, decision)?),
            ShapeName::Split => Shaped::Split(Split::open(&table, decision)?),
            ShapeName::YesNo => Shaped::YesNo(YesNo::open(&table, purpose, decision)?),
            ShapeName::Number => Shaped::Number(Number::open(purpose, decision)?),
            ShapeName::Mode => Shaped::Mode(Mode::open(purpose, decision)),
            ShapeName::Catalog => Shaped::Catalog(Catalog::open(purpose, decision)?),
            ShapeName::Divide => Shaped::Divide(Divide::open(purpose, decision)?),
            // Paying with permanents and combat's declarations are still a UI's to hold.
            ShapeName::Pay | ShapeName::Declare => return None,
        };
        let mut held = Held {
            view: (*decision.prompt).clone(),
            shape,
        };
        held.show();
        Some(held)
    }

    /// The state the prompt of `view`'s open decision opens in, if the session holds its
    /// shape's: `view` is a view of the game `s` at its viewer's decision `d`, or none at a
    /// halt. A game makes its own as it draws a decision's view; this one is for a decision
    /// put together by hand, as a test of a shape no card asks yet builds one.
    pub fn opening<L: Layout>(
        view: &PlayerView,
        s: &State<L>,
        t: &GameTable,
        d: Option<&Decision<L>>,
    ) -> Option<Held> {
        let mut scratch = Scratch::new();
        let shown = beryl_engine::view(s, t, &mut scratch);
        Held::open(s, shown, t, d, view)
    }

    /// The prompt's whole state now.
    pub fn view(&self) -> &PromptView {
        &self.view
    }

    /// Takes `click` and says what it did. A change counts a revision.
    pub fn click(&mut self, click: Click) -> Clicked {
        let clicked = match &mut self.shape {
            Shaped::Pick(pick) => pick.click(click),
            Shaped::Order(order) => order.click(click),
            Shaped::Split(split) => split.click(click),
            Shaped::YesNo(yes_no) => yes_no.click(click),
            Shaped::Number(number) => number.click(click),
            Shaped::Mode(mode) => mode.clicked(click),
            Shaped::Catalog(catalog) => catalog.click(click),
            Shaped::Divide(divide) => divide.click(click),
        };
        if matches!(clicked, Clicked::Changed) {
            self.view.revision += 1;
            self.show();
        }
        clicked
    }

    /// Words the prompt again for `decision`, the open decision drawn again: a priority's pass
    /// button says where passing leads, which a change of stops changes.
    pub(crate) fn reword(&mut self, decision: &DecisionView) {
        if let Shaped::Pick(pick) = &mut self.shape {
            pick.reword(decision);
        }
        self.show();
    }

    fn show(&mut self) {
        match &self.shape {
            Shaped::Pick(pick) => pick.show(&mut self.view),
            Shaped::Order(order) => order.show(&mut self.view),
            Shaped::Split(split) => split.show(&mut self.view),
            Shaped::YesNo(yes_no) => yes_no.show(&mut self.view),
            Shaped::Number(number) => number.show(&mut self.view),
            Shaped::Mode(mode) => mode.show(&mut self.view),
            Shaped::Catalog(catalog) => catalog.show(&mut self.view),
            Shaped::Divide(divide) => divide.show(&mut self.view),
        }
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
    if let Some(held) = Held::opening(view, s, t, d)
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
    /// The object in `slot` as the viewer sees it.
    pub(crate) fn object(&self, slot: u16) -> Option<&ObjectView> {
        self.view.objects.iter().find(|o| o.slot == slot)
    }

    /// The first of `offered` that the card `o` stands for: one it is interchangeable with,
    /// which the decision offers in its place (architecture §7.2: the engine offers one
    /// candidate for interchangeable objects). Only a card the viewer knows by name stands for
    /// another: whether two cards they cannot tell apart are alike is not theirs to learn.
    fn stands_for(&self, o: &ObjectView, offered: &[u16]) -> Option<u16> {
        let slot = |num: u16| L::Slot::from_num(num);
        o.name.as_ref()?;
        offered.iter().copied().find(|&c| {
            // Interchangeable objects are of one card, so of one name: most are told apart
            // without asking the engine.
            (self.object(c)).is_some_and(|c| c.name == o.name && c.zone == o.zone)
                && slot(o.slot)
                    .zip(slot(c))
                    .is_some_and(|(a, b)| interchangeable(self.shown, a, b))
        })
    }

    /// The cards a click takes as one of `offered`: each not offered itself, with the first of
    /// `offered` it stands for.
    pub(crate) fn stand_ins(&self, offered: &[u16]) -> Vec<(u16, u16)> {
        (self.view.objects.iter())
            .filter(|o| !offered.contains(&o.slot))
            .filter_map(|o| Some((o.slot, self.stands_for(o, offered)?)))
            .collect()
    }

    /// Those of `cards` a pick among `offered` can take: the cards offered and those that
    /// stand for one of them.
    pub(crate) fn takes(&self, cards: &[u16], offered: &[u16]) -> Vec<u16> {
        let stands = |&slot: &u16| {
            (self.object(slot)).is_some_and(|o| self.stands_for(o, offered).is_some())
        };
        (cards.iter().copied())
            .filter(|slot| offered.contains(slot) || stands(slot))
            .collect()
    }

    /// The cards a prompt among the cards that lie as `lies` says shows its viewer, those they
    /// know: the top `count` of a library from the top down, or all of it for no count, as a
    /// search looks through; a hand; a graveyard from the top down, its cards alone
    /// ([CR 111.6](https://yawgatog.com/resources/magic-rules/#R1116): "A token isn't a
    /// card").
    pub(crate) fn laid_out(&self, lies: Lies, count: u8) -> Vec<u16> {
        let players = &self.view.players;
        let player = match lies.player {
            Side::You => &players.you,
            Side::Opponent => &players.opponent,
        };
        let known = |slot: &u16| self.object(*slot).is_some_and(|o| o.name.is_some());
        match lies.zone {
            ZoneName::Library => {
                let top = if count == 0 {
                    usize::MAX
                } else {
                    usize::from(count)
                };
                let library = player.library.iter().rev().take(top);
                library.copied().filter(known).collect()
            }
            ZoneName::Graveyard => (player.graveyard.iter().rev().copied())
                .filter(|&slot| self.object(slot).is_some_and(|o| !o.token))
                .collect(),
            ZoneName::Hand => (self.view.objects.iter())
                .filter(|o| o.zone == ZoneName::Hand && o.owner == lies.player)
                .map(|o| o.slot)
                .collect(),
            _ => Vec::new(),
        }
    }
}

/// The cards `decision` offers, in its order.
pub(crate) fn offered(decision: &DecisionView) -> Vec<u16> {
    (decision.candidates.iter())
        .filter_map(|c| match c.option {
            Candidate::Object { slot } => Some(slot),
            _ => None,
        })
        .collect()
}

/// The picks that answer a prompt, in the engine's order, still to be made: the first answers
/// the open decision, and each one after it the decision of the same prompt that arrives next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    purpose: Purpose,
    steps: VecDeque<Step>,
    /// The run goes on after a choice of targets asked between its picks.
    waits: bool,
    /// The run's first pick continues the answer before it.
    continues: bool,
    /// The run that waits for this one to end: this one answers a prompt opened between that
    /// one's picks.
    under: Option<Box<Run>>,
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
    /// Each of these as a pick of this part, in this order: the picks put them in an order.
    InTurn(Part, VecDeque<Subject>),
}

/// An answer a run gives that names no object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Want {
    /// This number: a count of targets.
    Number(u8),
    /// No more picks.
    Done,
    /// Yes or no.
    Yes,
    No,
    /// The waiting trigger of this ability of the object in this slot, as a trigger is named
    /// from one decision to the next ([`Candidate::Trigger`]).
    Trigger {
        source: Option<u16>,
        ability: u8,
    },
}

impl Run {
    /// The picks `steps` of a prompt for `purpose`.
    pub(crate) fn new(purpose: Purpose, steps: impl IntoIterator<Item = Step>) -> Run {
        Run {
            purpose,
            steps: steps.into_iter().collect(),
            waits: false,
            continues: false,
            under: None,
        }
    }

    /// The run goes on after a choice of targets asked between its picks: a triggered ability
    /// put on the stack has its targets chosen before the next is
    /// ([CR 603.3d](https://yawgatog.com/resources/magic-rules/#R6033d): "The remainder of
    /// the process for putting a triggered ability on the stack is identical to the process
    /// for casting a spell listed in rules 601.2c-d"), and that choice is a prompt of its
    /// own.
    pub(crate) fn waiting(mut self) -> Run {
        self.waits = true;
        self
    }

    /// The run's first pick continues the answer before it, where that answer began a cast:
    /// what a spell is cast with is announced as part of casting it
    /// ([CR 601.2b](https://yawgatog.com/resources/magic-rules/#R6012b): "If the spell has a
    /// variable cost that will be paid as it's being cast (such as an {X} in its mana cost; see
    /// rule 107.3), the player announces the value of that variable"), so the choice log
    /// records it as that answer's continuation and one undo takes back the cast with it.
    pub(crate) fn continuing(mut self) -> Run {
        self.continues = true;
        self
    }

    /// Whether the run's first pick continues the answer before it ([`Run::continuing`]).
    pub(crate) fn continues(&self) -> bool {
        self.continues
    }

    /// This run over `waiting`, a run waiting through the prompt this one answers.
    pub(crate) fn over(mut self, waiting: Option<Run>) -> Run {
        self.under = waiting.map(Box::new);
        self
    }

    /// The run with picks still to make: this one, or once it has made its own, the one
    /// that waited for it.
    pub fn rest(self) -> Option<Run> {
        if self.steps.is_empty() {
            self.under.and_then(|under| under.rest())
        } else {
            Some(self)
        }
    }

    /// Whether `d`, a decision the run does not answer, leaves it waiting for its next pick.
    fn waits_through<L: Layout>(&self, d: &Decision<L>) -> bool {
        self.waits && !self.steps.is_empty() && d.prompt.purpose == Purpose::Target
    }

    /// The run's answer to `d`, the next decision of its player in the game `s`, if `d`
    /// belongs to the run's prompt and a step answers it. A step the engine did not ask,
    /// having one answer left (architecture §7.2), is passed over. With no answer the run is
    /// over, unless it waits through `d`: the decision opens as a prompt of its own.
    pub fn answer<L: Layout>(&mut self, s: &State<L>, d: &Decision<L>) -> Option<Choice> {
        if d.prompt.purpose != self.purpose {
            if self.waits_through(d) {
                return None;
            }
            // This run is over: one that waited for it goes on.
            self.steps.clear();
            let waiting = self.under.take()?;
            *self = *waiting;
            return self.answer(s, d);
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
pub(crate) fn options<L: Layout>(d: &Decision<L>) -> Vec<Opt<L>> {
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
            Step::One(part, _) | Step::Among(part, _) | Step::InTurn(part, _)
                if *part != d.prompt.part =>
            {
                None
            }
            Step::One(_, Want::Number(n)) => position(Opt::Number(*n)),
            Step::One(_, Want::Done) => position(Opt::Done),
            Step::One(_, Want::Yes) => position(Opt::Yes),
            Step::One(_, Want::No) => position(Opt::No),
            Step::One(_, Want::Trigger { source, ability }) => options.iter().position(|&o| {
                let Opt::Trigger(i) = o else {
                    return false;
                };
                let trigger = &s.triggers()[Into::<usize>::into(i)];
                trigger.source.map(|r| r.slot.num()) == *source && trigger.ability.get() == *ability
            }),
            Step::InTurn(_, wanted) => {
                // The next in turn itself, or else one the engine offers in its place.
                let &on = wanted.front()?;
                let found = named(d, options, on).or_else(|| alike(s, options, on))?;
                wanted.pop_front();
                Some(found)
            }
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
        match self {
            Step::Index(_) | Step::One(..) => true,
            Step::Among(_, wanted) => wanted.is_empty(),
            Step::InTurn(_, wanted) => wanted.is_empty(),
        }
    }
}

/// The candidate among `options`, those of `d`, that names `on` itself: an object offered, or
/// a blocker offered to be dealt lethal damage.
fn named<L: Layout>(d: &Decision<L>, options: &[Opt<L>], on: Subject) -> Option<usize> {
    options.iter().position(|&o| match (o, on) {
        (Opt::Object(slot) | Opt::Lethal(slot), Subject::Object { slot: wanted }) => {
            slot.num() == wanted
        }
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
    options.iter().position(
        |&o| matches!(o, Opt::Object(slot) | Opt::Lethal(slot) if interchangeable(s, slot, wanted)),
    )
}

/// A button with no caption.
pub(crate) fn button(id: ButtonId, label: &str, enabled: bool, primary: bool) -> PromptButton {
    PromptButton {
        id,
        label: label.to_owned(),
        caption: String::new(),
        enabled,
        primary,
    }
}
