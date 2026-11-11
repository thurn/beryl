//! Where the smart stops stop the human, as a strong player would be stopped: not at the
//! opponent's end step with nothing to do there, not at their own trigger, and not in each
//! combat step of their own turn; full control stops at each.

use beryl_cards::CARDS;
use beryl_cards::names::{
    COUNTERSPELL, ISLAND, MANA_CRYPT, PLAINS, SNAPCASTER_MAGE, THE_WANDERING_EMPEROR,
    THUNDERTRAP_TRAINER,
};
use beryl_engine::TurnStep;
use beryl_scenario::{P0, P1, Permanent, Scenario};
use beryl_search::Budget;
use beryl_session::protocol::{Candidate, Kind, PassTo, Side, Step, StepSet, ZoneName};
use beryl_session::{CandidateIndex, Game, Prepared, Seat, SessionId, Status, StopsSetting};

#[path = "common/play.rs"]
mod play;

use play::{Casts, Play, to_human};

const SESSION: SessionId = SessionId(1);
const HUMAN: Seat = Seat::P0;

const LANDS: &[&str] = &[ISLAND, ISLAND, ISLAND, ISLAND, ISLAND, ISLAND];

/// Four untapped lands that cast The Wandering Emperor.
const FOUR_LANDS: &[Permanent] = &[
    Permanent::new(PLAINS),
    Permanent::new(PLAINS),
    Permanent::new(ISLAND),
    Permanent::new(ISLAND),
];

/// The opponent's turn, with the human holding only a creature without flash.
const NOTHING_AT_INSTANT_SPEED: Scenario = Scenario::new("nothing-at-instant-speed")
    .at(2, P1, TurnStep::PrecombatMain)
    .battlefield(P0, FOUR_LANDS)
    .hand(P0, &[THUNDERTRAP_TRAINER])
    .library(P0, LANDS)
    .battlefield(P1, &[Permanent::new(ISLAND)])
    .library(P1, LANDS);

/// The human's turn, with a creature to attack an empty board and The Wandering Emperor to
/// flash in.
const ATTACK_HOLDING_FLASH: Scenario = Scenario::new("attack-holding-flash")
    .at(3, P0, TurnStep::PrecombatMain)
    .battlefield(
        P0,
        &[
            Permanent::new(SNAPCASTER_MAGE),
            Permanent::new(PLAINS),
            Permanent::new(PLAINS),
            Permanent::new(ISLAND),
            Permanent::new(ISLAND),
        ],
    )
    .hand(P0, &[THE_WANDERING_EMPEROR])
    .library(P0, LANDS)
    .battlefield(P1, &[Permanent::new(ISLAND)])
    .library(P1, LANDS);

/// The opponent's turn before the human's, whose Mana Crypt triggers in their upkeep while
/// they could flash in The Wandering Emperor.
const CRYPT_HOLDING_FLASH: Scenario = Scenario::new("crypt-holding-flash")
    .at(2, P1, TurnStep::PrecombatMain)
    .battlefield(
        P0,
        &[
            Permanent::new(MANA_CRYPT),
            Permanent::new(PLAINS),
            Permanent::new(PLAINS),
            Permanent::new(ISLAND),
            Permanent::new(ISLAND),
        ],
    )
    .hand(P0, &[THE_WANDERING_EMPEROR])
    .library(P0, LANDS)
    .battlefield(P1, &[Permanent::new(ISLAND)])
    .library(P1, LANDS);

/// The opponent's turn, with a creature to attack the human, who could flash in The
/// Wandering Emperor to block it or, once it attacks, exile it.
const DEFEND_HOLDING_FLASH: Scenario = Scenario::new("defend-holding-flash")
    .at(2, P1, TurnStep::PrecombatMain)
    .battlefield(P0, FOUR_LANDS)
    .hand(P0, &[THE_WANDERING_EMPEROR])
    .library(P0, LANDS)
    .battlefield(
        P1,
        &[Permanent::new(SNAPCASTER_MAGE), Permanent::new(ISLAND)],
    )
    .library(P1, LANDS);

/// Where the human was asked something: the turn's active player, its step, whether the stack
/// held anything, and what was asked.
#[derive(Debug, PartialEq)]
struct Stop {
    active: Side,
    step: Step,
    stack: bool,
    kind: Kind,
    /// Where passing priority was to take the human, as the pass button says.
    pass_to: Option<PassTo>,
}

/// Plays `scenario` under `stops` until the human's turn after the next begins: every
/// decision and halt of the human's. The human attacks with every creature, casts nothing and
/// otherwise passes, or takes the first candidate; the opponent is played by `agent`.
fn stops_in(scenario: &Scenario, stops: StopsSetting) -> Vec<Stop> {
    stops_against(scenario, stops, "random")
}

fn stops_against(scenario: &Scenario, stops: StopsSetting, agent: &str) -> Vec<Stop> {
    let (table, state) = beryl_scenario::position(&CARDS, scenario).unwrap();
    let last = state.turn().number() + 2;
    let random = beryl_search::agent("random").unwrap();
    let opponent = beryl_search::agent(agent).unwrap();
    let mut game = Game::prepared(
        Prepared {
            opponent: (opponent.new)(&table, 1, Budget::DEFAULT),
            stand_in: (random.new)(&table, 2, Budget::DEFAULT),
            scripted: false,
            hand_over: None,
            table,
            state,
            human: HUMAN,
            stops,
        },
        SESSION,
    );
    let mut asked = Vec::new();
    while let Some(view) = game.next_view() {
        if view.turn.number >= last {
            break;
        }
        let Status::Deciding { decision } = &view.status else {
            continue;
        };
        asked.push(Stop {
            active: view.turn.active,
            step: view.turn.step,
            stack: !view.stack.is_empty(),
            kind: decision.kind,
            pass_to: decision.pass_to.clone(),
        });
        let attack = decision
            .candidates
            .iter()
            .position(|c| matches!(c.option, Candidate::Attack { .. }));
        let index = match decision.kind {
            Kind::Attacker => attack.unwrap_or_else(|| {
                decision
                    .candidates
                    .iter()
                    .position(|c| c.option == Candidate::Done)
                    .expect("an attack declaration can end")
            }),
            _ => 0,
        };
        let checked = game.answer(decision.id, CandidateIndex(index as u16));
        game.choose(checked.unwrap());
    }
    asked
}

/// Asserts that each pass the human made to a step of the turn took them to that step: the
/// pass button names where the human is next stopped. Their attack is declared in the combat
/// it names.
fn assert_passes_land_where_named(asked: &[Stop]) {
    for pair in asked.windows(2) {
        let (Some(PassTo::Step { step }), next) = (&pair[0].pass_to, &pair[1]) else {
            continue;
        };
        let landed = match next.step {
            Step::DeclareAttackers if next.kind == Kind::Attacker => Step::BeginningOfCombat,
            landed => landed,
        };
        assert_eq!(landed, *step, "passed to {step:?}, stopped at {next:?}");
    }
}

fn priority_in(asked: &[Stop], active: Side, step: Step) -> usize {
    asked
        .iter()
        .filter(|s| s.active == active && s.step == step && s.kind == Kind::Priority)
        .count()
}

/// With nothing to cast at instant speed, the opponent's turn plays on to the human's own
/// without stopping them at its end step; full control stops them there.
#[test]
fn nothing_to_do_at_the_opponents_end_step_does_not_stop_there() {
    let smart = stops_in(&NOTHING_AT_INSTANT_SPEED, StopsSetting::SMART);
    assert!(
        smart.iter().all(|s| s.active == Side::You),
        "stopped in the opponent's turn: {smart:?}"
    );
    assert_eq!(priority_in(&smart, Side::You, Step::PrecombatMain), 1);
    let exact = stops_in(&NOTHING_AT_INSTANT_SPEED, StopsSetting::EXACT);
    assert_eq!(priority_in(&exact, Side::Opponent, Step::End), 1);
}

/// Holding a flash card, the human attacks and is not stopped again until their postcombat
/// main phase, where they could cast it; full control stops them in each combat step.
#[test]
fn holding_flash_does_not_stop_each_combat_step_of_ones_own_turn() {
    let smart = stops_in(&ATTACK_HOLDING_FLASH, StopsSetting::SMART);
    assert!(
        smart.iter().any(|s| s.kind == Kind::Attacker),
        "never attacked: {smart:?}"
    );
    let combat = [
        Step::BeginningOfCombat,
        Step::DeclareAttackers,
        Step::DeclareBlockers,
        Step::CombatDamage,
        Step::EndOfCombat,
    ];
    let in_combat = |asked: &[Stop], step| priority_in(asked, Side::You, step);
    for step in combat {
        assert_eq!(in_combat(&smart, step), 0, "stopped at {step:?}: {smart:?}");
    }
    assert_eq!(priority_in(&smart, Side::You, Step::PostcombatMain), 1);
    assert_passes_land_where_named(&smart);
    // The opponent's end step, where flashing in the Emperor is the play.
    assert_eq!(priority_in(&smart, Side::Opponent, Step::End), 1);
    let exact = stops_in(&ATTACK_HOLDING_FLASH, StopsSetting::EXACT);
    for step in [Step::DeclareAttackers, Step::DeclareBlockers] {
        assert!(in_combat(&exact, step) > 0, "full control passed {step:?}");
    }
}

/// The human's own Mana Crypt trigger resolves without their priority, though they could
/// respond with a flash card; full control asks.
#[test]
fn ones_own_trigger_resolves_without_ones_priority() {
    let on_trigger = |asked: &[Stop]| {
        asked
            .iter()
            .filter(|s| s.active == Side::You && s.step == Step::Upkeep && s.stack)
            .count()
    };
    let smart = stops_in(&CRYPT_HOLDING_FLASH, StopsSetting::SMART);
    assert_eq!(on_trigger(&smart), 0, "stopped at the trigger: {smart:?}");
    let exact = stops_in(&CRYPT_HOLDING_FLASH, StopsSetting::EXACT);
    assert!(on_trigger(&exact) > 0, "full control passed the trigger");
}

/// Holding a flash card, the defending human is stopped once the opponent declares
/// attackers, but not after declaring no blockers.
#[test]
fn holding_flash_stops_after_the_opponents_attack() {
    let smart = stops_against(&DEFEND_HOLDING_FLASH, StopsSetting::SMART, "greedy");
    let attacked = |s: &Stop| s.active == Side::Opponent && s.step == Step::DeclareAttackers;
    assert!(
        smart
            .iter()
            .any(|s| attacked(s) && s.kind == Kind::Priority && !s.stack),
        "not stopped at the attack: {smart:?}"
    );
    assert_eq!(
        priority_in(&smart, Side::Opponent, Step::DeclareBlockers),
        0,
        "stopped after no blocks: {smart:?}"
    );
    assert_passes_land_where_named(&smart);
}

/// The smart stops with the human's phase stops changed: `step` of the opponent's turns a stop
/// or not (`on`).
fn their_stop(step: TurnStep, on: bool) -> StopsSetting {
    let mut steps: Vec<TurnStep> = StopsSetting::SMART.theirs.steps().collect();
    steps.retain(|&s| s != step);
    if on {
        steps.push(step);
    }
    StopsSetting {
        theirs: StepSet::of(&steps),
        ..StopsSetting::SMART
    }
}

/// A phase stop on the opponent's upkeep gives the human, holding a flash card, priority
/// there; one cleared from the opponent's end step passes it by. The pass button names each
/// stop the human lands at.
#[test]
fn phase_stops_move_the_opponents_turn_stops() {
    let upkeep = stops_in(&ATTACK_HOLDING_FLASH, their_stop(TurnStep::Upkeep, true));
    assert_eq!(priority_in(&upkeep, Side::Opponent, Step::Upkeep), 1);
    assert_eq!(priority_in(&upkeep, Side::Opponent, Step::End), 1);
    assert_passes_land_where_named(&upkeep);

    let no_end = stops_in(&ATTACK_HOLDING_FLASH, their_stop(TurnStep::End, false));
    assert!(
        no_end.iter().all(|s| s.active == Side::You),
        "stopped in the opponent's turn: {no_end:?}"
    );
    assert_passes_land_where_named(&no_end);
}

/// With their own precombat main phase cleared, the human is neither halted nor stopped
/// there, and plays their land in the second main phase instead.
#[test]
fn a_cleared_main_phase_stop_passes_it_by() {
    let stops = StopsSetting {
        yours: StepSet::of(&[TurnStep::PostcombatMain]),
        ..StopsSetting::SMART
    };
    let asked = stops_in(&NOTHING_AT_INSTANT_SPEED, stops);
    assert!(
        asked
            .iter()
            .all(|s| !(s.active == Side::You && s.step == Step::PrecombatMain)),
        "stopped in main 1: {asked:?}"
    );
    assert_eq!(priority_in(&asked, Side::You, Step::PostcombatMain), 1);
    assert_passes_land_where_named(&asked);
}

/// The opponent's turn, with two Mana Crypts to cast one after the other, each of which the
/// human could counter.
const TWO_SPELLS_IN_TURN: Scenario = Scenario::new("two-spells-in-turn")
    .at(2, P1, TurnStep::PrecombatMain)
    .battlefield(P0, &[Permanent::new(ISLAND), Permanent::new(ISLAND)])
    .hand(P0, &[COUNTERSPELL])
    .library(P0, LANDS)
    .hand(P1, &[MANA_CRYPT, MANA_CRYPT])
    .library(P1, LANDS);

/// Passing on to the next stop ends where the opponent puts a spell on the stack: the human
/// gets priority over it, with the pass button resolving it (UI Conventions, "Passing to the
/// end step"), and passing it is a pass of its own.
#[test]
fn passing_on_stops_for_a_spell_the_opponent_casts() {
    let mut play = Play::against(&TWO_SPELLS_IN_TURN, Some(Box::new(Casts(0))));
    let crypts = |play: &Play, zone| play.named(MANA_CRYPT, zone, Side::Opponent).len();
    // The first Mana Crypt is on the stack: the human passes on, past its resolution.
    let first = play.decision();
    assert_eq!(crypts(&play, ZoneName::Stack), 1);
    assert_eq!(first.pass_to, Some(PassTo::Resolve));
    assert!(first.pass_on.is_some(), "no pass on: {first:?}");
    let pass = play.game.open().unwrap().pass_on(first.id).unwrap();
    play.game.pass_on(pass);
    play.view = to_human(&mut play.game);
    // The second is cast once the first resolved, and stops them.
    let second = play.decision();
    assert_eq!(second.kind, Kind::Priority);
    assert_eq!(second.pass_to, Some(PassTo::Resolve));
    assert_eq!(play.view.turn.step, Step::PrecombatMain);
    assert_eq!(crypts(&play, ZoneName::Battlefield), 1);
    assert_eq!(crypts(&play, ZoneName::Stack), 1);
    // The pass on is over: this pass resolves the spell and no more.
    play.choose(|c| *c == Candidate::Pass);
    assert_eq!(crypts(&play, ZoneName::Battlefield), 2);
}
