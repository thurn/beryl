//! Samples hidden cards across suspended games. A card seen in a subgame is evidence about
//! which cards entered it, but its subgame slot gives no clue to its prior position.
//!
//! Implements: none. Samples states consistent with a player's observations.

use std::collections::BTreeMap;

use crate::ids::SlotId;
use crate::layout::Layout;
use crate::rng::SplitMix64;
use crate::state::{Flag, State, Zone};
use crate::subgame::OuterGame;
use crate::table::GameTable;

use super::Observation;

struct DeckSlot<L: Layout> {
    slot: L::Slot,
    known: Option<L::Card>,
    in_child: bool,
}

impl<L: Layout> Observation<'_, L> {
    /// Samples each library carried into a subgame from the parent game's cards. Knowledge
    /// in either game constrains that sample, and the rest of the parent game is sampled
    /// from the cards left over.
    pub(super) fn redeal_subgames(
        &self,
        table: &GameTable,
        rng: &mut SplitMix64,
        show_hands: bool,
        sampled: &mut State<L>,
    ) -> bool {
        let mut games = vec![self.root];
        while let Some(parent) = games.last().and_then(|state| state.suspended_game()) {
            games.push(parent);
        }
        games.reverse();

        // A hidden pick has additional filter constraints and is handled by the same
        // redeal that normally samples a paused resolution.
        if games.iter().any(|game| {
            Self::new(game, self.viewer)
                .unseen_hidden_picks(table)
                .is_some()
        }) {
            return false;
        }

        let mut plans = vec![Vec::new(); games.len()];
        for seat in self.root.players().in_seat_order() {
            let slots: Vec<_> = (0..table.deck_len(seat))
                .map(|i| L::Slot::deck(seat, i))
                .collect();
            let decks: Vec<_> = games
                .iter()
                .enumerate()
                .map(|(level, &game)| {
                    self.deck_slots(game, games.get(level + 1).copied(), &slots, show_hands)
                })
                .collect();
            let mut minimum = BTreeMap::new();
            let mut requirements = Vec::with_capacity(decks.len());
            for deck in decks.iter().rev() {
                requirements.push(minimum.clone());
                minimum = minimum_deck(deck, minimum);
            }
            let mut cards: Vec<L::Card> = table.deck(seat).collect();
            for ((deck, child_needs), plan) in decks
                .iter()
                .zip(requirements.into_iter().rev())
                .zip(&mut plans)
            {
                let Some(child) = sample_deck(deck, &child_needs, cards, rng, plan) else {
                    return false;
                };
                cards = child;
            }
        }

        let mut game = sampled;
        for plan in plans.into_iter().rev() {
            for (slot, card) in plan {
                if game.object(slot).flags.has(Flag::Copy) {
                    game.deal_card_really_is(slot, card);
                } else {
                    game.object_mut(slot).card = card;
                }
            }
            game.reseed(rng.next_u64());
            let Some(parent) = game.outer.get_mut() else {
                break;
            };
            game = parent;
        }
        true
    }

    fn deck_slots(
        &self,
        game: &State<L>,
        child: Option<&State<L>>,
        slots: &[L::Slot],
        show_hands: bool,
    ) -> Vec<DeckSlot<L>> {
        slots
            .iter()
            .copied()
            .filter_map(|slot| {
                let zone = game.object(slot).zone;
                if zone == Zone::Nowhere {
                    return None;
                }
                let known = (show_hands && zone == Zone::Hand)
                    || game.knows_card_really_is(self.viewer, slot);
                Some(DeckSlot {
                    slot,
                    known: known.then(|| game.card_really_is(slot)),
                    in_child: child.is_some_and(|child| child.object(slot).zone != Zone::Nowhere),
                })
            })
            .collect()
    }
}

/// Each identity seen among the child's slots must appear at least as often in that child
/// as either game reveals. Known cards outside those slots are additional requirements.
fn minimum_deck<L: Layout>(
    deck: &[DeckSlot<L>],
    mut child: BTreeMap<L::Card, usize>,
) -> BTreeMap<L::Card, usize> {
    let mut inside = BTreeMap::<_, usize>::new();
    for card in deck
        .iter()
        .filter(|slot| slot.in_child)
        .filter_map(|slot| slot.known)
    {
        *inside.entry(card).or_default() += 1;
    }
    for (card, count) in inside {
        let needed = child.entry(card).or_default();
        *needed = (*needed).max(count);
    }
    for card in deck
        .iter()
        .filter(|slot| !slot.in_child)
        .filter_map(|slot| slot.known)
    {
        *child.entry(card).or_default() += 1;
    }
    child
}

/// Deals from the available deck, reserving the cards the child needs before placing the
/// unobserved cards at random. Returns the actual multiset dealt into the child deck.
fn sample_deck<L: Layout>(
    deck: &[DeckSlot<L>],
    child_needs: &BTreeMap<L::Card, usize>,
    mut available: Vec<L::Card>,
    rng: &mut SplitMix64,
    plan: &mut Vec<(L::Slot, L::Card)>,
) -> Option<Vec<L::Card>> {
    let mut fixed_inside = Vec::new();
    let mut unknown_inside = Vec::new();
    let mut unknown_outside = Vec::new();
    for slot in deck {
        if let Some(card) = slot.known {
            remove(&mut available, card)?;
            if slot.in_child {
                fixed_inside.push(card);
            }
        } else if slot.in_child {
            unknown_inside.push(slot.slot);
        } else {
            unknown_outside.push(slot.slot);
        }
    }

    let mut inside = Vec::new();
    for (&card, &count) in child_needs {
        let fixed = fixed_inside.iter().filter(|&&fixed| fixed == card).count();
        for _ in fixed..count {
            remove(&mut available, card)?;
            inside.push(card);
        }
    }
    let remaining = unknown_inside.len().checked_sub(inside.len())?;
    if available.len() != remaining + unknown_outside.len() {
        return None;
    }
    rng.shuffle(&mut available);
    inside.extend(available.drain(..remaining));
    rng.shuffle(&mut inside);
    plan.extend(unknown_inside.into_iter().zip(inside.iter().copied()));
    plan.extend(unknown_outside.into_iter().zip(available));
    fixed_inside.extend(inside);
    Some(fixed_inside)
}

fn remove<C: Copy + Eq>(cards: &mut Vec<C>, card: C) -> Option<()> {
    let index = cards.iter().position(|candidate| *candidate == card)?;
    cards.swap_remove(index);
    Some(())
}
