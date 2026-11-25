//! Builders for instructions that let a player cast a card.

use crate::continuous::Duration;
use crate::effect::{CastFace, CastTerms, Effect, Objects, Players};

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
        face: CastFace::Up,
        within_x: false,
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
        face: CastFace::Up,
        within_x: false,
    }
}

/// [`Effect::MayCast`] of a card chosen by the mana spent on X, cast face down: "[player] may
/// choose [card] whose mana cost could be paid by some amount of, or all of, the mana you
/// spent on {X}. If [they] do, [they] may cast that card face down as a 2/2 creature spell
/// without paying its mana cost", whose permanent is turned face up as it would be used
/// ([`CastFace::DownTurnedUpWhenUsed`]).
pub const fn may_cast_face_down_within_x(player: Players, card: Objects) -> Effect {
    Effect::MayCast {
        player,
        card,
        free: true,
        free_below: None,
        copy: false,
        face: CastFace::DownTurnedUpWhenUsed,
        within_x: true,
    }
}
