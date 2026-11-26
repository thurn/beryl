// Generated from the card store by `just card header`: notes go below the end marker.
//! Memory Lapse {1}{U}
//! Instant
//!
//! Counter target spell. If that spell is countered this way, put it on top of its owner's library
//! instead of into that player's graveyard.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::MemoryLapse
    .abilities(&[Ability::spell()
        .target(&TargetSpec::spell())
        .effect(&counter_to_top_of_library(target()))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        // Memory Lapse's self-replacement effect replaces going to the graveyard with going to
        // the top of the owner's library before Rest in Peace can replace it with exile.
        Scenario::new(
            "Memory Lapse puts a countered spell on top of its owner's library even with Rest in Peace",
        )
        .interaction(&[Card::RestInPeace])
        .battlefield(P0, &[Card::Island; 3])
        .hand(P0, &[Prop::UntargetedSpell])
        .battlefield(P1, &[Card::RestInPeace, Card::Island, Card::Island])
        .hand(P1, &[Card::MemoryLapse])
        .actions(&[
            ME.casts(Prop::UntargetedSpell),
            THEM.casts(Card::MemoryLapse),
            ME.has(Zone::Library, Prop::UntargetedSpell, 1),
            ME.has(Zone::Exile, Prop::UntargetedSpell, 0),
            THEM.has(Zone::Exile, Card::MemoryLapse, 1),
        ]),
        // Flashback exiles the spell if it would leave the stack for anywhere other than exile,
        // overriding Memory Lapse's self-replacement to the top of the library.
        Scenario::new("Memory Lapse countering a spell cast with flashback exiles it")
            .interaction(&[Card::SnapcasterMage])
            .battlefield(P0, &[Card::Island; 5])
            .hand(P0, &[Card::SnapcasterMage])
            .graveyard(P0, &[Prop::UntargetedSpell])
            .battlefield(P1, &[Card::Island; 2])
            .hand(P1, &[Card::MemoryLapse])
            .actions(&[
                ME.casts(Card::SnapcasterMage),
                Resolve,
                Resolve,
                ME.casts(Prop::UntargetedSpell),
                THEM.casts(Card::MemoryLapse),
                ME.has(Zone::Exile, Prop::UntargetedSpell, 1),
                ME.has(Zone::Library, Prop::UntargetedSpell, 0),
            ]),
        // A spell that can't be countered is not countered by Memory Lapse, so it stays on the
        // stack and resolves.
        Scenario::new("Memory Lapse on an uncounterable spell leaves it on the stack")
            .interaction(&[Card::CavernOfSouls])
            .battlefield(P0, &[Card::Plains])
            .hand(P0, &[Card::CavernOfSouls, Prop::Vanilla])
            .battlefield(P1, &[Card::Island; 2])
            .hand(P1, &[Card::MemoryLapse])
            .actions(&[
                ME.plays(Card::CavernOfSouls),
                ME.chooses_named("Elk"),
                ME.casts(Prop::Vanilla),
                THEM.casts(Card::MemoryLapse),
                Resolve,
                ME.controls(Prop::Vanilla, 1),
                ME.has(Zone::Library, Prop::Vanilla, 0),
            ]),
    ];
}
