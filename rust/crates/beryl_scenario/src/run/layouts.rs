//! Compare the engine's two state layouts as they play the same game.

use beryl_engine::{
    CardTable, Choice, Decision, GameTable, Large, Layout, Profile, Relayout, Replay, Scratch,
    State, Step, apply, next,
};

use super::{Runner, upcoming};
use crate::setup::{Slots, with_layout};
use crate::{Failure, Scenario};

/// Runs every action of a scenario, comparing the widened state and the decision with
/// those reached by playing the same choices in Large. A deal too big for Small plays
/// in Large alone, where the script and its assertions still have to pass.
pub fn plays_alike_in_both_layouts(
    cards: &'static CardTable,
    scenario: &Scenario,
) -> Result<(), Failure> {
    with_layout(
        cards,
        scenario,
        Slots::Dealt,
        |t, s| {
            let comparison = Box::new(LayoutComparison::new(&s));
            let mut runner = Runner::from_position(t, s);
            runner.comparison = Some(comparison);
            play_actions(runner, scenario)
        },
        |t, s| play_actions(Runner::from_position(t, s), scenario),
    )
}

fn play_actions<L: Layout>(mut runner: Runner<L>, scenario: &Scenario) -> Result<(), Failure> {
    let fail = |action, message| Failure {
        scenario: scenario.name,
        action,
        message,
    };
    for (i, &action) in scenario.actions.iter().enumerate() {
        runner.upcoming = upcoming(&scenario.actions[i..]);
        runner
            .act(action)
            .map_err(|message| fail(Some(i), message))?;
    }
    if let Some(comparison) = &runner.comparison {
        comparison
            .check_state(&runner.s)
            .map_err(|message| fail(scenario.actions.len().checked_sub(1), message))?;
    }
    Ok(())
}

/// A shadow game in Large, for tests that drive a Small game and compare the layouts at
/// each decision. The caller advances Small first and gives both games the same choices.
pub struct LayoutComparison {
    large: State<Large>,
    scratch: Scratch<Large>,
    decisions: usize,
}

impl LayoutComparison {
    pub fn new(small: &State) -> Self {
        Self {
            large: small.widen(),
            scratch: Scratch::new(),
            decisions: 0,
        }
    }

    pub(super) fn set_profile(&mut self, profile: Profile) {
        self.large.set_profile(profile);
    }

    pub fn apply(&mut self, table: &GameTable, choice: Choice) {
        apply(
            &mut self.large,
            table,
            &mut self.scratch,
            &mut Replay,
            choice,
        );
    }

    /// Advances Large and compares the resulting steps and states. A mismatch names the
    /// decision, its position in the game, and the first differing field.
    pub fn check_next<L: Layout>(
        &mut self,
        table: &GameTable,
        small: &State<L>,
        step: &Step<L>,
    ) -> Result<(), String> {
        let large = next(&mut self.large, table, &mut self.scratch, &mut Replay);
        let widened = step.relayout();
        if widened != large {
            let difference = match (&widened, &large) {
                (Step::Decide(left), Step::Decide(right)) => {
                    format!("decision.{}", decision_difference(left, right))
                }
                (Step::Over(_), Step::Over(_)) => "outcome".to_string(),
                _ => "step".to_string(),
            };
            return Err(format!(
                "{} differs in {difference}: Small reached {widened:?}, Large reached {large:?}",
                self.position(small),
            ));
        }
        self.check_state(small)?;
        self.decisions += usize::from(matches!(step, Step::Decide(_)));
        Ok(())
    }

    fn check_state<L: Layout>(&self, small: &State<L>) -> Result<(), String> {
        if let Some(field) = small.relayout().first_difference(&self.large) {
            return Err(format!(
                "{} differs in state.{field} between Small and Large",
                self.position(small),
            ));
        }
        Ok(())
    }

    fn position<L: Layout>(&self, small: &State<L>) -> String {
        let turn = small.turn();
        format!(
            "decision {} in turn {} {:?}",
            self.decisions + 1,
            turn.number(),
            turn.step(),
        )
    }
}

fn decision_difference(left: &Decision<Large>, right: &Decision<Large>) -> &'static str {
    let Decision {
        player,
        decider,
        kind,
        candidates,
        prompt,
        about,
    } = left;
    [
        (player != &right.player, "player"),
        (decider != &right.decider, "decider"),
        (kind != &right.kind, "kind"),
        (candidates != &right.candidates, "candidates"),
        (prompt != &right.prompt, "prompt"),
        (about != &right.about, "about"),
    ]
    .into_iter()
    .find_map(|(differs, field)| differs.then_some(field))
    .expect("the decisions differ")
}

#[cfg(test)]
mod tests {
    use beryl_engine::{CardTable, GameTable, Profile, Replay, Scratch, Seat, State, next};

    use super::LayoutComparison;

    #[test]
    fn a_layout_mismatch_names_the_first_decision_and_differing_state_field() {
        static CARDS: CardTable = CardTable::new(&[]);
        let table = GameTable::new(&CARDS, [&[], &[]]).unwrap();
        let mut small = State::new(&table, 0, Profile::EXACT).unwrap();
        let mut comparison = LayoutComparison::new(&small);
        small.board().set_life(Seat::P0, 12);
        let step = next(&mut small, &table, &mut Scratch::new(), &mut Replay);
        let failure = comparison.check_next(&table, &small, &step).unwrap_err();
        assert!(failure.starts_with("decision 1 in turn"));
        assert!(failure.contains("state.players"));
    }
}
