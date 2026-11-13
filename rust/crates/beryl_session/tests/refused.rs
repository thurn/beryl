//! Why the view says a card in the human's hand can't be played at their priority
//! (`DecisionView::refused`), as the engine decides it: no target, no mana, sorcery speed and
//! the permanent imposing it, a land played.

use beryl_cards::CARDS;
use beryl_cards::names::{
    COUNTERSPELL, ISLAND, TEFERI_TIME_RAVELER, THIRST_FOR_DISCOVERY, THUNDERTRAP_TRAINER,
};
use beryl_engine::{
    DecisionKind, Observation, Profile, Replay, Scratch, Seat, Step, TurnStep, next,
};
use beryl_scenario::{Action, P0, P1, Permanent, Scenario};
use beryl_session::protocol::{Moment, PlayerView, RefusalView, player_view};
use beryl_session::{DecisionId, SessionId, Status, StopsSetting};

const LIBRARY: &[&str] = &[ISLAND, ISLAND, ISLAND, ISLAND];
const ISLANDS: &[Permanent] = &[
    Permanent::new(ISLAND),
    Permanent::new(ISLAND),
    Permanent::new(ISLAND),
];

const ID: DecisionId = DecisionId {
    session: SessionId(1),
    seq: 1,
};

/// One refused card: its name, why, and the name of the permanent a timing refusal names.
type Seen = (String, RefusalView, Option<String>);

/// The refused cards of the human's open decision in `v`, by name.
fn seen(v: &PlayerView) -> Vec<Seen> {
    let Status::Deciding { decision } = &v.status else {
        panic!("no decision is open");
    };
    let name = |slot: u16| {
        v.objects
            .iter()
            .find(|o| o.slot == slot)
            .and_then(|o| o.name.clone())
            .expect("a known card")
    };
    let mut seen: Vec<Seen> = decision
        .refused
        .iter()
        .map(|r| {
            let by = match r.reason {
                RefusalView::Timing { by } => by.map(name),
                _ => None,
            };
            (name(r.slot), r.reason.clone(), by)
        })
        .collect();
    seen.sort_by(|a, b| a.0.cmp(&b.0));
    seen
}

/// At each of the human's priority decisions in `scenario`, their refused cards.
fn at_decisions(scenario: &Scenario) -> Vec<Vec<Seen>> {
    let mut all = Vec::new();
    beryl_scenario::visit(&CARDS, scenario, |t, s, d| {
        if d.player == P0 {
            let obs = Observation::new(s, d.player);
            let v = player_view(
                t,
                obs,
                Moment::Deciding(ID, d, None),
                StopsSetting::SMART,
                &[],
            );
            all.push(seen(&v));
        }
    })
    .unwrap_or_else(|e| panic!("{e}"));
    all
}

/// At `scenario`'s start, where the human holds a priority they can only pass, which is a
/// decision under their full control: their refused cards.
fn at_halt(scenario: &Scenario) -> Vec<Seen> {
    let (t, mut s) = beryl_scenario::position(&CARDS, scenario).unwrap();
    s.set_profile(Profile {
        full_control: Some(Seat::P0),
        ..s.profile()
    });
    let Step::Decide(d) = next(&mut s, &t, &mut Scratch::new(), &mut Replay) else {
        panic!("the game is over");
    };
    assert_eq!((d.player, d.kind), (Seat::P0, DecisionKind::Priority));
    let v = player_view(
        &t,
        Observation::new(&s, Seat::P0),
        Moment::Deciding(ID, &d, None),
        StopsSetting::SMART,
        &[],
    );
    seen(&v)
}

#[test]
fn a_counterspell_with_nothing_to_counter_has_no_target() {
    const SCENARIO: Scenario = Scenario::new("counterspell-no-target")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(P0, ISLANDS)
        .hand(P0, &[COUNTERSPELL, ISLAND])
        .library(P0, LIBRARY)
        .library(P1, LIBRARY)
        .actions(&[Action::pass(P0)]);
    assert_eq!(
        at_decisions(&SCENARIO),
        [[(COUNTERSPELL.to_owned(), RefusalView::Target, None)]]
    );
}

#[test]
fn a_creature_short_of_mana_lacks_it_and_a_second_land_waits() {
    // One Island: the Trainer's {1}{U} can't be paid, with its offspring or without. Once an
    // Island is played it can be, and the other Island waits for the next turn.
    const SCENARIO: Scenario = Scenario::new("no-mana-land-played")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(P0, &[Permanent::new(ISLAND)])
        .hand(P0, &[THUNDERTRAP_TRAINER, ISLAND, ISLAND])
        .library(P0, LIBRARY)
        .library(P1, LIBRARY)
        .actions(&[
            Action::PlayLand {
                player: P0,
                card: ISLAND,
            },
            Action::pass(P0),
        ]);
    assert_eq!(
        at_decisions(&SCENARIO),
        [
            vec![(THUNDERTRAP_TRAINER.to_owned(), RefusalView::Mana, None)],
            vec![(ISLAND.to_owned(), RefusalView::LandPlayed, None)],
        ]
    );
}

#[test]
fn teferi_holds_an_instant_to_sorcery_speed() {
    // The human's upkeep: the instant could be cast but for Teferi; the creature without
    // flash waits for the main phase whatever Teferi does.
    const SCENARIO: Scenario = Scenario::new("teferi-sorcery-speed")
        .at(3, P0, TurnStep::Upkeep)
        .permanents(P0, ISLANDS)
        .hand(P0, &[THIRST_FOR_DISCOVERY, THUNDERTRAP_TRAINER])
        .library(P0, LIBRARY)
        .permanents(P1, &[Permanent::new(TEFERI_TIME_RAVELER)])
        .library(P1, LIBRARY);
    let seen = at_halt(&SCENARIO);
    let [(thirst, held, by), trainer] = &seen[..] else {
        panic!("two refused cards: {seen:?}");
    };
    assert_eq!(thirst, THIRST_FOR_DISCOVERY);
    assert!(
        matches!(held, RefusalView::Timing { by: Some(_) }),
        "{held:?}"
    );
    assert_eq!(by.as_deref(), Some(TEFERI_TIME_RAVELER));
    assert_eq!(
        *trainer,
        (
            THUNDERTRAP_TRAINER.to_owned(),
            RefusalView::Timing { by: None },
            None
        )
    );
}
