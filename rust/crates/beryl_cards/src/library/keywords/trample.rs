//! Trample ([CR 702.19](https://yawgatog.com/resources/magic-rules/#R70219)), on a card complete
//! from the card store.

use beryl_engine::card::{Colors, Keywords, Subtype, Subtypes, Types};
use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};
use beryl_scenario_types::{PermanentState, SnapshotRef};

use crate::{names, props};

/// A 6/6 with trample, complete from the store.
pub const TRAMPLER: &str = names::COLOSSAL_DREADMAW;

pub(in crate::library) const fn two_damage(player: beryl_engine::Seat) -> Action {
    Action::assert(Assertion::PermanentState {
        object: SnapshotRef {
            zone: Zone::Battlefield,
            player,
            name: Some(TRAMPLER),
            index: 0,
        },
        state: PermanentState {
            colors: Colors::GREEN,
            types: Types::CREATURE,
            subtypes: Subtypes::of(&[Subtype::Dinosaur]),
            all_creature_types: false,
            keywords: Keywords::TRAMPLE,
            damage: 2,
            attached_to: None,
            pt: Some((6, 6)),
        },
    })
}

/// Blocked by a 2/1, the trampler assigns lethal damage to it and the rest to the player
/// ([CR 702.19b](https://yawgatog.com/resources/magic-rules/#R70219b)): "Once all those
/// blocking creatures are assigned lethal damage, any excess damage is assigned as its
/// controller chooses among those blocking creatures and the player, planeswalker, or battle
/// the creature is attacking." So 1 goes to the blocker and 5 to the player.
pub const SCENARIOS: &[Scenario] =
    &[
        Scenario::new("A trampler assigns lethal damage to its blocker and the rest to the player")
            .permanents(P0, &[Permanent::new(TRAMPLER)])
            .permanents(P1, &[Permanent::new(props::VANILLA.card)])
            .library(P0, &[names::ISLAND])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::BeginningOfCombat,
                },
                Action::pass(P0),
                Action::choose(P0, DecisionKind::Attacker, TRAMPLER),
                Action::choose(P1, DecisionKind::Blocker, props::VANILLA.card),
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::EndOfCombat,
                },
                Action::assert(Assertion::Life {
                    player: P1,
                    life: 15,
                }),
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Graveyard,
                    card: Some(props::VANILLA.card),
                    count: 1,
                }),
                two_damage(P0),
            ]),
    ];
