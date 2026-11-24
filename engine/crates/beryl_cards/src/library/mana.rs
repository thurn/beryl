//! Mana ([CR 106](https://yawgatog.com/resources/magic-rules/#R106)).

use beryl_engine::card::ManaColor;
use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};
use beryl_scenario_types::{ME, Resolve, THEM, advance_to, stack};

use crate::{names, props};

/// Mana a triggered ability adds is in its controller's pool like any other
/// ([CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064)): "When an effect instructs
/// a player to add mana, that mana goes into a player's mana pool." Unspent, it is gone once
/// the phase has ended (CR 500.5). The amount is the one settled as the ability was made:
/// three, the mana value of the sorcery countered.
pub const SCENARIOS: &[Scenario] = &[
    Scenario::new("Mana a triggered ability adds is gone once its phase has ended")
        .at(2, P1, TurnStep::PrecombatMain)
        .permanents(P0, &[Permanent::new(names::ISLAND); 2])
        .hand(P0, &[names::MANA_DRAIN])
        .library(P0, &[names::PLAINS, names::PLAINS])
        .permanents(P1, &[Permanent::new(names::ISLAND); 3])
        .hand(P1, &[props::UNTARGETED_SPELL.card])
        .actions(&[
            Action::cast(P1, props::UNTARGETED_SPELL.card),
            Action::cast(P0, names::MANA_DRAIN),
            Action::AdvanceTo {
                turn: 3,
                step: TurnStep::PrecombatMain,
            },
            Action::pass(P0),
            Action::assert(Assertion::Mana {
                player: P0,
                color: ManaColor::Colorless,
                amount: 3,
            }),
            Action::AdvanceTo {
                turn: 3,
                step: TurnStep::BeginningOfCombat,
            },
            Action::assert(Assertion::Mana {
                player: P0,
                color: ManaColor::Colorless,
                amount: 0,
            }),
        ]),
    // The delayed ability leaves the mana pool empty in the opponent's postcombat main phase.
    Scenario::new("Mana Drain: the countered spell's mana value at the caster's next main phase")
        .at(2, P1, TurnStep::PrecombatMain)
        .battlefield(P0, &[names::ISLAND, names::ISLAND])
        .hand(P0, &[names::MANA_DRAIN])
        .library(P0, &[names::PLAINS])
        .battlefield(P1, &[names::ISLAND; 3])
        .hand(P1, &[names::TIMETWISTER])
        .actions(&[
            THEM.casts(names::TIMETWISTER),
            ME.casts(names::MANA_DRAIN),
            THEM.has(Zone::Graveyard, names::TIMETWISTER, 1),
            advance_to(2, TurnStep::PostcombatMain),
            ME.mana(ManaColor::Colorless, 0),
            advance_to(3, TurnStep::PrecombatMain),
            ME.passes(),
            ME.mana(ManaColor::Colorless, 3),
        ]),
];

pub const EQUIVALENT_SOURCES: &[Scenario] =
    &[
        Scenario::new("Equivalent lands pay without a payment choice")
            .hand(P0, &[props::VANILLA.card])
            .battlefield(P0, &[names::ISLAND; 3])
            .actions(&[
                ME.casts(props::VANILLA.card),
                stack(1),
                ME.tapped(names::ISLAND, 2),
                Resolve,
                ME.controls(props::VANILLA.card, 1),
            ]),
    ];

pub const FLOATING_MANA: &[Scenario] =
    &[
        Scenario::new("A payment that floats mana is a distinct choice")
            .hand(P0, &[props::VANILLA.card])
            .battlefield(P0, &[names::ISLAND, names::ISLAND, names::MANA_CRYPT])
            .actions(&[
                ME.casts(props::VANILLA.card),
                ME.deciding(DecisionKind::PaymentPlan, 2),
                // One Island pays {U}, and Mana Crypt pays {1} with {C} left over.
                ME.answers(DecisionKind::PaymentPlan, Pick::Plan { i: 1 }),
                stack(1),
                ME.tapped(names::MANA_CRYPT, 1),
                ME.tapped(names::ISLAND, 1),
                ME.mana(ManaColor::Colorless, 1),
                Resolve,
                ME.controls(props::VANILLA.card, 1),
            ]),
    ];
