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

/// Priced mana abilities (CR 605.3a): activating a mana ability whose cost includes mana
/// requires paying that mana from another source or the pool first, and sequences of priced
/// sources must spend the right colors to leave the target cost payable.
pub const PRICED_MANA: &[Scenario] = &[
    Scenario::new("A priced mana source alone cannot pay for a one-mana spell")
        .battlefield(P0, &[names::AZORIUS_SIGNET])
        .hand(P0, &[props::EQUIPMENT.card, names::ISLAND])
        .actions(&[
            // Only Pass and playing the Island are legal: the Signet cannot pay its own {1}.
            ME.deciding(DecisionKind::Priority, 2),
            ME.has(Zone::Hand, props::EQUIPMENT.card, 1),
            ME.controls(props::EQUIPMENT.card, 0),
        ]),
    Scenario::new("A Signet and an Island pay {W}{U}")
        .battlefield(P0, &[names::AZORIUS_SIGNET, names::ISLAND])
        .hand(P0, &[props::ARTIFACT.card, props::EQUIPMENT.card])
        .actions(&[
            // {W} from the Signet casts Portable Hole and leaves {U} floating, which pays {1}.
            ME.casts(props::ARTIFACT.card),
            ME.mana(ManaColor::Blue, 1),
            Resolve,
            ME.controls(props::ARTIFACT.card, 1),
            ME.casts(props::EQUIPMENT.card),
            Resolve,
            ME.controls(props::EQUIPMENT.card, 1),
            ME.tapped(names::AZORIUS_SIGNET, 1),
            ME.tapped(names::ISLAND, 1),
            ME.mana(ManaColor::Blue, 0),
        ]),
    Scenario::new("Two Signets and one land pay {1}{W}{W} only by spending {U} into the second")
        .battlefield(
            P0,
            &[names::AZORIUS_SIGNET, names::AZORIUS_SIGNET, names::ISLAND],
        )
        .hand(P0, &[props::FLIER.card])
        .actions(&[
            ME.casts(props::FLIER.card),
            Resolve,
            ME.controls(props::FLIER.card, 1),
            ME.tapped(names::AZORIUS_SIGNET, 2),
            ME.tapped(names::ISLAND, 1),
        ]),
    Scenario::new("Mana left floating by a priced payment stays in the pool")
        .battlefield(P0, &[names::AZORIUS_SIGNET, names::ISLAND])
        .hand(P0, &[props::ARTIFACT.card])
        .actions(&[
            ME.casts(props::ARTIFACT.card),
            ME.mana(ManaColor::Blue, 1),
            ME.tapped(names::AZORIUS_SIGNET, 1),
            ME.tapped(names::ISLAND, 1),
            Resolve,
            ME.controls(props::ARTIFACT.card, 1),
        ]),
];
