// Generated from the card store by `just card header`: notes go below the end marker.
//! Reki, the History of Kamigawa {2}{G}
//! Legendary Creature — Human Shaman, 1/2
//!
//! Whenever you cast a legendary spell, draw a card.
// End of the generated card header.

use crate::prelude::*;

/// "Whenever you cast a legendary spell, draw a card." Implemented so that Double Major's
/// scenario can show that a copy of a spell is not cast ([CR 707.10]): "a copy of a spell
/// isn't cast".
///
/// [CR 707.10]: https://yawgatog.com/resources/magic-rules/#R70710
pub static CARD: CardDef = Printed::RekiTheHistoryOfKamigawa
    .abilities(&[Ability::triggered(&Trigger::casts(
        Players::You,
        Filter::Supertypes(Supertypes::LEGENDARY),
    ))
    .effect(&draw(you(), 1))])
    .card();
