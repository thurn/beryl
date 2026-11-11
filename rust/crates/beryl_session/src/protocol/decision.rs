//! The decision types: what the viewer is asked and what they may answer.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{CandidateIndex, DecisionId, Kind, Mana, Side, StackItem, Step, TargetRef, ZoneName};

/// The viewer's decision, its candidates in the order a [`CandidateIndex`] counts them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DecisionView {
    /// The initial Auto Pay proposal for Waterbend.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub waterbend: Option<Box<crate::ipc::WaterbendPreview>>,
    /// Legal targets that remove a permanent controlled by the viewer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub self_removal_targets: Option<Vec<u16>>,
    /// The recommended lethal pick in a combat damage division.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub damage_default: Option<CandidateIndex>,
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

/// Why a card can't be played ([`Refusal`](beryl_engine::Refusal)), by public slot: the first rule its most nearly
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

/// What a decision asks, from its [`Prompt`](beryl_engine::card::Prompt), by public slot and
/// relative to the viewer.
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

/// Where passing priority takes the viewer if the other player passes too ([`NextStop`](beryl_engine::NextStop)).
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
