//! The runner on test cards: what it accepts, and what it refuses.

use beryl_engine::card::{
    Ability, CardDef, Face, ManaColor, ManaCost, ManaSet, Subtype, Subtypes, Types,
};
use beryl_engine::{CardTable, DecisionKind, TurnStep, Zone};

use crate::{Action, Assertion, Ending, P0, P1, Permanent, Pick, Scenario, run, to_json};

static LAND: CardDef = CardDef {
    name: "Test Land",
    faces: &[Face {
        types: Types::LAND,
        subtypes: Subtypes::of(&[Subtype::Plains]),
        abilities: &[Ability::tap_for_mana(ManaSet::one(ManaColor::White))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static OTHER: CardDef = CardDef {
    name: "Other Land",
    faces: &[Face {
        types: Types::LAND,
        ..Face::EMPTY
    }],
    tokens: &[],
};

static BEAR: CardDef = CardDef {
    name: "Test Bear",
    faces: &[Face {
        cost: ManaCost::new(1, [1, 0, 0, 0, 0, 0]),
        types: Types::CREATURE,
        power: Some(2),
        toughness: Some(2),
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CYCLER: CardDef = CardDef {
    name: "Test Cycler",
    faces: &[Face {
        cost: ManaCost::generic(5),
        types: Types::SORCERY,
        abilities: &[Ability::cycling(ManaCost::generic(1))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[&LAND, &OTHER, &BEAR, &CYCLER]);

const L: &str = "Test Land";
/// Nine cards: one too many to keep after playing a land, and two kinds to discard.
const NINE: &[&str] = &[L, L, L, L, L, L, L, L, "Other Land"];

const fn count(player: u8, zone: Zone, n: u8) -> Action {
    Action::Assert {
        assertion: Assertion::Count {
            player,
            zone,
            card: None,
            count: n,
        },
    }
}

fn error(scenario: &Scenario) -> String {
    run(&CARDS, scenario).unwrap_err().to_string()
}

#[test]
fn a_scenario_sets_up_its_position() {
    const S: Scenario = Scenario::new("setup")
        .at(4, P1, TurnStep::PostcombatMain)
        .life(P0, 7)
        .hand(P1, &[L, "Other Land"])
        .battlefield(P0, &[Permanent::new(L).tapped(), Permanent::new(L)])
        .graveyard(P1, &[L])
        .exile(P0, &["Other Land"])
        .library(P0, &[L, L])
        .actions(&[
            Action::Assert {
                assertion: Assertion::Decision {
                    player: P1,
                    kind: DecisionKind::Priority,
                    candidates: 3,
                },
            },
            Action::Assert {
                assertion: Assertion::Life {
                    player: P0,
                    life: 7,
                },
            },
            Action::Assert {
                assertion: Assertion::Tapped {
                    player: P0,
                    card: L,
                    count: 1,
                },
            },
            count(P0, Zone::Battlefield, 2),
            count(P0, Zone::Exile, 1),
            count(P0, Zone::Library, 2),
            count(P1, Zone::Hand, 2),
            count(P1, Zone::Graveyard, 1),
        ]);
    run(&CARDS, &S).unwrap();
}

#[test]
fn a_scenario_casts_resolves_activates_and_attacks() {
    const BEAR: &str = "Test Bear";
    const S: Scenario = Scenario::new("cast and attack")
        .hand(P0, &[BEAR, "Test Cycler"])
        .battlefield(
            P0,
            &[Permanent::new(L), Permanent::new(L), Permanent::new(L)],
        )
        .library(P0, &[L, L])
        .library(P1, &[L])
        .actions(&[
            Action::Cast {
                player: P0,
                card: BEAR,
                face: 0,
                costs: 0,
            },
            Action::Assert {
                assertion: Assertion::Stack { count: 1 },
            },
            Action::Resolve,
            Action::Assert {
                assertion: Assertion::Stack { count: 0 },
            },
            Action::Assert {
                assertion: Assertion::Pt {
                    player: P0,
                    card: BEAR,
                    power: 2,
                    toughness: 2,
                },
            },
            Action::Assert {
                assertion: Assertion::Creature {
                    player: P0,
                    card: BEAR,
                    creature: true,
                },
            },
            Action::Activate {
                player: P0,
                card: "Test Cycler",
                ability: 0,
            },
            Action::Resolve,
            count(P0, Zone::Hand, 1),
            count(P0, Zone::Graveyard, 1),
            Action::AdvanceTo {
                turn: 3,
                step: TurnStep::BeginningOfCombat,
            },
            Action::Answer {
                player: P0,
                kind: DecisionKind::Priority,
                pick: Pick::Pass,
            },
            Action::Answer {
                player: P0,
                kind: DecisionKind::Attacker,
                pick: Pick::Card { card: BEAR },
            },
            Action::Assert {
                assertion: Assertion::Attacking {
                    player: P0,
                    card: BEAR,
                    count: 1,
                },
            },
            Action::AdvanceTo {
                turn: 3,
                step: TurnStep::EndOfCombat,
            },
            Action::Assert {
                assertion: Assertion::Life {
                    player: P1,
                    life: 18,
                },
            },
            Action::Assert {
                assertion: Assertion::Controls {
                    player: P1,
                    card: BEAR,
                    count: 0,
                },
            },
        ]);
    run(&CARDS, &S).unwrap();
}

#[test]
fn a_scripted_game_plays_lands_discards_and_reaches_the_next_turn() {
    // Eight cards in hand after the land: the cleanup step asks for a discard, scripted here.
    const S: Scenario = Scenario::new("discard")
        .hand(P0, NINE)
        .library(P1, &[L])
        .actions(&[
            Action::PlayLand {
                player: P0,
                card: L,
            },
            // Only passing is left: the advance stops there all the same.
            Action::AdvanceTo {
                turn: 1,
                step: TurnStep::End,
            },
            Action::Assert {
                assertion: Assertion::Decision {
                    player: P0,
                    kind: DecisionKind::Priority,
                    candidates: 1,
                },
            },
            Action::Answer {
                player: P0,
                kind: DecisionKind::Priority,
                pick: Pick::Pass,
            },
            Action::Answer {
                player: P0,
                kind: DecisionKind::Discard,
                pick: Pick::Card { card: L },
            },
            Action::AdvanceTo {
                turn: 2,
                step: TurnStep::PrecombatMain,
            },
            count(P0, Zone::Hand, 7),
            count(P0, Zone::Graveyard, 1),
            count(P1, Zone::Hand, 1),
            Action::Assert {
                assertion: Assertion::Outcome {
                    outcome: Ending::InProgress,
                },
            },
        ]);
    run(&CARDS, &S).unwrap();
}

#[test]
fn an_unscripted_decision_fails_the_scenario() {
    // The advance would pass through the cleanup step's discard.
    const S: Scenario = Scenario::new("unscripted")
        .hand(P0, NINE)
        .library(P1, &[L])
        .actions(&[Action::AdvanceTo {
            turn: 2,
            step: TurnStep::Upkeep,
        }]);
    let e = error(&S);
    assert!(
        e.contains("action 0: unscripted decision: player 0 Discard"),
        "{e}"
    );
}

#[test]
fn an_answer_for_another_decision_fails_the_scenario() {
    const S: Scenario = Scenario::new("wrong answer")
        .hand(P0, &[L])
        .actions(&[Action::PlayLand {
            player: P1,
            card: L,
        }]);
    let e = error(&S);
    assert!(e.contains("unscripted decision: player 0 Priority"), "{e}");
}

#[test]
fn a_false_assertion_fails_the_scenario() {
    const S: Scenario = Scenario::new("false").hand(P0, &[L]).actions(&[
        Action::PlayLand {
            player: P0,
            card: L,
        },
        Action::AdvanceTo {
            turn: 1,
            step: TurnStep::PostcombatMain,
        },
        count(P0, Zone::Battlefield, 2),
    ]);
    let e = error(&S);
    assert!(
        e.contains("action 2") && e.contains("expected 2, found 1"),
        "{e}"
    );
}

#[test]
fn a_card_that_is_not_offered_fails_the_scenario() {
    const S: Scenario = Scenario::new("not offered")
        .hand(P0, &[L])
        .actions(&[Action::PlayLand {
            player: P0,
            card: "Other Land",
        }]);
    let e = error(&S);
    assert!(e.contains("playing Other Land is not offered"), "{e}");
}

#[test]
fn advancing_to_a_step_without_priority_fails_the_scenario() {
    const S: Scenario = Scenario::new("untap")
        .library(P1, &[L])
        .actions(&[Action::AdvanceTo {
            turn: 2,
            step: TurnStep::Untap,
        }]);
    let e = error(&S);
    assert!(e.contains("passed turn 2 Untap without stopping"), "{e}");
}

#[test]
fn a_game_that_ends_first_fails_an_advance() {
    // The player on the draw has no library: they lose in their draw step.
    const S: Scenario = Scenario::new("decked").actions(&[
        Action::AdvanceTo {
            turn: 3,
            step: TurnStep::PrecombatMain,
        },
        Action::Assert {
            assertion: Assertion::Outcome {
                outcome: Ending::Win { player: P0 },
            },
        },
    ]);
    const T: Scenario = Scenario::new("decked").actions(&[Action::Assert {
        assertion: Assertion::Outcome {
            outcome: Ending::Win { player: P0 },
        },
    }]);
    let e = error(&S);
    assert!(e.contains("action 0: the game is over (Win(0))"), "{e}");
    run(&CARDS, &T).unwrap();
}

#[test]
fn scenarios_serialize_with_engine_names() {
    const S: Scenario = Scenario::new("json")
        .hand(P0, &[L])
        .actions(&[Action::AdvanceTo {
            turn: 1,
            step: TurnStep::End,
        }]);
    let json: serde_json::Value = serde_json::from_str(&to_json(&[(L, &[S])])).unwrap();
    assert_eq!(json[0]["card"], L);
    assert_eq!(json[0]["name"], "json");
    assert_eq!(json[0]["setup"]["step"], "PrecombatMain");
    assert_eq!(json[0]["setup"]["players"][0]["hand"][0], L);
    assert_eq!(json[0]["actions"][0]["action"], "AdvanceTo");
    assert_eq!(json[0]["actions"][0]["step"], "End");
}
