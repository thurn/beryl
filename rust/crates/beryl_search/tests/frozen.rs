//! `expert-frozen` never changes (docs/ai-strategy.md §3): its moves on a few seeded games are
//! pinned, so that a refactor of it, or of what it builds on, cannot move the bar silently.

mod common;

use beryl_engine::{Profile, Replay, Scratch, State, Step, apply, next, observe};
use beryl_search::{Agent, AgentDecision, FrozenExpertAgent, RandomAgent};
use common::mirror;

/// 64-bit FNV-1a, stable across runs and releases.
fn fnv(h: u64, byte: u8) -> u64 {
    (h ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
}

/// A hash of every choice in four seeded games, each game's outcome after its choices: two of
/// the frozen expert against itself and two against the random agent, seats alternated.
fn moves() -> u64 {
    let t = mirror();
    let mut h = 0xcbf2_9ce4_8422_2325;
    for seed in 0..4u64 {
        let frozen = |seed| Box::new(FrozenExpertAgent::new(&t, seed)) as Box<dyn Agent>;
        let mut agents: [Box<dyn Agent>; 2] = if seed < 2 {
            [frozen(seed), frozen(!seed)]
        } else {
            [frozen(seed), Box::new(RandomAgent::new(seed))]
        };
        if seed % 2 == 1 {
            agents.swap(0, 1);
        }
        let mut s = State::new(&t, seed, Profile::SMART).unwrap();
        let x = &mut Scratch::new();
        let mut asked = [AgentDecision(0); 2];
        let outcome = loop {
            match next(&mut s, &t, x, &mut Replay) {
                Step::Over(outcome) => break outcome,
                Step::Decide(d) => {
                    let i = d.player.index();
                    let c = agents[i].choose(observe(&s, &t, x, d.player), &d, asked[i].take());
                    h = fnv(h, u8::try_from(c.0).expect("a choice of a Small game"));
                    apply(&mut s, &t, x, &mut Replay, c);
                }
            }
        };
        h = format!("{outcome:?}").bytes().fold(h, fnv);
    }
    h
}

/// A change that moves this pin is a change to the reference opponent: every result measured
/// against it is void. Only an engine or card fix may do that, on purpose. It moved once
/// besides, with its rules unchanged, when every agent came to draw each decision's randomness
/// from its seed and the decision's index: the frozen expert deals the cards it cannot see in
/// its combat simulations that way, and the random opponent here picks that way.
#[test]
fn frozen_moves_never_change() {
    assert_eq!(moves(), 2_371_047_557_203_317_184);
}
