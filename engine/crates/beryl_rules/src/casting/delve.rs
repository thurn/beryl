//! Delve ([CR 702.66a]): "For each generic mana in this spell's total cost, you may exile a
//! card from your graveyard rather than pay that mana." The part of a spell's cost that delve
//! adds, and how many cards it may take; `parts` pays it.
//!
//! Implements: CR 702.66a
//!
//! [CR 702.66a]: https://yawgatog.com/resources/magic-rules/#R70266a

use crate::bitboard::Bitboard;
use crate::card::{
    Castings, Chosen, CostAction, CostPart, Face, FaceIx, Keywords, ManaCost, Measure,
};
use crate::filter::Filter;
use crate::ids::{Seat, SlotId};
use crate::layout::Layout;
use crate::mana::{self, Funds};
use crate::state::{State, Zone};
use crate::table::GameTable;

use super::parts::{candidates, payable_with_substitutes};

/// What delve has its spell's caster do rather than pay generic mana
/// ([CR 702.66a]): "For each generic mana in this spell's total cost, you may exile a card
/// from your graveyard rather than pay that mana." A part of no cost printed on a card, which
/// the casting steps name by [`DELVE_PART`].
///
/// [CR 702.66a]: https://yawgatog.com/resources/magic-rules/#R70266a
pub(super) static DELVE: CostPart = CostPart::Chosen(
    CostAction::Exile,
    Chosen {
        zone: Zone::Graveyard,
        filter: Filter::Any,
        other: false,
        measure: Measure::ForGeneric(u8::MAX),
    },
);

/// The index the casting steps give [`DELVE`] among a spell's parts, past any it prints.
pub(super) const DELVE_PART: u8 = u8::MAX;

/// Whether `face`, a spell's, has delve. Delve functions only while its spell is on the
/// stack ([CR 702.66a]), where an effect that grants or removes abilities reaches no spell
/// this engine plays, so the face's keywords answer.
///
/// [CR 702.66a]: https://yawgatog.com/resources/magic-rules/#R70266a
pub(super) fn delves(face: &Face) -> bool {
    face.keywords.contains(Keywords::DELVE)
}

/// Whether `player` can pay `cost` for casting the card in `slot` as its face `face`, a spell
/// with delve, cast with the options `costs`, exiling cards from their graveyard for its
/// generic mana (CR 702.66a). The rest may be paid with restricted mana the spell accepts,
/// such as mana that may be spent only on creature spells of a chosen type
/// ([`mana::Paying`]). Delve may take only the cards the spell's other parts leave
/// ([`delve_most`]).
#[allow(clippy::too_many_arguments)]
#[allow(
    clippy::disallowed_methods,
    reason = "reads the face being cast (see CR 709.3b)"
)]
pub(super) fn delve_pays<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    funds: &Funds<L>,
    player: Seat,
    slot: L::Slot,
    face: FaceIx,
    costs: Castings,
    cost: ManaCost,
) -> bool {
    let cards = delve_candidates(s, t, player, slot);
    let most = delve_most(s.copiable(slot).turned(t, face).face(t), costs, cards);
    let paying = mana::Paying::casting(s, t, slot, face);
    payable_with_substitutes(s, t, funds, cost, most, cards, Bitboard::EMPTY, paying)
}

/// The cards in `player`'s graveyard that delve could exile for the spell whose card is in
/// `slot`.
pub(super) fn delve_candidates<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    slot: L::Slot,
) -> Bitboard<L::Slot> {
    let CostPart::Chosen(action, chosen) = DELVE else {
        unreachable!("delve exiles chosen cards");
    };
    candidates(s, t, player, slot, action, &chosen)
}

/// The most of `cards`, those delve could exile, it may exile for a spell of `face` cast with
/// the options `costs`: as many as leave enough for the spell's parts that also take cards
/// from the graveyard, such as escape's "exile three other cards from your graveyard". Delve
/// exiles its cards as the mana is paid, before those parts ([`super::parts::pay`] says why), and
/// [CR 601.2h] says "The player pays the total cost", so delve may not take the cards a later
/// part needs. Every such part in the card table takes any card ([`ESCAPE_EXILES`]), as delve
/// does, so a count is enough.
///
/// [CR 601.2h]: https://yawgatog.com/resources/magic-rules/#R6012h
/// [`ESCAPE_EXILES`]: crate::card::ESCAPE_EXILES
pub(super) fn delve_most<S: SlotId>(face: &Face, costs: Castings, cards: Bitboard<S>) -> u8 {
    let held: u32 = face
        .cost_parts(costs)
        .map(|part| match part {
            CostPart::Chosen(
                _,
                Chosen {
                    zone: Zone::Graveyard,
                    measure: Measure::Count(n),
                    ..
                },
            ) => u32::from(*n),
            CostPart::This(_)
            | CostPart::Chosen(..)
            | CostPart::Loyalty(_)
            | CostPart::Life(_)
            | CostPart::OneOf(_)
            | CostPart::DiscardHand => 0,
        })
        .sum();
    u8::try_from(cards.count().saturating_sub(held)).unwrap_or(u8::MAX)
}
