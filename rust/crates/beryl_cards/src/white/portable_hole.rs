//! Portable Hole. {W} Artifact. "When this artifact enters, exile target nonland permanent an
//! opponent controls with mana value 2 or less until this artifact leaves the battlefield."

use beryl_card::effect::build::{TARGET, exile_until_this_leaves};
use beryl_engine::card::{
    Ability, Behavior, CardDef, Colors, Face, ManaCost, TargetKind, TargetSpec, Trigger, Types,
};
use beryl_engine::filter::{Filter, Whose};

use crate::names;

/// "Exile target ... until this artifact leaves the battlefield"
/// ([CR 610.3](https://yawgatog.com/resources/magic-rules/#R6103)): "A second one-shot effect
/// is created immediately after the specified event. This second one-shot effect returns the
/// object to its previous zone." A target gone, or no longer matching, by resolution stops
/// the ability ([CR 608.2b](https://yawgatog.com/resources/magic-rules/#R6082b)); a Hole
/// gone first exiles nothing ([CR 610.3b](https://yawgatog.com/resources/magic-rules/#R6103b)).
pub static CARD: CardDef = CardDef {
    name: names::PORTABLE_HOLE,
    faces: &[Face {
        cost: ManaCost::new(0, [1, 0, 0, 0, 0, 0]),
        types: Types::ARTIFACT,
        colors: Colors::WHITE,
        abilities: &[Ability {
            targets: &[TargetSpec::one(
                TargetKind::Permanent,
                Filter::All(&[
                    Filter::Not(&Filter::Types(Types::LAND)),
                    Filter::Whose(Whose::AnOpponent),
                    Filter::ManaValueAtMost(2),
                ]),
            )],
            ..Ability::triggered(
                &Trigger::ENTERS,
                Behavior::Data(&exile_until_this_leaves(TARGET)),
            )
        }],
        ..Face::EMPTY
    }],
    tokens: &[],
};

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_engine::{DecisionKind, Seat, TurnStep, Zone};
    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

    use crate::{names, props};

    const HOLE: &str = names::PORTABLE_HOLE;

    /// Mana value 2: a legal target.
    const TWO_DROP: &str = props::SMALL_CREATURE;

    /// Mana value 3: not a legal target.
    const THREE_DROP: &str = props::CREATURE;

    const fn count(player: Seat, zone: Zone, card: &'static str, count: u8) -> Action {
        Action::assert(Assertion::Count {
            player,
            zone,
            card: Some(card),
            count,
        })
    }

    const fn controls(player: Seat, card: &'static str, count: u8) -> Action {
        Action::assert(Assertion::Controls {
            player,
            card,
            count,
        })
    }

    pub const SCENARIOS: &[Scenario] = &[
        // P1's two-drop is the one legal target: not their three-drop, not P0's own two-drop. On
        // P1's turn Sink into Stupor returns the Hole to P0's hand, and the two-drop comes back
        // under P1's control.
        Scenario::new("Portable Hole exiles until it leaves the battlefield")
            // by-8g5: Playground keeps the Hole in slot zero, exercising linked-exile UI.
            .seed(12)
            .battlefield(
                P0,
                &[
                    Permanent::new(names::PLAINS),
                    Permanent::new(TWO_DROP).sick(),
                ],
            )
            .hand(P0, &[HOLE])
            .library(P0, &[names::ISLAND])
            .battlefield(
                P1,
                &[
                    Permanent::new(TWO_DROP),
                    Permanent::new(THREE_DROP),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                ],
            )
            .hand(P1, &[props::REMOVAL])
            .library(P1, &[names::ISLAND, names::ISLAND])
            .actions(&[
                Action::cast(P0, HOLE),
                Action::Resolve,
                Action::Resolve,
                count(P1, Zone::Exile, TWO_DROP, 1),
                controls(P1, THREE_DROP, 1),
                Action::AdvanceTo {
                    turn: 2,
                    step: TurnStep::PrecombatMain,
                },
                Action::cast(P1, props::REMOVAL),
                Action::choose(P1, DecisionKind::Target, HOLE),
                Action::Resolve,
                count(P0, Zone::Hand, HOLE, 1),
                count(P1, Zone::Exile, TWO_DROP, 0),
                controls(P1, TWO_DROP, 1),
            ]),
        // The three-drop's mana value is 3, and P0's own two-drop isn't an opponent's: with no
        // legal target the trigger never goes on the stack.
        Scenario::new("Portable Hole with no permanent of mana value 2 or less to exile")
            .battlefield(
                P0,
                &[Permanent::new(names::PLAINS), Permanent::new(TWO_DROP)],
            )
            .hand(P0, &[HOLE])
            .library(P0, &[names::ISLAND])
            .battlefield(P1, &[Permanent::new(THREE_DROP)])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, HOLE),
                Action::Resolve,
                Action::assert(Assertion::Stack { count: 0 }),
                controls(P0, HOLE, 1),
                controls(P1, THREE_DROP, 1),
                controls(P0, TWO_DROP, 1),
            ]),
        // With the trigger on the stack, P0 returns its target to P1's hand with Sink into
        // Stupor: the trigger does nothing, and the Hole stays.
        Scenario::new("Portable Hole's target gone before the trigger resolves")
            .battlefield(
                P0,
                &[
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                ],
            )
            .hand(P0, &[HOLE, props::REMOVAL])
            .library(P0, &[names::ISLAND])
            .battlefield(P1, &[Permanent::new(TWO_DROP)])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, HOLE),
                Action::Resolve,
                Action::assert(Assertion::Stack { count: 1 }),
                Action::cast(P0, props::REMOVAL),
                Action::Resolve,
                count(P1, Zone::Hand, TWO_DROP, 1),
                Action::Resolve,
                Action::assert(Assertion::Stack { count: 0 }),
                count(P1, Zone::Exile, TWO_DROP, 0),
                controls(P0, HOLE, 1),
            ]),
        // P1 answers the trigger by returning the Hole to P0's hand with Sink into Stupor: the
        // event the exile lasts until has happened, so the two-drop never leaves (CR 610.3b).
        // P0's two-drop in hand, with flash and the Islands for it, gives P0 a priority to pass
        // between the two resolutions.
        Scenario::new("Portable Hole gone before its trigger resolves")
            .battlefield(
                P0,
                &[
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                ],
            )
            .hand(P0, &[HOLE, TWO_DROP])
            .library(P0, &[names::ISLAND])
            .battlefield(
                P1,
                &[
                    Permanent::new(TWO_DROP),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                ],
            )
            .hand(P1, &[names::SINK_INTO_STUPOR])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, HOLE),
                Action::Resolve,
                Action::assert(Assertion::Stack { count: 1 }),
                Action::pass(P0),
                Action::cast(P1, names::SINK_INTO_STUPOR),
                Action::Resolve,
                Action::Resolve,
                Action::assert(Assertion::Stack { count: 0 }),
                count(P0, Zone::Hand, HOLE, 1),
                count(P1, Zone::Exile, TWO_DROP, 0),
                controls(P1, TWO_DROP, 1),
            ]),
    ];
}
