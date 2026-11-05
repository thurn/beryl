//! Scenarios: black-box tests of the rules, one card at a time (architecture §17.1).
//!
//! A [`Scenario`] is a value: a [`Setup`] (where each card is, life, the turn and step), then
//! [`Action`]s, each a scripted decision or an [`Assertion`]. Card files declare them as
//! constants with the `const` builder below. [`run`] plays one through the engine's public
//! decision API under the exact profile, and is strict: every decision the engine offers must
//! be scripted, so the decision surface itself stays under test. [`to_json`] serializes
//! scenarios for the oracle exporters (§17.2).
//!
//! The actions and assertions are the ones the engine's mechanics support so far; casting,
//! activation, combat and the layers add theirs.

mod run;
#[cfg(test)]
mod tests;

use beryl_engine::{DecisionKind, TurnStep, Zone};
use serde::{Serialize, Serializer};

pub use run::{Failure, run};

/// The players, in seat order.
pub const P0: u8 = 0;
pub const P1: u8 = 1;

/// One scenario: a position, then what happens and what must be true.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Scenario {
    pub name: &'static str,
    pub setup: Setup,
    pub actions: &'static [Action],
}

/// Where the game starts. Every card named belongs to the player whose list names it; the
/// decks are exactly those cards.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Setup {
    /// The game's seed, for its shuffles and other randomness.
    pub seed: u64,
    /// The turn number, from 1. The active player takes the odd turns.
    pub turn: u16,
    pub active: u8,
    /// The step the game starts in, with the active player holding priority and the step's
    /// turn-based actions already done.
    #[serde(serialize_with = "variant")]
    pub step: TurnStep,
    pub players: [PlayerSetup; 2],
}

/// One player's part of the setup.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct PlayerSetup {
    pub life: i16,
    pub hand: &'static [&'static str],
    pub battlefield: &'static [Permanent],
    /// Top first.
    pub graveyard: &'static [&'static str],
    pub exile: &'static [&'static str],
    /// Top first: the whole library, in order.
    pub library: &'static [&'static str],
}

/// A permanent on the battlefield at the start, under its owner's control. It has been there
/// since its controller's turn began.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Permanent {
    pub card: &'static str,
    pub tapped: bool,
}

impl Permanent {
    pub const fn new(card: &'static str) -> Permanent {
        Permanent {
            card,
            tapped: false,
        }
    }

    #[must_use]
    pub const fn tapped(self) -> Permanent {
        Permanent {
            tapped: true,
            ..self
        }
    }
}

/// One step of a scenario.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(tag = "action")]
pub enum Action {
    /// `player` plays the land card `card` from their hand, at a priority decision.
    PlayLand { player: u8, card: &'static str },
    /// `player` answers the open decision, which must be of this kind.
    Answer {
        player: u8,
        #[serde(serialize_with = "variant")]
        kind: DecisionKind,
        pick: Pick,
    },
    /// Every player passes priority until the game reaches this turn and step, where it
    /// stops with the active player holding priority. Any other decision on the way is
    /// unscripted.
    AdvanceTo {
        turn: u16,
        #[serde(serialize_with = "variant")]
        step: TurnStep,
    },
    /// Must be true at the current position: the open decision, or the end of the game.
    Assert { assertion: Assertion },
}

/// The answer to a decision.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(tag = "pick")]
pub enum Pick {
    /// Pass priority.
    Pass,
    /// The candidate object that is a card of this name.
    Card { card: &'static str },
}

/// A fact about the current position.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(tag = "assert")]
pub enum Assertion {
    Life {
        player: u8,
        life: i16,
    },
    /// How many cards `player` owns in `zone`: all of them, or those named `card`.
    Count {
        player: u8,
        #[serde(serialize_with = "variant")]
        zone: Zone,
        card: Option<&'static str>,
        count: u8,
    },
    /// How many of the permanents named `card` that `player` controls are tapped.
    Tapped {
        player: u8,
        card: &'static str,
        count: u8,
    },
    /// The open decision: who decides, what, and among how many candidates.
    Decision {
        player: u8,
        #[serde(serialize_with = "variant")]
        kind: DecisionKind,
        candidates: u8,
    },
    /// Whether and how the game has ended.
    Outcome {
        outcome: Ending,
    },
}

/// How a game stands, for [`Assertion::Outcome`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "ending")]
pub enum Ending {
    InProgress,
    Win { player: u8 },
    Draw,
}

/// Serializes an engine enum as its variant name, so the scenario types track the engine's
/// without mirroring them.
fn variant<T: std::fmt::Debug, S: Serializer>(value: &T, s: S) -> Result<S::Ok, S::Error> {
    s.collect_str(&format_args!("{value:?}"))
}

const NEW_PLAYER: PlayerSetup = PlayerSetup {
    life: 20,
    hand: &[],
    battlefield: &[],
    graveyard: &[],
    exile: &[],
    library: &[],
};

/// The builder: `Scenario::new(name)` starts at turn 1's precombat main phase, player 0
/// active, both at 20 life with no cards anywhere; each method sets one part.
impl Scenario {
    pub const fn new(name: &'static str) -> Scenario {
        Scenario {
            name,
            setup: Setup {
                seed: 0,
                turn: 1,
                active: P0,
                step: TurnStep::PrecombatMain,
                players: [NEW_PLAYER; 2],
            },
            actions: &[],
        }
    }

    #[must_use]
    pub const fn seed(mut self, seed: u64) -> Scenario {
        self.setup.seed = seed;
        self
    }

    /// Starts at `step` of turn `turn`, with `active` the active player.
    #[must_use]
    pub const fn at(mut self, turn: u16, active: u8, step: TurnStep) -> Scenario {
        self.setup.turn = turn;
        self.setup.active = active;
        self.setup.step = step;
        self
    }

    #[must_use]
    pub const fn life(mut self, player: u8, life: i16) -> Scenario {
        self.setup.players[player as usize].life = life;
        self
    }

    #[must_use]
    pub const fn hand(mut self, player: u8, cards: &'static [&'static str]) -> Scenario {
        self.setup.players[player as usize].hand = cards;
        self
    }

    #[must_use]
    pub const fn battlefield(mut self, player: u8, cards: &'static [Permanent]) -> Scenario {
        self.setup.players[player as usize].battlefield = cards;
        self
    }

    /// The graveyard, top first.
    #[must_use]
    pub const fn graveyard(mut self, player: u8, cards: &'static [&'static str]) -> Scenario {
        self.setup.players[player as usize].graveyard = cards;
        self
    }

    #[must_use]
    pub const fn exile(mut self, player: u8, cards: &'static [&'static str]) -> Scenario {
        self.setup.players[player as usize].exile = cards;
        self
    }

    /// The whole library, top first.
    #[must_use]
    pub const fn library(mut self, player: u8, cards: &'static [&'static str]) -> Scenario {
        self.setup.players[player as usize].library = cards;
        self
    }

    #[must_use]
    pub const fn actions(mut self, actions: &'static [Action]) -> Scenario {
        self.actions = actions;
        self
    }
}

/// One scenario as the dump writes it: the card it belongs to, and the scenario.
#[derive(Serialize)]
struct Entry<'a> {
    card: &'a str,
    #[serde(flatten)]
    scenario: &'a Scenario,
}

/// The scenarios of every card, as a JSON array of `{card, name, setup, actions}` objects:
/// what `beryl_scenario dump` writes for the oracle exporters (§17.2).
pub fn to_json(cards: &[(&str, &[Scenario])]) -> String {
    let entries: Vec<Entry> = cards
        .iter()
        .flat_map(|&(card, scenarios)| {
            scenarios
                .iter()
                .map(move |scenario| Entry { card, scenario })
        })
        .collect();
    serde_json::to_string_pretty(&entries).expect("scenarios always serialize")
}
