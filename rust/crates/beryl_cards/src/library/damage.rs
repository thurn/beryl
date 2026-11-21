//! Damage ([CR 120](https://yawgatog.com/resources/magic-rules/#R120)).

use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

use beryl_scenario_types::{ME, Resolve, THEM};

use crate::{names, props};

/// Damage an effect deals to a player is loss of life for that player alone
/// ([CR 120.3a](https://yawgatog.com/resources/magic-rules/#R1203a)): "Damage dealt to a
/// player by a source without infect causes that player to lose that much life." The seed is
/// one whose first flip is lost, so the artifact deals 3 to its controller.
pub const SCENARIOS: &[Scenario] =
    &[
        Scenario::new("Damage an effect deals to a player is that player's life lost")
            .seed(LOST_SEED)
            .at(1, P0, TurnStep::PostcombatMain)
            .permanents(P1, &[Permanent::new(names::MANA_CRYPT)])
            .library(P0, &[names::ISLAND])
            .library(P1, &[names::ISLAND, names::ISLAND])
            .actions(&[
                Action::AdvanceTo {
                    turn: 2,
                    step: TurnStep::Upkeep,
                },
                Action::pass(P1),
                Action::assert(Assertion::Life {
                    player: P1,
                    life: 17,
                }),
                Action::assert(Assertion::Life {
                    player: P0,
                    life: 20,
                }),
            ]),
    ];

const LOST_SEED: u64 = 2;

const BOLT: &str = names::LIGHTNING_BOLT;
const SAMBAR: &str = props::VANILLA.card;
const FLIER: &str = props::FLIER.card;

const fn bolt_with_targets(name: &'static str) -> Scenario {
    Scenario::new(name)
        .hand(P0, &[BOLT])
        .battlefield(P0, &[names::MOUNTAIN, SAMBAR])
        .battlefield(P1, &[names::PLAINS, FLIER])
}

pub const ANY_TARGET: &[Scenario] = &[
    bolt_with_targets("Any target includes both players and both creatures").actions(&[
        ME.casts(BOLT),
        ME.deciding(DecisionKind::Target, 4),
        ME.targets_player(THEM),
        Resolve,
        THEM.life(17),
        ME.life(20),
        ME.has(Zone::Graveyard, BOLT, 1),
        ME.controls(SAMBAR, 1),
        THEM.controls(FLIER, 1),
    ]),
    bolt_with_targets("Lethal damage destroys only the targeted creature").actions(&[
        ME.casts(BOLT),
        ME.targets(FLIER),
        Resolve,
        THEM.has(Zone::Graveyard, FLIER, 1),
        ME.controls(SAMBAR, 1),
        ME.life(20),
        THEM.life(20),
    ]),
    bolt_with_targets("Any target includes the caster").actions(&[
        ME.casts(BOLT),
        ME.targets_player(ME),
        Resolve,
        ME.life(17),
    ]),
    bolt_with_targets("Any target includes a creature the caster controls").actions(&[
        ME.casts(BOLT),
        ME.targets(SAMBAR),
        Resolve,
        ME.has(Zone::Graveyard, SAMBAR, 1),
    ]),
    Scenario::new("A damage spell does not resolve after its target is returned")
        .hand(P0, &[BOLT, props::INSTANT.card])
        .battlefield(P0, &[names::MOUNTAIN, names::ISLAND, names::ISLAND, FLIER])
        .hand(P1, &[props::REMOVAL.card])
        .battlefield(P1, &[names::ISLAND; 3])
        .actions(&[
            ME.casts(BOLT),
            ME.targets(FLIER),
            ME.passes(),
            THEM.casts(props::REMOVAL.card),
            THEM.targets(FLIER),
            Resolve,
            Resolve,
            ME.has(Zone::Graveyard, BOLT, 1),
            ME.has(Zone::Hand, FLIER, 1),
            ME.life(20),
            THEM.life(20),
        ]),
];
