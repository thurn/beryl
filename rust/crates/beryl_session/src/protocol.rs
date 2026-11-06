//! The `PlayerView` protocol (architecture §19.2, decision D14): what the UI draws, built from
//! one player's [`Observation`] so it carries nothing that player does not know. Every type
//! here has a generated TypeScript twin in `ui/src/generated/protocol.ts`
//! ([`crate::typescript`]).

use beryl_engine::card::{
    Ability, AbilityIx, AbilityKind, CastingOption, Castings, Colors, Cost, FaceAbility, FaceIx,
    ManaColor, ManaCost, Types,
};
use beryl_engine::event::{Event, Reason, Recipient};
use beryl_engine::state::{CounterKind, MulliganStatus, ObjectFlags, StackEntry, TurnStep};
use beryl_engine::{
    Ask, Candidates, Decision, DecisionKind, GameTable, NextStop, Observation, Opt, Outcome, Seat,
    Slot, StallReason, Stops, Zone, next_stop,
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
    pub turn: TurnView,
    pub players: Players,
    /// Permanents, in the order they entered.
    pub battlefield: Vec<u8>,
    /// Every object in a zone, by slot: the battlefield, the stack, and each player's library,
    /// hand, graveyard and exile.
    pub objects: Vec<ObjectView>,
    /// Bottom first.
    pub stack: Vec<StackItem>,
    /// The viewer's priority stops.
    pub stops: StopsSetting,
    /// What happened since the previous view, oldest first, for animation.
    pub events: Vec<GameEvent>,
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
pub struct CandidateIndex(pub u8);

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
    pub library: Vec<u8>,
    /// Bottom first.
    pub graveyard: Vec<u8>,
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
    pub slot: u8,
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
    pub attached_to: Option<u8>,
    /// The permanent that exiled it until it leaves (CR 610.3).
    pub exiled_by: Option<u8>,
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
        planeswalker: Option<u8>,
        blocked: bool,
    },
    /// Blocking the attacker in `attacker`, if it is still in combat.
    #[serde(rename_all = "camelCase")]
    Blocking { attacker: Option<u8> },
}

/// A spell or ability on the stack.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StackItem {
    /// The spell in `slot`, whose object says what it is and who controls it.
    Spell {
        slot: u8,
        targets: Vec<TargetRef>,
        /// The announced value of X.
        x: u8,
    },
    /// The ability at index `ability` of the face named `name` (among the abilities it grants
    /// to other objects, when `granted`), from the object in `source`, which may have left its
    /// zone since; an emblem's ability has none.
    Ability {
        source: Option<u8>,
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

/// A player or an object, as a target or a recipient of damage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TargetRef {
    Player { player: Side },
    Object { slot: u8 },
}

/// The viewer's decision, its candidates in the order a [`CandidateIndex`] counts them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DecisionView {
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
    /// The card the decision is for: the spell or ability resolving, or else the one on top of
    /// the stack (which a target, an X or a payment is chosen for); the attacker whose combat
    /// damage is divided; the land asking for life. None for priority and the other combat
    /// declarations.
    pub source: Option<u8>,
    /// The session halts the viewer here: a priority where passing is all they can do,
    /// which the engine would pass for them, offered under the smart stops at the start of
    /// their own precombat main phase (an end-turn button) and of the opponent's end step (a
    /// continue button), so the turn never moves on unseen.
    pub halt: bool,
    /// The decision is part of a spell the viewer is casting or an ability they are
    /// activating from their priority, whose costs are not yet paid: the `cancel` command
    /// takes it back.
    pub cancellable: bool,
}

/// What a decision asks ([`Ask`]), by public slot and relative to the viewer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AskView {
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
        eligible: Vec<u8>,
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
    TopOrBottom { card: u8 },
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
    pub blocks: Vec<u8>,
    /// For a candidate that activates an ability of the object's own face: how many activated
    /// abilities, mana abilities among them, its face lists before it, as
    /// [`StackItem::Ability`]'s `nth` counts them, so the UI finds the ability's paragraph.
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
    pub plan: Option<u8>,
    /// Permanents, by slot.
    pub taps: Vec<u8>,
}

/// What a candidate does, by public slot (architecture §7.1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Candidate {
    /// The object in this slot, for a decision among objects.
    Object {
        slot: u8,
    },
    Pass,
    PlayLand {
        slot: u8,
    },
    Play,
    Draw,
    Keep,
    Mulligan,
    /// The waiting triggered ability at `index`, from the object in `source`.
    Trigger {
        index: u8,
        source: Option<u8>,
    },
    Yes,
    No,
    Done,
    Attack {
        slot: u8,
    },
    /// Attack the planeswalker in `planeswalker`, or the defending player.
    Defender {
        planeswalker: Option<u8>,
    },
    Block {
        slot: u8,
    },
    Lethal {
        slot: u8,
    },
    /// Cast with face `face` up, paying the face's casting options at the indices in `costs`.
    Cast {
        slot: u8,
        face: u8,
        costs: Vec<u8>,
    },
    /// The ability at index `ability` among the object's own, or, when `granted`, among
    /// those other objects grant it.
    Activate {
        slot: u8,
        ability: u8,
        granted: bool,
    },
    Mana {
        slot: u8,
        color: Mana,
    },
    Pool {
        color: Mana,
    },
    Number {
        n: u8,
    },
    Plan {
        index: u8,
    },
    Player {
        player: Side,
    },
    /// Turn the face-down permanent in `slot` face up, paying its card's mana cost.
    TurnFaceUp {
        slot: u8,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum OutcomeView {
    Win {
        winner: Side,
        /// Why the other player lost.
        reason: LossReason,
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

/// Where the viewer gets priority (architecture §15, decision D13).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum StopsSetting {
    /// The smart stops: main phases, combat after declarations, the opponent's end step, and
    /// whenever the stack is non-empty.
    Smart,
    /// Full control: every priority the viewer receives, even where passing is all they can
    /// do.
    Exact,
}

impl StopsSetting {
    pub fn stops(self) -> Stops {
        match self {
            StopsSetting::Smart => Stops::Smart,
            StopsSetting::Exact => Stops::Exact,
        }
    }
}

/// One thing that happened (architecture §8.1), by public slot and relative to the viewer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum GameEvent {
    /// The object now in `slot` arrived from another zone.
    ZoneChange {
        slot: u8,
        from: ZoneName,
        to: ZoneName,
        reason: Why,
    },
    Damage {
        source: u8,
        to: TargetRef,
        amount: u8,
        combat: bool,
    },
    Draw {
        player: Side,
        cards: u8,
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
        slot: u8,
        counter: Counter,
        amount: u8,
    },
    CountersRemoved {
        slot: u8,
        counter: Counter,
        amount: u8,
    },
    Tap {
        slot: u8,
    },
    Untap {
        slot: u8,
    },
    TokensCreated {
        controller: Side,
        tokens: Vec<u8>,
    },
    Cast {
        slot: u8,
        controller: Side,
    },
    Attack {
        player: Side,
        attackers: Vec<u8>,
    },
    StepBegins {
        step: Step,
        active: Side,
    },
    GainControl {
        slot: u8,
        player: Side,
    },
    CoinFlip {
        player: Side,
        won: bool,
        /// The permanent whose ability flipped it.
        source: u8,
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
pub enum Moment<'a> {
    /// The viewer's decision `id` is open, asked by the resolution of the stack entry given,
    /// if one asks it.
    Deciding(DecisionId, &'a Decision, Option<&'a StackEntry>),
    /// The viewer is halted ([`DecisionView::halt`]) with decision `id`.
    Halted(DecisionId),
    Thinking,
    Playing,
    Stopped,
    /// The game is over; `conceded`, because the loser conceded.
    Over {
        outcome: Outcome,
        conceded: bool,
    },
}

/// Builds the viewer's view of `obs`'s position (the position the open decision is made in,
/// `beryl_engine::view`) at `moment`, with `events`, the engine's events since the previous view.
/// Everything in it, events included, comes through `obs`.
pub fn player_view(
    t: &GameTable,
    obs: Observation<'_>,
    moment: Moment<'_>,
    stops: StopsSetting,
    events: &[Event],
) -> PlayerView {
    let turn = obs.turn();
    let mut objects = Vec::new();
    let mut player_state = |p: Seat| {
        let player = obs.player(p);
        let [white, blue, black, red, green, colorless] = player.mana.amounts();
        let state = PlayerState {
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
        };
        for slots in [&state.library, &state.graveyard] {
            objects.extend(
                slots
                    .iter()
                    .map(|&slot| object_view(t, obs, Slot::new(slot))),
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
    let stack = obs.stack().iter().map(|e| stack_item(t, obs, e)).collect();
    objects.extend(
        obs.stack()
            .iter()
            .filter_map(StackEntry::spell)
            .map(|r| object_view(t, obs, r.slot)),
    );
    // A resolving spell's card stays on the stack until its resolution ends (CR 608.2n).
    if let Moment::Deciding(_, _, Some(entry)) = moment
        && let Some(r) = entry.spell()
        && obs.object(r.slot).zone == Zone::Stack
    {
        objects.push(object_view(t, obs, r.slot));
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
            decision: halt_view(obs, id, stops),
        },
        Moment::Thinking => Status::Thinking,
        Moment::Playing => Status::Playing,
        Moment::Stopped => Status::Stopped,
        Moment::Over { outcome, conceded } => Status::Over {
            outcome: outcome_view(obs, outcome, conceded),
        },
    };
    PlayerView {
        status,
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
        events: events.iter().map(|e| game_event(obs, e)).collect(),
    }
}

/// Player `p` as the viewer of `obs` sees them.
fn side(obs: Observation<'_>, p: Seat) -> Side {
    if p == obs.viewer() {
        Side::You
    } else {
        Side::Opponent
    }
}

/// Slots as the protocol numbers them.
fn slots(slots: impl IntoIterator<Item = Slot>) -> Vec<u8> {
    slots.into_iter().map(Slot::get).collect()
}

/// The name of the face up of the object in `slot`, if the viewer knows it; for a face-down
/// permanent, the name of its card, which it turns face up with (CR 712.15a).
fn name(t: &GameTable, obs: Observation<'_>, slot: Slot) -> Option<&'static str> {
    let face = match obs.object(slot).face {
        FaceIx::DOWN => FaceIx::FRONT,
        face => face,
    };
    face_name(t, obs, slot, face)
}

/// The name of face `face` of the object in `slot`, if the viewer knows it: a copy's is the
/// copied card's (CR 707.2), since a token copy is a token of that card.
fn face_name(
    t: &GameTable,
    obs: Observation<'_>,
    slot: Slot,
    face: FaceIx,
) -> Option<&'static str> {
    Some(t.def(obs.card(slot)?).face_name(face))
}

fn object_view(t: &GameTable, obs: Observation<'_>, slot: Slot) -> ObjectView {
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
    let some = |s: Option<Slot>| s.map(Slot::get);
    ObjectView {
        slot: slot.get(),
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
        attached_to: some(o.attached_to),
        exiled_by: if flags.has(ObjectFlags::EXILED_UNTIL) {
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

fn stack_item(t: &GameTable, obs: Observation<'_>, e: &StackEntry) -> StackItem {
    let targets = e
        .targets
        .iter()
        .map(|target| match (target.as_player(), target.as_object()) {
            (Some(player), _) => TargetRef::Player {
                player: side(obs, player),
            },
            (None, Some(o)) => TargetRef::Object { slot: o.slot.get() },
            (None, None) => unreachable!("a target is a player or an object"),
        })
        .collect();
    let source = e.object.map(|r| r.slot.get());
    match e.ability {
        None => StackItem::Spell {
            slot: source.expect("a spell's card"),
            targets,
            x: e.x,
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

fn decision_view(
    t: &GameTable,
    obs: Observation<'_>,
    id: DecisionId,
    d: &Decision,
    resolving: Option<&StackEntry>,
    stops: StopsSetting,
) -> DecisionView {
    let candidates = match &d.candidates {
        Candidates::Objects(b) => b
            .iter()
            .map(|slot| CandidateView {
                label: name(t, obs, slot).unwrap_or("a hidden card").to_owned(),
                option: Candidate::Object { slot: slot.get() },
                payments: Vec::new(),
                blocks: Vec::new(),
                nth: None,
                targets: Vec::new(),
            })
            .collect(),
        Candidates::Options(opts) => opts.iter().map(|&o| candidate(t, obs, o)).collect(),
    };
    DecisionView {
        id,
        kind: d.kind.into(),
        candidates,
        resolving: resolving.map(|e| stack_item(t, obs, e)),
        ask: ask_view(obs, d.ask),
        pass_to: (d.kind == DecisionKind::Priority).then(|| pass_to(obs, stops)),
        source: decision_source(obs, d, resolving),
        halt: false,
        cancellable: false,
    }
}

/// A halt ([`DecisionView::halt`]): priority with passing its one candidate.
fn halt_view(obs: Observation<'_>, id: DecisionId, stops: StopsSetting) -> DecisionView {
    DecisionView {
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
        pass_to: Some(pass_to(obs, stops)),
        source: None,
        halt: true,
        cancellable: false,
    }
}

fn ask_view(obs: Observation<'_>, ask: Ask) -> AskView {
    match ask {
        Ask::Plain => AskView::Plain,
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
        Ask::Explore => AskView::Explore,
        Ask::ManifestDread => AskView::ManifestDread,
        Ask::ExileCards { zone, count } => AskView::ExileCards {
            zone: zone.into(),
            count,
        },
        Ask::TopOrBottom { card } => AskView::TopOrBottom { card: card.get() },
        Ask::Search { max } => AskView::Search { max },
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

fn pass_to(obs: Observation<'_>, stops: StopsSetting) -> PassTo {
    match next_stop(obs, stops.stops()) {
        NextStop::Resolve => PassTo::Resolve,
        NextStop::CombatDamage => PassTo::CombatDamage,
        NextStop::Step(step) => PassTo::Step { step: step.into() },
        NextStop::NextTurn => PassTo::NextTurn,
    }
}

/// [`DecisionView::source`].
fn decision_source(
    obs: Observation<'_>,
    d: &Decision,
    resolving: Option<&StackEntry>,
) -> Option<u8> {
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
    slot.map(Slot::get)
}

fn candidate(t: &GameTable, obs: Observation<'_>, o: Opt) -> CandidateView {
    let named = |slot: Slot| name(t, obs, slot).unwrap_or("a hidden card");
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
            Candidate::PlayLand { slot: slot.get() },
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
            Candidate::Attack { slot: slot.get() },
        ),
        Opt::Defender(None) => (
            format!("Attack {}", player(obs.turn().active.opponent())),
            Candidate::Defender { planeswalker: None },
        ),
        Opt::Defender(Some(slot)) => (
            format!("Attack {}", named(slot)),
            Candidate::Defender {
                planeswalker: Some(slot.get()),
            },
        ),
        Opt::Block(slot) => (
            format!("Block with {}", named(slot)),
            Candidate::Block { slot: slot.get() },
        ),
        Opt::Lethal(slot) => (
            format!("Assign lethal damage to {}", named(slot)),
            Candidate::Lethal { slot: slot.get() },
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
                    slot: slot.get(),
                    color,
                },
            )
        }
        Opt::Pool(color) => {
            let (color, symbol) = mana(color);
            (format!("Spend {symbol}"), Candidate::Pool { color })
        }
        Opt::Number(n) => (n.to_string(), Candidate::Number { n }),
        Opt::Plan(index) => (format!("Payment {}", index + 1), Candidate::Plan { index }),
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
            Candidate::Object { slot: slot.get() },
        ),
        Opt::TurnFaceUp(slot) => (
            format!("Turn {} face up", named(slot)),
            Candidate::TurnFaceUp { slot: slot.get() },
        ),
    };
    CandidateView {
        label,
        option,
        payments: Vec::new(),
        blocks: Vec::new(),
        nth: activation_nth(t, obs, o),
        targets: Vec::new(),
    }
}

/// [`CandidateView::nth`].
fn activation_nth(t: &GameTable, obs: Observation<'_>, o: Opt) -> Option<u8> {
    let Opt::Activate {
        slot,
        ability: AbilityIx::Own(i),
    } = o
    else {
        return None;
    };
    let face = t.def(obs.card(slot)?).face(obs.object(slot).face);
    Some(ability_order(face.abilities, i).1)
}

/// The name of the face of the card in `slot` it is played with as a land, if the viewer knows
/// it: its first land face ([CR 712.12](https://yawgatog.com/resources/magic-rules/#R71212):
/// "A player playing a modal double-faced card ... as a land chooses one of its faces that's a
/// land before putting it onto the battlefield"), as Sink into Stupor plays as Soporific
/// Springs.
fn land_face_name(t: &GameTable, obs: Observation<'_>, slot: Slot) -> Option<&'static str> {
    let def = t.def(obs.card(slot)?);
    let face = def
        .face_ixs()
        .find(|&f| def.face(f).types.intersects(Types::LAND))?;
    Some(def.face_name(face))
}

/// A candidate that casts face `face` of the card in `slot` paying the options in `costs`.
fn cast_candidate(
    t: &GameTable,
    obs: Observation<'_>,
    slot: Slot,
    face: FaceIx,
    costs: Castings,
) -> (String, Candidate) {
    let spell = face_name(t, obs, slot, face).unwrap_or("a hidden card");
    (
        format!("Cast {spell}{}", casting_options(t, obs, slot, face, costs)),
        Candidate::Cast {
            slot: slot.get(),
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
fn casting_options(
    t: &GameTable,
    obs: Observation<'_>,
    slot: Slot,
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
fn activated_cost(
    t: &GameTable,
    obs: Observation<'_>,
    slot: Slot,
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
fn mana_text(cost: ManaCost) -> String {
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
fn activate(slot: Slot, ability: AbilityIx) -> Candidate {
    let (ability, granted) = match ability {
        AbilityIx::Own(i) => (i, false),
        AbilityIx::Granted(k) => (k, true),
    };
    Candidate::Activate {
        slot: slot.get(),
        ability,
        granted,
    }
}

/// Putting the waiting trigger at `index` on the stack.
fn trigger(t: &GameTable, obs: Observation<'_>, index: u8) -> (String, Candidate) {
    let trigger = obs.triggers()[usize::from(index)];
    (
        format!(
            "Put {}'s trigger on the stack",
            t.def(trigger.card).face_name(trigger.face)
        ),
        Candidate::Trigger {
            index,
            source: trigger.source.map(|r| r.slot.get()),
        },
    )
}

/// Mana symbols by [`ManaColor`] index.
const MANA_SYMBOLS: [&str; 6] = ["{W}", "{U}", "{B}", "{R}", "{G}", "{C}"];

fn outcome_view(obs: Observation<'_>, o: Outcome, conceded: bool) -> OutcomeView {
    match o {
        Outcome::Win(player) => OutcomeView::Win {
            winner: side(obs, player),
            reason: {
                // Both state-based losses may hold at one check: life is named.
                let loser = obs.player(player.opponent());
                if conceded {
                    LossReason::Concede
                } else if loser.life <= 0 {
                    LossReason::Life
                } else if loser.drew_from_empty {
                    LossReason::Library
                } else {
                    LossReason::Effect
                }
            },
        },
        Outcome::Draw => OutcomeView::Draw,
        Outcome::Stalled(reason) => OutcomeView::Stalled {
            reason: reason.into(),
        },
    }
}

/// What damage was dealt to, as the viewer names it.
fn recipient(obs: Observation<'_>, to: Recipient) -> TargetRef {
    match to {
        Recipient::Player(player) => TargetRef::Player {
            player: side(obs, player),
        },
        Recipient::Permanent(o) => TargetRef::Object { slot: o.slot.get() },
    }
}

/// An engine event as the viewer of `obs` is told it. Engine events name objects by slot and
/// carry no identity, and this takes no table: anything an event says about a card must come
/// from `obs`, which the information independence test redeals.
fn game_event(obs: Observation<'_>, e: &Event) -> GameEvent {
    let side = |p| side(obs, p);
    match *e {
        Event::ZoneChange {
            object,
            from,
            to,
            reason,
        } => GameEvent::ZoneChange {
            slot: object.slot.get(),
            from: from.into(),
            to: to.into(),
            reason: reason.into(),
        },
        Event::Damage {
            source,
            to,
            amount,
            combat,
        } => GameEvent::Damage {
            source: source.slot.get(),
            to: recipient(obs, to),
            amount,
            combat,
        },
        Event::Draw { player, cards } => GameEvent::Draw {
            player: side(player),
            cards,
        },
        Event::LifeGain { player, amount } => GameEvent::LifeGain {
            player: side(player),
            amount,
        },
        Event::LifeLoss { player, amount } => GameEvent::LifeLoss {
            player: side(player),
            amount,
        },
        Event::CountersAdded {
            object,
            kind,
            amount,
        } => GameEvent::CountersAdded {
            slot: object.slot.get(),
            counter: kind.into(),
            amount,
        },
        Event::CountersRemoved {
            object,
            kind,
            amount,
        } => GameEvent::CountersRemoved {
            slot: object.slot.get(),
            counter: kind.into(),
            amount,
        },
        Event::Tap { object } => GameEvent::Tap {
            slot: object.slot.get(),
        },
        Event::Untap { object } => GameEvent::Untap {
            slot: object.slot.get(),
        },
        Event::TokensCreated { controller, tokens } => GameEvent::TokensCreated {
            controller: side(controller),
            tokens: slots(tokens.iter()),
        },
        Event::Cast { spell, controller } => GameEvent::Cast {
            slot: spell.slot.get(),
            controller: side(controller),
        },
        Event::Attack { player, attackers } => GameEvent::Attack {
            player: side(player),
            attackers: slots(attackers.iter()),
        },
        Event::StepBegins { step, active } => GameEvent::StepBegins {
            step: step.into(),
            active: side(active),
        },
        Event::GainControl { object, player } => GameEvent::GainControl {
            slot: object.slot.get(),
            player: side(player),
        },
        Event::CoinFlip {
            player,
            won,
            source,
        } => GameEvent::CoinFlip {
            player: side(player),
            won,
            source: source.slot.get(),
        },
        Event::Lose { player } => GameEvent::Lose {
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
