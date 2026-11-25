//! The strict runner: plays a scenario through the public engine API (architecture §17.1).

use beryl_engine::card::{FaceIx, Types};
use beryl_engine::event::{Event, Tag};
use beryl_engine::state::{CastStep, ObjectRef, Pending};
use beryl_engine::{
    Breakpoint, CardTable, Choice, Decision, DecisionKind, GameTable, Large, Layout, Opt, Profile,
    Replay, Scratch, Seat, Small, State, Step, Stops, Zone, ZoneKey, apply, concede, next, view,
};

mod answer;
mod assertion;
mod layouts;
mod live;
mod snapshot;

use crate::setup::{Slots, setup, with_layout};
use crate::{Action, Assertion, CoinFlip, Failure, Forced, ManaMade, Pick, Position, Scenario};
pub use answer::{answer, describe};
use answer::{option_index, pick_in, takes, taking};
pub use layouts::{LayoutComparison, plays_alike_in_both_layouts};
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

/// Runs a scenario like [`run`], and returns its final snapshot: the facts about the position
/// after its last action (§17.2), empty after the game ends. Two definitions of a card that
/// pass the same scenario still differ when their snapshots do.
pub fn outcome(cards: &'static CardTable, scenario: &Scenario) -> Result<Vec<Assertion>, Failure> {
    outcome_with(cards, scenario, |_, _| {}, |_, _| {})
}

/// The oracle snapshot, with each scripted decision and its state available for comparisons
/// that can use Beryl-specific information, such as the mana left after a scripted payment.
pub fn outcome_with(
    cards: &'static CardTable,
    scenario: &Scenario,
    small: impl FnMut(&State<Small>, &Step<Small>),
    large: impl FnMut(&State<Large>, &Step<Large>),
) -> Result<Vec<Assertion>, Failure> {
    with_layout(
        cards,
        scenario,
        Slots::Dealt,
        scenario,
        |t, s, scenario| snapshot_from(Runner::from_position(t, s), scenario, small),
        |t, s, scenario| snapshot_from(Runner::from_position(t, s), scenario, large),
    )
}

fn snapshot_from<L: Layout>(
    mut runner: Runner<L>,
    scenario: &Scenario,
    mut observe: impl FnMut(&State<L>, &Step<L>),
) -> Result<Vec<Assertion>, Failure> {
    let failed = |action, message| Failure {
        scenario: scenario.name,
        action,
        message,
    };
    for (i, &action) in scenario.actions.iter().enumerate() {
        runner.upcoming = upcoming(&scenario.actions[i..]);
        let fail = |message| failed(Some(i), message);
        let step = runner.position().map_err(fail)?;
        observe(&runner.s, &step);
        runner.act(action).map_err(fail)?;
    }
    runner.upcoming = None;
    let fail = |message| failed(None, message);
    let snapshot = runner.snapshot().map_err(fail)?;
    let step = runner.position().map_err(fail)?;
    observe(&runner.s, &step);
    Ok(snapshot)
}

/// Runs a scenario like [`trace`], with each deck's slots numbered as `slots` says: its
/// assertions must pass and its positions match under every numbering.
pub fn trace_with(
    cards: &'static CardTable,
    scenario: &Scenario,
    slots: Slots,
) -> Result<Vec<Position>, Failure> {
    with_layout(cards, scenario, slots, scenario, trace_from, trace_from)
}

fn trace_from<L: Layout>(
    t: GameTable,
    s: State<L>,
    scenario: &Scenario,
) -> Result<Vec<Position>, Failure> {
    walk(
        Runner::from_position(t, s),
        scenario,
        &mut |_, _, _| {},
        &mut |_, _, _| {},
    )
}

/// Runs a scenario in Small, showing `visit` the table, the state and the open decision
/// at each action other than an assertion. Use [`visit_with_layout`] for scenarios that
/// need Large, including those that can start a subgame.
pub fn visit(
    cards: &'static CardTable,
    scenario: &Scenario,
    mut visit: impl FnMut(&GameTable, &State, &Decision),
) -> Result<(), Failure> {
    walk(
        Runner::new(cards, scenario, Slots::Dealt)?,
        scenario,
        &mut visit,
        &mut |_, _, _| {},
    )
    .map(|_| ())
}

/// Examines scenario decisions in either engine layout.
pub trait DecisionVisitor {
    fn decision<L: Layout>(&mut self, table: &GameTable, state: &State<L>, decision: &Decision<L>);
}

/// Runs a scenario with the same layout selection and assertions as [`run`]. The visitor
/// sees the open decision at each action other than an assertion, in the game that asks it.
pub fn visit_with_layout(
    cards: &'static CardTable,
    scenario: &Scenario,
    visitor: &mut impl DecisionVisitor,
) -> Result<(), Failure> {
    with_layout(
        cards,
        scenario,
        Slots::Dealt,
        (scenario, visitor),
        visit_from,
        visit_from,
    )
}

fn visit_from<L: Layout, V: DecisionVisitor>(
    table: GameTable,
    state: State<L>,
    (scenario, visitor): (&Scenario, &mut V),
) -> Result<(), Failure> {
    walk(
        Runner::from_position(table, state),
        scenario,
        &mut |t, s, d| visitor.decision(t, s, d),
        &mut |_, _, _| {},
    )
    .map(|_| ())
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
    let mut watch = |state: &State, events: &[Event], _: &[Tag]| watch(state, events);
    walk(
        Runner::new(cards, scenario, slots)?,
        scenario,
        &mut |_, _, _| {},
        &mut watch,
    )
    .map(|_| ())
}

/// Runs a scenario like [`watch`], showing `watch` each event's tag too, in step with the
/// events: for tests of what is built from what the events came from.
pub fn watch_tagged(
    cards: &'static CardTable,
    scenario: &Scenario,
    mut watch: impl FnMut(&State, &[Event], &[Tag]),
) -> Result<(), Failure> {
    walk(
        Runner::new(cards, scenario, Slots::Dealt)?,
        scenario,
        &mut |_, _, _| {},
        &mut watch,
    )
    .map(|_| ())
}

/// What is shown the state each action begins from: the events since the action before, and
/// each one's tag.
type Watch<'a, L> = &'a mut dyn FnMut(&State<L>, &[Event<L>], &[Tag<L>]);
type Visit<'a, L> = &'a mut dyn FnMut(&GameTable, &State<L>, &Decision<L>);

fn walk<L: Layout>(
    mut runner: Runner<L>,
    scenario: &Scenario,
    visit: Visit<'_, L>,
    watch: Watch<'_, L>,
) -> Result<Vec<Position>, Failure> {
    let fail = |action, message: String| Failure {
        scenario: scenario.name,
        action,
        message,
    };
    let mut positions = Vec::with_capacity(scenario.actions.len());
    for (i, &action) in scenario.actions.iter().enumerate() {
        runner.upcoming = upcoming(&scenario.actions[i..]);
        let step = runner.position().map_err(|m| fail(Some(i), m))?;
        if let Step::Decide(d) = &step
            && !matches!(action, Action::Assert { .. })
        {
            visit(&runner.t, &runner.s, d);
        }
        watch(&runner.s, runner.x.events.events(), runner.x.events.tags());
        positions.push(runner.describe_position(&step));
        runner.act(action).map_err(|m| fail(Some(i), m))?;
    }
    watch(&runner.s, runner.x.events.events(), runner.x.events.tags());
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

struct Runner<L: Layout = beryl_engine::Small> {
    t: GameTable,
    s: State<L>,
    x: Scratch<L>,
    /// The large game that follows this runner in the layout comparison test.
    comparison: Option<Box<LayoutComparison>>,
    /// The next action that is not an assertion, if any, which decides where the game stops
    /// before it.
    upcoming: Option<Action>,
    /// What `next` returned for the current state, or `None` after an `apply`.
    position: Option<Step<L>>,
    /// Whether the open decision is an entry copy choice, for [`trace`].
    enter_as_copy: bool,
    /// The forced answers since the last position, for [`trace`].
    forced: Vec<Forced>,
    /// The permanents on the battlefield at the last position, by name: a tapped one that
    /// has since left, as a Treasure sacrificed for mana, is named as it was.
    permanents: Vec<(ObjectRef<L>, &'static str)>,
}

/// A copy at the same position, with fresh working memory: a [`Scratch`] is never copied
/// with the state it works on.
impl<L: Layout> Clone for Runner<L> {
    fn clone(&self) -> Self {
        Runner {
            t: self.t.clone(),
            s: self.s.clone(),
            x: Scratch::recording(),
            comparison: None,
            upcoming: self.upcoming,
            position: self.position.clone(),
            enter_as_copy: self.enter_as_copy,
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
        Ok(Self::from_position(t, s))
    }
}

impl<L: Layout> Runner<L> {
    fn from_position(t: GameTable, s: State<L>) -> Self {
        Self {
            t,
            s,
            comparison: None,
            // Recording, for the coin flips each position reports.
            x: Scratch::recording(),
            upcoming: None,
            position: None,
            enter_as_copy: false,
            forced: Vec::new(),
            permanents: Vec::new(),
        }
    }

    /// The current position: the open decision or the end of the game. When the next action
    /// that answers decisions is an advance, its target is a breakpoint, so the game stops
    /// there even where the engine would pass silently; when it is a resolution, the current
    /// step is, so the game stops as soon as the active player has priority again. Every new
    /// position must keep the state's invariants (§17.4), which debug builds of `next` and
    /// `apply` check, so a setup or a rules path that breaks one panics here, and agree with
    /// the reference implementations, which fails here.
    fn position(&mut self) -> Result<Step<L>, String> {
        if let Some(step) = &self.position {
            return Ok(step.clone());
        }
        let breakpoint = match self.upcoming {
            Some(Action::AdvanceTo { turn, step }) => Some(Breakpoint { turn, step }),
            // The active player's next priority in this step, after the resolution.
            Some(Action::Resolve | Action::Concede { .. }) => {
                let now = self.s.turn();
                Some(Breakpoint {
                    turn: now.number(),
                    step: now.step(),
                })
            }
            _ => None,
        };
        self.set_profile(Profile {
            breakpoint,
            ..TRACED
        });
        loop {
            let step = next(&mut self.s, &self.t, &mut self.x, &mut Replay);
            if let Some(comparison) = &mut self.comparison {
                comparison.check_next(&self.t, &self.s, &step)?;
            }
            self.enter_as_copy = self.x.events.asking.take_enter_as_copy();
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
                            enter_as_copy: self.enter_as_copy,
                            decides_for: (d.player != d.decider).then_some(d.player),
                            answer: Action::answer(d.decider, d.kind, pick),
                        });
                    }
                    self.answer(Choice(0));
                }
                _ => {
                    self.position = Some(step.clone());
                    return Ok(step);
                }
            }
        }
    }

    fn set_profile(&mut self, profile: Profile) {
        self.s.set_profile(profile);
        if let Some(comparison) = &mut self.comparison {
            comparison.set_profile(profile);
        }
    }

    fn decision(&mut self) -> Result<Decision<L>, String> {
        match self.position()? {
            Step::Decide(d) => Ok(d),
            Step::Over(o) => Err(format!("the game is over ({o:?})")),
        }
    }

    fn answer(&mut self, c: Choice) {
        apply(&mut self.s, &self.t, &mut self.x, &mut Replay, c);
        if let Some(comparison) = &mut self.comparison {
            comparison.apply(&self.t, c);
        }
        self.position = None;
    }

    fn act(&mut self, action: Action) -> Result<(), String> {
        match action {
            action @ (Action::PlayLand { player, .. }
            | Action::Cast { player, .. }
            | Action::Activate { player, .. }
            | Action::TurnFaceUp { player, .. }
            | Action::Companion { player }) => {
                let what = taking(&action);
                self.priority_option(player, &what, |r, o| takes(&r.t, &r.s, &action, o))?;
            }
            Action::Resolve => {
                // Waiting triggers go on the stack first.
                self.decision()?;
                let game_depth = self.s.subgame_depth();
                let depth = self.s.stack().len();
                let Some(top) = self.s.stack().last().cloned() else {
                    return Err("nothing on the stack to resolve".to_string());
                };
                loop {
                    // A resolution that ends the game is done.
                    let Step::Decide(d) = self.position()? else {
                        return Ok(());
                    };
                    let stack = self.s.stack();
                    if self.s.subgame_depth() != game_depth
                        || stack.len() < depth
                        || stack[depth - 1] != top
                    {
                        return Ok(());
                    }
                    if d.kind != DecisionKind::Priority {
                        return Ok(());
                    }
                    let pass = option_index(&d, |o| o == Opt::Pass).expect("passing is offered");
                    self.answer(pass);
                }
            }
            Action::Concede { player } => {
                if self.s.outcome().is_some() {
                    return Err("the active game has already ended".to_owned());
                }
                concede(&mut self.s, &self.t, &mut self.x, player);
                self.position = None;
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
                let here = self.s.compare_named_step(turn, step);
                if here.is_gt()
                    && !(now.number() == turn
                        && self.s.has_turn_plan()
                        && self.s.later_steps().any(|(later, _)| later == step))
                {
                    return Err(format!(
                        "the game passed turn {turn} {step:?} without stopping there; now {}",
                        self.describe(&d)
                    ));
                }
                if d.kind != DecisionKind::Priority {
                    return Err(format!("unscripted decision: {}", self.describe(&d)));
                }
                if here.is_eq() && d.player == now.active() {
                    // Stopped: keep the decision, and stop no more.
                    self.set_profile(TRACED);
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
        wanted: impl Fn(&Self, Opt<L>) -> bool,
    ) -> Result<(), String> {
        let d = self.expect(player, DecisionKind::Priority)?;
        let c = option_index(&d, |o| wanted(self, o))
            .ok_or_else(|| format!("{what} is not offered: {}", self.describe(&d)))?;
        self.answer(c);
        Ok(())
    }

    /// The open decision, which `player` must answer and of `kind`: a scenario's actions are
    /// those of the player who answers, who controls the player it belongs to while one
    /// player controls another (CR 724.5), as in `XMage`'s tests.
    fn expect(&mut self, player: Seat, kind: DecisionKind) -> Result<Decision<L>, String> {
        let d = self.decision()?;
        if d.decider == player && d.kind == kind {
            Ok(d)
        } else {
            Err(format!(
                "unscripted decision: {}, where the scenario has player {player} answer {kind:?}",
                self.describe(&d)
            ))
        }
    }

    /// The coin flips, the taps and the mana made since the last position, from the recorded
    /// events, for [`describe_position`].
    fn take_events(&mut self) -> (Vec<CoinFlip>, Vec<&'static str>, Vec<ManaMade>) {
        let mut flips = Vec::new();
        let mut taps = Vec::new();
        let mut made = Vec::new();
        for e in self.x.events.drain() {
            match e {
                Event::CoinFlip { player, won, .. } => flips.push(CoinFlip { player, won }),
                Event::Tap { object } => taps.push(object),
                Event::Mana {
                    source: Some(object),
                    color,
                    restricted,
                    ..
                } => made.push((object, color, restricted)),
                _ => {}
            }
        }
        // A permanent by the name it had when the previous action began, should it have left
        // the battlefield since, as a source that sacrifices itself for mana.
        let named = |object| {
            let was = self.permanents.iter().find(|&&(r, _)| r == object);
            match was {
                Some(&(_, name)) if !self.s.is_current(object) => name,
                _ => self.name(object.slot),
            }
        };
        let taps = taps.into_iter().map(named).collect();
        let mana = made
            .into_iter()
            .map(|(object, color, restricted)| ManaMade {
                source: named(object),
                color,
                restricted,
            })
            .collect();
        (flips, taps, mana)
    }

    /// The position an action begins from, for [`trace`]. It takes the coin flips, taps and
    /// mana since the last position from the recorded events.
    fn describe_position(&mut self, step: &Step<L>) -> Position {
        let (flips, taps, mana) = self.take_events();
        self.permanents = self
            .s
            .battlefield()
            .iter()
            .map(|&slot| (self.s.object_ref(slot), self.name(slot)))
            .collect();
        let turn = self.s.turn();
        let stack = self.stack_items().collect();
        let (decider, decides_for, kind, purpose) = match step {
            Step::Decide(d) => (
                Some(d.decider),
                (d.player != d.decider).then_some(d.player),
                Some(d.kind),
                Some(d.prompt.purpose),
            ),
            Step::Over(_) => (None, None, None, None),
        };
        // A copy's first target option keeps its existing target. A spell cast during
        // resolution asks for a fresh target, with the cast visible in the replayed state.
        let kept = match step {
            Step::Decide(d)
                if d.kind == DecisionKind::Target
                    && matches!(self.s.pending(), Pending::Resolving(_))
                    && !matches!(
                        view(&self.s, &self.t, &mut self.x).pending(),
                        Pending::Casting(_)
                    ) =>
            {
                self.pick_for(d, Choice(0))
            }
            _ => None,
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
        let morphed = self
            .s
            .battlefield()
            .iter()
            .filter(|&&slot| self.s.object(slot).face() == FaceIx::DOWN && self.s.morphed(slot))
            .map(|&slot| self.name(slot))
            .collect();
        let mut creatures = [0u8; 2];
        if let Step::Decide(_) = step {
            for &slot in self.s.battlefield() {
                if self.s.derived(slot).types.contains(Types::CREATURE) {
                    let count = &mut creatures[self.s.controller(slot).index()];
                    *count = count.saturating_add(1);
                }
            }
        }
        Position {
            subgame_depth: self.s.subgame_depth(),
            damage_choice: self.damage_choice(step),
            forced: std::mem::take(&mut self.forced),
            creatures,
            flashback,
            face_down,
            morphed,
            turn: turn.number(),
            step: turn.step(),
            active: turn.active(),
            decider,
            decides_for,
            kind,
            purpose,
            paying_objects: matches!(self.s.pending(), Pending::Casting(cast) if matches!(cast.step, CastStep::Objects(_))),
            graveyard_cards: Seat::BOTH.map(|player| {
                self.s
                    .graveyard(player)
                    .iter()
                    .filter(|&&slot| self.s.card_behind(slot).is_some())
                    .count()
            }),
            stack,
            enter_as_copy: self.enter_as_copy,
            kept,
            flips,
            taps,
            mana,
        }
    }

    fn damage_choice(&self, step: &Step<L>) -> Option<crate::DamageChoice> {
        let (Pending::ExactCombatDamage(dividing), Step::Decide(d)) = (self.s.pending(), step)
        else {
            return None;
        };
        let recipient = d.about.sole()?;
        Some(crate::DamageChoice {
            source: self.name(dividing.source),
            recipient: self.name(recipient),
            remaining: dividing.remaining,
            recipients: dividing.recipients.count(),
        })
    }

    fn name(&self, slot: L::Slot) -> &'static str {
        self.s.name(&self.t, slot)
    }

    /// The candidate `c` of `d` as a pick, where the scenario vocabulary can name it. The pick
    /// may name an earlier candidate too, of the same name.
    fn pick_for(&self, d: &Decision<L>, c: Choice) -> Option<Pick> {
        crate::script::pick(&self.t, &self.s, d, c)
    }

    /// The candidate of `d` that `pick` names.
    fn pick(&self, d: &Decision<L>, pick: Pick) -> Option<Choice> {
        pick_in(&self.t, &self.s, d, pick)
    }

    /// The permanents named `card` that `player` controls, in the order they entered.
    fn permanents<'b>(&'b self, player: Seat, card: &'b str) -> impl Iterator<Item = L::Slot> + 'b {
        self.s
            .battlefield()
            .iter()
            .copied()
            .filter(move |&slot| self.s.controller(slot) == player && self.name(slot) == card)
    }

    /// The first permanent named `card` that `player` controls.
    fn permanent(&self, player: Seat, card: &str) -> Result<L::Slot, String> {
        self.permanents(player, card)
            .next()
            .ok_or_else(|| format!("player {player} controls no {card}"))
    }

    /// The first object named `card` in `zone`: a permanent `player` controls, a spell they
    /// control, or a card they own elsewhere.
    fn object_in(&self, player: Seat, zone: Zone, card: &str) -> Result<L::Slot, String> {
        let s = &self.s;
        let found = match zone {
            Zone::Battlefield => return self.permanent(player, card),
            Zone::Stack => (s.stack().iter())
                .filter_map(|entry| entry.spell().map(|spell| spell.slot))
                .find(|&slot| s.controller(slot) == player && self.name(slot) == card),
            _ => s
                .zone(ZoneKey::Of(player, zone))
                .filter(|slot| self.name(slot) == card)
                .min(|slot| slot),
        };
        found.ok_or_else(|| format!("player {player} has no {card} in {zone:?}"))
    }

    fn describe(&self, d: &Decision<L>) -> String {
        describe(&self.t, &self.s, d)
    }
}
