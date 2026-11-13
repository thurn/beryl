//! The split shape: cards dealt out between two piles, in an order where a pile keeps one.
//!
//! A scry deals the cards looked at between the bottom of the library and its top; a look
//! that takes some puts those in the hand and the rest on the bottom; cards put back go from
//! the hand onto the library. The prompt's two piles are its record's two places, in that
//! order, and every card opens in the second: on top as it lies, or in the hand it stays in.
//! A click on a card sends it to the other pile, and a drop puts it in a pile at a place.
//!
//! The engine asks for the first pile's cards one at a time, then for the order of the
//! second's ([`Part::First`], [`Part::Second`]): the run makes those picks from the piles as
//! they stand.

use beryl_engine::Layout;
use beryl_engine::card::{Part, Purpose};

use super::held::{Clicked, Run, Step, Table, Want, offered};
use super::{ButtonId, Click, Marked, PartName, Pile, PlaceName, PromptButton, PromptView};
use super::{ObjectId, ShapeBody, ids};
use crate::protocol::{Candidate, DecisionView};
use crate::script::{Mark, Subject};

/// The state of a prompt of the split shape.
#[derive(Clone, Debug)]
pub(crate) struct Split {
    purpose: Purpose,
    /// The first destination's pile, then the second's.
    piles: [Pile; 2],
    /// The first pile's picks offer an end: it takes any number of the cards.
    ends: bool,
    confirm: String,
}

/// The order a pile's cards are picked in.
enum Picks {
    /// As listed, the top first: each goes under the one before.
    Down,
    /// From the last listed up: each goes on the one before.
    Up,
    /// Any: the pile keeps no order.
    Any,
    /// None: the cards stay where they are.
    None,
}

fn picks(place: Option<PlaceName>) -> Picks {
    let Some(place) = place else {
        return Picks::None;
    };
    match place {
        PlaceName::Top | PlaceName::Bottom => Picks::Down,
        PlaceName::OnTop => Picks::Up,
        PlaceName::Hand | PlaceName::Graveyard | PlaceName::Exile | PlaceName::Battlefield => {
            Picks::Any
        }
    }
}

impl Split {
    /// The state the split prompt of `decision`, the open decision on `table`, opens in:
    /// every card in the second pile. A prompt that opens at its second part, the first
    /// pile's picks made, has that pile closed and empty.
    pub(crate) fn open<L: Layout>(table: &Table<'_, L>, decision: &DecisionView) -> Option<Split> {
        let prompt = &decision.prompt;
        let first = match prompt.part {
            PartName::First => true,
            PartName::Second => false,
            _ => return None,
        };
        let offered = offered(decision);
        // The cards still to deal out lie together: on top of the library, where the first
        // not among the picks ends them, or in the hand.
        let lies = prompt.from?;
        let cards: Vec<u16> = (table.laid_out(lies, table.record.count).into_iter())
            .take_while(|&slot| !table.takes(&[slot], &offered).is_empty())
            .collect();
        let ends = (decision.candidates.iter()).any(|c| c.option == Candidate::Done);
        let all = u8::try_from(cards.len()).unwrap_or(u8::MAX);
        let [to, rest] = table.record.places.map(PlaceName::of);
        let [to_label, rest_label] = prompt.words.piles.clone();
        Some(Split {
            purpose: table.d?.prompt.purpose,
            piles: [
                Pile {
                    place: to,
                    label: to_label,
                    cards: Vec::new(),
                    exactly: (first && !ends).then_some(table.record.most.min(all)),
                    closed: !first,
                },
                Pile {
                    place: rest,
                    label: rest_label,
                    cards: ids(cards),
                    exactly: None,
                    closed: false,
                },
            ],
            ends,
            confirm: prompt.words.confirm.clone(),
        })
    }

    /// The pile `slot` is in, and where in it.
    fn find(&self, slot: u16) -> Option<(usize, usize)> {
        (self.piles.iter().enumerate())
            .find_map(|(pile, p)| Some((pile, p.cards.iter().position(|&c| c == ObjectId(slot))?)))
    }

    /// Moves the card in `slot` to pile `to`, before what is at `at` there now, or to its end.
    fn drop(&mut self, slot: u16, to: usize, at: Option<usize>) -> Clicked {
        let Some((from, i)) = self.find(slot) else {
            return Clicked::Same;
        };
        let Some(pile) = self.piles.get(to) else {
            return Clicked::Same;
        };
        let len = pile.cards.len();
        let at = at.unwrap_or(len);
        let full = pile.exactly.is_some_and(|n| len >= usize::from(n));
        if self.piles[from].closed || pile.closed || at > len || (from != to && full) {
            return Clicked::Same;
        }
        // Within a pile, what lay after the card lies one sooner once it is taken out.
        let at = if from == to && at > i { at - 1 } else { at };
        if from == to && at == i {
            return Clicked::Same;
        }
        self.piles[from].cards.remove(i);
        self.piles[to].cards.insert(at, ObjectId(slot));
        Clicked::Changed
    }

    /// Whether each pile that takes a number of cards holds it.
    fn ready(&self) -> bool {
        (self.piles.iter()).all(|p| p.exactly.is_none_or(|n| p.cards.len() == usize::from(n)))
    }

    pub(crate) fn click(&mut self, click: Click) -> Clicked {
        match click {
            // To the other pile, at its end.
            Click::On {
                on: Subject::Object { slot },
            } => match self.find(slot) {
                Some((from, _)) => self.drop(slot, 1 - from, None),
                None => Clicked::Same,
            },
            Click::Drop {
                on: Subject::Object { slot },
                pile,
                at,
            } => self.drop(slot, usize::from(pile), Some(usize::from(at))),
            Click::Button {
                button: ButtonId::Confirm,
            } if self.ready() => Clicked::Answers(self.run()),
            _ => Clicked::Same,
        }
    }

    /// The picks that deal the cards out as the piles stand: the first pile's cards, and the
    /// end of them where the engine offers one and cards are left; then the second's, the
    /// last going unasked as the one left. A pile that keeps an order is picked in it.
    fn run(&self) -> Run {
        let [first, second] = &self.piles;
        let mut steps = Vec::new();
        if !first.closed {
            steps.extend(picked(Part::First, first.place, &first.cards));
            if self.ends && !second.cards.is_empty() {
                steps.push(Step::One(Part::First, Want::Done));
            }
        }
        let asked = second.cards.len().saturating_sub(1);
        steps.extend(picked(Part::Second, second.place, &second.cards[..asked]));
        Run::new(self.purpose, steps)
    }

    /// Writes the state into `view`: every card that can be moved is eligible.
    pub(crate) fn show(&self, view: &mut PromptView) {
        view.marks = (self.piles.iter())
            .filter(|pile| !pile.closed)
            .flat_map(|pile| pile.cards.iter())
            .map(|&ObjectId(slot)| Marked {
                on: Subject::Object { slot },
                mark: Mark::Eligible,
            })
            .collect();
        let ready = self.ready();
        view.buttons = vec![PromptButton {
            id: ButtonId::Confirm,
            label: self.confirm.clone(),
            caption: String::new(),
            enabled: ready,
            primary: ready,
        }];
        view.body = ShapeBody::Split {
            piles: self.piles.to_vec(),
        };
    }
}

/// The picks of `part` that put `cards`, a pile's as listed, in `place`; none for no cards.
fn picked(part: Part, place: Option<PlaceName>, cards: &[ObjectId]) -> Option<Step> {
    let cards = cards.iter().map(|&ObjectId(slot)| Subject::Object { slot });
    if cards.len() == 0 {
        return None;
    }
    match picks(place) {
        Picks::Down => Some(Step::InTurn(part, cards.collect())),
        Picks::Up => Some(Step::InTurn(part, cards.rev().collect())),
        Picks::Any => Some(Step::Among(part, cards.collect())),
        Picks::None => None,
    }
}
