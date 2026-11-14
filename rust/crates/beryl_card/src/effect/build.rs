//! Builders: the words a card file writes an [`Effect`] in.
//!
//! Each is a `const fn`, so a definition stays a `static`. A builder takes a
//! printed number; an instruction whose amount is worked out is written as its [`Effect`]
//! variant, with the [`Amount`] it reads:
//!
//! ```
//! use beryl_card::effect::build::{destroy, sequence, target, you};
//! use beryl_card::{Amount, Effect};
//!
//! // "Destroy target enchantment. You gain life equal to its mana value."
//! const EFFECT: Effect = sequence(&[
//!     destroy(target()),
//!     Effect::GainLife {
//!         player: you(),
//!         amount: Amount::ManaValue(target()),
//!     },
//! ]);
//! ```

use crate::continuous::{Duration, Modification, RuleChange};
use crate::cost::Cost;
use crate::counter::CounterKind;
use crate::definition::CardDef;
use crate::effect::{
    Amount, CastTerms, Comparison, Condition, CopyException, Effect, Found, LibraryOrder,
    LibraryPosition, Objects, Players, Recipients, Regeneration, Taken,
};
use crate::filter::Filter;
use crate::ids::Zone;
use crate::index::{AbilityIx, SpecIx};
use crate::mana::{ManaColor, ManaCost, ManaSet};
use crate::prompt::Prompt;

/// "This creature", "this spell": [`Objects::This`].
pub const fn this() -> Objects {
    Objects::This
}

/// The first target: [`Objects::Target`].
pub const fn target() -> Objects {
    nth_target(0)
}

/// "It", "them", "that card": [`Objects::Those`].
pub const fn those() -> Objects {
    Objects::Those
}

/// "That card", "that spell", of a triggered ability's event: [`Objects::TriggeredOn`].
pub const fn that_card() -> Objects {
    Objects::TriggeredOn
}

/// "That card from your graveyard": [`Objects::TriggeredOnIn`].
pub const fn that_card_from_your_graveyard() -> Objects {
    Objects::TriggeredOnIn(Zone::Graveyard)
}

/// "The cards exiled with this": [`Objects::ExiledWithThis`].
pub const fn exiled_with_this() -> Objects {
    Objects::ExiledWithThis
}

/// The `n`th target, counting from 0.
pub const fn nth_target(n: u8) -> Objects {
    Objects::Target(SpecIx::new(n))
}

/// The cards in `zone` that `players` own and that pass `filter`: [`Objects::InZoneOf`].
pub const fn cards_in(zone: Zone, players: &'static Players, filter: Filter) -> Objects {
    Objects::InZoneOf {
        zone,
        players,
        filter,
    }
}

/// "[Players]'s hand": every card in it.
pub const fn hand_of(players: &'static Players) -> Objects {
    cards_in(Zone::Hand, players, Filter::Any)
}

/// "[Players]'s library": every card in it.
pub const fn library_of(players: &'static Players) -> Objects {
    cards_in(Zone::Library, players, Filter::Any)
}

/// "[Players]'s graveyard": every card in it.
pub const fn graveyard_of(players: &'static Players) -> Objects {
    cards_in(Zone::Graveyard, players, Filter::Any)
}

/// "You": [`Players::You`].
pub const fn you() -> Players {
    Players::You
}

/// "Each opponent": [`Players::EachOpponent`].
pub const fn each_opponent() -> Players {
    Players::EachOpponent
}

/// "Each player": [`Players::EachPlayer`].
pub const fn each_player() -> Players {
    Players::EachPlayer
}

/// "Target player" or "target opponent", where it is the first target: [`Players::Target`].
pub const fn target_player() -> Players {
    nth_target_player(0)
}

/// The first target's controller, "its controller": [`Players::ControllerOf`].
pub const fn its_controller() -> Players {
    Players::ControllerOf(target())
}

/// The first target's owner, "its owner": [`Players::OwnerOf`].
pub const fn its_owner() -> Players {
    Players::OwnerOf(target())
}

/// "That player", within [`for_each_player`]: [`Players::ThatPlayer`].
pub const fn that_player() -> Players {
    Players::ThatPlayer
}

/// The player chosen for the `n`th target, counting from 0.
pub const fn nth_target_player(n: u8) -> Players {
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
pub const fn x() -> Amount {
    Amount::X
}

/// "That many": [`Amount::ThatMany`].
pub const fn that_many() -> Amount {
    Amount::ThatMany
}

/// "That card", the object a delayed triggered ability carries: [`Objects::Carried`].
pub const fn carried_object() -> Objects {
    Objects::Carried
}

/// The number a delayed triggered ability carries: [`Amount::Carried`].
pub const fn carried() -> Amount {
    Amount::Carried
}

/// "If you do": [`Condition::Did`].
pub const fn if_you_do() -> Condition {
    Condition::Did
}

/// "If you win the flip": [`Condition::WonFlip`].
pub const fn if_you_win_the_flip() -> Condition {
    Condition::WonFlip
}

/// "If you lose the flip": not [`Condition::WonFlip`].
pub const fn if_you_lose_the_flip() -> Condition {
    Condition::Not(&Condition::WonFlip)
}

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

/// [`Effect::Unless`] of mana: "[effect] unless [player] pays [cost]".
pub const fn unless_pays(effect: &'static Effect, player: Players, cost: ManaCost) -> Effect {
    unless(effect, player, Cost::mana(cost))
}

/// [`Effect::Unless`]: "[effect] unless [player] [pays a cost]".
pub const fn unless(effect: &'static Effect, player: Players, cost: Cost) -> Effect {
    Effect::Unless {
        effect,
        player,
        cost,
    }
}

/// [`Effect::ForEachPlayer`]: `effect` for each of `players`, who is [`that_player`] in it.
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
    Effect::Destroy {
        objects,
        regeneration: Regeneration::Allowed,
    }
}

/// [`Effect::Destroy`] with [`Regeneration::Forbidden`]: "destroy [permanents]. They can't be
/// regenerated."
pub const fn destroy_without_regeneration(objects: Objects) -> Effect {
    Effect::Destroy {
        objects,
        regeneration: Regeneration::Forbidden,
    }
}

/// [`Effect::Regenerate`]: "regenerate [permanents]".
pub const fn regenerate(objects: Objects) -> Effect {
    Effect::Regenerate(objects)
}

/// [`Effect::Attach`]: "attach [objects] to [host]".
pub const fn attach(objects: Objects, to: Objects) -> Effect {
    Effect::Attach { objects, to }
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

/// [`Effect::ReturnToBattlefield`]: "return [cards] to the battlefield under their owners'
/// control".
pub const fn return_to_battlefield(cards: Objects) -> Effect {
    Effect::ReturnToBattlefield(cards)
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

/// [`Effect::PutBack`]: "[player] puts [count] cards from their hand on top of their library
/// in any order".
pub const fn put_back(player: Players, count: u8) -> Effect {
    Effect::PutBack {
        player,
        count: Amount::Fixed(count),
    }
}

/// [`Effect::ShuffleInto`]: "[player] shuffles [cards] into their library".
pub const fn shuffle_into(player: Players, cards: Objects) -> Effect {
    Effect::ShuffleInto { player, cards }
}

/// [`Effect::Dig`]: "[player] looks at the top [look] cards of their library", takes some
/// into their hand and puts the rest on the bottom.
pub const fn dig(player: Players, look: u8, take: Taken, rest: LibraryOrder) -> Effect {
    Effect::Dig {
        player,
        look: Amount::Fixed(look),
        take,
        rest,
    }
}

/// [`Effect::Fateseal`] of one card: "[player] looks at the top card of [library]'s library
/// and may put it on the bottom of that library".
pub const fn fateseal(player: Players, library: Players) -> Effect {
    Effect::Fateseal {
        player,
        library,
        count: Amount::Fixed(1),
    }
}

/// [`Effect::Search`] for one card: "[player] searches their library for a [filter] card,
/// [puts it somewhere], then shuffles".
pub const fn search(player: Players, filter: Filter, found: Found) -> Effect {
    Effect::Search {
        player,
        filter,
        count: Amount::Fixed(1),
        found,
    }
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

/// [`Effect::DealDamage`] to the first target, a player or a permanent: "this deals [amount]
/// damage to any target", with [`crate::TargetSpec::any_target`].
pub const fn deal_damage_to_any_target(amount: u8) -> Effect {
    Effect::DealDamage {
        to: Recipients::target(SpecIx::new(0)),
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
/// [`carried`].
pub const fn delay_carrying(ability: u8, carrying: Amount) -> Effect {
    Effect::Delay {
        ability: AbilityIx::new(ability),
        carrying,
        object: None,
    }
}

/// [`Effect::Delay`] carrying an object: "[when], [effect]", the delayed triggered ability
/// that is the `ability`th of its face, which reads `object` as [`carried_object`].
pub const fn delay_with(ability: u8, object: Objects) -> Effect {
    Effect::Delay {
        ability: AbilityIx::new(ability),
        carrying: Amount::Fixed(0),
        object: Some(object),
    }
}

/// [`Effect::Modify`]: "[objects] get, gain or become [modifications] [duration]", the
/// parts of one effect that begin together.
pub const fn modify(
    objects: Objects,
    modifications: &'static [Modification],
    duration: Duration,
) -> Effect {
    Effect::Modify {
        objects,
        modifications,
        duration,
    }
}

/// [`Effect::ExchangeControl`]: "exchange control of [first] and [second]".
pub const fn exchange_control(first: Objects, second: Objects) -> Effect {
    Effect::ExchangeControl(first, second)
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
        object: None,
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
