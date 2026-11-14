// Generated from the card store by `just card header`: notes go below the end marker.
//! Containment Priest {1}{W}
//! Creature — Human Cleric, 2/2
//!
//! Flash
//! If a nontoken creature would enter and it wasn't cast, exile it instead.
// End of the generated card header.

use crate::prelude::*;

/// The second ability is a replacement effect ([CR 614.1a]): "Effects that use the word
/// 'instead' are replacement effects." Whether the object is a creature is asked of it as it
/// would exist on the battlefield ([CR 614.12]): "check the characteristics of the permanent as
/// it would exist on the battlefield". An enchantment that Opalescence would make a creature is
/// exiled; a land is not. A creature spell that resolves was cast, and enters. The Priest is not
/// on the battlefield as it enters, so it does not exile itself or a creature entering with it.
///
/// [CR 614.1a]: https://yawgatog.com/resources/magic-rules/#R6141a
/// [CR 614.12]: https://yawgatog.com/resources/magic-rules/#R61412
pub static CARD: CardDef = Printed::ContainmentPriest
    .keywords(Keywords::FLASH)
    .abilities(&[Ability::replacing(&Replacement::enters_not_cast(
        &Filter::All(&[Filter::Not(&Filter::Token), Filter::Types(Types::CREATURE)]),
        Zone::Exile,
    ))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const PRIEST: &str = Card::ContainmentPriest;
    const WISP: &str = Card::Flickerwisp;
    const SPIRIT_TOKEN: &str = "Spirit Token";

    pub const SCENARIOS: &[Scenario] = &[
        // The Priest is the opponent's, and replaces every player's creatures entering. Flickerwisp
        // was cast, so it enters. The creature it exiles wasn't cast as it returns at
        // the end step, so it is exiled instead.
        Scenario::new("a creature Flickerwisp exiled is exiled instead of returning")
            .interaction(&[WISP])
            .battlefield(P0, &[Card::Plains, Card::Plains, Card::Plains])
            .hand(P0, &[WISP])
            .battlefield(P1, &[Prop::SmallCreature, PRIEST])
            .library(P0, &[Card::Plains])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.casts(WISP),
                Resolve,
                ME.controls(WISP, 1),
                ME.targets(Prop::SmallCreature),
                Resolve,
                THEM.has(Zone::Exile, Prop::SmallCreature, 1),
                advance_to(1, TurnStep::End),
                Resolve,
                THEM.controls(Prop::SmallCreature, 0),
                THEM.has(Zone::Exile, Prop::SmallCreature, 1),
            ]),
        // A land is no creature, so it returns.
        Scenario::new("a land Flickerwisp exiled returns")
            .interaction(&[WISP])
            .battlefield(P0, &[Card::Plains, Card::Plains, Card::Plains])
            .hand(P0, &[WISP])
            .battlefield(P1, &[Card::Island, PRIEST])
            .library(P0, &[Card::Plains])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.casts(WISP),
                Resolve,
                ME.targets(Card::Island),
                Resolve,
                THEM.has(Zone::Exile, Card::Island, 1),
                advance_to(1, TurnStep::End),
                Resolve,
                THEM.controls(Card::Island, 1),
                THEM.has(Zone::Exile, Card::Island, 0),
            ]),
        // Under Opalescence, Glorious Anthem would be a creature on the battlefield, so it is
        // exiled instead of returning.
        Scenario::new("an enchantment Opalescence would make a creature is exiled")
            .interaction(&[WISP, Card::Opalescence, Card::GloriousAnthem])
            .battlefield(
                P0,
                &[Card::Opalescence, Card::Plains, Card::Plains, Card::Plains],
            )
            .hand(P0, &[WISP])
            .battlefield(P1, &[Card::GloriousAnthem, PRIEST])
            .library(P0, &[Card::Plains])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.casts(WISP),
                Resolve,
                ME.targets(Card::GloriousAnthem),
                Resolve,
                THEM.has(Zone::Exile, Card::GloriousAnthem, 1),
                advance_to(1, TurnStep::End),
                Resolve,
                THEM.controls(Card::GloriousAnthem, 0),
                THEM.has(Zone::Exile, Card::GloriousAnthem, 1),
            ]),
        // The Priest is not on the battlefield as it returns, so its ability does not exile it.
        Scenario::new("Containment Priest returning is not exiled by itself")
            .interaction(&[WISP])
            .battlefield(P0, &[PRIEST, Card::Plains, Card::Plains, Card::Plains])
            .hand(P0, &[WISP])
            .library(P0, &[Card::Plains])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.casts(WISP),
                Resolve,
                ME.targets(PRIEST),
                Resolve,
                ME.has(Zone::Exile, PRIEST, 1),
                advance_to(1, TurnStep::End),
                Resolve,
                ME.controls(PRIEST, 1),
                ME.has(Zone::Exile, PRIEST, 0),
            ]),
        // A token creature wasn't cast, but it is a token, so it enters.
        Scenario::new("a creature token enters")
            .interaction(&[Card::StaffOfTheStoryteller])
            .battlefield(P0, &[PRIEST, Card::Plains, Card::Plains])
            .hand(P0, &[Card::StaffOfTheStoryteller])
            .actions(&[
                ME.casts(Card::StaffOfTheStoryteller),
                Resolve,
                Resolve,
                ME.controls(SPIRIT_TOKEN, 1),
            ]),
    ];
}
