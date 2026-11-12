//! The scenario value types: what a card file writes to test itself (architecture §17.1).
//!
//! A [`Scenario`] is a value: a [`Setup`] (where each card is, life, the turn and step), then
//! [`Action`]s, each a scripted decision or an [`Assertion`]. Card files declare them as
//! constants with the `const` builder below.
//!
//! This crate depends on the vocabulary (`beryl_card`) and nothing else, so a rules or kernel
//! edit rebuilds no card crate, even with its scenarios compiled in (`docs/project-50k.md`,
//! Crates: "Data-only card crates depend on `beryl_card` alone"). The runner that plays a
//! scenario through the engine and the JSON dump the oracle exporters read are
//! `beryl_scenario`'s, which re-exports every type here; it serializes them from outside, so
//! no serializer is a dependency of a card crate either.
//!
//! The actions and assertions are the ones the engine's mechanics support so far.

use beryl_card::{
    Castings, CounterKind, DecisionKind, FaceIx, ManaColor, ObjectAbility, Seat, TurnStep, Zone,
};

/// The players, in seat order.
pub const P0: Seat = Seat::P0;
pub const P1: Seat = Seat::P1;

/// One scenario: a position, then what happens and what must be true.
#[derive(Clone, Copy, Debug)]
pub struct Scenario {
    pub name: &'static str,
    pub setup: Setup,
    pub actions: &'static [Action],
    /// The other cards this scenario is about, when it shows how particular cards interact
    /// ([`Scenario::interaction`]). Any other scenario names only its own card, that card's
    /// tokens, basic lands and the designated props, so one card's change cannot break
    /// another card's tests (plan, Verification). The oracles don't read it.
    pub interaction: &'static [&'static str],
}

/// Where the game starts. Every card named belongs to the player whose list names it; the
/// decks are exactly those cards.
#[derive(Clone, Copy, Debug)]
pub struct Setup {
    /// The game's seed, for its shuffles and other randomness.
    pub seed: u64,
    /// The turn number, from 1. The active player takes the odd turns.
    pub turn: u16,
    pub active: Seat,
    /// The step the game starts in, with the active player holding priority and the step's
    /// turn-based actions already done.
    pub step: TurnStep,
    pub players: [PlayerSetup; 2],
}

/// One player's part of the setup.
#[derive(Clone, Copy, Debug)]
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
#[derive(Clone, Copy, Debug)]
pub struct Permanent {
    pub card: &'static str,
    pub tapped: bool,
    /// The face up: the back face for a double-faced card that entered transformed.
    pub face: FaceIx,
    pub counters: Option<Counters>,
    /// Cards its owner owns in exile "exiled with" it, as its own ability would have exiled
    /// them (CR 607.2a).
    pub exiled_with: &'static [&'static str],
    /// A token, not a card (CR 111.1): one a card of the setup creates.
    pub token: bool,
    /// It came under its controller's control this turn, so it can't attack or use its tap
    /// abilities yet ([CR 302.6](https://yawgatog.com/resources/magic-rules/#R3026)).
    pub sick: bool,
}

/// Counters of one kind on a permanent at the start.
#[derive(Clone, Copy, Debug)]
pub struct Counters {
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
#[derive(Clone, Copy, Debug)]
pub enum Action {
    /// `player` plays the land card `card` from their hand, at a priority decision.
    PlayLand { player: Seat, card: &'static str },
    /// `player` begins casting the card `card` at a priority decision, with face `face` up
    /// and the casting options `costs` paid.
    /// The casting's own questions (X, targets, payment) follow as decisions.
    Cast {
        player: Seat,
        card: &'static str,
        face: FaceIx,
        costs: Castings,
    },
    /// `player` begins activating the ability `ability` of the permanent `card` at a priority
    /// decision.
    Activate {
        player: Seat,
        card: &'static str,
        ability: ObjectAbility,
    },
    /// `player` turns the face-down permanent `card` face up at a priority decision, a
    /// special action (CR 116.2b). Its payment's questions follow as decisions.
    TurnFaceUp { player: Seat, card: &'static str },
    /// Players pass priority until the top of the stack resolves, or until the next decision
    /// is not a priority decision (a question its resolution asks). The game then stops at
    /// the active player's next priority, even where passing is all they can do.
    Resolve,
    /// `player` answers the open decision, which must be of this kind.
    Answer {
        player: Seat,
        kind: DecisionKind,
        pick: Pick,
    },
    /// Every player passes priority until the game reaches this turn and step, where it
    /// stops with the active player holding priority. Any other decision on the way is
    /// unscripted.
    AdvanceTo { turn: u16, step: TurnStep },
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
#[derive(Clone, Copy, Debug)]
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
#[derive(Clone, Copy, Debug)]
pub enum Assertion {
    Life {
        player: Seat,
        life: i16,
    },
    /// How many cards `player` owns in `zone`: all of them, or those named `card`.
    Count {
        player: Seat,
        zone: Zone,
        card: Option<&'static str>,
        count: u8,
    },
    /// How many of the permanents named `card` that `player` controls are tapped.
    Tapped {
        player: Seat,
        card: &'static str,
        count: u8,
    },
    /// The open decision: who decides, what, and among how many candidates.
    Decision {
        player: Seat,
        kind: DecisionKind,
        candidates: u8,
    },
    /// Whether and how the game has ended.
    Outcome {
        outcome: Ending,
    },
    /// How many permanents named `card` `player` controls.
    Controls {
        player: Seat,
        card: &'static str,
        count: u8,
    },
    /// How many of the permanents named `card` that `player` controls are face down. A
    /// face-down permanent has no name (CR 708.2a); the scenario names it by its card.
    FaceDown {
        player: Seat,
        card: &'static str,
        count: u8,
    },
    /// How many of the permanents named `card` that `player` controls are attacking.
    Attacking {
        player: Seat,
        card: &'static str,
        count: u8,
    },
    /// The power and toughness of the first permanent named `card` that `player` controls
    /// and that is a token, or a card.
    Pt {
        player: Seat,
        card: &'static str,
        token: bool,
        power: i8,
        toughness: i8,
    },
    /// Whether the first permanent named `card` that `player` controls is a creature.
    Creature {
        player: Seat,
        card: &'static str,
        creature: bool,
    },
    /// How many counters of `kind` the first permanent named `card` that `player` controls
    /// has.
    Counters {
        player: Seat,
        card: &'static str,
        kind: CounterKind,
        count: u8,
    },
    /// How many spells and abilities are on the stack.
    Stack {
        count: u8,
    },
    /// The top cards of `player`'s library, top first.
    Library {
        player: Seat,
        top: &'static [&'static str],
    },
    /// How much unspent mana of `color` `player` has.
    Mana {
        player: Seat,
        color: ManaColor,
        amount: u8,
    },
}

/// How a game stands, for [`Assertion::Outcome`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    InProgress,
    Win { player: Seat },
    Draw,
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
            interaction: &[],
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

    /// Marks this scenario as one about how its card interacts with `cards` in particular,
    /// which it may then name beside its own card, tokens, basic lands and props.
    #[must_use]
    pub const fn interaction(mut self, cards: &'static [&'static str]) -> Scenario {
        self.interaction = cards;
        self
    }
}
