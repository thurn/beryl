//! The order shape: things put in an order, which the player changes by moving one at a time.
//!
//! Its one purpose is the order of a player's waiting triggered abilities
//! ([CR 603.3b](https://yawgatog.com/resources/magic-rules/#R6033b): "each player, in APNAP
//! order, puts each triggered ability they control with a trigger condition that isn't
//! another ability triggering on the stack in any order they choose"). The prompt lists them as they will resolve, first
//! to last, and opens in the order the session itself would put them in
//! ([`trigger_to_stack`]). The engine takes them as they go on the stack, so the picks run
//! from the last to resolve up
//! ([CR 405.2](https://yawgatog.com/resources/magic-rules/#R4052): "Each time an object is put
//! on the stack, it's put on top of all objects already there"), and the first to resolve
//! goes on unasked as the one left.

use beryl_engine::card::{Part, Purpose};
use beryl_engine::layout::Queue as _;
use beryl_engine::{Candidates, Choice, Decision, GameTable, Layout, Opt, State};

use super::held::{Clicked, Run, Step, Table, Want};
use super::{ButtonId, Click, PromptButton, PromptView, ShapeView};
use crate::protocol::{Candidate, DecisionView};
use crate::script::Subject;

/// The state of a prompt of the order shape.
#[derive(Clone, Debug)]
pub(crate) struct Order {
    /// The waiting triggers as they will resolve, the first first: each by the index of its
    /// candidate in the open decision.
    items: Vec<u16>,
    /// What names each candidate's trigger in the decisions after the first, as the indexes
    /// shift: the slot of its source and which of the source's abilities it is.
    named: Vec<(Option<u16>, u8)>,
    confirm: String,
}

impl Order {
    /// The state the order prompt of `decision`, the open decision on `table`, opens in.
    pub(crate) fn open<L: Layout>(table: &Table<'_, L>, decision: &DecisionView) -> Option<Order> {
        let named = (decision.candidates.iter())
            .map(|c| match c.option {
                Candidate::Trigger {
                    source, ability, ..
                } => Some((source, ability)),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Order {
            items: resolving_order(table.s, table.t, table.d?),
            named,
            confirm: decision.prompt.words.confirm.clone(),
        })
    }

    pub(crate) fn click(&mut self, click: Click) -> Clicked {
        match click {
            Click::Drop {
                on: Subject::Ability { id },
                pile: 0,
                at,
            } => {
                let Some(from) = (self.items.iter()).position(|&item| u32::from(item) == id) else {
                    return Clicked::Same;
                };
                let at = usize::from(at);
                if at > self.items.len() {
                    return Clicked::Same;
                }
                // Before what is at `at` now: once the item is taken out, what lay after it
                // lies one sooner.
                let to = if at > from { at - 1 } else { at };
                if to == from {
                    return Clicked::Same;
                }
                let item = self.items.remove(from);
                self.items.insert(to, item);
                Clicked::Changed
            }
            Click::Button {
                button: ButtonId::Confirm,
            } => Clicked::Answers(self.run()),
            _ => Clicked::Same,
        }
    }

    /// The picks that put the triggers on the stack in this order: the last to resolve, a
    /// candidate of the open decision, then each before it by its name, up to the second. The
    /// run waits through the targets each trigger asks for as it goes on the stack.
    fn run(&self) -> Run {
        let mut later = self.items.iter().skip(1).rev();
        let first = later.next().map(|&item| Step::Index(item));
        let rest = later.map(|&item| {
            let (source, ability) = self.named[usize::from(item)];
            Step::One(Part::Pick, Want::Trigger { source, ability })
        });
        Run::new(Purpose::TriggerOrder, first.into_iter().chain(rest)).waiting()
    }

    /// Writes the state into `view`. Nothing is marked: a waiting trigger is not on the
    /// table yet.
    pub(crate) fn show(&self, view: &mut PromptView) {
        view.marks = Vec::new();
        view.buttons = vec![PromptButton {
            id: ButtonId::Confirm,
            label: self.confirm.clone(),
            caption: String::new(),
            enabled: true,
            primary: true,
        }];
        let item = |&item: &u16| Subject::Ability { id: item.into() };
        view.state = Some(ShapeView::Order {
            items: self.items.iter().map(item).collect(),
        });
    }
}

/// Where a waiting trigger comes in the order the session resolves a player's simultaneous
/// triggers in, by what the engine knows of its ability:
///
/// 1. first, the triggers with neither targets nor a question as they resolve: plain effects
///    (cards, tokens, counters, life, a sacrifice), whose results the others may act on;
/// 2. then the ones with targets, which were chosen as they went on the stack (CR 603.3d);
/// 3. last, the ones that ask their controller something as they resolve, so the answer is
///    made seeing everything the others did;
///
/// and, within each group, in the order they triggered.
fn resolves<L: Layout>(s: &State<L>, t: &GameTable, opt: Opt<L>) -> (u8, L::Index) {
    let Opt::Trigger(i) = opt else {
        unreachable!("a trigger-order decision offers only triggers");
    };
    let tr = &s.triggers()[Into::<usize>::into(i)];
    let ability = &t.def(tr.card).face(tr.face).abilities[tr.ability.index()];
    let group = if ability.asks() {
        2
    } else {
        u8::from(!ability.targets.is_empty())
    };
    (group, i)
}

/// The triggers `d` offers, a trigger-order decision in the game `s`.
fn triggers<L: Layout>(d: &Decision<L>) -> &[Opt<L>] {
    let Candidates::Options(triggers) = &d.candidates else {
        unreachable!("a trigger-order decision offers options");
    };
    triggers.as_slice()
}

/// The candidates of `d`, by index, in the order the session would have their triggers
/// resolve ([`resolves`]), the first first.
fn resolving_order<L: Layout>(s: &State<L>, t: &GameTable, d: &Decision<L>) -> Vec<u16> {
    let triggers = triggers(d);
    let mut order: Vec<u16> = (0..triggers.len())
        .map(|i| u16::try_from(i).expect("a choice names its candidate"))
        .collect();
    order.sort_by_key(|&i| resolves(s, t, triggers[usize::from(i)]));
    order
}

/// Which of the human's waiting triggers to put on the stack next, under the smart stops. Each
/// pick goes on the stack above the ones before, so the first picked resolves last
/// ([CR 603.3b](https://yawgatog.com/resources/magic-rules/#R6033b): "each player, in APNAP
/// order, puts each triggered ability they control with a trigger condition that isn't
/// another ability triggering on the stack in any order they choose"): the one the session's order ([`resolves`])
/// has resolve last of those waiting.
pub fn trigger_to_stack<L: Layout>(s: &State<L>, t: &GameTable, d: &Decision<L>) -> Choice {
    let (last, _) = (triggers(d).iter().enumerate())
        .max_by_key(|&(_, &opt)| resolves(s, t, opt))
        .expect("a decision has a candidate");
    Choice(u16::try_from(last).expect("a choice names its candidate"))
}
