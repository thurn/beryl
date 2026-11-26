//! Builders for instructions that move objects between zones, or search, draw, discard,
//! mill and otherwise look through a library.

use crate::effect::{
    Amount, Effect, Found, LibraryOrder, LibraryPosition, Objects, Players, PutOnto, Regeneration,
    Taken,
};
use crate::filter::Filter;
use crate::ids::Zone;

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

/// [`Effect::CounterInto`] the top of its owner's library: "counter [spell]. If that spell is
/// countered this way, put it on top of its owner's library instead of into that player's
/// graveyard."
pub const fn counter_to_top_of_library(spells: Objects) -> Effect {
    Effect::CounterInto {
        spells,
        zone: Zone::Library,
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

/// [`Effect::PutOntoBattlefield`]: "return [objects] to the battlefield under [player]'s
/// control", untapped and not linked: [`PutOnto`] says the rest.
pub const fn put_onto_battlefield(onto: &'static PutOnto) -> Effect {
    Effect::PutOntoBattlefield(onto)
}

/// "Return [objects] to the battlefield under [player]'s control": a [`PutOnto`] for
/// [`put_onto_battlefield`], untapped and not linked.
pub const fn onto_battlefield_under(objects: Objects, player: Players) -> PutOnto {
    PutOnto {
        objects,
        under: Some(player),
        tapped: false,
        linked: false,
    }
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

/// [`Effect::PutAnyOnBottom`]: "[player] puts any number of cards from their hand on the
/// bottom of their library."
pub const fn put_any_on_bottom(player: Players) -> Effect {
    Effect::PutAnyOnBottom { player }
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

/// The controller may reveal a matching card they own from outside the game and
/// put it into their hand.
pub const fn wish(filter: Filter) -> Effect {
    Effect::Wish { filter }
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
