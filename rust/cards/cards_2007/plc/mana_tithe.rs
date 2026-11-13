// Generated from the card store by `just card header`: notes go below the end marker.
//! Mana Tithe {W}
//! Instant
//!
//! Counter target spell unless its controller pays {1}.
// End of the generated card header.

use crate::{names, printed};
use beryl_card::effect::build::{its_controller, target, counter, unless_pays};
use beryl_card::{Ability, CardDef, Filter, ManaCost, TargetKind, TargetSpec, card};

/// [CR 118.12a](https://yawgatog.com/resources/magic-rules/#R11812a): "[Do something] unless
/// [a player does something else]" means "[A player may do something else]. If [that player
/// doesn't], [do something]." A controller who can't pay is never asked.
pub static CARD: CardDef = card(
    names::MANA_TITHE,
    &[printed::MANA_TITHE.abilities(&[Ability::spell()
        .targeting(&[TargetSpec::one(TargetKind::Spell, Filter::Any)])
        .effect(&unless_pays(
            &counter(target()),
            its_controller(),
            ManaCost::generic(1),
        ))])],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_card::{DecisionKind, TurnStep, Zone};

    use beryl_scenario_types::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use crate::{names, props};

    /// Four lands for the planeswalker, two to pay with.
    const SIX_LANDS: &[Permanent] = &[
        Permanent::new(names::ISLAND),
        Permanent::new(props::NONBASIC_LAND),
        Permanent::new(names::PLAINS),
        Permanent::new(names::PLAINS),
        Permanent::new(names::ISLAND),
        Permanent::new(names::PLAINS),
    ];

    /// P1 has a card to draw, so the game waits in their turn 2 once the stack is empty. The
    /// artifact is the spell to counter: it costs {1}, so one of its controller's two lands
    /// stays free to pay the tithe.
    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Mana Tithe: the controller pays {1}")
            .permanents(
                P0,
                &[Permanent::new(names::PLAINS), Permanent::new(names::PLAINS)],
            )
            .hand(P0, &[props::ARTIFACT])
            .permanents(P1, &[Permanent::new(names::PLAINS)])
            .hand(P1, &[names::MANA_TITHE])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, props::ARTIFACT),
                Action::cast(P1, names::MANA_TITHE),
                Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Battlefield,
                    card: Some(props::ARTIFACT),
                    count: 1,
                }),
                Action::assert(Assertion::Tapped {
                    player: P0,
                    card: names::PLAINS,
                    count: 2,
                }),
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Graveyard,
                    card: Some(names::MANA_TITHE),
                    count: 1,
                }),
            ]),
        // The planeswalker ({2}{U}{U}) taps four of P0's six lands, and the two left pay the
        // tithe. (A game once showed it countered unasked from six lands: Elite Spellbinder's
        // {2} tax had tapped them all.)
        Scenario::new("Mana Tithe: lands left after paying for the spell pay {1}")
            .permanents(P0, SIX_LANDS)
            .hand(P0, &[props::PLANESWALKER])
            .permanents(P1, &[Permanent::new(names::PLAINS)])
            .hand(P1, &[names::MANA_TITHE])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, props::PLANESWALKER),
                Action::cast(P1, names::MANA_TITHE),
                Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::End,
                },
                Action::assert(Assertion::Controls {
                    player: P0,
                    card: props::PLANESWALKER,
                    count: 1,
                }),
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Graveyard,
                    card: Some(names::MANA_TITHE),
                    count: 1,
                }),
            ]),
        Scenario::new("Mana Tithe: a controller who can't pay isn't asked")
            .permanents(P0, &[Permanent::new(names::PLAINS)])
            .hand(P0, &[props::ARTIFACT])
            .permanents(P1, &[Permanent::new(names::PLAINS)])
            .hand(P1, &[names::MANA_TITHE])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, props::ARTIFACT),
                Action::cast(P1, names::MANA_TITHE),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Graveyard,
                    card: Some(props::ARTIFACT),
                    count: 1,
                }),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Battlefield,
                    card: Some(props::ARTIFACT),
                    count: 0,
                }),
            ]),
    ];
}
