// Generated from the card store by `just card header`: notes go below the end marker.
//! Containment Priest {1}{W}
//! Creature — Human Cleric, 2/2
//!
//! Flash
//! If a nontoken creature would enter and it wasn't cast, exile it instead.
// End of the generated card header.

use crate::prelude::*;

/// Opalescence can make an entering enchantment a creature, so the Priest can exile it.
/// [CR 614.12] says to “check the characteristics of the permanent as it would exist on the
/// battlefield.” The Priest applies from the battlefield. It is still outside that zone as
/// it and any creatures entering with it are checked.
///
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
    const IMPERSONATOR: &str = Card::CleverImpersonator;
    const SPIRIT_TOKEN: &str = "Spirit Token";

    const fn flicker_impersonator(name: &'static str) -> Scenario {
        const PERMANENTS: &[Permanent] = &[
            // XMage adds setup counters in the first main phase. Keep the 0/0 alive until then.
            Permanent::new(Card::GloriousAnthem),
            Permanent::new(IMPERSONATOR).counters(CounterKind::PlusOne, 1),
            Permanent::new(Prop::Equipment),
            Permanent::new(Card::Plains),
            Permanent::new(Card::Plains),
            Permanent::new(Card::Plains),
        ];
        Scenario::new(name)
            .interaction(&[WISP, IMPERSONATOR, Card::GloriousAnthem])
            .permanents(P0, PERMANENTS)
            .hand(P0, &[WISP])
            .battlefield(P1, &[Prop::Vanilla, PRIEST])
            .library(P0, &[Card::Plains])
            .library(P1, &[Card::Plains])
    }

    const fn flicker_and_choose(choice: Action, after: &[Action; 2]) -> [Action; 10] {
        [
            ME.casts(WISP),
            Resolve,
            ME.targets(IMPERSONATOR),
            Resolve,
            ME.has(Zone::Exile, IMPERSONATOR, 1),
            advance_to(1, TurnStep::End),
            Resolve,
            choice,
            after[0],
            after[1],
        ]
    }

    pub const SCENARIOS: &[Scenario] = &[
        // The Priest is the opponent's, and replaces every player's creatures entering. Flickerwisp
        // was cast, so it enters. The creature it exiles wasn't cast as it returns at
        // the end step, so it is exiled instead.
        Scenario::new("a creature Flickerwisp exiled is exiled instead of returning")
            .interaction(&[WISP])
            .battlefield(P0, &[Card::Plains, Card::Plains, Card::Plains])
            .hand(P0, &[WISP])
            .battlefield(P1, &[Prop::Vanilla, PRIEST])
            .library(P0, &[Card::Plains])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.casts(WISP),
                Resolve,
                ME.controls(WISP, 1),
                ME.targets(Prop::Vanilla),
                Resolve,
                THEM.has(Zone::Exile, Prop::Vanilla, 1),
                advance_to(1, TurnStep::End),
                Resolve,
                THEM.controls(Prop::Vanilla, 0),
                THEM.has(Zone::Exile, Prop::Vanilla, 1),
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
        flicker_impersonator(
            "a creature entering as a noncreature copy returns through Containment Priest",
        )
        .actions(
            &const {
                flicker_and_choose(
                    ME.picks_up_to(Prop::Equipment),
                    &[
                        ME.controls(Prop::Equipment, 2),
                        ME.has(Zone::Exile, IMPERSONATOR, 0),
                    ],
                )
            },
        ),
        flicker_impersonator(
            "a creature entering as a creature copy is exiled by Containment Priest",
        )
        .actions(
            &const {
                flicker_and_choose(
                    ME.picks_up_to(Prop::Vanilla),
                    &[
                        ME.controls(Prop::Vanilla, 0),
                        ME.has(Zone::Exile, IMPERSONATOR, 1),
                    ],
                )
            },
        ),
        flicker_impersonator("a creature declining to copy is exiled by Containment Priest")
            .actions(
                &const {
                    flicker_and_choose(
                        ME.done_picking(),
                        &[
                            ME.controls(IMPERSONATOR, 0),
                            ME.has(Zone::Exile, IMPERSONATOR, 1),
                        ],
                    )
                },
            ),
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
        // The Priest isn't on the battlefield as it enters with a creature in the same event,
        // so its replacement effect doesn't affect that creature or the Priest itself
        // (CR 614.12: a replacement effect from a permanent applies only if it is on the
        // battlefield; here it is not yet).
        Scenario::new("Containment Priest entering with another creature exiles neither")
            .interaction(&[Card::ShowAndTell])
            .hand(P0, &[Card::ShowAndTell, PRIEST])
            .battlefield(P0, &[Card::Island; 3])
            .hand(P1, &[Prop::Vanilla])
            .actions(&[
                ME.casts(Card::ShowAndTell),
                ME.picks_up_to(PRIEST),
                THEM.picks_up_to(Prop::Vanilla),
                ME.controls(PRIEST, 1),
                THEM.controls(Prop::Vanilla, 1),
                ME.has_cards(Zone::Exile, 0),
                THEM.has_cards(Zone::Exile, 0),
            ]),
        // A creature cast using escape enters even though the spell came from the graveyard.
        Scenario::new("a creature cast from the graveyard enters through Containment Priest")
            .interaction(&[Card::UnderworldBreach])
            .battlefield(P0, &[Card::UnderworldBreach, Card::Island, Card::Island])
            .battlefield(P1, &[Card::ContainmentPriest])
            .graveyard(
                P0,
                &[Prop::Vanilla, Card::Plains, Card::Plains, Card::Plains],
            )
            .actions(&[
                ME.escapes(Prop::Vanilla),
                Resolve,
                ME.controls(Prop::Vanilla, 1),
                ME.has(Zone::Exile, Prop::Vanilla, 0),
                ME.has(Zone::Exile, Card::Plains, 3),
            ]),
        // A creature cast from exile also enters. Spellbinder gives the opponent a way
        // to cast it from there while Priest is on the battlefield.
        Scenario::new("a creature cast from exile enters through Containment Priest")
            .interaction(&[Card::EliteSpellbinder])
            .permanents(
                P0,
                &[
                    Permanent::new(PRIEST).tapped(),
                    Permanent::new(Card::Plains),
                    Permanent::new(Card::Plains),
                    Permanent::new(Card::Plains),
                ],
            )
            .hand(P0, &[Card::EliteSpellbinder])
            .battlefield(P1, &[Card::Island; 4])
            .hand(P1, &[Prop::Vanilla, Card::Island])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.casts(Card::EliteSpellbinder),
                Resolve,
                Resolve,
                ME.yes(),
                THEM.has(Zone::Exile, Prop::Vanilla, 1),
                advance_to(2, TurnStep::PrecombatMain),
                THEM.casts(Prop::Vanilla),
                Resolve,
                THEM.controls(Prop::Vanilla, 1),
                THEM.has(Zone::Exile, Prop::Vanilla, 0),
            ]),
    ];
}
