use beryl_cards::{CARDS, decks::WU_CONTROL, names};
use beryl_engine::state::CounterKind;
use beryl_engine::{
    DecisionKind, Profile, Replay, Scratch, State, Step, TurnStep, apply, next, observe,
};
use beryl_scenario::{Action, Live, P0, P1, Permanent, Scenario};

use super::super::RolloutPolicy;
use super::*;
use crate::long::Family;

const ISLAND: Permanent = Permanent::new(names::ISLAND);
const LIBRARY: &[&str] = &[names::ISLAND; 12];

fn policy(t: &GameTable) -> RolloutPolicy {
    "pimc72/hidden/rollout/tuned/anchor+hold"
        .parse::<Family>()
        .unwrap()
        .rollout_policy(t)
}

/// Plain greedy must want the play, so a pass with hold demonstrates a timing change.
fn check_choice(scenario: &Scenario, setup: &[Action], card: &str, play: bool) {
    let mut live = Live::new(&CARDS, scenario).unwrap();
    for &action in setup {
        live.act(action).unwrap();
    }
    let Step::Decide(d) = live.position(Action::pass(P0)).unwrap() else {
        panic!("{}: the game ended", scenario.name);
    };
    assert_eq!((d.player, d.kind), (P0, DecisionKind::Priority));
    let t = live.table();
    let obs = Observation::new(live.state(), d.player);
    assert_eq!(obs.turn().step(), scenario.setup.step);
    let plain = RolloutPolicy::new(t).choose(t, obs, &d);
    assert_eq!(live.name_of(&d, plain), Some(card), "{}", scenario.name);
    let choice = policy(t).choose(t, obs, &d);
    if play {
        assert_eq!(choice, plain, "{}", scenario.name);
    } else {
        assert_eq!(
            d.candidates.option(choice),
            Some(Opt::Pass),
            "{}",
            scenario.name
        );
    }
}

#[test]
fn hold_waits_to_cast_flash_on_the_opponents_end_step() {
    const FLASH: Scenario = Scenario::new("flash")
        .hand(P0, &[names::SNAPCASTER_MAGE])
        .battlefield(P0, &[ISLAND; 2]);
    check_choice(
        &FLASH.at(4, P0, TurnStep::PrecombatMain),
        &[],
        names::SNAPCASTER_MAGE,
        false,
    );
    check_choice(
        &FLASH.at(4, P1, TurnStep::End),
        &[],
        names::SNAPCASTER_MAGE,
        true,
    );
}

#[test]
fn hold_still_answers_an_opponents_spell() {
    const COUNTER: Scenario = Scenario::new("answer Jace")
        .at(4, P1, TurnStep::PrecombatMain)
        .hand(P1, &[names::JACE_THE_MIND_SCULPTOR])
        .battlefield(P1, &[ISLAND; 4])
        .battlefield(P0, &[ISLAND; 4]);
    for hand in [&[names::MANA_DRAIN], &[names::SUBTLETY]] {
        check_choice(
            &COUNTER.hand(P0, hand),
            &[Action::cast(P1, names::JACE_THE_MIND_SCULPTOR)],
            hand[0],
            true,
        );
    }
}

#[test]
fn hold_uses_an_untargeted_draw_activation_in_main() {
    const DRAW: Scenario = Scenario::new("draw with Staff")
        .battlefield(
            P0,
            &[
                Permanent::new(names::PLAINS),
                Permanent::new(names::STAFF_OF_THE_STORYTELLER).counters(CounterKind::Story, 1),
            ],
        )
        .library(P0, LIBRARY);
    for (step, play) in [
        (TurnStep::Upkeep, false),
        (TurnStep::PrecombatMain, true),
        (TurnStep::BeginningOfCombat, false),
    ] {
        check_choice(
            &DRAW.at(4, P0, step),
            &[],
            names::STAFF_OF_THE_STORYTELLER,
            play,
        );
    }
}

#[test]
fn hold_recognizes_the_mirrors_draw_activations() {
    let abilities = [
        (names::STAFF_OF_THE_STORYTELLER, 2),
        (names::CURRENCY_CONVERTER, 1),
        (names::JACE_VRYNS_PRODIGY, 0),
    ];
    let deck = abilities.map(|(card, _)| (1, card));
    let t = GameTable::new(&CARDS, [&deck, &deck]).unwrap();
    let policy = policy(&t);
    for (name, ability) in abilities {
        let card = t.index_named::<beryl_engine::CardIx>(name).unwrap();
        let draws = policy.values().get(card).draws(ability);
        println!("{name}, ability {ability}: draws = {draws}");
        assert!(draws, "{name}, ability {ability}");
    }
}

#[test]
fn hold_waits_in_main_for_every_instant_or_flash_card_that_does_not_draw() {
    for card in CARDS.cards() {
        let face = card.front();
        if !face.types.intersects(Types::INSTANT) && !face.keywords.intersects(Keywords::FLASH) {
            continue;
        }
        let t = GameTable::new(&CARDS, [&[(1, card.name)], &[(1, names::ISLAND)]]).unwrap();
        let mut s = State::new(&t, 0, Profile::SMART).unwrap();
        let slot = s.library(P0)[0];
        s.board().begin_at(4, P0, TurnStep::PrecombatMain);
        s.board().place(slot, Zone::Hand).unwrap();
        let policy = policy(&t);
        let p = policy.view(&t, Observation::new(&s, P0), P0, false);
        let draws = face
            .spell_ability()
            .is_some_and(|a| p.info(slot).unwrap().draws(index_of(face, a)));
        assert_eq!(
            p.held(Opt::Cast {
                slot,
                face: FaceIx::FRONT,
                costs: Castings::NONE
            }),
            !draws,
            "{}",
            card.name
        );
    }
}

#[test]
fn hold_mirror_casts_at_the_opponents_end_step() {
    let t = GameTable::new(&CARDS, [WU_CONTROL, WU_CONTROL]).unwrap();
    let policy = policy(&t);
    let mut scratch = Scratch::new();
    let mut casts = 0;
    for seed in 0..50 {
        let mut s = State::new(&t, seed, Profile::SMART).unwrap();
        while let Step::Decide(d) = next(&mut s, &t, &mut scratch, &mut Replay) {
            let obs = observe(&s, &t, &mut scratch, d.player);
            let c = policy.choose(&t, obs, &d);
            if obs.turn().step() == TurnStep::End
                && obs.turn().active() != d.player
                && let Some(Opt::Cast { slot, face, .. }) = d.candidates.option(c)
            {
                let f = t.def(obs.card(slot).unwrap()).face(face);
                if f.types.intersects(Types::INSTANT) || f.keywords.intersects(Keywords::FLASH) {
                    casts += 1;
                }
            }
            apply(&mut s, &t, &mut scratch, &mut Replay, c);
        }
    }
    println!("{casts} instant or flash casts at the opponent's end step in 50 mirrors");
    assert!(casts > 0);
}
