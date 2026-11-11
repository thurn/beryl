//! Bounded exhaustive search from a scenario's end position (architecture §17.4): every
//! sequence of decisions to a fixed depth, with the state's checks at every position. Fuzzing
//! samples whole games; this enumerates the games near positions a card's author chose
//! because they matter.
//!
//! The state is `Copy`, so each branch plays on its own copy and nothing is undone. Chance is
//! not enumerated: a copy carries the game's seeded randomness, so every branch draws the
//! same. A position reached again with no more depth left to search than before is skipped,
//! by its state's hash, and a cap on the decisions taken from each scenario keeps one wide
//! position from taking the whole budget.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::fmt::{self, Write};
use std::hash::{Hash, Hasher};
use std::panic::{AssertUnwindSafe, catch_unwind};

use beryl_engine::{
    CardTable, Choice, Decision, DecisionKind, GameTable, Outcome, Replay, Scratch, StallReason,
    State, Step, apply, next,
};

use crate::run::TRACED;
use crate::{Action, Live, Scenario, answer, describe, script, setup_source, source};

/// What must hold at every position the search reaches.
pub type Check = fn(&GameTable, &State) -> Result<(), String>;

/// The engine's own checks: its invariants and its references (§17.4).
pub fn invariants(t: &GameTable, s: &State) -> Result<(), String> {
    s.check_invariants(t)
        .map_err(|v| format!("an engine invariant broke: {v:?}"))?;
    beryl_engine::check_references(s, t)
        .map_err(|v| format!("the engine disagrees with a reference: {v:?}"))
}

/// How far to search from each scenario.
#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    /// The most decisions in a sequence. A question with one answer is answered as the
    /// runner answers it, and is not one.
    pub depth: u8,
    /// The most decisions taken from one scenario's end position, over all its sequences.
    pub nodes: usize,
}

/// What a search from one scenario covered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Searched {
    /// The decisions taken.
    pub nodes: usize,
    /// Whether it stopped at [`Bounds::nodes`] with sequences left unplayed.
    pub capped: bool,
}

/// A sequence that breaks the check, from a scenario's end.
#[derive(Clone, Debug)]
pub struct Found {
    pub why: String,
    /// The scenario with its own actions, assertions aside, and then the sequence's, which
    /// replays to the failure; `None` where a decision on the sequence has no action.
    pub scenario: Option<Scenario>,
    /// How many of the actions are the sequence's.
    pub searched: usize,
    /// The scenario as a card file writes it, with why it fails in a comment above.
    pub text: String,
}

impl fmt::Display for Found {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// Searches every sequence of decisions within `bounds` from the end of `scenario`, its
/// assertions aside, checking `check` at every position and that nothing panics. A scenario
/// that fails to play to its end is a failure with no sequence.
pub fn exhaust(
    cards: &'static CardTable,
    scenario: &Scenario,
    bounds: Bounds,
    check: Check,
) -> Result<Searched, Box<Found>> {
    let failed = |why: String| Found {
        text: format!("// {}: {why}\n", scenario.name),
        why,
        scenario: None,
        searched: 0,
    };
    let mut live = Live::new(cards, scenario).map_err(|f| failed(f.message))?;
    for &action in scenario.actions {
        if !matches!(action, Action::Assert { .. }) {
            live.act(action).map_err(failed)?;
        }
    }
    // Where a scripted answer would begin: the game stops where the scenario's runner would
    // stop before one, which may be a priority its last action's breakpoint stopped at, and
    // from there every decision of the profile stops.
    let Step::Decide(first) = live
        .position(Action::pass(scenario.setup.active))
        .map_err(failed)?
    else {
        return Ok(Searched::default());
    };
    let mut root = *live.state();
    root.set_profile(TRACED);
    let t = live.table();
    let mut search = Search {
        t,
        check,
        // The search checks `check` itself at every position, and reports a panic.
        x: Scratch::unchecked(),
        bounds,
        seen: HashMap::new(),
        path: Vec::new(),
        searched: Searched::default(),
    };
    let result = catch_unwind(AssertUnwindSafe(|| {
        check(t, &root)?;
        search.at(&root, &first, bounds.depth)
    }))
    .unwrap_or_else(|panic| Err(panic_message(&*panic)));
    match result {
        Ok(()) => Ok(search.searched),
        Err(why) => Err(Box::new(found(
            t,
            scenario,
            root,
            &first,
            &search.path,
            why,
        ))),
    }
}

struct Search<'a> {
    t: &'a GameTable,
    check: Check,
    x: Scratch,
    bounds: Bounds,
    /// The positions searched, by hash, with the depth left when they were.
    seen: HashMap<u64, u8>,
    /// The choices from the root to the position being searched; on a failure, to it.
    path: Vec<Choice>,
    searched: Searched,
}

impl Search<'_> {
    /// Searches from `s`, before `next`, with `depth` decisions left.
    fn from(&mut self, mut s: State, depth: u8) -> Result<(), String> {
        match settle(self.t, &mut s, &mut self.x, self.check)? {
            Some(d) => self.at(&s, &d, depth),
            None => Ok(()),
        }
    }

    /// Searches from `s`, where `d` is open, with `depth` decisions left.
    fn at(&mut self, s: &State, d: &Decision, depth: u8) -> Result<(), String> {
        if depth == 0 {
            return Ok(());
        }
        let key = {
            let mut h = DefaultHasher::new();
            s.hash(&mut h);
            h.finish()
        };
        if self.seen.get(&key).is_some_and(|&left| left >= depth) {
            return Ok(());
        }
        self.seen.insert(key, depth);
        for c in (0..d.len() as u16).map(Choice) {
            if self.searched.nodes == self.bounds.nodes {
                self.searched.capped = true;
                return Ok(());
            }
            self.searched.nodes += 1;
            self.path.push(c);
            let mut child = *s;
            apply(&mut child, self.t, &mut self.x, &mut Replay, c);
            self.from(child, depth - 1)?;
            self.path.pop();
        }
        Ok(())
    }
}

/// Advances `s` to its next decision as the scenario runner does, answering each question
/// with one answer and checking `check` at every position: the decision, or `None` once the
/// game is over.
fn settle(
    t: &GameTable,
    s: &mut State,
    x: &mut Scratch,
    check: Check,
) -> Result<Option<Decision>, String> {
    loop {
        let step = next(s, t, x, &mut Replay);
        check(t, s)?;
        match step {
            Step::Decide(d) if d.is_empty() => return Err(format!("no candidates for {d:?}")),
            Step::Decide(d) if d.len() == 1 && d.kind != DecisionKind::Priority => {
                apply(s, t, x, &mut Replay, Choice(0));
            }
            Step::Decide(d) => return Ok(Some(d)),
            Step::Over(Outcome::Stalled(StallReason::Capacity)) => {
                return Err("a capacity ran out".to_owned());
            }
            Step::Over(_) => return Ok(None),
        }
    }
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| panic.downcast_ref::<&str>().map(|&m| (*m).to_owned()))
        .map_or_else(|| "a panic".to_owned(), |m| format!("a panic: {m}"))
}

/// The failing sequence `path` from `root`, the scenario's end where `first` is open, after the
/// scenario's own actions: each decision on the path is replayed to name its choice, and the
/// last choice, the one that fails, is named but not taken.
fn found(
    t: &GameTable,
    scenario: &Scenario,
    mut s: State,
    first: &Decision,
    path: &[Choice],
    why: String,
) -> Found {
    // Replays to the failure, which debug checks would panic at before it is described.
    let mut x = Scratch::unchecked();
    let mut actions: Option<Vec<Action>> = Some(
        scenario
            .actions
            .iter()
            .copied()
            .filter(|a| !matches!(a, Action::Assert { .. }))
            .collect(),
    );
    let mut lines: Vec<String> = actions
        .iter()
        .flatten()
        .map(|a| format!("        {},", source(a)))
        .collect();
    lines.push("        // The search from here:".to_owned());
    for (i, &c) in path.iter().enumerate() {
        let d = if i == 0 {
            *first
        } else {
            let Ok(Some(d)) = settle(t, &mut s, &mut x, |_, _| Ok(())) else {
                unreachable!("the path replays to a decision");
            };
            d
        };
        match script(t, &s, &d, c) {
            Some(action) if answer(t, &s, &d, &action) == Some(c) => {
                lines.push(format!("        {},", source(&action)));
                if let Some(actions) = &mut actions {
                    actions.push(action);
                }
            }
            _ => {
                lines.push(format!(
                    "        // not scriptable: {}, choice {}",
                    describe(t, &s, &d),
                    c.0
                ));
                actions = None;
            }
        }
        if i + 1 < path.len() {
            apply(&mut s, t, &mut x, &mut Replay, c);
        }
    }
    let mut text = format!("// {}: {why}\n", scenario.name);
    text.push_str(&setup_source(scenario));
    let _ = writeln!(text, "    .actions(&[\n{}\n    ])", lines.join("\n"));
    Found {
        why,
        scenario: actions.map(|a| scenario.actions(a.leak())),
        searched: path.len(),
        text,
    }
}
