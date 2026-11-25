//! Neighbouring values of instructions that turn a permanent's face or meld it.

use beryl_card::{Effect, Objects};

use super::Kind;

/// A transform becomes a flip and a flip a transform; a meld exiles its source alone, which
/// melds nothing.
pub(super) fn turned(effect: &Effect, add: &mut impl FnMut(Kind, String, Effect)) {
    if let Effect::Transform(o) = *effect {
        add(Kind::EffectObjects, "flip".into(), Effect::Flip(o));
    } else if let Effect::Flip(o) = *effect {
        add(Kind::EffectObjects, "turn".into(), Effect::Transform(o));
    } else if let Effect::Meld(_) = *effect {
        let exile = Effect::Exile(Objects::This);
        add(Kind::EffectObjects, "exile only this".into(), exile);
    }
}
