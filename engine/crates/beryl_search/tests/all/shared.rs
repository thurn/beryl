//! Hidden information in a shared-deck game: information independence, hidden picks from the
//! one unknown pool, and `Chance::replay` over a recorded shared-format game.

use std::hash::{DefaultHasher, Hash, Hasher};

use beryl_cards::{CARDS, names};
use beryl_engine::chance::Chance;
use beryl_engine::rng::SplitMix64;
use beryl_engine::state::Pending;
use beryl_engine::{
    Choice, Decision, Format, GameTable, Observation, Profile, Replay, Scratch, Seat, Slot, State,
    Step, Zone, ZoneKey, apply, next, observe,
};
use beryl_search::{Agent, AgentDecision, LongSearch, Settings};

use crate::common::{agents, play, redeal};

const SHARED_DECK: &[(u8, &str)] = &[
    (28, names::ISLAND),
    (12, names::PLAINS),
    (12, names::SNAPCASTER_MAGE),
    (10, names::COUNTERSPELL),
    (10, names::MANA_DRAIN),
    (8, names::BRAINSTORM),
];

fn shared_table(deck: &[(u8, &'static str)]) -> GameTable {
    GameTable::new(Format::SHARED_DECK, &CARDS, [deck, &[]]).unwrap()
}

/// An agent that hashes everything an `Observation` may show: if a view ever betrays a hidden
/// card, this agent's choice can differ between the true state and a redeterminized one.
struct Observer;

impl Agent for Observer {
    fn choose(&mut self, obs: Observation<'_>, d: &Decision, _: AgentDecision) -> Choice {
        let mut h = DefaultHasher::new();
        for slot in Slot::all() {
            let o = obs.object(slot);
            (
                o.zone(),
                o.owner(),
                obs.controller(slot),
                o.face(),
                o.flags(),
            )
                .hash(&mut h);
            obs.card(slot).hash(&mut h);
        }
        for seat in obs.zone_owners(Zone::Graveyard).in_seat_order() {
            obs.graveyard(seat).hash(&mut h);
        }
        for seat in obs.zone_owners(Zone::Library).in_seat_order() {
            for &slot in obs.library(seat) {
                obs.knows(slot).then_some(slot).hash(&mut h);
            }
        }
        (obs.viewer(), obs.turn(), obs.battlefield()).hash(&mut h);
        Choice((h.finish() % d.len() as u64) as u16)
    }
}

#[test]
fn agents_decide_the_same_on_a_redeterminized_shared_state() {
    let t = shared_table(SHARED_DECK);
    let mut rng = SplitMix64::new(7);
    let (mut decisions, mut redealt) = (0, 0);
    for seed in 0..4 {
        let mut s = State::new(&t, seed, Profile::EXACT).unwrap();
        play(
            &t,
            &mut s,
            &mut Scratch::new(),
            &mut agents(seed),
            |s, d| {
                let viewer = d.decider;
                let mut det = redeal(s, viewer, &t, &mut rng);
                det.check_invariants(&t).unwrap();
                for slot in Slot::all() {
                    if s.knows(viewer, slot) {
                        assert_eq!(det.object(slot), s.object(slot), "slot {slot}");
                    }
                }
                let ours = Observer.choose(Observation::new(s, viewer), d, AgentDecision(0));
                let theirs = Observer.choose(Observation::new(&det, viewer), d, AgentDecision(0));
                assert_eq!(
                    ours, theirs,
                    "seed {seed}: the view of a determinized shared state differs"
                );
                match next(&mut det, &t, &mut Scratch::new(), &mut Replay) {
                    Step::Decide(again) => assert_eq!(again, *d, "seed {seed}"),
                    Step::Over(o) => {
                        panic!("seed {seed}: the determinized shared game ended: {o:?}")
                    }
                }
                decisions += 1;
                if Slot::all().any(|slot| det.card_behind(slot) != s.card_behind(slot)) {
                    redealt += 1;
                }
            },
        );
    }
    assert!(decisions > 40, "too few decisions: {decisions}");
    assert!(redealt * 2 > decisions, "{redealt} of {decisions}");
}

#[test]
fn hidden_searches_decide_the_same_on_a_redeterminized_shared_state() {
    let t = shared_table(SHARED_DECK);
    let mut s = State::new(&t, 5, Profile::EXACT).unwrap();
    let mut x = Scratch::new();
    let mut players = agents(5);
    let mut asked = [AgentDecision(0); 2];
    let mut target = None;

    for _ in 0..200 {
        let Step::Decide(d) = next(&mut s, &t, &mut x, &mut Replay) else {
            break;
        };
        if s.turn().number() >= 3 && d.len() > 2 {
            target = Some((s, d));
            break;
        }
        let i = d.decider.index();
        let c = players[i].choose(observe(&s, &t, &mut x, d.decider), &d, asked[i].take());
        apply(&mut s, &t, &mut x, &mut Replay, c);
    }

    let (s, d) = target.expect("reached a mid-game shared decision");
    let det = observe(&s, &t, &mut x, d.decider).determinize(&t, &mut SplitMix64::new(17));
    assert!(Slot::all().any(|slot| det.card_behind(slot) != s.card_behind(slot)));

    for family in [
        "ismcts2/hidden/rollout/tuned",
        "pimc2/hidden/cut1/estimate",
        "turn72/hidden/rollout/tuned/anchor+guard",
    ] {
        let set = Settings {
            family: family.parse().unwrap(),
            work: 4_000,
            threads: 1,
        };
        let mut a = LongSearch::new(&t, set, 3);
        let mut b = LongSearch::new(&t, set, 3);
        let ca = a.choose(observe(&s, &t, &mut x, d.decider), &d, AgentDecision(0));
        let cb = b.choose(observe(&det, &t, &mut x, d.decider), &d, AgentDecision(0));
        assert_eq!(
            ca, cb,
            "{family}: choice differed on redeterminized shared state"
        );
        assert_eq!(
            a.last_search(),
            b.last_search(),
            "{family}: search report differed on redeterminized shared state"
        );
    }
}

#[test]
fn a_shared_hidden_pick_redeals_from_the_shared_pool() {
    let deck: &[(u8, &str)] = &[
        (24, names::SHOW_AND_TELL),
        (32, names::ISLAND),
        (12, names::GRISELBRAND),
        (12, names::COUNTERSPELL),
    ];
    let t = shared_table(deck);
    let mut rng = SplitMix64::new(7);
    let (mut reached, mut redealt) = (0, 0);
    for seed in 0..6 {
        let mut s = State::new(&t, seed, Profile::EXACT).unwrap();
        play(
            &t,
            &mut s,
            &mut Scratch::new(),
            &mut agents(seed),
            |s, d| {
                let Pending::Resolving(r) = s.pending() else {
                    return;
                };
                let picks: Vec<Slot> = r.hidden_picks().collect();
                let viewer = d.decider;
                if !r.asks_hidden() || picks.iter().all(|&slot| s.knows(viewer, slot)) {
                    return;
                }
                reached += 1;
                let mut x = Scratch::new();
                let seen = Observer.choose(observe(s, &t, &mut x, viewer), d, AgentDecision(0));
                for _ in 0..4 {
                    let mut det = redeal(s, viewer, &t, &mut rng);
                    det.check_invariants(&t).unwrap();
                    for &slot in &picks {
                        let card = det.card_behind(slot).expect("a card picked");
                        assert_ne!(t.def(card).name, names::COUNTERSPELL, "seed {seed}");
                        redealt += usize::from(det.card_behind(slot) != s.card_behind(slot));
                    }
                    let theirs =
                        Observer.choose(observe(&det, &t, &mut x, viewer), d, AgentDecision(0));
                    assert_eq!(seen, theirs, "seed {seed}: the paused view differs");
                    match next(&mut det, &t, &mut Scratch::new(), &mut Replay) {
                        Step::Decide(again) => assert_eq!(again, *d, "seed {seed}"),
                        Step::Over(o) => panic!("seed {seed}: the redealt game ended: {o:?}"),
                    }
                }
            },
        );
    }
    assert!(reached > 0, "no second player's hidden pick was asked");
    assert!(redealt > 0, "no picked card was redealt");
}

#[test]
fn a_recorded_shared_game_replays_without_mismatch() {
    let t = shared_table(SHARED_DECK);
    for seed in 10..15 {
        let initial = State::new(&t, seed, Profile::EXACT).unwrap();
        let mut s = initial;
        let mut x = Scratch::new();
        x.events.set_chance(Some(Chance::default()));

        let mut players = agents(seed);
        let mut asked = [AgentDecision(0); 2];
        let mut choices = Vec::new();
        let mut decisions = Vec::new();
        let mut known_cards = Vec::new();
        let mut outcome = None;

        for _ in 0..400 {
            match next(&mut s, &t, &mut x, &mut Replay) {
                Step::Over(o) => {
                    outcome = Some(o);
                    break;
                }
                Step::Decide(d) => {
                    decisions.push(d);
                    known_cards.push(snapshot_known(&s));
                    let c = if d.kind == beryl_engine::DecisionKind::KeepOrMulligan {
                        Choice(0)
                    } else {
                        let i = d.decider.index();
                        players[i].choose(observe(&s, &t, &mut x, d.decider), &d, asked[i].take())
                    };
                    choices.push(c);
                    apply(&mut s, &t, &mut x, &mut Replay, c);
                }
            }
        }
        assert!(
            choices.len() > 20,
            "seed {seed}: too few choices recorded: {}",
            choices.len()
        );

        let mut chance = x.events.take_chance().unwrap();
        assert_eq!(chance.take_mismatch(), None, "seed {seed}");
        let record = chance.take();
        assert!(!record.is_empty(), "seed {seed}");

        // Replay the recorded outcomes and choices from a state whose shared library holds
        // the same deck in a different random order and with a different RNG seed.
        let mut replay_state = Observation::new(&initial, Seat::P0)
            .determinize(&t, &mut SplitMix64::new(seed + 1_000));
        assert!(
            Slot::all().any(|slot| replay_state.card_behind(slot) != initial.card_behind(slot)),
            "seed {seed}: replay started with the same card order"
        );
        let mut rx = Scratch::new();
        let mut replay_chance = Chance::default();
        replay_chance.replay(record);
        rx.events.set_chance(Some(replay_chance));

        for (step, ((expected_d, expected_known), &choice)) in
            decisions.iter().zip(&known_cards).zip(&choices).enumerate()
        {
            match next(&mut replay_state, &t, &mut rx, &mut Replay) {
                Step::Decide(d) => {
                    assert_eq!(
                        &d, expected_d,
                        "seed {seed}: decision mismatch at step {step}"
                    );
                    assert_eq!(
                        snapshot_known(&replay_state),
                        *expected_known,
                        "seed {seed}: known cards differed at step {step}"
                    );
                    apply(&mut replay_state, &t, &mut rx, &mut Replay, choice);
                }
                Step::Over(o) => panic!("seed {seed}: replay ended early at step {step}: {o:?}"),
            }
        }
        if let Some(expected_outcome) = outcome {
            match next(&mut replay_state, &t, &mut rx, &mut Replay) {
                Step::Over(o) => assert_eq!(o, expected_outcome, "seed {seed}"),
                Step::Decide(d) => panic!("seed {seed}: expected game over, got {d:?}"),
            }
        }

        let mut replay_chance = rx.events.take_chance().unwrap();
        assert_eq!(replay_chance.take_mismatch(), None, "seed {seed}");
        replay_state.check_invariants(&t).unwrap();
        for seat in Seat::BOTH {
            assert_eq!(
                replay_state.zone(ZoneKey::Of(seat, Zone::Hand)).count(),
                s.zone(ZoneKey::Of(seat, Zone::Hand)).count(),
            );
        }
        assert_eq!(
            replay_state.library(Seat::P0).len(),
            s.library(Seat::P0).len()
        );
        assert_eq!(
            replay_state.graveyard(Seat::P0).len(),
            s.graveyard(Seat::P0).len()
        );
    }
}

fn snapshot_known(s: &State) -> Vec<(Slot, Option<beryl_engine::CardIx>)> {
    Slot::all()
        .filter(|&slot| s.knows(Seat::P0, slot) || s.knows(Seat::P1, slot))
        .map(|slot| (slot, s.card_behind(slot)))
        .collect()
}
