// Generated from the card store by `just card header`: notes go below the end marker.
//! Flickerwisp {1}{W}{W}
//! Creature — Elemental, 3/1
//!
//! Flying
//! When this creature enters, exile another target permanent. Return that card to the battlefield
//! under its owner's control at the beginning of the next end step.
// End of the generated card header.

use crate::prelude::*;

/// The delayed ability is ability 1. It carries the exiled object across the delay and
/// returns it even if Flickerwisp has left ([CR 603.7c]). A token ceases to exist in exile
/// and cannot return ([CR 111.7]).
///
/// [CR 603.7c]: https://yawgatog.com/resources/magic-rules/#R6037c
/// [CR 111.7]: https://yawgatog.com/resources/magic-rules/#R1117
pub static CARD: CardDef = Printed::Flickerwisp
    .keywords(Keywords::FLYING)
    .abilities(&[
        Ability::triggered(&Trigger::ENTERS)
            .target(&TargetSpec::permanent(Filter::Any).other())
            .effect(&sequence(&[exile(target()), delay_with(1, those())])),
        Ability::triggered(&Trigger::step(TurnStep::End, Players::EachPlayer).next())
            .effect(&return_to_battlefield(carried_object())),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const SPIRIT_TOKEN: &str = "Spirit Token";
    const WISP: &str = Card::Flickerwisp;

    pub const SCENARIOS: &[Scenario] = &[
        // A token that is exiled ceases to exist and does not return. P0's Staff of the
        // Storyteller makes the Spirit token, and the Flickerwisp exiles it.
        Scenario::new("Flickerwisp exiles a token, which does not return")
            .interaction(&[Card::StaffOfTheStoryteller])
            .hand(P0, &[Card::StaffOfTheStoryteller, WISP])
            .battlefield(P0, &[Card::Plains; 5])
            .library(P0, &[Card::Plains])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.casts(Card::StaffOfTheStoryteller),
                Resolve,
                Resolve,
                Resolve,
                ME.controls(SPIRIT_TOKEN, 1),
                ME.casts(WISP),
                Resolve,
                ME.targets(SPIRIT_TOKEN),
                Resolve,
                ME.controls(SPIRIT_TOKEN, 0),
                advance_to(1, TurnStep::End),
                Resolve,
                ME.controls(SPIRIT_TOKEN, 0),
                ME.has(Zone::Exile, SPIRIT_TOKEN, 0),
            ]),
        // The exiled creature loses its counter and its attachments. The Aura goes to the
        // graveyard, and the Equipment stays on the battlefield without giving its bonus.
        Scenario::new("Flickerwisp returns a creature without its counter or attachments")
            .interaction(&[Card::HolyStrength])
            .hand(P0, &[WISP, Card::HolyStrength])
            .permanents(
                P0,
                &[
                    Permanent::new(Prop::Vanilla).counters(CounterKind::PlusOne, 1),
                    Permanent::new(Prop::Equipment),
                    Permanent::new(Card::Plains),
                    Permanent::new(Card::Plains),
                    Permanent::new(Card::Plains),
                    Permanent::new(Card::Plains),
                    Permanent::new(Card::Plains),
                ],
            )
            .actions(&[
                ME.casts(Card::HolyStrength),
                Resolve,
                ME.activates(Prop::Equipment, 1),
                Resolve,
                ME.pt(Prop::Vanilla, 5, 5),
                ME.casts(WISP),
                Resolve,
                ME.targets(Prop::Vanilla),
                Resolve,
                ME.has(Zone::Graveyard, Card::HolyStrength, 1),
                ME.controls(Prop::Equipment, 1),
                advance_to(1, TurnStep::End),
                Resolve,
                ME.pt(Prop::Vanilla, 2, 1),
                ME.counters(Prop::Vanilla, CounterKind::PlusOne, 0),
            ]),
        // Underworld Breach enters after the end step starts, so its end step sacrifice never
        // triggers that turn. The delayed return also survives the loss of Flickerwisp.
        Scenario::new(
            "Underworld Breach returns after Flickerwisp dies and misses the end step trigger",
        )
        .interaction(&[Card::UnderworldBreach, Card::LightningBolt])
        .hand(P0, &[WISP, Card::LightningBolt])
        .battlefield(
            P0,
            &[Card::Plains, Card::Plains, Card::Plains, Card::Mountain],
        )
        .battlefield(P1, &[Card::UnderworldBreach])
        .library(P1, &[Card::Island; 2])
        .actions(&[
            ME.casts(WISP),
            Resolve,
            ME.targets(Card::UnderworldBreach),
            Resolve,
            ME.casts(Card::LightningBolt),
            ME.targets(WISP),
            Resolve,
            ME.has(Zone::Graveyard, WISP, 1),
            advance_to(1, TurnStep::End),
            Resolve,
            stack(0),
            THEM.controls(Card::UnderworldBreach, 1),
        ]),
    ];
}
