//! A scenario played live, for a generator that chooses each action from the position.

use beryl_engine::{CardTable, Choice, Decision, GameTable, Slot, State, Step};

use super::{Runner, upcoming};
use crate::setup::Slots;
use crate::{Action, Assertion, Failure, Scenario};

/// A scenario played one action at a time, for a generator that chooses each action from the
/// position it begins at (§17.2). Assertions go last: an action's position depends on the
/// next action that is not one.
#[derive(Clone)]
pub struct Live {
    runner: Runner,
    actions: Vec<Action>,
}

impl Live {
    /// The setup of `scenario`, before any action; its own actions are not run.
    pub fn new(cards: &'static CardTable, scenario: &Scenario) -> Result<Live, Failure> {
        Ok(Live {
            runner: Runner::new(cards, scenario, Slots::Dealt)?,
            actions: Vec::new(),
        })
    }

    /// The position an action of the same kind as `next` would begin from: an advance or
    /// a resolution stops the game where nothing else would.
    pub fn position(&mut self, next: Action) -> Result<Step, String> {
        if self.runner.position.is_none() {
            self.runner.upcoming = upcoming(&[next]);
        }
        self.runner.position()
    }

    /// Runs `action` from the current position, keeping it in the script.
    pub fn act(&mut self, action: Action) -> Result<(), String> {
        self.runner.upcoming = upcoming(&[action]);
        self.runner.position()?;
        self.runner.act(action)?;
        self.actions.push(action);
        Ok(())
    }

    /// The candidates of `d`, the open decision, that the scenario vocabulary can take, each
    /// with its action.
    pub fn picks(&self, d: &Decision) -> Vec<(Choice, Action)> {
        self.runner.picks(d)
    }

    /// The name of the object candidate `c` of `d` is or names, if it names one: what a
    /// pick of a card would find it by.
    pub fn name_of(&self, d: &Decision, c: Choice) -> Option<&'static str> {
        let r = &self.runner;
        d.candidates
            .object(c)
            .or_else(|| d.candidates.option(c).and_then(|o| r.named(o)))
            .map(|slot| r.name(slot))
    }

    /// The name a scenario knows the object in `slot` by: its face up's, or its card's for a
    /// face-down permanent.
    pub fn name(&self, slot: Slot) -> &'static str {
        self.runner.name(slot)
    }

    /// The actions run so far.
    pub fn actions(&self) -> &[Action] {
        &self.actions
    }

    pub fn state(&self) -> &State {
        &self.runner.s
    }

    pub fn table(&self) -> &GameTable {
        &self.runner.t
    }

    /// Facts about the whole public position here, as assertions, for oracles to compare;
    /// nothing once the game is over.
    pub fn snapshot(&mut self) -> Result<Vec<Assertion>, String> {
        self.runner.upcoming = None;
        self.runner.snapshot()
    }
}
