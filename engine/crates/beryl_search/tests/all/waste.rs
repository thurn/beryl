//! The rollout policy's waste rule: it does not evoke a spell that is not worth the card, and
//! in a softmax rollout it does not pick an opposing permanent when it chooses its own.

use beryl_cards::decks::WU_CONTROL;
use beryl_cards::{CARDS, names};
use beryl_engine::card::{Castings, Purpose};
use beryl_engine::rng::SplitMix64;
use beryl_engine::{
    Candidates, DecisionKind, Format, GameTable, Observation, Opt, Profile, Replay, Scratch, State,
    Step, TurnStep, Zone, next,
};
use beryl_scenario::{Action, Live, P0, P1, Permanent, Scenario};
use beryl_search::{
    Knowledge, ROLLOUT_TEMPERATURE, RolloutPolicy, Rules, Softmax, soft_rollout_after,
};

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
        .permanents(P1, &[Permanent::new(names::PLAINS); 3]);

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
fn waste_declines_opposing_permanents_in_softmax_mirrors() {
    let t = GameTable::new(Format::CONSTRUCTED, &CARDS, [WU_CONTROL, WU_CONTROL]).unwrap();
    let mut scratch = Scratch::new();
    let mut choices = 0;
    let mut opposing_offers = 0;
    let mut opposing_picks = 0;
    let policy = waste_policy(&t);
    for seed in 0..20 {
        let mut s = State::new(&t, seed, Profile::SMART).unwrap();
        let Step::Decide(d) = next(&mut s, &t, &mut scratch, &mut Replay) else {
            panic!("expected opening decision");
        };
        let mut rng = SplitMix64::new(seed);
        let mut soft = Softmax {
            rng: &mut rng,
            temperature: ROLLOUT_TEMPERATURE,
        };
        let choice = policy.choose_soft(&t, Observation::new(&s, d.decider), &d, &mut soft);
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
    assert!(opposing_offers > 0);
    assert!(choices > 0);
    assert_eq!(
        opposing_picks, 0,
        "opposing permanent choices: {opposing_picks} of {choices}, with {opposing_offers} offers"
    );
}
