//! Picks of cards from a zone that is not the table: a hand, the top of a library, a library
//! searched, a graveyard. A library's cards and a graveyard's are laid out in a tray; the
//! viewer's own hand is picked where it lies. None answers by a click: each has a button that
//! confirms what is picked, and one that declines where the prompt may answer with nothing.

use std::collections::VecDeque;

use beryl_engine::Layout;
use beryl_engine::card::Purpose;

use super::{Objects, Opens, card};
use crate::prompt::held::{Table, offered};
use crate::prompt::{Lies, PartName};
use crate::protocol::{Candidate, DecisionView, Side, ZoneName};
use crate::script::Subject;

impl Objects {
    /// A choice of cards from the zone `decision`'s prompt says they lie in, the open decision
    /// on `table`:
    ///
    /// - a search of a library, of the cards it can find, which may find nothing
    ///   ([CR 701.23b](https://yawgatog.com/resources/magic-rules/#R70123b): "that player
    ///   isn't required to find some or all of those cards");
    /// - a look at the top of a library or at a hand that may take one of its cards: asked
    ///   whether, then which, as one prompt that may take nothing;
    /// - cards of the viewer's hand to discard, to put on the bottom after a mulligan
    ///   ([CR 103.5](https://yawgatog.com/resources/magic-rules/#R1035)) or to exile for a
    ///   cost, and a discard of one card of a kind in place of several;
    /// - cards of a graveyard to exile for a cost
    ///   ([CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h): "The player pays the
    ///   total cost");
    /// - one of the cards on top of a library, as a manifest dread's
    ///   ([CR 701.62a](https://yawgatog.com/resources/magic-rules/#R70162a): "Look at the top
    ///   two cards of your library. Manifest one of them").
    ///
    /// A lone card a look or a search can take opens picked.
    pub(super) fn from_zone<L: Layout>(
        table: &Table<'_, L>,
        purpose: Purpose,
        decision: &DecisionView,
    ) -> Option<Objects> {
        let prompt = &decision.prompt;
        let lies = prompt.from?;
        let cards = table.laid_out(lies, prompt.count);
        let offered = offered(decision);
        let about = |slot: &u16| prompt.about.contains(slot);
        let in_graveyard = lies.zone == ZoneName::Graveyard;
        let (opens, takes) = match prompt.part {
            // A yes picks among the cards the question is about.
            PartName::Whether => (
                Opens::Whether,
                cards.iter().copied().filter(about).collect(),
            ),
            // Any card of the hand: one of those asked about alone, or the cards a no asks for.
            PartName::Instead => (
                Opens::Instead(cards.iter().copied().filter(about).map(card).collect()),
                cards.clone(),
            ),
            // A cost takes any of a graveyard's cards, whichever the engine offers first.
            PartName::Pick if purpose == Purpose::ExileCost && in_graveyard => {
                (Opens::Picks, cards.clone())
            }
            PartName::Pick => (Opens::Picks, table.takes(&cards, &offered)),
            _ => return None,
        };
        // A card offered that the cards laid out do not show is still one to pick.
        let missing = offered.iter().copied().filter(|slot| !takes.contains(slot));
        let takes: Vec<u16> = match opens {
            Opens::Picks => takes.iter().copied().chain(missing).collect(),
            _ => takes,
        };
        let ends = (decision.candidates.iter()).any(|c| c.option == Candidate::Done);
        let all = u8::try_from(takes.len()).unwrap_or(u8::MAX);
        let most = match opens {
            Opens::Instead(_) => prompt.most.min(all),
            _ => prompt.most.max(1).min(all),
        };
        let searched = lies.zone == ZoneName::Library && prompt.count == 0;
        let own_hand = lies
            == Lies {
                player: Side::You,
                zone: ZoneName::Hand,
            };
        let tray = if own_hand {
            Vec::new()
        } else if searched {
            // What a search can find, by name: the library's order is not the viewer's to
            // learn ([CR 701.24a](https://yawgatog.com/resources/magic-rules/#R70124a): the
            // shuffle that follows leaves it so "that no player knows their order").
            let name = |&slot: &u16| table.object(slot).and_then(|o| o.name.clone());
            let mut found = takes.clone();
            found.sort_by_key(|slot| (name(slot), *slot));
            found
        } else {
            cards.clone()
        };
        let lone = matches!(purpose, Purpose::Look | Purpose::Search) && takes.len() == 1;
        let pile = (opens == Opens::Picks && purpose == Purpose::ExileCost && in_graveyard)
            .then(|| Pile::of(table, &cards));
        Some(Objects {
            purpose,
            title: prompt.title.clone(),
            eligible: takes.iter().copied().map(card).collect(),
            tray,
            least: if ends || opens == Opens::Whether {
                0
            } else {
                most
            },
            most,
            picked: if lone {
                vec![card(takes[0])]
            } else {
                Vec::new()
            },
            opens,
            ends,
            confirmed: true,
            pile,
            friendly: None,
            warned: Vec::new(),
            aimed: Vec::new(),
            confirm: prompt.words.confirm.clone(),
            several: prompt.words.counted.clone(),
            decline: prompt.words.decline.clone(),
        })
    }
}

/// A pile whose cards the engine takes from the bottom up, each above the one before, so
/// that every set of cards is one sequence of answers (`beryl_engine`'s `exile_choices`): a
/// graveyard a cost exiles cards from. Of interchangeable cards it offers the lowest, and it
/// asks nothing where one answer is left.
#[derive(Clone, Debug)]
pub(super) struct Pile {
    /// The cards from the bottom up, each with its kind: the place in the pile of the lowest
    /// card it is interchangeable with.
    cards: Vec<(u16, usize)>,
}

impl Pile {
    /// The pile of `cards`, listed from the top down, on `table`.
    fn of<L: Layout>(table: &Table<'_, L>, cards: &[u16]) -> Pile {
        let up: Vec<u16> = cards.iter().rev().copied().collect();
        let kind = |i: usize| {
            (0..i)
                .find(|&lower| !table.takes(&up[i..=i], &up[lower..=lower]).is_empty())
                .unwrap_or(i)
        };
        Pile {
            cards: (up.iter().enumerate())
                .map(|(i, &slot)| (slot, kind(i)))
                .collect(),
        }
    }

    /// The picks the engine asks for, in its order, that take cards of the kinds of `picked`,
    /// as many of each: at each pick it offers the cards above the last taken that leave
    /// enough above them for the picks to come. A card of a kind still wanted is taken as the
    /// lowest of its kind offered, which leaves every later choice the picked one would; a
    /// pick with one kind to offer is not asked, and so is not among these.
    pub(super) fn asked(&self, picked: &[Subject]) -> VecDeque<Subject> {
        let kind_of = |on: &Subject| {
            let (_, kind) = (self.cards.iter()).find(|(slot, _)| card(*slot) == *on)?;
            Some(*kind)
        };
        let mut wanted: Vec<usize> = picked.iter().filter_map(kind_of).collect();
        let mut left = self.cards.clone();
        let mut from = 0;
        let mut asked = VecDeque::new();
        while !wanted.is_empty() && left.len() >= from + wanted.len() {
            let offered = &left[from..=left.len() - wanted.len()];
            let Some((kind, w)) = (offered.iter())
                .find_map(|(_, kind)| Some((*kind, wanted.iter().position(|k| k == kind)?)))
            else {
                break;
            };
            let lowest = (offered.iter())
                .position(|&(_, k)| k == kind)
                .expect("a card of the kind is offered");
            if offered.iter().any(|&(_, k)| k != kind) {
                asked.push_back(card(offered[lowest].0));
            }
            wanted.swap_remove(w);
            left.remove(from + lowest);
            from += lowest;
        }
        asked
    }
}
