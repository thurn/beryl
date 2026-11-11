//! Events: what a game action did, as the rules match triggers against it and as the UI or a
//! replay reads it (architecture §8.1, §8.2, decision D10), and the [`Sink`] executed events
//! go to. The pipeline that executes an event (replacements, the layer refresh, trigger
//! matching) is the rules', in `beryl_engine`'s `event` module.

use beryl_card::{Castings, CounterKind, FaceAbility, FaceIx, ManaColor, TurnStep, Zone};

use crate::bitboard::Bitboard;
use crate::chance::Chance;
use crate::ids::Seat;
use crate::layout::{Layout, Small};
use crate::object::ObjectRef;
use crate::relayout::Relayout;
use crate::stack::{StackEntry, TARGETS, Target};

/// Why an object changed zones. Triggers and replacement effects read it ("dies", "is put
/// into a graveyard from anywhere", "is discarded").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Reason {
    /// Played as a land (CR 305.1).
    Play,
    /// Drawn (CR 121.1).
    Draw,
    /// Discarded (CR 701.9a).
    Discard,
    /// Destroyed (CR 701.8a).
    Destroy,
    /// Sacrificed (CR 701.21a).
    Sacrifice,
    /// A state-based action (CR 704).
    StateBased,
    /// Exiled by an effect (CR 701.13a).
    Exile,
    /// A token created on the battlefield (CR 111.1).
    Created,
    /// Returned when an "until" exile ends (CR 610.3).
    Return,
    /// Any other effect that puts an object into a zone.
    Effect,
    /// Put on the stack as it is cast (CR 601.2a).
    Cast,
    /// Countered ([CR 701.6a](https://yawgatog.com/resources/magic-rules/#R7016a)).
    Counter,
    /// A spell resolving or failing to: a permanent spell entering the battlefield, an
    /// instant or sorcery put into its owner's graveyard (CR 608.2n, 608.3).
    Resolve,
}

/// The player or permanent dealt damage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Recipient<L: Layout = Small> {
    Player(Seat),
    Permanent(ObjectRef<L>),
}

/// Why a player lost the game
/// ([CR 104.3](https://yawgatog.com/resources/magic-rules/#R1043)): "There are several ways
/// to lose the game."
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LossCause<L: Layout = Small> {
    /// [CR 704.5a](https://yawgatog.com/resources/magic-rules/#R7045a): "If a player has 0 or
    /// less life, that player loses the game." What lowered it is in the events before.
    Life,
    /// [CR 704.5b](https://yawgatog.com/resources/magic-rules/#R7045b): "If a player attempted
    /// to draw a card from a library with no cards in it since the last time state-based
    /// actions were checked, that player loses the game."
    Library,
    /// An effect had their opponent win
    /// ([CR 104.2b](https://yawgatog.com/resources/magic-rules/#R1042b): "An effect may state
    /// that a player wins the game") or had them lose
    /// ([CR 104.3e](https://yawgatog.com/resources/magic-rules/#R1043e): "An effect may state
    /// that a player loses the game"): the ability of `face` of `card` that did, or its spell
    /// when `ability` is none, and the object that had it, if any.
    Effect {
        source: Option<ObjectRef<L>>,
        card: L::Card,
        face: FaceIx,
        ability: Option<FaceAbility>,
    },
    /// [CR 104.3a](https://yawgatog.com/resources/magic-rules/#R1043a): "A player can concede
    /// the game at any time. A player who concedes leaves the game immediately. That player
    /// loses the game."
    Concede,
}

/// One thing that happened (architecture §8.1). Simultaneous happenings that rules read as
/// one carry a set: one `TokensCreated` for every token an effect creates, one `Attack` for
/// every attacker declared. Flag writes and cache updates never go through events.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Event<L: Layout = Small> {
    /// An object changed zones; `object` is the new object
    /// ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)).
    ZoneChange {
        object: ObjectRef<L>,
        from: Zone,
        to: Zone,
        reason: Reason,
        /// A public spell moved to the top (`true`) or bottom (`false`) of its owner's library.
        library_top: Option<bool>,
    },
    Damage {
        source: ObjectRef<L>,
        to: Recipient<L>,
        amount: u8,
        combat: bool,
    },
    Draw {
        player: Seat,
        cards: u8,
    },
    /// A public slot taken from the library, in draw order. Kept only for presentation.
    DrawCard {
        slot: L::Slot,
    },
    /// The public result of a scry, without the identities or order of the cards.
    Scry {
        player: Seat,
        top: u8,
        bottom: u8,
    },
    LibraryShuffled {
        player: Seat,
    },
    LifeGain {
        player: Seat,
        amount: u8,
    },
    LifeLoss {
        player: Seat,
        amount: u8,
    },
    CountersAdded {
        object: ObjectRef<L>,
        kind: CounterKind,
        amount: u8,
    },
    CountersRemoved {
        object: ObjectRef<L>,
        kind: CounterKind,
        amount: u8,
    },
    Tap {
        object: ObjectRef<L>,
    },
    Untap {
        object: ObjectRef<L>,
    },
    /// Mana was added to a player's mana pool
    /// ([CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064)): "When an effect
    /// instructs a player to add mana, that mana goes into a player's mana pool." For the UI,
    /// which shows it flow from `source` (the mana ability's permanent, or the spell or
    /// ability that added it) to what it pays for.
    Mana {
        player: Seat,
        source: Option<ObjectRef<L>>,
        color: ManaColor,
        amount: u8,
    },
    /// Mana paid during resolution ([CR 608.2g](https://yawgatog.com/resources/magic-rules/#R6082g)):
    /// "If an effect gives a player the option to pay mana, they may activate mana abilities".
    /// Names the resolving item so its payment can be shown before it leaves the stack.
    ResolutionPayment {
        player: Seat,
        item: StackEntry<L>,
    },
    /// An ability left the stack at the end of resolution
    /// ([CR 608.2n](https://yawgatog.com/resources/magic-rules/#R6082n)): "the
    /// ability is removed from the stack and ceases to exist." Playback needs
    /// this boundary even when the next trigger arrives in the same view.
    AbilityFinished,
    /// "Whenever you create one or more tokens" reads this once per event.
    TokensCreated {
        controller: Seat,
        tokens: Bitboard<L::Slot>,
    },
    /// A spell was cast ([CR 601.2i](https://yawgatog.com/resources/magic-rules/#R6012i)), as
    /// the stack then showed it, for the UI: an opponent's spell that resolves unanswered has
    /// left the stack by the time anyone looks.
    Cast {
        spell: ObjectRef<L>,
        controller: Seat,
        targets: L::Queue<Target<L>, TARGETS>,
        x: u8,
        costs: Castings,
        flashback: bool,
    },
    /// An ability was put on the stack, for the UI's log: an activated one as it becomes
    /// activated, its costs paid
    /// ([CR 602.2b](https://yawgatog.com/resources/magic-rules/#R6022b): "The remainder of the
    /// process for activating an ability is identical to the process for casting a spell
    /// listed in rules 601.2b-i"), or a triggered one once its targets are chosen
    /// ([CR 603.3d](https://yawgatog.com/resources/magic-rules/#R6033d)). A triggered ability
    /// without targets goes unsaid: what it does says it. The ability as it stood on the
    /// stack then, which it may have left by the time anyone looks.
    Ability {
        source: Option<ObjectRef<L>>,
        controller: Seat,
        card: L::Card,
        face: FaceIx,
        ability: FaceAbility,
        targets: L::Queue<Target<L>, TARGETS>,
        x: u8,
    },
    /// A player declared attackers ([CR 508.1](https://yawgatog.com/resources/magic-rules/#R5081)).
    Attack {
        player: Seat,
        attackers: Bitboard<L::Slot>,
    },
    /// A step or phase began ([CR 500.6](https://yawgatog.com/resources/magic-rules/#R5006)).
    StepBegins {
        step: TurnStep,
        active: Seat,
    },
    GainControl {
        object: ObjectRef<L>,
        player: Seat,
    },
    /// A coin flip ([CR 705.2](https://yawgatog.com/resources/magic-rules/#R7052)): "If the
    /// call matches the result, the player wins the flip."
    CoinFlip {
        player: Seat,
        won: bool,
        /// The object whose ability flipped it.
        source: ObjectRef<L>,
    },
    Lose {
        player: Seat,
        cause: LossCause<L>,
    },
    Win {
        player: Seat,
    },
}

/// One bit per kind of event a triggered ability can listen to, for the per-card masks of
/// the game table (§6.4): a kind no card listens to costs one AND.
pub mod kind {
    pub const ENTERS: u16 = 1 << 0;
    pub const ATTACK: u16 = 1 << 1;
    pub const DAMAGE: u16 = 1 << 2;
    pub const STEP: u16 = 1 << 3;
    pub const TOKENS: u16 = 1 << 4;
    pub const CAST: u16 = 1 << 5;
    pub const DIES: u16 = 1 << 6;
    pub const DISCARD: u16 = 1 << 7;
    pub const DRAW: u16 = 1 << 8;
    /// No kind of event, and no [`super::Event`] has it: the bit of a card's mask
    /// ([`crate::GameTable`]) that says a static ability of it changes a rule for players
    /// ([`beryl_card::Rule::whose`]). The layers read each permanent's mask once, for what
    /// listens, and so learn in the same read whether any permanent changes a rule
    /// ([`crate::derive::Rederive::finish`]).
    pub const RULE_CHANGE: u16 = 1 << 15;
}

impl Copy for Event<Small> {}

impl<L: Layout> Event<L> {
    /// The listener kind of this event, or 0 if no trigger listens to it.
    pub fn kind(&self) -> u16 {
        match self {
            Event::ZoneChange {
                to: Zone::Battlefield,
                ..
            } => kind::ENTERS,
            Event::ZoneChange {
                reason: Reason::Discard,
                ..
            } => kind::DISCARD,
            Event::Attack { .. } => kind::ATTACK,
            Event::Damage { .. } => kind::DAMAGE,
            Event::StepBegins { .. } => kind::STEP,
            Event::TokensCreated { .. } => kind::TOKENS,
            Event::Cast { .. } => kind::CAST,
            Event::Draw { .. } => kind::DRAW,
            _ => 0,
        }
    }
}

/// Where executed events go for the UI or a replay (§8.2, step 6). Search leaves it off.
#[derive(Debug, Default)]
pub struct Sink<L: Layout = Small> {
    events: Vec<Event<L>>,
    on: bool,
    /// The work done since the scratch last took the counts: the sink goes everywhere the
    /// engine does it.
    pub work: Work,
    /// The game's chance, lent by the session for the calls that play its game on: what the
    /// knowledge chokepoints and coin flips record or replay ([`crate::chance`]).
    pub chance: Option<Box<Chance>>,
    /// While a paused body runs again, the cards its earlier runs revealed that no player
    /// knew as the resolution began. The run starts from that knowledge, so it reveals them
    /// again; chance counts each as revealed already, once ([`crate::chance`]).
    pub revealed: Bitboard<L::Slot>,
}

/// Counts of the engine's work below `next` and `apply`, moved into the scratch's
/// `Counts` when they return.
#[derive(Clone, Copy, Debug, Default)]
pub struct Work {
    /// Events executed, kept or not.
    pub events: u64,
    pub layer_recomputes: u64,
    pub solver_runs: u64,
}

impl<L: Layout> Sink<L> {
    /// A sink that drops every event.
    pub fn off() -> Self {
        Self::default()
    }

    /// A sink that keeps every event until drained.
    pub fn recording() -> Self {
        Self {
            on: true,
            ..Self::default()
        }
    }

    /// Lends the sink the game's chance, or with none takes it back: from then on the calls
    /// given this sink record each random event's outcome, or replay a record
    /// ([`crate::chance`]).
    pub fn set_chance(&mut self, chance: Option<Chance>) {
        self.chance = chance.map(Box::new);
    }

    /// Takes back the chance lent to the sink, if any.
    pub fn take_chance(&mut self) -> Option<Chance> {
        self.chance.take().map(|c| *c)
    }

    /// The events since the last drain, oldest first.
    pub fn drain(&mut self) -> std::vec::Drain<'_, Event<L>> {
        self.events.drain(..)
    }

    pub fn events(&self) -> &[Event<L>] {
        &self.events
    }

    /// How many events it holds.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Takes the events after the first `len`, those of a body run replayed (§7.3).
    pub fn take_from(&mut self, len: usize) -> std::vec::Drain<'_, Event<L>> {
        self.events.drain(len..)
    }

    /// Drops the events after the first `len`: those of a body run discarded (§7.3), or of a
    /// game run on until a capacity of its layout ran out, to be run again in a wider one.
    pub fn truncate(&mut self, len: usize) {
        self.events.truncate(len);
    }

    /// The same events in layout `M`, recorded or not as this sink records them.
    pub fn relayout<M: Layout>(&self) -> Sink<M> {
        Sink {
            events: self.events.iter().map(Relayout::relayout).collect(),
            on: self.on,
            work: self.work,
            chance: self.chance.clone(),
            revealed: crate::relayout::bits(self.revealed),
        }
    }

    /// Keeps an event only the UI reads, which no rule acts on and so counts as no work of
    /// the pipeline's: search pays one branch for it.
    pub fn note(&mut self, e: Event<L>) {
        if self.on {
            self.events.push(e);
        }
    }

    pub fn push(&mut self, e: Event<L>) {
        self.work.events += 1;
        if self.on {
            self.events.push(e);
        }
    }
}
