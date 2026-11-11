//! The identifiers card definitions share with the game state: players' seats, zones, steps
//! and kinds of counter. This crate holds them and the kernel crate depends on it
//! (`docs/project-50k.md`, Crates).

use std::fmt;

/// One of the two players, by seat: the first decklist's and the second's. A seat has no
/// "other player": the rules ask the state for a player's opponents, the next player in turn
/// order and the defending players (`beryl_kernel`'s contract, "Players").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Seat {
    #[default]
    P0,
    P1,
}

impl Seat {
    /// Both seats, in order.
    pub const BOTH: [Seat; 2] = [Seat::P0, Seat::P1];

    /// The seat at `index`.
    ///
    /// # Panics
    ///
    /// If `index` is neither 0 nor 1.
    pub const fn new(index: u8) -> Seat {
        match index {
            0 => Seat::P0,
            1 => Seat::P1,
            _ => panic!("a seat is 0 or 1"),
        }
    }

    /// The seat at `index`, if there is one.
    pub const fn try_new(index: u8) -> Option<Seat> {
        match index {
            0 => Some(Seat::P0),
            1 => Some(Seat::P1),
            _ => None,
        }
    }

    pub const fn get(self) -> u8 {
        self as u8
    }

    /// The index of this seat's entry in a per-player array.
    pub const fn index(self) -> usize {
        self as usize
    }
}

impl fmt::Display for Seat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.get().fmt(f)
    }
}

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
