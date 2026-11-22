// Generated from the card store by `just card header`: notes go below the end marker.
//! Bloodbraid Elf {2}{R}{G}
//! Creature — Elf Berserker, 3/2
//!
//! Haste (This creature can attack and {T} as soon as it comes under your control.)
//! Cascade (When you cast this spell, exile cards from the top of your library until you exile a
//! nonland card that costs less. You may cast it without paying its mana cost. Put the exiled cards
//! on the bottom in a random order.)
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::BloodbraidElf
    .keywords(Keywords::HASTE)
    .abilities(&[Ability::cascade()])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const ELF: &str = Card::BloodbraidElf;
    const BOLT: &str = Card::LightningBolt;
    const GEYSER: &str = Card::Braingeyser;
    const COUNTER: &str = Card::Counterspell;
    const MAJOR: &str = Card::DoubleMajor;
    const LANDS: &[&str] = &[Card::Mountain, Card::Forest, Card::Forest, Card::Forest];

    const fn setup(name: &'static str, library: &'static [&'static str]) -> Scenario {
        Scenario::new(name)
            .battlefield(P0, LANDS)
            .hand(P0, &[ELF])
            .library(P0, library)
    }

    pub const SCENARIOS: &[Scenario] = &[
        setup(
            "Cascade casts a targeted instant above the Elf",
            &[Card::Mountain, ELF, BOLT, Card::Plains],
        )
        .interaction(&[BOLT])
        .actions(&[
            ME.casts(ELF),
            stack(2),
            Resolve,
            ME.has(Zone::Exile, Card::Mountain, 1),
            ME.has(Zone::Exile, ELF, 1),
            ME.has(Zone::Exile, BOLT, 1),
            ME.chooses(DecisionKind::CastOffer, BOLT),
            ME.targets_player(THEM),
            stack(2),
            ME.has_cards(Zone::Exile, 0),
            ME.library_top(&[Card::Plains]),
            Resolve,
            THEM.life(17),
            ME.controls(ELF, 0),
            Resolve,
            ME.controls(ELF, 1),
        ]),
        setup("The Elf attacks on the turn it is cast", &[Card::Forest]).actions(&[
            ME.casts(ELF),
            Resolve,
            Resolve,
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(ELF),
            advance_to(1, TurnStep::EndOfCombat),
            THEM.life(17),
        ]),
        setup("Cascade casts a spell with X equal to zero", &[GEYSER])
            .interaction(&[GEYSER])
            .actions(&[
                ME.casts(ELF),
                Resolve,
                ME.chooses(DecisionKind::CastOffer, GEYSER),
                ME.targets_player(THEM),
                stack(2),
                Resolve,
                THEM.has_cards(Zone::Hand, 0),
                ME.has(Zone::Graveyard, GEYSER, 1),
            ]),
        setup(
            "Declining the cast puts the matching card on the bottom",
            &[BOLT, Card::Forest],
        )
        .interaction(&[BOLT])
        .actions(&[
            ME.casts(ELF),
            Resolve,
            ME.answers(DecisionKind::CastOffer, Pick::Pass),
            ME.library_top(&[Card::Forest, BOLT]),
            ME.has_cards(Zone::Exile, 0),
        ]),
        setup(
            "Cascade bottoms the whole library when no card qualifies",
            &[ELF, Card::Forest, Card::Mountain],
        )
        .actions(&[
            ME.casts(ELF),
            Resolve,
            stack(1),
            ME.has_cards(Zone::Library, 3),
            ME.has_cards(Zone::Exile, 0),
        ]),
        setup("Cascade resolves even when the Elf is countered", &[BOLT])
            .interaction(&[COUNTER, BOLT])
            .battlefield(P1, &[Card::Island, Card::Island])
            .hand(P1, &[COUNTER])
            .actions(&[
                ME.casts(ELF),
                THEM.casts(COUNTER),
                Resolve,
                ME.has(Zone::Graveyard, ELF, 1),
                stack(1),
                Resolve,
                ME.chooses(DecisionKind::CastOffer, BOLT),
                ME.targets_player(THEM),
                Resolve,
                THEM.life(17),
            ]),
        setup(
            "Copying the Elf spell does not trigger cascade again",
            &[BOLT],
        )
        .interaction(&[MAJOR, BOLT])
        .battlefield(
            P0,
            &[
                Card::Mountain,
                Card::Forest,
                Card::Forest,
                Card::Forest,
                Card::Forest,
                Card::Island,
            ],
        )
        .hand(P0, &[ELF, MAJOR])
        .actions(&[
            ME.casts(ELF),
            ME.casts(MAJOR),
            Resolve,
            stack(3),
            Resolve,
            ME.controls(ELF, 1),
            stack(2),
            Resolve,
            ME.chooses(DecisionKind::CastOffer, BOLT),
            ME.targets_player(THEM),
            Resolve,
            Resolve,
            ME.controls(ELF, 2),
            THEM.life(17),
        ]),
    ];
}
