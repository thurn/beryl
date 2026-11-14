//! The rollout loop and the answer sources it plays with.

use beryl_engine::{
    Answers, Choice, Decision, GameTable, Layout, Observation, Outcome, Replay, Scratch, State,
    Step, TurnStep, apply, apply_decided, next, next_within,
};

use crate::mcts::smart;

use super::{RolloutPolicy, Softmax};

/// The rollout policy as an inline answer source (§7.3): a resolving body asks it and runs on,
/// so a rollout never pauses.
pub struct Inline<'a> {
    pub policy: &'a RolloutPolicy,
    pub t: &'a GameTable,
}

impl<L: Layout> Answers<L> for Inline<'_> {
    fn inline(&self) -> bool {
        true
    }

    fn answer(&mut self, obs: Observation<'_, L>, d: &Decision<L>) -> Option<Choice> {
        Some(self.policy.choose(self.t, obs, d))
    }
}

/// Plays a game to its end under the rollout policy in both seats, every question answered
/// inline: the rollout loop search runs (§19.1). It copies nothing and allocates nothing.
///
/// Never inlined, so a caller's frame stays apart from the loop's: inlined into
/// `beryl_bench::rollout::counted_rollout`, it left Callgrind unable to follow that frame,
/// and the rollout benchmark counted half the work.
#[inline(never)]
pub fn rollout<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    policy: &RolloutPolicy,
) -> Outcome {
    rollout_to(s, t, x, policy, u16::MAX).expect("a game ends before turn 65535")
}

/// [`rollout`], cut off at the first decision of turn `turn` or later: the game's outcome if
/// it ends before then, else none, the state left at that decision.
pub fn rollout_to<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    policy: &RolloutPolicy,
    turn: u16,
) -> Option<Outcome> {
    let mut inline = Inline { policy, t };
    play_out(s, t, x, &mut inline, turn, |_, obs, d| {
        policy.choose(t, obs, d)
    })
}

/// [`rollout_to`] with every decision, inline ones too, drawn by `soft`
/// ([`RolloutPolicy::choose_soft`]).
pub fn soft_rollout_to<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    policy: &RolloutPolicy,
    soft: &mut Softmax<'_>,
    turn: u16,
) -> Option<Outcome> {
    let mut inline = SoftInline {
        policy,
        t,
        soft,
        record: |_: Observation<'_, L>, _: &Decision<L>, _: Choice| {},
    };
    play_out(s, t, x, &mut inline, turn, SoftInline::choose)
}

/// [`soft_rollout_to`] to the end of the game.
pub fn soft_rollout<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    policy: &RolloutPolicy,
    soft: &mut Softmax<'_>,
) -> Outcome {
    soft_rollout_to(s, t, x, policy, soft, u16::MAX).expect("a game ends before turn 65535")
}

pub(crate) enum RolloutEnd {
    Outcome(Outcome),
    Cutoff,
}

/// A softmax rollout with a hard allowance for decisions and copies. Returns
/// `None` if the allowance ran out.
pub(crate) fn soft_rollout_within<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    policy: &RolloutPolicy,
    soft: &mut Softmax<'_>,
    turn: u16,
    work_limit: u64,
) -> Option<RolloutEnd> {
    let mut inline = SoftInline {
        policy,
        t,
        soft,
        record: |_: Observation<'_, L>, _: &Decision<L>, _: Choice| {},
    };
    loop {
        match next_within(
            s,
            t,
            x,
            &mut inline,
            (u16::MAX, TurnStep::Cleanup),
            work_limit,
        )? {
            Step::Over(outcome) => return Some(RolloutEnd::Outcome(outcome)),
            Step::Decide(_) if s.turn().number() >= turn => return Some(RolloutEnd::Cutoff),
            Step::Decide(d) => {
                let c = inline.choose(Observation::new(s, d.player), &d);
                // An inline answer does not need a transactional copy.
                apply_decided(s, t, x, &mut inline, &d, c);
            }
        }
    }
}

/// Plays the forced root choice of a search, then rolls out the game. Calls `record` for
/// every policy answer, including those asked inline during resolution.
pub fn soft_rollout_after<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    policy: &RolloutPolicy,
    soft: &mut Softmax<'_>,
    choice: Choice,
    record: impl FnMut(Observation<'_, L>, &Decision<L>, Choice),
) -> Outcome {
    apply(s, t, x, &mut Replay, choice);
    smart(s);
    s.reseed(soft.rng.next_u64());
    let mut inline = SoftInline {
        policy,
        t,
        soft,
        record,
    };
    play_out(s, t, x, &mut inline, u16::MAX, SoftInline::choose)
        .expect("a game ends before turn 65535")
}

/// The rollout loop: `answers` answers inline, `choose` every decision `next` returns.
fn play_out<A: Answers<L>, L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    answers: &mut A,
    turn: u16,
    mut choose: impl FnMut(&mut A, Observation<'_, L>, &Decision<L>) -> Choice,
) -> Option<Outcome> {
    loop {
        match next(s, t, x, answers) {
            Step::Over(outcome) => return Some(outcome),
            Step::Decide(_) if s.turn().number() >= turn => return None,
            Step::Decide(d) => {
                let c = choose(answers, Observation::new(s, d.player), &d);
                apply_decided(s, t, x, answers, &d, c);
            }
        }
    }
}

/// [`Inline`] for a softmax rollout.
struct SoftInline<'a, 'r, R> {
    policy: &'a RolloutPolicy,
    t: &'a GameTable,
    soft: &'a mut Softmax<'r>,
    record: R,
}

impl<R> SoftInline<'_, '_, R> {
    fn choose<L: Layout>(&mut self, obs: Observation<'_, L>, d: &Decision<L>) -> Choice
    where
        R: FnMut(Observation<'_, L>, &Decision<L>, Choice),
    {
        let choice = self.policy.choose_soft(self.t, obs, d, self.soft);
        (self.record)(obs, d, choice);
        choice
    }
}

impl<L: Layout, R: FnMut(Observation<'_, L>, &Decision<L>, Choice)> Answers<L>
    for SoftInline<'_, '_, R>
{
    fn inline(&self) -> bool {
        true
    }

    fn answer(&mut self, obs: Observation<'_, L>, d: &Decision<L>) -> Option<Choice> {
        Some(self.choose(obs, d))
    }
}
