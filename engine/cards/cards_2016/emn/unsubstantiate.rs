// Generated from the card store by `just card header`: notes go below the end marker.
//! Unsubstantiate {1}{U}
//! Instant
//!
//! Return target spell or creature to its owner's hand.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::Unsubstantiate
    .abilities(&[Ability::spell()
        .target(&TargetSpec::one(
            TargetKind::SpellOrPermanent,
            Filter::AnyOf(&[Filter::InZone(Zone::Stack), Filter::Types(Types::CREATURE)]),
        ))
        .effect(&return_to_hand(target()))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        // Returning a spell to its owner's hand does not counter it, so Unsubstantiate works on
        // a spell that can't be countered.
        Scenario::new("Unsubstantiate returns an uncounterable spell to its owner's hand")
            .interaction(&[Card::CavernOfSouls])
            .battlefield(P0, &[Card::Plains])
            .hand(P0, &[Card::CavernOfSouls, Prop::Vanilla])
            .battlefield(P1, &[Card::Island; 2])
            .hand(P1, &[Card::Unsubstantiate])
            .actions(&[
                ME.plays(Card::CavernOfSouls),
                ME.chooses_named("Elk"),
                ME.casts(Prop::Vanilla),
                THEM.casts(Card::Unsubstantiate),
                ME.has(Zone::Hand, Prop::Vanilla, 1),
                ME.controls(Prop::Vanilla, 0),
            ]),
        // Unsubstantiate can target any spell on the stack, including a noncreature spell.
        Scenario::new("Unsubstantiate returns a noncreature spell to its owner's hand")
            .battlefield(P0, &[Card::Island; 3])
            .hand(P0, &[Prop::UntargetedSpell])
            .battlefield(P1, &[Card::Island; 2])
            .hand(P1, &[Card::Unsubstantiate])
            .actions(&[
                ME.casts(Prop::UntargetedSpell),
                THEM.casts(Card::Unsubstantiate),
                ME.has(Zone::Hand, Prop::UntargetedSpell, 1),
                ME.has(Zone::Graveyard, Prop::UntargetedSpell, 0),
            ]),
    ];
}
