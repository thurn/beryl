//! The game state: one flat `Copy` value with fixed capacities and no heap (architecture §4,
//! decisions D3 and D4).

use crate::bitboard::Bitboard;
use crate::card::{Delayed, ManaCost, Modification, Types};
use crate::decision::{Decision, Outcome};
use crate::fixed::Fixed;
use crate::profile::Profile;
use crate::rng::Rng;

/// Object slots: deck cards, tokens and spell copies.
pub const SLOTS: usize = 256;
/// Slots reserved for each player's deck: player `p`'s cards sit in
/// `p * DECK_SLOTS .. p * DECK_SLOTS + deck size`.
pub const DECK_SLOTS: usize = 60;
/// The first slot of the shared pool for tokens and spell copies, after both decks'.
pub const TOKEN_SLOTS: u8 = 2 * DECK_SLOTS as u8;
/// Permanents on the battlefield at once.
pub const BATTLEFIELD: usize = 128;
/// A `u8` slot or index field that refers to nothing.
pub const NONE: u8 = u8::MAX;

/// Where an object is. Objects in [`Zone::Nowhere`] are unused slots.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Zone {
    Library,
    Hand,
    Battlefield,
    Graveyard,
    Exile,
    Stack,
    #[default]
    Nowhere,
}

impl Zone {
    /// Zones with a membership bitboard per owner.
    pub const OWNED: usize = 5;

    /// [CR 400.2](https://yawgatog.com/resources/magic-rules/#R4002): "Graveyard, battlefield,
    /// stack, exile, ante, and command are public zones. ... Library and hand are hidden
    /// zones."
    pub fn is_public(self) -> bool {
        matches!(
            self,
            Zone::Battlefield | Zone::Graveyard | Zone::Exile | Zone::Stack
        )
    }
}

/// An object's status and combat flags, one bit each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ObjectFlags(pub u32);

impl ObjectFlags {
    pub const TAPPED: u32 = 1 << 0;
    /// Not under its controller's control continuously since their most recent turn began
    /// ([CR 302.6](https://yawgatog.com/resources/magic-rules/#R3026)).
    pub const SUMMONING_SICK: u32 = 1 << 1;
    /// An attacking creature
    /// ([CR 506.3](https://yawgatog.com/resources/magic-rules/#R5063)): "Only a creature can
    /// attack or block." What it attacks is in `combat`.
    pub const ATTACKING: u32 = 1 << 2;
    /// A token ([CR 111.1](https://yawgatog.com/resources/magic-rules/#R1111)): "a marker
    /// used to represent any permanent that isn't represented by a card."
    pub const TOKEN: u32 = 1 << 3;
    /// A copy whose copiable values are 1/1 by exception, such as an offspring token
    /// ([CR 707.9b](https://yawgatog.com/resources/magic-rules/#R7079b)): "Some copy effects
    /// modify a characteristic as part of the copying process. The final set of values for
    /// that characteristic becomes part of the copiable values of the copy."
    pub const ONE_ONE: u32 = 1 << 4;
    /// Exiled until the permanent in `linked` leaves the battlefield (CR 610.3).
    pub const EXILED_UNTIL: u32 = 1 << 5;
    /// A blocking creature ([CR 509.1g](https://yawgatog.com/resources/magic-rules/#R5091g)).
    /// The attacker it blocks is in `combat`.
    pub const BLOCKING: u32 = 1 << 8;
    /// An attacking creature that has been blocked
    /// ([CR 509.1h](https://yawgatog.com/resources/magic-rules/#R5091h)): "An attacking
    /// creature with one or more creatures declared as blockers for it becomes a blocked
    /// creature ... A creature remains blocked even if all the creatures blocking it are
    /// removed from combat."
    pub const BLOCKED: u32 = 1 << 9;
    /// An attacker attacking a planeswalker rather than a player. If that planeswalker has
    /// left the battlefield, `combat` is [`NONE`] and the creature attacks nothing.
    pub const ATTACKS_PLANESWALKER: u32 = 1 << 10;
    /// Had first strike as the first-strike combat damage step began, so it deals no damage
    /// in the second ([CR 510.4](https://yawgatog.com/resources/magic-rules/#R5104)).
    pub const STRUCK_FIRST: u32 = 1 << 11;
    /// Every flag that combat sets, cleared as combat ends.
    pub const COMBAT: u32 = Self::ATTACKING
        | Self::BLOCKING
        | Self::BLOCKED
        | Self::ATTACKS_PLANESWALKER
        | Self::STRUCK_FIRST;
    /// Entered the battlefield this turn.
    pub const ENTERED_THIS_TURN: u32 = 1 << 16;
    /// A loyalty ability of this permanent was activated this turn
    /// ([CR 606.3](https://yawgatog.com/resources/magic-rules/#R6063)).
    pub const LOYALTY_ACTIVATED: u32 = 1 << 17;
    /// Its spell was kicked ([CR 702.33d](https://yawgatog.com/resources/magic-rules/#R70233d)).
    pub const KICKED: u32 = 1 << 18;
    /// Its offspring cost was paid (CR 702.175a).
    pub const OFFSPRING: u32 = 1 << 19;
    /// Its impending cost was paid (CR 702.176a).
    pub const IMPENDING: u32 = 1 << 20;

    pub fn has(self, flag: u32) -> bool {
        self.0 & flag != 0
    }

    pub fn set(&mut self, flag: u32, on: bool) {
        if on {
            self.0 |= flag;
        } else {
            self.0 &= !flag;
        }
    }
}

/// A kind of counter ([CR 122.1](https://yawgatog.com/resources/magic-rules/#R1221)):
/// "Counters with the same name or description are interchangeable."
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CounterKind {
    #[default]
    PlusOne,
    MinusOne,
    Loyalty,
    Time,
    Story,
    Chorus,
}

/// Counters on an object: the common kinds, and one (kind, count) pair for the long tail.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Counters {
    pub plus_one: u8,
    pub minus_one: u8,
    pub loyalty: u8,
    pub other_kind: CounterKind,
    pub other: u8,
}

impl Counters {
    /// How many counters of a kind there are.
    pub fn get(self, kind: CounterKind) -> u8 {
        match kind {
            CounterKind::PlusOne => self.plus_one,
            CounterKind::MinusOne => self.minus_one,
            CounterKind::Loyalty => self.loyalty,
            _ if self.other_kind == kind => self.other,
            _ => 0,
        }
    }

    /// The count of a kind, to change. One object carries one long-tail kind at a time,
    /// which the cube's cards never exceed; adding a second kind replaces the first.
    pub(crate) fn get_mut(&mut self, kind: CounterKind) -> &mut u8 {
        match kind {
            CounterKind::PlusOne => &mut self.plus_one,
            CounterKind::MinusOne => &mut self.minus_one,
            CounterKind::Loyalty => &mut self.loyalty,
            _ => {
                if self.other_kind != kind {
                    debug_assert_eq!(self.other, 0, "a second long-tail counter kind");
                    self.other_kind = kind;
                    self.other = 0;
                }
                &mut self.other
            }
        }
    }
}

/// The base data of the object in one slot (architecture §4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Object {
    /// Index into the game table.
    pub card: u8,
    /// Bumped on every zone change (CR 400.7), so stale references are detectable.
    pub incarnation: u16,
    pub zone: Zone,
    pub owner: u8,
    pub controller: u8,
    pub flags: ObjectFlags,
    /// Damage marked this turn.
    pub damage: u8,
    pub counters: Counters,
    /// The object an Aura or Equipment is attached to, or [`NONE`].
    pub attached_to: u8,
    /// The object that exiled, imprinted or hid this one, or [`NONE`].
    pub linked: u8,
    /// The game index of the card this object copies, or [`NONE`].
    pub copy_of: u8,
    /// Battlefield entry order, for layers.
    pub timestamp: u16,
    /// A chosen color or type.
    pub chosen: u8,
    /// Which face is up.
    pub face: u8,
    /// In combat: the planeswalker an attacker attacks, or the attacker a blocker blocks;
    /// [`NONE`] for an attacker attacking a player, or once that object has left the
    /// battlefield.
    pub combat: u8,
}

impl Object {
    pub const EMPTY: Object = Object {
        card: NONE,
        incarnation: 0,
        zone: Zone::Nowhere,
        owner: 0,
        controller: 0,
        flags: ObjectFlags(0),
        damage: 0,
        counters: Counters {
            plus_one: 0,
            minus_one: 0,
            loyalty: 0,
            other_kind: CounterKind::PlusOne,
            other: 0,
        },
        attached_to: NONE,
        linked: NONE,
        copy_of: NONE,
        timestamp: 0,
        chosen: NONE,
        face: 0,
        combat: NONE,
    };
}

/// A reference to one incarnation of the object in a slot (architecture §5). A reference
/// whose incarnation no longer matches names an object that has left its zone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ObjectRef {
    pub slot: u8,
    pub incarnation: u16,
}

/// The result of applying continuous effects to a permanent (architecture §9), by
/// battlefield index. The layers work fills it in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Derived {
    pub power: i8,
    pub toughness: i8,
    pub types: u8,
    pub supertypes: u8,
    pub colors: u8,
    pub controller: u8,
    pub protection: u8,
    pub keywords: u64,
    pub subtypes: [u64; 2],
    pub granted: [AbilityRef; 4],
}

/// An ability of a card in the game table: the index into its front face's abilities, for an
/// ability granted by another object (§9). A `card` of [`NONE`] refers to nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct AbilityRef {
    pub card: u8,
    pub ability: u8,
}

/// Zone membership and the zones whose order the rules read (architecture §4.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Zones {
    /// Membership by owner, indexed by `Zone as usize` for the owned zones.
    pub members: [[Bitboard; Zone::OWNED]; 2],
    pub stack: Bitboard,
    /// Bottom first: the top card is the last.
    pub library: [Fixed<u8, DECK_SLOTS>; 2],
    /// Bottom first: the top card is the last.
    pub graveyard: [Fixed<u8, DECK_SLOTS>; 2],
    /// The next battlefield timestamp.
    pub next_timestamp: u16,
}

/// Where a player is in the London mulligan (CR 103.5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MulliganStatus {
    /// Yet to declare this round.
    #[default]
    Undeclared,
    /// Declared a mulligan this round, not yet taken.
    WillMulligan,
    /// Kept: the hand is the opening hand.
    Kept,
}

/// One player (architecture §4.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Player {
    pub life: i16,
    /// Unspent mana by `ManaColor` index.
    pub mana: [u8; 6],
    pub lands_played: u8,
    /// Attempted to draw from an empty library since state-based actions were last checked
    /// (CR 704.5b).
    pub drew_from_empty: bool,
    pub mulligans: u8,
    pub mulligan: MulliganStatus,
    /// Cards still to put on the bottom after the latest mulligan.
    pub to_bottom: u8,
    /// The emblems the player has, as game indices
    /// ([CR 114.1](https://yawgatog.com/resources/magic-rules/#R1141)): "An emblem is a
    /// marker used to represent an object that has one or more abilities, but usually no
    /// other characteristics."
    pub emblems: Fixed<u8, 4>,
    /// "Until your next turn, you may cast sorcery spells as though they had flash." (Teferi,
    /// Time Raveler)
    pub sorceries_as_flash: bool,
}

/// The steps of a turn ([CR 500.1](https://yawgatog.com/resources/magic-rules/#R5001)): "A
/// turn consists of five phases, in this order: beginning, precombat main, combat, postcombat
/// main, and ending."
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TurnStep {
    Untap,
    Upkeep,
    Draw,
    PrecombatMain,
    BeginningOfCombat,
    DeclareAttackers,
    DeclareBlockers,
    /// The first of two combat damage steps, which exists only when a creature in combat has
    /// first strike ([CR 510.4](https://yawgatog.com/resources/magic-rules/#R5104)).
    FirstStrikeDamage,
    CombatDamage,
    EndOfCombat,
    PostcombatMain,
    End,
    Cleanup,
}

/// Where the game is in its turns (architecture §4.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Turn {
    /// 0 before the first turn.
    pub number: u16,
    pub active: u8,
    pub starting_player: u8,
    pub step: TurnStep,
    /// The player with priority.
    pub priority: u8,
    /// Players who have passed in succession (CR 117.4).
    pub passes: u8,
    /// The cleanup step is checking for state-based actions (CR 514.3a): if none are
    /// performed, the turn ends instead of a player receiving priority.
    pub cleanup_check: bool,
}

/// Targets per stack entry (architecture §4.2).
pub const TARGETS: usize = 8;

/// One chosen target: an object or a player, and the instance of the word "target" it was
/// chosen for (an index into the ability's target specs). Four bytes, as an [`ObjectRef`]
/// with its padding used: a player is the slot [`NONE`] with the player in place of the
/// incarnation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Target {
    pub spec: u8,
    slot: u8,
    incarnation: u16,
}

impl Target {
    pub fn object(spec: u8, object: ObjectRef) -> Target {
        Target {
            spec,
            slot: object.slot,
            incarnation: object.incarnation,
        }
    }

    pub fn player(spec: u8, player: u8) -> Target {
        Target {
            spec,
            slot: NONE,
            incarnation: u16::from(player),
        }
    }

    /// The targeted object, if it is one.
    pub fn as_object(self) -> Option<ObjectRef> {
        (self.slot != NONE).then_some(ObjectRef {
            slot: self.slot,
            incarnation: self.incarnation,
        })
    }

    /// The targeted player, if it is one.
    pub fn as_player(self) -> Option<u8> {
        (self.slot == NONE).then_some(self.incarnation as u8)
    }
}

/// A spell or ability on the stack ([CR 405.1](https://yawgatog.com/resources/magic-rules/#R4051)):
/// "When a spell is cast, the physical card is put on the stack (see rule 601.2a). When an
/// ability is activated or triggers, it goes on top of the stack without any card associated
/// with it".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct StackEntry {
    /// The spell's card, or the ability's source.
    pub object: ObjectRef,
    pub controller: u8,
    /// The ability, or [`NONE`] for a spell.
    pub ability: u8,
    /// The game index and face of the spell's card, or of the card that has the ability,
    /// which outlast the source ([CR 113.7a](https://yawgatog.com/resources/magic-rules/#R1137a)):
    /// "Once activated or triggered, an ability exists on the stack independently of its
    /// source."
    pub card: u8,
    pub face: u8,
    /// For a triggered ability: what it triggered on.
    pub info: TriggerInfo,
    pub targets: Fixed<Target, TARGETS>,
    /// The announced value of X ([CR 107.3a](https://yawgatog.com/resources/magic-rules/#R1073a)).
    pub x: u8,
    /// The face's casting options paid, a bit per [`crate::card::Face::casting`] entry.
    pub costs: u8,
    /// If the spell would be put into its owner's graveyard, it is exiled instead.
    pub exile: bool,
}

impl StackEntry {
    pub fn is_spell(&self) -> bool {
        self.ability == NONE
    }
}

/// Last-known information about a permanent that left the battlefield (architecture §5),
/// as it last existed there
/// ([CR 603.10](https://yawgatog.com/resources/magic-rules/#R60310)): the game "looks back in
/// time" for leaves-the-battlefield abilities.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Lki {
    /// The game index of the card, the copiable values of a token copy.
    pub card: u8,
    pub power: i8,
    pub toughness: i8,
    pub types: Types,
    pub controller: u8,
    pub token: bool,
}

/// What a triggered ability triggered on: the object and player the event involved, an
/// amount, and last-known information for look-back triggers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TriggerInfo {
    pub object: ObjectRef,
    pub player: u8,
    pub amount: u8,
    pub lki: Lki,
}

/// A permission to cast a card from a zone other than a hand
/// ([CR 601.3](https://yawgatog.com/resources/magic-rules/#R6013)): "A player can begin to
/// cast a spell only if a rule or effect allows that player to cast it". It names one
/// incarnation of the card, so it ends when the card leaves that zone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CastPermission {
    pub object: ObjectRef,
    pub player: u8,
    /// Generic mana added to the cost, as in "A spell cast this way costs {2} more to cast."
    /// (Elite Spellbinder)
    pub extra: u8,
    /// The permission lasts until end of turn rather than while the card stays in its zone.
    pub this_turn: bool,
    /// "If that spell would be put into your graveyard, exile it instead." (Jace, Telepath
    /// Unbound)
    pub exile: bool,
}

/// An ability that has triggered and waits to be put on the stack.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PendingTrigger {
    /// The object with the ability; an emblem's trigger has none, so its slot is [`NONE`].
    pub source: ObjectRef,
    pub card: u8,
    pub face: u8,
    pub ability: u8,
    /// The player who controlled the source when it triggered
    /// ([CR 603.3a](https://yawgatog.com/resources/magic-rules/#R6033a)).
    pub controller: u8,
    pub info: TriggerInfo,
}

/// A delayed triggered ability, waiting for its moment (CR 603.7).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DelayedTrigger {
    pub trigger: PendingTrigger,
    pub at: Delayed,
}

/// How long a floating effect lasts
/// ([CR 611.2a](https://yawgatog.com/resources/magic-rules/#R6112a)): "A continuous effect
/// generated by the resolution of a spell or ability lasts as long as stated by the spell or
/// ability creating it (such as 'until end of turn'). If no duration is stated, it lasts
/// until the end of the game."
///
/// Every floating effect in the cube's cards affects one object, and ends too when that
/// object leaves its zone: the object it applied to no longer exists (CR 400.7).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Duration {
    /// "Until end of turn" and "this turn" (CR 514.2).
    #[default]
    EndOfTurn,
    /// "Until your next turn": it ends as that player's next turn begins.
    UntilYourNextTurn(u8),
    /// No duration stated.
    Indefinite,
}

/// What a floating effect does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Continuous {
    /// Modifies the affected permanent in a layer (§9).
    Layer(Modification),
    /// "If it would be put into a graveyard, exile it instead"
    /// ([CR 614.1a](https://yawgatog.com/resources/magic-rules/#R6141a)): "Effects that use
    /// the word 'instead' are replacement effects."
    ExileInsteadOfGraveyard,
}

/// A continuous effect from a resolved spell or ability
/// ([CR 611.2c](https://yawgatog.com/resources/magic-rules/#R6112c)): "the set of objects it
/// affects is determined when that continuous effect begins. After that point, the set won't
/// change."
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FloatingEffect {
    pub source: ObjectRef,
    pub timestamp: u16,
    pub duration: Duration,
    pub affected: ObjectRef,
    pub effect: Continuous,
}

impl Default for FloatingEffect {
    fn default() -> Self {
        FloatingEffect {
            source: ObjectRef::default(),
            timestamp: 0,
            duration: Duration::EndOfTurn,
            affected: ObjectRef::default(),
            effect: Continuous::ExileInsteadOfGraveyard,
        }
    }
}

/// The step a spell's casting or an ability's activation has reached (CR 601.2, 602.2). The
/// spell or ability is already the top of the stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CastStep {
    /// Announcing the value of X (CR 601.2b).
    X,
    /// Announcing how many targets to choose for the current spec (CR 601.2c).
    TargetCount,
    /// Choosing the targets of the current spec (CR 601.2c).
    Target,
    /// Choosing among payment plans that differ in their consequences (CR 601.2g, 601.2h).
    Plan,
    /// Paying mana one source at a time, under the `manual_mana` profile.
    Manual,
    /// Tapping creatures for a crew cost one at a time; `left` is the power still needed
    /// (CR 702.122a).
    Crew,
}

/// A spell being cast or an ability being activated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Casting {
    pub player: u8,
    pub step: CastStep,
    /// The target spec being chosen.
    pub spec: u8,
    /// Targets still to choose for it.
    pub left: u8,
    /// Generic mana a permission adds to the cost.
    pub extra: u8,
    /// Cast without paying its mana cost
    /// ([CR 118.9](https://yawgatog.com/resources/magic-rules/#R1189)).
    pub free: bool,
    /// Cast during the resolution of another spell or ability, so no player receives priority
    /// after it is cast ([CR 608.2g](https://yawgatog.com/resources/magic-rules/#R6082g)).
    pub resolving: bool,
    /// What remains to pay under manual payment.
    pub remaining: ManaCost,
    /// A triggered ability being put on the stack: targets only, no costs and no change of
    /// priority (CR 603.3d).
    pub triggered: bool,
}

/// A paused resolution (architecture §7.3): the answers its body has been given so far, and
/// the question it stopped at. The committed state is still the state before the resolution
/// began.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Resolving {
    pub record: Fixed<u8, 32>,
    pub question: Decision,
}

/// The open question when it belongs to a machine step rather than to priority. Flat, like
/// the rest of the state.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Pending {
    /// Priority, or nothing to ask.
    #[default]
    None,
    /// The coin flip's winner chooses who takes the first turn (CR 103.1).
    PlayOrDraw { chooser: u8 },
    /// The London mulligan (CR 103.5). Who is asked follows from the players' statuses.
    Mulligans,
    /// The active player discards down to their maximum hand size (CR 514.1).
    Discard,
    /// The legend rule (CR 704.5j): `kept` holds the permanent each player chose to keep so
    /// far.
    LegendRule { kept: Bitboard },
    /// A land being played asks whether its controller pays life so that it doesn't enter
    /// tapped ([`crate::card::Replacement::EntersTappedUnlessPayLife`]).
    EnterPayingLife { slot: u8, life: u8 },
    /// The active player declares attackers (CR 508.1), one creature at a time: `attacker`
    /// is the creature just chosen, whose defender is still to choose, or [`NONE`].
    DeclareAttackers { attacker: u8 },
    /// The defending player declares blockers (CR 509.1), one creature at a time: `blocker`
    /// is the creature just chosen, whose attacker is still to choose, or [`NONE`].
    DeclareBlockers { blocker: u8 },
    /// The active player divides blocked attackers' combat damage (CR 510.1c): `done` holds
    /// the attackers divided so far, `lethal` the blockers chosen to be dealt lethal damage.
    CombatDamage { done: Bitboard, lethal: Bitboard },
    /// A spell being cast or an ability being activated.
    Casting(Casting),
    /// A body paused for an answer.
    Resolving(Resolving),
    /// A resolving spell or ability lets `player` cast this card now (CR 608.2g).
    CastOffer {
        object: ObjectRef,
        player: u8,
        free: bool,
    },
}

/// Counters toward the budgets of the profile (architecture §12.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Budgets {
    /// Decisions offered this turn.
    pub decisions: u16,
    /// State-based checks in a row that performed something.
    pub state_based_iterations: u8,
}

/// Engine bookkeeping that is not game state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Flags {
    /// A change since `derived` was last computed may have changed a layer result.
    pub derived_dirty: bool,
}

/// The whole game, copied with one `memcpy` (architecture §4.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct State {
    pub(crate) objects: [Object; SLOTS],
    /// Slots on the battlefield, in the order they entered.
    pub(crate) battlefield: Fixed<u8, BATTLEFIELD>,
    /// Layer results, by battlefield index.
    pub(crate) derived: [Derived; BATTLEFIELD],
    pub(crate) zones: Zones,
    pub(crate) players: [Player; 2],
    pub(crate) turn: Turn,
    pub(crate) stack: Fixed<StackEntry, 64>,
    pub(crate) triggers: Fixed<PendingTrigger, 64>,
    pub(crate) delayed: Fixed<DelayedTrigger, 32>,
    pub(crate) effects: Fixed<FloatingEffect, 64>,
    pub(crate) permissions: Fixed<CastPermission, 8>,
    /// The value of X the spells that became these permanents were cast with, for their
    /// enters abilities ([CR 107.3m](https://yawgatog.com/resources/magic-rules/#R1073m)):
    /// "the value of X for that ability is the same as the value of X for that spell". Only
    /// a nonzero X is kept, and only while the permanent stays.
    pub(crate) entered_x: Fixed<(ObjectRef, u8), 8>,
    pub(crate) pending: Pending,
    /// `knowledge[p]` holds the slots whose identity player `p` knows (architecture §13).
    pub(crate) knowledge: [Bitboard; 2],
    pub(crate) rng: Rng,
    pub(crate) profile: Profile,
    pub(crate) budgets: Budgets,
    pub(crate) flags: Flags,
    pub(crate) outcome: Option<Outcome>,
}

const _: () = assert!(size_of::<State>() <= 20 * 1024);

/// The other player.
pub fn opponent(player: u8) -> u8 {
    player ^ 1
}

impl State {
    pub fn object(&self, slot: u8) -> &Object {
        &self.objects[usize::from(slot)]
    }

    /// The current incarnation of the object in a slot.
    pub fn object_ref(&self, slot: u8) -> ObjectRef {
        ObjectRef {
            slot,
            incarnation: self.object(slot).incarnation,
        }
    }

    /// Whether a reference still names the object it was taken from.
    pub fn is_current(&self, r: ObjectRef) -> bool {
        self.object(r.slot).incarnation == r.incarnation
    }

    pub fn player(&self, player: u8) -> &Player {
        &self.players[usize::from(player)]
    }

    pub fn turn(&self) -> &Turn {
        &self.turn
    }

    pub fn outcome(&self) -> Option<Outcome> {
        self.outcome
    }

    pub fn profile(&self) -> Profile {
        self.profile
    }

    /// Changes which decisions are offered from here on (architecture §15).
    pub fn set_profile(&mut self, profile: Profile) {
        self.profile = profile;
    }

    /// The slots a player owns in a zone other than the stack.
    pub fn zone(&self, owner: u8, zone: Zone) -> Bitboard {
        self.zones.members[usize::from(owner)][zone as usize]
    }

    /// A player's library as slots, bottom first.
    pub fn library(&self, player: u8) -> &[u8] {
        self.zones.library[usize::from(player)].as_slice()
    }

    /// A player's graveyard as slots, bottom first.
    pub fn graveyard(&self, player: u8) -> &[u8] {
        self.zones.graveyard[usize::from(player)].as_slice()
    }

    /// Slots on the battlefield, in the order they entered.
    pub fn battlefield(&self) -> &[u8] {
        self.battlefield.as_slice()
    }

    pub fn pending(&self) -> &Pending {
        &self.pending
    }

    /// The value of X the permanent in `slot` was cast with (CR 107.3m), 0 if none.
    pub fn x_of(&self, slot: u8) -> u8 {
        self.entered_x
            .iter()
            .find(|&&(r, _)| r.slot == slot && self.is_current(r))
            .map_or(0, |&(_, x)| x)
    }

    /// Whether `player` knows the identity of the object in `slot`.
    pub fn knows(&self, player: u8, slot: u8) -> bool {
        self.knowledge[usize::from(player)].contains(slot)
    }

    /// A permanent's characteristics after continuous effects (architecture §9). Reads never
    /// recompute: every path that changes a layer input recomputes before anything reads.
    ///
    /// # Panics
    ///
    /// If `slot` is not on the battlefield, or, in debug builds, if `derived` is stale.
    pub fn derived(&self, slot: u8) -> &Derived {
        debug_assert!(!self.flags.derived_dirty, "derived read while dirty");
        let i = self
            .battlefield
            .position(slot)
            .expect("derived characteristics of a permanent");
        &self.derived[i]
    }

    /// Who controls an object: for a permanent the layer-2 result, otherwise the base
    /// controller ([CR 109.4](https://yawgatog.com/resources/magic-rules/#R1094)).
    pub fn controller(&self, slot: u8) -> u8 {
        let o = self.object(slot);
        if o.zone == Zone::Battlefield {
            self.derived(slot).controller
        } else {
            o.controller
        }
    }

    /// The abilities waiting to be put on the stack.
    pub fn triggers(&self) -> &[PendingTrigger] {
        self.triggers.as_slice()
    }

    /// The stack, bottom first.
    pub fn stack(&self) -> &[StackEntry] {
        self.stack.as_slice()
    }

    /// The floating continuous effects.
    pub fn effects(&self) -> &[FloatingEffect] {
        self.effects.as_slice()
    }

    pub(crate) fn object_mut(&mut self, slot: u8) -> &mut Object {
        &mut self.objects[usize::from(slot)]
    }

    pub(crate) fn player_mut(&mut self, player: u8) -> &mut Player {
        &mut self.players[usize::from(player)]
    }
}
