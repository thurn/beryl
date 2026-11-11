//! The engine's enums mirrored under the protocol's wire names.

use beryl_engine::card::{Keywords, ManaColor, Types};
use beryl_engine::event::Reason;
use beryl_engine::state::{CounterKind, MulliganStatus, TurnStep};
use beryl_engine::{DecisionKind, StallReason, Zone};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The engine's enums, mirrored so the protocol owns its wire names.
macro_rules! mirror {
    ($(#[$meta:meta])* $name:ident from $engine:ty { $($variant:ident),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        pub enum $name {
            $($variant),*
        }

        impl From<$engine> for $name {
            fn from(v: $engine) -> Self {
                match v {
                    $(<$engine>::$variant => $name::$variant),*
                }
            }
        }

        impl From<$name> for $engine {
            fn from(v: $name) -> Self {
                match v {
                    $($name::$variant => <$engine>::$variant),*
                }
            }
        }
    };
}

mirror! {
    /// What is being decided ([`DecisionKind`]).
    Kind from DecisionKind {
        Priority, PlayOrDraw, KeepOrMulligan, TriggerOrder, Attacker, AttackTarget, Blocker,
        BlockTarget, DamageDivision, X, TargetCount, Target, PaymentPlan, ManaPayment, CastOffer,
        YesNo, ChooseObject, ChooseUpTo,
    }
}

mirror! {
    /// A step of the turn ([`TurnStep`]).
    Step from TurnStep {
        Untap, Upkeep, Draw, PrecombatMain, BeginningOfCombat, DeclareAttackers,
        DeclareBlockers, FirstStrikeDamage, CombatDamage, EndOfCombat, PostcombatMain, End,
        Cleanup,
    }
}

mirror! {
    /// A player's mulligan declaration this round ([`MulliganStatus`]).
    Declaration from MulliganStatus { Undeclared, WillMulligan, Kept }
}

mirror! {
    /// A zone ([`Zone`]).
    ZoneName from Zone { Library, Hand, Battlefield, Graveyard, Exile, Stack, Nowhere }
}

mirror! {
    /// Why an object changed zones ([`Reason`]).
    Why from Reason {
        Play, Draw, Discard, Destroy, Sacrifice, StateBased, Exile, Created, Return, Effect,
        Cast, Counter, Resolve,
    }
}

mirror! {
    /// A kind of counter ([`CounterKind`]).
    Counter from CounterKind { PlusOne, MinusOne, Loyalty, Time, Story, Chorus }
}

mirror! {
    /// Why the engine gave up on a game ([`StallReason`]).
    Stall from StallReason { DecisionsPerTurn, Turns, StateBasedIterations, Capacity }
}

mirror! {
    /// A type of mana ([`ManaColor`]).
    Mana from ManaColor { White, Blue, Black, Red, Green, Colorless }
}

/// A card type (CR 300.1), those the cube's cards use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum CardType {
    Artifact,
    Creature,
    Enchantment,
    Instant,
    Land,
    Planeswalker,
    Sorcery,
}

/// A keyword ability a permanent can show (CR 702), those the cube's permanents use in combat.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Keyword {
    FirstStrike,
    Flying,
    Reach,
    Vigilance,
}

impl Keyword {
    pub(super) const ALL: [(Keywords, Keyword); 4] = [
        (Keywords::FIRST_STRIKE, Keyword::FirstStrike),
        (Keywords::FLYING, Keyword::Flying),
        (Keywords::REACH, Keyword::Reach),
        (Keywords::VIGILANCE, Keyword::Vigilance),
    ];
}

impl CardType {
    pub(super) const ALL: [(Types, CardType); 7] = [
        (Types::ARTIFACT, CardType::Artifact),
        (Types::CREATURE, CardType::Creature),
        (Types::ENCHANTMENT, CardType::Enchantment),
        (Types::INSTANT, CardType::Instant),
        (Types::LAND, CardType::Land),
        (Types::PLANESWALKER, CardType::Planeswalker),
        (Types::SORCERY, CardType::Sorcery),
    ];
}
