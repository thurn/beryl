//! Neighbouring values of an offer to cast a card during a resolution.

use beryl_card::{CastFace, Effect};

use super::Kind;

/// A free cast becomes one that pays, and one that pays a free one. An offer that casts face
/// down or is limited by the mana spent on X also casts face up, and flips that limit.
pub(super) fn may_cast(effect: &Effect, add: &mut impl FnMut(Kind, String, Effect)) {
    let Effect::MayCast {
        player,
        card,
        free,
        free_below,
        copy,
        face,
        within_x,
    } = *effect
    else {
        return;
    };
    let offer = |free, free_below, face, within_x| Effect::MayCast {
        player,
        card,
        free,
        free_below,
        copy,
        face,
        within_x,
    };
    add(
        Kind::FreeCast,
        format!("free: {}", !free),
        offer(!free, None, face, within_x),
    );
    if face != CastFace::Up || within_x {
        let up = offer(free, free_below, CastFace::Up, within_x);
        add(Kind::FreeCast, "face: Up".to_owned(), up);
        let limit = offer(free, free_below, face, !within_x);
        add(Kind::FreeCast, format!("within_x: {}", !within_x), limit);
    }
}
