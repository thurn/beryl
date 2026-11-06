//! The game state: one flat `Copy` value with fixed capacities and no heap (architecture §4,
//! decisions D3 and D4).

use crate::bitboard::Bitboard;
use crate::card::{
    Castings, Colors, FaceAbility, FaceIx, Keywords, ManaColor, ManaCost, Modification, Subtypes,
    Supertypes, Types,
};
use crate::decision::{Decision, Outcome};
use crate::fixed::{Fixed, Full};
use crate::ids::{CardIx, Seat, Slot};
use crate::profile::Profile;
use crate::rng::Rng;

/// Object slots: deck cards, tokens and spell copies.
pub const SLOTS: usize = Slot::COUNT;
/// Slots reserved for each player's deck: player `p`'s cards sit in
/// `p * DECK_SLOTS .. p * DECK_SLOTS + deck size`.
pub const DECK_SLOTS: usize = 60;
/// The first slot of the shared pool for tokens and spell copies, after both decks'. Of the
/// 255 slots, the decks reserve 120 and the pool has the other 135.
pub const TOKEN_SLOTS: u8 = 2 * DECK_SLOTS as u8;
/// Permanents on the battlefield at once.
pub const BATTLEFIELD: usize = 128;
/// The [`State::battlefield_index`] of a slot not on the battlefield.
pub(crate) const OFF_BATTLEFIELD: u8 = u8::MAX;

/// The slot of the `i`th card of `player`'s deck.
pub(crate) fn deck_slot(player: Seat, i: usize) -> Slot {
    Slot::new((player.index() * DECK_SLOTS + i) as u8)
}

/// The slots of the shared pool for tokens and spell copies.
pub(crate) fn token_slots() -> impl Iterator<Item = Slot> {
    (TOKEN_SLOTS..SLOTS as u8).map(Slot::new)
}

/// [`token_slots`] as a set.
pub(crate) const TOKEN_POOL: Bitboard =
    Bitboard([0, !0 << (TOKEN_SLOTS - 64), !0, !0 >> (256 - SLOTS)]);

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
    /// Exiled by an ability of the object in `linked`, whose linked ability refers to the
    /// cards "exiled with" it ([CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a)).
    pub const EXILED_WITH: u32 = 1 << 6;
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
    /// left the battlefield, `combat` is `None` and the creature attacks nothing.
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
    /// Its evoke cost was paid (CR 702.74a).
    pub const EVOKED: u32 = 1 << 21;

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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Object {
    /// Index into the game table: which card this is. Engine-only, since an
    /// [`crate::Observation`] masks it; others read [`State::card`] or
    /// [`crate::Observation::card`].
    pub(crate) card: CardIx,
    /// Bumped on every zone change (CR 400.7), so stale references are detectable.
    pub incarnation: u16,
    pub zone: Zone,
    pub owner: Seat,
    /// The controller before continuous effects. A permanent's real controller is the layer-2
    /// result, [`State::controller`].
    pub(crate) base_controller: Seat,
    pub flags: ObjectFlags,
    /// Damage marked this turn.
    pub damage: u8,
    pub counters: Counters,
    /// The object an Aura or Equipment is attached to.
    pub attached_to: Option<Slot>,
    /// The object that exiled, imprinted or hid this one.
    pub linked: Option<Slot>,
    /// Battlefield entry order, for layers.
    pub timestamp: u16,
    /// Which face is up.
    pub face: FaceIx,
    /// In combat: the planeswalker an attacker attacks, or the attacker a blocker blocks;
    /// `None` for an attacker attacking a player, or once that object has left the
    /// battlefield.
    pub combat: Option<Slot>,
    /// The object's index in [`State::battlefield`], so finding a permanent's `derived` is
    /// one read rather than a search of the battlefield; [`OFF_BATTLEFIELD`] off it. Only
    /// [`State::enter_battlefield`] and [`State::leave_battlefield`] change it, and it fills
    /// what was padding.
    pub(crate) battlefield_index: u8,
}

impl Object {
    pub const EMPTY: Object = Object {
        card: CardIx::new(0),
        incarnation: 0,
        zone: Zone::Nowhere,
        owner: Seat::P0,
        base_controller: Seat::P0,
        flags: ObjectFlags(0),
        damage: 0,
        counters: Counters {
            plus_one: 0,
            minus_one: 0,
            loyalty: 0,
            other_kind: CounterKind::PlusOne,
            other: 0,
        },
        attached_to: None,
        linked: None,
        timestamp: 0,
        face: FaceIx::FRONT,
        combat: None,
        battlefield_index: OFF_BATTLEFIELD,
    };
}

const _: () = assert!(size_of::<Option<Slot>>() == 1 && size_of::<Option<CardIx>>() == 1);
const _: () = assert!(size_of::<Object>() == 24);

/// Every field but the battlefield index, a cache of the battlefield list's order, in
/// declaration order, as a derived `Hash` would: so a hash of a state is a hash of the game.
impl std::hash::Hash for Object {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        let Object {
            card,
            incarnation,
            zone,
            owner,
            base_controller,
            flags,
            damage,
            counters,
            attached_to,
            linked,
            timestamp,
            face,
            combat,
            battlefield_index: _,
        } = self;
        card.hash(h);
        incarnation.hash(h);
        zone.hash(h);
        owner.hash(h);
        base_controller.hash(h);
        flags.hash(h);
        damage.hash(h);
        counters.hash(h);
        attached_to.hash(h);
        linked.hash(h);
        timestamp.hash(h);
        face.hash(h);
        combat.hash(h);
    }
}

/// A reference to one incarnation of the object in a slot (architecture §5). A reference
/// whose incarnation no longer matches names an object that has left its zone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ObjectRef {
    pub slot: Slot,
    pub incarnation: u16,
}

/// The result of applying continuous effects to a permanent (architecture §9), by
/// battlefield index. The layers work fills it in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Derived {
    pub power: i8,
    pub toughness: i8,
    pub types: Types,
    pub supertypes: Supertypes,
    pub colors: Colors,
    pub controller: Seat,
    /// The colors it has protection from.
    pub protection: Colors,
    pub keywords: Keywords,
    pub subtypes: Subtypes,
    /// The abilities other objects grant this permanent (§9), in the order granted.
    pub granted: [Option<GrantRef>; 4],
}

/// An ability one card grants to other objects: the card's game index and the index into its
/// front face's [`crate::card::Face::grants`] (§9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GrantRef {
    pub card: CardIx,
    pub grant: u8,
}

/// Zone membership and the zones whose order the rules read (architecture §4.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Zones {
    /// Membership by owner, indexed by `Zone as usize` for the owned zones.
    pub members: [[Bitboard; Zone::OWNED]; 2],
    pub stack: Bitboard,
    /// Bottom first: the top card is the last.
    pub library: [Fixed<Slot, DECK_SLOTS>; 2],
    /// Bottom first: the top card is the last.
    pub graveyard: [Fixed<Slot, DECK_SLOTS>; 2],
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

/// Unspent mana, an amount of each [`ManaColor`]
/// ([CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064)): "When an effect instructs
/// a player to add mana, that mana goes into a mana pool."
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ManaPool([u8; 6]);

impl ManaPool {
    /// The amounts, indexed as [`ManaColor::ALL`].
    pub fn amounts(self) -> [u8; 6] {
        self.0
    }
}

impl std::ops::Index<ManaColor> for ManaPool {
    type Output = u8;
    fn index(&self, color: ManaColor) -> &u8 {
        &self.0[color.index()]
    }
}

impl std::ops::IndexMut<ManaColor> for ManaPool {
    fn index_mut(&mut self, color: ManaColor) -> &mut u8 {
        &mut self.0[color.index()]
    }
}

/// One player (architecture §4.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Player {
    pub life: i16,
    /// Unspent mana.
    pub mana: ManaPool,
    pub lands_played: u8,
    /// Attempted to draw from an empty library since state-based actions were last checked
    /// (CR 704.5b).
    pub drew_from_empty: bool,
    pub mulligans: u8,
    pub mulligan: MulliganStatus,
    /// Cards still to put on the bottom of the hand just kept, one per mulligan taken.
    pub to_bottom: u8,
    /// The emblems the player has, as game indices
    /// ([CR 114.1](https://yawgatog.com/resources/magic-rules/#R1141)): "An emblem is a
    /// marker used to represent an object that has one or more abilities, but usually no
    /// other characteristics."
    pub emblems: Fixed<CardIx, 4>,
    /// "Until your next turn, you may cast sorcery spells as though they had flash." (Teferi,
    /// Time Raveler)
    pub sorceries_as_flash: bool,
}

/// The steps of a turn ([CR 500.1](https://yawgatog.com/resources/magic-rules/#R5001)): "A
/// turn consists of five phases, in this order: beginning, precombat main, combat, postcombat
/// main, and ending."
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

impl TurnStep {
    /// Every step, in turn order.
    pub const ALL: [TurnStep; 13] = [
        TurnStep::Untap,
        TurnStep::Upkeep,
        TurnStep::Draw,
        TurnStep::PrecombatMain,
        TurnStep::BeginningOfCombat,
        TurnStep::DeclareAttackers,
        TurnStep::DeclareBlockers,
        TurnStep::FirstStrikeDamage,
        TurnStep::CombatDamage,
        TurnStep::EndOfCombat,
        TurnStep::PostcombatMain,
        TurnStep::End,
        TurnStep::Cleanup,
    ];
}

/// Where the game is in its turns (architecture §4.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Turn {
    /// 0 before the first turn.
    pub number: u16,
    pub active: Seat,
    pub starting_player: Seat,
    pub step: TurnStep,
    /// The player with priority.
    pub priority: Seat,
    /// Players who have passed in succession (CR 117.4).
    pub passes: u8,
    /// The cleanup step is checking for state-based actions (CR 514.3a): if none are
    /// performed, the turn ends instead of a player receiving priority.
    pub cleanup_check: bool,
    /// Creatures were declared as attackers this combat, so the declare blockers and combat
    /// damage steps happen even if none is attacking any more
    /// ([CR 508.8](https://yawgatog.com/resources/magic-rules/#R5088)).
    pub attacked: bool,
}

/// Targets per stack entry (architecture §4.2).
pub const TARGETS: usize = 8;

/// One chosen target: an object or a player, and the instance of the word "target" it was
/// chosen for (an index into the ability's target specs). Four bytes, as an [`ObjectRef`]
/// with its padding used, where an enum of the two would take eight: a player has no slot
/// and the player in place of the incarnation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Target {
    pub spec: u8,
    slot: Option<Slot>,
    incarnation: u16,
}

const _: () = assert!(size_of::<Target>() == 4);

impl Target {
    pub fn object(spec: u8, object: ObjectRef) -> Target {
        Target {
            spec,
            slot: Some(object.slot),
            incarnation: object.incarnation,
        }
    }

    pub fn player(spec: u8, player: Seat) -> Target {
        Target {
            spec,
            slot: None,
            incarnation: u16::from(player.get()),
        }
    }

    /// The targeted object, if it is one.
    pub fn as_object(self) -> Option<ObjectRef> {
        self.slot.map(|slot| ObjectRef {
            slot,
            incarnation: self.incarnation,
        })
    }

    /// The targeted player, if it is one.
    pub fn as_player(self) -> Option<Seat> {
        match self.slot {
            None => Seat::try_new(self.incarnation as u8),
            Some(_) => None,
        }
    }
}

/// A spell or ability on the stack ([CR 405.1](https://yawgatog.com/resources/magic-rules/#R4051)):
/// "When a spell is cast, the physical card is put on the stack (see rule 601.2a). When an
/// ability is activated or triggers, it goes on top of the stack without any card associated
/// with it".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct StackEntry {
    /// The spell's card, or the ability's source; an emblem's ability has none.
    pub object: Option<ObjectRef>,
    pub controller: Seat,
    /// The ability of `face`, or `None` for a spell.
    pub ability: Option<FaceAbility>,
    /// The game index and face of the spell's card, or of the card that has the ability,
    /// which outlast the source ([CR 113.7a](https://yawgatog.com/resources/magic-rules/#R1137a)):
    /// "Once activated or triggered, an ability exists on the stack independently of its
    /// source."
    pub card: CardIx,
    pub face: FaceIx,
    /// For a triggered ability: what it triggered on.
    pub info: TriggerInfo,
    pub targets: Fixed<Target, TARGETS>,
    /// The announced value of X ([CR 107.3a](https://yawgatog.com/resources/magic-rules/#R1073a)).
    pub x: u8,
    /// The face's casting options paid.
    pub costs: Castings,
    /// If the spell would be put into its owner's graveyard, it is exiled instead.
    pub exile: bool,
    /// Cast for its flashback cost, so it is exiled wherever else it would go as it leaves
    /// the stack ([CR 702.34a](https://yawgatog.com/resources/magic-rules/#R70234a)): "If the
    /// flashback cost was paid, exile this card instead of putting it anywhere else any time
    /// it would leave the stack."
    pub flashback: bool,
}

impl StackEntry {
    pub fn is_spell(&self) -> bool {
        self.ability.is_none()
    }

    /// The spell's card, if this is a spell.
    pub fn spell(&self) -> Option<ObjectRef> {
        self.object.filter(|_| self.is_spell())
    }
}

/// Last-known information about a permanent that left the battlefield (architecture §5),
/// as it last existed there
/// ([CR 603.10](https://yawgatog.com/resources/magic-rules/#R60310)): the game "looks back in
/// time" for leaves-the-battlefield abilities.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Lki {
    /// The game index of the card, the copiable values of a token copy.
    pub card: CardIx,
    pub power: i8,
    pub toughness: i8,
    pub types: Types,
    pub controller: Seat,
    pub token: bool,
}

/// What a triggered ability triggered on: the object and player the event involved, an
/// amount, and last-known information for look-back triggers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TriggerInfo {
    pub object: Option<ObjectRef>,
    pub player: Seat,
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
    pub player: Seat,
    /// Generic mana added to the cost, as in "A spell cast this way costs {2} more to cast."
    /// (Elite Spellbinder)
    pub extra: u8,
    /// The permission lasts until end of turn rather than while the card stays in its zone.
    pub this_turn: bool,
    /// "If that spell would be put into your graveyard, exile it instead." (Jace, Telepath
    /// Unbound)
    pub exile: bool,
    /// The card has flashback, its cost the card's mana cost (Snapcaster Mage)
    /// ([CR 702.34a](https://yawgatog.com/resources/magic-rules/#R70234a)): "'Flashback
    /// [cost]' means 'You may cast this card from your graveyard if the resulting spell is an
    /// instant or sorcery spell by paying [cost] rather than paying its mana cost'". Paying it
    /// is an alternative cost, so the card's own can't be added.
    pub flashback: bool,
}

/// An ability that has triggered and waits to be put on the stack.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PendingTrigger {
    /// The object with the ability; an emblem's trigger has none.
    pub source: Option<ObjectRef>,
    pub card: CardIx,
    pub face: FaceIx,
    /// The index of the ability among the face's own.
    pub ability: u8,
    /// The player who controlled the source when it triggered
    /// ([CR 603.3a](https://yawgatog.com/resources/magic-rules/#R6033a)).
    pub controller: Seat,
    pub info: TriggerInfo,
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
    UntilYourNextTurn(Seat),
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
    /// Tapping artifacts and creatures for a waterbend cost's generic mana one at a time;
    /// `left` is the generic mana they may still pay (CR 701.67a).
    Waterbend,
    /// Choosing a card a cost exiles (CR 601.2h): from the hand, as evoke's "Exile a blue card
    /// from your hand", or one of the cards from the graveyard of an additional cost such as
    /// Abhorrent Oculus's "exile six cards from your graveyard". `left` is the number still to
    /// exile. Graveyard cards are chosen in graveyard order, bottom first, each above the one
    /// before, from position `spec` up, so each set of cards is one sequence of answers (§7.2).
    Exile,
}

/// A spell being cast or an ability being activated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Casting {
    pub player: Seat,
    pub step: CastStep,
    /// The target spec being chosen; for [`CastStep::Exile`], the graveyard position picks
    /// continue from.
    pub spec: u8,
    /// Targets still to choose for it, or what another step still has to pay.
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
    /// Generic mana paid by tapping artifacts and creatures for a waterbend cost
    /// (CR 701.67a).
    pub bent: u8,
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
    PlayOrDraw { chooser: Seat },
    /// The London mulligan (CR 103.5). Who is asked follows from the players' statuses.
    Mulligans,
    /// The active player discards down to their maximum hand size (CR 514.1).
    Discard,
    /// The legend rule (CR 704.5j): `kept` holds the permanent each player chose to keep so
    /// far.
    LegendRule { kept: Bitboard },
    /// A land being played asks whether its controller pays life so that it doesn't enter
    /// tapped ([`crate::card::Replacement::EntersTappedUnlessPayLife`]).
    EnterPayingLife { slot: Slot, life: u8 },
    /// The active player declares attackers (CR 508.1), one creature at a time: `attacker`
    /// is the creature just chosen, whose defender is still to choose, if any.
    DeclareAttackers { attacker: Option<Slot> },
    /// The defending player declares blockers (CR 509.1), one creature at a time: `blocker`
    /// is the creature just chosen, whose attacker is still to choose, if any.
    DeclareBlockers { blocker: Option<Slot> },
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
        player: Seat,
        free: bool,
    },
    /// The controller of the face-down permanent in `slot` turns it face up, a special action
    /// (CR 116.2b), and chooses how to pay its mana cost: among payment plans, or under manual
    /// payment one source at a time, `remaining` being what is left to pay.
    TurningFaceUp { slot: Slot, remaining: ManaCost },
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
    pub(crate) battlefield: Fixed<Slot, BATTLEFIELD>,
    /// Layer results, by battlefield index.
    pub(crate) derived: [Derived; BATTLEFIELD],
    /// The kinds of event the permanents on the battlefield have triggered abilities for
    /// ([`crate::table::GameTable::listens`] of each), found with the layer results and
    /// current while they are.
    pub(crate) listening: u16,
    pub(crate) zones: Zones,
    pub(crate) players: [Player; 2],
    pub(crate) turn: Turn,
    pub(crate) stack: Fixed<StackEntry, 64>,
    pub(crate) triggers: Fixed<PendingTrigger, 64>,
    /// Delayed triggered abilities waiting for their moment (CR 603.7), which their
    /// ability's [`crate::card::Trigger::Delayed`] names.
    pub(crate) delayed: Fixed<PendingTrigger, 32>,
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

impl State {
    pub fn object(&self, slot: Slot) -> &Object {
        &self.objects[slot.index()]
    }

    /// The game index of the card in a slot. Players read it through
    /// [`crate::Observation::card`], which masks what they do not know.
    pub fn card(&self, slot: Slot) -> CardIx {
        self.object(slot).card
    }

    /// The current incarnation of the object in a slot.
    pub fn object_ref(&self, slot: Slot) -> ObjectRef {
        ObjectRef {
            slot,
            incarnation: self.object(slot).incarnation,
        }
    }

    /// Whether a reference still names the object it was taken from.
    pub fn is_current(&self, r: ObjectRef) -> bool {
        self.object(r.slot).incarnation == r.incarnation
    }

    pub fn player(&self, player: Seat) -> &Player {
        &self.players[player.index()]
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

    /// Reseeds the game's random streams, as search does for each copy it plays on so sibling
    /// rollouts diverge (architecture §14).
    pub fn reseed(&mut self, seed: u64) {
        self.rng = Rng::new(seed);
    }

    /// The slots a player owns in a zone other than the stack.
    pub fn zone(&self, owner: Seat, zone: Zone) -> Bitboard {
        self.zones.members[owner.index()][zone as usize]
    }

    /// A player's library as slots, bottom first.
    pub fn library(&self, player: Seat) -> &[Slot] {
        self.zones.library[player.index()].as_slice()
    }

    /// A player's graveyard as slots, bottom first.
    pub fn graveyard(&self, player: Seat) -> &[Slot] {
        self.zones.graveyard[player.index()].as_slice()
    }

    /// Slots on the battlefield, in the order they entered.
    pub fn battlefield(&self) -> &[Slot] {
        self.battlefield.as_slice()
    }

    pub fn pending(&self) -> &Pending {
        &self.pending
    }

    /// The value of X the permanent in `slot` was cast with (CR 107.3m), 0 if none.
    pub fn x_of(&self, slot: Slot) -> u8 {
        self.entered_x
            .iter()
            .find(|&&(r, _)| r.slot == slot && self.is_current(r))
            .map_or(0, |&(_, x)| x)
    }

    /// Whether `player` knows the identity of the object in `slot`.
    pub fn knows(&self, player: Seat, slot: Slot) -> bool {
        self.knowledge[player.index()].contains(slot)
    }

    /// A permanent's characteristics after continuous effects (architecture §9). Reads never
    /// recompute: every path that changes a layer input recomputes before anything reads.
    ///
    /// # Panics
    ///
    /// If `slot` is not on the battlefield, or, in debug builds, if `derived` is stale.
    pub fn derived(&self, slot: Slot) -> &Derived {
        debug_assert!(!self.flags.derived_dirty, "derived read while dirty");
        let i = self
            .battlefield_position(slot)
            .expect("derived characteristics of a permanent");
        &self.derived[i]
    }

    /// The index of the permanent in `slot` in the battlefield list, if it is on the
    /// battlefield.
    pub(crate) fn battlefield_position(&self, slot: Slot) -> Option<usize> {
        let i = self.object(slot).battlefield_index;
        (i != OFF_BATTLEFIELD).then_some(usize::from(i))
    }

    /// Lists `slot` last on the battlefield.
    pub(crate) fn enter_battlefield(&mut self, slot: Slot) -> Result<(), Full> {
        debug_assert!(self.battlefield_position(slot).is_none());
        self.battlefield.push(slot)?;
        self.object_mut(slot).battlefield_index = (self.battlefield.len() - 1) as u8;
        Ok(())
    }

    /// Takes `slot` off the battlefield list, keeping the others in order.
    pub(crate) fn leave_battlefield(&mut self, slot: Slot) {
        let Some(i) = self.battlefield_position(slot) else {
            return;
        };
        self.battlefield.remove(i);
        self.object_mut(slot).battlefield_index = OFF_BATTLEFIELD;
        for j in i..self.battlefield.len() {
            let later = self.battlefield.as_slice()[j];
            self.object_mut(later).battlefield_index = j as u8;
        }
    }

    /// Who controls an object: for a permanent the layer-2 result, otherwise the base
    /// controller ([CR 109.4](https://yawgatog.com/resources/magic-rules/#R1094)).
    pub fn controller(&self, slot: Slot) -> Seat {
        let o = self.object(slot);
        if o.zone == Zone::Battlefield {
            self.derived(slot).controller
        } else {
            o.base_controller
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

    /// The permissions to cast a card from a zone it could not otherwise be cast from.
    pub fn permissions(&self) -> &[CastPermission] {
        self.permissions.as_slice()
    }

    /// The floating continuous effects.
    pub fn effects(&self) -> &[FloatingEffect] {
        self.effects.as_slice()
    }

    pub(crate) fn object_mut(&mut self, slot: Slot) -> &mut Object {
        &mut self.objects[slot.index()]
    }

    pub(crate) fn player_mut(&mut self, player: Seat) -> &mut Player {
        &mut self.players[player.index()]
    }
}
