//! Scenarios: black-box tests of the rules, one card at a time (architecture §17.1).
//!
//! A [`Scenario`] is a value: a [`Setup`] (where each card is, life, the turn and step), then
//! [`Action`]s, each a scripted decision or an [`Assertion`]. Card files declare them as
//! constants with the `const` builder below. [`run`] plays one through the engine's public
//! decision API under the exact profile, and is strict: every decision the engine offers must
//! be scripted, so the decision surface itself stays under test. [`to_json`] serializes
//! scenarios for the oracle exporters (§17.2).
//!
//! The actions and assertions are the ones the engine's mechanics support so far.

mod run;
#[cfg(test)]
mod tests;

use beryl_engine::card::ManaColor;
use beryl_engine::state::CounterKind;
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
    /// `player` begins casting the card `card` at a priority decision, with face `face` up
    /// and the casting options in the bitmask `costs` (over the face's casting list) paid.
    /// The casting's own questions (X, targets, payment) follow as decisions.
    Cast {
        player: u8,
        card: &'static str,
        face: u8,
        costs: u8,
    },
    /// `player` begins activating ability `ability` (an index into its face's abilities) of
    /// the permanent `card` at a priority decision.
    Activate {
        player: u8,
        card: &'static str,
        ability: u8,
    },
    /// Players pass priority until the top of the stack resolves, or until the next decision
    /// is not a priority decision (a question its resolution asks). The game then stops at
    /// the active player's next priority, even where passing is all they can do.
    Resolve,
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

/// Shorthands for the common actions.
impl Action {
    /// `player` answers a decision of `kind` with `pick`.
    pub const fn answer(player: u8, kind: DecisionKind, pick: Pick) -> Action {
        Action::Answer { player, kind, pick }
    }

    /// `player` passes priority.
    pub const fn pass(player: u8) -> Action {
        Action::answer(player, DecisionKind::Priority, Pick::Pass)
    }

    /// `player` casts `card`'s front face, with none of its casting options.
    pub const fn cast(player: u8, card: &'static str) -> Action {
        Action::Cast {
            player,
            card,
            face: 0,
            costs: 0,
        }
    }

    /// `player` activates ability `ability` of `card`.
    pub const fn activate(player: u8, card: &'static str, ability: u8) -> Action {
        Action::Activate {
            player,
            card,
            ability,
        }
    }

    pub const fn assert(assertion: Assertion) -> Action {
        Action::Assert { assertion }
    }
}

/// The answer to a decision.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(tag = "pick")]
pub enum Pick {
    /// Pass priority.
    Pass,
    /// The first candidate that names a card of this name: a candidate object, or an option
    /// naming an object (an attacker, a blocker, a waiting trigger's source, a mana source).
    Card {
        card: &'static str,
    },
    /// A number: X, or how many targets.
    Number {
        n: u8,
    },
    Yes,
    No,
    /// Finish a declaration or a division.
    Done,
    /// A player, as a target.
    Player {
        player: u8,
    },
    /// The defending player, as what an attacker attacks.
    DefendingPlayer,
    /// The `i`th payment plan.
    Plan {
        i: u8,
    },
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
    /// How many permanents named `card` `player` controls.
    Controls {
        player: u8,
        card: &'static str,
        count: u8,
    },
    /// How many of the permanents named `card` that `player` controls are attacking.
    Attacking {
        player: u8,
        card: &'static str,
        count: u8,
    },
    /// The power and toughness of the first permanent named `card` that `player` controls
    /// and that is a token, or a card.
    Pt {
        player: u8,
        card: &'static str,
        token: bool,
        power: i8,
        toughness: i8,
    },
    /// Whether the first permanent named `card` that `player` controls is a creature.
    Creature {
        player: u8,
        card: &'static str,
        creature: bool,
    },
    /// How many counters of `kind` the first permanent named `card` that `player` controls
    /// has.
    Counters {
        player: u8,
        card: &'static str,
        #[serde(serialize_with = "variant")]
        kind: CounterKind,
        count: u8,
    },
    /// How many spells and abilities are on the stack.
    Stack {
        count: u8,
    },
    /// The top cards of `player`'s library, top first.
    Library {
        player: u8,
        top: &'static [&'static str],
    },
    /// How much unspent mana of `color` `player` has.
    Mana {
        player: u8,
        #[serde(serialize_with = "variant")]
        color: ManaColor,
        amount: u8,
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
