//! Permissions from permanents to cast a spell from a graveyard once each turn
//! ([`beryl_card::Ability::cast_from_graveyard_each_turn`]), as "Once during each of your
//! turns, you may cast a permanent spell with mana value 2 or less from your graveyard" says.
//!
//! A permission belongs to the permanent with the ability: a second such permanent gives a
//! second cast in the same turn, and a permanent that enters later in the turn is a new
//! object whose permission is unused. The spell is cast with the usual timing and costs. A
//! spell cast from the graveyard under another permission, such as flashback or escape,
//! uses none of these.
//!
//! Implements: none. Finds the permissions a static ability gives for casting legality.

use beryl_card::{Face, Words};

use crate::ids::Seat;
use crate::layout::Layout;
use crate::state::{Memo, State, Zone};
use crate::table::{Capability, GameTable};

/// The permanent whose unused permission lets `player` cast `face` from their graveyard with
/// `x` as the value of X, if one does: the first such permanent in battlefield order.
pub(super) fn permitter<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    face: &Face,
    x: u8,
) -> Option<L::Slot> {
    s.battlefield().iter().copied().find(|&slot| {
        s.controller(slot) == player
            && !s.cast_from_graveyard_this_turn(slot)
            && s.abilities(t, slot).any(|(_, ability)| {
                ability.graveyard_cast().is_some_and(|spells| {
                    let words: Words = s.words_of(slot);
                    crate::filter::matches_cast_face(spells, face, player, &words, x)
                })
            })
    })
}

/// Whether the card in `slot` is in `player`'s own graveyard while a card of the game gives
/// such permissions, so that casting it at priority without another permission relies on
/// one.
#[inline]
pub(super) fn relies_on_permission<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    slot: L::Slot,
) -> bool {
    t.can(Capability::CastsFromGraveyard) && {
        let object = s.object(slot);
        object.zone() == Zone::Graveyard && object.owner() == player
    }
}

/// The cards in `player`'s graveyard while they control a permanent whose permission is
/// unused: the cards a priority decision looks at for casts under such a permission. Which
/// faces of them a permission accepts is checked as each is offered.
#[cold]
#[inline(never)]
pub(super) fn sources<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
) -> crate::bitboard::Bitboard<L::Slot> {
    let any_unused = s.battlefield().iter().any(|&slot| {
        s.controller(slot) == player
            && !s.cast_from_graveyard_this_turn(slot)
            && s.abilities(t, slot)
                .any(|(_, ability)| ability.graveyard_cast().is_some())
    });
    if any_unused {
        s.zone(crate::state::ZoneKey::Of(player, Zone::Graveyard))
    } else {
        crate::bitboard::Bitboard::EMPTY
    }
}

/// Uses the permission of the first permanent that accepts the spell just cast, `face` with
/// `x`, as the spell becomes cast.
pub(super) fn use_permission<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    player: Seat,
    face: &Face,
    x: u8,
) -> Result<(), crate::fixed::Full> {
    if let Some(permitter) = permitter(s, t, player, face, x) {
        s.remember(permitter, Memo::CastFromGraveyard)?;
    }
    Ok(())
}
