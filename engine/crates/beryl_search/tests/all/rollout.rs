//! The softmax variant of the rollout policy and the greedy policy's Portable Hole rule, over
//! the WU Control mirror.

use beryl_cards::decks::WU_CONTROL;
use beryl_cards::{CARDS, names};
use beryl_engine::card::Words;
use beryl_engine::rng::SplitMix64;
use beryl_engine::{
    Choice, Format, GameTable, Opt, Profile, Replay, Scratch, State, Step, apply, next, observe,
};
use beryl_search::{RolloutPolicy, Softmax, rollout, soft_rollout};

/// At a temperature near zero the softmax policy plays greedy's game.
#[test]
fn a_cold_softmax_is_greedy() {
    let t = GameTable::new(Format::CONSTRUCTED, &CARDS, [WU_CONTROL, WU_CONTROL]).unwrap();
    let policy = RolloutPolicy::new(&t);
    let mut rng = SplitMix64::new(3);
    for seed in 0..4 {
        let start = State::new(&t, seed, Profile::SMART).unwrap();
        let mut x = Scratch::new();
        let greedy = rollout(&mut start.clone(), &t, &mut x, &policy);
        let mut soft = Softmax {
            rng: &mut rng,
            temperature: 1e-9,
        };
        let cold = soft_rollout(&mut start.clone(), &t, &mut x, &policy, &mut soft);
        assert_eq!(greedy, cold, "seed {seed}");
    }
}

/// Greedy casts Portable Hole only with something its enters trigger could exile: a
/// nonland permanent of the opponent's with mana value 2 or less. Deep Searcher, anchored to
/// greedy, once cast it onto a board of one land.
#[test]
fn greedy_holds_a_portable_hole_with_nothing_to_exile() {
    let t = GameTable::new(Format::CONSTRUCTED, &CARDS, [WU_CONTROL, WU_CONTROL]).unwrap();
    let policy = RolloutPolicy::new(&t);
    let hole = CARDS.get(names::PORTABLE_HOLE).unwrap().faces[0].abilities[0].targets()[0];
    let mut x = Scratch::new();
    let (mut cast, mut held) = (0, 0);
    for seed in 0..60 {
        let mut s = State::new(&t, seed, Profile::SMART).unwrap();
        while let Step::Decide(d) = next(&mut s, &t, &mut x, &mut Replay) {
            let obs = observe(&s, &t, &mut x, d.decider);
            let target = obs.battlefield().iter().any(|&slot| {
                obs.controller(slot) != d.player
                    && obs.matches(&t, &hole.filter, slot, &Words::NONE)
            });
            let is_hole = |c: Choice| {
                matches!(d.candidates.option(c), Some(Opt::Cast { slot, .. })
                    if obs.card(slot).is_some_and(|card| t.def(card).name == names::PORTABLE_HOLE))
            };
            let g = policy.choose(&t, obs, &d);
            if is_hole(g) {
                assert!(
                    target,
                    "seed {seed}: Portable Hole cast with nothing to exile"
                );
                cast += 1;
            } else if !target && (0..d.len()).any(|i| is_hole(Choice(i as u16))) {
                held += 1;
            }
            apply(&mut s, &t, &mut x, &mut Replay, g);
        }
    }
    assert!(cast > 0 && held > 0, "cast {cast}, held {held}");
}

/// In a shared-deck game, the rollout policy takes a free mulligan whenever it is offered and
/// the hand is not one it would keep, and takes a regular mulligan when no free one is offered.
#[test]
fn rollout_policy_takes_free_mulligan_when_offered_for_a_mulligan_hand() {
    let t = GameTable::new(Format::SHARED_DECK, &CARDS, [&[(80, names::ISLAND)], &[]]).unwrap();
    let policy = RolloutPolicy::new(&t);
    let mut s = State::new(&t, 3, Profile::SMART).unwrap();
    let mut x = Scratch::new();

    // Answer PlayOrDraw.
    let Step::Decide(d) = next(&mut s, &t, &mut x, &mut Replay) else {
        panic!("expected PlayOrDraw");
    };
    let obs = observe(&s, &t, &mut x, d.decider);
    let c = policy.choose(&t, obs, &d);
    apply(&mut s, &t, &mut x, &mut Replay, c);

    // 7-land hand: policy does not keep and FreeMulligan is offered, so it chooses FreeMulligan.
    let Step::Decide(d) = next(&mut s, &t, &mut x, &mut Replay) else {
        panic!("expected KeepOrMulligan");
    };
    let obs = observe(&s, &t, &mut x, d.decider);
    let c = policy.choose(&t, obs, &d);
    assert_eq!(d.candidates.option(c), Some(Opt::FreeMulligan));
}
