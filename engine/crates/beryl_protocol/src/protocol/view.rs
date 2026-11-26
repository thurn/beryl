//! The view types: what the UI draws of the game, relative to the viewer.

use beryl_kernel::SlotId;
use serde::{Deserialize, Serialize};

use crate::card_records::PrintingIx;

use super::{
    CardType, Counter, DecisionView, Declaration, FaceName, GameEvent, Keyword, ManualAbility,
    NamedCounter, OutcomeView, RowName, Step, StopsSetting, TableView, ZoneName,
};

/// An object on the wire: its public slot, the number a [`Subject::Object`](crate::script::Subject) and an
/// [`ObjectView`](crate::protocol::ObjectView) name it by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ObjectId(pub u16);

impl ObjectId {
    /// The wire name of the object in an engine `slot`.
    pub fn of(slot: impl SlotId) -> Self {
        ObjectId(slot.num())
    }
}

impl From<u16> for ObjectId {
    fn from(slot: u16) -> Self {
        ObjectId(slot)
    }
}

impl std::fmt::Display for ObjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<ObjectId> for u16 {
    fn from(id: ObjectId) -> Self {
        id.0
    }
}

/// Everything the UI draws for one player at one moment: the game as they see it, where it
/// stands (their open decision, the opponent deciding, a step beginning, a stop or the end),
/// and what happened since the previous view. Players are named relative to the viewer.
///
/// A session emits a view as the game starts, as each step of the turn begins, before and so
/// after each of the opponent's decisions, at each of the viewer's, and at the end: every
/// event of the game is in exactly one view.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerView {
    pub status: Status,
    /// Public scope of the opponent's card handling while their decision is open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handling: Option<CardHandling>,
    /// Public attackers whose destination is still being chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_attackers: Option<Vec<ObjectId>>,
    pub turn: TurnView,
    pub players: Players,
    /// The library and graveyard both players share, in a shared-deck game; none otherwise.
    /// In a shared game both players' [`PlayerState::library`] and [`PlayerState::graveyard`]
    /// are empty, every object whose `zone` is `Library` or `Graveyard` is in these piles, and
    /// a client never reads such an object's `owner` or `zone_side`. Exile stays as it is: the
    /// client draws one exile pile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared: Option<SharedPiles>,
    /// Permanents, in the order they entered.
    pub battlefield: Vec<ObjectId>,
    /// Every object in a zone, by slot: the battlefield, the stack, and each player's library,
    /// hand, graveyard, exile and sideboard.
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
    pub script: Option<crate::script::Script>,
    /// The manual table's controls and state for this viewer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<TableView>,
}

impl PlayerView {
    /// The stack in display order, with the entry whose resolution is waiting for a decision
    /// at the top. Callers can omit spells whose cards have already moved to another zone.
    pub fn stack_with_resolving(&self) -> impl DoubleEndedIterator<Item = &StackItem> {
        let resolving = match &self.status {
            Status::Deciding { decision } => decision.resolving.as_ref(),
            _ => None,
        };
        self.stack.iter().chain(resolving)
    }
}

/// Where the game stands for the viewer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Status {
    /// The viewer must decide; the game waits for their answer.
    Deciding {
        decision: DecisionView,
    },
    /// The opponent is deciding: the next view follows once they have.
    Thinking,
    /// The manual table is live with no engine decision pending.
    Free,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Side {
    You,
    Opponent,
}

/// The `owner` and `controller` of every object in a shared library or graveyard
/// ([`PlayerView::shared`]), in every seat's view. No player owns such a card, and a client
/// never reads either field for one; the session names one fixed side so that a seat's
/// script can say what its view says.
pub const SHARED_PILE_SIDE: Side = Side::You;

/// The library and graveyard that both players share in a shared-deck game
/// ([`PlayerView::shared`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedPiles {
    /// Bottom first.
    pub library: Vec<ObjectId>,
    /// Bottom first.
    pub graveyard: Vec<ObjectId>,
}

/// Which part of the table the opponent is handling. The scope names a zone and how
/// much of it is in use, without the candidates, their identities or any picks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "zone", rename_all = "camelCase")]
pub enum CardHandling {
    /// The top `count` cards, or the whole library during a search. In a shared game `side` is
    /// the player who looks, and the library is the shared one ([`PlayerView::shared`]).
    Library {
        side: Side,
        count: Option<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scry: Option<bool>,
    },
    Hand {
        side: Side,
    },
    /// A manual table lifts a counted packet from a hidden pile.
    Pile {
        side: Side,
        pile: ZoneName,
        count: u32,
        bottom: bool,
    },
}

/// Face up or face down
/// ([CR 110.5](https://yawgatog.com/resources/magic-rules/#R1105)): one of a permanent's
/// statuses. Cards elsewhere are face up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Facing {
    Up,
    Down,
}

/// A game session, numbered from 1 in the order sessions start.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SessionId(pub u32);

/// Names one decision of the human's in one session. The UI echoes the one it received to
/// answer it, and never looks inside.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionId {
    pub session: SessionId,
    /// Numbers the session's decisions of the human's from 1.
    pub seq: u32,
}

/// A candidate of a decision, by its position in [`DecisionView::candidates`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateIndex(pub u16);

/// The count of the active player's own turns, the current one included, when the game's
/// `number`th turn is under way. The two sides take turns in strict alternation
/// ([CR 103.8](https://yawgatog.com/resources/magic-rules/#R1038): "The starting player takes
/// their first turn."), so turns 1 and 2 are each side's first, turns 3 and 4 their second.
#[must_use]
pub const fn own_number(number: u16) -> u16 {
    number.div_ceil(2)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnView {
    /// The game's turns so far, both players' together: 0 before the first turn. Orders turns.
    pub number: u16,
    /// The active player's own turn count, which is what players call "turn N": the starting
    /// player's first turn and the other player's first turn are both 1. See [`own_number`].
    pub own_number: u16,
    pub active: Side,
    pub step: Step,
    /// The side with engine priority, if the game uses priority.
    pub priority: Option<Side>,
    /// Who takes the first turn (CR 103.1); none while the coin flip's winner chooses.
    pub starting: Option<Side>,
    /// Who won the coin flip that chose who chooses, in a game begun by one: the session's to
    /// say ([`crate::Game`]), as the position no longer holds it.
    pub flip_winner: Option<Side>,
    /// The side controlling the active player this turn, when another player does
    /// ([CR 724.1]): that side makes the active player's decisions, so a view whose
    /// `controlled_by` is [`Side::You`] and whose `active` is [`Side::Opponent`] acts for the
    /// opponent.
    ///
    /// [CR 724.1]: https://yawgatog.com/resources/magic-rules/#R7241
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controlled_by: Option<Side>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Players {
    pub you: PlayerState,
    pub opponent: PlayerState,
}

/// One player's public state and their ordered zones. Their hand and exile are the objects
/// they own in those zones.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerState {
    pub life: i32,
    /// Unspent mana.
    pub mana: ManaPool,
    pub lands_played: u8,
    pub mulligans: u8,
    /// Where they are in the mulligan (CR 103.5): kept, once the game begins.
    pub mulligan: Declaration,
    /// Bottom first. Empty in a shared-deck game, whose library is [`PlayerView::shared`]'s.
    pub library: Vec<ObjectId>,
    /// Bottom first. Empty in a shared-deck game, whose graveyard is [`PlayerView::shared`]'s.
    pub graveyard: Vec<ObjectId>,
    /// The emblems' names.
    pub emblems: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub named_counters: Vec<NamedCounter>,
}

/// Mana by type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManaPool {
    pub white: u32,
    pub blue: u32,
    pub black: u32,
    pub red: u32,
    pub green: u32,
    pub colorless: u32,
}

/// One object, with its identity only if the viewer knows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag describes independent object state"
)]
pub struct ObjectView {
    pub slot: ObjectId,
    pub zone: ZoneName,
    /// Who owns the card. In a shared-deck game a client never reads it for an object in the
    /// shared library or graveyard ([`PlayerView::shared`]), where it is [`SHARED_PILE_SIDE`],
    /// as `controller` is.
    pub owner: Side,
    /// The player whose zone contains this card, when it differs from its owner.
    /// Only the manual table can put a card in another player's pile or hand. In a shared-deck
    /// game a client never reads it for an object in the shared library or graveyard.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone_side: Option<Side>,
    pub controller: Side,
    /// The name of the face up, which keys its record in the card store
    /// ([`crate::card_records`]); none for a card the viewer has not seen. A face-down
    /// permanent's controller may look at it (CR 708.5): its card's name, for them.
    pub name: Option<String>,
    /// Which printing of the card's name this slot shows, an index into the printing records of
    /// `beryl_card_records`; none is the card's ordinary printing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub printing: Option<PrintingIx>,
    /// The other face of a known multi-face card, when one of its faces is up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub other_face: Option<String>,
    /// Whether a permanent is face up or face down, part of its status (CR 110.5). A face-down
    /// one is a 2/2 creature with no name or text, whatever its card (CR 708.2a).
    pub facing: Facing,
    pub token: bool,
    pub tapped: bool,
    /// Can't attack or use its tap abilities yet (CR 302.6), if it is a creature.
    pub summoning_sick: bool,
    pub damage: i32,
    pub counters: Vec<CounterView>,
    /// On the battlefield: the card types after continuous effects.
    pub types: Vec<CardType>,
    /// On the battlefield, for creatures: power and toughness after continuous effects.
    pub pt: Option<PowerToughness>,
    /// On the battlefield: the keyword abilities it has after continuous effects, printed and
    /// gained alike ([CR 613.1f](https://yawgatog.com/resources/magic-rules/#R6131f): "Layer
    /// 6: Ability-adding effects, keyword counters, ability-removing effects, and effects that say an object
    /// can't have an ability are applied"), so that one gained until end of turn shows.
    pub keywords: Vec<Keyword>,
    /// On the battlefield: its face up has a mana ability ([CR 605.1a]): "An activated ability
    /// is a mana ability if it meets all of the following criteria", one of which is that "it
    /// could add mana to a player’s mana pool when it resolves", as a mana rock's or a
    /// Treasure's "{T}: Add" does. The table stands a noncreature artifact with one among the
    /// lands, as a mana source.
    ///
    /// [CR 605.1a]: https://yawgatog.com/resources/magic-rules/#R6051a
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mana_ability: bool,
    pub attached_to: Option<ObjectId>,
    /// The permanent that exiled it until it leaves (CR 610.3).
    pub exiled_by: Option<ObjectId>,
    /// The object whose linked ability exiled it, if that object's other abilities may still
    /// find it among the cards "exiled with" it
    /// ([CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a)).
    pub exiled_with: Option<ObjectId>,
    /// Phased out ([CR 702.26b]): on the battlefield by its zone, and treated as though it
    /// does not exist.
    ///
    /// [CR 702.26b]: https://yawgatog.com/resources/magic-rules/#R70226b
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub phased_out: bool,
    /// In the merged zone: the merged permanent this card is part of, drawn under it
    /// ([CR 731.2]).
    ///
    /// [CR 731.2]: https://yawgatog.com/resources/magic-rules/#R7312
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_of: Option<ObjectId>,
    /// On the battlefield, for a card a copy effect gives another card's values: the name of
    /// the card it really is ([CR 707.2]), while `name` is the copied card's.
    ///
    /// [CR 707.2]: https://yawgatog.com/resources/magic-rules/#R7072
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub really: Option<String>,
    /// On the battlefield: the value it chose as it entered, by name ("Red", "Goblin",
    /// "Counterspell", "Another card")
    /// ([CR 614.12a](https://yawgatog.com/resources/magic-rules/#R61412a)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chosen: Option<String>,
    /// The rules text and type line as a text-changing effect changed them (CR 612.1), in
    /// place of its card's printed ones; none while they read as printed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_line: Option<String>,
    /// A custom token's colors, including an empty list for colorless tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_colors: Option<Box<[String]>>,
    /// The record key of an exact token printing on a manual table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_printing: Option<Box<str>>,
    pub combat: Option<Combat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned: Option<RowName>,
    /// Which face the manual table is showing. The engine uses `name` and `facing`.
    #[serde(default, skip_serializing_if = "FaceName::is_front")]
    pub face: FaceName,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub named_counters: Vec<NamedCounter>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub doesnt_untap: bool,
    /// In a hand: its player picked it in a hidden pick of the resolution that is paused,
    /// which puts the cards picked onto the battlefield once every player has picked. Which
    /// card was picked is public; the card itself is not, so it is drawn set apart from the
    /// hand, face down for a viewer who does not know it ([CR 101.4a]): "each player must
    /// clearly indicate which face-down card they are choosing."
    ///
    /// [CR 101.4a]: https://yawgatog.com/resources/magic-rules/#R1014a
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden_pick: bool,
    /// In the viewer's own graveyard: the viewer may cast it from there, under flashback, escape
    /// or another permission, whatever the step and whether they could pay. The table shows it
    /// at the end of the viewer's hand, ringed apart from the cards really in it, as MTG Arena
    /// does. Never set for the opponent's cards.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub castable_from_graveyard: bool,
    /// On the battlefield: the designations it has that no counter or status shows, a
    /// Class's level, a solved Case and a prepared creature, in that order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub designations: Vec<DesignationView>,
}

/// A designation a permanent has. A designation is not a characteristic, a counter or a
/// status, so nothing else in an [`ObjectView`] shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DesignationView {
    /// A Class's level. A Class without a level designation is at level 1 ([CR 717.2d]): "it
    /// is treated as though its level is 1." Shown for every Class, level 1 included.
    ///
    /// [CR 717.2d]: https://yawgatog.com/resources/magic-rules/#R7172d
    Level { level: u8 },
    /// A Case that is solved ([CR 720.3b](https://yawgatog.com/resources/magic-rules/#R7203b)).
    Solved,
    /// A prepared creature ([CR 723.1](https://yawgatog.com/resources/magic-rules/#R7231)),
    /// whose controller may cast the copy of its prepare spell in exile.
    Prepared,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerToughness {
    pub power: i32,
    pub toughness: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CounterView {
    pub kind: Counter,
    pub count: i32,
}

/// A creature's part in combat.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Combat {
    /// Attacking a player or the named planeswalker, or still in combat after the target left.
    #[serde(rename_all = "camelCase")]
    Attacking {
        planeswalker: Option<ObjectId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_departed: Option<bool>,
        blocked: bool,
    },
    /// Blocking the attacker in `attacker`, if it is still in combat.
    #[serde(rename_all = "camelCase")]
    Blocking { attacker: Option<ObjectId> },
}

/// A spell or ability on the stack.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StackItem {
    /// The spell in `slot`, whose object says what it is and who controls it.
    Spell {
        slot: ObjectId,
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
        source: Option<ObjectId>,
        ability: u8,
        granted: bool,
        name: String,
        /// A triggered ability rather than an activated one.
        triggered: bool,
        /// Its cost lets permanents be tapped in place of mana, each a pick of its
        /// controller's: the taps are seen before the ability is.
        #[serde(default, rename = "tapsToPay", skip_serializing_if = "Option::is_none")]
        taps_to_pay: Option<bool>,
        /// How many paragraphs of its face's text say abilities of its kind (triggered, or
        /// activated) before the one that says it, so the UI finds its paragraph.
        nth: u8,
        /// Its rules text as its definition says it, when the printed text of its face has no
        /// paragraph that says it: an ability another object grants, or one past the printed
        /// paragraphs of its kind, as a token's or an emblem's with no text in the card store
        /// is. None where the UI finds the `nth` paragraph.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        controller: Side,
        /// Present only on manual table markers.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        manual: Option<ManualAbility>,
        targets: Vec<TargetRef>,
        /// The announced value of X.
        x: u8,
    },
}

/// An alternative or additional cost a spell was cast with
/// ([CR 601.2b](https://yawgatog.com/resources/magic-rules/#R6012b)): "If the spell has
/// alternative or additional costs that will be paid as it's being cast ... the player
/// announces their intentions to pay any or all of those costs".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TargetRef {
    Player {
        player: Side,
    },
    Object {
        slot: ObjectId,
    },
    /// The activated or triggered ability at `place` on the stack, counted from the bottom
    /// as the view's stack lists it. Only a target is one.
    Ability {
        place: u16,
    },
}
