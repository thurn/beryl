// Generated from the card store by `just card header`: notes go below the end marker.
//! Sink into Stupor {1}{U}{U}
//! Instant
//!
//! Return target spell or nonland permanent an opponent controls to its owner's hand.
//!
//! Soporific Springs
//! Land
//!
//! As this land enters, you may pay 3 life. If you don't, it enters tapped.
//! {T}: Add {U}.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = card(
    Card::SinkIntoStupor,
    &[
        Printed::SinkIntoStupor.abilities(&[Ability::spell()
            .target(&TargetSpec::one(
                TargetKind::SpellOrPermanent,
                Filter::All(&[
                    Filter::Whose(&Players::EachOpponent),
                    Filter::Not(&Filter::Types(Types::LAND)),
                ]),
            ))
            .effect(&return_to_hand(target()))]),
        Printed::SoporificSprings.abilities(&[
            Ability::replacing(&Replacement::enters_tapped_unless_pay_life(3)),
            Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)),
        ]),
    ],
);
