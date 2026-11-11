//! References: which objects and which players an instruction means.

use crate::filter::Filter;
use crate::ids::Zone;

/// Which objects: none, one or several
/// ([CR 109.1](https://yawgatog.com/resources/magic-rules/#R1091)): "An object is an ability
/// on the stack, a card, a copy of a card, a token, a spell, a permanent, or an emblem."
///
/// An instruction acts on each of them. One that has left the zone the instruction expects
/// it in is not acted on, but what the effect needs to know of it is still known
/// ([CR 608.2h](https://yawgatog.com/resources/magic-rules/#R6082h)): "if it's no longer in
/// that zone, or if the effect has moved it from a public zone to a hidden zone, the effect
/// uses the object's last known information."
#[derive(Clone, Copy, Debug)]
pub enum Objects {
    /// The object the ability is on, or the spell itself: "this creature", "this spell"
    /// ([CR 201.5](https://yawgatog.com/resources/magic-rules/#R2015)): "Text that refers to
    /// the object it's on by name means just that particular object".
    This,
    /// What was chosen for the `n`th instance of the word "target", counting from 0
    /// ([CR 115.1](https://yawgatog.com/resources/magic-rules/#R1151)): "The targets are
    /// object(s) and/or player(s) the spell or ability will affect." Only those still legal
    /// ([CR 608.2b](https://yawgatog.com/resources/magic-rules/#R6082b)): "A target that's no
    /// longer in the zone it was in when it was targeted is illegal."
    Target(u8),
    /// What the instruction before acted on: "it", "them", "that card", "those tokens" (the
    /// [module](crate::effect) says which instruction that is).
    Those,
    /// Every object in a zone that passes a filter, read for the ability's controller: "all
    /// creatures", "each artifact you control", "the cards in your graveyard". An effect
    /// means permanents unless it says otherwise
    /// ([CR 609.2](https://yawgatog.com/resources/magic-rules/#R6092)), so the zone is said.
    All { zone: Zone, filter: Filter },
}

/// Which players: none, one or several
/// ([CR 102.1](https://yawgatog.com/resources/magic-rules/#R1021)): "A player is one of the
/// people in the game."
///
/// Several players take an instruction in turn order from the active player
/// ([CR 101.4](https://yawgatog.com/resources/magic-rules/#R1014)).
#[derive(Clone, Copy, Debug)]
pub enum Players {
    /// "You"
    /// ([CR 109.5](https://yawgatog.com/resources/magic-rules/#R1095)): "The words 'you' and
    /// 'your' on an object refer to the object's controller".
    You,
    /// "Each opponent"
    /// ([CR 102.2](https://yawgatog.com/resources/magic-rules/#R1022)): "In a two-player
    /// game, a player's opponent is the other player." With more players, each of them
    /// ([CR 102.3](https://yawgatog.com/resources/magic-rules/#R1023)).
    EachOpponent,
    /// "Each player".
    EachPlayer,
    /// The players chosen for the `n`th instance of the word "target", counting from 0 (CR
    /// 115.1): "target player", "target opponent". Its [`crate::TargetSpec`] says which
    /// players may be chosen.
    Target(u8),
    /// "Its controller", "their controllers"
    /// ([CR 110.2](https://yawgatog.com/resources/magic-rules/#R1102)): "A permanent's
    /// controller is, by default, the player under whose control it entered the battlefield."
    /// Of an object that has no controller where it is, its owner (CR 109.5), and of one that
    /// has left, its last controller (CR 608.2h).
    ControllerOf(Objects),
    /// "Its owner"
    /// ([CR 108.3](https://yawgatog.com/resources/magic-rules/#R1083)): "The owner of a card
    /// in the game is the player who started the game with it in their deck."
    OwnerOf(Objects),
    /// "That player", "they": the player an [`crate::Effect::ForEachPlayer`] is being done
    /// for.
    ThatPlayer,
}
