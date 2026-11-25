//! A tree's iteration that ran out of `Small`'s room, played again in `Large`
//! ([`crate::wide`]).

use crate::wide::{charge, ran_out, widened};

use super::{
    Choice, GameTable, Large, Layout, Replay, RolloutPolicy, Scoring, Scratch, SplitMix64, State,
    Step, Tree, apply, next, smart, value,
};

/// What an iteration of a tree needs to be played again: the tree's random stream before it
/// dealt the iteration's world, for a tree that deals one each iteration; the choices it made
/// from the root; and the stream before the leaf was scored, once it was.
pub(super) struct Again {
    pub(super) world: Option<SplitMix64>,
    pub(super) choices: [Choice; 256],
    pub(super) leaf: Option<SplitMix64>,
}

impl Again {
    pub(super) fn new(world: Option<SplitMix64>) -> Self {
        Self {
            world,
            choices: [Choice(0); 256],
            leaf: None,
        }
    }
}

impl<L: Layout> Tree<L> {
    /// `value`, the score of an iteration that left its game at `s` after `depth` choices,
    /// unless the game ran out of `Small`'s room: then the score of the iteration played again
    /// in `Large` ([`Tree::widened_value`]).
    #[inline]
    pub(super) fn unless_wide(
        &mut self,
        t: &GameTable,
        policy: &RolloutPolicy,
        s: &State<L>,
        again: &Again,
        depth: usize,
        value: f64,
    ) -> f64 {
        if ran_out(s) {
            self.widened_value(t, policy, again, depth).unwrap_or(value)
        } else {
            value
        }
    }

    /// The score of an iteration played again in `Large`, if the tree's states are `Small`:
    /// from the same world, dealt again or the tree's fixed one, through the same first
    /// `depth` choices, then scored from the same stream as the leaf was, or from a fresh one
    /// where a step of the tree itself ran out. Widening keeps every random stream's place, so
    /// the game reaches the point where `Small` ran out and goes on past it.
    #[cold]
    #[inline(never)]
    fn widened_value(
        &mut self,
        t: &GameTable,
        policy: &RolloutPolicy,
        again: &Again,
        depth: usize,
    ) -> Option<f64> {
        let (world, choices, leaf) = (again.world, &again.choices[..depth], again.leaf);
        let mut s = match world {
            Some(mut rng) => widened(&self.deal(t, &mut rng))?,
            None => widened(&self.state)?,
        };
        let mut x = Scratch::<Large>::unchecked();
        x.counts.copied();
        for (depth, &c) in choices.iter().enumerate() {
            // The tree's step to its next decision, which reached it in `Small` and so
            // reaches it here.
            if depth > 0
                && let Step::Over(outcome) = next(&mut s, t, &mut x, &mut Replay)
            {
                charge(&mut self.x, &x);
                return Some(value(outcome, self.turns_wide(s.elapsed_turns())));
            }
            apply(&mut s, t, &mut x, &mut Replay, c);
            if depth == 0 {
                smart(&mut s);
            }
        }
        let mut rng = leaf.unwrap_or_else(|| SplitMix64::new(self.rng.next_u64()));
        s.reseed(rng.next_u64());
        let scoring = Scoring {
            leaf: self.family.leaf,
            origin: self.origin,
            extra: &mut self.extra,
        };
        let score = scoring.score(&mut s, t, &mut x, policy, &mut rng);
        charge(&mut self.x, &x);
        Some(score)
    }

    /// The turns from the tree's first root to a game `elapsed` turns in.
    fn turns_wide(&self, elapsed: u16) -> u16 {
        elapsed.saturating_sub(self.origin)
    }
}
