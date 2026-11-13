//! Keep the unseen player's choices and responses, including their place in the stack.
//! The viewer controls their own decisions, so a response waits for the recorded stack
//! context rather than a count of human actions (which may take a different legal path).

use beryl_engine::{Choice, Decision, DecisionKind, GameTable, Layout, Observation, Seat};
use beryl_scenario::{Action, Scenario};
use beryl_search::{Agent, AgentDecision};

use super::{Variant, actor, auto};
use crate::boards::{self, Scripted, Shared};

pub type Context = Vec<(&'static str, bool, Seat)>;

/// Preserve targets that were automatic in the original scenario. Adding a second legal
/// target to Playground must not make the opponent pick a different one (by-yfw's token).
/// Keep original action ordinals for stable example links.
pub fn plan(scenario: &Scenario) -> (Vec<Action>, Vec<Option<Context>>, Vec<usize>) {
    let trace = beryl_scenario::trace(&beryl_cards::CARDS, scenario).ok();
    let mut actions = Vec::new();
    let mut contexts = Vec::new();
    let mut ordinals = Vec::new();
    let mut ordinal = 0;
    for (i, action) in scenario.actions.iter().enumerate() {
        if let Some(positions) = &trace {
            for forced in &positions[i].forced {
                if matches!(
                    forced.answer,
                    Action::Answer {
                        kind: DecisionKind::Target,
                        ..
                    }
                ) {
                    actions.push(forced.answer);
                    contexts.push(None);
                    ordinals.push(ordinal);
                }
            }
        }
        if auto::playground_action(action) {
            actions.push(*action);
            contexts.push(trace.as_ref().map(|positions| {
                positions[i]
                    .stack
                    .iter()
                    .map(|s| (s.name, s.spell, s.controller))
                    .collect()
            }));
            ordinals.push(ordinal);
            ordinal += 1;
        }
    }
    (actions, contexts, ordinals)
}

pub struct Opponent {
    script: Scripted,
    progress: Shared,
    contexts: Vec<Option<Context>>,
    table: GameTable,
    actions: Vec<Action>,
    seat: Seat,
    history: Vec<usize>,
}

impl Opponent {
    pub fn new(variant: &Variant, seat: Seat, table: &GameTable) -> Result<Self, String> {
        let actions: Vec<_> = variant
            .moment
            .play
            .iter()
            .filter(|a| actor(a) == Some(seat) || matches!(a, Action::AdvanceTo { .. }))
            .copied()
            .collect();
        let contexts = variant
            .moment
            .play
            .iter()
            .enumerate()
            .filter(|(_, a)| actor(a) == Some(seat))
            .map(|(i, action)| {
                if matches!(action, Action::Answer { .. }) {
                    None
                } else {
                    variant.contexts.get(i).cloned().flatten()
                }
            })
            .collect();
        let progress = Shared::default();
        let script =
            Scripted::with_actions(&actions, seat, table, std::sync::Arc::clone(&progress))?;
        Ok(Self {
            script,
            progress,
            contexts,
            table: table.clone(),
            actions,
            seat,
            history: Vec::new(),
        })
    }
}

impl<L: Layout> Agent<L> for Opponent {
    /// Asked one of its decisions again, as a game taken back by an undo asks it, it first
    /// goes back to stand before that decision.
    fn choose(
        &mut self,
        obs: Observation<'_, L>,
        decision: &Decision<L>,
        at: AgentDecision,
    ) -> Choice {
        let index = usize::try_from(at.0).expect("decision index fits usize");
        if index < self.history.len() {
            self.go_back(index);
        }
        let state = obs.unmasked_root();
        let taken = boards::lock(&self.progress).taken;
        self.history.push(taken);
        if decision.kind == DecisionKind::Priority
            && let Some(Some(expected)) = self.contexts.get(taken)
            && !state
                .stack()
                .iter()
                .map(|s| {
                    (
                        self.table.def(s.card).face_name(s.face),
                        s.is_spell(),
                        s.controller,
                    )
                })
                .eq(expected.iter().copied())
            && let Some(pass) =
                beryl_scenario::answer(&self.table, state, decision, &Action::pass(decision.player))
        {
            return pass;
        }
        self.script.choose(obs, decision, at)
    }
}

impl Opponent {
    /// Forgets its decisions from its own decision `index` on, and the script's progress
    /// through them.
    fn go_back(&mut self, index: usize) {
        let taken = self.history[index];
        self.history.truncate(index);
        self.progress = Shared::default();
        self.script = Scripted::with_actions(
            &self.actions,
            self.seat,
            &self.table,
            std::sync::Arc::clone(&self.progress),
        )
        .expect("the original script was valid");
        self.script.skip_taken(taken);
    }
}
