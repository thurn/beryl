//! Scenarios: black-box tests of the rules, one card at a time (architecture §17.1).
//!
//! A [`Scenario`] is a value: a [`Setup`] (where each card is, life, the turn and step), then
//! [`Action`]s, each a scripted decision or an [`Assertion`]. Card files declare them as
//! constants with the `const` builder below. [`run`] plays one through the engine's public
//! decision API under the exact profile, and is strict: every decision the engine offers must
//! be scripted, so the decision surface itself stays under test. [`to_json`] serializes
//! scenarios for the oracle exporters (§17.2), each with its [`trace`].
//!
//! The actions and assertions are the ones the engine's mechanics support so far.

mod exhaust;
mod run;
mod script;
mod setup;
#[cfg(test)]
mod tests;

use beryl_engine::card::{Castings, FaceIx, ManaColor, ObjectAbility};
use beryl_engine::state::CounterKind;
use beryl_engine::{CardTable, DecisionKind, Seat, TurnStep, Zone};
use serde::{Serialize, Serializer};

pub use exhaust::{Bounds, Check, Found, Searched, exhaust, invariants};
pub use run::{Live, answer, describe, run, trace, trace_with, visit, watch};
pub use script::{script, setup_source, source};
pub use setup::{Slots, position};

/// Why a scenario failed: at which action, and what went wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub scenario: &'static str,
    /// The index of the failing action, or `None` for the setup.
    pub action: Option<usize>,
    pub message: String,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.action {
            Some(i) => write!(f, "{}: action {i}: {}", self.scenario, self.message),
            None => write!(f, "{}: setup: {}", self.scenario, self.message),
        }
    }
}

impl std::error::Error for Failure {}

/// The players, in seat order.
pub const P0: Seat = Seat::P0;
pub const P1: Seat = Seat::P1;

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
    #[serde(serialize_with = "seat")]
    pub active: Seat,
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
/// since its controller's turn began. A planeswalker has the printed loyalty of its face up
/// unless `counters` says otherwise.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Permanent {
    pub card: &'static str,
    pub tapped: bool,
    /// The face up: the back face for a double-faced card that entered transformed.
    #[serde(serialize_with = "face_ix")]
    pub face: FaceIx,
    pub counters: Option<Counters>,
    /// Cards its owner owns in exile "exiled with" it, as its own ability would have exiled
    /// them (CR 607.2a).
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    pub exiled_with: &'static [&'static str],
    /// A token, not a card (CR 111.1): one a card of the setup creates.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub token: bool,
    /// It came under its controller's control this turn, so it can't attack or use its tap
    /// abilities yet ([CR 302.6](https://yawgatog.com/resources/magic-rules/#R3026)).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub sick: bool,
}

/// Counters of one kind on a permanent at the start.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Counters {
    #[serde(serialize_with = "variant")]
    pub kind: CounterKind,
    pub count: u8,
}

impl Permanent {
    pub const fn new(card: &'static str) -> Permanent {
        Permanent {
            card,
            tapped: false,
            face: FaceIx::FRONT,
            counters: None,
            exiled_with: &[],
            token: false,
            sick: false,
        }
    }

    /// A token named `card`, the name its definition gives it, which some card of the setup
    /// creates, so the game has its definition
    /// ([CR 111.1](https://yawgatog.com/resources/magic-rules/#R1111)): "A token is a marker
    /// used to represent any permanent that isn't represented by a card."
    pub const fn token(card: &'static str) -> Permanent {
        Permanent {
            token: true,
            ..Permanent::new(card)
        }
    }

    /// Summoning sick ([CR 302.6](https://yawgatog.com/resources/magic-rules/#R3026)): "A
    /// creature's activated ability with the tap symbol or the untap symbol in its activation
    /// cost can't be activated unless the creature has been under its controller's control
    /// continuously since their most recent turn began. A creature can't attack unless it has
    /// been under its controller's control continuously since their most recent turn began."
    #[must_use]
    pub const fn sick(self) -> Permanent {
        Permanent { sick: true, ..self }
    }

    /// With `cards` in exile "exiled with" it.
    #[must_use]
    pub const fn exiled_with(self, cards: &'static [&'static str]) -> Permanent {
        Permanent {
            exiled_with: cards,
            ..self
        }
    }

    /// With its back face up, as a double-faced card put onto the battlefield transformed.
    #[must_use]
    pub const fn transformed(self) -> Permanent {
        Permanent {
            face: FaceIx::BACK,
            ..self
        }
    }

    /// Face down, as a manifested permanent (CR 701.40a): a 2/2 creature with no name or
    /// text, which only its controller knows.
    #[must_use]
    pub const fn face_down(self) -> Permanent {
        Permanent {
            face: FaceIx::DOWN,
            ..self
        }
    }

    /// With `count` counters of `kind`.
    #[must_use]
    pub const fn counters(self, kind: CounterKind, count: u8) -> Permanent {
        Permanent {
            counters: Some(Counters { kind, count }),
            ..self
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
    PlayLand {
        #[serde(serialize_with = "seat")]
        player: Seat,
        card: &'static str,
    },
    /// `player` begins casting the card `card` at a priority decision, with face `face` up
    /// and the casting options `costs` paid.
    /// The casting's own questions (X, targets, payment) follow as decisions.
    Cast {
        #[serde(serialize_with = "seat")]
        player: Seat,
        card: &'static str,
        #[serde(serialize_with = "face_ix")]
        face: FaceIx,
        #[serde(serialize_with = "castings")]
        costs: Castings,
    },
    /// `player` begins activating the ability `ability` of the permanent `card` at a priority
    /// decision.
    Activate {
        #[serde(serialize_with = "seat")]
        player: Seat,
        card: &'static str,
        #[serde(serialize_with = "ability_ix")]
        ability: ObjectAbility,
    },
    /// `player` turns the face-down permanent `card` face up at a priority decision, a
    /// special action (CR 116.2b). Its payment's questions follow as decisions.
    TurnFaceUp {
        #[serde(serialize_with = "seat")]
        player: Seat,
        card: &'static str,
    },
    /// Players pass priority until the top of the stack resolves, or until the next decision
    /// is not a priority decision (a question its resolution asks). The game then stops at
    /// the active player's next priority, even where passing is all they can do.
    Resolve,
    /// `player` answers the open decision, which must be of this kind.
    Answer {
        #[serde(serialize_with = "seat")]
        player: Seat,
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
    pub const fn answer(player: Seat, kind: DecisionKind, pick: Pick) -> Action {
        Action::Answer { player, kind, pick }
    }

    /// `player` passes priority.
    pub const fn pass(player: Seat) -> Action {
        Action::answer(player, DecisionKind::Priority, Pick::Pass)
    }

    /// `player` casts `card`'s front face at priority, with none of its casting options.
    pub const fn cast(player: Seat, card: &'static str) -> Action {
        Action::Cast {
            player,
            card,
            face: FaceIx::FRONT,
            costs: Castings::NONE,
        }
    }

    /// `player` activates ability `ability` (an index into the face's abilities) of `card`
    /// at priority.
    pub const fn activate(player: Seat, card: &'static str, ability: u8) -> Action {
        Action::Activate {
            player,
            card,
            ability: ObjectAbility::own(ability),
        }
    }

    /// `player` activates the `k`th ability other objects grant `card`, at priority.
    pub const fn activate_granted(player: Seat, card: &'static str, k: u8) -> Action {
        Action::Activate {
            player,
            card,
            ability: ObjectAbility::Granted(k),
        }
    }

    /// `player` chooses the card named `card` for a decision of `kind`.
    pub const fn choose(player: Seat, kind: DecisionKind, card: &'static str) -> Action {
        Action::answer(player, kind, Pick::Card { card })
    }

    /// `assertion` must hold.
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
        #[serde(serialize_with = "seat")]
        player: Seat,
    },
    /// The defending player, as what an attacker attacks.
    DefendingPlayer,
    /// The `i`th payment plan.
    Plan {
        i: u16,
    },
}

/// A fact about the current position.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(tag = "assert")]
pub enum Assertion {
    Life {
        #[serde(serialize_with = "seat")]
        player: Seat,
        life: i16,
    },
    /// How many cards `player` owns in `zone`: all of them, or those named `card`.
    Count {
        #[serde(serialize_with = "seat")]
        player: Seat,
        #[serde(serialize_with = "variant")]
        zone: Zone,
        card: Option<&'static str>,
        count: u8,
    },
    /// How many of the permanents named `card` that `player` controls are tapped.
    Tapped {
        #[serde(serialize_with = "seat")]
        player: Seat,
        card: &'static str,
        count: u8,
    },
    /// The open decision: who decides, what, and among how many candidates.
    Decision {
        #[serde(serialize_with = "seat")]
        player: Seat,
        #[serde(serialize_with = "variant")]
        kind: DecisionKind,
        candidates: u8,
    },
    /// Whether and how the game has ended.
    Outcome { outcome: Ending },
    /// How many permanents named `card` `player` controls.
    Controls {
        #[serde(serialize_with = "seat")]
        player: Seat,
        card: &'static str,
        count: u8,
    },
    /// How many of the permanents named `card` that `player` controls are face down. A
    /// face-down permanent has no name (CR 708.2a); the scenario names it by its card.
    FaceDown {
        #[serde(serialize_with = "seat")]
        player: Seat,
        card: &'static str,
        count: u8,
    },
    /// How many of the permanents named `card` that `player` controls are attacking.
    Attacking {
        #[serde(serialize_with = "seat")]
        player: Seat,
        card: &'static str,
        count: u8,
    },
    /// The power and toughness of the first permanent named `card` that `player` controls
    /// and that is a token, or a card.
    Pt {
        #[serde(serialize_with = "seat")]
        player: Seat,
        card: &'static str,
        token: bool,
        power: i8,
        toughness: i8,
    },
    /// Whether the first permanent named `card` that `player` controls is a creature.
    Creature {
        #[serde(serialize_with = "seat")]
        player: Seat,
        card: &'static str,
        creature: bool,
    },
    /// How many counters of `kind` the first permanent named `card` that `player` controls
    /// has.
    Counters {
        #[serde(serialize_with = "seat")]
        player: Seat,
        card: &'static str,
        #[serde(serialize_with = "variant")]
        kind: CounterKind,
        count: u8,
    },
    /// How many spells and abilities are on the stack.
    Stack { count: u8 },
    /// The top cards of `player`'s library, top first.
    Library {
        #[serde(serialize_with = "seat")]
        player: Seat,
        top: &'static [&'static str],
    },
    /// How much unspent mana of `color` `player` has.
    Mana {
        #[serde(serialize_with = "seat")]
        player: Seat,
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
    Win {
        #[serde(serialize_with = "seat")]
        player: Seat,
    },
    Draw,
}

/// Serializes an engine enum as its variant name, so the scenario types track the engine's
/// without mirroring them.
fn variant<T: std::fmt::Debug, S: Serializer>(value: &T, s: S) -> Result<S::Ok, S::Error> {
    s.collect_str(&format_args!("{value:?}"))
}

/// An ability of a permanent, as `{"Own": i}` or `{"Granted": k}`.
#[allow(clippy::trivially_copy_pass_by_ref)] // the signature `serialize_with` calls
fn ability_ix<S: Serializer>(ability: &ObjectAbility, s: S) -> Result<S::Ok, S::Error> {
    match *ability {
        ObjectAbility::Own(i) => s.serialize_newtype_variant("ObjectAbility", 0, "Own", &i.get()),
        ObjectAbility::Granted(k) => s.serialize_newtype_variant("ObjectAbility", 1, "Granted", &k),
    }
}

/// A face, as its index among the card's faces.
#[allow(clippy::trivially_copy_pass_by_ref)] // the signature `serialize_with` calls
fn face_ix<S: Serializer>(face: &FaceIx, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_u8(face.get())
}

/// Casting options, as a bitmask over the face's list.
#[allow(clippy::trivially_copy_pass_by_ref)] // the signature `serialize_with` calls
fn castings<S: Serializer>(costs: &Castings, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_u8(costs.bits())
}

/// A player, as their seat number.
#[allow(clippy::trivially_copy_pass_by_ref)] // the signature `serialize_with` calls
fn seat<S: Serializer>(player: &Seat, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_u8(player.get())
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
    pub const fn at(mut self, turn: u16, active: Seat, step: TurnStep) -> Scenario {
        self.setup.turn = turn;
        self.setup.active = active;
        self.setup.step = step;
        self
    }

    #[must_use]
    pub const fn life(mut self, player: Seat, life: i16) -> Scenario {
        self.setup.players[player.index()].life = life;
        self
    }

    #[must_use]
    pub const fn hand(mut self, player: Seat, cards: &'static [&'static str]) -> Scenario {
        self.setup.players[player.index()].hand = cards;
        self
    }

    #[must_use]
    pub const fn battlefield(mut self, player: Seat, cards: &'static [Permanent]) -> Scenario {
        self.setup.players[player.index()].battlefield = cards;
        self
    }

    /// The graveyard, top first.
    #[must_use]
    pub const fn graveyard(mut self, player: Seat, cards: &'static [&'static str]) -> Scenario {
        self.setup.players[player.index()].graveyard = cards;
        self
    }

    #[must_use]
    pub const fn exile(mut self, player: Seat, cards: &'static [&'static str]) -> Scenario {
        self.setup.players[player.index()].exile = cards;
        self
    }

    /// The whole library, top first.
    #[must_use]
    pub const fn library(mut self, player: Seat, cards: &'static [&'static str]) -> Scenario {
        self.setup.players[player.index()].library = cards;
        self
    }

    #[must_use]
    pub const fn actions(mut self, actions: &'static [Action]) -> Scenario {
        self.actions = actions;
        self
    }
}

/// Where the game stood when an action began, as Beryl played it: the turn, the step, the
/// open decision and the stack, with the coin flips, taps and forced answers on the way. Oracles
/// script by turn and step rather than by decision, so the exporters place each action with
/// this (§17.2).
#[derive(Clone, Debug, Serialize)]
pub struct Position {
    pub turn: u16,
    #[serde(serialize_with = "variant")]
    pub step: TurnStep,
    #[serde(serialize_with = "seat")]
    pub active: Seat,
    /// Who decides at the open decision, and what; both `None` once the game is over.
    #[serde(serialize_with = "optional_seat")]
    pub decider: Option<Seat>,
    #[serde(serialize_with = "optional_variant")]
    pub kind: Option<DecisionKind>,
    /// Bottom first.
    pub stack: Vec<StackItem>,
    /// The coin flips since the previous action began, in order, so that an oracle can
    /// replay Beryl's seeded results with its own.
    pub flips: Vec<CoinFlip>,
    /// The permanents that became tapped since the previous action began, by name: among
    /// them the mana sources a payment tapped, which an oracle may choose differently.
    pub taps: Vec<&'static str>,
    /// The questions with one answer since the previous action began, which Beryl answers
    /// without asking and an oracle may ask.
    pub forced: Vec<Forced>,
    /// The cards that have flashback here, by name, as a Snapcaster Mage grants it: an oracle
    /// casts them with their flashback ability, which a permission to cast a card from its
    /// graveyard is not.
    pub flashback: Vec<&'static str>,
    /// The face-down permanents here, by their cards' names: an oracle names them otherwise,
    /// as they have no name (CR 708.2a).
    pub face_down: Vec<&'static str>,
    /// How many creatures each player controls here, by seat: whether an effect that counts
    /// them, as overloaded Winds of Abandon's searches, does anything, where an oracle asks
    /// regardless of the count.
    pub creatures: [u8; 2],
}

/// A question with one answer, as the answer a scenario would script, and the turn and step
/// it came in.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Forced {
    pub turn: u16,
    #[serde(serialize_with = "variant")]
    pub step: TurnStep,
    #[serde(flatten)]
    pub answer: Action,
}

/// A coin flip ([CR 705.2](https://yawgatog.com/resources/magic-rules/#R7052)): "If the call
/// matches the result, the player wins the flip."
#[derive(Clone, Copy, Debug, Serialize)]
pub struct CoinFlip {
    #[serde(serialize_with = "seat")]
    pub player: Seat,
    pub won: bool,
}

/// A spell or ability on the stack.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct StackItem {
    /// The spell's name, or the name of the ability's source.
    pub name: &'static str,
    pub spell: bool,
    #[serde(serialize_with = "seat")]
    pub controller: Seat,
}

#[allow(clippy::ref_option, clippy::trivially_copy_pass_by_ref)] // as `serialize_with` calls
fn optional_seat<S: Serializer>(player: &Option<Seat>, s: S) -> Result<S::Ok, S::Error> {
    match player {
        Some(p) => s.serialize_some(&p.get()),
        None => s.serialize_none(),
    }
}

#[allow(clippy::ref_option)] // the signature `serialize_with` calls
fn optional_variant<T: std::fmt::Debug, S: Serializer>(
    value: &Option<T>,
    s: S,
) -> Result<S::Ok, S::Error> {
    match value {
        Some(v) => s.serialize_some(&format!("{v:?}")),
        None => s.serialize_none(),
    }
}

/// One scenario as the dump writes it: the card it belongs to, the scenario, and the position
/// each action began from, or why Beryl failed it.
#[derive(Serialize)]
struct Entry<'a> {
    card: &'a str,
    #[serde(flatten)]
    scenario: Scenario,
    /// The index of the first of the actions the dump added: the snapshot's assertions.
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshot: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    trace: Option<Vec<Position>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure: Option<String>,
}

/// The scenarios of every card, each played on `table`, as a JSON array of `{card, name,
/// setup, actions, snapshot, trace}` objects (`failure` in place of `trace` for a scenario
/// Beryl fails): what `beryl_scenario dump` writes for the oracle exporters (§17.2). Each
/// scenario's actions end with the assertions of its final [`Live::snapshot`], so that an
/// oracle compares the whole public position it ends in, not only what its author asserted.
pub fn to_json(table: &'static CardTable, cards: &[(&str, &[Scenario])]) -> String {
    let entries: Vec<Entry> = cards
        .iter()
        .flat_map(|&(card, scenarios)| {
            scenarios.iter().map(move |scenario| {
                let (scenario, snapshot) = with_snapshot(table, scenario);
                let (trace, failure) = match trace(table, &scenario) {
                    Ok(t) => (Some(t), None),
                    Err(e) => (None, Some(e.to_string())),
                };
                Entry {
                    card,
                    scenario,
                    snapshot,
                    trace,
                    failure,
                }
            })
        })
        .collect();
    serde_json::to_string_pretty(&entries).expect("scenarios always serialize")
}

/// `scenario` with the assertions of its final snapshot after its own actions, and the index
/// of the first; unchanged where Beryl fails it, which [`trace`] then reports. The longer list
/// of actions is leaked: the dump is written once.
fn with_snapshot(table: &'static CardTable, scenario: &Scenario) -> (Scenario, Option<usize>) {
    let snapshot = || -> Result<Vec<Assertion>, String> {
        let mut live = Live::new(table, scenario).map_err(|f| f.message)?;
        // Assertions only check: the positions the others begin from are the same without.
        for &action in scenario.actions {
            if !matches!(action, Action::Assert { .. }) {
                live.act(action)?;
            }
        }
        live.snapshot()
    };
    match snapshot() {
        Ok(facts) if !facts.is_empty() => {
            let actions: Vec<Action> = scenario
                .actions
                .iter()
                .copied()
                .chain(facts.into_iter().map(Action::assert))
                .collect();
            let extended = Scenario {
                actions: Vec::leak(actions),
                ..*scenario
            };
            (extended, Some(scenario.actions.len()))
        }
        _ => (*scenario, None),
    }
}
