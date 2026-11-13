//! The decision types: what the viewer is asked and what they may answer.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{DecisionId, Kind, Mana, Side, StackItem, Step, TargetRef};
use crate::prompt::PromptView;

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
    /// The prompt the decision's pick belongs to: its shape and purpose, the record's
    /// parameters and its words.
    pub prompt: Box<PromptView>,
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
    /// trigger on the stack: how many paragraphs of its face's text say abilities of its kind
    /// (activated, mana abilities among them, or triggered) before the one that says it, as
    /// [`StackItem::Ability`]'s `nth` counts them, so the UI finds the ability's paragraph.
    pub nth: Option<u8>,
    /// For a candidate that activates an ability or puts a waiting trigger on the stack: the
    /// ability's rules text, when the printed text of its face has no paragraph for it, as
    /// [`StackItem::Ability`]'s `text` says. None otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub text: Option<String>,
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
