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
    .effect(&DRAW_A_CARD)])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        // Reki is cast first, so it is not on the battlefield when the scenario starts.
        Scenario::new("Reki draws when its controller casts a legendary spell")
            .battlefield(
                P0,
                &[
                    Card::Forest,
                    Card::Forest,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .hand(
                P0,
                &[Card::RekiTheHistoryOfKamigawa, Card::CaelornaCoralTyrant],
            )
            .library(P0, &[Card::Plains; 3])
            .actions(&[
                ME.casts(Card::RekiTheHistoryOfKamigawa),
                Resolve,
                ME.has(Zone::Hand, Card::Plains, 0),
                ME.casts(Card::CaelornaCoralTyrant),
                Resolve,
                ME.has(Zone::Hand, Card::Plains, 1),
                THEM.has_cards(Zone::Hand, 0),
            ]),
        // An opponent's legendary spell is not "you cast": Reki's controller draws nothing.
        Scenario::new("Reki does not draw when an opponent casts a legendary spell")
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[Card::RekiTheHistoryOfKamigawa])
            .battlefield(P1, &[Card::Island, Card::Island])
            .hand(P1, &[Card::CaelornaCoralTyrant])
            .library(P0, &[Card::Plains; 3])
            .actions(&[
                THEM.casts(Card::CaelornaCoralTyrant),
                Resolve,
                ME.has(Zone::Hand, Card::Plains, 0),
            ]),
    ];
}
