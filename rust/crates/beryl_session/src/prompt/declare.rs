//! The declare shape: creatures each aimed at something, the attackers and what each attacks,
//! or the blockers and the attacker each blocks.
//!
//! [CR 508.1a](https://yawgatog.com/resources/magic-rules/#R5081a): "The active player
//! chooses which creatures that they control, if any, will attack."
//! [CR 508.1b](https://yawgatog.com/resources/magic-rules/#R5081b): "If the defending player
//! controls any planeswalkers, ... the active player announces which player, planeswalker, or
//! battle each of the chosen creatures is attacking."
//! [CR 509.1a](https://yawgatog.com/resources/magic-rules/#R5091a): "The defending player
//! chooses which creatures they control, if any, will block. ... For each of the chosen
//! creatures, the defending player chooses one creature for it to block".
//!
//! The engine takes a declaration one pick at a time: a creature, then what it is aimed at
//! where there is more than one thing, then the next, then an end (architecture §11). The
//! prompt holds the declaration in the making, changed at will until it is confirmed, and
//! its run makes the picks as the engine asks for them ([`Aims`]).

use beryl_engine::card::{Part, Purpose};
use beryl_engine::{Choice, Decision, Layout, Opt, SlotId as _, State};

use super::ahead;
use super::held::{Clicked, Run, Step, Table, Want, alike, button, named, options};
use super::{
    ButtonId, Click, Marked, ObjectId, Pair, PartName, PromptView, ShapeBody, ids, slots_of,
};
use crate::protocol::{Candidate, DecisionView, Side};
use crate::script::{Arrow, Mark, Subject};

/// The state of a declaration.
#[derive(Clone, Debug)]
pub(crate) struct Declare {
    purpose: Purpose,
    arrow: Arrow,
    /// Whether a line is drawn from each creature declared to what it is aimed at: a
    /// blocker's to its attacker, and an attacker's to a planeswalker. The stage draws no
    /// line to a player (Arena, `IEXcjBn3BXk 22:16.53`), as the script draws none
    /// (`script/spans.rs`).
    lined: bool,
    /// The creatures that can be declared, each with what it can be aimed at: those the
    /// decision offers, in its order, then the ones that stand for one of those.
    able: Vec<(u16, Vec<Subject>)>,
    /// The creatures declared and aimed, in the order they were.
    pairs: Vec<Pair>,
    /// The creatures declared whose aim the next click gives.
    aiming: Vec<u16>,
    /// The prompt asks the aim alone, of creatures the engine has declared already: what
    /// they can be aimed at. A click on one answers.
    aims: Option<Vec<Subject>>,
    /// The buttons' words, `{picked}` still to fill.
    confirm: String,
    decline: String,
    auto: String,
    cancel: String,
}

impl Declare {
    /// The state the declaration `decision`, the open decision on `table`, opens in: nothing
    /// declared, or, where the engine asks an aim, its creatures awaiting one.
    pub(crate) fn open<L: Layout>(
        table: &Table<'_, L>,
        purpose: Purpose,
        decision: &DecisionView,
    ) -> Option<Declare> {
        let prompt = &decision.prompt;
        let d = table.d?;
        let (arrow, lined) = match purpose {
            Purpose::Attack => (Arrow::Attack, true),
            Purpose::Block => (Arrow::Block, true),
            _ => return None,
        };
        let mut declare = Declare {
            purpose,
            arrow,
            lined,
            able: Vec::new(),
            pairs: Vec::new(),
            aiming: Vec::new(),
            aims: None,
            confirm: prompt.words.confirm.clone(),
            decline: prompt.words.decline.clone(),
            auto: prompt.words.auto.clone(),
            cancel: prompt.words.cancel.clone(),
        };
        if prompt.part == PartName::Aim {
            declare.aiming = slots_of(&prompt.about);
            declare.aims = Some(aims(table, d));
            return Some(declare);
        }
        // What each creature offered can be aimed at is the engine's to say: its next
        // question, on a copy of the game in which every question is asked.
        for (i, candidate) in decision.candidates.iter().enumerate() {
            let (Candidate::Attack { slot } | Candidate::Block { slot }) = candidate.option else {
                continue;
            };
            let mut copy = ahead::forced(table.s);
            let choice = Choice(u16::try_from(i).expect("at most 64 candidates"));
            let aim = ahead::answer(table, &mut copy, choice)
                .filter(|next| next.prompt.purpose == purpose && next.prompt.part == Part::Aim);
            declare
                .able
                .push((slot, aim.map_or_else(Vec::new, |aim| aims(table, &aim))));
        }
        let offered: Vec<u16> = declare.able.iter().map(|&(slot, _)| slot).collect();
        for (slot, stands_for) in table.stand_ins(&offered) {
            let aims = declare.aims_of(stands_for).to_vec();
            declare.able.push((slot, aims));
        }
        Some(declare)
    }

    /// What the creature in `slot` can be aimed at.
    fn aims_of(&self, slot: u16) -> &[Subject] {
        (self.able.iter())
            .find(|&&(able, _)| able == slot)
            .map_or(&[], |(_, aims)| aims)
    }

    /// What every creature awaiting an aim can be aimed at.
    fn open_aims(&self) -> Vec<Subject> {
        if let Some(aims) = &self.aims {
            return aims.clone();
        }
        let Some((&first, rest)) = self.aiming.split_first() else {
            return Vec::new();
        };
        (self.aims_of(first).iter().copied())
            .filter(|aim| rest.iter().all(|&slot| self.aims_of(slot).contains(aim)))
            .collect()
    }

    fn declared(&self, slot: u16) -> bool {
        self.aiming.contains(&slot) || self.pairs.iter().any(|pair| pair.from == ObjectId(slot))
    }

    /// Declares the creature in `slot`: aimed at once where it has one thing to aim at, and
    /// awaiting its aim otherwise. Attackers await theirs together, a blocker alone.
    fn declare(&mut self, slot: u16) {
        match *self.aims_of(slot) {
            [to] => self.pairs.push(Pair {
                from: ObjectId(slot),
                to,
            }),
            _ if self.purpose == Purpose::Block => self.aiming = vec![slot],
            _ => self.aiming.push(slot),
        }
    }

    pub(crate) fn click(&mut self, click: Click) -> Clicked {
        match click {
            Click::On { on } => self.click_on(on),
            Click::Button {
                button: ButtonId::Confirm,
            } if !self.pairs.is_empty() && self.aiming.is_empty() => {
                Clicked::Answers(self.run(self.pairs.clone()))
            }
            Click::Button {
                button: ButtonId::Decline,
            } if self.aims.is_none() => {
                if self.pairs.is_empty() && self.aiming.is_empty() {
                    return Clicked::Answers(self.run(Vec::new()));
                }
                // With a declaration in the making, the button takes it back.
                self.pairs.clear();
                self.aiming.clear();
                Clicked::Changed
            }
            Click::Button {
                button: ButtonId::Auto,
            } if !self.auto.is_empty() => {
                // As they lie on the table.
                let mut rest: Vec<u16> = (self.able.iter().map(|&(slot, _)| slot))
                    .filter(|&slot| !self.declared(slot))
                    .collect();
                rest.sort_unstable();
                if rest.is_empty() {
                    return Clicked::Same;
                }
                for slot in rest {
                    self.declare(slot);
                }
                Clicked::Changed
            }
            _ => Clicked::Same,
        }
    }

    fn click_on(&mut self, on: Subject) -> Clicked {
        if self.open_aims().contains(&on) {
            // The engine has declared them: the aim is all it asks.
            if self.aims.is_some() {
                let steps = [Step::Among(Part::Aim, vec![on])];
                return Clicked::Answers(Run::new(self.purpose, steps));
            }
            for from in self.aiming.drain(..) {
                self.pairs.push(Pair {
                    from: ObjectId(from),
                    to: on,
                });
            }
            return Clicked::Changed;
        }
        let Subject::Object { slot } = on else {
            return Clicked::Same;
        };
        if self.aims.is_some() {
            return Clicked::Same;
        }
        if self.declared(slot) {
            self.aiming.retain(|&aiming| aiming != slot);
            self.pairs.retain(|pair| pair.from != ObjectId(slot));
            return Clicked::Changed;
        }
        if !self.able.iter().any(|&(able, _)| able == slot) {
            return Clicked::Same;
        }
        self.declare(slot);
        Clicked::Changed
    }

    /// The picks that declare `pairs`, and the end.
    fn run(&self, pairs: Vec<Pair>) -> Run {
        let steps = [
            (!pairs.is_empty()).then(|| Step::Aimed(Aims::new(pairs))),
            Some(Step::One(Part::Pick, Want::Done)),
        ];
        Run::new(self.purpose, steps.into_iter().flatten())
    }

    pub(crate) fn show(&self, view: &mut PromptView) {
        let marked = |mark: Mark| move |on: Subject| Marked { on, mark };
        let card = |slot: u16| Subject::Object { slot };
        let declared = (self.pairs.iter().map(|pair| pair.from.0))
            .chain(self.aiming.iter().copied())
            .map(card);
        let able = (self.able.iter().map(|&(slot, _)| slot))
            .filter(|&slot| !self.declared(slot))
            .map(card);
        view.marks = (declared.map(marked(Mark::Selected)))
            .chain(able.map(marked(Mark::Eligible)))
            .chain(self.open_aims().into_iter().map(marked(Mark::Eligible)))
            .collect();
        let count = self.pairs.len() + self.aiming.len();
        view.buttons = if self.aims.is_some() {
            Vec::new()
        } else if count == 0 {
            let all = !self.auto.is_empty();
            let auto = all.then(|| button(ButtonId::Auto, &self.auto, true, true));
            let none = button(ButtonId::Decline, &self.decline, true, !all);
            auto.into_iter().chain([none]).collect()
        } else {
            let confirm = (self.confirm)
                .replace("{picked}", &count.to_string())
                .replace("{s}", if count == 1 { "" } else { "s" });
            let aimed = self.aiming.is_empty();
            vec![
                button(ButtonId::Confirm, &confirm, aimed, aimed),
                button(ButtonId::Decline, &self.cancel, true, false),
            ]
        };
        view.body = ShapeBody::Declare {
            arrow: self.arrow,
            pairs: self.pairs.clone(),
            lined: self.lined,
            aiming: ids(self.aiming.iter().copied()),
        };
    }
}

/// What the creatures the aim `d` on `table` asks about can be aimed at: the candidates in
/// its order, then the attackers that stand for one of those (the engine offers one of
/// identical attackers, architecture §7.2).
fn aims<L: Layout>(table: &Table<'_, L>, d: &Decision<L>) -> Vec<Subject> {
    let mut aims = Vec::new();
    let mut attackers = Vec::new();
    for option in options(d) {
        match option {
            Opt::Defender(None) => aims.push(Subject::Player {
                player: Side::Opponent,
            }),
            Opt::Defender(Some(slot)) => aims.push(Subject::Object { slot: slot.num() }),
            Opt::Object(slot) => attackers.push(slot.num()),
            _ => {}
        }
    }
    let twins = table.stand_ins(&attackers);
    (aims.into_iter())
        .chain(attackers.iter().map(|&slot| Subject::Object { slot }))
        .chain((twins.iter()).map(|&(slot, _)| Subject::Object { slot }))
        .collect()
}

/// The picks of a declaration, still to be made: each creature as the engine offers it, then
/// what it is aimed at where the engine asks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Aims {
    /// The declarations still to make.
    left: Vec<Pair>,
    /// The one whose creature was just picked: the engine asks its aim next, unless it has
    /// one answer.
    aiming: Option<Pair>,
    /// For each attacker a blocker was aimed at, the one the engine took for it. Of identical
    /// attackers it offers one, and once blocked that one is told apart from its twins
    /// ([CR 509.1h](https://yawgatog.com/resources/magic-rules/#R5091h): "An attacking
    /// creature with one or more creatures declared as blockers for it becomes a blocked
    /// creature"), so the blockers aimed at one attacker after the first follow the first.
    taken: Vec<(u16, u16)>,
}

impl Aims {
    fn new(pairs: Vec<Pair>) -> Aims {
        Aims {
            left: pairs,
            aiming: None,
            taken: Vec::new(),
        }
    }

    /// Whether every declaration is made.
    pub(crate) fn is_spent(&self) -> bool {
        self.left.is_empty() && self.aiming.is_none()
    }

    /// Notes that the engine took the attacker in `took` for the one in `aimed`, and so will
    /// take the other for the first where the two differ.
    fn take(&mut self, aimed: u16, took: u16) {
        self.taken
            .retain(|&(from, _)| from != aimed && from != took);
        self.taken.push((aimed, took));
        if aimed != took {
            self.taken.push((took, aimed));
        }
    }

    /// The pick this makes at `d`, whose candidates are `options`, in the game `s`: at a pick
    /// of a creature, the first the engine offers that is declared, or else stands in the
    /// place of one that is; at an aim, what that creature was aimed at.
    pub(crate) fn answer<L: Layout>(
        &mut self,
        s: &State<L>,
        d: &Decision<L>,
        options: &[Opt<L>],
    ) -> Option<usize> {
        let card = |slot: u16| Subject::Object { slot };
        match d.prompt.part {
            Part::Pick => {
                // An aim the engine did not ask had one answer: the attacker its blocker is
                // blocking now.
                if let Some(pair) = self.aiming.take()
                    && let Subject::Object { slot: aimed } = pair.to
                    && let Some(blocker) = L::Slot::from_num(pair.from.0)
                    && s.object(blocker).is_blocking()
                    && let Some(took) = s.object(blocker).combat()
                {
                    self.take(aimed, took.num());
                }
                let itself = |pair: &Pair| named(d, options, card(pair.from.0));
                let stands = |pair: &Pair| alike(s, options, card(pair.from.0));
                let first = |found: &dyn Fn(&Pair) -> Option<usize>| {
                    (self.left.iter().enumerate())
                        .filter_map(|(at, pair)| Some((found(pair)?, at)))
                        .min()
                };
                let (found, at) = first(&itself).or_else(|| first(&stands))?;
                self.aiming = Some(self.left.remove(at));
                Some(found)
            }
            Part::Aim => {
                let pair = self.aiming.take()?;
                let Subject::Object { slot: aimed } = pair.to else {
                    return named(d, options, pair.to);
                };
                let taken = (self.taken.iter()).find(|&&(from, _)| from == aimed);
                if let Some(&(_, took)) = taken {
                    return named(d, options, card(took));
                }
                let found = named(d, options, pair.to).or_else(|| alike(s, options, pair.to))?;
                if let Opt::Object(took) = options[found] {
                    self.take(aimed, took.num());
                }
                Some(found)
            }
            _ => None,
        }
    }
}
