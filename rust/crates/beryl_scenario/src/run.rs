//! The strict runner: plays a scenario through `next` and `apply` only (architecture §17.1).

use std::fmt;

use beryl_engine::card::{FaceIx, Types};
use beryl_engine::event::Event;
use beryl_engine::state::{CounterKind, ObjectFlags, ObjectRef};
use beryl_engine::{
    Breakpoint, CardTable, Choice, Decision, DecisionKind, GameTable, Opt, Outcome, Profile,
    Replay, Scratch, Seat, Slot, State, Step, Stops, Zone, apply, next,
};

use crate::{Action, Assertion, CoinFlip, Ending, Forced, Pick, Position, Scenario, StackItem};

/// Why a scenario failed: at which action, and what went wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub scenario: &'static str,
    /// The index of the failing action, or `None` for the setup.
    pub action: Option<usize>,
    pub message: String,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.action {
            Some(i) => write!(f, "{}: action {i}: {}", self.scenario, self.message),
            None => write!(f, "{}: setup: {}", self.scenario, self.message),
        }
    }
}

impl std::error::Error for Failure {}

/// The exact profile, except that a question with one answer stops too: the runner answers it
/// as the engine would have silently, and the trace records the answer.
const TRACED: Profile = Profile {
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
    walk(cards, scenario, &mut |_, _, _| {})
}

/// Runs a scenario like [`run`], showing `visit` the table, the state and the open decision
/// each action other than an assertion answers or passes through: for tests of what a
/// decision says beyond the engine, such as the protocol's views of it.
pub fn visit(
    cards: &'static CardTable,
    scenario: &Scenario,
    mut visit: impl FnMut(&GameTable, &State, &Decision),
) -> Result<(), Failure> {
    walk(cards, scenario, &mut visit).map(|_| ())
}

fn walk(
    cards: &'static CardTable,
    scenario: &Scenario,
    visit: &mut dyn FnMut(&GameTable, &State, &Decision),
) -> Result<Vec<Position>, Failure> {
    let fail = |action, message: String| Failure {
        scenario: scenario.name,
        action,
        message,
    };
    let (t, s) = position(cards, scenario)?;
    let mut runner = Runner {
        t: &t,
        s,
        // Recording, for the coin flips each position reports.
        x: Scratch::recording(),
        actions: scenario.actions,
        position: None,
        forced: Vec::new(),
        permanents: Vec::new(),
    };
    let mut positions = Vec::with_capacity(scenario.actions.len());
    for i in 0..scenario.actions.len() {
        let step = runner.position(i).map_err(|m| fail(Some(i), m))?;
        if let Step::Decide(d) = &step
            && !matches!(scenario.actions[i], Action::Assert { .. })
        {
            visit(&t, &runner.s, d);
        }
        positions.push(runner.describe_position(&step));
        runner.act(i).map_err(|m| fail(Some(i), m))?;
    }
    Ok(positions)
}

/// A scenario's starting position and the table it is played on, for tests that drive the
/// engine themselves from a hand-built position, such as the agents'. Its actions are not
/// run.
pub fn position(
    cards: &'static CardTable,
    scenario: &Scenario,
) -> Result<(GameTable, State), Failure> {
    let fail = |message: String| Failure {
        scenario: scenario.name,
        action: None,
        message,
    };
    let decks = decks(scenario);
    let t = GameTable::new(cards, [decks[0].as_slice(), decks[1].as_slice()])
        .map_err(|e| fail(format!("{e:?}")))?;
    let s = arrange(&t, scenario).map_err(fail)?;
    Ok((t, s))
}

/// Every card the setup names, by owner: the decks the game is built from.
fn decks(scenario: &Scenario) -> [Vec<(u8, &'static str)>; 2] {
    scenario.setup.players.map(|p| {
        p.hand
            .iter()
            .chain(p.battlefield.iter().map(|b| &b.card))
            .chain(p.graveyard)
            .chain(p.exile)
            .chain(p.library)
            .map(|&name| (1, name))
            .collect()
    })
}

/// The setup's position: every card starts in its owner's library and moves to its zone.
fn arrange(t: &GameTable, scenario: &Scenario) -> Result<State, String> {
    let setup = &scenario.setup;
    let mut s = State::new(t, setup.seed, Profile::EXACT);
    s.begin_at(setup.turn, setup.active, setup.step);
    for (p, player) in Seat::BOTH.into_iter().zip(&setup.players) {
        s.set_life(p, player.life);
        let full = |_| "a zone is full".to_string();
        for &name in player.hand {
            s.place(library_slot(&s, t, p, name), Zone::Hand)
                .map_err(full)?;
        }
        for b in player.battlefield {
            let slot = library_slot(&s, t, p, b.card);
            s.place(slot, Zone::Battlefield).map_err(full)?;
            s.set_tapped(slot, b.tapped);
            s.set_face(slot, b.face);
            // CR 306.5b: a planeswalker "enters with a number of loyalty counters on it equal
            // to its printed loyalty number."
            let printed = t.def(s.card(slot)).face(b.face).loyalty;
            let counters = b
                .counters
                .map(|c| (c.kind, c.count))
                .or(printed.map(|n| (CounterKind::Loyalty, n)));
            if let Some((kind, n)) = counters {
                s.set_counters(slot, kind, n);
            }
        }
        // Top first: the last placed is on top.
        for &name in player.graveyard.iter().rev() {
            s.place(library_slot(&s, t, p, name), Zone::Graveyard)
                .map_err(full)?;
        }
        for &name in player.exile {
            s.place(library_slot(&s, t, p, name), Zone::Exile)
                .map_err(full)?;
        }
        let mut order = Vec::new();
        for &name in player.library {
            let slot = s
                .library(p)
                .iter()
                .copied()
                .find(|&slot| name_of(&s, t, slot) == name && !order.contains(&slot))
                .expect("the library holds the cards it lists");
            order.push(slot);
        }
        s.order_library(p, &order);
    }
    Ok(s)
}

/// A slot in `player`'s library holding the card `name`. The decks are built from the setup,
/// so there is always one.
fn library_slot(s: &State, t: &GameTable, player: Seat, name: &str) -> Slot {
    s.library(player)
        .iter()
        .copied()
        .find(|&slot| name_of(s, t, slot) == name)
        .expect("the deck holds every card the setup names")
}

/// The name of the object in `slot`: its face up's, which is the card's own name everywhere
/// but on the stack and the battlefield (CR 712.8a, 712.8f). A face-down permanent, which has
/// no name, goes by its card's.
fn name_of(s: &State, t: &GameTable, slot: Slot) -> &'static str {
    let def = t.def(s.card(slot));
    match s.object(slot).face {
        FaceIx::DOWN => def.name,
        face => def.face_name(face),
    }
}

struct Runner<'a> {
    t: &'a GameTable,
    s: State,
    x: Scratch,
    actions: &'static [Action],
    /// What `next` returned for the current state, or `None` after an `apply`.
    position: Option<Step>,
    /// The forced answers since the last position, for [`trace`].
    forced: Vec<Forced>,
    /// The permanents on the battlefield at the last position, by name: a tapped one that
    /// has since left, as a Treasure sacrificed for mana, is named as it was.
    permanents: Vec<(ObjectRef, &'static str)>,
}

impl Runner<'_> {
    /// The current position: the open decision or the end of the game. When the next action
    /// that answers decisions is an advance, its target is a breakpoint, so the game stops
    /// there even where the engine would pass silently; when it is a resolution, the current
    /// step is, so the game stops as soon as the active player has priority again. Every new
    /// position must keep the state's invariants (§17.4), so a setup or a rules path that
    /// breaks one fails here.
    fn position(&mut self, i: usize) -> Result<Step, String> {
        if let Some(step) = self.position {
            return Ok(step);
        }
        let upcoming = self.actions[i..]
            .iter()
            .find(|a| !matches!(a, Action::Assert { .. }));
        let breakpoint = match upcoming {
            Some(&Action::AdvanceTo { turn, step }) => Some(Breakpoint { turn, step }),
            // The active player's next priority in this step, after the resolution.
            Some(Action::Resolve) => {
                let now = self.s.turn();
                Some(Breakpoint {
                    turn: now.number,
                    step: now.step,
                })
            }
            _ => None,
        };
        self.s.set_profile(Profile {
            breakpoint,
            ..TRACED
        });
        loop {
            let step = next(&mut self.s, self.t, &mut self.x, &mut Replay);
            self.s
                .check_invariants(self.t)
                .map_err(|v| format!("an engine invariant broke: {v:?}"))?;
            match step {
                // A question with one answer, which the exact profile answers silently.
                Step::Decide(d) if d.len() == 1 && d.kind != DecisionKind::Priority => {
                    if let Some(pick) = self.forced_pick(&d) {
                        let turn = self.s.turn();
                        self.forced.push(Forced {
                            turn: turn.number,
                            step: turn.step,
                            answer: Action::answer(d.player, d.kind, pick),
                        });
                    }
                    apply(&mut self.s, self.t, &mut self.x, &mut Replay, Choice(0));
                }
                _ => {
                    self.position = Some(step);
                    return Ok(step);
                }
            }
        }
    }

    fn decision(&mut self, i: usize) -> Result<Decision, String> {
        match self.position(i)? {
            Step::Decide(d) => Ok(d),
            Step::Over(o) => Err(format!("the game is over ({o:?})")),
        }
    }

    fn answer(&mut self, c: Choice) {
        apply(&mut self.s, self.t, &mut self.x, &mut Replay, c);
        self.position = None;
    }

    fn act(&mut self, i: usize) -> Result<(), String> {
        match self.actions[i] {
            action @ (Action::PlayLand { player, .. }
            | Action::Cast { player, .. }
            | Action::Activate { player, .. }
            | Action::TurnFaceUp { player, .. }) => {
                let what = taking(&action);
                self.priority_option(i, player, &what, |r, o| takes(r.t, &r.s, &action, o))?;
            }
            Action::Resolve => {
                // Waiting triggers go on the stack first.
                self.decision(i)?;
                let depth = self.s.stack().len();
                let Some(&top) = self.s.stack().last() else {
                    return Err("nothing on the stack to resolve".to_string());
                };
                loop {
                    let d = self.decision(i)?;
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
                let d = self.expect(i, player, kind)?;
                let c = self
                    .pick(&d, pick)
                    .ok_or_else(|| format!("{pick:?} is not offered: {}", self.describe(&d)))?;
                self.answer(c);
            }
            Action::AdvanceTo { turn, step } => loop {
                let d = self.decision(i)?;
                let now = self.s.turn();
                let here = (now.number, now.step as u8);
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
                if here == target && d.player == now.active {
                    // Stopped: keep the decision, and stop no more.
                    self.s.set_profile(TRACED);
                    return Ok(());
                }
                let pass = option_index(&d, |o| o == Opt::Pass).expect("passing is offered");
                self.answer(pass);
            },
            Action::Assert { assertion } => {
                let position = self.position(i)?;
                self.check(&position, assertion)?;
            }
        }
        Ok(())
    }

    /// Answers `player`'s priority decision with the first option `wanted` accepts, or fails
    /// saying that `what` is not offered.
    fn priority_option(
        &mut self,
        i: usize,
        player: Seat,
        what: &str,
        wanted: impl Fn(&Self, Opt) -> bool,
    ) -> Result<(), String> {
        let d = self.expect(i, player, DecisionKind::Priority)?;
        let c = option_index(&d, |o| wanted(self, o))
            .ok_or_else(|| format!("{what} is not offered: {}", self.describe(&d)))?;
        self.answer(c);
        Ok(())
    }

    /// The open decision, which must be `player`'s and of `kind`.
    fn expect(&mut self, i: usize, player: Seat, kind: DecisionKind) -> Result<Decision, String> {
        let d = self.decision(i)?;
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
        let (decider, kind) = match *step {
            Step::Decide(d) => (Some(d.player), Some(d.kind)),
            Step::Over(_) => (None, None),
        };
        let flashback = self
            .s
            .permissions()
            .iter()
            .filter(|p| p.flashback && self.s.is_current(p.object))
            .map(|p| self.name(p.object.slot))
            .collect();
        let face_down = self
            .s
            .battlefield()
            .iter()
            .filter(|&&slot| self.s.object(slot).face == FaceIx::DOWN)
            .map(|&slot| self.name(slot))
            .collect();
        Position {
            forced: std::mem::take(&mut self.forced),
            flashback,
            face_down,
            turn: turn.number,
            step: turn.step,
            active: turn.active,
            decider,
            kind,
            stack,
            flips,
            taps,
        }
    }

    fn name(&self, slot: Slot) -> &'static str {
        name_of(&self.s, self.t, slot)
    }

    fn named(&self, o: Opt) -> Option<Slot> {
        named(&self.s, o)
    }

    /// The only answer to `d` as a pick, where the scenario vocabulary can name it.
    fn forced_pick(&self, d: &Decision) -> Option<Pick> {
        let c = Choice(0);
        if let Some(slot) = d.candidates.object(c) {
            return Some(Pick::Card {
                card: self.name(slot),
            });
        }
        let o = d.candidates.option(c)?;
        if let Some(slot) = self.named(o) {
            return Some(Pick::Card {
                card: self.name(slot),
            });
        }
        Some(match o {
            Opt::Pass => Pick::Pass,
            Opt::Yes => Pick::Yes,
            Opt::No => Pick::No,
            Opt::Done => Pick::Done,
            Opt::Number(n) => Pick::Number { n },
            Opt::Plan(i) => Pick::Plan { i },
            Opt::Player(player) => Pick::Player { player },
            Opt::Defender(None) => Pick::DefendingPlayer,
            _ => return None,
        })
    }

    /// The candidate of `d` that `pick` names.
    fn pick(&self, d: &Decision, pick: Pick) -> Option<Choice> {
        pick_in(self.t, &self.s, d, pick)
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
        describe(self.t, &self.s, d)
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
                    .filter(|&slot| s.object(slot).flags.has(ObjectFlags::TAPPED))
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
                    .filter(|&slot| s.object(slot).face == FaceIx::DOWN)
                    .count(),
                count,
            ),
            Assertion::Attacking {
                player,
                card,
                count,
            } => tally(
                self.permanents(player, card)
                    .filter(|&slot| s.object(slot).flags.has(ObjectFlags::ATTACKING))
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
                    .find(|&slot| s.object(slot).flags.has(ObjectFlags::TOKEN) == token)
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
                let n = s.object(slot).counters.get(kind);
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
                (s.player(player).life.to_string(), life.to_string())
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
                    .zone(player, zone)
                    .iter()
                    .filter(|&slot| card.is_none_or(|c| self.name(slot) == c))
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
                player,
                color,
                amount,
            } => (s.player(player).mana[color].to_string(), amount.to_string()),
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

/// The first option of `d` that `pick` accepts.
fn option_index(d: &Decision, mut pick: impl FnMut(Opt) -> bool) -> Option<Choice> {
    (0..d.len() as u8)
        .map(Choice)
        .find(|&c| d.candidates.option(c).is_some_and(&mut pick))
}

/// The choice that answers `d` as `action` would in a scenario, if `d` offers it, for an
/// action that answers one decision: playing a land, casting, activating, turning face up or
/// an answer. Whose decision it is and of what kind is the caller's to check. For scripts
/// that drive agents in place of the runner, such as the UI fixtures' boards.
pub fn answer(t: &GameTable, s: &State, d: &Decision, action: &Action) -> Option<Choice> {
    match *action {
        Action::Answer { pick, .. } => pick_in(t, s, d, pick),
        _ => option_index(d, |o| takes(t, s, action, o)),
    }
}

/// Whether `o` is the priority option `action` takes: playing, casting, activating or turning
/// face up its card.
fn takes(t: &GameTable, s: &State, action: &Action, o: Opt) -> bool {
    let name = |slot| name_of(s, t, slot);
    match *action {
        Action::PlayLand { card, .. } => matches!(o, Opt::PlayLand(slot) if name(slot) == card),
        Action::Cast {
            card, face, costs, ..
        } => matches!(o, Opt::Cast { slot, face: f, costs: m }
            if name(slot) == card && f == face && m == costs),
        Action::Activate { card, ability, .. } => matches!(o, Opt::Activate { slot, ability: a }
            if name(slot) == card && a == ability),
        Action::TurnFaceUp { card, .. } => {
            matches!(o, Opt::TurnFaceUp(slot) if name(slot) == card)
        }
        _ => false,
    }
}

/// What a priority action takes, as a failure names it.
fn taking(action: &Action) -> String {
    match *action {
        Action::PlayLand { card, .. } => format!("playing {card}"),
        Action::Cast {
            card, face, costs, ..
        } => format!(
            "casting {card} (face {}, costs {:#b})",
            face.get(),
            costs.bits()
        ),
        Action::Activate { card, ability, .. } => {
            format!("activating {card}'s ability {ability:?}")
        }
        Action::TurnFaceUp { card, .. } => format!("turning {card} face up"),
        action => format!("{action:?}"),
    }
}

/// The option of `d` that `pick` names.
fn pick_in(t: &GameTable, s: &State, d: &Decision, pick: Pick) -> Option<Choice> {
    let wanted = match pick {
        Pick::Card { card } => {
            return (0..d.len() as u8).map(Choice).find(|&c| {
                d.candidates
                    .object(c)
                    .or_else(|| d.candidates.option(c).and_then(|o| named(s, o)))
                    .is_some_and(|slot| name_of(s, t, slot) == card)
            });
        }
        Pick::Pass => Opt::Pass,
        Pick::Number { n } => Opt::Number(n),
        Pick::Yes => Opt::Yes,
        Pick::No => Opt::No,
        Pick::Done => Opt::Done,
        Pick::Player { player } => Opt::Player(player),
        Pick::DefendingPlayer => Opt::Defender(None),
        Pick::Plan { i } => Opt::Plan(i),
    };
    option_index(d, |o| o == wanted)
}

/// The object an option names, if it names one.
fn named(s: &State, o: Opt) -> Option<Slot> {
    match o {
        Opt::PlayLand(slot)
        | Opt::Attack(slot)
        | Opt::Block(slot)
        | Opt::Lethal(slot)
        | Opt::Cast { slot, .. }
        | Opt::Activate { slot, .. }
        | Opt::Mana { slot, .. }
        | Opt::Object(slot)
        | Opt::TurnFaceUp(slot) => Some(slot),
        Opt::Defender(slot) => slot,
        Opt::Trigger(i) => s
            .triggers()
            .get(usize::from(i))
            .and_then(|tr| tr.source)
            .map(|r| r.slot),
        _ => None,
    }
}

/// A decision as a scenario author reads it.
pub fn describe(t: &GameTable, s: &State, d: &Decision) -> String {
    let candidates: Vec<String> = (0..d.len() as u8)
        .map(|i| {
            let c = Choice(i);
            match (d.candidates.object(c), d.candidates.option(c)) {
                (Some(slot), _) => name_of(s, t, slot).to_string(),
                (_, Some(o)) => match named(s, o) {
                    Some(slot) => format!("{o:?} ({})", name_of(s, t, slot)),
                    None => format!("{o:?}"),
                },
                (None, None) => unreachable!("a choice within the candidates"),
            }
        })
        .collect();
    let turn = s.turn();
    format!(
        "player {} {:?} in turn {} {:?}, among [{}]",
        d.player,
        d.kind,
        turn.number,
        turn.step,
        candidates.join(", ")
    )
}
