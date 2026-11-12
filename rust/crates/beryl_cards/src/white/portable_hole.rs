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
    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use crate::names;

    const HOLE: &str = names::PORTABLE_HOLE;

    const JACE: &str = names::JACE_VRYNS_PRODIGY;

    const TEFERI: &str = names::TEFERI_TIME_RAVELER;

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

    const PLAINS: &[Permanent] = &[Permanent::new(names::PLAINS); 3];

    pub const SCENARIOS: &[Scenario] = &[
        // Jace (mana value 2) is the one legal target: not Teferi (3), not P0's own Mana Crypt.
        // On P1's turn Teferi's -3 returns the Hole to P0's hand, and Jace comes back under P1's
        // control.
        Scenario::new("Portable Hole exiles until it leaves the battlefield")
            // by-8g5: Playground keeps the Hole in slot zero, exercising linked-exile UI.
            .seed(12)
            .battlefield(
                P0,
                &[
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::MANA_CRYPT),
                ],
            )
            .hand(P0, &[HOLE])
            .library(P0, &[names::ISLAND])
            .battlefield(P1, &[Permanent::new(JACE), Permanent::new(TEFERI)])
            .library(P1, &[names::ISLAND, names::ISLAND])
            .actions(&[
                Action::cast(P0, HOLE),
                Action::Resolve,
                Action::Resolve,
                count(P1, Zone::Exile, JACE, 1),
                controls(P1, TEFERI, 1),
                Action::AdvanceTo {
                    turn: 2,
                    step: TurnStep::PrecombatMain,
                },
                Action::activate(P1, TEFERI, 2),
                Action::answer(P1, DecisionKind::TargetCount, Pick::Number { n: 1 }),
                Action::choose(P1, DecisionKind::Target, HOLE),
                Action::Resolve,
                count(P0, Zone::Hand, HOLE, 1),
                count(P1, Zone::Exile, JACE, 0),
                controls(P1, JACE, 1),
            ]),
        // Teferi's mana value is 3: with no legal target the trigger never goes on the stack.
        Scenario::new("Portable Hole with no permanent of mana value 2 or less to exile")
            .battlefield(
                P0,
                &[
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::MANA_CRYPT),
                ],
            )
            .hand(P0, &[HOLE])
            .library(P0, &[names::ISLAND])
            .battlefield(P1, &[Permanent::new(TEFERI)])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, HOLE),
                Action::Resolve,
                Action::assert(Assertion::Stack { count: 0 }),
                controls(P0, HOLE, 1),
                controls(P1, TEFERI, 1),
                controls(P0, names::MANA_CRYPT, 1),
            ]),
        // With the trigger on the stack, P0 destroys its target with Get Lost: the trigger does
        // nothing, and the Hole stays.
        Scenario::new("Portable Hole's target gone before the trigger resolves")
            .battlefield(P0, PLAINS)
            .hand(P0, &[HOLE, names::GET_LOST])
            .library(P0, &[names::ISLAND])
            .battlefield(P1, &[Permanent::new(JACE)])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, HOLE),
                Action::Resolve,
                Action::assert(Assertion::Stack { count: 1 }),
                Action::cast(P0, names::GET_LOST),
                Action::Resolve,
                count(P1, Zone::Graveyard, JACE, 1),
                Action::Resolve,
                Action::assert(Assertion::Stack { count: 0 }),
                count(P1, Zone::Exile, JACE, 0),
                controls(P0, HOLE, 1),
            ]),
        // P1 answers the trigger by returning the Hole to P0's hand with Sink into Stupor: the
        // event the exile lasts until has happened, so Jace never leaves (CR 610.3b).
        Scenario::new("Portable Hole gone before its trigger resolves")
            .battlefield(P0, &[Permanent::new(names::PLAINS)])
            .hand(P0, &[HOLE])
            .library(P0, &[names::ISLAND])
            .battlefield(
                P1,
                &[
                    Permanent::new(JACE),
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
                count(P1, Zone::Exile, JACE, 0),
                controls(P1, JACE, 1),
            ]),
    ];
}
