//! Builders: the words a card file writes an [`Effect`] in.
//!
//! Each is a `const fn` or a constant, so a definition stays a `static`. A builder takes a
//! printed number; an instruction whose amount is worked out is written as its [`Effect`]
//! variant, with the [`Amount`] it reads:
//!
//! ```
//! use beryl_card::effect::build::{TARGET, YOU, destroy, sequence};
//! use beryl_card::{Amount, Effect};
//!
//! // "Destroy target enchantment. You gain life equal to its mana value."
//! const EFFECT: Effect = sequence(&[
//!     destroy(TARGET),
//!     Effect::GainLife {
//!         player: YOU,
//!         amount: Amount::ManaValue(TARGET),
//!     },
//! ]);
//! ```

use crate::continuous::{Duration, Modification};
use crate::counter::CounterKind;
use crate::definition::CardDef;
use crate::effect::{
    Amount, Comparison, Condition, CopyException, Effect, LibraryPosition, Objects, Players,
};
use crate::index::SpecIx;
use crate::mana::{ManaCost, ManaSet};

/// "This creature", "this spell": [`Objects::This`].
pub const THIS: Objects = Objects::This;
/// The first target: [`Objects::Target`].
pub const TARGET: Objects = target(0);
/// "It", "them", "that card": [`Objects::Those`].
pub const THOSE: Objects = Objects::Those;

/// The `n`th target, counting from 0.
pub const fn target(n: u8) -> Objects {
    Objects::Target(SpecIx::new(n))
}

/// "You": [`Players::You`].
pub const YOU: Players = Players::You;
/// "Each opponent": [`Players::EachOpponent`].
pub const EACH_OPPONENT: Players = Players::EachOpponent;
/// "Each player": [`Players::EachPlayer`].
pub const EACH_PLAYER: Players = Players::EachPlayer;
/// "Target player" or "target opponent", where it is the first target: [`Players::Target`].
pub const TARGET_PLAYER: Players = target_player(0);
/// The first target's controller, "its controller": [`Players::ControllerOf`].
pub const ITS_CONTROLLER: Players = Players::ControllerOf(TARGET);
/// The first target's owner, "its owner": [`Players::OwnerOf`].
pub const ITS_OWNER: Players = Players::OwnerOf(TARGET);
/// "That player", within [`for_each_player`]: [`Players::ThatPlayer`].
pub const THAT_PLAYER: Players = Players::ThatPlayer;

/// The player chosen for the `n`th target, counting from 0.
pub const fn target_player(n: u8) -> Players {
    Players::Target(SpecIx::new(n))
}

/// "Its controller", of any objects.
pub const fn controller_of(objects: Objects) -> Players {
    Players::ControllerOf(objects)
}

/// "Its owner", of any objects.
pub const fn owner_of(objects: Objects) -> Players {
    Players::OwnerOf(objects)
}

/// X: [`Amount::X`].
pub const X: Amount = Amount::X;
/// "That many": [`Amount::ThatMany`].
pub const THAT_MANY: Amount = Amount::ThatMany;

/// "If you do": [`Condition::Did`].
pub const IF_YOU_DO: Condition = Condition::Did;

/// "If there is a [object]", as "if you control an artifact": at least one of `objects`.
pub const fn exists(objects: &'static Objects) -> Condition {
    Condition::Compare(
        Amount::Count(objects),
        Comparison::AtLeast,
        Amount::Fixed(1),
    )
}

/// [`Effect::Sequence`]: each instruction in turn.
pub const fn sequence(effects: &'static [Effect]) -> Effect {
    Effect::Sequence(effects)
}

/// [`Effect::If`] with nothing otherwise: "if [condition], [then]".
pub const fn when(condition: Condition, then: &'static Effect) -> Effect {
    Effect::If {
        condition,
        then,
        otherwise: None,
    }
}

/// [`Effect::If`]: "if [condition], [then]. Otherwise, [otherwise]."
pub const fn if_else(
    condition: Condition,
    then: &'static Effect,
    otherwise: &'static Effect,
) -> Effect {
    Effect::If {
        condition,
        then,
        otherwise: Some(otherwise),
    }
}

/// [`Effect::May`]: "[player] may [effect]".
pub const fn may(player: Players, effect: &'static Effect) -> Effect {
    Effect::May { player, effect }
}

/// [`Effect::Unless`]: "[effect] unless [player] pays [cost]".
pub const fn unless_pays(effect: &'static Effect, player: Players, cost: ManaCost) -> Effect {
    Effect::Unless {
        effect,
        player,
        cost,
    }
}

/// [`Effect::ForEachPlayer`]: `effect` for each of `players`, who is [`THAT_PLAYER`] in it.
pub const fn for_each_player(players: Players, effect: &'static Effect) -> Effect {
    Effect::ForEachPlayer { players, effect }
}

/// [`Effect::Counter`]: "counter [spell]".
pub const fn counter(objects: Objects) -> Effect {
    Effect::Counter(objects)
}

/// [`Effect::Destroy`]: "destroy [permanents]".
pub const fn destroy(objects: Objects) -> Effect {
    Effect::Destroy(objects)
}

/// [`Effect::Exile`]: "exile [objects]".
pub const fn exile(objects: Objects) -> Effect {
    Effect::Exile(objects)
}

/// [`Effect::ExileUntilThisLeaves`]: "exile [objects] until this leaves the battlefield".
pub const fn exile_until_this_leaves(objects: Objects) -> Effect {
    Effect::ExileUntilThisLeaves(objects)
}

/// [`Effect::Sacrifice`]: "sacrifice [permanents]".
pub const fn sacrifice(objects: Objects) -> Effect {
    Effect::Sacrifice(objects)
}

/// [`Effect::ReturnToHand`]: "return [objects] to their owners' hands".
pub const fn return_to_hand(objects: Objects) -> Effect {
    Effect::ReturnToHand(objects)
}

/// [`Effect::PutIntoLibrary`]: "put [objects] into their owners' libraries [position]".
pub const fn put_into_library(objects: Objects, position: LibraryPosition) -> Effect {
    Effect::PutIntoLibrary { objects, position }
}

/// [`Effect::Draw`]: "[player] draws [count] cards".
pub const fn draw(player: Players, count: u8) -> Effect {
    Effect::Draw {
        player,
        count: Amount::Fixed(count),
    }
}

/// [`Effect::Discard`]: "[player] discards [count] cards".
pub const fn discard(player: Players, count: u8) -> Effect {
    Effect::Discard {
        player,
        count: Amount::Fixed(count),
    }
}

/// [`Effect::Mill`]: "[player] mills [count] cards".
pub const fn mill(player: Players, count: u8) -> Effect {
    Effect::Mill {
        player,
        count: Amount::Fixed(count),
    }
}

/// [`Effect::Scry`]: "[player] scries [count]".
pub const fn scry(player: Players, count: u8) -> Effect {
    Effect::Scry {
        player,
        count: Amount::Fixed(count),
    }
}

/// [`Effect::GainLife`]: "[player] gains [amount] life".
pub const fn gain_life(player: Players, amount: u8) -> Effect {
    Effect::GainLife {
        player,
        amount: Amount::Fixed(amount),
    }
}

/// [`Effect::LoseLife`]: "[player] loses [amount] life".
pub const fn lose_life(player: Players, amount: u8) -> Effect {
    Effect::LoseLife {
        player,
        amount: Amount::Fixed(amount),
    }
}

/// [`Effect::CreateTokens`]: "[player] creates [count] [token] tokens".
pub const fn create_tokens(player: Players, token: &'static CardDef, count: u8) -> Effect {
    Effect::CreateTokens {
        player,
        token,
        count: Amount::Fixed(count),
    }
}

/// [`Effect::CreateCopies`] of one exact copy: "[player] creates a token that's a copy of
/// [object]".
pub const fn create_copy(player: Players, of: Objects) -> Effect {
    Effect::CreateCopies {
        player,
        of,
        count: Amount::Fixed(1),
        except: CopyException::None,
    }
}

/// [`Effect::AddCounters`]: "put [count] [kind] counters on [objects]".
pub const fn add_counters(objects: Objects, kind: CounterKind, count: u8) -> Effect {
    Effect::AddCounters {
        objects,
        kind,
        count: Amount::Fixed(count),
    }
}

/// [`Effect::RemoveCounters`]: "remove [count] [kind] counters from [objects]".
pub const fn remove_counters(objects: Objects, kind: CounterKind, count: u8) -> Effect {
    Effect::RemoveCounters {
        objects,
        kind,
        count: Amount::Fixed(count),
    }
}

/// [`Effect::AddMana`]: "[player] adds [mana]".
pub const fn add_mana(player: Players, mana: ManaSet) -> Effect {
    Effect::AddMana { player, mana }
}

/// [`Effect::Modify`]: "[objects] get, gain or become [modification] [duration]".
pub const fn modify(objects: Objects, modification: Modification, duration: Duration) -> Effect {
    Effect::Modify {
        objects,
        modification,
        duration,
    }
}

/// [`Effect::WinGame`]: "[player] wins the game".
pub const fn win_game(player: Players) -> Effect {
    Effect::WinGame(player)
}

/// [`Effect::LoseGame`]: "[player] loses the game".
pub const fn lose_game(player: Players) -> Effect {
    Effect::LoseGame(player)
}
