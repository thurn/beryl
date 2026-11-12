//! Collapsing the interchangeable: of objects that lead to the same game, one candidate each
//! (architecture §7.2).

use std::cell::OnceCell;

use crate::bitboard::Bitboard;
use crate::card::FaceIx;
use crate::layout::Layout;
use crate::state::{State, Zone};

/// Collapses interchangeable objects to one candidate each, the lowest slot (§7.2): objects
/// that differ only in their incarnation and timestamp lead to the same game. Everything
/// else counts: the face up, the owner (a stolen Mana Crypt goes to another graveyard than
/// your own), copy, link and chosen value as much as card, counters and flags. An object
/// another record of the state names ([`State::named`]: a target on the stack, a blocked
/// attacker, what a card is exiled with) is never collapsed with another, since its
/// signature doesn't show what refers to it.
///
/// Nor is a face-down permanent: whether two are the same card is hidden from all but their
/// controller ([CR 708.5](https://yawgatog.com/resources/magic-rules/#R7085)): "You can't look
/// at face-down cards in any other zone or face-down spells or permanents controlled by
/// another player." Offering one of them, not both, would tell the other player.
///
/// Nor are two copies of a card that different players know: in a hidden zone
/// ([CR 400.2](https://yawgatog.com/resources/magic-rules/#R4002)), "Library and hand are
/// hidden zones, even if all the cards in one such zone happen to be revealed", a copy the
/// other player has seen and one they haven't lead to different games, as playing the one they
/// know tells them nothing new and leaves them the other unknown.
pub(crate) fn collapse<L: Layout>(
    s: &State<L>,
    candidates: Bitboard<L::Slot>,
) -> Bitboard<L::Slot> {
    if candidates.count() < 2 {
        return candidates;
    }
    // Of interchangeable objects the candidate is the first in candidate order.
    collapse_ordered(s, s.in_candidate_order(candidates))
}

/// Whether two object slots are represented by one candidate (architecture §7.2). Read-only
/// payment previews use the same equivalence as real decisions, including named objects.
pub fn interchangeable<L: Layout>(s: &State<L>, a: L::Slot, b: L::Slot) -> bool {
    collapse(s, Bitboard::single(a) | Bitboard::single(b)).count() == 1
}

/// [`collapse`] keeping the first of interchangeable objects in the order of `slots`
/// rather than the lowest slot.
pub(crate) fn collapse_in_order<L: Layout>(
    s: &State<L>,
    slots: impl Iterator<Item = L::Slot>,
) -> Bitboard<L::Slot> {
    collapse_ordered(s, slots)
}

fn collapse_ordered<L: Layout>(
    s: &State<L>,
    slots: impl Iterator<Item = L::Slot>,
) -> Bitboard<L::Slot> {
    let signature = |slot: L::Slot| s.signature(slot);
    let known = |slot: L::Slot| {
        matches!(s.object(slot).zone(), Zone::Hand | Zone::Library).then(|| s.who_knows(slot))
    };
    // Found only once two candidates are of one card: until then every one is kept, named by
    // another record or not.
    let named = OnceCell::new();
    let mut kept = Bitboard::EMPTY;
    for slot in slots {
        // The signature holds the card, so objects of other cards are told apart without
        // building theirs.
        let of_card = |k: L::Slot| s.copiable(k).key() == s.copiable(slot).key();
        if s.object(slot).face() == FaceIx::DOWN || !kept.any(of_card) {
            kept.insert(slot);
            continue;
        }
        let named = *named.get_or_init(|| s.named());
        let own = OnceCell::new();
        if named.contains(slot)
            || !kept.any(|k| {
                !named.contains(k)
                    && of_card(k)
                    && known(k) == known(slot)
                    && signature(k) == *own.get_or_init(|| signature(slot))
            })
        {
            kept.insert(slot);
        }
    }
    kept
}
