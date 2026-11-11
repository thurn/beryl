//! A division of combat damage among an attacker's blockers, as a pick of the blockers dealt
//! lethal damage.
//!
//! [CR 510.1c](https://yawgatog.com/resources/magic-rules/#R5101c): "If two or more creatures
//! are blocking it, it assigns its combat damage to those creatures divided as its controller
//! chooses among them." The engine offers a division by its outcome: the blockers that die,
//! picked one at a time while the damage left covers another, and an end once the rest of
//! the damage has somewhere to go (architecture §11). What each pick leaves to pick is the
//! engine's to say, so the prompt opens by playing every run of picks ahead on a copy of the
//! game: after any picks it knows which blockers the damage still reaches, whether the
//! division can end there, and which blocker the session would take next
//! ([`beryl_engine::preferred_damage`]), which it marks as suggested.

use std::collections::BTreeMap;

use beryl_engine::card::{Part, Purpose};
use beryl_engine::{
    Choice, Decision, Layout, Opt, SlotId as _, State, interchangeable, preferred_damage,
};

use crate::prompt::ahead;
use crate::prompt::held::{Clicked, Run, Step, Table, Want, button, options};
use crate::prompt::{ButtonId, Click, Marked, PromptView, ShapeView};
use crate::protocol::DecisionView;
use crate::script::{Mark, Subject};

/// The state of a division of combat damage.
#[derive(Clone, Debug)]
pub(crate) struct Damage {
    /// The blockers, by kind: those of one kind are interchangeable, and the engine offers
    /// one of them at a time (architecture §7.2).
    kinds: Vec<Vec<u16>>,
    /// What each set of picks leaves, by the kinds picked, ascending.
    after: BTreeMap<Vec<usize>, Left>,
    /// The blockers picked to be dealt lethal damage, in the order they were.
    picked: Vec<u16>,
    confirm: String,
}

/// What a set of picks leaves.
#[derive(Clone, Debug, Default)]
struct Left {
    /// The kinds the damage left still covers one more of.
    offered: Vec<usize>,
    /// The division may end here.
    ends: bool,
    /// Ending is all that is left, so the engine asks nothing more.
    lone: bool,
    /// The kind the session would pick next.
    suggested: Option<usize>,
}

impl Damage {
    /// The state the division `decision`, the open decision on `table`, opens in: nothing
    /// picked.
    pub(crate) fn open<L: Layout>(table: &Table<'_, L>, decision: &DecisionView) -> Option<Damage> {
        let mut damage = Damage {
            kinds: Vec::new(),
            after: BTreeMap::new(),
            picked: Vec::new(),
            confirm: decision.prompt.words.confirm.clone(),
        };
        let copy = ahead::forced(table.s);
        damage.explore(table, &copy, table.d?, &mut Vec::new());
        Some(damage)
    }

    /// The kind of the blocker in `slot`: that of one it is interchangeable with as the
    /// prompt opens, or a new one.
    fn kind_of<L: Layout>(&mut self, table: &Table<'_, L>, slot: L::Slot) -> usize {
        let alike = |kind: &Vec<u16>| {
            kind.contains(&slot.num())
                || L::Slot::from_num(kind[0])
                    .is_some_and(|first| interchangeable(table.s, first, slot))
        };
        let kind = self.kinds.iter().position(alike).unwrap_or_else(|| {
            self.kinds.push(Vec::new());
            self.kinds.len() - 1
        });
        if !self.kinds[kind].contains(&slot.num()) {
            self.kinds[kind].push(slot.num());
        }
        kind
    }

    /// Notes what the picks of `picked`, which led to the division's decision `d` in `s`,
    /// leave, and plays each pick it offers on from there.
    fn explore<L: Layout>(
        &mut self,
        table: &Table<'_, L>,
        s: &State<L>,
        d: &Decision<L>,
        picked: &mut Vec<usize>,
    ) {
        let mut key = picked.clone();
        key.sort_unstable();
        if self.after.contains_key(&key) {
            return;
        }
        let options = options(d);
        let mut kind = |option: Opt<L>| match option {
            Opt::Lethal(slot) => Some(self.kind_of(table, slot)),
            _ => None,
        };
        let offered: Vec<(usize, usize)> = (options.iter().enumerate())
            .filter_map(|(i, &option)| Some((i, kind(option)?)))
            .collect();
        let suggested = preferred_damage(s, d)
            .and_then(|(Choice(choice), _)| kind(*options.get(usize::from(choice))?));
        let left = Left {
            offered: offered.iter().map(|&(_, kind)| kind).collect(),
            ends: options.contains(&Opt::Done),
            lone: options.len() == 1,
            suggested,
        };
        self.after.insert(key, left);
        for (i, kind) in offered {
            let mut copy = s.clone();
            let choice = Choice(u16::try_from(i).expect("at most 64 candidates"));
            let Some(next) = ahead::answer(table, &mut copy, choice) else {
                continue;
            };
            if next.prompt.purpose == Purpose::CombatDamage {
                picked.push(kind);
                self.explore(table, &copy, &next, picked);
                picked.pop();
            }
        }
    }

    fn kind(&self, slot: u16) -> Option<usize> {
        self.kinds.iter().position(|kind| kind.contains(&slot))
    }

    /// What the blockers picked leave.
    fn left(&self) -> Option<&Left> {
        let mut key: Vec<usize> = (self.picked.iter())
            .filter_map(|&slot| self.kind(slot))
            .collect();
        key.sort_unstable();
        self.after.get(&key)
    }

    /// The blockers of `kind` not picked.
    fn unpicked(&self, kind: usize) -> impl Iterator<Item = u16> + '_ {
        (self.kinds[kind].iter().copied()).filter(|slot| !self.picked.contains(slot))
    }

    pub(crate) fn click(&mut self, click: Click) -> Clicked {
        match click {
            Click::On {
                on: Subject::Object { slot },
            } => {
                if let Some(at) = self.picked.iter().position(|&picked| picked == slot) {
                    self.picked.remove(at);
                    return Clicked::Changed;
                }
                let covered = (self.kind(slot))
                    .zip(self.left())
                    .is_some_and(|(kind, left)| left.offered.contains(&kind));
                if !covered {
                    return Clicked::Same;
                }
                self.picked.push(slot);
                Clicked::Changed
            }
            Click::Button {
                button: ButtonId::Confirm,
            } => match self.left() {
                Some(left) if left.ends => Clicked::Answers(self.run(left)),
                _ => Clicked::Same,
            },
            _ => Clicked::Same,
        }
    }

    /// The picks of the blockers picked, in whatever order the engine offers them, and the
    /// end, where the engine asks for it.
    fn run(&self, left: &Left) -> Run {
        let picks = self.picked.iter().map(|&slot| Subject::Object { slot });
        let steps = [
            (!self.picked.is_empty()).then(|| Step::Among(Part::Pick, picks.collect())),
            (!left.lone).then_some(Step::One(Part::Pick, Want::Done)),
        ];
        Run::new(Purpose::CombatDamage, steps.into_iter().flatten())
    }

    pub(crate) fn show(&self, view: &mut PromptView) {
        let none = Left::default();
        let left = self.left().unwrap_or(&none);
        let suggested = (left.suggested).and_then(|kind| self.unpicked(kind).next());
        let marked = |mark: Mark| {
            move |slot: u16| Marked {
                on: Subject::Object { slot },
                mark,
            }
        };
        let eligible = (left.offered.iter())
            .flat_map(|&kind| self.unpicked(kind))
            .filter(|&slot| Some(slot) != suggested);
        view.marks = (self.picked.iter().copied().map(marked(Mark::Selected)))
            .chain(suggested.map(marked(Mark::Suggested)))
            .chain(eligible.map(marked(Mark::Eligible)))
            .collect();
        view.buttons = vec![button(
            ButtonId::Confirm,
            &self.confirm,
            left.ends,
            left.ends,
        )];
        let all: usize = self.kinds.iter().map(Vec::len).sum();
        view.state = Some(ShapeView::Pick {
            tray: Vec::new(),
            least: 0,
            most: u8::try_from(all).unwrap_or(u8::MAX),
            picked: (self.picked.iter())
                .map(|&slot| Subject::Object { slot })
                .collect(),
            warned: Vec::new(),
        });
    }
}
