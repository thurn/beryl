use beryl_cards::{CARDS, decks::WU_CONTROL, names};
use beryl_engine::card::Purpose;
use beryl_engine::rng::SplitMix64;
use beryl_engine::{Candidates, DecisionKind, Profile, Replay, Scratch, State, Step, next};
use beryl_scenario::{Action, Live, P0, P1, Permanent, Scenario};

use super::super::{RolloutPolicy, Softmax, soft_rollout_after};
use super::*;
use crate::{ROLLOUT_TEMPERATURE, values::Knowledge};

fn waste_policy(t: &GameTable) -> RolloutPolicy {
    RolloutPolicy::with_rules(
        t,
        Knowledge::Tuned,
        Rules {
            waste: true,
            ..Rules::default()
        },
    )
}

#[test]
fn waste_evokes_only_for_a_worthwhile_spell() {
    const EVOKE: Scenario = Scenario::new("evoke Subtlety")
        .at(4, P1, TurnStep::PrecombatMain)
        .hand(P0, &[names::SUBTLETY, names::SNAPCASTER_MAGE])
        .hand(P1, &[names::ELITE_SPELLBINDER])
        .battlefield(P1, &[Permanent::new(names::PLAINS); 3]);

    for answer_spell in [false, true] {
        let mut live = Live::new(&CARDS, &EVOKE).unwrap();
        if answer_spell {
            live.act(Action::cast(P1, names::ELITE_SPELLBINDER))
                .unwrap();
        } else {
            live.act(Action::pass(P1)).unwrap();
        }
        let Step::Decide(d) = live.position(Action::pass(P0)).unwrap() else {
            panic!("expected priority");
        };
        assert_eq!((d.player, d.kind), (P0, DecisionKind::Priority));

        let t = live.table();
        let obs = Observation::new(live.state(), P0);
        let plain = RolloutPolicy::new(t).choose(t, obs, &d);
        assert!(matches!(
            d.candidates.option(plain),
            Some(Opt::Cast { costs, .. }) if costs == Castings::only(0)
        ));
        let choice = waste_policy(t).choose(t, obs, &d);
        if answer_spell {
            assert_eq!(choice, plain);
        } else {
            assert_eq!(d.candidates.option(choice), Some(Opt::Pass));
        }
    }
}

#[test]
fn waste_scores_every_evoke_cast_zero_on_an_empty_board_and_stack() {
    for card in CARDS.cards() {
        for (face, definition) in [FaceIx::FRONT, FaceIx::BACK].into_iter().zip(card.faces) {
            for (index, option) in definition.casting.iter().enumerate() {
                if !matches!(option, CastingOption::Evoke { .. }) {
                    continue;
                }
                let t = GameTable::new(&CARDS, [&[(1, card.name)], &[(1, names::ISLAND)]]).unwrap();
                let mut s = State::new(&t, 0, Profile::SMART).unwrap();
                let slot = s.library(P0)[0];
                s.board().place(slot, Zone::Hand).unwrap();
                let policy = waste_policy(&t);
                let p = policy.view(&t, Observation::new(&s, P0), P0, false);
                let costs = Castings::only(u8::try_from(index).unwrap());
                assert_eq!(p.cast(slot, face, costs), 0, "{}", card.name);
            }
        }
    }
}

#[test]
fn waste_declines_opposing_permanents_in_softmax_mirrors() {
    let t = GameTable::new(&CARDS, [WU_CONTROL, WU_CONTROL]).unwrap();
    let mut scratch = Scratch::new();
    let mut choices = 0;
    let mut opposing_offers = 0;
    let mut opposing_picks = 0;
    let policy = waste_policy(&t);
    for seed in 0..200 {
        let mut s = State::new(&t, seed, Profile::SMART).unwrap();
        let Step::Decide(d) = next(&mut s, &t, &mut scratch, &mut Replay) else {
            panic!("expected opening decision");
        };
        let mut rng = SplitMix64::new(seed);
        let mut soft = Softmax {
            rng: &mut rng,
            temperature: ROLLOUT_TEMPERATURE,
        };
        let choice = policy.choose_soft(&t, Observation::new(&s, d.player), &d, &mut soft);
        soft_rollout_after(
            &mut s,
            &t,
            &mut scratch,
            &policy,
            &mut soft,
            choice,
            |obs, d, c| {
                if d.kind != DecisionKind::ChooseUpTo || d.prompt.purpose == Purpose::Search {
                    return;
                }
                let Candidates::Options(options) = &d.candidates else {
                    panic!("expected choose-up-to options");
                };
                let opposing_permanent = |option| {
                    matches!(option, Opt::Object(slot) if obs.object(slot).zone() == Zone::Battlefield
                        && obs.controller(slot) != d.player)
                };
                opposing_offers += usize::from(options.iter().copied().any(opposing_permanent));
                if let Some(Opt::Object(slot)) = d.candidates.option(c)
                    && obs.object(slot).zone() == Zone::Battlefield
                {
                    choices += 1;
                    opposing_picks += usize::from(obs.controller(slot) != d.player);
                }
            },
        );
    }
    println!(
        "Opposing permanent choices: {opposing_picks} of {choices}, with {opposing_offers} offers"
    );
    assert!(opposing_offers > 0);
    assert!(choices > 0);
    assert_eq!(opposing_picks, 0);
}
