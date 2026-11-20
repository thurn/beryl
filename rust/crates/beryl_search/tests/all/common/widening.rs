//! A random game that starts in [`Small`] and goes on in [`Large`] from the engine call that
//! ran out of a capacity, as the app's games do (`beryl_session`'s `Boards::widen`).

use std::collections::HashMap;

use beryl_engine::{
    Choice, Decision, GameTable, Large, Layout, Misfit, Outcome, Replay, Scratch, Small,
    StallReason, State, Step, apply, next, observe,
};
use beryl_search::{Agent, AgentDecision};

use super::agents;

/// How a game went: its outcome, its decisions, and the list that filled if it left
/// [`Small`].
pub struct Widened {
    pub outcome: Outcome,
    pub decisions: u32,
    pub filled: Option<&'static str>,
}

/// The engine call a game makes next: advancing to the next decision, or applying a choice.
#[derive(Clone, Copy)]
enum Call {
    Next,
    Apply(Choice),
}

/// Makes `call` on `s`, returning the step [`next`] reached, if it was that call.
fn call<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    call: Call,
) -> Option<Step<L>> {
    match call {
        Call::Next => Some(next(s, t, x, &mut Replay)),
        Call::Apply(c) => {
            apply(s, t, x, &mut Replay, c);
            None
        }
    }
}

/// Plays game `seed` from `s` with random agents, counting the decisions by kind in `kinds`
/// and checking at every decision that it has a candidate and that every reference names an
/// object. Debug assertions, which `just fuzz` keeps on, check the invariants at every step.
/// When an engine call runs `Small` out of a capacity, the game goes on in `Large` from the
/// state before that call, which makes the call again; running out in `Large` ends the game
/// `Stalled(Capacity)`.
pub fn play_widening(
    t: &GameTable,
    mut s: State<Small>,
    seed: u64,
    kinds: &mut HashMap<String, u64>,
) -> Widened {
    /// Names the game when the engine panics during it.
    struct Game(u64);
    impl Drop for Game {
        fn drop(&mut self) {
            if std::thread::panicking() {
                eprintln!("the panic was in game {}", self.0);
            }
        }
    }
    let _game = Game(seed);
    let mut agents = agents(seed);
    let mut asked = [AgentDecision(0); 2];
    let mut decisions = 0;
    let mut x = Scratch::new();
    let mut c = Call::Next;
    // The game in `Small`, until a call runs out of capacity.
    let (mut large, mut xl, mut step, filled) = loop {
        let before = s;
        let step = call(&mut s, t, &mut x, c);
        if s.outcome() == Some(Outcome::Stalled(StallReason::Capacity)) {
            let mut large: State<Large> = before.widen();
            let mut xl = Scratch::new();
            let step = call(&mut large, t, &mut xl, c);
            let list = filled(&large, t);
            break (large, xl, step, list);
        }
        c = match step {
            None => Call::Next,
            Some(Step::Over(outcome)) => {
                return Widened {
                    outcome,
                    decisions,
                    filled: None,
                };
            }
            Some(Step::Decide(d)) => {
                seen(&s, t, &d, seed, kinds);
                decisions += 1;
                let i = d.decider.index();
                Call::Apply(agents[i].choose(
                    observe(&s, t, &mut x, d.decider),
                    &d,
                    asked[i].take(),
                ))
            }
        };
    };
    // The game in `Large`, from the call that ran out, starting with the step it reached.
    let outcome = loop {
        match step
            .take()
            .unwrap_or_else(|| next(&mut large, t, &mut xl, &mut Replay))
        {
            Step::Over(outcome) => break outcome,
            Step::Decide(d) => {
                seen(&large, t, &d, seed, kinds);
                decisions += 1;
                let i = d.decider.index();
                let c =
                    agents[i].choose(observe(&large, t, &mut xl, d.decider), &d, asked[i].take());
                apply(&mut large, t, &mut xl, &mut Replay, c);
            }
        }
    };
    Widened {
        outcome,
        decisions,
        filled: Some(filled),
    }
}

/// Counts decision `d` of game `seed` by its kind, and fails the game if `d` has no candidate
/// or a reference in `s` names no object.
fn seen<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    d: &Decision<L>,
    seed: u64,
    kinds: &mut HashMap<String, u64>,
) {
    *kinds.entry(format!("{:?}", d.kind)).or_default() += 1;
    assert!(!d.is_empty(), "seed {seed}: {d:?}");
    if let Err(v) = beryl_engine::check_references(s, t) {
        panic!("seed {seed}: a reference: {v:?}");
    }
}

/// The list of `Small` that the widened game `large` outgrew, just after the call that ran out.
/// A list back within its room by then filled only during the call.
fn filled(large: &State<Large>, t: &GameTable) -> &'static str {
    match large.fits::<Small>(t) {
        Err(Misfit::ListTooLong { list, .. }) => list,
        Err(Misfit::PoolTooSmall { .. }) => "the token and copy pool",
        Err(_) => "another limit",
        Ok(()) => "a list, during one call",
    }
}
