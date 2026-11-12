//! The strict runner: plays a scenario through `next` and `apply` only (architecture §17.1).

use beryl_engine::card::{FaceIx, Types};
use beryl_engine::event::Event;
use beryl_engine::state::{ObjectFlags, ObjectRef};
use beryl_engine::{
    Breakpoint, CardTable, Choice, Decision, DecisionKind, GameTable, Opt, Outcome, Profile,
    Replay, Scratch, Seat, Slot, State, Step, Stops, Zone, ZoneKey, apply, next,
};

mod answer;
mod live;
mod snapshot;

use crate::setup::{Slots, setup};
use crate::{
    Action, Assertion, CoinFlip, Ending, Failure, Forced, Pick, Position, Scenario, StackItem,
};
pub use answer::{answer, describe};
use answer::{option_index, pick_in, takes, taking};
pub use live::Live;

/// The exact profile, except that a question with one answer stops too: the runner answers it
/// as the engine would have silently, and the trace records the answer.
pub(crate) const TRACED: Profile = Profile {
    stops: Stops::Forced,
    ..Profile::EXACT
};

/// Runs a scenario over the cards of `cards`, under the exact profile. The questions with one
/// answer that profile answers silently stop here (`Stops::Forced`), and the runner answers
/// them the same way, so that [`trace`] can record them.
pub fn run(cards: &'static CardTable, scenario: &Scenario) -> Result<(), Failure> {
    trace(cards, scenario).map(|_| ())
}

/// Runs a scenario like [`run`], and returns the position each action began from: what the
/// oracle exporters need to place an action at its turn, step and stack (§17.2).
pub fn trace(cards: &'static CardTable, scenario: &Scenario) -> Result<Vec<Position>, Failure> {
    trace_with(cards, scenario, Slots::Dealt)
}

/// Runs a scenario like [`trace`], with each deck's slots numbered as `slots` says: its
/// assertions must pass and its positions match under every numbering.
pub fn trace_with(
    cards: &'static CardTable,
    scenario: &Scenario,
    slots: Slots,
) -> Result<Vec<Position>, Failure> {
    walk(cards, scenario, slots, &mut |_, _, _| {}, &mut |_, _| {})
}

/// Runs a scenario like [`run`], showing `visit` the table, the state and the open decision
/// each action other than an assertion answers or passes through: for tests of what a
/// decision says beyond the engine, such as the protocol's views of it.
pub fn visit(
    cards: &'static CardTable,
    scenario: &Scenario,
    mut visit: impl FnMut(&GameTable, &State, &Decision),
) -> Result<(), Failure> {
    walk(cards, scenario, Slots::Dealt, &mut visit, &mut |_, _| {}).map(|_| ())
}

/// Runs a scenario like [`run`], showing `watch` the state each action begins from and the
/// events since the action before, and once more what its last action left: for tests that
/// two definitions of a card play the same game.
pub fn watch(
    cards: &'static CardTable,
    scenario: &Scenario,
    watch: impl FnMut(&State, &[Event]),
) -> Result<(), Failure> {
    watch_with(cards, scenario, Slots::Dealt, watch)
}

/// Runs a scenario like [`watch`], with each deck's slots numbered as `slots` says: its events
/// must name the same cards in the same order under every numbering.
pub fn watch_with(
    cards: &'static CardTable,
    scenario: &Scenario,
    slots: Slots,
    mut watch: impl FnMut(&State, &[Event]),
) -> Result<(), Failure> {
    walk(cards, scenario, slots, &mut |_, _, _| {}, &mut watch).map(|_| ())
}

fn walk(
    cards: &'static CardTable,
    scenario: &Scenario,
    slots: Slots,
    visit: &mut dyn FnMut(&GameTable, &State, &Decision),
    watch: &mut dyn FnMut(&State, &[Event]),
) -> Result<Vec<Position>, Failure> {
    let fail = |action, message: String| Failure {
        scenario: scenario.name,
        action,
        message,
    };
    let mut runner = Runner::new(cards, scenario, slots)?;
    let mut positions = Vec::with_capacity(scenario.actions.len());
    for (i, &action) in scenario.actions.iter().enumerate() {
        runner.upcoming = upcoming(&scenario.actions[i..]);
        let step = runner.position().map_err(|m| fail(Some(i), m))?;
        if let Step::Decide(d) = &step
            && !matches!(action, Action::Assert { .. })
        {
            visit(&runner.t, &runner.s, d);
        }
        watch(&runner.s, runner.x.events.events());
        positions.push(runner.describe_position(&step));
        runner.act(action).map_err(|m| fail(Some(i), m))?;
    }
    watch(&runner.s, runner.x.events.events());
    Ok(positions)
}

/// The first of `actions` that is not an assertion: the one whose kind decides where the
/// game stops before it.
fn upcoming(actions: &[Action]) -> Option<Action> {
    actions
        .iter()
        .copied()
        .find(|a| !matches!(a, Action::Assert { .. }))
}

struct Runner {
    t: GameTable,
    s: State,
    x: Scratch,
    /// The next action that is not an assertion, if any, which decides where the game stops
    /// before it.
    upcoming: Option<Action>,
    /// What `next` returned for the current state, or `None` after an `apply`.
    position: Option<Step>,
    /// The forced answers since the last position, for [`trace`].
    forced: Vec<Forced>,
    /// The permanents on the battlefield at the last position, by name: a tapped one that
    /// has since left, as a Treasure sacrificed for mana, is named as it was.
    permanents: Vec<(ObjectRef, &'static str)>,
}

/// A copy at the same position, with fresh working memory: a [`Scratch`] is never copied
/// with the state it works on.
impl Clone for Runner {
    fn clone(&self) -> Runner {
        Runner {
            t: self.t.clone(),
            s: self.s,
            x: Scratch::recording(),
            upcoming: self.upcoming,
            position: self.position,
            forced: self.forced.clone(),
            permanents: self.permanents.clone(),
        }
    }
}

impl Runner {
    fn new(
        cards: &'static CardTable,
        scenario: &Scenario,
        slots: Slots,
    ) -> Result<Runner, Failure> {
        let (t, s) = setup(cards, scenario, slots)?;
        Ok(Runner {
            t,
            s,
            // Recording, for the coin flips each position reports.
            x: Scratch::recording(),
            upcoming: None,
            position: None,
            forced: Vec::new(),
            permanents: Vec::new(),
        })
    }

    /// The current position: the open decision or the end of the game. When the next action
    /// that answers decisions is an advance, its target is a breakpoint, so the game stops
    /// there even where the engine would pass silently; when it is a resolution, the current
    /// step is, so the game stops as soon as the active player has priority again. Every new
    /// position must keep the state's invariants (§17.4), which debug builds of `next` and
    /// `apply` check, so a setup or a rules path that breaks one panics here, and agree with
    /// the reference implementations, which fails here.
    fn position(&mut self) -> Result<Step, String> {
        if let Some(step) = self.position {
            return Ok(step);
        }
        let breakpoint = match self.upcoming {
            Some(Action::AdvanceTo { turn, step }) => Some(Breakpoint { turn, step }),
            // The active player's next priority in this step, after the resolution.
            Some(Action::Resolve) => {
                let now = self.s.turn();
                Some(Breakpoint {
                    turn: now.number(),
                    step: now.step(),
                })
            }
            _ => None,
        };
        self.s.set_profile(Profile {
            breakpoint,
            ..TRACED
        });
        loop {
            let step = next(&mut self.s, &self.t, &mut self.x, &mut Replay);
            beryl_engine::check_references(&self.s, &self.t)
                .map_err(|v| format!("the engine disagrees with a reference: {v:?}"))?;
            match step {
                // A question with one answer, which the exact profile answers silently.
                Step::Decide(d) if d.len() == 1 && d.kind != DecisionKind::Priority => {
                    // The only answer, where the scenario vocabulary can name it.
                    if let Some(pick) = self.pick_for(&d, Choice(0)) {
                        let turn = self.s.turn();
                        self.forced.push(Forced {
                            turn: turn.number(),
                            step: turn.step(),
                            purpose: d.prompt.purpose,
                            answer: Action::answer(d.player, d.kind, pick),
                        });
                    }
                    apply(&mut self.s, &self.t, &mut self.x, &mut Replay, Choice(0));
                }
                _ => {
                    self.position = Some(step);
                    return Ok(step);
                }
            }
        }
    }

    fn decision(&mut self) -> Result<Decision, String> {
        match self.position()? {
            Step::Decide(d) => Ok(d),
            Step::Over(o) => Err(format!("the game is over ({o:?})")),
        }
    }

    fn answer(&mut self, c: Choice) {
        apply(&mut self.s, &self.t, &mut self.x, &mut Replay, c);
        self.position = None;
    }

    fn act(&mut self, action: Action) -> Result<(), String> {
        match action {
            action @ (Action::PlayLand { player, .. }
            | Action::Cast { player, .. }
            | Action::Activate { player, .. }
            | Action::TurnFaceUp { player, .. }) => {
                let what = taking(&action);
                self.priority_option(player, &what, |r, o| takes(&r.t, &r.s, &action, o))?;
            }
            Action::Resolve => {
                // Waiting triggers go on the stack first.
                self.decision()?;
                let depth = self.s.stack().len();
                let Some(&top) = self.s.stack().last() else {
                    return Err("nothing on the stack to resolve".to_string());
                };
                loop {
                    // A resolution that ends the game is done.
                    let Step::Decide(d) = self.position()? else {
                        return Ok(());
                    };
                    let stack = self.s.stack();
                    if stack.len() < depth || stack[depth - 1] != top {
                        return Ok(());
                    }
                    if d.kind != DecisionKind::Priority {
                        return Ok(());
                    }
                    let pass = option_index(&d, |o| o == Opt::Pass).expect("passing is offered");
                    self.answer(pass);
                }
            }
            Action::Answer { player, kind, pick } => {
                let d = self.expect(player, kind)?;
                let c = self
                    .pick(&d, pick)
                    .ok_or_else(|| format!("{pick:?} is not offered: {}", self.describe(&d)))?;
                self.answer(c);
            }
            Action::AdvanceTo { turn, step } => loop {
                let d = self.decision()?;
                let now = self.s.turn();
                let here = (now.number(), now.step() as u8);
                let target = (turn, step as u8);
                if here > target {
                    return Err(format!(
                        "the game passed turn {turn} {step:?} without stopping there; now {}",
                        self.describe(&d)
                    ));
                }
                if d.kind != DecisionKind::Priority {
                    return Err(format!("unscripted decision: {}", self.describe(&d)));
                }
                if here == target && d.player == now.active() {
                    // Stopped: keep the decision, and stop no more.
                    self.s.set_profile(TRACED);
                    return Ok(());
                }
                let pass = option_index(&d, |o| o == Opt::Pass).expect("passing is offered");
                self.answer(pass);
            },
            Action::Assert { assertion } => {
                let position = self.position()?;
                self.check(&position, assertion)?;
            }
        }
        Ok(())
    }

    /// Answers `player`'s priority decision with the first option `wanted` accepts, or fails
    /// saying that `what` is not offered.
    fn priority_option(
        &mut self,
        player: Seat,
        what: &str,
        wanted: impl Fn(&Self, Opt) -> bool,
    ) -> Result<(), String> {
        let d = self.expect(player, DecisionKind::Priority)?;
        let c = option_index(&d, |o| wanted(self, o))
            .ok_or_else(|| format!("{what} is not offered: {}", self.describe(&d)))?;
        self.answer(c);
        Ok(())
    }

    /// The open decision, which must be `player`'s and of `kind`.
    fn expect(&mut self, player: Seat, kind: DecisionKind) -> Result<Decision, String> {
        let d = self.decision()?;
        if d.player == player && d.kind == kind {
            Ok(d)
        } else {
            Err(format!(
                "unscripted decision: {}, where the scenario has player {player} answer {kind:?}",
                self.describe(&d)
            ))
        }
    }

    /// The position an action begins from, for [`trace`]. It takes the coin flips and taps
    /// since the last position from the recorded events.
    fn describe_position(&mut self, step: &Step) -> Position {
        let mut flips = Vec::new();
        let mut taps = Vec::new();
        for e in self.x.events.drain() {
            match e {
                Event::CoinFlip { player, won, .. } => flips.push(CoinFlip { player, won }),
                Event::Tap { object } => taps.push(object),
                _ => {}
            }
        }
        let taps = taps
            .into_iter()
            .map(|object| {
                let was = self.permanents.iter().find(|&&(r, _)| r == object);
                match was {
                    Some(&(_, name)) if !self.s.is_current(object) => name,
                    _ => self.name(object.slot),
                }
            })
            .collect();
        self.permanents = self
            .s
            .battlefield()
            .iter()
            .map(|&slot| (self.s.object_ref(slot), self.name(slot)))
            .collect();
        let turn = self.s.turn();
        let stack = self
            .s
            .stack()
            .iter()
            .map(|e| StackItem {
                name: self.t.def(e.card).face_name(e.face),
                spell: e.is_spell(),
                controller: e.controller,
            })
            .collect();
        let (decider, kind, purpose) = match *step {
            Step::Decide(d) => (Some(d.player), Some(d.kind), Some(d.prompt.purpose)),
            Step::Over(_) => (None, None, None),
        };
        let flashback = self
            .s
            .cast_permissions()
            .filter(|p| p.terms.is_flashback())
            .map(|p| self.name(p.card.slot))
            .collect();
        let face_down = self
            .s
            .battlefield()
            .iter()
            .filter(|&&slot| self.s.object(slot).face() == FaceIx::DOWN)
            .map(|&slot| self.name(slot))
            .collect();
        let mut creatures = [0u8; 2];
        if let Step::Decide(_) = step {
            for &slot in self.s.battlefield() {
                if self.s.derived(slot).types.contains(Types::CREATURE) {
                    creatures[self.s.controller(slot).index()] += 1;
                }
            }
        }
        Position {
            forced: std::mem::take(&mut self.forced),
            creatures,
            flashback,
            face_down,
            turn: turn.number(),
            step: turn.step(),
            active: turn.active(),
            decider,
            kind,
            purpose,
            stack,
            flips,
            taps,
        }
    }

    fn name(&self, slot: Slot) -> &'static str {
        self.s.name(&self.t, slot)
    }

    fn named(&self, o: Opt) -> Option<Slot> {
        o.object_named(&self.s)
    }

    /// The candidate `c` of `d` as a pick, where the scenario vocabulary can name it. The pick
    /// may name an earlier candidate too, of the same name.
    fn pick_for(&self, d: &Decision, c: Choice) -> Option<Pick> {
        crate::script::pick(&self.t, &self.s, d, c)
    }

    /// The candidate of `d` that `pick` names.
    fn pick(&self, d: &Decision, pick: Pick) -> Option<Choice> {
        pick_in(&self.t, &self.s, d, pick)
    }

    /// The permanents named `card` that `player` controls, in the order they entered.
    fn permanents<'b>(&'b self, player: Seat, card: &'b str) -> impl Iterator<Item = Slot> + 'b {
        self.s
            .battlefield()
            .iter()
            .copied()
            .filter(move |&slot| self.s.controller(slot) == player && self.name(slot) == card)
    }

    /// The first permanent named `card` that `player` controls.
    fn permanent(&self, player: Seat, card: &str) -> Result<Slot, String> {
        self.permanents(player, card)
            .next()
            .ok_or_else(|| format!("player {player} controls no {card}"))
    }

    fn describe(&self, d: &Decision) -> String {
        describe(&self.t, &self.s, d)
    }

    /// The actual and expected values of an assertion about permanents.
    fn permanent_fact(&self, assertion: Assertion) -> Result<(String, String), String> {
        let s = &self.s;
        let tally = |found: usize, expected: u8| (found.to_string(), expected.to_string());
        Ok(match assertion {
            Assertion::Tapped {
                player,
                card,
                count,
            } => tally(
                self.permanents(player, card)
                    .filter(|&slot| s.object(slot).flags().has(ObjectFlags::TAPPED))
                    .count(),
                count,
            ),
            Assertion::Controls {
                player,
                card,
                count,
            } => tally(self.permanents(player, card).count(), count),
            Assertion::FaceDown {
                player,
                card,
                count,
            } => tally(
                self.permanents(player, card)
                    .filter(|&slot| s.object(slot).face() == FaceIx::DOWN)
                    .count(),
                count,
            ),
            Assertion::Attacking {
                player,
                card,
                count,
            } => tally(
                self.permanents(player, card)
                    .filter(|&slot| s.object(slot).flags().has(ObjectFlags::ATTACKING))
                    .count(),
                count,
            ),
            Assertion::Pt {
                player,
                card,
                token,
                power,
                toughness,
            } => {
                let slot = self
                    .permanents(player, card)
                    .find(|&slot| s.object(slot).flags().has(ObjectFlags::TOKEN) == token)
                    .ok_or_else(|| {
                        let what = if token { "token" } else { "card" };
                        format!("player {player} controls no {card} {what}")
                    })?;
                let d = s.derived(slot);
                (
                    format!("{}/{}", d.power, d.toughness),
                    format!("{power}/{toughness}"),
                )
            }
            Assertion::Creature {
                player,
                card,
                creature,
            } => {
                let d = s.derived(self.permanent(player, card)?);
                let is = d.types.contains(Types::CREATURE);
                (is.to_string(), creature.to_string())
            }
            Assertion::Counters {
                player,
                card,
                kind,
                count,
            } => {
                let slot = self.permanent(player, card)?;
                let n = s.counters_of(slot, kind);
                (n.to_string(), count.to_string())
            }
            _ => unreachable!("an assertion about permanents"),
        })
    }

    fn check(&self, position: &Step, assertion: Assertion) -> Result<(), String> {
        let position = *position;
        let s = &self.s;
        let (actual, expected) = match assertion {
            Assertion::Life { player, life } => {
                (s.player(player).life().to_string(), life.to_string())
            }
            Assertion::Count {
                player,
                zone,
                card,
                count,
            } => {
                if matches!(zone, Zone::Stack | Zone::Nowhere) {
                    return Err(format!("no owned {zone:?} to count"));
                }
                let n = s
                    .zone(ZoneKey::Of(player, zone))
                    .filter(|slot| card.is_none_or(|c| self.name(slot) == c))
                    .count();
                (n.to_string(), count.to_string())
            }
            Assertion::Tapped { .. }
            | Assertion::Controls { .. }
            | Assertion::FaceDown { .. }
            | Assertion::Attacking { .. }
            | Assertion::Pt { .. }
            | Assertion::Creature { .. }
            | Assertion::Counters { .. } => {
                // Control and characteristics are layer results, up to date only at an open
                // decision.
                if let Step::Over(o) = position {
                    return Err(format!("no permanents to read: the game is over ({o:?})"));
                }
                self.permanent_fact(assertion)?
            }
            Assertion::Stack { count } => (s.stack().len().to_string(), count.to_string()),
            Assertion::Library { player, top } => {
                let library = s.library(player).iter().rev().take(top.len());
                let actual: Vec<&str> = library.map(|&slot| self.name(slot)).collect();
                (format!("{actual:?}"), format!("{top:?}"))
            }
            Assertion::Mana {
                player: p,
                color,
                amount,
            } => (s.player(p).mana()[color].to_string(), amount.to_string()),
            Assertion::Decision {
                player,
                kind,
                candidates,
            } => {
                let actual = match position {
                    Step::Decide(d) => self.describe(&d),
                    Step::Over(o) => format!("the game over ({o:?})"),
                };
                let matches = matches!(position, Step::Decide(d)
                    if d.player == player && d.kind == kind && d.len() == usize::from(candidates));
                if matches {
                    return Ok(());
                }
                (
                    actual,
                    format!("player {player} {kind:?} among {candidates} candidates"),
                )
            }
            Assertion::Outcome { outcome } => {
                let actual = match position {
                    Step::Decide(_) => Ending::InProgress,
                    Step::Over(Outcome::Win(player)) => Ending::Win { player },
                    Step::Over(Outcome::Draw) => Ending::Draw,
                    Step::Over(o @ Outcome::Stalled(_)) => {
                        return Err(format!("the game stalled: {o:?}"));
                    }
                };
                (format!("{actual:?}"), format!("{outcome:?}"))
            }
        };
        if actual == expected {
            Ok(())
        } else {
            Err(format!(
                "{assertion:?}: expected {expected}, found {actual}"
            ))
        }
    }
}
