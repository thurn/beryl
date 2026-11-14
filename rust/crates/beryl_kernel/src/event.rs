//! Events: what a game action did, as the rules match triggers against it and as the UI or a replay
//! reads it (architecture §8.1, §8.2, decision D10), and the [`Sink`] executed events go to, which
//! keeps beside each what it came from ([`crate::cause`]). The pipeline that executes an event
//! (replacements, the layer refresh, trigger matching) is the rules', in `beryl_engine`'s `event`
//! module.

use beryl_card::{Castings, CounterKind, FaceAbility, FaceIx, ManaColor, TurnStep, Zone};

use crate::bitboard::Bitboard;
use crate::cause::{Cause, Scopes, StackIx, Tag};
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
    /// Countered ([CR 701.6a]).
    ///
    /// [CR 701.6a]: https://yawgatog.com/resources/magic-rules/#R7016a
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

/// Why a player lost the game ([CR 104.3]): "There are several ways to lose the game."
///
/// [CR 104.3]: https://yawgatog.com/resources/magic-rules/#R1043
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LossCause<L: Layout = Small> {
    /// [CR 704.5a](https://yawgatog.com/resources/magic-rules/#R7045a): "If a player has 0 or less
    /// life, that player loses the game." What lowered it is in the events before.
    Life,
    /// [CR 704.5b](https://yawgatog.com/resources/magic-rules/#R7045b): "If a player attempted to
    /// draw a card from a library with no cards in it since the last time state-based actions were
    /// checked, that player loses the game."
    Library,
    /// An effect had their opponent win ([CR 104.2b]: "An effect may state that a player wins the
    /// game") or had them lose ([CR 104.3e]: "An effect may state that a player loses the game"):
    /// the ability of `face` of `card` that did, or its spell when `ability` is none, and the
    /// object that had it, if any.
    ///
    /// [CR 104.2b]: https://yawgatog.com/resources/magic-rules/#R1042b
    /// [CR 104.3e]: https://yawgatog.com/resources/magic-rules/#R1043e
    Effect {
        source: Option<ObjectRef<L>>,
        card: L::Card,
        face: FaceIx,
        ability: Option<FaceAbility>,
    },
    /// [CR 104.3a](https://yawgatog.com/resources/magic-rules/#R1043a): "A player can concede the
    /// game at any time. A player who concedes leaves the game immediately. That player loses the
    /// game."
    Concede,
}

/// One thing that happened (architecture §8.1). Simultaneous happenings that rules read as
/// one carry a set: one `TokensCreated` for every token an effect creates, one `Attack` for
/// every attacker declared. Flag writes and cache updates never go through events.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Event<L: Layout = Small> {
    /// An object changed zones; `object` is the new object ([CR 400.7]).
    ///
    /// [CR 400.7]: https://yawgatog.com/resources/magic-rules/#R4007
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
    /// A player put a card on the top (`top`) or the bottom of their library by their own
    /// choice ([CR 608.2d]): "the player announces these while applying the effect." The
    /// public result of the choice, without the card's identity.
    ///
    /// [CR 608.2d]: https://yawgatog.com/resources/magic-rules/#R6082d
    TopOrBottom {
        player: Seat,
        top: bool,
    },
    LibraryShuffled {
        player: Seat,
    },
    /// A card was revealed ([CR 701.20a]): "To reveal a card, show that card to all players for a
    /// brief time." For the UI, which holds it up to be read; no rule acts on it.
    ///
    /// [CR 701.20a]: https://yawgatog.com/resources/magic-rules/#R70120a
    Reveal {
        /// The player whose effect revealed the card.
        player: Seat,
        slot: L::Slot,
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
    /// Mana was added to a player's mana pool ([CR 106.4]): "When an effect instructs a player to
    /// add mana, that mana goes into a player's mana pool." For the UI, which shows it flow from
    /// `source` (the mana ability's permanent, or the spell or ability that added it) to what it
    /// pays for.
    ///
    /// [CR 106.4]: https://yawgatog.com/resources/magic-rules/#R1064
    Mana {
        player: Seat,
        source: Option<ObjectRef<L>>,
        color: ManaColor,
        amount: u8,
    },
    /// Mana paid during resolution ([CR 608.2g]): "If an effect gives a player the option to pay
    /// mana, they may activate mana abilities". Names the resolving item so its payment can be
    /// shown before it leaves the stack.
    ///
    /// [CR 608.2g]: https://yawgatog.com/resources/magic-rules/#R6082g
    ResolutionPayment {
        player: Seat,
        item: StackEntry<L>,
    },
    /// An ability left the stack at the end of resolution ([CR 608.2n]): "the ability is removed
    /// from the stack and ceases to exist." Playback needs this boundary even when the next trigger
    /// arrives in the same view.
    ///
    /// [CR 608.2n]: https://yawgatog.com/resources/magic-rules/#R6082n
    AbilityFinished,
    /// A spell or ability arrived on the stack, on top ([CR 405.2]: "Each time an object is put on
    /// the stack, it's put on top of all objects already there."), as it stood then: a spell or an
    /// activated ability before anything is announced for it (CR 601.2a, 602.2a), a triggered
    /// ability before its targets are chosen (CR 603.3d). For whoever follows the stack through the
    /// events ([`StackIx`]); no rule acts on it.
    ///
    /// [CR 405.2]: https://yawgatog.com/resources/magic-rules/#R4052
    StackArrival {
        entry: StackEntry<L>,
    },
    /// The spell or ability at `entry` left the stack: resolved or failed to (CR 608.2n, 608.3),
    /// countered ([CR 701.6a]: "To counter a spell or ability means to cancel it, removing it from
    /// the stack."), removed as a triggered ability with nothing legal to choose ([CR 603.3d]: "the
    /// ability is simply removed from the stack"), or its card moved elsewhere by an effect. A
    /// spell's card moves in an event of its own, before this one; a resolving spell whose own
    /// effect moved its card leaves only as its resolution ends ([CR 608.2m]: "If an instant spell,
    /// sorcery spell, or ability that can legally resolve leaves the stack once it starts to
    /// resolve, it will continue to resolve fully."). The entries above it fall by one place. For
    /// whoever follows the stack; no rule acts on it.
    ///
    /// [CR 701.6a]: https://yawgatog.com/resources/magic-rules/#R7016a
    /// [CR 603.3d]: https://yawgatog.com/resources/magic-rules/#R6033d
    /// [CR 608.2m]: https://yawgatog.com/resources/magic-rules/#R6082m
    StackDeparture {
        entry: StackIx,
    },
    /// "Whenever you create one or more tokens" reads this once per event.
    TokensCreated {
        controller: Seat,
        tokens: Bitboard<L::Slot>,
    },
    /// A spell was cast ([CR 601.2i]), as the stack then showed it, for the UI: an opponent's spell
    /// that resolves unanswered has left the stack by the time anyone looks.
    ///
    /// [CR 601.2i]: https://yawgatog.com/resources/magic-rules/#R6012i
    Cast {
        spell: ObjectRef<L>,
        controller: Seat,
        targets: L::Queue<Target<L>, TARGETS>,
        x: u8,
        costs: Castings,
        flashback: bool,
    },
    /// An ability was put on the stack, for the UI's log: an activated one as it becomes activated,
    /// its costs paid ([CR 602.2b]: "The remainder of the process for activating an ability is
    /// identical to the process for casting a spell listed in rules 601.2b-i"), or a triggered one
    /// once its targets are chosen ([CR 603.3d]), or at once if it has none. The ability as it
    /// stood on the stack then, which it may have left by the time anyone looks.
    ///
    /// [CR 602.2b]: https://yawgatog.com/resources/magic-rules/#R6022b
    /// [CR 603.3d]: https://yawgatog.com/resources/magic-rules/#R6033d
    Ability {
        source: Option<ObjectRef<L>>,
        controller: Seat,
        card: L::Card,
        face: FaceIx,
        ability: FaceAbility,
        targets: L::Queue<Target<L>, TARGETS>,
        x: u8,
    },
    /// A player declared attackers ([CR 508.1]).
    ///
    /// [CR 508.1]: https://yawgatog.com/resources/magic-rules/#R5081
    Attack {
        player: Seat,
        attackers: Bitboard<L::Slot>,
    },
    /// A creature was declared as a blocker for `attacker` ([CR 509.1a]: "The defending player
    /// chooses which creatures they control, if any, will block. ... For each of the chosen
    /// creatures, the defending player chooses one creature for it to block"), one event for each
    /// blocker of a declaration, together. For the UI, which draws the block; no rule acts on it
    /// yet, so it is only noted.
    ///
    /// [CR 509.1a]: https://yawgatog.com/resources/magic-rules/#R5091a
    Block {
        player: Seat,
        blocker: ObjectRef<L>,
        attacker: ObjectRef<L>,
    },
    /// A step or phase began ([CR 500.6]).
    ///
    /// [CR 500.6]: https://yawgatog.com/resources/magic-rules/#R5006
    StepBegins {
        step: TurnStep,
        active: Seat,
    },
    GainControl {
        object: ObjectRef<L>,
        player: Seat,
    },
    /// A coin flip ([CR 705.2]): "If the call matches the result, the player wins the flip."
    ///
    /// [CR 705.2]: https://yawgatog.com/resources/magic-rules/#R7052
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

/// A kind of event a triggered ability can listen to (§6.4). A card's [`Listens`] has one
/// bit for each.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum EventKind {
    Enters,
    Attack,
    Damage,
    Step,
    Tokens,
    Cast,
    Dies,
    Discard,
    Draw,
}

/// What a card's abilities may react to, as one mask for the game table (§6.4): the
/// [`EventKind`]s its triggered abilities listen to, and two marks that are no event: a
/// static ability that changes a rule for players, and one that replaces its controller's
/// draws. A kind no card listens to costs one AND, and the layers learn in the same read of
/// each permanent's mask whether any permanent changes a rule
/// ([`crate::derive::Rederive::finish`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Listens(u16);

impl Listens {
    pub const NONE: Listens = Listens(0);
    /// A static ability of it changes a rule for players
    /// ([`crate::rule_changes::RuleKind::of_players`]).
    pub const RULE_CHANGE: Listens = Listens(1 << 15);
    /// A static ability of it replaces its controller's draws (`beryl_card`'s
    /// `Replacement::WouldDraw`). With no such permanent on the battlefield a draw reads no
    /// ability ([`crate::State::listening`]).
    pub const DRAW_REPLACEMENT: Listens = Listens(1 << 14);

    /// The mask of one kind of event.
    #[inline]
    pub const fn event(kind: EventKind) -> Listens {
        Listens(1 << kind as u8)
    }

    /// Whether it listens to events of `kind`.
    #[inline]
    pub const fn hears(self, kind: EventKind) -> bool {
        self.0 & Listens::event(kind).0 != 0
    }

    /// Whether it has every bit of `other`: an event kind or a mark.
    #[inline]
    pub const fn contains(self, other: Listens) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether it has any bit of `other`.
    #[inline]
    pub const fn intersects(self, other: Listens) -> bool {
        self.0 & other.0 != 0
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl std::ops::BitOr for Listens {
    type Output = Listens;

    #[inline]
    fn bitor(self, other: Listens) -> Listens {
        Listens(self.0 | other.0)
    }
}

impl std::ops::BitOr<EventKind> for Listens {
    type Output = Listens;

    #[inline]
    fn bitor(self, kind: EventKind) -> Listens {
        self | Listens::event(kind)
    }
}

impl std::ops::BitOrAssign for Listens {
    #[inline]
    fn bitor_assign(&mut self, other: Listens) {
        self.0 |= other.0;
    }
}

impl Copy for Event<Small> {}

impl<L: Layout> Event<L> {
    /// The kind of event a trigger may listen to this as, or `None` if no trigger listens to
    /// it.
    pub fn kind(&self) -> Option<EventKind> {
        Some(match self {
            Event::ZoneChange {
                to: Zone::Battlefield,
                ..
            } => EventKind::Enters,
            Event::ZoneChange {
                reason: Reason::Discard,
                ..
            } => EventKind::Discard,
            Event::Attack { .. } => EventKind::Attack,
            Event::Damage { .. } => EventKind::Damage,
            Event::StepBegins { .. } => EventKind::Step,
            Event::TokensCreated { .. } => EventKind::Tokens,
            Event::Cast { .. } => EventKind::Cast,
            Event::Draw { .. } => EventKind::Draw,
            _ => return None,
        })
    }
}

/// An event a recording sink kept, with what the rules were doing as it happened.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Tagged<L: Layout = Small> {
    pub event: Event<L>,
    pub tag: Tag<L>,
}

/// Where executed events go for the UI or a replay (§8.2, step 6). Search leaves it off.
///
/// A recording sink keeps a [`Tag`] beside each event, written from the scope the rules have
/// open ([`Sink::enter`]) and the group ([`Sink::together`]). Tags are no part of an
/// [`Event`], so trigger matching never reads one, and a sink that is off keeps neither: a
/// scope or a group costs it one branch to open and one to close, and an event nothing more
/// than it did.
#[derive(Debug, Default)]
pub struct Sink<L: Layout = Small> {
    events: Vec<Event<L>>,
    /// The tag of each event, in step with `events`.
    tags: Vec<Tag<L>>,
    scopes: Scopes<L>,
    /// Where in `events` the first event of the batch being kept is ([`Sink::push_joined`]).
    batch: usize,
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

    /// The events since the last drain, oldest first, without their tags.
    pub fn drain(&mut self) -> std::vec::Drain<'_, Event<L>> {
        self.tags.clear();
        self.events.drain(..)
    }

    /// The events since the last drain, oldest first, each with its tag.
    pub fn drain_tagged(&mut self) -> impl Iterator<Item = Tagged<L>> + '_ {
        self.take_from(0)
    }

    pub fn events(&self) -> &[Event<L>] {
        &self.events
    }

    /// The tag of each of [`Sink::events`], in step with them.
    pub fn tags(&self) -> &[Tag<L>] {
        &self.tags
    }

    /// How many events it holds.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Takes the events after the first `len`, those of a body run replayed (§7.3), with
    /// their tags.
    pub fn take_from(&mut self, len: usize) -> impl Iterator<Item = Tagged<L>> + '_ {
        let tags = self.tags.drain(len..);
        let events = self.events.drain(len..);
        events.zip(tags).map(|(event, tag)| Tagged { event, tag })
    }

    /// Drops the events after the first `len`, and their tags: those of a body run discarded
    /// (§7.3), or of a game run on until a capacity of its layout ran out, to be run again in
    /// a wider one.
    pub fn truncate(&mut self, len: usize) {
        self.events.truncate(len);
        self.tags.truncate(len);
    }

    /// The same events and tags in layout `M`, recorded or not as this sink records them. No
    /// scope is open between the engine's calls, and a game run on until a capacity ran out
    /// left its own behind, so the new sink starts with none.
    pub fn relayout<M: Layout>(&self) -> Sink<M> {
        Sink {
            events: self.events.iter().map(Relayout::relayout).collect(),
            tags: self.tags.iter().map(Relayout::relayout).collect(),
            scopes: Scopes::default(),
            batch: self.batch,
            on: self.on,
            work: self.work,
            chance: self.chance.clone(),
            revealed: crate::relayout::bits(self.revealed),
        }
    }

    /// Opens a scope: until [`Sink::leave`], the events kept are tagged with `cause`, which
    /// is given the spell or ability the scope around this one names, if any. Scopes nest,
    /// and with none open the cause is [`Cause::Rules`]. A sink that is off never asks for
    /// the cause.
    #[inline]
    pub fn enter(&mut self, cause: impl FnOnce(Option<StackIx>) -> Cause<L>) {
        if self.on {
            self.open(cause(self.scopes.entry()));
        }
    }

    /// Opens the scope of `player`'s own action, about the spell or ability the scope around
    /// it names: a cost paid as that one resolves.
    #[inline]
    pub fn enter_action(&mut self, player: Seat) {
        self.enter(|entry| Cause::Player { player, entry });
    }

    /// Opens the scope of an effect of an ability of `source` that is not on the stack,
    /// about the spell or ability the scope around it names.
    #[inline]
    pub fn enter_source(&mut self, source: impl FnOnce() -> ObjectRef<L>) {
        self.enter(|entry| Cause::Object {
            source: source(),
            entry,
        });
    }

    /// Opens the scope of the resolution of the spell or ability the state has just taken off the
    /// top of its stack, which then holds `below` entries ([CR 608.1]). The entry keeps its place
    /// until [`Sink::resolved`] says it left.
    ///
    /// [CR 608.1]: https://yawgatog.com/resources/magic-rules/#R6081
    #[inline]
    pub fn enter_resolution(&mut self, below: impl FnOnce() -> usize) {
        if self.on {
            self.open_resolution(below());
        }
    }

    #[cold]
    #[inline(never)]
    fn open(&mut self, cause: Cause<L>) {
        self.scopes.enter(cause);
    }

    #[cold]
    #[inline(never)]
    fn open_resolution(&mut self, below: usize) {
        self.scopes.resolve(below);
    }

    #[cold]
    #[inline(never)]
    fn close(&mut self) {
        self.scopes.leave();
    }

    /// Closes the innermost scope.
    #[inline]
    pub fn leave(&mut self) {
        if self.on {
            self.close();
        }
    }

    /// Opens a group: the events kept until [`Sink::apart`] happened as one event of the
    /// rules ([`Tag::together`]). A group opened inside another is part of it.
    #[inline]
    pub fn together(&mut self) {
        if self.on {
            self.join();
        }
    }

    /// Closes the group [`Sink::together`] opened.
    #[inline]
    pub fn apart(&mut self) {
        if self.on {
            self.part();
        }
    }

    /// Opens a group as [`Sink::together`] does, of which the event kept last is part: what the
    /// rules do "immediately after" an event, with nothing between ([CR 610.3]).
    ///
    /// [CR 610.3]: https://yawgatog.com/resources/magic-rules/#R6103
    #[inline]
    pub fn together_with_last(&mut self) {
        if self.on {
            self.rejoin();
        }
    }

    #[cold]
    #[inline(never)]
    fn join(&mut self) {
        self.scopes.together(false);
    }

    #[cold]
    #[inline(never)]
    fn rejoin(&mut self) {
        self.scopes.together(!self.events.is_empty());
    }

    #[cold]
    #[inline(never)]
    fn part(&mut self) {
        self.scopes.apart();
    }

    /// Whether it keeps events: what only a recording sink is told may be left unread
    /// otherwise.
    #[inline]
    pub fn records(&self) -> bool {
        self.on
    }

    /// Whether no scope or group is open, as between the engine's calls.
    pub fn is_idle(&self) -> bool {
        self.scopes.is_empty()
    }

    /// Notes that the entry now on top of the stack, which `entry` gives, arrived there.
    #[inline]
    pub fn arrived(&mut self, entry: impl FnOnce() -> StackEntry<L>) {
        if self.on {
            self.keep(Event::StackArrival { entry: entry() });
        }
    }

    /// Notes that the entry the state has just taken from `index` of its stack left it.
    #[inline]
    pub fn departed(&mut self, index: impl FnOnce() -> usize) {
        if self.on {
            let entry = self.scopes.place(index());
            self.depart(entry);
        }
    }

    /// Notes that the spell or ability that is resolving left the stack, as the last part of its
    /// resolution ([CR 608.2n]: "As the final part of an ability's resolution, the ability is
    /// removed from the stack and ceases to exist.").
    ///
    /// [CR 608.2n]: https://yawgatog.com/resources/magic-rules/#R6082n
    #[inline]
    pub fn resolved(&mut self) {
        if self.on
            && let Some(entry) = self.scopes.resolving()
        {
            self.depart(entry);
        }
    }

    #[cold]
    #[inline(never)]
    fn depart(&mut self, entry: StackIx) {
        self.keep(Event::StackDeparture { entry });
        self.scopes.departed(entry);
    }

    /// Out of line, so that a sink that is off pays for a kept event only the branch around
    /// the call: the callers stay as small as when there was no tag to write.
    #[cold]
    #[inline(never)]
    fn keep(&mut self, e: Event<L>) {
        self.events.push(e);
        let tag = self.scopes.tag();
        self.tags.push(tag);
    }

    /// Keeps an event of a batch: its first, or with `joined` one after, which is then of
    /// the first's group with whatever was kept between them, as a card the move revealed.
    #[cold]
    #[inline(never)]
    fn keep_batched(&mut self, e: Event<L>, joined: bool) {
        let at = self.events.len();
        self.keep(e);
        if joined {
            // A sink drained or cut since the first has no first to join.
            let first = self.batch.min(at);
            for tag in &mut self.tags[first + 1..] {
                tag.together = true;
            }
        } else {
            self.batch = at;
        }
    }

    /// Keeps an event only the UI reads, which no rule acts on and so counts as no work of
    /// the pipeline's: search pays one branch for it.
    pub fn note(&mut self, e: Event<L>) {
        if self.on {
            self.keep(e);
        }
    }

    /// Keeps an event as [`Sink::note`] does, as the effect of an ability of `source` that is
    /// not on the stack ([`Cause::Object`]), about the spell or ability the open scope names:
    /// the one event of a scope, without opening it.
    pub fn note_from(&mut self, source: Option<ObjectRef<L>>, e: Event<L>) {
        if self.on {
            self.keep_from(source, e);
        }
    }

    #[cold]
    #[inline(never)]
    fn keep_from(&mut self, source: Option<ObjectRef<L>>, e: Event<L>) {
        if let Some(source) = source {
            self.scopes.enter(Cause::Object {
                source,
                entry: self.scopes.entry(),
            });
            self.keep(e);
            self.scopes.leave();
        } else {
            self.keep(e);
        }
    }

    pub fn push(&mut self, e: Event<L>) {
        self.work.events += 1;
        if self.on {
            self.keep(e);
        }
    }

    /// [`Sink::push`], for an event of a batch: its first, or with `joined` one after it,
    /// which happened as one event of the rules with the first ([`Tag::together`]). It
    /// costs a sink that is off no branch a push does not, and it never asks `joined`.
    #[inline]
    pub fn push_joined(&mut self, e: Event<L>, joined: impl FnOnce() -> bool) {
        self.work.events += 1;
        if self.on {
            self.keep_batched(e, joined());
        }
    }
}
