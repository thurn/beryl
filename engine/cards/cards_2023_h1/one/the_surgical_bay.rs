// Generated from the card store by `just card header`: notes go below the end marker.
//! The Surgical Bay
//! Land — Sphere
//!
//! This land enters tapped.
//! {T}: Add {U}.
//! {1}{U}, {T}, Sacrifice this land: Draw a card.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::TheSurgicalBay
    .abilities(&[
        Ability::replacing(&Replacement::ENTERS_TAPPED),
        Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)),
        Ability::activated(Cost::of(
            mana("{1}{U}"),
            &[CostPart::TAP, CostPart::SACRIFICE],
        ))
        .effect(&draw(you(), 1)),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] =
        &[
            Scenario::new("tapped Surgical Bay cannot activate its draw ability")
                .permanents(
                    P0,
                    &[
                        Permanent::new(Card::TheSurgicalBay).tapped(),
                        Permanent::new(Card::Island),
                        Permanent::new(Card::Island),
                    ],
                )
                .hand(P0, &[Card::Island])
                .actions(&[ME.deciding(DecisionKind::Priority, 2)]),
            Scenario::new(
                "The Surgical Bay is sacrificed to the shared graveyard and draws from the shared library",
            )
            .shared_library(&[Card::Plains])
            .battlefield(P0, &[Card::TheSurgicalBay, Card::Island, Card::Island])
            .actions(&[
                ME.activates(Card::TheSurgicalBay, 2),
                Resolve,
                ME.has(Zone::Graveyard, Card::TheSurgicalBay, 1),
                THEM.has(Zone::Graveyard, Card::TheSurgicalBay, 1),
                ME.has(Zone::Hand, Card::Plains, 1),
            ]),
        ];
}
