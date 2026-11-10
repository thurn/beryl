//! The `PlayerView` protocol (architecture §19.2, decision D14): what the UI draws, built from
//! one player's [`Observation`] so it carries nothing that player does not know. Every type
//! here has a generated TypeScript twin in `ui/src/generated/protocol.ts`
//! ([`crate::typescript`]).

use beryl_engine::card::{
    Ability, AbilityIx, AbilityKind, CastingOption, Castings, Colors, Cost, Face, FaceAbility,
    FaceIx, Keywords, ManaColor, ManaCost, Types,
};
use beryl_engine::event::{Event, LossCause, Reason, Recipient};
use beryl_engine::layout::Queue;
use beryl_engine::state::{
    CounterKind, MulliganStatus, ObjectFlags, ObjectRef, StackEntry, TurnStep,
};
use beryl_engine::{
    Ask, Candidates, Decision, DecisionKind, GameTable, Layout, NextStop, Observation, Opt,
    Outcome, Refusal, Seat, SlotId, Small, StallReason, StepStops, Stops, Zone,
    can_declare_blockers, next_step_stop, next_stop, refusal,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Everything the UI draws for one player at one moment: the game as they see it, where it
/// stands (their open decision, the opponent deciding, a step beginning, a stop or the end),
/// and what happened since the previous view. Players are named relative to the viewer.
///
/// A session emits a view as the game starts, as each step of the turn begins, before and so
/// after each of the opponent's decisions, at each of the viewer's, and at the end: every
/// event of the game is in exactly one view.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PlayerView {
    pub status: Status,
    /// Public scope of the opponent's card handling while their decision is open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub handling: Option<CardHandling>,
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
    /// Attacking a player, or the planeswalker in `planeswalker`.
    #[serde(rename_all = "camelCase")]
    Attacking {
        planeswalker: Option<u16>,
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

/// The viewer's decision, its candidates in the order a [`CandidateIndex`] counts them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DecisionView {
    /// The suggested waterbend payment, editable without committing the taps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub waterbend: Option<crate::ipc::WaterbendPreview>,
    /// What an answer names.
    pub id: DecisionId,
    pub kind: Kind,
    pub candidates: Vec<CandidateView>,
    /// The spell or ability whose resolution asks this, if one does. It has left the stack
    /// list by then, as the first step of resolving it
    /// ([CR 608.2](https://yawgatog.com/resources/magic-rules/#R6082)): "If the object that's
    /// resolving is an instant spell, a sorcery spell, or an ability, its resolution may involve
    /// several steps." A resolving spell's card is still among the objects, on the stack.
    pub resolving: Option<StackItem>,
    /// What the decision asks, where its kind alone does not say.
    pub ask: AskView,
    /// For a priority decision, where passing takes the viewer, for the pass button.
    pub pass_to: Option<PassTo>,
    /// For a priority where passing stops the viewer again in this step (the top of the stack
    /// resolving, or combat damage), where the `pass_on` command takes them instead: on to
    /// their next stop in a later step, unless the opponent does something first.
    pub pass_on: Option<PassTo>,
    /// The card the decision is for: the spell or ability resolving, or else the one on top of
    /// the stack (which a target, an X or a payment is chosen for); the attacker whose combat
    /// damage is divided; the land asking for life. None for priority and the other combat
    /// declarations.
    pub source: Option<u16>,
    /// The session halts the viewer here: a priority where passing is all they can do,
    /// which the engine would pass for them, offered under the smart stops at the start of
    /// their own precombat main phase (an end-turn button), so their turn never ends unseen.
    pub halt: bool,
    /// The decision is part of a spell the viewer is casting or an ability they are
    /// activating from their priority, whose costs are not yet paid: the `cancel` command
    /// takes it back.
    pub cancellable: bool,
    /// How many of the viewer's answers the `undo` command can take back, latest first, across
    /// the opponent's decisions and turns. Automatic cast follow-ups go back with the preceding
    /// answer. Zero with nothing to take back.
    pub undo: u16,
    /// How many of the viewer's answers stand before this decision, counting those too old
    /// to take back: its place among them, the same whenever an undo comes back to it, which
    /// the UI marks its log by.
    pub undo_depth: u32,
    /// At a priority, each card in the viewer's hand they may neither cast nor play as a land,
    /// and why: what the UI says of one a click finds nothing to do with.
    pub refused: Vec<Refused>,
}

/// A card in the viewer's hand they may neither cast nor play as a land now.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Refused {
    pub slot: u16,
    pub reason: RefusalView,
}

/// Why a card can't be played ([`Refusal`]), by public slot: the first rule its most nearly
/// legal way of being played breaks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RefusalView {
    /// Only at sorcery speed, which it is not. `by`: the opponent's permanent limiting the
    /// viewer to sorcery speed, as Teferi, Time Raveler does, if the card could be cast now
    /// but for it.
    Timing { by: Option<u16> },
    /// A land, and the viewer has played their land this turn.
    LandPlayed,
    /// Its mana cost can't be paid.
    Mana,
    /// Another of its costs, such as a card to exile, can't be paid.
    Cost,
    /// A target it requires can't be chosen.
    Target,
}

/// What a decision asks ([`Ask`]), by public slot and relative to the viewer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AskView {
    /// Creatures whose attack destination the player must choose.
    AttackTarget { attackers: Vec<u16> },
    /// Nothing more than the decision's kind says.
    Plain,
    /// Scry `count`: a `chooseUpTo` picks a card for the bottom, a `chooseObject` the next
    /// card for the top.
    Scry { count: u8 },
    /// A look at the cards in `player`'s `zone` (the top `count` of a library, or a whole
    /// hand) that may move one of the `eligible` cards to `to`: a `yesNo` whether to, then a
    /// `chooseObject` for which.
    Look {
        player: Side,
        zone: ZoneName,
        count: u8,
        to: ZoneName,
        eligible: Vec<u16>,
    },
    /// A card from the hand to put on top of the library, `count` of them still to put, each
    /// on the one before.
    PutBack { count: u8 },
    /// Whether to put the top card of `player`'s library on the bottom.
    Fateseal { player: Side },
    /// A `chooseObject` for a card to put into the hand from the top `count` of the
    /// viewer's library, which they have looked at, `take` of them still to put.
    Take { count: u8, take: u8 },
    /// A `chooseObject` for the next of the top `count` cards of the viewer's library, which
    /// they have looked at, to put on the bottom, under the ones before.
    Bottom { count: u8 },
    /// Whether to discard a basic land card rather than `count` cards.
    DiscardBasicLand { count: u8 },
    /// Whether the card in `card` goes on top of its owner's library (yes) or the bottom
    /// (no).
    TopOrBottom { card: u16 },
    /// Whether to move the card in `card` to `to`, as "you may exile that card".
    MayMove { card: u16, to: ZoneName },
    /// Whether the card an explore revealed goes to the graveyard.
    Explore,
    /// A `chooseObject` for which of the top two cards of the viewer's library, which they
    /// have looked at, to manifest as they manifest dread, the other going to the graveyard.
    ManifestDread,
    /// A card from the viewer's `zone`, their hand or graveyard, to exile to pay a cost,
    /// `count` of them still to exile. From the graveyard they are asked for from the bottom
    /// up, each above the one before, the lowest of interchangeable ones offered.
    ExileCards { zone: ZoneName, count: u8 },
    /// A `chooseUpTo` for a card a library search finds, up to `max` of them.
    Search { max: u8 },
    /// A `yesNo` whether to search the viewer's library at all.
    MaySearch,
    /// Permanents to untap, up to `max`.
    Untap { max: u8 },
    /// A `crew` decision's creature to tap for a crew cost of `crew` power, `left` of it
    /// still to tap.
    Crew { crew: u8, left: u8 },
    /// A `waterbend` decision's artifact or creature to tap for one generic mana of the cost,
    /// `cost` still to pay, as Oracle text writes it.
    Waterbend { cost: String },
    /// Whether to pay `cost`, as Oracle text writes it, so that the effect does not happen.
    UnlessPays { cost: String },
    /// Whether to pay `life` so that a land doesn't enter tapped.
    PayLife { life: u8 },
    /// A card to discard in the cleanup step, `count` of them still to discard to reach the
    /// maximum hand size.
    Cleanup { count: u8 },
}

/// Where passing priority takes the viewer if the other player passes too ([`NextStop`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PassTo {
    /// The top of the stack resolves.
    Resolve,
    /// Combat damage is dealt.
    CombatDamage,
    /// The viewer next gets priority, or declares attackers, in this step of the turn.
    Step { step: Step },
    /// The turn ends.
    NextTurn,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CandidateView {
    /// A short English description, such as "Cast Mana Drain".
    pub label: String,
    pub option: Candidate,
    /// For a candidate that casts a spell or activates an ability, or answers its X or its
    /// payment plan: each way the cast can then be paid, with what it taps. None otherwise.
    pub payments: Vec<Payment>,
    /// For a candidate that declares a blocker: the attackers it may block, by slot, one of
    /// each set of interchangeable attackers (architecture §7.2). None otherwise.
    pub blocks: Vec<u16>,
    /// For a candidate that activates an ability of the object's own face, or puts a waiting
    /// trigger on the stack: how many abilities of its kind (activated, mana abilities among
    /// them, or triggered) its face lists before it, as [`StackItem::Ability`]'s `nth` counts
    /// them, so the UI finds the ability's paragraph.
    pub nth: Option<u8>,
    /// For a candidate that announces one or more targets: what the first of them may be, one
    /// of each set of interchangeable objects (§7.2), so the UI can take the count and the
    /// target as one pick. None otherwise.
    pub targets: Vec<TargetRef>,
}

/// One way a cast or activation is paid, found by playing it out on a copy of the game: the
/// value of X and the payment plan it takes, where the cast asks for them, and the permanents
/// it taps. The engine's default payment is the first with a given X.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Payment {
    pub x: Option<u8>,
    /// The index of the payment plan among those the cast offers (they differ in what they
    /// consume or leave floating, architecture §10).
    pub plan: Option<u16>,
    /// Permanents, by slot.
    pub taps: Vec<u16>,
    /// For a payment at a value of X: the total mana cost that value makes, as Oracle text
    /// writes costs, so the UI can say what each X costs.
    pub cost: Option<String>,
}

/// What a candidate does, by public slot (architecture §7.1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Candidate {
    /// The object in this slot, for a decision among objects.
    Object {
        slot: u16,
    },
    Pass,
    PlayLand {
        slot: u16,
    },
    Play,
    Draw,
    Keep,
    Mulligan,
    /// The waiting triggered ability at `index`, from the object in `source`: its face's
    /// `ability`th, by which a later decision's candidates (each index shifts as triggers go
    /// onto the stack) still name it.
    Trigger {
        index: u16,
        source: Option<u16>,
        ability: u8,
    },
    Yes,
    No,
    Done,
    Attack {
        slot: u16,
    },
    /// Attack the planeswalker in `planeswalker`, or the defending player.
    Defender {
        planeswalker: Option<u16>,
    },
    Block {
        slot: u16,
    },
    Lethal {
        slot: u16,
    },
    /// Cast with face `face` up, paying the face's casting options at the indices in `costs`.
    Cast {
        slot: u16,
        face: u8,
        costs: Vec<u8>,
    },
    /// The ability at index `ability` among the object's own, or, when `granted`, among
    /// those other objects grant it.
    Activate {
        slot: u16,
        ability: u8,
        granted: bool,
    },
    Mana {
        slot: u16,
        color: Mana,
    },
    Pool {
        color: Mana,
    },
    Number {
        n: u8,
    },
    Plan {
        index: u16,
    },
    Player {
        player: Side,
    },
    /// Turn the face-down permanent in `slot` face up, paying its card's mana cost.
    TurnFaceUp {
        slot: u16,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum OutcomeView {
    Win {
        winner: Side,
        /// Why the other player lost.
        reason: LossReason,
        /// The card that made them lose, when one did and the session saw it.
        by: Option<Culprit>,
    },
    Draw,
    /// The engine gave up when a budget or capacity ran out; not a rules outcome.
    Stalled {
        reason: Stall,
    },
}

/// Why a player lost the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum LossReason {
    /// [CR 704.5a](https://yawgatog.com/resources/magic-rules/#R7045a): "If a player has 0 or
    /// less life, that player loses the game."
    Life,
    /// [CR 704.5b](https://yawgatog.com/resources/magic-rules/#R7045b): "If a player attempted
    /// to draw a card from a library with no cards in it since the last time state-based
    /// actions were checked, that player loses the game."
    Library,
    /// An effect had the other player win
    /// ([CR 104.2b](https://yawgatog.com/resources/magic-rules/#R1042b)): "An effect may state
    /// that a player wins the game."
    Effect,
    /// [CR 104.3a](https://yawgatog.com/resources/magic-rules/#R1043a): "A player can concede
    /// the game at any time."
    Concede,
}

/// The card that ended the game: the one whose effect had a player win or lose, or the source
/// of the damage that took a player's last life.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Culprit {
    /// The name of its face.
    pub name: String,
    /// Which of its abilities did it, as its card's text orders them; none for a spell, or
    /// for damage.
    pub ability: Option<AbilityRef>,
}

/// One of a face's abilities, as its rules text finds it: the `nth` of its `kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AbilityRef {
    pub kind: AbilityClass,
    pub nth: u8,
}

/// The kinds of ability a face's rules text tells apart
/// ([CR 113.3](https://yawgatog.com/resources/magic-rules/#R1133)): "There are four general
/// categories of abilities", of which spell abilities have no paragraph of their own and mana
/// abilities are written as activated ones are.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum AbilityClass {
    Triggered,
    Activated,
    /// A static ability, one that creates a replacement effect included.
    Static,
}

/// Where the viewer gets priority (architecture §15, decision D13).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct StopsSetting {
    /// Full control: every priority the viewer receives, even where passing is all they can
    /// do. Otherwise the smart stops: wherever the viewer can act, an opponent's doings on the
    /// stack and the steps below; and their own precombat main phase in any case, while it is
    /// one of them.
    pub full_control: bool,
    /// The steps of the viewer's own turns where they stop with the stack empty, as Arena's
    /// phase stops: by default their main phases, and combat once creatures attack.
    #[ts(as = "Vec<Step>")]
    pub yours: StepSet,
    /// The steps of the opponent's turns where the viewer stops with the stack empty: by
    /// default combat once creatures attack, and the end step.
    #[ts(as = "Vec<Step>")]
    pub theirs: StepSet,
}

impl StopsSetting {
    /// The smart stops' default steps.
    pub const SMART: StopsSetting = StopsSetting {
        full_control: false,
        yours: StepSet::of(&[
            TurnStep::PrecombatMain,
            TurnStep::DeclareAttackers,
            TurnStep::DeclareBlockers,
            TurnStep::PostcombatMain,
        ]),
        theirs: StepSet::of(&[
            TurnStep::DeclareAttackers,
            TurnStep::DeclareBlockers,
            TurnStep::End,
        ]),
    };
    /// Full control.
    pub const EXACT: StopsSetting = StopsSetting {
        full_control: true,
        ..StopsSetting::SMART
    };

    pub fn stops(self) -> Stops {
        if self.full_control {
            Stops::Exact
        } else {
            Stops::Smart
        }
    }

    /// The steps as the engine holds them for the viewer.
    pub fn step_stops(self) -> StepStops {
        TurnStep::ALL
            .into_iter()
            .fold(StepStops::NONE, |stops, step| {
                stops.with(step, true, self.yours.has(step)).with(
                    step,
                    false,
                    self.theirs.has(step),
                )
            })
    }
}

impl Default for StopsSetting {
    fn default() -> StopsSetting {
        StopsSetting::SMART
    }
}

/// A set of steps of the turn, which the wire carries as a list of [`Step`]s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct StepSet(u16);

impl StepSet {
    pub const fn of(steps: &[TurnStep]) -> StepSet {
        let mut bits = 0;
        let mut i = 0;
        while i < steps.len() {
            bits |= 1 << steps[i] as u16;
            i += 1;
        }
        StepSet(bits)
    }

    pub const fn has(self, step: TurnStep) -> bool {
        self.0 & (1 << step as u16) != 0
    }

    pub fn steps(self) -> impl Iterator<Item = TurnStep> {
        TurnStep::ALL
            .into_iter()
            .filter(move |&step| self.has(step))
    }
}

impl Serialize for StepSet {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.steps().map(Step::from))
    }
}

impl<'de> Deserialize<'de> for StepSet {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<StepSet, D::Error> {
        let steps = Vec::<Step>::deserialize(deserializer)?;
        Ok(StepSet(steps.into_iter().fold(0, |bits, step| {
            bits | 1 << TurnStep::from(step) as u16
        })))
    }
}

/// One thing that happened (architecture §8.1), by public slot and relative to the viewer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum GameEvent {
    /// The object now in `slot` arrived from another zone.
    ZoneChange {
        slot: u16,
        from: ZoneName,
        to: ZoneName,
        reason: Why,
        /// Whether a spell went to the top or the bottom of its owner's library.
        #[serde(rename = "libraryTop", skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        library_top: Option<bool>,
    },
    Damage {
        source: u16,
        to: TargetRef,
        amount: u8,
        combat: bool,
    },
    Draw {
        player: Side,
        cards: u8,
        /// The public slots drawn, top first. Older recordings carry only the count.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        slots: Option<Vec<u16>>,
    },
    LibraryShuffled {
        player: Side,
    },
    LifeGain {
        player: Side,
        amount: u8,
    },
    LifeLoss {
        player: Side,
        amount: u8,
    },
    CountersAdded {
        slot: u16,
        counter: Counter,
        amount: u8,
    },
    CountersRemoved {
        slot: u16,
        counter: Counter,
        amount: u8,
    },
    Tap {
        slot: u16,
    },
    Untap {
        slot: u16,
    },
    /// Mana added to a player's mana pool
    /// ([CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064)): "When an effect
    /// instructs a player to add mana, that mana goes into a player's mana pool." The UI
    /// shows it flow from its source to the spell or ability it pays for.
    Mana {
        player: Side,
        /// The mana ability's permanent, or the spell or ability that added it, in the slot
        /// it had then: a source sacrificed for mana has left it by the time the view shows.
        source: Option<u16>,
        color: Mana,
        amount: u8,
    },
    /// Mana paid into the named resolving spell or ability.
    ResolutionPayment {
        player: Side,
        item: StackItem,
    },
    TokensCreated {
        controller: Side,
        tokens: Vec<u16>,
    },
    Cast {
        slot: u16,
        controller: Side,
        /// The spell as it went on the stack: an opponent's spell that resolves unanswered
        /// is gone from the stack by the next view, which shows its cast and resolution
        /// together.
        item: StackItem,
    },
    /// An ability was activated, or a triggered one with targets put on the stack, as the
    /// stack then showed it.
    Ability {
        item: StackItem,
        /// A loyalty ability's cost: the loyalty counters it added or removed, which the
        /// activation says rather than the counters
        /// ([CR 606.4](https://yawgatog.com/resources/magic-rules/#R6064)): "The cost to activate
        /// a loyalty ability of a permanent is to put on or remove from that permanent a certain
        /// number of loyalty counters, as shown by the loyalty symbol in the ability's cost."
        loyalty: Option<i8>,
    },
    Attack {
        player: Side,
        attackers: Vec<u16>,
    },
    StepBegins {
        step: Step,
        active: Side,
    },
    GainControl {
        slot: u16,
        player: Side,
    },
    CoinFlip {
        player: Side,
        won: bool,
        /// The permanent whose ability flipped it.
        source: u16,
    },
    Lose {
        player: Side,
    },
    Win {
        player: Side,
    },
}

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
        Priority, PlayOrDraw, KeepOrMulligan, PutOnBottom, Discard, LegendRule, TriggerOrder,
        PayLife, Attacker, AttackTarget, Blocker, BlockTarget, DamageDivision, X, TargetCount,
        Target, PaymentPlan, ManaPayment, CastOffer, YesNo, ChooseObject, Crew, ChooseUpTo, Waterbend,
        ExileCost,
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
    const ALL: [(Keywords, Keyword); 4] = [
        (Keywords::FIRST_STRIKE, Keyword::FirstStrike),
        (Keywords::FLYING, Keyword::Flying),
        (Keywords::REACH, Keyword::Reach),
        (Keywords::VIGILANCE, Keyword::Vigilance),
    ];
}

impl CardType {
    const ALL: [(Types, CardType); 7] = [
        (Types::ARTIFACT, CardType::Artifact),
        (Types::CREATURE, CardType::Creature),
        (Types::ENCHANTMENT, CardType::Enchantment),
        (Types::INSTANT, CardType::Instant),
        (Types::LAND, CardType::Land),
        (Types::PLANESWALKER, CardType::Planeswalker),
        (Types::SORCERY, CardType::Sorcery),
    ];
}

/// Where the game stands, as the session knows it: what [`player_view`] turns into a [`Status`].
#[derive(Clone, Copy, Debug)]
pub enum Moment<'a, L: Layout = Small> {
    /// The viewer's decision `id` is open, asked by the resolution of the stack entry given,
    /// if one asks it.
    Deciding(DecisionId, &'a Decision<L>, Option<&'a StackEntry<L>>),
    /// The viewer is halted ([`DecisionView::halt`]) with decision `id`.
    Halted(DecisionId),
    /// The other player is deciding, in the resolution of the stack entry given if one asks
    /// it.
    Thinking {
        resolving: Option<&'a StackEntry<L>>,
        decision: Option<&'a Decision<L>>,
    },
    Playing,
    Stopped,
    /// The game is over; `cause`, why the loser lost, when the session saw it
    /// ([`Event::Lose`]), and `hurt_by`, the source of the damage that last lowered their
    /// life, if damage rather than a loss of life did.
    Over {
        outcome: Outcome,
        cause: Option<LossCause<L>>,
        hurt_by: Option<ObjectRef<L>>,
    },
}

/// Builds the viewer's view of `obs`'s position (the position the open decision is made in,
/// `beryl_engine::view`) at `moment`, with `events`, the engine's events since the previous view.
/// Everything in it, events included, comes through `obs`.
pub fn player_view<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    moment: Moment<'_, L>,
    stops: StopsSetting,
    events: &[Event<L>],
) -> PlayerView {
    let turn = obs.turn();
    let mut objects = Vec::new();
    let mut player_state = |p: Seat| {
        let state = player_state(t, obs, p);
        for slots in [&state.library, &state.graveyard] {
            objects.extend(
                slots
                    .iter()
                    .map(|&slot| object_view(t, obs, L::Slot::from_index(usize::from(slot)))),
            );
        }
        // Unordered zones: their objects say where they are.
        for slots in [Zone::Hand, Zone::Exile].map(|zone| obs.zone(p, zone)) {
            objects.extend(slots.iter().map(|slot| object_view(t, obs, slot)));
        }
        state
    };
    let viewer = obs.viewer();
    let players = Players {
        you: player_state(viewer),
        opponent: player_state(viewer.opponent()),
    };
    objects.extend(obs.battlefield().iter().map(|&s| object_view(t, obs, s)));
    let mut stack: Vec<_> = obs.stack().iter().map(|e| stack_item(t, obs, e)).collect();
    objects.extend(
        obs.stack()
            .iter()
            .filter_map(StackEntry::spell)
            .map(|r| object_view(t, obs, r.slot)),
    );
    // A resolving spell's card stays on the stack until its resolution ends
    // ([CR 608.2n](https://yawgatog.com/resources/magic-rules/#R6082n): "As the final part of
    // an instant or sorcery spell's resolution, the spell itself is put into its owner's
    // graveyard").
    let resolving = match moment {
        Moment::Deciding(_, _, entry)
        | Moment::Thinking {
            resolving: entry, ..
        } => entry,
        _ => None,
    };
    if let Some(entry) = resolving
        && let Some(r) = entry.spell()
        && obs.object(r.slot).zone == Zone::Stack
    {
        objects.push(object_view(t, obs, r.slot));
    }
    // While the other player answers a question of its resolution, the entry stays on top of
    // the viewer's stack: the viewer's own questions name it apart ([`DecisionView::resolving`]).
    if let Moment::Thinking {
        resolving: Some(entry),
        ..
    } = moment
    {
        stack.push(stack_item(t, obs, entry));
    }
    objects.sort_by_key(|o| o.slot);
    let status = match moment {
        Moment::Deciding(id, d, resolving) => {
            debug_assert_eq!(d.player, viewer, "a view shows only its viewer's decision");
            Status::Deciding {
                decision: decision_view(t, obs, id, d, resolving, stops),
            }
        }
        Moment::Halted(id) => Status::Deciding {
            decision: halt_view(t, obs, id, stops),
        },
        Moment::Thinking { .. } => Status::Thinking,
        Moment::Playing => Status::Playing,
        Moment::Stopped => Status::Stopped,
        Moment::Over { .. } => Status::Over {
            outcome: outcome_view(t, obs, moment),
        },
    };
    let handling = match moment {
        Moment::Thinking {
            decision: Some(d), ..
        } if d.player != viewer => card_handling(obs, d),
        _ => None,
    };
    PlayerView {
        status,
        handling,
        turn: TurnView {
            number: turn.number,
            active: side(obs, turn.active),
            step: turn.step.into(),
            priority: side(obs, turn.priority),
            starting: obs.starting_player().map(|p| side(obs, p)),
            flip_winner: None,
        },
        players,
        battlefield: slots(obs.battlefield().iter().copied()),
        objects,
        stack,
        stops,
        events: game_events(t, obs, events),
        undone: false,
    }
}

fn player_state<L: Layout>(t: &GameTable, obs: Observation<'_, L>, p: Seat) -> PlayerState {
    let player = obs.player(p);
    let [white, blue, black, red, green, colorless] = player.mana.amounts();
    PlayerState {
        life: player.life,
        mana: ManaPool {
            white,
            blue,
            black,
            red,
            green,
            colorless,
        },
        lands_played: player.lands_played,
        mulligans: player.mulligans,
        mulligan: player.mulligan.into(),
        library: slots(obs.library(p).iter().copied()),
        graveyard: slots(obs.graveyard(p).iter().copied()),
        emblems: player
            .emblems
            .iter()
            .map(|&e| t.def(e).name.to_owned())
            .collect(),
    }
}

fn card_handling<L: Layout>(obs: Observation<'_, L>, d: &Decision<L>) -> Option<CardHandling> {
    let library = |player, count: Option<u8>| {
        let size = obs.library(player).len();
        let count = count.map(|n| n.min(u8::try_from(size).unwrap_or(u8::MAX)));
        (size > 0 && count != Some(0)).then(|| CardHandling::Library {
            side: side(obs, player),
            count,
        })
    };
    let hand = |player| {
        (!obs.zone(player, Zone::Hand).is_empty()).then(|| CardHandling::Hand {
            side: side(obs, player),
        })
    };
    match d.ask {
        Ask::Scry { count } | Ask::Take { count, .. } | Ask::Bottom { count } => {
            library(d.player, Some(count))
        }
        Ask::Look {
            player,
            zone,
            count,
            ..
        } => match zone {
            Zone::Library => library(player, Some(count)),
            Zone::Hand => hand(player),
            _ => None,
        },
        Ask::Fateseal { player } => library(player, Some(1)),
        Ask::ManifestDread => library(d.player, Some(2)),
        Ask::Search { .. } => library(d.player, None),
        Ask::PutBack { .. }
        | Ask::DiscardBasicLand { .. }
        | Ask::ExileCards {
            zone: Zone::Hand, ..
        } => hand(d.player),
        _ => match d.kind {
            DecisionKind::KeepOrMulligan | DecisionKind::PutOnBottom | DecisionKind::Discard => {
                hand(d.player)
            }
            _ => None,
        },
    }
}

/// Player `p` as the viewer of `obs` sees them.
fn side<L: Layout>(obs: Observation<'_, L>, p: Seat) -> Side {
    if p == obs.viewer() {
        Side::You
    } else {
        Side::Opponent
    }
}

/// Slots as the protocol numbers them.
fn slots<S: SlotId>(slots: impl IntoIterator<Item = S>) -> Vec<u16> {
    slots.into_iter().map(SlotId::num).collect()
}

/// The name of the face up of the object in `slot`, if the viewer knows it; for a face-down
/// permanent, the name of its card, which it turns face up with (CR 712.15a).
fn name<L: Layout>(t: &GameTable, obs: Observation<'_, L>, slot: L::Slot) -> Option<&'static str> {
    let face = match obs.object(slot).face {
        FaceIx::DOWN => FaceIx::FRONT,
        face => face,
    };
    face_name(t, obs, slot, face)
}

/// The name of face `face` of the object in `slot`, if the viewer knows it: a copy's is the
/// copied card's (CR 707.2), since a token copy is a token of that card.
fn face_name<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    slot: L::Slot,
    face: FaceIx,
) -> Option<&'static str> {
    Some(t.def(obs.card(slot)?).face_name(face))
}

fn object_view<L: Layout>(t: &GameTable, obs: Observation<'_, L>, slot: L::Slot) -> ObjectView {
    let o = obs.object(slot);
    let flags = o.flags;
    let on_battlefield = o.zone == Zone::Battlefield;
    let derived = on_battlefield.then(|| obs.derived(slot));
    let types = derived.map_or(Types::NONE, |d| d.types);
    let creature = types.contains(Types::CREATURE);
    let counters = [
        CounterKind::PlusOne,
        CounterKind::MinusOne,
        CounterKind::Loyalty,
        CounterKind::Time,
        CounterKind::Story,
        CounterKind::Chorus,
    ]
    .into_iter()
    .filter_map(|kind| {
        let count = o.counters.get(kind);
        (count > 0).then(|| CounterView {
            kind: kind.into(),
            count,
        })
    })
    .collect();
    let some = |s: Option<L::Slot>| s.map(SlotId::num);
    ObjectView {
        slot: slot.num(),
        zone: o.zone.into(),
        owner: side(obs, o.owner),
        controller: side(obs, obs.controller(slot)),
        name: name(t, obs, slot).map(str::to_owned),
        facing: if o.face == FaceIx::DOWN {
            Facing::Down
        } else {
            Facing::Up
        },
        token: flags.has(ObjectFlags::TOKEN),
        tapped: flags.has(ObjectFlags::TAPPED),
        summoning_sick: creature && flags.has(ObjectFlags::SUMMONING_SICK),
        damage: o.damage,
        counters,
        types: CardType::ALL
            .iter()
            .filter(|&&(bit, _)| types.contains(bit))
            .map(|&(_, ty)| ty)
            .collect(),
        pt: derived.filter(|_| creature).map(|d| PowerToughness {
            power: d.power,
            toughness: d.toughness,
        }),
        keywords: derived.map_or_else(Vec::new, |d| {
            Keyword::ALL
                .iter()
                .filter(|&&(bit, _)| d.keywords.contains(bit))
                .map(|&(_, k)| k)
                .collect()
        }),
        attached_to: some(o.attached_to),
        exiled_by: if flags.has(ObjectFlags::EXILED_UNTIL) {
            some(o.linked)
        } else {
            None
        },
        exiled_with: if flags.has(ObjectFlags::EXILED_WITH) {
            some(o.linked)
        } else {
            None
        },
        combat: if flags.has(ObjectFlags::ATTACKING) {
            Some(Combat::Attacking {
                planeswalker: if flags.has(ObjectFlags::ATTACKS_PLANESWALKER) {
                    some(o.combat)
                } else {
                    None
                },
                blocked: flags.has(ObjectFlags::BLOCKED),
            })
        } else if flags.has(ObjectFlags::BLOCKING) {
            Some(Combat::Blocking {
                attacker: some(o.combat),
            })
        } else {
            None
        },
    }
}

/// Whether ability `index` of `list` is triggered, and how many abilities of its kind
/// (triggered, or activated) come before it: the face's text says them in that order, so the
/// UI finds its paragraph.
fn ability_order(list: &[Ability], index: u8) -> (bool, u8) {
    // Triggered, or activated (mana abilities are written as activated ones are); the other
    // kinds have no paragraph of their own to find.
    let class = |a: &Ability| match a.kind {
        AbilityKind::Triggered => Some(true),
        AbilityKind::Activated | AbilityKind::Mana => Some(false),
        _ => None,
    };
    let kind = list.get(usize::from(index)).and_then(class) == Some(true);
    let nth = list
        .iter()
        .take(usize::from(index))
        .filter(|a| class(a) == Some(kind))
        .count();
    (
        kind,
        u8::try_from(nth).expect("a face has fewer than 256 abilities"),
    )
}

/// The costs the spell `e` was cast with: a flashback cost, which is an alternative cost
/// ([CR 702.34a](https://yawgatog.com/resources/magic-rules/#R70234a)), or else the options of
/// its face that were paid.
fn paid_costs<L: Layout>(t: &GameTable, e: &StackEntry<L>) -> Vec<PaidCost> {
    if e.flashback {
        return vec![PaidCost::Flashback];
    }
    e.costs
        .paid(t.def(e.card).face(e.face))
        .map(|option| match option {
            CastingOption::Kicker { .. } => PaidCost::Kicker,
            CastingOption::Offspring(_) => PaidCost::Offspring,
            CastingOption::Impending { .. } => PaidCost::Impending,
            CastingOption::Overload { .. } => PaidCost::Overload,
            CastingOption::Evoke { .. } => PaidCost::Evoke,
        })
        .collect()
}

fn stack_item<L: Layout>(t: &GameTable, obs: Observation<'_, L>, e: &StackEntry<L>) -> StackItem {
    let targets = e
        .targets
        .iter()
        .map(|target| match (target.as_player(), target.as_object()) {
            (Some(player), _) => TargetRef::Player {
                player: side(obs, player),
            },
            (None, Some(o)) => TargetRef::Object { slot: o.slot.num() },
            (None, None) => unreachable!("a target is a player or an object"),
        })
        .collect();
    let source = e.object.map(|r| r.slot.num());
    match e.ability {
        None => StackItem::Spell {
            slot: source.expect("a spell's card"),
            targets,
            x: e.x,
            paid: paid_costs(t, e),
        },
        Some(ability) => {
            let face = t.def(e.card).face(e.face);
            let (list, index) = match ability {
                FaceAbility::Own(i) => (face.abilities, i),
                FaceAbility::Grant(i) => (face.grants, i),
            };
            let (kind, nth) = ability_order(list, index);
            StackItem::Ability {
                source,
                ability: index,
                granted: matches!(ability, FaceAbility::Grant(_)),
                name: t.def(e.card).face_name(e.face).to_owned(),
                triggered: kind,
                nth,
                controller: side(obs, e.controller),
                targets,
                x: e.x,
            }
        }
    }
}

/// What each candidate of `d` is called, to the seat `obs` sees for: as the UI names the
/// human's, for the agent's decisions in a report ([`crate::ai`]).
pub(crate) fn candidate_labels<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    d: &Decision<L>,
) -> Vec<String> {
    match &d.candidates {
        Candidates::Objects(b) => b
            .iter()
            .map(|slot| name(t, obs, slot).unwrap_or("a hidden card").to_owned())
            .collect(),
        Candidates::Options(opts) => opts.iter().map(|&o| candidate(t, obs, o).label).collect(),
    }
}

fn decision_view<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    id: DecisionId,
    d: &Decision<L>,
    resolving: Option<&StackEntry<L>>,
    stops: StopsSetting,
) -> DecisionView {
    let candidates = match &d.candidates {
        Candidates::Objects(b) => b
            .iter()
            .map(|slot| CandidateView {
                label: name(t, obs, slot).unwrap_or("a hidden card").to_owned(),
                option: Candidate::Object { slot: slot.num() },
                payments: Vec::new(),
                blocks: Vec::new(),
                nth: None,
                targets: Vec::new(),
            })
            .collect(),
        Candidates::Options(opts) => opts.iter().map(|&o| candidate(t, obs, o)).collect(),
    };
    DecisionView {
        waterbend: None,
        id,
        kind: d.kind.into(),
        candidates,
        resolving: resolving.map(|e| stack_item(t, obs, e)),
        ask: ask_view(obs, d.ask),
        pass_to: (d.kind == DecisionKind::Priority).then(|| pass_to(obs, stops).into()),
        pass_on: if d.kind == DecisionKind::Priority {
            pass_on(obs, stops).map(Into::into)
        } else {
            None
        },
        source: decision_source(obs, d, resolving),
        halt: false,
        cancellable: false,
        undo: 0,
        undo_depth: 0,
        refused: if d.kind == DecisionKind::Priority {
            refused(t, obs)
        } else {
            Vec::new()
        },
    }
}

/// The cards in the viewer's hand they may neither cast nor play as a land at their priority
/// ([`DecisionView::refused`]).
fn refused<L: Layout>(t: &GameTable, obs: Observation<'_, L>) -> Vec<Refused> {
    obs.zone(obs.viewer(), Zone::Hand)
        .iter()
        .filter_map(|slot| {
            let reason = match refusal(obs, t, slot)? {
                Refusal::Timing { by } => RefusalView::Timing {
                    by: by.map(SlotId::num),
                },
                Refusal::LandPlayed => RefusalView::LandPlayed,
                Refusal::Mana => RefusalView::Mana,
                Refusal::Cost => RefusalView::Cost,
                Refusal::Target => RefusalView::Target,
            };
            Some(Refused {
                slot: slot.num(),
                reason,
            })
        })
        .collect()
}

/// A halt ([`DecisionView::halt`]): priority with passing its one candidate.
fn halt_view<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    id: DecisionId,
    stops: StopsSetting,
) -> DecisionView {
    DecisionView {
        waterbend: None,
        id,
        kind: Kind::Priority,
        candidates: vec![CandidateView {
            label: "Pass".to_owned(),
            option: Candidate::Pass,
            payments: Vec::new(),
            blocks: Vec::new(),
            nth: None,
            targets: Vec::new(),
        }],
        resolving: None,
        ask: AskView::Plain,
        pass_to: Some(pass_to(obs, stops).into()),
        pass_on: None,
        source: None,
        halt: true,
        cancellable: false,
        undo: 0,
        undo_depth: 0,
        refused: refused(t, obs),
    }
}

fn ask_view<L: Layout>(obs: Observation<'_, L>, ask: Ask<L>) -> AskView {
    match ask {
        Ask::Plain => AskView::Plain,
        Ask::AttackTarget { attackers } => AskView::AttackTarget {
            attackers: attackers.iter().map(SlotId::num).collect(),
        },
        Ask::Scry { count } => AskView::Scry { count },
        Ask::Look {
            player,
            zone,
            count,
            to,
            eligible,
        } => AskView::Look {
            player: side(obs, player),
            zone: zone.into(),
            count,
            to: to.into(),
            eligible: slots(eligible.iter()),
        },
        Ask::PutBack { count } => AskView::PutBack { count },
        Ask::Fateseal { player } => AskView::Fateseal {
            player: side(obs, player),
        },
        Ask::Take { count, take } => AskView::Take { count, take },
        Ask::Bottom { count } => AskView::Bottom { count },
        Ask::DiscardBasicLand { count } => AskView::DiscardBasicLand { count },
        Ask::MayMove { card, to } => AskView::MayMove {
            card: card.num(),
            to: to.into(),
        },
        Ask::Explore => AskView::Explore,
        Ask::ManifestDread => AskView::ManifestDread,
        Ask::ExileCards { zone, count } => AskView::ExileCards {
            zone: zone.into(),
            count,
        },
        Ask::TopOrBottom { card } => AskView::TopOrBottom { card: card.num() },
        Ask::Search { max } => AskView::Search { max },
        Ask::MaySearch => AskView::MaySearch,
        Ask::Untap { max } => AskView::Untap { max },
        Ask::Crew { crew, left } => AskView::Crew { crew, left },
        Ask::Waterbend { cost } => AskView::Waterbend {
            cost: mana_text(cost),
        },
        Ask::UnlessPays { cost } => AskView::UnlessPays {
            cost: mana_text(cost),
        },
        Ask::PayLife { life, .. } => AskView::PayLife { life },
        Ask::Cleanup { count } => AskView::Cleanup { count },
    }
}

/// [`DecisionView::pass_to`]: the engine's [`next_stop`], less the declare blockers step for a
/// defender with nothing to block with under the smart stops ([`skip_idle_blocks`]).
fn pass_to<L: Layout>(obs: Observation<'_, L>, stops: StopsSetting) -> NextStop {
    skip_idle_blocks(
        obs,
        stops,
        next_stop(obs, stops.stops(), stops.step_stops()),
    )
}

/// [`DecisionView::pass_on`]: where the viewer's next stop in a later step is, when passing
/// would stop them again in this one, for the stack resolving or for combat damage.
pub(crate) fn pass_on<L: Layout>(obs: Observation<'_, L>, stops: StopsSetting) -> Option<NextStop> {
    match pass_to(obs, stops) {
        NextStop::Resolve | NextStop::CombatDamage => {
            let later = next_step_stop(obs, stops.step_stops(), obs.turn().step);
            Some(skip_idle_blocks(obs, stops, later))
        }
        NextStop::Step(_) | NextStop::NextTurn => None,
    }
}

/// `next` less the declare blockers step for a defender with nothing to block with under the
/// smart stops. The session passes them through it, as no creature blocks ([`crate::Game`]),
/// on to their next stop after it: by default the end step, as the postcombat main phase is
/// the active player's.
fn skip_idle_blocks<L: Layout>(
    obs: Observation<'_, L>,
    stops: StopsSetting,
    next: NextStop,
) -> NextStop {
    match next {
        NextStop::Step(TurnStep::DeclareBlockers)
            if !stops.full_control
                && obs.turn().active != obs.viewer()
                && !can_declare_blockers(obs) =>
        {
            next_step_stop(obs, stops.step_stops(), TurnStep::DeclareBlockers)
        }
        next => next,
    }
}

impl From<NextStop> for PassTo {
    fn from(next: NextStop) -> PassTo {
        match next {
            NextStop::Resolve => PassTo::Resolve,
            NextStop::CombatDamage => PassTo::CombatDamage,
            NextStop::Step(step) => PassTo::Step { step: step.into() },
            NextStop::NextTurn => PassTo::NextTurn,
        }
    }
}

/// [`DecisionView::source`].
fn decision_source<L: Layout>(
    obs: Observation<'_, L>,
    d: &Decision<L>,
    resolving: Option<&StackEntry<L>>,
) -> Option<u16> {
    let slot = match (d.kind, d.ask) {
        (_, Ask::PayLife { land, .. }) => Some(land),
        (
            DecisionKind::Priority
            | DecisionKind::Attacker
            | DecisionKind::AttackTarget
            | DecisionKind::Blocker
            | DecisionKind::BlockTarget,
            _,
        ) => None,
        (DecisionKind::DamageDivision, _) => {
            // The blockers offered block the attacker being divided.
            let Candidates::Options(opts) = &d.candidates else {
                return None;
            };
            opts.iter().find_map(|&o| match o {
                Opt::Lethal(blocker) => obs.object(blocker).combat,
                _ => None,
            })
        }
        _ => resolving
            .or_else(|| obs.stack().last())
            .and_then(|e| e.object)
            .map(|r| r.slot),
    };
    slot.map(SlotId::num)
}

fn candidate<L: Layout>(t: &GameTable, obs: Observation<'_, L>, o: Opt<L>) -> CandidateView {
    let named = |slot: L::Slot| name(t, obs, slot).unwrap_or("a hidden card");
    let player = |p: Seat| match side(obs, p) {
        Side::You => "you",
        Side::Opponent => "your opponent",
    };
    let mana = |color: ManaColor| (Mana::from(color), MANA_SYMBOLS[color.index()]);
    let (label, option) = match o {
        Opt::Pass => ("Pass".to_owned(), Candidate::Pass),
        Opt::PlayLand(slot) => (
            format!(
                "Play {}",
                land_face_name(t, obs, slot).unwrap_or("a hidden card")
            ),
            Candidate::PlayLand { slot: slot.num() },
        ),
        Opt::Play => ("Play first".to_owned(), Candidate::Play),
        Opt::Draw => ("Draw first".to_owned(), Candidate::Draw),
        Opt::Keep => ("Keep".to_owned(), Candidate::Keep),
        Opt::Mulligan => ("Mulligan".to_owned(), Candidate::Mulligan),
        Opt::Trigger(index) => trigger(t, obs, index),
        Opt::Yes => ("Yes".to_owned(), Candidate::Yes),
        Opt::No => ("No".to_owned(), Candidate::No),
        Opt::Done => ("Done".to_owned(), Candidate::Done),
        Opt::Attack(slot) => (
            format!("Attack with {}", named(slot)),
            Candidate::Attack { slot: slot.num() },
        ),
        Opt::Defender(None) => (
            format!("Attack {}", player(obs.turn().active.opponent())),
            Candidate::Defender { planeswalker: None },
        ),
        Opt::Defender(Some(slot)) => (
            format!("Attack {}", named(slot)),
            Candidate::Defender {
                planeswalker: Some(slot.num()),
            },
        ),
        Opt::Block(slot) => (
            format!("Block with {}", named(slot)),
            Candidate::Block { slot: slot.num() },
        ),
        Opt::Lethal(slot) => (
            format!("Assign lethal damage to {}", named(slot)),
            Candidate::Lethal { slot: slot.num() },
        ),
        Opt::Cast { slot, face, costs } => cast_candidate(t, obs, slot, face, costs),
        Opt::Activate { slot, ability } => (
            match activated_cost(t, obs, slot, ability) {
                Some(cost) => format!("{}: {cost}", named(slot)),
                None => format!("Activate {}", named(slot)),
            },
            activate(slot, ability),
        ),
        Opt::Mana { slot, color } => {
            let (color, symbol) = mana(color);
            (
                format!("Tap {} for {symbol}", named(slot)),
                Candidate::Mana {
                    slot: slot.num(),
                    color,
                },
            )
        }
        Opt::Pool(color) => {
            let (color, symbol) = mana(color);
            (format!("Spend {symbol}"), Candidate::Pool { color })
        }
        Opt::Number(n) => (n.to_string(), Candidate::Number { n }),
        Opt::Plan(index) => {
            let index = index.into();
            (format!("Payment {}", index + 1), Candidate::Plan { index })
        }
        Opt::Player(p) => (
            match side(obs, p) {
                Side::You => "You",
                Side::Opponent => "Your opponent",
            }
            .to_owned(),
            Candidate::Player {
                player: side(obs, p),
            },
        ),
        Opt::Object(slot) => (
            named(slot).to_owned(),
            Candidate::Object { slot: slot.num() },
        ),
        Opt::TurnFaceUp(slot) => (
            format!("Turn {} face up", named(slot)),
            Candidate::TurnFaceUp { slot: slot.num() },
        ),
    };
    CandidateView {
        label,
        option,
        payments: Vec::new(),
        blocks: Vec::new(),
        nth: ability_nth(t, obs, o),
        targets: Vec::new(),
    }
}

/// [`CandidateView::nth`].
fn ability_nth<L: Layout>(t: &GameTable, obs: Observation<'_, L>, o: Opt<L>) -> Option<u8> {
    let (face, i) = match o {
        Opt::Activate {
            slot,
            ability: AbilityIx::Own(i),
        } => (t.def(obs.card(slot)?).face(obs.object(slot).face), i),
        Opt::Trigger(index) => {
            let trigger = obs.triggers()[Into::<usize>::into(index)];
            (t.def(trigger.card).face(trigger.face), trigger.ability)
        }
        _ => return None,
    };
    Some(ability_order(face.abilities, i).1)
}

/// The name of the face of the card in `slot` it is played with as a land, if the viewer knows
/// it: its first land face ([CR 712.12](https://yawgatog.com/resources/magic-rules/#R71212):
/// "A player playing a modal double-faced card ... as a land chooses one of its faces that's a
/// land before putting it onto the battlefield"), as Sink into Stupor plays as Soporific
/// Springs.
fn land_face_name<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    slot: L::Slot,
) -> Option<&'static str> {
    let def = t.def(obs.card(slot)?);
    let face = def
        .face_ixs()
        .find(|&f| def.face(f).types.intersects(Types::LAND))?;
    Some(def.face_name(face))
}

/// A candidate that casts face `face` of the card in `slot` paying the options in `costs`.
fn cast_candidate<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    slot: L::Slot,
    face: FaceIx,
    costs: Castings,
) -> (String, Candidate) {
    let spell = face_name(t, obs, slot, face).unwrap_or("a hidden card");
    (
        format!("Cast {spell}{}", casting_options(t, obs, slot, face, costs)),
        Candidate::Cast {
            slot: slot.num(),
            face: face.get(),
            costs: costs.indices().collect(),
        },
    )
}

/// How a cast of face `face` of the card in `slot` paying the options in `costs` reads after
/// "Cast <name>", so that the casts of one card tell apart
/// ([CR 601.2b](https://yawgatog.com/resources/magic-rules/#R6012b): "If the spell has
/// alternative or additional costs that will be paid as it's being cast ... the player
/// announces their intentions to pay any or all of those costs").
fn casting_options<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    slot: L::Slot,
    face: FaceIx,
    costs: Castings,
) -> String {
    let Some(card) = obs.card(slot) else {
        return String::new();
    };
    // CR 702.34a: a flashback cost is an alternative cost, so no other is paid with it. A
    // permission lasts until the turn ends, so one whose card has since changed zones, as
    // when Timetwister draws it again, no longer names this object.
    let incarnation = obs.object(slot).incarnation;
    if obs.permissions().iter().any(|p| {
        p.object.slot == slot
            && p.object.incarnation == incarnation
            && p.player == obs.viewer()
            && p.flashback
    }) {
        return " with flashback".to_owned();
    }
    costs
        .paid(t.def(card).face(face))
        .map(|option| match option {
            CastingOption::Kicker { cost, .. } => format!(" with kicker {}", mana_text(cost)),
            CastingOption::Offspring(cost) => format!(" with offspring {}", mana_text(cost)),
            CastingOption::Impending { cost, counters } => {
                format!(" with impending {counters}, {}", mana_text(cost))
            }
            CastingOption::Overload { cost, .. } => format!(" with overload {}", mana_text(cost)),
            CastingOption::Evoke { cost, exile } => {
                let mut parts = Vec::new();
                if cost != ManaCost::NONE {
                    parts.push(mana_text(cost));
                }
                if exile != Colors::NONE {
                    parts.push(format!("exile a {} card", color_words(exile)));
                }
                format!(" with evoke, {}", parts.join(", "))
            }
        })
        .collect()
}

/// A set of colors as card text names them: "blue", "white or blue".
fn color_words(colors: Colors) -> String {
    [
        (Colors::WHITE, "white"),
        (Colors::BLUE, "blue"),
        (Colors::BLACK, "black"),
        (Colors::RED, "red"),
        (Colors::GREEN, "green"),
    ]
    .into_iter()
    .filter(|&(c, _)| colors.contains(c))
    .map(|(_, word)| word)
    .collect::<Vec<_>>()
    .join(" or ")
}

/// The cost of activating ability `ability` of the object in `slot`, as its text reads
/// ([CR 602.1a](https://yawgatog.com/resources/magic-rules/#R6021a): "Activated abilities
/// have a cost and an effect. They are written as '[Cost]: [Effect.] [Activation
/// instructions (if any).]'"), if the viewer knows the card. A loyalty ability's cost is its
/// loyalty symbol ([CR 606.4](https://yawgatog.com/resources/magic-rules/#R6064)).
fn activated_cost<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    slot: L::Slot,
    ability: AbilityIx,
) -> Option<String> {
    let (card, face, ability) = match ability {
        AbilityIx::Own(i) => (obs.card(slot)?, obs.object(slot).face, FaceAbility::Own(i)),
        AbilityIx::Granted(k) => {
            let grant = obs.derived(slot).granted[usize::from(k)]?;
            (grant.card, FaceIx::FRONT, FaceAbility::Grant(grant.grant))
        }
    };
    Some(cost_text(&t.def(card).face(face).ability(ability).cost))
}

/// An activation cost as Oracle text writes it: "{W}, {T}, Remove a story counter".
fn cost_text(cost: &Cost) -> String {
    if let Some(loyalty) = cost.loyalty {
        // A minus sign, as printed, rather than a hyphen.
        return match loyalty {
            0 => "0".to_owned(),
            l if l > 0 => format!("+{l}"),
            l => format!("\u{2212}{}", l.unsigned_abs()),
        };
    }
    if cost.crew > 0 {
        return format!("Crew {}", cost.crew);
    }
    let mut parts = Vec::new();
    if cost.waterbend > 0 {
        parts.push(format!("Waterbend {}", mana_text(cost.mana)));
    } else if cost.mana != ManaCost::NONE {
        parts.push(mana_text(cost.mana));
    }
    if cost.tap {
        parts.push("{T}".to_owned());
    }
    if cost.sacrifice_self {
        parts.push("Sacrifice this".to_owned());
    }
    if cost.discard_self {
        parts.push("Discard this card".to_owned());
    }
    if let Some(kind) = cost.remove {
        parts.push(format!("Remove a {} counter", counter_name(kind)));
    }
    if parts.is_empty() {
        "Activate".to_owned()
    } else {
        parts.join(", ")
    }
}

/// A mana cost in braces, X first, as Oracle text writes it: "{X}{2}{W}{W}"
/// ([CR 107.4](https://yawgatog.com/resources/magic-rules/#R1074)).
pub(crate) fn mana_text(cost: ManaCost) -> String {
    let mut text = "{X}".repeat(usize::from(cost.x));
    if cost.generic > 0 || (cost.x == 0 && cost.symbols == [0; 6]) {
        text.push('{');
        text.push_str(&cost.generic.to_string());
        text.push('}');
    }
    for (symbol, &count) in MANA_SYMBOLS.iter().zip(&cost.symbols) {
        text.push_str(&symbol.repeat(usize::from(count)));
    }
    text
}

fn counter_name(kind: CounterKind) -> &'static str {
    match kind {
        CounterKind::PlusOne => "+1/+1",
        CounterKind::MinusOne => "-1/-1",
        CounterKind::Loyalty => "loyalty",
        CounterKind::Time => "time",
        CounterKind::Story => "story",
        CounterKind::Chorus => "chorus",
    }
}

/// Activating ability `ability` of the object in `slot`.
fn activate<S: SlotId>(slot: S, ability: AbilityIx) -> Candidate {
    let (ability, granted) = match ability {
        AbilityIx::Own(i) => (i, false),
        AbilityIx::Granted(k) => (k, true),
    };
    Candidate::Activate {
        slot: slot.num(),
        ability,
        granted,
    }
}

/// Putting the waiting trigger at `index` on the stack.
fn trigger<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    index: L::Index,
) -> (String, Candidate) {
    let trigger = obs.triggers()[Into::<usize>::into(index)];
    (
        format!(
            "Put {}'s trigger on the stack",
            t.def(trigger.card).face_name(trigger.face)
        ),
        Candidate::Trigger {
            index: index.into(),
            source: trigger.source.map(|r| r.slot.num()),
            ability: trigger.ability,
        },
    )
}

/// Mana symbols by [`ManaColor`] index.
const MANA_SYMBOLS: [&str; 6] = ["{W}", "{U}", "{B}", "{R}", "{G}", "{C}"];

fn outcome_view<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    over: Moment<'_, L>,
) -> OutcomeView {
    let Moment::Over {
        outcome,
        cause,
        hurt_by,
    } = over
    else {
        unreachable!("the game is over");
    };
    match outcome {
        Outcome::Win(player) => {
            let loser = obs.player(player.opponent());
            let (reason, by) = match cause {
                Some(LossCause::Life) => (
                    LossReason::Life,
                    hurt_by
                        .and_then(|r| name(t, obs, r.slot))
                        .map(|name| Culprit {
                            name: name.to_owned(),
                            ability: None,
                        }),
                ),
                Some(LossCause::Library) => (LossReason::Library, None),
                Some(LossCause::Effect {
                    card,
                    face,
                    ability,
                    ..
                }) => (
                    LossReason::Effect,
                    Some(Culprit {
                        name: t.def(card).face_name(face).to_owned(),
                        ability: ability.map(|a| ability_ref(t.def(card).face(face), a)),
                    }),
                ),
                Some(LossCause::Concede) => (LossReason::Concede, None),
                // Without the loss's event, the state-based losses are read off the loser,
                // life first as the engine names them.
                None if loser.life <= 0 => (LossReason::Life, None),
                None if loser.drew_from_empty => (LossReason::Library, None),
                None => (LossReason::Effect, None),
            };
            OutcomeView::Win {
                winner: side(obs, player),
                reason,
                by,
            }
        }
        Outcome::Draw => OutcomeView::Draw,
        Outcome::Stalled(reason) => OutcomeView::Stalled {
            reason: reason.into(),
        },
    }
}

/// Where the rules text of `face` says ability `a`: the how-manyth of its kind.
fn ability_ref(face: &Face, a: FaceAbility) -> AbilityRef {
    let (list, index) = match a {
        FaceAbility::Own(i) => (face.abilities, i),
        FaceAbility::Grant(i) => (face.grants, i),
    };
    let class = |a: &Ability| match a.kind {
        AbilityKind::Triggered => Some(AbilityClass::Triggered),
        AbilityKind::Activated | AbilityKind::Mana => Some(AbilityClass::Activated),
        AbilityKind::Static | AbilityKind::Replacement => Some(AbilityClass::Static),
        AbilityKind::Spell => None,
    };
    let kind = list
        .get(usize::from(index))
        .and_then(class)
        .unwrap_or(AbilityClass::Static);
    let nth = list
        .iter()
        .take(usize::from(index))
        .filter(|a| class(a) == Some(kind))
        .count();
    AbilityRef {
        kind,
        nth: u8::try_from(nth).expect("a face has fewer than 256 abilities"),
    }
}

/// What damage was dealt to, as the viewer names it.
fn recipient<L: Layout>(obs: Observation<'_, L>, to: Recipient<L>) -> TargetRef {
    match to {
        Recipient::Player(player) => TargetRef::Player {
            player: side(obs, player),
        },
        Recipient::Permanent(o) => TargetRef::Object { slot: o.slot.num() },
    }
}

/// An [`Event::Ability`] as the viewer of `obs` is told it: as the stack showed it.
fn ability_event<L: Layout>(t: &GameTable, obs: Observation<'_, L>, e: &Event<L>) -> GameEvent {
    let Event::Ability {
        source,
        controller,
        card,
        face,
        ability,
        targets,
        x,
    } = e.clone()
    else {
        unreachable!("an ability's event");
    };
    let entry = StackEntry {
        object: source,
        controller,
        ability: Some(ability),
        card,
        face,
        targets,
        x,
        ..StackEntry::default()
    };
    GameEvent::Ability {
        item: stack_item(t, obs, &entry),
        loyalty: t.def(card).face(face).ability(ability).cost.loyalty,
    }
}

/// The spell an [`Event::Cast`] put on the stack, as the stack showed it then. Its card is
/// public on the stack ([CR 601.2a](https://yawgatog.com/resources/magic-rules/#R6012a): "the
/// player first moves that card ... from where it is to the stack"), and `obs` names it while
/// it stays public.
fn cast_item<L: Layout>(t: &GameTable, obs: Observation<'_, L>, e: &Event<L>) -> StackItem {
    let Event::Cast {
        spell,
        controller,
        targets,
        x,
        costs,
        flashback,
    } = e.clone()
    else {
        unreachable!("a cast's event");
    };
    // A spell gone back to a hidden zone since (bounced to its owner's hand) keeps its slot,
    // its targets and X; the costs its card was cast with are its card's to say.
    let entry = match obs.card(spell.slot) {
        Some(card) => StackEntry {
            card,
            face: obs.object(spell.slot).face,
            costs,
            flashback,
            ..StackEntry::default()
        },
        None => StackEntry::default(),
    };
    let entry = StackEntry {
        object: Some(spell),
        controller,
        targets,
        x,
        ..entry
    };
    stack_item(t, obs, &entry)
}

/// An [`Event::CountersAdded`] or [`Event::CountersRemoved`] as the viewer is told it.
fn counters_event<L: Layout>(e: &Event<L>) -> GameEvent {
    match *e {
        Event::CountersAdded {
            object,
            kind,
            amount,
        } => GameEvent::CountersAdded {
            slot: object.slot.num(),
            counter: kind.into(),
            amount,
        },
        Event::CountersRemoved {
            object,
            kind,
            amount,
        } => GameEvent::CountersRemoved {
            slot: object.slot.num(),
            counter: kind.into(),
            amount,
        },
        _ => unreachable!("a counters event"),
    }
}

/// An [`Event::Mana`] as the viewer of `obs` is told it: who made it, from what, all public
/// ([CR 605.3b](https://yawgatog.com/resources/magic-rules/#R6053b): a mana ability
/// "resolves immediately after it is activated", in view of both players).
fn mana_event<L: Layout>(obs: Observation<'_, L>, e: &Event<L>) -> GameEvent {
    let &Event::Mana {
        player,
        source,
        color,
        amount,
    } = e
    else {
        unreachable!("a mana event")
    };
    GameEvent::Mana {
        player: side(obs, player),
        source: source.map(|o| o.slot.num()),
        color: color.into(),
        amount,
    }
}

/// Combine the individual public handles with the draw count the rules listen to.
fn game_events<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    events: &[Event<L>],
) -> Vec<GameEvent> {
    let mut drawn = Vec::new();
    events
        .iter()
        .filter_map(|e| {
            if let Event::DrawCard { slot } = e {
                drawn.push(slot.num());
                return None;
            }
            let mut event = game_event(t, obs, e);
            if let GameEvent::Draw { slots, .. } = &mut event {
                *slots = (!drawn.is_empty()).then(|| std::mem::take(&mut drawn));
            }
            Some(event)
        })
        .collect()
}

/// An engine event as the viewer of `obs` is told it. Engine events name objects by slot and
/// carry no identity: anything an event says about a card must come from `obs`, which the
/// information independence test redeals. The table names only an ability's card, public as
/// the stack shows it.
fn game_event<L: Layout>(t: &GameTable, obs: Observation<'_, L>, e: &Event<L>) -> GameEvent {
    let side = |p| side(obs, p);
    match *e {
        Event::ZoneChange {
            object,
            from,
            to,
            reason,
            library_top,
        } => GameEvent::ZoneChange {
            slot: object.slot.num(),
            from: from.into(),
            to: to.into(),
            reason: reason.into(),
            library_top,
        },
        Event::Damage {
            source,
            to,
            amount,
            combat,
        } => GameEvent::Damage {
            source: source.slot.num(),
            to: recipient(obs, to),
            amount,
            combat,
        },
        Event::Draw { player, cards } => GameEvent::Draw {
            player: side(player),
            cards,
            slots: None,
        },
        Event::DrawCard { .. } => unreachable!("draw slots are grouped with their draw"),
        Event::LibraryShuffled { player } => GameEvent::LibraryShuffled {
            player: side(player),
        },
        Event::LifeGain { player, amount } => GameEvent::LifeGain {
            player: side(player),
            amount,
        },
        Event::LifeLoss { player, amount } => GameEvent::LifeLoss {
            player: side(player),
            amount,
        },
        Event::CountersAdded { .. } | Event::CountersRemoved { .. } => counters_event(e),
        Event::Tap { object } => GameEvent::Tap {
            slot: object.slot.num(),
        },
        Event::Untap { object } => GameEvent::Untap {
            slot: object.slot.num(),
        },
        Event::Mana { .. } => mana_event(obs, e),
        Event::ResolutionPayment { player, ref item } => GameEvent::ResolutionPayment {
            player: side(player),
            item: stack_item(t, obs, item),
        },
        Event::TokensCreated { controller, tokens } => GameEvent::TokensCreated {
            controller: side(controller),
            tokens: slots(tokens.iter()),
        },
        Event::Cast {
            spell, controller, ..
        } => GameEvent::Cast {
            slot: spell.slot.num(),
            controller: side(controller),
            item: cast_item(t, obs, e),
        },
        Event::Ability { .. } => ability_event(t, obs, e),
        Event::Attack { player, attackers } => GameEvent::Attack {
            player: side(player),
            attackers: slots(attackers.iter()),
        },
        Event::StepBegins { step, active } => GameEvent::StepBegins {
            step: step.into(),
            active: side(active),
        },
        Event::GainControl { object, player } => GameEvent::GainControl {
            slot: object.slot.num(),
            player: side(player),
        },
        Event::CoinFlip {
            player,
            won,
            source,
        } => GameEvent::CoinFlip {
            player: side(player),
            won,
            source: source.slot.num(),
        },
        Event::Lose { player, .. } => GameEvent::Lose {
            player: side(player),
        },
        Event::Win { player } => GameEvent::Win {
            player: side(player),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn costs_read_as_printed() {
        let mana = |generic, symbols, x| ManaCost {
            generic,
            symbols,
            x,
        };
        // Jace, the Mind Sculptor's -1, Map's cost, Staff of the Storyteller's, kicker {X}.
        let loyalty = Cost {
            loyalty: Some(-1),
            ..Cost::NONE
        };
        assert_eq!(cost_text(&loyalty), "\u{2212}1");
        let map = Cost {
            mana: mana(1, [0; 6], 0),
            tap: true,
            sacrifice_self: true,
            ..Cost::NONE
        };
        assert_eq!(cost_text(&map), "{1}, {T}, Sacrifice this");
        let staff = Cost {
            mana: mana(0, [1, 0, 0, 0, 0, 0], 0),
            tap: true,
            remove: Some(CounterKind::Story),
            ..Cost::NONE
        };
        assert_eq!(cost_text(&staff), "{W}, {T}, Remove a story counter");
        assert_eq!(mana_text(mana(0, [0; 6], 1)), "{X}");
        assert_eq!(mana_text(mana(2, [0, 2, 0, 0, 0, 0], 0)), "{2}{U}{U}");
    }
}
