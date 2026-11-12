//! Oust. {W} Sorcery. "Put target creature into its owner's library second from the top. Its
//! controller gains 3 life."

use crate::names;
use beryl_card::effect::build::{ITS_CONTROLLER, TARGET, gain_life, put_into_library, sequence};
use beryl_card::{
    Ability, CardDef, Colors, Face, Filter, LibraryPosition, TargetKind, TargetSpec, Types, card,
    mana,
};

/// "Second from the top" is under one card, or the top of an empty library. "Its controller"
/// is the creature's as it was on the battlefield: in a library it has no controller
/// ([CR 108.4a](https://yawgatog.com/resources/magic-rules/#R1084a)): "If anything asks for
/// the controller of a card that doesn't have one (because it's not a permanent or spell),
/// use its owner instead." Owner and controller differ only for a stolen creature, which
/// goes to its owner's library while its controller gains the life.
pub static CARD: CardDef = card(
    names::OUST,
    &[Face::EMPTY
        .cost(mana("{W}"))
        .types(Types::SORCERY)
        .colors(Colors::WHITE)
        .abilities(&[Ability::spell()
            .targeting(&[TargetSpec::one(
                TargetKind::Permanent,
                Filter::Types(Types::CREATURE),
            )])
            .effect(&sequence(&[
                put_into_library(TARGET, LibraryPosition::FromTop(2)),
                gain_life(ITS_CONTROLLER, 3),
            ]))])],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_card::{DecisionKind, Seat, TurnStep, Zone};
    use beryl_scenario_types::{Action, Assertion, P0, P1, Permanent, Scenario};

    use crate::{names, props};

    const SPELLBINDER: &str = names::ELITE_SPELLBINDER;

    /// P0 casts Oust on P1's Elite Spellbinder, whose library is `top`. The Plains left in P0's
    /// hand keeps the game in P0's main phase once Oust has resolved.
    const fn oust_onto(name: &'static str, top: &'static [&'static str]) -> Scenario {
        const PLAINS: &[Permanent] = &[Permanent::new(names::PLAINS)];
        const SPELLBINDERS: &[Permanent] = &[Permanent::new(SPELLBINDER)];
        Scenario::new(name)
            .battlefield(P0, PLAINS)
            .hand(P0, &[names::OUST, names::PLAINS])
            .battlefield(P1, SPELLBINDERS)
            .library(P1, top)
    }

    const fn library(top: &'static [&'static str]) -> Action {
        Action::assert(Assertion::Library { player: P1, top })
    }

    const fn life(player: Seat, life: i16) -> Action {
        Action::assert(Assertion::Life { player, life })
    }

    pub const SCENARIOS: &[Scenario] = &[
        // With no legal target it can't be cast
        // ([CR 601.2c](https://yawgatog.com/resources/magic-rules/#R6012c)): P1's only
        // permanent is a noncreature artifact, so passing is P0's only option.
        Scenario::new("Oust can't be cast without a target")
            .battlefield(
                P0,
                &[
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                ],
            )
            .hand(P0, &[names::OUST])
            .battlefield(P1, &[Permanent::new(props::ARTIFACT)])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::PrecombatMain,
                },
                Action::assert(Assertion::Decision {
                    player: P0,
                    kind: DecisionKind::Priority,
                    candidates: 1,
                }),
            ]),
        // Second from the top of an empty library is its top: the creature is its only card.
        oust_onto(
            "Oust on an empty library makes the creature its only card",
            &[],
        )
        .actions(&[
            Action::cast(P0, names::OUST),
            library(&[SPELLBINDER]),
            life(P1, 23),
            life(P0, 20),
        ]),
        oust_onto(
            "Oust on a library of one puts the creature under it",
            &[names::ISLAND],
        )
        .actions(&[
            Action::cast(P0, names::OUST),
            library(&[names::ISLAND, SPELLBINDER]),
        ]),
        oust_onto(
            "Oust puts the creature second from the top and its controller gains 3",
            &[names::ISLAND, names::PLAINS],
        )
        .actions(&[
            Action::cast(P0, names::OUST),
            library(&[names::ISLAND, SPELLBINDER, names::PLAINS]),
            Action::assert(Assertion::Count {
                player: P1,
                zone: Zone::Battlefield,
                card: Some(SPELLBINDER),
                count: 0,
            }),
            Action::assert(Assertion::Count {
                player: P0,
                zone: Zone::Graveyard,
                card: Some(names::OUST),
                count: 1,
            }),
            life(P1, 23),
        ]),
        // UI regression by-97z: the lower spell's target stays legible beneath a response. P0's
        // own Counterspell keeps each of P0's priorities a decision.
        Scenario::new("Oust beneath an opposing counterspell")
            .battlefield(
                P0,
                &[
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                ],
            )
            .hand(P0, &[names::OUST, props::INSTANT, names::PLAINS])
            .battlefield(
                P1,
                &[
                    Permanent::new(SPELLBINDER),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                ],
            )
            .hand(P1, &[props::INSTANT])
            .library(P0, &[names::ISLAND])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, names::OUST),
                Action::pass(P0),
                Action::cast(P1, props::INSTANT),
                Action::pass(P0),
                Action::assert(Assertion::Controls {
                    player: P1,
                    card: SPELLBINDER,
                    count: 1,
                }),
            ]),
    ];
}
