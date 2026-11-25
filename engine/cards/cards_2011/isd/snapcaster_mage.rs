// Generated from the card store by `just card header`: notes go below the end marker.
//! Snapcaster Mage {1}{U}
//! Creature — Human Wizard, 2/1
//!
//! Flash
//! When this creature enters, target instant or sorcery card in your graveyard gains flashback
//! until end of turn. The flashback cost is equal to its mana cost. (You may cast that card from
//! your graveyard for its flashback cost. Then exile it.)
// End of the generated card header.

use crate::prelude::*;

/// Flashback uses the targeted card's mana cost as the alternative cost. If the flashback
/// cost was paid, the card is exiled even if an effect would return the spell to hand
/// ([CR 702.34a]).
///
/// [CR 702.34a]: https://yawgatog.com/resources/magic-rules/#R70234a
pub static CARD: CardDef = Printed::SnapcasterMage
    .keywords(Keywords::FLASH)
    .abilities(&[Ability::triggered(&Trigger::ENTERS)
        .target(&TargetSpec::graveyard_card(Filter::All(&[
            instant_or_sorcery(),
            yours(),
        ])))
        .effect(&gain_flashback(target()))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const SNAPCASTER: &str = Card::SnapcasterMage;

    const SPELLBINDER: &str = Prop::Flier;

    pub const SCENARIOS: &[Scenario] = &[
        // Damn given flashback is cast for its mana cost only: the flashback cost is the
        // alternative cost, so overload can't be added (CR 118.9a), though P0 could pay it. The
        // two candidates are passing and the {B}{B} cast.
        Scenario::new("Snapcaster Mage: flashback is the one alternative cost")
            .interaction(&[Card::Damn])
            .battlefield(
                P0,
                &[
                    Card::Island,
                    Card::Island,
                    Card::Swamp,
                    Card::Swamp,
                    Card::Plains,
                    Card::Plains,
                ],
            )
            .hand(P0, &[SNAPCASTER])
            .graveyard(P0, &[Card::Damn])
            .battlefield(P1, &[SPELLBINDER])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.casts(SNAPCASTER),
                Resolve,
                Resolve,
                ME.deciding(DecisionKind::Priority, 2),
                ME.casts(Card::Damn),
                ME.targets(SPELLBINDER),
                Resolve,
                THEM.has(Zone::Graveyard, SPELLBINDER, 1),
                ME.has(Zone::Exile, Card::Damn, 1),
            ]),
    ];
}
