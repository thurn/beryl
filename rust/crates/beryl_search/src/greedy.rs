//! The rollout policy and optional combat planner as a player.

use beryl_engine::{Choice, Decision, GameTable, Large, Layout, Observation, Small};

use crate::{
    Agent, AgentDecision, AgentInfo, Budget, GameAgent, Knowledge, RolloutPolicy, Rules, Widen,
    Widening, combat,
};

/// The deterministic rollout policy as an opponent.
#[derive(Clone, Debug)]
pub struct GreedyAgent {
    table: GameTable,
    policy: RolloutPolicy,
}

impl GreedyAgent {
    pub fn new(t: &GameTable) -> Self {
        Self::with_rules(t, Rules::default())
    }

    fn with_rules(t: &GameTable, rules: Rules) -> Self {
        Self {
            table: t.clone(),
            policy: RolloutPolicy::with_rules(t, Knowledge::Tuned, rules),
        }
    }
}

impl<L: Layout> Agent<L> for GreedyAgent {
    fn choose(&mut self, obs: Observation<'_, L>, d: &Decision<L>, _at: AgentDecision) -> Choice {
        self.policy.choose(&self.table, obs, d)
    }
}

struct WithCombat<L: Layout> {
    greedy: GreedyAgent,
    seed: u64,
    next: AgentDecision,
    plan: combat::Plan<L>,
}

impl<L: Layout> Agent<L> for WithCombat<L> {
    fn choose(&mut self, obs: Observation<'_, L>, d: &Decision<L>, at: AgentDecision) -> Choice {
        if at != self.next {
            // Undo or a jump in replay invalidates the pending declarations.
            self.plan = combat::Plan::default();
        }
        self.next = at.next();
        if combat::handles(d.kind) {
            self.plan.choose(
                &self.greedy.table,
                &self.greedy.policy,
                obs,
                d,
                self.seed,
                at,
            )
        } else {
            self.greedy.choose(obs, d, at)
        }
    }

    fn resume_point(&self) -> Option<AgentDecision> {
        Some(AgentDecision(0))
    }
}

impl Widen for WithCombat<Small> {
    type Large = WithCombat<Large>;

    fn widen(&self) -> Self::Large {
        WithCombat {
            greedy: self.greedy.clone(),
            seed: self.seed,
            next: self.next,
            plan: self.plan.widen(),
        }
    }
}

// Every subset uses the same order in the name. These are measurement tools outside
// the app's list of opponents.
pub(super) static VARIANTS: &[AgentInfo] = &[
    variant::<true, false, false, false>("greedy+hold"),
    variant::<false, true, false, false>("greedy+waste"),
    variant::<false, false, true, false>("greedy+combat"),
    variant::<true, true, false, false>("greedy+hold+waste"),
    variant::<true, false, true, false>("greedy+hold+combat"),
    variant::<false, true, true, false>("greedy+waste+combat"),
    variant::<true, true, true, false>("greedy+hold+waste+combat"),
    variant::<false, false, true, true>("greedy+combat+walker"),
    variant::<true, false, true, true>("greedy+hold+combat+walker"),
    variant::<false, true, true, true>("greedy+waste+combat+walker"),
    variant::<true, true, true, true>("greedy+hold+waste+combat+walker"),
];

const fn variant<const HOLD: bool, const WASTE: bool, const COMBAT: bool, const WALKER: bool>(
    name: &'static str,
) -> AgentInfo {
    AgentInfo {
        name,
        title: "Greedy",
        description: "Plays the rollout policy with the rules named",
        new: build::<HOLD, WASTE, COMBAT, WALKER>,
    }
}

fn build<const HOLD: bool, const WASTE: bool, const COMBAT: bool, const WALKER: bool>(
    t: &GameTable,
    seed: u64,
    _budget: Budget,
) -> Box<dyn GameAgent + Send> {
    let greedy = GreedyAgent::with_rules(
        t,
        Rules {
            hold: HOLD,
            waste: WASTE,
            walker: WALKER,
        },
    );
    if COMBAT {
        Box::new(Widening::new(WithCombat {
            greedy,
            seed,
            next: AgentDecision(0),
            plan: combat::Plan::default(),
        }))
    } else {
        Box::new(greedy)
    }
}
