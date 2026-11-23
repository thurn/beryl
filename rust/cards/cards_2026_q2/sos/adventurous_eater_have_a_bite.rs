// Generated from the card store by `just card header`: notes go below the end marker.
//! Adventurous Eater {2}{B}
//! Creature — Human Warlock, 3/2
//!
//! This creature enters prepared. (While it's prepared, you may cast a copy of its spell. Doing so
//! unprepares it.)
//!
//! Have a Bite {B}
//! Sorcery
//!
//! Put a +1/+1 counter on target creature. You gain 1 life.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = card_with_shape(
    Card::AdventurousEaterHaveABite,
    PrintedShape::AdventurousEater,
    &[
        Printed::AdventurousEater.abilities(&[Ability::replacing(&Replacement::ENTERS_PREPARED)]),
        Printed::HaveABite.abilities(&[Ability::spell().target(&TargetSpec::creature()).effect(
            &sequence(&[
                add_counters(target(), CounterKind::PlusOne, 1),
                gain_life(you(), 1),
            ]),
        )]),
    ],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const CARD_NAME: &str = Card::AdventurousEaterHaveABite;
    const EATER: &str = "Adventurous Eater";
    const BITE: &str = "Have a Bite";
    const SWAMP: &str = Card::Swamp;
    const ISLAND: &str = Card::Island;
    const PLAINS: &str = Card::Plains;
    const BOLT: &str = Card::LightningBolt;
    const COUNTER: &str = Prop::Instant;
    const OKO: &str = Card::OkoThiefOfCrowns;
    const OTHER: &str = Prop::Vanilla;
    const ENTRY: Scenario = Scenario::setup()
        .hand(P0, &[CARD_NAME])
        .battlefield(P0, &[SWAMP; 4]);

    pub const SCENARIOS: &[Scenario] = &[
        ENTRY
            .titled("Adventurous Eater prepares Have a Bite as it enters")
            .actions(&[
                ME.has(Zone::Hand, EATER, 1),
                ME.deciding(DecisionKind::Priority, 2),
                ME.casts(EATER),
                Resolve,
                ME.pt(EATER, 3, 2),
                ME.has(Zone::Exile, BITE, 1),
                ME.casts_face(BITE, FaceIx::BACK),
                ME.has(Zone::Exile, BITE, 0),
                Resolve,
                ME.pt(EATER, 4, 3),
                ME.life(21),
                ME.has(Zone::Graveyard, BITE, 0),
            ]),
        ENTRY
            .titled("Have a Bite cannot be cast during the opponent's turn")
            .interaction(&[BOLT])
            .hand(P0, &[CARD_NAME, BOLT])
            .battlefield(P0, &[SWAMP, SWAMP, SWAMP, SWAMP, Card::Mountain])
            .library(P1, &[ISLAND, ISLAND])
            .actions(&[
                ME.casts(EATER),
                Resolve,
                advance_to(2, TurnStep::PrecombatMain),
                THEM.passes(),
                ME.deciding(DecisionKind::Priority, 2),
                ME.has(Zone::Exile, BITE, 1),
            ]),
        ENTRY
            .titled("Have a Bite remains on the stack when Adventurous Eater leaves")
            .interaction(&[BOLT])
            .battlefield(P1, &[Card::Mountain, OTHER])
            .hand(P1, &[BOLT])
            .actions(&[
                ME.casts(EATER),
                Resolve,
                ME.casts_face(BITE, FaceIx::BACK),
                ME.targets(OTHER),
                THEM.casts(BOLT),
                THEM.targets(EATER),
                Resolve,
                ME.has(Zone::Graveyard, EATER, 1),
                THEM.counters(OTHER, CounterKind::PlusOne, 1),
                ME.life(21),
            ]),
        ENTRY
            .titled("Have a Bite does not resolve after losing its only target")
            .interaction(&[BOLT])
            .battlefield(P1, &[Card::Mountain])
            .hand(P1, &[BOLT])
            .actions(&[
                ME.casts(EATER),
                Resolve,
                ME.casts_face(BITE, FaceIx::BACK),
                THEM.casts(BOLT),
                THEM.targets(EATER),
                Resolve,
                ME.life(20),
                ME.has(Zone::Graveyard, EATER, 1),
                ME.has(Zone::Graveyard, BITE, 0),
                ME.has(Zone::Exile, BITE, 0),
            ]),
        ENTRY
            .titled("Countering Have a Bite leaves Adventurous Eater unprepared")
            .battlefield(P1, &[ISLAND, ISLAND])
            .hand(P1, &[COUNTER])
            .actions(&[
                ME.casts(EATER),
                Resolve,
                ME.casts_face(BITE, FaceIx::BACK),
                THEM.casts(COUNTER),
                ME.has(Zone::Graveyard, BITE, 0),
                ME.has(Zone::Exile, BITE, 0),
                ME.pt(EATER, 3, 2),
                ME.life(20),
            ]),
        ENTRY
            .titled("A token copy enters prepared and its original's copy ceases to exist")
            .interaction(&[Card::FracturedIdentity])
            .hand(P0, &[CARD_NAME, Card::FracturedIdentity])
            .battlefield(
                P0,
                &[SWAMP, SWAMP, SWAMP, PLAINS, ISLAND, ISLAND, ISLAND, ISLAND],
            )
            .battlefield(P1, &[SWAMP])
            .actions(&[
                ME.casts(EATER),
                Resolve,
                ME.has(Zone::Exile, BITE, 1),
                ME.casts(Card::FracturedIdentity),
                Resolve,
                ME.has(Zone::Exile, EATER, 1),
                ME.has(Zone::Exile, BITE, 0),
                THEM.controls(EATER, 1),
                THEM.has(Zone::Exile, BITE, 1),
                advance_to(2, TurnStep::PrecombatMain),
                THEM.casts_face(BITE, FaceIx::BACK),
                Resolve,
                THEM.token_pt(EATER, 4, 3),
            ]),
        ENTRY
            .titled("The new controller can cast an existing prepare spell copy")
            .interaction(&[OKO])
            .permanents(P1, &[Permanent::new(OKO).counters(CounterKind::Loyalty, 6)])
            .battlefield(P1, &[SWAMP, OTHER])
            .actions(&[
                ME.casts(EATER),
                Resolve,
                advance_to(2, TurnStep::PrecombatMain),
                THEM.activates(OKO, 2),
                Resolve,
                THEM.controls(EATER, 1),
                ME.has(Zone::Exile, BITE, 1),
                THEM.casts_face(BITE, FaceIx::BACK),
                THEM.targets(EATER),
                Resolve,
                THEM.pt(EATER, 4, 3),
                THEM.life(21),
                ME.has(Zone::Exile, BITE, 0),
            ]),
        ENTRY
            .titled("Losing all abilities does not unprepare Adventurous Eater")
            .interaction(&[OKO])
            .permanents(P1, &[Permanent::new(OKO).counters(CounterKind::Loyalty, 4)])
            .actions(&[
                ME.casts(EATER),
                Resolve,
                advance_to(2, TurnStep::PrecombatMain),
                THEM.activates(OKO, 1),
                Resolve,
                ME.pt(EATER, 3, 3),
                ME.has(Zone::Exile, BITE, 1),
                advance_to(3, TurnStep::PrecombatMain),
                ME.casts_face(BITE, FaceIx::BACK),
                Resolve,
                ME.pt(EATER, 4, 4),
            ]),
        ENTRY
            .titled("A prepared permanent retains its spell after it stops being a creature")
            .interaction(&[Card::SwiftReconfiguration])
            .hand(P0, &[CARD_NAME, Card::SwiftReconfiguration])
            .battlefield(P0, &[SWAMP, SWAMP, SWAMP, SWAMP, PLAINS])
            .battlefield(P1, &[OTHER])
            .actions(&[
                ME.casts(EATER),
                Resolve,
                ME.casts(Card::SwiftReconfiguration),
                ME.targets(EATER),
                Resolve,
                ME.creature(EATER, false),
                ME.has(Zone::Exile, BITE, 1),
                ME.casts_face(BITE, FaceIx::BACK),
                Resolve,
                THEM.counters(OTHER, CounterKind::PlusOne, 1),
            ]),
        ENTRY
            .titled("Have a Bite cannot be cast without a creature to target")
            .interaction(&[Card::SwiftReconfiguration])
            .hand(P0, &[CARD_NAME, Card::SwiftReconfiguration, ISLAND])
            .battlefield(P0, &[SWAMP, SWAMP, SWAMP, SWAMP, PLAINS])
            .actions(&[
                ME.casts(EATER),
                Resolve,
                ME.casts(Card::SwiftReconfiguration),
                Resolve,
                ME.creature(EATER, false),
                ME.deciding(DecisionKind::Priority, 2),
                ME.has(Zone::Exile, BITE, 1),
            ]),
        ENTRY
            .titled("A prepared permanent gets a new copy when it phases in")
            .interaction(&[Card::TeferisProtection])
            .hand(P0, &[CARD_NAME, Card::TeferisProtection])
            .battlefield(P0, &[SWAMP, SWAMP, SWAMP, SWAMP, PLAINS, PLAINS, PLAINS])
            .actions(&[
                ME.casts(EATER),
                Resolve,
                ME.has(Zone::Exile, BITE, 1),
                ME.casts(Card::TeferisProtection),
                Resolve,
                ME.has(Zone::Exile, BITE, 0),
                advance_to(3, TurnStep::PrecombatMain),
                ME.has(Zone::Exile, BITE, 1),
                ME.casts_face(BITE, FaceIx::BACK),
                Resolve,
                ME.pt(EATER, 4, 3),
            ]),
        ENTRY
            .titled("The inset is a card name a player can choose")
            .interaction(&[Card::PithingNeedle])
            .hand(P0, &[CARD_NAME, Card::PithingNeedle])
            .actions(&[
                ME.casts(Card::PithingNeedle),
                ME.chooses_named(BITE),
                ME.controls(Card::PithingNeedle, 1),
            ]),
    ];
}
