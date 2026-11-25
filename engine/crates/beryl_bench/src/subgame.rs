//! The cost of a rollout step inside a subgame, in the `Large` layout, against the same kind
//! of step in a plain `Large` game (`docs/benchmarking.md`, "The present-cost gate").
//!
//! A subgame (Shahrazad) is a second game played to its end while the main game waits, and
//! only `State<Large>` can suspend a game, so the paired-control method of [`crate::family`]
//! (which runs `State<Small>`) cannot measure it: a state inside a subgame has no control
//! with the card taken out. Instead, two sets of positions are cut from the same seeded games
//! between rollout-policy players, each with Shahrazad in its deck: the first decision of
//! every third turn of a subgame, and the first decision of the same turn numbers of a main
//! game that has not entered a subgame yet. A subgame starts at turn 1 with fresh hands, so
//! both sets are the opening turns of a game. Each set is rolled out under the rollout policy,
//! each in a counted function of its own ([`subgame_step`], [`plain_step`]), and the
//! benchmark reports instructions per step for both.
//!
//! The steps before a subgame begins cost what a plain game's do: Shahrazad is a spell with
//! no static or triggered ability, so the control of such a step is the step.

use std::hint::black_box;

use beryl_cards::CARDS;
use beryl_cards::names;
use beryl_engine::{
    Choice, Decision, Format, GameTable, Large, Observation, Profile, Scratch, State, Step, Stops,
    apply, next,
};
use beryl_search::{Inline, RolloutPolicy};

use crate::family::{EVERY, Family, MAX_GAMES, PER_GAME, POSITIONS, ROLLOUTS};
use crate::rollout::SEED;

/// Shahrazad swapped into [`WU_CONTROL`](beryl_cards::decks::WU_CONTROL), four copies, so a
/// seeded game casts one often enough.
pub const DECK: Family = Family {
    name: "subgame",
    swaps: &[
        (1, names::SHAHRAZAD, names::REPRIEVE),
        (1, names::SHAHRAZAD, names::OUST),
        (1, names::SHAHRAZAD, names::PORTABLE_HOLE),
        (1, names::SHAHRAZAD, names::NO_MORE_LIES),
    ],
};

/// Steps one rollout takes at most. A subgame can run for hundreds of decisions; this keeps
/// both sides to the same opening turns.
pub const STEPS: u64 = 150;

/// Decisions one game is played for while cutting, at most.
const CUT_DECISIONS: u32 = 6000;

/// The positions of both sides and the table they were cut from.
pub struct SubgameWorkload {
    pub table: GameTable,
    pub policy: RolloutPolicy,
    pub subgame: Vec<State<Large>>,
    pub plain: Vec<State<Large>>,
}

/// One game's positions: plays the rollout policy in both seats and keeps the first decision
/// of every [`EVERY`]th turn inside a subgame (`sub`) and, before any subgame has begun,
/// outside one (`plain`), [`PER_GAME`] of each at most.
fn cut(
    t: &GameTable,
    policy: &RolloutPolicy,
    seed: u64,
    sub: &mut Vec<State<Large>>,
    plain: &mut Vec<State<Large>>,
) {
    let mut s = State::new(t, seed, Profile::SMART)
        .expect("decks that fit Small")
        .widen();
    let mut x = Scratch::<Large>::unchecked();
    let mut a = Inline { policy, t };
    let (mut kept_sub, mut kept_plain, mut next_sub, mut next_plain) = (0, 0, 1, 1);
    let mut entered = false;
    for _ in 0..CUT_DECISIONS {
        let Step::Decide(d) = next(&mut s, t, &mut x, &mut a) else {
            return;
        };
        entered |= s.in_subgame();
        let turn = s.turn().number();
        if s.in_subgame() {
            if kept_sub < PER_GAME && turn >= next_sub {
                sub.push(s.clone());
                kept_sub += 1;
                next_sub = turn + EVERY;
            }
        } else if !entered && kept_plain < PER_GAME && turn >= next_plain {
            plain.push(s.clone());
            kept_plain += 1;
            next_plain = turn + EVERY;
        }
        let c = policy.choose(t, Observation::new(&s, d.decider), &d);
        apply(&mut s, t, &mut x, &mut a, c);
    }
}

impl SubgameWorkload {
    /// Plays seeded games with [`DECK`] until [`POSITIONS`] subgame positions are cut.
    /// Deterministic: `subgame_corpus_never_changes` pins it.
    pub fn new() -> SubgameWorkload {
        let deck = DECK.deck();
        let table = GameTable::new(Format::CONSTRUCTED, &CARDS, [&deck, &deck])
            .expect("the deck makes a table");
        let policy = RolloutPolicy::new(&table);
        let (mut subgame, mut plain) = (Vec::new(), Vec::new());
        for game in 0..MAX_GAMES * 4 {
            if subgame.len() >= POSITIONS {
                break;
            }
            cut(
                &table,
                &policy,
                SEED ^ (game << 1),
                &mut subgame,
                &mut plain,
            );
        }
        subgame.truncate(POSITIONS);
        plain.truncate(POSITIONS);
        SubgameWorkload {
            table,
            policy,
            subgame,
            plain,
        }
    }

    /// Each position reseeded [`ROLLOUTS`] times, as the families' starts are.
    fn starts(set: &[State<Large>]) -> Vec<State<Large>> {
        let mut starts = Vec::new();
        for (i, position) in set.iter().enumerate() {
            for r in 0..ROLLOUTS {
                let mut s = position.clone();
                s.reseed(SEED ^ ((i as u64) << 8 | r));
                s.set_profile(Profile {
                    stops: Stops::Smart,
                    breakpoint: None,
                    full_control: None,
                    asks_targets: None,
                    taps_mana: None,
                    ..s.profile()
                });
                starts.push(s);
            }
        }
        starts
    }
}

impl Default for SubgameWorkload {
    fn default() -> Self {
        Self::new()
    }
}

/// One transition of a rollout inside a subgame, counted by the subgame benchmark.
#[inline(never)]
pub fn subgame_step(
    s: &mut State<Large>,
    t: &GameTable,
    x: &mut Scratch<Large>,
    a: &mut Inline<'_>,
    d: &Decision<Large>,
    c: Choice,
) -> Step<Large> {
    black_box(0x5355_4247);
    beryl_engine::apply_decided(s, t, x, a, d, c);
    next(s, t, x, a)
}

/// [`subgame_step`] on a plain game: the same body, counted by the plain benchmark.
#[inline(never)]
pub fn plain_step(
    s: &mut State<Large>,
    t: &GameTable,
    x: &mut Scratch<Large>,
    a: &mut Inline<'_>,
    d: &Decision<Large>,
    c: Choice,
) -> Step<Large> {
    black_box(0x504C_4149);
    beryl_engine::apply_decided(s, t, x, a, d, c);
    next(s, t, x, a)
}

/// Which side a rollout counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Subgame,
    Plain,
}

/// Rolls each start of `side` out for at most [`STEPS`] steps, stopping a subgame rollout when
/// its subgame ends and a plain one when it enters a subgame. Returns the steps taken.
pub fn roll(w: &SubgameWorkload, side: Side) -> u64 {
    let t = &w.table;
    let mut a = Inline {
        policy: &w.policy,
        t,
    };
    let mut x = Scratch::<Large>::unchecked();
    let set = match side {
        Side::Subgame => &w.subgame,
        Side::Plain => &w.plain,
    };
    let mut steps = 0;
    for mut s in SubgameWorkload::starts(set) {
        let mut step = next(&mut s, t, &mut x, &mut a);
        let mut taken = 0;
        while let Step::Decide(d) = step {
            if taken == STEPS || s.in_subgame() != (side == Side::Subgame) {
                break;
            }
            let c = w.policy.choose(t, Observation::new(&s, d.decider), &d);
            step = match side {
                Side::Subgame => subgame_step(&mut s, t, &mut x, &mut a, &d, c),
                Side::Plain => plain_step(&mut s, t, &mut x, &mut a, &d, c),
            };
            taken += 1;
        }
        steps += taken;
    }
    steps
}

#[cfg(test)]
mod tests;
