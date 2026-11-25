//! Builders for the objects, players, amounts and conditions an instruction reads.

use crate::effect::{Amount, Comparison, Condition, Objects, Players};
use crate::filter::Filter;
use crate::ids::Zone;
use crate::index::SpecIx;

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

/// The top `count` cards of each named player's library.
pub const fn top_of_library(players: &'static Players, count: u8) -> Objects {
    Objects::LibraryTop { players, count }
}

/// The set minus the objects the preceding instruction acted on.
pub const fn except_those(objects: &'static Objects) -> Objects {
    Objects::ExceptThose(objects)
}

/// The `n`th target, counting from 0.
pub const fn nth_target(n: u8) -> Objects {
    Objects::Target(SpecIx::new(n))
}

/// The cards in `zone` that `players` own and that pass `filter`: [`Objects::InZoneOf`].
/// On the battlefield, this selects by controller.
pub const fn cards_in(zone: Zone, players: &'static Players, filter: Filter) -> Objects {
    Objects::InZoneOf {
        zone,
        players,
        filter,
    }
}

/// "[Filter] permanents", every player's: the objects on the battlefield that pass `filter`.
/// "Destroy all creatures" is `destroy(permanents(creature()))`.
pub const fn permanents(filter: Filter) -> Objects {
    Objects::All {
        zone: Zone::Battlefield,
        filter,
    }
}

/// Permanents these players control that pass `filter`.
pub const fn permanents_controlled_by(players: &'static Players, filter: Filter) -> Objects {
    cards_in(Zone::Battlefield, players, filter)
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

/// "That player", within [`for_each_player`](super::for_each_player): [`Players::ThatPlayer`].
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
