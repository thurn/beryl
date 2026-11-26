//! The cards of merged and melded permanents put into a graveyard or library, which their
//! owner arranges ([CR 731.3a]): "If a merged permanent is put into its owner's graveyard
//! or library, that player may arrange the new objects in any order. If it's put into its
//! owner's library, that player doesn't reveal the order." A melded permanent's two cards are
//! arranged the same way ([CR 713.8a]): "that player may arrange the two cards in any order."
//!
//! The kernel puts the cards there as the permanent moves, in the order they merged above the
//! permanent's own card (`State::move_object`), since a mutator can't ask. After the batch,
//! [`open`] opens a question, [`Pending::ArrangePiles`], for each owner with two or more such
//! cards in one zone: the owner picks them one at a time, lowest first, and the last is left on
//! top. In a library, the other players forget the cards first, so the order stays hidden.
//!
//! The question waits for the engine's next stop, as the order of permanents that enter
//! together does (`arrival_timestamps`), rather than pausing the move: a batch can be part of
//! a state-based action, which has no resolution to pause. A batch moved while another question
//! is open (as a cost being paid) keeps the kernel's order without asking.
//!
//! Implements: CR 713.8a, CR 731.3a
//!
//! [CR 731.3a]: https://yawgatog.com/resources/magic-rules/#R7313a
//! [CR 713.8a]: https://yawgatog.com/resources/magic-rules/#R7138a

use crate::bitboard::Bitboard;
use crate::card::{Prompt, Purpose};
use crate::decision::{Decision, DecisionKind};
use crate::engine::collapse;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::state::{Pending, State, Zone};

/// The zones whose cards an owner arranges, in the order they are asked about.
const ZONES: [Zone; 2] = [Zone::Graveyard, Zone::Library];

/// The cards of the merged or melded permanents among `moves`, each a slot with where it was
/// and where it goes, that leave the battlefield for a graveyard or library, the permanents'
/// own cards among them. A token that ceases to exist leaves its cards in a graveyard. Read
/// before the batch moves, which ends what the stores hold of the piles; the caller asks only
/// while some permanent is made of several cards (`State::has_piles`).
#[cold]
#[inline(never)]
pub(crate) fn leaving<L: Layout>(
    s: &State<L>,
    moves: impl Iterator<Item = (L::Slot, Zone, Zone)>,
) -> Bitboard<L::Slot> {
    let mut cards = Bitboard::EMPTY;
    for (slot, from, to) in moves {
        if from != Zone::Battlefield
            || !matches!(to, Zone::Graveyard | Zone::Library | Zone::Nowhere)
        {
            continue;
        }
        let before = cards;
        for card in s.pile(slot) {
            cards.insert(card);
        }
        if cards != before {
            cards.insert(slot);
        }
    }
    cards
}

/// Opens the question of arranging `cards`, those of [`leaving`] once the batch has moved, for
/// each owner with two or more of them in one graveyard or library, and hides the order of
/// those in a library from the other players.
#[cold]
#[inline(never)]
pub(crate) fn open<L: Layout>(s: &mut State<L>, cards: Bitboard<L::Slot>) {
    let mut group = Bitboard::EMPTY;
    for player in s.players_in_turn_order(s.players()) {
        for zone in ZONES {
            let theirs = owned_in(s, cards, player, zone);
            if theirs.count() < 2 {
                continue;
            }
            group |= theirs;
            if zone == Zone::Library {
                s.hide_library_order(player, theirs);
            }
        }
    }
    if !group.is_empty() && matches!(s.pending(), Pending::None) {
        s.ask(Pending::ArrangePiles { group });
    }
}

/// The question of [`Pending::ArrangePiles`]: the first owner in turn order with cards of
/// `group` picks the one of them in their graveyard, or else their library, to put lowest.
pub(crate) fn decision<L: Layout>(s: &State<L>, group: Bitboard<L::Slot>) -> Decision<L> {
    let (player, theirs) = next_to_arrange(s, group);
    Decision::objects(
        player,
        DecisionKind::ChooseObject,
        collapse(s, theirs),
        Prompt::new(Purpose::PileOrder).most(1),
    )
}

/// Answers the question of [`Pending::ArrangePiles`] with `picked`, which goes lowest of the
/// cards its owner has left to arrange in its zone. Once one of those is left, it stays on top,
/// and the next zone or owner is asked, if any.
pub(crate) fn pick<L: Layout>(
    s: &mut State<L>,
    group: Bitboard<L::Slot>,
    picked: L::Slot,
) -> Result<(), Full> {
    let (_, theirs) = next_to_arrange(s, group);
    // In the shared-deck format the cards have no owner, and the active player arranges them.
    let lowest = s.arrange_lowest(picked, theirs, s.turn().active())?;
    let mut left = group;
    left.remove(lowest);
    if (left & theirs).count() < 2 {
        left &= !theirs;
    }
    if left.is_empty() {
        s.settle_question();
    } else {
        s.ask(Pending::ArrangePiles { group: left });
    }
    Ok(())
}

/// The first owner in turn order with cards of `group` in their graveyard or library, with
/// those of them in the first of those zones that holds any.
fn next_to_arrange<L: Layout>(s: &State<L>, group: Bitboard<L::Slot>) -> (Seat, Bitboard<L::Slot>) {
    s.players_in_turn_order(s.players())
        .flat_map(|player| ZONES.map(|zone| (player, owned_in(s, group, player, zone))))
        .find(|(_, theirs)| !theirs.is_empty())
        .expect("a question of arranging cards with none left")
}

/// The cards of `cards` in `zone` that `player` arranges ([`arranger`]).
fn owned_in<L: Layout>(
    s: &State<L>,
    cards: Bitboard<L::Slot>,
    player: Seat,
    zone: Zone,
) -> Bitboard<L::Slot> {
    cards.filter(|slot| s.object(slot).zone() == zone && arranger(s, slot) == player)
}

/// The player who arranges the card in `slot` among the others of its pile: its owner, or for
/// a card in a shared graveyard or library, which has no owner ([`State::has_owner`]), the
/// active player. CR 731.3a gives the choice to the owner, and the shared-deck format has
/// none, so the active player makes it as they make the first of simultaneous choices (see
/// [CR 101.4]): "the active player ... makes any choices required, then the next player in
/// turn order".
///
/// [CR 101.4]: https://yawgatog.com/resources/magic-rules/#R1014
fn arranger<L: Layout>(s: &State<L>, slot: L::Slot) -> Seat {
    if s.has_owner(slot) {
        s.object(slot).owner()
    } else {
        s.turn().active()
    }
}
