//! The view types: what the UI draws of the game, relative to the viewer.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{
    CardType, Counter, DecisionView, Declaration, GameEvent, Keyword, OutcomeView, Step,
    StopsSetting, ZoneName,
};

/// Everything the UI draws for one player at one moment: the game as they see it, where it
/// stands (their open decision, the opponent deciding, a step beginning, a stop or the end),
/// and what happened since the previous view. Players are named relative to the viewer.
///
/// A session emits a view as the game starts, as each step of the turn begins, before and so
/// after each of the opponent's decisions, at each of the viewer's, and at the end: every
/// event of the game is in exactly one view.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PlayerView {
    pub status: Status,
    /// Public scope of the opponent's card handling while their decision is open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub handling: Option<CardHandling>,
    /// Public attackers whose destination is still being chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pending_attackers: Option<Vec<u16>>,
    pub turn: TurnView,
    pub players: Players,
    /// Permanents, in the order they entered.
    pub battlefield: Vec<u16>,
    /// Every object in a zone, by slot: the battlefield, the stack, and each player's library,
    /// hand, graveyard and exile.
    pub objects: Vec<ObjectView>,
    /// Bottom first.
    pub stack: Vec<StackItem>,
    /// The viewer's priority stops.
    pub stops: StopsSetting,
    /// What happened since the previous view, oldest first, for animation.
    pub events: Vec<GameEvent>,
    /// The viewer's latest answer was taken back (`undo`): what happened since the decision
    /// asked again never did.
    pub undone: bool,
    /// What the viewer's screen does between the view before and this one
    /// ([`crate::script`]): every view a game in progress sends has one. A view built on its
    /// own, and one recorded before scripts were, has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub script: Option<crate::script::Script>,
}

/// Where the game stands for the viewer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Status {
    /// The viewer must decide; the game waits for their answer.
    Deciding {
        decision: DecisionView,
    },
    /// The opponent is deciding: the next view follows once they have.
    Thinking,
    /// A step of the turn began, and the game goes on through it without the viewer: the next
    /// view follows at once.
    Playing,
    /// The session stopped while the agent was deciding (it was replaced or closed): no
    /// decision opens again.
    Stopped,
    Over {
        outcome: OutcomeView,
    },
}

/// A player, relative to the viewer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Side {
    You,
    Opponent,
}

/// Which part of the table the opponent is handling. The scope names a zone and how
/// much of it is in use, without the candidates, their identities or any picks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "zone", rename_all = "camelCase")]
pub enum CardHandling {
    /// The top `count` cards, or the whole library during a search.
    Library {
        side: Side,
        count: Option<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        scry: Option<bool>,
    },
    Hand {
        side: Side,
    },
}

/// Face up or face down
/// ([CR 110.5](https://yawgatog.com/resources/magic-rules/#R1105)): one of a permanent's
/// statuses. Cards elsewhere are face up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Facing {
    Up,
    Down,
}

/// A game session, numbered from 1 in the order sessions start.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
pub struct SessionId(pub u32);

/// Names one decision of the human's in one session. The UI echoes the one it received to
/// answer it, and never looks inside.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DecisionId {
    pub session: SessionId,
    /// Numbers the session's decisions of the human's from 1.
    pub seq: u32,
}

/// A candidate of a decision, by its position in [`DecisionView::candidates`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CandidateIndex(pub u16);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TurnView {
    /// 0 before the first turn.
    pub number: u16,
    pub active: Side,
    pub step: Step,
    pub priority: Side,
    /// Who takes the first turn (CR 103.1); none while the coin flip's winner chooses.
    pub starting: Option<Side>,
    /// Who won the coin flip that chose who chooses, in a game begun by one: the session's to
    /// say ([`crate::Game`]), as the position no longer holds it.
    pub flip_winner: Option<Side>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Players {
    pub you: PlayerState,
    pub opponent: PlayerState,
}

/// One player's public state and their ordered zones. Their hand and exile are the objects
/// they own in those zones.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PlayerState {
    pub life: i16,
    /// Unspent mana.
    pub mana: ManaPool,
    pub lands_played: u8,
    pub mulligans: u8,
    /// Where they are in the mulligan (CR 103.5): kept, once the game begins.
    pub mulligan: Declaration,
    /// Bottom first.
    pub library: Vec<u16>,
    /// Bottom first.
    pub graveyard: Vec<u16>,
    /// The emblems' names.
    pub emblems: Vec<String>,
}

/// Mana by type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ManaPool {
    pub white: u8,
    pub blue: u8,
    pub black: u8,
    pub red: u8,
    pub green: u8,
    pub colorless: u8,
}

/// One object, with its identity only if the viewer knows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ObjectView {
    pub slot: u16,
    pub zone: ZoneName,
    pub owner: Side,
    pub controller: Side,
    /// The name of the face up, which keys the card data in `data/cards.json`; none for a card
    /// the viewer has not seen. A face-down permanent's controller may look at it (CR 708.5):
    /// its card's name, for them.
    pub name: Option<String>,
    /// Whether a permanent is face up or face down, part of its status (CR 110.5). A face-down
    /// one is a 2/2 creature with no name or text, whatever its card (CR 708.2a).
    pub facing: Facing,
    pub token: bool,
    pub tapped: bool,
    /// Can't attack or use its tap abilities yet (CR 302.6), if it is a creature.
    pub summoning_sick: bool,
    pub damage: u8,
    pub counters: Vec<CounterView>,
    /// On the battlefield: the card types after continuous effects.
    pub types: Vec<CardType>,
    /// On the battlefield, for creatures: power and toughness after continuous effects.
    pub pt: Option<PowerToughness>,
    /// On the battlefield: the keyword abilities it has after continuous effects, printed and
    /// gained alike ([CR 613.1f](https://yawgatog.com/resources/magic-rules/#R6131f): "Layer
    /// 6: Ability-adding effects, ability-removing effects, and effects that say an object
    /// can't have an ability are applied"), so that one gained until end of turn shows.
    pub keywords: Vec<Keyword>,
    pub attached_to: Option<u16>,
    /// The permanent that exiled it until it leaves (CR 610.3).
    pub exiled_by: Option<u16>,
    /// The object whose linked ability exiled it, if that object's other abilities may still
    /// find it among the cards "exiled with" it
    /// ([CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a)).
    pub exiled_with: Option<u16>,
    pub combat: Option<Combat>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PowerToughness {
    pub power: i8,
    pub toughness: i8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CounterView {
    pub kind: Counter,
    pub count: u8,
}

/// A creature's part in combat.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Combat {
    /// Attacking a player or the named planeswalker, or still in combat after the target left.
    #[serde(rename_all = "camelCase")]
    Attacking {
        planeswalker: Option<u16>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        target_departed: Option<bool>,
        blocked: bool,
    },
    /// Blocking the attacker in `attacker`, if it is still in combat.
    #[serde(rename_all = "camelCase")]
    Blocking { attacker: Option<u16> },
}

/// A spell or ability on the stack.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StackItem {
    /// The spell in `slot`, whose object says what it is and who controls it.
    Spell {
        slot: u16,
        targets: Vec<TargetRef>,
        /// The announced value of X.
        x: u8,
        /// The alternative and additional costs it was cast with, in its card's order: what
        /// they change, as an overload cost turns each "target" into "each", its printed text
        /// does not say.
        paid: Vec<PaidCost>,
    },
    /// The ability at index `ability` of the face named `name` (among the abilities it grants
    /// to other objects, when `granted`), from the object in `source`, which may have left its
    /// zone since; an emblem's ability has none.
    Ability {
        source: Option<u16>,
        ability: u8,
        granted: bool,
        name: String,
        /// A triggered ability rather than an activated one.
        triggered: bool,
        /// This activation has a Waterbend cost.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        waterbend: Option<bool>,
        /// How many abilities of its kind (triggered, or activated) come before it in its
        /// list: the face's text says them in that order, so the UI finds its paragraph.
        nth: u8,
        controller: Side,
        targets: Vec<TargetRef>,
        /// The announced value of X.
        x: u8,
    },
}

/// An alternative or additional cost a spell was cast with
/// ([CR 601.2b](https://yawgatog.com/resources/magic-rules/#R6012b)): "If the spell has
/// alternative or additional costs that will be paid as it's being cast ... the player
/// announces their intentions to pay any or all of those costs".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum PaidCost {
    Kicker,
    Offspring,
    Impending,
    Overload,
    Evoke,
    /// Cast from a graveyard for its flashback cost
    /// ([CR 702.34a](https://yawgatog.com/resources/magic-rules/#R70234a)).
    Flashback,
}

/// A player or an object, as a target or a recipient of damage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TargetRef {
    Player { player: Side },
    Object { slot: u16 },
}
