//! References: which objects and which players an instruction means.

use crate::cost::CostAction;
use crate::filter::Filter;
use crate::ids::Zone;
use crate::index::SpecIx;

/// Which objects: none, one or several
/// ([CR 109.1](https://yawgatog.com/resources/magic-rules/#R1091)): "An object is an ability
/// on the stack, a card, a copy of a card, a token, a spell, a permanent, or an emblem."
///
/// An instruction acts on each of them. One that has left the zone the instruction expects
/// it in is not acted on, but what the effect needs to know of it is still known
/// ([CR 608.2h](https://yawgatog.com/resources/magic-rules/#R6082h)): "if it's no longer in
/// that zone, or if the effect has moved it from a public zone to a hidden zone, the effect
/// uses the object's last known information."
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
    Target(SpecIx),
    /// The object this one is attached to: "enchanted creature", "equipped creature"
    /// ([CR 303.4b](https://yawgatog.com/resources/magic-rules/#R3034b)): "The object or
    /// player an Aura is attached to is called enchanted." Nothing while this is attached to
    /// nothing, or is no longer on the battlefield.
    AttachedTo,
    /// What the instruction before acted on: "it", "them", "that card", "those tokens" (the
    /// [module](crate::effect) says which instruction that is).
    Those,
    /// What the ability's trigger event was about: "that card", "that spell", "that creature"
    /// ([CR 603.2](https://yawgatog.com/resources/magic-rules/#R6032)): "Whenever a game
    /// event or game state matches a triggered ability's trigger event, that ability
    /// automatically triggers." The object as the event left it: one that has changed zones
    /// since is a new object
    /// ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)), and this is nothing.
    TriggeredOn,
    /// [`Objects::TriggeredOn`] while it is in its owner's `zone`: "that card from your
    /// graveyard". A discarded card that a replacement effect put somewhere else
    /// ([CR 614.1a](https://yawgatog.com/resources/magic-rules/#R6141a)) is not in the
    /// graveyard, and this is nothing.
    TriggeredOnIn(Zone),
    /// "The cards exiled with this": those in exile that an ability of this object exiled,
    /// linked to it ([`crate::Effect::ExileLinked`];
    /// [CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a)): "The second ability
    /// refers only to cards in the exile zone that were put there as a result of an
    /// instruction to exile them in the first ability." Known whether or not this is still
    /// where it was.
    ExiledWithThis,
    /// Every object in a zone that passes a filter, read for the ability's controller: "all
    /// creatures", "each artifact you control", "the cards in your graveyard". An effect
    /// means permanents unless it says otherwise
    /// ([CR 609.2](https://yawgatog.com/resources/magic-rules/#R6092)), so the zone is said.
    All { zone: Zone, filter: Filter },
    /// The cards in a zone each player has one of, of the players named, that pass a filter:
    /// "target opponent's hand", "a nonland card from it", "that player's graveyard"
    /// ([CR 400.1](https://yawgatog.com/resources/magic-rules/#R4001)): "Each player has their
    /// own library, hand, and graveyard." The filter is read for the ability's controller.
    InZoneOf {
        zone: Zone,
        players: &'static Players,
        filter: Filter,
    },
    /// Every card in several zones of these players, as one set: "their hand and
    /// graveyard". Zones each player has one of
    /// ([CR 400.1](https://yawgatog.com/resources/magic-rules/#R4001)): "Each player has
    /// their own library, hand, and graveyard." One zone is [`Objects::InZoneOf`].
    CardsIn {
        players: &'static Players,
        zones: &'static [Zone],
    },
    /// What paying the cost of the spell or ability did this to: "the sacrificed creature",
    /// "the exiled card", "the revealed card"
    /// ([CR 608.2k](https://yawgatog.com/resources/magic-rules/#R6082k)): "If an ability's
    /// effect refers to a specific untargeted object that has been previously referred to by
    /// that ability's cost or trigger condition, it still affects that object even if the
    /// object has changed characteristics." What a cost moved is known as it last was
    /// (CR 608.2h).
    ///
    /// The engine does not keep what a cost took, so a card table refuses a definition that
    /// names it (`beryl_kernel`'s `Unpayable::Named`). When a card needs it, the step that
    /// pays a part with objects has each one remember, as a memo, the spell or the source
    /// it paid for, and this reads the objects that remember the one resolving.
    Paid(CostAction),
}

impl Objects {
    /// Whether it is what a cost took ([`Objects::Paid`]).
    pub const fn names_paid(&self) -> bool {
        matches!(self, Objects::Paid(_))
    }
}

/// Which players: none, one or several
/// ([CR 102.1](https://yawgatog.com/resources/magic-rules/#R1021)): "A player is one of the
/// people in the game."
///
/// Several players take an instruction in turn order from the active player
/// ([CR 101.4](https://yawgatog.com/resources/magic-rules/#R1014)).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
    /// "Each player": everyone in the game (CR 102.1), whoever controls the effect.
    EachPlayer,
    /// The players chosen for the `n`th instance of the word "target", counting from 0 (CR
    /// 115.1): "target player", "target opponent". Its [`crate::TargetSpec`] says which
    /// players may be chosen: any for "target player", and for "target opponent" those a
    /// [`Filter::Whose`] of [`Players::EachOpponent`] accepts.
    Target(SpecIx),
    /// "Its controller", "their controllers"
    /// ([CR 110.2](https://yawgatog.com/resources/magic-rules/#R1102)): "A permanent's
    /// controller is, by default, the player under whose control it entered the battlefield."
    /// Of an object that has no controller where it is, its owner (CR 109.5). Of one an
    /// instruction of the same effect moved, its last controller (CR 608.2h), which is kept
    /// for a target ([`Objects::Target`]) and for what the instruction before moved
    /// ([`Objects::Those`]) and for nothing else: of the ability's source, what that is
    /// attached to or what it triggered on, once moved, it is refused
    /// ([`crate::Malformed::LastKnown`]).
    ControllerOf(Objects),
    /// "Its owner"
    /// ([CR 108.3](https://yawgatog.com/resources/magic-rules/#R1083)): "The owner of a card
    /// in the game is the player who started the game with it in their deck."
    OwnerOf(Objects),
    /// "That player", "they": the player an [`crate::Effect::ForEachPlayer`] is being done
    /// for.
    ThatPlayer,
    /// "Each player other than [players]", "each other player": everyone in the game (CR
    /// 102.1) but them.
    EachOther(&'static Players),
}

/// What damage is dealt to
/// ([CR 120.1](https://yawgatog.com/resources/magic-rules/#R1201)): "Objects can deal damage
/// to battles, creatures, planeswalkers, and players."
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Recipients {
    /// "To you", "to each opponent", "to that player".
    Players(Players),
    /// "To target creature", "to each creature": the permanents among them.
    Permanents(Objects),
}

impl Recipients {
    /// Whether it reads what a cost took ([`Objects::Paid`]).
    pub const fn names_paid(&self) -> bool {
        match self {
            Recipients::Players(players) => players.names_paid(),
            Recipients::Permanents(objects) => objects.names_paid(),
        }
    }
}

impl Objects {
    /// Whether this means something only a resolving spell or ability has: its targets, or
    /// what an instruction before acted on. An expression that does can be read for a
    /// resolution and for nothing else, a static ability least of all
    /// ([`crate::LayerSpec::needs_a_resolution`]).
    pub const fn needs_a_resolution(&self) -> bool {
        match self {
            Objects::This | Objects::AttachedTo | Objects::ExiledWithThis => false,
            Objects::Target(_)
            | Objects::Those
            | Objects::Paid(_)
            | Objects::TriggeredOn
            | Objects::TriggeredOnIn(_) => true,
            Objects::All { filter, .. } => filter.needs_a_resolution(),
            Objects::InZoneOf {
                players, filter, ..
            } => players.needs_a_resolution() || filter.needs_a_resolution(),
            Objects::CardsIn { players, .. } => players.needs_a_resolution(),
        }
    }
}

impl Players {
    /// Whether this means something only a resolving spell or ability has
    /// ([`Objects::needs_a_resolution`]): its targets, or the player an instruction is being
    /// done for.
    pub const fn needs_a_resolution(&self) -> bool {
        match self {
            Players::You | Players::EachOpponent | Players::EachPlayer => false,
            Players::Target(_) | Players::ThatPlayer => true,
            Players::ControllerOf(objects) | Players::OwnerOf(objects) => {
                objects.needs_a_resolution()
            }
            Players::EachOther(players) => players.needs_a_resolution(),
        }
    }
}

impl Players {
    /// Whether it reads what a cost took ([`Objects::Paid`]).
    pub const fn names_paid(&self) -> bool {
        match self {
            Players::ControllerOf(objects) | Players::OwnerOf(objects) => objects.names_paid(),
            Players::EachOther(players) => players.names_paid(),
            Players::You
            | Players::EachOpponent
            | Players::EachPlayer
            | Players::Target(_)
            | Players::ThatPlayer => false,
        }
    }
}
