//! The strict runner: plays a scenario through `next` and `apply` only (architecture §17.1).

use std::fmt;

use beryl_engine::state::ObjectFlags;
use beryl_engine::{
    Breakpoint, CardTable, Choice, Decision, DecisionKind, GameTable, Opt, Outcome, Profile,
    Replay, Scratch, State, Step, Zone, apply, next,
};

use crate::{Action, Assertion, Ending, Pick, Scenario};

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

/// Runs a scenario over the cards of `cards`, under the exact profile.
pub fn run(cards: &'static CardTable, scenario: &Scenario) -> Result<(), Failure> {
    let fail = |action, message: String| Failure {
        scenario: scenario.name,
        action,
        message,
    };
    let decks = decks(scenario);
    let t = GameTable::new(cards, [decks[0].as_slice(), decks[1].as_slice()])
        .map_err(|e| fail(None, format!("{e:?}")))?;
    let s = arrange(&t, scenario).map_err(|m| fail(None, m))?;
    let mut runner = Runner {
        t: &t,
        s,
        x: Scratch::new(),
        actions: scenario.actions,
        position: None,
    };
    for i in 0..scenario.actions.len() {
        runner.act(i).map_err(|m| fail(Some(i), m))?;
    }
    Ok(())
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
    for (p, player) in setup.players.iter().enumerate() {
        let p = p as u8;
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
fn library_slot(s: &State, t: &GameTable, player: u8, name: &str) -> u8 {
    s.library(player)
        .iter()
        .copied()
        .find(|&slot| name_of(s, t, slot) == name)
        .expect("the deck holds every card the setup names")
}

fn name_of(s: &State, t: &GameTable, slot: u8) -> &'static str {
    t.def(s.object(slot).card).name
}

struct Runner<'a> {
    t: &'a GameTable,
    s: State,
    x: Scratch,
    actions: &'static [Action],
    /// What `next` returned for the current state, or `None` after an `apply`.
    position: Option<Step>,
}

impl Runner<'_> {
    /// The current position: the open decision or the end of the game. When the next action
    /// that answers decisions is an advance, its target is a breakpoint, so the game stops
    /// there even where the engine would pass silently.
    fn position(&mut self, i: usize) -> Step {
        if let Some(step) = self.position {
            return step;
        }
        let upcoming = self.actions[i..]
            .iter()
            .find(|a| !matches!(a, Action::Assert { .. }));
        let breakpoint = match upcoming {
            Some(&Action::AdvanceTo { turn, step }) => Some(Breakpoint { turn, step }),
            _ => None,
        };
        self.s.set_profile(Profile {
            breakpoint,
            ..Profile::EXACT
        });
        let step = next(&mut self.s, self.t, &mut self.x, &mut Replay);
        self.position = Some(step);
        step
    }

    fn decision(&mut self, i: usize) -> Result<Decision, String> {
        match self.position(i) {
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
            Action::PlayLand { player, card } => {
                let d = self.expect(i, player, DecisionKind::Priority)?;
                let c = option_index(
                    &d,
                    |o| matches!(o, Opt::PlayLand(slot) if self.name(slot) == card),
                )
                .ok_or_else(|| format!("playing {card} is not offered: {}", self.describe(&d)))?;
                self.answer(c);
            }
            Action::Answer { player, kind, pick } => {
                let d = self.expect(i, player, kind)?;
                let c = match pick {
                    Pick::Pass => option_index(&d, |o| o == Opt::Pass),
                    Pick::Card { card } => (0..d.len() as u8)
                        .map(Choice)
                        .find(|&c| d.candidates.object(c).is_some_and(|o| self.name(o) == card)),
                }
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
                if here == target {
                    // Stopped: keep the decision, and stop no more.
                    self.s.set_profile(Profile::EXACT);
                    return Ok(());
                }
                self.answer(Choice(0));
            },
            Action::Assert { assertion } => {
                let position = self.position(i);
                self.check(position, assertion)?;
            }
        }
        Ok(())
    }

    /// The open decision, which must be `player`'s and of `kind`.
    fn expect(&mut self, i: usize, player: u8, kind: DecisionKind) -> Result<Decision, String> {
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

    fn name(&self, slot: u8) -> &'static str {
        name_of(&self.s, self.t, slot)
    }

    /// A decision as a scenario author reads it.
    fn describe(&self, d: &Decision) -> String {
        let candidates: Vec<String> = (0..d.len() as u8)
            .map(|i| {
                let c = Choice(i);
                match (d.candidates.object(c), d.candidates.option(c)) {
                    (Some(slot), _) => self.name(slot).to_string(),
                    (_, Some(Opt::PlayLand(slot))) => format!("PlayLand({})", self.name(slot)),
                    (_, Some(o)) => format!("{o:?}"),
                    (None, None) => unreachable!("a choice within the candidates"),
                }
            })
            .collect();
        let turn = self.s.turn();
        format!(
            "player {} {:?} in turn {} {:?}, among [{}]",
            d.player,
            d.kind,
            turn.number,
            turn.step,
            candidates.join(", ")
        )
    }

    fn check(&self, position: Step, assertion: Assertion) -> Result<(), String> {
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
            Assertion::Tapped {
                player,
                card,
                count,
            } => {
                let n = s
                    .battlefield()
                    .iter()
                    .filter(|&&slot| {
                        let o = s.object(slot);
                        o.controller == player
                            && self.name(slot) == card
                            && o.flags.has(ObjectFlags::TAPPED)
                    })
                    .count();
                (n.to_string(), count.to_string())
            }
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
