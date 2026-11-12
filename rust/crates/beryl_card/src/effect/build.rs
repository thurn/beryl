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

use crate::continuous::{Duration, Modification, RuleChange};
use crate::counter::CounterKind;
use crate::definition::CardDef;
use crate::effect::{
    Amount, CastTerms, Comparison, Condition, CopyException, Effect, LibraryPosition, Objects,
    Players, Recipients,
};
use crate::ids::Zone;
use crate::index::{AbilityIx, SpecIx};
use crate::mana::{ManaColor, ManaCost, ManaSet};
use crate::prompt::Prompt;

/// "This creature", "this spell": [`Objects::This`].
pub const THIS: Objects = Objects::This;
/// The first target: [`Objects::Target`].
pub const TARGET: Objects = target(0);
/// "It", "them", "that card": [`Objects::Those`].
pub const THOSE: Objects = Objects::Those;

/// "That card", "that spell", of a triggered ability's event: [`Objects::TriggeredOn`].
pub const THAT_CARD: Objects = Objects::TriggeredOn;
/// "The cards exiled with this": [`Objects::ExiledWithThis`].
pub const EXILED_WITH_THIS: Objects = Objects::ExiledWithThis;

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

/// "Each player other than [players]": [`Players::EachOther`].
pub const fn each_player_other_than(players: &'static Players) -> Players {
    Players::EachOther(players)
}

/// X: [`Amount::X`].
pub const X: Amount = Amount::X;
/// "That many": [`Amount::ThatMany`].
pub const THAT_MANY: Amount = Amount::ThatMany;

/// The number a delayed triggered ability carries: [`Amount::Carried`].
pub const CARRIED: Amount = Amount::Carried;

/// "If you do": [`Condition::Did`].
pub const IF_YOU_DO: Condition = Condition::Did;

/// "If you win the flip": [`Condition::WonFlip`].
pub const IF_YOU_WIN_THE_FLIP: Condition = Condition::WonFlip;
/// "If you lose the flip": not [`Condition::WonFlip`].
pub const IF_YOU_LOSE_THE_FLIP: Condition = Condition::Not(&Condition::WonFlip);

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
pub const fn if_then(condition: Condition, then: &'static Effect) -> Effect {
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

/// [`Effect::Choose`]: "[player] chooses one of [among]", for what `prompt` says.
pub const fn choose(player: Players, among: Objects, prompt: Prompt) -> Effect {
    Effect::Choose {
        player,
        among,
        prompt,
        optional: false,
    }
}

/// [`Effect::Choose`], optional: "[player] may choose one of [among]", for what `prompt`
/// says.
pub const fn may_choose(player: Players, among: Objects, prompt: Prompt) -> Effect {
    Effect::Choose {
        player,
        among,
        prompt,
        optional: true,
    }
}

/// [`Effect::LookAt`]: "[player] looks at [cards]".
pub const fn look_at(player: Players, cards: Objects) -> Effect {
    Effect::LookAt { player, cards }
}

/// [`Effect::Counter`]: "counter [spell]".
pub const fn counter(objects: Objects) -> Effect {
    Effect::Counter(objects)
}

/// [`Effect::CounterInto`] exile: "counter [spell]. If that spell is countered this way, exile
/// it instead of putting it into its owner's graveyard."
pub const fn counter_and_exile(spells: Objects) -> Effect {
    Effect::CounterInto {
        spells,
        zone: Zone::Exile,
    }
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

/// [`Effect::ExileLinked`]: "exile [cards]", to be found among "the cards exiled with this".
pub const fn exile_linked(cards: Objects) -> Effect {
    Effect::ExileLinked(cards)
}

/// [`Effect::Sacrifice`]: "sacrifice [permanents]".
pub const fn sacrifice(objects: Objects) -> Effect {
    Effect::Sacrifice(objects)
}

/// [`Effect::ReturnToHand`]: "return [objects] to their owners' hands".
pub const fn return_to_hand(objects: Objects) -> Effect {
    Effect::ReturnToHand(objects)
}

/// [`Effect::PutIntoGraveyard`]: "put [objects] into their owners' graveyards".
pub const fn put_into_graveyard(objects: Objects) -> Effect {
    Effect::PutIntoGraveyard(objects)
}

/// [`Effect::ReturnTransformed`]: "return [cards] to the battlefield transformed under their
/// owners' control".
pub const fn return_transformed(cards: Objects) -> Effect {
    Effect::ReturnTransformed(cards)
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

/// [`Effect::Explore`]: "[permanents] explore".
pub const fn explore(permanents: Objects) -> Effect {
    Effect::Explore(permanents)
}

/// [`Effect::ManifestDread`]: "[player] manifests dread".
pub const fn manifest_dread(player: Players) -> Effect {
    Effect::ManifestDread(player)
}

/// [`Effect::FlipCoin`]: "[player] flips a coin".
pub const fn flip_coin(player: Players) -> Effect {
    Effect::FlipCoin(player)
}

/// [`Effect::DealDamage`] to players: "this deals [amount] damage to [player]".
pub const fn deal_damage_to(player: Players, amount: u8) -> Effect {
    Effect::DealDamage {
        to: Recipients::Players(player),
        amount: Amount::Fixed(amount),
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

/// [`Effect::CreateAttackingTokens`]: "[player] creates [count] [token] tokens that are tapped
/// and attacking [attacking] or a planeswalker they control".
pub const fn create_attacking_tokens(
    player: Players,
    token: &'static CardDef,
    count: u8,
    attacking: Players,
) -> Effect {
    Effect::CreateAttackingTokens {
        player,
        token,
        count: Amount::Fixed(count),
        attacking,
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

/// [`Effect::AddManaOfKind`]: "[player] adds [amount] mana of [kind]".
pub const fn add_mana_of_kind(player: Players, kind: ManaColor, amount: Amount) -> Effect {
    Effect::AddManaOfKind {
        player,
        kind,
        amount,
    }
}

/// [`Effect::Delay`] carrying a number: "[when], [effect]", the delayed triggered ability
/// that is the `ability`th of its face, counting from 0, which reads `carrying` as
/// [`CARRIED`].
pub const fn delay_carrying(ability: u8, carrying: Amount) -> Effect {
    Effect::Delay {
        ability: AbilityIx::new(ability),
        carrying,
    }
}

/// [`Effect::Modify`]: "[objects] get, gain or become [modification] [duration]".
pub const fn modify(objects: Objects, modification: Modification, duration: Duration) -> Effect {
    Effect::Modify {
        objects,
        modification,
        duration,
    }
}

/// [`Effect::ChangeRule`]: "[rule change] [duration]".
pub const fn change_rule(rule: &'static RuleChange, duration: Duration) -> Effect {
    Effect::ChangeRule { rule, duration }
}

/// [`Effect::GainFlashback`]: "[cards] gain flashback until end of turn. The flashback cost
/// is equal to its mana cost."
pub const fn gain_flashback(cards: Objects) -> Effect {
    Effect::GainFlashback(cards)
}

/// [`Effect::PermitCast`]: "[player] may cast [cards] [duration]", on `terms`.
pub const fn permit_cast(
    player: Players,
    cards: Objects,
    duration: Duration,
    terms: CastTerms,
) -> Effect {
    Effect::PermitCast {
        player,
        cards,
        duration,
        terms,
    }
}

/// [`Effect::MayCast`] for nothing: "[player] may cast [card] without paying its mana cost".
pub const fn may_cast_free(player: Players, card: Objects) -> Effect {
    Effect::MayCast {
        player,
        card,
        free: true,
    }
}

/// [`Effect::UntapUpTo`]: "[player] untaps up to [count] [permanents]".
pub const fn untap_up_to(player: Players, count: u8, permanents: Objects) -> Effect {
    Effect::UntapUpTo {
        player,
        count: Amount::Fixed(count),
        permanents,
    }
}

/// [`Effect::Delay`] carrying nothing: "[when], [effect]", the delayed triggered ability
/// that is the `ability`th of its face, counting from 0.
pub const fn delay(ability: u8) -> Effect {
    Effect::Delay {
        ability: AbilityIx::new(ability),
        carrying: Amount::Fixed(0),
    }
}

/// [`Effect::GetEmblem`]: "[player] gets an emblem with [abilities]".
pub const fn get_emblem(player: Players, emblem: &'static CardDef) -> Effect {
    Effect::GetEmblem { player, emblem }
}

/// [`Effect::WinGame`]: "[player] wins the game".
pub const fn win_game(player: Players) -> Effect {
    Effect::WinGame(player)
}

/// [`Effect::LoseGame`]: "[player] loses the game".
pub const fn lose_game(player: Players) -> Effect {
    Effect::LoseGame(player)
}
