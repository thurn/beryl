//! The game state: one flat `Copy` value with fixed capacities and no heap (architecture §4,
//! decisions D3 and D4).

use crate::bitboard::Bitboard;
use crate::decision::Outcome;
use crate::fixed::Fixed;
use crate::profile::Profile;
use crate::rng::Rng;

/// Object slots: deck cards, tokens and spell copies.
pub const SLOTS: usize = 256;
/// Slots reserved for each player's deck: player `p`'s cards sit in
/// `p * DECK_SLOTS .. p * DECK_SLOTS + deck size`.
pub const DECK_SLOTS: usize = 60;
/// Permanents on the battlefield at once.
pub const BATTLEFIELD: usize = 128;
/// A `u8` slot or index field that refers to nothing.
pub const NONE: u8 = u8::MAX;

/// Where an object is. Objects in [`Zone::Nowhere`] are unused slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Zone {
    Library,
    Hand,
    Battlefield,
    Graveyard,
    Exile,
    Stack,
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
    pub const ATTACKING: u32 = 1 << 2;

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

/// Counters on an object: the common kinds, and one (kind, count) pair for the long tail.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Counters {
    pub plus_one: u8,
    pub minus_one: u8,
    pub loyalty: u8,
    pub other_kind: u8,
    pub other: u8,
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
            other_kind: 0,
            other: 0,
        },
        attached_to: NONE,
        linked: NONE,
        copy_of: NONE,
        timestamp: 0,
        chosen: NONE,
        face: 0,
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

/// An ability of a card in the game table.
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

/// An object on the stack. The casting work extends it with targets, modes, X and costs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct StackEntry {
    pub object: ObjectRef,
    pub controller: u8,
    /// The ability, or [`NONE`] for a spell.
    pub ability: u8,
}

/// An ability that has triggered and waits to be put on the stack.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PendingTrigger {
    pub source: ObjectRef,
    pub controller: u8,
    pub ability: u8,
}

/// A delayed triggered ability, waiting for its event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DelayedTrigger {
    pub source: ObjectRef,
    pub controller: u8,
    pub ability: u8,
}

/// How long a floating effect lasts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Duration {
    /// "Until end of turn" and "this turn" (CR 514.2).
    #[default]
    EndOfTurn,
}

/// A continuous effect from a resolved spell or ability. The layers work extends it with
/// its layer, affected set and modification.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FloatingEffect {
    pub source: ObjectRef,
    pub timestamp: u16,
    pub duration: Duration,
}

/// The open question when it belongs to a machine step rather than to priority.
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

    /// Whether `player` knows the identity of the object in `slot`.
    pub fn knows(&self, player: u8, slot: u8) -> bool {
        self.knowledge[usize::from(player)].contains(slot)
    }

    pub(crate) fn object_mut(&mut self, slot: u8) -> &mut Object {
        &mut self.objects[usize::from(slot)]
    }

    pub(crate) fn player_mut(&mut self, player: u8) -> &mut Player {
        &mut self.players[usize::from(player)]
    }
}
