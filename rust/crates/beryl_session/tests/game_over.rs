//! What the end of the game says of why the loser lost
//! ([CR 104.3](https://yawgatog.com/resources/magic-rules/#R1043)): "There are several ways to
//! lose the game." The card whose effect ended it, with the ability that did, or the source of
//! the damage that took the last life.

use beryl_cards::{CARDS, names};
use beryl_engine::event::{Event, Recipient};
use beryl_engine::two_player::TwoPlayerSeat as _;
use beryl_engine::{
    Choice, Observation, Opt, Outcome, Replay, Scratch, Seat, Step, TurnStep, apply, next,
};
use beryl_scenario::{Action, P0, P1, Permanent, Scenario};
use beryl_session::StopsSetting;
use beryl_session::protocol::{
    AbilityClass, AbilityRef, Culprit, LossReason, Moment, OutcomeView, Side, Status, player_view,
};

/// The end of `scenario` as `viewer` is told it: its actions that answer decisions are taken,
/// every other decision passed or answered with its first candidate, until the game ends.
fn ending(scenario: &Scenario, viewer: Seat) -> OutcomeView {
    let (t, mut s) = beryl_scenario::position(&CARDS, scenario).unwrap_or_else(|e| panic!("{e}"));
    let mut x = Scratch::recording();
    let scripted = scenario.actions.iter().filter(|a| {
        matches!(
            a,
            Action::Cast { .. } | Action::Activate { .. } | Action::Answer { .. }
        )
    });
    let mut actions = scripted.peekable();
    let outcome = loop {
        let d = match next(&mut s, &t, &mut x, &mut Replay) {
            Step::Decide(d) => d,
            Step::Over(o) => break o,
        };
        let c = match actions.peek() {
            Some(action) => match beryl_scenario::answer(&t, &s, &d, action) {
                Some(c) => {
                    actions.next();
                    c
                }
                None if d.len() == 1 => Choice(0),
                None => panic!(
                    "{action:?} doesn't answer {}",
                    beryl_scenario::describe(&t, &s, &d)
                ),
            },
            None => (0..d.len() as u16)
                .map(Choice)
                .find(|&c| d.candidates.option(c) == Some(Opt::Pass))
                .unwrap_or(Choice(0)),
        };
        apply(&mut s, &t, &mut x, &mut Replay, c);
    };
    let Outcome::Win(winner) = outcome else {
        panic!("no winner: {outcome:?}");
    };
    let loser = winner.opponent();
    let events = x.events.events();
    let cause = events.iter().find_map(|e| match *e {
        Event::Lose { player, cause } if player == loser => Some(cause),
        _ => None,
    });
    assert!(cause.is_some(), "the loss is an event");
    // As the session finds it: the last event that lowered the loser's life.
    let hurt_by = events.iter().rev().find_map(|e| match *e {
        Event::Damage {
            source,
            to: Recipient::Player(p),
            ..
        } if p == loser => Some(Some(source)),
        Event::LifeLoss { player, .. } if player == loser => Some(None),
        _ => None,
    });
    let view = player_view(
        &t,
        Observation::new(&s, viewer),
        Moment::Over {
            outcome,
            cause,
            hurt_by: hurt_by.flatten(),
        },
        StopsSetting::SMART,
        &[],
    );
    let Status::Over { outcome } = view.status else {
        panic!("the game is over");
    };
    outcome
}

fn jace(name: &str) -> &'static Scenario {
    let (_, scenarios) = beryl_cards::scenarios()
        .iter()
        .find(|&&(c, _)| c == names::JACE_WIELDER_OF_MYSTERIES)
        .expect("Jace's scenarios");
    scenarios
        .iter()
        .find(|s| s.name == name)
        .expect("the scenario")
}

fn by_jace(kind: AbilityClass, nth: u8) -> Culprit {
    Culprit {
        name: names::JACE_WIELDER_OF_MYSTERIES.to_owned(),
        ability: Some(AbilityRef { kind, nth }),
        text: None,
    }
}

#[test]
fn jaces_replacement_names_jace_and_his_first_paragraph() {
    let scenario = jace("+1 milling his controller empty wins on the draw");
    assert_eq!(
        ending(scenario, P1),
        OutcomeView::Win {
            winner: Side::Opponent,
            reason: LossReason::Effect,
            by: Some(by_jace(AbilityClass::Static, 0)),
        }
    );
}

#[test]
fn jaces_minus_eight_names_his_second_loyalty_ability() {
    let scenario = jace("−8 draws the library empty and wins");
    assert_eq!(
        ending(scenario, P0),
        OutcomeView::Win {
            winner: Side::You,
            reason: LossReason::Effect,
            by: Some(by_jace(AbilityClass::Activated, 1)),
        }
    );
}

#[test]
fn drawing_from_an_empty_library_names_no_card() {
    let scenario = jace("the opponent's draw from an empty library still loses");
    assert_eq!(
        ending(scenario, P1),
        OutcomeView::Win {
            winner: Side::Opponent,
            reason: LossReason::Library,
            by: None,
        }
    );
}

/// Mana Crypt's flip, lost on this seed as in its own scenarios, deals its controller their
/// last 3 life.
static CRYPT_TO_ZERO: Scenario = Scenario::new("Mana Crypt takes the last 3 life")
    .seed(2)
    .at(2, P1, TurnStep::PostcombatMain)
    .life(P0, 3)
    .battlefield(P0, &[Permanent::new(names::MANA_CRYPT)])
    .library(P0, &[names::ISLAND]);

#[test]
fn zero_life_names_the_damage_source() {
    assert_eq!(
        ending(&CRYPT_TO_ZERO, P0),
        OutcomeView::Win {
            winner: Side::Opponent,
            reason: LossReason::Life,
            by: Some(Culprit {
                name: names::MANA_CRYPT.to_owned(),
                ability: None,
                text: None,
            }),
        }
    );
}
