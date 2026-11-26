// Generated from the card store by `just card header`: notes go below the end marker.
//! Haunted Fengraf
//! Land
//!
//! {T}: Add {C}.
//! {3}, {T}, Sacrifice this land: Return a creature card at random from your graveyard to your
//! hand.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::HauntedFengraf
    .abilities(&[
        Ability::tap_for_mana(ManaSet::one(ManaColor::Colorless)),
        Ability::activated(Cost::of(mana("{3}"), &[CostPart::TAP, CostPart::SACRIFICE])).effect(
            &sequence(&[
                choose_at_random(cards_in(Zone::Graveyard, &you(), creature())),
                return_to_hand(those()),
            ]),
        ),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const FENGRAF: &str = Card::HauntedFengraf;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Haunted Fengraf returns a creature card from your graveyard to your hand")
            .battlefield(P0, &[FENGRAF, Card::Island, Card::Island, Card::Island])
            .permanents(P0, &[Permanent::new(FENGRAF).tapped()])
            .graveyard(P0, &[Prop::Vanilla, Prop::Instant])
            .graveyard(P1, &[Prop::Flier])
            .actions(&[
                ME.deciding(DecisionKind::Priority, 2),
                ME.activates(FENGRAF, 1),
                Resolve,
                ME.has(Zone::Hand, Prop::Vanilla, 1),
                ME.has(Zone::Graveyard, Prop::Instant, 1),
                ME.has(Zone::Graveyard, FENGRAF, 1),
                THEM.has(Zone::Graveyard, Prop::Flier, 1),
            ]),
        Scenario::new("Haunted Fengraf does not return an opponent's creature card")
            .battlefield(P0, &[FENGRAF, Card::Island, Card::Island, Card::Island])
            .graveyard(P1, &[Prop::Flier])
            .actions(&[
                ME.activates(FENGRAF, 1),
                Resolve,
                ME.has_cards(Zone::Hand, 0),
                THEM.has(Zone::Graveyard, Prop::Flier, 1),
                THEM.has_cards(Zone::Hand, 0),
            ]),
        Scenario::new(
            "Haunted Fengraf can return a creature card put into the graveyard in response",
        )
        .battlefield(
            P0,
            &[
                FENGRAF,
                Card::Island,
                Card::Island,
                Card::Island,
                Card::Island,
                Card::Island,
                Card::Swamp,
            ],
        )
        .hand(P0, &[Prop::FlashCreature])
        .battlefield(P1, &[Card::Island, Card::Island])
        .hand(P1, &[Prop::Instant])
        .actions(&[
            ME.activates(FENGRAF, 1),
            ME.casts(Prop::FlashCreature),
            THEM.casts(Prop::Instant),
            ME.has(Zone::Hand, Prop::FlashCreature, 1),
            ME.has(Zone::Graveyard, Prop::FlashCreature, 0),
        ]),
    ];
}
