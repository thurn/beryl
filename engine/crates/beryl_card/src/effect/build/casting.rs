//! Builders for instructions that let a player cast a card.

use crate::continuous::Duration;
use crate::effect::{CastTerms, Effect, Objects, Players};

/// [`Effect::GainFlashback`]: "[cards] gain flashback until end of turn. The flashback cost
/// is equal to its mana cost."
pub const fn gain_flashback(cards: Objects) -> Effect {
    Effect::GainFlashback(cards)
}

/// [`Effect::PermitCast`]: "[player] may cast [cards] [duration]", on `terms`.
pub const fn permit_cast(
    player: Players,
    cards: Objects,
    duration: Duration,
    terms: CastTerms,
) -> Effect {
    Effect::PermitCast {
        player,
        cards,
        duration,
        terms,
    }
}

/// [`Effect::MayCast`] for nothing: "[player] may cast [card] without paying its mana cost".
pub const fn may_cast_free(player: Players, card: Objects) -> Effect {
    Effect::MayCast {
        player,
        card,
        free: true,
        free_below: None,
        copy: false,
    }
}

/// [`Effect::MayCast`] of a copy: "copy [card]. [Player] may cast the copy without paying its
/// mana cost".
pub const fn may_cast_copy_free(player: Players, card: Objects) -> Effect {
    Effect::MayCast {
        player,
        card,
        free: true,
        free_below: None,
        copy: true,
    }
}
